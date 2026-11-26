//! THE SOLVE'S SHARED GLOBALS (formerly src/Anim/CharacterRigState.cpp, which the reference build keeps): the FK scratch,
//! the hand stop and wrist lock, the solve cache, the body probes, the finger layers' and gestures' switches, the
//! recoil, grasp and two-hand grip controls and read-outs, and the wheel grab's blends. Every one keeps its C name,
//! type and initial value (compared byte for byte with the C++ object's data when this file was made); the C++ half
//! declares them `extern "C"` (Anim/CharacterRig.hpp, Anim/VrikState.hpp, Anim/Cyberware.hpp, ...) and the solve
//! reads them through its extern declarations (env.rs and the modules' own). The `CyberpunkVR_*` ones are exported
//! from the DLL by name (rust/exported-data.txt), as their `__declspec(dllexport)` definitions were.
//!
//! The `std::atomic<float>` wheel blends are AtomicU32 holding the f32's bits; the clock and tracking hooks are
//! nullable C function pointers (null = the real clock).
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicI32, AtomicU32};

#[no_mangle]
pub static mut g_VRUpperOwned: [bool; 2048] = [false; 2048];
#[no_mangle]
pub static mut g_VRShadowSource: [i32; 2048] = [0; 2048];
#[no_mangle]
pub static mut g_VRHeadReferenceValid: bool = false;
#[no_mangle]
pub static mut g_VRIKVehicleUpperActive: bool = false;
#[no_mangle]
pub static mut g_VRHeadReferenceModelRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_solveCacheModel: bool = false;
#[no_mangle]
pub static mut g_VRIKBodyBendAngle: f32 = 0.0;
#[no_mangle]
pub static mut g_VRIKBodyBendPelvis: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKBodyEyeTarget: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKBodyEyeSolved: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKBodyEyeBound: bool = false;
#[no_mangle]
pub static mut g_VRIKGirdleReach: [[f32; 4]; 2] = [[0.0; 4]; 2];
#[no_mangle]
pub static mut CyberpunkVR_GripReach: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_SquatKneeMode: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_SquatFeetUnderHips: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_SquatSymmetric: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugSquatStance: [f32; 2] = [0.0; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugToes: [[f32; 3]; 2] = [[0.0; 3]; 2];
#[no_mangle]
pub static mut CyberpunkVR_GripProtract: f32 = 0.085;
#[no_mangle]
pub static mut CyberpunkVR_GripTorsoMaxDeg: f32 = 20.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugGripShortMm: [f32; 2] = [0.0, 0.0];
#[no_mangle]
pub static mut CyberpunkVR_DebugGripTorsoDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugGripReach: [f32; 6] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
#[no_mangle]
pub static mut CyberpunkVR_ClipBodyMoves: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugClipBody: [f32; 12] = [0.0; 12];
#[no_mangle]
pub static mut CyberpunkVR_NativeCyberwarePasses: i32 = 1;
#[no_mangle]
pub static mut g_VRIKHandPin: [[f32; 3]; 2] = [[0.0; 3]; 2];
#[no_mangle]
pub static mut g_VRIKHandAim: [[f32; 8]; 2] = [[0.0; 8]; 2];
#[no_mangle]
pub static mut g_VRIKTorsoOn: bool = false;
#[no_mangle]
pub static mut g_VRCyberwareFamily: i32 = 0;
/// V's body: 1 a woman's, 0 a man's, -1 not known yet (the rig policy; main's NativeState.cpp `volatile int`): the
/// physics hands' skin (cvr-physhands hand_skin) is a woman's or a man's.
#[no_mangle]
pub static g_VRBodyFemale: AtomicI32 = AtomicI32::new(-1);
/// Bumped (release) each time the whole-rig FK snapshot (g_VRFKSnapPos/Rot) is written, for its readers on other threads.
#[no_mangle]
pub static g_VRFKSnapSeq: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
#[no_mangle]
pub static mut g_fkPos: [[f32; 3]; 2048] = [[0.0; 3]; 2048];
#[no_mangle]
pub static mut g_fkRot: [[f32; 4]; 2048] = [[0.0; 4]; 2048];
#[no_mangle]
pub static mut g_VRFKSnapPos: [[f32; 3]; 2048] = [[0.0; 3]; 2048];
#[no_mangle]
pub static mut g_VRFKSnapRot: [[f32; 4]; 2048] = [[0.0; 4]; 2048];
#[no_mangle]
pub static mut g_VRFKSnapCount: i32 = 0;
#[no_mangle]
pub static mut g_VRHandStopModel: [[f32; 3]; 2] = [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
#[no_mangle]
pub static mut g_VRHandStopValid: [i32; 2] = [0, 0];
#[no_mangle]
pub static mut g_VRHandStopDeadband: f32 = 0.02;
#[no_mangle]
pub static mut g_VRHandRotLock: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]];
#[no_mangle]
pub static mut g_VRHandRotLockOn: [i32; 2] = [0, 0];
#[no_mangle]
pub static mut g_VRHandRawModel: [[f32; 3]; 2] = [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];
#[no_mangle]
pub static mut g_VRHandRawValid: [i32; 2] = [0, 0];
#[no_mangle]
pub static mut g_VRHandRawRot: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]];
#[no_mangle]
pub static mut g_VRBendLimitClockUs: Option<unsafe extern "C" fn() -> i64> = None;
#[no_mangle]
pub static mut g_solveCacheTick: u32 = 0xFFFFFFFF;
#[no_mangle]
pub static g_VrikFrameEpoch: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut g_VrikOverwritePassHist: [i32; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
#[no_mangle]
pub static mut g_VrikBatchGapNew: i32 = 0;
#[no_mangle]
pub static mut g_VrikBatchGapSame: i32 = 0;
#[no_mangle]
pub static mut g_VrikBatchGapNewMin: f32 = 0.0;
#[no_mangle]
pub static mut g_VrikBatchGapSameMax: f32 = 0.0;
#[no_mangle]
pub static mut g_VrikEngineOverwriteMm: f32 = 0.0;
#[no_mangle]
pub static mut g_VrikEngineOverwriteBone: i32 = -1;
#[no_mangle]
pub static mut g_VrikEngineOverwriteHits: i32 = 0;
#[no_mangle]
pub static mut g_VrikEngineOverwritePasses: i32 = 0;
#[no_mangle]
pub static mut g_VrikShakePeakMm: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]];
#[no_mangle]
pub static mut g_VrikShakeSlowMm: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]];
#[no_mangle]
pub static mut g_VrikShakeSlowN: [[i32; 4]; 2] = [[0, 0, 0, 0], [0, 0, 0, 0]];
#[no_mangle]
pub static mut g_solveCacheN: i32 = 0;
#[no_mangle]
pub static mut g_solveCacheIdx: [i32; 2048] = [0; 2048];
#[no_mangle]
pub static mut g_solveCacheVal: [[f32; 7]; 2048] = [[0.0; 7]; 2048];
#[no_mangle]
pub static mut g_solveCacheYaw: f32 = 0.0;
#[no_mangle]
pub static mut g_solveCacheSnapCtr: f32 = -1.0;
#[no_mangle]
pub static mut g_handsStableSeq: u32 = 0;
#[no_mangle]
pub static mut g_handsStableValid: bool = false;
#[no_mangle]
pub static mut g_viewPkt: [f32; 17] = [
    0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];
