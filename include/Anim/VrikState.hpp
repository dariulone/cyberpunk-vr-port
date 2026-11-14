#pragma once

// ================================================================================================
// The VRIK ABI: every global the pose path shares with the natives, and the dimensions that size
// them.
//
// These declarations were scattered through a 4,400-line header that carried its own
// implementation, interleaved with the code using them. They are collected here because they are
// not that file's private state -- they are the contract between the pose-apply detour
// (src/Anim/VrikHook.cpp) and src/Natives/Natives.cpp, which DEFINES all of them.
//
// Nothing here is defined. If you add a name, define it exactly once in Natives.cpp.
// ================================================================================================

#include "Utils/SharedSlots.hpp"

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>

#include <windows.h>
#include <atomic>
#include <cstdint>

// ---- the dimensions: ABI, not private constants. They size the arrays below, so the pose path and
// ---- the natives must agree about them byte for byte.
inline constexpr int VRSMALL_MAX = 16;
inline constexpr int VRSMALL_BONES_MAX = 40;
inline constexpr int VRPAIR_MAX = 24;
// THREE RIGS, not two: a break-action shotgun's top lever lives on a third skeleton, the RECEIVER
// (w_shotgun_dual__rcv1, `barrel_open`), apart from the barrel's frame rig and the shells' magazine rig.
// 2 = that receiver rig.
inline constexpr int VRRIG_N = 3;
// 32 writes and 128 signatures: the shotguns add seven rigs to the thirteen the pistols registered (16
// was one short of the Satara alone), and the Satara's barrel turns its hinge, the lever and the
// rails' twelve magnets at once. 48 held until the SMGs and LMGs (2026-10-25) took reload/rigs.lua to 58:
// the ten past 48 were refused and those guns were recorded on the previous gun's rig.
inline constexpr int VRRIG_WRITES = 32;
inline constexpr int VRRIG_SIG_MAX  = 128;  // registered signatures
inline constexpr int VRRIG_SIG_NAMES = 4;   // named bones checked per signature
inline constexpr int VRRIG_TRACKS = 16;
inline constexpr float    VRRIG_OFF_MAX = 0.80f;
inline constexpr int VRPOSE_CENSUS_MAX = 96;
inline constexpr int VRIK_TRANS_OFF = 0;   // Translation (Vector4)
inline constexpr int VRIK_ROT_OFF   = 16;  // Rotation (Quaternion x,y,z,w)
// The player's meta-rig grows with what is worn and drawn: clothing with its own dangle bones plus drawn arm
// cyberware took a woman's rig past the old 800 (686 holstered, more with the mantis blades out), and a rig over
// the limit is refused -- VRIK let go of the arms, which stayed in the game's flat idle, and the implants'
// swings never started. Fixed-size arrays below are this long; the configure-time scratch is static, not stack.
inline constexpr int VRIK_MAX_BONES = 2048;

// One player solve or rebind owns the shared bone indices, FK scratch and solve
// cache. Weapon/cyberware changes can replace a768-bone meta-rig with a620-bone
// rig; exposing new track pointers before its metadata is ready is unsafe.
inline SRWLOCK g_PlayerPoseStateLock = SRWLOCK_INIT;
// Last completed binding owns the camera's accumulated yaw during a rebind.
// Its temporary boneCount=0 pauses new motion but must not reset the view offset.
inline std::atomic<bool> g_PlayerPoseHeadingReady{false};
// THE MIRROR DOUBLE (src/Natives/VrikArming.cpp): the scene's NPC copy of V, bound from CET, takes the player's
// pose after each pass. While one is bound the scene does not suspend VRIK: the tier below reads -1 then.
bool VRIK_MirrorDoubleActive();
int VRIK_EffectiveCutsceneSuspendTier();
void VRIK_MirrorDoublePass(uint8_t* boneBuf,uintptr_t trackBuf,uint32_t slots);
inline bool VRIK_PlayerPoseFits(uint32_t slots,int bones,int fkCount) {
    return bones>0 && bones<=VRIK_MAX_BONES && slots>=static_cast<uint32_t>(bones) &&
        fkCount>=0 && fkCount<=bones;
}

