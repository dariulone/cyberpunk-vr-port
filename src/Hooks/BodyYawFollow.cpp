// BodyYawFollow -- PHYSICAL BODY ROTATION: the character turns under the headset, the view does not
// turn with it. Gated by "Physical body rotation" in the overlay (vrport.ini
// xr_physical_body_rotation), off by default.
//
// THE GAME TURNS THE CHARACTER; THIS ONLY ASKS IT TO. The angle goes into the engine's own per-frame
// heading delta on foot -- the channel the snap turn already uses (src/Hooks/OnFootDeltaHead.cpp) --
// so the entity yaw moves, and with it the drawn mesh, the collision, the aim and the movement
// direction, because the engine moved them itself. That is not "through input": nothing synthesises a
// mouse or a stick; it is the same heading accumulator the snap turn writes.
//
// TWO ROUTES WERE TRIED FIRST AND BOTH FAILED. They are recorded because each looked obviously right:
//
//   1. state+0x1D0 at the store site sub_140336390. The only write to that field in the whole
//      function, so it looked like the source -- and it is a copy nothing propagates: with -45 deg
//      sitting in it, the game's own body heading stayed at the engine yaw to five digits.
//   2. The components' world rotations, pre-multiplied by Rz(offset) from PatchCamera's stub on
//      UpdateWorldTransforms. This one DID reach the transforms -- measured with a live scan: all 102
//      of the player's transform records carried our angle, and the body visibly turned.
//
//      IT WAS REJECTED FOR THE WRONG REASON, and the correction belongs here because the wrong reason
//      was recorded as measured. The hands rode along, and that was blamed on the transform route --
//      but the cause was a mismatch in LocateCamera that this route did not create and does not
//      depend on: the published view orientation is composed from `bodyGameForward`, which is the
//      camera component's pre-write quaternion, and the camera INHERITS its yaw from the parent
//      component this route rotates. So the published view carried E+offset while the camera we
//      composed (from the census value) carried E, and the hand offset from the head came out rotated
//      by the whole offset. The heading route had the same defect with the signs swapped -- published
//      E, drawn E-realign -- and fixing it once at the source fixed the hands in both.
//
//      What DOES rule this route out is the gameplay half, which no frame fix reaches: the engine
//      derives aim, movement direction, cover and the collision capsule from its own heading, not from
//      these transforms, so the body turns in the picture while the character still shoots and walks
//      the old way. Second, we do not own the set: 102 records plus the components, of which 2 pass
//      our hook per frame (measured 144/s at 72 fps), and whether the one the skinning uses is among
//      them -- and whether the engine recomputes it after our write -- is a race with its own pass
//      order, not an invariant. It remains the right route for a PURELY VISUAL body turn.
//
// WHERE THE CANCELLATION LIVES, AND WHY NOT IN THE RECENTER BASE. The heading also feeds the camera,
// so injecting into it would swing the view. The old on-foot code cancelled that with
// RotateBaseYaw(step): the frame loop reports the head relative to that base both ways --
//     relPos = RotateVector(conj(base.ori), headPos - base.pos)
//     relOri = conj(base.ori) * headOri
// -- so the head's orientation and its room position each lose what the heading gained, and the view,
// the play space and the head-local hand poses all stay put. Correct in the algebra, wrong in the
// ORDER: the heading changes inside the game tick while the base only takes effect on the next XR
// cycle, so for one frame the view swings by the whole step. That is the camera drift this feature
// was always reported to have.
//
// So the base is left alone -- recentring keeps working exactly as before -- and the cancellation is
// done on our side, in the same frame, where the view is composed:
//
//     body   yaw = E            (engine's own, ours included: E = E0 + realign)
//     view   yaw = E - realign  (PatchCamera, LocateCamera's head-offset recipe)
//     solve  yaw = E            (world->model in the pose path)
//
// The view is then exactly what it would have been had the body never turned, the play space is
// anchored to the heading that existed at recenter, and the hands need no compensation of their own:
// their poses are head-local against an untouched base, and the solve converting with the body's TRUE
// yaw puts the model-space target back at the controller. That last point is also self-correcting --
// the solve reads the yaw the engine actually ended up at (from the census), so a heading the engine
// clamps or eases still leaves the hands on the controllers.

