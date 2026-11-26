#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The camera link's and camera math's exports (src/Camera/CameraLink.cpp and CameraMath.cpp, or their Rust port in
// rust/crates/cvr-rig/src/vrik/camera_link.rs and camera_math.rs): defined here for both builds, so the counters and
// switches keep their addresses whichever half runs the code.
#if !CVR_RUST_VRIK  // the Rust build's are rust/crates/cvr-rig/src/vrik/state_globals.rs
#include "Hooks/CameraPoseTransportState.hpp"
#include <atomic>
#include <cstdint>

extern "C" {
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_BodyCentreMatch{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_BodyCentreMiss{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_BodyLuaCentreMatch{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_BodyLuaCentreRejected{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PlacedPoseMatch{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PlacedPoseMiss{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PlacedPoseAmbiguous{0};
// Frames identified by a BIT-FOR-BIT match (the normal path) versus by nearest-neighbour (the fallback). Measured
// over a session: exact tracked match one for one, i.e. the engine hands the quaternion to the render camera verbatim.
__declspec(dllexport) uint64_t CyberpunkVR_DebugFinalExact = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugFinalApprox = 0;
// Equal orientations can occur while tracking position is moving: legacy quaternion ambiguity, not a count of
// equivalent full poses.
__declspec(dllexport) uint64_t CyberpunkVR_DebugFinalExactTies = 0;
// The same three numbers for the SECOND view, kept apart from MAIN's: if vrcamMatch tracks match one for one the eyes
// are drawn from the same frames; a gap IS the second eye's lag.
__declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamFinalMatch = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamFinalNoMatch = 0;
__declspec(dllexport) uint32_t CyberpunkVR_DebugVrcamFinalAge = 0;
// COHERENT HAND ANCHOR: a hand is reconstructed correctly only when the head pose and the head-relative offset come
// from the SAME instant (Locate publishes a framed recipe through CameraLink).
__declspec(dllexport) int CyberpunkVR_CoherentHandAnchor = 1;
// 1 = the render camera's basis rows are written transposed. MEASURED, not chosen (camera_math.rs /
// CameraMath.cpp WriteRenderCameraBasis); 0 keeps the straight form for comparison.
__declspec(dllexport) int CyberpunkVR_CamFinalRowOrder = 1;
// The pose identity's counters (src/Camera/PoseIdentity.cpp, or rust/crates/cvr-rig/src/vrik/pose_identity.rs).
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdComponent[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdCopied{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdCopyChanged{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdFinal[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdFinalMiss[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdVrikSource{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseIdVrikInputChanged{0};
// WorldSpaceBlendCamera's (src/Hooks/WorldSpaceBlendCamera.cpp, or rust/crates/cvr-rig/src/vrik/world_blend.rs).
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldBlendPoseCalls{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldBlendPoseApplied{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldBlendPoseMissing{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_WorldBlendPoseChanged{};
// CameraPoseTransport's (src/Hooks/CameraPoseTransport.cpp, or rust/crates/cvr-rig/src/vrik/pose_transport.rs).
__declspec(dllexport) PoseBlendDebug CyberpunkVR_PoseBlendDebug{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_PoseBlendDebugSeq{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_PoseMainReadMissing[2]{};
__declspec(dllexport) ExternalCameraDebug CyberpunkVR_ExternalCameraDebug[2]{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ExternalCameraDebugSeq[2]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ExternalSelectionStates[4]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ExternalNotify[3]{};
// PatchCamera's (src/Hooks/PatchCamera.cpp, or rust/crates/cvr-rig/src/vrik/patch_camera.rs).
__declspec(dllexport) uint64_t CyberpunkVR_DebugVehicleWorldRecompose = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugSceneWorldRecompose = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugTakeoverWorldRecompose = 0;
__declspec(dllexport) float CyberpunkVR_DebugFppSceneComfortLagDeg = 0.0f;
// LocateCamera's (src/Hooks/LocateCamera.cpp, or rust/crates/cvr-rig/src/vrik/locate_camera.rs).
__declspec(dllexport) int32_t  CyberpunkVR_AdsWeaponZoomSync = 1;
__declspec(dllexport) uint64_t CyberpunkVR_DebugAdsZoomWrites = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugAdsZoomRejects = 0;
__declspec(dllexport) float CyberpunkVR_DebugSceneComfortLagDeg = 0.0f;
__declspec(dllexport) uint32_t CyberpunkVR_DebugSceneComfortCuts = 0;
__declspec(dllexport) uint64_t CyberpunkVR_DebugSceneLabelPublished = 0;
// The rotation LocateCamera composed, held until MAIN's own position for the frame is known (the braindance push).
float g_bdPushQuat[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
}
#endif
#endif