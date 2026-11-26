//! THE BODY HOOKS', CAMERA LINK'S, HAPTICS', TANK'S AND WEAPON AIM'S SHARED STATE (formerly src/Hooks/BodyHooksState.cpp,
//! src/Camera/CameraLinkState.cpp, src/Runtimes/HapticsState.cpp, src/Hooks/PanzerState.cpp and
//! src/Hooks/WeaponAimState.cpp, which the reference build keeps): their switches, tuning and diagnostic read-outs, with the same C names, types and initial values (every
//! symbol's initial bytes compared with the C++ objects' when this file was written, tools/rust_migration/coff_data.py).
//! The C++ half declares them `extern "C"` in their headers; the `CyberpunkVR_*` ones are exported by name
//! (rust/exported-data.txt), as their `__declspec(dllexport)` definitions were. The read-out structs the C++ headers
//! define (RoomscaleDebug, PoseBlendDebug, ExternalCameraDebug, the tank's) are kept here as their bytes.
#![allow(non_upper_case_globals)]
use core::sync::atomic::*;

/// A C++ object kept as its bytes: its C++ users see its struct; the size and the initial bytes are the C++
/// definition's, the alignment the largest its size allows (at most 16).
#[repr(C, align(8))]
pub struct Opaque8<const N: usize>(pub [u8; N]);
/// A C++ object kept as its bytes: its C++ users see its struct; the size and the initial bytes are the C++
/// definition's, the alignment the largest its size allows (at most 16).
#[repr(C, align(16))]
pub struct Opaque16<const N: usize>(pub [u8; N]);

