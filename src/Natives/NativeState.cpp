// The definitions behind the VRIK and weapon-aim ABI.
//
// Anim/VrikState.hpp and Anim/WeaponAimState.hpp DECLARE these; this is where they live. They were
// 419 definitions wedged between the includes and the first native of a 9,800-line file, which is
// why the file read as one thing: the state of three subsystems was its preamble.
//
// Nothing here does anything. If a name needs a value at startup, it gets an initialiser here and
// nowhere else -- these are shared across four translation units now, and a second definition would
// be a duplicate symbol rather than a quiet second copy, which is the good outcome.

#include "Anim/VrikHook.hpp"
#include "Core/Paths.hpp"
#include "Anim/WeaponAim.hpp"

#include <windows.h>
#include <atomic>
#include <cstdint>
#include <string>

// ---- Weapon-aim (Hooks/WeaponAim.cpp). Its ~150 experiment globals, and the PrepareAttack and
//      live-projectile finders', went with their hooks on 2026-10-17. ----
volatile uintptr_t g_waExeBase = 0;

// Every diagnostic dump sits beside cyberpunkvrport.log in the plugin's log folder, every recorded pose in its
// grips folder (Core/Paths.hpp).
std::string VRDiagPath(const char* name) { return cvr::paths::FileA(cvr::paths::Folder::Log, name); }
std::string VRGripPath(const char* name) { return cvr::paths::FileA(cvr::paths::Folder::Grips, name); }

HANDLE g_hMapFile = NULL;
// NOT static: the pose path in src/Anim/VrikHook.cpp uses this, and used to reach it only
// because it was textually included into this file. See Anim/VrikState.hpp.
float* g_pSharedHands = nullptr;
bool g_chunkDebugEnabled = false;
bool g_chunkDebugWasEnabled = false;
int32_t g_chunkDebugComponentIndex = -1;
int32_t g_chunkDebugHand = 1; // 0 = left, 1 = right
int32_t g_chunkDebugBitSlots[4] = {0, -1, -1, -1};
int32_t g_animInputTestMode = 0; // 0 off, 1 constant test pose, 2 mapped VR local positions
int32_t g_animParamPersistentPreset = 0;
float g_animParamPersistentValue = 0.0f;
int32_t g_animParamPersistentLastResult = 0;
int32_t g_rootGraphFloatPersistentPreset = 0;
float g_rootGraphFloatPersistentValue = 0.0f;
int32_t g_rootGraphFloatPersistentLastResult = 0;
int32_t g_rootGraphVectorPersistentPreset = 0;
RED4ext::Vector4 g_rootGraphVectorPersistentValue = {0, 0, 0, 0};
int32_t g_rootGraphVectorPersistentLastResult = 0;
int32_t g_rootMetaRigTrackPersistentPreset = 0;
float g_rootMetaRigTrackPersistentValue = 0.0f;
int32_t g_rootMetaRigTrackPersistentLastResult = 0;

int32_t g_rootLiveTrackPersistentPreset = 0;
float g_rootLiveTrackPersistentValue = 0.0f;
int32_t g_rootLiveTrackPersistentArrayMode = 0;
int32_t g_rootLiveTrackPersistentLastResult = 0;

RED4ext::Vector4 g_CameraWorldPos = {0,0,0,1};
int g_CalibrationBoneIndex = -1;

