#pragma once

#include "Runtimes/OpenXRManager.hpp"   // OpenXRHeadPose

#include <cstdint>

// ================================================================================================
// CameraLink -- what the three camera patch sites hand each other, and the only way to touch it.
//
// This is NOT a hook. It registers nothing, patches nothing, and owns no trampoline. It exists so
// that the values LocateCamera, PatchCamera and FinalCamera pass between themselves have exactly
// one definition and one documented access protocol, now that the three live in separate
// translation units.
//
// THE OWNERSHIP RULE, and it is what keeps this file from becoming the monolith again:
//
//     State whose only readers are inside its own writing hook stays file-static in that hook's
//     file. State read by anyone else lives here. Nothing else goes in.
//
// If this header starts collecting per-hook diagnostic counters, the split has failed in exactly
// the way Core/VrCoreShared.hpp's own banner warns about.
//
// ---- WHY TWO OF THESE ARE FUNCTIONS AND THE REST ARE VARIABLES ---------------------------------
//
// The tree already settled this question in both directions. Core/LiveControls.hpp: forty volatile
// scalars, no accessors, because the `volatile` IS the whole synchronisation and a getter per
// scalar buys nothing. Core/Telemetry.hpp: the opposite, because those addresses are baked into
// machine code.
//
// The camera link splits on a third line: whether the access protocol can be expressed as a load.
// Two of these objects cannot be.
//
//   * The write quaternion is a SEQLOCK. Four floats written by one thread and read by another; a
//     reader that catches two from before a write and two from after gets a quaternion that
//     existed at no instant in time. The odd/even sequence counter is the contract, so the four
//     floats are not reachable and CamWriteQuatPublish / CamWriteQuatRead are.
//   * The write ring is a 16-entry history with a monotonic head. Finding a record means walking
//     backwards from an acquire-loaded head, and the id ordering is what disambiguates a tie.
//
// Making the raw objects unreachable is the structural gain. Today a "quick read of the ring" is
// one line away and would compile; after this it cannot be written outside this file.
//
// ---- WHAT THIS FILE DOES NOT OWN, BUT THE THREE HOOKS SHARE ------------------------------------
//
// OpenXRManager::AcquireFrameHeadSample -- the once-per-aim-epoch latch whose entire purpose is
// that LocateCamera and PatchCamera get the SAME struct. It stays in OpenXRManager; it is named
// here because it is co-owned by two of the three hooks and is the reason PatchCamera's
// compare-exchange on the aim epoch exists. A reader who takes the list below as the complete set
// of shared state will get that wrong.
//
// ---- THE INVARIANT WITH NO OTHER HOME ----------------------------------------------------------
//
// EXACTLY ONE OF LocateCamera AND PatchCamera MAY CALL PushRenderHeadPose.
//
// Locate publishes the frame's head-pose label only `if (!composeAtWrite)`; Patch publishes inside
// the branch gated on the same conjunction (CamWriteInPatch && CamComposeAtWrite). Publishing from
// both would let whichever ran last label the image with a pose that was never written into the
// camera -- which is the mismatch the compositor turns into judder.
//
// That is policy over two live-switchable exported ints, evaluated in two files that no longer see
// each other. It is written here because after the split there is no other file in which it is
// written at all.
//
// KNOWN DEFECT, recorded rather than quietly fixed: both flags exist to be toggled mid-session, the
// toggle is not atomic against the engine job threads, and the two sites evaluate the conjunction
// at different instants -- so a live flip can leave BOTH publishers active for an interval. The
// symptom is a burst of frames labelled with poses that were never written into any camera, from a
// build where every hook reports ok. Fixing it means one publisher by construction, which is a
// behaviour change and does not belong in a commit whose value is that it changes nothing.
// ================================================================================================