// ---- the shared globals ----
extern RED4ext::Vector4 g_CameraWorldPos; 
extern int g_CalibrationBoneIndex;
extern volatile int       g_SmallN;
extern volatile uintptr_t g_SmallBuf[VRSMALL_MAX];
extern volatile uint32_t  g_SmallA4[VRSMALL_MAX];
extern volatile uint32_t  g_SmallBones[VRSMALL_MAX];
extern volatile uint32_t  g_SmallOut[VRSMALL_MAX];
extern volatile uint32_t  g_SmallIn[VRSMALL_MAX];
extern volatile int       g_SmallFull;
extern volatile int       g_PairN;
extern volatile uint32_t  g_PairBones[VRPAIR_MAX];
extern volatile uint32_t  g_PairTracks[VRPAIR_MAX];
extern volatile uint32_t  g_PairOut[VRPAIR_MAX];    // hits with a weapon in hand
extern volatile uint32_t  g_PairIn[VRPAIR_MAX];     // hits with none
extern volatile uintptr_t g_PairBuf[VRPAIR_MAX];    // the last track buffer seen with this pair
extern volatile int       g_PairFull;
extern volatile uintptr_t g_RigBuf[VRRIG_N];      // track buffer, once a matching pass has been seen
extern volatile uint32_t  g_RigSeen[VRRIG_N];     // passes matched
extern volatile uint32_t  g_RigBones[VRRIG_N];    // what the descriptor reported, for the log
extern volatile uint32_t  g_RigTracks[VRRIG_N];
extern volatile int       g_RigWriteN;
extern volatile int       g_RigWriteWhich[VRRIG_WRITES];
extern volatile int       g_RigWriteBone[VRRIG_WRITES];
extern volatile float     g_RigWriteOff[VRRIG_WRITES][3];
extern volatile float     g_RigWriteBase[VRRIG_WRITES][3];
extern volatile int       g_RigWriteHaveBase[VRRIG_WRITES];
extern volatile uint32_t  g_RigWriteApplied[VRRIG_WRITES];  // times the SET write actually ran, per slot
extern volatile int       g_RigWriteSlot[VRRIG_WRITES];     // pose-buffer slot the remap resolved this bone to, or -1
extern volatile float     g_RigWriteRotAxis[VRRIG_WRITES][3];
extern volatile float     g_RigWriteRotAngle[VRRIG_WRITES];
extern volatile float     g_RigWriteBaseRot[VRRIG_WRITES][4];
extern volatile int       g_RigWriteHaveBaseRot[VRRIG_WRITES];
extern volatile float     g_RigWriteScale[VRRIG_WRITES];
extern volatile int       g_RigWriteAbs[VRRIG_WRITES];
extern volatile float     g_RigWriteQuat[VRRIG_WRITES][4];
extern volatile int       g_RigWriteQuatOn[VRRIG_WRITES];
extern volatile int       g_RigWriteEnabled[VRRIG_WRITES];
extern volatile int       g_RigWritePin[VRRIG_WRITES];
extern volatile float     g_RigPose[VRRIG_N][20][7];
extern volatile int       g_RigPoseHave[VRRIG_N];
extern volatile int       g_RigSigN;
extern volatile int       g_RigSigWhich[VRRIG_SIG_MAX];
extern volatile uint32_t  g_RigSigBones[VRRIG_SIG_MAX];
extern volatile int       g_RigSigIdx[VRRIG_SIG_MAX][VRRIG_SIG_NAMES];
extern volatile uint64_t  g_RigSigHash[VRRIG_SIG_MAX][VRRIG_SIG_NAMES];
extern volatile float     g_RigTrackVal[VRRIG_N][VRRIG_TRACKS];
extern volatile int       g_RigTrackHave[VRRIG_N];
extern volatile float     g_RigTrackSet[VRRIG_N][VRRIG_TRACKS];
extern volatile int       g_RigTrackOn[VRRIG_N][VRRIG_TRACKS];
extern volatile uint32_t  g_RigPassSeen[4];
extern volatile int       g_RigPassRn[4];
extern volatile int       g_RigPassDst[4];
extern volatile int       g_RigPassSlots[4];   // poseDesc slot count per a4 (buffer size)
extern volatile int       g_RigPassBufLo[4];   // low 31 bits of the pass's bone buffer (buffer identity)
extern volatile int       g_RigPassMap0[8];    // the a4==0 pass's first 4 remap pairs (src,dst)x4
extern volatile int       g_PoseCensusOn;
extern volatile int       g_PoseCensusN;
extern volatile uintptr_t g_PoseCensusBuf[VRPOSE_CENSUS_MAX];
extern volatile uint32_t  g_PoseCensusA4[VRPOSE_CENSUS_MAX];
extern volatile uint32_t  g_PoseCensusHits[VRPOSE_CENSUS_MAX];
extern volatile uint8_t   g_PoseCensusArmed[VRPOSE_CENSUS_MAX];
extern volatile uint32_t  g_PoseCensusHitsOut[VRPOSE_CENSUS_MAX];   // weapon in hand
extern volatile uint32_t  g_PoseCensusHitsIn[VRPOSE_CENSUS_MAX];    // holstered
extern volatile int       g_PoseCensusWeaponOut;   // set from script each frame
extern volatile int       g_PoseCensusFull;        // 1 = the table saturated, so entries are MISSING
extern volatile float     g_WeaponPartOff[24][3];
extern volatile int       g_WeaponPartWriteOn;
extern volatile uintptr_t g_WeaponTrackBufA;
extern volatile uintptr_t g_WeaponTrackBufB;
extern volatile int       g_WeaponRigActive;      // 1 = the hook should look for this rig
extern volatile int       g_WeaponPartCount;
extern volatile int       g_WeaponPartIdx[24];    // bone index per part, -1 if the name is not on this rig
extern volatile float     g_WeaponPartPos[24][3]; // captured PARENT-LOCAL translation
extern volatile float     g_WeaponPartRot[24][4];
extern volatile int       g_WeaponPartHave;
extern volatile uint64_t  g_WeaponMatchCalls;
extern volatile uintptr_t g_PlayerTrackBufA;
extern volatile uintptr_t g_PlayerTrackBufB;
extern volatile uint64_t  g_AnimPoseTotalCalls;
extern volatile uint64_t  g_AnimPoseMatchCalls;
extern volatile uintptr_t g_AnimPoseLastBoneBuf; // last matched player bone buffer (debug)
extern float* g_pSharedHands;                    // shared-memory VR hand data (16 floats/hand layout)
extern volatile int       g_VRBind;              // 0 off, 1=right pos, 2=right pos+rot, 3=both pos(+rot)
extern volatile float     g_VRBindScale;         // position scale (VR units -> model units)
extern volatile float     g_VRBindOffX;
extern volatile float     g_VRBindOffY;
extern volatile float     g_VRBindOffZ;
extern volatile int       g_VRBindAxis;          // axis-remap preset 0..5
extern volatile float     g_VRWristR_I, g_VRWristR_J, g_VRWristR_K, g_VRWristR_R;
extern volatile float     g_VRWristL_I, g_VRWristL_J, g_VRWristL_K, g_VRWristL_R;
extern volatile float     g_VRScaleR, g_VRScaleL;
extern volatile float     g_VROffRX, g_VROffRY, g_VROffRZ;
extern volatile float     g_VROffLX, g_VROffLY, g_VROffLZ;
extern volatile float     g_VRShoulderRX, g_VRShoulderRY, g_VRShoulderRZ;
extern volatile float     g_VRShoulderLX, g_VRShoulderLY, g_VRShoulderLZ;
extern volatile float     g_VRElbowPoleR, g_VRElbowPoleL;
extern volatile float     g_VRElbowSwingR, g_VRElbowSwingL;
extern volatile int       g_VRRightBoneIdx;      // default 24 (RightHand)
extern volatile int       g_VRFppCamIdx[5];      // Torso_fppCamera_* chain (frozen every pass)
extern volatile int       g_VRCamBoneFreeze;     // live toggle: SetVRCamBoneFreeze(0/1) from CET
extern volatile int       g_VRRightUpLegIdx;     // right hip bone (RightUpLeg)
extern volatile int       g_VRLeftUpLegIdx;      // left  hip bone (LeftUpLeg)
extern volatile int       g_VRHipsIdx;           // Hips (pelvis)
extern volatile int       g_VRRightLegIdx;       // RightLeg (knee)
extern volatile int       g_VRLeftLegIdx;        // LeftLeg  (knee)
extern volatile int       g_VRRightFootIdx;      // RightFoot
extern volatile int       g_VRLeftFootIdx;       // LeftFoot
extern volatile int       g_VRNeckIdx;           // Neck
extern volatile int       g_VRNeck1Idx;          // Neck1
extern volatile int       g_VRLeftBoneIdx;       // default 23 (LeftHand)
extern volatile int       g_VRHeadBoneIdx;       // head bone (resolved by name), -1 = none
extern volatile int       g_VREyeLeftIdx;        // LeftEye bone (resolved by name), -1 = none
extern volatile int       g_VREyeRightIdx;       // RightEye bone (resolved by name), -1 = none
extern volatile int       g_VRUseHeadRelative;   // 1 = compose hand pose relative to the head bone
extern volatile int       g_VRDiagCapture;       // 1 = snapshot bones 0..31 (pre-write) into g_VRDiagBones
extern float              g_VRDiagBones[32 * 7];  // per bone: translation(3) + quaternion(4), in buffer space
extern volatile int       g_VRSmokeFingerActive;    // 1 = write captured finger locals every player pass
extern volatile int       g_VRSmokeFingerCapture;   // 1 = capture request (latched on next player pass)
extern volatile int       g_VRSmokeFingerHave;      // 1 once a pose has been captured
extern volatile int       g_VRSmokeFingerCount;     // resolved right-hand finger bone count (0 = none)
extern int                g_VRSmokeFingerIdx[32];   // resolved right-hand finger bone indices
extern float              g_VRSmokeFingerRot[32][4];// captured parent-local rotation (x,y,z,w) per finger bone
extern volatile int       g_VRSmokeCigIdx;
extern volatile int       g_VRSmokeMouthBoneIdx;   // WeaponRight1 leaf (mouth pin), separate from grip
extern volatile int       g_VRSmokeCigHave;
extern volatile int       g_VRSmokeCigEnable;
extern float              g_VRSmokeCigPos[3];
extern float              g_VRSmokeCigRot[4];
extern volatile float     g_VRSmokeCigOffP[3];
extern volatile float     g_VRSmokeCigOffQ[4];
extern volatile float     g_VRSmokeMouthDist;
extern volatile float     g_VRSmokeMouthDistL;
extern volatile int       g_VRSmokeMouthAnchor;
extern volatile float     g_VRSmokeMouthPos[3];
extern volatile float     g_VRSmokeMouthRot[4];
extern volatile int       g_VRSmokeAnchorValid;
extern volatile float     g_VRSmokeAnchorLocalPos[3];
extern volatile float     g_VRSmokeAnchorLocalRot[4];
extern volatile int       g_VRSmokeAnchorBoneSel;
extern volatile int       g_VRSmokeAnchorBoneIdx;
extern volatile int       g_VRSmokeAltAnchorValid;
extern volatile float     g_VRSmokeAltAnchorLocalPos[3];
extern volatile float     g_VRSmokeAltAnchorLocalRot[4];
extern volatile float     g_VRSmokeCigScaleY;
extern volatile float     g_VRSmokeSmokePos[3];
extern volatile float     g_VRSmokeSmokeRot[4];
extern volatile float     g_VRSmokeMouthWorldPos[3];
extern volatile float     g_VRSmokeMouthWorldRot[4];
extern volatile int       g_VRSmokeMouthWorldValid;
extern volatile float     g_VRViewWorldPos[3];
extern volatile float     g_VRViewWorldRot[4];
extern volatile int       g_VRViewWorldValid;
extern volatile int       g_VRPalmRIdx;
extern volatile int       g_VRPalmLIdx;
extern volatile float     g_VRPalmModelR[3];
extern volatile float     g_VRPalmModelL[3];
extern volatile float     g_VRPalmModelRotR[4];
extern volatile float     g_VRPalmModelRotL[4];
extern volatile float     g_VRCamModelPos[3];
extern volatile float     g_VRCamModelRot[4];
extern volatile int       g_VRPalmModelValid;
extern volatile float     g_VRBodyBone[11][3];
extern volatile int       g_VRBodyBoneOk[11];
extern volatile int       g_VRSmokeFingerActiveL;
extern volatile int       g_VRSmokeFingerCaptureL;
extern volatile int       g_VRSmokeFingerHaveL;
extern volatile int       g_VRSmokeFingerCountL;
extern int                g_VRSmokeFingerIdxL[32];
extern float              g_VRSmokeFingerRotL[32][4];
extern volatile int       g_VRSmokeLighterIdx;
extern volatile int       g_VRSmokeLighterHave;
extern volatile int       g_VRSmokeLighterEnable;
extern float              g_VRSmokeLighterPos[3];
extern float              g_VRSmokeLighterRot[4];
extern volatile float     g_VRSmokeLighterOffP[3];
extern volatile float     g_VRSmokeLighterOffQ[4];
extern volatile float     g_VRSmokeThumbFlickL[4];
extern volatile float     g_VRSmokeThumbPressManualL;
extern int                g_VRSmokeThumbIsL[32];
extern volatile int       g_VRSmokeLeftUseCig;
extern volatile int       g_VRSmokeRightLighter;
extern volatile int       g_VRSmokeCigLHave;
extern float              g_VRSmokeFingerRotLC[32][4];
extern float              g_VRSmokeCigLPos[3];
extern float              g_VRSmokeCigLRot[4];
// THE LEFT HAND'S RESTING POSE, captured from the game rather than authored.
//
// There is no "default hand pose" anywhere in this port: while a weapon is out, the game animates the
// left hand into the SUPPORT half of a two-handed grip, and in VR that hand is empty and out in the
// open, so it wears a claw for no reason. The pose that should be there is the one the game itself
// plays when the hands are empty -- so it is taken from there, live, and replayed. Nothing is invented,
// nothing is tuned, and it follows whatever rig the player is on.
//
// A BASE LAYER, never an override: it is written before the reload/preview layer, so a preview still
// fades in from these fingers over its own blend, exactly as it faded in from the animation before.
extern float              g_VRRestFingerRot[32][4];
extern volatile int       g_VRRestFingerHave;      // 1 once a frame has been recorded or loaded
extern volatile int       g_VRRestFingerCount;     // how many bones the recording covers
extern volatile int       g_VRRestFingerApply;     // 1 = replay it while armed (the feature switch)