// Pose-apply hook state. See vrik_hook.h.
volatile uintptr_t g_WeaponTrackBufA = 0;
volatile uintptr_t g_WeaponTrackBufB = 0;
volatile int       g_WeaponRigActive = 0;
volatile int       g_WeaponPartCount = 0;
volatile int       g_WeaponPartIdx[24] = {};
volatile float     g_WeaponPartPos[24][3] = {};
volatile float     g_WeaponPartRot[24][4] = {};
volatile int       g_WeaponPartHave = 0;
volatile uint64_t  g_WeaponMatchCalls = 0;
volatile int       g_PoseCensusOn = 0;
volatile int       g_PoseCensusN = 0;
volatile uintptr_t g_PoseCensusBuf[VRPOSE_CENSUS_MAX] = {};
volatile uint32_t  g_PoseCensusA4[VRPOSE_CENSUS_MAX] = {};
volatile uint32_t  g_PoseCensusHits[VRPOSE_CENSUS_MAX] = {};
volatile uint8_t   g_PoseCensusArmed[VRPOSE_CENSUS_MAX] = {};
volatile uint32_t  g_PoseCensusHitsOut[VRPOSE_CENSUS_MAX] = {};
volatile uint32_t  g_PoseCensusHitsIn[VRPOSE_CENSUS_MAX] = {};
volatile int       g_PoseCensusWeaponOut = 0;
volatile int       g_PoseCensusFull = 0;
volatile int       g_SmallN = 0;
volatile uintptr_t g_SmallBuf[VRSMALL_MAX] = {};
volatile uint32_t  g_SmallA4[VRSMALL_MAX] = {};
volatile uint32_t  g_SmallBones[VRSMALL_MAX] = {};
volatile uint32_t  g_SmallOut[VRSMALL_MAX] = {};
volatile uint32_t  g_SmallIn[VRSMALL_MAX] = {};
volatile int       g_SmallFull = 0;
volatile int       g_PairN = 0;
volatile uint32_t  g_PairBones[VRPAIR_MAX] = {};
volatile uint32_t  g_PairTracks[VRPAIR_MAX] = {};
volatile uint32_t  g_PairOut[VRPAIR_MAX] = {};
volatile uint32_t  g_PairIn[VRPAIR_MAX] = {};
volatile uintptr_t g_PairBuf[VRPAIR_MAX] = {};
volatile int       g_PairFull = 0;
volatile uintptr_t g_RigBuf[VRRIG_N] = {};
volatile uint32_t  g_RigSeen[VRRIG_N] = {};
volatile uint32_t  g_RigBones[VRRIG_N] = {};
volatile uint32_t  g_RigTracks[VRRIG_N] = {};
volatile int       g_RigWriteN = 0;
volatile int       g_RigWriteWhich[VRRIG_WRITES] = {};
volatile int       g_RigWriteBone[VRRIG_WRITES] = {};
volatile float     g_RigWriteOff[VRRIG_WRITES][3] = {};
volatile float     g_RigWriteBase[VRRIG_WRITES][3] = {};
volatile int       g_RigWriteHaveBase[VRRIG_WRITES] = {};
volatile uint32_t  g_RigWriteApplied[VRRIG_WRITES] = {};
volatile int       g_RigWriteSlot[VRRIG_WRITES] = {};
volatile float     g_RigWriteRotAxis[VRRIG_WRITES][3] = {};
volatile float     g_RigWriteRotAngle[VRRIG_WRITES] = {};
volatile float     g_RigWriteBaseRot[VRRIG_WRITES][4] = {};
volatile int       g_RigWriteHaveBaseRot[VRRIG_WRITES] = {};
volatile int       g_RigWritePin[VRRIG_WRITES] = {};    // a4==0 authoritative slot; -1 until resolved
volatile float     g_RigPose[VRRIG_N][20][7] = {};      // the GAME's own local transform per rig bone (read-back)
volatile int       g_RigPoseHave[VRRIG_N] = {};
volatile int       g_RigSigN = 0;                       // registered rig signatures (see VRRigSignature)
volatile int       g_RigSigWhich[VRRIG_SIG_MAX] = {};
volatile uint32_t  g_RigSigBones[VRRIG_SIG_MAX] = {};
volatile int       g_RigSigIdx[VRRIG_SIG_MAX][VRRIG_SIG_NAMES] = {};
volatile uint64_t  g_RigSigHash[VRRIG_SIG_MAX][VRRIG_SIG_NAMES] = {};
volatile float     g_RigTrackVal[VRRIG_N][VRRIG_TRACKS] = {};   // the game's own float-track values (read-back)
volatile int       g_RigTrackHave[VRRIG_N] = {};
volatile float     g_RigTrackSet[VRRIG_N][VRRIG_TRACKS] = {};   // ours, applied when the On flag beside it is set
volatile int       g_RigTrackOn[VRRIG_N][VRRIG_TRACKS] = {};
volatile float     g_RigWriteScale[VRRIG_WRITES] = {};  // uniform scale write per slot; <=0 = off
volatile int       g_RigWriteAbs[VRRIG_WRITES] = {};     // 1 = the offset IS the value (recorded-path replay)
volatile float     g_RigWriteQuat[VRRIG_WRITES][4] = {};
volatile int       g_RigWriteQuatOn[VRRIG_WRITES] = {};
// PER-SLOT ENABLE, so a bone can be handed BACK to the animation. A registered write slot used to apply on every
// pose pass for ever, and an offset of zero is NOT the same thing -- it pins the bone to the latched base. That is
// what silently killed the slide's recoil: the module stopped claiming the slide, wrote its closed rest once, and
// the write simply stood there for the rest of the session. VRRigWriteClear is no answer either -- it drops every
// write there is, including the magazine's.
volatile int       g_RigWriteEnabled[VRRIG_WRITES] = {};
volatile uint32_t  g_RigPassSeen[4] = {};               // frame-rig pass telemetry, by a4
volatile int       g_RigPassRn[4] = {};
volatile int       g_RigPassDst[4] = {};
volatile int       g_RigPassSlots[4] = {};
volatile int       g_RigPassBufLo[4] = {};
volatile int       g_RigPassMap0[8] = {};
// The left hand's resting pose, captured from the game's own empty-handed animation. See VrikState.hpp.
float              g_VRRestFingerRot[32][4] = {};
volatile int       g_VRRestFingerHave = 0;
volatile int       g_VRRestFingerCount = 0;
volatile int       g_VRRestFingerApply = 1;
// Reload finger pose (free hand grip while holding a weapon part). Keyed by hand (0 left, 1 right); indexed to
// match the smoke resolver's finger lists (g_VRSmokeFingerIdxL / g_VRSmokeFingerIdx).
volatile int       g_VRReloadFingerActive[2] = {0, 0};
float              g_VRReloadFingerRot[2][32][4] = {};
volatile int       g_VRReloadFingerSet[2][32] = {};
volatile float     g_VRReloadFingerBlend[2] = {1.0f, 1.0f};
// Reload recorder: publish the ANIMATED skeleton FK to the VRBoneModelPos/Rot snapshot each player pass (the
// normal fill is VRIK-solve-only, dead with VRIK off). On only around a recording take.
volatile int       g_VRRecordFK = 0;
volatile float     g_WeaponPartOff[24][3] = {};
volatile int       g_WeaponPartWriteOn = 0;
// Why arming succeeded or failed, stage by stage. [0] animated object found, [1] trackBufA set, [2] trackBufB set,
// [3] metaRig bone count, [4] parts matched, [5] animated components the PLAYER entity owns, [6] how many of those
// were not "root". Reported through VRWeaponRigStatus with negative slots.
volatile int       g_WeaponRigDiag[8] = {};
// The component names found, so a name can be read rather than guessed at.
char        g_WeaponRigNames[512] = {};

