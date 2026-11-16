// PhysHands -- see include/Natives/PhysHands.hpp.
//
// THE SLOTS. The port carries no PhysX headers, so every call below is by table slot, each read in PhysX3_x64.dll itself
// (2026-11-08): the scene's and the rigid body's tables by the error messages their functions hold and by the slots the
// port already calls; PxPhysics's table read live (PxGetPhysics -> its table at RVA 0x20A310) and its functions told
// apart by their arguments (createRigidStatic/createRigidDynamic take a transform, createShape a geometry, materials, a
// count, a flag and the shape flags by address, createMaterial three floats and then adds the material to the physics).
// Before the first call every slot used is checked against the address of its function in this very DLL; any other DLL
// leaves the hands as they were.
//
// THE FILTER. The game's shader (Cyberpunk2077.exe+0x320794, every scene's) lets two shapes meet when the group of
// either (filter words 0-1) has a bit in the other's mask (words 2-3), and sends a pair to the game's own callback when
// either mask has bit 62 or 63. So a hand's group is a bit no mask around it has (nothing reaches for it), and its mask
// is what it may touch, chosen group by group (ChooseMask): the world, light loose bodies and kinematic ones (people,
// doors), never a group of the player's own and not one of heavy bodies (cars). A census of the shapes near the
// player finds them, a slice a frame, before the hands are made.
#include "Natives/PhysHands.hpp"
#include "Utils/DebugGate.hpp"
#include "Natives/HeldGripPose.hpp"
#include "Anim/GraspPose.hpp"
#include "Anim/LeftHanded.hpp"
#include "Natives/HandSkin.hpp"
#include "Natives/HeldBody.hpp"
#include "Natives/DebugView.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Anim/ScenePolicy.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Camera/CameraState.hpp"
#include "Core/LiveControls.hpp"
#include "Core/Paths.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/LadderInput.hpp"
#include "Hooks/ScannerGesture.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Lobby/MenuLobby.hpp"
#include "Utils/SharedSlots.hpp"
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/Vector4.hpp>
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <share.h>
#include <cstring>
#include <string>
#include <thread>

extern void Log(const char* fmt, ...);
static void QuietLog(const char*, ...) {}
// Hooks/AnimPose.cpp: whether a grapple, a carried body and a finisher hand the arms to their animation
extern "C" int CyberpunkVR_VrikTakedownSuspend, CyberpunkVR_VrikBodyCarryHandOff, CyberpunkVR_VrikFinisherSuspend;

namespace cvr::physhands {
bool HeldNear(int owner, const float* a, const float* b, float radius);
namespace {

// ---- math (quaternions x y z w) ----
void QMul(const float* a, const float* b, float* o) {
    const float x = a[3]*b[0] + a[0]*b[3] + a[1]*b[2] - a[2]*b[1];
    const float y = a[3]*b[1] - a[0]*b[2] + a[1]*b[3] + a[2]*b[0];
    const float z = a[3]*b[2] + a[0]*b[1] - a[1]*b[0] + a[2]*b[3];
    const float w = a[3]*b[3] - a[0]*b[0] - a[1]*b[1] - a[2]*b[2];
    o[0] = x; o[1] = y; o[2] = z; o[3] = w;
}
void QRot(const float* q, const float* v, float* o) {
    const float tx = 2*(q[1]*v[2] - q[2]*v[1]), ty = 2*(q[2]*v[0] - q[0]*v[2]), tz = 2*(q[0]*v[1] - q[1]*v[0]);
    o[0] = v[0] + q[3]*tx + q[1]*tz - q[2]*ty;
    o[1] = v[1] + q[3]*ty + q[2]*tx - q[0]*tz;
    o[2] = v[2] + q[3]*tz + q[0]*ty - q[1]*tx;
}
void QConj(const float* q, float* o) { o[0] = -q[0]; o[1] = -q[1]; o[2] = -q[2]; o[3] = q[3]; }
bool QNorm(float* q) {
    const float n = std::sqrt(q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3]);
    if (!std::isfinite(n) || n < 1e-4f) return false;
    for (int k = 0; k < 4; ++k) q[k] /= n;
    return true;
}

// ---- one writer, any readers: a sequence counter round each copy ----
template <class T> struct Seq {
    std::atomic<uint32_t> seq{0};
    T data{};
    void Write(const T& v) {
        const uint32_t s = seq.load(std::memory_order_relaxed);
        seq.store(s + 1, std::memory_order_relaxed);
        std::atomic_thread_fence(std::memory_order_release);
        std::memcpy(static_cast<void*>(&data), &v, sizeof(T));
        std::atomic_thread_fence(std::memory_order_release);
        seq.store(s + 2, std::memory_order_release);
    }
    bool Read(T& out) const {
        for (int tries = 0; tries < 8; ++tries) {
            const uint32_t a = seq.load(std::memory_order_acquire);
            if (a & 1) continue;
            std::memcpy(&out, static_cast<const void*>(&data), sizeof(T));
            std::atomic_thread_fence(std::memory_order_acquire);
            if (seq.load(std::memory_order_relaxed) == a) return a != 0;
        }
        return false;
    }
};

// ---- PhysX by slot ----
struct PxTransform { float q[4]; float p[3]; };
struct PxBounds { float lo[3], hi[3]; };
struct BoxGeometry { int type; float half[3]; };
constexpr uintptr_t kPhysicsTable = 0x20A310, kSceneTable = 0x20CAF0, kDynamicTable = 0x20B2E8, kShapeTable = 0x20E9A8,
                    kMaterialTable = 0x209820;
constexpr uintptr_t kPhysicsGlobal = 0x25B5D8;   // what PxGetPhysics returns
// PxPhysics
constexpr int kPhysicsGetNbScenes = 19, kPhysicsGetScenes = 20, kCreateRigidDynamic = 22, kCreateShape = 27, kCreateMaterial = 32;
// PxScene
constexpr int kAddActor = 10, kRemoveActor = 13, kSceneGetNbActors = 18, kSceneGetActors = 19, kSceneGetNbAggregates = 26,
              kSceneGetAggregates = 27, kAggregateGetActors = 11;
// resetFiltering(PxActor&): MSVC puts the two overloads in reverse, so the actor's is 55 (the one switching on the actor's
// type with "only PxParticleBase and PxRigidActor support this operation!"), the shapes' 54
constexpr int kSceneResetFiltering = 55;
// PxActor / PxRigidActor / PxRigidBody / PxRigidDynamic
constexpr int kRelease = 0, kGetType = 5, kGetScene = 6, kGetWorldBounds = 9, kSetActorFlag = 10, kGetGlobalPose = 20,
              kSetGlobalPose = 21, kAttachShape = 23, kGetShapes = 26, kSetCMassLocalPose = 29, kSetMass = 31, kGetMass = 32, kSetInertia = 34,
              kGetLinearVelocity = 37, kSetLinearVelocity = 38, kGetAngularVelocity = 39, kSetAngularVelocity = 40,
              kSetRigidBodyFlag = 45, kGetRigidBodyFlags = 47, kSetMaxDepenetration = 50, kSetLinearDamping = 56,
              kSetAngularDamping = 58, kSetMaxAngularVelocity = 60, kIsSleeping = 62, kWakeUp = 72, kPutToSleep = 73,
              kSetSolverIterations = 74;
// getCMassLocalPose (the body-to-actor transform) and getLinearDamping, for a body held by its point (HoldBody)
constexpr int kGetCMassLocalPose = 30, kGetLinearDamping = 57;
// getMassSpaceInertiaTensor (the buffered tensor, like setMassSpaceInertiaTensor's 34) and getMaxAngularVelocity
constexpr int kGetInertia = 35, kGetMaxAngularVelocity = 61;
// PxShape
constexpr int kShapeRelease = 0, kShapeGetGeometryType = 7, kShapeSetGeometry = 8, kShapeGetBox = 10, kShapeGetSphere = 11, kShapeGetCapsule = 12,
              kShapeGetConvex = 14, kShapeGetTriangles = 15, kShapeGetHeightField = 16, kShapeSetLocalPose = 18,
              kShapeGetLocalPose = 19, kShapeSetSimFilter = 20, kShapeGetSimFilter = 21, kShapeSetMaterials = 24, kShapeSetContactOffset = 28,
              kShapeGetContactOffset = 29, kShapeSetFlags = 33,
              kShapeGetFlags = 34;
// PxMaterial (NpMaterial's table, found by the one function holding "PxMaterial::setRestitution", slot 11; the 3.4 header's
// order from there: get/set friction, restitution, flags, then the combine modes)
constexpr int kMaterialSetFrictionCombine = 16;
constexpr int kCombineMin = 1;   // PxCombineMode::eMIN
enum Geometry { kGeomSphere, kGeomPlane, kGeomCapsule, kGeomBox, kGeomConvex, kGeomTriangles, kGeomHeightField };
// PxActorFlag, PxRigidBodyFlag
constexpr int kDisableGravity = 2, kDisableSimulation = 8, kEnableCcd = 4, kEnableSpeculativeCcd = 32;

struct SlotCheck { uintptr_t table; int slot; uintptr_t rva; };
constexpr SlotCheck kChecks[] = {
    {kPhysicsTable, kPhysicsGetNbScenes, 0x47fc0}, {kPhysicsTable, kPhysicsGetScenes, 0x48000},
    {kPhysicsTable, kCreateRigidDynamic, 0x48150}, {kPhysicsTable, kCreateShape, 0x48270}, {kPhysicsTable, kCreateMaterial, 0x48410},
    {kSceneTable, kAddActor, 0x520d0}, {kSceneTable, kRemoveActor, 0x521e0}, {kSceneTable, kSceneGetNbActors, 0x52b60},
    {kSceneTable, kSceneGetActors, 0x52cb0}, {kSceneTable, kSceneGetNbAggregates, 0x52790}, {kSceneTable, kSceneGetAggregates, 0x527a0},
    {kDynamicTable, kRelease, 0x4a800}, {kDynamicTable, kGetType, 0x4df20}, {kDynamicTable, kGetWorldBounds, 0x4df30},
    {kDynamicTable, kSetActorFlag, 0x4e430}, {kDynamicTable, kSetGlobalPose, 0x4a8e0}, {kDynamicTable, kAttachShape, 0x4d400},
    {kDynamicTable, kSetMass, 0x4edc0}, {kDynamicTable, kGetMass, 0x4dce0}, {kDynamicTable, kSetInertia, 0x4ee60},
    // setCMassLocalPose: the large one before getCMassLocalPose (30, the body-to-actor transform, buffered or not) and setMass
    {kDynamicTable, kSetCMassLocalPose, 0x4b4d0},
    {kDynamicTable, kGetLinearVelocity, 0x4dcc0}, {kDynamicTable, kSetLinearVelocity, 0x4bd90},
    {kDynamicTable, kGetAngularVelocity, 0x4d800}, {kDynamicTable, kSetAngularVelocity, 0x4be10},
    {kDynamicTable, kSetRigidBodyFlag, 0x4f1f0}, {kDynamicTable, kGetRigidBodyFlags, 0x4ded0},
    {kDynamicTable, kSetMaxDepenetration, 0x4eff0},   // writes +0xfc under the 0x400 dirty bit, as its getter (51) reads
    {kDynamicTable, kSetLinearDamping, 0x4bc10}, {kDynamicTable, kSetAngularDamping, 0x4bcd0},
    {kDynamicTable, kSetMaxAngularVelocity, 0x4be90}, {kDynamicTable, kWakeUp, 0x4c2d0}, {kDynamicTable, kSetSolverIterations, 0x4c300},
    {kDynamicTable, kGetCMassLocalPose, 0x4d820}, {kDynamicTable, kGetLinearDamping, 0x4bca0},
    {kDynamicTable, kGetInertia, 0x4dd20}, {kDynamicTable, kGetMaxAngularVelocity, 0x4bf20},
    {kShapeTable, kShapeRelease, 0x661e0}, {kShapeTable, kShapeGetGeometryType, 0x66250}, {kShapeTable, kShapeSetGeometry, 0x66260}, {kShapeTable, kShapeGetBox, 0x66480},
    {kShapeTable, kShapeGetSphere, 0x664d0}, {kShapeTable, kShapeGetCapsule, 0x66500}, {kShapeTable, kShapeGetConvex, 0x66570},
    {kShapeTable, kShapeGetTriangles, 0x66600}, {kShapeTable, kShapeGetHeightField, 0x66680},
    {kShapeTable, kShapeSetLocalPose, 0x66700}, {kShapeTable, kShapeGetLocalPose, 0x66910}, {kShapeTable, kShapeSetSimFilter, 0x66960},
    {kShapeTable, kShapeGetSimFilter, 0x66a80}, {kShapeTable, kShapeSetFlags, 0x671b0}, {kShapeTable, kShapeGetFlags, 0x67200},
    // setMaterials: the one before getNbMaterials, getMaterials and getMaterialFromInternalFaceIndex (its message)
    {kShapeTable, kShapeSetMaterials, 0x66b10},
    // the contact offset's setter and getter (the getter reads the buffered value under flag 0x10, as getRestOffset's
    // reads its own under 0x20 two slots on)
    {kShapeTable, kShapeSetContactOffset, 0x66ed0}, {kShapeTable, kShapeGetContactOffset, 0x66fd0},
    // isSleeping reads the body's sleep word; putToSleep is the jump into Scb::Body right after wakeUp's
    {kDynamicTable, kIsSleeping, 0x4c090}, {kDynamicTable, kPutToSleep, 0x4c2f0},
    {kMaterialTable, kMaterialSetFrictionCombine, 0x42da0},
    {kSceneTable, kSceneResetFiltering, 0x53530},
};
// PxGeometryQuery, exported by PhysX3Common_x64.dll: no scene in it, so the worker may call it.
using PenetrationFn = bool (*)(float* direction, float* depth, const void* geom0, const void* pose0, const void* geom1, const void* pose1);
PenetrationFn penetration = nullptr;
// ...and the square of a point's distance from a shape, with the nearest point on it (0 inside)
using PointDistanceFn = float (*)(const float* point, const void* geom, const void* pose, float* closest);
PointDistanceFn pointDistance = nullptr;
template <class F> F At(void* object, int slot) { return reinterpret_cast<F>((*reinterpret_cast<void***>(object))[slot]); }

uintptr_t dll = 0;
void* physics = nullptr;
int proved = 0;   // 0 not yet, 1 yes, -1 no
bool Prove() {
    if (proved) return proved > 0;
    proved = -1;
    dll = reinterpret_cast<uintptr_t>(GetModuleHandleA("PhysX3_x64.dll"));
    if (!dll) return false;
    for (const auto& c : kChecks) {
        const uintptr_t at = *reinterpret_cast<const uintptr_t*>(dll + c.table + 8 * uintptr_t(c.slot));
        if (at != dll + c.rva) {
            Log("[physhands] PhysX3_x64.dll is not the one read (table 0x%llx slot %d): the hands stay as they were\n",
                (unsigned long long)c.table, c.slot);
            return false;
        }
    }
    physics = *reinterpret_cast<void**>(dll + kPhysicsGlobal);
    if (!physics || *reinterpret_cast<uintptr_t*>(physics) != dll + kPhysicsTable) {
        Log("[physhands] no PxPhysics where it was read: the hands stay as they were\n");
        return false;
    }
    if (HMODULE common = GetModuleHandleA("PhysX3Common_x64.dll"))
        penetration = reinterpret_cast<PenetrationFn>(GetProcAddress(
            common, "?computePenetration@PxGeometryQuery@physx@@SA_NAEAVPxVec3@2@AEAMAEBVPxGeometry@2@AEBVPxTransform@2@23@Z"));
    if (!penetration) Log("[physhands] no PxGeometryQuery::computePenetration: the fingers are not bent\n");
    if (HMODULE common = GetModuleHandleA("PhysX3Common_x64.dll"))
        pointDistance = reinterpret_cast<PointDistanceFn>(GetProcAddress(
            common, "?pointDistance@PxGeometryQuery@physx@@SAMAEBVPxVec3@2@AEBVPxGeometry@2@AEBVPxTransform@2@PEAV32@@Z"));
    proved = 1;
    Log("[physhands] PhysX checked: %zu slots where they were read\n", sizeof(kChecks) / sizeof(kChecks[0]));
    return true;
}

// The game's world scene: the one whose userData is the game's object for it (that object holds it at +0x18), and that
// object's +0x134 is where PhysX's origin is in the game's world.
void* WorldScene(float* origin) {
    void* found[32]{};
    const uint32_t n = At<uint32_t (*)(void*)>(physics, kPhysicsGetNbScenes)(physics);
    const uint32_t got = n ? At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(physics, kPhysicsGetScenes)(physics, found, 32, 0) : 0;
    for (uint32_t i = 0; i < got && i < 32; ++i) {
        void* scene = found[i];
        void* owner = scene ? *reinterpret_cast<void**>(static_cast<char*>(scene) + 8) : nullptr;
        if (owner && *reinterpret_cast<void**>(static_cast<char*>(owner) + 0x18) == scene) {
            const float* o = reinterpret_cast<const float*>(static_cast<char*>(owner) + 0x134);
            origin[0] = o[0]; origin[1] = o[1]; origin[2] = o[2];
            return scene;
        }
    }
    return nullptr;
}
bool Origin(void* scene, float* origin) {
    void* owner = *reinterpret_cast<void**>(static_cast<char*>(scene) + 8);
    if (!owner || *reinterpret_cast<void**>(static_cast<char*>(owner) + 0x18) != scene) return false;
    const float* o = reinterpret_cast<const float*>(static_cast<char*>(owner) + 0x134);
    origin[0] = o[0]; origin[1] = o[1]; origin[2] = o[2];
    return std::isfinite(o[0] + o[1] + o[2]);
}

// ---- settings: config/physhands.ini, read again by the worker whenever it changes ----
// TWO STRENGTHS. In the air the hand follows the controller as fast as `move_accel` lets it, so it never trails; once
// something has stopped it (the solver changed the velocity it was given) it pushes with `force` alone until it has
// been free for a moment. So it swings quick and presses soft: a cup slides, a wall does not shake it.
struct Settings {
    int enabled = 1;
    int withWeapon = 1;           // the hands keep their bodies with a weapon out (and with cyberware arms, which are one)
    int ride = 1;                 // the bodies move with the player's entity each step (Steer); 0: carried by its pace
    int fingers = 1;              // the fingers bend on the world
    float mass = 0.4f;            // kg
    float force = 20.0f;          // N: the most a hand presses the world with...
    float pushForce = 50.0f;      // N: ...and pushes a loose thing with (a cup goes, a crate's friction holds it); Bonelab's
                                  // PhysHand.newtonDamp, the velocity-matching budget its ApplyForce has before extra mass
    int fingerShapes = 1;         // the fingers are capsules on the hand's body
    int forearm = 1;              // so is the forearm
    int drawn = 1;                // 1: the drawn hand IS the body (carried forward by its velocity to the frame); 0: tracking
                                  // plus the body's offset, the body's place only while it is held off
    float moveAccel = 600.0f;     // m/s^2: the most it speeds up in the air
    float spinAccel = 2000.0f;    // rad/s^2: the most it turns with in the air...
    float spinContact = 60.0f;    // ...and touching something
    float pressSpeed = 0.15f;     // m/s: touching, the most it leans into what it touches...
    float pressSpin = 1.0f;       // rad/s: ...and turns toward tracking
    float pressGain = 0.25f;      // the share of the gap it closes a step while it presses the world (at 0.5 a hand pressed
                                  // 8 cm into a wall swung 2-3 mm either way every other step, 2026-11-09)
    // Measured pressed 12 cm into the floor, the target still (2026-11-08): at 40 N, 250 rad/s^2 and 0.6 m/s the body
    // rocked on its fingertips and the drawn hand moved 6-7 mm a step; stiffer turning made it 20 mm; at these numbers
    // 2.2 mm, and 1.8 mm under a 1.5 mm tracking tremor -- less than the tremor itself.
    float maxSpeed = 20.0f;       // m/s
    float depenetration = 0.8f;   // m/s: the most PhysX pushes it out of what it got into (unbounded, it kicked the hand)
    float teleport = 1.0f;        // m off its target: put there, not steered (a load, a teleport)
    float deadband = 0.004f;      // m of offset the drawn hand ignores
    float turnDeadband = 0.03f;   // rad of turn it ignores...
    float maxTurn = 0.35f;        // ...and the most of it shown: a box pivoting on its edge bent the drawn wrist past a wrist's
    float stickHold = 0.008f;     // m: on a surface the drawn hand stays put while its body stirs within this...
    float stickTurn = 0.05f;      // rad: ...and keeps its turn while the body rocks within this
    float smoothHz = 6.0f;        // the drawn hand's offset filter: its cutoff at rest...
    float smoothBeta = 40.0f;     // ...and how much faster it gets per m/s the offset moves
    float half[3] = {0.065f, 0.045f, 0.016f};   // the hand's box, m: along the fingers, across, through the palm
    float along = 0.065f;         // m from the wrist to the box's middle, along the fingers: the palm and the first joints;
                                  // the rest of each finger is the finger solve's
    // FRICTION AS BONELAB HAS IT (PhysHand.SetFriction, read 2026-11-09): an open hand is its low-friction material, a
    // closed one (the grip past half) its high one. Combined with the surface's by the lesser, so an open hand drawn along
    // a table or a wall slides on it whatever the surface is: at 0.35 averaged with the table's it stuck and jumped.
    float friction = 0.05f;
    float frictionGrip = 0.6f;
    // m/s: the most PhysX pushes a loose thing near the hands out of what it got into. The game leaves it unbounded, so a
    // thing a hand or a held one ended up in was thrown, not moved aside. 0 leaves the game's.
    float propDepenetration = 1.0f;
    float stackDepenetration = 0.01f;   // m/s: the same for a thing standing in another (nested plates): about none
    float looseAfterGrip = 0.25f; // s after the grip opens before the palm meets loose things again (and none in it)
    int weapon = 1;               // the gun in the hand is boxes on that hand's body: the other hand meets them, the world does not
    // rad/s and rad/s^2: a gun's hand keeps its turn this firmly while it touches something. As soft as an empty hand's
    // (press_spin, spin_contact), a pistol pushed at a wall turned sideways and the hand went on to the wall; held as
    // Bonelab's wrist holds (25 N m), the wall pushes the hand back along the barrel instead.
    float weaponSpin = 12.0f;
    float weaponSpinAccel = 1500.0f;
    // m/s, rad/s: THE BODY GOES WITH TRACKING'S OWN SPEED, and a gap it was left behind by (a wall it slid along, a table
    // it was held on) is closed at no more than this on top. Closed in one step, a hand that came free 15 cm behind its
    // target was drawn there in one frame. A muscle's pace, as Bonelab's arm closes it by force.
    float catchSpeed = 2.5f;
    float catchSpin = 10.0f;
    int trace = 0;                // 1: every drawn frame written to log/physhands_trace.csv (for tuning)
    int conform = 1;              // the fingers lie on what the palm is on: flat on a wall, round an edge
    int stackDrop = 1;            // a thing taken off a stack: what stood on it comes down as if it were gone
    int heldAlone = 1;            // a held thing meets no loose thing while held, only the world
    // THE HAND PUSHES THE BODY (Bonelab: the hand's force goes back into the body at the upper arm, -F). Tracking held off
    // more than body_push_dead into the world sends the player away from it along the floor at body_push_gain per metre
    // past that, at most body_push_most m/s: a wall pushed is pushed off from, as a body is.
    // Bonelab's -F goes into the torso for any force at all, so its body backs off a wall as soon as the hand is held, and
    // the hand stays where the controller is. At 5 cm dead and 4/s the player stood still with the hand 5-8 cm into a
    // wall: drawn on its surface, short of the controller. 1.5 cm and 12/s (83 ms) back off within a few frames.
    int bodyPush = 1;
    float bodyPushGain = 12.0f;   // 1/s
    float bodyPushDead = 0.015f;  // m
    float bodyPushMost = 2.0f;    // m/s
    // THE ARM (Bonelab's hand hangs on its arm's joints): the body is not sent farther from the shoulder than V's arm
    // reaches (upper arm + forearm from the arm solve, plus arm_reach_spare) -- tracking past that is held at the arm's
    // length, so a hand meets nothing V's arm could not, and pushes the body only as far as the arm does.
    int armReach = 1;
    float armReachSpare = 0.03f;  // m
    // PEOPLE: a living person near the hands is a few capsules on their bones (HandCollision reads them): the hands stop
    // on them and the fingers lie on them, and nobody is pushed
    int people = 1;
    // V'S OWN HAND: the palm's box, the finger capsules and the forearm's fitted to the skin of V's arms, a woman's or a
    // man's (Natives/HandSkin.hpp); 0: the ini's box (half_along, half_across, half_through, along) and the old widths
    int skin = 1;
    // THE SKIN'S MARGIN: the palm's box, the fingers' capsules and the forearm's grown by this much (m) past the skin, so
    // what is drawn stops a hair off a surface rather than in it, and a thing whose collision sits a little inside its
    // look (a railing's top, a wall's plaster) does not swallow the skin
    float skinMargin = 0.002f;
    // CONTINUOUS COLLISION: 1 swept (PhysX's CCD: the step is cut at the time of impact), 2 speculative (contacts made as
    // far out as the body moves in a step); and the hand's shapes' contact offset, m (0: PhysX's own, 2 cm).
    // THE CONTACT OFFSET IS WHAT STOPS A QUICK HAND: the swept CCD did nothing measurable here (nor did speculative), and a
    // contact is made only within the offset, so a hand moving more than that in a step went in by the rest -- slapped at
    // a wall at 1-2 m/s in 22 ms steps, its palm went 14-27 mm in and its fingertips 36 mm, and PhysX threw it back out.
    // At 5 cm the fingers stayed out (2026-11-09).
    int ccd = 1;
    float contactOffset = 0.05f;
    // ON THE WORLD AS BONELAB'S HAND IS (PhysHand.ApplyForce / ApplyTorque2, read 2026-11-09): no soft mode that lets the
    // hand lag. Tracking's own move and turn each step pass in full, along a surface and turning on it; the gap the body
    // is behind by is closed slowly (press_catch m/s, press_catch_spin rad/s) while tracking's place is in the world, at
    // catch_speed / catch_spin once it is clear, and the push into the surface held to press_speed. What keeps a pressed
    // hand still is Bonelab's settling: the most the hand speeds up or turns faster by
    // falls to 15% (turning 10%) while what it is asked for stays as it was, and comes back as the ask grows (linear
    // 3.2/s up, 4/s down; turning 12/s up once the turn rate is 3 rad/s off, 8/s down). At press_gain 0.25 and press_spin
    // 1 rad/s a hand on a railing slid a quarter of its gap a step behind tracking and hardly turned: drawn to the rail.
    // settle=0: that old way.
    int settle = 1;
    float pressCatch = 0.5f;
    float pressCatchSpin = 2.0f;
    // THE FINGERS' JOINTS AS HINGES WHILE THEY LIE ON SOMETHING (Anim/FingerContact.cpp): the animation's own turn of a
    // middle or last finger bone off its hinge (a twist about the bone's line, a lean to the side) is dropped, the
    // knuckle's spread kept and its twist dropped, from the rig's straight reference -- so a finger straightened on a
    // wall lies on its pads instead of keeping the curled hand's lean. 0: the animation's pose as it is.
    int fingerHingeOnly = 1;
    // THE HAND THAT HOLDS ON (climb): the grip closed on a top of the world (a railing's, a ledge's) or on an edge the
    // fingers curl over keeps the hand where it took hold, and the player is moved so that tracking comes back to it --
    // the controller drawn down lifts the player, at most climb_most m/s, the feet no higher than 10 cm over the hold.
    // Not on a wall's face: walls are not climbed.
    int climb = 1;
    float climbMost = 3.0f;
    // V'S OWN BODY AND THE OTHER HAND: the palm and the fingers stop on V's trunk, the crown of the head and the other arm's
    // upper arm (self), and on the other hand's palm and fingers (hands_meet) -- not with a gun out, not while a thing is
    // held; the forearms meet neither (they lie along the trunk)
    int self = 1;
    int handsMeet = 1;
    // A PERSON TAKEN HOLD OF: the grip closed on a part of someone keeps the hand on it as it moves, grab_give m toward the
    // controller at most, and lets go by itself when the controller is grab_release m from it
    int grabPeople = 1;
    float grabRelease = 0.30f;
    float grabGive = 0.04f;
    unsigned long long groupOverride = 0, maskOverride = 0;   // 0: the census decides
    int logEvery = 10;            // s between statistics
};
Seq<Settings> settings;
constexpr int kSettingsVersion = 26;

// ---- what crosses threads ----
struct Target { float pos[3]; float rot[4]; float entityPos[3]; float entityRot[4]; uint64_t at; };   // model space, anim thread
Seq<Target> targets[2];
std::atomic<uint32_t> reachHeld[2];   // steps tracking was past the arm's reach (statistics)
struct Shoulder { float model[3]; float reach; uint64_t at; };   // the arm solve's shoulder, model space, and the arm's
                                                                  // length (upper arm + forearm), m (anim thread)
Seq<Shoulder> shoulders[2];
struct Step {   // after a step: where each body is and where it was sent, PhysX's space; the frame's entity
    float pose[2][7];
    float sent[2][7];
    float sentTrack[2][3];   // tracking as that step had it, before the arm's reach brought it back (PhysX's space)
    float sentEntity[2][3];  // the player's entity as that tracking had it (PhysX's space)
    bool valid[2];
    float entityRot[4];
    float origin[3];
    bool touching[2];
    bool onWorld[2];
    float vel[2][3], spin[2][3];
    double when;   // seconds (performance counter), right after the fetch
    float dt;      // the step's own length, s
    uint64_t at;
    uint64_t serial;
    uint64_t scannerEpoch;
};
Seq<Step> steps;
// worker -> anim thread: the offset (model space), and, for a hand held off by something, where its body is in the game's
// world (smoothed) and how much the drawn hand is that place rather than tracking plus the offset
struct Offset {
    float pos[3]; float rot[4]; float body[3]; float held; bool on; uint64_t at;
    // the body itself, for drawn=1: its rotation, velocities (zero while it is stuck on the world), when it was there
    bool bodyMode; float bodyRot[4], vel[3], spin[3]; double when; float stepDt;
    float contact;   // 0..1, eased: how much a gun's hand is turned as its body is rather than as tracking has it
    float raw[3]; uint8_t touching, onWorld, trace;   // for the trace: the body as PhysX has it (world), the step's touch
    float sent[3], sentRot[4];   // where that step sent the body: tracking as it was then (world)
    float sentTrack[3];          // tracking itself at that step (world): sent, less the arm's reach bringing it back
    float sentEntity[3];         // the player's entity as that tracking had it (world)
    bool pressed;                // on the world and refused: drawn at the latch, tracking's move not added
    float origin[3];             // PhysX's origin in the game's world at that step
    float rawRot[4];             // the body's turn as PhysX has it (world), with o.raw its place
    float margin;                // the skin's margin the body's shapes are grown by
    uint64_t scannerEpoch;
};
Seq<Offset> offsets[2];
// physics thread -> anim thread: the hold a hand has taken on the world (climb), the game's world: its place and turn
struct HoldPose { float p[3]; float q[4]; bool on; uint64_t at; };
Seq<HoldPose> holdPoses[2];
// script thread -> physics thread: a body a hand holds fast and its pose in the hand bone's frame (HoldBody), asked again
// every frame it is held
struct BodyHold { void* actor; void* scene; void* ref; uint32_t proxy, body, refBody; float p[3]; float q[4]; float g[3]; float torque; bool on; uint64_t at; };
Seq<BodyHold> bodyHolds[2];
// anim thread -> physics thread: the hand bone where its body (or tracking) has it this frame, before a held body's turn
// is given it (Adjust), the game's world: what a body held is put on at the next step
struct DrawnHand { float p[3]; float q[4]; uint64_t at; };
Seq<DrawnHand> drawnHands[2];
// physics thread -> anim thread: how far a body held by the hand is turned off the turn the hand has it at (HoldBody), the
// game's world, and the point held in the hand bone's frame: the drawn hand is turned so about that point
struct HoldLoad { float wrist[4]; float seat[3]; bool on; uint64_t at; };;
Seq<HoldLoad> holdLoads[2];
struct Wanted { bool on[2]; uint64_t at; };   // worker -> physics thread
Seq<Wanted> wanted;

// physics thread only
void* scene = nullptr;
void* material = nullptr;
void* actor[2] = {nullptr, nullptr};
void* shape[2] = {nullptr, nullptr};
void* fingerShape[2][5][3] = {};   // each finger's segments, capsules on the hand's body (the thumb's first is in the palm)
float fingerHalf[2][5][3] = {};
void* forearmShape[2] = {nullptr, nullptr};
float forearmHalf[2] = {};
void* forearmUpper[2] = {nullptr, nullptr};   // the forearm tapers to the wrist: two capsules, the wrist's and the elbow's
float forearmUpperHalf[2] = {}, forearmRadius[2][2] = {}, fingerRadius[2][5][3] = {};
// A GUN'S BOXES, on the body of the hand that holds it (weapon_shapes.lua's, measured in that hand by the script): parked
// small in the palm while there is none
// a gun's fitted boxes (up to 12, tools/emit_weapon_shapes.py) and a mounted muzzle's and scope's
constexpr int kMostWeaponBoxes = 14;
void* weaponShape[2][kMostWeaponBoxes] = {};
float weaponHalf[2][kMostWeaponBoxes][3] = {};   // as posed this step (the boxes in use come first)
float weaponMade[2][kMostWeaponBoxes][3] = {};   // each shape's geometry as last set
PxTransform weaponLocal[2][kMostWeaponBoxes] = {};
int weaponOn[2] = {0, 0};          // boxes in use, as posed this step
void* materialGrip = nullptr;
bool gripMaterial[2] = {false, false};
uint64_t looseFrom[2] = {0, 0};    // the palm meets loose things again from then (ms): it closed on something
bool looseMet[2] = {true, true};
bool simulating[2] = {false, false};
uint32_t filterNow[2][4] = {};
float sent[2][7] = {};
float sentTrack[2][3] = {};
float sentEntity[2][3] = {}, targetEntity[2][3] = {};   // the entity of the tracking a step was sent to (PhysX's space)
bool sentValid[2] = {false, false};
float gave[2][6] = {};            // the velocities each body was given before the step: linear (the wrist's), angular
float gaveCom[2][3] = {};         // ...and the linear one as PhysX was given it: the centre of mass's
float settleLin[2] = {1.f, 1.f}, settleAng[2] = {1.f, 1.f}, lastAsk[2] = {0.f, 0.f};   // Bonelab's settling, on the world
bool trackClear[2] = {false, false};   // this step, on the world: tracking's palm in nothing (the gap closes at full pace)
bool gaveValid[2] = {false, false};
uint64_t touchUntil[2] = {0, 0};  // touching something until then (ms)
float pushedOut[2][3] = {};       // the way the solver last pushed each body: out of what it touches
float appliedMass[2] = {}, appliedDepenetration[2] = {}, madeHalf[2][3] = {}, madeAlong[2] = {};   // as each body was made or last set
float madeCentre[2][3] = {};   // the palm's box's middle in the hand bone's frame
int madeSkin[2] = {-2, -2};    // the skin the box was fitted to: 0 a man's, 1 a woman's, -1 the ini's
int appliedCcd[2] = {-1, -1};
float appliedContactOffset[2] = {-1.f, -1.f}, ownContactOffset[2] = {0.f, 0.f};   // as set; PhysX's own, read as made
std::atomic<bool> live[2] = {false, false};
std::atomic<uint64_t> liveScannerEpoch{0};
uint64_t stepScannerEpoch = 0;   // physics thread: the epoch of the step being fetched
std::atomic<uint64_t> touchedSteps[2] = {0, 0}, worldSteps[2] = {0, 0};
bool onWorld[2] = {false, false};   // this step's touch was on the world (physics thread)
// ...and on what the player may push off from: the world or a person, not V's own body or the other hand -- a hand at
// V's side pushed V away from V, and V walked off on its own (2026-11-10)
bool onPushable[2] = {false, false};

// ---- the census of filters near the player, a slice a step ----
// Each group value seen, with how many shapes of each kind carry it. The groups OR'd together say nothing: every kind's
// union covered the statics' bits (2026-11-08: static 5c00c, kinematic 5d04f, callback 5d058), so the hand's mask is
// chosen from the groups one by one.
enum Kind { kindStatic, kindLight, kindKinematic, kindHeavy, kindCallback, kindPlayer, kindCount };
struct GroupSeen {
    unsigned long long group;
    int n[kindCount];
    int capsules, limbs;   // kinematic capsules, and of them the thin short ones a body's limbs are
};
constexpr int kMostGroups = 48;
struct Census {
    int phase = 0;   // 0 not begun, 1 actors, 2 aggregates, 3 done
    uint32_t count = 0, next = 0;
    unsigned long long maskAll = 0, groupStatic = 0, groupLight = 0, groupKinematic = 0, groupHeavy = 0, groupCallback = 0;
    int shapes = 0, statics = 0, lights = 0, kinematics = 0, heavies = 0, callbacks = 0, universal = 0, overflow = 0, playerSaid = 0;
    GroupSeen seen[kMostGroups];
    int groups = 0;
    float at[3] = {};
    uint64_t steadySince = 0;
};
Census census;
constexpr int kMostListed = 40000;
void* listed[kMostListed];
constexpr float kCensusRadius = 15.0f, kHeavy = 50.0f, kNearPlayer = 0.5f;
uint32_t handFilter[4] = {}, handFilterStatic[4] = {};
bool filterReady = false;
// BITS NO SHAPE ROUND HERE HAS in its group or its mask (the census): the left hand's and the right hand's -- each hand's
// palm and fingers carry their own in their group and the other's in their mask while the two meet (either word lets a
// pair meet, so both or neither) -- and the parts of V's own body each hand meets (in that part's group; the hand's mask
// has its own while it meets them).
unsigned long long bitLeft = 0, bitRight = 0, bitSelfLeft = 0, bitSelfRight = 0;
bool selfBitsReady = false;
bool handsMeetOn = false, selfOn[2] = {false, false};                    // physics thread
bool scriptedOn[2] = {false, false};                                      // physics thread: Scripted(), as last seen
// THIS HAND'S GUN BOXES MEET THE OTHER HAND: off while the other hand is a script's (Scripted), on again once that hand
// is out of the boxes (OtherInGun) -- not once the palms are 26 cm apart, as the hands meeting each other needs: a
// pistol held in both hands never gets that far, and the gun's collision was gone for good (2026-11-11).
bool gunMeetOn[2] = {false, false};                                       // physics thread
extern "C" int CyberpunkVR_TwoHandActive;                                 // Anim/TwoHandGrip.cpp
// A HAND A SCRIPT IS PLACING: the wrist the reload, the holsters or the grenade claimed (shared slot 162, with their
// VRHandStopModel hold on it), the two-hand hold's support hand, or the free hand closing its grip with a gun out (taking
// the foregrip, a magazine, a holstered gun). Such a hand does not meet the gun's boxes nor the other hand: it is sent
// into them on purpose, and stopped on them it hung a few cm short of where the script put it -- the magazine never
// reached the well, the support hand off the foregrip (2026-11-11). Not by g_VRHandStopValid: HandCollision's own pass
// writes a stop on every unclaimed wrist it pushes and clears it again, and the gun's collision came and went with it.
bool Scripted(int side) {
    if (side < 0 || side > 1) return false;
    if (g_pSharedHands) {
        const float owned = g_pSharedHands[vrshared::kReloadOwnedHand];
        if (owned > -0.5f && int(owned + 0.5f) == side) return true;
    }
    if (!g_hasWeaponEquipped) return false;
    const bool dual = g_pSharedHands && g_pSharedHands[vrshared::kDualWieldActive] > 0.5f;
    if (dual) return false;
    const int gunHand = cvr::anim::WeaponInLeftHand() ? 0 : 1;
    if (side == gunHand) return false;
    if (CyberpunkVR_TwoHandActive != 0) return true;
    return g_pSharedHands && g_pSharedHands[side ? 49 : vrshared::kLeftGripPressed] > 0.5f;
}
std::atomic<bool> handsMeetShown{false}, selfOnShown[2] = {false, false};  // the same, for the worker

void Sample(void* a, int type) {
    // PxActorType: 0 static, 1 dynamic, 4 an articulation's link (a rigid body too); particles and cloth have no shapes to ask
    if (type != 0 && type != 1 && type != 4) return;
    if (type == 4) type = 1;
    PxBounds b{};
    At<void* (*)(void*, PxBounds*, float)>(a, kGetWorldBounds)(a, &b, 1.0f);
    float d2 = 0;
    for (int k = 0; k < 3; ++k) {
        const float c = census.at[k], e = c < b.lo[k] ? b.lo[k] - c : (c > b.hi[k] ? c - b.hi[k] : 0.f);
        d2 += e * e;
    }
    if (!(d2 <= kCensusRadius * kCensusRadius)) return;
    if (a == actor[0] || a == actor[1]) return;
    // a body in the player's own column (half a metre round the feet point, from the feet to over the head): the
    // player's controller capsule and any body of the player's own that rides with it
    float h2 = 0;
    for (int k = 0; k < 2; ++k) {
        const float c = census.at[k], e = c < b.lo[k] ? b.lo[k] - c : (c > b.hi[k] ? c - b.hi[k] : 0.f);
        h2 += e * e;
    }
    const bool byPlayer = h2 <= kNearPlayer * kNearPlayer && b.hi[2] >= census.at[2] - 0.2f && b.lo[2] <= census.at[2] + 2.1f;
    bool kinematic = false, heavy = false;
    if (type == 1) {
        uint8_t flags = 0;
        At<void* (*)(void*, uint8_t*)>(a, kGetRigidBodyFlags)(a, &flags);
        kinematic = (flags & 1) != 0;
        heavy = At<float (*)(void*)>(a, kGetMass)(a) > kHeavy;
    }
    void* list[8]{};
    const uint32_t n = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(a, kGetShapes)(a, list, 8, 0);
    for (uint32_t i = 0; i < n && i < 8; ++i) {
        uint32_t w[4]{};
        At<void* (*)(void*, uint32_t*)>(list[i], kShapeGetSimFilter)(list[i], w);
        const unsigned long long group = w[0] | (unsigned long long)w[1] << 32, mask = w[2] | (unsigned long long)w[3] << 32;
        // a shape of every group meeting every mask (the world's floor plane under the map) says nothing about anyone
        if (group == ~0ull || mask == ~0ull) { ++census.universal; continue; }
        ++census.shapes;
        census.maskAll |= mask;
        Kind kind;
        if (mask >> 62) { census.groupCallback |= group; ++census.callbacks; }
        if (type == 0) { census.groupStatic |= group; ++census.statics; kind = kindStatic; }
        else if (kinematic) { census.groupKinematic |= group; ++census.kinematics; kind = kindKinematic; }
        else if (heavy) { census.groupHeavy |= group; ++census.heavies; kind = kindHeavy; }
        else { census.groupLight |= group; ++census.lights; kind = kindLight; }
        GroupSeen* g = nullptr;
        for (int j = 0; j < census.groups && !g; ++j)
            if (census.seen[j].group == group) g = &census.seen[j];
        if (!g && census.groups < kMostGroups) {
            g = &census.seen[census.groups++];
            *g = GroupSeen{group, {}, 0, 0};
        }
        if (!g) { ++census.overflow; continue; }
        ++g->n[kind];
        if (mask >> 62) ++g->n[kindCallback];
        const int geometry = At<int (*)(void*)>(list[i], kShapeGetGeometryType)(list[i]);
        if (kinematic && geometry == kGeomCapsule) {
            struct { int type; float radius, halfHeight; } c{};
            if (At<bool (*)(void*, void*)>(list[i], kShapeGetCapsule)(list[i], &c)) {
                ++g->capsules;
                if (c.radius < 0.15f && c.halfHeight < 0.4f) ++g->limbs;
            }
        }
        if (byPlayer && kinematic) {
            ++g->n[kindPlayer];
            if (census.playerSaid < 8) {
                ++census.playerSaid;
                Log("[physhands]   the player's? a kinematic shape, geometry %d, bounds %.2f x %.2f x %.2f m from %.2f m over the feet, "
                    "group %016llx mask %016llx\n", geometry, b.hi[0] - b.lo[0], b.hi[1] - b.lo[1], b.hi[2] - b.lo[2],
                    b.lo[2] - census.at[2], group, mask);
            }
        }
    }
}

// THE HAND'S MASK, a bit for each group worth meeting. A group is worth it when what it holds is the world, light props
// or kinematic bodies (people's, doors: they push the hand, it cannot push them) -- the world alone for a hand holding
// something -- none of it the player's own and its heavy bodies a tenth of the rest at most. A bit meets every group
// that has it, so each wanted group gets the one of its bits that meets the fewest shapes not wanted; a bit any of the
// player's groups has is never taken. The 2026-11-08 census at H10 had the world in 5c004, props in 1000 and 1c008
// (with kinematic bodies in both), the player in 0 and 1. With kinematic groups left out the hands went through people.
// Pairs to the game's filter callback are fine: it and the contact report look the actor up by the id in its userData,
// and the hands' -1 finds nothing (Cyberpunk2077.exe 2.5: pairFound 0xC0EDC0, lookup 0x27D430, onContact 0x93FF10).
// A group whose kinematic capsules are mostly fat and tall is characters' controllers (an NPC's is a 0.4 m column round
// it, the player's is group 1): a hand would stop a hand's width short of the body, so those are not met; the thin
// short ones are limbs, and a hand stops on a limb.
bool WorthMeeting(const GroupSeen& g, bool statics) {
    // THE WORLD ALONE (a holding hand, the fingers, a held thing): a group of mostly static shapes. A group with one static
    // shape and forty-four kinematic bodies (0x4000, people's) took its only bit, 14, which the stall's plates (1c008) have
    // too: a held plate still met the stack it came out of (2026-11-09).
    if (statics)
        return g.n[kindStatic] > 2 * (g.n[kindLight] + g.n[kindKinematic] + g.n[kindHeavy]) && !g.n[kindPlayer];
    const int good = g.n[kindStatic] + (statics ? 0 : g.n[kindLight] + g.n[kindKinematic]);
    const bool controllers = g.capsules - g.limbs > g.limbs && g.n[kindStatic] + g.n[kindLight] == 0;
    return good > 0 && !g.n[kindPlayer] && g.n[kindHeavy] * 10 <= good && !controllers;
}
unsigned long long ChooseMask(bool statics, unsigned long long group, const char** rule) {
    unsigned long long player = 0;
    for (int j = 0; j < census.groups; ++j)
        if (census.seen[j].n[kindPlayer]) player |= census.seen[j].group;
    const unsigned long long allowed = ~(3ull << 62) & ~group & ~player;
    unsigned long long mask = 0;
    int missed = 0;
    for (int j = 0; j < census.groups; ++j) {
        const GroupSeen& g = census.seen[j];
        if (!WorthMeeting(g, statics) || (g.group & mask)) continue;
        int bestBit = -1, bestCost = 0;
        for (int bit = 0; bit < 62; ++bit) {
            if (!(g.group >> bit & 1) || !(allowed >> bit & 1)) continue;
            int cost = 0;
            for (int k = 0; k < census.groups; ++k) {
                const GroupSeen& o = census.seen[k];
                if ((o.group >> bit & 1) && !WorthMeeting(o, statics))
                    cost += o.n[kindStatic] + o.n[kindLight] + o.n[kindKinematic] + o.n[kindHeavy];
            }
            if (bestBit < 0 || cost < bestCost) bestBit = bit, bestCost = cost;
        }
        if (bestBit < 0) ++missed;
        else mask |= 1ull << bestBit;
    }
    *rule = missed ? "some wanted groups have only the player's bits" : "a bit a wanted group";
    return mask;
}
int Meets(unsigned long long mask, Kind kind) {
    int n = 0;
    for (int j = 0; j < census.groups; ++j)
        if (census.seen[j].group & mask) n += census.seen[j].n[kind];
    return n;
}

// THE LISTS ARE ASKED FOR AGAIN EACH STEP, and only this step's slice of them read. A list kept from one step to the next
// held actors the game released in between, on the way in from a load, and the census called through one of them
// (2026-11-08 22:02, ReportQueue ...-220237). A list that changed under the walk only moves which actors a slice sees.
void CensusSlice(void* sc, const float* origin) {
    constexpr uint32_t kSlice = 400;
    if (census.phase == 0) {
        // the player in a world that has stepped steadily for three seconds: not a menu's, not a loading screen's
        const uint64_t now = GetTickCount64();
        if (!CyberpunkVR_PlayerEntityValid || g_menuModeValue != 0) { census.steadySince = 0; return; }
        if (!census.steadySince) census.steadySince = now;
        if (now - census.steadySince < 3000) return;
        for (int k = 0; k < 3; ++k) census.at[k] = CyberpunkVR_PlayerEntityPos[k] - origin[k];
        census.next = 0;
        census.phase = 1;
        return;
    }
    if (census.phase == 1) {
        const uint16_t types = 1 | 2;
        census.count = At<uint32_t (*)(void*, const uint16_t*, void**, uint32_t, uint32_t)>(sc, kSceneGetActors)(sc, &types, listed, kMostListed, 0);
        const uint32_t end = census.next + kSlice < census.count ? census.next + kSlice : census.count;
        for (uint32_t i = census.next; i < end; ++i) Sample(listed[i], At<int (*)(void*)>(listed[i], kGetType)(listed[i]));
        census.next = end;
        if (end >= census.count) {
            census.next = 0;
            census.phase = 2;
        }
        return;
    }
    if (census.phase == 2) {
        census.count = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(sc, kSceneGetAggregates)(sc, listed, kMostListed, 0);
        const uint32_t end = census.next + kSlice / 8 < census.count ? census.next + kSlice / 8 : census.count;
        for (uint32_t i = census.next; i < end; ++i) {
            void* members[64]{};
            const uint32_t n = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(listed[i], kAggregateGetActors)(listed[i], members, 64, 0);
            for (uint32_t m = 0; m < n && m < 64; ++m) Sample(members[m], At<int (*)(void*)>(members[m], kGetType)(members[m]));
        }
        census.next = end;
        if (end < census.count) return;
        census.phase = 3;
        Settings s{};
        settings.Read(s);
        // the hand's group: the highest bit under 62 no mask round here has
        unsigned long long group = 0;
        for (int bit = 61; bit >= 32 && !group; --bit)
            if (!(census.maskAll >> bit & 1)) group = 1ull << bit;
        if (s.groupOverride) group = s.groupOverride;
        const char *rule = "", *ruleStatic = "";
        unsigned long long mask = ChooseMask(false, group, &rule);
        unsigned long long maskStatic = ChooseMask(true, group, &ruleStatic);
        if (s.maskOverride) mask = s.maskOverride, maskStatic = s.maskOverride & census.groupStatic;
        handFilter[0] = uint32_t(group); handFilter[1] = uint32_t(group >> 32);
        handFilter[2] = uint32_t(mask); handFilter[3] = uint32_t(mask >> 32);
        std::memcpy(handFilterStatic, handFilter, sizeof(handFilter));
        handFilterStatic[2] = uint32_t(maskStatic); handFilterStatic[3] = uint32_t(maskStatic >> 32);
        filterReady = group != 0 && mask != 0;
        {
            unsigned long long groups = census.groupStatic | census.groupLight | census.groupKinematic | census.groupHeavy | census.groupCallback;
            for (int j = 0; j < census.groups; ++j) groups |= census.seen[j].group;
            const unsigned long long used = census.maskAll | groups | group | (3ull << 62);
            unsigned long long bits[4] = {};
            int found = 0;
            for (int bit = 61; bit >= 0 && found < 4; --bit)
                if (!(used >> bit & 1)) bits[found++] = 1ull << bit;
            selfBitsReady = found == 4 && filterReady;
            bitLeft = selfBitsReady ? bits[0] : 0;
            bitRight = selfBitsReady ? bits[1] : 0;
            bitSelfLeft = selfBitsReady ? bits[2] : 0;
            bitSelfRight = selfBitsReady ? bits[3] : 0;
            Log("[physhands] the hands' own bits: left %016llx, right %016llx, V's body for the left %016llx, for the right %016llx%s\n",
                bitLeft, bitRight, bitSelfLeft, bitSelfRight, selfBitsReady ? "" : " (too few free: the hands meet neither V nor each other)");
        }
        Log("[physhands] census of %d shapes within %.0f m (and %d of every group): static %d (groups %016llx), light %d (%016llx), kinematic %d (%016llx), "
            "heavy %d (%016llx), to the game's callback %d (groups %016llx); every mask %016llx. The hands: group %016llx, mask %016llx "
            "(holding: %016llx)%s\n",
            census.shapes, kCensusRadius, census.universal, census.statics, census.groupStatic, census.lights, census.groupLight, census.kinematics,
            census.groupKinematic, census.heavies, census.groupHeavy, census.callbacks, census.groupCallback, census.maskAll, group, mask,
            maskStatic, filterReady ? "" : ": nothing to meet, no hands");
        for (int j = 0; j < census.groups; ++j) {
            const GroupSeen& g = census.seen[j];
            Log("[physhands]   group %016llx: static %d, light %d, kinematic %d (capsules %d, limb-sized %d), heavy %d; to the callback %d; "
                "the player's %d\n", g.group, g.n[kindStatic], g.n[kindLight], g.n[kindKinematic], g.capsules, g.limbs, g.n[kindHeavy],
                g.n[kindCallback], g.n[kindPlayer]);
        }
        Log("[physhands] the mask (%s) meets static %d, light %d, kinematic %d, heavy %d, the player's %d; holding (%s) static %d, light %d%s\n",
            rule, Meets(mask, kindStatic), Meets(mask, kindLight), Meets(mask, kindKinematic), Meets(mask, kindHeavy), Meets(mask, kindPlayer), ruleStatic,
            Meets(maskStatic, kindStatic), Meets(maskStatic, kindLight), census.overflow ? " (more groups than kept)" : "");
    }
}

// ---- making and steering the bodies (physics thread) ----
// THE PALM AND A GUN'S BOXES meet what the hand may touch (this filter); THE FINGERS AND THE FOREARM the world alone,
// always (their filter is set when they are made). A finger capsule is put where the finger is drawn each step, not moved
// there: closed in a stack of plates it appeared inside them, and PhysX threw them apart.
// PHYSX 3.4 KEEPS A PAIR MADE UNDER THE OLD FILTER until it is asked to filter it again: a held plate set apart from the
// stack it came out of went on striking it, a hand that closed on a thing went on pushing it. So every change is followed
// by this, on the thread that steps the scene, between steps.
void Refilter(void* actorToFilter) {
    if (scene && actorToFilter) At<void (*)(void*, void*)>(scene, kSceneResetFiltering)(scene, actorToFilter);
}

uint32_t weaponFilterNow[2][4] = {}, fingerFilterNow[2][4] = {}, forearmFilterNow[2][4] = {};
// The filters of a hand's shapes: its palm's, its gun's boxes', its fingers', its forearm's.
void SetFilter(int side, const uint32_t* palm, const uint32_t* weapon, const uint32_t* fingers, const uint32_t* forearm) {
    if (!shape[side]) return;
    bool changed = false;
    if (std::memcmp(filterNow[side], palm, sizeof(filterNow[side]))) {
        At<void (*)(void*, const uint32_t*)>(shape[side], kShapeSetSimFilter)(shape[side], palm);
        std::memcpy(filterNow[side], palm, sizeof(filterNow[side]));
        changed = true;
    }
    if (std::memcmp(weaponFilterNow[side], weapon, sizeof(weaponFilterNow[side]))) {
        for (void* sh : weaponShape[side])
            if (sh) At<void (*)(void*, const uint32_t*)>(sh, kShapeSetSimFilter)(sh, weapon);
        std::memcpy(weaponFilterNow[side], weapon, sizeof(weaponFilterNow[side]));
        changed = true;
    }
    if (std::memcmp(fingerFilterNow[side], fingers, sizeof(fingerFilterNow[side]))) {
        for (auto& finger : fingerShape[side])
            for (void* sh : finger)
                if (sh) At<void (*)(void*, const uint32_t*)>(sh, kShapeSetSimFilter)(sh, fingers);
        std::memcpy(fingerFilterNow[side], fingers, sizeof(fingerFilterNow[side]));
        changed = true;
    }
    if (std::memcmp(forearmFilterNow[side], forearm, sizeof(forearmFilterNow[side]))) {
        for (void* sh : {forearmShape[side], forearmUpper[side]})
            if (sh) At<void (*)(void*, const uint32_t*)>(sh, kShapeSetSimFilter)(sh, forearm);
        std::memcpy(forearmFilterNow[side], forearm, sizeof(forearmFilterNow[side]));
        changed = true;
    }
    if (changed) Refilter(actor[side]);
}

// THE PALM'S AND THE FINGERS' FILTERS: the world's (with the loose things, or the world alone), and V's own body and the
// other hand while the hand meets them. The fingers meet the world alone, as made.
// A GUN'S BOXES meet nothing of the world -- a gun stopped by a wall turned the aim -- and the other hand by this hand's own
// bit while the two meet: in that bit's group, meeting nothing themselves (the pair rule is an or). THE FOREARM meets the
// world, and the other hand's palm and fingers by the same bit (the other hand went through it); not V's own body (it lies
// along the trunk) and not the other forearm (arms cross).
void HandFilters(int side, bool loose, uint32_t* palm, uint32_t* fingers, uint32_t* weapon, uint32_t* forearm) {
    const uint32_t* base = loose ? handFilter : handFilterStatic;
    unsigned long long group = handFilter[0] | (unsigned long long)handFilter[1] << 32;
    unsigned long long mask = base[2] | (unsigned long long)base[3] << 32;
    unsigned long long maskF = handFilterStatic[2] | (unsigned long long)handFilterStatic[3] << 32;
    unsigned long long gunGroup = 0;
    unsigned long long foreGroup = handFilterStatic[0] | (unsigned long long)handFilterStatic[1] << 32;
    const unsigned long long bit = side ? bitRight : bitLeft, otherBit = side ? bitLeft : bitRight;
    if (handsMeetOn) {
        group |= bit;
        mask |= otherBit;
        maskF |= otherBit;
        foreGroup |= bit;
    }
    // the gun's boxes carry this hand's bit while they meet the other hand; this hand meets the other's gun by its bit
    if (gunMeetOn[side]) gunGroup = bit;
    if (gunMeetOn[1 - side]) {
        mask |= otherBit;
        maskF |= otherBit;
    }
    weapon[0] = uint32_t(gunGroup);
    weapon[1] = uint32_t(gunGroup >> 32);
    weapon[2] = weapon[3] = 0;
    forearm[0] = uint32_t(foreGroup);
    forearm[1] = uint32_t(foreGroup >> 32);
    forearm[2] = handFilterStatic[2];
    forearm[3] = handFilterStatic[3];
    if (selfOn[side]) {
        mask |= side ? bitSelfRight : bitSelfLeft;
        maskF |= side ? bitSelfRight : bitSelfLeft;
    }
    palm[0] = fingers[0] = uint32_t(group);
    palm[1] = fingers[1] = uint32_t(group >> 32);
    palm[2] = uint32_t(mask);
    palm[3] = uint32_t(mask >> 32);
    fingers[2] = uint32_t(maskF);
    fingers[3] = uint32_t(maskF >> 32);
}

// The hand's material on every shape of it: the open hand's or the closed one's.
void SetMaterials(int side, bool grip) {
    if (gripMaterial[side] == grip || !shape[side] || !materialGrip) return;
    void* mats[1] = {grip ? materialGrip : material};
    const auto set = [&](void* sh) {
        if (sh) At<void (*)(void*, void* const*, uint16_t)>(sh, kShapeSetMaterials)(sh, mats, 1);
    };
    set(shape[side]);
    for (auto& finger : fingerShape[side])
        for (void* sh : finger) set(sh);
    set(forearmShape[side]);
    set(forearmUpper[side]);
    for (void* sh : weaponShape[side]) set(sh);
    gripMaterial[side] = grip;
}

// A shape on body a, simulation only, at `local` (a capsule's axis is its x), with this filter.
void* Attach(void* a, const void* geometry, const PxTransform& local, const uint32_t* filter) {
    void* mats[1] = {material};
    const uint8_t flags = 1;
    void* sh = At<void* (*)(void*, const void*, void* const*, uint16_t, bool, const uint8_t*)>(physics, kCreateShape)(physics, geometry, mats, 1, true, &flags);
    if (!sh) return nullptr;
    At<void (*)(void*, const PxTransform*)>(sh, kShapeSetLocalPose)(sh, &local);
    At<void (*)(void*, const uint32_t*)>(sh, kShapeSetSimFilter)(sh, filter);
    At<bool (*)(void*, void*)>(a, kAttachShape)(a, sh);
    At<void (*)(void*)>(sh, kShapeRelease)(sh);
    return sh;
}
void* Capsule(void* a, float radius, float half, const PxTransform& local) {
    const struct { int type; float radius, halfHeight; } g{kGeomCapsule, radius, half};
    return Attach(a, &g, local, handFilterStatic);
}
// what TouchesWorld counts: the world alone; with the people near; with V's own body and the other hand as well
enum TouchOf { kTouchWorld = 0, kTouchPeople = 1, kTouchAll = 2 };
bool TouchesWorld(int side, const PxTransform& at, const Settings& s, int what = kTouchAll);
void PoseShapes(int side, const Settings& s);

// The palm's box for this hand: V's own skin once the body is known (Natives/HandSkin.hpp), the ini's otherwise. What it
// was fitted to: 0 a man's, 1 a woman's, -1 the ini's.
int BoxFor(int side, const Settings& s, float* half, float* centre) {
    const int female = g_VRBodyFemale;
    const HandSkin* k = s.skin ? SkinFor(female, side) : nullptr;
    if (k) {
        for (int i = 0; i < 3; ++i) half[i] = 0.5f * (k->boxHi[i] - k->boxLo[i]), centre[i] = 0.5f * (k->boxHi[i] + k->boxLo[i]);
        return female ? 1 : 0;
    }
    std::memcpy(half, s.half, sizeof(s.half));
    centre[0] = side == 1 ? -s.along : s.along;
    centre[1] = centre[2] = 0;
    return -1;
}
// ...grown by the skin's margin
int BoxWithMargin(int side, const Settings& s, float* half, float* centre) {
    const int of = BoxFor(side, s, half, centre);
    for (int i = 0; i < 3; ++i) half[i] += s.skinMargin;
    return of;
}

// THE HAND'S MASS IN ITS PALM: the body's centre of mass at the palm box's middle, the box's own inertia about it. At the
// wrist (the actor's origin, PhysX's default) the steering pushed the wrist and a wall stopped the palm's face 4 cm on:
// pressed on, the box pivoted on its heel and lifted its fingers' end 18 mm off the wall, and the fingers curled down to
// it -- from V's eyes they went into the wall and only the palm held (2026-11-09).
void Cross(const float* a, const float* b, float* o);
void SetMassAt(void* a, float m, const float* half, const float* c) {
    const PxTransform at{{0, 0, 0, 1}, {c[0], c[1], c[2]}};
    At<void (*)(void*, const PxTransform*)>(a, kSetCMassLocalPose)(a, &at);
    At<void (*)(void*, float)>(a, kSetMass)(a, m);
    // ...turning as hard to turn as about the wrist (the box's own inertia and its shift there): with the box's own alone,
    // a third of it, the palm pressed on a wall rocked on it more (the latch flipping 8-14 times in 13 s, 2 before); a
    // hand on its forearm is slow to turn anyway
    const float X = 2 * half[0], Y = 2 * half[1], Z = 2 * half[2];
    const float inertia[3] = {m / 12 * (Y * Y + Z * Z) + m * (c[1] * c[1] + c[2] * c[2]),
                              m / 12 * (X * X + Z * Z) + m * (c[0] * c[0] + c[2] * c[2]),
                              m / 12 * (X * X + Y * Y) + m * (c[0] * c[0] + c[1] * c[1])};
    At<void (*)(void*, const float*)>(a, kSetInertia)(a, inertia);
}

bool Make(int side, const PxTransform& at, const Settings& s) {
    if (!material) {
        material = At<void* (*)(void*, float, float, float)>(physics, kCreateMaterial)(physics, s.friction, s.friction * 0.85f, 0.0f);
        materialGrip = At<void* (*)(void*, float, float, float)>(physics, kCreateMaterial)(physics, s.frictionGrip, s.frictionGrip * 0.85f, 0.0f);
        if (!material || !materialGrip) return false;
        if (*reinterpret_cast<uintptr_t*>(material) == dll + kMaterialTable)
            At<void (*)(void*, int)>(material, kMaterialSetFrictionCombine)(material, kCombineMin);
    }
    void* a = At<void* (*)(void*, const PxTransform*)>(physics, kCreateRigidDynamic)(physics, &at);
    if (!a) return false;
    float boxHalf[3], boxCentre[3];
    const int skinOf = BoxWithMargin(side, s, boxHalf, boxCentre);
    const BoxGeometry box{3, {boxHalf[0], boxHalf[1], boxHalf[2]}};
    void* mats[1] = {material};
    const uint8_t flags = 1;   // a simulation shape only: no ray, no sweep, no character controller sees it
    void* sh = At<void* (*)(void*, const void*, void* const*, uint16_t, bool, const uint8_t*)>(physics, kCreateShape)(physics, &box, mats, 1, true, &flags);
    if (!sh) {
        At<void (*)(void*)>(a, kRelease)(a);
        return false;
    }
    // the box along the fingers: -x of the right hand's bone, +x of the left's
    const PxTransform local{{0, 0, 0, 1}, {boxCentre[0], boxCentre[1], boxCentre[2]}};
    At<void (*)(void*, const PxTransform*)>(sh, kShapeSetLocalPose)(sh, &local);
    At<void (*)(void*, const uint32_t*)>(sh, kShapeSetSimFilter)(sh, handFilter);
    std::memcpy(filterNow[side], handFilter, sizeof(handFilter));
    std::memset(weaponFilterNow[side], 0, sizeof(weaponFilterNow[side]));
    std::memcpy(fingerFilterNow[side], handFilterStatic, sizeof(handFilterStatic));
    std::memcpy(forearmFilterNow[side], handFilterStatic, sizeof(handFilterStatic));
    At<bool (*)(void*, void*)>(a, kAttachShape)(a, sh);
    At<void (*)(void*)>(sh, kShapeRelease)(sh);   // the body holds it now
    // THE FINGERS AND THE FOREARM, capsules on the same body, posed each step as they are drawn (PoseShapes): what a
    // fingertip or the forearm meets stops the hand as the palm does, and is pushed as by the palm. Made in the palm.
    for (int f = 0; f < 5; ++f)
        for (int j = 0; j < 3; ++j) {
            fingerShape[side][f][j] = (f == 4 && j == 0) ? nullptr : Capsule(a, 0.008f, 0.01f, local);
            fingerHalf[side][f][j] = 0.01f;
        }
    forearmShape[side] = Capsule(a, 0.035f, 0.08f, local);
    forearmHalf[side] = 0.08f;
    forearmUpper[side] = Capsule(a, 0.035f, 0.08f, local);
    forearmUpperHalf[side] = 0.08f;
    forearmRadius[side][0] = forearmRadius[side][1] = 0.035f;
    for (auto& f : fingerRadius[side])
        for (float& r : f) r = 0.008f;
    for (int i = 0; i < kMostWeaponBoxes; ++i) {
        const BoxGeometry parked{kGeomBox, {0.004f, 0.004f, 0.004f}};
        static const uint32_t none[4] = {0, 0, 0, 0};   // nothing until the hands meet (HandFilters)
        weaponShape[side][i] = Attach(a, &parked, local, none);
        for (int k = 0; k < 3; ++k) weaponMade[side][i][k] = 0.004f;
    }
    weaponOn[side] = 0;
    gripMaterial[side] = false;
    SetMassAt(a, s.mass, boxHalf, boxCentre);
    const float X = 2 * boxHalf[0], Y = 2 * boxHalf[1], Z = 2 * boxHalf[2];
    At<void (*)(void*, int, bool)>(a, kSetActorFlag)(a, kDisableGravity, true);
    // continuous collision and the contact offset: set in BeforeSimulate, as they can change on a made body
    appliedCcd[side] = -1;
    appliedContactOffset[side] = -1.f;
    ownContactOffset[side] = At<float (*)(void*)>(sh, kShapeGetContactOffset)(sh);
    At<void (*)(void*, float)>(a, kSetLinearDamping)(a, 0.0f);
    At<void (*)(void*, float)>(a, kSetAngularDamping)(a, 0.05f);
    At<void (*)(void*, float)>(a, kSetMaxAngularVelocity)(a, 200.0f);
    At<void (*)(void*, float)>(a, kSetMaxDepenetration)(a, s.depenetration);
    At<void (*)(void*, uint32_t, uint32_t)>(a, kSetSolverIterations)(a, 16, 4);
    // PxActor::userData (+0x10): the game reads a proxy id there (low 32 bits) and a body index (the next 16); all ones
    // finds nothing in its handle table, where 0 could name a live proxy
    *reinterpret_cast<uintptr_t*>(static_cast<char*>(a) + 0x10) = ~uintptr_t(0);
    At<void (*)(void*, void*, const void*)>(scene, kAddActor)(scene, a, nullptr);
    actor[side] = a;
    shape[side] = sh;
    simulating[side] = true;
    appliedMass[side] = s.mass;
    appliedDepenetration[side] = s.depenetration;
    std::memcpy(madeHalf[side], boxHalf, sizeof(boxHalf));
    std::memcpy(madeCentre[side], boxCentre, sizeof(boxCentre));
    madeAlong[side] = std::fabs(boxCentre[0]);
    madeSkin[side] = skinOf;
    Log("[physhands] %s hand made: body %p, a %.1f kg box %.3f x %.3f x %.3f m about (%.3f %.3f %.3f), %s, group %08x%08x mask %08x%08x\n",
        side ? "right" : "left", a, s.mass, X, Y, Z, boxCentre[0], boxCentre[1], boxCentre[2],
        skinOf == 1 ? "a woman's skin" : (skinOf == 0 ? "a man's skin" : "the ini's box"), handFilter[1], handFilter[0], handFilter[3],
        handFilter[2]);
    return true;
}

// Where tracking puts the hand now, PhysX's space.
bool TargetNow(int side, const float* origin, PxTransform& out) {
    Target t{};
    if (!targets[side].Read(t) || GetTickCount64() - t.at > 150) return false;
    float p[3];
    QRot(t.entityRot, t.pos, p);
    for (int k = 0; k < 3; ++k) out.p[k] = t.entityPos[k] + p[k] - origin[k];
    for (int k = 0; k < 3; ++k) targetEntity[side][k] = t.entityPos[k] - origin[k];
    QMul(t.entityRot, t.rot, out.q);
    return QNorm(out.q) && std::isfinite(out.p[0] + out.p[1] + out.p[2]);
}

// THE PLAYER'S OWN PACE (world, m/s), from the entity's place as the game thread writes it each frame: over the time
// between two different places, not a step's (two steps can fall in one frame, the second seeing no move). A jump of
// more than a metre is a teleport, not a pace. Physics thread.
float playerPace[3] = {0, 0, 0};
void PlayerPace() {
    static float last[3] = {0, 0, 0};
    static LARGE_INTEGER lastAt{};
    static bool have = false;
    LARGE_INTEGER now, freq;
    QueryPerformanceCounter(&now);
    QueryPerformanceFrequency(&freq);
    if (!CyberpunkVR_PlayerEntityValid) {
        have = false;
        playerPace[0] = playerPace[1] = playerPace[2] = 0;
        return;
    }
    const float p[3] = {CyberpunkVR_PlayerEntityPos[0], CyberpunkVR_PlayerEntityPos[1], CyberpunkVR_PlayerEntityPos[2]};
    if (!std::isfinite(p[0] + p[1] + p[2])) return;
    const double since = double(now.QuadPart - lastAt.QuadPart) / double(freq.QuadPart);
    float d[3], d2 = 0;
    for (int k = 0; k < 3; ++k) d[k] = p[k] - last[k], d2 += d[k] * d[k];
    if (!have) {
        std::memcpy(last, p, sizeof(last));
        lastAt = now;
        have = true;
        return;
    }
    if (d2 < 1e-10f) {
        if (since > 0.1) playerPace[0] = playerPace[1] = playerPace[2] = 0;   // standing, or the game stalled
        return;
    }
    std::memcpy(last, p, sizeof(last));
    lastAt = now;
    if (d2 > 1.f || since <= 1e-4 || since > 0.1) {
        playerPace[0] = playerPace[1] = playerPace[2] = 0;
        return;
    }
    for (int k = 0; k < 3; ++k) playerPace[k] = float(d[k] / since);
    const float sp = std::sqrt(playerPace[0] * playerPace[0] + playerPace[1] * playerPace[1] + playerPace[2] * playerPace[2]);
    if (sp > 30.f) for (auto& c : playerPace) c *= 30.f / sp;
}

// The bodies ride with the player's entity (Steer): no further than this in a step (more is a teleport, put); how many
// steps rode (statistics) and how far this step's ride went (the held-off diagnostic compares against it).
constexpr float kRideMost = 1.0f;
std::atomic<uint32_t> rodeSteps[2] = {};
float rodeBy[2][3] = {};

void Steer(int side, const PxTransform& to, float dt, const Settings& s, bool put, bool riding) {
    void* a = actor[side];
    // THE HAND RIDES WITH THE PLAYER (Bonelab's hands are on its moving rig): each step the body is moved by exactly the
    // entity's move that came with this step's target (targetEntity: the same frame's), and steered only by the hand's
    // own move against the player. Carried by the player's PACE instead -- estimated from the entity's places as the
    // physics thread saw them, a frame late and uneven -- a running hand was sent a little too fast one step and too slow
    // the next and shook, and at a dash's end it kept the dash's 10-15 m/s for up to 0.1 s and flew on ahead
    // (2026-11-14). Not while the hand holds on (a climb, a grab): there the body stays where it took hold and the player
    // follows it. A move of a metre or more is a teleport (put, below).
    // (Not with the entity's TURN: the hands' frame turns with the camera's heading, which the body follows with a lag of
    // its own; turned by the entity's turn the bodies were sent off tracking up to 5 cm through a stick's turn, 2026-11-14.)
    static float rideFrom[2][3] = {};
    static bool rideValid[2] = {false, false};
    float ride[3] = {0, 0, 0}, ride2 = 0;
    if (rideValid[side])
        for (int k = 0; k < 3; ++k) ride[k] = targetEntity[side][k] - rideFrom[side][k], ride2 += ride[k] * ride[k];
    std::memcpy(rideFrom[side], targetEntity[side], sizeof(rideFrom[side]));
    rideValid[side] = std::isfinite(rideFrom[side][0] + rideFrom[side][1] + rideFrom[side][2]);
    const bool rides = riding && s.ride != 0;
    // Riding, the body's velocity is its own move against the player: the player's pace is 0 to everything below.
    const float zeroPace[3] = {0, 0, 0};
    const float* pace = rides ? zeroPace : playerPace;
    // THE PLAYER'S CHANGE OF PACE SINCE THE LAST STEP, the hand carried by it whole: a dash's start is 10-15 m/s in a few
    // steps, and held to a touch's push (50 N) a hand on the other or on the player's own body fell behind, was struck by
    // the body it lagged, touched again, and shook. The caps below hold only the hand's own move against the player.
    static float paceWas[2][3] = {};
    float paceDv[3];
    for (int k = 0; k < 3; ++k) paceDv[k] = pace[k] - paceWas[side][k], paceWas[side][k] = pace[k];
    PxTransform now{};
    At<void (*)(void*, PxTransform*)>(a, kGetGlobalPose)(a, &now);
    if (rides && !put && simulating[side] && ride2 > 1e-12f && ride2 < kRideMost * kRideMost) {
        for (int k = 0; k < 3; ++k) now.p[k] += ride[k];
        At<void (*)(void*, const PxTransform*, bool)>(a, kSetGlobalPose)(a, &now, true);
        // the last step's target moved with the player too: what is left of tracking's move is the hand's own
        if (sentValid[side])
            for (int k = 0; k < 3; ++k) sent[side][4 + k] += ride[k];
        rodeSteps[side].fetch_add(1, std::memory_order_relaxed);
        std::memcpy(rodeBy[side], ride, sizeof(ride));
    } else {
        rodeBy[side][0] = rodeBy[side][1] = rodeBy[side][2] = 0;
    }
    float gap2 = 0;
    for (int k = 0; k < 3; ++k) gap2 += (to.p[k] - now.p[k]) * (to.p[k] - now.p[k]);
    if (put || !simulating[side] || !(gap2 < s.teleport * s.teleport)) {
        // put there: just made, just back on, so far off (a teleport, a load) that steering would fling it through
        // things, the player just moved by a teleport, or stuck behind something an arm does not reach round
        At<void (*)(void*, const PxTransform*, bool)>(a, kSetGlobalPose)(a, &to, true);
        const float zero[3] = {0, 0, 0};
        At<void (*)(void*, const float*, bool)>(a, kSetLinearVelocity)(a, zero, true);
        At<void (*)(void*, const float*, bool)>(a, kSetAngularVelocity)(a, zero, true);
        gaveValid[side] = false;
        if (!simulating[side]) {
            At<void (*)(void*, int, bool)>(a, kSetActorFlag)(a, kDisableSimulation, false);
            simulating[side] = true;
        }
        return;
    }
    float v[3]{}, w[3]{};
    At<void (*)(void*, float*)>(a, kGetLinearVelocity)(a, v);
    At<void (*)(void*, float*)>(a, kGetAngularVelocity)(a, w);
    const bool carried = gaveValid[side];   // (the touch test below compares PhysX's velocity with the one given: not carried yet)
    // TOUCHING: nothing else acts on the body (no gravity, no damping), so a velocity that is not the one it was given
    // is the solver's doing -- something stopped or turned it. Both as PhysX has them, the centre of mass's: a free
    // body's stays what it was given through the step, while the wrist's turns with the body about the palm's middle.
    // Compared as the wrist's, a quick turn read as a touch (w^2 |c| dt: 0.05 m/s at 10 rad/s) and the hand in the air
    // was steered at a push's strength and drawn as one letting go of a wall -- it floated behind a swing.
    const uint64_t tick = GetTickCount64();
    if (gaveValid[side]) {
        float dl = 0, da = 0;
        for (int k = 0; k < 3; ++k) {
            dl += (v[k] - gaveCom[side][k]) * (v[k] - gaveCom[side][k]);
            da += (w[k] - gave[side][3 + k]) * (w[k] - gave[side][3 + k]);
        }
        // a short window: past it the hand is quick again. At 250 ms a hand drawn off a table stayed weak and slow
        // that long, fell behind and caught up in a jump (2026-11-08: "the hand teleports instead of colliding")
        if (dl > 0.05f * 0.05f || da > 0.5f * 0.5f) touchUntil[side] = tick + 60;
        // the way the solver pushed it: out of what it touches (an impulse changes the centre of mass's velocity by J/m)
        if (dl > 0.05f * 0.05f) {
            const float n = std::sqrt(dl);
            for (int k = 0; k < 3; ++k) pushedOut[side][k] = (v[k] - gaveCom[side][k]) / n;
        }
    }
    // PhysX's velocity is the centre of mass's (the palm's middle); the steering below is the wrist's (the actor's origin):
    // the wrist moves at the palm's velocity less the turn's share, w x (palm - wrist)
    float com[3], wc[3];
    QRot(now.q, madeCentre[side], com);
    Cross(w, com, wc);
    for (int k = 0; k < 3; ++k) v[k] -= wc[k];
    const bool touching = tick < touchUntil[side];
    if (touching) touchedSteps[side].fetch_add(1, std::memory_order_relaxed);
    // ON THE WORLD OR ON A LOOSE THING. Pressed on a table the hand leans soft and turns little, so it lies still; on a
    // cup it pushes with a hand's force at the hand's own pace. With the world's caps on every touch a cup barely moved.
    const bool world = touching && TouchesWorld(side, now, s);
    onWorld[side] = world;
    onPushable[side] = world && TouchesWorld(side, now, s, kTouchPeople);
    if (world) worldSteps[side].fetch_add(1, std::memory_order_relaxed);
    // all the way there by the step's end, the change held to what the hand has: quick in the air, a hand's push on
    // something
    float set[3], dv[3];
    // PRESSED, it is not flung at the target every step: a body sent at the far side of a table at 9 m/s and stopped
    // by it each step rocked on its edge and shook the drawn hand (5 mm a step, 2026-11-08). It leans in at a walking
    // pace, which is still all of `force`.
    // ...and on the world it closes only part of the gap a step: sent all the way at once, a body pressed into the floor
    // and sliding on it (friction 0.35) overshot each step and stirred 6-7 mm
    float want[3];
    const bool settle = world && s.settle != 0;
    const float gain = world && !settle ? s.pressGain : 1.f;
    // ...a share of the hand's OWN gap, against the player: the player's own pace is carried whole. Running with the hands
    // palm to palm (each the other's world here), a share of a gap that held the run's 10 cm a step left a hand 4-11 cm
    // behind, touching and letting go every few frames (2026-11-11)
    for (int k = 0; k < 3; ++k) want[k] = pace[k] + ((to.p[k] - now.p[k]) / dt - pace[k]) * gain;
    if (world && gain < 1.f) {
        // away from the surface it is all the way: lifted off a table at a share of its gap a step, the body was left 3 cm
        // behind tracking for the few steps it still touched, and the drawn hand jumped up as it let go
        const float* n = pushedOut[side];
        float out = 0;
        for (int k = 0; k < 3; ++k) out += ((to.p[k] - now.p[k]) / dt - pace[k]) * n[k];
        if (out > 0) for (int k = 0; k < 3; ++k) want[k] += n[k] * out * (1.f - gain);
    }
    // all the way by the step's end is tracking's own move this step plus the gap the body already had; the gap's share
    // is held to catch_speed
    if (sentValid[side] && (!world || settle)) {
        float carry[3], gap[3], g = 0;
        for (int k = 0; k < 3; ++k) {
            carry[k] = (to.p[k] - sent[side][4 + k]) / dt;
            gap[k] = want[k] - carry[k];
            g += gap[k] * gap[k];
        }
        g = std::sqrt(g);
        // ON THE WORLD, TRACKING'S PLACE CLEAR: the gap closes at catch_speed as in the air. A hand that caught on a seam or
        // an edge as it slid, its controller already out in the open, came free half a metre a second at a time and was
        // drawn centimetres behind it while it still touched. Tracking's place in the world (pressed in), at press_catch:
        // faster, a hand pressed 8 cm into a wall rocked, 4 mm off its line a frame (and split by the solver's push
        // direction, the part of a deep press off that direction rocked it 15 mm sideways, period 2).
        trackClear[side] = settle && g > 0.01f && !TouchesWorld(side, to, s);
        const float catchMost = settle && !trackClear[side] ? s.pressCatch : s.catchSpeed;
        if (g > catchMost)
            for (int k = 0; k < 3; ++k) want[k] = carry[k] + gap[k] * (catchMost / g);
    }
    if (world) {
        // only the part into the surface: along it and away from it the hand is as quick as ever
        const float* n = pushedOut[side];
        float into = 0;   // (against the player: what a running player's hand touches runs with it, the other hand or V's own body)
        for (int k = 0; k < 3; ++k) into -= (want[k] - pace[k]) * n[k];
        if (into > s.pressSpeed) for (int k = 0; k < 3; ++k) want[k] += n[k] * (into - s.pressSpeed);
    }
    if (carried)
        for (int k = 0; k < 3; ++k) v[k] += paceDv[k];
    for (int k = 0; k < 3; ++k) dv[k] = want[k] - v[k];
    const float dvn = std::sqrt(dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]);
    if (settle) {
        // Bonelab's _jerkedMult: up while the ask grows, down toward 15% while it does not
        settleLin[side] = dvn > lastAsk[side] + 0.005f ? std::min(1.f, settleLin[side] + 3.2f * dt) : std::max(0.15f, settleLin[side] - 4.f * dt);
    } else {
        settleLin[side] = 1.f;
    }
    lastAsk[side] = dvn;
    const float most = (settle ? s.moveAccel * settleLin[side] : world ? s.force / s.mass : touching ? s.pushForce / s.mass : s.moveAccel) * dt;
    const float kv = dvn > most ? most / dvn : 1.f;
    for (int k = 0; k < 3; ++k) set[k] = v[k] + dv[k] * kv;
    {
        // the speed cap on the hand's own move: against the player, not the world
        float rel[3];
        for (int k = 0; k < 3; ++k) rel[k] = set[k] - pace[k];
        const float sp = std::sqrt(rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]);
        if (sp > s.maxSpeed) for (int k = 0; k < 3; ++k) set[k] = pace[k] + rel[k] * (s.maxSpeed / sp);
    }
    float inv[4], dq[4];
    QConj(now.q, inv);
    QMul(to.q, inv, dq);
    if (dq[3] < 0) for (auto& c : dq) c = -c;
    const float sn = std::sqrt(dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]);
    const float angle = 2.f * std::atan2(sn, dq[3]);
    float spin[3]{};
    if (sn > 1e-7f) for (int k = 0; k < 3; ++k) spin[k] = dq[k] / sn * angle / dt;
    // the same for the turn: tracking's own turn this step, and the turn the body was already behind by at no more than
    // catch_spin
    float turnRate = 0;   // tracking's own turn this step, rad/s (for the settling)
    if (sentValid[side] && (!world || settle)) {
        float pinv[4], tq[4], gq[4];
        QConj(sent[side], pinv);
        QMul(to.q, pinv, tq);              // tracking's turn this step
        QMul(sent[side], inv, gq);         // where tracking was against where the body is: the turn it is behind by
        const auto vec = [](float* q, float d, float* out) {
            if (q[3] < 0) for (int k = 0; k < 4; ++k) q[k] = -q[k];
            const float n = std::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2]);
            const float a = 2.f * std::atan2(n, q[3]);
            for (int k = 0; k < 3; ++k) out[k] = n > 1e-7f ? q[k] / n * a / d : 0.f;
        };
        float carry[3], behind[3];
        vec(tq, dt, carry);
        vec(gq, dt, behind);
        const float b = std::sqrt(behind[0] * behind[0] + behind[1] * behind[1] + behind[2] * behind[2]);
        // the turn the same way: tracking's place clear, at catch_spin
        const float catchMost = settle && !trackClear[side] ? s.pressCatchSpin : s.catchSpin;
        if (b > catchMost)
            for (int k = 0; k < 3; ++k) spin[k] = carry[k] + behind[k] * (catchMost / b);
        float off = 0;
        for (int k = 0; k < 3; ++k) off += (carry[k] - w[k]) * (carry[k] - w[k]);
        turnRate = std::sqrt(off);
    }
    const bool armed = weaponOn[side] > 0;
    if (settle && !armed) {
        // Bonelab's _angJerkMult: up at 12/s while the turn rate is 3 rad/s off tracking's, else down toward 10% at 8/s
        settleAng[side] = turnRate > 3.f ? std::min(1.f, settleAng[side] + 12.f * dt) : std::max(0.1f, settleAng[side] - 8.f * dt);
    } else {
        settleAng[side] = 1.f;
    }
    if (world && !(settle && !armed)) {
        const float most = armed ? s.weaponSpin : s.pressSpin;
        const float spn = std::sqrt(spin[0] * spin[0] + spin[1] * spin[1] + spin[2] * spin[2]);
        if (spn > most) for (auto& c : spin) c *= most / spn;
    }
    float dw[3];
    for (int k = 0; k < 3; ++k) dw[k] = spin[k] - w[k];
    const float mostW = (settle && !armed ? s.spinAccel * settleAng[side] : world ? (armed ? s.weaponSpinAccel : s.spinContact) : s.spinAccel) * dt;
    const float dwn = std::sqrt(dw[0] * dw[0] + dw[1] * dw[1] + dw[2] * dw[2]);
    const float kw = dwn > mostW ? mostW / dwn : 1.f;
    float wset[3];
    for (int k = 0; k < 3; ++k) wset[k] = w[k] + dw[k] * kw;
    if (!std::isfinite(set[0] + set[1] + set[2] + wset[0] + wset[1] + wset[2])) return;
    float setCom[3], ws[3];
    Cross(wset, com, ws);
    for (int k = 0; k < 3; ++k) setCom[k] = set[k] + ws[k];
    At<void (*)(void*, const float*, bool)>(a, kSetLinearVelocity)(a, setCom, true);
    At<void (*)(void*, const float*, bool)>(a, kSetAngularVelocity)(a, wset, true);
    for (int k = 0; k < 3; ++k) gave[side][k] = set[k], gave[side][3 + k] = wset[k], gaveCom[side][k] = setCom[k];
    gaveValid[side] = true;
}

void Park(int side) {
    if (actor[side] && simulating[side]) {
        At<void (*)(void*, int, bool)>(actor[side], kSetActorFlag)(actor[side], kDisableSimulation, true);
        simulating[side] = false;
    }
    live[side].store(false, std::memory_order_release);
    sentValid[side] = false;
    gaveValid[side] = false;
}

// costs, for the statistics: the physics thread's own work, microseconds
std::atomic<uint64_t> beforeUs{0}, afterUs{0}, beforeMostUs{0}, afterMostUs{0}, stepsSeen{0};
void Charge(std::atomic<uint64_t>& sum, std::atomic<uint64_t>& most, LARGE_INTEGER t0) {
    LARGE_INTEGER t1, f;
    QueryPerformanceCounter(&t1);
    QueryPerformanceFrequency(&f);
    const uint64_t us = uint64_t((t1.QuadPart - t0.QuadPart) * 1000000 / f.QuadPart);
    sum.fetch_add(us, std::memory_order_relaxed);
    uint64_t was = most.load(std::memory_order_relaxed);
    while (us > was && !most.compare_exchange_weak(was, us)) {}
}

// THE SCENE PLAYS THE BODY: the user's cutscene tier (and the vehicle's, mounted), a grapple, a carried body, a finisher, a
// surveillance camera -- whatever hands VRIK's arms to the animation (Hooks/AnimPose.cpp's early-outs) takes the hands'
// bodies out of the world too: an animated arm swung through a table would shove what is on it.
bool SceneOwnsArms() {
    const bool mounted = SharedPose(31) > .5f;
    return cvr::anim::ShouldSuspendVrik(g_sceneTier.load(std::memory_order_relaxed), mounted, VRIK_EffectiveCutsceneSuspendTier(),
                                        g_liveControls.xrVehicleCutsceneSuspendTier) ||
           cvr::anim::ShouldSuspendVrikForTakedown(g_VRTakedownState, CyberpunkVR_VrikTakedownSuspend) ||
           cvr::anim::ShouldSuspendVrikForBodyCarry(g_VRBodyCarryingState, g_VRBodyDisposalState, g_VRLocomotionState,
                                                    CyberpunkVR_VrikBodyCarryHandOff) ||
           cvr::anim::ShouldSuspendVrikForFinisher(g_VRFinisherActive, GetTickCount64() - g_VRFinisherSinceMs, CyberpunkVR_VrikFinisherSuspend) ||
           DeviceCamActive();
}

// The drawn hand, frame to frame (animation thread, for the statistics): how far it leaves the line of its last two
// frames. A hand moving steadily scores nothing; a jump scores its size.
std::atomic<uint64_t> drawnJerkUm[2] = {0, 0}, drawnJerkMostUm[2] = {0, 0}, drawnJerkN[2] = {0, 0}, drawnJumps[2] = {0, 0};
std::atomic<uint64_t> trackJerkUm[2] = {0, 0}, trackJerkMostUm[2] = {0, 0}, trackJerkN[2] = {0, 0};
// The drawn hand against tracking, frame to frame: how much its offset from tracking changes. Tracking is the hand where
// the frame is shown; in the air the offset is nil and stays so, on a wall it grows and shrinks smoothly.
std::atomic<uint64_t> slipUm[2] = {0, 0}, slipMostUm[2] = {0, 0}, slipN[2] = {0, 0}, slipJumps[2] = {0, 0};
std::atomic<uint64_t> airSlipUm[2] = {0, 0}, airSlipN[2] = {0, 0};
// the trace's ring (animation thread writes, the worker empties)
struct TraceRow {
    double t; int side; float drawn[3], body[3], raw[3], vel[3], lead, track[3]; uint8_t touching, onWorld;
    float rot[4], bodyRot[4];   // the drawn hand's turn and its body's, world
    float trackRot[4];          // tracking's turn, world
    uint8_t pressed; float moveW; float bend[5];   // the latch, how much of tracking's move is added, each finger's bends
    float in[5];                                   // each finger as drawn, how deep in what is near (m)
    float drawnIn;                                 // the body's shapes where the hand is drawn: the skin's deepest (m)
};
constexpr int kTraceRing = 8192;
TraceRow traceRing[kTraceRing];
std::atomic<uint32_t> traceHead{0}, traceTail{0};
float lastTrack[2][3] = {};
std::atomic<float> drawnIn[2] = {};   // trace=1: the body's shapes where the hand is drawn, the skin's deepest in what is near


// ---- the worker ----
HANDLE stepped = nullptr;
std::atomic<bool> workerUp{false};

void ReadSettings(Settings& s) {
    const std::string path = cvr::paths::FileA(cvr::paths::Folder::Config, "physhands.ini");
    FILE* f = nullptr;
    const auto write = [&](const Settings& d) {
        FILE* out = nullptr;
        if (fopen_s(&out, path.c_str(), "w") || !out) return;
        std::fprintf(out,
                     "; Physics hands (prototype). Read again while the game runs, within a second of a save; mass and\n"
                     "; depenetration at once, the box and friction when the hands are next made (a load).\n"
                     "version=%d\nenabled=%d\nwith_weapon=%d\ndrawn=%d\nfingers=%d\nfinger_shapes=%d\nforearm=%d\nmass=%.2f\n"
                     "; N: pressing the world, pushing a loose thing; m/s^2: how quick it is in the air\n"
                     "force=%.0f\npush_force=%.0f\nmove_accel=%.0f\nspin_accel=%.0f\nspin_contact=%.0f\npress_speed=%.2f\npress_spin=%.1f\npress_gain=%.2f\n"
                     "max_speed=%.1f\ndepenetration=%.2f\n"
                     "teleport=%.2f\ndeadband=%.3f\nturn_deadband=%.3f\nmax_turn=%.2f\nstick_hold=%.3f\nstick_turn=%.3f\nsmooth_hz=%.1f\nsmooth_beta=%.1f\n"
                     "half_along=%.3f\nhalf_across=%.3f\nhalf_through=%.3f\nalong=%.3f\nfriction=%.2f\nfriction_grip=%.2f\n"
                     "prop_depenetration=%.2f\nstack_depenetration=%.3f\nloose_after_grip=%.2f\nweapon=%d\nweapon_spin=%.1f\nweapon_spin_accel=%.0f\n"
                     "catch_speed=%.2f\ncatch_spin=%.1f\nconform=%d\nstack_drop=%d\nheld_alone=%d\ntrace=0\n"
                     "; the hand pressed into the world pushes the body away (1/s per metre past the dead zone, m, m/s)\n"
                     "body_push=%d\nbody_push_gain=%.1f\nbody_push_dead=%.3f\nbody_push_most=%.2f\n"
                     "; the hand's body is held within V's arm's reach of the shoulder (m spare past upper arm + forearm)\n"
                     "arm_reach=%d\narm_reach_spare=%.3f\n"
                     "; people near the hands are capsules on their bones: the hands stop on them, the fingers lie on them\n"
                     "people=%d\n"
                     "; the hand's shapes fitted to the skin of V's arms (0: the box above)\n"
                     "skin=%d\n"
                     "; the shapes grown past the skin by this much, m: drawn a hair off a surface, not in it\n"
                     "skin_margin=%.4f\n"
                     "; continuous collision: 1 swept, 2 speculative, 0 none; the shapes' contact offset, m (0: PhysX's own)\n"
                     "ccd=%d\ncontact_offset=%.3f\n"
                     "; on the world: tracking's move in full, the gap closed at press_catch m/s / press_catch_spin rad/s,\n"
                     "; Bonelab's settling of the hand's push and turn (settle=0: the old soft press)\n"
                     "settle=%d\npress_catch=%.2f\npress_catch_spin=%.2f\n"
                     "; the fingers' joints as hinges on a surface (the animation's lean and twist dropped)\nfinger_hinge_only=%d\n"
                     "; the grip closed on a top or an edge holds on and the player is pulled after it (m/s)\nclimb=%d\nclimb_most=%.1f\n"
                     "; 0: from the census of the shapes round the player\ngroup=0\nmask=0\nlog_every=%d\n",
                     kSettingsVersion, d.enabled, d.withWeapon, d.drawn, d.fingers, d.fingerShapes, d.forearm, d.mass, d.force, d.pushForce, d.moveAccel, d.spinAccel, d.spinContact,
                     d.pressSpeed, d.pressSpin, d.pressGain, d.maxSpeed, d.depenetration, d.teleport, d.deadband, d.turnDeadband, d.maxTurn, d.stickHold, d.stickTurn, d.smoothHz, d.smoothBeta, d.half[0], d.half[1],
                     d.half[2], d.along, d.friction, d.frictionGrip, d.propDepenetration, d.stackDepenetration, d.looseAfterGrip, d.weapon, d.weaponSpin, d.weaponSpinAccel, d.catchSpeed, d.catchSpin, d.conform,
                     d.stackDrop, d.heldAlone, d.bodyPush, d.bodyPushGain, d.bodyPushDead, d.bodyPushMost, d.armReach, d.armReachSpare, d.people, d.skin, d.skinMargin,
                     d.ccd, d.contactOffset, d.settle, d.pressCatch, d.pressCatchSpin, d.fingerHingeOnly, d.climb, d.climbMost, d.logEvery);
        std::fprintf(out,
                     "; the palm and fingers meet V's own body (self) and the other hand (hands_meet); the grip closed on a\n"
                     "; person holds that part, grab_give m toward the controller, and lets go at grab_release m\n"
                     "self=%d\nhands_meet=%d\ngrab_people=%d\ngrab_release=%.2f\ngrab_give=%.3f\n",
                     d.self, d.handsMeet, d.grabPeople, d.grabRelease, d.grabGive);
        std::fclose(out);
    };
    if (fopen_s(&f, path.c_str(), "r") || !f) {
        write(s);   // written once, so the numbers are there to change
        return;
    }
    Settings r = s;
    int version = 0;
    char line[256];
    while (std::fgets(line, sizeof(line), f)) {
        float v = 0;
        unsigned long long u = 0;
        int i = 0;
        if (std::sscanf(line, "version=%d", &i) == 1) version = i;
        else if (std::sscanf(line, "enabled=%d", &i) == 1) r.enabled = i;
        else if (std::sscanf(line, "with_weapon=%d", &i) == 1) r.withWeapon = i;
        else if (std::sscanf(line, "ride=%d", &i) == 1) r.ride = i;
        else if (std::sscanf(line, "fingers=%d", &i) == 1) r.fingers = i;
        else if (std::sscanf(line, "mass=%f", &v) == 1 && v > 0.05f && v < 50) r.mass = v;
        else if (std::sscanf(line, "force=%f", &v) == 1 && v > 1 && v < 10000) r.force = v;
        else if (std::sscanf(line, "push_force=%f", &v) == 1 && v > 1 && v < 10000) r.pushForce = v;
        else if (std::sscanf(line, "finger_shapes=%d", &i) == 1) r.fingerShapes = i;
        else if (std::sscanf(line, "forearm=%d", &i) == 1) r.forearm = i;
        else if (std::sscanf(line, "drawn=%d", &i) == 1) r.drawn = i;
        else if (std::sscanf(line, "move_accel=%f", &v) == 1 && v > 1) r.moveAccel = v;
        else if (std::sscanf(line, "spin_accel=%f", &v) == 1 && v > 1) r.spinAccel = v;
        else if (std::sscanf(line, "spin_contact=%f", &v) == 1 && v > 1) r.spinContact = v;
        else if (std::sscanf(line, "max_speed=%f", &v) == 1 && v > 0.5f) r.maxSpeed = v;
        else if (std::sscanf(line, "press_speed=%f", &v) == 1 && v > 0.01f) r.pressSpeed = v;
        else if (std::sscanf(line, "press_spin=%f", &v) == 1 && v > 0.01f) r.pressSpin = v;
        else if (std::sscanf(line, "press_gain=%f", &v) == 1 && v > 0.01f && v <= 1) r.pressGain = v;
        else if (std::sscanf(line, "depenetration=%f", &v) == 1 && v > 0.01f) r.depenetration = v;
        else if (std::sscanf(line, "teleport=%f", &v) == 1 && v > 0.05f) r.teleport = v;
        else if (std::sscanf(line, "deadband=%f", &v) == 1 && v >= 0) r.deadband = v;
        else if (std::sscanf(line, "turn_deadband=%f", &v) == 1 && v >= 0) r.turnDeadband = v;
        else if (std::sscanf(line, "max_turn=%f", &v) == 1 && v >= 0) r.maxTurn = v;
        else if (std::sscanf(line, "stick_hold=%f", &v) == 1 && v >= 0) r.stickHold = v;
        else if (std::sscanf(line, "stick_turn=%f", &v) == 1 && v >= 0) r.stickTurn = v;
        else if (std::sscanf(line, "smooth_hz=%f", &v) == 1 && v > 0.1f) r.smoothHz = v;
        else if (std::sscanf(line, "smooth_beta=%f", &v) == 1 && v >= 0) r.smoothBeta = v;
        else if (std::sscanf(line, "half_along=%f", &v) == 1 && v > 0.005f) r.half[0] = v;
        else if (std::sscanf(line, "half_across=%f", &v) == 1 && v > 0.005f) r.half[1] = v;
        else if (std::sscanf(line, "half_through=%f", &v) == 1 && v > 0.003f) r.half[2] = v;
        else if (std::sscanf(line, "along=%f", &v) == 1) r.along = v;
        else if (std::sscanf(line, "friction=%f", &v) == 1 && v >= 0) r.friction = v;
        else if (std::sscanf(line, "friction_grip=%f", &v) == 1 && v >= 0) r.frictionGrip = v;
        else if (std::sscanf(line, "prop_depenetration=%f", &v) == 1 && v >= 0) r.propDepenetration = v;
        else if (std::sscanf(line, "stack_depenetration=%f", &v) == 1 && v >= 0) r.stackDepenetration = v;
        else if (std::sscanf(line, "loose_after_grip=%f", &v) == 1 && v >= 0) r.looseAfterGrip = v;
        else if (std::sscanf(line, "weapon=%d", &i) == 1) r.weapon = i;
        else if (std::sscanf(line, "weapon_spin=%f", &v) == 1 && v > 0.1f) r.weaponSpin = v;
        else if (std::sscanf(line, "weapon_spin_accel=%f", &v) == 1 && v > 1) r.weaponSpinAccel = v;
        else if (std::sscanf(line, "catch_speed=%f", &v) == 1 && v > 0.05f) r.catchSpeed = v;
        else if (std::sscanf(line, "catch_spin=%f", &v) == 1 && v > 0.1f) r.catchSpin = v;
        else if (std::sscanf(line, "trace=%d", &i) == 1) r.trace = i;
        else if (std::sscanf(line, "conform=%d", &i) == 1) r.conform = i;
        else if (std::sscanf(line, "stack_drop=%d", &i) == 1) r.stackDrop = i;
        else if (std::sscanf(line, "held_alone=%d", &i) == 1) r.heldAlone = i;
        else if (std::sscanf(line, "body_push=%d", &i) == 1) r.bodyPush = i;
        else if (std::sscanf(line, "body_push_gain=%f", &v) == 1 && v >= 0 && v < 100) r.bodyPushGain = v;
        else if (std::sscanf(line, "body_push_dead=%f", &v) == 1 && v >= 0 && v < 1) r.bodyPushDead = v;
        else if (std::sscanf(line, "body_push_most=%f", &v) == 1 && v >= 0 && v < 10) r.bodyPushMost = v;
        else if (std::sscanf(line, "arm_reach=%d", &i) == 1) r.armReach = i;
        else if (std::sscanf(line, "people=%d", &i) == 1) r.people = i;
        else if (std::sscanf(line, "skin=%d", &i) == 1) r.skin = i;
        else if (std::sscanf(line, "skin_margin=%f", &v) == 1 && v >= 0 && v <= 0.01f) r.skinMargin = v;
        else if (std::sscanf(line, "ccd=%d", &i) == 1 && i >= 0 && i <= 2) r.ccd = i;
        else if (std::sscanf(line, "settle=%d", &i) == 1) r.settle = i;
        else if (std::sscanf(line, "press_catch=%f", &v) == 1 && v > 0.01f) r.pressCatch = v;
        else if (std::sscanf(line, "press_catch_spin=%f", &v) == 1 && v > 0.05f) r.pressCatchSpin = v;
        else if (std::sscanf(line, "finger_hinge_only=%d", &i) == 1) r.fingerHingeOnly = i;
        else if (std::sscanf(line, "climb=%d", &i) == 1) r.climb = i;
        else if (std::sscanf(line, "climb_most=%f", &v) == 1 && v > 0.1f && v < 10.f) r.climbMost = v;
        else if (std::sscanf(line, "contact_offset=%f", &v) == 1 && v >= 0 && v <= 0.2f) r.contactOffset = v;
        else if (std::sscanf(line, "arm_reach_spare=%f", &v) == 1 && v >= 0 && v < 0.5f) r.armReachSpare = v;
        else if (std::sscanf(line, "self=%d", &i) == 1) r.self = i;
        else if (std::sscanf(line, "hands_meet=%d", &i) == 1) r.handsMeet = i;
        else if (std::sscanf(line, "grab_people=%d", &i) == 1) r.grabPeople = i;
        else if (std::sscanf(line, "grab_release=%f", &v) == 1 && v > 0.05f && v < 2.f) r.grabRelease = v;
        else if (std::sscanf(line, "grab_give=%f", &v) == 1 && v >= 0 && v < 0.5f) r.grabGive = v;
        else if (std::sscanf(line, "group=%llx", &u) == 1) r.groupOverride = u;
        else if (std::sscanf(line, "mask=%llx", &u) == 1) r.maskOverride = u;
        else if (std::sscanf(line, "log_every=%d", &i) == 1 && i > 0) r.logEvery = i;
    }
    std::fclose(f);
    if (version < kSettingsVersion) {
        // an older prototype's numbers (a 1 kg hand at 250 N, a box the length of the fingers): written again with today's
        write(s);
        Log("[physhands] config/physhands.ini was version %d: written again with version %d's numbers\n", version, kSettingsVersion);
        return;
    }
    s = r;
}

// ---- the world round the player, for the fingers (physics thread fills, worker reads) ----
// The game's scene answers no overlap query (Natives/DebugView.cpp found it for the gun), so the shapes near the player
// are found the way the census found them, by walking the scene's static actors a slice a step, and each one near is
// copied: its geometry (a mesh's by its pointer, which lives as long as its sector) and its pose. Statics only: what
// moves would be where it was a cycle ago, and a loose thing's mesh can go with it.
struct NearShape { alignas(8) uint8_t geometry[64]; PxTransform pose; PxBounds bounds; int type; };
constexpr int kMostNear = 1536;
struct NearList { NearShape s[kMostNear]; int n; float center[3]; uint64_t at; };
NearList nearLists[2];
std::atomic<int> nearFront{-1};   // the list the worker reads, -1 none yet
SRWLOCK nearLock = SRWLOCK_INIT;  // held shared by the worker while it reads the front; the swap takes it exclusively
struct NearWalk { int phase = 0; uint32_t next = 0; int fill = 0; uint64_t startedAt = 0; };
NearWalk nearWalk;
std::atomic<uint64_t> nearCycles{0}, nearMostShapes{0};
constexpr float kNearRadius = 2.5f;   // round the player's chest: both hands' reach, and a cycle's walking

// ---- PEOPLE: HandCollision reads a living person's bones (their slot component's slots) and hands over a capsule for
// each part every frame; the physics thread poses a kinematic capsule of its own for each ----
constexpr int kMostPeople = 96;
// ...and V's own body's after them (SelfCapsules), made and posed the same way
constexpr int kMostSelf = 6, kMostBodies = kMostPeople + kMostSelf;
// a shape's owner: V; 0 a person the script did not name; > 0 one part of a person (the script's id: person * 32 + part)
constexpr int kSelfOwner = -1;
struct Person { float a[3], b[3], r; int owner; };
struct People { Person c[kMostPeople]; int n; uint64_t at; };
People peopleStaged;   // script thread
Seq<People> people;
void* personActor[kMostBodies] = {};
void* personShape[kMostBodies] = {};
float personGeom[kMostBodies][2] = {};
uint32_t personFilter[kMostBodies][4] = {};
bool personLive[kMostBodies] = {};
// as posed this step, PhysX's space: each one's owner and the hands that meet it (bits: 1 the left, 2 the right)
struct PeopleNear { NearShape s[kMostBodies]; int owner[kMostBodies]; uint8_t meets[kMostBodies]; int n; uint64_t at; };
NearShape personNear[kMostBodies];   // physics thread
int personOwner[kMostBodies] = {};
uint8_t personMeets[kMostBodies] = {};
int personNearN = 0;
// Whether a hand meets one of them now: a person always, V's own body while that hand meets it.
inline bool PersonMeets(int i, int side) {
    return (personMeets[i] >> side & 1) && (personOwner[i] != kSelfOwner || selfOn[side]);
}
// V's own body as last posed, the game's world, for the script's view
struct SelfCap { float a[3], b[3], r; uint8_t meets; };
struct SelfShown { SelfCap c[kMostSelf]; int n; uint64_t at; };
Seq<SelfShown> selfShown;
Seq<PeopleNear> peopleNear;          // the same, for the worker's fingers
using BoundsFn = PxBounds (*)(const void* geometry, const void* pose, float inflation);
BoundsFn worldBounds = nullptr;

void NearSlice(void* sc, const float* origin) {
    constexpr uint32_t kSlice = 1200;
    NearList& L = nearLists[nearWalk.fill];
    if (nearWalk.phase == 0) {
        if (!worldBounds) {
            HMODULE common = GetModuleHandleA("PhysX3Common_x64.dll");
            worldBounds = common ? reinterpret_cast<BoundsFn>(GetProcAddress(
                                       common, "?getWorldBounds@PxGeometryQuery@physx@@SA?AVPxBounds3@2@AEBVPxGeometry@2@AEBVPxTransform@2@M@Z"))
                                 : nullptr;
            if (!worldBounds) return;
        }
        const int front = nearFront.load(std::memory_order_acquire);
        float center[3];
        for (int k = 0; k < 3; ++k) center[k] = CyberpunkVR_PlayerEntityPos[k] - origin[k];
        center[2] += 1.1f;
        // THE WORLD STANDS STILL: walked again once the player has moved a third of the margin, or every few seconds
        // (a door, a sector come in); standing, the walk costs nothing
        if (front >= 0) {
            const NearList& old = nearLists[front];
            float d2 = 0;
            for (int k = 0; k < 3; ++k) d2 += (center[k] - old.center[k]) * (center[k] - old.center[k]);
            if (d2 < 0.5f * 0.5f && GetTickCount64() - old.at < 3000) return;
        }
        nearWalk.fill = front == 0 ? 1 : 0;
        NearList& F = nearLists[nearWalk.fill];
        F.n = 0;
        std::memcpy(F.center, center, sizeof(center));
        nearWalk.next = 0;
        nearWalk.phase = 1;
        nearWalk.startedAt = GetTickCount64();
        return;
    }
    if (nearWalk.phase == 1) {
        const uint16_t statics = 1;
        void* slice[kSlice];
        const uint32_t got = At<uint32_t (*)(void*, const uint16_t*, void**, uint32_t, uint32_t)>(sc, kSceneGetActors)(sc, &statics, slice, kSlice, nearWalk.next);
        const float r2 = kNearRadius * kNearRadius;
        for (uint32_t i = 0; i < got && L.n < kMostNear; ++i) {
            void* a = slice[i];
            PxBounds b{};
            At<void* (*)(void*, PxBounds*, float)>(a, kGetWorldBounds)(a, &b, 1.0f);
            float d2 = 0;
            for (int k = 0; k < 3; ++k) {
                const float c = L.center[k], e = c < b.lo[k] ? b.lo[k] - c : (c > b.hi[k] ? c - b.hi[k] : 0.f);
                d2 += e * e;
            }
            if (!(d2 <= r2)) continue;
            PxTransform global{};
            At<void (*)(void*, PxTransform*)>(a, kGetGlobalPose)(a, &global);
            // one actor can be a stretch of baked world of thousands of shapes: only those near are kept
            for (uint32_t start = 0; start < 8192 && L.n < kMostNear;) {
                void* shapes[64]{};
                const uint32_t n = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(a, kGetShapes)(a, shapes, 64, start);
                start += n;
                for (uint32_t j = 0; j < n && L.n < kMostNear; ++j) {
                    void* sh = shapes[j];
                    uint8_t flags = 0;
                    At<void* (*)(void*, uint8_t*)>(sh, kShapeGetFlags)(sh, &flags);
                    if ((flags & 4) || !(flags & 1)) continue;   // a trigger, or not a simulation shape: the hand's box does not meet it
                    uint32_t w[4]{};
                    At<void* (*)(void*, uint32_t*)>(sh, kShapeGetSimFilter)(sh, w);
                    const unsigned long long group = w[0] | (unsigned long long)w[1] << 32;
                    const unsigned long long mask = handFilter[2] | (unsigned long long)handFilter[3] << 32;
                    if (group == ~0ull || !(group & mask)) continue;   // what the hand's box does not meet, the fingers do not
                    NearShape& e = L.s[L.n];
                    e.type = At<int (*)(void*)>(sh, kShapeGetGeometryType)(sh);
                    bool ok = false;
                    switch (e.type) {
                    case kGeomBox: ok = At<bool (*)(void*, void*)>(sh, kShapeGetBox)(sh, e.geometry); break;
                    case kGeomSphere: ok = At<bool (*)(void*, void*)>(sh, kShapeGetSphere)(sh, e.geometry); break;
                    case kGeomCapsule: ok = At<bool (*)(void*, void*)>(sh, kShapeGetCapsule)(sh, e.geometry); break;
                    case kGeomConvex: ok = At<bool (*)(void*, void*)>(sh, kShapeGetConvex)(sh, e.geometry); break;
                    case kGeomTriangles: ok = At<bool (*)(void*, void*)>(sh, kShapeGetTriangles)(sh, e.geometry); break;
                    case kGeomHeightField: ok = At<bool (*)(void*, void*)>(sh, kShapeGetHeightField)(sh, e.geometry); break;
                    default: break;   // a plane under the map
                    }
                    if (!ok) continue;
                    PxTransform local{};
                    At<void (*)(void*, PxTransform*)>(sh, kShapeGetLocalPose)(sh, &local);
                    float lp[3];
                    QRot(global.q, local.p, lp);
                    for (int k = 0; k < 3; ++k) e.pose.p[k] = global.p[k] + lp[k];
                    QMul(global.q, local.q, e.pose.q);
                    e.bounds = worldBounds(e.geometry, &e.pose, 1.0f);
                    float s2 = 0;
                    for (int k = 0; k < 3; ++k) {
                        const float c = L.center[k], d = c < e.bounds.lo[k] ? e.bounds.lo[k] - c : (c > e.bounds.hi[k] ? c - e.bounds.hi[k] : 0.f);
                        s2 += d * d;
                    }
                    if (s2 <= r2) ++L.n;
                }
                if (n < 64) break;
            }
        }
        nearWalk.next += got;
        if (got < kSlice) nearWalk.phase = 2;
        return;
    }
    // done: handed to the worker when it is not reading, else tried again next step
    L.at = GetTickCount64();
    if (TryAcquireSRWLockExclusive(&nearLock)) {
        nearFront.store(nearWalk.fill, std::memory_order_release);
        ReleaseSRWLockExclusive(&nearLock);
        nearCycles.fetch_add(1, std::memory_order_relaxed);
        uint64_t most = nearMostShapes.load(std::memory_order_relaxed);
        while (uint64_t(L.n) > most && !nearMostShapes.compare_exchange_weak(most, uint64_t(L.n))) {}
        nearWalk.phase = 0;
    }
}

// ---- the loose things round the player (physics thread) ----
// Dynamic bodies of the game within reach of the hands, kept by address with the userData the game gave each. A released
// actor keeps neither its table (the pool's free list is written over it) nor, once its memory is reused, that userData,
// so both are checked before anything touches it (the rule HeldBody's Alive keeps). For three things: none of them is
// thrown out of an overlap (its depenetration is capped), the palm meets them again only once it is in none of them (as
// Bonelab's PhysHand.CheckFingerEnable gives the fingers their collider back), and what stood on a thing taken off a
// stack comes down.
struct Loose { void* actor; uint64_t user; PxBounds b; bool stacked; };
constexpr int kMostLoose = 384;
Loose loose[2][kMostLoose];
int looseN[2] = {0, 0};
int looseFront = 0;                 // the list in use; the walk fills the other
struct LooseWalk { int phase = 0; uint32_t next = 0; uint64_t at = 0; float center[3] = {}; };
LooseWalk looseWalk;
uint64_t looseCappedAt = 0;         // the list whose things had their depenetration capped (its walk's time)
std::atomic<uint64_t> looseListed{0}, looseCapped{0}, palmRefused{0}, looseLetThrough{0};
constexpr int kSceneGetGravity = 67;   // as Natives/DebugView.cpp and HeldBody call it

bool LooseAlive(void* a, uint64_t user) {
    return a && *reinterpret_cast<const uintptr_t*>(a) == dll + kDynamicTable &&
           *reinterpret_cast<const uint64_t*>(static_cast<const char*>(a) + 0x10) == user;
}

// LOOSE THINGS FOR THE FINGERS: the shapes of the loose things by either hand, posed as the step left them (physics
// thread, after the fetch), for the worker's finger solve -- a finger that brushes a bottle or a crate bends on it as on
// the world. Only shapes with no mesh behind them: a convex hull goes as its own box (its local bounds), since the worker
// reads the list a moment later and a thing's mesh can be gone by then.
constexpr int kMostLooseNear = 48;
struct LooseShapesNear { NearShape s[kMostLooseNear]; int n; uint64_t at; };
Seq<LooseShapesNear> looseShapesNear;
constexpr int kConvexLocalBounds = 13;   // Gu::ConvexMesh::getLocalBounds (PhysX3Common), as HeldBody reads it

bool LooseShapeNear(void* shape, const PxTransform& g, NearShape& t) {
    __try {
        PxTransform local{};
        At<void (*)(void*, PxTransform*)>(shape, kShapeGetLocalPose)(shape, &local);
        int type = At<int (*)(void*)>(shape, kShapeGetGeometryType)(shape);
        std::memset(t.geometry, 0, sizeof(t.geometry));
        bool ok = false;
        if (type == kGeomBox) ok = At<bool (*)(void*, void*)>(shape, kShapeGetBox)(shape, t.geometry);
        else if (type == kGeomSphere) ok = At<bool (*)(void*, void*)>(shape, kShapeGetSphere)(shape, t.geometry);
        else if (type == kGeomCapsule) ok = At<bool (*)(void*, void*)>(shape, kShapeGetCapsule)(shape, t.geometry);
        else if (type == kGeomConvex) {
            alignas(8) uint8_t cg[64]{};
            if (At<bool (*)(void*, void*)>(shape, kShapeGetConvex)(shape, cg)) {
                void* mesh = *reinterpret_cast<void**>(cg + 32);
                const float* scale = reinterpret_cast<const float*>(cg + 4);
                if (mesh) {
                    float b[6]{};
                    reinterpret_cast<void* (*)(void*, float*)>((*reinterpret_cast<void***>(mesh))[kConvexLocalBounds])(mesh, b);
                    BoxGeometry box{kGeomBox, {0, 0, 0}};
                    float centre[3], moved[3];
                    for (int k = 0; k < 3; ++k) {
                        box.half[k] = 0.5f * (b[3 + k] - b[k]) * std::fabs(scale[k]);
                        centre[k] = 0.5f * (b[3 + k] + b[k]) * scale[k];
                    }
                    QRot(local.q, centre, moved);
                    for (int k = 0; k < 3; ++k) local.p[k] += moved[k];
                    ok = box.half[0] > 0 && box.half[1] > 0 && box.half[2] > 0;
                    std::memcpy(t.geometry, &box, sizeof(box));
                    type = kGeomBox;
                }
            }
        }
        if (!ok || !worldBounds) return false;
        t.type = type;
        float lp[3];
        QRot(g.q, local.p, lp);
        for (int k = 0; k < 3; ++k) t.pose.p[k] = g.p[k] + lp[k];
        QMul(g.q, local.q, t.pose.q);
        t.bounds = worldBounds(t.geometry, &t.pose, 1.0f);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// within 25 cm of a hand's body (as the step left it); a hand let through loose things (looseFrom) has none
void PublishLooseNear(const Step& st) {
    static LooseShapesNear snap;
    snap.n = 0;
    const uint64_t tick = GetTickCount64();
    const int f = looseFront;
    for (int i = 0; i < looseN[f] && snap.n < kMostLooseNear; ++i) {
        const Loose& e = loose[f][i];
        if (!LooseAlive(e.actor, e.user)) continue;
        PxBounds b{};
        At<void* (*)(void*, PxBounds*, float)>(e.actor, kGetWorldBounds)(e.actor, &b, 1.0f);
        bool close = false;
        for (int side = 0; side < 2 && !close; ++side) {
            if (!st.valid[side] || tick < looseFrom[side]) continue;
            float d2 = 0;
            for (int k = 0; k < 3; ++k) {
                const float c = st.pose[side][4 + k], out = c < b.lo[k] ? b.lo[k] - c : (c > b.hi[k] ? c - b.hi[k] : 0.f);
                d2 += out * out;
            }
            close = d2 < 0.25f * 0.25f;
        }
        if (!close) continue;
        PxTransform g{};
        At<void (*)(void*, PxTransform*)>(e.actor, kGetGlobalPose)(e.actor, &g);
        void* list[4]{};
        const uint32_t ns = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(e.actor, kGetShapes)(e.actor, list, 4, 0);
        for (uint32_t j = 0; j < ns && j < 4 && snap.n < kMostLooseNear; ++j)
            if (LooseShapeNear(list[j], g, snap.s[snap.n])) ++snap.n;
    }
    snap.at = tick;
    looseShapesNear.Write(snap);
}

// One dynamic actor, kept when it is near, not a hand's, not kinematic, not heavy, and a simulation shape of it is one the
// palm meets.
void LooseSample(void* a, const float* center, Loose* out, int& n) {
    if (n >= kMostLoose || !a || a == actor[0] || a == actor[1]) return;
    if (*reinterpret_cast<const uintptr_t*>(a) != dll + kDynamicTable) return;   // an articulation's link
    PxBounds b{};
    At<void* (*)(void*, PxBounds*, float)>(a, kGetWorldBounds)(a, &b, 1.0f);
    float d2 = 0;
    for (int k = 0; k < 3; ++k) {
        const float c = center[k], e = c < b.lo[k] ? b.lo[k] - c : (c > b.hi[k] ? c - b.hi[k] : 0.f);
        d2 += e * e;
    }
    if (!(d2 <= 2.0f * 2.0f)) return;
    for (int i = 0; i < n; ++i)
        if (out[i].actor == a) return;   // listed already (an aggregate's member is the scene's too)
    uint8_t flags = 0;
    At<void* (*)(void*, uint8_t*)>(a, kGetRigidBodyFlags)(a, &flags);
    if ((flags & 1) || At<float (*)(void*)>(a, kGetMass)(a) > kHeavy) return;
    void* list[4]{};
    const uint32_t ns = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(a, kGetShapes)(a, list, 4, 0);
    const unsigned long long mask = handFilter[2] | (unsigned long long)handFilter[3] << 32;
    bool met = false;
    for (uint32_t i = 0; i < ns && i < 4 && !met; ++i) {
        uint8_t sf = 0;
        At<void* (*)(void*, uint8_t*)>(list[i], kShapeGetFlags)(list[i], &sf);
        if (!(sf & 1)) continue;
        uint32_t w[4]{};
        At<void* (*)(void*, uint32_t*)>(list[i], kShapeGetSimFilter)(list[i], w);
        met = ((w[0] | (unsigned long long)w[1] << 32) & mask) != 0;
    }
    if (!met) return;
    out[n].actor = a;
    out[n].user = *reinterpret_cast<const uint64_t*>(static_cast<const char*>(a) + 0x10);
    out[n].b = b;
    out[n].stacked = false;
    ++n;
}

// STACKED: a thing whose bounds are most of the way inside a neighbour's (the stall's plates nest: a 9 cm bowl every 3 cm).
// Their shapes are in each other; asleep they stand, woken PhysX throws them apart and they break on the floor.
void MarkStacked(Loose* l, int n) {
    for (int i = 0; i < n; ++i)
        for (int j = i + 1; j < n; ++j) {
            const PxBounds &a = l[i].b, &b = l[j].b;
            float o[3], s[3];
            for (int k = 0; k < 3; ++k) {
                o[k] = std::min(a.hi[k], b.hi[k]) - std::max(a.lo[k], b.lo[k]);
                s[k] = std::min(a.hi[k] - a.lo[k], b.hi[k] - b.lo[k]);
            }
            if (o[0] > 0.5f * s[0] && o[1] > 0.5f * s[1] && o[2] > 0.3f * s[2]) l[i].stacked = l[j].stacked = true;
        }
}

// A slice a step, as the census: the scene's dynamic actors, then the aggregates' members. Walked again after the player
// has moved 0.4 m or every 1.5 s.
void LooseSlice(void* sc, const float* origin) {
    constexpr uint32_t kSlice = 400;
    const int fill = 1 - looseFront;
    if (looseWalk.phase == 0) {
        float center[3];
        for (int k = 0; k < 3; ++k) center[k] = CyberpunkVR_PlayerEntityPos[k] - origin[k];
        center[2] += 1.1f;
        float d2 = 0;
        for (int k = 0; k < 3; ++k) d2 += (center[k] - looseWalk.center[k]) * (center[k] - looseWalk.center[k]);
        if (looseWalk.at && d2 < 0.4f * 0.4f && GetTickCount64() - looseWalk.at < 1500) return;
        std::memcpy(looseWalk.center, center, sizeof(center));
        looseN[fill] = 0;
        looseWalk.next = 0;
        looseWalk.phase = 1;
        return;
    }
    if (looseWalk.phase == 1) {
        const uint16_t dynamics = 2;
        void* slice[kSlice];
        const uint32_t got = At<uint32_t (*)(void*, const uint16_t*, void**, uint32_t, uint32_t)>(sc, kSceneGetActors)(sc, &dynamics, slice, kSlice, looseWalk.next);
        for (uint32_t i = 0; i < got; ++i) LooseSample(slice[i], looseWalk.center, loose[fill], looseN[fill]);
        looseWalk.next += got;
        if (got < kSlice) looseWalk.next = 0, looseWalk.phase = 2;
        return;
    }
    void* aggregates[kSlice / 8];
    const uint32_t got = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(sc, kSceneGetAggregates)(sc, aggregates, kSlice / 8, looseWalk.next);
    for (uint32_t i = 0; i < got; ++i) {
        void* members[64]{};
        const uint32_t n = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(aggregates[i], kAggregateGetActors)(aggregates[i], members, 64, 0);
        for (uint32_t m = 0; m < n && m < 64; ++m)
            if (At<int (*)(void*)>(members[m], kGetType)(members[m]) == 1) LooseSample(members[m], looseWalk.center, loose[fill], looseN[fill]);
    }
    looseWalk.next += got;
    if (got < kSlice / 8) {
        MarkStacked(loose[fill], looseN[fill]);
        looseFront = fill;
        looseWalk.at = GetTickCount64();
        looseWalk.phase = 0;
        looseListed.store(uint64_t(looseN[fill]), std::memory_order_relaxed);
    }
}

// Each new list's things get the capped depenetration (once: the setting stays on the actor).
// A STACKED ONE KEEPS ITS OVERLAPS: the depenetration cap only limits how fast PhysX undoes a penetration already there;
// stopping things from going further into each other is the contact's job and stays. So woken -- by a hand, by a take
// next to it -- a plate standing in the one under it stays standing there instead of being thrown off.
std::atomic<uint64_t> looseStacked{0};
void LooseCap(const Settings& s) {
    if (s.propDepenetration <= 0 || looseCappedAt == looseWalk.at) return;
    looseCappedAt = looseWalk.at;
    const int f = looseFront;
    uint64_t n = 0, stacked = 0;
    for (int i = 0; i < looseN[f]; ++i) {
        const Loose& e = loose[f][i];
        if (!LooseAlive(e.actor, e.user)) continue;
        At<void (*)(void*, float)>(e.actor, kSetMaxDepenetration)(e.actor, e.stacked ? s.stackDepenetration : s.propDepenetration);
        ++n;
        stacked += e.stacked;
    }
    looseCapped.store(n, std::memory_order_relaxed);
    looseStacked.store(stacked, std::memory_order_relaxed);
}

bool Penetration(float* dir, float* depth, const void* g0, const PxTransform* p0, const struct NearShape& s);

// The palm in a loose thing: its box against each near one's shapes.
bool PalmInLoose(int side, const PxTransform& at) {
    if (!penetration) return false;
    const BoxGeometry box{kGeomBox, {madeHalf[side][0], madeHalf[side][1], madeHalf[side][2]}};
    const float off[3] = {madeCentre[side][0], madeCentre[side][1], madeCentre[side][2]};
    PxTransform pose{};
    float r[3];
    QRot(at.q, off, r);
    for (int k = 0; k < 3; ++k) pose.p[k] = at.p[k] + r[k];
    std::memcpy(pose.q, at.q, sizeof(pose.q));
    const float reach = std::sqrt(box.half[0] * box.half[0] + box.half[1] * box.half[1] + box.half[2] * box.half[2]);
    const int f = looseFront;
    for (int i = 0; i < looseN[f]; ++i) {
        const Loose& e = loose[f][i];
        if (!LooseAlive(e.actor, e.user)) continue;
        PxBounds b{};
        At<void* (*)(void*, PxBounds*, float)>(e.actor, kGetWorldBounds)(e.actor, &b, 1.0f);
        bool apart = false;
        for (int k = 0; k < 3 && !apart; ++k) apart = pose.p[k] + reach < b.lo[k] || pose.p[k] - reach > b.hi[k];
        if (apart) continue;
        PxTransform g{};
        At<void (*)(void*, PxTransform*)>(e.actor, kGetGlobalPose)(e.actor, &g);
        void* list[4]{};
        const uint32_t ns = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(e.actor, kGetShapes)(e.actor, list, 4, 0);
        for (uint32_t j = 0; j < ns && j < 4; ++j) {
            NearShape t{};
            t.type = At<int (*)(void*)>(list[j], kShapeGetGeometryType)(list[j]);
            bool ok = false;
            switch (t.type) {
            case kGeomBox: ok = At<bool (*)(void*, void*)>(list[j], kShapeGetBox)(list[j], t.geometry); break;
            case kGeomSphere: ok = At<bool (*)(void*, void*)>(list[j], kShapeGetSphere)(list[j], t.geometry); break;
            case kGeomCapsule: ok = At<bool (*)(void*, void*)>(list[j], kShapeGetCapsule)(list[j], t.geometry); break;
            case kGeomConvex: ok = At<bool (*)(void*, void*)>(list[j], kShapeGetConvex)(list[j], t.geometry); break;
            default: break;
            }
            if (!ok) continue;
            PxTransform local{};
            At<void (*)(void*, PxTransform*)>(list[j], kShapeGetLocalPose)(list[j], &local);
            float lp[3];
            QRot(g.q, local.p, lp);
            for (int k = 0; k < 3; ++k) t.pose.p[k] = g.p[k] + lp[k];
            QMul(g.q, local.q, t.pose.q);
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, &box, &pose, t) && depth > 0.002f) return true;
        }
    }
    return false;
}

// ---- what stood on a thing taken off a stack (physics thread) ----
// The game's loose things never meet each other (HeldBody.cpp, their filter): a stack stands because its things sleep.
// Taken out of the middle, the one above hung in the air, or, woken, fell through the rest to the table. So what stood on
// it comes down together by as much as the taken one held it up, and sleeps again there, as if that one had never been.
struct Drop {
    void* actor[16]; uint64_t user[16]; float rest[16]; int n; float distance, fallen, speed; uint64_t watchUntil; int said;
    void* shape[16]; uint32_t saved[16][4]; bool apart[16];   // each thing's (first) shape, and its own filter while apart
};
Drop drops[4];
// WHAT CAME DOWN SLEEPS APART: the hands' group and the world's mask, so nothing loose reaches it. Given its own filter
// back asleep, PhysX woke the plates standing in each other; woken and let fall by gravity, they slid off the counter
// (the contacts of shapes deep in each other push sideways). It is given back when something else wakes it, or when it
// is taken (before HeldBody reads its filter).
struct Apart { void* actor; uint64_t user; void* shape; uint32_t saved[4]; PxTransform rest; int kept; };
constexpr int kMostApart = 64;
Apart aparts[kMostApart];
int apartN = 0;
void Refilter(void* actorToFilter);
void Rejoin(int i) {
    if (LooseAlive(aparts[i].actor, aparts[i].user)) {
        At<void (*)(void*, const uint32_t*)>(aparts[i].shape, kShapeSetSimFilter)(aparts[i].shape, aparts[i].saved);
        Refilter(aparts[i].actor);
    }
    aparts[i] = aparts[--apartN];
}
void RejoinActor(void* a) {
    for (int i = apartN - 1; i >= 0; --i)
        if (aparts[i].actor == a) Rejoin(i);
}
// KEPT WHERE IT WAS PUT: something wakes what came down a few steps after it is put to sleep (the game, its aggregate),
// and apart from everything loose it would fall through a counter whose group no mask reaches. Woken, it is brought back
// to its place and put to sleep again.
std::atomic<uint64_t> apartKept{0};
void ApartStep(void* sc, float dt) {
    float g[3] = {0, 0, -9.81f};
    bool gravity = false;
    const float zero[3] = {0, 0, 0};
    for (int i = apartN - 1; i >= 0; --i) {
        Apart& ap = aparts[i];
        if (!LooseAlive(ap.actor, ap.user)) { aparts[i] = aparts[--apartN]; continue; }
        if (At<bool (*)(void*)>(ap.actor, kIsSleeping)(ap.actor)) continue;
        if (!gravity) {
            float r[3]{};
            At<void* (*)(void*, float*)>(sc, kSceneGetGravity)(sc, r);
            if (std::isfinite(r[0] + r[1] + r[2]) && std::fabs(r[2]) > 1.f && std::fabs(r[2]) < 30.f) std::memcpy(g, r, sizeof(g));
            gravity = true;
        }
        PxTransform at{};
        At<void (*)(void*, PxTransform*)>(ap.actor, kGetGlobalPose)(ap.actor, &at);
        float v[3], d2 = 0;
        for (int k = 0; k < 3; ++k) {
            const float d = ap.rest.p[k] - at.p[k];
            d2 += d * d;
            v[k] = d / dt - g[k] * dt;
        }
        if (d2 < 0.001f * 0.001f) {
            At<void (*)(void*, const float*, bool)>(ap.actor, kSetLinearVelocity)(ap.actor, zero, false);
            At<void (*)(void*, const float*, bool)>(ap.actor, kSetAngularVelocity)(ap.actor, zero, false);
            At<void (*)(void*)>(ap.actor, kPutToSleep)(ap.actor);
            continue;
        }
        At<void (*)(void*, const float*, bool)>(ap.actor, kSetLinearVelocity)(ap.actor, v, true);
        At<void (*)(void*, const float*, bool)>(ap.actor, kSetAngularVelocity)(ap.actor, zero, true);
        if (++ap.kept == 1) apartKept.fetch_add(1, std::memory_order_relaxed);
    }
}
std::atomic<uint64_t> dropsMade{0};

// A take, said once each: the taken actor's flags and sleep, for knowing what the game's loose things are.
void LogTaken(void* a) {
    static int said = 0;
    if (said++ >= 16) return;
    uint8_t flags = 0;
    At<void* (*)(void*, uint8_t*)>(a, kGetRigidBodyFlags)(a, &flags);
    PxBounds b{};
    At<void* (*)(void*, PxBounds*, float)>(a, kGetWorldBounds)(a, &b, 1.0f);
    Log("[physhands] taken: actor %p, userData %016llx, %.2f kg, flags %02x, asleep %d, %.3f x %.3f x %.3f m\n", a,
        (unsigned long long)*reinterpret_cast<const uint64_t*>(static_cast<const char*>(a) + 0x10), At<float (*)(void*)>(a, kGetMass)(a),
        flags, int(At<bool (*)(void*)>(a, kIsSleeping)(a)), b.hi[0] - b.lo[0], b.hi[1] - b.lo[1], b.hi[2] - b.lo[2]);
}

void TakenFromStack(void* taken) {
    RejoinActor(taken);
    Settings s{};
    settings.Read(s);
    if (!s.stackDrop || proved <= 0 || !taken || *reinterpret_cast<const uintptr_t*>(taken) != dll + kDynamicTable) return;
    LogTaken(taken);
    PxBounds t{};
    At<void* (*)(void*, PxBounds*, float)>(taken, kGetWorldBounds)(taken, &t, 1.0f);
    const float th = t.hi[2] - t.lo[2], tArea = (t.hi[0] - t.lo[0]) * (t.hi[1] - t.lo[1]);
    struct Up { void* a; uint64_t u; PxBounds b; };
    Up up[32];
    int n = 0;
    const int f = looseFront;
    for (int i = 0; i < looseN[f] && n < 32; ++i) {
        const Loose& e = loose[f][i];
        if (e.actor == taken || !LooseAlive(e.actor, e.user)) continue;
        PxBounds b{};
        At<void* (*)(void*, PxBounds*, float)>(e.actor, kGetWorldBounds)(e.actor, &b, 1.0f);
        const float ox = std::min(b.hi[0], t.hi[0]) - std::max(b.lo[0], t.lo[0]);
        const float oy = std::min(b.hi[1], t.hi[1]) - std::max(b.lo[1], t.lo[1]);
        if (ox <= 0 || oy <= 0) continue;
        const float area = (b.hi[0] - b.lo[0]) * (b.hi[1] - b.lo[1]);
        if (ox * oy < 0.3f * std::min(area, tArea)) continue;          // beside it
        if (b.lo[2] < t.lo[2] + 0.2f * th || b.lo[2] > t.hi[2] + 0.3f) continue;   // under it or well above
        up[n++] = Up{e.actor, e.user, b};
    }
    std::sort(up, up + n, [](const Up& a, const Up& b) { return a.b.lo[2] < b.b.lo[2]; });
    // the ones standing on it, each on the one before: no gap of more than 2 cm up the stack
    float top = t.hi[2];
    int kept = 0;
    for (int i = 0; i < n; ++i) {
        if (up[i].b.lo[2] > top + 0.02f) break;
        top = std::max(top, up[i].b.hi[2]);
        up[kept++] = up[i];
    }
    if (!kept) return;
    // LET DOWN TOGETHER, STRAIGHT, by as much as the taken one held them up, apart from everything loose, and put to sleep
    // there: things that do not meet each other (cans: group 1000, mask ec) would fall through the rest to the table, and
    // things that do (the stall's plates: 1c008, 13cf) stand in each other and were thrown off the counter by gravity
    const float distance = up[0].b.lo[2] - t.lo[2];
    if (!(distance > 0.002f && distance < 0.3f)) return;
    Drop* d = nullptr;
    for (auto& x : drops)
        if (!x.n && !x.watchUntil) { d = &x; break; }
    if (!d) return;
    d->n = std::min(kept, 16);
    for (int i = 0; i < d->n; ++i) {
        d->actor[i] = up[i].a, d->user[i] = up[i].u;
        RejoinActor(up[i].a);
        void* sa[1]{};
        uint32_t w[4]{};
        d->apart[i] = false;
        if (!At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(up[i].a, kGetShapes)(up[i].a, sa, 1, 0)) continue;
        At<void* (*)(void*, uint32_t*)>(sa[0], kShapeGetSimFilter)(sa[0], w);
        d->shape[i] = sa[0];
        std::memcpy(d->saved[i], w, sizeof(w));
        // and no callback bits: nothing of this is the game's to answer
        const uint32_t iso[4] = {handFilter[0], handFilter[1], handFilterStatic[2], handFilterStatic[3]};
        At<void (*)(void*, const uint32_t*)>(sa[0], kShapeSetSimFilter)(sa[0], iso);
        Refilter(up[i].a);
        d->apart[i] = true;
    }
    d->distance = distance;
    d->fallen = 0;
    d->speed = 0;
    d->watchUntil = 0;
    d->said = 0;
    dropsMade.fetch_add(1, std::memory_order_relaxed);
    Log("[physhands] taken off a stack: %d things stood on it (the lowest asleep %d), coming down %.3f m together\n", d->n,
        int(At<bool (*)(void*)>(up[0].a, kIsSleeping)(up[0].a)), distance);
}

void DropStep(void* sc, float dt) {
    bool any = false;
    for (const auto& x : drops) any |= x.n > 0 || x.watchUntil;
    if (!any) return;
    float g[3] = {0, 0, -9.81f};
    {
        float r[3]{};
        At<void* (*)(void*, float*)>(sc, kSceneGetGravity)(sc, r);
        if (std::isfinite(r[0] + r[1] + r[2]) && std::fabs(r[2]) > 1.f && std::fabs(r[2]) < 30.f) std::memcpy(g, r, sizeof(g));
    }
    const float zero[3] = {0, 0, 0};
    for (auto& d : drops) {
        if (d.watchUntil) {
            // WATCHED AFTER IT IS DOWN: whether it stays asleep where it was put, and what it does if not
            const uint64_t now = GetTickCount64();
            for (int i = 0; i < 16 && d.rest[i] != 0 && d.said < 12; ++i) {
                if (!LooseAlive(d.actor[i], d.user[i])) continue;
                PxTransform g{};
                At<void (*)(void*, PxTransform*)>(d.actor[i], kGetGlobalPose)(d.actor[i], &g);
                const bool asleep = At<bool (*)(void*)>(d.actor[i], kIsSleeping)(d.actor[i]);
                if (!asleep || std::fabs(g.p[2] - d.rest[i]) > 0.005f) {
                    ++d.said;
                    Log("[physhands] let down: thing %d %s, %.3f m from where it was put (%llu ms after)\n", i, asleep ? "asleep" : "AWAKE",
                        g.p[2] - d.rest[i], (unsigned long long)(now + 3000 - d.watchUntil));
                }
            }
            if (now > d.watchUntil) d.watchUntil = 0;
            continue;
        }
        if (!d.n) continue;
        if (d.fallen >= d.distance - 1e-4f) {
            // there: still, and asleep as it was
            for (int i = 0; i < d.n; ++i) {
                if (!LooseAlive(d.actor[i], d.user[i])) continue;
                At<void (*)(void*, const float*, bool)>(d.actor[i], kSetLinearVelocity)(d.actor[i], zero, false);
                At<void (*)(void*, const float*, bool)>(d.actor[i], kSetAngularVelocity)(d.actor[i], zero, false);
                At<void (*)(void*)>(d.actor[i], kPutToSleep)(d.actor[i]);
                if (d.apart[i]) {
                    if (apartN >= kMostApart) Rejoin(0);
                    Apart ap{d.actor[i], d.user[i], d.shape[i], {d.saved[i][0], d.saved[i][1], d.saved[i][2], d.saved[i][3]}, {}, 0};
                    At<void (*)(void*, PxTransform*)>(d.actor[i], kGetGlobalPose)(d.actor[i], &ap.rest);
                    aparts[apartN++] = ap;
                    d.apart[i] = false;
                }
                PxTransform g{};
                At<void (*)(void*, PxTransform*)>(d.actor[i], kGetGlobalPose)(d.actor[i], &g);
                d.rest[i] = g.p[2];
            }
            for (int i = d.n; i < 16; ++i) d.rest[i] = 0;
            d.n = 0;
            d.watchUntil = GetTickCount64() + 3000;
            continue;
        }
        if (LooseAlive(d.actor[0], d.user[0])) {
            PxTransform at{};
            float lv[3]{};
            At<void (*)(void*, PxTransform*)>(d.actor[0], kGetGlobalPose)(d.actor[0], &at);
            At<void (*)(void*, float*)>(d.actor[0], kGetLinearVelocity)(d.actor[0], lv);
            Log("[physhands] coming down: fallen %.3f of %.3f m; the first thing at z %.3f moving %.3f m/s (gravity %.2f, step %.4f s)\n",
                d.fallen, d.distance, at.p[2], lv[2], g[2], dt);
        }
        d.speed = std::min(d.speed - g[2] * dt, 2.5f);
        float step = d.speed * dt;
        if (d.fallen + step > d.distance) step = d.distance - d.fallen;
        // straight down by `step` over the step: gravity adds its share within it
        const float v[3] = {-g[0] * dt, -g[1] * dt, -step / dt - g[2] * dt};
        for (int i = 0; i < d.n; ++i) {
            if (!LooseAlive(d.actor[i], d.user[i])) continue;
            At<void (*)(void*, const float*, bool)>(d.actor[i], kSetLinearVelocity)(d.actor[i], v, true);
            At<void (*)(void*, const float*, bool)>(d.actor[i], kSetAngularVelocity)(d.actor[i], zero, true);
        }
        d.fallen += step;
    }
}

// ---- a test stack (physics thread, asked for by VRPhysHandsTestStack) ----
// The few light loose things nearest a point, the smallest first, brought one onto another over the first by velocities
// (a body put somewhere by setGlobalPose is put back by the game: its own idea of where the thing is wins) and put to sleep
// there: a stack to take the middle of. A test tool: nothing calls it but a test script. Ask with dump to log the loose
// things near instead.
struct StackAsk { float at[3]; uint64_t at_ms; uint32_t serial; bool dump; bool shapes; };
Seq<StackAsk> stackAsks;
uint32_t stackDone = 0;
void* stackMade[4] = {};
uint64_t stackUser[4] = {};
float stackGoal[4][3] = {};
int stackN = 0, stackSteps = 0;

// THE SHAPES ROUND A POINT (a test tool: VRPhysHandsTestStack with w <= -2): every rigid actor's shapes within 1.5 m of
// it, the scene's and its aggregates', with what kind of actor holds them, their geometry, place and filter
void DumpShapes(void* sc, const float* p, const float* origin) {
    int said = 0;
    const auto one = [&](void* a) {
        int type = At<int (*)(void*)>(a, kGetType)(a);
        if (type != 0 && type != 1 && type != 4) return;
        PxBounds b{};
        At<void* (*)(void*, PxBounds*, float)>(a, kGetWorldBounds)(a, &b, 1.0f);
        float d2 = 0;
        for (int k = 0; k < 3; ++k) {
            const float e = p[k] < b.lo[k] ? b.lo[k] - p[k] : (p[k] > b.hi[k] ? p[k] - b.hi[k] : 0.f);
            d2 += e * e;
        }
        if (!(d2 <= 1.5f * 1.5f) || said > 160) return;
        if (a == actor[0] || a == actor[1]) return;
        bool kinematic = false;
        float mass = 0;
        if (type != 0) {
            uint8_t flags = 0;
            At<void* (*)(void*, uint8_t*)>(a, kGetRigidBodyFlags)(a, &flags);
            kinematic = (flags & 1) != 0;
            mass = At<float (*)(void*)>(a, kGetMass)(a);
        }
        const unsigned long long user = *reinterpret_cast<const unsigned long long*>(static_cast<const char*>(a) + 0x10);
        PxTransform global{};
        At<void (*)(void*, PxTransform*)>(a, kGetGlobalPose)(a, &global);
        void* list[16]{};
        const uint32_t n = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(a, kGetShapes)(a, list, 16, 0);
        for (uint32_t i = 0; i < n && i < 16 && said <= 160; ++i) {
            void* sh = list[i];
            uint8_t sflags = 0;
            At<void* (*)(void*, uint8_t*)>(sh, kShapeGetFlags)(sh, &sflags);
            uint32_t w[4]{};
            At<void* (*)(void*, uint32_t*)>(sh, kShapeGetSimFilter)(sh, w);
            const int geom = At<int (*)(void*)>(sh, kShapeGetGeometryType)(sh);
            float g[4] = {0, 0, 0, 0};
            struct { int type; float v[3]; } gb{};
            if (geom == kGeomBox && At<bool (*)(void*, void*)>(sh, kShapeGetBox)(sh, &gb)) std::memcpy(g, gb.v, sizeof(gb.v));
            if (geom == kGeomSphere && At<bool (*)(void*, void*)>(sh, kShapeGetSphere)(sh, &gb)) g[0] = gb.v[0];
            if (geom == kGeomCapsule && At<bool (*)(void*, void*)>(sh, kShapeGetCapsule)(sh, &gb)) g[0] = gb.v[0], g[1] = gb.v[1];
            PxTransform local{};
            At<void (*)(void*, PxTransform*)>(sh, kShapeGetLocalPose)(sh, &local);
            float lp[3], wp[3];
            QRot(global.q, local.p, lp);
            for (int k = 0; k < 3; ++k) wp[k] = global.p[k] + lp[k] + origin[k];
            ++said;
            Log("[physhands] shape: actor %p %s%s %.1f kg user %016llx; geometry %d (%.3f %.3f %.3f) at (%.3f %.3f %.3f), %.2f m off; "
                "flags %02x group %08x%08x mask %08x%08x\n", a, type == 0 ? "static" : (kinematic ? "kinematic" : "dynamic"),
                type == 4 ? " (articulation link)" : "", mass, user, geom, g[0], g[1], g[2], wp[0], wp[1], wp[2],
                std::sqrt((wp[0] - origin[0] - p[0]) * (wp[0] - origin[0] - p[0]) + (wp[1] - origin[1] - p[1]) * (wp[1] - origin[1] - p[1]) +
                          (wp[2] - origin[2] - p[2]) * (wp[2] - origin[2] - p[2])),
                sflags, w[1], w[0], w[3], w[2]);
        }
    };
    const uint16_t types = 1 | 2;
    const uint32_t count = At<uint32_t (*)(void*, const uint16_t*, void**, uint32_t, uint32_t)>(sc, kSceneGetActors)(sc, &types, listed, kMostListed, 0);
    for (uint32_t i = 0; i < count; ++i) one(listed[i]);
    const uint32_t ag = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(sc, kSceneGetAggregates)(sc, listed, kMostListed, 0);
    for (uint32_t i = 0; i < ag; ++i) {
        void* members[64]{};
        const uint32_t m = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(listed[i], kAggregateGetActors)(listed[i], members, 64, 0);
        for (uint32_t j = 0; j < m && j < 64; ++j) one(members[j]);
    }
    Log("[physhands] shapes round (%.2f %.2f %.2f): %d listed (of %u actors, %u aggregates)\n", p[0] + origin[0], p[1] + origin[1],
        p[2] + origin[2], said, count, ag);
}

void TestStackStep(void* sc, const float* origin, float dt) {
    const float zero[3] = {0, 0, 0};
    if (stackN && stackSteps) {
        // brought there: each by the velocity that takes it the rest of the way within a few steps, gravity's share taken off
        float g[3] = {0, 0, -9.81f};
        At<void* (*)(void*, float*)>(sc, kSceneGetGravity)(sc, g);
        bool there = true;
        for (int i = 1; i < stackN; ++i) {
            if (!LooseAlive(stackMade[i], stackUser[i])) continue;
            PxTransform at{};
            At<void (*)(void*, PxTransform*)>(stackMade[i], kGetGlobalPose)(stackMade[i], &at);
            float v[3], d2 = 0;
            for (int k = 0; k < 3; ++k) {
                const float d = stackGoal[i][k] - at.p[k];
                d2 += d * d;
                v[k] = std::clamp(d / std::max(dt, 0.008f) * 0.5f, -1.5f, 1.5f) - g[k] * dt;
            }
            if (d2 > 0.002f * 0.002f) there = false;
            At<void (*)(void*, const float*, bool)>(stackMade[i], kSetLinearVelocity)(stackMade[i], v, true);
            At<void (*)(void*, const float*, bool)>(stackMade[i], kSetAngularVelocity)(stackMade[i], zero, true);
        }
        if (there || --stackSteps == 0) {
            for (int i = 0; i < stackN; ++i) {
                if (!LooseAlive(stackMade[i], stackUser[i])) continue;
                At<void (*)(void*, const float*, bool)>(stackMade[i], kSetLinearVelocity)(stackMade[i], zero, false);
                At<void (*)(void*, const float*, bool)>(stackMade[i], kSetAngularVelocity)(stackMade[i], zero, false);
                At<void (*)(void*)>(stackMade[i], kPutToSleep)(stackMade[i]);
            }
            Log("[physhands] test stack %s\n", there ? "in place, asleep" : "not all there in time, asleep where they were");
            stackSteps = 0;
        }
    }
    StackAsk ask{};
    if (!stackAsks.Read(ask) || ask.serial == stackDone) return;
    stackDone = ask.serial;
    float p[3];
    for (int k = 0; k < 3; ++k) p[k] = ask.at[k] - origin[k];
    if (ask.shapes) {
        DumpShapes(sc, p, origin);
        return;
    }
    const int f = looseFront;
    if (ask.dump) {
        for (int i = 0; i < looseN[f]; ++i) {
            const Loose& e = loose[f][i];
            if (!LooseAlive(e.actor, e.user)) continue;
            PxBounds b{};
            At<void* (*)(void*, PxBounds*, float)>(e.actor, kGetWorldBounds)(e.actor, &b, 1.0f);
            Log("[physhands] loose %016llx: %.2f kg, asleep %d, centre (%.3f %.3f %.3f), %.3f x %.3f x %.3f m\n", (unsigned long long)e.user,
                At<float (*)(void*)>(e.actor, kGetMass)(e.actor), int(At<bool (*)(void*)>(e.actor, kIsSleeping)(e.actor)),
                0.5f * (b.lo[0] + b.hi[0]) + origin[0], 0.5f * (b.lo[1] + b.hi[1]) + origin[1], 0.5f * (b.lo[2] + b.hi[2]) + origin[2],
                b.hi[0] - b.lo[0], b.hi[1] - b.lo[1], b.hi[2] - b.lo[2]);
        }
        return;
    }
    struct C { void* a; uint64_t u; PxBounds b; float d; };
    C c[64];
    int n = 0;
    for (int i = 0; i < looseN[f] && n < 64; ++i) {
        const Loose& e = loose[f][i];
        if (!LooseAlive(e.actor, e.user) || At<float (*)(void*)>(e.actor, kGetMass)(e.actor) > 3.f) continue;
        PxBounds b{};
        At<void* (*)(void*, PxBounds*, float)>(e.actor, kGetWorldBounds)(e.actor, &b, 1.0f);
        float d2 = 0;
        for (int k = 0; k < 3; ++k) {
            const float m = 0.5f * (b.lo[k] + b.hi[k]) - p[k];
            d2 += m * m;
        }
        if (d2 > 0.8f * 0.8f) continue;
        c[n++] = C{e.actor, e.user, b, d2};
    }
    std::sort(c, c + n, [](const C& x, const C& y) { return x.d < y.d; });
    stackN = std::min(n, 3);
    if (stackN < 2) {
        Log("[physhands] test stack: %d light loose things within 0.8 m of (%.2f %.2f %.2f), not enough\n", n, ask.at[0], ask.at[1], ask.at[2]);
        stackN = 0;
        return;
    }
    // over the nearest one, each on the top of the last
    const float cx = 0.5f * (c[0].b.lo[0] + c[0].b.hi[0]), cy = 0.5f * (c[0].b.lo[1] + c[0].b.hi[1]);
    float top = c[0].b.hi[2];
    for (int i = 0; i < stackN; ++i) {
        stackMade[i] = c[i].a;
        stackUser[i] = c[i].u;
        PxTransform g{};
        At<void (*)(void*, PxTransform*)>(c[i].a, kGetGlobalPose)(c[i].a, &g);
        std::memcpy(stackGoal[i], g.p, sizeof(g.p));
        if (i == 0) continue;
        const float bx = 0.5f * (c[i].b.lo[0] + c[i].b.hi[0]), by = 0.5f * (c[i].b.lo[1] + c[i].b.hi[1]);
        stackGoal[i][0] += cx - bx;
        stackGoal[i][1] += cy - by;
        stackGoal[i][2] += top - c[i].b.lo[2] + 0.002f;
        top += c[i].b.hi[2] - c[i].b.lo[2];
    }
    stackSteps = 240;
    Log("[physhands] test stack of %d at (%.2f %.2f), top %.3f m over the first's foot; userData %016llx %016llx %016llx\n", stackN,
        cx + origin[0], cy + origin[1], top - c[0].b.lo[2], (unsigned long long)stackUser[0], (unsigned long long)stackUser[1],
        (unsigned long long)(stackN > 2 ? stackUser[2] : 0));
}

// ---- the gun in a hand (script thread writes, physics thread reads) ----
struct Weapon {
    int n;
    float frame[7];   // the weapon's frame in the hand bone's: rotation x y z w, then position
    float lo[kMostWeaponBoxes][3], hi[kMostWeaponBoxes][3];   // its boxes in its own frame
    uint64_t at;
};
Seq<Weapon> weapons[2];
Weapon weaponStaged[2] = {};   // the script's thread only: parts come one by one, the frame commits them

// ---- the fingers (worker) ----
Seq<FingerHand> fingerHands[2];
struct Forearm { float elbow[3]; uint64_t at; };   // the elbow in the hand bone's frame, from the arm solve
Seq<Forearm> forearms[2];
// a: as drawn (yielding, then wrapped round what the palm is on); y: only what the fingers yielded, for their capsules on
// the body. Wrapped capsules pressed into the table they curled toward and lifted the hand off it: the fingers then found
// nothing, hung further and lifted it more.
// A CRITICALLY DAMPED FOLLOWER: x chases `goal` with velocity v, rate w (1/s), solved exactly over dt (no overshoot, no
// jump in velocity, stable at any step). The speed it reaches is held to `most`.
static void Damped(float& x, float& v, float goal, float w, float dt, float most) {
    const float e = x - goal, k = v + w * e, ex = std::exp(-w * dt);
    x = goal + (e + k * dt) * ex;
    v = (v - w * k * dt) * ex;
    if (v > most) v = most;
    if (v < -most) v = -most;
}

struct Bends { float a[5][3]; float y[5][3]; float flex[5][3]; bool on; uint64_t at; uint64_t scannerEpoch; };
Seq<Bends> bends[2];
std::atomic<uint64_t> fingerCalls{0}, fingerContacts{0}, fingerUs{0}, fingerSolves{0}, fingerFaults{0};
std::atomic<uint64_t> palmShifted{0}, palmWraps{0};   // solves posed down onto the palm; solves with the palm on something
std::atomic<uint64_t> fingersOut{0};                   // eased fingers found in something and put out of it at once
std::atomic<uint64_t> fingersOpened{0};                // ...and out toward the open hand, the solve being in it too
std::atomic<uint64_t> backPressed{0};
std::atomic<bool> edgeWrap[2] = {false, false};       // worker: the palm on something, its fingers curled over its edge                  // solves with the back of the hand pressed on something (fingers straightened)
std::atomic<float> shownBends[2][5][3] = {};           // the eased bends last given, for the statistics
std::atomic<float> shownDepth[2][5] = {};               // trace=1: each finger as drawn (b.a at the body), m deep in what is near

// One PxGeometryQuery::computePenetration, a fault in it (a mesh released under a list a cycle old) caught.
bool Penetration(float* dir, float* depth, const void* g0, const PxTransform* p0, const NearShape& s) {
    __try {
        return penetration(dir, depth, g0, p0, s.geometry, &s.pose);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        fingerFaults.fetch_add(1, std::memory_order_relaxed);
        return false;
    }
}

void Compose(const float* aq, const float* ap, const float* bq, const float* bp, float* oq, float* op);
void AxisAngle(const float* axis, float a, float* q);

// V'S OWN BODY for the hands, the game's world (physics thread), from the pose the arm solve left (the FK snapshot, model
// space) and the player's place: the trunk as two columns side by side from the waist (the first spine joint) to the top
// of the chest (the last), a little ahead of the spine, which is near the back; the crown of the head round the head's
// joint, by the head's own up and forward -- not the face, so a hand brought to the mouth or the eyes is not stopped; and
// each upper arm, from below the shoulder to short of the elbow, for the other hand. No hips and no thighs: the holsters
// are there.
std::atomic<float> selfPad{0.f};   // m V's clothes add round the trunk (the script, from what is worn)
int SelfCapsules(SelfCap* out) {
    if (!CyberpunkVR_PlayerEntityValid) return 0;
    const float pad = std::clamp(selfPad.load(std::memory_order_relaxed), 0.f, 0.04f);
    const int count = std::min(int(g_VRFKSnapCount), int(VRIK_MAX_BONES));
    const auto have = [&](int i) { return i >= 0 && i < count; };
    const int spines = std::clamp(int(g_VRSpineCount), 0, 8);
    const int hips = g_VRHipsIdx, neck = g_VRNeckIdx, head = g_VRHeadBoneIdx;
    const int la = g_VRLeftUpperArmIdx, ra = g_VRRightUpperArmIdx, lf = g_VRLeftForeArmIdx, rf = g_VRRightForeArmIdx;
    if (spines < 2 || !have(hips) || !have(neck) || !have(la) || !have(ra) || !have(g_VRSpineIdx[0]) || !have(g_VRSpineIdx[spines - 1]))
        return 0;
    const auto at = [&](int i, float* p) {
        for (int k = 0; k < 3; ++k) p[k] = g_VRFKSnapPos[i][k];
        return std::isfinite(p[0] + p[1] + p[2]);
    };
    const auto len = [](const float* v) { return std::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]); };
    const auto dot = [](const float* a, const float* b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; };
    const auto unit = [&](float* v) {
        const float n = len(v);
        if (!(n > 1e-5f)) return false;
        for (int k = 0; k < 3; ++k) v[k] /= n;
        return true;
    };
    float H[3], N[3], LA[3], RA[3], S0[3], ST[3];
    if (!at(hips, H) || !at(neck, N) || !at(la, LA) || !at(ra, RA) || !at(g_VRSpineIdx[0], S0) || !at(g_VRSpineIdx[spines - 1], ST)) return 0;
    float up[3], lat[3], fwd[3];
    for (int k = 0; k < 3; ++k) up[k] = N[k] - H[k], lat[k] = RA[k] - LA[k];
    const float upLen = len(up), latLen = len(lat);
    if (!(upLen > 0.25f && upLen < 1.0f) || !(latLen > 0.15f && latLen < 0.7f)) return 0;
    unit(up);
    const float along = dot(lat, up);
    for (int k = 0; k < 3; ++k) lat[k] -= up[k] * along;
    if (!unit(lat)) return 0;
    Cross(up, lat, fwd);   // up x right: forward
    int n = 0;
    for (int s = -1; s <= 1; s += 2) {
        SelfCap& c = out[n++];
        for (int k = 0; k < 3; ++k) {
            c.a[k] = S0[k] + lat[k] * (s * 0.06f) + fwd[k] * 0.05f;
            c.b[k] = ST[k] + lat[k] * (s * 0.065f) + fwd[k] * 0.05f;
        }
        c.r = 0.105f + pad;
        c.meets = 3;
    }
    float Hd[3], q[4];
    if (have(head) && at(head, Hd)) {
        for (int k = 0; k < 4; ++k) q[k] = g_VRFKSnapRot[head][k];
        float hu[3], hf[3];
        for (int k = 0; k < 3; ++k) hu[k] = Hd[k] - N[k];
        if (!unit(hu)) std::memcpy(hu, up, sizeof(hu));
        std::memcpy(hf, fwd, sizeof(hf));
        if (QNorm(q)) {
            // the head's own axes: of its frame's three, the one nearest the neck's line is its up, of the other two the
            // one nearest the body's forward its forward
            float axes[3][3];
            for (int a = 0; a < 3; ++a) {
                const float e[3] = {a == 0 ? 1.f : 0.f, a == 1 ? 1.f : 0.f, a == 2 ? 1.f : 0.f};
                QRot(q, e, axes[a]);
            }
            int iu = 0;
            for (int a = 1; a < 3; ++a)
                if (std::fabs(dot(axes[a], hu)) > std::fabs(dot(axes[iu], hu))) iu = a;
            int jf = iu == 0 ? 1 : 0;
            for (int a = 0; a < 3; ++a)
                if (a != iu && std::fabs(dot(axes[a], fwd)) > std::fabs(dot(axes[jf], fwd))) jf = a;
            if (std::fabs(dot(axes[iu], hu)) > 0.6f && std::fabs(dot(axes[jf], fwd)) > 0.5f) {
                const float su = dot(axes[iu], hu) < 0 ? -1.f : 1.f, sf = dot(axes[jf], fwd) < 0 ? -1.f : 1.f;
                for (int k = 0; k < 3; ++k) hu[k] = axes[iu][k] * su, hf[k] = axes[jf][k] * sf;
            }
        }
        SelfCap& c = out[n++];
        for (int k = 0; k < 3; ++k) {
            const float centre = Hd[k] + hu[k] * 0.095f - hf[k] * 0.01f;
            c.a[k] = centre - lat[k] * 0.012f;
            c.b[k] = centre + lat[k] * 0.012f;
        }
        c.r = 0.075f;
        c.meets = 3;
    }
    for (int s = 0; s < 2; ++s) {
        const int a = s ? ra : la, f = s ? rf : lf;
        float A[3], F[3], d[3];
        if (!have(f) || !at(a, A) || !at(f, F)) continue;
        for (int k = 0; k < 3; ++k) d[k] = F[k] - A[k];
        const float l = len(d);
        if (!(l > 0.15f && l < 0.5f)) continue;
        SelfCap& c = out[n++];
        for (int k = 0; k < 3; ++k) c.a[k] = A[k] + d[k] * 0.22f, c.b[k] = A[k] + d[k] * 0.85f;
        c.r = 0.045f + 0.6f * pad;
        c.meets = s ? 1 : 2;   // the right upper arm for the left hand, the left for the right
    }
    // model space to the game's world
    float eq[4];
    for (int k = 0; k < 4; ++k) eq[k] = CyberpunkVR_PlayerEntityQuat[k];
    if (!QNorm(eq)) return 0;
    for (int i = 0; i < n; ++i)
        for (float* p : {out[i].a, out[i].b}) {
            float w[3];
            QRot(eq, p, w);
            for (int k = 0; k < 3; ++k) p[k] = CyberpunkVR_PlayerEntityPos[k] + w[k];
        }
    static bool said = false;
    if (!said) {
        said = true;
        Log("[physhands] V's own body for the hands: %d capsules (trunk %.2f m, shoulders %.2f m apart; forward in the model %.2f %.2f %.2f)\n", n,
            len(up) * upLen, latLen, fwd[0], fwd[1], fwd[2]);
    }
    return n;
}

// THE PEOPLE NEAR THE HANDS, posed each step (physics thread). A kinematic capsule for each of HandCollision's, in the
// hands' own group and meeting that alone: the hands (palm, fingers, forearm, a held thing) stop on them and the fingers
// lie on them; no ray, sweep or character controller sees them (a simulation shape only), and the person is not pushed --
// they are the game's to move, these only follow their bones. A living NPC has no simulated shape of its own: their
// controller's capsules are query shapes (flags 0a, filter 0), so the hands went through people (2026-11-09).
// V'S OWN BODY after them, the same way: in the bits of the hands that meet each part (SelfCapsules) and meeting nothing,
// so a palm or a finger whose mask has that bit stops on it and nothing else does (not the world, not the forearms).
void PeopleStep(const float* origin, const Settings& s) {
    People P{};
    const bool enabled = g_liveControls.xrHandPhysics != 0 && g_liveControls.xrHandCollision != 0;
    const bool fresh = enabled && s.people && filterReady && material && scene && people.Read(P) && GetTickCount64() - P.at < 300;
    const int n = fresh ? std::clamp(P.n, 0, kMostPeople) : 0;
    static SelfShown shown;
    shown.n = enabled && s.self && selfBitsReady && filterReady && material && scene ? SelfCapsules(shown.c) : 0;
    shown.at = GetTickCount64();
    selfShown.Write(shown);
    const int ns = shown.n;
    const uint32_t filterPeople[4] = {handFilter[0], handFilter[1], handFilter[0], handFilter[1]};
    static PeopleNear snap;
    int m = 0;
    const auto park = [](int i) {
        if (personActor[i] && personLive[i]) {
            At<void (*)(void*, int, bool)>(personActor[i], kSetActorFlag)(personActor[i], kDisableSimulation, true);
            personLive[i] = false;
        }
    };
    for (int i = 0; i < kMostBodies; ++i) {
        const bool own = i >= kMostPeople;
        const int j = i - kMostPeople;
        if (own ? j >= ns : i >= n) {
            park(i);
            continue;
        }
        const float* ca = own ? shown.c[j].a : P.c[i].a;
        const float* cb = own ? shown.c[j].b : P.c[i].b;
        const float cr = own ? shown.c[j].r : P.c[i].r;
        const int owner = own ? kSelfOwner : P.c[i].owner;
        const uint8_t meets = own ? shown.c[j].meets : uint8_t(3);
        uint32_t filter[4];
        if (own) {
            const unsigned long long g = (meets & 1 ? bitSelfLeft : 0) | (meets & 2 ? bitSelfRight : 0);
            filter[0] = uint32_t(g);
            filter[1] = uint32_t(g >> 32);
            filter[2] = filter[3] = 0;
        } else {
            std::memcpy(filter, filterPeople, sizeof(filter));
        }
        float a[3], b[3], d[3], len = 0;
        for (int k = 0; k < 3; ++k) {
            a[k] = ca[k] - origin[k];
            b[k] = cb[k] - origin[k];
            d[k] = b[k] - a[k];
            len += d[k] * d[k];
        }
        len = std::sqrt(len);
        if (!std::isfinite(len + a[0] + a[1] + a[2]) || !(cr > 0.005f && cr < 0.5f) || len > 2.f) {
            park(i);
            continue;
        }
        // THE PART HELD AND WHAT IS NEXT TO IT meet nothing while a hand holds them: the hold keeps the hand on the part, and
        // a contact as well threw the hand about with every move of the part (the capsules are put where the bones are, not
        // moved there), and the fingers on it with the hand
        if (!own && owner > 0 && HeldNear(owner, a, b, cr)) filter[0] = filter[1] = filter[2] = filter[3] = 0;
        PxTransform pose{{0, 0, 0, 1}, {0.5f * (a[0] + b[0]), 0.5f * (a[1] + b[1]), 0.5f * (a[2] + b[2])}};
        if (len > 1e-4f) {
            // the capsule's axis is its x: the turn taking x to the line between its ends
            const float u[3] = {d[0] / len, d[1] / len, d[2] / len};
            const float w = 1.f + u[0];
            if (w > 1e-6f) {
                float q[4] = {0.f, -u[2], u[1], w};
                if (QNorm(q)) std::memcpy(pose.q, q, sizeof(q));
            } else {
                pose.q[0] = 0; pose.q[1] = 0; pose.q[2] = 1; pose.q[3] = 0;   // the other way: half a turn about z
            }
        }
        const float half = std::max(0.5f * len, 0.001f);
        const struct { int type; float radius, halfHeight; } g{kGeomCapsule, cr, half};
        void*& act = personActor[i];
        if (!act) {
            act = At<void* (*)(void*, const PxTransform*)>(physics, kCreateRigidDynamic)(physics, &pose);
            if (!act) break;
            At<void (*)(void*, int, bool)>(act, kSetRigidBodyFlag)(act, 1, true);   // PxRigidBodyFlag::eKINEMATIC
            void* mats[1] = {material};
            const uint8_t flags = 1;   // a simulation shape only
            void* sh = At<void* (*)(void*, const void*, void* const*, uint16_t, bool, const uint8_t*)>(physics, kCreateShape)(physics, &g, mats, 1, true, &flags);
            if (!sh) {
                At<void (*)(void*)>(act, kRelease)(act);
                act = nullptr;
                break;
            }
            At<void (*)(void*, const uint32_t*)>(sh, kShapeSetSimFilter)(sh, filter);
            At<bool (*)(void*, void*)>(act, kAttachShape)(act, sh);
            At<void (*)(void*)>(sh, kShapeRelease)(sh);
            *reinterpret_cast<uintptr_t*>(static_cast<char*>(act) + 0x10) = ~uintptr_t(0);   // no proxy of the game's
            At<void (*)(void*, void*, const void*)>(scene, kAddActor)(scene, act, nullptr);
            personShape[i] = sh;
            personGeom[i][0] = cr;
            personGeom[i][1] = half;
            std::memcpy(personFilter[i], filter, sizeof(filter));
            personLive[i] = true;
            static int said = 0;
            if (said++ < 4 || (own && j == 0)) Log("[physhands] %s capsule made (%d): radius %.3f m, %.3f m long\n", own ? "V's own" : "a person's", i, cr, 2 * half);
        } else {
            if (std::fabs(cr - personGeom[i][0]) > 5e-4f || std::fabs(half - personGeom[i][1]) > 5e-4f) {
                At<void (*)(void*, const void*)>(personShape[i], kShapeSetGeometry)(personShape[i], &g);
                personGeom[i][0] = cr;
                personGeom[i][1] = half;
            }
            if (std::memcmp(personFilter[i], filter, sizeof(filter)) != 0) {
                At<void (*)(void*, const uint32_t*)>(personShape[i], kShapeSetSimFilter)(personShape[i], filter);
                std::memcpy(personFilter[i], filter, sizeof(filter));
                Refilter(act);
            }
            if (!personLive[i]) {
                At<void (*)(void*, int, bool)>(act, kSetActorFlag)(act, kDisableSimulation, false);
                personLive[i] = true;
            }
            At<void (*)(void*, const PxTransform*, bool)>(act, kSetGlobalPose)(act, &pose, true);
        }
        NearShape& e = snap.s[m];
        std::memset(e.geometry, 0, sizeof(e.geometry));
        std::memcpy(e.geometry, &g, sizeof(g));
        e.pose = pose;
        e.bounds = worldBounds ? worldBounds(e.geometry, &e.pose, 1.0f) : PxBounds{};
        e.type = kGeomCapsule;
        snap.owner[m] = owner;
        snap.meets[m] = meets;
        ++m;
    }
    snap.n = m;
    snap.at = GetTickCount64();
    std::memcpy(personNear, snap.s, sizeof(NearShape) * size_t(m));
    std::memcpy(personOwner, snap.owner, sizeof(int) * size_t(m));
    std::memcpy(personMeets, snap.meets, size_t(m));
    personNearN = m;
    peopleNear.Write(snap);
}

// THE BODY'S SHAPES AS POSED THIS STEP, in the hand bone's frame (physics thread writes, the animation thread reads):
// the palm's box, the finger capsules, the forearm's -- what the drawn hand is kept out of the world by (KeepDrawnOut)
struct BodyShape { alignas(4) uint8_t geometry[16]; PxTransform local; float reach; uint8_t part; };   // 0 palm, 1 finger, 2 forearm, 3 gun
constexpr int kBodyShapes = 18 + kMostWeaponBoxes;
struct BodyShapes { BodyShape s[kBodyShapes]; int n; uint64_t at; };
Seq<BodyShapes> bodyShapes[2];

// The palm, grown by a centimetre and a half, in a static shape near the player (physics thread: the list the worker
// reads is never written while it is the front one) -- or any of the body's other shapes, its fingers' and its forearm's
// capsules grown by a centimetre: a hand held off a wall by its fingertips or its thumb is on the wall. Asked of the palm
// alone, it was "on something loose", steered at a push's strength and all of the gap a step, and drove its fingers'
// capsules a centimetre and more into the wall, which threw the hand back off it (2026-11-09).
int OtherHandShapes(int side, const float* pose, NearShape* out, int cap, BodyShapes* scratch, int partOnly = -1);
bool TouchesWorld(int side, const PxTransform& at, const Settings& s, int what) {
    const int front = nearFront.load(std::memory_order_acquire);
    if (front < 0 || !penetration) return false;
    const NearList& L = nearLists[front];
    // THE OTHER HAND while the two meet is the world here too: pressed on, a hand is held soft as on a wall. Taken for a
    // loose thing, it was pushed at a loose thing's strength and the two went 4 cm into each other (2026-11-10).
    static BodyShapes otherScratch;   // physics thread
    static NearShape other[kBodyShapes];
    int otherN = 0;
    if (what >= kTouchAll && handsMeetOn && actor[1 - side]) {
        PxTransform op{};
        At<void (*)(void*, PxTransform*)>(actor[1 - side], kGetGlobalPose)(actor[1 - side], &op);
        const float pose[7] = {op.q[0], op.q[1], op.q[2], op.q[3], op.p[0], op.p[1], op.p[2]};
        otherN = OtherHandShapes(1 - side, pose, other, kBodyShapes, &otherScratch);
    }
    constexpr float kGrow = 0.015f;
    const BoxGeometry box{kGeomBox, {madeHalf[side][0] + kGrow, madeHalf[side][1] + kGrow, madeHalf[side][2] + kGrow}};
    const float off[3] = {madeCentre[side][0], madeCentre[side][1], madeCentre[side][2]};
    PxTransform pose{};
    float r[3];
    QRot(at.q, off, r);
    for (int k = 0; k < 3; ++k) pose.p[k] = at.p[k] + r[k];
    std::memcpy(pose.q, at.q, sizeof(pose.q));
    int calls = 0;
    const auto touches = [&](const BoxGeometry& g, const PxTransform& at) {
        const float reach = std::sqrt(g.half[0] * g.half[0] + g.half[1] * g.half[1] + g.half[2] * g.half[2]);
        for (int i = 0; i < L.n && calls < 32; ++i) {
            const NearShape& e = L.s[i];
            bool apart = false;
            for (int k = 0; k < 3 && !apart; ++k) apart = at.p[k] + reach < e.bounds.lo[k] || at.p[k] - reach > e.bounds.hi[k];
            if (apart) continue;
            ++calls;
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, &g, &at, e) && depth > 0) return true;
        }
        // a person is the world here: pressed on, the hand is held soft and drawn still, as on a wall (V's own body too,
        // while the hand meets it)
        for (int i = 0; what >= kTouchPeople && i < personNearN && calls < 48; ++i) {
            if (!PersonMeets(i, side) || (personOwner[i] == kSelfOwner && what < kTouchAll)) continue;
            const NearShape& e = personNear[i];
            bool apart = false;
            for (int k = 0; k < 3 && !apart; ++k) apart = at.p[k] + reach < e.bounds.lo[k] || at.p[k] - reach > e.bounds.hi[k];
            if (apart) continue;
            ++calls;
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, &g, &at, e) && depth > 0) return true;
        }
        for (int i = 0; i < otherN && calls < 64; ++i) {
            const NearShape& e = other[i];
            bool apart = false;
            for (int k = 0; k < 3 && !apart; ++k) apart = at.p[k] + reach < e.bounds.lo[k] || at.p[k] - reach > e.bounds.hi[k];
            if (apart) continue;
            ++calls;
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, &g, &at, e) && depth > 0) return true;
        }
        return false;
    };
    if (touches(box, pose)) return true;
    {
        static BodyShapes bs;   // physics thread
        if (bodyShapes[side].Read(bs) && GetTickCount64() - bs.at < 200) {
            for (int i = 1; i < bs.n && i < kBodyShapes; ++i) {
                int type = 0;
                std::memcpy(&type, bs.s[i].geometry, sizeof(type));
                if (type != kGeomCapsule) continue;
                struct { int type; float radius, halfHeight; } g{};
                std::memcpy(&g, bs.s[i].geometry, sizeof(g));
                g.radius += 0.01f;
                PxTransform w{};
                Compose(at.q, at.p, bs.s[i].local.q, bs.s[i].local.p, w.q, w.p);
                const float reach = g.halfHeight + g.radius;
                for (int j = 0; j < L.n && calls < 96; ++j) {
                    const NearShape& e = L.s[j];
                    bool apart = false;
                    for (int k = 0; k < 3 && !apart; ++k) apart = w.p[k] + reach < e.bounds.lo[k] || w.p[k] - reach > e.bounds.hi[k];
                    if (apart) continue;
                    ++calls;
                    float dir[3]{}, depth = 0;
                    if (Penetration(dir, &depth, &g, &w, e) && depth > 0) return true;
                }
            }
        }
    }
    // a gun's boxes meet nothing of the world (HandFilters): a gun in a wall is not the hand on it
    return false;
}

// WHAT A HAND CAN HOLD ON BY, from the world's shapes round it (physics thread): the palm's box grown by 2 cm and each
// finger's capsule by 1.2 cm, and each one's way out of what it is in, at the point of it that is deepest that way (on the
// surface, not the shape's middle: the palm's middle stands 2 cm further off a wall than the fingers' do). A TOP: a way out
// up (a railing's, a ledge's, a crate's). AN EDGE OR A CORNER: two ways out more than 40 degrees apart, the one point 1.5
// cm behind the other's face -- fingers round a pillar's corner, over a ledge's lip, round a pole. The two faces of an
// inside corner face each other and are not held, nor is a wall's face alone: walls are not climbed. 0 none, 1 a top, 2
// an edge or a corner.
int HoldFeature(int side, const PxTransform& at) {
    const int front = nearFront.load(std::memory_order_acquire);
    if (front < 0 || !penetration) return 0;
    const NearList& L = nearLists[front];
    struct Hit { float p[3], n[3]; };
    Hit hits[24];
    int nh = 0, calls = 0;
    // `extent(n)`: how far the shape reaches from its middle against n (its point deepest in what pushes it out along n)
    const auto probe = [&](const void* g, const PxTransform& pose, float reach, const auto& extent) {
        for (int i = 0; i < L.n && calls < 160 && nh < 24; ++i) {
            const NearShape& e = L.s[i];
            bool apart = false;
            for (int k = 0; k < 3 && !apart; ++k) apart = pose.p[k] + reach < e.bounds.lo[k] || pose.p[k] - reach > e.bounds.hi[k];
            if (apart) continue;
            ++calls;
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, g, &pose, e) && depth > 0 && std::isfinite(dir[0] + dir[1] + dir[2])) {
                Hit& h = hits[nh++];
                const float x = extent(dir);
                for (int k = 0; k < 3; ++k) h.p[k] = pose.p[k] - dir[k] * x;
                std::memcpy(h.n, dir, sizeof(h.n));
            }
        }
    };
    {
        const BoxGeometry box{kGeomBox, {madeHalf[side][0] + 0.02f, madeHalf[side][1] + 0.02f, madeHalf[side][2] + 0.02f}};
        PxTransform pose{};
        float r[3];
        QRot(at.q, madeCentre[side], r);
        for (int k = 0; k < 3; ++k) pose.p[k] = at.p[k] + r[k];
        std::memcpy(pose.q, at.q, sizeof(pose.q));
        float axes[3][3];
        for (int a = 0; a < 3; ++a) {
            const float e[3] = {a == 0 ? 1.f : 0.f, a == 1 ? 1.f : 0.f, a == 2 ? 1.f : 0.f};
            QRot(at.q, e, axes[a]);
        }
        const auto extent = [&](const float* n) {
            float x = 0;
            for (int a = 0; a < 3; ++a)
                x += std::fabs(n[0] * axes[a][0] + n[1] * axes[a][1] + n[2] * axes[a][2]) * (box.half[a] - 0.02f);
            return x;
        };
        probe(&box, pose, std::sqrt(box.half[0] * box.half[0] + box.half[1] * box.half[1] + box.half[2] * box.half[2]), extent);
    }
    static BodyShapes bs;   // physics thread
    if (bodyShapes[side].Read(bs) && GetTickCount64() - bs.at < 200)
        for (int i = 0; i < bs.n && i < kBodyShapes; ++i) {
            if (bs.s[i].part != 1) continue;
            struct { int type; float radius, halfHeight; } g{};
            std::memcpy(&g, bs.s[i].geometry, sizeof(g));
            if (g.type != kGeomCapsule) continue;
            const float skin = g.radius;
            g.radius += 0.012f;
            PxTransform w{};
            Compose(at.q, at.p, bs.s[i].local.q, bs.s[i].local.p, w.q, w.p);
            float ax[3];
            const float ex[3] = {1.f, 0.f, 0.f};
            QRot(w.q, ex, ax);
            const auto extent = [&](const float* n) {
                return skin + std::fabs(n[0] * ax[0] + n[1] * ax[1] + n[2] * ax[2]) * g.halfHeight;
            };
            probe(&g, w, g.halfHeight + g.radius, extent);
        }
    for (int i = 0; i < nh; ++i)
        if (hits[i].n[2] >= 0.45f) return 1;
    for (int i = 0; i < nh; ++i)
        for (int j = i + 1; j < nh; ++j) {
            const Hit &a = hits[i], &b = hits[j];
            const float c = a.n[0] * b.n[0] + a.n[1] * b.n[1] + a.n[2] * b.n[2];
            if (!(c < 0.77f)) continue;
            float ab = 0, ba = 0;
            for (int k = 0; k < 3; ++k) ab += (b.p[k] - a.p[k]) * a.n[k], ba += (a.p[k] - b.p[k]) * b.n[k];
            if (ab < -0.015f || ba < -0.015f) return 2;
        }
    return 0;
}

// The part of a person a hand closes on (physics thread): of the people's capsules the palm's box grown by 2 cm or a
// finger's capsule grown by 1 cm is in, the one it is deepest in, of a part the script named (owner > 0).
bool PersonAt(int side, const PxTransform& at, int& index) {
    index = -1;
    float best = 0;
    const auto probe = [&](const void* g, const PxTransform& pose, float reach) {
        for (int i = 0; i < personNearN; ++i) {
            if (personOwner[i] <= 0) continue;
            const NearShape& e = personNear[i];
            bool apart = false;
            for (int k = 0; k < 3 && !apart; ++k) apart = pose.p[k] + reach < e.bounds.lo[k] || pose.p[k] - reach > e.bounds.hi[k];
            if (apart) continue;
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, g, &pose, e) && depth > best) best = depth, index = i;
        }
    };
    {
        const BoxGeometry box{kGeomBox, {madeHalf[side][0] + 0.02f, madeHalf[side][1] + 0.02f, madeHalf[side][2] + 0.02f}};
        PxTransform pose{};
        float r[3];
        QRot(at.q, madeCentre[side], r);
        for (int k = 0; k < 3; ++k) pose.p[k] = at.p[k] + r[k];
        std::memcpy(pose.q, at.q, sizeof(pose.q));
        probe(&box, pose, std::sqrt(box.half[0] * box.half[0] + box.half[1] * box.half[1] + box.half[2] * box.half[2]));
    }
    static BodyShapes bs;   // physics thread
    if (bodyShapes[side].Read(bs) && GetTickCount64() - bs.at < 200)
        for (int i = 0; i < bs.n && i < kBodyShapes; ++i) {
            if (bs.s[i].part != 1) continue;
            struct { int type; float radius, halfHeight; } g{};
            std::memcpy(&g, bs.s[i].geometry, sizeof(g));
            if (g.type != kGeomCapsule) continue;
            g.radius += 0.01f;
            PxTransform w{};
            Compose(at.q, at.p, bs.s[i].local.q, bs.s[i].local.p, w.q, w.p);
            probe(&g, w, g.halfHeight + g.radius);
        }
    return index >= 0;
}

// Where a person's part is this step (physics thread), by the script's id for it. False: not among them now.
bool PersonPose(int owner, PxTransform& out) {
    for (int i = 0; i < personNearN; ++i)
        if (personOwner[i] == owner) {
            out = personNear[i].pose;
            return true;
        }
    return false;
}

// A hand in V's own body (physics thread): its palm's box or a finger's capsule within 5 mm of a part that hand would meet.
bool InSelf(int side) {
    if (!actor[side]) return false;
    PxTransform at{};
    At<void (*)(void*, PxTransform*)>(actor[side], kGetGlobalPose)(actor[side], &at);
    static BodyShapes bs;   // physics thread
    const bool shapes = bodyShapes[side].Read(bs) && GetTickCount64() - bs.at < 200;
    for (int i = 0; i < personNearN; ++i) {
        if (personOwner[i] != kSelfOwner || !(personMeets[i] >> side & 1)) continue;
        const NearShape& e = personNear[i];
        {
            const BoxGeometry box{kGeomBox, {madeHalf[side][0] + 0.005f, madeHalf[side][1] + 0.005f, madeHalf[side][2] + 0.005f}};
            PxTransform pose{};
            float r[3];
            QRot(at.q, madeCentre[side], r);
            for (int k = 0; k < 3; ++k) pose.p[k] = at.p[k] + r[k];
            std::memcpy(pose.q, at.q, sizeof(pose.q));
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, &box, &pose, e) && depth > 0) return true;
        }
        for (int j = 0; shapes && j < bs.n && j < kBodyShapes; ++j) {
            if (bs.s[j].part != 1) continue;
            struct { int type; float radius, halfHeight; } g{};
            std::memcpy(&g, bs.s[j].geometry, sizeof(g));
            if (g.type != kGeomCapsule) continue;
            g.radius += 0.005f;
            PxTransform w{};
            Compose(at.q, at.p, bs.s[j].local.q, bs.s[j].local.p, w.q, w.p);
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, &g, &w, e) && depth > 0) return true;
        }
    }
    return false;
}

// The two hands apart (physics thread): no shape of one within 1.5 cm of the other's (the palms, the fingers, the forearms,
// a gun's boxes, which reach 30 cm and more past the palm), and the palms' middles 12 cm apart. Without the shapes to go
// by, the palms 26 cm apart.
bool HandsApart() {
    if (!actor[0] || !actor[1]) return false;
    float c[2][3], pose[2][7];
    for (int side = 0; side < 2; ++side) {
        PxTransform at{};
        At<void (*)(void*, PxTransform*)>(actor[side], kGetGlobalPose)(actor[side], &at);
        float r[3];
        QRot(at.q, madeCentre[side], r);
        for (int k = 0; k < 3; ++k) c[side][k] = at.p[k] + r[k];
        for (int k = 0; k < 4; ++k) pose[side][k] = at.q[k];
        for (int k = 0; k < 3; ++k) pose[side][4 + k] = at.p[k];
    }
    float d2 = 0;
    for (int k = 0; k < 3; ++k) d2 += (c[0][k] - c[1][k]) * (c[0][k] - c[1][k]);
    static BodyShapes scratch[2];   // physics thread
    static NearShape A[kBodyShapes], B[kBodyShapes];
    const int na = OtherHandShapes(0, pose[0], A, kBodyShapes, &scratch[0]);
    const int nb = OtherHandShapes(1, pose[1], B, kBodyShapes, &scratch[1]);
    if (!na || !nb) return d2 > 0.26f * 0.26f;
    if (!(d2 > 0.12f * 0.12f)) return false;
    constexpr float kClear = 0.015f;
    for (int i = 0; i < na; ++i) {
        alignas(4) uint8_t g[16];
        std::memcpy(g, A[i].geometry, sizeof(g));
        int type = 0;
        std::memcpy(&type, g, sizeof(type));
        float f[3];
        std::memcpy(f, g + 4, sizeof(f));
        if (type == kGeomBox) f[0] += kClear, f[1] += kClear, f[2] += kClear;
        else if (type == kGeomCapsule) f[0] += kClear;
        else continue;
        std::memcpy(g + 4, f, sizeof(f));
        for (int j = 0; j < nb; ++j) {
            bool apart = false;
            for (int k = 0; k < 3 && !apart; ++k)
                apart = A[i].bounds.hi[k] + kClear < B[j].bounds.lo[k] || A[i].bounds.lo[k] - kClear > B[j].bounds.hi[k];
            if (apart) continue;
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, g, &A[i].pose, B[j]) && depth > 0) return false;
        }
    }
    return true;
}

// THE OTHER HAND for a hand's fingers (the worker): its palm's box, its fingers' and its forearm's capsules and its gun's
// boxes, as its body was posed after the step (`pose`, PhysX's space). Without the forearm a hand went through the other
// arm; without the gun the free hand could not lie on it.
int OtherHandShapes(int side, const float* pose, NearShape* out, int cap, BodyShapes* scratch, int partOnly) {
    BodyShapes& bs = *scratch;   // the caller's thread's
    if (!worldBounds || !bodyShapes[side].Read(bs) || GetTickCount64() - bs.at > 200) return 0;
    int n = 0;
    for (int i = 0; i < bs.n && i < kBodyShapes && n < cap; ++i) {
        const BodyShape& b = bs.s[i];
        if (partOnly >= 0 && b.part != partOnly) continue;
        NearShape& e = out[n++];
        std::memset(e.geometry, 0, sizeof(e.geometry));
        std::memcpy(e.geometry, b.geometry, sizeof(b.geometry));
        std::memcpy(&e.type, b.geometry, sizeof(int));
        Compose(pose, pose + 4, b.local.q, b.local.p, e.pose.q, e.pose.p);
        e.bounds = worldBounds(e.geometry, &e.pose, 1.0f);
    }
    return n;
}

// THE WAYS OUT OF THE OTHER HAND for this one (physics thread): of this hand's palm box and its fingers' capsules, grown
// by 4 and 2 cm, against the other hand's palm and fingers, each pair's way out and how far the shapes still are from
// touching along it (up to `most`, 4 floats each: the way, then the room). Every pair, not the deepest's alone: fingers
// curled round the other hand had the deepest, and theirs pointed into it. Grown, so a hand sent at the other from 2 cm
// is let on those 2 cm and no more: asked only once the two touched, a hand sent 10 cm into the other at full speed hit
// it, and PhysX threw the two apart (2026-11-10).
int OtherHandOut(int side, const PxTransform& at, float* out, int most) {
    if (!handsMeetOn || !actor[1 - side]) return 0;
    static BodyShapes otherScratch, mine;   // physics thread
    static NearShape other[kBodyShapes];
    PxTransform op{};
    At<void (*)(void*, PxTransform*)>(actor[1 - side], kGetGlobalPose)(actor[1 - side], &op);
    const float pose[7] = {op.q[0], op.q[1], op.q[2], op.q[3], op.p[0], op.p[1], op.p[2]};
    const int n = OtherHandShapes(1 - side, pose, other, kBodyShapes, &otherScratch);
    if (!n) return 0;
    int found = 0;
    const auto probe = [&](const void* g, const PxTransform& w, float grown) {
        for (int i = 0; i < n && found < most; ++i) {
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, g, &w, other[i]) && depth > 0 && std::isfinite(dir[0] + dir[1] + dir[2])) {
                std::memcpy(out + 4 * found, dir, 3 * sizeof(float));
                out[4 * found + 3] = std::max(0.f, grown - depth);
                ++found;
            }
        }
    };
    {
        constexpr float kGrown = 0.04f;
        const BoxGeometry box{kGeomBox, {madeHalf[side][0] + kGrown, madeHalf[side][1] + kGrown, madeHalf[side][2] + kGrown}};
        PxTransform w{};
        float r[3];
        QRot(at.q, madeCentre[side], r);
        for (int k = 0; k < 3; ++k) w.p[k] = at.p[k] + r[k];
        std::memcpy(w.q, at.q, sizeof(w.q));
        probe(&box, w, kGrown);
    }
    if (bodyShapes[side].Read(mine) && GetTickCount64() - mine.at < 200)
        for (int i = 0; i < mine.n && i < kBodyShapes; ++i) {
            if (mine.s[i].part != 1) continue;
            struct { int type; float radius, halfHeight; } g{};
            std::memcpy(&g, mine.s[i].geometry, sizeof(g));
            if (g.type != kGeomCapsule) continue;
            g.radius += 0.02f;
            PxTransform w{};
            Compose(at.q, at.p, mine.s[i].local.q, mine.s[i].local.p, w.q, w.p);
            probe(&g, w, 0.02f);
        }
    return found;
}

// THE OTHER HAND IN THIS HAND'S GUN (physics thread): its palm box and its fingers' capsules, grown by 1 cm, against the
// gun's boxes as posed last step. The gun's collision with it is taken up again only once this is false: given the boxes
// while in them, PhysX throws the hand out.
bool OtherInGun(int side) {
    const int other = 1 - side;
    if (!actor[side] || !actor[other]) return false;
    static BodyShapes gunScratch, mine;   // physics thread
    static NearShape gun[kBodyShapes];
    PxTransform gp{}, op{};
    At<void (*)(void*, PxTransform*)>(actor[side], kGetGlobalPose)(actor[side], &gp);
    At<void (*)(void*, PxTransform*)>(actor[other], kGetGlobalPose)(actor[other], &op);
    const float pose[7] = {gp.q[0], gp.q[1], gp.q[2], gp.q[3], gp.p[0], gp.p[1], gp.p[2]};
    const int n = OtherHandShapes(side, pose, gun, kBodyShapes, &gunScratch, 3);
    if (!n) return false;
    const auto probe = [&](const void* g, const PxTransform& w) {
        for (int i = 0; i < n; ++i) {
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, g, &w, gun[i]) && depth > 0) return true;
        }
        return false;
    };
    {
        constexpr float kGrown = 0.01f;
        const BoxGeometry box{kGeomBox, {madeHalf[other][0] + kGrown, madeHalf[other][1] + kGrown, madeHalf[other][2] + kGrown}};
        PxTransform w{};
        float r[3];
        QRot(op.q, madeCentre[other], r);
        for (int k = 0; k < 3; ++k) w.p[k] = op.p[k] + r[k];
        std::memcpy(w.q, op.q, sizeof(w.q));
        if (probe(&box, w)) return true;
    }
    if (bodyShapes[other].Read(mine) && GetTickCount64() - mine.at < 200)
        for (int i = 0; i < mine.n && i < kBodyShapes; ++i) {
            if (mine.s[i].part != 1) continue;
            struct { int type; float radius, halfHeight; } g{};
            std::memcpy(&g, mine.s[i].geometry, sizeof(g));
            if (g.type != kGeomCapsule) continue;
            g.radius += 0.01f;
            PxTransform w{};
            Compose(op.q, op.p, mine.s[i].local.q, mine.s[i].local.p, w.q, w.p);
            if (probe(&g, w)) return true;
        }
    return false;
}

// Nothing static near the player on the straight line from a to b (PhysX's space): a ball of 3 cm walked along it.
// Physics thread, like TouchesWorld. False when there is no list to ask; `what` then says what was met, when it was.
bool PathFree(const float* a, const float* b, char* what = nullptr, size_t whatSize = 0, float radius = 0.03f) {
    const int front = nearFront.load(std::memory_order_acquire);
    if (front < 0 || !penetration) return false;
    const NearList& L = nearLists[front];
    const struct { int type; float radius; } ball{kGeomSphere, radius};
    float d[3];
    for (int k = 0; k < 3; ++k) d[k] = b[k] - a[k];
    const float len = std::sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    const int n = std::clamp(int(len / std::max(radius, 0.01f)) + 1, 2, 96);
    for (int i = 0; i <= n; ++i) {
        PxTransform at{};
        at.q[3] = 1;
        for (int k = 0; k < 3; ++k) at.p[k] = a[k] + d[k] * (float(i) / n);
        for (int j = 0; j < L.n; ++j) {
            const NearShape& e = L.s[j];
            bool apart = false;
            for (int k = 0; k < 3 && !apart; ++k) apart = at.p[k] + ball.radius < e.bounds.lo[k] || at.p[k] - ball.radius > e.bounds.hi[k];
            if (apart) continue;
            float dir[3]{}, depth = 0;
            if (Penetration(dir, &depth, &ball, &at, e) && depth > 0) {
                if (what)
                    std::snprintf(what, whatSize, "at %d/%d of the line a shape of type %d, %.1f cm in, its bounds %.2f %.2f %.2f .. %.2f %.2f %.2f",
                                  i, n, e.type, 100.f * depth, e.bounds.lo[0], e.bounds.lo[1], e.bounds.lo[2], e.bounds.hi[0],
                                  e.bounds.hi[1], e.bounds.hi[2]);
                return false;
            }
        }
    }
    return true;
}

constexpr float kTeleportStep = 1.0f;       // m the player's entity moves in one step: a teleport, not a walk or a fall
constexpr uint64_t kTeleportPutMs = 300;    // the hands put where tracking is at least that long after it...
constexpr uint64_t kTeleportPutMostMs = 2000;   // ...and on while tracking's place still jumps, up to this
constexpr float kStuckGap = 0.25f;          // m a body is held off where tracking is...
constexpr uint64_t kStuckMs = 400;          // ...for this long before it is asked whether it is stuck
constexpr float kBehindGap = 0.03f;         // m: closer, held off by something thin between it and tracking...
constexpr uint64_t kBehindMs = 250;         // ...this long, asked as well
uint64_t farSince[2] = {0, 0}, stuckAsked[2] = {0, 0};

// STUCK. Bonelab's hands hang on a jointed arm, which does not reach round a wall, and its menu has "unstick
// controllers" for when they hang anyway; these hang on nothing, so it is done here, by itself. A body the world holds
// more than kStuckGap off where tracking is for kStuckMs (or more than kBehindGap for kBehindMs, with something between
// it and tracking) is put there -- when the palm there is in nothing and a straight arm from the shoulder gets there
// through nothing. A hand pushed into a wall, under a table or through a thin wall fails one of
// the two and stays where it is stopped. Physics thread, between steps.
// THE ARM'S REACH: tracking farther from the shoulder (as the arm solve placed it this frame) than the arm's length is
// brought back along the shoulder's line to that length. Nothing is limited without a fresh solve.
static void ArmReach(int side, PxTransform& to, const float* origin, uint64_t tick, const Settings& s) {
    Shoulder sh{};
    Target t{};
    if (!shoulders[side].Read(sh) || tick - sh.at > 300 || sh.reach <= 0 || !targets[side].Read(t)) return;
    float r[3], d[3], n = 0;
    QRot(t.entityRot, sh.model, r);
    for (int k = 0; k < 3; ++k) {
        const float shoulder = t.entityPos[k] + r[k] - origin[k];
        d[k] = to.p[k] - shoulder;
        n += d[k] * d[k];
    }
    n = std::sqrt(n);
    const float most = sh.reach + s.armReachSpare;
    if (!(n > most) || !std::isfinite(n)) return;
    const float k1 = most / n;
    for (int k = 0; k < 3; ++k) to.p[k] -= d[k] * (1.f - k1);
    reachHeld[side].fetch_add(1, std::memory_order_relaxed);
}

bool Unstick(int side, const PxTransform& to, const float* origin, int holding, uint64_t tick, const Settings& s) {
    void* a = actor[side];
    if (!a || !simulating[side] || (holding & (1 << side))) {
        farSince[side] = 0;
        return false;
    }
    PxTransform now{};
    At<void (*)(void*, PxTransform*)>(a, kGetGlobalPose)(a, &now);
    float gap2 = 0;
    for (int k = 0; k < 3; ++k) gap2 += (to.p[k] - now.p[k]) * (to.p[k] - now.p[k]);
    if (!(gap2 > kBehindGap * kBehindGap)) {
        farSince[side] = 0;
        return false;
    }
    // FAR, or close with something between: a body put by a teleport's passing frames on the far side of a thin wall sat
    // 23 cm off tracking behind it, under the far gap, for good (2026-11-09)
    const bool distant = gap2 > kStuckGap * kStuckGap;
    if (!farSince[side]) farSince[side] = tick;
    if (tick - farSince[side] < (distant ? kStuckMs : kBehindMs) || tick - stuckAsked[side] < (distant ? 200u : 100u)) return false;
    stuckAsked[side] = tick;
    static uint64_t toldAt[2] = {0, 0};
    const auto refuse = [&](const char* why) {
        if (distant && tick - toldAt[side] >= 1000) {
            toldAt[side] = tick;
            Log("[physhands] %s hand %.2f m off tracking for %llu ms, left there: %s\n", side ? "right" : "left", std::sqrt(gap2),
                (unsigned long long)(tick - farSince[side]), why);
        }
        return false;
    };
    // the world list asked must be the one round where the player is now
    const int front = nearFront.load(std::memory_order_acquire);
    if (front < 0) return refuse("no world list yet");
    {
        float c[3], d2 = 0;
        for (int k = 0; k < 3; ++k) c[k] = CyberpunkVR_PlayerEntityPos[k] - origin[k];
        c[2] += 1.1f;
        for (int k = 0; k < 3; ++k) d2 += (c[k] - nearLists[front].center[k]) * (c[k] - nearLists[front].center[k]);
        if (d2 > 0.5f * 0.5f) return refuse("the world list is still the old place's");
    }
    Shoulder sh{};
    Target t{};
    if (!shoulders[side].Read(sh) || tick - sh.at > 300 || !targets[side].Read(t)) return refuse("no shoulder from the arm solve");
    float shoulder[3], palm[3], r[3];
    QRot(t.entityRot, sh.model, r);
    for (int k = 0; k < 3; ++k) shoulder[k] = t.entityPos[k] + r[k] - origin[k];
    const float off[3] = {madeCentre[side][0], madeCentre[side][1], madeCentre[side][2]};
    QRot(to.q, off, r);
    for (int k = 0; k < 3; ++k) palm[k] = to.p[k] + r[k];
    // held off by the world only: a hand held back by a crate it pushes is not stuck, and put inside the crate it would
    // throw it
    if (!onWorld[side]) return refuse("held off by something loose, not by the world");
    if (TouchesWorld(side, to, s)) return refuse("tracking's place is in the world (pushed into it)");
    if (PalmInLoose(side, to)) return refuse("tracking's place is in a loose thing");
    char met[200] = "";
    {
        // from 6 cm out of the shoulder: the shoulder is inside the player, and a wall V stands with its back to met the
        // first ball and refused every hand (2026-11-09)
        float d[3], n = 0;
        for (int k = 0; k < 3; ++k) d[k] = palm[k] - shoulder[k], n += d[k] * d[k];
        n = std::sqrt(n);
        if (n > 0.12f)
            for (int k = 0; k < 3; ++k) shoulder[k] += d[k] * (0.06f / n);
    }
    if (!PathFree(shoulder, palm, met, sizeof(met))) {
        char why[400];
        std::snprintf(why, sizeof(why), "something between the shoulder (%.2f %.2f %.2f) and tracking's palm (%.2f %.2f %.2f): %s (PhysX's "
                      "space, the world's minus %.2f %.2f %.2f)",
                      shoulder[0] + origin[0], shoulder[1] + origin[1], shoulder[2] + origin[2], palm[0] + origin[0],
                      palm[1] + origin[1], palm[2] + origin[2], met, origin[0], origin[1], origin[2]);
        return refuse(why);
    }
    if (!distant) {
        // close: only when the straight way from the body's palm to tracking's crosses something (a thin ball, so a palm
        // merely pressed on a wall with tracking just off it is not "behind" it)
        float bodyPalm[3];
        QRot(now.q, off, r);
        for (int k = 0; k < 3; ++k) bodyPalm[k] = now.p[k] + r[k];
        if (PathFree(bodyPalm, palm, nullptr, 0, 0.008f)) return false;
    }
    Log("[physhands] %s hand stuck %.2f m off tracking for %llu ms behind something: put back where tracking is\n",
        side ? "right" : "left", std::sqrt(gap2), (unsigned long long)(tick - farSince[side]));
    farSince[side] = 0;
    return true;
}

// A capsule's pose (its axis x) and half height between two points.
float Between(const float* a, const float* b, PxTransform& out) {
    float d[3];
    for (int k = 0; k < 3; ++k) d[k] = b[k] - a[k], out.p[k] = 0.5f * (a[k] + b[k]);
    const float len = std::sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    if (len < 1e-4f) { out.q[0] = out.q[1] = out.q[2] = 0; out.q[3] = 1; return 0; }
    for (auto& v : d) v /= len;
    if (d[0] < -0.9999f) { out.q[0] = 0; out.q[1] = 0; out.q[2] = 1; out.q[3] = 0; }
    else { out.q[0] = 0; out.q[1] = -d[2]; out.q[2] = d[1]; out.q[3] = 1.f + d[0]; QNorm(out.q); }
    return 0.5f * len;
}
void SetCapsule(void* sh, float radius, float half, float& made, float& madeRadius, const PxTransform& local) {
    if (std::fabs(half - made) > 0.002f || std::fabs(radius - madeRadius) > 0.0003f) {
        const struct { int type; float radius, halfHeight; } g{kGeomCapsule, radius, half};
        At<void (*)(void*, const void*)>(sh, kShapeSetGeometry)(sh, &g);
        made = half;
        madeRadius = radius;
    }
    At<void (*)(void*, const PxTransform*)>(sh, kShapeSetLocalPose)(sh, &local);
}

// Each step, before it: the finger capsules where the fingers are drawn (the layers' pose and the worker's bends), the
// forearm's from the wrist toward the elbow; parked in the palm when there is nothing to pose them by.
void PoseShapes(int side, const Settings& s) {
    const uint64_t now = GetTickCount64();
    // what is posed, published as the function leaves (whichever way)
    struct Publish {
        BodyShapes b{};
        int side;
        ~Publish() { b.at = GetTickCount64(); bodyShapes[side].Write(b); }
    } pub{{}, side};
    const auto keep = [&](const void* geometry, size_t size, const PxTransform& local, float reach, int part) {
        if (pub.b.n >= kBodyShapes) return;
        BodyShape& e = pub.b.s[pub.b.n++];
        std::memcpy(e.geometry, geometry, size);
        e.local = local;
        e.reach = reach;
        e.part = uint8_t(part);
    };
    const auto capsule = [&](void* sh, float radius, float half, float& made, float& madeRadius, const PxTransform& local) {
        SetCapsule(sh, radius, half, made, madeRadius, local);
        const struct { int type; float radius, halfHeight; } g{kGeomCapsule, madeRadius, made};
        keep(&g, sizeof(g), local, made + madeRadius, (&made == &forearmHalf[side] || &made == &forearmUpperHalf[side]) ? 2 : 1);
    };
    // THE BOX, V'S OWN: the body is known some time after the hands are made (the rig's policy), and a change of it (a
    // new rig, the skin setting) is taken at once -- the box's size and place, and the mass's spread with it
    {
        float half[3], centre[3];
        const int skinOf = BoxWithMargin(side, s, half, centre);
        if ((skinOf != madeSkin[side] || std::fabs(half[0] - madeHalf[side][0]) > 1e-5f) && shape[side]) {
            const BoxGeometry g{kGeomBox, {half[0], half[1], half[2]}};
            At<void (*)(void*, const void*)>(shape[side], kShapeSetGeometry)(shape[side], &g);
            const PxTransform at{{0, 0, 0, 1}, {centre[0], centre[1], centre[2]}};
            At<void (*)(void*, const PxTransform*)>(shape[side], kShapeSetLocalPose)(shape[side], &at);
            std::memcpy(madeHalf[side], half, sizeof(half));
            std::memcpy(madeCentre[side], centre, sizeof(centre));
            madeAlong[side] = std::fabs(centre[0]);
            madeSkin[side] = skinOf;
            appliedMass[side] = -1.f;   // the inertia again, for the new box
            Log("[physhands] %s hand's palm fitted to %s skin: box %.3f x %.3f x %.3f m about (%.3f %.3f %.3f)\n", side ? "right" : "left",
                skinOf == 1 ? "a woman's" : (skinOf == 0 ? "a man's" : "no (the ini's box)"), 2 * half[0], 2 * half[1], 2 * half[2],
                centre[0], centre[1], centre[2]);
        }
    }
    const PxTransform palm{{0, 0, 0, 1}, {madeCentre[side][0], madeCentre[side][1], madeCentre[side][2]}};
    {
        const BoxGeometry g{kGeomBox, {madeHalf[side][0], madeHalf[side][1], madeHalf[side][2]}};
        keep(&g, sizeof(g), palm, std::sqrt(g.half[0] * g.half[0] + g.half[1] * g.half[1] + g.half[2] * g.half[2]), 0);
    }
    FingerHand h{};
    const bool chains = s.fingerShapes && fingerHands[side].Read(h) && now - h.at < 200;
    Bends b{};
    const bool bent = chains && bends[side].Read(b) && b.on && now - b.at < 250;
    for (int f = 0; f < 5; ++f) {
        const FingerChain& c = h.f[f];
        float J[4][3], Fq[3][4] = {{0, 0, 0, 1}, {0, 0, 0, 1}, {0, 0, 0, 1}};
        if (chains) {
            float pq[4], pp[3];
            std::memcpy(pq, c.base, sizeof(pq));
            std::memcpy(pp, c.base + 4, sizeof(pp));
            float F[4] = {0, 0, 0, 1};
            for (int j = 0; j < 3; ++j) {
                float turn[4], lq[4];
                AxisAngle(c.hinge[j], bent ? b.y[f][j] : 0.f, turn);
                QMul(c.local[j], turn, lq);
                float nq[4], np[3];
                Compose(pq, pp, lq, c.local[j] + 4, nq, np);
                std::memcpy(pq, nq, sizeof(pq));
                std::memcpy(pp, np, sizeof(pp));
                std::memcpy(J[j], np, sizeof(J[j]));
                std::memcpy(F, nq, sizeof(F));
                std::memcpy(Fq[j], nq, sizeof(Fq[j]));
            }
            float tip[3];
            QRot(F, c.tip, tip);
            for (int k = 0; k < 3; ++k) J[3][k] = pp[k] + tip[k];
        }
        for (int j = 0; j < 3; ++j) {
            void* sh = fingerShape[side][f][j];
            if (!sh) continue;
            PxTransform local = palm;
            float half = fingerHalf[side][f][j];
            float radius = 0.008f;
            if (chains && c.radius[j] > 0) {
                // the segment's line, moved off the bone to the skin's centre (in the bone's frame)
                float o[3], a[3], e[3];
                QRot(Fq[j], c.offset[j], o);
                for (int k = 0; k < 3; ++k) a[k] = J[j][k] + o[k], e[k] = J[j + 1][k] + o[k];
                half = Between(a, e, local);
                radius = c.radius[j] - 0.0005f;
            }
            capsule(sh, radius, half > 0.003f ? half : 0.003f, fingerHalf[side][f][j], fingerRadius[side][f][j], local);
        }
    }
    // THE GUN: its boxes where the script measured it in this hand; parked in the palm without one (or with a stale one)
    {
        Weapon wp{};
        const bool armed = s.weapon && g_hasWeaponEquipped && weapons[side].Read(wp) && wp.n > 0 && now - wp.at < 1500;
        int on = 0;
        for (int i = 0; i < kMostWeaponBoxes; ++i) {
            void* sh = weaponShape[side][i];
            if (!sh) continue;
            PxTransform local = palm;
            float half[3] = {0.004f, 0.004f, 0.004f};
            if (armed && i < wp.n) {
                float c[3];
                for (int k = 0; k < 3; ++k) c[k] = 0.5f * (wp.lo[i][k] + wp.hi[i][k]), half[k] = std::max(0.005f, 0.5f * (wp.hi[i][k] - wp.lo[i][k]));
                const float id[4] = {0, 0, 0, 1};
                Compose(wp.frame, wp.frame + 4, id, c, local.q, local.p);
                weaponLocal[side][on] = local;
                std::memcpy(weaponHalf[side][on], half, sizeof(half));
                ++on;
                const BoxGeometry kept{kGeomBox, {half[0], half[1], half[2]}};
                keep(&kept, sizeof(kept), local, std::sqrt(half[0] * half[0] + half[1] * half[1] + half[2] * half[2]), 3);
            }
            float* made = weaponMade[side][i];
            bool changed = false;
            for (int k = 0; k < 3; ++k) changed |= std::fabs(half[k] - made[k]) > 0.001f;
            if (changed) {
                const BoxGeometry g{kGeomBox, {half[0], half[1], half[2]}};
                At<void (*)(void*, const void*)>(sh, kShapeSetGeometry)(sh, &g);
            }
            if (changed) std::memcpy(made, half, sizeof(half));
            At<void (*)(void*, const PxTransform*)>(sh, kShapeSetLocalPose)(sh, &local);
        }
        weaponOn[side] = on;
    }
    void* fa = forearmShape[side];
    void* fu = forearmUpper[side];
    if (!fa || !fu) return;
    Forearm arm{};
    if (s.forearm && forearms[side].Read(arm) && now - arm.at < 200) {
        const float len = std::sqrt(arm.elbow[0] * arm.elbow[0] + arm.elbow[1] * arm.elbow[1] + arm.elbow[2] * arm.elbow[2]);
        if (len > 0.15f) {
            const HandSkin* k = s.skin ? SkinFor(g_VRBodyFemale, side) : nullptr;
            float a[3], m[3], e[3];
            PxTransform local{};
            if (k) {
                // V's own forearm, which tapers to the wrist: 3-11 cm from it, and from 11 cm to 4 cm short of the elbow
                for (int i = 0; i < 3; ++i)
                    a[i] = arm.elbow[i] * (0.03f / len), m[i] = arm.elbow[i] * (0.11f / len), e[i] = arm.elbow[i] * ((len - 0.04f) / len);
                float half = Between(a, m, local);
                capsule(fa, k->foreLower + s.skinMargin, half > 0.01f ? half : 0.01f, forearmHalf[side], forearmRadius[side][0], local);
                half = Between(m, e, local);
                capsule(fu, k->foreUpper + s.skinMargin, half > 0.01f ? half : 0.01f, forearmUpperHalf[side], forearmRadius[side][1], local);
                return;
            }
            // from 6 cm up the forearm (the palm's box has the wrist) to 5 cm short of the elbow
            for (int i = 0; i < 3; ++i) a[i] = arm.elbow[i] * (0.06f / len), e[i] = arm.elbow[i] * ((len - 0.05f) / len);
            const float half = Between(a, e, local);
            capsule(fa, 0.035f, half > 0.01f ? half : 0.01f, forearmHalf[side], forearmRadius[side][0], local);
            capsule(fu, 0.01f, 0.01f, forearmUpperHalf[side], forearmRadius[side][1], palm);
            return;
        }
    }
    capsule(fa, 0.035f, forearmHalf[side], forearmHalf[side], forearmRadius[side][0], palm);
    capsule(fu, 0.01f, 0.01f, forearmUpperHalf[side], forearmRadius[side][1], palm);
}

void Compose(const float* aq, const float* ap, const float* bq, const float* bp, float* oq, float* op) {
    float r[3];
    QRot(aq, bp, r);
    for (int k = 0; k < 3; ++k) op[k] = ap[k] + r[k];
    QMul(aq, bq, oq);
    QNorm(oq);
}
void AxisAngle(const float* axis, float a, float* q) {
    const float s = std::sin(0.5f * a);
    q[0] = axis[0] * s; q[1] = axis[1] * s; q[2] = axis[2] * s; q[3] = std::cos(0.5f * a);
}
void Cross(const float* a, const float* b, float* o) {
    o[0] = a[1] * b[2] - a[2] * b[1];
    o[1] = a[2] * b[0] - a[0] * b[2];
    o[2] = a[0] * b[1] - a[1] * b[0];
}

// A finger's chain posed with joint turns b from the hand's frame (bq, bp in the world): each joint and the tip (J), each
// bone's frame (F), each joint's axis in the world.
void PoseChain(const FingerChain& c, const float* bq, const float* bp, const float* b, float J[4][3], float F[3][7], float axis[3][3]) {
    float pq[4], pp[3];
    std::memcpy(pq, bq, sizeof(pq));
    std::memcpy(pp, bp, sizeof(pp));
    for (int j = 0; j < 3; ++j) {
        float turn[4], lq[4];
        AxisAngle(c.hinge[j], b[j], turn);
        QMul(c.local[j], turn, lq);
        Compose(pq, pp, lq, c.local[j] + 4, F[j], F[j] + 4);
        std::memcpy(pq, F[j], sizeof(pq));
        std::memcpy(pp, F[j] + 4, sizeof(pp));
        std::memcpy(J[j], F[j] + 4, sizeof(J[j]));
        QRot(F[j], c.hinge[j], axis[j]);
    }
    float tip[3];
    QRot(F[2], c.tip, tip);
    for (int k = 0; k < 3; ++k) J[3][k] = F[2][4 + k] + tip[k];
}

// Each segment of a posed chain against what is near: its push out (summed), where it is pushed, and the deepest of all.
float Segments(const FingerChain& c, const float J0[4][3], const float F[3][7], const NearList& L, int& calls, int* contacts, float push[3][3],
               float at[3][3]) {
    float deepest = 0;
    for (int s = 0; s < 3; ++s) {
        if (push) push[s][0] = push[s][1] = push[s][2] = 0;
        if (c.radius[s] <= 0) continue;
        // the segment's line, off the bone to the skin's centre (Natives/HandSkin.hpp)
        float o[3] = {0, 0, 0}, J[2][3];
        if (F) QRot(F[s], c.offset[s], o);
        for (int k = 0; k < 3; ++k) J[0][k] = J0[s][k] + o[k], J[1][k] = J0[s + 1][k] + o[k];
        float d[3];
        for (int k = 0; k < 3; ++k) d[k] = J[1][k] - J[0][k];
        const float len = std::sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
        if (len < 1e-4f) continue;
        for (auto& v : d) v /= len;
        struct { int type; float radius, halfHeight; } cap{kGeomCapsule, c.radius[s], 0.5f * len};
        PxTransform pose{};
        // the capsule's axis is its x: the shortest turn from x to the segment
        if (d[0] < -0.9999f) { pose.q[0] = 0; pose.q[1] = 0; pose.q[2] = 1; pose.q[3] = 0; }
        else { pose.q[0] = 0; pose.q[1] = -d[2]; pose.q[2] = d[1]; pose.q[3] = 1.f + d[0]; QNorm(pose.q); }
        float lo[3], hi[3];
        for (int k = 0; k < 3; ++k) {
            pose.p[k] = 0.5f * (J[0][k] + J[1][k]);
            lo[k] = std::min(J[0][k], J[1][k]) - c.radius[s];
            hi[k] = std::max(J[0][k], J[1][k]) + c.radius[s];
            if (at) at[s][k] = J[0][k] + 0.7f * (J[1][k] - J[0][k]);
        }
        for (int i = 0; i < L.n && calls < 1400; ++i) {
            const NearShape& e = L.s[i];
            if (hi[0] < e.bounds.lo[0] || lo[0] > e.bounds.hi[0] || hi[1] < e.bounds.lo[1] || lo[1] > e.bounds.hi[1] ||
                hi[2] < e.bounds.lo[2] || lo[2] > e.bounds.hi[2])
                continue;
            ++calls;
            float dir[3]{}, depth = 0;
            if (!Penetration(dir, &depth, &cap, &pose, e) || !(depth > 0.0005f) || !std::isfinite(depth)) continue;
            if (push) for (int k = 0; k < 3; ++k) push[s][k] += dir[k] * depth;
            if (contacts) ++*contacts;
            deepest = std::max(deepest, depth);
        }
    }
    return deepest;
}

// A FINGER IS PUSHED OUT OF THE WORLD BY ITS JOINTS. Each segment is a capsule; PhysX says how deep each is in what is
// near and which way out. Each joint turns by the share of every push past it that its turn can give (a Jacobian's
// transpose, each joint's column normalised), the far joints yielding more than the knuckle -- a fingertip on a table
// bends back at its last joint before the finger lifts -- and never past the joint's anatomical range. A few rounds.
// THEN, WITH THE PALM ON SOMETHING (wrap), IT LIES ON IT: the finger curls toward the palm until a segment touches -- at
// once on a flat wall, where it stays as flat as the push left it, round the edge of a table the palm is on -- and no
// further than a relaxed hand's wrap. Not the thumb: it stays on top of an edge.
// A PALM ON SOMETHING OPENS THE HAND FIRST: the animation's hand is often half closed (no finger tracking), and pressed on
// a wall it stayed a loose fist, its knuckles on the wall. From the open hand the fingers yield and wrap as above, so
// they lie flat on a wall, round an edge, and hang over a drop.
bool wrapProbe = false;   // trace=1: the next SolveFinger logs its finger's depth along the whole wrap (worker thread)
// THE OPEN HAND from the chain as the animation has it: each joint turned back about its hinge by the angle its segment
// makes with the one before (the first with the palm's own line), within its range.
// The skin's margin as the fingers' solve has it (physics thread): a capsule grown by it may be that far in a thing with
// the skin itself still out, and the checks for a finger left in something allow it.
float fingerSlack = 0.002f;

void OpenAngles(const FingerChain& c, const float* hq, const float* hp, const float* bq, const float* bp, float* a) {
    a[0] = a[1] = a[2] = 0;
    float F[3][7], J[4][3], axis[3][3];
    PoseChain(c, bq, bp, a, J, F, axis);
    float prev[3];
    for (int k = 0; k < 3; ++k) prev[k] = J[0][k] - hp[k];   // the palm's own line: wrist to this finger's knuckle
    {
        // ...laid in the plane of the palm's face (the box's thickness axis, the hand frame's z, taken out): the wrist to
        // knuckle line leans off it, so the fingers opened along it stood off a wall the palm lay flat on and only their
        // curled tips touched it (2026-11-09)
        const float zAxis[3] = {0, 0, 1};
        float n[3];
        QRot(hq, zAxis, n);
        const float d = prev[0] * n[0] + prev[1] * n[1] + prev[2] * n[2];
        for (int k = 0; k < 3; ++k) prev[k] -= d * n[k];
    }
    for (int j = 0; j < 3; ++j) {
        float seg[3], cr[3];
        for (int k = 0; k < 3; ++k) seg[k] = J[j + 1][k] - J[j][k];
        Cross(prev, seg, cr);
        const float angle = std::atan2(cr[0] * axis[j][0] + cr[1] * axis[j][1] + cr[2] * axis[j][2],
                                       prev[0] * seg[0] + prev[1] * seg[1] + prev[2] * seg[2]);
        // only ever straightened: a joint the measure finds bent backward (the fingertip's point is not always on the
        // bone's line) is left as the animation has it rather than curled by that much
        if (c.moves[j] && std::isfinite(angle)) a[j] = std::clamp(std::min(0.f, -angle), c.lo[j], c.hi[j]);
        std::memcpy(prev, seg, sizeof(prev));
    }
}

void SolveFinger(const FingerChain& c, const float* hq, const float* hp, const NearList& L, bool wrap, float* out, float* yielded, float* opened,
                 int& calls, int& contacts, float squeeze = 0.f, float* wrapT = nullptr) {
    if (wrapT) *wrapT = -1.f;
    static const float kWrap[3] = {1.1f, 1.5f, 0.9f};      // rad at the full wrap: knuckle, middle joint, last joint
    // how far a finger pressed on its pad bends back past straight at most (each within its anatomical range): the
    // knuckle and the last joint give, the middle joint hardly (its volar plate)
    static const float kBack[3] = {0.35f, 0.10f, 0.30f};
    float bq[4], bp[3];
    Compose(hq, hp, c.base, c.base + 4, bq, bp);
    float a[3] = {0, 0, 0}, open[3];
    OpenAngles(c, hq, hp, bq, bp, open);
    std::memcpy(opened, open, sizeof(open));
    const auto depthOf = [&](const float* b) {
        float F[3][7], J[4][3], axis[3][3];
        PoseChain(c, bq, bp, b, J, F, axis);
        return Segments(c, J, F, L, calls, nullptr, nullptr, nullptr);
    };
    const float inMost = fingerSlack + 0.0015f;
    // A FINGER MOVES ON ONE CURL, never joint by joint (HIGGS's FingerAnimator: one value per finger, each joint on the
    // way between the open and the closed pose): from the open hand (straight) to the animation's pose, and past the open
    // hand bent back a little. Pushed out joint by joint (each joint turned by the share of the push its lever gave), a
    // finger's joints went their own ways -- the last ones the wrong way and, about a hinge not quite the bone's, sideways.
    const auto back = [&](float v, float* b) {
        for (int j = 0; j < 3; ++j) b[j] = c.moves[j] ? std::max(c.lo[j], open[j] - kBack[j] * v) : 0.f;
    };
    const auto pressedOut = [&](float* b) {
        // the open hand in it too: bent back by the least that has it out, or all the way
        float lo = 0, hi = 1, t[3];
        back(1, t);
        if (depthOf(t) > inMost) {
            std::memcpy(b, t, sizeof(t));
            return;
        }
        for (int it = 0; it < 5; ++it) {
            const float mid = 0.5f * (lo + hi);
            back(mid, t);
            (depthOf(t) > inMost ? lo : hi) = mid;
        }
        back(hi, b);
    };
    // ...and the other way, curled on from the animation's pose toward a fist by the least that has it out: a finger met on
    // its back (its nail's side) gives toward the palm, as a finger does (2026-11-10: a finger brushed on its back went
    // straight into what it met, opening only took it deeper). False when no curl within the joints' ranges does it.
    const auto curledOut = [&](float* b) {
        static const float kCurl[3] = {1.1f, 1.5f, 0.9f};   // as the wrap's full curl
        const auto curlAt = [&](float v, float* t) {
            for (int j = 0; j < 3; ++j) t[j] = c.moves[j] ? std::clamp(kCurl[j] * v, c.lo[j], c.hi[j]) : 0.f;
        };
        float t[3], lo = 0, hi = -1;
        for (float v = 0.1f; v < 1.25f; v += 0.1f) {
            curlAt(v, t);
            if (!(depthOf(t) > inMost)) { hi = v; break; }
            lo = v;
        }
        if (hi < 0) return false;
        for (int it = 0; it < 4; ++it) {
            const float mid = 0.5f * (lo + hi);
            curlAt(mid, t);
            (depthOf(t) > inMost ? lo : hi) = mid;
        }
        curlAt(hi, b);
        return true;
    };
    if (wrap) {
        std::memcpy(a, open, sizeof(a));
        if (depthOf(open) > inMost) pressedOut(a), ++contacts;
    } else {
        const float zero[3] = {0, 0, 0};
        if (depthOf(zero) > inMost) {
            ++contacts;
            if (depthOf(open) > inMost) {
                // the open hand in it too: bent back past straight (a fingertip pressed on its pad) or curled on (a finger
                // met on its back), whichever is out, and of both the one that turns the finger less
                float backB[3], curlB[3];
                pressedOut(backB);
                const bool curlOk = curledOut(curlB);
                const bool backOk = !(depthOf(backB) > inMost);
                float backCost = 0, curlCost = 0;
                for (int j = 0; j < 3; ++j) backCost += std::fabs(backB[j]), curlCost += std::fabs(curlB[j]);
                const bool curl = curlOk && (!backOk || curlCost < backCost);
                std::memcpy(a, curl ? curlB : backB, sizeof(a));
            } else {
                // the most curl of the way from the open hand that is out of what is near (HIGGS keeps the least curled
                // first contact of its three probes): halved toward the animation's pose
                float lo = 0, hi = 1, t[3];
                for (int it = 0; it < 6; ++it) {
                    const float mid = 0.5f * (lo + hi);
                    for (int j = 0; j < 3; ++j) t[j] = open[j] * (1.f - mid);
                    (depthOf(t) > inMost ? hi : lo) = mid;
                }
                for (int j = 0; j < 3; ++j) a[j] = open[j] * (1.f - lo);
            }
        }
    }
    std::memcpy(yielded, a, sizeof(a));
    if (wrap) {
        // THE WRAP'S PATH, knuckle first: over t 0..1 the knuckle alone (a straight finger tips onto a wall or a table it
        // lies on, so the hand lies flat), over 1..2 the middle and last joints (a finger whose straight reach meets
        // nothing, over an edge, curls on round it). Curled all three at once, a palm flat on a wall touched it with
        // bent fingertips only (2026-11-09).
        const auto bendAt = [&](float t, float* b) {
            const float w[3] = {std::min(t, 1.f), std::max(0.f, t - 1.f), std::max(0.f, t - 1.f)};
            for (int j = 0; j < 3; ++j) b[j] = c.moves[j] ? std::clamp(a[j] + w[j] * kWrap[j], c.lo[j], c.hi[j]) : a[j];
        };
        const auto touchesAt = [&](float t) {
            float b[3], F[3][7], J[4][3], axis[3][3];
            bendAt(t, b);
            PoseChain(c, bq, bp, b, J, F, axis);
            return Segments(c, J, F, L, calls, nullptr, nullptr, nullptr) > 0.0015f;
        };
        if (wrapProbe) {
            wrapProbe = false;
            char line[520];
            int n = std::snprintf(line, sizeof(line), "[physhands] right middle finger along its wrap from the open hand (%.2f %.2f %.2f, hi %.2f %.2f %.2f), mm in at t 0..2 (knuckle to 1):",
                                  a[0], a[1], a[2], c.hi[0], c.hi[1], c.hi[2]);
            for (int i = 0; i <= 20 && n > 0 && n < int(sizeof(line)) - 8; ++i) {
                float b[3], F[3][7], J[4][3], axis[3][3];
                bendAt(0.1f * i, b);
                PoseChain(c, bq, bp, b, J, F, axis);
                int dummy = 0;
                n += std::snprintf(line + n, sizeof(line) - n, " %.1f", 1000.f * Segments(c, J, F, L, dummy, nullptr, nullptr, nullptr));
            }
            Log("%s\n", line);
        }
        if (wrapT) *wrapT = 0.06f;
        if (!touchesAt(0.06f)) {
            // THE FIRST TOUCH ON THE WAY IN, stepped for and then halved between the last step clear and the first that
            // touches. Not halved over the whole wrap: a fingertip curled all the way comes back up off a table (a fist),
            // so "touches when curled all the way" said no and the fingers of a palm on the floor hung in the air.
            // A finger that meets nothing however far it curls (past the edge of a step, over a drop) hangs: a share of
            // the wrap, as a relaxed finger droops over an edge. Curled all the way it closed into a fist on the curb.
            // Past the wrap, on to the joints' anatomical ends: knuckles already over a railing's far edge reach its far
            // face only curled near a fist's knuckle angle; at the wrap's end they still hung in the air (2026-11-09).
            // Steps short enough that a fingertip moves less than its capsule's radius: a mesh answers only where a
            // capsule crosses its triangles, and an eighth of the wrap carried the tip through a railing's face from
            // outside to inside in one step.
            constexpr float kDrape = 0.35f, kStep = 0.03f, kMost = 2.0f;
            float lo = 0.06f, hi = -1.f;
            for (float t = 0.06f + kStep; t < kMost + 0.5f * kStep; t += kStep) {
                const float at = std::min(t, kMost);
                if (touchesAt(at)) { hi = at; break; }
                lo = at;
            }
            if (hi < 0) {
                // nothing met: the relaxed droop, all three joints a share of the wrap
                for (int j = 0; j < 3; ++j)
                    if (c.moves[j]) a[j] = std::clamp(a[j] + kDrape * kWrap[j], c.lo[j], c.hi[j]);
            } else {
                for (int it = 0; it < 4; ++it) {
                    const float mid = 0.5f * (lo + hi);
                    (touchesAt(mid) ? hi : lo) = mid;
                }
                float b[3];
                bendAt(lo, b);
                std::memcpy(a, b, sizeof(a));
                if (wrapT) *wrapT = lo;
            }
            if (hi < 0 && wrapT) *wrapT = -1.f;
        }
        if (squeeze > 0) {
            // THE GRIP SHOWS: closed on what the palm is on, the fingers press on -- on a flat face the fingertips dig in
            // (the knuckle lifts a little, the middle and last joints bend), round an edge they curl tighter. The last
            // word below keeps them out of it.
            static const float kSqueeze[3] = {-0.2f, 0.6f, 0.35f};
            for (int j = 0; j < 3; ++j)
                if (c.moves[j]) a[j] = std::clamp(a[j] + kSqueeze[j] * squeeze, c.lo[j], c.hi[j]);
        }
        // THE LAST WORD: a finger the wrap left in something is turned back toward the open hand (laid in the
        // palm's plane, clear of what the palm is on) by the least of the way that has it out. A grip closed on a
        // railing with the fingertips at its far edge carried the solve 11-14 mm into it for a fifth of a second
        // (2026-11-09).
        // ...or, the open hand in it too (laid in the palm's plane through the knuckles, a finger's flesh reaches a few
        // millimetres under the palm's face), toward the animation's own pose: a fist closed on a table is clear of it
        // (in = the skin in it, past the margin the capsules are grown by: a flat finger on a table the palm is pressed on
        // reads 2-3 mm in by its grown capsule and is right where it is; taken for "in", it was turned back to the
        // animation's curl and the hand pressed on a wall showed a claw, its fingertips hidden in it)
        const float deep = depthOf(a);
        if (deep > inMost) {
            const float zero[3] = {0, 0, 0};
            const float openDeep = depthOf(open);
            const float* to = openDeep < deep ? open : nullptr;
            if (openDeep > inMost && depthOf(zero) < std::min(deep, openDeep)) to = zero;
            if (to) {
                float lo = 0, hi = 1;
                for (int it = 0; it < 6; ++it) {
                    const float mid = 0.5f * (lo + hi);
                    float b[3];
                    for (int j = 0; j < 3; ++j) b[j] = a[j] + (to[j] - a[j]) * mid;
                    (depthOf(b) > inMost ? lo : hi) = mid;
                }
                for (int j = 0; j < 3; ++j) a[j] += (to[j] - a[j]) * hi;
            }
        }
    }
    std::memcpy(out, a, sizeof(a));
}

// THE PALM ON SOMETHING, on its own side: the palm's box grown by a centimetre in a shape near, the way out of it backward
// against the palm's facing (the fingers' curl). The back of the hand on a wall is not the palm on it: the fingers would
// curl into the air.
float PalmDepth(int side, const float* hq, const float* hp, const NearList& L, int& calls);
bool PalmOn(int side, const float* hq, const float* hp, const float* facing, const NearList& L, int& calls, bool held) {
    // the palm moved on a centimetre and a half the way it faces goes deeper into what is there. A mesh gives one way out
    // for the whole box: on a railing's top with the heel of the hand over its near edge that way was sideways, the test
    // below said "the back of the hand", and the hand stayed the animation's loose fist on the rail (2026-11-09).
    // HELD OFF BY ITS FINGERTIPS: a hand the animation holds half closed meets a wall it is pushed at palm first with its
    // curled fingertips, which stand 3-4 cm proud of the palm; the body stops on them, the palm never reaches the wall, and
    // the fingers stayed curled (their tips pushed back by curling them further). So with the body held by something,
    // what is in front of the palm within 4.5 cm counts as the palm on it: the hand opens and the fingers lie on it.
    {
        const float here = PalmDepth(side, hq, hp, L, calls);
        const float reach[3] = {0.015f, 0.03f, 0.045f};
        for (int i = 0; i < (held ? 3 : 1); ++i) {
            float on[3];
            for (int k = 0; k < 3; ++k) on[k] = hp[k] + facing[k] * reach[i];
            if (PalmDepth(side, hq, on, L, calls) > here + 0.004f) return true;
        }
    }
    const BoxGeometry box{kGeomBox, {madeHalf[side][0] + 0.01f, madeHalf[side][1] + 0.01f, madeHalf[side][2] + 0.01f}};
    const float off[3] = {madeCentre[side][0], madeCentre[side][1], madeCentre[side][2]};
    PxTransform pose{};
    float r[3];
    QRot(hq, off, r);
    for (int k = 0; k < 3; ++k) pose.p[k] = hp[k] + r[k];
    std::memcpy(pose.q, hq, sizeof(pose.q));
    const float reach = std::sqrt(box.half[0] * box.half[0] + box.half[1] * box.half[1] + box.half[2] * box.half[2]);
    for (int i = 0; i < L.n && calls < 1400; ++i) {
        const NearShape& e = L.s[i];
        bool apart = false;
        for (int k = 0; k < 3 && !apart; ++k) apart = pose.p[k] + reach < e.bounds.lo[k] || pose.p[k] - reach > e.bounds.hi[k];
        if (apart) continue;
        ++calls;
        float dir[3]{}, depth = 0;
        if (Penetration(dir, &depth, &box, &pose, e) && depth > 0 && dir[0] * facing[0] + dir[1] * facing[1] + dir[2] * facing[2] < -0.4f)
            return true;
    }
    return false;
}

// How deep the palm's own box is in what is near, the hand at (hq, hp).
float PalmDepth(int side, const float* hq, const float* hp, const NearList& L, int& calls) {
    const BoxGeometry box{kGeomBox, {madeHalf[side][0], madeHalf[side][1], madeHalf[side][2]}};
    const float off[3] = {madeCentre[side][0], madeCentre[side][1], madeCentre[side][2]};
    PxTransform pose{};
    float r[3];
    QRot(hq, off, r);
    for (int k = 0; k < 3; ++k) pose.p[k] = hp[k] + r[k];
    std::memcpy(pose.q, hq, sizeof(pose.q));
    const float reach = std::sqrt(box.half[0] * box.half[0] + box.half[1] * box.half[1] + box.half[2] * box.half[2]);
    float deepest = 0;
    for (int i = 0; i < L.n && calls < 1400; ++i) {
        const NearShape& e = L.s[i];
        bool apart = false;
        for (int k = 0; k < 3 && !apart; ++k) apart = pose.p[k] + reach < e.bounds.lo[k] || pose.p[k] - reach > e.bounds.hi[k];
        if (apart) continue;
        ++calls;
        float dir[3]{}, depth = 0;
        if (Penetration(dir, &depth, &box, &pose, e) && std::isfinite(depth)) deepest = std::max(deepest, depth);
    }
    return deepest;
}

void SolveFingers(const Step& st, const Settings& s, double dtS) {
    static float shown[2][5][3] = {}, shownYield[2][5][3] = {}, shownV[2][5][3] = {}, shownYieldV[2][5][3] = {};
    const int front = nearFront.load(std::memory_order_acquire);
    LARGE_INTEGER t0;
    QueryPerformanceCounter(&t0);
    AcquireSRWLockShared(&nearLock);
    const NearList* L0 = front >= 0 ? &nearLists[nearFront.load(std::memory_order_acquire)] : nullptr;
    // ...and the people near and the loose things by the hands, posed this step, V's own body while a hand meets it and
    // the other hand while the two meet: the world's list and theirs, one list a hand (a finger that brushes a bottle or a
    // crate bends on it as on a wall; a palm on a crate lays its fingers on it, on the other hand, on V's chest)
    const NearList* lists[2] = {L0, L0};
    if (L0) {
        static NearList merged[2];
        static PeopleNear pn;
        static LooseShapesNear ln;
        static NearShape extra[kMostBodies + kBodyShapes];
        const uint64_t nowT = GetTickCount64();
        const bool peopleOk = peopleNear.Read(pn) && pn.n > 0 && nowT - pn.at < 300;
        const int looseCount = looseShapesNear.Read(ln) && ln.n > 0 && nowT - ln.at < 300 ? ln.n : 0;
        for (int side = 0; side < 2; ++side) {
            int ne = 0;
            if (peopleOk)
                for (int i = 0; i < pn.n && i < kMostBodies; ++i)
                    if ((pn.meets[i] >> side & 1) && (pn.owner[i] != kSelfOwner || selfOnShown[side].load(std::memory_order_relaxed)))
                        extra[ne++] = pn.s[i];
            static BodyShapes otherScratch;   // the worker's
            if (handsMeetShown.load(std::memory_order_relaxed) && st.valid[1 - side])
                ne += OtherHandShapes(1 - side, st.pose[1 - side], extra + ne, kBodyShapes, &otherScratch);
            if (ne + looseCount == 0) continue;
            NearList& M = merged[side];
            int at = std::max(0, std::min(L0->n, kMostNear - ne - looseCount));
            // the world's part copied only when it is another list (a list is never written while it is the front one):
            // V's own body is always there, and the whole list a hand a step was 2 x 200 kB
            static const NearList* copiedFrom[2] = {nullptr, nullptr};
            static uint64_t copiedAt[2] = {0, 0};
            static int copiedN[2] = {-1, -1};
            if (copiedFrom[side] != L0 || copiedAt[side] != L0->at || copiedN[side] != at) {
                std::memcpy(M.s, L0->s, sizeof(NearShape) * size_t(at));
                copiedFrom[side] = L0;
                copiedAt[side] = L0->at;
                copiedN[side] = at;
            }
            const int nl = std::max(0, std::min(looseCount, kMostNear - at));
            if (nl) std::memcpy(M.s + at, ln.s, sizeof(NearShape) * size_t(nl));
            at += nl;
            const int nx = std::max(0, std::min(ne, kMostNear - at));
            if (nx) std::memcpy(M.s + at, extra, sizeof(NearShape) * size_t(nx));
            M.n = at + nx;
            std::memcpy(M.center, L0->center, sizeof(M.center));
            M.at = L0->at;
            lists[side] = &M;
        }
    }
    int totalCalls = 0, contacts = 0;
    fingerSlack = s.skinMargin;
    for (int side = 0; side < 2; ++side) {
        const NearList* L = lists[side];
        // A BUDGET OF QUERIES PER HAND, AND ONE OF ITS OWN FOR PUTTING THE EASED FINGERS OUT. A query past the budget
        // answers "clear" (Segments, PalmDepth: at most 1400 a call count). Shared by both hands and every stage, it ran
        // out in a wrist turned quickly on a wall, and the last check -- the one that keeps a drawn finger out -- read
        // fingers 10-21 mm in the wall as clear (2026-11-09; ~0.17 us a query, 1100 a step at the most on average).
        int calls = 0, putCalls = 0;
        FingerHand h{};
        float want[5][3] = {}, wantYield[5][3] = {}, wantOpen[5][3] = {};
        const bool on = s.fingers && penetration && L && st.valid[side] && live[side].load(std::memory_order_acquire) &&
                        fingerHands[side].Read(h) && h.free && GetTickCount64() - h.at < 200 && GetTickCount64() - L->at < 5000;
        if (on) {
            const float* hq = st.pose[side];
            float hp[3];
            std::memcpy(hp, st.pose[side] + 4, sizeof(hp));
            // PRESSED, THE FINGERS GIVE. Their capsules are on the body, so a hand put down palm first stood on its
            // fingertips: PhysX kept them out of the table and the solver, posed where the body is, found nothing to bend.
            // They are solved where the hand would be held by its palm alone -- moved on toward where tracking sent it
            // (at most 5 cm) until the palm's box meets what is there -- so they lie flat on a table the hand is pressed
            // on, the capsules posed so let the body down onto the palm, and round an edge they curl over it.
            {
                float r[3], rn = 0;
                for (int k = 0; k < 3; ++k) r[k] = st.sent[side][4 + k] - hp[k], rn += r[k] * r[k];
                rn = std::sqrt(rn);
                if (rn > 0.002f) {
                    const float most = std::min(rn, 0.05f) / rn;
                    for (auto& v : r) v *= most;
                    const auto palmIn = [&](float t) {
                        float p[3];
                        for (int k = 0; k < 3; ++k) p[k] = hp[k] + r[k] * t;
                        return PalmDepth(side, hq, p, *L, calls) > 0.0015f;
                    };
                    // the FIRST place on the way where the palm meets something: a mesh answers only where the box
                    // crosses its triangles, so a palm sunk right through a railing's thin top read as meeting nothing
                    // and was posed all the way down (2026-11-09, the H10 atrium railing)
                    float lo = 0, hi = 1;
                    if (!palmIn(0)) {
                        constexpr int kSteps = 8;
                        int met = 0;
                        for (int i = 1; i <= kSteps && !met; ++i)
                            if (palmIn(float(i) / kSteps)) met = i;
                        if (!met) lo = hi;
                        else {
                            lo = float(met - 1) / kSteps;
                            hi = float(met) / kSteps;
                            for (int it = 0; it < 4; ++it) {
                                const float mid = 0.5f * (lo + hi);
                                (palmIn(mid) ? hi : lo) = mid;
                            }
                        }
                    }
                    for (int k = 0; k < 3; ++k) hp[k] += r[k] * lo;
                    if (lo > 0) palmShifted.fetch_add(1, std::memory_order_relaxed);
                }
            }
            // THE PALM'S FACING: the plane of the palm (the wrist to the middle knuckle, the little knuckle to the index
            // knuckle), turned to the side a straight middle finger curls toward about its knuckle's hinge. Asked of the
            // middle tip's own move as it curled, a hand the animation holds as a loose fist answered "back along the
            // palm", and a palm flat on a railing was not on it (2026-11-09).
            bool wrap = false;
            float palmFacing[3] = {0, 0, 0};
            bool haveFacing = false;
            if (s.conform) {
                const float zero[3] = {0, 0, 0};
                float J[3][4][3], axis[3][3][3], F[3][7];
                const int which[3] = {1, 0, 3};   // middle, index, little
                for (int i = 0; i < 3; ++i) {
                    const FingerChain& c = h.f[which[i]];
                    float bq[4], bp[3];
                    Compose(hq, hp, c.base, c.base + 4, bq, bp);
                    PoseChain(c, bq, bp, zero, J[i], F, axis[i]);
                }
                float along[3], across[3], facing[3], move[3];
                for (int k = 0; k < 3; ++k) along[k] = J[0][0][k] - hp[k], across[k] = J[1][0][k] - J[2][0][k];
                Cross(along, across, facing);
                Cross(axis[0][0], along, move);
                float n = 0, d = 0;
                for (int k = 0; k < 3; ++k) n += facing[k] * facing[k], d += facing[k] * move[k];
                n = std::sqrt(n);
                if (n > 1e-6f && std::isfinite(d)) {
                    const float sign = d < 0 ? -1.f : 1.f;
                    for (auto& v : facing) v *= sign / n;
                    std::memcpy(palmFacing, facing, sizeof(palmFacing));
                    haveFacing = true;
                    // HELD A MOMENT: the touch flickers off for a step as a hand rests on a wall (its 60 ms window), and
                    // with it the palm-on test's reach; the fingers went back to the animation's curl for that step, into
                    // the wall, and open again (2026-11-09: 86 deg out and back in two frames). The body's touch counts
                    // for 100 ms, and a palm found on something stays on it 150 ms: at 250 ms the fingers of a hand lifted
                    // off a railing still reached down to it, which read as a palm drawn to the rail.
                    static uint64_t touchedAt[2] = {0, 0}, wrapUntil[2] = {0, 0};
                    const uint64_t nowT = GetTickCount64();
                    if (st.touching[side]) touchedAt[side] = nowT;
                    wrap = PalmOn(side, hq, hp, facing, *L, calls, nowT - touchedAt[side] < 100);
                    if (wrap) wrapUntil[side] = nowT + 150;
                    else wrap = nowT < wrapUntil[side];
                    if (wrap) palmWraps.fetch_add(1, std::memory_order_relaxed);
                }
            }
            static uint64_t wrapProbeAt = 0;
            const bool gripping = g_pSharedHands && g_pSharedHands[side ? 49 : vrshared::kLeftGripPressed] > 0.5f;
            float middleWrap = -1.f;
            for (int f = 0; f < 5; ++f) {
                if (s.trace && side == 1 && f == 1 && wrap && GetTickCount64() - wrapProbeAt > 2000) wrapProbeAt = GetTickCount64(), wrapProbe = true;
                SolveFinger(h.f[f], hq, hp, *L, wrap && f != 4, want[f], wantYield[f], wantOpen[f], calls, contacts,
                            gripping && wrap && f != 4 ? 1.f : 0.f, f == 1 ? &middleWrap : nullptr);
                wrapProbe = false;
            }
            // past the knuckle's share of the wrap before it met something: the fingers curl over an edge (a railing's
            // top, a ledge), not lie on a face
            edgeWrap[side].store(wrap && middleWrap > 1.1f, std::memory_order_relaxed);
            // THE BACK OF THE HAND PRESSED ON SOMETHING (the knuckles on a wall, a table): the fingers straighten a little
            // with the press, as a relaxed hand's do when its back is pushed flat -- from 4 mm held off to 4.4 cm, up to 35%
            // of the way to the open hand. The way out of the surface is where the body was refused (its place less where
            // tracking sent it); the back is on it when the palm faces that way, out of the surface.
            if (haveFacing && !wrap && st.onWorld[side]) {
                float r[3], rn = 0;
                for (int k = 0; k < 3; ++k) r[k] = st.pose[side][4 + k] - st.sent[side][4 + k], rn += r[k] * r[k];
                rn = std::sqrt(rn);
                const float back = rn > 1e-4f ? (r[0] * palmFacing[0] + r[1] * palmFacing[1] + r[2] * palmFacing[2]) / rn : 0.f;
                if (back > 0.5f && rn > 0.004f) {
                    const float k = 0.35f * std::clamp((rn - 0.004f) / 0.04f, 0.f, 1.f) * std::clamp((back - 0.5f) / 0.3f, 0.f, 1.f);
                    for (int f = 0; f < 5; ++f)
                        for (int j = 0; j < 3; ++j)
                            if (wantOpen[f][j] < 0) want[f][j] = std::min(want[f][j], wantOpen[f][j] * k);
                    backPressed.fetch_add(1, std::memory_order_relaxed);
                }
            }
            // trace=1: every 2 s, how deep each finger of the right hand is in what is near, curled 0, half and all the way
            // from the animation's pose, and how far the hand was posed down onto its palm
            static uint64_t probeAt = 0;
            if (s.trace && side == 1 && GetTickCount64() - probeAt > 2000) {
                probeAt = GetTickCount64();
                static const float kW[3] = {1.1f, 1.5f, 0.9f};
                float d[4][3];
                for (int f = 0; f < 4; ++f) {
                    const FingerChain& c = h.f[f];
                    float bq[4], bp[3];
                    Compose(hq, hp, c.base, c.base + 4, bq, bp);
                    for (int i = 0; i < 3; ++i) {
                        const float t = 0.5f * float(i);
                        float b[3], F[3][7], J[4][3], axis[3][3];
                        for (int j = 0; j < 3; ++j) b[j] = c.moves[j] ? std::clamp(t * kW[j], c.lo[j], c.hi[j]) : 0.f;
                        PoseChain(c, bq, bp, b, J, F, axis);
                        int dummy = 0;
                        d[f][i] = 1000.f * Segments(c, J, F, *L, dummy, nullptr, nullptr, nullptr);
                    }
                }
                float moved = 0;
                for (int k = 0; k < 3; ++k) moved += (hp[k] - st.pose[side][4 + k]) * (hp[k] - st.pose[side][4 + k]);
                int dummy = 0;
                Log("[physhands] right fingers probe: posed down %.1f mm, palm %.1f mm in, wrap %d; mm in at curl 0/half/all: index "
                    "%.1f/%.1f/%.1f, middle %.1f/%.1f/%.1f, ring %.1f/%.1f/%.1f, little %.1f/%.1f/%.1f; near shapes %d\n",
                    1000.f * std::sqrt(moved), 1000.f * PalmDepth(side, hq, hp, *L, dummy), int(wrap), d[0][0], d[0][1], d[0][2],
                    d[1][0], d[1][1], d[1][2], d[2][0], d[2][1], d[2][2], d[3][0], d[3][1], d[3][2], L->n);
            }
        }
        // quick into a bend, slower out of it: a finger lifted off a table eases back
        Bends b{};
        bool bent = false;
        // THE ANIMATION OPENED UNDER A BEND THAT KEPT A FINGER OUT OF SOMETHING: the bend is the finger's turn from the
        // animation's pose, and what it held was the finger's angle; eased out from where it was, a grip let go with the
        // palm on a wall threw the fingers back past straight for the 0.2 s the bend took to go (2026-11-10). So it is
        // the less at once by how far the animation opened, never past straight the other way.
        static float shownFlex[2][5][3] = {};
        static bool shownFlexKnown[2] = {false, false};
        if (on) {
            for (int f = 0; f < 5; ++f)
                for (int j = 0; j < 3; ++j) {
                    const float now = h.f[f].flex[j];
                    float& v = shown[side][f][j];
                    if (shownFlexKnown[side] && v < 0) {
                        const float opened = shownFlex[side][f][j] - now;
                        if (opened > 0) {
                            v = std::min(0.f, v + opened);
                            if (v == 0) shownV[side][f][j] = 0;
                            else shownV[side][f][j] = std::max(shownV[side][f][j], 0.f);
                        }
                    }
                    shownFlex[side][f][j] = now;
                    b.flex[f][j] = now;
                }
            shownFlexKnown[side] = true;
        } else {
            shownFlexKnown[side] = false;
            std::memcpy(b.flex, shownFlex[side], sizeof(b.flex));
        }
        for (int f = 0; f < 5; ++f) {
            for (int j = 0; j < 3; ++j) {
                // critically damped, quicker into a bend than out of it, at most 6 rad/s: eased in a first-order way
                // (30 ms) a finger met by a wall moved half its bend in the first frame, 15-19 deg (2026-11-09)
                float& v = shown[side][f][j];
                const float tau = std::fabs(want[f][j]) > std::fabs(v) ? 0.035f : 0.07f;
                Damped(v, shownV[side][f][j], want[f][j], 1.f / tau, float(std::clamp(dtS, 1e-4, 0.1)), 6.f);
                if (std::fabs(v) < 1e-4f && std::fabs(want[f][j]) < 1e-4f) v = 0, shownV[side][f][j] = 0;
            }
            // THE EASED FINGER IN SOMETHING: out of it at once, by the least of the way to the solve that does it, eased on
            // from there. Eased alone, fingers drooping in the air as the hand came down on a railing took 0.4 s to
            // straighten and were 21 mm in it meanwhile, and a grip closed on it curled them 11 mm in before the solve's
            // counter-turn caught up (2026-11-09)
            if (on) {
                const float kInMost = fingerSlack + 0.0006f;
                const FingerChain& c = h.f[f];
                float bq[4], bp[3];
                Compose(st.pose[side], st.pose[side] + 4, c.base, c.base + 4, bq, bp);
                const auto depthAt = [&](float t) {
                    float a[3], F[3][7], J[4][3], axis[3][3];
                    for (int j = 0; j < 3; ++j) a[j] = shown[side][f][j] + (want[f][j] - shown[side][f][j]) * t;
                    PoseChain(c, bq, bp, a, J, F, axis);
                    return Segments(c, J, F, *L, putCalls, nullptr, nullptr, nullptr);
                };
                const float now = depthAt(0);
                if (now > kInMost) {
                    float lo = 0, hi = 1;
                    if (depthAt(1) < now) {
                        for (int it = 0; it < 6; ++it) {
                            const float mid = 0.5f * (lo + hi);
                            (depthAt(mid) > kInMost ? lo : hi) = mid;
                        }
                        for (int j = 0; j < 3; ++j) {
                            shown[side][f][j] += (want[f][j] - shown[side][f][j]) * hi;
                            shownV[side][f][j] = 0;
                        }
                        fingersOut.fetch_add(1, std::memory_order_relaxed);
                    }
                    // the solve in it too, here where the hand is (it was solved where the palm would be held): toward the
                    // open hand, which lies in the palm's plane
                    const float left = depthAt(0);
                    if (left > kInMost) {
                        OpenAngles(c, st.pose[side], st.pose[side] + 4, bq, bp, wantOpen[f]);
                        {
                            // the open hand in it as well: the animation's own pose, when that is clear of it
                            float F[3][7], J[4][3], axis[3][3];
                            PoseChain(c, bq, bp, wantOpen[f], J, F, axis);
                            const float openIn = Segments(c, J, F, *L, putCalls, nullptr, nullptr, nullptr);
                            if (!(openIn < left) && openIn > fingerSlack + 0.0015f) {
                                const float zero[3] = {0, 0, 0};
                                PoseChain(c, bq, bp, zero, J, F, axis);
                                if (Segments(c, J, F, *L, putCalls, nullptr, nullptr, nullptr) < left) wantOpen[f][0] = wantOpen[f][1] = wantOpen[f][2] = 0;
                            }
                        }
                        const auto openAt = [&](float t) {
                            float a[3], F[3][7], J[4][3], axis[3][3];
                            for (int j = 0; j < 3; ++j) a[j] = shown[side][f][j] + (wantOpen[f][j] - shown[side][f][j]) * t;
                            PoseChain(c, bq, bp, a, J, F, axis);
                            return Segments(c, J, F, *L, putCalls, nullptr, nullptr, nullptr);
                        };
                        if (openAt(1) < left) {
                            lo = 0, hi = 1;
                            for (int it = 0; it < 6; ++it) {
                                const float mid = 0.5f * (lo + hi);
                                (openAt(mid) > kInMost ? lo : hi) = mid;
                            }
                            for (int j = 0; j < 3; ++j) {
                                shown[side][f][j] += (wantOpen[f][j] - shown[side][f][j]) * hi;
                                shownV[side][f][j] = 0;
                            }
                            fingersOpened.fetch_add(1, std::memory_order_relaxed);
                        }
                        // ...and the open hand in it as well (a wrist turned fingertips first toward a wall the palm is on):
                        // bent back past straight, as a finger pressed on its pad gives at the knuckle and the last joint,
                        // by the least that has it out. The solve and the open hand both in, the fingers stayed 5-15 mm in
                        // the wall for a tenth of a second at a time (2026-11-09).
                        if (depthAt(0) > kInMost) {
                            static const float kBackMost[3] = {0.35f, 0.10f, 0.30f};
                            const auto backAt = [&](float v, float* b) {
                                for (int j = 0; j < 3; ++j)
                                    b[j] = c.moves[j] ? std::max(c.lo[j], wantOpen[f][j] - kBackMost[j] * v) : 0.f;
                            };
                            const auto backDepth = [&](float v) {
                                float b[3], F[3][7], J[4][3], axis[3][3];
                                backAt(v, b);
                                PoseChain(c, bq, bp, b, J, F, axis);
                                return Segments(c, J, F, *L, putCalls, nullptr, nullptr, nullptr);
                            };
                            const float now0 = depthAt(0);
                            if (backDepth(1) < now0) {
                                float lo2 = 0, hi2 = 1;
                                if (backDepth(1) <= kInMost)
                                    for (int it = 0; it < 5; ++it) {
                                        const float mid = 0.5f * (lo2 + hi2);
                                        (backDepth(mid) > kInMost ? lo2 : hi2) = mid;
                                    }
                                float b[3];
                                backAt(hi2, b);
                                for (int j = 0; j < 3; ++j) shown[side][f][j] = b[j], shownV[side][f][j] = 0;
                            }
                        }
                        // trace=1: a finger left in something, and why (rate-limited)
                        static uint64_t toldAt = 0;
                        const float after = depthAt(0);
                        if (s.trace && after > 0.002f && GetTickCount64() - toldAt > 300) {
                            toldAt = GetTickCount64();
                            float wantAt[3], openNow[3];
                            std::memcpy(wantAt, want[f], sizeof(wantAt));
                            std::memcpy(openNow, wantOpen[f], sizeof(openNow));
                            float F[3][7], J[4][3], axis[3][3];
                            PoseChain(c, bq, bp, wantAt, J, F, axis);
                            const float dWant = Segments(c, J, F, *L, putCalls, nullptr, nullptr, nullptr);
                            PoseChain(c, bq, bp, openNow, J, F, axis);
                            const float dOpen = Segments(c, J, F, *L, putCalls, nullptr, nullptr, nullptr);
                            float zero[3] = {0, 0, 0};
                            PoseChain(c, bq, bp, zero, J, F, axis);
                            const float dAnim = Segments(c, J, F, *L, putCalls, nullptr, nullptr, nullptr);
                            Log("[physhands] %s finger %d left %.1f mm in: shown %.2f %.2f %.2f, solve %.2f %.2f %.2f (%.1f mm), open %.2f %.2f %.2f "
                                "(%.1f mm), animation's (%.1f mm), palm %.1f mm in\n",
                                side ? "right" : "left", f, 1000.f * after, shown[side][f][0], shown[side][f][1], shown[side][f][2], wantAt[0],
                                wantAt[1], wantAt[2], 1000.f * dWant, openNow[0], openNow[1], openNow[2], 1000.f * dOpen, 1000.f * dAnim,
                                1000.f * PalmDepth(side, st.pose[side], st.pose[side] + 4, *L, putCalls));
                        }
                    }
                }
            }
            for (int j = 0; j < 3; ++j) {
                const float v = shown[side][f][j];
                b.a[f][j] = v;
                shownBends[side][f][j].store(v, std::memory_order_relaxed);
                float& y = shownYield[side][f][j];
                Damped(y, shownYieldV[side][f][j], wantYield[f][j], std::fabs(wantYield[f][j]) > std::fabs(y) ? 1.f / 0.035f : 1.f / 0.07f,
                       float(std::clamp(dtS, 1e-4, 0.1)), 6.f);
                if (std::fabs(y) < 1e-4f && std::fabs(wantYield[f][j]) < 1e-4f) y = 0, shownYieldV[side][f][j] = 0;
                b.y[f][j] = y;
                bent |= v != 0 || y != 0;
            }
        }
        b.on = bent;
        b.scannerEpoch = st.scannerEpoch;
        b.at = GetTickCount64();
        bends[side].Write(b);
        // trace=1, WHAT IS DRAWN, MEASURED: each finger turned as the animation is given it (b.a), on the hand where its
        // body is (where it is drawn, not where the fingers were solved), against what is near
        for (int f = 0; f < 5; ++f) {
            float deep = 0;
            if (on && s.trace) {
                const FingerChain& c = h.f[f];
                float bq[4], bp[3], F[3][7], J[4][3], axis[3][3];
                Compose(st.pose[side], st.pose[side] + 4, c.base, c.base + 4, bq, bp);
                PoseChain(c, bq, bp, b.a[f], J, F, axis);
                int dummy = 0;
                deep = Segments(c, J, F, *L, dummy, nullptr, nullptr, nullptr);
            }
            shownDepth[side][f].store(deep, std::memory_order_relaxed);
        }
        totalCalls += calls + putCalls;
    }
    ReleaseSRWLockShared(&nearLock);
    if (totalCalls) {
        fingerCalls.fetch_add(uint64_t(totalCalls), std::memory_order_relaxed);
        fingerContacts.fetch_add(uint64_t(contacts), std::memory_order_relaxed);
        fingerSolves.fetch_add(1, std::memory_order_relaxed);
        LARGE_INTEGER t1, f;
        QueryPerformanceCounter(&t1);
        QueryPerformanceFrequency(&f);
        fingerUs.fetch_add(uint64_t((t1.QuadPart - t0.QuadPart) * 1000000 / f.QuadPart), std::memory_order_relaxed);
    }
}

double NowS() {
    LARGE_INTEGER t, f;
    QueryPerformanceCounter(&t);
    QueryPerformanceFrequency(&f);
    return double(t.QuadPart) / double(f.QuadPart);
}

// The drawn hand's offset through a filter whose cutoff rises with how fast the offset moves (a "1 euro" filter): still
// against a wall it is calm, coming off it is quick.
struct Smoothed { float p[3]; float r[3]; bool on; };
struct SmoothedPoint { float p[3]; bool on; };
void Smooth(Smoothed& m, const float* p, const float* r, double dtS, const Settings& s) {
    if (!m.on) {
        std::memcpy(m.p, p, sizeof(m.p));
        std::memcpy(m.r, r, sizeof(m.r));
        m.on = true;
        return;
    }
    const float dt = float(std::clamp(dtS, 1e-4, 0.1));
    float dp = 0, dr = 0;
    for (int k = 0; k < 3; ++k) dp += (p[k] - m.p[k]) * (p[k] - m.p[k]), dr += (r[k] - m.r[k]) * (r[k] - m.r[k]);
    const float speed = std::sqrt(dp) / dt + 0.1f * std::sqrt(dr) / dt;   // a radian counted as 10 cm
    const float cutoff = s.smoothHz + s.smoothBeta * speed;
    const float alpha = 1.f - std::exp(-2.f * 3.14159265f * cutoff * dt);
    for (int k = 0; k < 3; ++k) m.p[k] += (p[k] - m.p[k]) * alpha, m.r[k] += (r[k] - m.r[k]) * alpha;
}

void Work() {
    SetThreadDescription(GetCurrentThread(), L"CPVR physics hands");
    Settings s{};
    ReadSettings(s);
    settings.Write(s);
    FILETIME seenWrite{};
    uint64_t readAt = GetTickCount64(), loggedAt = GetTickCount64();
    float offMost[2] = {}, offSum[2] = {};
    uint64_t offN = 0;
    bool wasOn[2] = {false, false};
    uint64_t lastStepAt = 0, lastScannerEpoch = 0;
    double lastSmoothAt = 0;
    Smoothed smoothed[2] = {};
    Smoothed bodySmoothed[2] = {};
    float held[2] = {0, 0}, latch[2][3] = {}, heldGoal[2] = {0, 0}, stuck[2][3] = {}, stuckTurn[2][4] = {};
    bool stuckOn[2] = {false, false}, stuckTurnOn[2] = {false, false};
    // the pressed hand's filter: the body's place and turn a step before, its speed filtered; the press as it was
    float stuckPrev[2][3] = {}, stuckPrevTurn[2][4] = {{0, 0, 0, 1}, {0, 0, 0, 1}}, stuckVel[2][3] = {}, stuckSpin[2] = {0, 0};
    bool pressedWas[2] = {false, false};
    // what the drawn hand is off its body as it lets go of the world (eased out), and how much a gun's turn is the body's
    float residual[2][3] = {}, residualTurn[2][4] = {{0, 0, 0, 1}, {0, 0, 0, 1}}, contactW[2] = {0, 0};
    bool latchOn[2] = {false, false};
    float lastShown[2][3] = {}, jitterSum[2] = {}, jitterMost[2] = {}, lastBody[2][3] = {}, bodyJitterSum[2] = {}, lastSent[2][3] = {}, sentJitterSum[2] = {};
    bool lastShownOn[2] = {false, false};
    uint32_t jitterN[2] = {0, 0};
    for (;;) {
        WaitForSingleObject(stepped, 50);
        const uint64_t now = GetTickCount64();
        // the settings, when the file has changed
        if (now - readAt > 1000) {
            readAt = now;
            WIN32_FILE_ATTRIBUTE_DATA a{};
            const std::wstring path = cvr::paths::File(cvr::paths::Folder::Config, L"physhands.ini");
            if (GetFileAttributesExW(path.c_str(), GetFileExInfoStandard, &a) && CompareFileTime(&a.ftLastWriteTime, &seenWrite)) {
                seenWrite = a.ftLastWriteTime;
                Settings fresh{};
                ReadSettings(fresh);
                settings.Write(fresh);
                s = fresh;
                Log("[physhands] settings read: on %d, %.2f kg, %.0f N, %.0f rad/s^2, deadband %.3f m\n", s.enabled, s.mass, s.force,
                    s.spinAccel, s.deadband);
            }
        }
        // when the hands are wanted: on foot, in the world, no menu, no ladder, no water (and no weapon out, if so set)
        const bool weapon = g_pSharedHands && g_pSharedHands[vrshared::kWeaponFlag] > 0.5f;
        const bool scene = SceneOwnsArms();
        const bool scanner = cvr::input::scannerHandPhysics.Active();
        const bool world = !scanner && s.enabled && g_liveControls.xrHandPhysics != 0 && g_liveControls.xrHandCollision != 0 && g_menuModeValue == 0 && !g_isInVehicle &&
                           CyberpunkVR_PlayerEntityValid && !cvr::lobby::WorldShown() && !cvr::ladder::Active() &&
                           !cvr::swimming::Active() && (s.withWeapon || !weapon) && !scene;
        Wanted w{};
        for (int side = 0; side < 2; ++side) {
            Target t{};
            w.on[side] = world && targets[side].Read(t) && now - t.at < 150;
            if (w.on[side] != wasOn[side]) {
                wasOn[side] = w.on[side];
                Log("[physhands] %s hand %s\n", side ? "right" : "left",
                    w.on[side] ? "wanted" : scanner ? "not wanted: scanner or world slow motion (tracking stays live)"
                                              : scene ? "not wanted: a scene plays the body (the cutscene tier, a grapple, a carried body, a finisher, a camera)"
                                              : "not wanted (vehicle, menu, ladder, water, no tracking, or a weapon with with_weapon=0)");
            }
        }
        w.at = now;
        wanted.Write(w);
        // the drawn hands: each body's place against where it was sent, in the player's model space, through the filter
        Step st{};
        if (steps.Read(st) && now - st.at < 200 && st.serial != lastStepAt) {
            lastStepAt = st.serial;
            if (st.scannerEpoch != lastScannerEpoch) {
                // A worker can miss the parked step. Do not blend a newly placed hand with a pre-scan contact.
                for (int side = 0; side < 2; ++side) {
                    smoothed[side].on = bodySmoothed[side].on = false;
                    held[side] = heldGoal[side] = contactW[side] = 0;
                    latchOn[side] = stuckOn[side] = stuckTurnOn[side] = pressedWas[side] = lastShownOn[side] = false;
                    residual[side][0] = residual[side][1] = residual[side][2] = 0;
                    residualTurn[side][0] = residualTurn[side][1] = residualTurn[side][2] = 0;
                    residualTurn[side][3] = 1;
                }
                lastScannerEpoch = st.scannerEpoch;
            }
            const double t = NowS();
            const double dtS = lastSmoothAt > 0 ? t - lastSmoothAt : 1.0 / 60.0;
            lastSmoothAt = t;
            float inv[4];
            QConj(st.entityRot, inv);
            for (int side = 0; side < 2; ++side) {
                Offset o{};
                o.at = now;
                o.scannerEpoch = st.scannerEpoch;
                o.on = st.valid[side] && live[side].load(std::memory_order_acquire) &&
                       cvr::input::scannerHandPhysics.Accepts(st.scannerEpoch);
                if (o.on) {
                    float dw[3], raw[3];
                    for (int k = 0; k < 3; ++k) dw[k] = st.pose[side][4 + k] - st.sent[side][4 + k];
                    QRot(inv, dw, raw);
                    const float len = std::sqrt(raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]);
                    const float keep = len > s.deadband ? (len - s.deadband) / len : 0.f;
                    for (auto& c : raw) c *= keep;
                    // the turn the body could not make, the same way round, as a rotation vector
                    float sentInv[4], qw[4], tmp[4], q[4];
                    QConj(st.sent[side], sentInv);
                    QMul(st.pose[side], sentInv, qw);
                    QMul(inv, qw, tmp);
                    QMul(tmp, st.entityRot, q);
                    float turn[3] = {0, 0, 0};
                    if (QNorm(q)) {
                        if (q[3] < 0) for (auto& c : q) c = -c;
                        const float sn = std::sqrt(q[0] * q[0] + q[1] * q[1] + q[2] * q[2]);
                        const float angle = 2.f * std::atan2(sn, q[3]);
                        const float kept = std::min(angle > s.turnDeadband ? angle - s.turnDeadband : 0.f, s.maxTurn);
                        if (sn > 1e-7f) for (int k = 0; k < 3; ++k) turn[k] = q[k] / sn * kept;
                    }
                    Smooth(smoothed[side], raw, turn, dtS, s);
                    std::memcpy(o.pos, smoothed[side].p, sizeof(o.pos));
                    // HELD OFF, THE DRAWN HAND IS THE BODY. Tracking plus an offset carries the controller's own tremor
                    // through whatever the offset's filter does; the body against a wall has none. So past a centimetre
                    // or two of offset the drawn hand is the body's place (filtered alone), tracking only where nothing
                    // stops it.
                    {
                        float bw[3];
                        for (int k = 0; k < 3; ++k) bw[k] = st.pose[side][4 + k] + st.origin[k];
                        const float none[3] = {0, 0, 0};
                        Smooth(bodySmoothed[side], bw, none, dtS, s);
                        // only a hand really on something is drawn at its body: in the air the body trails tracking by a
                        // step, and drawing it there made a quick hand jump
                        // on while it touches or is held off more than a centimetre, off once it is back within a few
                        // millimetres: a touch flickers as a hand slides along a wall, and the drawn hand went in and out
                        if (st.touching[side] || len > 0.012f) heldGoal[side] = 1.f;
                        else if (len < 0.006f) heldGoal[side] = 0.f;
                        const float goal = heldGoal[side];
                        held[side] += (goal - held[side]) * float(1.0 - std::exp(-dtS / 0.04));
                        if (held[side] < 0.01f) held[side] = 0;
                        o.held = held[side];
                        // STILL AT THE CONTACT: the drawn place stays put while the body stirs within a few millimetres,
                        // and is dragged along (by the excess) when the hand slides on the surface
                        if (o.held > 0.5f) {
                            if (!latchOn[side]) std::memcpy(latch[side], bodySmoothed[side].p, sizeof(latch[side])), latchOn[side] = true;
                            float d[3], dn = 0;
                            for (int k = 0; k < 3; ++k) d[k] = bw[k] - latch[side][k], dn += d[k] * d[k];
                            dn = std::sqrt(dn);
                            if (dn > s.stickHold) for (int k = 0; k < 3; ++k) latch[side][k] += d[k] * (1.f - s.stickHold / dn);
                            std::memcpy(o.body, latch[side], sizeof(o.body));
                        } else {
                            latchOn[side] = false;
                            std::memcpy(o.body, bodySmoothed[side].p, sizeof(o.body));
                        }
                    }
                    // THE HAND IS ITS BODY (drawn=1): where PhysX has it, moved on by tracking on the animation thread,
                    // held still while it presses the world and stirs within a few millimetres
                    o.bodyMode = s.drawn != 0;
                    if (o.bodyMode) {
                        float bw[3];
                        for (int k = 0; k < 3; ++k) bw[k] = st.pose[side][4 + k] + st.origin[k];
                        std::memcpy(o.bodyRot, st.pose[side], sizeof(o.bodyRot));
                        std::memcpy(o.spin, st.spin[side], sizeof(o.spin));
                        o.when = st.when;
                        o.stepDt = st.dt;
                        // PRESSED: on the world and refused more than a few millimetres of where it was sent. Only then is
                        // it held still. A hand lifting off a table touches it a few steps more while it follows tracking
                        // up; held still through them it was left 4 cm behind and jumped up as it let go.
                        float refused2 = 0;
                        for (int k = 0; k < 3; ++k) refused2 += (st.pose[side][4 + k] - st.sent[side][4 + k]) * (st.pose[side][4 + k] - st.sent[side][4 + k]);
                        // ...and pressed until refused under 3 mm again: at the one edge the press flickered on and off as a
                        // hand slid on a wall (10 times in 12 s), and each change moved the drawn hand
                        constexpr float kPressedFrom = 0.006f, kPressedTo = 0.003f;
                        const bool pressed = st.onWorld[side] &&
                                             (refused2 > kPressedFrom * kPressedFrom || (pressedWas[side] && refused2 > kPressedTo * kPressedTo));
                        pressedWas[side] = pressed;
                        o.pressed = pressed;
                        if (pressed) {
                            // from where it is drawn now, what a contact just let go of had left included
                            if (!stuckOn[side]) {
                                for (int k = 0; k < 3; ++k) stuck[side][k] = bw[k] + residual[side][k];
                                std::memcpy(stuckPrev[side], bw, sizeof(stuckPrev[side]));
                                std::memcpy(stuckPrevTurn[side], st.pose[side], sizeof(stuckPrevTurn[side]));
                                for (auto& c : stuckVel[side]) c = 0;
                                stuckSpin[side] = 0;
                                stuckOn[side] = true;
                            }
                            if (!stuckTurnOn[side]) {
                                QMul(residualTurn[side], st.pose[side], stuckTurn[side]);
                                if (!QNorm(stuckTurn[side])) std::memcpy(stuckTurn[side], st.pose[side], sizeof(stuckTurn[side]));
                                stuckTurnOn[side] = true;
                            }
                            // FOLLOWS ITS BODY THROUGH A FILTER (one-euro): still, the stir of a box pressed on the world (a few
                            // millimetres a step) is filtered away at kStuckHz; sliding, the cut-off rises with the body's
                            // speed and the drawn hand keeps up. The leash it had (still within stick_hold, dragged by the
                            // excess) held a hand sliding on a wall 8 mm behind its body and dropped that at once each time
                            // the press ended: the hand stuck and slipped (2026-11-09, 73 frames over 2 mm in 12 s).
                            constexpr float kStuckHz = 1.5f, kStuckBeta = 40.f, kStuckSpeedHz = 2.f, kPi = 3.14159265f;
                            const float sdt = float(std::clamp(dtS, 1e-3, 0.1));
                            float sp = 0;
                            {
                                const float a = 1.f - std::exp(-2.f * kPi * kStuckSpeedHz * sdt);
                                for (int k = 0; k < 3; ++k) {
                                    stuckVel[side][k] += ((bw[k] - stuckPrev[side][k]) / sdt - stuckVel[side][k]) * a;
                                    sp += stuckVel[side][k] * stuckVel[side][k];
                                }
                                sp = std::sqrt(sp);
                                float pinv[4], dq[4];
                                QConj(stuckPrevTurn[side], pinv);
                                QMul(st.pose[side], pinv, dq);
                                if (dq[3] < 0) for (auto& c : dq) c = -c;
                                const float an = 2.f * std::atan2(std::sqrt(dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]), dq[3]);
                                stuckSpin[side] += (an / sdt - stuckSpin[side]) * a;
                                std::memcpy(stuckPrev[side], bw, sizeof(stuckPrev[side]));
                                std::memcpy(stuckPrevTurn[side], st.pose[side], sizeof(stuckPrevTurn[side]));
                            }
                            const float cutoff = kStuckHz + kStuckBeta * (sp + 0.1f * stuckSpin[side]);   // a radian as 10 cm
                            const float alpha = 1.f - std::exp(-2.f * kPi * cutoff * sdt);
                            float d[3], dn = 0;
                            for (int k = 0; k < 3; ++k) stuck[side][k] += (bw[k] - stuck[side][k]) * alpha;
                            for (int k = 0; k < 3; ++k) d[k] = bw[k] - stuck[side][k], dn += d[k] * d[k];
                            dn = std::sqrt(dn);
                            // and never farther off its body than stick_hold
                            if (dn > s.stickHold) for (int k = 0; k < 3; ++k) stuck[side][k] += d[k] * (1.f - s.stickHold / dn);
                            std::memcpy(o.body, stuck[side], sizeof(o.body));
                            o.vel[0] = o.vel[1] = o.vel[2] = 0;
                            // the turn held the same way: a body pressed on the world rocks on its edge a few degrees
                            if (!stuckTurnOn[side]) std::memcpy(stuckTurn[side], st.pose[side], sizeof(stuckTurn[side])), stuckTurnOn[side] = true;
                            // the turn the same way: filtered toward the body's, never more than stick_turn off it
                            float inv[4], dq[4];
                            QConj(stuckTurn[side], inv);
                            QMul(st.pose[side], inv, dq);
                            if (dq[3] < 0) for (auto& c : dq) c = -c;
                            const float sn = std::sqrt(dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]);
                            const float angle = 2.f * std::atan2(sn, dq[3]);
                            if (sn > 1e-7f) {
                                const float left = std::min(angle * (1.f - alpha), s.stickTurn);
                                const float part = angle - left, h = std::sin(0.5f * part) / sn;
                                const float step[4] = {dq[0] * h, dq[1] * h, dq[2] * h, std::cos(0.5f * part)};
                                float q[4];
                                QMul(step, stuckTurn[side], q);
                                if (QNorm(q)) std::memcpy(stuckTurn[side], q, sizeof(q));
                            }
                            std::memcpy(o.bodyRot, stuckTurn[side], sizeof(o.bodyRot));
                            o.spin[0] = o.spin[1] = o.spin[2] = 0;
                            for (int k = 0; k < 3; ++k) residual[side][k] = stuck[side][k] - bw[k];
                            float bodyInv[4];
                            QConj(st.pose[side], bodyInv);
                            QMul(stuckTurn[side], bodyInv, residualTurn[side]);
                            QNorm(residualTurn[side]);
                        } else {
                            stuckOn[side] = false;
                            stuckTurnOn[side] = false;
                            // LETTING GO EASES OUT: what the drawn hand was held off its body (up to stick_hold, stick_turn)
                            // goes over 60 ms. Dropped at once, the hand jumped by it as it left a table.
                            const float keep = float(std::exp(-dtS / 0.06));
                            for (int k = 0; k < 3; ++k) residual[side][k] *= keep, o.body[k] = bw[k] + residual[side][k];
                            float* rt = residualTurn[side];
                            if (rt[3] < 0) for (int k = 0; k < 4; ++k) rt[k] = -rt[k];
                            const float rs = std::sqrt(rt[0] * rt[0] + rt[1] * rt[1] + rt[2] * rt[2]);
                            if (rs > 1e-6f) {
                                const float angle = 2.f * std::atan2(rs, rt[3]) * keep, h = std::sin(0.5f * angle) / rs;
                                rt[0] *= h; rt[1] *= h; rt[2] *= h; rt[3] = std::cos(0.5f * angle);
                            } else {
                                rt[0] = rt[1] = rt[2] = 0; rt[3] = 1;
                            }
                            QMul(rt, st.pose[side], o.bodyRot);
                            QNorm(o.bodyRot);
                            std::memcpy(o.vel, st.vel[side], sizeof(o.vel));
                        }
                        // a gun's hand turns as its body does while it touches something, as tracking has it otherwise
                        const float goal = st.touching[side] ? 1.f : 0.f;
                        contactW[side] += (goal - contactW[side]) * float(1.0 - std::exp(-dtS / (goal > contactW[side] ? 0.03 : 0.15)));
                        o.contact = contactW[side];
                        o.held = 1;
                        std::memcpy(o.raw, bw, sizeof(o.raw));
                        o.touching = st.touching[side];
                        o.onWorld = st.onWorld[side];
                        o.trace = uint8_t(s.trace != 0);
                        std::memcpy(o.origin, st.origin, sizeof(o.origin));
                        std::memcpy(o.rawRot, st.pose[side], sizeof(o.rawRot));
                        o.margin = s.skinMargin;
                        for (int k = 0; k < 3; ++k) o.sent[k] = st.sent[side][4 + k] + st.origin[k];
                        for (int k = 0; k < 3; ++k) o.sentTrack[k] = st.sentTrack[side][k] + st.origin[k];
                        for (int k = 0; k < 3; ++k) o.sentEntity[k] = st.sentEntity[side][k] + st.origin[k];
                        std::memcpy(o.sentRot, st.sent[side], sizeof(o.sentRot));
                    }
                    const float* r = smoothed[side].r;
                    const float a = std::sqrt(r[0] * r[0] + r[1] * r[1] + r[2] * r[2]);
                    if (a > 1e-6f) {
                        const float sa = std::sin(0.5f * a) / a;
                        o.rot[0] = r[0] * sa; o.rot[1] = r[1] * sa; o.rot[2] = r[2] * sa; o.rot[3] = std::cos(0.5f * a);
                    } else {
                        o.rot[3] = 1;
                    }
                    offSum[side] += len;
                    if (len > offMost[side]) offMost[side] = len;
                    // how much the drawn hand moves in the world a step while it is held off: the shaking (tracking's
                    // own tremor cancels in it, as it does on screen)
                    // in the game's world, as PhysX's origin can move under the player
                    float shownWorld[3], ow[3], bodyWorld[3];
                    QRot(st.entityRot, o.pos, ow);
                    for (int k = 0; k < 3; ++k) {
                        bodyWorld[k] = st.pose[side][4 + k] + st.origin[k];
                        shownWorld[k] = (st.sent[side][4 + k] + st.origin[k] + ow[k]) * (1.f - o.held) + o.body[k] * o.held;
                    }
                    float jump = 0, bodyJump = 0, sentJump = 0, sentWorld[3];
                    for (int k = 0; k < 3; ++k) {
                        sentWorld[k] = st.sent[side][4 + k] + st.origin[k];
                        jump += (shownWorld[k] - lastShown[side][k]) * (shownWorld[k] - lastShown[side][k]);
                        bodyJump += (bodyWorld[k] - lastBody[side][k]) * (bodyWorld[k] - lastBody[side][k]);
                        sentJump += (sentWorld[k] - lastSent[side][k]) * (sentWorld[k] - lastSent[side][k]);
                    }
                    jump = std::sqrt(jump);
                    bodyJump = std::sqrt(bodyJump);
                    sentJump = std::sqrt(sentJump);
                    std::memcpy(lastSent[side], sentWorld, sizeof(sentWorld));
                    if (len > 0.02f && lastShownOn[side]) {
                        jitterSum[side] += jump;
                        bodyJitterSum[side] += bodyJump;
                        sentJitterSum[side] += sentJump;
                        ++jitterN[side];
                        if (jump > jitterMost[side]) jitterMost[side] = jump;
                    }
                    std::memcpy(lastShown[side], shownWorld, sizeof(shownWorld));
                    std::memcpy(lastBody[side], bodyWorld, sizeof(bodyWorld));
                    lastShownOn[side] = true;
                } else {
                    o.rot[3] = 1;
                    smoothed[side].on = false;
                    bodySmoothed[side].on = false;
                    held[side] = 0;
                    heldGoal[side] = 0;
                    latchOn[side] = false;
                    stuckOn[side] = false;
                    stuckTurnOn[side] = false;
                    lastShownOn[side] = false;
                    residual[side][0] = residual[side][1] = residual[side][2] = 0;
                    residualTurn[side][0] = residualTurn[side][1] = residualTurn[side][2] = 0;
                    residualTurn[side][3] = 1;
                    contactW[side] = 0;
                }
                offsets[side].Write(o);
            }
            ++offN;
            SolveFingers(st, s, dtS);
        }
        // the trace, appended
        {
            static FILE* traceFile = nullptr;
            const uint32_t head = traceHead.load(std::memory_order_acquire);
            uint32_t tail = traceTail.load(std::memory_order_relaxed);
            if (head != tail) {
                if (!traceFile) {
                    const std::string path = cvr::paths::FileA(cvr::paths::Folder::Log, "physhands_trace.csv");
                    if ((traceFile = _fsopen(path.c_str(), "a", _SH_DENYNO)) != nullptr)
                        std::fprintf(traceFile, "t,side,drawn_x,drawn_y,drawn_z,body_x,body_y,body_z,raw_x,raw_y,raw_z,vel_x,vel_y,vel_z,move,"
                                                "track_x,track_y,track_z,touching,on_world,rot_x,rot_y,rot_z,rot_w,brot_x,brot_y,brot_z,brot_w,"
                                                "pressed,move_w,bend_0,bend_1,bend_2,bend_3,bend_4,in_0,in_1,in_2,in_3,in_4,trot_x,trot_y,trot_z,trot_w,drawn_in\n");
                }
                for (; tail != head; ++tail) {
                    const TraceRow& r = traceRing[tail % kTraceRing];
                    if (traceFile)
                        std::fprintf(traceFile, "%.5f,%d,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.3f,%.3f,%.3f,%.4f,%.4f,%.4f,%.4f,%d,%d,"
                                                "%.5f,%.5f,%.5f,%.5f,%.5f,%.5f,%.5f,%.5f,%d,%.3f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.4f,%.5f,%.5f,%.5f,%.5f,%.2f\n",
                                     r.t, r.side, r.drawn[0], r.drawn[1], r.drawn[2], r.body[0], r.body[1], r.body[2], r.raw[0], r.raw[1],
                                     r.raw[2], r.vel[0], r.vel[1], r.vel[2], r.lead, r.track[0], r.track[1], r.track[2], r.touching, r.onWorld,
                                     r.rot[0], r.rot[1], r.rot[2], r.rot[3], r.bodyRot[0], r.bodyRot[1], r.bodyRot[2], r.bodyRot[3],
                                     r.pressed, r.moveW, r.bend[0], r.bend[1], r.bend[2], r.bend[3], r.bend[4], 1000.f * r.in[0],
                                     1000.f * r.in[1], 1000.f * r.in[2], 1000.f * r.in[3], 1000.f * r.in[4], r.trackRot[0], r.trackRot[1],
                                     r.trackRot[2], r.trackRot[3], 1000.f * r.drawnIn);
                }
                traceTail.store(tail, std::memory_order_release);
                if (traceFile) std::fflush(traceFile);
            } else if (traceFile && !s.trace) {
                std::fclose(traceFile);
                traceFile = nullptr;
            }
        }
        // THE STATISTICS EVERY log_every SECONDS: a tuning aid, seven lines at a time, written with trace=1 or the launcher's
        // DEBUG box only. The block runs either way: its counters are taken (and reset) in the calls' arguments.
        if (now - loggedAt > uint64_t(s.logEvery) * 1000) {
            void (*say)(const char*, ...) = (s.trace || cvr::RuntimeDiagnosticsEnabled()) ? &Log : &QuietLog;
            const uint64_t n = stepsSeen.exchange(0);
            say("[physhands] %llu steps: before the step %.1f us mean (most %llu), after %.1f us (most %llu); the drawn hand off "
                "tracking left %.3f mean %.3f most, right %.3f mean %.3f most m; pressed, its step to step move left %.4f mean %.4f most, "
                "right %.4f mean %.4f most m (the body itself %.4f %.4f, where it was sent %.4f %.4f); touching steps %llu %llu (the world %llu %llu); on %d %d\n",
                (unsigned long long)n, n ? double(beforeUs.exchange(0)) / double(n) : 0.0, (unsigned long long)beforeMostUs.exchange(0),
                n ? double(afterUs.exchange(0)) / double(n) : 0.0, (unsigned long long)afterMostUs.exchange(0),
                offN ? offSum[0] / float(offN) : 0.f, offMost[0], offN ? offSum[1] / float(offN) : 0.f, offMost[1],
                jitterN[0] ? jitterSum[0] / float(jitterN[0]) : 0.f, jitterMost[0], jitterN[1] ? jitterSum[1] / float(jitterN[1]) : 0.f,
                jitterMost[1], jitterN[0] ? bodyJitterSum[0] / float(jitterN[0]) : 0.f, jitterN[1] ? bodyJitterSum[1] / float(jitterN[1]) : 0.f,
                jitterN[0] ? sentJitterSum[0] / float(jitterN[0]) : 0.f, jitterN[1] ? sentJitterSum[1] / float(jitterN[1]) : 0.f,
                (unsigned long long)touchedSteps[0].exchange(0), (unsigned long long)touchedSteps[1].exchange(0),
                (unsigned long long)worldSteps[0].exchange(0), (unsigned long long)worldSteps[1].exchange(0),
                int(live[0].load()), int(live[1].load()));
            {
                double mean[2] = {}, most[2] = {};
                uint64_t jumps[2] = {};
                for (int side = 0; side < 2; ++side) {
                    const uint64_t n = drawnJerkN[side].exchange(0);
                    mean[side] = n ? double(drawnJerkUm[side].exchange(0)) / double(n) / 1000.0 : 0.0;
                    most[side] = double(drawnJerkMostUm[side].exchange(0)) / 1000.0;
                    jumps[side] = drawnJumps[side].exchange(0);
                }
                double tmean[2] = {}, tmost[2] = {};
                for (int side = 0; side < 2; ++side) {
                    const uint64_t n = trackJerkN[side].exchange(0);
                    tmean[side] = n ? double(trackJerkUm[side].exchange(0)) / double(n) / 1000.0 : 0.0;
                    tmost[side] = double(trackJerkMostUm[side].exchange(0)) / 1000.0;
                }
                double smean[2] = {}, smost[2] = {}, amean[2] = {};
                uint64_t sjumps[2] = {};
                for (int side = 0; side < 2; ++side) {
                    const uint64_t n = slipN[side].exchange(0), an = airSlipN[side].exchange(0);
                    smean[side] = n ? double(slipUm[side].exchange(0)) / double(n) / 1000.0 : 0.0;
                    smost[side] = double(slipMostUm[side].exchange(0)) / 1000.0;
                    sjumps[side] = slipJumps[side].exchange(0);
                    amean[side] = an ? double(airSlipUm[side].exchange(0)) / double(an) / 1000.0 : 0.0;
                }
                say("[physhands] drawn hand's offset from tracking, frame to frame: left %.2f mm mean, %.1f most, %llu over 3 mm, in the "
                    "air %.2f; right %.2f mean, %.1f most, %llu over 3 mm, in the air %.2f\n",
                    smean[0], smost[0], (unsigned long long)sjumps[0], amean[0], smean[1], smost[1], (unsigned long long)sjumps[1], amean[1]);
                say("[physhands] drawn hand off its own line in time, frame to frame: left %.2f mm mean, %.1f most, %llu over 3 mm (tracking "
                    "%.2f, %.1f); right %.2f mean, %.1f most, %llu over 3 mm (tracking %.2f, %.1f). Loose things near %llu (capped %llu), the "
                    "palm kept off them %llu steps, let through them %llu times, stacks let down %llu, stacked things %llu, let-down "
                    "things kept in place %llu; tracking past the arm's reach left %u right %u steps\n",
                    mean[0], most[0], (unsigned long long)jumps[0], tmean[0], tmost[0], mean[1], most[1], (unsigned long long)jumps[1],
                    tmean[1], tmost[1],
                    (unsigned long long)looseListed.load(), (unsigned long long)looseCapped.load(), (unsigned long long)palmRefused.exchange(0),
                    (unsigned long long)looseLetThrough.exchange(0),
                    (unsigned long long)dropsMade.exchange(0), (unsigned long long)looseStacked.load(),
                    (unsigned long long)apartKept.exchange(0), reachHeld[0].exchange(0), reachHeld[1].exchange(0));
            }
            const uint64_t solves = fingerSolves.exchange(0);
            say("[physhands] fingers: %llu solves with something near, %llu penetration calls, %llu in contact, %.1f us a solve; "
                "world list: %llu cycles, at most %llu shapes; %llu faults; posed down to the palm %llu, palm on something %llu; eased fingers put out %llu, opened %llu; the back of the hand pressed %llu\n",
                (unsigned long long)solves, (unsigned long long)fingerCalls.exchange(0), (unsigned long long)fingerContacts.exchange(0),
                solves ? double(fingerUs.exchange(0)) / double(solves) : 0.0, (unsigned long long)nearCycles.exchange(0),
                (unsigned long long)nearMostShapes.exchange(0), (unsigned long long)fingerFaults.load(),
                (unsigned long long)palmShifted.exchange(0), (unsigned long long)palmWraps.exchange(0),
                (unsigned long long)fingersOut.exchange(0), (unsigned long long)fingersOpened.exchange(0),
                (unsigned long long)backPressed.exchange(0));
            {
                // the bends the solver gives each finger now (rad past the animation's, knuckle/middle/last): negative
                // straightens, positive curls toward the palm
                float bn[2][5][3];
                for (int sd = 0; sd < 2; ++sd)
                    for (int f = 0; f < 5; ++f)
                        for (int j = 0; j < 3; ++j) bn[sd][f][j] = shownBends[sd][f][j].load(std::memory_order_relaxed);
                for (int sd = 0; sd < 2; ++sd)
                    say("[physhands] %s bends: thumb %+.2f %+.2f %+.2f, index %+.2f %+.2f %+.2f, middle %+.2f %+.2f %+.2f, ring %+.2f "
                        "%+.2f %+.2f, little %+.2f %+.2f %+.2f\n", sd ? "right" : "left",
                        bn[sd][4][0], bn[sd][4][1], bn[sd][4][2], bn[sd][0][0], bn[sd][0][1], bn[sd][0][2], bn[sd][1][0], bn[sd][1][1],
                        bn[sd][1][2], bn[sd][2][0], bn[sd][2][1], bn[sd][2][2], bn[sd][3][0], bn[sd][3][1], bn[sd][3][2]);
            }
            offSum[0] = offSum[1] = offMost[0] = offMost[1] = 0;
            jitterSum[0] = jitterSum[1] = jitterMost[0] = jitterMost[1] = bodyJitterSum[0] = bodyJitterSum[1] = sentJitterSum[0] = sentJitterSum[1] = 0;
            jitterN[0] = jitterN[1] = 0;
            offN = 0;
            loggedAt = now;
        }
    }
}

void StartWorker() {
    bool expected = false;
    if (!workerUp.compare_exchange_strong(expected, true)) return;
    stepped = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    std::thread(Work).detach();
}

}  // namespace

static void NoteTracking(int side, const float* modelPos);

// THE TRACE (trace=1): each drawn frame -- the drawn place, the body's as drawn and as PhysX has it, its velocity, what it
// was carried by, tracking's target, the touch -- through a ring the worker empties into log/physhands_trace.csv.
static void TraceFrame(int side, const float* at, const Offset& o, float lead, const float* modelRot, float moveW, const float* trackModelRot) {
    const uint32_t h = traceHead.load(std::memory_order_relaxed);
    if (h - traceTail.load(std::memory_order_acquire) >= kTraceRing) return;
    TraceRow& r = traceRing[h % kTraceRing];
    r.t = NowS();
    r.side = side;
    std::memcpy(r.drawn, at, sizeof(r.drawn));
    std::memcpy(r.body, o.body, sizeof(r.body));
    std::memcpy(r.raw, o.raw, sizeof(r.raw));
    std::memcpy(r.vel, o.vel, sizeof(r.vel));
    std::memcpy(r.track, lastTrack[side], sizeof(r.track));
    r.lead = lead;
    r.touching = o.touching;
    r.onWorld = o.onWorld;
    {
        float q[4] = {0, 0, 0, 1};
        if (modelRot) QMul(CyberpunkVR_PlayerEntityQuat, modelRot, q);
        if (q[3] < 0) for (auto& c : q) c = -c;
        std::memcpy(r.rot, q, sizeof(r.rot));
        float b[4] = {o.bodyRot[0], o.bodyRot[1], o.bodyRot[2], o.bodyRot[3]};
        if (b[3] < 0) for (auto& c : b) c = -c;
        std::memcpy(r.bodyRot, b, sizeof(r.bodyRot));
        float t[4] = {0, 0, 0, 1};
        if (trackModelRot) QMul(CyberpunkVR_PlayerEntityQuat, trackModelRot, t);
        if (t[3] < 0) for (auto& c : t) c = -c;
        std::memcpy(r.trackRot, t, sizeof(r.trackRot));
    }
    r.pressed = o.pressed ? 1 : 0;
    r.moveW = moveW;
    r.drawnIn = drawnIn[side].load(std::memory_order_relaxed);
    for (int f = 0; f < 5; ++f) {
        float sum = 0;
        for (int j = 0; j < 3; ++j) sum += shownBends[side][f][j].load(std::memory_order_relaxed);
        r.bend[f] = sum;
        r.in[f] = shownDepth[side][f].load(std::memory_order_relaxed);
    }
    traceHead.store(h + 1, std::memory_order_release);
}

void NoteTarget(int side, const float* modelPos, const float* modelRot) {
    if (side < 0 || side > 1 || !modelPos || !modelRot) return;
    Target t{};
    for (int k = 0; k < 3; ++k) t.pos[k] = modelPos[k], t.entityPos[k] = CyberpunkVR_PlayerEntityPos[k];
    for (int k = 0; k < 4; ++k) t.rot[k] = modelRot[k], t.entityRot[k] = CyberpunkVR_PlayerEntityQuat[k];
    t.at = GetTickCount64();
    targets[side].Write(t);
    NoteTracking(side, modelPos);
}

bool On(int side) {
    return side >= 0 && side <= 1 && live[side].load(std::memory_order_acquire) &&
           cvr::input::scannerHandPhysics.Accepts(liveScannerEpoch.load(std::memory_order_acquire));
}
bool Tracing() {
    Settings s{};
    return settings.Read(s) && s.trace != 0;
}

bool FingerHingeOnly() {
    Settings s{};
    return !settings.Read(s) || s.fingerHingeOnly != 0;
}


void NoteForearm(int side, const float* elbowModel, const float* wristModel, const float* handModelRot,
                 const float* shoulderModel, float reach) {
    if (side < 0 || side > 1 || !elbowModel || !wristModel || !handModelRot) return;
    if (shoulderModel && std::isfinite(shoulderModel[0] + shoulderModel[1] + shoulderModel[2])) {
        Shoulder sh{};
        std::memcpy(sh.model, shoulderModel, sizeof(sh.model));
        sh.reach = reach;
        if (!std::isfinite(sh.reach) || sh.reach < 0.3f || sh.reach > 1.2f) sh.reach = 0;   // not an arm: no limit from it
        sh.at = GetTickCount64();
        shoulders[side].Write(sh);
    }
    Forearm f{};
    float d[3], inv[4];
    for (int k = 0; k < 3; ++k) d[k] = elbowModel[k] - wristModel[k];
    QConj(handModelRot, inv);
    float q[4] = {inv[0], inv[1], inv[2], inv[3]};
    if (!QNorm(q)) return;
    QRot(q, d, f.elbow);
    f.at = GetTickCount64();
    forearms[side].Write(f);
}

// A point's track frame to frame: how far each frame's point is off the line through the two before it, carried by the
// time between (frames come unevenly; a point moving steadily scores nothing however they come). For the drawn hand and,
// as the yardstick, for tracking's own target.
struct Track { double t1 = 0, t2 = 0; float p1[3] = {}, p2[3] = {}; int have = 0; };
static float OffItsLine(Track& tr, const float* world, double now) {
    float off = -1;
    if (now - tr.t1 < 0.004) return -1;   // a second solve of the same frame
    if (now - tr.t1 > 0.1) tr.have = 0;
    if (tr.have >= 2 && tr.t1 > tr.t2) {
        const double k = (now - tr.t1) / (tr.t1 - tr.t2);
        float d = 0;
        for (int i = 0; i < 3; ++i) {
            const float e = world[i] - (tr.p1[i] + (tr.p1[i] - tr.p2[i]) * float(k));
            d += e * e;
        }
        off = std::sqrt(d);
    }
    std::memcpy(tr.p2, tr.p1, sizeof(tr.p1));
    std::memcpy(tr.p1, world, sizeof(tr.p1));
    tr.t2 = tr.t1;
    tr.t1 = now;
    if (tr.have < 2) ++tr.have;
    return off;
}
static void Tally(std::atomic<uint64_t>& sum, std::atomic<uint64_t>& most, std::atomic<uint64_t>& n, float off) {
    const uint64_t um = uint64_t(off * 1e6f);
    sum.fetch_add(um, std::memory_order_relaxed);
    n.fetch_add(1, std::memory_order_relaxed);
    uint64_t was = most.load(std::memory_order_relaxed);
    while (um > was && !most.compare_exchange_weak(was, um)) {}
}
static void NoteDrawn(int side, const float* world) {
    static thread_local Track tr[2];
    const float off = OffItsLine(tr[side], world, NowS());
    if (off < 0) return;
    Tally(drawnJerkUm[side], drawnJerkMostUm[side], drawnJerkN[side], off);
    if (off > 0.003f) drawnJumps[side].fetch_add(1, std::memory_order_relaxed);
}
static void NoteSlip(int side, const float* drawn, const float* track, bool inAir) {
    static thread_local float last[2][3];
    static thread_local double lastAt[2] = {0, 0};
    const double now = NowS();
    if (now - lastAt[side] < 0.004) return;   // a second solve of the same frame
    float e[3], d2 = 0;
    for (int k = 0; k < 3; ++k) e[k] = drawn[k] - track[k], d2 += (e[k] - last[side][k]) * (e[k] - last[side][k]);
    const bool have = now - lastAt[side] < 0.1;
    std::memcpy(last[side], e, sizeof(e));
    lastAt[side] = now;
    if (!have) return;
    const float off = std::sqrt(d2);
    Tally(slipUm[side], slipMostUm[side], slipN[side], off);
    if (off > 0.003f) slipJumps[side].fetch_add(1, std::memory_order_relaxed);
    if (inAir) {
        airSlipUm[side].fetch_add(uint64_t(off * 1e6f), std::memory_order_relaxed);
        airSlipN[side].fetch_add(1, std::memory_order_relaxed);
    }
}
static void NoteTracking(int side, const float* modelPos) {
    static thread_local Track tr[2];
    float w[3];
    QRot(CyberpunkVR_PlayerEntityQuat, modelPos, w);
    for (int k = 0; k < 3; ++k) w[k] += CyberpunkVR_PlayerEntityPos[k];
    std::memcpy(lastTrack[side], w, sizeof(w));
    const float off = OffItsLine(tr[side], w, NowS());
    if (off >= 0) Tally(trackJerkUm[side], trackJerkMostUm[side], trackJerkN[side], off);
}

// THE DRAWN HAND STAYS OUT OF THE WORLD. The body meets a wall at its step; the drawn hand is that body moved on by
// tracking since, and the step that says the body touched comes a frame later: slapped at a wall at 4 m/s, the drawn hand
// went 19 mm into it for a frame and sprang back (2026-11-09). So the body's shapes as last posed (the palm's box, the
// finger capsules, the forearm's, grown by the skin's margin) are tried where the hand is drawn against what is near the
// hands -- the world's shapes and the people's -- and the way from the body to it is cut back, halved a few times, until no
// shape is deeper in than it is at the body itself (and a millimetre's grace). The palm's box alone let a slap's curled
// fingertips, which the body had stopped on, go 17 mm into the wall. Animation thread; the lists are the worker's, read
// under its lock.
// THE TURN AS WELL, AND PRESSED TOO: the way is from the body as PhysX has it (its place and its turn) to the drawn place
// and turn, cut back together. A wrist turned toward a wall was drawn with tracking's turn until the eased turn offset
// caught up (30 ms, at most 4 rad/s), and the fingers 15 cm out went into the wall meanwhile.
std::atomic<uint64_t> drawnCut[2] = {0, 0}, drawnCutUm[2] = {0, 0};
static float ShapeDepth(const BodyShape& b, const float* at, const float* worldRot, const float* origin, const NearList* L,
                        const PeopleNear* P, int side) {
    PxTransform pose{};
    float r[3];
    QRot(worldRot, b.local.p, r);
    for (int k = 0; k < 3; ++k) pose.p[k] = at[k] + r[k] - origin[k];
    QMul(worldRot, b.local.q, pose.q);
    QNorm(pose.q);
    float deepest = 0;
    const auto test = [&](const NearShape& e) {
        for (int k = 0; k < 3; ++k)
            if (pose.p[k] + b.reach < e.bounds.lo[k] || pose.p[k] - b.reach > e.bounds.hi[k]) return;
        float dir[3]{}, depth = 0;
        if (Penetration(dir, &depth, b.geometry, &pose, e) && std::isfinite(depth)) deepest = std::max(deepest, depth);
    };
    if (L) for (int i = 0; i < L->n && i < kMostNear; ++i) test(L->s[i]);
    // V's own body: the palm's and the fingers' alone, of the parts this hand meets while it does (the forearm lies along
    // the trunk, and a forearm "in" it held the drawn hand back)
    const bool own = b.part != 2 && selfOnShown[side].load(std::memory_order_relaxed);
    if (P)
        for (int i = 0; i < P->n && i < kMostBodies; ++i)
            if (P->owner[i] != kSelfOwner || (own && (P->meets[i] >> side & 1))) test(P->s[i]);
    return deepest;
}
static void QSlerp(const float* a, const float* b0, float t, float* o) {
    float b[4] = {b0[0], b0[1], b0[2], b0[3]};
    float d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if (d < 0) {
        for (auto& c : b) c = -c;
        d = -d;
    }
    float wa = 1.f - t, wb = t;
    if (d < 0.9995f) {
        const float th = std::acos(std::min(d, 1.f)), sn = std::sin(th);
        wa = std::sin((1.f - t) * th) / sn;
        wb = std::sin(t * th) / sn;
    }
    for (int k = 0; k < 4; ++k) o[k] = a[k] * wa + b[k] * wb;
    QNorm(o);
}
static void KeepDrawnOut(int side, const Offset& o, float* at, float* modelRot) {
    drawnIn[side].store(0.f, std::memory_order_relaxed);
    const bool cut = !g_hasWeaponEquipped;
    if ((!cut && !o.trace) || !modelRot) return;
    static thread_local BodyShapes bs;
    if (!bodyShapes[side].Read(bs) || bs.n <= 0 || GetTickCount64() - bs.at > 200) return;
    float wq[4], bq[4] = {o.rawRot[0], o.rawRot[1], o.rawRot[2], o.rawRot[3]};
    QMul(CyberpunkVR_PlayerEntityQuat, modelRot, wq);
    if (!QNorm(wq) || !QNorm(bq)) return;
    // from the body as PhysX has it
    float d[3], dn = 0;
    for (int k = 0; k < 3; ++k) d[k] = at[k] - o.raw[k], dn += d[k] * d[k];
    float turnDot = std::fabs(wq[0] * bq[0] + wq[1] * bq[1] + wq[2] * bq[2] + wq[3] * bq[3]);
    const bool moved = dn >= 0.001f * 0.001f || turnDot < 0.99998f;   // a millimetre, or about a degree
    static thread_local PeopleNear pn;
    const PeopleNear* P = peopleNear.Read(pn) && pn.n > 0 && GetTickCount64() - pn.at < 300 ? &pn : nullptr;
    AcquireSRWLockShared(&nearLock);
    const int front = nearFront.load(std::memory_order_acquire);
    const NearList* L = front >= 0 ? &nearLists[front] : nullptr;
    const float grace = o.margin + 0.001f;
    // the shapes deeper where drawn than at the body, and how deep each may be
    int bad[kBodyShapes], nb = 0;
    float allowed[kBodyShapes];
    for (int i = 0; i < bs.n && i < kBodyShapes && cut && moved; ++i) {
        const float there = ShapeDepth(bs.s[i], at, wq, o.origin, L, P, side);
        if (!(there > grace)) continue;
        const float atBody = ShapeDepth(bs.s[i], o.raw, bq, o.origin, L, P, side);
        const float most = std::max(grace, atBody + 0.0005f);
        if (there > most) bad[nb] = i, allowed[nb] = most, ++nb;
    }
    float cutTo = 1;
    if (nb > 0) {
        float lo = 0, hi = 1;
        for (int it = 0; it < 6; ++it) {
            const float mid = 0.5f * (lo + hi);
            float p[3], q[4];
            for (int k = 0; k < 3; ++k) p[k] = o.raw[k] + d[k] * mid;
            QSlerp(bq, wq, mid, q);
            bool in = false;
            for (int j = 0; j < nb && !in; ++j) in = ShapeDepth(bs.s[bad[j]], p, q, o.origin, L, P, side) > allowed[j];
            (in ? hi : lo) = mid;
        }
        for (int k = 0; k < 3; ++k) at[k] = o.raw[k] + d[k] * lo;
        QSlerp(bq, wq, lo, wq);
        {
            float inv[4], m[4];
            QConj(CyberpunkVR_PlayerEntityQuat, inv);
            QMul(inv, wq, m);
            if (QNorm(m)) std::memcpy(modelRot, m, sizeof(m));
        }
        cutTo = lo;
        drawnCut[side].fetch_add(1, std::memory_order_relaxed);
        drawnCutUm[side].fetch_add(uint64_t(1e6f * std::sqrt(dn) * (1.f - lo)), std::memory_order_relaxed);
    }
    if (o.trace) {
        float worst = 0;
        int which = -1;
        for (int i = 0; i < bs.n && i < kBodyShapes; ++i) {
            const float e = ShapeDepth(bs.s[i], at, wq, o.origin, L, P, side);
            if (e > worst) worst = e, which = i;
        }
        drawnIn[side].store(std::max(0.f, worst - o.margin), std::memory_order_relaxed);
        // a drawn shape over a centimetre in: which, and how the cut saw it (rate-limited)
        static uint64_t toldAt[2] = {0, 0};
        if (which >= 0 && worst - o.margin > 0.01f && GetTickCount64() - toldAt[side] > 200) {
            toldAt[side] = GetTickCount64();
            float p[3];
            for (int k = 0; k < 3; ++k) p[k] = o.raw[k] + d[k];
            const float before = ShapeDepth(bs.s[which], p, wq, o.origin, L, P, side);
            const float atBody = ShapeDepth(bs.s[which], o.raw, bq, o.origin, L, P, side);
            int type = 0;
            std::memcpy(&type, bs.s[which].geometry, sizeof(type));
            Log("[physhands] %s hand drawn %.1f mm in by its shape %d (type %d, of %d): %.1f mm before the cut, %.1f at the body; "
                "%d shapes cut for, to %.2f of %.1f mm; pressed %d\n", side ? "right" : "left", 1000.f * (worst - o.margin), which, type,
                bs.n, 1000.f * before, 1000.f * atBody, nb, cutTo, 1000.f * std::sqrt(dn), int(o.pressed));
        }
    }
    ReleaseSRWLockShared(&nearLock);
}

static bool AdjustBody(int side, float* modelPos, float* modelRot);

// THE HAND THAT HOLDS ON (climb) IS DRAWN WHERE AND AS IT TOOK HOLD, as the grenade's pin holds the hand: the hold's place
// and turn in the game's world, brought into the model's space each frame (the player pulled up after it leaves it where
// it is), whatever the controller does. Eased in and out over 30 ms. Drawn from its body, the hold slid off where it was
// taken as the body was pushed about on the railing, and turned as the controller did.
static bool AdjustHold(int side, float* modelPos, float* modelRot) {
    static float holdW[2] = {0, 0}, holdP[2][3] = {}, holdQ[2][4] = {{0, 0, 0, 1}, {0, 0, 0, 1}};
    static double holdAt[2] = {0, 0};
    constexpr float kHoldTau = 0.03f;
    if (!AdjustBody(side, modelPos, modelRot)) {
        holdW[side] = 0;
        return false;
    }
    HoldPose hp{};
    const bool on = holdPoses[side].Read(hp) && hp.on && GetTickCount64() - hp.at < 200 &&
                    std::isfinite(hp.p[0] + hp.p[1] + hp.p[2] + hp.q[0] + hp.q[1] + hp.q[2] + hp.q[3]);
    const double nowS = NowS(), dt = std::clamp(nowS - holdAt[side], 0.0, 0.1);
    if (nowS - holdAt[side] > 0.004) holdAt[side] = nowS;
    if (on) {
        // taken now: at once at the hold's weight so far (none), eased in from where the body had it
        std::memcpy(holdP[side], hp.p, sizeof(holdP[side]));
        std::memcpy(holdQ[side], hp.q, sizeof(holdQ[side]));
    }
    holdW[side] += ((on ? 1.f : 0.f) - holdW[side]) * float(1.0 - std::exp(-dt / kHoldTau));
    if (holdW[side] < 0.001f) return true;
    float inv[4], d[3], m[3], q[4];
    QConj(CyberpunkVR_PlayerEntityQuat, inv);
    for (int k = 0; k < 3; ++k) d[k] = holdP[side][k] - CyberpunkVR_PlayerEntityPos[k];
    QRot(inv, d, m);
    QMul(inv, holdQ[side], q);
    if (!QNorm(q) || !std::isfinite(m[0] + m[1] + m[2])) return true;
    const float w = holdW[side] > 0.999f ? 1.f : holdW[side];
    for (int k = 0; k < 3; ++k) modelPos[k] += (m[k] - modelPos[k]) * w;
    if (modelRot) {
        if (q[0] * modelRot[0] + q[1] * modelRot[1] + q[2] * modelRot[2] + q[3] * modelRot[3] < 0) for (auto& c : q) c = -c;
        float r[4];
        for (int k = 0; k < 4; ++k) r[k] = modelRot[k] + (q[k] - modelRot[k]) * w;
        if (QNorm(r)) for (int k = 0; k < 4; ++k) modelRot[k] = r[k];
    }
    return true;
}

// The completed physics pose supplies the held wrist's orientation. The controller still drives the grip point
// and the weighted body; publishing the pre-load wrist avoids feeding the correction back into that drive.
constexpr float kLoadTau = 0.08f;
static void BearLoad(int side, float* modelPos, float* modelRot) {
    static float w[2] = {0, 0};
    static double at[2] = {0, 0};
    static HoldLoad last[2] = {};
    static uint64_t epochs[2] = {};
    const auto epoch=cvr::input::scannerHandPhysics.Epoch();
    if(epochs[side]!=epoch) { epochs[side]=epoch;w[side]=0;last[side]={};at[side]=NowS(); }
    if (!modelPos) return;
    // the hand as it is before the body's turn: the body held is put on it
    {
        DrawnHand dh{};
        float d[3];
        QRot(CyberpunkVR_PlayerEntityQuat, modelPos, d);
        for (int i = 0; i < 3; ++i) dh.p[i] = CyberpunkVR_PlayerEntityPos[i] + d[i];
        if (modelRot) QMul(CyberpunkVR_PlayerEntityQuat, modelRot, dh.q);
        else std::memcpy(dh.q, CyberpunkVR_PlayerEntityQuat, sizeof(dh.q));
        dh.at = GetTickCount64();
        if (QNorm(dh.q) && std::isfinite(dh.p[0] + dh.p[1] + dh.p[2])) drawnHands[side].Write(dh);
    }
    if (!modelRot) return;
    HoldLoad L{};
    const bool fresh = g_liveControls.xrHandPhysics != 0 && holdLoads[side].Read(L) && L.on && GetTickCount64() - L.at < 200;
    if (fresh) last[side] = L;
    const double now = NowS(), dt = std::clamp(now - at[side], 0.0, 0.1);
    if (now - at[side] > 0.004) at[side] = now;
    w[side] += ((fresh ? 1.f : 0.f) - w[side]) * float(1.0 - std::exp(-dt / kLoadTau));
    if (w[side] < 0.001f) return;
    if (fresh && w[side] > 0.99f) w[side] = 1.f;
    const HoldLoad& u = last[side];
    float wp[3], wq[4], d[3];
    QRot(CyberpunkVR_PlayerEntityQuat, modelPos, d);
    for (int i = 0; i < 3; ++i) wp[i] = CyberpunkVR_PlayerEntityPos[i] + d[i];
    QMul(CyberpunkVR_PlayerEntityQuat, modelRot, wq);
    if (!QNorm(wq) || !cvr::held_grip::SeatRotation(wp, wq, u.wrist, u.seat, w[side])) return;
    float inv[4], m[3];
    QConj(CyberpunkVR_PlayerEntityQuat, inv);
    for (int i = 0; i < 3; ++i) d[i] = wp[i] - CyberpunkVR_PlayerEntityPos[i];
    QRot(inv, d, m);
    if (!std::isfinite(m[0] + m[1] + m[2])) return;
    std::memcpy(modelPos, m, sizeof(m));
    float mq[4];
    QMul(inv, wq, mq);
    if (QNorm(mq)) std::memcpy(modelRot, mq, sizeof(mq));
}

bool HoldsBody(int side) {
    if (side < 0 || side > 1 || g_liveControls.xrHandPhysics == 0 || g_liveControls.xrHandCollision == 0) return false;
    HoldLoad load{};
    return holdLoads[side].Read(load) && load.on && GetTickCount64() - load.at < 200;
}

bool Adjust(int side, float* modelPos, float* modelRot) {
    const bool drawn = AdjustHold(side, modelPos, modelRot);
    BearLoad(side, modelPos, modelRot);
    return drawn;
}

static bool AdjustBody(int side, float* modelPos, float* modelRot) {
    if (!On(side) || !modelPos) return false;
    Offset o{};
    if (!offsets[side].Read(o) || !o.on || GetTickCount64() - o.at > 200 ||
        !cvr::input::scannerHandPhysics.Accepts(o.scannerEpoch)) return false;
    if (o.bodyMode) {
        // THE BODY, MOVED ON BY WHAT TRACKING HAS DONE SINCE THE STEP THAT SENT IT. A free body ends its step where it was
        // sent, so this is tracking itself, frame for frame; a body something held back is that much short of it. Carried
        // by its velocity over a step instead, the hand swung 7 mm either side of tracking at half a metre a second: each
        // step's velocity is the last frame's move over the last step's length, and frames and steps come unevenly.
        // Touching, the part of tracking's move into what refused the body is left out; pressed on the world, the body
        // is held where it is (the latch) and none of it is added.
        float track[3], move[3] = {0, 0, 0}, inv[4];
        QRot(CyberpunkVR_PlayerEntityQuat, modelPos, track);
        for (int k = 0; k < 3; ++k) track[k] += CyberpunkVR_PlayerEntityPos[k];
        // THE PLAYER'S OWN MOVE SINCE THE STEP rides along whole: what follows holds back or leaves out only the hand's own
        // move against the player. A hand pressed on the other while running was latched where the step left it in the
        // world, and fell a run's frame behind each frame it stayed pressed. (A jump past a metre is a teleport.)
        float pm[3], pm2 = 0;
        for (int k = 0; k < 3; ++k) pm[k] = CyberpunkVR_PlayerEntityPos[k] - o.sentEntity[k], pm2 += pm[k] * pm[k];
        if (!(pm2 < 1.f)) pm[0] = pm[1] = pm[2] = 0;
        // eased out as the body is pressed on the world and in as it leaves, so the hand neither stops dead nor jumps by
        // a frame's move
        static thread_local float moveW[2] = {1.f, 1.f};
        static thread_local double moveAt[2] = {0, 0};
        const double nowS = NowS(), frameDt = std::clamp(nowS - moveAt[side], 0.0, 0.1);
        if (nowS - moveAt[side] > 0.004) moveAt[side] = nowS;
        const float moveGoal = o.pressed ? 0.f : 1.f;
        moveW[side] += (moveGoal - moveW[side]) * float(1.0 - std::exp(-frameDt / 0.04));
        if (moveW[side] > 0.001f) {
            // tracking's move since the step, from where tracking was then: not from where the body was sent, which the
            // arm's reach may have brought back -- the difference undid that, and a hand past the reach (into a railing
            // the shoulder could not get it to) was drawn where tracking was, through what the body stopped on
            for (int k = 0; k < 3; ++k) move[k] = (track[k] - o.sentTrack[k] - pm[k]) * moveW[side];
            if (o.touching || o.onWorld) {
                float b[3], bn = 0;
                for (int k = 0; k < 3; ++k) b[k] = o.sent[k] - o.raw[k], bn += b[k] * b[k];
                bn = std::sqrt(bn);
                if (bn > 0.001f) {
                    const float into = (move[0] * b[0] + move[1] * b[1] + move[2] * b[2]) / bn;
                    if (into > 0) for (int k = 0; k < 3; ++k) move[k] -= b[k] / bn * into;
                }
            }
        }
        // PRESSED, TRACKING'S MOVE ALONG THE SURFACE STILL PASSES; only the part into it and out of it is held. The body
        // a frame draws is a step old, and sliding a pressed hand on a wall at a quarter metre a second it was drawn 6 mm
        // behind tracking on average, 11 at the most, where the same hand only touching was 0.4 mm off. The way into the
        // surface is where the body was refused (its place less where it was sent, more than the press's 3 mm).
        if (o.pressed || moveW[side] < 0.999f) {
            float r[3], rn = 0;
            for (int k = 0; k < 3; ++k) r[k] = o.raw[k] - o.sent[k], rn += r[k] * r[k];
            rn = std::sqrt(rn);
            if (rn > 0.003f) {
                float full[3], fn = 0;
                for (int k = 0; k < 3; ++k) full[k] = track[k] - o.sentTrack[k] - pm[k], fn += full[k] * r[k] / rn;
                const float w = 1.f - moveW[side];
                for (int k = 0; k < 3; ++k) move[k] += (full[k] - r[k] / rn * fn) * w;
            }
        }
        float at[3], d[3], m[3];
        for (int k = 0; k < 3; ++k) at[k] = o.body[k] + move[k] + pm[k];
        {
            // THE CHANGE OF STATE, SOFTENED. Pressed, the hand stands still; let go it moves at tracking's pace at once: a
            // hand lifted off the floor at 0.8 m/s went from still to that in one frame (13-20 mm off its line), and the
            // same landing. From the press or the touch changing, the drawn hand is where it would be (the goal) plus an
            // OFFSET that dies away critically damped (kRelTau): at the change the offset is what the drawn hand would jump
            // by and its rate what its velocity would jump by (at most kRelRateMost), so neither jumps, and it only ever
            // shrinks toward the goal -- it never carries the hand past it.
            // As a velocity of its own changing by at most 5 m/s^2, a hand stopped dead after a quick swing that had read
            // a touch on the way went on past where it stopped at the swing's speed: 1.1 m off tracking for half a second
            // (2026-11-09).
            constexpr double kRelMost = 0.4;
            constexpr float kRelTau = 0.03f, kRelRateMost = 1.0f, kRelOffMost = 0.06f;
            // per hand, not per thread: the animation jobs that call this run on more than one thread, and state kept per
            // thread came apart (the follower ran on 1 frame of 3367)
            static float relAt[2][3] = {}, relV[2][3] = {}, relGoal[2][3] = {}, relE[2][3] = {}, relEV[2][3] = {},
                         relTrack[2][3] = {};
            static double relT[2] = {0, 0}, relFrom[2] = {-1, -1};
            static bool relHave[2] = {false, false}, relPressed[2] = {false, false}, relTouch[2] = {false, false},
                        relStart[2] = {false, false};
            const bool touch = o.touching || o.onWorld;
            const bool wasHeld = relPressed[side] || relTouch[side];
            // letting go only: a landing stops dead on what it lands on (eased, it would sink a few centimetres in first),
            // and a hand lagging as it lifts off is still above what it left
            if (relHave[side] && ((relPressed[side] && !o.pressed) || (relTouch[side] && !touch))) relFrom[side] = nowS, relStart[side] = true;
            relPressed[side] = o.pressed;
            relTouch[side] = touch;
            const double dtS = nowS - relT[side];
            if (relHave[side] && dtS < 0.004) {
                std::memcpy(at, relAt[side], sizeof(at));   // a second solve of the same frame: the same place
            } else {
                const float dt = float(dtS);
                const bool recent = relHave[side] && dtS < 0.1;
                float goalV[3] = {0, 0, 0};
                if (recent)
                    for (int k = 0; k < 3; ++k) goalV[k] = (at[k] - relGoal[side][k]) / dt;
                // ...and seen here, in the frame it happens: on something last frame, and the place it would be drawn
                // now moves off tracking by more than 0.25 m/s at once. The flags come from the step after, a frame late,
                // and the jump got through. In the air it never starts: there the hand is tracking, frame for frame. Nor
                // when tracking itself speeds up: a hand resting on a railing lifted at 0.5 m/s was held 14 mm under
                // tracking as if it had jumped, which a real hand had done too.
                if (recent && wasHeld && relFrom[side] < 0) {
                    float g = 0, v = 0, off2 = 0;
                    for (int k = 0; k < 3; ++k) {
                        const float off = (at[k] - track[k]) - (relGoal[side][k] - relTrack[side][k]);
                        g += goalV[k] * goalV[k], v += relV[side][k] * relV[side][k], off2 += off * off;
                    }
                    if (std::sqrt(g) > std::sqrt(v) + 0.25f && std::sqrt(off2) > 0.25f * dt) relFrom[side] = nowS, relStart[side] = true;
                }
                std::memcpy(relGoal[side], at, sizeof(at));
                std::memcpy(relTrack[side], track, sizeof(track));
                if (!recent) relFrom[side] = -1, relStart[side] = false;
                if (relStart[side]) {
                    // the offset and its rate at the change: last frame's drawn place carried on by its own velocity, and
                    // that velocity, against the goal's
                    relStart[side] = false;
                    float en = 0, vn = 0;
                    for (int k = 0; k < 3; ++k) {
                        relE[side][k] = relAt[side][k] + relV[side][k] * dt - at[k];
                        relEV[side][k] = relV[side][k] - goalV[k];
                        en += relE[side][k] * relE[side][k];
                        vn += relEV[side][k] * relEV[side][k];
                    }
                    en = std::sqrt(en);
                    vn = std::sqrt(vn);
                    if (en > kRelOffMost) for (auto& c : relE[side]) c *= kRelOffMost / en;
                    if (vn > kRelRateMost) for (auto& c : relEV[side]) c *= kRelRateMost / vn;
                    if (!std::isfinite(en + vn)) relFrom[side] = -1;
                }
                if (relFrom[side] >= 0 && nowS - relFrom[side] < kRelMost) {
                    // the exact step of a critically damped offset: x'' = -2w x' - w^2 x
                    const float w = 1.f / kRelTau, ex = std::exp(-w * dt);
                    float e2 = 0, v2 = 0;
                    for (int k = 0; k < 3; ++k) {
                        const float x = relE[side][k], v = relEV[side][k], c = v + w * x;
                        relE[side][k] = (x + c * dt) * ex;
                        relEV[side][k] = (v - w * c * dt) * ex;
                        e2 += relE[side][k] * relE[side][k];
                        v2 += relEV[side][k] * relEV[side][k];
                    }
                    for (int k = 0; k < 3; ++k) at[k] += relE[side][k], relV[side][k] = goalV[k] + relEV[side][k];
                    // caught up: tracking's own pace from here on
                    if (e2 < 0.0005f * 0.0005f && v2 < 0.03f * 0.03f) relFrom[side] = -1;
                } else {
                    relFrom[side] = -1;
                    std::memcpy(relV[side], goalV, sizeof(goalV));
                }
                // NEVER DRAWN DEEPER THAN TRACKING, OUT OF WHAT IT WAS PRESSED ON. Bonelab's hand is its body, which leaves
                // a surface in the step tracking does. Here the drawn hand is held at the latch while pressed and eased off
                // it after, and a hand lifted off a railing was drawn up to 38 mm under tracking for 150 ms. Along the way
                // out the press last had (the body's place less where it was sent), a drawn hand short of where a free
                // body would be is brought out to it at once. It only ever moves the hand toward tracking, so it can
                // neither push it in nor carry it past.
                {
                    static float outN[2][3] = {};
                    static double outAt[2] = {-1, -1};
                    float r[3], rn = 0;
                    for (int k = 0; k < 3; ++k) r[k] = o.raw[k] - o.sent[k], rn += r[k] * r[k];
                    rn = std::sqrt(rn);
                    if (o.pressed && rn > 0.003f) {
                        for (int k = 0; k < 3; ++k) outN[side][k] = r[k] / rn;
                        outAt[side] = nowS;
                    }
                    if (outAt[side] >= 0 && nowS - outAt[side] < 0.4) {
                        // where a free body would be: tracking, less what the arm's reach took off it at the step
                        const float* n = outN[side];
                        float e = 0;
                        for (int k = 0; k < 3; ++k) e += (track[k] - o.sentTrack[k] + o.sent[k] - at[k]) * n[k];
                        if (e > 0 && std::isfinite(e)) {
                            for (int k = 0; k < 3; ++k) at[k] += n[k] * e;
                            if (relFrom[side] >= 0) {
                                // the follower's offset loses that part too, and its rate back in
                                float back = 0;
                                for (int k = 0; k < 3; ++k) relE[side][k] += n[k] * e, back += relEV[side][k] * n[k];
                                if (back < 0) for (int k = 0; k < 3; ++k) relEV[side][k] -= n[k] * back;
                            }
                        }
                    }
                }
                std::memcpy(relAt[side], at, sizeof(at));
                relT[side] = nowS;
                relHave[side] = std::isfinite(at[0] + at[1] + at[2]);
            }
        }
        for (int k = 0; k < 3; ++k) d[k] = at[k] - CyberpunkVR_PlayerEntityPos[k];
        QConj(CyberpunkVR_PlayerEntityQuat, inv);
        QRot(inv, d, m);
        if (!std::isfinite(m[0] + m[1] + m[2])) return false;
        for (int k = 0; k < 3; ++k) modelPos[k] = m[k];
        NoteDrawn(side, at);
        NoteSlip(side, at, track, !o.touching && !o.onWorld && moveW[side] > 0.99f);
        const float traceLead = std::sqrt(move[0] * move[0] + move[1] * move[1] + move[2] * move[2]);
        // THE TURN. An empty hand's is its body's, turned on by tracking's turn since the step the same way. A gun's
        // hand's is tracking's in the air (the aim, not a step behind it) and its body's as the gun meets something,
        // eased between: a barrel on a wall turns away as the body is turned.
        float trackRot[4] = {0, 0, 0, 1};
        if (modelRot) std::memcpy(trackRot, modelRot, sizeof(trackRot));
        if (modelRot) {
            float step[4] = {0, 0, 0, 1}, q[4], r[4];
            if (moveW[side] > 0.001f) {
                float trackRot[4], sentInv[4];
                QMul(CyberpunkVR_PlayerEntityQuat, modelRot, trackRot);
                QConj(o.sentRot, sentInv);
                QMul(trackRot, sentInv, step);
                if (step[3] < 0) for (auto& c : step) c = -c;
                const bool unit = QNorm(step);
                const float sn = std::sqrt(step[0] * step[0] + step[1] * step[1] + step[2] * step[2]);
                if (unit && sn > 1e-7f) {
                    const float angle = 2.f * std::atan2(sn, step[3]) * moveW[side], h = std::sin(0.5f * angle) / sn;
                    step[0] *= h; step[1] *= h; step[2] *= h; step[3] = std::cos(0.5f * angle);
                } else {
                    step[0] = step[1] = step[2] = 0; step[3] = 1;
                }
            }
            QMul(step, o.bodyRot, q);
            QMul(inv, q, r);
            if (!QNorm(r)) return true;
            const float t = g_hasWeaponEquipped ? std::clamp(o.contact, 0.f, 1.f) : 1.f;
            float from[4] = {modelRot[0], modelRot[1], modelRot[2], modelRot[3]};
            if (from[0] * r[0] + from[1] * r[1] + from[2] * r[2] + from[3] * r[3] < 0) for (auto& c : from) c = -c;
            for (int k = 0; k < 4; ++k) r[k] = from[k] + (r[k] - from[k]) * t;
            if (QNorm(r)) for (int k = 0; k < 4; ++k) modelRot[k] = r[k];
            // THE TURN OFF TRACKING'S, EASED: in the air it is none and the hand turns as tracking does, frame for frame; a
            // body that meets a wall is turned by it at once (8-10 deg a frame, 2026-11-09) and that turn is followed
            // critically damped (30 ms, at most 4 rad/s) instead of shown in one frame. Per hand, not per thread.
            {
                static float offV[2][3] = {}, offX[2][3] = {};
                static double offAt[2] = {0, 0};
                float tinv[4], off[4], ov[3] = {0, 0, 0};
                QConj(trackRot, tinv);
                QMul(modelRot, tinv, off);
                if (off[3] < 0) for (auto& c : off) c = -c;
                const float on = std::sqrt(off[0] * off[0] + off[1] * off[1] + off[2] * off[2]);
                if (on > 1e-7f) {
                    const float a = 2.f * std::atan2(on, off[3]);
                    for (int k = 0; k < 3; ++k) ov[k] = off[k] / on * a;
                }
                const double odt = nowS - offAt[side];
                if (odt > 0.004) {
                    if (odt > 0.2) std::memcpy(offX[side], ov, sizeof(ov)), offV[side][0] = offV[side][1] = offV[side][2] = 0;
                    else
                        for (int k = 0; k < 3; ++k) Damped(offX[side][k], offV[side][k], ov[k], 1.f / 0.03f, float(odt), 4.f);
                    offAt[side] = nowS;
                }
                const float* x = offX[side];
                const float xn = std::sqrt(x[0] * x[0] + x[1] * x[1] + x[2] * x[2]);
                float shownOff[4] = {0, 0, 0, 1};
                if (xn > 1e-7f) {
                    const float h = std::sin(0.5f * xn) / xn;
                    shownOff[0] = x[0] * h; shownOff[1] = x[1] * h; shownOff[2] = x[2] * h; shownOff[3] = std::cos(0.5f * xn);
                }
                float fin[4];
                QMul(shownOff, trackRot, fin);
                if (QNorm(fin)) for (int k = 0; k < 4; ++k) modelRot[k] = fin[k];
            }
        }
        {
            // the drawn palm kept out of what is near (above), and the place given again
            float was[3] = {at[0], at[1], at[2]};
            KeepDrawnOut(side, o, at, modelRot);
            if (at[0] != was[0] || at[1] != was[1] || at[2] != was[2]) {
                float dd[3], mm[3], iv[4];
                for (int k = 0; k < 3; ++k) dd[k] = at[k] - CyberpunkVR_PlayerEntityPos[k];
                QConj(CyberpunkVR_PlayerEntityQuat, iv);
                QRot(iv, dd, mm);
                if (std::isfinite(mm[0] + mm[1] + mm[2])) for (int k = 0; k < 3; ++k) modelPos[k] = mm[k];
            }
        }
        if (o.trace) TraceFrame(side, at, o, traceLead, modelRot, moveW[side], modelRot ? trackRot : nullptr);
        return true;
    }
    for (int k = 0; k < 3; ++k) modelPos[k] += o.pos[k];
    if (o.held > 0) {
        // the body's place in model space, by the entity as it is this frame
        float d[3], inv[4], m[3];
        for (int k = 0; k < 3; ++k) d[k] = o.body[k] - CyberpunkVR_PlayerEntityPos[k];
        QConj(const_cast<const float*>(CyberpunkVR_PlayerEntityQuat), inv);
        QRot(inv, d, m);
        if (std::isfinite(m[0] + m[1] + m[2]))
            for (int k = 0; k < 3; ++k) modelPos[k] += (m[k] - modelPos[k]) * o.held;
    }
    if (modelRot) {
        float r[4];
        QMul(o.rot, modelRot, r);
        if (QNorm(r)) for (int k = 0; k < 4; ++k) modelRot[k] = r[k];
    }
    return true;
}

static float lastStepDt = 1.f / 60.f;   // physics thread: the step being simulated

// the hands' push on the player's body (physics thread writes, the player's move reads): world x y, m/s
struct Push { float v[2]; uint64_t at; };
static Seq<Push> bodyPushes;
// ...and the pull of the hands that hold on (climb), world x y z m/s
struct Pull { float v[3]; uint64_t at; bool on; };
static Seq<Pull> bodyPulls;
static bool climbOn[2] = {false, false}, gripWas[2] = {false, false};
static float climbAnchor[2][3] = {};   // where each hand took hold, the game's world
// ...and how it was turned then (PhysX's turn is the game's world's: the origin only moves things). THE HOLD IS FIXED, as
// the grenade's pin fixes the hand: the body is sent to this place and turn, and the drawn hand is them (Adjust).
static float climbTurn[2][4] = {{0, 0, 0, 1}, {0, 0, 0, 1}};
// a person's part held: its id, the capsule's middle and axis and the hand as they were taken hold of, when it was last
// seen, the hold as last placed (PhysX's space) and the controller off it (the game's world); a hold pulled off spends
// the grip until it is let go. The hand goes with the capsule's middle and the swing of its axis alone: a capsule's own
// turn is only its axis's shortest arc from x, which for an arm lying along -x (folded arms) turned half round about it
// from one step to the next, and the hold went round with it, 20-30 cm (2026-11-10).
static bool grabOn[2] = {false, false}, gripSpent[2] = {false, false};
static int grabOwner[2] = {0, 0};
// A capsule of the person a hand holds, near that hand (physics thread, PhysX's space): of the same person (the script's id
// over 32), its segment within its radius and 12 cm of the hand's bone.
bool HeldNear(int owner, const float* a, const float* b, float radius) {
    for (int sd = 0; sd < 2; ++sd) {
        if (!grabOn[sd] || grabOwner[sd] / 32 != owner / 32 || !actor[sd]) continue;
        PxTransform at{};
        At<void (*)(void*, PxTransform*)>(actor[sd], kGetGlobalPose)(actor[sd], &at);
        float ab[3], l2 = 0, t = 0;
        for (int k = 0; k < 3; ++k) {
            ab[k] = b[k] - a[k];
            l2 += ab[k] * ab[k];
            t += (at.p[k] - a[k]) * ab[k];
        }
        t = l2 > 1e-8f ? std::clamp(t / l2, 0.f, 1.f) : 0.f;
        float d2 = 0;
        for (int k = 0; k < 3; ++k) d2 += (a[k] + ab[k] * t - at.p[k]) * (a[k] + ab[k] * t - at.p[k]);
        if (d2 < (radius + 0.12f) * (radius + 0.12f)) return true;
    }
    return false;
}
// THE GRASP A HELD PART ASKS FOR (the script's, VRPhysHandsGrabStyle: 1 a can, 2 a can with the fingers wide, 3 gently,
// 4 the palm), for the part it names: the fingers are closed on that part's capsule as it lies in the hand.
static std::atomic<int> grabStyleOwner[2] = {0, 0}, grabStyle[2] = {0, 0};
static bool graspGiven[2] = {false, false};   // physics thread
// A person's part, its capsule (physics thread): pose (its axis x), radius and half height. False: not among them now.
bool PersonShape(int owner, PxTransform& pose, float& radius, float& halfHeight) {
    for (int i = 0; i < personNearN; ++i)
        if (personOwner[i] == owner) {
            struct { int type; float radius, halfHeight; } g{};
            std::memcpy(&g, personNear[i].geometry, sizeof(g));
            if (g.type != kGeomCapsule) return false;
            pose = personNear[i].pose;
            radius = g.radius;
            halfHeight = g.halfHeight;
            return true;
        }
    return false;
}
static PxTransform grabHand[2] = {}, grabLast[2] = {};
// THE SEAT: the hold's hand (grabHand) eased to this, once the grasp has seated the part (kSeatTau)
static PxTransform grabSeat[2] = {};
static bool grabSeating[2] = {false, false};
constexpr float kSeatTau = 0.05f;   // s
static float grabMid[2][3] = {}, grabAxis[2][3] = {};
static uint64_t grabSeenAt[2] = {0, 0};
static float grabPull[2][3] = {};
struct Grab { int owner; float pull[3]; uint64_t at; };
static Seq<Grab> grabs[2];
static std::atomic<bool> gripSpentShown[2] = {false, false};
static std::atomic<bool> holdsOnShown[2] = {false, false};   // climbOn, for the other threads
static float pushWant[2] = {0, 0}, pushShown[2] = {0, 0};

bool HoldsOn(int side) { return On(side) && holdsOnShown[side].load(std::memory_order_relaxed); }   // on the world or a person

bool BodyPull(float* velocity) {
    if (!velocity || !g_liveControls.xrHandPhysics || !g_liveControls.xrHandCollision || (!On(0) && !On(1))) return false;
    Pull p{};
    if (!bodyPulls.Read(p) || !p.on || GetTickCount64() - p.at > 100) return false;
    if (!std::isfinite(p.v[0] + p.v[1] + p.v[2])) return false;
    std::memcpy(velocity, p.v, sizeof(p.v));
    return true;
}

bool BodyPush(float* velocityXY) {
    if (!velocityXY || !g_liveControls.xrHandPhysics || !g_liveControls.xrHandCollision || (!On(0) && !On(1))) return false;
    Push p{};
    if (!bodyPushes.Read(p) || GetTickCount64() - p.at > 100) return false;
    if (!(std::fabs(p.v[0]) + std::fabs(p.v[1]) > 1e-4f) || !std::isfinite(p.v[0] + p.v[1])) return false;
    velocityXY[0] = p.v[0];
    velocityXY[1] = p.v[1];
    return true;
}

static void HoldSteps(void* sc, const float* origin, float dt);

void BeforeSimulate(void* sc, float dt) {
    if (!workerUp.load(std::memory_order_acquire) || !Prove() || !sc) return;
    if (dt > 1e-4f && dt < 0.1f) lastStepDt = dt;
    LARGE_INTEGER t0;
    QueryPerformanceCounter(&t0);
    float origin[3];
    if (sc != scene || !Origin(sc, origin)) return;   // the hands live in the world scene only
    HoldSteps(sc, origin, dt);
    Wanted w{};
    const bool fresh = wanted.Read(w) && GetTickCount64() - w.at < 300;
    Settings s{};
    settings.Read(s);
    const int holding = cvr::heldbody::HoldingMask();
    const uint64_t tick = GetTickCount64();
    PlayerPace();
    LooseCap(s);
    ApartStep(sc, dt > 1e-4f ? dt : 1.f / 60.f);
    DropStep(sc, dt > 1e-4f ? dt : 1.f / 60.f);
    TestStackStep(sc, origin, dt > 1e-4f ? dt : 1.f / 60.f);
    PeopleStep(origin, s);
    // V'S OWN BODY AND THE OTHER HAND: the other hand met while nothing is held (a bottle passed from one to the other) and
    // both hands have bodies (a gun held in both hands takes the support hand's away), with a gun out too: the free hand
    // stops on the gun's boxes and lies on them; V's own body while no gun is out (a reload reaches to the chest and the
    // belt). Taken up only while out of them -- given a body it is already in, PhysX throws the hand out of it
    {
        const bool armed = g_hasWeaponEquipped;
        for (int side = 0; side < 2; ++side) {
            const bool now = Scripted(side);
            if (now != scriptedOn[side])
                Log("[physhands] the %s hand is %s\n", side ? "right" : "left",
                    now ? "a script's: it meets neither the gun nor the other hand" : "its own again");
            scriptedOn[side] = now;
        }
        // RUNNING, THE HANDS DO NOT MEET EACH OTHER (nor the gun in the other): past 2.5 m/s, back under 1.5. Each body is
        // a part of a step behind its controller at a run, the two by different parts as each is pushed, and two hands
        // held together shook against each other at a sprint (2026-11-11: 102 frames of 112 over 5 mm); they meet again
        // as before (apart, or out of the gun) once the player slows.
        static bool running = false;
        {
            const float pace = std::sqrt(playerPace[0] * playerPace[0] + playerPace[1] * playerPace[1]);
            const bool was = running;
            running = running ? pace > 1.5f : pace > 2.5f;
            if (running != was) Log("[physhands] the player %s (%.1f m/s): the hands %s\n", running ? "runs" : "slows", pace,
                                    running ? "meet neither each other nor the gun in the other" : "may meet each other again");
        }
        const bool wantHands = selfBitsReady && s.handsMeet && !holding && actor[0] && actor[1] && simulating[0] && simulating[1] &&
                               !scriptedOn[0] && !scriptedOn[1] && !running;
        const bool handsWas = handsMeetOn;
        if (!wantHands) handsMeetOn = false;
        else if (!handsMeetOn && HandsApart()) handsMeetOn = true;
        if (handsMeetOn != handsWas) Log("[physhands] the hands %s each other\n", handsMeetOn ? "meet" : "no longer meet");
        handsMeetShown.store(handsMeetOn, std::memory_order_relaxed);
        for (int side = 0; side < 2; ++side) {
            const bool wantGun = selfBitsReady && s.weapon && armed && !holding && actor[0] && actor[1] && simulating[0] &&
                                 simulating[1] && weaponOn[side] > 0 && !scriptedOn[1 - side] && !running;
            const bool was = gunMeetOn[side];
            if (!wantGun) gunMeetOn[side] = false;
            else if (!gunMeetOn[side] && !OtherInGun(side)) gunMeetOn[side] = true;
            if (gunMeetOn[side] != was)
                Log("[physhands] the gun in the %s hand %s the other hand\n", side ? "right" : "left", gunMeetOn[side] ? "meets" : "no longer meets");
        }
        for (int side = 0; side < 2; ++side) {
            const bool want = selfBitsReady && s.self && !armed && actor[side] && simulating[side];
            const bool was = selfOn[side];
            if (!want) selfOn[side] = false;
            else if (!selfOn[side] && !InSelf(side)) selfOn[side] = true;
            if (selfOn[side] != was) Log("[physhands] the %s hand %s V's own body\n", side ? "right" : "left", selfOn[side] ? "meets" : "no longer meets");
            selfOnShown[side].store(selfOn[side], std::memory_order_relaxed);
        }
    }
    // THE PLAYER MOVED BY A TELEPORT (Bonelab resets its hands at the controllers when it teleports the rig): for a moment
    // both bodies are put where tracking is, step by step. Steered there, a body met what stood between and could end on
    // the far side of a wall the tracking frames of the move passed through.
    static float lastEntity[3] = {};
    static bool lastEntityValid = false;
    static uint64_t putUntil = 0, putFrom = 0;
    static float putAt[2][3] = {};
    {
        float d2 = 0;
        for (int k = 0; k < 3; ++k) d2 += (CyberpunkVR_PlayerEntityPos[k] - lastEntity[k]) * (CyberpunkVR_PlayerEntityPos[k] - lastEntity[k]);
        if (lastEntityValid && d2 > kTeleportStep * kTeleportStep) {
            putUntil = tick + kTeleportPutMs;
            putFrom = tick;
            Log("[physhands] the player moved %.2f m in a step (a teleport): the hands are put where tracking is for %llu ms "
                "or while it still jumps\n", std::sqrt(d2), (unsigned long long)kTeleportPutMs);
        }
        for (int k = 0; k < 3; ++k) lastEntity[k] = CyberpunkVR_PlayerEntityPos[k];
        lastEntityValid = std::isfinite(lastEntity[0] + lastEntity[1] + lastEntity[2]);
    }
    // Park on either transition, even if the scanner opened and closed between two physics steps.
    // Rearming therefore uses Steer's existing put-at-tracking path, never a pre-scan body/velocity.
    const uint64_t scannerEpoch = cvr::input::scannerHandPhysics.Epoch();
    const bool scannerReset = scannerEpoch != stepScannerEpoch;
    stepScannerEpoch = scannerEpoch;
    if (scannerReset) {
        pushWant[0] = pushWant[1] = pushShown[0] = pushShown[1] = 0;
    }
    // BOTH HANDS' TRACKING THIS STEP, read before either is steered, and the last step's: the hands-meet test below
    // compares one hand's way into the other with what the other's tracking did meanwhile.
    static float trackWas[2][3] = {};
    static bool trackWasValid[2] = {false, false};
    float trackIs[2][3] = {};
    bool trackIsValid[2] = {false, false};
    for (int k2 = 0; k2 < 2; ++k2) {
        PxTransform t{};
        trackIsValid[k2] = TargetNow(k2, origin, t);
        if (trackIsValid[k2]) std::memcpy(trackIs[k2], t.p, sizeof(trackIs[k2]));
    }
    for (int side = 0; side < 2; ++side) {
        PxTransform to{};
        const bool want = !scannerReset && cvr::input::scannerHandPhysics.Accepts(scannerEpoch) &&
                          g_liveControls.xrHandPhysics != 0 && g_liveControls.xrHandCollision != 0 &&
                          fresh && w.on[side] && filterReady && TargetNow(side, origin, to);
        if (!want) {
            // A parked body must release its climb/grab too, not keep pulling the player from a stale anchor.
            climbOn[side] = grabOn[side] = false;
            holdsOnShown[side].store(false, std::memory_order_relaxed);
            holdPoses[side].Write(HoldPose{});
            grabs[side].Write(Grab{});
            gripSpentShown[side].store(false, std::memory_order_relaxed);
            grabPull[side][0] = grabPull[side][1] = grabPull[side][2] = 0;
            if (graspGiven[side] && cvr::anim::GraspPerson(side, 0, nullptr, nullptr, 0.f, 0.f)) graspGiven[side] = false;
            Park(side);
            touchUntil[side] = 0;
            onWorld[side] = onPushable[side] = false;
            gripWas[side] = g_pSharedHands && g_pSharedHands[side ? 49 : vrshared::kLeftGripPressed] > 0.5f;
            farSince[side] = 0;
            continue;
        }
        if (!actor[side] && !Make(side, to, s)) {
            Park(side);
            continue;
        }
        // the settings that can change on a made body
        if (appliedMass[side] != s.mass) {
            SetMassAt(actor[side], s.mass, madeHalf[side], madeCentre[side]);
            appliedMass[side] = s.mass;
        }
        if (appliedCcd[side] != s.ccd) {
            // swept and speculative are not both taken (PhysX 3.4 drops the speculative one): the one off first
            void* a = actor[side];
            const auto flag = [&](int f, bool on) { At<void (*)(void*, int, bool)>(a, kSetRigidBodyFlag)(a, f, on); };
            if (s.ccd != 1) flag(kEnableCcd, false);
            if (s.ccd != 2) flag(kEnableSpeculativeCcd, false);
            if (s.ccd == 1) flag(kEnableCcd, true);
            if (s.ccd == 2) flag(kEnableSpeculativeCcd, true);
            appliedCcd[side] = s.ccd;
            Log("[physhands] %s hand: continuous collision %s\n", side ? "right" : "left", s.ccd == 1 ? "swept" : (s.ccd == 2 ? "speculative" : "off"));
        }
        if (appliedContactOffset[side] != s.contactOffset && shape[side]) {
            const float c = s.contactOffset > 0 ? s.contactOffset : ownContactOffset[side];
            const auto set = [&](void* sh) {
                if (sh) At<void (*)(void*, float)>(sh, kShapeSetContactOffset)(sh, c);
            };
            set(shape[side]);
            for (auto& finger : fingerShape[side])
                for (void* sh : finger) set(sh);
            set(forearmShape[side]);
            set(forearmUpper[side]);
            for (void* sh : weaponShape[side]) set(sh);
            appliedContactOffset[side] = s.contactOffset;
            Log("[physhands] %s hand's shapes: contact offset %.3f m (PhysX's own %.3f)\n", side ? "right" : "left", c, ownContactOffset[side]);
        }
        if (appliedDepenetration[side] != s.depenetration) {
            At<void (*)(void*, float)>(actor[side], kSetMaxDepenetration)(actor[side], s.depenetration);
            appliedDepenetration[side] = s.depenetration;
        }
        // THE PALM AND WHAT IT CLOSES ON. A hand closing on a thing (the grip past half), or holding one by its body, does
        // not meet the loose things: the one taken would fight it and its neighbours would be shoved. It meets them again a
        // moment after it opens, and only once it is in none of them: given them back inside a thing, PhysX threw it.
        const bool grip = g_pSharedHands && g_pSharedHands[side ? 49 : vrshared::kLeftGripPressed] > 0.5f;
        SetMaterials(side, grip);
        if (grip || (holding & (1 << side))) looseFrom[side] = tick + uint64_t(s.looseAfterGrip * 1000.f);
        bool meet = tick >= looseFrom[side];
        if (meet && !looseMet[side]) {
            PxTransform at{};
            At<void (*)(void*, PxTransform*)>(actor[side], kGetGlobalPose)(actor[side], &at);
            if (PalmInLoose(side, at)) {
                meet = false;
                looseFrom[side] = tick + 50;
                palmRefused.fetch_add(1, std::memory_order_relaxed);
            }
        }
        looseMet[side] = meet;
        {
            uint32_t palmF[4], fingerF[4], gunF[4], foreF[4];
            HandFilters(side, meet, palmF, fingerF, gunF, foreF);
            SetFilter(side, palmF, gunF, fingerF, foreF);
        }
        PoseShapes(side, s);
        if (tick < putUntil) {
            // the frames of a teleport move tracking's place about (the entity, its turn, the hands' solve come a frame
            // apart): put on while it still jumps, so the last put is where it settles
            float j2 = 0;
            for (int k = 0; k < 3; ++k) j2 += (to.p[k] - putAt[side][k]) * (to.p[k] - putAt[side][k]);
            if (j2 > 0.02f * 0.02f && tick - putFrom < kTeleportPutMostMs) putUntil = std::max(putUntil, tick + 100);
            std::memcpy(putAt[side], to.p, sizeof(putAt[side]));
        }
        std::memcpy(sentTrack[side], to.p, sizeof(to.p));
        std::memcpy(sentEntity[side], targetEntity[side], sizeof(sentEntity[side]));
        // THE HAND THAT HOLDS ON: the grip closed on a top (the way out of it up), on an edge the fingers curl over or on a
        // corner -- not on a wall's face -- and the body stays where it took hold while the grip lasts; the player follows
        // (below). The grip read through a 150 ms let-go (a grip that drops for a frame does not let go). Taken hold of
        // within 0.6 s of the press, with the hand within 1.5 cm of the world: at the press's step alone and only with the
        // body pushed by the world, a grip closed a moment before the hand came down on a railing, or with the hand resting
        // on it unpushed, took no hold (2026-11-10). A grip held as the hand comes onto a top or an edge catches it too (no
        // gun out). A hold pulled off by itself spends the grip: it takes nothing more until it is let go.
        static uint64_t gripOffAt[2] = {0, 0}, gripPressAt[2] = {0, 0}, worldFrom[2] = {0, 0}, worldOffAt[2] = {0, 0};
        static bool worldWas[2] = {false, false}, catchable[2] = {false, false};
        if (grip) gripOffAt[side] = 0;
        else if (!gripOffAt[side]) gripOffAt[side] = tick;
        if (grip && !gripWas[side]) gripPressAt[side] = tick;
        const bool holdGrip = grip || (gripOffAt[side] && tick - gripOffAt[side] < 150);
        if (!holdGrip) gripSpent[side] = false;
        // a touch that comes after 0.3 s off the world is the hand coming onto something; the touch's own flicker (its 60 ms
        // window as a hand rests on a wall) is not -- caught so, a palm on a wall with the grip closed took a hold
        if (onWorld[side] && !worldWas[side]) {
            worldFrom[side] = tick;
            catchable[side] = worldOffAt[side] && tick - worldOffAt[side] >= 300;
        }
        if (!onWorld[side] && worldWas[side]) worldOffAt[side] = tick;
        worldWas[side] = onWorld[side];
        const bool pressFresh = grip && !gripSpent[side] && tick - gripPressAt[side] < 600;
        const bool caught = grip && !gripSpent[side] && !g_hasWeaponEquipped && onWorld[side] && catchable[side] &&
                            tick - worldFrom[side] < 150 && gripPressAt[side] + 150 < worldFrom[side];
        PxTransform bodyNow{};
        At<void (*)(void*, PxTransform*)>(actor[side], kGetGlobalPose)(actor[side], &bodyNow);
        // A PERSON TAKEN HOLD OF: the grip closed with the palm or the fingers on a part of someone (the script names each of
        // their capsules) -- the hand stays on that part as it moves, a little way toward the controller (grab_give, eased
        // in), and lets go by itself once the controller is grab_release from it.
        if (grabOn[side]) {
            PxTransform cap{};
            const bool seen = PersonPose(grabOwner[side], cap);
            if (seen) grabSeenAt[side] = tick;
            bool off = !holdGrip;
            const char* why = "the grip opened 150 ms ago";
            if (!off && !seen && tick - grabSeenAt[side] > 250) off = true, why = "the person is gone";
            if (!off) {
                PxTransform hold = grabLast[side];
                if (grabSeating[side]) {
                    PxTransform& h = grabHand[side];
                    const PxTransform& w = grabSeat[side];
                    const float k = 1.f - std::exp(-(dt > 1e-4f ? dt : 1.f / 60.f) / kSeatTau);
                    const float sgn = h.q[0] * w.q[0] + h.q[1] * w.q[1] + h.q[2] * w.q[2] + h.q[3] * w.q[3] < 0 ? -1.f : 1.f;
                    float left = 0;
                    for (int k3 = 0; k3 < 3; ++k3) {
                        h.p[k3] += (w.p[k3] - h.p[k3]) * k;
                        left = std::max(left, std::fabs(w.p[k3] - h.p[k3]));
                    }
                    for (int k4 = 0; k4 < 4; ++k4) h.q[k4] += (w.q[k4] * sgn - h.q[k4]) * k;
                    QNorm(h.q);
                    if (left < 0.0005f) h = w, grabSeating[side] = false;
                }
                if (seen) {
                    const float ex[3] = {1.f, 0.f, 0.f};
                    float axis[3], swing[4], rel[3], r[3];
                    QRot(cap.q, ex, axis);
                    const float* a0 = grabAxis[side];
                    const float c = a0[0] * axis[0] + a0[1] * axis[1] + a0[2] * axis[2];
                    Cross(a0, axis, swing);
                    swing[3] = 1.f + c;
                    if (!(c > -0.95f) || !QNorm(swing)) swing[0] = swing[1] = swing[2] = 0.f, swing[3] = 1.f;
                    for (int k = 0; k < 3; ++k) rel[k] = grabHand[side].p[k] - grabMid[side][k];
                    QRot(swing, rel, r);
                    for (int k = 0; k < 3; ++k) hold.p[k] = cap.p[k] + r[k];
                    QMul(swing, grabHand[side].q, hold.q);
                    QNorm(hold.q);
                    // trace=1: the hold as it is put, every 150 ms (the game's world)
                    static uint64_t toldAt[2] = {0, 0};
                    if (s.trace && tick - toldAt[side] >= 150) {
                        toldAt[side] = tick;
                        Log("[physhands] %s hold on part %d: capsule middle %.3f %.3f %.3f (moved %.3f) axis %.2f %.2f %.2f (swung %.0f deg); "
                            "hold %.3f %.3f %.3f, controller %.3f %.3f %.3f\n", side ? "right" : "left", grabOwner[side] % 32,
                            cap.p[0] + origin[0], cap.p[1] + origin[1], cap.p[2] + origin[2],
                            std::sqrt((cap.p[0] - grabMid[side][0]) * (cap.p[0] - grabMid[side][0]) + (cap.p[1] - grabMid[side][1]) * (cap.p[1] - grabMid[side][1]) +
                                      (cap.p[2] - grabMid[side][2]) * (cap.p[2] - grabMid[side][2])),
                            axis[0], axis[1], axis[2], 2.f * std::acos(std::min(1.f, std::fabs(swing[3]))) / 0.0174533f,
                            hold.p[0] + origin[0], hold.p[1] + origin[1], hold.p[2] + origin[2], to.p[0] + origin[0], to.p[1] + origin[1],
                            to.p[2] + origin[2]);
                    }
                }
                grabLast[side] = hold;
                float d[3], dist = 0;
                for (int k = 0; k < 3; ++k) d[k] = to.p[k] - hold.p[k], dist += d[k] * d[k];
                dist = std::sqrt(dist);
                std::memcpy(grabPull[side], d, sizeof(d));
                if (!(dist <= s.grabRelease)) {
                    off = true;
                    gripSpent[side] = true;
                    why = "the controller pulled away from it";
                } else {
                    const float give = s.grabGive > 1e-4f ? s.grabGive * (1.f - std::exp(-dist / s.grabGive)) : 0.f;
                    const float k = dist > 1e-4f ? give / dist : 0.f;
                    for (int i = 0; i < 3; ++i) to.p[i] = hold.p[i] + d[i] * k;
                    // turned as the part is, a quarter of the way to the controller's turn
                    const float sgn = hold.q[0] * to.q[0] + hold.q[1] * to.q[1] + hold.q[2] * to.q[2] + hold.q[3] * to.q[3] < 0 ? -1.f : 1.f;
                    float q[4];
                    for (int i = 0; i < 4; ++i) q[i] = hold.q[i] + (to.q[i] * sgn - hold.q[i]) * 0.25f;
                    if (QNorm(q)) std::memcpy(to.q, q, sizeof(q));
                }
            }
            if (off) {
                grabOn[side] = false;
                Log("[physhands] %s hand lets go of a person (%s; their part %d, %.2f m from the controller)\n", side ? "right" : "left", why,
                    grabOwner[side] % 32, std::sqrt(grabPull[side][0] * grabPull[side][0] + grabPull[side][1] * grabPull[side][1] +
                                                    grabPull[side][2] * grabPull[side][2]));
            }
        } else if (pressFresh && s.grabPeople && !climbOn[side] && !(holding & (1 << side))) {
            int index = -1;
            if (PersonAt(side, bodyNow, index)) {
                const PxTransform& cap = personNear[index].pose;
                const float ex[3] = {1.f, 0.f, 0.f};
                QRot(cap.q, ex, grabAxis[side]);
                std::memcpy(grabMid[side], cap.p, sizeof(grabMid[side]));
                grabHand[side] = bodyNow;
                grabOn[side] = true;
                grabSeating[side] = false;
                grabOwner[side] = personOwner[index];
                grabSeenAt[side] = tick;
                grabLast[side] = bodyNow;
                float d2 = 0;
                for (int k = 0; k < 3; ++k) d2 += (to.p[k] - bodyNow.p[k]) * (to.p[k] - bodyNow.p[k]);
                Log("[physhands] %s hand takes hold of a person (their part %d, id %d), %.2f m from the controller\n", side ? "right" : "left",
                    grabOwner[side] % 32, grabOwner[side] / 32, std::sqrt(d2));
                to.p[0] = bodyNow.p[0], to.p[1] = bodyNow.p[1], to.p[2] = bodyNow.p[2];
            }
        }
        if (!grabOn[side]) grabPull[side][0] = grabPull[side][1] = grabPull[side][2] = 0;
        // the fingers on the part held: its capsule in the hand bone's frame (the body's), its axis x turned to z
        {
            const int style = grabOn[side] && grabStyleOwner[side].load(std::memory_order_relaxed) == grabOwner[side]
                                  ? grabStyle[side].load(std::memory_order_relaxed)
                                  : 0;
            PxTransform cap{};
            float radius = 0, half = 0;
            if (style > 0 && PersonShape(grabOwner[side], cap, radius, half)) {
                // in the hold's frame (the hand as the hold puts it, before the give toward the controller): the hand
                // a few cm ahead of the part as it is pulled is not the part moving in the hand
                const PxTransform& hf = grabLast[side];
                const float inv[4] = {-hf.q[0], -hf.q[1], -hf.q[2], hf.q[3]};
                const float ry[4] = {0.f, 0.70710678f, 0.f, 0.70710678f};
                float d[3], p[3], q[4], qz[4], seatP[3], seatQ[4];
                for (int k = 0; k < 3; ++k) d[k] = cap.p[k] - hf.p[k];
                QRot(inv, d, p);
                QMul(inv, cap.q, q);
                QMul(q, ry, qz);
                bool seated = false;
                if (QNorm(qz) && cvr::anim::GraspPerson(side, style, p, qz, half, radius, seatP, seatQ, &seated)) {
                    graspGiven[side] = true;
                    // SEATED: the hand moved by X = (the part in the hand) * (its seat)^-1, in the hand's frame -- the
                    // hold as it is this step taken on the capsule as it is now, and eased to itself times X
                    if (seated) {
                        const float sc[4] = {-seatQ[0], -seatQ[1], -seatQ[2], seatQ[3]};
                        float xq[4], xp[3], t[3];
                        QMul(qz, sc, xq);
                        QNorm(xq);
                        QRot(xq, seatP, t);
                        for (int k = 0; k < 3; ++k) xp[k] = p[k] - t[k];
                        const float ex[3] = {1.f, 0.f, 0.f};
                        QRot(cap.q, ex, grabAxis[side]);
                        std::memcpy(grabMid[side], cap.p, sizeof(grabMid[side]));
                        grabHand[side] = grabLast[side];
                        PxTransform& w = grabSeat[side];
                        QMul(grabLast[side].q, xq, w.q);
                        QNorm(w.q);
                        QRot(grabLast[side].q, xp, t);
                        for (int k = 0; k < 3; ++k) w.p[k] = grabLast[side].p[k] + t[k];
                        grabSeating[side] = true;
                        Log("[physhands] %s hand seats on the person's part %d for its grasp: moved %.3f m, turned %.0f deg\n",
                            side ? "right" : "left", grabOwner[side] % 32, std::sqrt(xp[0] * xp[0] + xp[1] * xp[1] + xp[2] * xp[2]),
                            2.f * std::acos(std::min(1.f, std::fabs(xq[3]))) / 0.0174533f);
                    }
                }
            } else if (graspGiven[side] && cvr::anim::GraspPerson(side, 0, nullptr, nullptr, 0.f, 0.f)) {
                graspGiven[side] = false;
            }
        }
        if (!holdGrip && climbOn[side]) {
            climbOn[side] = false;
            Log("[physhands] %s hand lets go (the grip opened 150 ms ago)\n", side ? "right" : "left");
        }
        else if (!grabOn[side] && !climbOn[side] && (pressFresh || caught) && s.climb && !(holding & (1 << side)) &&
                 TouchesWorld(side, bodyNow, s, kTouchWorld)) {
            // a top by the shapes round the hand, not by the way the solver last pushed it: with the fingers curling on a
            // wall that pointed up now and then, and a palm on a wall held on (2026-11-10)
            const bool edge = edgeWrap[side].load(std::memory_order_relaxed);
            const int feature = HoldFeature(side, bodyNow);
            const bool top = feature == 1;
            if (feature || edge) {
                for (int k = 0; k < 3; ++k) climbAnchor[side][k] = bodyNow.p[k] + origin[k];
                std::memcpy(climbTurn[side], bodyNow.q, sizeof(climbTurn[side]));
                climbOn[side] = true;
                Log("[physhands] %s hand holds on (%s%s) at %.2f %.2f %.2f\n", side ? "right" : "left",
                    top ? (onWorld[side] ? "a top" : "a top within reach") : feature == 2 ? "an edge or a corner" : "an edge the fingers curl over",
                    caught ? ", caught with the grip held" : (tick - gripPressAt[side] > 30 ? ", after the press" : ""),
                    climbAnchor[side][0], climbAnchor[side][1], climbAnchor[side][2]);
            }
        }
        gripWas[side] = grip;
        holdsOnShown[side].store(climbOn[side] || grabOn[side], std::memory_order_relaxed);
        {
            HoldPose hp{};
            hp.on = climbOn[side];
            std::memcpy(hp.p, climbAnchor[side], sizeof(hp.p));
            std::memcpy(hp.q, climbTurn[side], sizeof(hp.q));
            hp.at = GetTickCount64();
            holdPoses[side].Write(hp);
        }
        gripSpentShown[side].store(gripSpent[side], std::memory_order_relaxed);
        {
            Grab g{};
            g.owner = grabOn[side] ? grabOwner[side] : 0;
            std::memcpy(g.pull, grabPull[side], sizeof(g.pull));
            g.at = tick;
            grabs[side].Write(g);
        }
        if (climbOn[side]) {
            for (int k = 0; k < 3; ++k) to.p[k] = climbAnchor[side][k] - origin[k];
            std::memcpy(to.q, climbTurn[side], sizeof(to.q));
        }
        if (s.armReach && !climbOn[side] && !grabOn[side]) ArmReach(side, to, origin, tick, s);
        const bool put = tick < putUntil || Unstick(side, to, origin, holding, tick, s);
        // where the last step sent the body: a free body ended that step there, so its distance from it is what held it
        // (against tracking itself, a hand carried at a walk is a step's walk behind: 3-5 cm at 2-3 m/s, read as held)
        float prevSent[3];
        const bool hadSent = sentValid[side];
        std::memcpy(prevSent, sent[side] + 4, sizeof(prevSent));
        // THE OTHER HAND THIS ONE PRESSES ON: tracking's way into it taken out of where the body is sent, as a wall's is --
        // so a hand pushed at the other stops on it and the other, held still, is not shoved. Two bodies of a weight, each
        // sent on at a press's pace, the one moving carried the still one 8 cm off its controller (2026-11-10).
        if (handsMeetOn && !put) {
            // AGAINST THE OTHER HAND'S TRACKING, not the player's pace: a move both hands make together is no press. The
            // pace took out a run's step (2026-11-11) but not a stick's smooth turn, which sweeps both hands round the
            // player 3-4 cm a step at arm's length: read as a way into the other hand, the target was pulled back one
            // step and let go the next, and with the hands close the body's speed swung 1.0 / 0.02 m/s step to step --
            // the drawn hand shook 6-7 mm a frame through the whole turn (2026-11-14). The pace stays the fallback.
            const float stepDt = dt > 1e-4f ? dt : 1.f / 60.f;
            const int other = 1 - side;
            float together[3] = {playerPace[0] * stepDt, playerPace[1] * stepDt, playerPace[2] * stepDt};
            if (trackIsValid[other] && trackWasValid[other]) {
                float m2 = 0;
                for (int k = 0; k < 3; ++k) m2 += (trackIs[other][k] - trackWas[other][k]) * (trackIs[other][k] - trackWas[other][k]);
                if (m2 < 0.5f * 0.5f)
                    for (int k = 0; k < 3; ++k) together[k] = trackIs[other][k] - trackWas[other][k];
            }
            float n[4 * 24];
            const int ways = OtherHandOut(side, bodyNow, n, 24);
            for (int pass = 0; pass < 2 && ways; ++pass)
                for (int i = 0; i < ways; ++i) {
                    const float* w = n + 4 * i;
                    // (against the player: the other hand runs with it, and a run's 10 cm a step forward read as a way into
                    // the other hand's fingers held this one back, touching and letting go every few frames, 2026-11-11)
                    float into = 0;
                    for (int k = 0; k < 3; ++k) into -= (to.p[k] - bodyNow.p[k] - together[k]) * w[k];
                    const float past = into - w[3];
                    if (past > 0)
                        for (int k = 0; k < 3; ++k) to.p[k] += w[k] * past;
                }
        }
        Steer(side, to, dt > 1e-4f ? dt : 1.f / 60.f, s, put, !climbOn[side] && !grabOn[side]);
        for (int k = 0; k < 3; ++k) prevSent[k] += rodeBy[side][k];   // the last step's target rode with the body
        std::memcpy(sent[side], to.q, sizeof(to.q));
        std::memcpy(sent[side] + 4, to.p, sizeof(to.p));
        sentValid[side] = true;
        // HELD OFF WHERE TRACKING IS FREE (always on, a diagnostic): the body more than 3 cm off where the last step sent it,
        // for 250 ms, while tracking's palm is in nothing -- the hand could be at the controller and is not. Logged as it
        // starts (what is between, the flags, the settling) and as it ends (how long, how far at most).
        if (hadSent) {
            static uint64_t heldFrom[2] = {0, 0}, heldTold[2] = {0, 0}, heldEndTold[2] = {0, 0};
            static float heldMost[2] = {0, 0};
            static bool heldOn[2] = {false, false};
            PxTransform now{};
            At<void (*)(void*, PxTransform*)>(actor[side], kGetGlobalPose)(actor[side], &now);
            float g2 = 0;
            for (int k = 0; k < 3; ++k) g2 += (prevSent[k] - now.p[k]) * (prevSent[k] - now.p[k]);
            const float gap = std::sqrt(g2);
            {
                // HELD BY A LOOSE THING, THE HAND GOES THROUGH IT (HIGGS turns a hand's contacts off for a moment after a
                // throw): a crate or a chair the hand pushes and does not move held it up to 20 cm off the controller, and
                // a hand held by a loose thing is never put back by Unstick (that would throw it). More than 4 cm for
                // 150 ms, not holding it: the palm meets no loose thing for 0.4 s, and again only once it is out of them.
                static uint64_t looseHeldFrom[2] = {0, 0};
                const bool byLoose = tick < touchUntil[side] && !onWorld[side] && std::isfinite(gap) && gap > 0.04f && !put &&
                                     !(holding & (1 << side)) && looseMet[side];
                if (!byLoose) looseHeldFrom[side] = 0;
                else if (!looseHeldFrom[side]) looseHeldFrom[side] = tick;
                else if (tick - looseHeldFrom[side] >= 150) {
                    looseFrom[side] = tick + 400;
                    looseHeldFrom[side] = 0;
                    looseLetThrough.fetch_add(1, std::memory_order_relaxed);
                    static uint64_t toldAt[2] = {0, 0};
                    if (tick - toldAt[side] >= 1000) {
                        toldAt[side] = tick;
                        Log("[physhands] %s hand held %.0f mm off by a loose thing for 150 ms: let through it for 0.4 s\n",
                            side ? "right" : "left", 1000.f * gap);
                    }
                }
            }
            bool held = std::isfinite(gap) && gap > 0.03f && !put && !(holding & (1 << side));
            if (held) held = !TouchesWorld(side, to, s) && !PalmInLoose(side, to);
            if (held) {
                if (!heldFrom[side]) heldFrom[side] = tick, heldMost[side] = 0;
                heldMost[side] = std::max(heldMost[side], gap);
                if (!heldOn[side] && tick - heldFrom[side] >= 250) {
                    heldOn[side] = true;
                    if (tick - heldTold[side] >= 1000) {
                        heldTold[side] = tick;
                        const float off[3] = {madeCentre[side][0], madeCentre[side][1], madeCentre[side][2]};
                        float r[3], a[3], b[3], reach = 0;
                        QRot(now.q, off, r);
                        for (int k = 0; k < 3; ++k) a[k] = now.p[k] + r[k];
                        QRot(to.q, off, r);
                        for (int k = 0; k < 3; ++k) b[k] = to.p[k] + r[k], reach += (sentTrack[side][k] - to.p[k]) * (sentTrack[side][k] - to.p[k]);
                        char met[200] = "nothing";
                        if (PathFree(a, b, met, sizeof(met), 0.008f)) std::snprintf(met, sizeof(met), "nothing");
                        Log("[physhands] %s hand held %.0f mm off tracking %llu ms with tracking's palm in nothing: on the world %d, "
                            "touching %d, the arm's reach took %.0f mm, settling %.2f %.2f; between the palms: %s\n",
                            side ? "right" : "left", 1000.f * gap, (unsigned long long)(tick - heldFrom[side]), int(onWorld[side]),
                            int(tick < touchUntil[side]), 1000.f * std::sqrt(reach), settleLin[side], settleAng[side], met);
                    }
                }
            } else if (heldFrom[side]) {
                if (heldOn[side] && tick - heldEndTold[side] >= 1000) {
                    heldEndTold[side] = tick;
                    Log("[physhands] %s hand held off tracking for %llu ms, %.0f mm at the most, now %.0f mm off\n", side ? "right" : "left",
                        (unsigned long long)(tick - heldFrom[side]), 1000.f * heldMost[side], 1000.f * gap);
                }
                heldFrom[side] = 0;
                heldOn[side] = false;
            }
        }
        // THE HAND'S PUSH ON THE BODY: how far tracking is held into the world, along the way out of it, on the floor
        if (s.bodyPush && onPushable[side] && !put && !(holding & (1 << side)) && !climbOn[side] && !grabOn[side]) {
            PxTransform now{};
            At<void (*)(void*, PxTransform*)>(actor[side], kGetGlobalPose)(actor[side], &now);
            // only how far tracking is held in along the floor, the way out of the surface laid flat: the player moves
            // on the floor, and a press down that went in counted too. A palm pressed 2 cm down on a railing's rounded
            // top sent V 16 cm away along the floor, which did nothing to the press, until the arm's reach held the hand
            // 6 cm short of tracking. Scaled by how upright the surface is: a table's top pushes nobody.
            const float* n = pushedOut[side];
            const float nh = std::sqrt(n[0] * n[0] + n[1] * n[1]);
            if (nh > 0.2f) {
                const float into = -((to.p[0] - now.p[0]) * n[0] + (to.p[1] - now.p[1]) * n[1]) / nh;
                const float past = into - s.bodyPushDead;
                if (past > 0 && std::isfinite(past))
                    for (int k = 0; k < 2; ++k) pushWant[k] += n[k] * std::min(s.bodyPushGain * past, s.bodyPushMost);
            }
        }
    }
    for (int k2 = 0; k2 < 2; ++k2) {
        trackWasValid[k2] = trackIsValid[k2];
        if (trackIsValid[k2]) std::memcpy(trackWas[k2], trackIs[k2], sizeof(trackWas[k2]));
    }
    {
        // eased (80 ms), both hands' together, at most body_push_most; published for the player's move (Hooks/RoomscaleMove)
        const float stepDt = dt > 1e-4f ? dt : 1.f / 60.f;
        const float w = 1.f - std::exp(-stepDt / 0.08f);
        float m2 = pushWant[0] * pushWant[0] + pushWant[1] * pushWant[1];
        if (m2 > s.bodyPushMost * s.bodyPushMost) for (auto& c : pushWant) c *= s.bodyPushMost / std::sqrt(m2);
        for (int k = 0; k < 2; ++k) pushShown[k] += (pushWant[k] - pushShown[k]) * w, pushWant[k] = 0;
        Push p{};
        p.v[0] = pushShown[0];
        p.v[1] = pushShown[1];
        p.at = tick;
        bodyPushes.Write(p);
    }
    {
        // THE PLAYER PULLED AFTER THE HANDS THAT HOLD ON: so that tracking comes back to where each hand took hold
        // (closed in 60 ms, at most climb_most m/s), both hands' together; the feet rise no higher than 10 cm over the
        // highest hold -- onto a railing, not on up past it
        const float stepDt = dt > 1e-4f ? dt : 1.f / 60.f;
        float want[3] = {0, 0, 0}, topZ = -1e9f;
        int holds = 0;
        for (int side = 0; side < 2; ++side) {
            if (!climbOn[side]) continue;
            for (int k = 0; k < 3; ++k) want[k] += (climbAnchor[side][k] - (sentTrack[side][k] + origin[k])) / 0.06f;
            topZ = std::max(topZ, climbAnchor[side][2]);
            ++holds;
        }
        Pull p{};
        p.at = tick;
        p.on = holds > 0;
        if (holds) {
            for (auto& c : want) c /= float(holds);
            const float room = topZ + 0.10f - CyberpunkVR_PlayerEntityPos[2];
            if (want[2] > 0) want[2] = std::min(want[2], std::max(0.f, room) / stepDt);
            const float m = std::sqrt(want[0] * want[0] + want[1] * want[1] + want[2] * want[2]);
            if (m > s.climbMost) for (auto& c : want) c *= s.climbMost / m;
            if (std::isfinite(m)) std::memcpy(p.v, want, sizeof(p.v));
            static uint64_t toldAt = 0;
            if (s.trace && tick - toldAt >= 250) {
                toldAt = tick;
                Log("[physhands] holding on: pull %.2f %.2f %.2f m/s, the feet %.2f m under the highest hold\n", p.v[0], p.v[1], p.v[2], topZ - CyberpunkVR_PlayerEntityPos[2]);
            }
        }
        bodyPulls.Write(p);
    }
    Charge(beforeUs, beforeMostUs, t0);
}

// THE NEAREST POINT OF A BODY'S OWN SHAPES (a ragdoll's limb: its capsule, box or hull), for the carry of a body lying
// dead: the flesh toward the palm, the clothes being laid over it by the script.
static bool BodySurfaceInner(void* actor, const float* origin, const float* point, float* closest, float* distance) {
    if (!pointDistance || !actor) return false;
    PxTransform ap{};
    At<void (*)(void*, PxTransform*)>(actor, kGetGlobalPose)(actor, &ap);
    void* list[16]{};
    const uint32_t n = At<uint32_t (*)(void*, void**, uint32_t, uint32_t)>(actor, kGetShapes)(actor, list, 16, 0);
    const float pt[3] = {point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]};
    float best = 1e9f, bestP[3] = {0, 0, 0};
    for (uint32_t i = 0; i < n && i < 16; ++i) {
        void* sh = list[i];
        alignas(16) uint8_t g[96]{};
        bool ok = false;
        switch (At<int (*)(void*)>(sh, kShapeGetGeometryType)(sh)) {
        case kGeomBox: ok = At<bool (*)(void*, void*)>(sh, kShapeGetBox)(sh, g); break;
        case kGeomSphere: ok = At<bool (*)(void*, void*)>(sh, kShapeGetSphere)(sh, g); break;
        case kGeomCapsule: ok = At<bool (*)(void*, void*)>(sh, kShapeGetCapsule)(sh, g); break;
        case kGeomConvex: ok = At<bool (*)(void*, void*)>(sh, kShapeGetConvex)(sh, g); break;
        default: break;   // what pointDistance does not measure
        }
        if (!ok) continue;
        PxTransform lp{}, wp{};
        At<void (*)(void*, PxTransform*)>(sh, kShapeGetLocalPose)(sh, &lp);
        float t[3];
        QRot(ap.q, lp.p, t);
        for (int k = 0; k < 3; ++k) wp.p[k] = ap.p[k] + t[k];
        QMul(ap.q, lp.q, wp.q);
        if (!QNorm(wp.q)) continue;
        float c[3] = {pt[0], pt[1], pt[2]};
        const float d2 = pointDistance(pt, g, &wp, c);
        if (!std::isfinite(d2) || d2 < 0) continue;
        float m2 = 0;
        for (int k = 0; k < 3; ++k) m2 += (c[k] - pt[k]) * (c[k] - pt[k]);
        const float d = d2 > 0 ? std::sqrt(m2) : 0.f;   // (the square it returns, read from the point it gives)
        if (d < best) {
            best = d;
            if (d2 > 0) std::memcpy(bestP, c, sizeof(bestP));
            else std::memcpy(bestP, pt, sizeof(bestP));
        }
    }
    if (best > 1e8f) return false;
    for (int k = 0; k < 3; ++k) closest[k] = bestP[k] + origin[k];
    *distance = best;
    return true;
}

bool BodySurface(void* actor, const float* origin, const float* point, float* closest, float* distance) {
    __try {
        return BodySurfaceInner(actor, origin, point, closest, distance);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// A BODY HELD BY THE HAND AT THE POINT IT CLOSED ON, AS A PISTOL IS (HoldBody, the script's carry of a body lying dead).
// The point held goes with the hand bone, at every step, exactly: it is made the body's centre of mass, the body's mass and
// inertia raised so that the rest of the body pulling on it by its joints cannot drag it off the hand, and each step it is
// given the velocity that brings it there by the step's end. Its turn is the hand's grip: steered toward the turn the hand
// has it at by no more than the hand's torque (two hands on it: both), so a light part turns with the wrist and a body
// hanging from a leg turns the leg further by its weight -- and the drawn hand is turned with it (BearLoad), so the leg
// stays in the hand as it was taken, as a pistol does, and the hand gives to the weight. Held with its turn left loose, the
// leg slid round in the fingers as the wrist turned; held with it fixed, the leg was a lever (a foot lifted 59 cm had the
// hips 49 cm up, and the hand turned by a measured pull threw the body a metre up). Two hands on one body hold it together
// (its turn between theirs, the midpoint of the points held where the midpoint of the hands has it); two on two of its
// parts each hold their own. Let go, the body has its own centre, mass, inertia, damping and spin ceiling back and goes on
// as it moved. Its table and its (proxy, body) words are read before each touch: a ragdoll whose NPC streamed out is freed
// into PhysX's pool, which keeps its slabs (see Natives/HeldBody.cpp, Alive).
constexpr float kHoldEase = 0.15f;       // s: from where it lay in the hand to where the hold has it
constexpr float kHoldJump = 0.5f;        // m: a drawn hand this far from the last step's (a teleport) eases it in again
constexpr float kHoldMassScale = 20.f;   // the held body's mass and inertia while held, times its own...
constexpr float kHoldLeastMass = 100.f;  // ...and its mass no less than this (kg)
constexpr float kHoldMostSpeed = 20.f;   // m/s: the most the point held is given to catch the hand
constexpr float kHoldMostSpin = 40.f;    // rad/s: the most it is given to turn with the hand (its ceiling raised to it)
struct Held {
    void* actor;
    uint32_t proxy, body;
    int holders;
    PxTransform ownCentre;      // its own centre of mass, mass, inertia, damping and spin ceiling, put back when let go
    float ownMass, ownDamping, ownInertia[3], ownSpin;
    float madeMass, madeInertia[3];
    float centre[3];            // its centre of mass while held (its frame): the point held, or between two
    float yield[4];             // how far it is turned off the turn the hands have it at, the world
    uint32_t steps;
    float offSum, offMost, yieldMost;
    uint64_t saidAt;
};
Held helds[2] = {};
struct HoldRun {
    int held;                   // its Held, -1 none
    void* actor;
    uint32_t proxy, body;
    float wantP[3], wantQ[4];   // where the hold has it in the hand's frame (the script's, carried onto this body when taken)
    float fromP[3], fromQ[4];   // where it lay in the hand's frame when taken
    float t;                    // s since taken
    float handP[3];             // the hand at the last step, PhysX's frame
    bool hasLast;
    float grip[3];              // the point held, in the body's frame
    float torque;               // the hand's grip torque (N m)
    bool toOn;                  // where this hand has the body this step (PhysX's frame), and the body in the hand's frame
    PxTransform to;
    float relP[3], relQ[4];
};
HoldRun holdRun[2] = {};
bool holdRunReady = false;

static bool HeldAlive(const void* actor, uint32_t proxy, uint32_t body) {
    const auto* a = static_cast<const uint8_t*>(actor);
    return a && dll && *reinterpret_cast<const uintptr_t*>(a) == dll + kDynamicTable &&
           *reinterpret_cast<const uint32_t*>(a + 0x10) == proxy && *reinterpret_cast<const uint16_t*>(a + 0x14) == uint16_t(body);
}
static bool HeldPose(void* actor, PxTransform& out) {
    __try {
        At<void (*)(void*, PxTransform*)>(actor, kGetGlobalPose)(actor, &out);
        return std::isfinite(out.p[0] + out.p[1] + out.p[2] + out.q[0] + out.q[1] + out.q[2] + out.q[3]);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}
// Taken: its centre of mass moved to `centre`, its mass and inertia raised, no linear damping, its spin ceiling raised.
static bool HeldTake(void* actor, const float* centre, Held& h) {
    __try {
        At<void (*)(void*, PxTransform*)>(actor, kGetCMassLocalPose)(actor, &h.ownCentre);
        h.ownMass = At<float (*)(void*)>(actor, kGetMass)(actor);
        h.ownDamping = At<float (*)(void*)>(actor, kGetLinearDamping)(actor);
        h.ownSpin = At<float (*)(void*)>(actor, kGetMaxAngularVelocity)(actor);
        At<void (*)(void*, float*)>(actor, kGetInertia)(actor, h.ownInertia);
        if (!std::isfinite(h.ownMass) || h.ownMass <= 0.f || !std::isfinite(h.ownInertia[0] + h.ownInertia[1] + h.ownInertia[2]))
            return false;
        const float scale = std::max(kHoldMassScale, kHoldLeastMass / h.ownMass);
        h.madeMass = h.ownMass * scale;
        for (int k = 0; k < 3; ++k) h.madeInertia[k] = std::max(1e-4f, h.ownInertia[k]) * scale;
        PxTransform c = h.ownCentre;
        std::memcpy(c.p, centre, sizeof(c.p));
        std::memcpy(h.centre, centre, sizeof(h.centre));
        At<void (*)(void*, const PxTransform*)>(actor, kSetCMassLocalPose)(actor, &c);
        At<void (*)(void*, float)>(actor, kSetMass)(actor, h.madeMass);
        At<void (*)(void*, const float*)>(actor, kSetInertia)(actor, h.madeInertia);
        At<void (*)(void*, float)>(actor, kSetLinearDamping)(actor, 0.f);
        At<void (*)(void*, float)>(actor, kSetMaxAngularVelocity)(actor, std::max(h.ownSpin, kHoldMostSpin));
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}
// Its centre moved while held (a second hand on it, or one let go).
static bool HeldCentre(Held& h, const float* centre) {
    __try {
        PxTransform c = h.ownCentre;
        std::memcpy(c.p, centre, sizeof(c.p));
        At<void (*)(void*, const PxTransform*)>(h.actor, kSetCMassLocalPose)(h.actor, &c);
        std::memcpy(h.centre, centre, sizeof(h.centre));
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}
// Let go: its own centre, mass, inertia, damping and spin ceiling, its velocity carried from its centre while held to its own.
static bool HeldGive(const Held& h) {
    __try {
        void* actor = h.actor;
        PxTransform pose{};
        float v[3], w[3], d[3], off[3], add[3];
        At<void (*)(void*, PxTransform*)>(actor, kGetGlobalPose)(actor, &pose);
        At<void (*)(void*, float*)>(actor, kGetLinearVelocity)(actor, v);
        At<void (*)(void*, float*)>(actor, kGetAngularVelocity)(actor, w);
        for (int k = 0; k < 3; ++k) d[k] = h.ownCentre.p[k] - h.centre[k];
        QRot(pose.q, d, off);
        Cross(w, off, add);
        for (int k = 0; k < 3; ++k) v[k] += add[k];
        At<void (*)(void*, const PxTransform*)>(actor, kSetCMassLocalPose)(actor, &h.ownCentre);
        At<void (*)(void*, float)>(actor, kSetMass)(actor, h.ownMass);
        At<void (*)(void*, const float*)>(actor, kSetInertia)(actor, h.ownInertia);
        At<void (*)(void*, float)>(actor, kSetLinearDamping)(actor, h.ownDamping);
        At<void (*)(void*, float)>(actor, kSetMaxAngularVelocity)(actor, h.ownSpin > 0.f ? h.ownSpin : 7.f);
        if (std::isfinite(v[0] + v[1] + v[2])) At<void (*)(void*, const float*, bool)>(actor, kSetLinearVelocity)(actor, v, true);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}
// One step of a held body: velocity toward the requested grip point and a damped angular drive. Grip torque is
// measured against the original inertia; the solver's artificial inertia multiplier must not weaken it.
// `yield` is the current rotation error; `off` is the grip-position error.
static bool HeldDrive(Held& h, const PxTransform& to, float torque, float step, float* off, float* yield) {
    __try {
        void* actor = h.actor;
        PxTransform pose{};
        float v[3], w[3], d[3], at[3], want[3];
        At<void (*)(void*, PxTransform*)>(actor, kGetGlobalPose)(actor, &pose);
        At<void (*)(void*, float*)>(actor, kGetAngularVelocity)(actor, w);
        QRot(pose.q, h.centre, d);
        for (int k = 0; k < 3; ++k) at[k] = pose.p[k] + d[k];
        QRot(to.q, h.centre, d);
        float o2 = 0;
        for (int k = 0; k < 3; ++k) {
            want[k] = to.p[k] + d[k];
            v[k] = (want[k] - at[k]) / step;
            o2 += (want[k] - at[k]) * (want[k] - at[k]);
        }
        *off = std::sqrt(o2);
        const float vn = std::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
        if (vn > kHoldMostSpeed)
            for (float& c : v) c *= kHoldMostSpeed / vn;
        v[2] += 9.81f * step;
        float qi[4], dq[4], wt[3] = {0, 0, 0};
        QConj(to.q, qi);
        QMul(pose.q, qi, yield);   // it now, off the hand's turn
        QConj(pose.q, qi);
        QMul(to.q, qi, dq);
        if (dq[3] < 0)
            for (auto& c : dq) c = -c;
        const float sn = std::sqrt(dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]);
        if (sn > 1e-7f) {
            const float angle = 2.f * std::atan2(sn, dq[3]);
            for (int k = 0; k < 3; ++k) wt[k] = dq[k] / sn * angle;
        }
        // The grip follows the controller against the original physical inertia. The artificial mass/inertia
        // increase stabilizes connected ragdoll bodies; applying it here too weakens the wrist by 20x or more.
        cvr::held_grip::DriveAngular(wt, w, h.ownInertia, torque, step, kHoldMostSpin, w);
        if (!std::isfinite(v[0] + v[1] + v[2] + w[0] + w[1] + w[2])) return false;
        At<void (*)(void*, const float*, bool)>(actor, kSetLinearVelocity)(actor, v, true);
        At<void (*)(void*, const float*, bool)>(actor, kSetAngularVelocity)(actor, w, true);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

static void HoldUnrun(HoldRun& r) {
    r = HoldRun{};
    r.held = -1;
}
// This hand lets go: the body it held is its own again once no hand holds it.
static void HoldRelease(int side) {
    HoldRun& r = holdRun[side];
    if (r.held >= 0) {
        Held& h = helds[r.held];
        if (--h.holders <= 0) {
            const bool still = HeldAlive(h.actor, h.proxy, h.body) && !cvr::debugview::BodyGone(h.proxy, h.body);
            const bool given = still && HeldGive(h);
            Log("[physhands] %s hand lets go of the body it held (proxy %u body %u) after %u steps: %s; its centre ended a step %.3f "
                "m off the hands on average, %.3f at the most; turned off the hands' turn by its weight %.0f deg at the most\n",
                side ? "right" : "left", h.proxy, h.body, h.steps, !still ? "its body is gone" : given ? "its own again" : "PhysX refused",
                h.steps ? h.offSum / float(h.steps) : 0.f, h.offMost, h.yieldMost * 57.2958f);
            h = Held{};
        } else {
            Log("[physhands] %s hand lets go of the body both held (proxy %u body %u): the other holds it alone\n", side ? "right" : "left",
                h.proxy, h.body);
        }
    }
    HoldLoad none{};
    none.at = GetTickCount64();
    holdLoads[side].Write(none);
    HoldUnrun(r);
}

// One hand: its asking read, taken or let go, and where it has the body this step.
static void HoldSide(int side, void* sc, const float* origin, float step) {
    BodyHold h{};
    HoldRun& r = holdRun[side];
    r.toOn = false;
    const bool asked = !cvr::input::scannerHandPhysics.Active() && g_liveControls.xrHandPhysics != 0 && g_liveControls.xrHandCollision != 0 &&
                       bodyHolds[side].Read(h) && h.on && h.actor && h.scene == sc && GetTickCount64() - h.at < 300;
    const bool alive = asked && !cvr::debugview::BodyGone(h.proxy, h.body) && HeldAlive(h.actor, h.proxy, h.body);
    if (r.held >= 0 && (!alive || h.actor != r.actor)) HoldRelease(side);
    if (!alive) return;
    r.torque = h.torque;
    DrawnHand dh{};
    if (!drawnHands[side].Read(dh) || GetTickCount64() - dh.at > 200) return;   // not drawn: no new target this step
    float hp[3], d[3];
    for (int k = 0; k < 3; ++k) hp[k] = dh.p[k] - origin[k];
    const float* hq = dh.q;
    bool take = r.held < 0;
    if (!take && r.hasLast) {
        float j2 = 0;
        for (int k = 0; k < 3; ++k) j2 += (hp[k] - r.handP[k]) * (hp[k] - r.handP[k]);
        if (j2 > kHoldJump * kHoldJump) {
            take = true;
            Log("[physhands] %s hand jumped %.2f m holding a body: eased in again from where the body is\n", side ? "right" : "left", std::sqrt(j2));
        }
    }
    if (take) {
        PxTransform a{};
        if (!HeldPose(h.actor, a)) return;
        float hi[4];
        QConj(hq, hi);
        for (int k = 0; k < 3; ++k) d[k] = a.p[k] - hp[k];
        QRot(hi, d, r.fromP);
        QMul(hi, a.q, r.fromQ);
        if (!QNorm(r.fromQ)) return;
        if (r.held < 0) {
            // given in another body's frame (the one the script asked of): carried onto this one as the two lie now
            std::memcpy(r.grip, h.g, sizeof(r.grip));
            std::memcpy(r.wantP, h.p, sizeof(r.wantP));
            std::memcpy(r.wantQ, h.q, sizeof(r.wantQ));
            if (h.ref && h.ref != h.actor) {
                PxTransform b{};
                if (!HeldAlive(h.ref, h.proxy, h.refBody) || !HeldPose(h.ref, b)) return;
                // T = b^-1 a (this body in the asked one's frame); the hold's pose times T; the point held by T^-1
                float bi[4], t[3], tp[3], tq[4], g[3];
                QConj(b.q, bi);
                for (int k = 0; k < 3; ++k) t[k] = a.p[k] - b.p[k];
                QRot(bi, t, tp);
                QMul(bi, a.q, tq);
                if (!QNorm(tq)) return;
                QRot(h.q, tp, t);
                for (int k = 0; k < 3; ++k) r.wantP[k] = h.p[k] + t[k];
                QMul(h.q, tq, r.wantQ);
                if (!QNorm(r.wantQ)) return;
                float ti[4];
                QConj(tq, ti);
                for (int k = 0; k < 3; ++k) t[k] = h.g[k] - tp[k];
                QRot(ti, t, g);
                std::memcpy(r.grip, g, sizeof(r.grip));
            }
            // the other hand's body already: held by both
            int at = -1;
            for (int i = 0; i < 2; ++i)
                if (helds[i].actor == h.actor && helds[i].holders > 0) at = i;
            if (at >= 0) {
                ++helds[at].holders;
                Log("[physhands] %s hand holds the body the other holds too (proxy %u body %u): held by both, the pull shared\n",
                    side ? "right" : "left", h.proxy, h.body);
            } else {
                for (int i = 0; i < 2 && at < 0; ++i)
                    if (helds[i].holders <= 0) at = i;
                if (at < 0) return;
                Held& n = helds[at];
                n = Held{};
                if (!HeldTake(h.actor, r.grip, n)) return;
                n.actor = h.actor;
                n.proxy = h.proxy;
                n.body = h.body;
                n.holders = 1;
                float g2 = 0;
                for (int k = 0; k < 3; ++k) g2 += (r.wantP[k] - r.fromP[k]) * (r.wantP[k] - r.fromP[k]);
                Log("[physhands] %s hand holds a body (proxy %u body %u%s) at the point it closed on, as a pistol: %.1f kg carried as %.0f, "
                    "%.3f m from where the hold has it, eased in over %.0f ms; turned by a grip of %.0f N m\n",
                    side ? "right" : "left", h.proxy, h.body, h.ref && h.ref != h.actor ? ", the part's own, asked of another" : "", n.ownMass,
                    n.madeMass, std::sqrt(g2), kHoldEase * 1000.f, h.torque);
            }
            r.held = at;
            r.actor = h.actor;
            r.proxy = h.proxy;
            r.body = h.body;
        }
        r.t = 0;
        r.hasLast = false;
    }
    r.t += step;
    const float s = std::min(1.f, r.t / kHoldEase), e = s * s * (3.f - 2.f * s);
    const float sgn = r.fromQ[0] * r.wantQ[0] + r.fromQ[1] * r.wantQ[1] + r.fromQ[2] * r.wantQ[2] + r.fromQ[3] * r.wantQ[3] < 0 ? -1.f : 1.f;
    for (int k = 0; k < 3; ++k) r.relP[k] = r.fromP[k] + (r.wantP[k] - r.fromP[k]) * e;
    for (int k = 0; k < 4; ++k) r.relQ[k] = r.fromQ[k] + (r.wantQ[k] * sgn - r.fromQ[k]) * e;
    if (!QNorm(r.relQ)) return;
    QMul(hq, r.relQ, r.to.q);
    if (!QNorm(r.to.q)) return;
    QRot(hq, r.relP, d);
    for (int k = 0; k < 3; ++k) r.to.p[k] = hp[k] + d[k];
    std::memcpy(r.handP, hp, sizeof(r.handP));
    r.hasLast = true;
    r.toOn = true;
}

static void HoldSteps(void* sc, const float* origin, float dt) {
    if (!holdRunReady) {
        for (auto& r : holdRun) HoldUnrun(r);
        holdRunReady = true;
    }
    const float step = dt > 1e-4f && dt < 0.1f ? dt : 1.f / 60.f;
    for (int side = 0; side < 2; ++side) HoldSide(side, sc, origin, step);
    const uint64_t now = GetTickCount64();
    for (int i = 0; i < 2; ++i) {
        Held& h = helds[i];
        if (!h.actor || h.holders <= 0) continue;
        int sides[2], n = 0;
        for (int side = 0; side < 2; ++side)
            if (holdRun[side].held == i && holdRun[side].toOn) sides[n++] = side;
        if (!n) continue;
        const HoldRun& a = holdRun[sides[0]];
        PxTransform to = a.to;
        float centre[3];
        std::memcpy(centre, a.grip, sizeof(centre));
        if (n == 2) {
            // TWO HANDS ON IT: its turn between theirs, the midpoint of the points held where the midpoint of the hands has it
            const HoldRun& b = holdRun[sides[1]];
            const float sgn = a.to.q[0] * b.to.q[0] + a.to.q[1] * b.to.q[1] + a.to.q[2] * b.to.q[2] + a.to.q[3] * b.to.q[3] < 0 ? -1.f : 1.f;
            for (int k = 0; k < 4; ++k) to.q[k] = a.to.q[k] + b.to.q[k] * sgn;
            if (!QNorm(to.q)) continue;
            float ga[3], gb[3], mid[3], d[3];
            QRot(a.to.q, a.grip, ga);
            QRot(b.to.q, b.grip, gb);
            for (int k = 0; k < 3; ++k) {
                centre[k] = 0.5f * (a.grip[k] + b.grip[k]);
                mid[k] = 0.5f * (a.to.p[k] + ga[k] + b.to.p[k] + gb[k]);
            }
            QRot(to.q, centre, d);
            for (int k = 0; k < 3; ++k) to.p[k] = mid[k] - d[k];
        }
        float c2 = 0;
        for (int k = 0; k < 3; ++k) c2 += (centre[k] - h.centre[k]) * (centre[k] - h.centre[k]);
        if (c2 > 1e-6f && !HeldCentre(h, centre)) continue;
        float torque = 0;
        for (int j = 0; j < n; ++j) torque += holdRun[sides[j]].torque;
        float off = 0, yield[4] = {0, 0, 0, 1};
        if (!HeldDrive(h, to, torque, step, &off, yield)) continue;
        float least = 1e9f;
        for (int j = 0; j < n; ++j) least = std::min(least, holdRun[sides[j]].t);
        const bool eased = least > kHoldEase + 0.05f;
        if (eased) {
            ++h.steps;
            h.offSum += off;
            h.offMost = std::max(h.offMost, off);
            const float ys = std::sqrt(yield[0] * yield[0] + yield[1] * yield[1] + yield[2] * yield[2]);
            h.yieldMost = std::max(h.yieldMost, 2.f * std::atan2(ys, std::fabs(yield[3])));
        }
        if (now - h.saidAt > 2000 && eased) {
            h.saidAt = now;
            const float ys = std::sqrt(yield[0] * yield[0] + yield[1] * yield[1] + yield[2] * yield[2]);
            Log("[physhands] a body held by %s (proxy %u body %u): turned %.0f deg off the hand%s by its weight (a grip of %.0f N m), "
                "its centre %.3f m off\n", n == 2 ? "both hands" : sides[0] ? "the right hand" : "the left hand", h.proxy, h.body,
                2.f * std::atan2(ys, std::fabs(yield[3])) * 57.2958f, n == 2 ? "s" : "", torque, off);
        }
    }
}

bool ActorPoint(void* actor, const float* origin, const float* local, float* world) {
    PxTransform a{};
    if (!actor || !HeldPose(actor, a)) return false;
    float d[3];
    QRot(a.q, local, d);
    for (int k = 0; k < 3; ++k) world[k] = a.p[k] + d[k] + origin[k];
    return std::isfinite(world[0] + world[1] + world[2]);
}

bool HoldBody(int side, void* actor, void* actorScene, uint32_t proxy, uint32_t body, const float* pos, const float* rot,
              const float* grip, void* ref, uint32_t refBody) {
    if (side < 0 || side > 1) return false;
    static bool said[2] = {false, false};
    BodyHold h{};
    if (actor && pos && rot && grip && actorScene == cvr::physhands::scene) {
        h.actor = actor;
        h.scene = actorScene;
        h.proxy = proxy;
        h.body = body;
        h.ref = ref;
        h.refBody = refBody;
        std::memcpy(h.p, pos, sizeof(h.p));
        std::memcpy(h.q, rot, sizeof(h.q));
        std::memcpy(h.g, grip, sizeof(h.g));
        h.torque = pos[3];
        h.on = QNorm(h.q) && std::isfinite(h.p[0] + h.p[1] + h.p[2] + h.g[0] + h.g[1] + h.g[2] + h.torque);
    }
    h.at = GetTickCount64();
    bodyHolds[side].Write(h);
    if (actor && !h.on && !said[side]) {
        said[side] = true;
        Log("[physhands] %s hand cannot hold a body fast (proxy %u body %u): %s\n", side ? "right" : "left", proxy, body,
            actorScene != cvr::physhands::scene ? "not the hands' scene" : "no pose for it in the hand");
    }
    return actor ? h.on : true;
}

static void PublishHeldWrists(uint64_t now) {
    if (!holdRunReady) return;
    for (int side = 0; side < 2; ++side) {
        const HoldRun& r = holdRun[side];
        HoldLoad load{};
        load.at = now;
        if (r.held >= 0 && r.toOn) {
            const Held& held = helds[r.held];
            PxTransform pose{};
            if (held.actor && HeldAlive(held.actor, held.proxy, held.body) &&
                !cvr::debugview::BodyGone(held.proxy, held.body) && HeldPose(held.actor, pose) &&
                cvr::held_grip::WristRotation(pose.q, r.relQ, load.wrist)) {
                float g[3];
                QRot(r.relQ, r.grip, g);
                for (int k = 0; k < 3; ++k) load.seat[k] = r.relP[k] + g[k];
                load.on = true;
            }
        }
        holdLoads[side].Write(load);
    }
}

void AfterFetch(void* sc) {
    StartWorker();
    if (!Prove() || !sc) return;
    LARGE_INTEGER t0;
    QueryPerformanceCounter(&t0);
    float origin[3];
    // the world scene, asked again every so often: a load can bring another
    static uint64_t askedAt = 0;
    const uint64_t now = GetTickCount64();
    if (!scene || now - askedAt > 2000) {
        askedAt = now;
        void* world = WorldScene(origin);
        if (world != scene) {
            if (scene) Log("[physhands] the world scene changed (%p -> %p): the hands start again\n", scene, world);
            scene = world;
            // the bodies of a released scene were taken out of it, not released; they are put in the new one when made again
            for (int side = 0; side < 2; ++side) {
                actor[side] = nullptr;
                shape[side] = nullptr;
                simulating[side] = false;
                live[side].store(false);
            }
            for (int i = 0; i < kMostBodies; ++i) personActor[i] = personShape[i] = nullptr, personLive[i] = false;
            personNearN = 0;
            census = Census{};
            filterReady = false;
            // the world list's meshes were the old world's
            AcquireSRWLockExclusive(&nearLock);
            nearFront.store(-1, std::memory_order_release);
            ReleaseSRWLockExclusive(&nearLock);
            nearWalk = NearWalk{};
            looseN[0] = looseN[1] = 0;
            apartN = 0;
            looseWalk = LooseWalk{};
            looseCappedAt = 0;
            for (auto& d : drops) d.n = 0;
            for (int side = 0; side < 2; ++side) {
                for (auto& finger : fingerShape[side])
                    for (void*& sh : finger) sh = nullptr;
                forearmShape[side] = nullptr;
                forearmUpper[side] = nullptr;
                madeSkin[side] = -2;
                for (void*& sh : weaponShape[side]) sh = nullptr;
                weaponOn[side] = 0;
            }
        }
    }
    if (sc != scene || !Origin(sc, origin)) return;
    PublishHeldWrists(now);
    if (census.phase < 3) {
        CensusSlice(sc, origin);
        Charge(afterUs, afterMostUs, t0);
        return;
    }
    Step st{};
    st.scannerEpoch = stepScannerEpoch;
    liveScannerEpoch.store(st.scannerEpoch, std::memory_order_release);
    for (int side = 0; side < 2; ++side) {
        st.valid[side] = actor[side] && simulating[side] && sentValid[side] &&
                         cvr::input::scannerHandPhysics.Accepts(st.scannerEpoch);
        if (!st.valid[side]) continue;
        PxTransform p{};
        At<void (*)(void*, PxTransform*)>(actor[side], kGetGlobalPose)(actor[side], &p);
        std::memcpy(st.pose[side], p.q, sizeof(p.q));
        std::memcpy(st.pose[side] + 4, p.p, sizeof(p.p));
        std::memcpy(st.sent[side], sent[side], sizeof(sent[side]));
        std::memcpy(st.sentTrack[side], sentTrack[side], sizeof(sentTrack[side]));
        std::memcpy(st.sentEntity[side], sentEntity[side], sizeof(sentEntity[side]));
        live[side].store(true, std::memory_order_release);
    }
    for (int k = 0; k < 4; ++k) st.entityRot[k] = CyberpunkVR_PlayerEntityQuat[k];
    for (int k = 0; k < 3; ++k) st.origin[k] = origin[k];
    for (int side = 0; side < 2; ++side) {
        st.touching[side] = st.valid[side] && now < touchUntil[side];
        st.onWorld[side] = st.touching[side] && onWorld[side];
        if (!st.valid[side]) continue;
        At<void (*)(void*, float*)>(actor[side], kGetLinearVelocity)(actor[side], st.vel[side]);
        At<void (*)(void*, float*)>(actor[side], kGetAngularVelocity)(actor[side], st.spin[side]);
        {
            // the wrist's velocity, as the steering has it (PhysX's is the palm's middle's)
            float com[3], wc[3];
            QRot(st.pose[side], madeCentre[side], com);
            Cross(st.spin[side], com, wc);
            for (int k = 0; k < 3; ++k) st.vel[side][k] -= wc[k];
        }
    }
    st.when = NowS();
    st.dt = lastStepDt;
    st.at = now;
    static uint64_t serial = 0;
    st.serial = ++serial;
    // the loose things by the hands for the fingers, before the step is told: the worker solves both together
    PublishLooseNear(st);
    steps.Write(st);
    stepsSeen.fetch_add(1, std::memory_order_relaxed);
    if (stepped) SetEvent(stepped);
    // the world round the player for the fingers, a slice a step, while a hand is on
    Settings s{};
    settings.Read(s);
    if (s.fingers && penetration && (st.valid[0] || st.valid[1])) NearSlice(sc, origin);
    if (st.valid[0] || st.valid[1]) LooseSlice(sc, origin);
    Charge(afterUs, afterMostUs, t0);
}

void PublishFingers(int side, const FingerHand& hand) {
    if (side < 0 || side > 1) return;
    Settings s{};
    settings.Read(s);
    FingerHand h = hand;
    for (auto& c : h.f)
        for (float& r : c.radius)
            if (r > 0) r += s.skinMargin;
    fingerHands[side].Write(h);
}

bool FingerBends(int side, float bend[5][3], float flexAt[5][3]) {
    if (side < 0 || side > 1 || !On(side)) return false;
    Bends b{};
    if (!bends[side].Read(b) || !b.on || GetTickCount64() - b.at > 250 ||
        !cvr::input::scannerHandPhysics.Accepts(b.scannerEpoch)) return false;
    std::memcpy(bend, b.a, sizeof(b.a));
    if (flexAt) std::memcpy(flexAt, b.flex, sizeof(b.flex));
    return true;
}

void Taken(void* actor) { TakenFromStack(actor); }

void FilterAgain(void* sc, void* a) {
    if (proved > 0 && sc && a) At<void (*)(void*, void*)>(sc, kSceneResetFiltering)(sc, a);
}

bool HeldAlone(const uint32_t* original, uint32_t* out) {
    Settings s{};
    settings.Read(s);
    if (!s.heldAlone || !filterReady) return false;
    // the hands' own group, which no mask in the game has, and the world's mask: no loose thing reaches for it, it reaches
    // for none, and the hands do not meet it; the game's callback bits kept, so it still sounds and breaks on the world
    out[0] = handFilter[0];
    out[1] = handFilter[1];
    out[2] = handFilterStatic[2];
    out[3] = handFilterStatic[3] | (original[3] & 0xC0000000u);
    return true;
}

void TestStack(const float* worldAt, bool dump, bool dumpShapes) {
    static uint32_t serial = 0;
    StackAsk a{};
    a.dump = dump;
    a.shapes = dumpShapes;
    std::memcpy(a.at, worldAt, sizeof(a.at));
    a.at_ms = GetTickCount64();
    a.serial = ++serial;
    stackAsks.Write(a);
}

bool SetWeapon(int side, int count, const float* frame) {
    if (side < 0 || side > 1) return false;
    Weapon& w = weaponStaged[side];
    if (std::clamp(count, 0, kMostWeaponBoxes) != w.n) {
        static int said = 0;
        if (said++ < 24)
            Log("[physhands] the gun in the %s hand: %d boxes, its grip at (%.3f %.3f %.3f) in the hand bone's frame\n", side ? "right" : "left",
                count, frame ? frame[4] : 0.f, frame ? frame[5] : 0.f, frame ? frame[6] : 0.f);
    }
    w.n = std::clamp(count, 0, kMostWeaponBoxes);
    if (frame) std::memcpy(w.frame, frame, sizeof(w.frame));
    if (w.n > 0 && !QNorm(w.frame)) w.n = 0;
    w.at = GetTickCount64();
    weapons[side].Write(w);
    return true;
}

bool SetPerson(int index, const float* a, const float* b, float radius, int owner) {
    if (index < 0 || index >= kMostPeople || !std::isfinite(a[0] + a[1] + a[2] + b[0] + b[1] + b[2] + radius)) return false;
    Person& c = peopleStaged.c[index];
    std::memcpy(c.a, a, sizeof(c.a));
    std::memcpy(c.b, b, sizeof(c.b));
    c.r = radius;
    c.owner = owner > 0 ? owner : 0;
    return true;
}

bool SelfShape(int index, float* a, float* b, float* radius, int* meets) {
    SelfShown v{};
    if (!selfShown.Read(v) || GetTickCount64() - v.at > 300 || index < 0 || index >= v.n) return false;
    std::memcpy(a, v.c[index].a, sizeof(v.c[index].a));
    std::memcpy(b, v.c[index].b, sizeof(v.c[index].b));
    *radius = v.c[index].r;
    *meets = v.c[index].meets;
    return true;
}

int GrabbedPerson(int side, float* pull) {
    if (!On(side)) return 0;
    Grab g{};
    if (!grabs[side].Read(g) || GetTickCount64() - g.at > 200) return 0;
    if (pull) std::memcpy(pull, g.pull, sizeof(g.pull));
    return g.owner;
}

bool GripSpent(int side) { return side >= 0 && side < 2 && gripSpentShown[side].load(std::memory_order_relaxed); }

void SetGrabStyle(int side, int owner, int style) {
    if (side < 0 || side > 1) return;
    grabStyle[side].store(std::clamp(style, 0, 4), std::memory_order_relaxed);
    grabStyleOwner[side].store(owner, std::memory_order_relaxed);
}

void SetSelfPad(float metres) {
    if (std::isfinite(metres)) selfPad.store(std::clamp(metres, 0.f, 0.04f), std::memory_order_relaxed);
}

void SetPeople(int count) {
    peopleStaged.n = std::clamp(count, 0, kMostPeople);
    peopleStaged.at = GetTickCount64();
    people.Write(peopleStaged);
}

bool SetWeaponPart(int side, int index, const float* lo, const float* hi) {
    if (side < 0 || side > 1 || index < 0 || index >= kMostWeaponBoxes) return false;
    for (int k = 0; k < 3; ++k) {
        if (!std::isfinite(lo[k] + hi[k]) || hi[k] < lo[k] || hi[k] - lo[k] > 2.f) return false;
        weaponStaged[side].lo[index][k] = lo[k];
        weaponStaged[side].hi[index][k] = hi[k];
    }
    return true;
}

}  // namespace cvr::physhands

// VRPhysHandWeapon(side, count, at, turn) -> Bool: the gun in that hand (0 left, 1 right) is `count` boxes (given before by
// VRPhysHandWeaponPart), its frame at `at` (x y z) turned by `turn` (x y z w) in the hand bone's frame; count 0: none. Said
// again at least every second while it is held: a gun not heard of for 1.5 s is gone.
void VRPhysHandWeapon(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    int32_t side = 0, count = 0;
    RED4ext::Vector4 at{}, turn{};
    RED4ext::GetParameter(stack, &side);
    RED4ext::GetParameter(stack, &count);
    RED4ext::GetParameter(stack, &at);
    RED4ext::GetParameter(stack, &turn);
    stack->code++;
    const float frame[7] = {turn.X, turn.Y, turn.Z, turn.W, at.X, at.Y, at.Z};
    const bool ok = cvr::physhands::SetWeapon(side, count, frame);
    if (out) *out = ok;
}

// VRPhysHandsPerson(index, a, b) -> Bool: one capsule of the people near the hands, between a and b (the game's world), its
// radius in a.w; VRPhysHandsPeople(count) hands the first `count` over. Every frame: a list not renewed for 0.3 s is gone.
void VRPhysHandsPerson(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    int32_t index = 0;
    RED4ext::Vector4 a{}, b{};
    RED4ext::GetParameter(stack, &index);
    RED4ext::GetParameter(stack, &a);
    RED4ext::GetParameter(stack, &b);
    stack->code++;
    const float pa[3] = {a.X, a.Y, a.Z}, pb[3] = {b.X, b.Y, b.Z};
    // b.w: the script's id for this part of this person (person * 32 + part), 0 for a part not to be taken hold of
    const bool ok = cvr::physhands::SetPerson(index, pa, pb, a.W, std::isfinite(b.W) && b.W > 0.5f && b.W < 1.6e7f ? int(b.W + 0.5f) : 0);
    if (out) *out = ok;
}

// VRPhysHandsSelfShape(index, end) -> Vector4: V's own body as the hands meet it, capsule `index` (the game's world): end 0
// its first end with its radius in w, end 1 its other end with the hands that meet it in w (1 the left, 2 the right, 3
// both); w -1 past the last.
void VRPhysHandsSelfShape(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, RED4ext::Vector4* out, int64_t) {
    int32_t index = 0, end = 0;
    RED4ext::GetParameter(stack, &index);
    RED4ext::GetParameter(stack, &end);
    stack->code++;
    RED4ext::Vector4 v{};
    v.W = -1.f;
    float a[3], b[3], r = 0;
    int meets = 0;
    if (cvr::physhands::SelfShape(index, a, b, &r, &meets)) {
        const float* p = end ? b : a;
        v.X = p[0], v.Y = p[1], v.Z = p[2], v.W = end ? float(meets) : r;
    }
    if (out) *out = v;
}

// VRPhysHandsGrab(side) -> Vector4: the person's part that hand holds (the script's id in w, 0 none) and the controller's
// place off the hold (x y z, the game's world).
void VRPhysHandsGrab(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, RED4ext::Vector4* out, int64_t) {
    int32_t side = 0;
    RED4ext::GetParameter(stack, &side);
    stack->code++;
    float pull[3] = {0, 0, 0};
    const int owner = cvr::physhands::GrabbedPerson(side, pull);
    RED4ext::Vector4 v{};
    v.X = pull[0], v.Y = pull[1], v.Z = pull[2], v.W = float(owner);
    if (out) *out = v;
}

// VRPhysHandsSelfPad(metres) -> Bool: how much V's clothes add round V's own trunk for the hands (the script, from what is
// worn: a jacket's, a shirt's), 0 to 4 cm.
void VRPhysHandsSelfPad(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    float metres = 0;
    RED4ext::GetParameter(stack, &metres);
    stack->code++;
    cvr::physhands::SetSelfPad(metres);
    if (out) *out = true;
}

// VRPhysHandsGrabStyle(side, owner, style) -> Bool: the grasp the person's part `owner` (the script's id) asks for while
// that hand holds it: 1 a can, 2 a can with the fingers wide, 3 gently, 4 the palm; 0 the fingers as they lie.
void VRPhysHandsGrabStyle(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    int32_t side = -1, owner = 0, style = 0;
    RED4ext::GetParameter(stack, &side);
    RED4ext::GetParameter(stack, &owner);
    RED4ext::GetParameter(stack, &style);
    stack->code++;
    cvr::physhands::SetGrabStyle(side, owner, style);
    if (out) *out = side == 0 || side == 1;
}

void VRPhysHandsPeople(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    int32_t count = 0;
    RED4ext::GetParameter(stack, &count);
    stack->code++;
    cvr::physhands::SetPeople(count);
    if (out) *out = true;
}

// VRPhysHandsTestStack(at) -> Bool: a test tool -- the few light loose things nearest `at` (the game's world) stood one on
// another and put to sleep, the next PhysX step.
void VRPhysHandsTestStack(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    RED4ext::Vector4 at{};
    RED4ext::GetParameter(stack, &at);
    stack->code++;
    const float p[3] = {at.X, at.Y, at.Z};
    cvr::physhands::TestStack(p, at.W < 0.f, at.W <= -2.f);   // w < 0: log the loose things near instead; w <= -2: every shape
    if (out) *out = true;
}

// VRPhysHandWeaponPart(side, index, lo, hi) -> Bool: one box of the gun in that hand, its corners in the gun's own frame.
void VRPhysHandWeaponPart(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    int32_t side = 0, index = 0;
    RED4ext::Vector4 lo{}, hi{};
    RED4ext::GetParameter(stack, &side);
    RED4ext::GetParameter(stack, &index);
    RED4ext::GetParameter(stack, &lo);
    RED4ext::GetParameter(stack, &hi);
    stack->code++;
    const float a[3] = {lo.X, lo.Y, lo.Z}, b[3] = {hi.X, hi.Y, hi.Z};
    const bool ok = cvr::physhands::SetWeaponPart(side, index, a, b);
    if (out) *out = ok;
}