// ---- src/Hooks/BodyHooksState.cpp: the body hooks' switches and read-outs (roomscale, heading, yaw follow, crouch, ladder, swimming)
#[no_mangle]
pub static mut CyberpunkVR_HeadLeadsCapsule: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_BodyStrictUnderHead: i32 = 1;
/// 1: with the head-led body turn and strict roomscale, the visible body keeps the dead zones (cvr_body::dead_zones)
/// while the capsule stays under the head. 0, the default since 2026-11-07: the visible body is under the head every
/// frame with the capsule, as BONELAB's physics body is (eased or not, the zones read as the body trailing the head).
#[no_mangle]
pub static mut CyberpunkVR_BodyDeadZones: i32 = 0;
/// 1: the body goes to a new place eased (BodyDeadZones::SHOW_OMEGA); 0: in one frame.
#[no_mangle]
pub static mut CyberpunkVR_BodyDeadZoneEase: i32 = 1;
/// 1: in strict roomscale the visible body stands under the RENDERED head's neck every frame (pose_hook.rs).
#[no_mangle]
pub static mut CyberpunkVR_BodyUnderRenderedHead: i32 = 1;
/// The body less the neck (x, y tracking metres), looking down, breached, inner breached, distance (game metres), the
/// shown body less the neck (x, y).
#[no_mangle]
pub static mut CyberpunkVR_DebugBodyZones: [f32; 8] = [0.0; 8];
#[no_mangle]
pub static mut CyberpunkVR_RoomscaleDebug: Opaque16<208> = Opaque16([0; 208]);
#[no_mangle]
pub static CyberpunkVR_RoomscaleDebugSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RoomscaleHooksReady: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SwimmingFeedbackReads: [u64; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_SwimmingPropertyFeedbackReads: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_SwimmingRedirect: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut CyberpunkVR_BodyHeadingHooksReady: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_BodyHeadingCommits: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_BodyHeadingRequests: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_BodyTrackingYawRad: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBodyHeadingSkip: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_BodyYawFollow: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrikNativePairPublished: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrikNativePairRejected: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrikNativePairPhaseMiss: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_BodyYawFollowDeadDeg: f32 = 5.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHybridTrackedWeight: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHybridHandsDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHybridNeckPullDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHybridAlignDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugHybridEstimate: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTrackedTrace: [f32; 8] = [0.0; 8];
#[no_mangle]
pub static mut CyberpunkVR_DebugTrackedGates: u32 = 0;
/// The head-led turn: looking down (0/1), the head's yaw, the body's (degrees, the model's [0, 360)), re-seeds.
#[no_mangle]
pub static mut CyberpunkVR_DebugHeadLedTurn: [f32; 4] = [0.0; 4];
/// The body's turning speed (deg/s), upright and looking down alike; cvr_body::head_led::DEFAULT_BODY_TURN_CONFIG.
#[no_mangle]
pub static mut CyberpunkVR_BodyTurnSpeedDps: i32 = 300;
/// The upright cone (deg): a look within it of where the body last re-aimed leaves the body.
#[no_mangle]
pub static mut CyberpunkVR_BodyUprightConeDeg: i32 = 10;
#[no_mangle]
pub static mut CyberpunkVR_BodyYawRealignRad: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBodyFollowOffsetDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBodyFollowErrDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBodyFollowCalls: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugBodyFollowApplied: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_BodyYawFinalRad: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_BodyYawFinalValid: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PlayerEntityPos: [f32; 3] = [0.0, 0.0, 0.0];
#[no_mangle]
pub static mut CyberpunkVR_PlayerEntityQuat: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut CyberpunkVR_PlayerEntityValid: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugYawWriteMs: f64 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_EngineBodyYawZ: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_EngineBodyYawW: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_EngineBodyYawValid: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugYawWritesAll: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugYawWritesPlayer: u64 = 0;
#[no_mangle]
pub static mut CvrSwimmingActionOriginal: *mut core::ffi::c_void = core::ptr::null_mut();

// ---- src/Camera/CameraLinkState.cpp: the camera link's switches and read-outs (pose blend, external camera, transport)
#[no_mangle]
pub static CyberpunkVR_BodyCentreMatch: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_BodyCentreMiss: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_BodyLuaCentreMatch: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_BodyLuaCentreRejected: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_PlacedPoseMatch: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_PlacedPoseMiss: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_PlacedPoseAmbiguous: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalExact: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalApprox: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFinalExactTies: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamFinalMatch: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamFinalNoMatch: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugVrcamFinalAge: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_CoherentHandAnchor: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_CamFinalRowOrder: i32 = 1;
#[no_mangle]
pub static CyberpunkVR_PoseIdComponent: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_PoseIdCopied: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_PoseIdCopyChanged: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_PoseIdFinal: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_PoseIdFinalMiss: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_PoseIdVrikSource: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_PoseIdVrikInputChanged: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldBlendPoseCalls: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldBlendPoseApplied: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldBlendPoseMissing: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_WorldBlendPoseChanged: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PoseBlendDebug: Opaque8<456> = Opaque8([0; 456]);
#[no_mangle]
pub static CyberpunkVR_PoseBlendDebugSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PoseMainReadMissing: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static mut CyberpunkVR_ExternalCameraDebug: Opaque16<208> = Opaque16([0; 208]);
#[no_mangle]
pub static CyberpunkVR_ExternalCameraDebugSeq: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_ExternalSelectionStates: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
#[no_mangle]
pub static CyberpunkVR_ExternalNotify: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
#[no_mangle]
pub static mut CyberpunkVR_DebugVehicleWorldRecompose: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSceneWorldRecompose: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTakeoverWorldRecompose: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFppSceneComfortLagDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_AdsWeaponZoomSync: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugAdsZoomWrites: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugAdsZoomRejects: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSceneComfortLagDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSceneComfortCuts: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugSceneLabelPublished: u64 = 0;
#[no_mangle]
pub static mut g_bdPushQuat: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

// ---- src/Runtimes/HapticsState.cpp: the haptics' profiles and statistics
#[no_mangle]
pub static mut CyberpunkVR_Haptics: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_HapticScale: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_HapticSupportMul: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_HapticProfiles: [[f32; 6]; 17] = [
    [0.85, 28.0, 0.25, 30.0, 0.00, 0.0],
    [0.60, 22.0, 0.00, 0.0, 0.00, 0.0],
    [1.00, 55.0, 0.55, 70.0, 0.20, 60.0],
    [1.00, 45.0, 0.60, 60.0, 0.25, 90.0],
    [0.30, 14.0, 0.00, 0.0, 0.00, 0.0],
    [0.90, 40.0, 0.40, 50.0, 0.00, 0.0],
    [1.00, 20.0, 0.00, 35.0, 0.75, 25.0],
    [0.65, 30.0, 0.20, 30.0, 0.00, 0.0],
    [1.00, 15.0, 0.45, 25.0, 0.00, 0.0],
    [1.00, 18.0, 0.35, 30.0, 0.00, 0.0],
    [0.45, 25.0, 0.00, 0.0, 0.00, 0.0],
    [0.80, 16.0, 0.00, 12.0, 0.35, 12.0],
    [0.15, 12.0, 0.00, 0.0, 0.00, 0.0],
    [0.22, 12.0, 0.00, 0.0, 0.00, 0.0],
    [1.00, 35.0, 0.50, 55.0, 0.00, 0.0],
    [0.50, 45.0, 0.00, 0.0, 0.00, 0.0],
    // a lodged blade sliding: a short contact
    [0.24, 24.0, 0.00, 0.0, 0.00, 0.0],
];
#[no_mangle]
pub static mut CyberpunkVR_HapticStats: [i32; 8] = [0; 8];

// ---- src/Hooks/PanzerState.cpp: the tank's steering, weapon aim and cannon read-outs
#[no_mangle]
pub static mut CyberpunkVR_PanzerGazeSteering: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_PanzerSteeringDebug: Opaque16<80> = Opaque16([0; 80]);
#[no_mangle]
pub static CyberpunkVR_PanzerSteeringDebugSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PanzerWeaponAimDebug: Opaque16<128> = Opaque16([0; 128]);
#[no_mangle]
pub static CyberpunkVR_PanzerWeaponAimDebugSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PanzerCannonDebug: Opaque16<96> = Opaque16([0; 96]);
#[no_mangle]
pub static CyberpunkVR_PanzerCannonDebugSeq: AtomicU32 = AtomicU32::new(0);

// ---- src/Hooks/WeaponAimState.cpp: the hitscan from the muzzle's, the gaze-centred targeting's and the install's switches
// and diagnostics
#[no_mangle]
pub static mut CyberpunkVR_DebugWaInstalled: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_GazeTargetState: usize = 0;
#[no_mangle]
pub static mut CyberpunkVR_GazeTargetUpdates: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_HitscanFromMuzzle: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayPatched: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayCalls: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayMine: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayOriginWrites: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayForwardWrites: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayConeRays: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayRecoil: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayForeign: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayLeftGun: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugDualShot: [f32; 4] = [0.0; 4];
#[no_mangle]
pub static mut CyberpunkVR_DebugDualShotRecord: [u32; 3] = [0; 3];
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayStaleMuzzle: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRaySightLine: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRaySightLift: [f32; 4] = [0.0; 4];
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayRawForward: [f32; 4] = [0.0; 4];
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayOutForward: [f32; 4] = [0.0; 4];
#[no_mangle]
pub static mut CyberpunkVR_ConeCentreFromCamera: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugConeFirstRay: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayShared: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_HitscanSharedRay: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRaySharedMissing: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugPhysicalRayKeys: [u64; 64] = [0; 64];