// The parts a firearm's rig carries, in the order script sees them. Taken from the Silverhand's own bone list
// (tools/emit_weapon_collision_data.py prints it); a name absent from another weapon's rig simply resolves to -1.
const char* kWeaponPartNames[] = {
    "mag_std", "front_slider", "back_slider", "rotator", "weapon_trigger",
    "hammer", "bullet", "bullet_reload", "barrel",
};
constexpr int kWeaponPartN = int(sizeof(kWeaponPartNames) / sizeof(kWeaponPartNames[0]));

volatile uintptr_t g_PlayerTrackBufA = 0;
volatile uintptr_t g_PlayerTrackBufB = 0;
volatile uint64_t  g_AnimPoseTotalCalls = 0;
volatile uint64_t  g_AnimPoseMatchCalls = 0;
volatile uintptr_t g_AnimPoseLastBoneBuf = 0;

volatile int       g_VRBind = 4;   // 4 = full-arm model-space IK, the only mode there is
volatile float     g_VRBindScale = 1.0f;
volatile float     g_VRBindOffX = 0.0f;
volatile float     g_VRBindOffY = 0.0f;
volatile float     g_VRBindOffZ = 0.23f;  // calibrated: hand anchored ~0.23m above head bone
volatile int       g_VRBindAxis = 1;     // Y-up -> Z-up mapping by default
// Calibrated per-hand wrist corrections, the baked base (Runtimes/WristBase.hpp): right = euler(0,-90,-21),
// left = euler(-180,-90,-15), in SetVRHandOffset's XYZ convention. Replaced as soon as the calibration is published.
volatile float     g_VRWristR_I = 0.12885998f,  g_VRWristR_J = -0.69526621f, g_VRWristR_K = -0.12885998f, g_VRWristR_R = 0.69526621f;
volatile float     g_VRWristL_I = -0.70105738f, g_VRWristL_J = -0.09229596f, g_VRWristL_K = 0.70105738f,  g_VRWristL_R = 0.09229596f;
// Per-hand reach scale + position offset (calibrated: R slightly shorter avatar reach than L).
// 1.0 = true 1:1 reach (no hardcoded per-hand reach fudge; calibration may still override).
volatile float     g_VRScaleR = 1.0f, g_VRScaleL = 1.0f;
volatile float     g_VROffRX = 0.0f, g_VROffRY = 0.0f, g_VROffRZ = 0.0f;
volatile float     g_VROffLX = 0.0f, g_VROffLY = 0.0f, g_VROffLZ = 0.0f;
// T-pose measured real arm length per hand (metres), shoulder->controller in the T-pose.
// 0 = unset -> the gizmo-path arm-bone scaling (VRIK_ArmScale) is disabled. Published by the
// auto-calibration into shared slots [77]/[78]; read in PollVRCalibFromShared.
volatile float     g_VRUserArmLenR = 0.0f, g_VRUserArmLenL = 0.0f;
volatile float     g_VRUserEyeHeight = 0.0f; // T-pose HMD floor height (metres); for Phase 3 body scale
// Phase 2 body-under-HMD: bend the spine so the chest sits under the HMD (fixes head-ahead-of-
// body + gizmo!=hand). Tunable via the overlay; defaults are a first guess to refine from diag.
volatile int       g_VRBodyUnderHMD = 1;
volatile int       g_VRNeutralizeAnimGraph = 1;
volatile float     g_VRChestDrop = 0.40f;   // eyes -> chest down (m)
volatile float     g_VRChestFwd  = -0.05f;  // eyes -> chest forward(+)/back(-) (m)
volatile float     g_VRHeadDrop  = 0.08f;   // head bone sits this far ABOVE the eyes/HMD (m)
volatile float     g_VRSquatThreshold = 0.20f; // HMD must drop more than this (m) before the body squats
volatile float     g_VRCamSmooth = 0.12f; // body-anchor camera low-pass (per-frame lerp; 1=off). Absorbs weapon recoil/draw jerks.
volatile float     g_VRIKDbgChest[3]    = {0,0,0};
volatile float     g_VRIKDbgClav[2][8]  = {{0,0,0,0,0,0,0,0},{0,0,0,0,0,0,0,0}};
volatile float     g_VRIKDbgHipsYaw = 0.0f;
volatile float     g_VRIKDbgShModel[3] = {0,0,0};
volatile float     g_VRIKDbgHandFK[3] = {0,0,0};
volatile float     g_VRIKDbgTargetTrace[3] = {0,0,0};
volatile int       g_VRPoseCapGen = 0;
volatile int       g_VRIKSolvesLastTick = 0;
volatile int       g_VRIKSolvesMaxTick = 0;
volatile uintptr_t g_VRIKLastBufA = 0;
volatile uintptr_t g_VRIKLastBufB = 0;
volatile int       g_VRIKReplayTotal = 0;
// Fresh solves, i.e. how often the arms/body actually move. The replay path re-writes the SAME
// pose bit for bit, so this counter -- not the pose-apply count -- is the VRIK frame rate.
volatile int       g_VRIKFreshTotal = 0;
// Where the hand frame takes the view quaternion from. 1 = the view PACKET (seqlock [143]),
// the same latch the time-align's headOri_view comes from, so the re-basing cancels exactly.
// 0 = the hands snapshot's copy of shared [104..107], which is what it was: a second capture of
// the same publisher, refreshed on the HANDS' cadence, leaving the rotation between two view
// publications in the hand frame -- visible only while the head turns.
extern "C" __declspec(dllexport) volatile int CyberpunkVR_VrikHandFrameOneLatch = 1;
volatile int       CyberpunkVR_VrikHandFrameAlign = 1;
volatile float     g_VRIKDbgChestTgt[3] = {0,0,0};
// Anatomical offset from the HMD to the SHOULDER joint, in HMD-LOCAL OpenXR axes
// (X = right, Y = up, Z = backward). The plugin uses this to convert the HMD-local controller
// position into a shoulder-relative offset so the wrist target stays put when the head rotates.
// Defaults: ~17cm sideways, 17cm below HMD, 5cm behind. Right = +X, Left = -X.
volatile float     g_VRShoulderRX =  0.14f, g_VRShoulderRY = -0.17f, g_VRShoulderRZ = 0.05f;
volatile float     g_VRShoulderLX = -0.14f, g_VRShoulderLY = -0.17f, g_VRShoulderLZ = 0.05f;
// Per-hand elbow pole spin (degrees): fine outward/inward nudge of the bend normal; 0 = natural.
volatile float     g_VRElbowPoleR = 0.0f, g_VRElbowPoleL = 0.0f;
// Elbow-swing heuristic gain (per hand). 1.0 = the faithful heuristic; the left arm is
// mirrored inside the solver, so both default to +1.0.
volatile float     g_VRElbowSwingR = 1.0f, g_VRElbowSwingL = 1.0f;
volatile int       g_VRRightBoneIdx = 24;
volatile int       g_VRLeftBoneIdx = 23;
volatile int       g_VRHeadBoneIdx = -1;  // resolved from metaRig bone names in VRIK_DoArmPlayer
volatile int       g_VREyeLeftIdx  = -1;  // "LeftEye"  metaRig bone, -1 = not found
volatile int       g_VREyeRightIdx = -1;  // "RightEye" metaRig bone, -1 = not found
// FPP-camera control bones (Torso_fppCamera_*): the rig chain the FPP camera slot follows.
// ALL animation-driven camera motion (per-shot recoil kick, melee swing sway, sprint settle,
// idle breathing lean) is authored on these joints inside each weapon/locomotion .anims set.
// The pose hook FREEZES their locals every player pose pass (vrik_hook.h) — the rig-level
// equivalent of the per-weapon "camera-track removal" anim mods, but weapon-agnostic (those
// mods can't process base_melee/katana/knife/revolver anim packings; a rig freeze doesn't care).
volatile int       g_VRFppCamIdx[5] = { -1, -1, -1, -1, -1 };
// Aim_JNT shake-kill MODE (CET: SetVRCamBoneFreeze(mode)). The per-bone bitmask experiment
// isolated the ENTIRE baked camera shake (shot kick / melee swing sway / sprint settle) to
// Torso_fppCamera_Aim_JNT alone — the other four fppCamera joints are left untouched now.
//   0 = stock (DEFAULT until yaw-live is validated in-headset)
//   1 = YAW-LIVE freeze: swing (pitch/roll shake) + translation frozen to rest, twist about
//       the model vertical passes live (the joint also carries the camera's live yaw
//       response; freezing it whole = snap/sprint doubles — proven by mode 2)
//   2 = FULL freeze (diagnostic reference: shake dead, snap/sprint doubles present)
//   3 = SWING-ONLY freeze: mode 1 + translation LIVE (mode 1 still doubled => part of the
//       live response sits in the translation channel; this isolates it)
// DEFAULT = 3, in-headset validated: shake dead (shots, melee swings, sprint settle),
// standing snap turns clean with weapon and empty hands. Known residual: snap DURING SPRINT
// still ghosts — under investigation (first: does stock mode 0 sprint-snap ghost too?).
// MODE 4 (current TEST default) = the mode-3 swing-only freeze on ALL FIVE fppCamera
// joints, not just Aim_JNT. Hunting the [RENDERCAM] foreign frames: episodic 5-8 deg
// pitch/roll reaching the render that the Aim_JNT-only freeze provably lets through
// (mask isolation was validated on shots/melee/sprint, not landing/vault/hit anims).
// If in-headset trembling dies with 4, keep it (or narrow to the joint [82]/[83]
// fingers); if not, revert to 3 and the camera pitch source is NOT the bone chain.
volatile int       g_VRCamBoneFreeze = 4;
// Clean-pair XY slew rate (m/s), live via SetVRPairSlew. 1.0 = compromise default
// (0.5 tested "floaty", raw tested "flash-lurch"); see the limiter in SetVRTransforms.
volatile float     g_VRPairSlewRate = 1.0f;
// Clean-pair one-tick PREDICTION factor (ticks of lead), live via SetVRPairLead.
// The visible sprint-transient jerk is NOT the 20cm camera-lead motion itself (view
// and body ride it TOGETHER) -- it is the solve->render clock skew (~1 tick): the
// anchor consumes the pair one tick before the frame renders, so body trails view by
// rate x skew. Leading the published pair by its own velocity x this factor cancels
// that skew, letting the transition run at natural speed with no relative flash.
// TRIED AT 1.0 AGAINST "I walk BACKWARDS and see my own body" (2026-08-19): WORSE, reverted to 0.
// And it could not have helped, which is worth writing down because the reasoning was available
// before the test: s_pvel is the velocity of the PAIR, not of the player. During steady walking the
// pair is constant -- the limiter passes steady states exactly, by construction -- so s_pvel is ~0
// and the lead term contributes nothing. Lead only acts on the transients at start and stop, which
// is where it felt worse in the headset.
//
// So the backwards-walk body is NOT pair lag. What remains: either the game genuinely holds the FPP
// camera BEHIND the entity while walking backwards (the mirror of the measured 0.20 m forward lead
// during sprint), in which case the pair faithfully reproduces it and the avatar really is in front
// of the eye; or the backwards-step animation pitches the torso forward into view, which is a pose
// question and not an anchor one. Measure which before touching either.
volatile float     g_VRPairLeadTicks = 0.0f;
volatile int       g_VRUseHeadRelative = 1;
volatile int       g_VRDiagCapture = 0;
float              g_VRDiagBones[32 * 7] = {0};