extern volatile int       g_VRReloadFingerActive[2];
extern float              g_VRReloadFingerRot[2][32][4];
extern volatile int       g_VRReloadFingerSet[2][32];
extern volatile float     g_VRReloadFingerBlend[2];   // 0..1 preview ramp; 1 = full pose, below = nlerp onto live
extern volatile int       g_VRRecordFK;
extern volatile float     g_VRPlayerYaw;          // player world yaw (degrees), pushed from Lua each frame
extern volatile float     g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR; // FPP camera (HMD) world quaternion
extern volatile float     g_VRCamPosX, g_VRCamPosY, g_VRCamPosZ;
extern volatile float     g_VREntityPosX, g_VREntityPosY, g_VREntityPosZ;
extern volatile int       g_VRCamPosValid;
extern volatile float     g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR;
// ---- what the weapon state machine is doing, for the non-VRIK ADS stabilizer --------------------
//
// Published by redscript through SetVRWeaponPoseState / SetVRWeaponRaiseTransition, because these
// three facts have no native channel: the weapon's PlayerStateMachine value (5 = the real ranged
// Ready state), the remaining ADS aim-in time authored by AimingStateEvents, and whether the
// PublicSafeToReady raise is running. The stabilizer needs all three to know which muzzle
// directions are a real aim and when the transition it is correcting has ended.
//
// PLAIN GLOBALS, not shared slots: the natives that receive them and the pose hook that reads them
// are the same DLL now (dabinn's original crossed a plugin boundary that no longer exists here).
// The player's LOCOMOTION state machine value, from gamePSMLocomotionStates: 1 = Crouch,
// 12 = CrouchSprint, 13 = CrouchDodge, 10 = Slide, 0 = Default, 2 = Sprint. -1 = nobody has told us,
// which is what a missing CET bridge looks like and is treated as "no crouch gate".
//
// Published by SetVRLocomotionState from the VRIK CET mod, which reads the PlayerStateMachine
// blackboard -- the same read the weapon state above uses, one field over. Consumed by the XInput
// merge to keep the sprint detent from standing a sneaking player up.
// IS THE GAME SPRINTING, straight from its own SprintEvents state machine: 1 inside the state,
// 0 outside it, -1 = nobody has told us. This is the feedback the sprint gesture closes its loop on.
//
// It exists because the blackboard could not answer: measured, PlayerStateMachine.Locomotion stays at
// Default(0) through a player sprint, so a loop that waited for Sprint(2) never saw its target and kept
// toggling the sprint back off. The state machine itself is unambiguous.
extern volatile int       g_VRSprintActive;
extern volatile int       g_VRLocomotionState;
// The DETAILED locomotion state (gamePSMDetailedLocomotionStates: 5 Slide, 14 Fall, 18-22 the jumps, 23-25 the
// landings), from the same blackboard and push. -1 = nobody has told us. The body under the view takes the clip's
// own torso in those (VRIK_PlaceBodyUnderHMD); the plain state cannot tell a fall from standing (it stays Default).
extern volatile int       g_VRLocomotionDetailed;
// THE TAKEDOWN, BODY-CARRYING AND BODY-DISPOSAL STATES, published together by SetVRPairedAnimationState
// from the VRIK CET mod beside the locomotion value (-1 = nobody has told us). gamePSMTakedown:
// 1 EnteringGrapple, 2 Grapple, 3 Leap, 4 Takedown. gamePSMBodyCarrying: 1 PickUp, 2 Carry, 3 Dispose,
// 4 Drop, 5 Aim, 6 Throw. BodyDisposalDetailed (gamePSMDetailedBodyDisposal): 1 Dispose, 2 Lethal,
// 3 NonLethal -- putting a body into a container is a workspot during which BodyCarrying is already 0.
// What each hands to the animation is decided in Anim/ScenePolicy.hpp.
extern volatile int       g_VRTakedownState;
extern volatile int       g_VRBodyCarryingState;
extern volatile int       g_VRBodyDisposalState;
// AnimFeature_Carry.useBothHands as the carry state machine last wrote it (1 both arms, 0 the left arm
// alone, -1 = nobody has told us), from SetVRBodyCarryBothHands in the BodyCarry redscript.
extern volatile int       g_VRBodyCarryBothHands;
// A FINISHER's paired animation: the player is in a workspot tagged FinisherWorkspot (PlayerPuppet
// m_isInFinisher, from SetVRFinisherActive in the Finisher redscript; 1 inside, 0 outside, -1 = nobody
// has told us), and since when (GetTickCount64 ms).
extern volatile int       g_VRFinisherActive;
extern volatile uint64_t  g_VRFinisherSinceMs;
extern volatile float     g_VRWeaponPsmState;
extern volatile float     g_VRAimInRemaining;
extern volatile int       g_VRWeaponRaiseTransition;
extern volatile float     g_VRCamPairLocalX, g_VRCamPairLocalY, g_VRCamPairLocalZ;
extern volatile int       g_VRCamPairValid;