#include "Core/VrCoreShared.hpp"
#include "Hooks/Hook.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Anim/CharacterRig.hpp"   // g_VREntityPos*: the player's world position for the publish
#include "Anim/VrikState.hpp"
#include "Camera/CameraLink.hpp"

#include <windows.h>
#include <atomic>
#include <cstdint>
#include <cmath>

extern void Log(const char* fmt, ...);

// ---- controls -----------------------------------------------------------------------------------
//
// The switch itself lives in LiveControls (xr_physical_body_rotation, persisted); this mirror exists
// so the pose path and the camera write have one plain symbol to test on their hot paths.
extern "C" __declspec(dllexport) int   CyberpunkVR_BodyYawFollow        = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairPublished = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairRejected = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairPhaseMiss = 0;
// 25 DEGREES OF FREE LOOK, and that is the only number this feature has.
//
// Inside the cone nothing is issued: you can glance around without the character turning, which is
// what a neck is for. Outside it the WHOLE residual goes in the frame it appears -- no rate limit, no
// hold timer, no per-frame ceiling, no stepping. The body therefore never lags the head by more than
// the cone, and it settles exactly on the cone edge rather than oscillating across it, because only
// the part beyond the edge is ever asked for.
//
// Tried at 5, which tracked the head almost rigidly, and at 0, where the body faces exactly where the
// head faces at all times. Both work; 25 is the one that leaves a neck.
extern "C" __declspec(dllexport) float CyberpunkVR_BodyYawFollowDeadDeg = 25.0f;

namespace {
std::atomic<uint32_t> s_nativePairSeq{0};
struct AtomicNativePair {
    std::atomic<float> camQuat[4]{};
    std::atomic<float> entityQuat[4]{};
    std::atomic<float> cameraMinusEntity[3]{};
    std::atomic<uint32_t> valid{0};
    std::atomic<uint32_t> consumerEpoch{0};
};
AtomicNativePair s_nativePair{};

void PublishNativePair(const cvr::camera::LocatedCameraFrame& camera,
                       const float entityPos[3], const float entityQuat[4],
                       uint32_t consumerEpoch) {
    const float dx = camera.worldPos[0] - entityPos[0];
    const float dy = camera.worldPos[1] - entityPos[1];
    const float dz = camera.worldPos[2] - entityPos[2];
    const float spanSq = dx * dx + dy * dy + dz * dz;
    const bool valid = std::isfinite(spanSq) && spanSq >= 1.0e-4f && spanSq < 9.0f;

    s_nativePairSeq.fetch_add(1u, std::memory_order_acq_rel);
    for (int i = 0; i < 4; ++i) {
        s_nativePair.camQuat[i].store(camera.worldQuat[i], std::memory_order_relaxed);
        s_nativePair.entityQuat[i].store(entityQuat[i], std::memory_order_relaxed);
    }
    s_nativePair.cameraMinusEntity[0].store(dx, std::memory_order_relaxed);
    s_nativePair.cameraMinusEntity[1].store(dy, std::memory_order_relaxed);
    s_nativePair.cameraMinusEntity[2].store(dz, std::memory_order_relaxed);
    s_nativePair.valid.store(valid ? 1u : 0u, std::memory_order_relaxed);
    s_nativePair.consumerEpoch.store(consumerEpoch, std::memory_order_relaxed);
    s_nativePairSeq.fetch_add(1u, std::memory_order_release);
    if (valid) ++CyberpunkVR_DebugVrikNativePairPublished;
    else ++CyberpunkVR_DebugVrikNativePairRejected;
}
}  // namespace