// SMOKE FINGER-HOLD pose (see vrik_hook.h). Right-hand deform finger + metacarpal bones
// only; captured live from the vanilla hold-cigarette workspot and replayed each pass to
// curl the fingers around the cigarette while VRIK keeps the wrist on the controller.
volatile int       g_VRSmokeFingerActive  = 0;
volatile int       g_VRSmokeFingerCapture = 0;
volatile int       g_VRSmokeFingerHave    = 0;
volatile int       g_VRSmokeFingerCount   = 0;
int                g_VRSmokeFingerIdx[32] = {0};
float              g_VRSmokeFingerRot[32][4] = {{0}};
char               g_VRSmokeFingerName[32][48] = {};   // finger bone names (name-keyed dump/load)
// Cigarette slot = WeaponRight bone (28, child of RightHand): moving its LOCAL transform
// moves the attached cig so it sits in the captured finger pinch. Full T+R (unlike fingers,
// rotation-only). Live nudge (OffP/OffQ) lets the pose be fine-tuned in VR then baked.
volatile int       g_VRSmokeCigIdx    = -1;
volatile int       g_VRSprintActive = -1;
volatile int       g_VRLocomotionState = -1;
volatile int       g_VRLocomotionDetailed = -1;
volatile int       g_VRTakedownState = -1;
volatile int       g_VRBodyCarryingState = -1;
volatile int       g_VRBodyDisposalState = -1;
volatile int       g_VRBodyCarryBothHands = -1;
volatile int       g_VRFinisherActive = -1;
volatile uint64_t  g_VRFinisherSinceMs = 0;
volatile float     g_VRWeaponPsmState = 0.0f;
volatile float     g_VRAimInRemaining = 0.0f;
volatile int       g_VRWeaponRaiseTransition = 0;
volatile int       g_VRSmokeMouthBoneIdx = -1;   // WeaponRight1 leaf: pinned to the mouth (cig at lips)
volatile int       g_VRSmokeCigHave   = 0;
volatile int       g_VRSmokeCigEnable = 1;            // 0 = leave WeaponRight alone (fingers only)
float              g_VRSmokeCigPos[3] = {0.0f, 0.0f, 0.0f};
float              g_VRSmokeCigRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
volatile float     g_VRSmokeCigOffP[3] = {-0.012f, -0.008f, -0.015f}; // hand grip position nudge (bone-local) [tuned]
volatile float     g_VRSmokeCigOffQ[4] = {0.0f, 0.0f, 0.0f, 1.0f}; // live rotation nudge (quat)
volatile float     g_VRSmokeMouthDist  = 999.0f;                // cig-slot -> mouth, model space (VRIK frame)
volatile float     g_VRSmokeMouthDistL = 999.0f;                // LEFT hand -> mouth (HMD-local metres)
// MOUTH ANCHOR: when set, the pose hook pins the cig (WeaponRight bone) to a head-anchored point so
// it stays at the lips hands-free (arm can drop). Pos = model-space offset from the head bone; Rot =
// the cig's model-space orientation at the mouth. Both live-tunable via SetVRSmokeMouthOffset in VR.
volatile int       g_VRSmokeMouthAnchor = 0;
volatile float     g_VRSmokeMouthPos[3] = {0.0f, 0.07f, -0.08f};   // HMD-local: x=right, y=forward, z=up (m) [tuned]
volatile float     g_VRSmokeMouthRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
// WeaponRight LOCAL transform that pins the cig at the mouth, computed by the fresh arm solve and
// replayed in the every-pass grip-apply block (so replays don't snap the cig back to the hand).
volatile int       g_VRSmokeAnchorValid  = 0;
volatile float     g_VRSmokeAnchorLocalPos[3] = {0.0f, 0.0f, 0.0f};
volatile float     g_VRSmokeAnchorLocalRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
// GENERAL mouth-pin: pin an ARBITRARY (non-hand) bone to the HMD mouth so a prop attached to a
// non-weapon slot (e.g. Splinter->Neck1/Head) rides the lips hands-free, leaving BOTH weapon slots
// free. Sel: 0=off (use the WeaponRight path above), 1=Neck1, 2=Head, 3=Neck. Resolved to a bone
// index in the pose hook; local computed from the parent's model FK + the mouth model pose.
volatile int       g_VRSmokeAnchorBoneSel  = 0;
volatile int       g_VRSmokeAnchorBoneIdx  = -1;
volatile int       g_VRSmokeAltAnchorValid = 0;
volatile float     g_VRSmokeAltAnchorLocalPos[3] = {0.0f, 0.0f, 0.0f};
volatile float     g_VRSmokeAltAnchorLocalRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
// Burn-down: cig slot-bone Y scale (1.0 full .. shrinks as it burns). Experiment: whether the item
// attachment inherits the WeaponRight bone scale. Driven from reds (burn length).
volatile float     g_VRSmokeCigScaleY   = 1.0f;
// Exhale smoke: its OWN HMD-local offset (pos + orientation), tunable live, independent of the cig
// mouth anchor. World pose (below) = view pose (real HMD) composed with these.
volatile float     g_VRSmokeSmokePos[3]  = {0.0f, 0.07f, -0.08f};
volatile float     g_VRSmokeSmokeRot[4]  = {0.0f, 0.0f, 0.0f, 1.0f};
volatile float     g_VRSmokeMouthWorldPos[3] = {0.0f, 0.0f, 0.0f};
volatile float     g_VRSmokeMouthWorldRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
volatile int       g_VRSmokeMouthWorldValid  = 0;