// One coherent camera/entity publication from SetVRPlayerYaw.  The individual globals above are
// retained for the native/debug ABI, but a pose solve must not assemble a transform by reading them
// one field at a time while the Lua/native writer is in the middle of the next push.
struct VrikTransformSnapshot {
    float camQuat[4];
    float entityQuat[4];
    float cameraMinusEntity[3];   // filtered world-space pair from this same push
    uint32_t valid;
    float bodyCameraMinusEntity[3]; // same packet's native camera base, before VR/bake/IPD
    uint32_t unavailable; // missing centre/expired publication, distinct from a detached camera
};
// Returns true for a coherent publication. Inspect both valid and unavailable:
// an expired/missing-centre packet may retain a recent solved pose; explicit
// invalidation releases it instead of using a different coordinate frame.
bool VRIK_ReadTransformSnapshot(VrikTransformSnapshot* out);
// Native per-frame equivalent. LocateCamera publishes camera(N-1); the next player transform tick
// pairs it with entity(N-1), avoiding both the Lua clock and the Lua pair slew limiter.
bool VRIK_ReadNativeTransformSnapshot(VrikTransformSnapshot* out);
// One completed MAIN component write and that camera owner's actual root
// transform. No Present counter or delayed pairing participates in this packet.
void VRIK_PublishNativeCameraPair(const int32_t* centre,const int32_t* bodyBase,
    const int32_t* entity,const float* cameraQuat,const float* entityQuat,
    uintptr_t owner,uint64_t origin,bool ownerStable);