bool VRIK_ReadNativeTransformSnapshot(VrikTransformSnapshot* out) {
    if (!out) return false;
    const uint32_t expectedEpoch = g_VrikFrameEpoch.load(std::memory_order_relaxed);
    for (int tries = 0; tries < 4; ++tries) {
        const uint32_t s0 = s_nativePairSeq.load(std::memory_order_acquire);
        if (s0 == 0u) return false;
        if (s0 & 1u) continue;
        VrikTransformSnapshot tmp{};
        for (int i = 0; i < 4; ++i) {
            tmp.camQuat[i] = s_nativePair.camQuat[i].load(std::memory_order_relaxed);
            tmp.entityQuat[i] = s_nativePair.entityQuat[i].load(std::memory_order_relaxed);
        }
        for (int i = 0; i < 3; ++i) {
            tmp.cameraMinusEntity[i] =
                s_nativePair.cameraMinusEntity[i].load(std::memory_order_relaxed);
        }
        tmp.valid = s_nativePair.valid.load(std::memory_order_relaxed);
        const uint32_t consumerEpoch =
            s_nativePair.consumerEpoch.load(std::memory_order_relaxed);
        if (s_nativePairSeq.load(std::memory_order_acquire) == s0 &&
            consumerEpoch == expectedEpoch &&
            g_VrikFrameEpoch.load(std::memory_order_relaxed) == expectedEpoch) {
            *out = tmp;
            return true;
        }
    }
    return false;
}

// THE ACCUMULATED REALIGN, radians, game space about +Z: how much of the engine's current heading is
// ours rather than the player's own turning. LOAD-BEARING -- the view is composed from
// (engine yaw - this), and if it is wrong the view drifts by the error.
extern "C" __declspec(dllexport) float CyberpunkVR_BodyYawRealignRad = 0.0f;

// Readable live: the same realign in degrees, the head-against-body residual, and the two counters.
extern "C" __declspec(dllexport) float    CyberpunkVR_DebugBodyFollowOffsetDeg = 0.0f;
extern "C" __declspec(dllexport) float    CyberpunkVR_DebugBodyFollowErrDeg = 0.0f;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBodyFollowCalls = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugBodyFollowApplied = 0;

// THE BODY'S TRUE YAW, radians, game convention -- the engine's own value, read at the store site.
// The pose path converts world->model with this while the follower is on: model space IS the entity
// frame, so the entity's actual angle is the only correct converter, and the camera heading (which we
// deliberately hold back by the realign) is not it.
extern "C" __declspec(dllexport) float CyberpunkVR_BodyYawFinalRad = 0.0f;
extern "C" __declspec(dllexport) int   CyberpunkVR_BodyYawFinalValid = 0;
// The player's frame, published so nothing in the pose path has to ask CET for it.
extern "C" __declspec(dllexport) float CyberpunkVR_PlayerEntityPos[3]  = { 0.0f, 0.0f, 0.0f };
extern "C" __declspec(dllexport) float CyberpunkVR_PlayerEntityQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
extern "C" __declspec(dllexport) int   CyberpunkVR_PlayerEntityValid = 0;

namespace {

// The HMD's yaw relative to the recenter base, radians, about the XR vertical (+Y).
bool HeadYawRelBase(float* outYaw) {
    OpenXRHeadPose hp{};
    if (!OpenXRManager::Get().GetHeadPose(&hp) || !hp.valid) return false;
    const float y = hp.oriY, z = hp.oriZ, x = hp.oriX, w = hp.oriW;
    *outYaw = std::atan2(2.0f * (w * y + x * z), 1.0f - 2.0f * (y * y + z * z));
    return true;
}

}  // namespace