// Raw rendered-view pose in world space (see vrik_hook.h). The basketball grip builds the hand's
// world position from this plus the controller's HMD-local offset, so the ball sits where the real
// hand is rather than where the FPP camera thinks it is.
volatile float     g_VRViewWorldPos[3] = {0.0f, 0.0f, 0.0f};
volatile float     g_VRViewWorldRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
volatile int       g_VRViewWorldValid  = 0;

// Palm tracking for the VR basketball, sourced from the solved avatar skeleton.
volatile int       g_VRPalmRIdx = -1;
volatile int       g_VRPalmLIdx = -1;
volatile float     g_VRBodyBone[11][3] = {};
volatile int       g_VRBodyBoneOk[11] = {};
volatile float     g_VRPalmModelR[3] = {0.0f, 0.0f, 0.0f};
volatile float     g_VRPalmModelL[3] = {0.0f, 0.0f, 0.0f};
volatile float     g_VRPalmModelRotR[4] = {0.0f, 0.0f, 0.0f, 1.0f};
volatile float     g_VRPalmModelRotL[4] = {0.0f, 0.0f, 0.0f, 1.0f};
volatile float     g_VRCamModelPos[3] = {0.0f, 0.0f, 0.0f};
volatile float     g_VRCamModelRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
volatile int       g_VRPalmModelValid = 0;

