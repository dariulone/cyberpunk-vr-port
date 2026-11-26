#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The exports of the body hooks -- the roomscale move, the body's heading, its follower, the body-yaw store site, the
// swimming action reader (src/Hooks/RoomscaleMove.cpp, BodyHeading.cpp, BodyYawFollow.cpp, BodyYawCensus.cpp, SwimmingInput.cpp, or with CVR_RUST_VRIK their
// Rust port in rust/crates/cvr-rig/src/vrik): defined here for both builds, so the switches and counters keep their
// addresses whichever half runs the code, and tools/vrik_tests links them as they are.
#if !CVR_RUST_VRIK  // the Rust build's are rust/crates/cvr-rig/src/vrik/state_globals.rs
#include "Hooks/RoomscaleDebug.hpp"
#include <atomic>
#include <cstdint>

extern "C" {
// ---- the roomscale move
// 1: the view keeps the headset's lead over a blocked capsule (up to the capsule's free footprint);
// 0: the old ledger, the refused distance is consumed and the view is held on the capsule.
__declspec(dllexport) int CyberpunkVR_HeadLeadsCapsule = 1;
// 1: the body strictly under the head (RoomscaleMovement.hpp, Movement::Begin's strict): every tick the capsule is asked
// for the head's whole lead, and the velocity readers other than the solver see it only as a walk (PhysicalWalkGate);
// 0: the follow -- the free zone, the critically damped follow and its settle.
__declspec(dllexport) int CyberpunkVR_BodyStrictUnderHead = 1;
__declspec(dllexport) RoomscaleDebug CyberpunkVR_RoomscaleDebug{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_RoomscaleDebugSeq{0};
__declspec(dllexport) int CyberpunkVR_RoomscaleHooksReady = 0;
__declspec(dllexport) uint64_t CyberpunkVR_SwimmingFeedbackReads[2]{};
__declspec(dllexport) uint64_t CyberpunkVR_SwimmingPropertyFeedbackReads{};
__declspec(dllexport) float CyberpunkVR_SwimmingRedirect[3]{};

// ---- the body's heading
__declspec(dllexport) int CyberpunkVR_BodyHeadingHooksReady = 0;
__declspec(dllexport) uint64_t CyberpunkVR_BodyHeadingCommits = 0;
__declspec(dllexport) uint64_t CyberpunkVR_BodyHeadingRequests = 0;
__declspec(dllexport) float CyberpunkVR_BodyTrackingYawRad = 0;
// Bounded fault-injection gate for live regression: skips requests, never the
// native adapter or its persistent state. Default zero; no UI/INI setting.
__declspec(dllexport) int CyberpunkVR_DebugBodyHeadingSkip = 0;

// ---- the follower
// The switch itself lives in LiveControls (xr_physical_body_rotation, persisted); this mirror exists
// so the pose path and the camera write have one plain symbol to test on their hot paths.
__declspec(dllexport) int   CyberpunkVR_BodyYawFollow        = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairPublished = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairRejected = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugVrikNativePairPhaseMiss = 0;
// FIVE DEGREES OF FREE LOOK, per the roomscale movement requirement. Glances inside the cone leave the body still.
// Crossing it starts a short body-only catch-up toward the centre, restoring the full free-look zone. Camera tracking
// consumes its independent reference immediately.
__declspec(dllexport) float CyberpunkVR_BodyYawFollowDeadDeg = 5.0f;
// Hybrid: the hands' share of the body's turn, 0 (head cone) .. 1 (tracked); fades between the two.
__declspec(dllexport) float CyberpunkVR_DebugHybridTrackedWeight = 0.0f;
// Hybrid: the hands' heading and the neck limit's pull on top of it (degrees, the follower's offset convention).
__declspec(dllexport) float CyberpunkVR_DebugHybridHandsDeg = 0.0f;
__declspec(dllexport) float CyberpunkVR_DebugHybridNeckPullDeg = 0.0f;
__declspec(dllexport) float CyberpunkVR_DebugHybridAlignDeg = 0.0f;   // hands' alignment to the body
__declspec(dllexport) int   CyberpunkVR_DebugHybridEstimate = 0;      // 1: the hands' estimate is valid
// The hands' estimator's last update (TrackedBodyYaw::Trace): delta, wristMean, headDelta, headLead, accepted, evidence,
// residual, target (radians / seconds / metres), then the gate bits as a float-sized integer
__declspec(dllexport) float CyberpunkVR_DebugTrackedTrace[8] = {};
__declspec(dllexport) uint32_t CyberpunkVR_DebugTrackedGates = 0;
// Legacy export name retained for tooling: committed body-to-tracking angle.
__declspec(dllexport) float CyberpunkVR_BodyYawRealignRad = 0.0f;
// Readable live: the same realign in degrees, the head-against-body residual, and the two counters.
__declspec(dllexport) float    CyberpunkVR_DebugBodyFollowOffsetDeg = 0.0f;
__declspec(dllexport) float    CyberpunkVR_DebugBodyFollowErrDeg = 0.0f;
__declspec(dllexport) uint64_t CyberpunkVR_DebugBodyFollowCalls = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugBodyFollowApplied = 0;
// THE BODY'S TRUE YAW, radians, game convention -- the engine's own value, read at the store site. The pose path
// converts world->model with this while the follower is on: model space IS the entity frame.
__declspec(dllexport) float CyberpunkVR_BodyYawFinalRad = 0.0f;
__declspec(dllexport) int   CyberpunkVR_BodyYawFinalValid = 0;
// The player's frame, published so nothing in the pose path has to ask CET for it.
__declspec(dllexport) float CyberpunkVR_PlayerEntityPos[3]  = { 0.0f, 0.0f, 0.0f };
__declspec(dllexport) float CyberpunkVR_PlayerEntityQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
__declspec(dllexport) int   CyberpunkVR_PlayerEntityValid = 0;

// ---- the body-yaw store site (census; the yaw is load-bearing: the solve converts world to model with it)
__declspec(dllexport) double   CyberpunkVR_DebugYawWriteMs = 0.0;
__declspec(dllexport) float    CyberpunkVR_EngineBodyYawZ  = 0.0f;
__declspec(dllexport) float    CyberpunkVR_EngineBodyYawW  = 1.0f;
__declspec(dllexport) int      CyberpunkVR_EngineBodyYawValid = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugYawWritesAll = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugYawWritesPlayer = 0;

// ---- the swimming action reader: its original, which the asm thunk (SwimmingActionLeafThunk.asm) calls
void* CvrSwimmingActionOriginal{};
}
#endif
#endif
