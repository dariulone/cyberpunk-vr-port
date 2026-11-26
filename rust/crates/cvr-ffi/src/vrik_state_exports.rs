//! The VRIK and weapon-aim ABI's globals (formerly src/Natives/NativeState.cpp's definitions; Anim/VrikState.hpp
//! and Anim/WeaponAimState.hpp declare them with C linkage, the C++ pose path and the natives use them, and so
//! does cvr-rig through its own `extern "C"` declarations). Plain data, initialised as the C++ initialisers were.
#![allow(non_upper_case_globals)]

#[no_mangle]
pub static mut g_waExeBase: usize = 0;
#[no_mangle]
pub static mut g_hMapFile: usize = 0;
#[no_mangle]
pub static mut g_pSharedHands: usize = 0;
#[no_mangle]
pub static mut g_chunkDebugEnabled: bool = false;
#[no_mangle]
pub static mut g_chunkDebugWasEnabled: bool = false;
#[no_mangle]
pub static mut g_chunkDebugComponentIndex: i32 = -1;
#[no_mangle]
pub static mut g_chunkDebugHand: i32 = 1;
#[no_mangle]
pub static mut g_chunkDebugBitSlots: [i32; 4] = [0, -1, -1, -1];
#[no_mangle]
pub static mut g_animInputTestMode: i32 = 0;
#[no_mangle]
pub static mut g_animParamPersistentPreset: i32 = 0;
#[no_mangle]
pub static mut g_animParamPersistentValue: f32 = 0.0;
#[no_mangle]
pub static mut g_animParamPersistentLastResult: i32 = 0;
#[no_mangle]
pub static mut g_rootGraphFloatPersistentPreset: i32 = 0;
#[no_mangle]
pub static mut g_rootGraphFloatPersistentValue: f32 = 0.0;
#[no_mangle]
pub static mut g_rootGraphFloatPersistentLastResult: i32 = 0;
#[no_mangle]
pub static mut g_rootGraphVectorPersistentPreset: i32 = 0;
#[no_mangle]
pub static mut g_rootGraphVectorPersistentValue: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
#[no_mangle]
pub static mut g_rootGraphVectorPersistentLastResult: i32 = 0;
#[no_mangle]
pub static mut g_rootMetaRigTrackPersistentPreset: i32 = 0;
#[no_mangle]
pub static mut g_rootMetaRigTrackPersistentValue: f32 = 0.0;
#[no_mangle]
pub static mut g_rootMetaRigTrackPersistentLastResult: i32 = 0;
#[no_mangle]
pub static mut g_rootLiveTrackPersistentPreset: i32 = 0;
#[no_mangle]
pub static mut g_rootLiveTrackPersistentValue: f32 = 0.0;
#[no_mangle]
pub static mut g_rootLiveTrackPersistentArrayMode: i32 = 0;
#[no_mangle]
pub static mut g_rootLiveTrackPersistentLastResult: i32 = 0;
#[no_mangle]
pub static mut g_CameraWorldPos: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_CalibrationBoneIndex: i32 = -1;
#[no_mangle]
pub static mut g_WeaponTrackBufA: usize = 0;
#[no_mangle]
pub static mut g_WeaponTrackBufB: usize = 0;
#[no_mangle]
pub static mut g_WeaponRigActive: i32 = 0;
#[no_mangle]
pub static mut g_WeaponPartCount: i32 = 0;
#[no_mangle]
pub static mut g_WeaponPartIdx: [i32; 24] = [0; 24];
#[no_mangle]
pub static mut g_WeaponPartPos: [[f32; 3]; 24] = [[0.0; 3]; 24];
#[no_mangle]
pub static mut g_WeaponPartRot: [[f32; 4]; 24] = [[0.0; 4]; 24];
#[no_mangle]
pub static mut g_WeaponPartHave: i32 = 0;
#[no_mangle]
pub static mut g_WeaponMatchCalls: u64 = 0;
#[no_mangle]
pub static mut g_PoseCensusOn: i32 = 0;
#[no_mangle]
pub static mut g_PoseCensusN: i32 = 0;
#[no_mangle]
pub static mut g_PoseCensusBuf: [usize; 96] = [0; 96];
#[no_mangle]
pub static mut g_PoseCensusA4: [u32; 96] = [0; 96];
#[no_mangle]
pub static mut g_PoseCensusHits: [u32; 96] = [0; 96];
#[no_mangle]
pub static mut g_PoseCensusArmed: [u8; 96] = [0; 96];
#[no_mangle]
pub static mut g_PoseCensusHitsOut: [u32; 96] = [0; 96];
#[no_mangle]
pub static mut g_PoseCensusHitsIn: [u32; 96] = [0; 96];
#[no_mangle]
pub static mut g_PoseCensusWeaponOut: i32 = 0;
#[no_mangle]
pub static mut g_PoseCensusFull: i32 = 0;
#[no_mangle]
pub static mut g_SmallN: i32 = 0;
#[no_mangle]
pub static mut g_SmallBuf: [usize; 16] = [0; 16];
#[no_mangle]
pub static mut g_SmallA4: [u32; 16] = [0; 16];
#[no_mangle]
pub static mut g_SmallBones: [u32; 16] = [0; 16];
#[no_mangle]
pub static mut g_SmallOut: [u32; 16] = [0; 16];
#[no_mangle]
pub static mut g_SmallIn: [u32; 16] = [0; 16];
#[no_mangle]
pub static mut g_SmallFull: i32 = 0;
#[no_mangle]
pub static mut g_PairN: i32 = 0;
#[no_mangle]
pub static mut g_PairBones: [u32; 24] = [0; 24];
#[no_mangle]
pub static mut g_PairTracks: [u32; 24] = [0; 24];
#[no_mangle]
pub static mut g_PairOut: [u32; 24] = [0; 24];
#[no_mangle]
pub static mut g_PairIn: [u32; 24] = [0; 24];
#[no_mangle]
pub static mut g_PairBuf: [usize; 24] = [0; 24];
#[no_mangle]
pub static mut g_PairFull: i32 = 0;
#[no_mangle]
pub static mut g_RigBuf: [usize; 3] = [0; 3];
#[no_mangle]
pub static mut g_RigSeen: [u32; 3] = [0; 3];
#[no_mangle]
pub static mut g_RigBones: [u32; 3] = [0; 3];
#[no_mangle]
pub static mut g_RigTracks: [u32; 3] = [0; 3];
#[no_mangle]
pub static mut g_RigWriteN: i32 = 0;
#[no_mangle]
pub static mut g_RigWriteWhich: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigWriteBone: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigWriteOff: [[f32; 3]; 32] = [[0.0; 3]; 32];
#[no_mangle]
pub static mut g_RigWriteBase: [[f32; 3]; 32] = [[0.0; 3]; 32];
#[no_mangle]
pub static mut g_RigWriteHaveBase: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigWriteApplied: [u32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigWriteSlot: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigWriteRotAxis: [[f32; 3]; 32] = [[0.0; 3]; 32];
#[no_mangle]
pub static mut g_RigWriteRotAngle: [f32; 32] = [0.0; 32];
#[no_mangle]
pub static mut g_RigWriteBaseRot: [[f32; 4]; 32] = [[0.0; 4]; 32];
#[no_mangle]
pub static mut g_RigWriteHaveBaseRot: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigWritePin: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigPose: [[[f32; 7]; 20]; 3] = [[[0.0; 7]; 20]; 3];
#[no_mangle]
pub static mut g_RigPoseHave: [i32; 3] = [0; 3];
#[no_mangle]
pub static mut g_RigSigN: i32 = 0;
#[no_mangle]
pub static mut g_RigSigWhich: [i32; 128] = [0; 128];
#[no_mangle]
pub static mut g_RigSigBones: [u32; 128] = [0; 128];
#[no_mangle]
pub static mut g_RigSigIdx: [[i32; 4]; 128] = [[0; 4]; 128];
#[no_mangle]
pub static mut g_RigSigHash: [[u64; 4]; 128] = [[0; 4]; 128];
#[no_mangle]
pub static mut g_RigTrackVal: [[f32; 16]; 3] = [[0.0; 16]; 3];
#[no_mangle]
pub static mut g_RigTrackHave: [i32; 3] = [0; 3];
#[no_mangle]
pub static mut g_RigTrackSet: [[f32; 16]; 3] = [[0.0; 16]; 3];
#[no_mangle]
pub static mut g_RigTrackOn: [[i32; 16]; 3] = [[0; 16]; 3];
#[no_mangle]
pub static mut g_RigWriteScale: [f32; 32] = [0.0; 32];
#[no_mangle]
pub static mut g_RigWriteAbs: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigWriteQuat: [[f32; 4]; 32] = [[0.0; 4]; 32];
#[no_mangle]
pub static mut g_RigWriteQuatOn: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigWriteEnabled: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_RigPassSeen: [u32; 4] = [0; 4];
#[no_mangle]
pub static mut g_RigPassRn: [i32; 4] = [0; 4];
#[no_mangle]
pub static mut g_RigPassDst: [i32; 4] = [0; 4];
#[no_mangle]
pub static mut g_RigPassSlots: [i32; 4] = [0; 4];
#[no_mangle]
pub static mut g_RigPassBufLo: [i32; 4] = [0; 4];
#[no_mangle]
pub static mut g_RigPassMap0: [i32; 8] = [0; 8];
#[no_mangle]
pub static mut g_VRRestFingerRot: [[f32; 4]; 32] = [[0.0; 4]; 32];
#[no_mangle]
pub static mut g_VRRestFingerHave: i32 = 0;
#[no_mangle]
pub static mut g_VRRestFingerCount: i32 = 0;
#[no_mangle]
pub static mut g_VRRestFingerApply: i32 = 1;
#[no_mangle]
pub static mut g_VRReloadFingerActive: [i32; 2] = [0; 2];
#[no_mangle]
pub static mut g_VRReloadFingerRot: [[[f32; 4]; 32]; 2] = [[[0.0; 4]; 32]; 2];
#[no_mangle]
pub static mut g_VRReloadFingerSet: [[i32; 32]; 2] = [[0; 32]; 2];
#[no_mangle]
pub static mut g_VRReloadFingerBlend: [f32; 2] = [1.0, 1.0];
#[no_mangle]
pub static mut g_VRRecordFK: i32 = 0;
#[no_mangle]
pub static mut g_WeaponPartOff: [[f32; 3]; 24] = [[0.0; 3]; 24];
#[no_mangle]
pub static mut g_WeaponPartWriteOn: i32 = 0;
#[no_mangle]
pub static mut g_WeaponRigDiag: [i32; 8] = [0; 8];
#[no_mangle]
pub static mut g_WeaponRigNames: [u8; 512] = [0; 512];
#[no_mangle]
pub static mut g_PlayerTrackBufA: usize = 0;
#[no_mangle]
pub static mut g_PlayerTrackBufB: usize = 0;
#[no_mangle]
pub static mut g_AnimPoseTotalCalls: u64 = 0;
#[no_mangle]
pub static mut g_AnimPoseMatchCalls: u64 = 0;
#[no_mangle]
pub static mut g_AnimPoseLastBoneBuf: usize = 0;
#[no_mangle]
pub static mut g_VRBind: i32 = 4;
#[no_mangle]
pub static mut g_VRBindScale: f32 = 1.0;
#[no_mangle]
pub static mut g_VRBindOffX: f32 = 0.0;
#[no_mangle]
pub static mut g_VRBindOffY: f32 = 0.0;
#[no_mangle]
pub static mut g_VRBindOffZ: f32 = 0.23;
#[no_mangle]
pub static mut g_VRBindAxis: i32 = 1;
#[no_mangle]
pub static mut g_VRWristR_I: f32 = 0.12885998;
#[no_mangle]
pub static mut g_VRWristR_J: f32 = -0.69526621;
#[no_mangle]
pub static mut g_VRWristR_K: f32 = -0.12885998;
#[no_mangle]
pub static mut g_VRWristR_R: f32 = 0.69526621;
#[no_mangle]
pub static mut g_VRWristL_I: f32 = -0.70105738;
#[no_mangle]
pub static mut g_VRWristL_J: f32 = -0.09229596;
#[no_mangle]
pub static mut g_VRWristL_K: f32 = 0.70105738;
#[no_mangle]
pub static mut g_VRWristL_R: f32 = 0.09229596;
#[no_mangle]
pub static mut g_VRScaleR: f32 = 1.0;
#[no_mangle]
pub static mut g_VRScaleL: f32 = 1.0;
#[no_mangle]
pub static mut g_VROffRX: f32 = 0.0;
#[no_mangle]
pub static mut g_VROffRY: f32 = 0.0;
#[no_mangle]
pub static mut g_VROffRZ: f32 = 0.0;
#[no_mangle]
pub static mut g_VROffLX: f32 = 0.0;
#[no_mangle]
pub static mut g_VROffLY: f32 = 0.0;
#[no_mangle]
pub static mut g_VROffLZ: f32 = 0.0;
#[no_mangle]
pub static mut g_VRUserArmLenR: f32 = 0.0;
#[no_mangle]
pub static mut g_VRUserArmLenL: f32 = 0.0;
#[no_mangle]
pub static mut g_VRUserEyeHeight: f32 = 0.0;
#[no_mangle]
pub static mut g_VRBodyUnderHMD: i32 = 1;
#[no_mangle]
pub static mut g_VRNeutralizeAnimGraph: i32 = 1;
#[no_mangle]
pub static mut g_VRChestDrop: f32 = 0.40;
#[no_mangle]
pub static mut g_VRChestFwd: f32 = -0.05;
#[no_mangle]
pub static mut g_VRHeadDrop: f32 = 0.08;
#[no_mangle]
pub static mut g_VRSquatThreshold: f32 = 0.20;
#[no_mangle]
pub static mut g_VRCamSmooth: f32 = 0.12;
#[no_mangle]
pub static mut g_VRIKDbgChest: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKDbgClav: [[f32; 8]; 2] = [[0.0; 8]; 2];
#[no_mangle]
pub static mut g_VRIKDbgHipsYaw: f32 = 0.0;
#[no_mangle]
pub static mut g_VRIKDbgShModel: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKDbgHandFK: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKDbgTargetTrace: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRPoseCapGen: i32 = 0;
#[no_mangle]
pub static mut g_VRIKSolvesLastTick: i32 = 0;
#[no_mangle]
pub static mut g_VRIKSolvesMaxTick: i32 = 0;
#[no_mangle]
pub static mut g_VRIKLastBufA: usize = 0;
#[no_mangle]
pub static mut g_VRIKLastBufB: usize = 0;
#[no_mangle]
pub static mut g_VRIKReplayTotal: i32 = 0;
#[no_mangle]
pub static mut g_VRIKFreshTotal: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_VrikHandFrameAlign: i32 = 1;
#[no_mangle]
pub static mut g_VRIKDbgChestTgt: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRShoulderRX: f32 = 0.14;
#[no_mangle]
pub static mut g_VRShoulderRY: f32 = -0.17;
#[no_mangle]
pub static mut g_VRShoulderRZ: f32 = 0.05;
#[no_mangle]
pub static mut g_VRShoulderLX: f32 = -0.14;
#[no_mangle]
pub static mut g_VRShoulderLY: f32 = -0.17;
#[no_mangle]
pub static mut g_VRShoulderLZ: f32 = 0.05;
#[no_mangle]
pub static mut g_VRElbowPoleR: f32 = 0.0;
#[no_mangle]
pub static mut g_VRElbowPoleL: f32 = 0.0;
#[no_mangle]
pub static mut g_VRElbowSwingR: f32 = 1.0;
#[no_mangle]
pub static mut g_VRElbowSwingL: f32 = 1.0;
#[no_mangle]
pub static mut g_VRRightBoneIdx: i32 = 24;
#[no_mangle]
pub static mut g_VRLeftBoneIdx: i32 = 23;
#[no_mangle]
pub static mut g_VRHeadBoneIdx: i32 = -1;
#[no_mangle]
pub static mut g_VREyeLeftIdx: i32 = -1;
#[no_mangle]
pub static mut g_VREyeRightIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRFppCamIdx: [i32; 5] = [-1, -1, -1, -1, -1];
#[no_mangle]
pub static mut g_VRCamBoneFreeze: i32 = 4;
#[no_mangle]
pub static mut g_VRPairSlewRate: f32 = 1.0;
#[no_mangle]
pub static mut g_VRPairLeadTicks: f32 = 0.0;
#[no_mangle]
pub static mut g_VRUseHeadRelative: i32 = 1;
#[no_mangle]
pub static mut g_VRDiagCapture: i32 = 0;
#[no_mangle]
pub static mut g_VRDiagBones: [f32; 224] = [0.0; 224];
#[no_mangle]
pub static mut g_VRSmokeFingerActive: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerCapture: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerHave: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerCount: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerIdx: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_VRSmokeFingerRot: [[f32; 4]; 32] = [[0.0; 4]; 32];
#[no_mangle]
pub static mut g_VRSmokeFingerName: [[u8; 48]; 32] = [[0; 48]; 32];
#[no_mangle]
pub static mut g_VRSmokeCigIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRSprintActive: i32 = -1;
#[no_mangle]
pub static mut g_VRLocomotionState: i32 = -1;
#[no_mangle]
pub static mut g_VRLocomotionDetailed: i32 = -1;
#[no_mangle]
pub static mut g_VRTakedownState: i32 = -1;
#[no_mangle]
pub static mut g_VRBodyCarryingState: i32 = -1;
#[no_mangle]
pub static mut g_VRBodyDisposalState: i32 = -1;
#[no_mangle]
pub static mut g_VRBodyCarryBothHands: i32 = -1;
#[no_mangle]
pub static mut g_VRFinisherActive: i32 = -1;
#[no_mangle]
pub static mut g_VRFinisherSinceMs: u64 = 0;
#[no_mangle]
pub static mut g_VRWeaponPsmState: f32 = 0.0;
#[no_mangle]
pub static mut g_VRAimInRemaining: f32 = 0.0;
#[no_mangle]
pub static mut g_VRWeaponRaiseTransition: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeMouthBoneIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRSmokeCigHave: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeCigEnable: i32 = 1;
#[no_mangle]
pub static mut g_VRSmokeCigPos: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRSmokeCigRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeCigOffP: [f32; 3] = [-0.012, -0.008, -0.015];
#[no_mangle]
pub static mut g_VRSmokeCigOffQ: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeMouthDist: f32 = 999.0;
#[no_mangle]
pub static mut g_VRSmokeMouthDistL: f32 = 999.0;
#[no_mangle]
pub static mut g_VRSmokeMouthAnchor: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeMouthPos: [f32; 3] = [0.0, 0.07, -0.08];
#[no_mangle]
pub static mut g_VRSmokeMouthRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeAnchorValid: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeAnchorLocalPos: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRSmokeAnchorLocalRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeAnchorBoneSel: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeAnchorBoneIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRSmokeAltAnchorValid: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeAltAnchorLocalPos: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRSmokeAltAnchorLocalRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeCigScaleY: f32 = 1.0;
#[no_mangle]
pub static mut g_VRSmokeSmokePos: [f32; 3] = [0.0, 0.07, -0.08];
#[no_mangle]
pub static mut g_VRSmokeSmokeRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeMouthWorldPos: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRSmokeMouthWorldRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeMouthWorldValid: i32 = 0;
#[no_mangle]
pub static mut g_VRViewWorldPos: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRViewWorldRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRViewWorldValid: i32 = 0;
#[no_mangle]
pub static mut g_VRPalmRIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRPalmLIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRBodyBone: [[f32; 3]; 11] = [[0.0; 3]; 11];
#[no_mangle]
pub static mut g_VRBodyBoneOk: [i32; 11] = [0; 11];
#[no_mangle]
pub static mut g_VRPalmModelR: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRPalmModelL: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRPalmModelRotR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRPalmModelRotL: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRCamModelPos: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRCamModelRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRPalmModelValid: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerActiveL: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerCaptureL: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerHaveL: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerCountL: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerIdxL: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_VRSmokeFingerRotL: [[f32; 4]; 32] = [[0.0; 4]; 32];
#[no_mangle]
pub static mut g_VRSmokeFingerNameL: [[u8; 48]; 32] = [[0; 48]; 32];
#[no_mangle]
pub static mut g_VRSmokeLighterIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRSmokeLighterHave: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeLighterEnable: i32 = 1;
#[no_mangle]
pub static mut g_VRSmokeLighterPos: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRSmokeLighterRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeLighterOffP: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRSmokeLighterOffQ: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeThumbFlickL: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRSmokeThumbPressManualL: f32 = 0.0;
#[no_mangle]
pub static mut g_VRSmokeThumbIsL: [i32; 32] = [0; 32];
#[no_mangle]
pub static mut g_VRSmokeLeftUseCig: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeRightLighter: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeCigLHave: i32 = 0;
#[no_mangle]
pub static mut g_VRSmokeFingerRotLC: [[f32; 4]; 32] = [[0.0; 4]; 32];
#[no_mangle]
pub static mut g_VRSmokeCigLPos: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRSmokeCigLRot: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
#[no_mangle]
pub static mut g_VRBoneParent: [i16; 2048] = [0; 2048];
#[no_mangle]
pub static mut g_VRBoneCount: i32 = 0;
#[no_mangle]
pub static mut g_VRFKCount: i32 = 0;
#[no_mangle]
pub static mut g_VRRightUpperArmIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRRightForeArmIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRLeftUpperArmIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRLeftForeArmIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRForeTwistR: [i32; 3] = [-1, -1, -1];
#[no_mangle]
pub static mut g_VRForeTwistL: [i32; 3] = [-1, -1, -1];
#[no_mangle]
pub static mut g_VRSpineIdx: [i32; 8] = [-1, -1, -1, -1, -1, -1, -1, -1];
#[no_mangle]
pub static mut g_VRSpineCount: i32 = 0;
#[no_mangle]
pub static mut g_VRRightUpLegIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRLeftUpLegIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRHipsIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRRightLegIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRLeftLegIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRRightFootIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRLeftFootIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRNeckIdx: i32 = -1;
#[no_mangle]
pub static mut g_VRNeck1Idx: i32 = -1;
#[no_mangle]
pub static mut g_VRIKDbgTarget: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKDbgShoulder: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKDbgElbow: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKDbgLocal: [f32; 4] = [0.0; 4];
#[no_mangle]
pub static mut g_VRIKDbgTargetL: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKDbgShoulderL: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKDbgElbowL: [f32; 3] = [0.0; 3];
#[no_mangle]
pub static mut g_VRIKDbgLensL: [f32; 2] = [0.0; 2];
#[no_mangle]
pub static mut g_VRIKDbgLocalL: [f32; 4] = [0.0; 4];
#[no_mangle]
pub static mut g_VRIKDbgLens: [f32; 2] = [0.0; 2];
/// The weapon parts the rig hook looks up by name, in their slot order.
#[no_mangle]
pub static mut kWeaponPartNames: [*const core::ffi::c_char; 9] = [
    c"mag_std".as_ptr(),
    c"front_slider".as_ptr(),
    c"back_slider".as_ptr(),
    c"rotator".as_ptr(),
    c"weapon_trigger".as_ptr(),
    c"hammer".as_ptr(),
    c"bullet".as_ptr(),
    c"bullet_reload".as_ptr(),
    c"barrel".as_ptr(),
];