// LEFT-HAND mirror (lighter grip): same machinery for the left fingers + WeaponLeft slot.
volatile int       g_VRSmokeFingerActiveL  = 0;
volatile int       g_VRSmokeFingerCaptureL = 0;
volatile int       g_VRSmokeFingerHaveL    = 0;
volatile int       g_VRSmokeFingerCountL   = 0;
int                g_VRSmokeFingerIdxL[32] = {0};
float              g_VRSmokeFingerRotL[32][4] = {{0}};
char               g_VRSmokeFingerNameL[32][48] = {};
volatile int       g_VRSmokeLighterIdx    = -1;   // WeaponLeft bone
volatile int       g_VRSmokeLighterHave   = 0;
volatile int       g_VRSmokeLighterEnable = 1;
float              g_VRSmokeLighterPos[3] = {0.0f, 0.0f, 0.0f};
float              g_VRSmokeLighterRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
volatile float     g_VRSmokeLighterOffP[3] = {0.0f, 0.0f, 0.0f};
volatile float     g_VRSmokeLighterOffQ[4] = {0.0f, 0.0f, 0.0f, 1.0f};
// LEFT thumb "press the lighter" flick: extra rotation added to the left thumb bones,
// scaled by press (0..1) = left VR trigger (shared[67]) or the manual override (tuning).
volatile float     g_VRSmokeThumbFlickL[4] = {0.0f, 0.0f, 0.0f, 1.0f}; // full-press delta quat
volatile float     g_VRSmokeThumbPressManualL = 0.0f;                   // >0 forces press (test/tune)
int                g_VRSmokeThumbIsL[32] = {0};                         // 1 if that left finger slot is a thumb bone