bool VRIK_ReadCurrentBodyFrame(float* worldPosition,float* worldRotation,float* trackingYaw);

extern volatile float     g_VRUserArmLenR, g_VRUserArmLenL;
extern volatile int       g_VRBodyUnderHMD;   // 1 = reposition upper body under the HMD
extern volatile float     g_VRChestDrop;      // eyes -> chest, down along bodyUp (m)
extern volatile float     g_VRChestFwd;       // eyes -> chest, along bodyFwd (m, -=back)
extern volatile float     g_VRHeadDrop;       // HMD -> head bone, down along bodyUp (m)
extern volatile float     g_VRSquatThreshold; // HMD drop deadzone before the body squats (m)
extern volatile float     g_VRCamSmooth;      // body-anchor camera low-pass (per-frame lerp; 1=off)
extern volatile float     g_VRIKDbgChest[3];
extern volatile float     g_VRIKDbgHipsYaw;
extern volatile float     g_VRIKDbgShModel[3];
extern volatile float     g_VRIKDbgHandFK[3];    // solved right hand, model space (post-solve FK)
extern volatile float     g_VRIKDbgTargetTrace[3]; // right-hand IK target of the last solve
extern volatile int       g_VRPoseCapGen;
extern volatile int       g_VRIKSolvesLastTick;   // matched solves during the PREVIOUS entSeq tick
extern volatile int       g_VRIKSolvesMaxTick;    // max solves per tick since enable
extern volatile uintptr_t g_VRIKLastBufA;         // distinct bone buffers seen within one tick
extern volatile uintptr_t g_VRIKLastBufB;
extern volatile int       g_VRIKReplayTotal;      // same-tick replays served from the solve cache
extern volatile int       g_VRIKFreshTotal;       // solves that actually recomputed the pose
extern "C" __declspec(dllexport) extern volatile int CyberpunkVR_VrikHandFrameOneLatch;
extern volatile int       CyberpunkVR_VrikHandFrameAlign;
extern volatile float     g_VRIKDbgClav[2][8];
extern volatile float     g_VRIKDbgChestTgt[3];
extern int16_t            g_VRBoneParent[VRIK_MAX_BONES]; // metaRig parent index per bone
extern volatile int       g_VRBoneCount;           // bone count (0 = not resolved)
extern volatile int       g_VRBodyFemale;          // V's body: 1 a woman's, 0 a man's, -1 not known yet (the rig policy)
extern volatile int       g_VRFKCount;             // solver-touched bone prefix (0 = full count)
extern volatile int       g_VRRightUpperArmIdx;    // RightArm  (shoulder joint / upper-arm start)
extern volatile int       g_VRRightForeArmIdx;     // RightForeArm (elbow)
extern volatile int       g_VRLeftUpperArmIdx;     // LeftArm
extern volatile int       g_VRLeftForeArmIdx;      // LeftForeArm
extern int                g_VRForeTwistR[3];       // resolved r_Wrist_0..2 / forearmTwist01..03
extern int                g_VRForeTwistL[3];       // resolved l_Wrist_0..2 / forearmTwist01..03
extern int                g_VRSpineIdx[8];         // Spine* torso chain
extern volatile int       g_VRSpineCount;
extern volatile float     g_VRIKDbgTarget[3];
extern volatile float     g_VRIKDbgShoulder[3];
extern volatile float     g_VRIKDbgElbow[3];
extern volatile float     g_VRIKDbgLens[2];        // upperArmLen, foreArmLen
extern volatile float     g_VRIKDbgLocal[4];       // hand pos in body frame: lx,ly,lz, crossAmount
extern volatile float     g_VRIKDbgTargetL[3];     // same, LEFT arm
extern volatile float     g_VRIKDbgShoulderL[3];
extern volatile float     g_VRIKDbgElbowL[3];
extern volatile float     g_VRIKDbgLensL[2];
extern volatile float     g_VRIKDbgLocalL[4];