// THE STEP TO INJECT INTO THE ENGINE'S HEADING THIS FRAME, radians. Called once per frame from the
// on-foot heading hook, which is the game's own turn channel.
//
// THE ERROR IS HEAD-AGAINST-BODY, and that is what makes the loop closed. Measuring the HMD against
// the recenter base has no feedback in it -- the body turning does not change that number -- and the
// realign ran away at 119 deg in two seconds, wrapping through 180. What closes is the residual:
//
//     residual = hmdYawRelBase - realign
//
// because the view is (engine yaw - realign) * mappedHmd while the body carries the engine yaw, so
// the engine's own value cancels out of the difference and what is left is how far the head is turned
// relative to the body. MAPPING = +1, observed rather than derived: the axis map (XR y -> game z)
// predicts it, one build contradicted it, and that build had the recenter base spinning the whole
// world -- a direction cannot be judged against a rotating world.
// GIVE THE REALIGN BACK. Every consumer subtracts this accumulator unconditionally -- the camera
// write (PatchCamera.cpp), the body forward LocateCamera publishes, and the movement axes
// (OnFootMoveXY.cpp) -- so a value left standing after the follower stops issuing steps is a
// permanent yaw error in all three, and nothing self-corrects it: the loop that would unwind it is
// the same loop that is no longer running.
//
// THE BRANCH BELOW COULD NOT DO THIS, which is why this function exists. BodyYawFollowStep is
// called from exactly one place, OnFootDeltaHead.cpp, and that site returns BEFORE the call in both
// states that need the release -- `if (g_isInVehicle) return;` at the top and the `if (!bodyRot)`
// early-out -- while also being what SETS CyberpunkVR_BodyYawFollow. So by the time control reaches
// the step the flag is always 1 and the guard inside it is dead code.
//
// Two states need it, and the difference matters:
//
//   THE FEATURE SWITCHED OFF -- the view stops cancelling, which is a one-time step of whatever had
//   accumulated. That is the intended behaviour (the body keeps the rotation it was given and the
//   view stops pretending it did not happen); the alternative is carrying the offset for the session.
//
//   MOUNTED -- and here the freeze was the real defect. The follower does not run in a vehicle, so
//   the accumulator holds the value it had on foot while PatchCamera keeps subtracting it from a yaw
//   that now comes from the CAR. Enter a car looking aside and the whole drive is spent looking that
//   far off the road (the cone is 25 deg, so the residue is head yaw minus 25), with no way back:
//   the only thing that unwinds the accumulator is the on-foot loop. Released here, the vehicle view
//   is composed from the car's own heading and nothing else, and stepping out re-converges normally.
extern "C" void BodyYawFollowRelease() {
    if (CyberpunkVR_BodyYawRealignRad == 0.0f) return;
    CyberpunkVR_BodyYawRealignRad = 0.0f;
    CyberpunkVR_DebugBodyFollowOffsetDeg = 0.0f;
}

extern "C" float BodyYawFollowStep() {
    ++CyberpunkVR_DebugBodyFollowCalls;
    if (!CyberpunkVR_BodyYawFollow) {
        BodyYawFollowRelease();
        return 0.0f;
    }
    float hmdYaw = 0.0f;
    if (!HeadYawRelBase(&hmdYaw)) return 0.0f;

    float resid = hmdYaw - CyberpunkVR_BodyYawRealignRad;
    while (resid >  3.14159265f) resid -= 6.28318531f;
    while (resid < -3.14159265f) resid += 6.28318531f;
    CyberpunkVR_DebugBodyFollowErrDeg = resid * 57.2957795f;

    // Asymmetric on purpose, and that is what keeps it stable: only the part beyond the cone is
    // issued, so the body settles exactly on the cone edge, and looking back toward it shrinks the
    // residual by itself with nothing issued -- no unwinding, no oscillation across the edge.
    float cone = CyberpunkVR_BodyYawFollowDeadDeg * 0.01745329252f;
    if (cone < 0.0f) cone = 0.0f;
    float step = 0.0f;
    if (resid >  cone) step = resid - cone;
    else if (resid < -cone) step = resid + cone;
    if (step == 0.0f) return 0.0f;

    // NO CEILING, NO RATE, NO HOLD. The whole residual outside the cone goes in this frame, on the
    // user's call: the body is to be as fast as the channel can carry it. A 45 deg per-frame clamp
    // lived here briefly; the snap turn puts that much through this same channel in one frame anyway,
    // so the clamp only ever limited how fast a big head turn could be answered. If the engine ever
    // refuses part of a large delta the view would drift by the refused part -- that would show up as
    // the view creeping during fast turns, and nothing else looks like it.

    CyberpunkVR_BodyYawRealignRad += step;
    while (CyberpunkVR_BodyYawRealignRad >  3.14159265f) CyberpunkVR_BodyYawRealignRad -= 6.28318531f;
    while (CyberpunkVR_BodyYawRealignRad < -3.14159265f) CyberpunkVR_BodyYawRealignRad += 6.28318531f;
    CyberpunkVR_DebugBodyFollowOffsetDeg = CyberpunkVR_BodyYawRealignRad * 57.2957795f;
    ++CyberpunkVR_DebugBodyFollowApplied;
    return step;
}