// LEFT-HAND CIGARETTE grip (separate from the lighter): used when the cig is grabbed into the LEFT
// hand from the mouth. Same left finger bones (g_VRSmokeFingerIdxL / g_VRSmokeFingerCountL / NameL)
// but a different pose, loaded from CyberpunkVR_SmokeGrip_Left.ini. g_VRSmokeLeftUseCig picks which
// pose the left-hand apply uses: 0 = lighter, 1 = cigarette.
volatile int       g_VRSmokeLeftUseCig  = 0;
// LEFT-HANDED: the RIGHT hand holds the lighter -- the lighter pose above, mirrored onto the right hand and
// WeaponRight (Anim/LeftHanded.cpp), its thumb on the right trigger. 0 = the right hand holds the cigarette.
volatile int       g_VRSmokeRightLighter = 0;
volatile int       g_VRSmokeCigLHave    = 0;              // cig-left pose available (fingers + WeaponLeft slot)
float              g_VRSmokeFingerRotLC[32][4] = {{0}};   // parallel to g_VRSmokeFingerRotL (same bones)
float              g_VRSmokeCigLPos[3]  = {0.0f, 0.0f, 0.0f};
float              g_VRSmokeCigLRot[4]  = {0.0f, 0.0f, 0.0f, 1.0f};