// ---- state the NATIVES reach into, defined in src/Anim/VrikHook.cpp ---------------------------
//
// This is the family the seam analysis flagged before the split began: leave any of these with
// internal linkage and each translation unit gets its own copy, so bone measurements come back 0,
// the hand stop never fires and a held object sits at the origin -- with nothing logged, because
// from the compiler's point of view nothing went wrong. Declared here for exactly that reason: one
// object, named in one place.
//
// The declarators are copied from the definitions verbatim. Reconstructing them from a parsed type
// and a dimension produced `float (*g_fkPos)[3]` for a `float g_fkPos[N][3]` -- a pointer where an
// array was meant. It compiles, and the linker catches it; a subtler mismatch would not be caught.
extern volatile int   g_VRFKSnapCount;
extern volatile float g_VRFKSnapPos[VRIK_MAX_BONES][3];
extern volatile float g_VRFKSnapRot[VRIK_MAX_BONES][4];
extern volatile float g_VRHandRawModel[2][3];
extern volatile float g_VRHandRawRot[2][4];
extern volatile int   g_VRHandRawValid[2];
extern volatile float g_VRHandRotLock[2][4];
extern volatile int   g_VRHandRotLockOn[2];
extern volatile float g_VRHandStopDeadband;
extern volatile float g_VRHandStopModel[2][3];
extern volatile int   g_VRHandStopValid[2];
extern float g_fkPos[VRIK_MAX_BONES][3];
extern bool     g_handsStableValid;
extern float g_vrikViewScaleUsed;

// THE INCLUDE-ORDER DEPENDENCY THIS SPLIT WAS ALWAYS GOING TO EXPOSE.
//
// The pose path uses g_pSharedHands and never declared it: it compiled only because Natives.cpp
// happened to include the right things before including the pose header. That is not a contract, it
// is a coincidence with a build behind it, and it is the first thing that breaks when the header
// becomes a translation unit of its own. Declared here, where the rest of the ABI lives.
extern float* g_pSharedHands;