#[no_mangle]
pub static mut g_viewPktValid: bool = false;
#[no_mangle]
pub static mut g_vrikViewScaleUsed: f32 = 0.0;
#[no_mangle]
pub static mut s_vrSharedSquatDrop: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerHave: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerCaps: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerRefused: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerSaved: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerLoadTried: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerLoaded: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RestFingerCaptureReq: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RestFingerSaveReq: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestApplyCalls: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestBones: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestGate: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RestFingerForce: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RestFingerCaptureReqR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RestFingerSaveReqR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RestFingerApplyR: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_RestFingerForceR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerHaveR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerSavedR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerRefusedR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerCapsR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestBonesR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestGateR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestFingerLoadTriedR: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_CarryGripApply: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugCarryGripHave: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCarryGripSnaps: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCarryGripBones: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_GestureMode: [i32; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_GestureClosed: [[f32; 5]; 2] = [[0.0; 5]; 2];
#[no_mangle]
pub static mut CyberpunkVR_GestureStraight: [[f32; 5]; 2] = [[0.0; 5]; 2];
#[no_mangle]
pub static mut CyberpunkVR_FingerBones: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_FingerSpread: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_FingerSpreadOn: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugLeftHandedTwins: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugLeftHandedResidual: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugFingerFlex: [f32; 4] = [0.0; 4];
#[no_mangle]
pub static mut CyberpunkVR_NonVrikAdsStabilizer: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugAdsStabApplies: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugAdsStabHipCaptures: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_VrikReloadHandOff: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_ReloadHandOffInSec: f32 = 0.20;
#[no_mangle]
pub static mut CyberpunkVR_ReloadHandOffOutSec: f32 = 0.30;
#[no_mangle]
pub static mut CyberpunkVR_ReloadHandOffLeadSec: f32 = 0.15;
#[no_mangle]
pub static mut CyberpunkVR_ReloadHandOffStats: [f32; 8] = [0.0; 8];
#[no_mangle]
pub static mut g_VRIKClockQpc: Option<unsafe extern "C" fn() -> i64> = None;
#[no_mangle]
pub static mut g_VRIKClockMs: Option<unsafe extern "C" fn() -> u64> = None;
#[no_mangle]
pub static mut g_VRIKClockUs: Option<unsafe extern "C" fn() -> u64> = None;
#[no_mangle]
pub static mut g_VRIKWheelFrame: Option<unsafe extern "C" fn(*mut core::ffi::c_void) -> bool> =
    None;
#[no_mangle]
pub static g_wheelBlendRight: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static g_wheelBlendLeft: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static g_wheelSteer: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static g_wheelSteerDeg: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static g_wheelHornMask: AtomicI32 = AtomicI32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_HandRecoil: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_HandRecoilBackCm: f32 = 3.0;
#[no_mangle]
pub static mut CyberpunkVR_HandRecoilBackMaxCm: f32 = 4.0;
#[no_mangle]
pub static mut CyberpunkVR_HandRecoilRiseDeg: f32 = 12.3;
#[no_mangle]
pub static mut CyberpunkVR_HandRecoilReturnMs: f32 = 180.0;
#[no_mangle]
pub static mut CyberpunkVR_WeaponKickDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_WeaponKickScale: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_WeaponClass: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_WeaponKickDegLeft: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_WeaponClassLeft: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RecoilDownPistol: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_RecoilDownRifle: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_RecoilDownShotgun: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_PistolBackMul: f32 = 0.30;
#[no_mangle]
pub static mut CyberpunkVR_RifleRiseMul: f32 = 0.35;
#[no_mangle]
pub static mut CyberpunkVR_RifleBackMul: f32 = 2.00;
#[no_mangle]
pub static mut CyberpunkVR_ShotgunRiseMul: f32 = 1.00;
#[no_mangle]
pub static mut CyberpunkVR_ShotgunBackMul: f32 = 2.00;
#[no_mangle]
pub static mut CyberpunkVR_SniperRiseMul: f32 = 2.00;
#[no_mangle]
pub static mut CyberpunkVR_SniperBackMul: f32 = 5.00;
#[no_mangle]
pub static mut CyberpunkVR_RecoilScale: f32 = 0.70;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandBackMul: f32 = 0.60;
#[no_mangle]
pub static mut CyberpunkVR_HandRecoilRefKick: f32 = 1.0;
#[no_mangle]
pub static mut CyberpunkVR_HandRecoilMaxDeg: f32 = 28.0;
#[no_mangle]
pub static mut CyberpunkVR_HandRecoilReturnPow: f32 = 0.15;
#[no_mangle]
pub static mut CyberpunkVR_HandRecoilReturnMinMs: f32 = 80.0;
#[no_mangle]
pub static mut CyberpunkVR_RecoilClimbFrac: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_RecoilClimbMaxDeg: f32 = 8.0;
#[no_mangle]
pub static mut CyberpunkVR_RecoilClimbMs: f32 = 250.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRecoilClimbDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_HandRecoilReturnMaxMs: f32 = 420.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRecoilShots: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRecoilCm: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRecoilTicks: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRecoilAppliedDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRecoilPeakDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugRecoilPeakDegLeft: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugGrasp: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_DebugGraspAngles: [[f32; 17]; 2] = [[0.0; 17]; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugGraspPalm: [[f32; 12]; 2] = [[0.0; 12]; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugGraspNormalFlip: [i32; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugGraspThumb: [[f32; 4]; 2] = [[0.0; 4]; 2];
#[no_mangle]
pub static mut CyberpunkVR_TwoHandGrip: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandSuppress: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTwoHandHoldIdx: i32 = -1;
#[no_mangle]
pub static mut CyberpunkVR_CarryLeft: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_CarryBlendMs: f32 = 320.0;
#[no_mangle]
pub static mut CyberpunkVR_CarryBlendZeta: f32 = 0.65;
#[no_mangle]
pub static mut CyberpunkVR_DebugCarryBlend: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandRadius: f32 = 0.06;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandRecoil: f32 = 0.286;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandFadeS: f32 = 0.15;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandEngageMs: f32 = 220.0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandEngageZeta: f32 = 0.75;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandWeight: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandLever: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandReleaseMs: f32 = 110.0;
#[no_mangle]
pub static mut CyberpunkVR_CarryReturnHand: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_CarryReturnMs: f32 = 260.0;
#[no_mangle]
pub static mut CyberpunkVR_CarryReturnZeta: f32 = 0.70;
#[no_mangle]
pub static mut CyberpunkVR_DebugCarryReturn: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_CarryGripRadius: f32 = 0.09;
#[no_mangle]
pub static mut CyberpunkVR_CarryGripMs: f32 = 200.0;
#[no_mangle]
pub static mut CyberpunkVR_CarryGripZeta: f32 = 0.80;
#[no_mangle]
pub static mut CyberpunkVR_CarryGripBlend: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugCarryReach: f32 = -1.0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandAimGain: f32 = 1.00;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandLeverFull: f32 = 0.35;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandAimTau: f32 = 0.12;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandAimMaxDeg: f32 = 60.0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandAimDeadDeg: f32 = 4.0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandAim: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandAimReleaseMs: f32 = 400.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTwoHandAimDeg: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandBreakRadius: f32 = 0.60;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandBreakMs: f32 = 250.0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandActive: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandBlend: f32 = 0.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTwoHandDist: f32 = -1.0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTwoHandHave: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_TwoHandCaptureReq: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTwoHandRefused: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTwoHandSaved: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTwoHandLoaded: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_DebugTwoHandBaseMm: f32 = -1.0;
#[no_mangle]
pub static mut g_VRTwoHandSlide: [f32; 3] = [0.0, 0.0, 0.0];
#[no_mangle]
pub static mut g_VRTwoHandSlideUntil: u64 = 0;
#[no_mangle]
pub static mut g_VRTwoHandCtrlL: [f32; 3] = [0.0, 0.0, 0.0];
#[no_mangle]
pub static mut g_VRTwoHandCtrlLValid: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_BodyOrbitRestNeck: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_DebugRestNeck: [f32; 2] = [f32::NAN, f32::NAN];
#[no_mangle]
pub static mut CyberpunkVR_VrikTakedownSuspend: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_VrikBodyCarryHandOff: i32 = 1;
#[no_mangle]
pub static mut CyberpunkVR_VrikFinisherSuspend: i32 = 1;