// Full-arm IK (g_VRBind == 4): bone hierarchy + chain indices (resolved in VRIK_DoArmPlayer).
int16_t            g_VRBoneParent[VRIK_MAX_BONES] = {0};
volatile int       g_VRBoneCount = 0;
volatile int       g_VRFKCount = 0;   // solver-touched bone prefix (0 = use full count)
volatile int       g_VRRightUpperArmIdx = -1; // RightArm  (upper-arm start / shoulder joint)
volatile int       g_VRRightForeArmIdx  = -1; // RightForeArm (elbow)
volatile int       g_VRLeftUpperArmIdx  = -1; // LeftArm
volatile int       g_VRLeftForeArmIdx   = -1; // LeftForeArm
// Forearm twist chains (r/l_forearmTwist01..03_JNT): wrist pronation is distributed along
// these (fractions elbow->wrist) instead of twisting only the hand bone / moving the elbow.
int                g_VRForeTwistR[3]    = {-1,-1,-1};
int                g_VRForeTwistL[3]    = {-1,-1,-1};
int                g_VRSpineIdx[8]      = {-1,-1,-1,-1,-1,-1,-1,-1}; // Spine* torso chain
volatile int       g_VRSpineCount       = 0;
// Hip bones used by the hand-to-holster equip system. The IN-GAME right wrist + these two hip
// bones are all in the same puppet model space, so the Euclidean distance computed in model space
// equals the distance in world space (rigid transform).
volatile int       g_VRRightUpLegIdx    = -1; // RightUpLeg (right hip)
volatile int       g_VRLeftUpLegIdx     = -1; // LeftUpLeg  (left hip)
// Lower-body chain for full-body "move hips under HMD + keep feet on the ground" (Phase 2b).
volatile int       g_VRHipsIdx          = -1; // Hips (pelvis root of the visible body)
volatile int       g_VRRightLegIdx      = -1; // RightLeg  (knee)
volatile int       g_VRLeftLegIdx       = -1; // LeftLeg   (knee)
volatile int       g_VRRightFootIdx     = -1; // RightFoot
volatile int       g_VRLeftFootIdx      = -1; // LeftFoot
volatile int       g_VRNeckIdx          = -1; // Neck (base of the neck, for the spine curve)
volatile int       g_VRNeck1Idx         = -1; // Neck1 (upper neck, if present)

// IK diagnostics (last solve, model space).
volatile float     g_VRIKDbgTarget[3]   = {0,0,0};
volatile float     g_VRIKDbgShoulder[3] = {0,0,0};
volatile float     g_VRIKDbgElbow[3]    = {0,0,0};
volatile float     g_VRIKDbgLocal[4]    = {0,0,0,0};
volatile float     g_VRIKDbgTargetL[3]  = {0,0,0};
volatile float     g_VRIKDbgShoulderL[3]= {0,0,0};
volatile float     g_VRIKDbgElbowL[3]   = {0,0,0};
volatile float     g_VRIKDbgLensL[2]    = {0,0};
volatile float     g_VRIKDbgLocalL[4]   = {0,0,0,0};
volatile float     g_VRIKDbgLens[2]     = {0,0};