// Called from the body-yaw store site (src/Hooks/BodyYawCensus.cpp), once per frame for the player.
// Publishes the body's own transform for the pose path; writes nothing into the game.
// THE POSITION COMES FROM THE ENGINE, IN THE SAME INSTANT AS THE YAW.
//
// It used to be copied from g_VREntityPos*, the CET push, while the yaw beside it came straight off
// the store site. That was corrected here, but it did not make g_lastLocate* a matching camera:
// LocateCamera publishes after animation, so animation sees camera(N-1) beside this entity(N).
// VRIK_ComputeCamModel therefore no longer subtracts these two absolutes; it consumes the coherent
// relative pair from one SetVRPlayerYaw push. This current engine position remains the right source
// for the camera-mount and script consumers below.
extern "C" void BodyYawFollowTick(float engineZ, float engineW, const float* enginePos) {
    float wz = engineZ, ww = engineW;
    if (ww < 0.0f) { wz = -wz; ww = -ww; }
    if (wz == 0.0f && ww == 0.0f) return;
    if (!enginePos) return;
    const float yaw = 2.0f * std::atan2(wz, ww);
    const float h = yaw * 0.5f;
    const float currentEntityQuat[4] = { 0.0f, 0.0f, std::sin(h), std::cos(h) };

    // At this point in frame N, LocateCamera has only published frame N-1. Pair that camera with
    // the entity saved by this callback in frame N-1, then roll the saved entity forward. Both
    // inputs advance on the engine frame clock; no CET/Lua update or slew-filter state participates.
    static bool s_previousEntityValid = false;
    static float s_previousEntityPos[3] = {};
    static float s_previousEntityQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
    static uint32_t s_previousEntityEpoch = 0;
    static uint32_t s_lastPairedCameraSeq = 0;
    const uint32_t currentEpoch = g_VrikFrameEpoch.load(std::memory_order_relaxed);
    cvr::camera::LocatedCameraFrame camera{};
    if (s_previousEntityValid && cvr::camera::LocatedCameraFrameRead(&camera) &&
        camera.sequence != s_lastPairedCameraSeq) {
        if (camera.frameEpoch == s_previousEntityEpoch) {
            PublishNativePair(camera, s_previousEntityPos, s_previousEntityQuat, currentEpoch);
        } else {
            ++CyberpunkVR_DebugVrikNativePairPhaseMiss;
        }
        s_lastPairedCameraSeq = camera.sequence;
    }
    for (int i = 0; i < 3; ++i) s_previousEntityPos[i] = enginePos[i];
    for (int i = 0; i < 4; ++i) s_previousEntityQuat[i] = currentEntityQuat[i];
    s_previousEntityEpoch = currentEpoch;
    s_previousEntityValid = true;

    CyberpunkVR_BodyYawFinalRad = yaw;
    CyberpunkVR_BodyYawFinalValid = 1;
    CyberpunkVR_PlayerEntityPos[0] = enginePos[0];
    CyberpunkVR_PlayerEntityPos[1] = enginePos[1];
    CyberpunkVR_PlayerEntityPos[2] = enginePos[2];
    CyberpunkVR_PlayerEntityQuat[0] = 0.0f;
    CyberpunkVR_PlayerEntityQuat[1] = 0.0f;
    CyberpunkVR_PlayerEntityQuat[2] = currentEntityQuat[2];
    CyberpunkVR_PlayerEntityQuat[3] = currentEntityQuat[3];
    CyberpunkVR_PlayerEntityValid = 1;
}

// FOR THE RECORD, a door that is real but not the one in: sub_140336390 calls [vt+0x40] on its state
// provider (vtable 0x142AEDBD8) right before storing the transform, handing it r8 = &position,
// r9 = &quaternion, and on that class the slot is a bare `retn` -- an adjust-my-transform hook the
// engine invokes every frame and nobody implements. Claiming it worked, but it never fired for the
// player (0 calls against 6552 counted player frames), so the player's state uses a provider of a
// different class. Still an extension point for other characters.