namespace cvr::camera {

// ---- the seqlock quaternion --------------------------------------------------------------------
void CamWriteQuatPublish(float x, float y, float z, float w);
// false when no consistent snapshot could be taken; `out` is then untouched.
bool CamWriteQuatRead(float out[4]);

// ---- the write ring ----------------------------------------------------------------------------
// File the composed quaternion next to the XR sample it came from.
void CamWriteRecordPush(const float q[4], const OpenXRHeadPose& p);
// Identify the frame's pose from the quaternion the engine is about to render with. `outAge` is how
// many writes back it was found, `outTies` how many records were within tolerance.
bool CamWriteRecordFind(const float q[4], OpenXRHeadPose* out, uint32_t* outAge, uint32_t* outTies);

// ---- the located camera frame used by native VRIK pairing -------------------------------------
//
// Animation for frame N runs before LocateCamera publishes frame N. BodyYawFollow can therefore
// pair this publication with the entity transform it saved on the preceding player tick: both are
// frame N-1. Keep this separate from g_lastLocate*: those legacy scalar mirrors are read by several
// diagnostics without a common lock and cannot form a legal cross-thread C++ snapshot.
struct LocatedCameraFrame {
    float worldPos[3];
    float worldQuat[4];
    uint32_t sequence;
    uint32_t frameEpoch;
};
void LocatedCameraFramePublish(const LocatedCameraFrame& f);
// false when no frame has been published yet or a consistent read could not be taken.
bool LocatedCameraFrameRead(LocatedCameraFrame* out);

// ---- the view frame handed to the solve --------------------------------------------------------
//
// ONE STRUCT, ONE INSTANT, AND THAT IS THE WHOLE POINT.
//
// The solve used to read these fields out of shared memory ([104..111], [141], [227..230]) even
// though the producer lives in this same DLL -- a leftover from when VRIK was a second plugin. The
// hop is removable, but only as a WHOLE: every field here is published together and differenced
// against the others downstream, so taking them from separate live objects mixes instants inside
// one frame of reference and lands as a body/hand offset. (Tried it field by field; it broke the
// hand anchor, twice. Recorded so it is not retried that way.)
//
// So the producer fills this in one go, under a seqlock, at the same point it fills the shared
// slots -- and the shared slots stay, because CET/redscript read them across a real boundary.
struct ViewFrame {
    float viewQuat[4];      // composed heading * HMD, game axes -- what [104..107] carried
    float worldDelta[3];    // head displacement still to be added by the consumer -- [108..110]
    float deltaSemantics;   // [111]: 2 = the delta form above
    float headingRad;       // [141]
    float headOri[4];       // the head orientation the view was composed from, XR axes -- [227..230]
    float stampMs;          // [68], for the age census
};
void ViewFramePublish(const ViewFrame& f);
// false when no consistent snapshot could be taken, or nothing has been published yet.
bool ViewFrameRead(ViewFrame* out);

// ---- the barrel-direction packet ---------------------------------------------------------------
//
// ONE INSTANT AGAIN, for a different pair of quantities (dabinn, TofuExpress 821e8a4e).
//
// The barrel dot is the muzzle's world direction projected through the camera the frame is rendered
// with. The overlay used to take those two from different places at different times: the latest
// LOCATED quaternion, read at Present, against the muzzle slots, sampled by CET on its own tick.
// Turn the head quickly and the two describe different moments, and the dot smears into a
// velocity-dependent trail of stale positions; a reader can even straddle the four-float quaternion
// write and get a rotation that never existed.
//
// So both are published together, at the final-camera callback, which is also the better camera:
// what MAIN renders with, not what we asked for -- the located value goes through the camera mixer
// and is scaled by the camera weight, so it is not necessarily what ends up on screen.
//
// This is data coherence and nothing else. No smoothing, no prediction: a trail made of real
// samples from the wrong instants is fixed by taking one instant, not by averaging.
struct BarrelFrame {
    float camQuat[4];     // the quaternion MAIN is rendering this frame with, game axes
    float muzzleFwd[3];   // the weapon's world forward, latched (see the publisher)
};
void BarrelFramePublish(const BarrelFrame& f);
// false when nothing consistent could be read, or nothing has been published yet.
bool BarrelFrameRead(BarrelFrame* out);

}  // namespace cvr::camera

// The identification counters. Exported so they can be read live, and defined beside the search
// that writes them.
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugFinalExact;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugFinalApprox;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugFinalExactTies;
// The same identification, counted for the SECOND view and kept apart from MAIN's so the two can be
// compared. Tracking one for one means both eyes are drawn from the same frames; a gap IS the second
// eye's lag, which was invisible while that eye borrowed MAIN's label.
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugVrcamFinalMatch;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugVrcamFinalNoMatch;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_DebugVrcamFinalAge;
