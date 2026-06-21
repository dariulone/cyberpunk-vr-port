#include <RED4ext/RED4ext.hpp>
#include <RED4ext/GameEngine.hpp>
#include <RED4ext/Containers/StaticArray.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/CProperty.hpp>
#include <RED4ext/Scripting/Natives/Generated/WorldPosition.hpp>
#include <RED4ext/Scripting/Natives/Transform.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimGraph.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_IK.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_MeleeIKData.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_WeaponUser.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableBool.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableContainer.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableInt.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableQuaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableTransform.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimationControlBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterAnimFeature.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IKTargetAddEvent.hpp>
#include <RED4ext/Scripting/Natives/Generated/red/Event.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/entIPlacedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimatedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/StaticOrientationProvider.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystem.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystemScriptInterface.hpp>
#include <RED4ext/Scripting/Natives/entSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/GarmentSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/MeshComponent.hpp>
#include <windows.h>
#include <algorithm>
#include <cctype>
#include <cstring>
#include <atomic>
#include <cstdint>
#include <fstream>
#include <utility>
#include <iomanip>
#include <string>

#include "vrik_hook.h"
#include "weapon_aim_hook.h"

// ---- Weapon-aim native hook state (ShotInputClassify redirect) ----
volatile uint64_t  g_shotTick = 0;
volatile uint64_t  g_goCalls = 0;
volatile uint64_t  g_goMutated = 0;
volatile int       g_goMode = 0;
volatile float     g_goTestYaw = 0.0f;
volatile int       g_goPlane = 0;
volatile float     g_goLastQuat[4] = {0,0,0,0};
volatile uint64_t  g_xfCalls = 0;
volatile uint64_t  g_xfMutated = 0;
volatile int       g_xfMode = 0;
volatile float     g_xfTestYaw = 0.0f;
volatile int       g_xfTestPlane = 0;
volatile uint32_t  g_waLastRetRva = 0;
volatile float     g_xfLastOut[4] = {0,0,0,0};
// FIRE-SHOT hook -- live scanner + flexible override of the shot state.
volatile uint64_t  g_fireCalls = 0;
volatile uint64_t  g_fireMutated = 0;
volatile int       g_fireMode = 0;
volatile int       g_firePlane = 0;
volatile float     g_fireTestAng = 0.0f;
volatile int       g_fireNeg = 0;
volatile float     g_fireDir[4] = {0,0,0,0};
volatile float     g_fireDirOut[4] = {0,0,0,0};
volatile int       g_fireScanSrc = 0;
volatile int       g_fireScanRange = 0x2300;
volatile int       g_fireOvrSrc = 0;
volatile int       g_fireOvrOff = 0x80;
volatile int       g_fireXform = 0;
volatile int       g_fireXformOff = 0xF0;
volatile int       g_fireCamSnap = 0;
volatile int       g_fireCamSnapOff = 0xF0;
volatile int       g_fireHitCount = 0;
volatile int       g_fireHitOff[24] = {0};
volatile float     g_fireHitVec[24*3] = {0};
volatile float     g_fireHitDot[24] = {0};
volatile int       g_fireInShot = 0;
// TargetHelper clean controller-redirect (target = origin + ctrlFwd*100).
volatile int       g_waTgtCtrl = 0;
volatile int       g_waTgtNeg = 0;
volatile uint64_t  g_waTgtOvr = 0;
// Projectile ShootEvent startVelocity -> controller (the player bullet IS a projectile).
volatile int       g_waProjCtrl = 0;
volatile int       g_waProjNeg = 0;
volatile int       g_waProjUnguide = 1;
volatile float     g_waProjRange = 1000.0f;
volatile int       g_waProjAlways = 0;
volatile int       g_waProjOriginRow = 3;
volatile uint32_t  g_waProjLastRetRva = 0;
volatile uint32_t  g_waProjRejectReason = 0;
volatile uint32_t  g_waProjGateRva = 0x4E5109;
volatile uint64_t  g_waProjRet36F9FF = 0;
volatile uint64_t  g_waProjRet36FD7C = 0;
volatile uint64_t  g_waProjRet4E5109 = 0;
volatile uint64_t  g_waProjRet4E615F = 0;
volatile float     g_shotOrigin[3] = {0,0,0};
volatile float     g_projDump[64] = {0};
// TRACE-DISPATCHER hook -- the hitscan funnel, gated to the player shot.
volatile uint64_t  g_trShotCalls = 0;
volatile int       g_trRetCount = 0;
volatile uint32_t  g_trRetRing[16] = {0};
volatile uint32_t  g_trCallerRay[16*12] = {0};
volatile float     g_trCallerDir[16*4] = {0};
volatile uint32_t  g_trCallerHits[16] = {0};
volatile int       g_trOverride = 0;
volatile uint32_t  g_trGateRet = 0;
volatile int       g_trWriteOff = 0x18;
volatile int       g_trForce = 0;
volatile int       g_trNeg = 0;
volatile uint64_t  g_trOvrCount = 0;
volatile int       g_shotInProgress = 0;
volatile uintptr_t g_exeBaseTrace = 0;
volatile uint32_t  g_traceRvas[128] = {0};
volatile uint32_t  g_traceRvaCounts[128] = {0};
volatile int       g_traceCount = 0;
volatile uint64_t  g_traceHits = 0;
volatile uintptr_t g_traceAddr = 0;
volatile int       g_traceActive = 0;
volatile int       g_traceGated = 0;
volatile int       g_traceWriteOnly = 0;
volatile uint64_t  g_ssCalls = 0;
volatile uint64_t  g_ssSnapped = 0;
volatile uintptr_t g_ssCamPtr = 0;
volatile int       g_ssEnable = 0;
volatile int       g_ssMode = 0;
volatile float     g_ssTestYaw = 0.0f;
volatile float     g_ssCamQuat[4] = {0,0,0,1};
volatile float     g_ssDiagD0[4] = {0,0,0,0};
volatile float     g_ssDiagF0[4] = {0,0,0,0};
volatile float     g_ssDiag110[4] = {0,0,0,0};
volatile uint64_t  g_waHeadCalls = 0;
volatile uintptr_t g_waHeadObj = 0;
volatile int       g_waHeadForce = 0;
volatile float     g_waHeadYaw = 0.0f;
volatile float     g_waHeadPitch = 0.0f;
volatile float     g_waHeadOrig4E4 = 0.0f;
volatile float     g_waHeadOrig4E8 = 0.0f;
volatile float     g_waHeadVal4B8 = 0.0f;
volatile int       g_waHeadFlag474 = 0;
volatile uint64_t g_waXhCalls = 0;
volatile uint64_t g_waXhMutated = 0;
volatile int      g_waXhSnapped = 0;
volatile float    g_waXhPos[4] = {0};
volatile float    g_waXhDir[4] = {0};
volatile uint64_t g_waProjCalls = 0;
volatile uint64_t g_waProjMutated = 0;
volatile uint64_t g_waTargetCalls = 0;
volatile uint64_t g_waTargetFromShot = 0;
volatile uint64_t g_waClassifyCalls = 0;
volatile uint64_t g_waClassifyFromShot = 0;
volatile uint64_t g_waRedirects = 0;
volatile uint64_t g_waPhysCalls = 0;
volatile uint64_t g_waPhysMutated = 0;
volatile int      g_waPhysPatched = 0;
volatile uint64_t g_waNormShot = 0;
volatile uint64_t g_waNormMutated = 0;
volatile int      g_waNormPatched = 0;
volatile uint64_t g_waFireNormShot = 0;
volatile uint64_t g_waFireNormMutated = 0;
volatile int      g_waFireNormPatched = 0;
volatile int      g_waDbgSnapped = 0;
volatile float    g_waDbgArg3[72] = {0};
volatile float    g_waDbgRay[40] = {0};
volatile float    g_waDbgRayEntry[28] = {0};
volatile uint64_t g_waCandA = 0;
volatile uint64_t g_waCandB = 0;
volatile uint64_t g_waSVP = 0;
volatile uint64_t g_waSFVW = 0;
volatile int      g_waInstalled = 0;
volatile float    g_waTargetOrigin[4] = {0};
volatile float    g_waTargetDir[4] = {0};
volatile uintptr_t g_waExeBase = 0;
volatile int      g_waEnable = 0;
volatile int      g_waMode = 0;
volatile float    g_waFwd[3] = {0, 0, 0};
volatile float    g_waPos[3] = {0, 0, 0};
volatile float    g_waGateDist = 5.0f;
volatile uint32_t g_waFwdSeq = 0;

// --- PrepareAttack hook (projectile launch dir lever; gameAttack_Projectile::PrepareAttack
//     builds the launch event with launchParams.logicalOrientationProvider) ---
volatile uint64_t g_paCalls = 0;       // hook invocations
volatile uint64_t g_paSwaps = 0;       // provider swaps applied
volatile int      g_paInstalled = 0;
volatile int      g_paOn = 1;          // instrument (read-only) enabled
volatile int      g_paSwap = 0;        // 1 = swap launch orientation provider -> controller
volatile uint64_t g_paA1 = 0, g_paA2 = 0, g_paRet = 0;
volatile uintptr_t g_paImpl = 0;       // resolved instance-vtable PrepareAttack impl (diag)
volatile int      g_paProvBase = -1;   // which candidate held the provider: 0=ret 1=*ret 2=a2
volatile int      g_paProvOff = -1;    // byte offset of the OrientationProvider handle
char g_paRetType[96]  = {0};
char g_paProvType[96] = {0};
volatile uint64_t g_paEvQ[24] = {0};   // candidate-event qwords (the base that held the provider)

// --- live projectile finder/steer (gameprojectileComponent) ---
volatile uintptr_t g_projCompVtbl = 0;   // resolved instance vtable (CreateInstance)
volatile uintptr_t g_projLive = 0;       // last found live projectile component
volatile uintptr_t g_projOrientAddr = 0; // abs addr of worldTransform.Orientation (+0xe0) — CE target
volatile int       g_projFound = 0;
volatile int       g_projSteer = 0;      // overwrite orientation -> controller each tick
volatile uint64_t  g_projSteers = 0;
volatile float     g_projOrientQ[4] = {0,0,0,1};  // last read orientation
volatile uint64_t  g_projDumpQ[40] = {0};

// All diagnostic .txt logs go next to the game exe == where dxgi.dll lives (bin\x64),
// so every log (proxy cyberpunkvrport.log + these .txt dumps) sits in one place.
static std::string VRDiagPath(const char* name) {
    char p[MAX_PATH];
    DWORD n = GetModuleFileNameA(nullptr, p, MAX_PATH);
    std::string s(p, n);
    size_t slash = s.find_last_of('\\');
    if (slash != std::string::npos) s.resize(slash + 1);
    s += name;
    return s;
}

static HANDLE g_hMapFile = NULL;
static float* g_pSharedHands = nullptr;
static bool g_chunkDebugEnabled = false;
static bool g_chunkDebugWasEnabled = false;
static int32_t g_chunkDebugComponentIndex = -1;
static int32_t g_chunkDebugHand = 1; // 0 = left, 1 = right
static int32_t g_chunkDebugBitSlots[4] = {0, -1, -1, -1};
static int32_t g_animInputTestMode = 0; // 0 off, 1 constant test pose, 2 mapped VR local positions
static int32_t g_animParamPersistentPreset = 0;
static float g_animParamPersistentValue = 0.0f;
static int32_t g_animParamPersistentLastResult = 0;
static int32_t g_rootGraphFloatPersistentPreset = 0;
static float g_rootGraphFloatPersistentValue = 0.0f;
static int32_t g_rootGraphFloatPersistentLastResult = 0;
static int32_t g_rootGraphVectorPersistentPreset = 0;
static RED4ext::Vector4 g_rootGraphVectorPersistentValue = {0, 0, 0, 0};
static int32_t g_rootGraphVectorPersistentLastResult = 0;
static int32_t g_rootMetaRigTrackPersistentPreset = 0;
static float g_rootMetaRigTrackPersistentValue = 0.0f;
static int32_t g_rootMetaRigTrackPersistentLastResult = 0;

static int32_t g_rootLiveTrackPersistentPreset = 0;
static float g_rootLiveTrackPersistentValue = 0.0f;
static int32_t g_rootLiveTrackPersistentArrayMode = 0;
static int32_t g_rootLiveTrackPersistentLastResult = 0;

RED4ext::Vector4 g_CameraWorldPos = {0,0,0,1};
int g_CalibrationBoneIndex = -1;
void* g_PlayerAnimComponent = nullptr;

// Bone-hook diagnostics (written from Hooked_ComponentFunc21).
volatile uint64_t g_hookTotalCalls = 0;   // every invocation of the hooked function
volatile uint64_t g_hookMatchCalls = 0;   // invocations where rcx == g_PlayerAnimComponent
volatile uint64_t g_hookBoneWrites = 0;   // invocations that reached the bone write

// Capture mode: record rcx of every call that has a skeleton (rcx+0x168->+0xE0 != null),
// so we can check whether the player's animated component flows through this function.
volatile int      g_hookCapture = 0;
volatile uint64_t g_hookSkeletalCalls = 0;
volatile uint32_t g_capturedRcx[32] = {0};   // distinct low-32 rcx of skeletal components
volatile uint64_t g_capturedFull[32] = {0};  // full 64-bit rcx, for arm-by-index / bone count
volatile int      g_capturedCount = 0;

// Pose-apply hook state. See vrik_hook.h.
volatile uintptr_t g_PlayerTrackBufA = 0;
volatile uintptr_t g_PlayerTrackBufB = 0;
volatile int       g_AnimPoseDebug = 0;
volatile uint64_t  g_AnimPoseTotalCalls = 0;
volatile uint64_t  g_AnimPoseMatchCalls = 0;
volatile uintptr_t g_AnimPoseLastBoneBuf = 0;
volatile int       g_AnimPoseTestBone = -1;
volatile float     g_AnimPoseTestMag = 1.0f;

volatile int       g_VRBind = 0;
volatile float     g_VRBindScale = 1.0f;
volatile float     g_VRBindOffX = 0.0f;
volatile float     g_VRBindOffY = 0.0f;
volatile float     g_VRBindOffZ = 0.23f;  // calibrated: hand anchored ~0.23m above head bone
volatile int       g_VRBindAxis = 1;     // Y-up -> Z-up mapping by default
// Calibrated per-hand wrist corrections: right = euler(0,-90,0), left = euler(-180,-90,0).
volatile float     g_VRWristR_I = 0.0f,        g_VRWristR_J = -0.70710678f, g_VRWristR_K = 0.0f,        g_VRWristR_R = 0.70710678f;
volatile float     g_VRWristL_I = -0.70710678f, g_VRWristL_J = 0.0f,        g_VRWristL_K = 0.70710678f, g_VRWristL_R = 0.0f;
// Per-hand reach scale + position offset (calibrated: R slightly shorter avatar reach than L).
volatile float     g_VRScaleR = 1.05f, g_VRScaleL = 1.06f;
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
volatile int       g_VRUseHeadRelative = 1;
volatile int       g_VRDiagCapture = 0;
float              g_VRDiagBones[32 * 7] = {0};

// Full-arm IK (g_VRBind == 4): bone hierarchy + chain indices (resolved in VRIK_DoArmPlayer).
int16_t            g_VRBoneParent[256] = {0};
volatile int       g_VRBoneCount = 0;
volatile int       g_VRRightUpperArmIdx = -1; // RightArm  (upper-arm start / shoulder joint)
volatile int       g_VRRightForeArmIdx  = -1; // RightForeArm (elbow)
volatile int       g_VRLeftUpperArmIdx  = -1; // LeftArm
volatile int       g_VRLeftForeArmIdx   = -1; // LeftForeArm
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

// Defined later in this file; forward-declared so the per-frame calibration
// bridge in UpdateVRIKAnimInputs can read them / trigger a diag dump.
extern volatile float g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR;
static void WriteVRDiagCore(float camX, float camY, float camZ,
                            float qi, float qj, float qk, float qr);

// Reads IK calibration the in-headset overlay published into shared memory
// (slots [33..48]) and applies it to the live globals. When [33] (valid) is 0
// the plugin keeps its baked defaults so the CET sliders still work standalone.
// Also polls the one-shot diag-request counter ([48]) and dumps a diag file
// when it changes, so the overlay's "Log VR Diag" button works in-headset.
static void PollVRCalibFromShared() {
    if (!g_pSharedHands) return;
    // Shoulder anatomical offsets from auto-calibration (slots 70..75, validity in 76).
    // These live outside the regular [34..47] calibration block.
    if (g_pSharedHands[76] != 0.0f) {
        g_VRShoulderRX = g_pSharedHands[70];
        g_VRShoulderRY = g_pSharedHands[71];
        g_VRShoulderRZ = g_pSharedHands[72];
        g_VRShoulderLX = g_pSharedHands[73];
        g_VRShoulderLY = g_pSharedHands[74];
        g_VRShoulderLZ = g_pSharedHands[75];
    }
    // T-pose measured anatomy from auto-calibration: [77]/[78] = real arm length R/L (m),
    // [79] = HMD eye height (m), [80] = valid. Drives the gizmo-path arm-bone scaling
    // (a straight real arm -> straight avatar arm) instead of the old position-scale hack.
    if (g_pSharedHands[80] != 0.0f) {
        g_VRUserArmLenR   = g_pSharedHands[77];
        g_VRUserArmLenL   = g_pSharedHands[78];
        g_VRUserEyeHeight = g_pSharedHands[79];
    }
    if (g_pSharedHands[33] != 0.0f) {
        const float* c = &g_pSharedHands[34]; // scaleR,scaleL,heightR,heightL,swingR,swingL,poleR,poleL, wRpyr(3), wLpyr(3)
        g_VRScaleR = c[0]; g_VRScaleL = c[1];
        g_VROffRZ  = c[2]; g_VROffLZ  = c[3];
        g_VRElbowSwingR = c[4]; g_VRElbowSwingL = c[5];
        g_VRElbowPoleR  = c[6]; g_VRElbowPoleL  = c[7];
        // Wrist corrections: euler(pitch,yaw,roll) deg -> quat (same XYZ convention as SetVRHandOffset).
        const float d2r = 0.01745329252f * 0.5f;
        for (int side = 0; side < 2; ++side) {
            float p = c[8 + side*3], y = c[9 + side*3], r = c[10 + side*3];
            float cp = std::cos(p*d2r), sp = std::sin(p*d2r);
            float cy = std::cos(y*d2r), sy = std::sin(y*d2r);
            float cr = std::cos(r*d2r), sr = std::sin(r*d2r);
            float qi = sp*cy*cr + cp*sy*sr;
            float qj = cp*sy*cr - sp*cy*sr;
            float qk = cp*cy*sr + sp*sy*cr;
            float qr = cp*cy*cr - sp*sy*sr;
            if (side == 0) { g_VRWristR_I = qi; g_VRWristR_J = qj; g_VRWristR_K = qk; g_VRWristR_R = qr; }
            else           { g_VRWristL_I = qi; g_VRWristL_J = qj; g_VRWristL_K = qk; g_VRWristL_R = qr; }
        }
    }
    // One-shot diag request (monotonic counter from the overlay).
    static int s_lastDiagReq = 0;
    int req = static_cast<int>(g_pSharedHands[48]);
    if (req != s_lastDiagReq) {
        s_lastDiagReq = req;
        // Camera position isn't published; the decisive diag lines don't need it.
        WriteVRDiagCore(0, 0, 0, g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR);
    }
}

static RED4ext::world::AnimationSystem* ScanForAnimationSystemInBlock(uint8_t* aBase, size_t aSize, std::ofstream* aOut = nullptr);
static const char* ClassifyQword(uint64_t v);
static uint64_t SafeReadQword(uint8_t* base, size_t off);
static uint32_t SafeReadU32(uint8_t* base, size_t off);
static float SafeReadFloat(uint8_t* base, size_t off);
static RED4ext::CClass* SafeGetObjectType(void* aPtr);

// Cached pointer to the live worldAnimationSystem. The discovery scan
// (SafeGetObjectType over hundreds of arbitrary heap pointers) is the main
// source of intermittent crashes, so we run it once and reuse the result.
static RED4ext::world::AnimationSystem* g_cachedAnimationSystem = nullptr;

static bool ContainsInsensitive(const char* haystack, const char* needle) {
    if (!haystack || !needle) return false;
    std::string h(haystack);
    std::string n(needle);
    std::transform(h.begin(), h.end(), h.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    std::transform(n.begin(), n.end(), n.begin(), [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
    return h.find(n) != std::string::npos;
}

static bool EqualsInsensitive(const char* a, const char* b) {
    if (!a || !b) return false;
    for (; *a && *b; ++a, ++b) {
        if (std::tolower(static_cast<unsigned char>(*a)) != std::tolower(static_cast<unsigned char>(*b)))
            return false;
    }
    return *a == *b;
}

static bool ClassIsA(RED4ext::CClass* type, RED4ext::CName className) {
    while (type) {
        if (type->name == className) return true;
        type = type->parent;
    }
    return false;
}

static bool IsLikelyFppArmComponent(const char* componentName) {
    if (!componentName) return false;
    return ContainsInsensitive(componentName, "_fpp_") ||
           ContainsInsensitive(componentName, "fpp_lights") ||
           ContainsInsensitive(componentName, "strongarms_holstered") ||
           ContainsInsensitive(componentName, "personal_link_default_holstered") ||
           ContainsInsensitive(componentName, "injection_mark") ||
           ContainsInsensitive(componentName, "_pma_base__") ||
           ContainsInsensitive(componentName, "_pwa_base__") ||
           ContainsInsensitive(componentName, "_pma_fpp__neck") ||
           ContainsInsensitive(componentName, "_pwa_fpp__neck") ||
           ContainsInsensitive(componentName, "holstered_arms_data");
}

static uint64_t BuildChunkDebugMask() {
    uint64_t mask = 0;
    for (int i = 0; i < 4; ++i) {
        const int32_t bit = g_chunkDebugBitSlots[i];
        if (bit >= 0 && bit < 64) {
            mask |= (1ull << bit);
        }
    }
    return mask;
}

void EnsureSharedMemory() {
    if (!g_pSharedHands) {
        g_hMapFile = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE, 0, 512, "CyberpunkVR_Hands_Shared");
        if (g_hMapFile) g_pSharedHands = (float*)MapViewOfFile(g_hMapFile, FILE_MAP_ALL_ACCESS, 0, 0, 512);
    }
}

// Seqlock reader facility (g_handsStable / RefreshHandsSnapshot / SharedPose) is
// defined in vrik_hook.h (included above) so the native AnimPose hook there — the
// real per-frame body-IK consumer — can use the same latched snapshot.

void GetLeftVRHandValid(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) *aOut = g_handsStableValid ? (SharedPose(0) > 0.0f) : false;
}

void GetRightVRHandValid(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) *aOut = g_handsStableValid ? (SharedPose(8) > 0.0f) : false;
}

void GetLeftVRHandPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) {
        if (g_handsStableValid) {
            aOut->X = SharedPose(1); aOut->Y = SharedPose(2); aOut->Z = SharedPose(3);
        } else { aOut->X = aOut->Y = aOut->Z = 0.0f; }
        aOut->W = 1.0f;
    }
}

void GetRightVRHandPos(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Vector4* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) {
        if (g_handsStableValid) {
            aOut->X = SharedPose(9); aOut->Y = SharedPose(10); aOut->Z = SharedPose(11);
        } else { aOut->X = aOut->Y = aOut->Z = 0.0f; }
        aOut->W = 1.0f;
    }
}

void GetLeftVRHandRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) {
        if (g_handsStableValid) {
            aOut->i = SharedPose(4); aOut->j = SharedPose(5); aOut->k = SharedPose(6); aOut->r = SharedPose(7);
        } else { aOut->i = aOut->j = aOut->k = 0.0f; aOut->r = 1.0f; }
    }
}

void GetRightVRHandRot(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, RED4ext::Quaternion* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory(); RefreshHandsSnapshot();
    if (aOut) {
        if (g_handsStableValid) {
            aOut->i = SharedPose(12); aOut->j = SharedPose(13); aOut->k = SharedPose(14); aOut->r = SharedPose(15);
        } else { aOut->i = aOut->j = aOut->k = 0.0f; aOut->r = 1.0f; }
    }
}

void IsVRHandLinked(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, bool* aOut, int64_t a4) {
    aFrame->code++; EnsureSharedMemory();
    if (aOut) *aOut = (g_pSharedHands != nullptr);
}

static RED4ext::WeakHandle<RED4ext::IScriptable> g_rightHandEntity;

void SetVRRightHandEntity(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4ext::Handle<RED4ext::IScriptable> ent;
    RED4ext::GetParameter(aFrame, &ent);
    aFrame->code++;
    g_rightHandEntity = ent;
}

void DumpVRFppComponents(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle) { if (aOut) *aOut = -1; return; }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity) { if (aOut) *aOut = -2; return; }

    std::ofstream out(VRDiagPath("fpp_components_dump.txt"), std::ios::out | std::ios::trunc);
    if (!out.is_open()) { if (aOut) *aOut = -3; return; }

    out << "Player entity templatePathHash=0x" << std::hex << playerEntity->templatePath.hash << std::dec << "\n";
    out << "Components:\n";

    int32_t count = 0;
    for (auto& componentHandle : playerEntity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;

        RED4ext::CClass* type = component->GetType();
        const char* typeName = type ? type->name.ToString() : "<null>";
        const char* componentName = component->name.ToString();

        out << "[" << count << "] ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(component) << std::dec
            << " name=" << (componentName ? componentName : "<null>")
            << " type=" << (typeName ? typeName : "<null>")
            << " enabled=" << (component->isEnabled ? 1 : 0);

        if (ClassIsA(type, "entSkinnedMeshComponent")) {
            auto* skinned = reinterpret_cast<RED4ext::ent::SkinnedMeshComponent*>(component);
            out << " meshAppearance=" << skinned->meshAppearance.ToString()
                << " meshPathHash=0x" << std::hex << skinned->mesh.path.hash << std::dec
                << " chunkMask=0x" << std::hex << skinned->chunkMask << std::dec;
        } else if (ClassIsA(type, "entMeshComponent")) {
            auto* mesh = reinterpret_cast<RED4ext::ent::MeshComponent*>(component);
            out << " meshAppearance=" << mesh->meshAppearance.ToString()
                << " meshPathHash=0x" << std::hex << mesh->mesh.path.hash << std::dec
                << " chunkMask=0x" << std::hex << mesh->chunkMask << std::dec
                << " visualScale=(" << mesh->visualScale.X << ", " << mesh->visualScale.Y << ", " << mesh->visualScale.Z << ")";
        }

        if (IsLikelyFppArmComponent(componentName)) {
            out << " [LIKELY_FPP_ARM]";
        }
        out << "\n";
        ++count;
    }

    out.close();
    if (aOut) *aOut = count;
}

void SetVRFppChunkDebugEnabled(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t enabled = 0;
    RED4ext::GetParameter(aFrame, &enabled);
    aFrame->code++;
    // VRFPP chunk debug disabled in this build.
    g_chunkDebugEnabled = false;
}

void SetVRFppChunkDebugComponentIndex(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    g_chunkDebugComponentIndex = -1;
}

void SetVRFppChunkDebugHand(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t hand = 1;
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;
    g_chunkDebugHand = 1;
}

void SetVRFppChunkDebugBits(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t bit0 = -1, bit1 = -1, bit2 = -1, bit3 = -1;
    RED4ext::GetParameter(aFrame, &bit0);
    RED4ext::GetParameter(aFrame, &bit1);
    RED4ext::GetParameter(aFrame, &bit2);
    RED4ext::GetParameter(aFrame, &bit3);
    aFrame->code++;
    g_chunkDebugBitSlots[0] = -1;
    g_chunkDebugBitSlots[1] = -1;
    g_chunkDebugBitSlots[2] = -1;
    g_chunkDebugBitSlots[3] = -1;
}

static RED4ext::ent::AnimationControllerComponent* FindPlayerAnimationController()
{
    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle)
        return nullptr;

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity)
        return nullptr;

    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;
        RED4ext::CClass* type = component->GetType();
        if (type && type->name == "entAnimationControllerComponent")
            return reinterpret_cast<RED4ext::ent::AnimationControllerComponent*>(component);
    }
    return nullptr;
}

static RED4ext::ent::Entity* FindPlayerEntity()
{
    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle)
        return nullptr;

    return reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
}

template<typename T>
static T* GetGameSystem(const char* aClassName)
{
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* gameInstance = framework ? framework->gameInstance : nullptr;
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass(aClassName) : nullptr;
    if (!gameInstance || !cls)
        return nullptr;

    return reinterpret_cast<T*>(gameInstance->GetSystem(cls));
}

static RED4ext::world::AnimationSystem* GetWorldAnimationSystem()
{
    auto* iface = GetGameSystem<RED4ext::world::AnimationSystemScriptInterface>("worldAnimationSystemScriptInterface");
    return iface ? iface->animationSystem : nullptr;
}

static bool IsValidAnimationSystemPtr(void* aPtr)
{
    if (std::strcmp(ClassifyQword(reinterpret_cast<uint64_t>(aPtr)), "HEAP") != 0)
        return false;
    auto* type = SafeGetObjectType(aPtr);
    return type && type->name == "worldAnimationSystem";
}

// Fast, safe path: the worldAnimationSystem consistently sits at the fixed
// chain runtimeScene+0x8 -> +0x38 (confirmed across runs in the lookup dumps).
// This avoids the full memory scan that calls GetType() on hundreds of
// arbitrary pointers and intermittently crashes. Only one GetType() call here.
static RED4ext::world::AnimationSystem* TryKnownAnimationSystemChain(RED4ext::world::RuntimeScene* aRuntimeScene)
{
    if (!aRuntimeScene)
        return nullptr;

    uint64_t parent = SafeReadQword(reinterpret_cast<uint8_t*>(aRuntimeScene), 0x8);
    if (std::strcmp(ClassifyQword(parent), "HEAP") != 0)
        return nullptr;

    uint64_t cand = SafeReadQword(reinterpret_cast<uint8_t*>(parent), 0x38);
    if (std::strcmp(ClassifyQword(cand), "HEAP") != 0)
        return nullptr;

    if (IsValidAnimationSystemPtr(reinterpret_cast<void*>(cand)))
        return reinterpret_cast<RED4ext::world::AnimationSystem*>(cand);

    return nullptr;
}

static RED4ext::world::AnimationSystem* FindWorldAnimationSystemFromScene(RED4ext::world::RuntimeScene* aRuntimeScene,
                                                                          uintptr_t aFrameworkScene,
                                                                          std::ofstream* aOut = nullptr)
{
    // Reuse a previously discovered system if it still looks valid. This skips
    // the expensive/fragile memory scan on every call.
    if (g_cachedAnimationSystem && IsValidAnimationSystemPtr(g_cachedAnimationSystem))
        return g_cachedAnimationSystem;
    g_cachedAnimationSystem = nullptr;

    if (auto* sys = GetWorldAnimationSystem())
    {
        g_cachedAnimationSystem = sys;
        return sys;
    }

    // Fast direct chain first (cheap + safe), scan only as last resort.
    if (auto* sys = TryKnownAnimationSystemChain(aRuntimeScene))
    {
        if (aOut)
            *aOut << "found via fixed chain runtimeScene+0x8->+0x38\n";
        g_cachedAnimationSystem = sys;
        return sys;
    }

    if (aOut)
        *aOut << "GetWorldAnimationSystem() returned null, scanning runtime scene memory...\n";

    if (auto* sys = ScanForAnimationSystemInBlock(reinterpret_cast<uint8_t*>(aRuntimeScene), 0x4B8, aOut))
    {
        g_cachedAnimationSystem = sys;
        return sys;
    }

    if (aFrameworkScene)
    {
        if (auto* sys = ScanForAnimationSystemInBlock(reinterpret_cast<uint8_t*>(aFrameworkScene), 0x800, aOut))
        {
            g_cachedAnimationSystem = sys;
            return sys;
        }
    }

    return nullptr;
}

static RED4ext::anim::AnimatedObject* FindPlayerAnimatedObjectByComponentName(const char* aComponentName)
{
    auto* playerEntity = FindPlayerEntity();
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* animationSystem = playerEntity
        ? FindWorldAnimationSystemFromScene(playerEntity->runtimeScene, framework ? framework->unk18 : 0, nullptr)
        : nullptr;
    if (!playerEntity || !animationSystem || !aComponentName)
        return nullptr;

    for (uint32_t bucketIndex = 0; bucketIndex < RED4ext::world::AnimationSystem::BucketCount; ++bucketIndex)
    {
        __try
        {
            auto& bucket = animationSystem->entitityBuckets[bucketIndex];
            const uint32_t entryCount = bucket.entities.Size();
            for (uint32_t entryIndex = 0; entryIndex < entryCount; ++entryIndex)
            {
                auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(bucket.entities[entryIndex].instance);
                if (entity != playerEntity)
                    continue;

                auto* animatedComponent = bucket.animatedComponents[entryIndex];
                const char* name = animatedComponent ? animatedComponent->name.ToString() : nullptr;
                if (name && std::strcmp(name, aComponentName) == 0)
                    return bucket.animatedObjects[entryIndex];
            }
        }
        __except (EXCEPTION_EXECUTE_HANDLER)
        {
        }
    }

    return nullptr;
}

// Returns the live animatedComponent instance from the worldAnimationSystem bucket
// (the object actually driven each frame), which differs from the handle wrapper
// in playerEntity->components. This is the instance whose IComponent::Update fires.
static RED4ext::ent::AnimatedComponent* FindPlayerBucketAnimatedComponent(const char* aComponentName)
{
    auto* playerEntity = FindPlayerEntity();
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* animationSystem = playerEntity
        ? FindWorldAnimationSystemFromScene(playerEntity->runtimeScene, framework ? framework->unk18 : 0, nullptr)
        : nullptr;
    if (!playerEntity || !animationSystem || !aComponentName)
        return nullptr;

    for (uint32_t bucketIndex = 0; bucketIndex < RED4ext::world::AnimationSystem::BucketCount; ++bucketIndex)
    {
        __try
        {
            auto& bucket = animationSystem->entitityBuckets[bucketIndex];
            const uint32_t entryCount = bucket.entities.Size();
            for (uint32_t entryIndex = 0; entryIndex < entryCount; ++entryIndex)
            {
                auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(bucket.entities[entryIndex].instance);
                if (entity != playerEntity)
                    continue;

                auto* animatedComponent = bucket.animatedComponents[entryIndex];
                const char* name = animatedComponent ? animatedComponent->name.ToString() : nullptr;
                if (name && std::strcmp(name, aComponentName) == 0)
                    return animatedComponent;
            }
        }
        __except (EXCEPTION_EXECUTE_HANDLER)
        {
        }
    }

    return nullptr;
}

void DumpAnimationSystemLookup(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("animation_system_lookup.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* gameInstance = framework ? framework->gameInstance : nullptr;
    auto* rtti = RED4ext::CRTTISystem::Get();

    out << "playerEntity=0x" << std::hex << reinterpret_cast<uintptr_t>(playerEntity)
        << " runtimeScene=0x" << reinterpret_cast<uintptr_t>(playerEntity ? playerEntity->runtimeScene : nullptr)
        << "\n";
    out << "engine=0x" << reinterpret_cast<uintptr_t>(engine)
        << " framework=0x" << reinterpret_cast<uintptr_t>(framework)
        << " gameInstance=0x" << reinterpret_cast<uintptr_t>(gameInstance)
        << " worldSceneFromFramework=0x" << static_cast<uintptr_t>(framework ? framework->unk18 : 0)
        << std::dec << "\n\n";

    const char* names[] = {
        "worldAnimationSystem",
        "worldAnimationSystemScriptInterface",
        "AnimationSystem",
        "worldRuntimeScene"
    };

    for (const char* name : names)
    {
        auto* nativeCls = rtti ? rtti->GetClass(name) : nullptr;
        auto* scriptCls = rtti ? rtti->GetClassByScriptName(name) : nullptr;
        auto* nativeSys = (gameInstance && nativeCls) ? gameInstance->GetSystem(nativeCls) : nullptr;
        auto* scriptSys = (gameInstance && scriptCls) ? gameInstance->GetSystem(scriptCls) : nullptr;

        out << "name=" << name << "\n";
        out << "  nativeCls=0x" << std::hex << reinterpret_cast<uintptr_t>(nativeCls)
            << " scriptCls=0x" << reinterpret_cast<uintptr_t>(scriptCls) << std::dec << "\n";
        out << "  nativeClsName=" << (nativeCls ? nativeCls->name.ToString() : "<null>")
            << " scriptClsName=" << (scriptCls ? scriptCls->name.ToString() : "<null>") << "\n";
        out << "  nativeSystem=0x" << std::hex << reinterpret_cast<uintptr_t>(nativeSys)
            << " scriptSystem=0x" << reinterpret_cast<uintptr_t>(scriptSys) << std::dec << "\n\n";
    }

    auto* scanned = FindWorldAnimationSystemFromScene(playerEntity ? playerEntity->runtimeScene : nullptr,
                                                      framework ? framework->unk18 : 0,
                                                      &out);
    out << "scannedAnimationSystem=0x" << std::hex << reinterpret_cast<uintptr_t>(scanned) << std::dec << "\n";

    if (aOut) *aOut = 1;
}

static const char* GetTypeNameForDump(RED4ext::rtti::IType* aType)
{
    if (!aType)
        return "<null>";

    const char* computed = aType->GetComputedName().ToString();
    if (computed && computed[0])
        return computed;

    const char* native = aType->GetName().ToString();
    return native ? native : "<unnamed>";
}

static void DumpFunctionDetails(std::ofstream& aOut, RED4ext::CBaseFunction* aFunc)
{
    if (!aFunc)
        return;

    aOut << aFunc->fullName.ToString() << "\n";
    aOut << "  shortName=" << aFunc->shortName.ToString() << "\n";
    aOut << "  return=" << (aFunc->returnType ? GetTypeNameForDump(aFunc->returnType->type) : "Void") << "\n";
    aOut << "  params(" << aFunc->params.Size() << ")\n";

    for (uint32_t i = 0; i < aFunc->params.Size(); ++i)
    {
        auto* param = aFunc->params[i];
        aOut << "    [" << i << "] name=" << (param ? param->name.ToString() : "<null>")
             << " type=" << (param ? GetTypeNameForDump(param->type) : "<null>")
             << " out=" << ((param && param->flags.isOut) ? 1 : 0)
             << " optional=" << ((param && param->flags.isOptional) ? 1 : 0)
             << "\n";
    }

    aOut << "\n";
}

static void DumpClassProperties(std::ofstream& aOut, const char* aClassName)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass(aClassName) : nullptr;

    aOut << "==================================================\n";
    aOut << "CLASS " << aClassName << "\n";
    aOut << "==================================================\n";
    if (!cls)
    {
        aOut << "<missing>\n\n";
        return;
    }

    RED4ext::DynArray<RED4ext::CProperty*> props;
    cls->GetProperties(props);

    aOut << "size=" << cls->size << " alignment=" << cls->alignment << " propCount=" << props.Size() << "\n";
    for (uint32_t i = 0; i < props.Size(); ++i)
    {
        auto* prop = props[i];
        aOut << "  [" << i << "] name=" << (prop ? prop->name.ToString() : "<null>")
             << " type=" << (prop ? GetTypeNameForDump(prop->type) : "<null>")
             << " offset=0x" << std::hex << (prop ? prop->valueOffset : 0) << std::dec
             << " inHolder=" << ((prop && prop->flags.inValueHolder) ? 1 : 0)
             << " handle=" << ((prop && prop->flags.isHandle) ? 1 : 0)
             << "\n";
    }

    aOut << "\n";
}

static RED4ext::CProperty* FindFirstPropertyByType(RED4ext::CClass* aClass, const char* aTypeSubstring, uint32_t aMinOffset = 0)
{
    if (!aClass || !aTypeSubstring)
        return nullptr;

    RED4ext::DynArray<RED4ext::CProperty*> props;
    aClass->GetProperties(props);
    for (uint32_t i = 0; i < props.Size(); ++i)
    {
        auto* prop = props[i];
        if (!prop || prop->valueOffset < aMinOffset)
            continue;

        if (ContainsInsensitive(GetTypeNameForDump(prop->type), aTypeSubstring))
            return prop;
    }

    return nullptr;
}

static RED4ext::Handle<RED4ext::ent::IPositionProvider> CreateStaticPositionProvider(const RED4ext::Vector4& aPosition)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entStaticPositionProvider") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    if (auto* vectorProp = FindFirstPropertyByType(cls, "Vector4", 0x50))
    {
        vectorProp->SetValue<RED4ext::Vector4>(instance, aPosition);
        return RED4ext::Handle<RED4ext::ent::IPositionProvider>(reinterpret_cast<RED4ext::ent::IPositionProvider*>(instance));
    }

    if (auto* worldPosProp = FindFirstPropertyByType(cls, "WorldPosition", 0x50))
    {
        const RED4ext::WorldPosition worldPos(aPosition);
        worldPosProp->SetValue<RED4ext::WorldPosition>(instance, worldPos);
        return RED4ext::Handle<RED4ext::ent::IPositionProvider>(reinterpret_cast<RED4ext::ent::IPositionProvider*>(instance));
    }

    return {};
}

static RED4ext::Handle<RED4ext::ent::IOrientationProvider> CreateStaticOrientationProvider()
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entStaticOrientationProvider") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* provider = reinterpret_cast<RED4ext::ent::StaticOrientationProvider*>(instance);
    provider->staticOrientation.i = 0.0f;
    provider->staticOrientation.j = 0.0f;
    provider->staticOrientation.k = 0.0f;
    provider->staticOrientation.r = 1.0f;
    return RED4ext::Handle<RED4ext::ent::IOrientationProvider>(provider);
}

// Static orientation provider with a given quaternion (= the VR controller aim). Used to swap the
// projectile launch's logicalOrientationProvider so the bullet flies down the controller/barrel.
static RED4ext::Handle<RED4ext::ent::IOrientationProvider> CreateStaticOrientationProviderQ(const RED4ext::Quaternion& aQuat)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entStaticOrientationProvider") : nullptr;
    if (!cls) return {};
    auto* instance = cls->CreateInstance(true);
    if (!instance) return {};
    cls->InitializeProperties(instance);
    auto* provider = reinterpret_cast<RED4ext::ent::StaticOrientationProvider*>(instance);
    provider->staticOrientation = aQuat;
    return RED4ext::Handle<RED4ext::ent::IOrientationProvider>(provider);
}

static void AppendPlayerControllerIKState(std::ofstream& aOut)
{
    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
    {
        aOut << "playerEntity=<null>\n";
        return;
    }

    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        auto* type = component->GetType();
        if (!type || type->name != "entAnimationControllerComponent")
            continue;

        auto* controller = reinterpret_cast<RED4ext::ent::AnimationControllerComponent*>(component);
        aOut << "controller name=" << component->name.ToString()
             << " ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(controller) << std::dec
             << " targetData.size=" << controller->ikTargetController.targetData.Size()
             << " ikParams=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.ikParams.instance) << std::dec
             << "\n";

        for (uint32_t i = 0; i < controller->ikTargetController.targetData.Size(); ++i)
        {
            const auto& target = controller->ikTargetController.targetData[i];
            aOut << "  [" << i << "] id=" << target.targetReference.id
                 << " part=" << target.targetReference.part.ToString()
                 << " posProvider=0x" << std::hex << reinterpret_cast<uintptr_t>(target.positionProvider.instance)
                 << " orientProvider=0x" << reinterpret_cast<uintptr_t>(target.orientationProvider.instance)
                 << std::dec << "\n";
        }
    }
}

static void DumpBindingInfo(std::ofstream& aOut, RED4ext::ent::AnimationControlBinding* aBinding, const char* aPrefix)
{
    if (!aBinding)
    {
        aOut << aPrefix << "binding=<null>\n";
        return;
    }

    auto* serializable = reinterpret_cast<RED4ext::ISerializable*>(aBinding);
    auto* type = serializable->GetType();
    auto* binding = reinterpret_cast<RED4ext::ent::IBinding*>(aBinding);

    aOut << aPrefix
         << "binding ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(aBinding) << std::dec
         << " type=" << (type ? type->name.ToString() : "<null>")
         << " enabled=" << (binding->enabled ? 1 : 0)
         << " bindName=" << binding->bindName.ToString()
         << "\n";
}

static void DumpAnimTrackParameters(std::ofstream& aOut, RED4ext::ent::AnimatedComponent* aAnimated, const char* aPrefix)
{
    if (!aAnimated)
        return;

    for (uint32_t i = 0; i < aAnimated->animParameters.Size(); ++i)
    {
        const auto& param = aAnimated->animParameters[i];
        aOut << aPrefix
             << "param[" << i << "] track=" << param.animTrackName.ToString()
             << " name=" << param.parameterName.ToString()
             << " default=" << param.defaultValue
             << "\n";
    }
}

template<typename T>
static T* LoadResourceRef(RED4ext::Ref<T>& aRef)
{
    if (aRef.path.IsEmpty())
        return nullptr;

    if (!aRef.IsLoaded())
    {
        if (!aRef.Load())
            return nullptr;
    }

    auto& handle = aRef.Get();
    return handle.instance;
}

static void DumpNameListFiltered(std::ofstream& aOut, const char* aLabel, const RED4ext::DynArray<RED4ext::CName>& aNames)
{
    aOut << aLabel << " count=" << aNames.Size() << "\n";
    for (uint32_t i = 0; i < aNames.Size(); ++i)
    {
        const char* name = aNames[i].ToString();
        if (!name)
            continue;

        if (ContainsInsensitive(name, "arm") ||
            ContainsInsensitive(name, "hand") ||
            ContainsInsensitive(name, "ik") ||
            ContainsInsensitive(name, "weapon") ||
            ContainsInsensitive(name, "zoom") ||
            ContainsInsensitive(name, "render") ||
            ContainsInsensitive(name, "visibility"))
        {
            aOut << "  - " << name << "\n";
        }
    }
}

static RED4ext::ent::AnimatedComponent* FindPlayerAnimatedComponentByName(const char* aName)
{
    if (!aName)
        return nullptr;

    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
        return nullptr;

    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        auto* type = component->GetType();
        if (!type || type->name != "entAnimatedComponent")
            continue;

        const char* name = component->name.ToString();
        if (name && std::strcmp(name, aName) == 0)
            return reinterpret_cast<RED4ext::ent::AnimatedComponent*>(component);
    }

    return nullptr;
}

static bool IsInterestingAnimName(const char* aName)
{
    if (!aName)
        return false;

    return ContainsInsensitive(aName, "arm") ||
           ContainsInsensitive(aName, "hand") ||
           ContainsInsensitive(aName, "ik") ||
           ContainsInsensitive(aName, "weapon") ||
           ContainsInsensitive(aName, "zoom") ||
           ContainsInsensitive(aName, "render") ||
           ContainsInsensitive(aName, "visibility") ||
           ContainsInsensitive(aName, "camera") ||
           ContainsInsensitive(aName, "left") ||
           ContainsInsensitive(aName, "right");
}

static void DumpAnimGraphVariables(std::ofstream& aOut, RED4ext::anim::AnimGraph* aGraph, const char* aPrefix)
{
    auto* vars = aGraph ? aGraph->variables.instance : nullptr;
    if (!vars)
    {
        aOut << aPrefix << "graphVariables=<null>\n";
        return;
    }

    aOut << aPrefix << "graphVariables bool=" << vars->boolVariables.Size()
         << " int=" << vars->intVariables.Size()
         << " float=" << vars->floatVariables.Size()
         << " vector=" << vars->vectorVariables.Size()
         << " quat=" << vars->quaternionVariables.Size()
         << " transform=" << vars->transformVariables.Size()
         << "\n";

    for (uint32_t i = 0; i < vars->boolVariables.Size(); ++i)
    {
        auto* v = vars->boolVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "bool[" << i << "] name=" << name
             << " value=" << (v->value ? 1 : 0)
             << " default=" << (v->default_ ? 1 : 0)
             << "\n";
    }

    for (uint32_t i = 0; i < vars->intVariables.Size(); ++i)
    {
        auto* v = vars->intVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "int[" << i << "] name=" << name
             << " value=" << v->value
             << " default=" << v->default_
             << " min=" << v->min
             << " max=" << v->max
             << "\n";
    }

    for (uint32_t i = 0; i < vars->floatVariables.Size(); ++i)
    {
        auto* v = vars->floatVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "float[" << i << "] name=" << name
             << " value=" << v->value
             << " default=" << v->default_
             << " min=" << v->min
             << " max=" << v->max
             << "\n";
    }

    for (uint32_t i = 0; i < vars->vectorVariables.Size(); ++i)
    {
        auto* v = vars->vectorVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "vector[" << i << "] name=" << name
             << " value=(" << v->x << ", " << v->y << ", " << v->z << ", " << v->w << ")"
             << " default=(" << v->default_.X << ", " << v->default_.Y << ", " << v->default_.Z << ", " << v->default_.W << ")"
             << "\n";
    }

    for (uint32_t i = 0; i < vars->quaternionVariables.Size(); ++i)
    {
        auto* v = vars->quaternionVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "quat[" << i << "] name=" << name
             << " value=(roll=" << v->roll << ", pitch=" << v->pitch << ", yaw=" << v->yaw << ")"
             << "\n";
    }

    for (uint32_t i = 0; i < vars->transformVariables.Size(); ++i)
    {
        auto* v = vars->transformVariables[i].instance;
        const char* name = v ? v->name.ToString() : nullptr;
        if (!v || !IsInterestingAnimName(name))
            continue;
        aOut << aPrefix << "transform[" << i << "] name=" << name << "\n";
    }
}

static void DumpMetaRigTracks(std::ofstream& aOut, RED4ext::anim::MetaRig* aMetaRig, const char* aPrefix)
{
    if (!aMetaRig)
    {
        aOut << aPrefix << "metaRig=<null>\n";
        return;
    }

    aOut << aPrefix << "metaRig hash=0x" << std::hex << aMetaRig->hash << std::dec
         << " boneCount=" << aMetaRig->boneNames.Size()
         << " trackCount=" << aMetaRig->trackNames.Size()
         << " refTrackCount=" << aMetaRig->referenceTracks.Size()
         << "\n";

    const uint32_t count = (aMetaRig->trackNames.Size() < aMetaRig->referenceTracks.Size())
        ? aMetaRig->trackNames.Size()
        : aMetaRig->referenceTracks.Size();
    for (uint32_t i = 0; i < count; ++i)
    {
        const char* name = aMetaRig->trackNames[i].ToString();
        if (!IsInterestingAnimName(name))
            continue;

        aOut << aPrefix << "track[" << i << "] name=" << (name ? name : "<null>")
             << " value=" << aMetaRig->referenceTracks[i]
             << "\n";
    }
}

static bool ResolveRootMetaRigTrackPreset(int32_t aMode, RED4ext::CName& aOut)
{
    switch (aMode)
    {
    case 1:
        aOut = RED4ext::CName("leftArmBodyPartVis");
        return true;
    case 2:
        aOut = RED4ext::CName("rightArmBodyPartVis");
        return true;
    case 3:
        aOut = RED4ext::CName("leftHandWorldSpace");
        return true;
    case 4:
        aOut = RED4ext::CName("rightHandWorldSpace");
        return true;
    case 5:
        aOut = RED4ext::CName("allowFeetIk");
        return true;
    case 6:
        aOut = RED4ext::CName("enableLeftFootIk");
        return true;
    case 7:
        aOut = RED4ext::CName("enableRightFootIk");
        return true;
    case 8:
        aOut = RED4ext::CName("cameraUpOffset");
        return true;
    default:
        return false;
    }
}

static void AppendRootMetaRigTrackLog(const char* aSource, const RED4ext::CName& aKey, float aValue, int32_t aResult)
{
    std::ofstream out(VRDiagPath("root_metarig_track_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "source=" << (aSource ? aSource : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << aValue
        << " result=" << aResult
        << "\n";
}

static int32_t SetRootMetaRigTrackValue(const RED4ext::CName& aName, float aValue)
{
    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    auto* metaRig = animatedObject ? animatedObject->metaRig : nullptr;
    if (!metaRig)
        return -81;

    const uint32_t count = (metaRig->trackNames.Size() < metaRig->referenceTracks.Size())
        ? metaRig->trackNames.Size()
        : metaRig->referenceTracks.Size();

    int32_t updated = 0;
    for (uint32_t i = 0; i < count; ++i)
    {
        if (metaRig->trackNames[i] == aName)
        {
            __try
            {
                metaRig->referenceTracks[i] = aValue;
                ++updated;
            }
            __except (EXCEPTION_EXECUTE_HANDLER)
            {
                return -86;
            }
        }
    }

    return updated;
}

// Writes into the LIVE runtime track storage of the root AnimatedObject, not the
// read-only metaRig->referenceTracks resource defaults (which crash on write).
// The live float arrays were discovered by scan:
//   animatedObject+0x8  -> owner+0x40  (DynArray<float>, mirrors track values)
//   animatedObject+0x18 -> owner+0x18  (DynArray<float>, mirrors track values)
// DynArray layout: entries@+0x0, capacity@+0x8, size@+0xC.
// aArrayMode: 0 = both arrays, 1 = 0x8/0x40 only, 2 = 0x18/0x18 only.
static int32_t SetRootLiveTrackValue(const RED4ext::CName& aName, float aValue, int32_t aArrayMode)
{
    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    if (!animatedObject)
        return -90;

    __try
    {
        auto* metaRig = animatedObject->metaRig;
        if (!metaRig || std::strcmp(ClassifyQword(reinterpret_cast<uint64_t>(metaRig)), "HEAP") != 0)
            return -90;

        // Resolve track name -> index from the live trackNames table.
        int32_t trackIndex = -1;
        const uint32_t nameCount = metaRig->trackNames.Size();
        if (nameCount == 0 || nameCount > 8192)
            return -91;
        for (uint32_t i = 0; i < nameCount; ++i)
        {
            if (metaRig->trackNames[i] == aName)
            {
                trackIndex = static_cast<int32_t>(i);
                break;
            }
        }
        if (trackIndex < 0)
            return -91;

        struct LiveArray { size_t ownerOff; size_t arrOff; };
        const LiveArray arrays[2] = { { 0x8, 0x40 }, { 0x18, 0x18 } };

        uint8_t* base = reinterpret_cast<uint8_t*>(animatedObject);
        int32_t writes = 0;

        for (int32_t a = 0; a < 2; ++a)
        {
            if (aArrayMode == 1 && a != 0) continue;
            if (aArrayMode == 2 && a != 1) continue;

            uint64_t owner = SafeReadQword(base, arrays[a].ownerOff);
            if (std::strcmp(ClassifyQword(owner), "HEAP") != 0)
                continue;

            uint8_t* ownerBase = reinterpret_cast<uint8_t*>(owner);
            uint64_t entries = SafeReadQword(ownerBase, arrays[a].arrOff + 0x0);
            uint32_t size = SafeReadU32(ownerBase, arrays[a].arrOff + 0xC);
            if (std::strcmp(ClassifyQword(entries), "HEAP") != 0)
                continue;
            if (static_cast<uint32_t>(trackIndex) >= size)
                continue;

            float* arr = reinterpret_cast<float*>(entries);
            arr[trackIndex] = aValue;
            ++writes;
        }

        return writes;
    }
    __except (EXCEPTION_EXECUTE_HANDLER)
    {
        return -92;
    }
}

// Reads the current LIVE track value (the same arrays SetRootLiveTrackValue writes).
// Returns value*1000 rounded as int so it survives the Int32 return path, or a
// negative sentinel on failure. aArrayMode: 1 = 0x8/0x40, 2 = 0x18/0x18 (default 1).
static int32_t ReadRootLiveTrackValue(const RED4ext::CName& aName, int32_t aArrayMode)
{
    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    if (!animatedObject)
        return -90;

    __try
    {
        auto* metaRig = animatedObject->metaRig;
        if (!metaRig || std::strcmp(ClassifyQword(reinterpret_cast<uint64_t>(metaRig)), "HEAP") != 0)
            return -90;

        int32_t trackIndex = -1;
        const uint32_t nameCount = metaRig->trackNames.Size();
        if (nameCount == 0 || nameCount > 8192)
            return -91;
        for (uint32_t i = 0; i < nameCount; ++i)
        {
            if (metaRig->trackNames[i] == aName) { trackIndex = static_cast<int32_t>(i); break; }
        }
        if (trackIndex < 0)
            return -91;

        const size_t ownerOff = (aArrayMode == 2) ? 0x18 : 0x8;
        const size_t arrOff   = (aArrayMode == 2) ? 0x18 : 0x40;

        uint64_t owner = SafeReadQword(reinterpret_cast<uint8_t*>(animatedObject), ownerOff);
        if (std::strcmp(ClassifyQword(owner), "HEAP") != 0)
            return -92;
        uint8_t* ownerBase = reinterpret_cast<uint8_t*>(owner);
        uint64_t entries = SafeReadQword(ownerBase, arrOff + 0x0);
        uint32_t size = SafeReadU32(ownerBase, arrOff + 0xC);
        if (std::strcmp(ClassifyQword(entries), "HEAP") != 0 || static_cast<uint32_t>(trackIndex) >= size)
            return -92;

        float v = reinterpret_cast<float*>(entries)[trackIndex];
        return static_cast<int32_t>(v * 1000.0f);
    }
    __except (EXCEPTION_EXECUTE_HANDLER)
    {
        return -92;
    }
}

static int32_t SetRootGraphFloatVariable(const RED4ext::CName& aName, float aValue)
{
    auto* animated = FindPlayerAnimatedComponentByName("root");
    auto* graph = animated ? LoadResourceRef(animated->graph) : nullptr;
    auto* vars = graph ? graph->variables.instance : nullptr;
    if (!vars)
        return -70;

    int32_t updated = 0;
    for (uint32_t i = 0; i < vars->floatVariables.Size(); ++i)
    {
        auto* v = vars->floatVariables[i].instance;
        if (v && v->name == aName)
        {
            v->value = aValue;
            ++updated;
        }
    }
    return updated;
}

static int32_t SetRootGraphBoolVariable(const RED4ext::CName& aName, bool aValue)
{
    auto* animated = FindPlayerAnimatedComponentByName("root");
    auto* graph = animated ? LoadResourceRef(animated->graph) : nullptr;
    auto* vars = graph ? graph->variables.instance : nullptr;
    if (!vars)
        return -71;

    int32_t updated = 0;
    for (uint32_t i = 0; i < vars->boolVariables.Size(); ++i)
    {
        auto* v = vars->boolVariables[i].instance;
        if (v && v->name == aName)
        {
            v->value = aValue;
            ++updated;
        }
    }
    return updated;
}

static int32_t SetRootGraphVectorVariable(const RED4ext::CName& aName, const RED4ext::Vector4& aValue)
{
    auto* animated = FindPlayerAnimatedComponentByName("root");
    auto* graph = animated ? LoadResourceRef(animated->graph) : nullptr;
    auto* vars = graph ? graph->variables.instance : nullptr;
    if (!vars)
        return -72;

    int32_t updated = 0;
    for (uint32_t i = 0; i < vars->vectorVariables.Size(); ++i)
    {
        auto* v = vars->vectorVariables[i].instance;
        if (v && v->name == aName)
        {
            v->x = aValue.X;
            v->y = aValue.Y;
            v->z = aValue.Z;
            v->w = aValue.W;
            ++updated;
        }
    }
    return updated;
}

static void DumpAnimatedResourceDetails(std::ofstream& aOut, RED4ext::ent::AnimatedComponent* aAnimated, const char* aPrefix)
{
    if (!aAnimated)
        return;

    auto* graph = LoadResourceRef(aAnimated->graph);
    if (graph)
    {
        aOut << aPrefix << "graphLoaded=1 animFeatures=" << graph->animFeatures.Size()
             << " additionalAnimDatabases=" << graph->additionalAnimDatabases.Size()
             << " useAnimCommands=" << (graph->useAnimCommands ? 1 : 0)
             << " hasMixerSlot=" << (graph->hasMixerSlot ? 1 : 0)
             << "\n";
        DumpAnimGraphVariables(aOut, graph, aPrefix);

        for (uint32_t i = 0; i < graph->animFeatures.Size(); ++i)
        {
            const auto& feature = graph->animFeatures[i];
            const char* name = feature.name.ToString();
            const char* className = feature.className.ToString();
            aOut << aPrefix << "feature[" << i << "] name=" << (name ? name : "<null>")
                 << " class=" << (className ? className : "<null>")
                 << " forceAllocate=" << (feature.forceAllocate ? 1 : 0)
                 << "\n";
        }

        for (uint32_t i = 0; i < graph->additionalAnimDatabases.Size(); ++i)
        {
            const auto& db = graph->additionalAnimDatabases[i];
            aOut << aPrefix << "animDb[" << i << "] name=" << db.name.ToString()
                 << " dbHash=0x" << std::hex << db.animDatabase.path.hash
                 << " overrideHash=0x" << db.overrideAnimDatabase.path.hash
                 << std::dec << "\n";
        }
    }
    else
    {
        aOut << aPrefix << "graphLoaded=0\n";
    }

    auto* rig = LoadResourceRef(aAnimated->rig);
    if (rig)
    {
        aOut << aPrefix << "rigLoaded=1 boneCount=" << rig->boneNames.Size()
             << " trackCount=" << rig->trackNames.Size()
             << " ikSetups=" << rig->ikSetups.Size()
             << "\n";
        DumpNameListFiltered(aOut, "    rig.trackNames", rig->trackNames);
        DumpNameListFiltered(aOut, "    rig.boneNames", rig->boneNames);
    }
    else
    {
        aOut << aPrefix << "rigLoaded=0\n";
    }

    aOut << aPrefix << "setup.gameplay count=" << aAnimated->animations.gameplay.Size() << "\n";
    for (uint32_t i = 0; i < aAnimated->animations.gameplay.Size(); ++i)
    {
        const auto& entry = aAnimated->animations.gameplay[i];
        aOut << aPrefix << "gameplay[" << i << "] animSetHash=0x" << std::hex << entry.animSet.path.hash << std::dec
             << " priority=" << static_cast<int32_t>(entry.priority)
             << " varCount=" << entry.variableNames.Size() << "\n";
        DumpNameListFiltered(aOut, "      gameplay.variables", entry.variableNames);
    }

    aOut << aPrefix << "setup.cinematics count=" << aAnimated->animations.cinematics.Size() << "\n";
    for (uint32_t i = 0; i < aAnimated->animations.cinematics.Size(); ++i)
    {
        const auto& entry = aAnimated->animations.cinematics[i];
        aOut << aPrefix << "cinematics[" << i << "] animSetHash=0x" << std::hex << entry.animSet.path.hash << std::dec
             << " priority=" << static_cast<int32_t>(entry.priority)
             << " varCount=" << entry.variableNames.Size() << "\n";
        DumpNameListFiltered(aOut, "      cinematic.variables", entry.variableNames);
    }
}

static void DumpEntityAnimationInfo(std::ofstream& aOut, RED4ext::ent::Entity* aEntity, const char* aReason)
{
    if (!aEntity)
        return;

    aOut << "ENTITY reason=" << (aReason ? aReason : "<null>")
         << " ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(aEntity) << std::dec
         << " appearance=" << aEntity->appearanceName.ToString()
         << " templateHash=0x" << std::hex << aEntity->templatePath.hash << std::dec
         << " status=" << static_cast<int32_t>(aEntity->status)
         << "\n";

    for (auto& componentHandle : aEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        auto* type = component->GetType();
        if (!type)
            continue;

        const bool isController = (type->name == "entAnimationControllerComponent");
        const bool isAnimated = (type->name == "entAnimatedComponent");
        if (!isController && !isAnimated)
            continue;

        aOut << "  component name=" << component->name.ToString()
             << " type=" << type->name.ToString()
             << " ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(component) << std::dec
             << " enabled=" << (component->isEnabled ? 1 : 0)
             << (IsLikelyFppArmComponent(component->name.ToString()) ? " [LIKELY_FPP_ARM]" : "")
             << "\n";

        if (isController)
        {
            auto* controller = reinterpret_cast<RED4ext::ent::AnimationControllerComponent*>(component);
            aOut << "    controlBinding=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->controlBinding.instance)
                 << " ikTargets=" << std::dec << controller->ikTargetController.targetData.Size()
                 << " ikParams=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.ikParams.instance)
                 << std::dec << "\n";
            DumpBindingInfo(aOut, controller->controlBinding.instance, "    ");
        }
        else
        {
            auto* animated = reinterpret_cast<RED4ext::ent::AnimatedComponent*>(component);
            aOut << "    rigHash=0x" << std::hex << animated->rig.path.hash
                 << " graphHash=0x" << animated->graph.path.hash
                 << " controlBinding=0x" << reinterpret_cast<uintptr_t>(animated->controlBinding.instance)
                 << std::dec << " animParams=" << animated->animParameters.Size()
                 << "\n";
            DumpBindingInfo(aOut, animated->controlBinding.instance, "    ");
            DumpAnimTrackParameters(aOut, animated, "    ");
            DumpAnimatedResourceDetails(aOut, animated, "    ");
        }
    }

    aOut << "\n";
}

static RED4ext::Handle<RED4ext::anim::AnimFeature> CreateWeaponUserFeature(const RED4ext::Vector4& aLeft,
                                                                           const RED4ext::Vector4& aRight)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("animAnimFeature_WeaponUser") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* feature = reinterpret_cast<RED4ext::anim::AnimFeature_WeaponUser*>(instance);
    feature->ikLeftHandLocalPosition = aLeft;
    feature->ikRightHandLocalPosition = aRight;

    return RED4ext::Handle<RED4ext::anim::AnimFeature>(reinterpret_cast<RED4ext::anim::AnimFeature*>(feature));
}

static RED4ext::Handle<RED4ext::anim::AnimFeature> CreateIKFeature(const RED4ext::Vector4& aPoint,
                                                                   const RED4ext::Vector4& aNormal,
                                                                   float aWeight)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("animAnimFeature_IK") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* feature = reinterpret_cast<RED4ext::anim::AnimFeature_IK*>(instance);
    feature->point = aPoint;
    feature->normal = aNormal;
    feature->weight = aWeight;
    return RED4ext::Handle<RED4ext::anim::AnimFeature>(reinterpret_cast<RED4ext::anim::AnimFeature*>(feature));
}

static RED4ext::Handle<RED4ext::anim::AnimFeature> CreateMeleeIKDataFeature(const RED4ext::Vector4& aHead,
                                                                             const RED4ext::Vector4& aChest,
                                                                             const RED4ext::Vector4& aOffset)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("animAnimFeature_MeleeIKData") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* feature = reinterpret_cast<RED4ext::anim::AnimFeature_MeleeIKData*>(instance);
    feature->isValid = true;
    feature->headPosition = aHead;
    feature->chestPosition = aChest;
    feature->ikOffset = aOffset;
    return RED4ext::Handle<RED4ext::anim::AnimFeature>(reinterpret_cast<RED4ext::anim::AnimFeature*>(feature));
}

static RED4ext::Handle<RED4ext::ent::AnimInputSetterVector> CreateVectorInputEvent(const RED4ext::CName& aKey,
                                                                                     const RED4ext::Vector4& aValue)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entAnimInputSetterVector") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* evt = reinterpret_cast<RED4ext::ent::AnimInputSetterVector*>(instance);
    evt->key = aKey;
    evt->value = aValue;
    return RED4ext::Handle<RED4ext::ent::AnimInputSetterVector>(evt);
}

static RED4ext::Handle<RED4ext::ent::AnimInputSetterFloat> CreateFloatInputEvent(const RED4ext::CName& aKey,
                                                                                  float aValue)
{
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entAnimInputSetterFloat") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* evt = reinterpret_cast<RED4ext::ent::AnimInputSetterFloat*>(instance);
    evt->key = aKey;
    evt->value = aValue;
    return RED4ext::Handle<RED4ext::ent::AnimInputSetterFloat>(evt);
}

static RED4ext::Handle<RED4ext::ent::AnimInputSetterAnimFeature> CreateFeatureInputEvent(const RED4ext::CName& aKey,
                                                                                           const RED4ext::Handle<RED4ext::anim::AnimFeature>& aFeature)
{
    if (!aFeature)
        return {};

    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entAnimInputSetterAnimFeature") : nullptr;
    if (!cls)
        return {};

    auto* instance = cls->CreateInstance(true);
    if (!instance)
        return {};

    cls->InitializeProperties(instance);

    auto* evt = reinterpret_cast<RED4ext::ent::AnimInputSetterAnimFeature*>(instance);
    evt->key = aKey;
    evt->delay = 0.0f;
    evt->value = aFeature;
    return RED4ext::Handle<RED4ext::ent::AnimInputSetterAnimFeature>(evt);
}

static RED4ext::Handle<RED4ext::ent::AnimInputSetterAnimFeature> CreateFeatureInputEvent(const RED4ext::CName& aKey,
                                                                                           const RED4ext::Vector4& aLeft,
                                                                                           const RED4ext::Vector4& aRight)
{
    auto feature = CreateWeaponUserFeature(aLeft, aRight);
    return CreateFeatureInputEvent(aKey, feature);
}

static bool QueuePlayerEvent(const RED4ext::Handle<RED4ext::red::Event>& aEvent)
{
    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
        return false;

    auto* cls = playerEntity->GetType();
    auto* func = cls ? cls->GetFunction("QueueEvent") : nullptr;
    if (!func)
        return false;

    RED4ext::StackArgs_t args;
    args.emplace_back(nullptr, const_cast<RED4ext::Handle<RED4ext::red::Event>*>(&aEvent));
    return RED4ext::ExecuteFunction(playerEntity, func, nullptr, args);
}

static int32_t SetFloatInputDirect(const RED4ext::CName& aKey, float aValue)
{
    auto* controller = FindPlayerAnimationController();
    if (!controller)
        return -40;

    auto* cls = controller->GetType();
    auto* func = cls ? cls->GetFunction("SetInputFloat") : nullptr;
    if (!func)
        return -41;

    RED4ext::StackArgs_t args;
    args.emplace_back(nullptr, const_cast<RED4ext::CName*>(&aKey));
    args.emplace_back(nullptr, &aValue);
    return RED4ext::ExecuteFunction(controller, func, nullptr, args) ? 10 : -42;
}

static int32_t QueueFloatInputEvent(const RED4ext::CName& aKey, float aValue)
{
    auto evt = CreateFloatInputEvent(aKey, aValue);
    if (!evt)
        return -43;

    return QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(evt)) ? 11 : -44;
}

static void AppendAnimFloatTestLog(const char* aRouteName, const RED4ext::CName& aKey, float aValue, int32_t aResult)
{
    std::ofstream out(VRDiagPath("anim_float_input_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "route=" << (aRouteName ? aRouteName : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << aValue
        << " result=" << aResult
        << "\n";
}

static bool ResolveAnimFloatPreset(int32_t aMode, RED4ext::CName& aOut)
{
    switch (aMode)
    {
    case 1:
        aOut = RED4ext::CName("visibilityLeftArm");
        return true;
    case 2:
        aOut = RED4ext::CName("visibilityRightArm");
        return true;
    case 3:
        aOut = RED4ext::CName("renderPlaneLeftArm");
        return true;
    case 4:
        aOut = RED4ext::CName("renderPlane");
        return true;
    case 5:
        aOut = RED4ext::CName("renderPlaneInspect");
        return true;
    default:
        return false;
    }
}

static bool ResolveRootGraphFloatPreset(int32_t aMode, RED4ext::CName& aOut)
{
    switch (aMode)
    {
    case 1:
        aOut = RED4ext::CName("disable_maya_engine_right_hand");
        return true;
    case 2:
        aOut = RED4ext::CName("disable_maya_engine_left_hand");
        return true;
    case 3:
        aOut = RED4ext::CName("pla_right_hand_attach");
        return true;
    case 4:
        aOut = RED4ext::CName("pla_left_hand_attach");
        return true;
    case 5:
        aOut = RED4ext::CName("procedural_ironsight_camera");
        return true;
    case 6:
        aOut = RED4ext::CName("camera_pitch");
        return true;
    case 7:
        aOut = RED4ext::CName("camera_yaw");
        return true;
    default:
        return false;
    }
}

static bool ResolveRootGraphVectorPreset(int32_t aMode, RED4ext::CName& aOut)
{
    switch (aMode)
    {
    case 1:
        aOut = RED4ext::CName("weapon_offset_shoulder");
        return true;
    case 2:
        aOut = RED4ext::CName("weapon_offset_aiming");
        return true;
    case 3:
        aOut = RED4ext::CName("weapon_rotation_shoulder");
        return true;
    case 4:
        aOut = RED4ext::CName("weapon_rotation_aiming");
        return true;
    case 5:
        aOut = RED4ext::CName("debug_stand_camera_position");
        return true;
    case 6:
        aOut = RED4ext::CName("debug_crouch_camera_position");
        return true;
    default:
        return false;
    }
}

static int32_t ForceVRNeutralAnimGraphInputs()
{
    int32_t writes = 0;
    auto add = [&](int32_t r) {
        if (r > 0) writes += r;
    };

    // Weapon/camera stance inputs that visually move the FPP camera, shoulders and hands.
    // Do not touch ironsight/ADS-specific inputs; weapon aiming must stay gameplay-correct.
    add(SetRootGraphFloatVariable(RED4ext::CName("disable_maya_engine_right_hand"), 1.0f));
    add(SetRootGraphFloatVariable(RED4ext::CName("disable_maya_engine_left_hand"), 1.0f));
    add(SetRootGraphFloatVariable(RED4ext::CName("camera_pitch"), 0.0f));
    add(SetRootGraphFloatVariable(RED4ext::CName("camera_yaw"), 0.0f));

    RED4ext::Vector4 zero{0, 0, 0, 0};
    add(SetRootGraphVectorVariable(RED4ext::CName("weapon_offset_shoulder"), zero));
    add(SetRootGraphVectorVariable(RED4ext::CName("weapon_rotation_shoulder"), zero));
    add(SetRootGraphVectorVariable(RED4ext::CName("debug_stand_camera_position"), zero));
    add(SetRootGraphVectorVariable(RED4ext::CName("debug_crouch_camera_position"), zero));

    // Runtime track arrays are the values the evaluated graph reads. Keep camera vertical offset
    // neutral and leave feet IK enabled; this is applied every frame while VRIK is active.
    add(SetRootLiveTrackValue(RED4ext::CName("cameraUpOffset"), 0.0f, 0));
    add(SetRootLiveTrackValue(RED4ext::CName("allowFeetIk"), 1.0f, 0));
    add(SetRootLiveTrackValue(RED4ext::CName("enableLeftFootIk"), 1.0f, 0));
    add(SetRootLiveTrackValue(RED4ext::CName("enableRightFootIk"), 1.0f, 0));

    return writes;
}

static void AppendRootGraphVariableLog(const char* aSource, const RED4ext::CName& aKey, const char* aValueText, int32_t aResult)
{
    std::ofstream out(VRDiagPath("root_graph_variable_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "source=" << (aSource ? aSource : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << (aValueText ? aValueText : "<null>")
        << " result=" << aResult
        << "\n";
}

static void AppendDirectAnimParamLog(const char* aSource, const RED4ext::CName& aKey, float aValue, int32_t aResult)
{
    std::ofstream out(VRDiagPath("anim_parameter_direct_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "source=" << (aSource ? aSource : "<null>")
        << " key=" << aKey.ToString()
        << " value=" << aValue
        << " result=" << aResult
        << "\n";
}

static int32_t SetPlayerAnimatedParameterValue(const RED4ext::CName& aKey, float aValue)
{
    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
        return -50;

    int32_t updated = 0;
    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        auto* type = component->GetType();
        if (!type || type->name != "entAnimatedComponent")
            continue;

        auto* animated = reinterpret_cast<RED4ext::ent::AnimatedComponent*>(component);
        for (uint32_t i = 0; i < animated->animParameters.Size(); ++i)
        {
            auto& param = animated->animParameters[i];
            if (param.parameterName == aKey || param.animTrackName == aKey)
            {
                param.defaultValue = aValue;
                ++updated;
            }
        }
    }

    return updated;
}

static int32_t QueueVectorInputEvents(const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight)
{
    RED4ext::CName leftKey("ikLeftHandLocalPosition");
    RED4ext::CName rightKey("ikRightHandLocalPosition");

    auto leftEvt = CreateVectorInputEvent(leftKey, aLeft);
    auto rightEvt = CreateVectorInputEvent(rightKey, aRight);
    if (!leftEvt || !rightEvt)
        return -20;

    int32_t queued = 0;
    if (QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(leftEvt)))
        ++queued;
    if (QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(rightEvt)))
        ++queued;

    return queued == 2 ? 6 : -21;
}

static int32_t QueueFeatureInputEvent(const RED4ext::CName& aFeatureName,
                                      const RED4ext::Handle<RED4ext::anim::AnimFeature>& aFeature)
{
    auto evt = CreateFeatureInputEvent(aFeatureName, aFeature);
    if (!evt)
        return -22;

    return QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(evt)) ? 7 : -23;
}

static int32_t QueueFeatureInputEvent(const RED4ext::CName& aFeatureName,
                                      const RED4ext::Vector4& aLeft,
                                      const RED4ext::Vector4& aRight)
{
    auto feature = CreateWeaponUserFeature(aLeft, aRight);
    return QueueFeatureInputEvent(aFeatureName, feature);
}

static void AppendAnimTestLog(const char* aModeName, int32_t aResult, const RED4ext::Vector4& aLeft, const RED4ext::Vector4& aRight)
{
    std::ofstream out(VRDiagPath("vrik_anim_input_test_log.txt"), std::ios::app);
    if (!out.is_open())
        return;

    out << "mode=" << (aModeName ? aModeName : "<null>")
        << " result=" << aResult
        << " left=(" << aLeft.X << ", " << aLeft.Y << ", " << aLeft.Z << ", " << aLeft.W << ")"
        << " right=(" << aRight.X << ", " << aRight.Y << ", " << aRight.Z << ", " << aRight.W << ")"
        << "\n";
}

static bool FillAnimTestPose(int32_t aMode, RED4ext::Vector4& aLeft, RED4ext::Vector4& aRight)
{
    if (aMode == 2)
    {
        EnsureSharedMemory();
        if (!g_pSharedHands)
            return false;

        // Approximate XR-local -> game-local mapping based on previous working hand-space conversion.
        aLeft.X = g_pSharedHands[1];
        aLeft.Y = -g_pSharedHands[3];
        aLeft.Z = g_pSharedHands[2];
        aLeft.W = 1.0f;

        aRight.X = g_pSharedHands[9];
        aRight.Y = -g_pSharedHands[11];
        aRight.Z = g_pSharedHands[10];
        aRight.W = 1.0f;
        return true;
    }

    // Keep all non-VR test modes on an exaggerated pose so visual response is obvious.
    aLeft.X = -0.65f; aLeft.Y = 0.75f; aLeft.Z = 0.35f; aLeft.W = 1.0f;
    aRight.X = 0.65f; aRight.Y = 0.75f; aRight.Z = 0.35f; aRight.W = 1.0f;
    return true;
}

static int32_t ApplyFeatureHandle(RED4ext::ent::AnimationControllerComponent* aController,
                                  const RED4ext::CName& aFeatureName,
                                  const RED4ext::Handle<RED4ext::anim::AnimFeature>& aFeature)
{
    if (!aController)
        return -10;

    auto* cls = aController->GetType();
    auto* func = cls ? cls->GetFunction("ApplyFeature") : nullptr;
    if (!func)
        return -11;

    if (func->params.Size() < 2)
        return -12;

    if (!aFeature)
        return -13;

    RED4ext::StackArgs_t args;
    args.emplace_back(nullptr, const_cast<RED4ext::CName*>(&aFeatureName));
    args.emplace_back(nullptr, const_cast<RED4ext::Handle<RED4ext::anim::AnimFeature>*>(&aFeature));

    const bool ok = RED4ext::ExecuteFunction(aController, func, nullptr, args);
    return ok ? 2 : -14;
}

static int32_t ApplyWeaponUserFeature(RED4ext::ent::AnimationControllerComponent* aController,
                                      const RED4ext::CName& aFeatureName,
                                      const RED4ext::Vector4& aLeft,
                                      const RED4ext::Vector4& aRight)
{
    auto feature = CreateWeaponUserFeature(aLeft, aRight);
    return ApplyFeatureHandle(aController, aFeatureName, feature);
}

void TestWeaponUserFeatureRoute(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    RED4ext::Vector4 left{};
    RED4ext::Vector4 right{};
    FillAnimTestPose(1, left, right);

    const RED4ext::CName featureName("weapon_user");
    int32_t result = -60;
    const char* modeName = "weapon_user_unknown";

    if (route == 0)
    {
        auto* controller = FindPlayerAnimationController();
        result = ApplyWeaponUserFeature(controller, featureName, left, right);
        modeName = "weapon_user_apply";
    }
    else if (route == 1)
    {
        result = QueueFeatureInputEvent(featureName, left, right);
        modeName = "weapon_user_queue";
    }

    AppendAnimTestLog(modeName, result, left, right);
    if (aOut) *aOut = result;
}

void TestIKFeatureRoute(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    const char* featureNameStr = nullptr;
    const char* logName = nullptr;
    if (mode == 1)
    {
        featureNameStr = "playerIK";
        logName = route == 0 ? "playerIK_apply" : "playerIK_queue";
    }
    else if (mode == 2)
    {
        featureNameStr = "interactionIK";
        logName = route == 0 ? "interactionIK_apply" : "interactionIK_queue";
    }
    else
    {
        if (aOut) *aOut = -61;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    auto* transform = playerEntity ? playerEntity->transformComponent : nullptr;
    if (!playerEntity || !transform)
    {
        if (aOut) *aOut = -62;
        return;
    }

    RED4ext::Vector4 point = transform->worldTransform.Position.AsVector4();
    point.X += 0.45f;
    point.Y += 0.35f;
    point.Z += 1.15f;
    point.W = 1.0f;

    RED4ext::Vector4 normal{};
    normal.X = 0.0f;
    normal.Y = 0.0f;
    normal.Z = 1.0f;
    normal.W = 0.0f;

    auto feature = CreateIKFeature(point, normal, 1.0f);
    const RED4ext::CName featureName(featureNameStr);
    int32_t result = -63;
    if (route == 0)
    {
        auto* controller = FindPlayerAnimationController();
        result = ApplyFeatureHandle(controller, featureName, feature);
    }
    else if (route == 1)
    {
        result = QueueFeatureInputEvent(featureName, feature);
    }

    AppendAnimTestLog(logName, result, point, normal);
    if (aOut) *aOut = result;
}

void TestMeleeIKDataFeatureRoute(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    auto* playerEntity = FindPlayerEntity();
    auto* transform = playerEntity ? playerEntity->transformComponent : nullptr;
    if (!playerEntity || !transform)
    {
        if (aOut) *aOut = -64;
        return;
    }

    RED4ext::Vector4 base = transform->worldTransform.Position.AsVector4();
    RED4ext::Vector4 head = base;
    RED4ext::Vector4 chest = base;
    RED4ext::Vector4 offset{};

    head.Z += 1.55f; head.W = 1.0f;
    chest.Z += 1.15f; chest.W = 1.0f;
    offset.X = 0.55f; offset.Y = 0.35f; offset.Z = 0.15f; offset.W = 0.0f;

    auto feature = CreateMeleeIKDataFeature(head, chest, offset);
    const RED4ext::CName featureName("MeleeIKData");

    int32_t result = -65;
    const char* logName = route == 0 ? "MeleeIKData_apply" : "MeleeIKData_queue";
    if (route == 0)
    {
        auto* controller = FindPlayerAnimationController();
        result = ApplyFeatureHandle(controller, featureName, feature);
    }
    else if (route == 1)
    {
        result = QueueFeatureInputEvent(featureName, feature);
    }

    AppendAnimTestLog(logName, result, head, offset);
    if (aOut) *aOut = result;
}

void DumpRootGraphVariables(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("root_graph_variables.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -73;
        return;
    }

    auto* animated = FindPlayerAnimatedComponentByName("root");
    auto* graph = animated ? LoadResourceRef(animated->graph) : nullptr;
    if (!graph)
    {
        if (aOut) *aOut = -74;
        return;
    }

    DumpAnimGraphVariables(out, graph, "");
    out.close();
    if (aOut) *aOut = 1;
}

void SetRootGraphBoolVariableByName(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName name;
    bool value = false;
    RED4ext::GetParameter(aFrame, &name);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    if (aOut) *aOut = SetRootGraphBoolVariable(name, value);
}

void SetRootGraphFloatVariableByName(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName name;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &name);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    if (aOut) *aOut = SetRootGraphFloatVariable(name, value);
}

void SetRootGraphVectorVariableByName(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName name;
    RED4ext::Vector4 value{};
    RED4ext::GetParameter(aFrame, &name);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    if (aOut) *aOut = SetRootGraphVectorVariable(name, value);
}

void TestRootGraphFloatPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootGraphFloatPreset(mode, name))
    {
        if (aOut) *aOut = -75;
        return;
    }

    const int32_t result = SetRootGraphFloatVariable(name, value);
    AppendRootGraphVariableLog("float_preset", name, std::to_string(value).c_str(), result);
    if (aOut) *aOut = result;
}

void TestRootGraphVectorPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    RED4ext::Vector4 value{};
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootGraphVectorPreset(mode, name))
    {
        if (aOut) *aOut = -76;
        return;
    }

    const int32_t result = SetRootGraphVectorVariable(name, value);
    std::string valueText = std::to_string(value.X) + "," + std::to_string(value.Y) + "," + std::to_string(value.Z) + "," + std::to_string(value.W);
    AppendRootGraphVariableLog("vector_preset", name, valueText.c_str(), result);
    if (aOut) *aOut = result;
}

void SetRootGraphFloatPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    g_rootGraphFloatPersistentPreset = mode;
    g_rootGraphFloatPersistentValue = value;
    g_rootGraphFloatPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void SetRootGraphVectorPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    RED4ext::Vector4 value{};
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    g_rootGraphVectorPersistentPreset = mode;
    g_rootGraphVectorPersistentValue = value;
    g_rootGraphVectorPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void GetRootGraphPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut)
    {
        *aOut = g_rootGraphVectorPersistentPreset != 0 ? g_rootGraphVectorPersistentLastResult : g_rootGraphFloatPersistentLastResult;
    }
}

void SetVRIKAnimInputTestMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    g_animInputTestMode = mode;
}

// Plain-C++ arm helper (resolves the player's live track buffers). Shared by the
// script-callable ArmVRAnimPosePlayer and the in-VR overlay auto-activation below.
// Returns bitmask: 1=bufA set, 2=bufB set, -1=player not ready.
static int VRIK_DoArmPlayer();

void UpdateVRIKAnimInputs(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    // --- In-VR overlay activation -------------------------------------------
    // The in-headset menu (imgui_overlay) writes a tracking-request code into
    // shared-memory slot [32] (0 = off, 2 = position+rotation). CET calls us
    // every frame on the game thread, so installing the hooks / arming the
    // player here is exactly as safe as the manual "Start VR Tracking" button.
    // Edge-triggered on g_VRBind so the CET button still works independently.
    EnsureSharedMemory();

    if (g_pSharedHands) {
        static bool s_vrHooksInstalled = false;
        static bool s_vrArmed = false;
        static int  s_lastReq = 0;
        static int  s_rearmCounter = 0;
        int req = static_cast<int>(g_pSharedHands[32]);
        if (req > 0) {
            if (!s_vrHooksInstalled) {
                // Only the pose-apply hook is needed. The old ComponentFunc21 hook
                // (InstallVRIKMinHook) is a dead end -- g_PlayerAnimComponent is never
                // assigned so it does nothing, yet it trampolines a super-hot per-component
                // Update function and tanked FPS (70+ -> 10-15).
                InstallAnimPoseHook();
                s_vrHooksInstalled = true;
            }
            // Re-arm: initial bootstrap, then once every ~60 frames (~1s @ 60fps) to refresh the
            // cached track-buffer / bone-index pointers when the player puppet changes underneath
            // us (save load, area transition, vehicle in/out, scripted scene). Without this the
            // pointers go stale and tracking stops working until the user toggles it off+on.
            ++s_rearmCounter;
            if (!s_vrArmed || s_rearmCounter >= 60) {
                if (VRIK_DoArmPlayer() > 0) s_vrArmed = true;
                s_rearmCounter = 0;
            }
            if (s_lastReq <= 0) g_VRBind = req;   // off -> on edge
            if (g_VRNeutralizeAnimGraph != 0) {
                ForceVRNeutralAnimGraphInputs();
            }
            // Keep the diag bone snapshot fresh while tracking, so the overlay's
            // "Log VR Diag" works without the CET window's capture toggle.
            g_VRDiagCapture = 1;
        } else {
            if (s_lastReq > 0) { g_VRBind = 0; g_VRDiagCapture = 0; } // on -> off edge
            s_vrArmed = false;                     // re-arm on next activation
            s_rearmCounter = 0;
        }
        s_lastReq = req;

        // Pull IK calibration the overlay published + service one-shot diag requests.
        PollVRCalibFromShared();
    }
    // ------------------------------------------------------------------------

    if (g_rootGraphFloatPersistentPreset != 0)
    {
        RED4ext::CName name;
        if (ResolveRootGraphFloatPreset(g_rootGraphFloatPersistentPreset, name))
        {
            g_rootGraphFloatPersistentLastResult = SetRootGraphFloatVariable(name, g_rootGraphFloatPersistentValue);
        }
        else
        {
            g_rootGraphFloatPersistentLastResult = -77;
        }
    }

    if (g_rootGraphVectorPersistentPreset != 0)
    {
        RED4ext::CName name;
        if (ResolveRootGraphVectorPreset(g_rootGraphVectorPersistentPreset, name))
        {
            g_rootGraphVectorPersistentLastResult = SetRootGraphVectorVariable(name, g_rootGraphVectorPersistentValue);
        }
        else
        {
            g_rootGraphVectorPersistentLastResult = -78;
        }
    }

    if (g_rootMetaRigTrackPersistentPreset != 0)
    {
        RED4ext::CName name;
        if (ResolveRootMetaRigTrackPreset(g_rootMetaRigTrackPersistentPreset, name))
        {
            g_rootMetaRigTrackPersistentLastResult = SetRootMetaRigTrackValue(name, g_rootMetaRigTrackPersistentValue);
        }
        else
        {
            g_rootMetaRigTrackPersistentLastResult = -85;
        }
    }

    if (g_rootLiveTrackPersistentPreset != 0)
    {
        RED4ext::CName name;
        if (ResolveRootMetaRigTrackPreset(g_rootLiveTrackPersistentPreset, name))
        {
            g_rootLiveTrackPersistentLastResult = SetRootLiveTrackValue(name, g_rootLiveTrackPersistentValue, g_rootLiveTrackPersistentArrayMode);
        }
        else
        {
            g_rootLiveTrackPersistentLastResult = -93;
        }
    }

    if (g_animParamPersistentPreset != 0)
    {
        RED4ext::CName inputName;
        if (ResolveAnimFloatPreset(g_animParamPersistentPreset, inputName))
        {
            g_animParamPersistentLastResult = SetPlayerAnimatedParameterValue(inputName, g_animParamPersistentValue);
        }
        else
        {
            g_animParamPersistentLastResult = -52;
        }
    }

    if (g_animInputTestMode == 0)
    {
        if (aOut)
        {
            if (g_rootGraphVectorPersistentPreset != 0)
                *aOut = g_rootGraphVectorPersistentLastResult;
            else if (g_rootGraphFloatPersistentPreset != 0)
                *aOut = g_rootGraphFloatPersistentLastResult;
            else if (g_rootMetaRigTrackPersistentPreset != 0)
                *aOut = g_rootMetaRigTrackPersistentLastResult;
            else if (g_rootLiveTrackPersistentPreset != 0)
                *aOut = g_rootLiveTrackPersistentLastResult;
            else if (g_animParamPersistentPreset != 0)
                *aOut = g_animParamPersistentLastResult;
            else
                *aOut = 0;
        }
        return;
    }

    RED4ext::Vector4 left{};
    RED4ext::Vector4 right{};
    if (!FillAnimTestPose(g_animInputTestMode, left, right))
    {
        if (aOut) *aOut = -3;
        return;
    }

    if (g_animInputTestMode >= 6)
    {
        int32_t result = -99;
        const char* modeName = "queue_unknown";

        if (g_animInputTestMode == 6)
        {
            modeName = "queue_vector";
            result = QueueVectorInputEvents(left, right);
        }
        else if (g_animInputTestMode == 7)
        {
            modeName = "queue_feature_WeaponUser";
            result = QueueFeatureInputEvent(RED4ext::CName("WeaponUser"), left, right);
        }
        else if (g_animInputTestMode == 8)
        {
            modeName = "queue_feature_AnimFeature_WeaponUser";
            result = QueueFeatureInputEvent(RED4ext::CName("AnimFeature_WeaponUser"), left, right);
        }
        else if (g_animInputTestMode == 9)
        {
            modeName = "queue_feature_animAnimFeature_WeaponUser";
            result = QueueFeatureInputEvent(RED4ext::CName("animAnimFeature_WeaponUser"), left, right);
        }

        AppendAnimTestLog(modeName, result, left, right);
        if (aOut) *aOut = result;
        return;
    }

    auto* controller = FindPlayerAnimationController();
    if (!controller)
    {
        if (aOut) *aOut = -1;
        return;
    }

    if (g_animInputTestMode >= 3)
    {
        RED4ext::CName featureName("WeaponUser");
        if (g_animInputTestMode == 4)
            featureName = RED4ext::CName("AnimFeature_WeaponUser");
        else if (g_animInputTestMode == 5)
            featureName = RED4ext::CName("animAnimFeature_WeaponUser");

        const int32_t result = ApplyWeaponUserFeature(controller, featureName, left, right);
        AppendAnimTestLog("apply_feature", result, left, right);
        if (aOut) *aOut = result;
        return;
    }

    auto* cls = controller->GetType();
    auto* func = cls ? cls->GetFunction("SetInputVector") : nullptr;
    if (!func)
    {
        if (aOut) *aOut = -2;
        return;
    }

    RED4ext::CName leftKey("ikLeftHandLocalPosition");
    RED4ext::CName rightKey("ikRightHandLocalPosition");

    RED4ext::StackArgs_t leftArgs;
    leftArgs.emplace_back(nullptr, &leftKey);
    leftArgs.emplace_back(nullptr, &left);
    RED4ext::ExecuteFunction(controller, func, nullptr, leftArgs);

    RED4ext::StackArgs_t rightArgs;
    rightArgs.emplace_back(nullptr, &rightKey);
    rightArgs.emplace_back(nullptr, &right);
    RED4ext::ExecuteFunction(controller, func, nullptr, rightArgs);

    AppendAnimTestLog("set_input_vector", 1, left, right);
    if (aOut) *aOut = 1;
}

void DumpAnimControllerListeners(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("anim_controller_listeners.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* controllerCls = rtti ? rtti->GetClass("entAnimationControllerComponent") : nullptr;
    if (!controllerCls)
    {
        if (aOut) *aOut = -2;
        return;
    }

    const char* eventNames[] = {
        "redEvent",
        "entAnimInputSetter",
        "entAnimInputSetterVector",
        "entAnimInputSetterAnimFeature",
        "entIKTargetAddEvent"
    };

    out << "CLASS entAnimationControllerComponent\n";
    out << "listenerCount=" << controllerCls->listeners.Size() << "\n\n";

    out << "Known event type ids:\n";
    for (const char* eventName : eventNames)
    {
        auto* eventCls = rtti->GetClass(eventName);
        out << "  " << eventName << " -> eventTypeId="
            << (eventCls ? eventCls->eventTypeId : -1)
            << "\n";
    }

    out << "\nListeners:\n";
    for (uint32_t i = 0; i < controllerCls->listeners.Size(); ++i)
    {
        const auto& listener = controllerCls->listeners[i];
        out << "  [" << i << "] callback=" << listener.callbackName.ToString()
            << " eventTypeId=" << listener.eventTypeId
            << " scripted=" << (listener.isScripted ? 1 : 0)
            << "\n";
    }

    out.close();
    if (aOut) *aOut = static_cast<int32_t>(controllerCls->listeners.Size());
}

void DumpAnimControllerFunctionDetails(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("anim_controller_function_details.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* controllerCls = rtti ? rtti->GetClass("entAnimationControllerComponent") : nullptr;
    auto* entityCls = rtti ? rtti->GetClass("entEntity") : nullptr;

    const char* controllerFuncs[] = {
        "ApplyFeature",
        "PushEvent",
        "SetInputVector",
        "SetInputFloat",
        "SetInputBool",
        "SetInputQuaternion",
        "OnSetInputVectorEvent"
    };

    const char* entityFuncs[] = {
        "QueueEvent",
        "QueueEventForNodeID",
        "QueueEventForEntityID"
    };

    out << "CLASS entAnimationControllerComponent\n\n";
    if (controllerCls)
    {
        for (const char* name : controllerFuncs)
            DumpFunctionDetails(out, controllerCls->GetFunction(name));
    }

    out << "CLASS entEntity\n\n";
    if (entityCls)
    {
        for (const char* name : entityFuncs)
            DumpFunctionDetails(out, entityCls->GetFunction(name));
    }

    out.close();
    if (aOut) *aOut = 1;
}

void DumpInterestingAnimClassProperties(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("anim_interesting_class_properties.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    const char* classNames[] = {
        "entIBinding",
        "entAnimationControlBinding",
        "entStaticPositionProvider",
        "entStaticOrientationProvider",
        "entIKTargetAddEvent",
        "entIKTargetRemoveEvent",
        "entAnimInputSetterVector",
        "entAnimInputSetterAnimFeature",
        "entAnimatedComponent",
        "entAnimationControllerComponent"
    };

    for (const char* className : classNames)
        DumpClassProperties(out, className);

    out.close();
    if (aOut) *aOut = 1;
}

void DumpAnimationSystemCandidates(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("animation_system_candidates.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    if (!playerEntity)
    {
        if (aOut) *aOut = -2;
        return;
    }

    DumpEntityAnimationInfo(out, playerEntity, "player_entity");

    out.close();
    if (aOut) *aOut = 1;
}

void DumpPlayerAnimatedObjectRuntime(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("player_animated_object_runtime.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -79;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    auto* engine = RED4ext::CGameEngine::Get();
    auto* framework = engine ? engine->framework : nullptr;
    auto* animationSystem = playerEntity
        ? FindWorldAnimationSystemFromScene(playerEntity->runtimeScene, framework ? framework->unk18 : 0, &out)
        : nullptr;
    if (!playerEntity || !animationSystem)
    {
        if (aOut) *aOut = -80;
        return;
    }

    int32_t matches = 0;
    for (uint32_t bucketIndex = 0; bucketIndex < RED4ext::world::AnimationSystem::BucketCount; ++bucketIndex)
    {
        auto& bucket = animationSystem->entitityBuckets[bucketIndex];
        const uint32_t entryCount = bucket.entities.Size();
        for (uint32_t entryIndex = 0; entryIndex < entryCount; ++entryIndex)
        {
            auto& entityHandle = bucket.entities[entryIndex];
            auto* entity = reinterpret_cast<RED4ext::ent::Entity*>(entityHandle.instance);
            if (entity != playerEntity)
                continue;

            auto* animatedComponent = bucket.animatedComponents[entryIndex];
            auto* animatedObject = bucket.animatedObjects[entryIndex];

            out << "bucket=" << bucketIndex << " entry=" << entryIndex
                << " animatedObject=0x" << std::hex << reinterpret_cast<uintptr_t>(animatedObject)
                << " animatedComponent=0x" << reinterpret_cast<uintptr_t>(animatedComponent)
                << std::dec;
            if (animatedComponent)
                out << " componentName=" << animatedComponent->name.ToString();
            out << "\n";

            if (animatedObject)
            {
                out << "  metaRigID=" << animatedObject->metaRigID
                    << " metaRig=0x" << std::hex << reinterpret_cast<uintptr_t>(animatedObject->metaRig)
                    << " metaRigInfo=0x" << reinterpret_cast<uintptr_t>(animatedObject->metaRigInfo)
                    << std::dec
                    << " distance=" << animatedObject->distanceFromCamera
                    << " cameraLevel=" << animatedObject->cameraDistanceLevel
                    << " lastLevel=" << animatedObject->lastDistanceLevel
                    << "\n";
                DumpMetaRigTracks(out, animatedObject->metaRig, "  ");
            }

            if (entryIndex < bucket.componentBindings.Size())
            {
                const auto& bindingSet = bucket.componentBindings[entryIndex];
                out << "  componentBindings=" << bindingSet.bindings.Size() << "\n";
                for (uint32_t i = 0; i < bindingSet.bindings.Size(); ++i)
                {
                    const auto& binding = bindingSet.bindings[i];
                    out << "    [" << i << "] placed=0x" << std::hex << reinterpret_cast<uintptr_t>(binding.placedComponent.instance)
                        << " animComp=0x" << reinterpret_cast<uintptr_t>(binding.animComponent)
                        << " attachment=0x" << reinterpret_cast<uintptr_t>(binding.transformAttachment.instance)
                        << std::dec;
                    if (binding.animComponent)
                        out << " animCompName=" << binding.animComponent->name.ToString();
                    out << "\n";
                }
            }

            out << "\n";
            ++matches;
        }
    }

    out.close();
    if (aOut) *aOut = matches;
}

void DumpRootMetaRigTracks(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("root_metarig_tracks.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -82;
        return;
    }

    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    if (!animatedObject || !animatedObject->metaRig)
    {
        if (aOut) *aOut = -83;
        return;
    }

    DumpMetaRigTracks(out, animatedObject->metaRig, "");
    out.close();
    if (aOut) *aOut = 1;
}

void DumpRootAnimatedObjectFloatArrayCandidates(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    std::ofstream out(VRDiagPath("root_animated_object_float_candidates.txt"), std::ios::trunc);
    if (!out.is_open())
    {
        if (aOut) *aOut = -87;
        return;
    }

    auto* animatedObject = FindPlayerAnimatedObjectByComponentName("root");
    auto* metaRig = animatedObject ? animatedObject->metaRig : nullptr;
    if (!animatedObject || !metaRig)
    {
        if (aOut) *aOut = -88;
        return;
    }

    const uint32_t expectedTrackCount = (metaRig->trackNames.Size() < metaRig->referenceTracks.Size())
        ? metaRig->trackNames.Size()
        : metaRig->referenceTracks.Size();

    out << "animatedObject=0x" << std::hex << reinterpret_cast<uintptr_t>(animatedObject)
        << " metaRig=0x" << reinterpret_cast<uintptr_t>(metaRig)
        << std::dec << " expectedTrackCount=" << expectedTrackCount << "\n\n";

    int32_t candidates = 0;
    uint8_t* base = reinterpret_cast<uint8_t*>(animatedObject);

    auto dumpCandidate = [&](const char* label, size_t ownerOff, uint8_t* ownerBase, size_t off)
    {
        uint64_t entries = SafeReadQword(ownerBase, off + 0x0);
        uint32_t capacity = SafeReadU32(ownerBase, off + 0x8);
        uint32_t size = SafeReadU32(ownerBase, off + 0xC);
        if (std::strcmp(ClassifyQword(entries), "HEAP") != 0)
            return;
        if (size == 0 || size > 4096 || capacity < size || capacity > 8192)
            return;
        if (size != expectedTrackCount && size != expectedTrackCount * 2 && size != expectedTrackCount * 4)
            return;

        out << label << " ownerOff=0x" << std::hex << ownerOff << " arrOff=0x" << off
            << " entries=0x" << entries << std::dec
            << " size=" << size << " capacity=" << capacity << "\n";

        uint8_t* arrBase = reinterpret_cast<uint8_t*>(entries);
        const uint32_t preview = size < 12 ? size : 12;
        for (uint32_t i = 0; i < preview; ++i)
        {
            out << "  [" << i << "]=" << SafeReadFloat(arrBase, i * sizeof(float)) << "\n";
        }
        out << "\n";
        ++candidates;
    };

    for (size_t off = 0; off + 0x10 <= 0x180; off += 4)
        dumpCandidate("direct", off, base, off);

    for (size_t ptrOff = 0; ptrOff + 8 <= 0x180; ptrOff += 8)
    {
        uint64_t p = SafeReadQword(base, ptrOff);
        if (std::strcmp(ClassifyQword(p), "HEAP") != 0)
            continue;

        uint8_t* nested = reinterpret_cast<uint8_t*>(p);
        for (size_t off = 0; off + 0x10 <= 0x200; off += 4)
            dumpCandidate("nested", ptrOff, nested, off);
    }

    out.close();
    if (aOut) *aOut = candidates;
}

void TestRootMetaRigTrackPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootMetaRigTrackPreset(mode, name))
    {
        if (aOut) *aOut = -84;
        return;
    }

    const int32_t result = SetRootMetaRigTrackValue(name, value);
    AppendRootMetaRigTrackLog("oneshot", name, value, result);
    if (aOut) *aOut = result;
}

void SetRootMetaRigTrackPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    g_rootMetaRigTrackPersistentPreset = mode;
    g_rootMetaRigTrackPersistentValue = value;
    g_rootMetaRigTrackPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void GetRootMetaRigTrackPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_rootMetaRigTrackPersistentLastResult;
}

void TestRootLiveTrackPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    int32_t arrayMode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    RED4ext::GetParameter(aFrame, &arrayMode);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootMetaRigTrackPreset(mode, name))
    {
        if (aOut) *aOut = -84;
        return;
    }

    const int32_t result = SetRootLiveTrackValue(name, value, arrayMode);
    AppendRootMetaRigTrackLog("live_oneshot", name, value, result);
    if (aOut) *aOut = result;
}

void SetRootLiveTrackPresetPersistent(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    int32_t arrayMode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    RED4ext::GetParameter(aFrame, &arrayMode);
    aFrame->code++;

    g_rootLiveTrackPersistentPreset = mode;
    g_rootLiveTrackPersistentValue = value;
    g_rootLiveTrackPersistentArrayMode = arrayMode;
    g_rootLiveTrackPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void GetRootLiveTrackPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_rootLiveTrackPersistentLastResult;
}

void ReadRootLiveTrackPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    int32_t arrayMode = 1;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &arrayMode);
    aFrame->code++;

    RED4ext::CName name;
    if (!ResolveRootMetaRigTrackPreset(mode, name))
    {
        if (aOut) *aOut = -84;
        return;
    }

    if (aOut) *aOut = ReadRootLiveTrackValue(name, arrayMode);
}

void RunIKTargetAddTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;

    const char* leftPart = nullptr;
    const char* rightPart = nullptr;
    const char* label = nullptr;

    switch (mode)
    {
    case 1:
        label = "LeftHand_RightHand";
        leftPart = "LeftHand";
        rightPart = "RightHand";
        break;
    case 2:
        label = "l_hand_r_hand";
        leftPart = "l_hand";
        rightPart = "r_hand";
        break;
    case 3:
        label = "IK_Pad_Shoulders";
        leftPart = "IK_Pad_LeftShoulder";
        rightPart = "IK_Pad_RightShoulder";
        break;
    case 4:
        label = "LeftShoulder_RightShoulder";
        leftPart = "LeftShoulder";
        rightPart = "RightShoulder";
        break;
    default:
        if (aOut) *aOut = -30;
        return;
    }

    auto* playerEntity = FindPlayerEntity();
    auto* transform = playerEntity ? playerEntity->transformComponent : nullptr;
    if (!playerEntity || !transform)
    {
        if (aOut) *aOut = -31;
        return;
    }

    const RED4ext::Vector4 playerPos = transform->worldTransform.Position.AsVector4();
    RED4ext::Vector4 leftPos = playerPos;
    RED4ext::Vector4 rightPos = playerPos;
    leftPos.X -= 0.35f; leftPos.Y += 0.30f; leftPos.Z += 1.15f; leftPos.W = 1.0f;
    rightPos.X += 0.35f; rightPos.Y += 0.30f; rightPos.Z += 1.15f; rightPos.W = 1.0f;

    auto leftProvider = CreateStaticPositionProvider(leftPos);
    auto rightProvider = CreateStaticPositionProvider(rightPos);
    auto orientationProvider = CreateStaticOrientationProvider();

    std::ofstream log(VRDiagPath("ik_target_add_test_log.txt"), std::ios::app);
    log << "mode=" << mode << " label=" << label << "\n";
    log << "playerPos=(" << playerPos.X << ", " << playerPos.Y << ", " << playerPos.Z << ", " << playerPos.W << ")\n";
    log << "leftPos=(" << leftPos.X << ", " << leftPos.Y << ", " << leftPos.Z << ", " << leftPos.W << ") rightPos=("
        << rightPos.X << ", " << rightPos.Y << ", " << rightPos.Z << ", " << rightPos.W << ")\n";

    if (!leftProvider || !rightProvider)
    {
        log << "providerCreation=failed\n";
        if (aOut) *aOut = -32;
        return;
    }

    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("entIKTargetAddEvent") : nullptr;
    if (!cls)
    {
        if (aOut) *aOut = -33;
        return;
    }

    auto* leftInstance = cls->CreateInstance(true);
    auto* rightInstance = cls->CreateInstance(true);
    if (!leftInstance || !rightInstance)
    {
        if (aOut) *aOut = -34;
        return;
    }

    cls->InitializeProperties(leftInstance);
    cls->InitializeProperties(rightInstance);

    auto* leftEvt = reinterpret_cast<RED4ext::ent::IKTargetAddEvent*>(leftInstance);
    auto* rightEvt = reinterpret_cast<RED4ext::ent::IKTargetAddEvent*>(rightInstance);

    leftEvt->targetPositionProvider = leftProvider;
    leftEvt->bodyPart = RED4ext::CName(leftPart);
    leftEvt->orientationProvider = orientationProvider;
    leftEvt->request.weightPosition = 1.0f;
    leftEvt->request.weightOrientation = 0.0f;
    leftEvt->request.transitionIn = 0.0f;
    leftEvt->request.transitionOut = 0.0f;
    leftEvt->request.priority = 100;

    rightEvt->targetPositionProvider = rightProvider;
    rightEvt->bodyPart = RED4ext::CName(rightPart);
    rightEvt->orientationProvider = orientationProvider;
    rightEvt->request.weightPosition = 1.0f;
    rightEvt->request.weightOrientation = 0.0f;
    rightEvt->request.transitionIn = 0.0f;
    rightEvt->request.transitionOut = 0.0f;
    rightEvt->request.priority = 100;

    const bool leftQueued = QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(RED4ext::Handle<RED4ext::ent::IKTargetAddEvent>(leftEvt)));
    const bool rightQueued = QueuePlayerEvent(static_cast<RED4ext::Handle<RED4ext::red::Event>>(RED4ext::Handle<RED4ext::ent::IKTargetAddEvent>(rightEvt)));

    log << "leftQueued=" << (leftQueued ? 1 : 0) << " rightQueued=" << (rightQueued ? 1 : 0) << "\n";
    log << "leftOutRef id=" << leftEvt->outIKTargetRef.id << " part=" << leftEvt->outIKTargetRef.part.ToString() << "\n";
    log << "rightOutRef id=" << rightEvt->outIKTargetRef.id << " part=" << rightEvt->outIKTargetRef.part.ToString() << "\n";
    AppendPlayerControllerIKState(log);
    log << "--------------------------------------------------\n";

    if (aOut) *aOut = (leftQueued ? 1 : 0) + (rightQueued ? 1 : 0);
}

void TestAnimFloatInput(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName inputName;
    float value = 0.0f;
    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &inputName);
    RED4ext::GetParameter(aFrame, &value);
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    int32_t result = -45;
    const char* routeName = "unknown";
    if (route == 0)
    {
        routeName = "direct";
        result = SetFloatInputDirect(inputName, value);
    }
    else if (route == 1)
    {
        routeName = "queue";
        result = QueueFloatInputEvent(inputName, value);
    }

    AppendAnimFloatTestLog(routeName, inputName, value, result);
    if (aOut) *aOut = result;
}

void TestAnimFloatPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    int32_t route = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    RED4ext::GetParameter(aFrame, &route);
    aFrame->code++;

    RED4ext::CName inputName;
    if (!ResolveAnimFloatPreset(mode, inputName))
    {
        if (aOut) *aOut = -46;
        return;
    }

    int32_t result = -47;
    const char* routeName = "unknown_preset";
    if (route == 0)
    {
        routeName = "direct_preset";
        result = SetFloatInputDirect(inputName, value);
    }
    else if (route == 1)
    {
        routeName = "queue_preset";
        result = QueueFloatInputEvent(inputName, value);
    }

    AppendAnimFloatTestLog(routeName, inputName, value, result);
    if (aOut) *aOut = result;
}

void SetPlayerAnimParameter(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    RED4ext::CName inputName;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &inputName);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    const int32_t result = SetPlayerAnimatedParameterValue(inputName, value);
    AppendDirectAnimParamLog("oneshot", inputName, value, result);
    if (aOut) *aOut = result;
}

void SetPlayerAnimParameterPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    RED4ext::CName inputName;
    if (!ResolveAnimFloatPreset(mode, inputName))
    {
        if (aOut) *aOut = -51;
        return;
    }

    const int32_t result = SetPlayerAnimatedParameterValue(inputName, value);
    AppendDirectAnimParamLog("oneshot_preset", inputName, value, result);
    if (aOut) *aOut = result;
}

void SetPlayerAnimParameterPersistentPreset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);

    int32_t mode = 0;
    float value = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &value);
    aFrame->code++;

    g_animParamPersistentPreset = mode;
    g_animParamPersistentValue = value;
    g_animParamPersistentLastResult = 0;
    if (aOut) *aOut = 1;
}

void GetPlayerAnimParameterPersistentLastResult(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_animParamPersistentLastResult;
}

void RestoreVRFppArms(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle) { if (aOut) *aOut = -1; return; }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity) { if (aOut) *aOut = -2; return; }

    const RED4ext::CName placedComponentName("entIPlacedComponent");
    const RED4ext::CName meshComponentName("entMeshComponent");
    const RED4ext::CName skinnedMeshComponentName("entSkinnedMeshComponent");
    const RED4ext::CName garmentSkinnedMeshComponentName("entGarmentSkinnedMeshComponent");

    int32_t restored = 0;
    for (auto& componentHandle : playerEntity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;
        const char* componentName = component->name.ToString();
        if (!IsLikelyFppArmComponent(componentName)) continue;

        RED4ext::CClass* type = component->GetType();
        if (!type || !ClassIsA(type, placedComponentName)) continue;
        auto* placed = reinterpret_cast<RED4ext::ent::IPlacedComponent*>(component);

        component->isEnabled = true;
        if (ClassIsA(type, skinnedMeshComponentName) || ClassIsA(type, garmentSkinnedMeshComponentName)) {
            auto* skinned = reinterpret_cast<RED4ext::ent::SkinnedMeshComponent*>(component);
            skinned->chunkMask = 0xFFFFFFFFFFFFFFFFull;
        }
        if (ClassIsA(type, meshComponentName)) {
            auto* mesh = reinterpret_cast<RED4ext::ent::MeshComponent*>(component);
            mesh->chunkMask = 0xFFFFFFFFFFFFFFFFull;
            mesh->visualScale.X = 1.0f;
            mesh->visualScale.Y = 1.0f;
            mesh->visualScale.Z = 1.0f;
        }
        placed->localTransform.Position.x.Bits = 0;
        placed->localTransform.Position.y.Bits = 0;
        placed->localTransform.Position.z.Bits = 0;
        ++restored;
    }

    // Reset debug state too, so no later call re-hides things unexpectedly.
    g_chunkDebugEnabled = false;
    g_chunkDebugWasEnabled = false;
    if (aOut) *aOut = restored;
}

void ForceHideVRFppArms(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    // VRFPP hide path disabled in this build.
    // For backward compatibility, calling this function now restores the arms.
    RestoreVRFppArms(aContext, aFrame, aOut, a4);
}



static const char* ClassifyQword(uint64_t v) {
    if (v >= 0x0000010000000000ull && v < 0x0000700000000000ull) return "HEAP";
    if (v >= 0x00007F0000000000ull && v < 0x00008000000000ull)   return "CODE/VTBL";
    return "";
}

static uint64_t SafeReadQword(uint8_t* base, size_t off) {
    __try { return *reinterpret_cast<uint64_t*>(base + off); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
}

static float SafeReadFloat(uint8_t* base, size_t off) {
    __try { return *reinterpret_cast<float*>(base + off); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return 0.0f; }
}

static uint32_t SafeReadU32(uint8_t* base, size_t off) {
    __try { return *reinterpret_cast<uint32_t*>(base + off); }
    __except(EXCEPTION_EXECUTE_HANDLER) { return 0; }
}

static RED4ext::CClass* SafeGetObjectType(void* aPtr)
{
    __try
    {
        auto* obj = reinterpret_cast<RED4ext::ISerializable*>(aPtr);
        return obj ? obj->GetType() : nullptr;
    }
    __except (EXCEPTION_EXECUTE_HANDLER)
    {
        return nullptr;
    }
}

static RED4ext::world::AnimationSystem* ScanForAnimationSystemInBlock(uint8_t* aBase, size_t aSize, std::ofstream* aOut)
{
    if (!aBase)
        return nullptr;

    for (size_t off = 0; off + 8 <= aSize; off += 8)
    {
        uint64_t p = SafeReadQword(aBase, off);
        if (std::strcmp(ClassifyQword(p), "HEAP") != 0)
            continue;

        auto* type = SafeGetObjectType(reinterpret_cast<void*>(p));
        const char* typeName = type ? type->name.ToString() : nullptr;
        if (typeName && aOut && ContainsInsensitive(typeName, "animationsystem"))
        {
            *aOut << "direct off=0x" << std::hex << off << " ptr=0x" << p << std::dec << " type=" << typeName << "\n";
        }

        if (type && type->name == "worldAnimationSystem")
            return reinterpret_cast<RED4ext::world::AnimationSystem*>(p);

        if (type && type->name == "worldAnimationSystemScriptInterface")
        {
            auto* iface = reinterpret_cast<RED4ext::world::AnimationSystemScriptInterface*>(p);
            if (iface->animationSystem)
                return iface->animationSystem;
        }

        uint8_t* nested = reinterpret_cast<uint8_t*>(p);
        for (size_t inner = 0; inner + 8 <= 0x200; inner += 8)
        {
            uint64_t q = SafeReadQword(nested, inner);
            if (std::strcmp(ClassifyQword(q), "HEAP") != 0)
                continue;

            auto* innerType = SafeGetObjectType(reinterpret_cast<void*>(q));
            const char* innerTypeName = innerType ? innerType->name.ToString() : nullptr;
            if (innerTypeName && aOut && ContainsInsensitive(innerTypeName, "animationsystem"))
            {
                *aOut << "nested parentOff=0x" << std::hex << off << " innerOff=0x" << inner << " ptr=0x" << q << std::dec
                    << " type=" << innerTypeName << "\n";
            }

            if (innerType && innerType->name == "worldAnimationSystem")
                return reinterpret_cast<RED4ext::world::AnimationSystem*>(q);

            if (innerType && innerType->name == "worldAnimationSystemScriptInterface")
            {
                auto* iface = reinterpret_cast<RED4ext::world::AnimationSystemScriptInterface*>(q);
                if (iface->animationSystem)
                    return iface->animationSystem;
            }
        }
    }

    return nullptr;
}

void DumpAnimMemory(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle) { if (aOut) *aOut = -1; return; }
    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity) { if (aOut) *aOut = -2; return; }

    std::ofstream out(VRDiagPath("anim_memory_dump.txt"), std::ios::trunc);
    int dumped = 0;

    for (auto& componentHandle : playerEntity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;
        RED4ext::CClass* type = component->GetType();
        if (!type) continue;
        
        if (type->name == "entAnimatedComponent" && std::string(component->name.ToString()) == "root") {
            const char* nm = component->name.ToString();
            out << "==================================================\n";
            out << "COMPONENT name=" << nm << " ptr=0x" << std::hex << (uintptr_t)component << " type=" << type->name.ToString() << "\n";
            out << "==================================================\n";
            
            uint8_t* base = reinterpret_cast<uint8_t*>(component);
            
            // Level 1: Find Heap Pointers
            for (size_t off1 = 0x130; off1 < 0x2B0; off1 += 8) {
                uint64_t ptr1 = SafeReadQword(base, off1);
                if (std::string(ClassifyQword(ptr1)) == "HEAP") {
                    
                    // Scan inside ptr1
                    uint8_t* b1 = reinterpret_cast<uint8_t*>(ptr1);
                    for (size_t off2 = 0; off2 < 0x400; off2 += 8) {
                        uint64_t ptr2 = SafeReadQword(b1, off2);
                        if (std::string(ClassifyQword(ptr2)) == "HEAP") {
                            
                            // Scan inside ptr2 for QsTransform array
                            uint8_t* b2 = reinterpret_cast<uint8_t*>(ptr2);
                            int score = 0;
                            for (size_t i = 0; i < 10; ++i) {
                                float w = SafeReadFloat(b2, (i * 32) + 12);
                                if (w >= 0.99f && w <= 1.01f) {
                                    score++;
                                }
                            }
                            
                            if (score >= 3) {
                                out << "!!! FOUND BONE ARRAY CANDIDATE !!!\n";
                                out << "Component Base + 0x" << std::hex << off1 << " -> + 0x" << off2 << " -> ARRAY!\n";
                                out << "Score: " << std::dec << score << "/10 bones matched W=1.0\n\n";
                            }
                        }
                    }
                    
                    // Also check if ptr1 ITSELF is the array
                    int score = 0;
                    for (size_t i = 0; i < 10; ++i) {
                        float w = SafeReadFloat(b1, (i * 32) + 12);
                        if (w >= 0.99f && w <= 1.01f) {
                            score++;
                        }
                    }
                    if (score >= 3) {
                        out << "!!! FOUND BONE ARRAY CANDIDATE (Direct) !!!\n";
                        out << "Component Base + 0x" << std::hex << off1 << " -> ARRAY!\n";
                        out << "Score: " << std::dec << score << "/10 bones matched W=1.0\n\n";
                    }
                }
            }
            ++dumped;
        }
    }

    out.close();
    if (aOut) *aOut = dumped;
}

void SetVRBoneDebugIndex(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    g_CalibrationBoneIndex = index;
}

// Wires the post-eval bone-write hook to a specific player animated component.
// Until this is set, g_PlayerAnimComponent stays null and the hook's filter
// (rcx == g_PlayerAnimComponent) never matches, so the bone write never runs.
// mode: 0=disable, 1=root, 2=deformations, 3=shadow. Returns 1 if set, else 0.
void ArmVRBoneHook(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;

    if (mode == 0) {
        g_PlayerAnimComponent = nullptr;
        if (aOut) *aOut = 1;
        return;
    }

    // modes 1-3: component handle from playerEntity->components
    // modes 4-6: LIVE bucket component from worldAnimationSystem (the updated instance)
    void* comp = nullptr;
    switch (mode) {
        case 1: comp = FindPlayerAnimatedComponentByName("root"); break;
        case 2: comp = FindPlayerAnimatedComponentByName("deformations"); break;
        case 3: comp = FindPlayerAnimatedComponentByName("shadow"); break;
        case 4: comp = FindPlayerBucketAnimatedComponent("root"); break;
        case 5: comp = FindPlayerBucketAnimatedComponent("deformations"); break;
        case 6: comp = FindPlayerBucketAnimatedComponent("shadow"); break;
        default: comp = nullptr; break;
    }
    g_PlayerAnimComponent = comp;
    if (aOut) *aOut = comp ? 1 : 0;
}

// Returns the currently armed bone-hook component pointer as low 32 bits (debug),
// or 0 if not armed.
void GetVRBoneHookArmed(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = static_cast<int32_t>(reinterpret_cast<uintptr_t>(g_PlayerAnimComponent) & 0xFFFFFFFF);
}

// mode: 0=total hook calls, 1=filter matches (rcx==player comp), 2=bone-write reached,
//       3=skeletal calls captured.
void GetVRBoneHookStats(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    uint64_t v = (mode == 1) ? g_hookMatchCalls : (mode == 2) ? g_hookBoneWrites
               : (mode == 3) ? g_hookSkeletalCalls : g_hookTotalCalls;
    if (v > 0x7FFFFFFF) v = 0x7FFFFFFF;
    if (aOut) *aOut = static_cast<int32_t>(v);
}

// Enables/disables capture mode and resets the captured list.
void SetVRBoneHookCapture(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    if (on) { g_capturedCount = 0; g_hookSkeletalCalls = 0; }
    g_hookCapture = on ? 1 : 0;
    if (aOut) *aOut = 1;
}

void GetVRBoneHookCapturedCount(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = g_capturedCount;
}

// Returns the captured skeletal rcx low-32 at index (compare against GetVRBoneHookArmed).
void GetVRBoneHookCapturedRcx(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = 0;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    if (index < 0 || index >= 32 || index >= g_capturedCount) { if (aOut) *aOut = 0; return; }
    if (aOut) *aOut = static_cast<int32_t>(g_capturedRcx[index]);
}

// Reads the bone count (DynArray size) of a captured skeletal component, so we can
// identify the player among the captured list by bone count (player root = 619).
// rcx -> modelInstance(+0x168) -> bone DynArray (+0xE0 ptr, +0xE8 size).
void GetVRBoneHookCapturedBoneCount(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = 0;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    if (aOut) *aOut = -1;
    if (index < 0 || index >= 32 || index >= g_capturedCount) return;
    void* rcx = reinterpret_cast<void*>(g_capturedFull[index]);
    if (!VRIK_IsReadable(rcx, 0x170)) return;
    void* mi = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(rcx) + 0x168);
    if (!VRIK_IsReadable(mi, 0xF0)) return;
    uint32_t size = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(mi) + 0xE8);
    if (aOut) *aOut = static_cast<int32_t>(size);
}

// Arms the bone-hook with a captured component pointer directly (bypasses bucket
// resolution). Use after identifying the player by bone count.
void ArmVRBoneHookByCaptureIndex(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = 0;
    RED4ext::GetParameter(aFrame, &index);
    aFrame->code++;
    if (index < 0 || index >= 32 || index >= g_capturedCount) { if (aOut) *aOut = 0; return; }
    g_PlayerAnimComponent = reinterpret_cast<void*>(g_capturedFull[index]);
    if (aOut) *aOut = 1;
}

// Prints the player body component's bone buffer base address so it can be fed to a
// memory write-breakpoint ("find what writes to this address") -> reveals the
// real MetaRig->bone pose-apply function to hook for VR hands.
// component(+0x168)=modelInstance, (+0xE0)=bone DynArray entries ptr (the float buffer).
void GetPlayerBoneBufferAddress(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = 0;

    std::ofstream out(VRDiagPath("player_bone_buffer.txt"), std::ios::trunc);
    void* comp = FindPlayerBucketAnimatedComponent("root");
    if (!comp || !VRIK_IsReadable(comp, 0x170)) {
        if (out.is_open()) out << "component not resolved or unreadable comp=0x" << std::hex
                               << reinterpret_cast<uintptr_t>(comp) << "\n";
        if (aOut) *aOut = -1;
        return;
    }
    void* modelInstance = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(comp) + 0x168);
    if (!VRIK_IsReadable(modelInstance, 0xF0)) {
        if (out.is_open()) out << "modelInstance unreadable mi=0x" << std::hex
                               << reinterpret_cast<uintptr_t>(modelInstance) << "\n";
        if (aOut) *aOut = -2;
        return;
    }
    if (out.is_open()) {
        out << std::hex
            << "component    = 0x" << reinterpret_cast<uintptr_t>(comp) << "\n"
            << "modelInstance= 0x" << reinterpret_cast<uintptr_t>(modelInstance) << std::dec << "\n\n";

        // Three candidate pose arrays found by the session-1 scan off modelInstance.
        const uint32_t candOffsets[] = { 0xE0, 0x208, 0x220 };
        for (uint32_t co : candOffsets) {
            void* arr = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(modelInstance) + co);
            uint32_t szA = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(modelInstance) + co + 0x8);
            uint32_t szB = *reinterpret_cast<uint32_t*>(reinterpret_cast<uint8_t*>(modelInstance) + co + 0xC);
            out << "candidate modelInstance+0x" << std::hex << co
                << " -> buffer=0x" << reinterpret_cast<uintptr_t>(arr) << std::dec
                << "  size@+8=" << szA << " size@+C=" << szB << "\n";
            if (VRIK_IsReadable(arr, 0x40)) {
                float* f = reinterpret_cast<float*>(arr);
                out << "   floats:";
                for (int i = 0; i < 8; ++i) out << " " << f[i];
                out << "\n";
            } else {
                out << "   (buffer not readable)\n";
            }
        }
    }
    void* boneBuffer = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(modelInstance) + 0xE0);
    if (aOut) *aOut = static_cast<int32_t>(reinterpret_cast<uintptr_t>(boneBuffer) & 0xFFFFFFFF);
}

// ---- Pose-apply hook control ----

// Installs the MinHook on the pose-apply function (module+0x17DDB4).
void InstallVRAnimPoseHook(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    // Only the pose-apply hook is installed; the ComponentFunc21 hook is dead code
    // that crushed FPS (see UpdateVRIKAnimInputs note).
    bool ok = InstallAnimPoseHook();
    if (aOut) *aOut = ok ? 1 : 0;
}

// ---- Weapon-aim native hook (M1 instrumentation) script API ----

// Installs the MinHooks on the projectile (+0x28D4B8) and TargetHelper (+0x46F774)
// shot functions. M1 = read-only instrumentation; returns 1 on success.
void InstallWeaponAimHook(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    bool ok = InstallWeaponAimHooks();
    if (aOut) *aOut = ok ? 1 : 0;
}

// Writes the current hook stats + last sampled vectors to weapon_aim_native.txt.
void DumpWeaponAimHookStats(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    std::ofstream out(VRDiagPath("weapon_aim_native.txt"), std::ios::out | std::ios::trunc);
    if (!out.is_open()) { if (aOut) *aOut = -1; return; }
    out << "installed=" << g_waInstalled << " enable=" << g_waEnable << " mode=" << g_waMode << "\n";
    out << "XFORM-GETTER calls=" << g_xfCalls << " mutated=" << g_xfMutated
        << " mode=" << g_xfMode << " testYaw=" << g_xfTestYaw << " shotInProg=" << g_shotInProgress << "  <== THE camera->shot lever\n";
    out << "  xf out-orient = " << g_xfLastOut[0] << " " << g_xfLastOut[1] << " " << g_xfLastOut[2] << " " << g_xfLastOut[3] << "\n";
    out << "SHOTSNAP calls=" << g_ssCalls << " snapped=" << g_ssSnapped
        << " camPtr=0x" << std::hex << g_ssCamPtr << std::dec
        << " enable=" << g_ssEnable << " mode=" << g_ssMode << " testYaw=" << g_ssTestYaw << "\n";
    out << "  bracket quat = " << g_ssCamQuat[0] << " " << g_ssCamQuat[1] << " " << g_ssCamQuat[2] << " " << g_ssCamQuat[3] << "\n";
    out << "  cam+0xD0(local) = " << g_ssDiagD0[0] << " " << g_ssDiagD0[1] << " " << g_ssDiagD0[2] << " " << g_ssDiagD0[3] << "\n";
    out << "  cam+0xF0(world) = " << g_ssDiagF0[0] << " " << g_ssDiagF0[1] << " " << g_ssDiagF0[2] << " " << g_ssDiagF0[3]
        << "  <== bracket target (default)\n";
    out << "  cam+0x110       = " << g_ssDiag110[0] << " " << g_ssDiag110[1] << " " << g_ssDiag110[2] << " " << g_ssDiag110[3] << "\n";
    out << "HEADING calls=" << g_waHeadCalls << " camObj=0x" << std::hex << g_waHeadObj << std::dec
        << " force=" << g_waHeadForce << "\n";
    out << "  set yaw/pitch=" << g_waHeadYaw << "/" << g_waHeadPitch
        << "  orig[+4E4]/[+4E8]=" << g_waHeadOrig4E4 << "/" << g_waHeadOrig4E8
        << "  [+4B8]=" << g_waHeadVal4B8 << " flag[+474]=" << g_waHeadFlag474 << "\n";
    out << "xhUpd calls=" << g_waXhCalls << " mutated=" << g_waXhMutated << "  <== crosshair-aim (UI)\n";
    out << "  cache+0x350 pos = " << g_waXhPos[0] << " " << g_waXhPos[1] << " " << g_waXhPos[2] << " " << g_waXhPos[3] << "\n";
    out << "  cache+0x370 dir = " << g_waXhDir[0] << " " << g_waXhDir[1] << " " << g_waXhDir[2] << " " << g_waXhDir[3]
        << "  (|xyz|=" << std::sqrt(g_waXhDir[0]*g_waXhDir[0]+g_waXhDir[1]*g_waXhDir[1]+g_waXhDir[2]*g_waXhDir[2]) << ")\n";
    out << "exeBase=0x" << std::hex << g_waExeBase << std::dec << "\n";
    out << "projCalls=" << g_waProjCalls << " projMutated=" << g_waProjMutated << "  <== projectile bullet lever\n";
    out << "  projCtrl=" << g_waProjCtrl << " always=" << g_waProjAlways << " fireInShot=" << g_fireInShot
        << " rejectReason=" << g_waProjRejectReason << " lastRetRva=0x" << std::hex << g_waProjLastRetRva << std::dec
        << " gateRva=0x" << std::hex << g_waProjGateRva << std::dec
        << " ctrlLen2=" << g_projDump[44] << " spd=" << g_projDump[45] << " targetLen2=" << g_projDump[46] << "\n";
    out << "  projRetCounts: 36F9FF(queue)=" << g_waProjRet36F9FF << " 36FD7C(update)=" << g_waProjRet36FD7C
        << " 4E5109(active)=" << g_waProjRet4E5109 << " 4E615F(alt)=" << g_waProjRet4E615F << "\n";
    out << "targetCalls=" << g_waTargetCalls << " fromShot=" << g_waTargetFromShot
        << " redirects=" << g_waRedirects << " lastRetRva=0x" << std::hex << g_waLastRetRva << std::dec
        << " testYaw=" << g_xfTestYaw << " plane=" << g_xfTestPlane << "\n";
    out << "classifyCalls=" << g_waClassifyCalls << " fromShot=" << g_waClassifyFromShot << "\n";
    out << "normPatched=" << g_waNormPatched << " normShot(@0x46F0E5)=" << g_waNormShot
        << " normMutated=" << g_waNormMutated << "  <== bullet-dir lever\n";
    out << "fireNormPatched=" << g_waFireNormPatched << " fireNormShot(@0x84C968)=" << g_waFireNormShot
        << " fireNormMutated=" << g_waFireNormMutated << "  <== weapon fire Normalize(target-muzzle)\n";
    out << "physPatched=" << g_waPhysPatched << " physCalls(@0x46F1EA)=" << g_waPhysCalls
        << " physMutated=" << g_waPhysMutated << "\n";
    out << "shotPipeline: CandA(0x291D9C8)=" << g_waCandA << " CandB(0x291DD54)=" << g_waCandB
        << " SVP(0x292263C)=" << g_waSVP << " SFVW(0x29216D0)=" << g_waSFVW << "\n";
    out << "physArgSnapshot snapped=" << g_waDbgSnapped << " (look for the unit dir vector ~= camera forward)\n";
    out << "-- arg3 (basis?) floats @+0x00..0x120 --\n";
    for (int i = 0; i < 72; i += 4)
        out << "   +0x" << std::hex << (i*4) << std::dec << ": "
            << g_waDbgArg3[i] << " " << g_waDbgArg3[i+1] << " " << g_waDbgArg3[i+2] << " " << g_waDbgArg3[i+3] << "\n";
    out << "-- rayList floats @+0x00..0xA0 --\n";
    for (int i = 0; i < 40; i += 4)
        out << "   +0x" << std::hex << (i*4) << std::dec << ": "
            << g_waDbgRay[i] << " " << g_waDbgRay[i+1] << " " << g_waDbgRay[i+2] << " " << g_waDbgRay[i+3] << "\n";
    out << "-- rayList[0] entry floats @+0x00..0x70 --\n";
    for (int i = 0; i < 28; i += 4)
        out << "   +0x" << std::hex << (i*4) << std::dec << ": "
            << g_waDbgRayEntry[i] << " " << g_waDbgRayEntry[i+1] << " " << g_waDbgRayEntry[i+2] << " " << g_waDbgRayEntry[i+3] << "\n";
    out << "publishedFwd  = " << g_waFwd[0] << " " << g_waFwd[1] << " " << g_waFwd[2] << " (seq " << g_waFwdSeq << ")\n";
    out << "publishedPos  = " << g_waPos[0] << " " << g_waPos[1] << " " << g_waPos[2] << "\n";
    out << "shotOrigin    = " << g_waTargetOrigin[0] << " " << g_waTargetOrigin[1] << " " << g_waTargetOrigin[2] << "\n";
    out << "origAimDelta  = " << g_waTargetDir[0] << " " << g_waTargetDir[1] << " " << g_waTargetDir[2] << "\n";
    out.close();
    if (aOut) *aOut = 1;
}

// Live stat getter for the overlay (no file). which: 0=targetCalls, 1=redirects,
// 2=projCalls, 3=installed, 4=lastFwdSeq.
void GetWeaponAimStat(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t which = 0;
    RED4ext::GetParameter(aFrame, &which);
    aFrame->code++;
    int32_t v = 0;
    switch (which) {
        case 0: v = static_cast<int32_t>(g_waTargetCalls); break;
        case 1: v = static_cast<int32_t>(g_waRedirects); break;
        case 2: v = static_cast<int32_t>(g_waProjCalls); break;
        case 3: v = g_waInstalled; break;
        case 4: v = static_cast<int32_t>(g_waFwdSeq); break;
        case 5: v = static_cast<int32_t>(g_waClassifyFromShot); break;
        case 6: v = static_cast<int32_t>(g_waClassifyCalls); break;
        case 7: v = static_cast<int32_t>(g_waTargetFromShot); break;
        case 8: v = static_cast<int32_t>(g_waPhysCalls); break;
        case 9: v = static_cast<int32_t>(g_waPhysMutated); break;
        case 10: v = g_waPhysPatched; break;
        case 11: v = static_cast<int32_t>(g_waCandA); break;
        case 12: v = static_cast<int32_t>(g_waCandB); break;
        case 13: v = static_cast<int32_t>(g_waSVP); break;
        case 14: v = static_cast<int32_t>(g_waSFVW); break;
        case 15: v = static_cast<int32_t>(g_waNormShot); break;
        case 16: v = static_cast<int32_t>(g_waNormMutated); break;
        case 17: v = g_waNormPatched; break;
        case 18: v = static_cast<int32_t>(g_waProjMutated); break;
        case 19: v = static_cast<int32_t>(g_waXhCalls); break;
        case 20: v = static_cast<int32_t>(g_waXhMutated); break;
        case 21: v = static_cast<int32_t>(g_waHeadCalls); break;
        case 22: v = (g_waHeadObj != 0) ? 1 : 0; break;
        case 23: v = g_waHeadForce; break;
        case 24: v = static_cast<int32_t>(g_ssCalls); break;
        case 25: v = static_cast<int32_t>(g_ssSnapped); break;
        case 26: v = (g_ssCamPtr != 0) ? 1 : 0; break;
        case 27: v = static_cast<int32_t>(g_xfCalls); break;
        case 28: v = static_cast<int32_t>(g_xfMutated); break;
        case 29: v = static_cast<int32_t>(g_goCalls); break;
        case 30: v = static_cast<int32_t>(g_goMutated); break;
        case 31: v = static_cast<int32_t>(g_waTgtOvr); break;
        case 32: v = g_waFireNormPatched; break;
        case 33: v = static_cast<int32_t>(g_waFireNormShot); break;
        case 34: v = static_cast<int32_t>(g_waFireNormMutated); break;
        case 35: v = static_cast<int32_t>(g_waProjRejectReason); break;
        case 36: v = static_cast<int32_t>(g_waProjLastRetRva); break;
        case 37: v = g_waProjCtrl; break;
        case 38: v = g_waProjAlways; break;
        case 39: v = static_cast<int32_t>(g_waProjRet36F9FF); break;
        case 40: v = static_cast<int32_t>(g_waProjRet36FD7C); break;
        case 41: v = static_cast<int32_t>(g_waProjRet4E5109); break;
        case 42: v = static_cast<int32_t>(g_waProjRet4E615F); break;
        case 43: v = static_cast<int32_t>(g_waProjGateRva); break;
        default: v = -1; break;
    }
    if (aOut) *aOut = v;
}

// Writes the live FPP-camera address + key field addresses to cam_addr.txt, formatted for
// an external memory-inspection tool ("find what writes/accesses to this address"). No guessing -- it finds the
// exact instruction that writes the aim/orientation, on the live game, no restarts.
void DumpVRCamAddr(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    std::ofstream out(VRDiagPath("cam_addr.txt"), std::ios::out | std::ios::trunc);
    if (!out.is_open()) { if (aOut) *aOut = -1; return; }
    const uintptr_t cam = g_ssCamPtr;
    out << "=== Camera watch targets (live FPP camera) ===\n";
    out << "camPtr            = " << std::hex << cam << "\n";
    out << "cam+0xF0 (WORLD orientation quat, what render+shot use):  " << (cam + 0xF0) << "\n";
    out << "cam+0xD0 (local orientation, usually identity):           " << (cam + 0xD0) << "\n";
    out << "cam+0x110 (world position):                               " << (cam + 0x110) << std::dec << "\n";
    out << "\nIn your memory-inspection tool: attach to Cyberpunk2077.exe -> Memory View -> Ctrl+G -> paste\n";
    out << "the cam+0xF0 address -> right-click the byte -> 'Find out what ACCESSES this address'\n";
    out << "-> shoot at the wall + turn head -> the instruction list = the aim readers/writers.\n";
    out << "Send me that instruction list (the 'Cyberpunk2077.exe+XXXXXX' addresses).\n";
    out.close();
    if (aOut) *aOut = 1;
}

// HW-breakpoint trace control: start watching the shot camera field (camPtr+offset) for
// reads, fire a shot, stop, dump the accessor RVAs. offsetSel: 0=+0x110(origin) 1=+0xF0(orient).
void StartVRCamTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t offsetSel = 0, gated = 0, writeOnly = 0;
    RED4ext::GetParameter(aFrame, &offsetSel);
    RED4ext::GetParameter(aFrame, &gated);
    RED4ext::GetParameter(aFrame, &writeOnly);
    aFrame->code++;
    uintptr_t watch = 0;
    if (offsetSel == 2) {
        // LOCATED camera (dxgi HMD-injection / render cam) quat, from shared mem [51]/[52]+16.
        if (g_pSharedHands) {
            uint32_t lo=0, hi=0;
            std::memcpy(&lo, &g_pSharedHands[51], 4);
            std::memcpy(&hi, &g_pSharedHands[52], 4);
            uintptr_t cam = (static_cast<uintptr_t>(hi) << 32) | static_cast<uintptr_t>(lo);
            if (cam) watch = cam + 16;
        }
    } else {
        const uintptr_t off = (offsetSel == 1) ? 0xF0 : 0x110;
        if (g_ssCamPtr) watch = g_ssCamPtr + off;
    }
    if (watch) { Wa_StartTrace(watch, gated, writeOnly); if (aOut) *aOut = 1; }
    else if (aOut) *aOut = 0;
}
void StopVRCamTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    Wa_StopTrace();
    if (aOut) *aOut = g_traceCount;
}
void DumpVRCamTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    std::ofstream out(VRDiagPath("cam_trace.txt"), std::ios::out | std::ios::trunc);
    if (!out.is_open()) { if (aOut) *aOut = -1; return; }
    out << "watched=0x" << std::hex << g_traceAddr << std::dec
        << " hits=" << g_traceHits << " uniqueRVAs=" << g_traceCount << " active=" << g_traceActive << "\n";
    out << "accessor RVAs + hit COUNT (low=per-shot reader, high=per-frame render):\n";
    for (int i = 0; i < g_traceCount && i < 128; ++i)
        out << "  0x" << std::hex << (0x140000000ull + g_traceRvas[i]) << std::dec
            << "  count=" << g_traceRvaCounts[i] << "\n";
    out.close();
    if (aOut) *aOut = g_traceCount;
}

// CET publishes the FPP camera object pointer each frame so the ShotSnap hook can
// bracket cam+0xD0. Mirrors SetVRRightHandEntity (handle -> instance pointer).
// cam+0xD0 additive head-inject control.
volatile int g_headLocalEnable = 0;
volatile int g_headLocalConv = 0;

// ADDITIVE HEAD INJECT: write the head-relative quaternion (hmdRel,
// shared slots 16..19) into the FPP cam LOCAL orientation @ cam+0xD0. The game composes
// world = heading (X) local(cam+0xD0), so the VIEW gets the head while the HEADING (=stick
// aim) is untouched -> the bullet follows the stick, not the head. dxgi must be in SKIP-HMD
// ALWAYS mode (HMD now flows via cam+0xD0). conv selects the VR->game-local axis mapping.
// Separate __try function (the script-callback can't use __try -- it has C++ unwinding objects).
static void WriteHeadLocal() {
    if (!(g_headLocalEnable && g_ssCamPtr && g_pSharedHands)) return;
    RefreshHandsSnapshot();
    const float hi = SharedPose(16), hj = SharedPose(17), hk = SharedPose(18), hr = SharedPose(19);
    const float l = hi*hi + hj*hj + hk*hk + hr*hr;
    if (l <= 0.25f) return;
    float qi, qj, qk, qr;
    switch (g_headLocalConv) {
        case 1: qi =  hi; qj = -hk; qk =  hj; qr = hr; break; // VRIK map (i,-k,j,r)
        case 2: qi = -hi; qj = -hj; qk = -hk; qr = hr; break; // inverse
        case 3: qi =  hi; qj =  hk; qk = -hj; qr = hr; break;
        case 4: qi = -hi; qj =  hk; qk =  hj; qr = hr; break;
        case 5: qi =  hk; qj =  hj; qk = -hi; qr = hr; break;
        default: qi = hi; qj = hj; qk = hk; qr = hr; break; // identity
    }
    __try {
        float* q = reinterpret_cast<float*>(g_ssCamPtr + 0xD0);
        q[0] = qi; q[1] = qj; q[2] = qk; q[3] = qr;
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

void SetVRShotCamera(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    RED4ext::Handle<RED4ext::IScriptable> cam;
    RED4ext::GetParameter(aFrame, &cam);
    aFrame->code++;
    g_ssCamPtr = reinterpret_cast<uintptr_t>(cam.instance); // FPP camera component instance
    WriteHeadLocal();
}

void SetVRHeadLocal(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t enable = 0, conv = 0;
    RED4ext::GetParameter(aFrame, &enable);
    RED4ext::GetParameter(aFrame, &conv);
    aFrame->code++;
    g_headLocalEnable = enable; g_headLocalConv = conv;
    if (aOut) *aOut = conv;
}

// SKIP-HMD test: tells dxgi (via shared mem [58]) to skip the HMD camera overwrite.
// mode 0=off, 1=ALWAYS (no head -- view follows game aim), 2=shot-frame only (decouple test).
void SetVRSkipHmdTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    EnsureSharedMemory();
    if (g_pSharedHands) reinterpret_cast<volatile uint32_t*>(g_pSharedHands)[58] = static_cast<uint32_t>(mode);
    if (aOut) *aOut = mode;
}

// MENU OPEN bridge: redscript sets this when a full-screen menu (e.g. the world
// map) opens/closes. dxgi reads shared[81] in OnLocateCameraCallback and
// ApplySettings/DLSSResolutionOverride: while the flag is set, dxgi stops
// driving the game camera with the HMD orientation (menu/map does NOT swim with
// head rotation) and suspends the square-
// resolution force (fixes map pin drift on pan/zoom).
// SLOT CHOICE — slot [81] is dedicated and unused elsewhere. Do NOT use [63]
// (overwritten every frame by the hand delta-quaternion in OnLocateCameraCallback)
// or [70..75] (shoulder-calibration slots read by PollVRCalibFromShared as
// g_VRShoulderRX/RY/RZ — writing the map flag there zeroes the VRIK shoulder
// pose and causes body jitter whenever the map opens/closes).
static constexpr int kWorldMapMenuOpenSharedSlot = 81;
void SetVRMenuOpen(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t open = 0;
    RED4ext::GetParameter(aFrame, &open);
    aFrame->code++;
    EnsureSharedMemory();
    if (g_pSharedHands) {
        reinterpret_cast<volatile uint32_t*>(g_pSharedHands)[kWorldMapMenuOpenSharedSlot] = static_cast<uint32_t>(open ? 1 : 0);
    }
    if (aOut) *aOut = open;
}

// GetWorldOrientation (0x802390) override -- the Cheat-Engine-confirmed shot aim reader.
// mode: 0=off, 1=ALWAYS (test view+bullet), 2=gated-by-shot (decouple). plane for testYaw.
void SetVRGetOrient(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0, plane = 0; float testYaw = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &testYaw);
    RED4ext::GetParameter(aFrame, &plane);
    aFrame->code++;
    g_goMode = mode; g_goTestYaw = testYaw; g_goPlane = plane;
    if (aOut) *aOut = static_cast<int32_t>(g_goMutated);
}

// Camera-transform getter override control. mode: 0=off, 1=ALWAYS (test), 2=gated-by-shot.
void SetVRXformOverride(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0; float testYaw = 0.0f; int32_t plane = 0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &testYaw);
    RED4ext::GetParameter(aFrame, &plane);
    aFrame->code++;
    g_xfMode = mode; g_xfTestYaw = testYaw; g_xfTestPlane = plane;
    if (aOut) *aOut = static_cast<int32_t>(g_waTargetFromShot);
}

// FIRE-SHOT lever. mode: 0=scan-only, 1=bend-test (rotate the field at the
// override offset by `angle` rad about `plane`), 2=controller override (write dxgi controller
// forward [shared 60..62]; neg flips sign). Returns the fire-call count.
void SetVRFireMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0, plane = 0, neg = 0; float angle = 0.0f;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &plane);
    RED4ext::GetParameter(aFrame, &angle);
    RED4ext::GetParameter(aFrame, &neg);
    aFrame->code++;
    g_fireMode = mode; g_firePlane = plane; g_fireTestAng = angle; g_fireNeg = neg;
    if (aOut) *aOut = static_cast<int32_t>(g_fireCalls);
}

// Configure the auto-scanner: src 0=r8(shot-ctx) 1=rdx(arg2) 2=*(rdx+0x10)(transform) 3=*(rdx),
// range = bytes to scan. Returns the scan source for confirmation.
void SetVRFireScan(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t src = 0, range = 0x2300;
    RED4ext::GetParameter(aFrame, &src);
    RED4ext::GetParameter(aFrame, &range);
    aFrame->code++;
    g_fireScanSrc = src; if (range > 0) g_fireScanRange = range;
    if (aOut) *aOut = src;
}

// Configure the override target (where mode 1/2 writes): src enum (same as scan), byte offset.
void SetVRFireOverrideTarget(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t src = 0, off = 0;
    RED4ext::GetParameter(aFrame, &src);
    RED4ext::GetParameter(aFrame, &off);
    aFrame->code++;
    g_fireOvrSrc = src; g_fireOvrOff = off;
    if (aOut) *aOut = off;
}

// Read back fire-hook state. idx: 0..3 override-target field (pre-write), 4..7 what we wrote,
// 8 calls, 9 mutated, 10 hitCount, 11 scanSrc.
void GetVRFireDump(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = 0;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    float v = 0.0f;
    if (idx >= 0 && idx <= 3) v = g_fireDir[idx];
    else if (idx >= 4 && idx <= 7) v = g_fireDirOut[idx - 4];
    else if (idx == 8) v = static_cast<float>(g_fireCalls);
    else if (idx == 9) v = static_cast<float>(g_fireMutated);
    else if (idx == 10) v = static_cast<float>(g_fireHitCount);
    else if (idx == 11) v = static_cast<float>(g_fireScanSrc);
    if (aOut) *aOut = v;
}

// Read an auto-scan hit. hit = 0..g_fireHitCount-1; field: 0=byteOffset, 1=x, 2=y, 3=z, 4=dotCtrl.
void GetVRFireHit(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t hit = 0, field = 0;
    RED4ext::GetParameter(aFrame, &hit);
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    float v = 0.0f;
    if (hit >= 0 && hit < g_fireHitCount && hit < 24) {
        if (field == 0) v = static_cast<float>(g_fireHitOff[hit]);
        else if (field >= 1 && field <= 3) v = g_fireHitVec[hit*3 + (field-1)];
        else if (field == 4) v = g_fireHitDot[hit];
    }
    if (aOut) *aOut = v;
}

// TargetHelper clean controller-redirect: target = origin + controllerFwd*100. on, neg(flip).
// Returns the override count (so the UI confirms it applied during the shot).
void SetVRTargetCtrl(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0, neg = 0;
    RED4ext::GetParameter(aFrame, &on);
    RED4ext::GetParameter(aFrame, &neg);
    aFrame->code++;
    g_waTgtCtrl = on; g_waTgtNeg = neg;
    if (aOut) *aOut = static_cast<int32_t>(g_waTgtOvr);
}

// ★ TRANSFORM-orientation override: write the controller aim quat into the shooter transform
// (*(rdx+0x10)) that the bullet raycast uses. mode: 0 off, 1 +0xF0(world), 2 +0xD0(local), 3 both.
// off = the world-orient quat offset (default 0xF0) for scrubbing. Returns g_fireMutated.
void SetVRFireXform(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0, off = 0xF0;
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &off);
    aFrame->code++;
    g_fireXform = mode; if (off >= 0 && off <= 0x400) g_fireXformOff = off;
    if (aOut) *aOut = static_cast<int32_t>(g_fireMutated);
}

// ★ CAM-SNAP: during the shot, force the FPP camera orientation to the controller so the projectile
// launch's orientation provider (which reads the camera) launches the bullet down the controller.
// mode on/off; off = cam quat offset (0xF0 world / 0xD0 local).
void SetVRFireCamSnap(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0, off = 0xF0;
    RED4ext::GetParameter(aFrame, &on);
    RED4ext::GetParameter(aFrame, &off);
    aFrame->code++;
    g_fireCamSnap = on; if (off >= 0 && off <= 0x400) g_fireCamSnapOff = off;
    if (aOut) *aOut = static_cast<int32_t>(g_fireMutated);
}

// ★ Projectile ShootEvent startVelocity -> controller forward (the player bullet is a projectile).
// on, neg(flip). Returns g_waProjMutated count.
void SetVRProjCtrl(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0, neg = 0, unguide = 1, always = 0;
    RED4ext::GetParameter(aFrame, &on);
    RED4ext::GetParameter(aFrame, &neg);
    RED4ext::GetParameter(aFrame, &unguide);
    RED4ext::GetParameter(aFrame, &always);
    aFrame->code++;
    g_waProjCtrl = on; g_waProjNeg = neg; g_waProjUnguide = unguide; g_waProjAlways = always;
    if (aOut) *aOut = static_cast<int32_t>(g_waProjMutated);
}

// Read projectile-event diagnostics. idx 0-2 startPoint, 3-5 startVelocity(pre), 6-8 targetPos(pre),
// 9 guided flag, 10-12 controller dir written.
void GetVRProjDump(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = 0;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    float v = (idx >= 0 && idx < 64) ? g_projDump[idx] : 0.0f;
    if (aOut) *aOut = v;
}

// Select which localToWorld row (0..3) is used as the world muzzle origin for targetPosition.
void SetVRProjOriginRow(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t row = 3;
    RED4ext::GetParameter(aFrame, &row);
    aFrame->code++;
    if (row >= 0 && row <= 3) g_waProjOriginRow = row;
    if (aOut) *aOut = g_waProjOriginRow;
}

// Restrict projectile copy mutation to one return RVA. 0 = all callers; default is 0x4E5109.
// This avoids mutating queue/template copies.
void SetVRProjGateRva(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t rva = 0;
    RED4ext::GetParameter(aFrame, &rva);
    aFrame->code++;
    g_waProjGateRva = static_cast<uint32_t>(rva);
    if (aOut) *aOut = static_cast<int32_t>(g_waProjGateRva);
}

// Pump the player/camera WORLD position (from CET player:GetWorldPosition()) -> the projectile
// targetPosition origin (same world frame as the event's targetPosition).
void SetVRShotOrigin(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float x = 0, y = 0, z = 0;
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    aFrame->code++;
    g_shotOrigin[0] = x; g_shotOrigin[1] = y; g_shotOrigin[2] = z;
    if (aOut) *aOut = 1;
}

// === TRACE-DISPATCHER funnel instrumentation ===
// Control override: on=1 rewrites the ray END point to the controller forward during the shot;
// asFloat = ray origin/end format (1 float Vec3, 0 fixed-point); neg flips; gateRet (hex RVA, 0
// = all shot traces) restricts override to one caller once identified.
void SetVRTraceOverride(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0, writeOff = 0x18, force = 0, neg = 0, gateRet = 0;
    RED4ext::GetParameter(aFrame, &on);
    RED4ext::GetParameter(aFrame, &writeOff);  // byte offset in ray struct to write the unit dir
    RED4ext::GetParameter(aFrame, &force);     // 1 = write even if current isn't a unit vector
    RED4ext::GetParameter(aFrame, &neg);
    RED4ext::GetParameter(aFrame, &gateRet);
    aFrame->code++;
    g_trOverride = on; g_trWriteOff = writeOff; g_trForce = force; g_trNeg = neg; g_trGateRet = static_cast<uint32_t>(gateRet);
    if (aOut) *aOut = static_cast<int32_t>(g_trOvrCount);
}

// Reset the captured return-RVA ring so a fresh shot's callers can be observed.
void ResetVRTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    g_trRetCount = 0; g_trShotCalls = 0; g_trOvrCount = 0;
    for (int i = 0; i < 12; ++i) { g_trRetRing[i] = 0; }
    if (aOut) *aOut = 0;
}

// Read trace summary. idx: 0=retCount, 1=shotCalls, 2=ovrCount; 10..25 = retRing[idx-10] (RVA).
void GetVRTrace(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = 0;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    float v = 0.0f;
    if (idx == 0) v = static_cast<float>(g_trRetCount);
    else if (idx == 1) v = static_cast<float>(g_trShotCalls);
    else if (idx == 2) v = static_cast<float>(g_trOvrCount);
    else if (idx >= 10 && idx < 26) v = static_cast<float>(g_trRetRing[idx - 10]);
    if (aOut) *aOut = v;
}

// Read a captured caller's ray. caller = 0..g_trRetCount-1. field: 0=retRVA, 1=hits,
// 10..21 = ray dword[field-10] as INT value, 30..41 = ray dword[field-30] as FLOAT.
// The bullet caller's ray has a real origin (muzzle/camera world pos) at dwords [2..4] (+0x08).
void GetVRTraceCaller(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, float* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t caller = 0, field = 0;
    RED4ext::GetParameter(aFrame, &caller);
    RED4ext::GetParameter(aFrame, &field);
    aFrame->code++;
    float v = 0.0f;
    if (caller >= 0 && caller < g_trRetCount && caller < 16) {
        if (field == 0) v = static_cast<float>(g_trRetRing[caller]);
        else if (field == 1) v = static_cast<float>(g_trCallerHits[caller]);
        else if (field >= 10 && field < 22) v = static_cast<float>(static_cast<int32_t>(g_trCallerRay[caller*12 + (field-10)]));
        else if (field >= 30 && field < 42) { uint32_t r = g_trCallerRay[caller*12 + (field-30)]; float f; memcpy(&f, &r, 4); v = f; }
        else if (field >= 50 && field < 54) v = g_trCallerDir[caller*4 + (field-50)]; // arg3 direction xyz w
    }
    if (aOut) *aOut = v;
}

// ShotSnap control: enable + compose mode + static-yaw sanity test (radians).
void SetVRShotSnap(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t enable = 0, mode = 0; float testYaw = 0.0f;
    RED4ext::GetParameter(aFrame, &enable);
    RED4ext::GetParameter(aFrame, &mode);
    RED4ext::GetParameter(aFrame, &testYaw);
    aFrame->code++;
    g_ssEnable = enable; g_ssMode = mode; g_ssTestYaw = testYaw;
    if (aOut) *aOut = static_cast<int32_t>(g_ssCalls);
}

// Heading-decouple test control from CET: force flag + static yaw/pitch offset.
void SetVRHeadingTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t force = 0; float yaw = 0.0f, pitch = 0.0f;
    RED4ext::GetParameter(aFrame, &force);
    RED4ext::GetParameter(aFrame, &yaw);
    RED4ext::GetParameter(aFrame, &pitch);
    aFrame->code++;
    g_waHeadForce = force; g_waHeadYaw = yaw; g_waHeadPitch = pitch;
    if (aOut) *aOut = static_cast<int32_t>(g_waHeadCalls);
}

// CET pushes the live weapon aim each frame: forward (unit, world), muzzle world pos,
// enable + mode + gate distance. mode bit0 = override the shot origin with the muzzle pos.
void SetVRWeaponAim(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float fx = 0, fy = 0, fz = 0, px = 0, py = 0, pz = 0, gate = 5.0f;
    int32_t enable = 0, mode = 0;
    RED4ext::GetParameter(aFrame, &fx); RED4ext::GetParameter(aFrame, &fy); RED4ext::GetParameter(aFrame, &fz);
    RED4ext::GetParameter(aFrame, &px); RED4ext::GetParameter(aFrame, &py); RED4ext::GetParameter(aFrame, &pz);
    RED4ext::GetParameter(aFrame, &enable); RED4ext::GetParameter(aFrame, &mode); RED4ext::GetParameter(aFrame, &gate);
    aFrame->code++;
    g_waFwd[0] = fx; g_waFwd[1] = fy; g_waFwd[2] = fz;
    g_waPos[0] = px; g_waPos[1] = py; g_waPos[2] = pz;
    g_waEnable = enable; g_waMode = mode; g_waGateDist = gate;
    ++g_waFwdSeq;
    if (aOut) *aOut = static_cast<int32_t>(g_waFwdSeq);
}

// Resolves the player's live track buffers (a2[7][3] candidates) so the hook can
// identify the player call. Returns bitmask: 1=bufA set, 2=bufB set.
static int VRIK_DoArmPlayer() {
    auto* animObj = FindPlayerAnimatedObjectByComponentName("root");
    if (!animObj || !VRIK_IsReadable(animObj, 0x40)) return -1;
    uint8_t* base = reinterpret_cast<uint8_t*>(animObj);

    g_PlayerTrackBufA = 0;
    g_PlayerTrackBufB = 0;

    // A: *(*(animObj+0x8) + 0x40)
    void* ownerA = *reinterpret_cast<void**>(base + 0x8);
    if (VRIK_IsReadable(ownerA, 0x48))
        g_PlayerTrackBufA = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(ownerA) + 0x40);

    // B: *(*(animObj+0x18) + 0x18)
    void* ownerB = *reinterpret_cast<void**>(base + 0x18);
    if (VRIK_IsReadable(ownerB, 0x20))
        g_PlayerTrackBufB = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(ownerB) + 0x18);

    // Resolve the head + hand bone indices from the metaRig bone names so the
    // pose hook does not rely on hard-coded guesses. The buffer the hook writes
    // (a2[7][0]) is indexed the same as metaRig->boneNames. Prefer an exact name
    // match, fall back to the shortest name containing the needles (so we get the
    // hand root, not a finger like "RightHandThumb1").
    auto* metaRig = animObj->metaRig;
    if (metaRig && std::strcmp(ClassifyQword(reinterpret_cast<uint64_t>(metaRig)), "HEAP") == 0)
    {
        const uint32_t boneCount = metaRig->boneNames.Size();
        if (boneCount > 0 && boneCount < 8192)
        {
            int head = -1, rightHand = -1, leftHand = -1;
            int rightArm = -1, rightFore = -1, leftArm = -1, leftFore = -1;
            int spineTmp[8] = {-1,-1,-1,-1,-1,-1,-1,-1};
            int spineTmpCount = 0;
            const size_t kNoMatch = static_cast<size_t>(-1);
            size_t headLen = kNoMatch, rightLen = kNoMatch, leftLen = kNoMatch;
            for (uint32_t i = 0; i < boneCount; ++i)
            {
                const char* nm = metaRig->boneNames[i].ToString();
                if (!nm || !nm[0])
                    continue;
                const size_t len = std::strlen(nm);

                if (EqualsInsensitive(nm, "Head")) { head = static_cast<int>(i); headLen = 0; }
                else if (headLen != 0 && ContainsInsensitive(nm, "head") && len < headLen)
                { head = static_cast<int>(i); headLen = len; }

                const bool isHand = ContainsInsensitive(nm, "hand");
                if (isHand && ContainsInsensitive(nm, "right"))
                {
                    if (EqualsInsensitive(nm, "RightHand")) { rightHand = static_cast<int>(i); rightLen = 0; }
                    else if (rightLen != 0 && len < rightLen) { rightHand = static_cast<int>(i); rightLen = len; }
                }
                if (isHand && ContainsInsensitive(nm, "left"))
                {
                    if (EqualsInsensitive(nm, "LeftHand")) { leftHand = static_cast<int>(i); leftLen = 0; }
                    else if (leftLen != 0 && len < leftLen) { leftHand = static_cast<int>(i); leftLen = len; }
                }

                // Arm-chain joints for the full IK (exact names on the player rig).
                if (EqualsInsensitive(nm, "RightArm"))     rightArm  = static_cast<int>(i);
                if (EqualsInsensitive(nm, "RightForeArm")) rightFore = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftArm"))      leftArm   = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftForeArm"))  leftFore  = static_cast<int>(i);
                // Torso chain. Weapon-ready poses mostly bend Spine* backward, which moves the
                // shoulders before our arm IK runs. Keep only the spine bones, not hips/head.
                if (ContainsInsensitive(nm, "spine") && spineTmpCount < 8) {
                    spineTmp[spineTmpCount++] = static_cast<int>(i);
                }
                // Hip + leg bones: holster proximity AND the full-body lower chain (move hips
                // under the HMD, keep feet planted via leg IK).
                if (EqualsInsensitive(nm, "RightUpLeg"))   g_VRRightUpLegIdx = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftUpLeg"))    g_VRLeftUpLegIdx  = static_cast<int>(i);
                if (EqualsInsensitive(nm, "RightLeg"))     g_VRRightLegIdx   = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftLeg"))      g_VRLeftLegIdx    = static_cast<int>(i);
                if (EqualsInsensitive(nm, "RightFoot"))    g_VRRightFootIdx  = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftFoot"))     g_VRLeftFootIdx   = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Hips"))         g_VRHipsIdx       = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Neck"))         g_VRNeckIdx       = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Neck1"))        g_VRNeck1Idx      = static_cast<int>(i);
            }

            if (head >= 0)      g_VRHeadBoneIdx  = head;
            if (rightHand >= 0) g_VRRightBoneIdx = rightHand;
            if (leftHand >= 0)  g_VRLeftBoneIdx  = leftHand;
            if (rightArm >= 0)  g_VRRightUpperArmIdx = rightArm;
            if (rightFore >= 0) g_VRRightForeArmIdx  = rightFore;
            if (leftArm >= 0)   g_VRLeftUpperArmIdx  = leftArm;
            if (leftFore >= 0)  g_VRLeftForeArmIdx   = leftFore;
            g_VRSpineCount = spineTmpCount;
            for (int s = 0; s < 8; ++s) g_VRSpineIdx[s] = (s < spineTmpCount) ? spineTmp[s] : -1;

            // Copy the parent-index table so the pose hook can run FK each frame.
            const uint32_t pc = metaRig->parentIndeces.Size();
            const uint32_t copyN = (pc < boneCount ? pc : boneCount);
            int written = 0;
            for (uint32_t i = 0; i < copyN && i < 256; ++i) { g_VRBoneParent[i] = metaRig->parentIndeces[i]; ++written; }
            g_VRBoneCount = written;

            std::ofstream out(VRDiagPath("vrik_bone_resolve.txt"), std::ios::trunc);
            if (out.is_open())
                out << "boneCount=" << boneCount
                    << " parentCount=" << pc
                    << " head=" << g_VRHeadBoneIdx
                    << " rightHand=" << g_VRRightBoneIdx
                    << " leftHand=" << g_VRLeftBoneIdx
                    << " rightArm=" << g_VRRightUpperArmIdx
                    << " rightForeArm=" << g_VRRightForeArmIdx
                    << " leftArm=" << g_VRLeftUpperArmIdx
                    << " leftForeArm=" << g_VRLeftForeArmIdx
                    << " spineCount=" << g_VRSpineCount << "\n";
        }
    }

    return (g_PlayerTrackBufA ? 1 : 0) | (g_PlayerTrackBufB ? 2 : 0);
}

void ArmVRAnimPosePlayer(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    int r = VRIK_DoArmPlayer();
    if (aOut) *aOut = r;
}

void SetVRAnimPoseDebug(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    g_AnimPoseDebug = mode;
    if (aOut) *aOut = 1;
}

// mode 0=total calls, 1=player-match calls, 2=last matched bone buffer low-32.
void GetVRAnimPoseStats(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    uint64_t v = (mode == 1) ? g_AnimPoseMatchCalls
               : (mode == 2) ? g_AnimPoseLastBoneBuf
               : g_AnimPoseTotalCalls;
    if (aOut) *aOut = static_cast<int32_t>(v & 0xFFFFFFFF);
}

// Single-bone calibration: pushes only bone[index] translation by magnitude on every
// axis (debug mode 2). Sweep indices to find which one is a hand.
void SetVRAnimPoseBoneTest(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t index = -1;
    float   mag = 1.0f;
    RED4ext::GetParameter(aFrame, &index);
    RED4ext::GetParameter(aFrame, &mag);
    aFrame->code++;
    g_AnimPoseTestBone = index;
    g_AnimPoseTestMag = mag;
    g_AnimPoseDebug = (index >= 0) ? 2 : 0;
    if (aOut) *aOut = 1;
}

// Dumps every metaRig bone name with its index so we can locate the hand bones.
void DumpPlayerBoneNames(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    if (aOut) *aOut = 0;

    std::ofstream out(VRDiagPath("player_bone_names.txt"), std::ios::trunc);
    auto* animObj = FindPlayerAnimatedObjectByComponentName("root");
    auto* metaRig = animObj ? animObj->metaRig : nullptr;
    if (!metaRig) { if (aOut) *aOut = -1; return; }

    const uint32_t n = metaRig->boneNames.Size();
    if (out.is_open()) {
        out << "boneCount=" << n << "\n";
        for (uint32_t i = 0; i < n; ++i) {
            const char* name = metaRig->boneNames[i].ToString();
            out << i << "\t" << (name ? name : "<null>") << "\n";
        }
    }
    if (aOut) *aOut = static_cast<int32_t>(n);
}

// VR Transform data from Lua (Camera and Player Model Space)




volatile float g_VRPlayerYaw = 0.0f;

volatile float g_VRCamI = 0.0f;
volatile float g_VRCamJ = 0.0f;
volatile float g_VRCamK = 0.0f;
volatile float g_VRCamR = 1.0f;

// FPP camera (HMD) world position + player entity world position, pushed from Lua each
// frame (init.lua getCameraWorldPose + player:GetWorldPosition). The full-arm IK converts
// the gizmo's WORLD hand target into the bone buffer's MODEL space using these, so the
// hand lands exactly on the gizmo. See VRIK camModel block in vrik_hook.h.
volatile float g_VRCamPosX = 0.0f, g_VRCamPosY = 0.0f, g_VRCamPosZ = 0.0f;
volatile float g_VREntityPosX = 0.0f, g_VREntityPosY = 0.0f, g_VREntityPosZ = 0.0f;
volatile int   g_VRCamPosValid = 0;   // 0 until Lua has pushed a camera/entity pose
// Player entity world ORIENTATION quaternion (i,j,k,r). The world->model rotation is its
// conjugate; the full-arm IK uses it to convert the gizmo world target into model space.
// GetWorldOrientation().yaw was nil (silently 0), so we now take the real quaternion.
volatile float g_VREntityQI = 0.0f, g_VREntityQJ = 0.0f, g_VREntityQK = 0.0f, g_VREntityQR = 1.0f;

void SetVRPlayerYaw(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);

    float pYaw = 0.0f;
    float ci = 0.0f, cj = 0.0f, ck = 0.0f, cr = 1.0f;
    float camX = 0.0f, camY = 0.0f, camZ = 0.0f;   // FPP camera (HMD) world position
    float entX = 0.0f, entY = 0.0f, entZ = 0.0f;   // player entity world position
    float eqi = 0.0f, eqj = 0.0f, eqk = 0.0f, eqr = 1.0f; // entity world orientation quaternion

    RED4ext::GetParameter(aFrame, &pYaw);
    RED4ext::GetParameter(aFrame, &ci);
    RED4ext::GetParameter(aFrame, &cj);
    RED4ext::GetParameter(aFrame, &ck);
    RED4ext::GetParameter(aFrame, &cr);
    RED4ext::GetParameter(aFrame, &camX);
    RED4ext::GetParameter(aFrame, &camY);
    RED4ext::GetParameter(aFrame, &camZ);
    RED4ext::GetParameter(aFrame, &entX);
    RED4ext::GetParameter(aFrame, &entY);
    RED4ext::GetParameter(aFrame, &entZ);
    RED4ext::GetParameter(aFrame, &eqi);
    RED4ext::GetParameter(aFrame, &eqj);
    RED4ext::GetParameter(aFrame, &eqk);
    RED4ext::GetParameter(aFrame, &eqr);
    aFrame->code++;

    g_VRPlayerYaw = pYaw;
    // FPP camera (HMD) world quaternion -- used by the full-arm IK to place the
    // hand target in world space (world->model via -yaw), so head turns don't drag it.
    g_VRCamI = ci; g_VRCamJ = cj; g_VRCamK = ck; g_VRCamR = cr;
    // Camera (HMD) + entity world position -> lets the IK convert the gizmo world target
    // into model space (camModelPos = Rz(-yaw)*(camPos - entityPos)). The legacy quat-only
    // call (5 params) leaves these at 0 and g_VRCamPosValid stays 0 -> IK falls back to the
    // head-relative path.
    g_VRCamPosX = camX; g_VRCamPosY = camY; g_VRCamPosZ = camZ;
    g_VREntityPosX = entX; g_VREntityPosY = entY; g_VREntityPosZ = entZ;
    g_VREntityQI = eqi; g_VREntityQJ = eqj; g_VREntityQK = eqk; g_VREntityQR = eqr;
    g_VRCamPosValid = 1;

    if (aOut) *aOut = 1;
}

void SetVRBindMode(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    int32_t mode = 0;
    RED4ext::GetParameter(aFrame, &mode);
    aFrame->code++;
    g_VRBind = mode;
    if (aOut) *aOut = 1;
}

// Per-hand reach scale + position offset. hand: 0 = right, 1 = left, else = both.
// Also keeps the legacy global scale/offset (modes 1..3) in sync when hand == both.
void SetVRBindParams(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float scale = 1.0f, x = 0.0f, y = 0.0f, z = 0.0f;
    int32_t axis = 1, hand = 2;
    RED4ext::GetParameter(aFrame, &scale);
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    RED4ext::GetParameter(aFrame, &axis);
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;

    g_VRBindAxis = axis;
    if (hand != 1) { g_VRScaleR = scale; g_VROffRX = x; g_VROffRY = y; g_VROffRZ = z; }
    if (hand != 0) { g_VRScaleL = scale; g_VROffLX = x; g_VROffLY = y; g_VROffLZ = z; }
    if (hand == 2) { // also drive legacy globals used by modes 1..3
        g_VRBindScale = scale; g_VRBindOffX = x; g_VRBindOffY = y; g_VRBindOffZ = z;
    }

    if (aOut) *aOut = 1;
}

// Per-hand elbow pole spin in degrees (rotates the IK elbow direction around the
// shoulder->hand axis). hand: 0 = right, 1 = left, else = both.
void SetVRElbowPole(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float angle = 0.0f;
    int32_t hand = 2;
    RED4ext::GetParameter(aFrame, &angle);
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;
    if (hand != 1) g_VRElbowPoleR = angle;
    if (hand != 0) g_VRElbowPoleL = angle;
    if (aOut) *aOut = 1;
}

// Per-hand elbow-swing gain. Scales the arm-swing position heuristic that swings the elbow
// as the hand sweeps through its arc. 1.0 = the faithful heuristic, 0 = elbow locked straight
// down, negative = swing the other way. hand: 0 = right, 1 = left, else = both.
void SetVRElbowSwing(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float gain = 1.0f;
    int32_t hand = 2;
    RED4ext::GetParameter(aFrame, &gain);
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;
    if (hand != 1) g_VRElbowSwingR = gain;
    if (hand != 0) g_VRElbowSwingL = gain;
    if (aOut) *aOut = 1;
}

// Constant wrist-orientation correction (degrees, hand-local pitch/yaw/roll) per hand.
// hand: 0 = right, 1 = left, anything else = both. Lets the user dial in the palm/finger
// alignment live from the CET console without a rebuild. Applied as handRot = mapQuat *
// wristCorr in VRIK_BuildHandTarget. Calibrated defaults: R(0,-90,0), L(-180,-90,0).
void SetVRHandOffset(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float pitch = 0.0f, yaw = 0.0f, roll = 0.0f;
    int32_t hand = 2; // default: both
    RED4ext::GetParameter(aFrame, &pitch);
    RED4ext::GetParameter(aFrame, &yaw);
    RED4ext::GetParameter(aFrame, &roll);
    RED4ext::GetParameter(aFrame, &hand);
    aFrame->code++;

    const float d2r = 0.01745329252f * 0.5f;
    float cp = std::cos(pitch*d2r), sp = std::sin(pitch*d2r);
    float cy = std::cos(yaw*d2r),   sy = std::sin(yaw*d2r);
    float cr = std::cos(roll*d2r),  sr = std::sin(roll*d2r);
    // XYZ (pitch about X, yaw about Y, roll about Z) intrinsic compose.
    float qi = sp*cy*cr + cp*sy*sr;
    float qj = cp*sy*cr - sp*cy*sr;
    float qk = cp*cy*sr + sp*sy*cr;
    float qr = cp*cy*cr - sp*sy*sr;

    if (hand != 1) { g_VRWristR_I = qi; g_VRWristR_J = qj; g_VRWristR_K = qk; g_VRWristR_R = qr; }
    if (hand != 0) { g_VRWristL_I = qi; g_VRWristL_J = qj; g_VRWristL_K = qk; g_VRWristL_R = qr; }

    if (aOut) *aOut = 1;
}

void SetVRBindBones(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t leftIdx = 23, rightIdx = 24;
    RED4ext::GetParameter(aFrame, &leftIdx);
    RED4ext::GetParameter(aFrame, &rightIdx);
    aFrame->code++;
    
    g_VRLeftBoneIdx = leftIdx;
    g_VRRightBoneIdx = rightIdx;
    
    if (aOut) *aOut = 1;
}

// Override the resolved head bone index (calibration). -1 disables head-relative.
void SetVRHeadBone(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t idx = -1;
    RED4ext::GetParameter(aFrame, &idx);
    aFrame->code++;
    g_VRHeadBoneIdx = idx;
    if (aOut) *aOut = g_VRHeadBoneIdx;
}

// Toggle head-relative hand composition (1 = on, 0 = write head-local offset directly).
void SetVRUseHeadRelative(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 1;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    g_VRUseHeadRelative = on ? 1 : 0;
    if (aOut) *aOut = g_VRUseHeadRelative;
}

void SetVRDiagCapture(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    int32_t on = 0;
    RED4ext::GetParameter(aFrame, &on);
    aFrame->code++;
    g_VRDiagCapture = on ? 1 : 0;
    if (aOut) *aOut = g_VRDiagCapture;
}

// Diagnostic: logs the gizmo-computed world target (camPos + camQuat*mapLocalPos)
// next to the actual character arm-bone poses captured from the live pose buffer
// (g_VRDiagBones, snapshotted pre-write by the hook when SetVRDiagCapture(1)).
// The decisive lines compare (bufHand - bufHead) against (gizmoWorld - camPos):
// if they match, the bone buffer is model-space and head-relative IK is valid.
// Call from Lua each frame (or on a hotkey) passing the FPP camera world pose.
// Core diag writer, callable without a script frame (also used by the F10-overlay
// trigger path). camPos may be 0 -- the decisive comparison lines (gizmoWorld-cam.pos
// and bufHand-bufHead) are independent of the camera's absolute position.
static void WriteVRDiagCore(float camX, float camY, float camZ,
                            float qi, float qj, float qk, float qr) {
    int32_t aOutLocal = 0; int32_t* aOut = &aOutLocal;
    EnsureSharedMemory();
    if (aOut) *aOut = 0;

    // Right-hand gizmo target, identical math to the CET gizmo (init.lua).
    float raw[3] = { 0, 0, 0 };
    if (g_pSharedHands) { raw[0] = g_pSharedHands[9]; raw[1] = g_pSharedHands[10]; raw[2] = g_pSharedHands[11]; }
    float local[3] = { raw[0], -raw[2], raw[1] };          // mapLocalPos: (x, -z, y)
    float camQuat[4] = { qi, qj, qk, qr };
    float worldOff[3];
    VRIK_QuatRotateVec(camQuat, local, worldOff);
    float gizmo[3] = { camX + worldOff[0], camY + worldOff[1], camZ + worldOff[2] };

    std::ofstream out(VRDiagPath("vrik_diag.txt"), std::ios::app);
    if (!out.is_open()) { if (aOut) *aOut = -1; return; }
    out << std::fixed << std::setprecision(4);
    out << "==== LogVRDiag ====\n";
    out << "cam.pos  = (" << camX << ", " << camY << ", " << camZ << ")\n";
    out << "cam.quat = (" << qi << ", " << qj << ", " << qk << ", " << qr << ")\n";
    out << "VR.rawR  = (" << raw[0] << ", " << raw[1] << ", " << raw[2] << ")\n";
    out << "gizmoWorld(R) = (" << gizmo[0] << ", " << gizmo[1] << ", " << gizmo[2] << ")\n";
    // HMD orientation rel to base (slots 16..19) + head-independent base-frame offset.
    if (g_pSharedHands) {
        const float* h = &g_pSharedHands[16];
        float baseOff[3] = {
            (h[3]*h[3]-h[0]*h[0]-h[1]*h[1]-h[2]*h[2])*raw[0] + 2.0f*(h[0]*h[1]-h[3]*h[2])*raw[1] + 2.0f*(h[0]*h[2]+h[3]*h[1])*raw[2],
            2.0f*(h[0]*h[1]+h[3]*h[2])*raw[0] + (h[3]*h[3]-h[0]*h[0]+h[1]*h[1]-h[2]*h[2])*raw[1] + 2.0f*(h[1]*h[2]-h[3]*h[0])*raw[2],
            2.0f*(h[0]*h[2]-h[3]*h[1])*raw[0] + 2.0f*(h[1]*h[2]+h[3]*h[0])*raw[1] + (h[3]*h[3]-h[0]*h[0]-h[1]*h[1]+h[2]*h[2])*raw[2],
        };
        out << "hmdRel   = (" << h[0] << ", " << h[1] << ", " << h[2] << ", " << h[3] << ")\n";
        out << "baseOff(hmdRel*raw) = (" << baseOff[0] << ", " << baseOff[1] << ", " << baseOff[2] << ")\n";
        out << "mapLocal(x,-z,y)    = (" << baseOff[0] << ", " << -baseOff[2] << ", " << baseOff[1] << ")\n";
    }
    out << "headIdx=" << g_VRHeadBoneIdx << " rightIdx=" << g_VRRightBoneIdx << " leftIdx=" << g_VRLeftBoneIdx
        << " diagCapture=" << g_VRDiagCapture << " lastBoneBuf=0x" << std::hex << g_AnimPoseLastBoneBuf << std::dec << "\n";

    struct NamedBone { int idx; const char* name; };
    const NamedBone bones[] = {
        {2, "Hips"}, {13, "Spine3"}, {16, "Neck"}, {22, "Head"},
        {15, "RightShoulder"}, {18, "RightArm"}, {21, "RightForeArm"}, {24, "RightHand"},
        {14, "LeftShoulder"},  {17, "LeftArm"},  {20, "LeftForeArm"},  {23, "LeftHand"},
    };
    for (const auto& b : bones) {
        if (b.idx < 0 || b.idx >= 32) continue;
        const float* d = &g_VRDiagBones[b.idx * 7];
        out << "bone[" << b.idx << "] " << b.name
            << " pos=(" << d[0] << ", " << d[1] << ", " << d[2] << ")"
            << " quat=(" << d[3] << ", " << d[4] << ", " << d[5] << ", " << d[6] << ")\n";
    }

    // Decisive comparison: model-space buffer => these two offsets match.
    const float* head = &g_VRDiagBones[22 * 7];
    const float* hand = &g_VRDiagBones[24 * 7];
    const float* sh   = &g_VRDiagBones[15 * 7];
    const float* fa   = &g_VRDiagBones[21 * 7];
    out << "bufHand24 - bufHead22 = (" << (hand[0]-head[0]) << ", " << (hand[1]-head[1]) << ", " << (hand[2]-head[2]) << ")\n";
    out << "gizmoWorld  - cam.pos = (" << (gizmo[0]-camX) << ", " << (gizmo[1]-camY) << ", " << (gizmo[2]-camZ) << ")\n";
    // Rest bone lengths (for the future IK): shoulder->forearm->hand.
    float l1 = std::sqrt((fa[0]-sh[0])*(fa[0]-sh[0]) + (fa[1]-sh[1])*(fa[1]-sh[1]) + (fa[2]-sh[2])*(fa[2]-sh[2]));
    float l2 = std::sqrt((hand[0]-fa[0])*(hand[0]-fa[0]) + (hand[1]-fa[1])*(hand[1]-fa[1]) + (hand[2]-fa[2])*(hand[2]-fa[2]));
    out << "restLen shoulder->forearm=" << l1 << " forearm->hand=" << l2 << "\n";

    // Full-IK (mode 4) intermediates from the last solve, in model space.
    out << "IK chain: rArm=" << g_VRRightUpperArmIdx << " rFore=" << g_VRRightForeArmIdx
        << " rHand=" << g_VRRightBoneIdx << " boneCount=" << g_VRBoneCount
        << " bind=" << g_VRBind << "\n";
    out << "IK target(model)   = (" << g_VRIKDbgTarget[0] << ", " << g_VRIKDbgTarget[1] << ", " << g_VRIKDbgTarget[2] << ")\n";
    out << "IK shoulder(model) = (" << g_VRIKDbgShoulder[0] << ", " << g_VRIKDbgShoulder[1] << ", " << g_VRIKDbgShoulder[2] << ")\n";
    out << "IK elbow(model)    = (" << g_VRIKDbgElbow[0] << ", " << g_VRIKDbgElbow[1] << ", " << g_VRIKDbgElbow[2] << ")\n";
    out << "IK hand body(lx,ly,lz,cross) = (" << g_VRIKDbgLocal[0] << ", " << g_VRIKDbgLocal[1] << ", " << g_VRIKDbgLocal[2] << ", " << g_VRIKDbgLocal[3] << ")\n";
    out << "IK lens upper=" << g_VRIKDbgLens[0] << " fore=" << g_VRIKDbgLens[1]
        << " scale=" << g_VRBindScale << " yaw=" << g_VRPlayerYaw << "\n";

    // ---- Phase-1 gate: gizmo-exact 1:1 validation -------------------------------
    // Reconstruct the camera (HMD) + the right gizmo hand in MODEL space from the world
    // transforms (world->model = Rz(-yaw)) and compare to what the IK actually used:
    //   * camModel  ~= head bone model pos  (small, ~constant eye offset) -> world->model OK.
    //   * gizmoModel ~= IK target(model)     (~0)                          -> 1:1 target OK.
    // Both require the 11-param SetVRPlayerYaw push (g_VRCamPosValid=1).
    {
        // Use the REAL camera/entity transforms the hook uses (the cam.pos params above are 0
        // when the diag is triggered from the overlay path). world->model = conj(entityQuat).
        float entQ[4] = { g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR };
        float en = std::sqrt(entQ[0]*entQ[0]+entQ[1]*entQ[1]+entQ[2]*entQ[2]+entQ[3]*entQ[3]);
        if (en > 1e-4f) { entQ[0]/=en; entQ[1]/=en; entQ[2]/=en; entQ[3]/=en; } else { entQ[0]=0;entQ[1]=0;entQ[2]=0;entQ[3]=1; }
        float invEnt[4] = { -entQ[0], -entQ[1], -entQ[2], entQ[3] };
        auto toModel = [&](float x, float y, float z, float* o) {
            float v[3] = { x, y, z }; VRIK_QuatRotateVec(invEnt, v, o);
        };
        // Real-world right gizmo hand from the hook's camera pose (local = mapLocal(raw), above).
        float camQ2[4] = { g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR };
        float woff[3]; VRIK_QuatRotateVec(camQ2, local, woff);
        float gizR[3] = { g_VRCamPosX + woff[0], g_VRCamPosY + woff[1], g_VRCamPosZ + woff[2] };
        float camModelP[3];  toModel(g_VRCamPosX - g_VREntityPosX, g_VRCamPosY - g_VREntityPosY, g_VRCamPosZ - g_VREntityPosZ, camModelP);
        float gizmoModel[3]; toModel(gizR[0]-g_VREntityPosX, gizR[1]-g_VREntityPosY, gizR[2]-g_VREntityPosZ, gizmoModel);
        out << "camPosValid=" << g_VRCamPosValid
            << " camPos=(" << g_VRCamPosX << ", " << g_VRCamPosY << ", " << g_VRCamPosZ << ")"
            << " entityPos=(" << g_VREntityPosX << ", " << g_VREntityPosY << ", " << g_VREntityPosZ << ")\n";
        out << "entityQuat = (" << g_VREntityQI << ", " << g_VREntityQJ << ", " << g_VREntityQK << ", " << g_VREntityQR << ")\n";
        out << "camModel(reconstructed) = (" << camModelP[0] << ", " << camModelP[1] << ", " << camModelP[2] << ")\n";
        if (g_VRHeadBoneIdx >= 0 && g_VRHeadBoneIdx < 256) {
            const float* hp = g_fkPos[g_VRHeadBoneIdx];
            out << "head bone(model FK)     = (" << hp[0] << ", " << hp[1] << ", " << hp[2] << ")"
                << "  delta camModel-head = (" << (camModelP[0]-hp[0]) << ", " << (camModelP[1]-hp[1]) << ", " << (camModelP[2]-hp[2]) << ")\n";
        }
        out << "gizmoModel(reconstructed) = (" << gizmoModel[0] << ", " << gizmoModel[1] << ", " << gizmoModel[2] << ")\n";
        out << "GATE delta gizmoModel - IKtarget = (" << (gizmoModel[0]-g_VRIKDbgTarget[0]) << ", "
            << (gizmoModel[1]-g_VRIKDbgTarget[1]) << ", " << (gizmoModel[2]-g_VRIKDbgTarget[2]) << ")  (want ~0)\n";
        out << "userArmLen R/L=" << g_VRUserArmLenR << "/" << g_VRUserArmLenL
            << " eyeHeight=" << g_VRUserEyeHeight << "\n";
        out << "bodyUnderHMD=" << g_VRBodyUnderHMD << " chestDrop=" << g_VRChestDrop
            << " chestFwd=" << g_VRChestFwd << "\n";
        out << "chest target(model) = (" << g_VRIKDbgChestTgt[0] << ", " << g_VRIKDbgChestTgt[1] << ", " << g_VRIKDbgChestTgt[2] << ")\n";
        out << "chest actual(model) = (" << g_VRIKDbgChest[0] << ", " << g_VRIKDbgChest[1] << ", " << g_VRIKDbgChest[2] << ")\n";

        // Lower-body resolve + FK positions (model space, post-solve). If any reads UNRESOLVED the
        // bone name didn't match -> leg IK never runs -> squat can't bend the knees.
        out << "lowerbody idx: hips=" << g_VRHipsIdx
            << " rUpLeg=" << g_VRRightUpLegIdx << " rLeg=" << g_VRRightLegIdx << " rFoot=" << g_VRRightFootIdx
            << " lUpLeg=" << g_VRLeftUpLegIdx << " lLeg=" << g_VRLeftLegIdx << " lFoot=" << g_VRLeftFootIdx
            << " neck=" << g_VRNeckIdx << "\n";
        auto pfk = [&](int idx, const char* nm) {
            if (idx >= 0 && idx < 256)
                out << "  " << nm << "[" << idx << "] fk=(" << g_fkPos[idx][0] << ", " << g_fkPos[idx][1] << ", " << g_fkPos[idx][2] << ")\n";
            else
                out << "  " << nm << " = UNRESOLVED\n";
        };
        pfk(g_VRHipsIdx, "Hips"); pfk(g_VRRightUpLegIdx, "RUpLeg"); pfk(g_VRRightLegIdx, "RLeg"); pfk(g_VRRightFootIdx, "RFoot");
        pfk(g_VRLeftUpLegIdx, "LUpLeg"); pfk(g_VRLeftLegIdx, "LLeg"); pfk(g_VRLeftFootIdx, "LFoot");

        // Current avatar arm length in FK (post-scale) vs the target userArmLen -- if these don't
        // match, the bicep/forearm scaling isn't reaching the user's real arm length.
        auto fkArm = [&](int up, int fore, int hand) -> float {
            if (up<0||fore<0||hand<0||up>=256||fore>=256||hand>=256) return 0.0f;
            auto d=[&](int a,int b){ float dx=g_fkPos[a][0]-g_fkPos[b][0],dy=g_fkPos[a][1]-g_fkPos[b][1],dz=g_fkPos[a][2]-g_fkPos[b][2]; return std::sqrt(dx*dx+dy*dy+dz*dz); };
            return d(fore,up)+d(hand,fore);
        };
        auto fkLeg = [&](int up, int knee, int foot) -> float {
            if (up<0||knee<0||foot<0||up>=256||knee>=256||foot>=256) return 0.0f;
            auto d=[&](int a,int b){ float dx=g_fkPos[a][0]-g_fkPos[b][0],dy=g_fkPos[a][1]-g_fkPos[b][1],dz=g_fkPos[a][2]-g_fkPos[b][2]; return std::sqrt(dx*dx+dy*dy+dz*dz); };
            return d(knee,up)+d(foot,knee);
        };
        out << "avatar arm FK R/L = " << fkArm(g_VRRightUpperArmIdx,g_VRRightForeArmIdx,g_VRRightBoneIdx)
            << "/" << fkArm(g_VRLeftUpperArmIdx,g_VRLeftForeArmIdx,g_VRLeftBoneIdx)
            << "  target userArmLen=" << g_VRUserArmLenR << "/" << g_VRUserArmLenL << "\n";
        out << "avatar leg FK R/L = " << fkLeg(g_VRRightUpLegIdx,g_VRRightLegIdx,g_VRRightFootIdx)
            << "/" << fkLeg(g_VRLeftUpLegIdx,g_VRLeftLegIdx,g_VRLeftFootIdx) << "\n";
    }
    out << "LK target(model)   = (" << g_VRIKDbgTargetL[0] << ", " << g_VRIKDbgTargetL[1] << ", " << g_VRIKDbgTargetL[2] << ")\n";
    out << "LK shoulder(model) = (" << g_VRIKDbgShoulderL[0] << ", " << g_VRIKDbgShoulderL[1] << ", " << g_VRIKDbgShoulderL[2] << ")\n";
    out << "LK elbow(model)    = (" << g_VRIKDbgElbowL[0] << ", " << g_VRIKDbgElbowL[1] << ", " << g_VRIKDbgElbowL[2] << ")\n";
    out << "LK hand body(lx,ly,lz,cross) = (" << g_VRIKDbgLocalL[0] << ", " << g_VRIKDbgLocalL[1] << ", " << g_VRIKDbgLocalL[2] << ", " << g_VRIKDbgLocalL[3] << ")\n";
    out << "LK lens upper=" << g_VRIKDbgLensL[0] << " fore=" << g_VRIKDbgLensL[1] << "\n\n";

    if (aOut) *aOut = 1;
}

// Script-callable thin wrapper: reads the FPP camera world pose from the frame
// and forwards to WriteVRDiagCore. Same entry as the CET "Log VR Diag" button.
void LogVRDiag(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    float camX = 0, camY = 0, camZ = 0, qi = 0, qj = 0, qk = 0, qr = 1;
    RED4ext::GetParameter(aFrame, &camX);
    RED4ext::GetParameter(aFrame, &camY);
    RED4ext::GetParameter(aFrame, &camZ);
    RED4ext::GetParameter(aFrame, &qi);
    RED4ext::GetParameter(aFrame, &qj);
    RED4ext::GetParameter(aFrame, &qk);
    RED4ext::GetParameter(aFrame, &qr);
    aFrame->code++;
    WriteVRDiagCore(camX, camY, camZ, qi, qj, qk, qr);
    if (aOut) *aOut = 1;
}

// Dump VTable specifically for entAnimatedComponent
void DumpAnimVTable(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, void* aOut, int64_t a4) {
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(aOut); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    
    std::ofstream out(VRDiagPath("anim_vtable_dump.txt"), std::ios::trunc);
    
    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle) { out << "No player\n"; return; }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity) { out << "No player entity\n"; return; }

    for (auto& componentHandle : playerEntity->components) {
        auto* component = componentHandle.instance;
        if (!component) continue;
        
        RED4ext::CClass* type = component->GetType();
        if (type && type->name == "entAnimatedComponent") {
            out << "Found entAnimatedComponent at: " << std::hex << (uintptr_t)component << "\n";
            
            uintptr_t* vtable = *reinterpret_cast<uintptr_t**>(component);
            out << "VTable Address: " << std::hex << (uintptr_t)vtable << "\n";
            
            HMODULE hMod = GetModuleHandleA("Cyberpunk2077.exe");
            uintptr_t base = (uintptr_t)hMod;
            
            for(int i = 0; i < 60; ++i) { // dump 60 just in case
                uintptr_t func = vtable[i];
                out << "  [" << std::dec << i << std::hex << "] Func: " << func << " (Cyberpunk2077.exe+" << (func - base) << ")\n";
            }
            break; 
        }
    }
    out.close();
}

void DumpAnimControllerComponents(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    RED4ext::ScriptGameInstance gameInstance;
    RED4ext::Handle<RED4ext::IScriptable> playerHandle;
    RED4ext::ExecuteGlobalFunction("GetPlayer;GameInstance", &playerHandle, gameInstance);
    if (!playerHandle)
    {
        if (aOut) *aOut = -1;
        return;
    }

    auto* playerEntity = reinterpret_cast<RED4ext::ent::Entity*>(playerHandle.instance);
    if (!playerEntity)
    {
        if (aOut) *aOut = -2;
        return;
    }

    std::ofstream out(VRDiagPath("anim_controller_dump.txt"), std::ios::trunc);
    int dumped = 0;

    for (auto& componentHandle : playerEntity->components)
    {
        auto* component = componentHandle.instance;
        if (!component)
            continue;

        RED4ext::CClass* type = component->GetType();
        if (!type)
            continue;

        if (type->name == "entAnimationControllerComponent")
        {
            auto* controller = reinterpret_cast<RED4ext::ent::AnimationControllerComponent*>(component);
            out << "==================================================\n";
            out << "AnimationControllerComponent ptr=0x" << std::hex << reinterpret_cast<uintptr_t>(controller) << std::dec << "\n";
            out << "name=" << component->name.ToString() << " enabled=" << (component->isEnabled ? 1 : 0) << "\n";
            out << "lookAtController ptr(backref)=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->lookAtController.animationControllerComponent) << std::dec << "\n";
            out << "ikTargetController ptr(backref)=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.animationControllerComponent) << std::dec << "\n";
            out << "ikTargetController.targetData.size=" << controller->ikTargetController.targetData.Size() << "\n";

            for (uint32_t i = 0; i < controller->ikTargetController.targetData.Size(); ++i)
            {
                const auto& target = controller->ikTargetController.targetData[i];
                out << "  [" << i << "] id=" << target.targetReference.id
                    << " part=" << target.targetReference.part.ToString()
                    << " posProvider=0x" << std::hex << reinterpret_cast<uintptr_t>(target.positionProvider.instance)
                    << " orientProvider=0x" << reinterpret_cast<uintptr_t>(target.orientationProvider.instance)
                    << std::dec << "\n";
            }

            out << "ikParams=0x" << std::hex << reinterpret_cast<uintptr_t>(controller->ikTargetController.ikParams.instance) << std::dec << "\n";
            ++dumped;
        }
    }

    out.close();
    if (aOut) *aOut = dumped;
}

void DumpRuntimeClassFunctions(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext);
    RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;

    auto* rtti = RED4ext::CRTTISystem::Get();
    std::ofstream out(VRDiagPath("runtime_class_functions.txt"), std::ios::trunc);
    int dumped = 0;

    const char* classesToDump[] = {
        "entAnimationControllerComponent",
        "entEntity",
        "entAnimatedComponent"
    };

    for (const char* className : classesToDump)
    {
        RED4ext::CClass* cls = rtti->GetClass(className);
        if (!cls)
            continue;

        out << "==================================================\n";
        out << "CLASS " << className << "\n";
        out << "==================================================\n";

        out << "Member functions:\n";
        for (auto* func : cls->funcs)
        {
            if (!func)
                continue;
            out << "  - " << func->fullName.ToString() << "\n";
        }

        out << "Static functions:\n";
        for (auto* func : cls->staticFuncs)
        {
            if (!func)
                continue;
            out << "  - " << func->fullName.ToString() << "\n";
        }

        out << "\n";
        ++dumped;
    }

    out.close();
    if (aOut) *aOut = dumped;
}

// ============================================================================
// PROJECTILE-AIM RTTI ENUMERATOR (2026-06-15). Dumps the exact native classes /
// methods / properties of the projectile launch + orientation-provider chain, so we
// hook/swap the RIGHT thing by name (the "beat the registrar wall via RTTI" plan).
// Output: vr_projectile_rtti.txt. Trigger: native DumpVRProjectileRtti() (CET button).
// ============================================================================
static void DumpClassDetail(std::ofstream& out, RED4ext::CRTTISystem* rtti, const char* className)
{
    RED4ext::CClass* cls = rtti->GetClass(className);
    out << "\n==================================================\n";
    out << "CLASS " << className << (cls ? "" : "   <NOT FOUND>") << "\n";
    if (!cls) return;
    // parent chain
    out << "  parents:";
    for (RED4ext::CClass* p = cls->parent; p; p = p->parent) out << " " << p->name.ToString();
    out << "\n  size=0x" << std::hex << cls->GetSize() << std::dec << "\n";

    out << "  -- properties (name : type @offset) --\n";
    RED4ext::DynArray<RED4ext::CProperty*> props;
    cls->GetProperties(props);
    for (uint32_t i = 0; i < props.Size(); ++i) {
        auto* prop = props[i];
        if (!prop) continue;
        out << "    +0x" << std::hex << prop->valueOffset << std::dec
            << "  " << prop->name.ToString() << " : " << GetTypeNameForDump(prop->type) << "\n";
    }
    out << "  -- functions (N=native, E=event) --\n";
    for (auto* func : cls->funcs) {
        if (!func) continue;
        out << "    [" << (func->flags.isNative ? "N" : "s") << (func->flags.isEvent ? "E" : " ")
            << "] " << func->fullName.ToString();
        if (func->returnType && func->returnType->type)
            out << " -> " << GetTypeNameForDump(func->returnType->type);
        out << "  (" << func->params.Size() << " params)\n";
    }
    for (auto* func : cls->staticFuncs) {
        if (!func) continue;
        out << "    [static] " << func->fullName.ToString() << "\n";
    }
}

void DumpVRProjectileRtti(RED4ext::IScriptable* aContext, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t a4)
{
    RED4EXT_UNUSED_PARAMETER(aContext); RED4EXT_UNUSED_PARAMETER(a4);
    aFrame->code++;
    auto* rtti = RED4ext::CRTTISystem::Get();
    if (!rtti) { if (aOut) *aOut = -1; return; }
    std::ofstream out(VRDiagPath("vr_projectile_rtti.txt"), std::ios::trunc);

    // 1) ALL orientation/position provider classes (derived from the interfaces) =
    //    candidates to instantiate (entStaticOrientationProvider) or swap into launch params.
    const char* providerBases[] = { "entIOrientationProvider", "entIPositionProvider",
                                    "gameIOrientationProvider", "gameIPositionProvider" };
    for (const char* base : providerBases) {
        auto* baseCls = rtti->GetClass(base);
        out << "\n#### derived of " << base << (baseCls ? "" : " <NOT FOUND>") << " ####\n";
        if (!baseCls) continue;
        RED4ext::DynArray<RED4ext::CClass*> derived;
        rtti->GetDerivedClasses(baseCls, derived);
        for (uint32_t i = 0; i < derived.Size(); ++i)
            if (derived[i]) out << "  - " << derived[i]->name.ToString() << "\n";
    }

    // 2) the projectile launch / attack / event chain — full detail (props + native methods).
    const char* classes[] = {
        "gameAttack_Projectile", "gameIAttack", "gamedataAttack_Projectile_Record",
        "gameprojectileObject", "gameprojectileComponent", "gameprojectileSpawnerComponent",
        "gameprojectileLauncherComponent",
        "gameprojectileShootEvent", "gameprojectileSetUpEvent", "gameprojectileSetUpAndLaunchEvent",
        "gameprojectileLaunchEvent", "gameprojectileLaunchParams", "gameprojectileWeaponParams",
        "gameprojectileTrajectoryParams", "gameprojectileLinearTrajectoryParams",
        "entStaticOrientationProvider", "entStaticPositionProvider",
        "entEntityOrientationProvider", "entEntityPositionProvider",
        "gameuiWeaponShootParams", "gameTargetingSystem",
    };
    for (const char* c : classes) DumpClassDetail(out, rtti, c);

    out.close();
    if (aOut) *aOut = 1;
}

// ============================================================================
// PrepareAttack HOOK — projectile launch direction lever.
// gameAttack_Projectile::PrepareAttack builds the launch event
// whose launchParams.logicalOrientationProvider sets the projectile direction (reads the camera for
// the player). We instrument it (read-only: auto-detect the event base + the OrientationProvider
// handle offset by RTTI type name), then optionally SWAP that provider to a controller-aimed
// entStaticOrientationProvider so the bullet flies down the barrel. All derefs are SEH-guarded.
// ============================================================================
static constexpr uintptr_t kPrepareAttackOffset = 0x1D912B0;
typedef uintptr_t (*PaFn)(uintptr_t, uintptr_t);
static PaFn OrigPA = nullptr;

static void PaSafeTypeName(uintptr_t p, char* out, size_t n) {
    out[0] = 0;
    if (p < 0x100000) return;
    __try {
        auto* obj = reinterpret_cast<RED4ext::IScriptable*>(p);
        auto* t = obj->GetType();
        if (t) { const char* s = t->name.ToString(); if (s) strncpy_s(out, n, s, _TRUNCATE); }
    } __except (EXCEPTION_EXECUTE_HANDLER) { out[0] = 0; }
}
static bool PaScanForProvider(uintptr_t base, int& outOff, char* typeOut, size_t typeN, volatile uint64_t* qdump) {
    bool found = false;
    __try {
        for (uint32_t o = 0; o < 0xC0; o += 8) {
            uintptr_t q = *reinterpret_cast<uintptr_t*>(base + o);
            if (qdump) qdump[o / 8] = q;
            if (q > 0x100000 && q < 0x7FFFFFFFFFFFull) {
                char tn[96]; PaSafeTypeName(q, tn, sizeof(tn));
                if (tn[0] && strstr(tn, "OrientationProvider")) {
                    outOff = (int)o; if (typeOut) strncpy_s(typeOut, typeN, tn, _TRUNCATE); found = true;
                }
            }
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return found;
}

// Swap the launch event's logical (+visual) orientation provider to a controller-aimed static
// provider. Separate fn (no __try) because RED4ext::Handle has a destructor (can't unwind under SEH).
static void PaSwapProvider(uintptr_t evt, int off, const RED4ext::Quaternion& q) {
    auto hLog = CreateStaticOrientationProviderQ(q);
    if (!hLog.instance) return;
    *reinterpret_cast<RED4ext::Handle<RED4ext::ent::IOrientationProvider>*>(evt + off) = hLog;
    auto hVis = CreateStaticOrientationProviderQ(q);   // launchParams: logical@+0x18 visual@+0x38 (=+0x20)
    if (hVis.instance)
        *reinterpret_cast<RED4ext::Handle<RED4ext::ent::IOrientationProvider>*>(evt + off + 0x20) = hVis;
    g_paSwaps++;
}

extern "C" uintptr_t Hooked_PrepareAttack(uintptr_t a1, uintptr_t a2) {
    uintptr_t r = OrigPA ? OrigPA(a1, a2) : 0;
    if (!g_paOn) return r;
    g_paCalls++; g_paA1 = a1; g_paA2 = a2; g_paRet = r;
    PaSafeTypeName(r, (char*)g_paRetType, sizeof(g_paRetType));

    // Identify the launch-event base + the OrientationProvider handle offset. The PrepareAttack ABI
    // may return the event by ptr (r), by hidden-ret (r -> &Handle -> *r = event), or as 'this' (a2).
    uintptr_t cand[3] = { r, 0, a2 };
    __try { if (r > 0x100000) cand[1] = *reinterpret_cast<uintptr_t*>(r); } __except (EXCEPTION_EXECUTE_HANDLER) { cand[1] = 0; }
    int base = -1, off = -1;
    for (int b = 0; b < 3; ++b) {
        if (cand[b] < 0x100000) continue;
        int o = -1; char tn[96] = {0};
        if (PaScanForProvider(cand[b], o, tn, sizeof(tn), (b == 0 ? g_paEvQ : nullptr))) {
            base = b; off = o; strncpy_s((char*)g_paProvType, sizeof(g_paProvType), tn, _TRUNCATE);
            // dump the winning base's qwords
            __try { for (uint32_t k = 0; k < 0xC0; k += 8) g_paEvQ[k/8] = *reinterpret_cast<uintptr_t*>(cand[b] + k); } __except (EXCEPTION_EXECUTE_HANDLER) {}
            break;
        }
    }
    if (base >= 0) { g_paProvBase = base; g_paProvOff = off; }

    // SWAP: replace the detected logical (and visual) orientation provider with a controller static
    // provider so the launch aims down the barrel. Guarded: only on a confirmed detection.
    if (g_paSwap && g_paProvBase >= 0 && g_paProvOff >= 0 && g_pSharedHands) {
        RED4ext::Quaternion q;
        q.i = g_pSharedHands[53]; q.j = g_pSharedHands[54]; q.k = g_pSharedHands[55]; q.r = g_pSharedHands[56];
        const float l2 = q.i*q.i + q.j*q.j + q.k*q.k + q.r*q.r;
        if (l2 > 0.5f && l2 < 2.0f) {
            uintptr_t evt = (g_paProvBase == 0) ? r : (g_paProvBase == 1 ? cand[1] : a2);
            PaSwapProvider(evt, g_paProvOff, q);
        }
    }
    return r;
}

void InstallVRPrepareAttack(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    if (g_paInstalled) { if (aOut) *aOut = 2; return; }
    MH_Initialize();
    const uintptr_t modBase = reinterpret_cast<uintptr_t>(GetModuleHandleA("Cyberpunk2077.exe"));
    if (!modBase) { if (aOut) *aOut = -1; return; }

    // The engine calls PrepareAttack as a direct C++ virtual,
    // so resolve the concrete instance-vtable impl: create a throwaway gameAttack_Projectile, read
    // *(inst) = vtable, [vtable + 0x168] = PrepareAttack impl, and hook THAT address.
    void* tgt = nullptr;
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("gameAttack_Projectile") : nullptr;
    if (cls) {
        void* inst = cls->CreateInstance(true);
        if (inst) {
            __try {
                uintptr_t vt = *reinterpret_cast<uintptr_t*>(inst);
                if (vt >= modBase && vt < modBase + 0x10000000) {
                    uintptr_t impl = *reinterpret_cast<uintptr_t*>(vt + 0x168);
                    if (impl >= modBase && impl < modBase + 0x10000000) {
                        g_paImpl = impl;
                        tgt = reinterpret_cast<void*>(impl);
                    }
                }
            } __except (EXCEPTION_EXECUTE_HANDLER) { tgt = nullptr; }
            // leak the throwaway instance (one-time, tiny) — safer than guessing the free path.
        }
    }
    // Fallback: hook the RTTI thunk (only catches script invocations).
    if (!tgt) tgt = reinterpret_cast<void*>(modBase + kPrepareAttackOffset);

    bool ok = (MH_CreateHook(tgt, reinterpret_cast<void*>(&Hooked_PrepareAttack), reinterpret_cast<void**>(&OrigPA)) == MH_OK)
           && (MH_EnableHook(tgt) == MH_OK);
    g_paInstalled = ok ? 1 : 0;
    if (aOut) *aOut = ok ? (g_paImpl ? 1 : 3) : 0;   // 1=hooked impl, 3=hooked thunk fallback
}
void SetVRPrepareAttackSwap(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t on = 0; RED4ext::GetParameter(aFrame, &on); aFrame->code++;
    g_paSwap = on;
}
void GetVRPADump(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t) {
    int32_t idx = 0; RED4ext::GetParameter(aFrame, &idx); aFrame->code++;
    double v = 0.0;
    switch (idx) {
        case 0: v = (double)g_paCalls; break;
        case 1: v = (double)g_paSwaps; break;
        case 2: v = (double)g_paInstalled; break;
        case 3: v = (double)g_paProvBase; break;
        case 4: v = (double)g_paProvOff; break;
        case 5: v = (double)g_paSwap; break;
        case 6: v = (g_paImpl != 0) ? 1.0 : 0.0; break;  // 1 = hooked the real instance-vtable impl
        default:
            if (idx >= 10 && idx < 34) v = (double)g_paEvQ[idx - 10];
            break;
    }
    if (aOut) *aOut = (float)v;
}

// ============================================================================
// LIVE PROJECTILE finder + CE-target dump + orientation steer test.
// The projectile launch is native (no script-wrapper hook works). So instead: find the LIVE
// gameprojectileComponent in memory (by its instance vtable), report the absolute address of its
// worldTransform.Orientation (+0xe0) so CE "find out what writes" pinpoints the native fn that sets
// the launch direction; and a steer-test that overwrites that orientation with the controller aim.
// ============================================================================
static uintptr_t ResolveProjCompVtable() {
    if (g_projCompVtbl) return g_projCompVtbl;
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass("gameprojectileComponent") : nullptr;
    if (!cls) return 0;
    void* inst = cls->CreateInstance(true);  // leak (one-time) — just need the vtable
    if (!inst) return 0;
    __try { g_projCompVtbl = *reinterpret_cast<uintptr_t*>(inst); } __except (EXCEPTION_EXECUTE_HANDLER) { g_projCompVtbl = 0; }
    return g_projCompVtbl;
}
volatile int g_projTotal = 0;   // total vtable matches (pooled + active)
volatile int g_projValid = 0;   // matches that pass RTTI GetType()=="gameprojectileComponent"

static uintptr_t SafeReadPtr(uintptr_t a) {
    __try { return *reinterpret_cast<uintptr_t*>(a); } __except (EXCEPTION_EXECUTE_HANDLER) { return 0; }
}
// SEH-guarded readability check via VirtualQuery (avoids the AV that crashed the game when a garbage
// float like 0xF51C006E was treated as a pointer and dereferenced).
static bool IsReadable(uintptr_t a, size_t n) {
    if (a < 0x10000) return false;
    MEMORY_BASIC_INFORMATION mbi;
    if (!VirtualQuery(reinterpret_cast<void*>(a), &mbi, sizeof(mbi))) return false;
    if (mbi.State != MEM_COMMIT) return false;
    DWORD prot = mbi.Protect;
    if (prot & (PAGE_NOACCESS | PAGE_GUARD)) return false;
    if (!(prot & (PAGE_READONLY|PAGE_READWRITE|PAGE_EXECUTE_READ|PAGE_EXECUTE_READWRITE|PAGE_WRITECOPY|PAGE_EXECUTE_WRITECOPY))) return false;
    uintptr_t end = reinterpret_cast<uintptr_t>(mbi.BaseAddress) + mbi.RegionSize;
    return (a + n) <= end;
}
// Validate a candidate is a REAL gameprojectileComponent instance — STRUCTURALLY, with NO virtual
// calls (calling GetType() on a stack transient whose first qword == vtbl crashed the game). The
// component is 0x920 bytes (RTTI dump); a real heap instance has that fully committed, a stack/RTTI
// transient does not. Plus a couple of field-plausibility checks. All reads IsReadable/SEH-gated.
static bool IsProjComp(uintptr_t obj, uintptr_t vtbl) {
    if (!IsReadable(obj, 0x920)) return false;        // full component must be committed+readable
    if (SafeReadPtr(obj) != vtbl) return false;        // first qword == the component vtable
    // entIComponent fields: name(CName)@+0x40 nonzero; id(CRUID)@+0x60 nonzero on a constructed comp.
    uint64_t nm = SafeReadPtr(obj + 0x40);
    uint64_t id = SafeReadPtr(obj + 0x60);
    if (nm == 0 && id == 0) return false;              // unconstructed/pooled blank -> skip
    // a real component sits on a heap allocation, not a thread stack: stacks are tiny regions.
    MEMORY_BASIC_INFORMATION mbi;
    if (!VirtualQuery(reinterpret_cast<void*>(obj), &mbi, sizeof(mbi))) return false;
    if (mbi.RegionSize < 0x20000) return false;        // skip small (stack-like) regions
    return true;
}
// returns offset of a unit-quaternion in the transform region (=orientation), or -1 if none.
// An ACTIVE flying projectile has a valid unit quat; pooled/dormant ones are zeroed/garbage.
static int DetectOrientOffset(uintptr_t comp) {
    __try {
        for (uint32_t o = 0xB0; o <= 0x140; o += 4) {
            float* q = reinterpret_cast<float*>(comp + o);
            // reject NaN/inf and require near-unit magnitude with a non-trivial quat
            float m = q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3];
            if (m > 0.96f && m < 1.04f &&
                q[0] > -1.01f && q[0] < 1.01f && q[1] > -1.01f && q[1] < 1.01f &&
                q[2] > -1.01f && q[2] < 1.01f && q[3] > -1.01f && q[3] < 1.01f)
                return (int)o;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return -1;
}
static bool ProjIsActive(uintptr_t comp) { return DetectOrientOffset(comp) >= 0; }
static uintptr_t FindLiveProjectile(uintptr_t vtbl) {
    if (!vtbl) return 0;
    // validate the cached one first — must still be a real projectile component
    if (g_projLive) {
        if (IsProjComp(g_projLive, vtbl) && ProjIsActive(g_projLive)) return g_projLive;
        g_projLive = 0;
    }
    int total = 0, valid = 0; uintptr_t firstValid = 0;
    MEMORY_BASIC_INFORMATION mbi;
    uintptr_t addr = 0;
    while (VirtualQuery(reinterpret_cast<void*>(addr), &mbi, sizeof(mbi))) {
        uintptr_t base = reinterpret_cast<uintptr_t>(mbi.BaseAddress);
        size_t sz = mbi.RegionSize ? mbi.RegionSize : 0x1000;
        // heap only: committed, private, plain RW, bounded — skips images and most stacks
        if (mbi.State == MEM_COMMIT && mbi.Type == MEM_PRIVATE && mbi.Protect == PAGE_READWRITE && sz <= 64ull*1024*1024) {
            __try {
                uintptr_t* p = reinterpret_cast<uintptr_t*>(base);
                const size_t n = sz / 8;
                for (size_t i = 0; i < n; ++i) {
                    if (p[i] == vtbl) {
                        ++total;
                        uintptr_t comp = base + i*8;
                        if (IsProjComp(comp, vtbl)) {          // RTTI-validated real instance
                            ++valid; if (!firstValid) firstValid = comp;
                            if (ProjIsActive(comp)) { g_projLive = comp; g_projTotal = total; g_projValid = valid; return comp; }
                        }
                    }
                }
            } __except (EXCEPTION_EXECUTE_HANDLER) {}
        }
        addr = base + sz;
        if (addr < base) break;
    }
    g_projTotal = total; g_projValid = valid;
    g_projLive = firstValid;   // a real (but maybe dormant) instance, or 0
    return firstValid;
}
// SEH-guarded raw reads (no C++ objects here, so __try is allowed). Fills the diag globals.
static void ReadProjFields(uintptr_t comp, int qoff) {
    __try {
        float* q = reinterpret_cast<float*>(comp + qoff);
        for (int i = 0; i < 4; ++i) g_projOrientQ[i] = q[i];
        for (uint32_t o = 0xC0; o < 0x160; o += 8)
            g_projDumpQ[(o - 0xC0) / 8] = *reinterpret_cast<uintptr_t*>(comp + o);
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}
// scan-and-collect RTTI-VALIDATED projectile component instances (no stack/RTTI-struct false positives)
static int CollectProjectiles(uintptr_t vtbl, uintptr_t* out, int maxN) {
    int cnt = 0, total = 0; if (!vtbl) return 0;
    MEMORY_BASIC_INFORMATION mbi; uintptr_t addr = 0;
    while (VirtualQuery(reinterpret_cast<void*>(addr), &mbi, sizeof(mbi))) {
        uintptr_t base = reinterpret_cast<uintptr_t>(mbi.BaseAddress);
        size_t sz = mbi.RegionSize ? mbi.RegionSize : 0x1000;
        if (mbi.State == MEM_COMMIT && mbi.Type == MEM_PRIVATE && mbi.Protect == PAGE_READWRITE && sz <= 64ull*1024*1024) {
            __try {
                uintptr_t* p = reinterpret_cast<uintptr_t*>(base); const size_t n = sz/8;
                for (size_t i = 0; i < n && cnt < maxN; ++i) {
                    if (p[i] == vtbl) { ++total; uintptr_t comp = base + i*8; if (IsProjComp(comp, vtbl)) out[cnt++] = comp; }
                }
            } __except (EXCEPTION_EXECUTE_HANDLER) {}
        }
        addr = base + sz; if (addr < base) break; if (cnt >= maxN) break;
    }
    g_projTotal = total; g_projValid = cnt;
    return cnt;
}
static void DumpRegion(std::ofstream& out, uintptr_t obj, uint32_t lo, uint32_t hi) {
    static float fbuf[128]; static uintptr_t qbuf[64];
    int nf = 0, nq = 0;
    __try {
        for (uint32_t o = lo; o < hi; o += 4) { if (nf < 128) fbuf[nf++] = *reinterpret_cast<float*>(obj + o); }
        for (uint32_t o = lo; o < hi; o += 8) { if (nq < 64) qbuf[nq++] = *reinterpret_cast<uintptr_t*>(obj + o); }
    } __except (EXCEPTION_EXECUTE_HANDLER) { return; }
    int fi = 0;
    for (uint32_t o = lo; o < hi; o += 8) {
        out << "    +0x" << std::hex << o << std::dec << "  q=0x" << std::hex << qbuf[(o-lo)/8] << std::dec
            << "  f=(" << fbuf[fi] << ", " << fbuf[fi+1] << ")\n";
        fi += 2;
    }
}
void DumpVRLiveProjectile(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    uintptr_t vtbl = ResolveProjCompVtable();
    uintptr_t matches[16];
    int total = CollectProjectiles(vtbl, matches, 16);
    g_projTotal = total;
    std::ofstream out(VRDiagPath("vr_live_projectile.txt"), std::ios::trunc);
    out << "gameprojectileComponent vtbl=0x" << std::hex << vtbl << std::dec
        << "  rawMatches=" << g_projTotal << "  RTTI-valid=" << total << "\n";
    int activeIdx = -1, activeOff = -1;
    for (int k = 0; k < total; ++k) {
        uintptr_t comp = matches[k];
        int qoff = DetectOrientOffset(comp);
        out << "\n==== match[" << k << "] = 0x" << std::hex << comp << std::dec
            << (qoff >= 0 ? "  ACTIVE (unit-quat @+0x" : "  (no unit-quat)") ;
        if (qoff >= 0) out << std::hex << qoff << std::dec << ")";
        out << "\n";
        DumpRegion(out, comp, 0xC0, 0x160);
        // follow heap pointers one level (parentTransform/binding/entity may hold the live transform)
        static const uint32_t pofs[] = { 0x90u, 0xd8u, 0xf8u, 0x138u, 0x150u };
        for (uint32_t po : pofs) {
            uintptr_t hp = SafeReadPtr(comp + po);
            if (IsReadable(hp, 0x60)) {   // only deref genuinely-committed memory (no AV like 0xF51C006E)
                out << "  -> [+0x" << std::hex << po << "] = 0x" << hp << std::dec << "  (deref +0x00..+0x60):\n";
                DumpRegion(out, hp, 0x00, 0x60);
            }
        }
        if (qoff >= 0 && activeIdx < 0) { activeIdx = k; activeOff = qoff; }
    }
    if (activeIdx >= 0) {
        g_projLive = matches[activeIdx]; g_projFound = 1;
        g_projOrientAddr = matches[activeIdx] + activeOff;
        ReadProjFields(matches[activeIdx], activeOff);
        out << "\n*** ACTIVE match[" << activeIdx << "]; CE 'find what writes' 0x" << std::hex << g_projOrientAddr << std::dec << " ***\n";
    } else {
        g_projFound = total > 0 ? 1 : 0; if (total) g_projLive = matches[0];
        out << "\n*** no ACTIVE projectile (unit-quat) found; inspect the heap-ptr derefs above for the live transform ***\n";
    }
    if (aOut) *aOut = (activeIdx >= 0) ? 1 : (total > 0 ? 2 : 0);
}
// per-frame steer (called from onUpdate via the CET pump): set the live projectile's orientation to
// the controller aim quat (shared[53..56]). Tests whether the trajectory re-reads orientation.
static void ProjSteerTick() {
    if (!g_projSteer || !g_pSharedHands) return;
    uintptr_t vtbl = g_projCompVtbl ? g_projCompVtbl : ResolveProjCompVtable();
    uintptr_t comp = FindLiveProjectile(vtbl);
    if (!comp) return;
    int qoff = DetectOrientOffset(comp);
    if (qoff < 0) return;   // not an active projectile (no unit-quat orientation)
    __try {
        float qx=g_pSharedHands[53], qy=g_pSharedHands[54], qz=g_pSharedHands[55], qw=g_pSharedHands[56];
        if (qx*qx+qy*qy+qz*qz+qw*qw > 0.5f) {
            float* o = reinterpret_cast<float*>(comp + qoff);
            o[0]=qx; o[1]=qy; o[2]=qz; o[3]=qw;
            g_projSteers++;
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}
void SetVRProjSteer(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t on = 0; RED4ext::GetParameter(aFrame, &on); aFrame->code++; g_projSteer = on;
}
void VRProjSteerTick(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    aFrame->code++; ProjSteerTick();
}
void GetVRProjLiveDump(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t) {
    int32_t idx = 0; RED4ext::GetParameter(aFrame, &idx); aFrame->code++;
    double v = 0.0;
    switch (idx) {
        case 0: v = (double)g_projFound; break;
        case 1: v = (double)g_projSteers; break;
        case 2: v = (double)(g_projLive & 0xFFFFFFFF); break;
        case 3: v = (double)((g_projLive >> 32) & 0xFFFFFFFF); break;
        case 4: v = (double)g_projOrientQ[0]; break;
        case 5: v = (double)g_projOrientQ[1]; break;
        case 6: v = (double)g_projOrientQ[2]; break;
        case 7: v = (double)g_projOrientQ[3]; break;
        case 8: v = (g_projCompVtbl != 0) ? 1.0 : 0.0; break;
        case 9: v = (double)g_projTotal; break;   // raw vtable matches (incl. stack/RTTI junk)
        case 10: v = (double)g_projValid; break;  // RTTI-validated real instances
        default: break;
    }
    if (aOut) *aOut = (float)v;
}

// ============================================================================
// ORIENTATION-PROVIDER GetOrientation VMT INSTRUMENT (the user's "stand at the register" plan).
// We can't pin the GetOrientation vtable slot statically (auto-analysis off, CClass vs instance
// vtable confusion). So: CreateInstance the provider class -> read its REAL instance vtable -> VMT-
// instrument the interface-tail slots (read-only: each stub bumps a per-slot counter + records the
// output quaternion, then calls the original). Fire a projectile -> the slot that fires == the one
// the launch calls; its output is the camera-forward quat. Then we flip that stub to OVERRIDE the
// output with the controller aim. No shared-memory writes (avoids the worldTransform hang).
// ============================================================================
static constexpr int kProvSlotLo = 3;      // first vtable slot to instrument
static constexpr int kProvNSlots = 48;     // slots [3..50]
static constexpr int kProvNCls   = 3;      // entEntity / entStatic / entFunc
static const char*   kProvNames[kProvNCls] = {
    "entEntityOrientationProvider", "entStaticOrientationProvider", "entFuncOrientationProvider" };
static uintptr_t g_provVtbl[kProvNCls] = {0};
static void*     g_provOrig[kProvNCls][kProvNSlots] = {0};
volatile uint64_t g_provCalls[kProvNCls][kProvNSlots] = {0};
volatile int      g_provOverrideCls  = -1;
volatile int      g_provOverrideSlot = -1; // (cls,slot) whose stub overwrites the out-quat
volatile uint64_t g_provOverrides = 0;
volatile float    g_provLastQ[4] = {0,0,0,1};
volatile float    g_provOrigQ[4] = {0,0,0,1};   // provider's ORIGINAL out-quat (=camera) for the override slot
volatile float    g_provCtrlQ[4] = {0,0,0,1};   // controller quat shared[12..15] captured at the same call
volatile float    g_provHmdQ[4]  = {0,0,0,1};   // hmd quat shared[16..19] captured at the same call
volatile float    g_provMuzzleQ[4] = {0,0,0,1}; // weapon muzzle WORLD orientation (CET publishes it)
volatile uint32_t g_provMuzzleSeq = 0;          // freshness
volatile float    g_provDeltaQ[4] = {0,0,0,1};  // mode6 cone-rotation delta (recomputed per muzzle seq)
volatile uint32_t g_provDeltaSeq = 0xFFFFFFFF;  // seq the delta was computed for
volatile int      g_provQuatMode = 1;      // 0=raw shared[53..56], 1=build from controller fwd shared[60..62]
volatile int      g_provFwdAxis  = 1;      // which body axis is "forward": 0=+X 1=+Y 2=+Z (RED uses +Y)

// Build a world orientation quaternion whose chosen forward axis = the controller forward vector.
// up = world +Z; matrix [right, fwd, up] -> quaternion. Falls back gracefully near-parallel.
static void BuildOrientFromFwd(float fx, float fy, float fz, int fwdAxis, float* q) {
    float fl = std::sqrt(fx*fx+fy*fy+fz*fz); if (fl < 1e-4f) { q[0]=0;q[1]=0;q[2]=0;q[3]=1; return; }
    fx/=fl; fy/=fl; fz/=fl;
    float ux=0, uy=0, uz=1;
    if (std::fabs(fz) > 0.95f) { ux=0; uy=1; uz=0; }            // forward near world-up -> use +Y as ref
    // right = normalize(cross(fwd, up))
    float rx = fy*uz - fz*uy, ry = fz*ux - fx*uz, rz = fx*uy - fy*ux;
    float rl = std::sqrt(rx*rx+ry*ry+rz*rz); if (rl<1e-4f){rx=1;ry=0;rz=0;rl=1;} rx/=rl;ry/=rl;rz/=rl;
    // recompute up = cross(right, fwd)
    float vx = ry*fz - rz*fy, vy = rz*fx - rx*fz, vz = rx*fy - ry*fx;
    // columns of rotation matrix depend on which axis is forward (RED forward = +Y)
    float m00,m01,m02, m10,m11,m12, m20,m21,m22;
    // X column=right, the forward axis column=fwd, remaining=up
    if (fwdAxis == 1) { // +Y = forward
        m00=rx; m10=ry; m20=rz;      // X = right
        m01=fx; m11=fy; m21=fz;      // Y = forward
        m02=vx; m12=vy; m22=vz;      // Z = up
    } else if (fwdAxis == 0) { // +X = forward
        m00=fx; m10=fy; m20=fz;
        m01=rx; m11=ry; m21=rz;
        m02=vx; m12=vy; m22=vz;
    } else { // +Z = forward
        m00=rx; m10=ry; m20=rz;
        m01=vx; m11=vy; m21=vz;
        m02=fx; m12=fy; m22=fz;
    }
    // matrix -> quaternion (Shepperd)
    float tr = m00+m11+m22;
    if (tr > 0.0f) { float s=std::sqrt(tr+1.0f)*2.0f; q[3]=0.25f*s; q[0]=(m21-m12)/s; q[1]=(m02-m20)/s; q[2]=(m10-m01)/s; }
    else if (m00>m11 && m00>m22) { float s=std::sqrt(1.0f+m00-m11-m22)*2.0f; q[3]=(m21-m12)/s; q[0]=0.25f*s; q[1]=(m01+m10)/s; q[2]=(m02+m20)/s; }
    else if (m11>m22) { float s=std::sqrt(1.0f+m11-m00-m22)*2.0f; q[3]=(m02-m20)/s; q[0]=(m01+m10)/s; q[1]=0.25f*s; q[2]=(m12+m21)/s; }
    else { float s=std::sqrt(1.0f+m22-m00-m11)*2.0f; q[3]=(m10-m01)/s; q[0]=(m02+m20)/s; q[1]=(m12+m21)/s; q[2]=0.25f*s; }
}

// Rotate vector v by quaternion q (q=x,y,z,w): o = q * v * q^-1.
static inline void ProvRotVec(const float* q, const float* v, float* o) {
    const float x=q[0],y=q[1],z=q[2],w=q[3];
    const float tx=2.0f*(y*v[2]-z*v[1]);
    const float ty=2.0f*(z*v[0]-x*v[2]);
    const float tz=2.0f*(x*v[1]-y*v[0]);
    o[0]=v[0]+w*tx+(y*tz-z*ty);
    o[1]=v[1]+w*ty+(z*tx-x*tz);
    o[2]=v[2]+w*tz+(x*ty-y*tx);
}

// Each stub knows its (class C, slot S): bump counter; sample/override the out-quat; call original.
template <int C, int S>
static uintptr_t __fastcall ProvStub(uintptr_t rcx, uintptr_t rdx, uintptr_t r8, uintptr_t r9) {
    g_provCalls[C][S]++;
    using Fn = uintptr_t(__fastcall*)(uintptr_t,uintptr_t,uintptr_t,uintptr_t);
    Fn orig = reinterpret_cast<Fn>(g_provOrig[C][S]);
    uintptr_t ret = orig ? orig(rcx, rdx, r8, r9) : 0;
    uintptr_t outp = rdx ? rdx : ret;   // out-quat: rdx buffer (or returned rax)
    if (IsReadable(outp, 16)) {
        __try {
            float* q = reinterpret_cast<float*>(outp);
            float m = q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3];
            if (m > 0.9f && m < 1.1f) {  // unit quaternion -> this slot returns an orientation
                g_provLastQ[0]=q[0]; g_provLastQ[1]=q[1]; g_provLastQ[2]=q[2]; g_provLastQ[3]=q[3];
                // DIAGNOSTIC capture, ALWAYS read-only, hardwired to entFunc(C==2) vtable slot 33
                // (S = 33 - kProvSlotLo = 30) = the PISTOL launch orientation. Capture the original
                // (=camera) quat + the controller quat so the exact transform can be derived offline.
                if (C == 2 && S == 30) {
                    g_provOrigQ[0]=q[0]; g_provOrigQ[1]=q[1]; g_provOrigQ[2]=q[2]; g_provOrigQ[3]=q[3];
                    if (g_pSharedHands) {
                        g_provCtrlQ[0]=g_pSharedHands[12]; g_provCtrlQ[1]=g_pSharedHands[13]; g_provCtrlQ[2]=g_pSharedHands[14]; g_provCtrlQ[3]=g_pSharedHands[15];
                        g_provHmdQ[0]=g_pSharedHands[16]; g_provHmdQ[1]=g_pSharedHands[17]; g_provHmdQ[2]=g_pSharedHands[18]; g_provHmdQ[3]=g_pSharedHands[19];
                    }
                }
                // ENABLE: VR-overlay-driven via shared[58] -> override slot 33 (S==30) on ANY provider
                // class (pistol=entFunc, grenade=entEntity), mode shared[59] (default 5 = game muzzle).
                // Native SetVRProvOverrideSlot path kept as a manual fallback.
                bool sharedOn = (g_pSharedHands && g_pSharedHands[58] > 0.5f && S == 30);
                bool nativeOn = (g_provOverrideCls == C && g_provOverrideSlot == S);
                if ((sharedOn || nativeOn) && g_pSharedHands) {
                    // shared (VR-overlay) path defaults to mode 6 = muzzle + preserved spread.
                    const int mode = sharedOn ? 6 : g_provQuatMode;
                    if (mode == 6) {
                        // MUZZLE + PRESERVE SPREAD (shotguns): rotate the whole shot CONE so its center
                        // lands on the barrel, keeping each pellet's relative offset.
                        //   delta = muzzle (X) conj(qCenter)  -- qCenter = first pellet's quat this frame
                        //   qNew  = delta (X) q               -- per pellet
                        // 1 pellet (pistol) -> qNew = muzzle exactly. N pellets -> pattern around muzzle.
                        float mq[4]={g_provMuzzleQ[0],g_provMuzzleQ[1],g_provMuzzleQ[2],g_provMuzzleQ[3]};
                        if (mq[0]*mq[0]+mq[1]*mq[1]+mq[2]*mq[2]+mq[3]*mq[3] > 0.5f) {
                            if (g_provDeltaSeq != g_provMuzzleSeq) {   // recompute once per frame (first pellet)
                                float cqx=-q[0],cqy=-q[1],cqz=-q[2],cqw=q[3];           // conj(q)
                                g_provDeltaQ[0]=mq[3]*cqx+mq[0]*cqw+mq[1]*cqz-mq[2]*cqy; // mq (X) conj(q)
                                g_provDeltaQ[1]=mq[3]*cqy-mq[0]*cqz+mq[1]*cqw+mq[2]*cqx;
                                g_provDeltaQ[2]=mq[3]*cqz+mq[0]*cqy-mq[1]*cqx+mq[2]*cqw;
                                g_provDeltaQ[3]=mq[3]*cqw-mq[0]*cqx-mq[1]*cqy-mq[2]*cqz;
                                g_provDeltaSeq = g_provMuzzleSeq;
                            }
                            float dx=g_provDeltaQ[0],dy=g_provDeltaQ[1],dz=g_provDeltaQ[2],dw=g_provDeltaQ[3];
                            float ax=q[0],ay=q[1],az=q[2],aw=q[3];                       // delta (X) q
                            q[0]=dw*ax+dx*aw+dy*az-dz*ay;
                            q[1]=dw*ay-dx*az+dy*aw+dz*ax;
                            q[2]=dw*az+dx*ay-dy*ax+dz*aw;
                            q[3]=dw*aw-dx*ax-dy*ay-dz*az;
                            g_provOverrides++;
                        }
                    } else if (mode == 5) {
                        // MUZZLE: use the game's own muzzle WORLD orientation (CET publishes it via
                        // weapon:GetMuzzleSlotWorldTransform). Pure game-world, NO controller-space math.
                        // g_provMuzzleFwdAxis selects which muzzle local axis = barrel (default +Y).
                        float mq[4]={g_provMuzzleQ[0],g_provMuzzleQ[1],g_provMuzzleQ[2],g_provMuzzleQ[3]};
                        if (mq[0]*mq[0]+mq[1]*mq[1]+mq[2]*mq[2]+mq[3]*mq[3] > 0.5f) {
                            if (g_provFwdAxis == 1) {            // muzzle orientation used directly
                                q[0]=mq[0]; q[1]=mq[1]; q[2]=mq[2]; q[3]=mq[3];
                            } else {                            // rebuild +Y-forward from the chosen muzzle axis
                                float ax[3]={0,0,0}; ax[g_provFwdAxis==0?0:2]=1.0f;
                                float fw[3]; ProvRotVec(mq, ax, fw);
                                float nq[4]; BuildOrientFromFwd(fw[0],fw[1],fw[2],1,nq);
                                q[0]=nq[0];q[1]=nq[1];q[2]=nq[2];q[3]=nq[3];
                            }
                            g_provOverrides++;
                        }
                    } else if (mode == 4) {
                        // DATA-DERIVED correct aim. The controller BARREL = its local -Z axis (not +Y!).
                        //   barrel_base = rot(ctrl, (0,0,-1));  barrel_head = rot(conj(hmd), barrel_base)
                        //   fwd_world   = rot(camera/origQ, barrel_head);  qNew = lookQuat(fwd_world,+Y)
                        // Identity when controller points where the head looks -> crosshair; tracks the
                        // controller as it deviates. (Verified against the aligned-sample quaternions.)
                        float cq[4]={g_pSharedHands[12],g_pSharedHands[13],g_pSharedHands[14],g_pSharedHands[15]};
                        float hq[4]={g_pSharedHands[16],g_pSharedHands[17],g_pSharedHands[18],g_pSharedHands[19]};
                        if (cq[0]*cq[0]+cq[1]*cq[1]+cq[2]*cq[2]+cq[3]*cq[3]>0.5f &&
                            hq[0]*hq[0]+hq[1]*hq[1]+hq[2]*hq[2]+hq[3]*hq[3]>0.5f) {
                            float zaxis[3]={0,0,-1}, cb[3], ch[3], cw[3];
                            ProvRotVec(cq, zaxis, cb);                 // barrel in base
                            float hconj[4]={-hq[0],-hq[1],-hq[2],hq[3]};
                            ProvRotVec(hconj, cb, ch);                 // -> head-local
                            float oq[4]={q[0],q[1],q[2],q[3]};
                            ProvRotVec(oq, ch, cw);                    // -> world via camera basis
                            float nq[4]; BuildOrientFromFwd(cw[0], cw[1], cw[2], 1 /*+Y*/, nq);
                            q[0]=nq[0]; q[1]=nq[1]; q[2]=nq[2]; q[3]=nq[3]; g_provOverrides++;
                        }
                    } else if (mode == 3) {
                        // qNew = original(camera) * [ inv(swap(hmd)) * swap(controller) ]
                        // = camera rotated by the controller's offset-from-head, in game-local space.
                        // swap=(x,-z,y,w) is the VR->game axis map (matches VRIK). Identity when the
                        // controller points where the head looks -> bullet stays on crosshair; tracks
                        // the controller as it deviates. (Derived from the aligned-sample numbers.)
                        float cqx=g_pSharedHands[12], cqy=g_pSharedHands[13], cqz=g_pSharedHands[14], cqw=g_pSharedHands[15];
                        float hqx=g_pSharedHands[16], hqy=g_pSharedHands[17], hqz=g_pSharedHands[18], hqw=g_pSharedHands[19];
                        if (cqx*cqx+cqy*cqy+cqz*cqz+cqw*cqw>0.5f && hqx*hqx+hqy*hqy+hqz*hqz+hqw*hqw>0.5f) {
                            // swapped controller, swapped head
                            float sc[4]={cqx,-cqz,cqy,cqw};
                            float sh[4]={hqx,-hqz,hqy,hqw};
                            float ih[4]={-sh[0],-sh[1],-sh[2],sh[3]};           // inv(swap(hmd))
                            // delta = ih (X) sc
                            float dx=ih[3]*sc[0]+ih[0]*sc[3]+ih[1]*sc[2]-ih[2]*sc[1];
                            float dy=ih[3]*sc[1]-ih[0]*sc[2]+ih[1]*sc[3]+ih[2]*sc[0];
                            float dz=ih[3]*sc[2]+ih[0]*sc[1]-ih[1]*sc[0]+ih[2]*sc[3];
                            float dw=ih[3]*sc[3]-ih[0]*sc[0]-ih[1]*sc[1]-ih[2]*sc[2];
                            // qNew = original (X) delta
                            float ax=q[0],ay=q[1],az=q[2],aw=q[3];
                            q[0]=aw*dx+ax*dw+ay*dz-az*dy;
                            q[1]=aw*dy-ax*dz+ay*dw+az*dx;
                            q[2]=aw*dz+ax*dy-ay*dx+az*dw;
                            q[3]=aw*dw-ax*dx-ay*dy-az*dz;
                            g_provOverrides++;
                        }
                    } else if (mode == 2) {
                        // EXACTLY VRIK_WriteHand: handWorld = headQuat * swap(controllerQuat).
                        // The provider's ORIGINAL out-quat == the camera/head orientation, so it plays
                        // headQuat; delta = swap(shared[12..15]) = (x,-z,y,w) of the controller quat.
                        // qNew = original (X) delta  -> the VRIK-correct world hand aim (head-stable).
                        float hx=g_pSharedHands[12], hy=g_pSharedHands[13], hz=g_pSharedHands[14], hw=g_pSharedHands[15];
                        if (hx*hx+hy*hy+hz*hz+hw*hw > 0.5f) {
                            float dx=hx, dy=-hz, dz=hy, dw=hw;          // VR->game axis swap (i,-k,j,r)
                            float ax=q[0],ay=q[1],az=q[2],aw=q[3];      // original (head/camera)
                            q[0]=aw*dx+ax*dw+ay*dz-az*dy;               // Hamilton original (X) delta
                            q[1]=aw*dy-ax*dz+ay*dw+az*dx;
                            q[2]=aw*dz+ax*dy-ay*dx+az*dw;
                            q[3]=aw*dw-ax*dx-ay*dy-az*dz;
                            g_provOverrides++;
                        }
                    } else if (mode == 1) {
                        float fx=g_pSharedHands[60], fy=g_pSharedHands[61], fz=g_pSharedHands[62];
                        if (fx*fx+fy*fy+fz*fz > 0.25f) {
                            float nq[4]; BuildOrientFromFwd(fx, fy, fz, g_provFwdAxis, nq);
                            q[0]=nq[0]; q[1]=nq[1]; q[2]=nq[2]; q[3]=nq[3]; g_provOverrides++;
                        }
                    } else {
                        float cx=g_pSharedHands[53],cy=g_pSharedHands[54],cz=g_pSharedHands[55],cw=g_pSharedHands[56];
                        if (cx*cx+cy*cy+cz*cz+cw*cw > 0.5f) { q[0]=cx; q[1]=cy; q[2]=cz; q[3]=cw; g_provOverrides++; }
                    }
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return ret;
}
static void* g_provStubTbl[kProvNCls][kProvNSlots] = {0};
template <int C, int... I> static void FillRow(std::integer_sequence<int, I...>) {
    ((g_provStubTbl[C][I] = reinterpret_cast<void*>(&ProvStub<C, I>)), ...);
}
volatile int g_provInstalled = 0;
static int InstallProvClass(int c) {
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass(kProvNames[c]) : nullptr;
    if (!cls) return -1;
    void* inst = cls->CreateInstance(true);
    if (!inst) return -2;
    uintptr_t vt = 0;
    __try { vt = *reinterpret_cast<uintptr_t*>(inst); } __except (EXCEPTION_EXECUTE_HANDLER) {}
    if (!vt) return -3;
    g_provVtbl[c] = vt;
    DWORD oldp = 0;
    if (!VirtualProtect(reinterpret_cast<void*>(vt + kProvSlotLo*8), kProvNSlots*8, PAGE_EXECUTE_READWRITE, &oldp)) return -4;
    for (int i = 0; i < kProvNSlots; ++i) {
        uintptr_t* slot = reinterpret_cast<uintptr_t*>(vt + (kProvSlotLo + i)*8);
        g_provOrig[c][i] = reinterpret_cast<void*>(*slot);
        *slot = reinterpret_cast<uintptr_t>(g_provStubTbl[c][i]);
    }
    VirtualProtect(reinterpret_cast<void*>(vt + kProvSlotLo*8), kProvNSlots*8, oldp, &oldp);
    return 1;
}
void InstallVRProvInstrument(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    if (g_provInstalled) { if (aOut) *aOut = 2; return; }
    FillRow<0>(std::make_integer_sequence<int, kProvNSlots>{});
    FillRow<1>(std::make_integer_sequence<int, kProvNSlots>{});
    FillRow<2>(std::make_integer_sequence<int, kProvNSlots>{});
    int ok = 0;
    for (int c = 0; c < kProvNCls; ++c) if (InstallProvClass(c) == 1) ++ok;
    g_provInstalled = ok > 0 ? 1 : 0;
    if (aOut) *aOut = ok;   // number of provider classes instrumented (expect 3)
}
// arg = cls*1000 + REAL vtable slot (e.g. 0*1000+33 for entEntity slot 33); -1 = off.
// Stored override slot is the STUB INDEX (realSlot - kProvSlotLo) to match ProvStub<C,S>.
void SetVRProvOverrideSlot(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t v = -1; RED4ext::GetParameter(aFrame, &v); aFrame->code++;
    if (v < 0) { g_provOverrideCls = -1; g_provOverrideSlot = -1; }
    else {
        int realSlot = v % 1000;
        g_provOverrideCls = v / 1000;
        g_provOverrideSlot = realSlot - kProvSlotLo;   // -> stub index
    }
}
void SetVRProvQuatMode(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t mode = 1, axis = 1; RED4ext::GetParameter(aFrame, &mode); RED4ext::GetParameter(aFrame, &axis);
    aFrame->code++; g_provQuatMode = mode; g_provFwdAxis = axis;
}
void SetVRMuzzleQuat(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float i=0,j=0,k=0,r=1; RED4ext::GetParameter(aFrame,&i); RED4ext::GetParameter(aFrame,&j);
    RED4ext::GetParameter(aFrame,&k); RED4ext::GetParameter(aFrame,&r); aFrame->code++;
    g_provMuzzleQ[0]=i; g_provMuzzleQ[1]=j; g_provMuzzleQ[2]=k; g_provMuzzleQ[3]=r; ++g_provMuzzleSeq;
    // Publish the muzzle WORLD forward (+Y of the quat) to shared[24..26] (FREE slots; [33..47] are
    // VRIK IK-calib, [50..62] weapon-aim) so the dxgi overlay can project an EXACT barrel crosshair
    // (this dir through the located camera = the eye view).
    if (g_pSharedHands) {
        g_pSharedHands[24] = 2.0f*(i*j - k*r);
        g_pSharedHands[25] = 1.0f - 2.0f*(i*i + k*k);
        g_pSharedHands[26] = 2.0f*(j*k + i*r);
        g_pSharedHands[27] = 1.0f;  // valid
    }
}
// Publish the current ADS/scope zoom factor to shared[28] so the dxgi overlay can scale the
// barrel laser-dot's screen offset by it (the scope magnifies the image but the bullet still
// leaves the barrel). CET pushes PlayerStateMachine.ZoomLevel each frame; 1.0 = no zoom.
void SetVRZoomLevel(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float z = 1.0f; RED4ext::GetParameter(aFrame, &z); aFrame->code++;
    if (g_pSharedHands) g_pSharedHands[28] = (z > 0.01f && z < 64.0f) ? z : 1.0f;
}
// Melee fire pulse -> shared[29]. The CET weapon mod pulses this on a VR swing; the dxgi XInput merge
// reads it and forces the right trigger so the GAME performs its own native melee attack (native
// damage / crits / numbers / armor). A swing doesn't press RT by itself, hence the inject.
// Melee RT IMPULSE: the CET mod calls this on a detected VR swing with a small frame count (e.g. 4).
// dxgi (HookedXInputGetState) sees shared[29] > 0 and taps RT for that many frames, so the game enters
// its NATIVE melee-attack PSM state — dealing fully native damage / combo / numbers / markers — then
// decrements it back to 0. (Not a sustained hold, so it's a single attack press, not spam.)
void SetVRMeleeFire(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t v = 0; RED4ext::GetParameter(aFrame, &v); aFrame->code++;
    if (g_pSharedHands) g_pSharedHands[29] = (float)v;
}
// Read the held-trigger power flag (shared[30], published by the dxgi XInput merge while in melee
// mode). The CET weapon mod uses it as the power-attack modifier for the next swing.
void GetVRMeleeTrigger(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    if (aOut) *aOut = (g_pSharedHands && g_pSharedHands[30] > 0.5f) ? 1 : 0;
}
// Generic shared-slot read for CET mods that need raw values (hand HMD-local poses, grip analog,
// etc.). idx must be 0..127 -- the shared block is 128 floats (CyberpunkVR_Hands_Shared, 512 bytes).
// Slot map (used today): [0]/[8] hand-valid, [1-3]/[9-11] hand pos HMD-local, [4-7]/[12-15] hand
// orient, [16-19] HMD orient, [24-26]+[27] muzzle fwd+valid, [28] zoom, [30] RT-held bool,
// [32] hand-tracking mode, [49] right grip, [58] weapon-aim enable, [70..76] shoulder calibration.
void GetVRSharedSlot(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t) {
    int32_t idx = 0; RED4ext::GetParameter(aFrame, &idx); aFrame->code++;
    if (aOut) *aOut = (g_pSharedHands && idx >= 0 && idx < 128) ? g_pSharedHands[idx] : 0.0f;
}
void GetVRProvDump(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t) {
    int32_t idx = 0; RED4ext::GetParameter(aFrame, &idx); aFrame->code++;
    double v = 0.0;
    if (idx == 103) v = g_provQuatMode;
    else if (idx == 104) v = g_provFwdAxis;
    else if (idx == 100) v = g_provInstalled;
    else if (idx == 101) v = (double)g_provOverrides;
    else if (idx == 102) v = (g_provOverrideCls < 0) ? -1 : (g_provOverrideCls*1000 + g_provOverrideSlot);
    else if (idx >= 200 && idx < 204) v = g_provLastQ[idx-200];
    else if (idx >= 210 && idx < 214) v = g_provOrigQ[idx-210];   // original (camera) quat
    else if (idx >= 214 && idx < 218) v = g_provCtrlQ[idx-214];   // controller quat shared[12..15]
    else if (idx >= 218 && idx < 222) v = g_provHmdQ[idx-218];    // hmd quat shared[16..19]
    else if (idx >= 0 && idx < kProvNCls*kProvNSlots) v = (double)g_provCalls[idx / kProvNSlots][idx % kProvNSlots];
    if (aOut) *aOut = (float)v;
}
// Clear the per-(class,slot) GetOrientation fire counters + sampled quats. Lets the user check each
// weapon fresh: clear -> equip+fire one weapon -> read which entFunc/entEntity slot fired.
void ResetVRProvCounts(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    aFrame->code++;
    for (int c = 0; c < kProvNCls; ++c)
        for (int s = 0; s < kProvNSlots; ++s) g_provCalls[c][s] = 0;
    g_provOverrides = 0;
    for (int i = 0; i < 4; ++i) { g_provLastQ[i]=0; g_provOrigQ[i]=0; g_provCtrlQ[i]=0; g_provHmdQ[i]=0; }
}

RED4EXT_C_EXPORT void RED4EXT_CALL PostRegisterTypes() {
    auto rtti = RED4ext::CRTTISystem::Get();
    RED4ext::CBaseFunction::Flags flags = {.isNative = true, .isStatic = true};

    auto f1 = RED4ext::CGlobalFunction::Create("GetLeftVRHandValid", "GetLeftVRHandValid", &GetLeftVRHandValid);
    f1->flags = flags; f1->SetReturnType("Bool"); rtti->RegisterFunction(f1);

    auto f2 = RED4ext::CGlobalFunction::Create("GetRightVRHandValid", "GetRightVRHandValid", &GetRightVRHandValid);
    f2->flags = flags; f2->SetReturnType("Bool"); rtti->RegisterFunction(f2);

    auto f3 = RED4ext::CGlobalFunction::Create("GetLeftVRHandPos", "GetLeftVRHandPos", &GetLeftVRHandPos);
    f3->flags = flags; f3->SetReturnType("Vector4"); rtti->RegisterFunction(f3);

    auto f4 = RED4ext::CGlobalFunction::Create("GetRightVRHandPos", "GetRightVRHandPos", &GetRightVRHandPos);
    f4->flags = flags; f4->SetReturnType("Vector4"); rtti->RegisterFunction(f4);

    auto f5 = RED4ext::CGlobalFunction::Create("GetLeftVRHandRot", "GetLeftVRHandRot", &GetLeftVRHandRot);
    f5->flags = flags; f5->SetReturnType("Quaternion"); rtti->RegisterFunction(f5);

    auto f6 = RED4ext::CGlobalFunction::Create("GetRightVRHandRot", "GetRightVRHandRot", &GetRightVRHandRot);
    f6->flags = flags; f6->SetReturnType("Quaternion"); rtti->RegisterFunction(f6);

    auto f7 = RED4ext::CGlobalFunction::Create("IsVRHandLinked", "IsVRHandLinked", &IsVRHandLinked);
    f7->flags = flags; f7->SetReturnType("Bool"); rtti->RegisterFunction(f7);

    auto f8 = RED4ext::CGlobalFunction::Create("ForceHideVRFppArms", "ForceHideVRFppArms", &ForceHideVRFppArms);
    f8->flags = flags; f8->SetReturnType("Int32"); rtti->RegisterFunction(f8);

    auto f8b = RED4ext::CGlobalFunction::Create("RestoreVRFppArms", "RestoreVRFppArms", &RestoreVRFppArms);
    f8b->flags = flags; f8b->SetReturnType("Int32"); rtti->RegisterFunction(f8b);

    auto f9 = RED4ext::CGlobalFunction::Create("SetVRRightHandEntity", "SetVRRightHandEntity", &SetVRRightHandEntity);
    f9->flags = flags; f9->AddParam("handle:IScriptable", "entity"); rtti->RegisterFunction(f9);

    auto f10 = RED4ext::CGlobalFunction::Create("DumpVRFppComponents", "DumpVRFppComponents", &DumpVRFppComponents);
    f10->flags = flags; f10->SetReturnType("Int32"); rtti->RegisterFunction(f10);

    auto f11 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugEnabled", "SetVRFppChunkDebugEnabled", &SetVRFppChunkDebugEnabled);
    f11->flags = flags; f11->AddParam("Int32", "enabled"); rtti->RegisterFunction(f11);

    auto f12 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugComponentIndex", "SetVRFppChunkDebugComponentIndex", &SetVRFppChunkDebugComponentIndex);
    f12->flags = flags; f12->AddParam("Int32", "index"); rtti->RegisterFunction(f12);

    auto f13 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugHand", "SetVRFppChunkDebugHand", &SetVRFppChunkDebugHand);
    f13->flags = flags; f13->AddParam("Int32", "hand"); rtti->RegisterFunction(f13);

    auto f14 = RED4ext::CGlobalFunction::Create("SetVRFppChunkDebugBits", "SetVRFppChunkDebugBits", &SetVRFppChunkDebugBits);
    f14->flags = flags;
    f14->AddParam("Int32", "bit0");
    f14->AddParam("Int32", "bit1");
    f14->AddParam("Int32", "bit2");
    f14->AddParam("Int32", "bit3");
    rtti->RegisterFunction(f14);

    auto f15 = RED4ext::CGlobalFunction::Create("SetVRBoneDebugIndex", "SetVRBoneDebugIndex", &SetVRBoneDebugIndex);
    f15->flags = flags; f15->AddParam("Int32", "index"); rtti->RegisterFunction(f15);

    auto f15a = RED4ext::CGlobalFunction::Create("ArmVRBoneHook", "ArmVRBoneHook", &ArmVRBoneHook);
    f15a->flags = flags; f15a->SetReturnType("Int32"); f15a->AddParam("Int32", "mode"); rtti->RegisterFunction(f15a);

    auto f15b = RED4ext::CGlobalFunction::Create("GetVRBoneHookArmed", "GetVRBoneHookArmed", &GetVRBoneHookArmed);
    f15b->flags = flags; f15b->SetReturnType("Int32"); rtti->RegisterFunction(f15b);

    auto f15c = RED4ext::CGlobalFunction::Create("GetVRBoneHookStats", "GetVRBoneHookStats", &GetVRBoneHookStats);
    f15c->flags = flags; f15c->SetReturnType("Int32"); f15c->AddParam("Int32", "mode"); rtti->RegisterFunction(f15c);

    auto f15d = RED4ext::CGlobalFunction::Create("SetVRBoneHookCapture", "SetVRBoneHookCapture", &SetVRBoneHookCapture);
    f15d->flags = flags; f15d->SetReturnType("Int32"); f15d->AddParam("Int32", "on"); rtti->RegisterFunction(f15d);

    auto f15e = RED4ext::CGlobalFunction::Create("GetVRBoneHookCapturedCount", "GetVRBoneHookCapturedCount", &GetVRBoneHookCapturedCount);
    f15e->flags = flags; f15e->SetReturnType("Int32"); rtti->RegisterFunction(f15e);

    auto f15f = RED4ext::CGlobalFunction::Create("GetVRBoneHookCapturedRcx", "GetVRBoneHookCapturedRcx", &GetVRBoneHookCapturedRcx);
    f15f->flags = flags; f15f->SetReturnType("Int32"); f15f->AddParam("Int32", "index"); rtti->RegisterFunction(f15f);

    auto f15g = RED4ext::CGlobalFunction::Create("GetVRBoneHookCapturedBoneCount", "GetVRBoneHookCapturedBoneCount", &GetVRBoneHookCapturedBoneCount);
    f15g->flags = flags; f15g->SetReturnType("Int32"); f15g->AddParam("Int32", "index"); rtti->RegisterFunction(f15g);

    auto f15h = RED4ext::CGlobalFunction::Create("ArmVRBoneHookByCaptureIndex", "ArmVRBoneHookByCaptureIndex", &ArmVRBoneHookByCaptureIndex);
    f15h->flags = flags; f15h->SetReturnType("Int32"); f15h->AddParam("Int32", "index"); rtti->RegisterFunction(f15h);

    auto f15i = RED4ext::CGlobalFunction::Create("GetPlayerBoneBufferAddress", "GetPlayerBoneBufferAddress", &GetPlayerBoneBufferAddress);
    f15i->flags = flags; f15i->SetReturnType("Int32"); rtti->RegisterFunction(f15i);

    auto f15j = RED4ext::CGlobalFunction::Create("InstallVRAnimPoseHook", "InstallVRAnimPoseHook", &InstallVRAnimPoseHook);
    f15j->flags = flags; f15j->SetReturnType("Int32"); rtti->RegisterFunction(f15j);

    auto f15k = RED4ext::CGlobalFunction::Create("ArmVRAnimPosePlayer", "ArmVRAnimPosePlayer", &ArmVRAnimPosePlayer);
    f15k->flags = flags; f15k->SetReturnType("Int32"); rtti->RegisterFunction(f15k);

    auto f15l = RED4ext::CGlobalFunction::Create("SetVRAnimPoseDebug", "SetVRAnimPoseDebug", &SetVRAnimPoseDebug);
    f15l->flags = flags; f15l->SetReturnType("Int32"); f15l->AddParam("Int32", "mode"); rtti->RegisterFunction(f15l);

    auto f15m = RED4ext::CGlobalFunction::Create("GetVRAnimPoseStats", "GetVRAnimPoseStats", &GetVRAnimPoseStats);
    f15m->flags = flags; f15m->SetReturnType("Int32"); f15m->AddParam("Int32", "mode"); rtti->RegisterFunction(f15m);

    auto f15n = RED4ext::CGlobalFunction::Create("SetVRAnimPoseBoneTest", "SetVRAnimPoseBoneTest", &SetVRAnimPoseBoneTest);
    f15n->flags = flags; f15n->SetReturnType("Int32"); f15n->AddParam("Int32", "index"); f15n->AddParam("Float", "mag"); rtti->RegisterFunction(f15n);

    auto f15o = RED4ext::CGlobalFunction::Create("DumpPlayerBoneNames", "DumpPlayerBoneNames", &DumpPlayerBoneNames);
    f15o->flags = flags; f15o->SetReturnType("Int32"); rtti->RegisterFunction(f15o);

    auto f15p = RED4ext::CGlobalFunction::Create("SetVRBindMode", "SetVRBindMode", &SetVRBindMode);
    f15p->flags = flags; f15p->SetReturnType("Int32"); f15p->AddParam("Int32", "mode"); rtti->RegisterFunction(f15p);

    auto f15q = RED4ext::CGlobalFunction::Create("SetVRBindParams", "SetVRBindParams", &SetVRBindParams);
    f15q->flags = flags; f15q->SetReturnType("Int32"); 
    f15q->AddParam("Float", "scale"); f15q->AddParam("Float", "x"); f15q->AddParam("Float", "y"); f15q->AddParam("Float", "z"); f15q->AddParam("Int32", "axis"); f15q->AddParam("Int32", "hand");
    rtti->RegisterFunction(f15q);

    auto f15qE = RED4ext::CGlobalFunction::Create("SetVRElbowPole", "SetVRElbowPole", &SetVRElbowPole);
    f15qE->flags = flags; f15qE->SetReturnType("Int32");
    f15qE->AddParam("Float", "angle"); f15qE->AddParam("Int32", "hand");
    rtti->RegisterFunction(f15qE);

    auto f15qS = RED4ext::CGlobalFunction::Create("SetVRElbowSwing", "SetVRElbowSwing", &SetVRElbowSwing);
    f15qS->flags = flags; f15qS->SetReturnType("Int32");
    f15qS->AddParam("Float", "gain"); f15qS->AddParam("Int32", "hand");
    rtti->RegisterFunction(f15qS);

    auto f15qO = RED4ext::CGlobalFunction::Create("SetVRHandOffset", "SetVRHandOffset", &SetVRHandOffset);
    f15qO->flags = flags; f15qO->SetReturnType("Int32");
    f15qO->AddParam("Float", "pitch"); f15qO->AddParam("Float", "yaw"); f15qO->AddParam("Float", "roll"); f15qO->AddParam("Int32", "hand");
    rtti->RegisterFunction(f15qO);

    auto f15r = RED4ext::CGlobalFunction::Create("SetVRBindBones", "SetVRBindBones", &SetVRBindBones);
    f15r->flags = flags; f15r->SetReturnType("Int32"); 
    f15r->AddParam("Int32", "leftIdx"); f15r->AddParam("Int32", "rightIdx");
    rtti->RegisterFunction(f15r);

    auto f15rH = RED4ext::CGlobalFunction::Create("SetVRHeadBone", "SetVRHeadBone", &SetVRHeadBone);
    f15rH->flags = flags; f15rH->SetReturnType("Int32"); f15rH->AddParam("Int32", "index"); rtti->RegisterFunction(f15rH);

    auto f15rR = RED4ext::CGlobalFunction::Create("SetVRUseHeadRelative", "SetVRUseHeadRelative", &SetVRUseHeadRelative);
    f15rR->flags = flags; f15rR->SetReturnType("Int32"); f15rR->AddParam("Int32", "on"); rtti->RegisterFunction(f15rR);

    auto f15rC = RED4ext::CGlobalFunction::Create("SetVRDiagCapture", "SetVRDiagCapture", &SetVRDiagCapture);
    f15rC->flags = flags; f15rC->SetReturnType("Int32"); f15rC->AddParam("Int32", "on"); rtti->RegisterFunction(f15rC);

    auto f15rD = RED4ext::CGlobalFunction::Create("LogVRDiag", "LogVRDiag", &LogVRDiag);
    f15rD->flags = flags; f15rD->SetReturnType("Int32");
    f15rD->AddParam("Float", "camX"); f15rD->AddParam("Float", "camY"); f15rD->AddParam("Float", "camZ");
    f15rD->AddParam("Float", "qi"); f15rD->AddParam("Float", "qj"); f15rD->AddParam("Float", "qk"); f15rD->AddParam("Float", "qr");
    rtti->RegisterFunction(f15rD);



    auto f15s = RED4ext::CGlobalFunction::Create("SetVRPlayerYaw", "SetVRPlayerYaw", &SetVRPlayerYaw);
    f15s->flags = flags; f15s->SetReturnType("Int32");
    f15s->AddParam("Float", "yaw");
    f15s->AddParam("Float", "ci"); f15s->AddParam("Float", "cj");
    f15s->AddParam("Float", "ck"); f15s->AddParam("Float", "cr");
    f15s->AddParam("Float", "camX"); f15s->AddParam("Float", "camY"); f15s->AddParam("Float", "camZ");
    f15s->AddParam("Float", "entX"); f15s->AddParam("Float", "entY"); f15s->AddParam("Float", "entZ");
    f15s->AddParam("Float", "eqi"); f15s->AddParam("Float", "eqj");
    f15s->AddParam("Float", "eqk"); f15s->AddParam("Float", "eqr");
    rtti->RegisterFunction(f15s);

    auto f19 = RED4ext::CGlobalFunction::Create("DumpAnimVTable", "DumpAnimVTable", &DumpAnimVTable);
    f19->flags = flags; f19->SetReturnType("Int32"); rtti->RegisterFunction(f19);

    auto f20 = RED4ext::CGlobalFunction::Create("DumpAnimMemory", "DumpAnimMemory", &DumpAnimMemory);
    f20->flags = flags; f20->SetReturnType("Int32"); rtti->RegisterFunction(f20);

    auto f21 = RED4ext::CGlobalFunction::Create("DumpAnimControllerComponents", "DumpAnimControllerComponents", &DumpAnimControllerComponents);
    f21->flags = flags; f21->SetReturnType("Int32"); rtti->RegisterFunction(f21);

    auto f22 = RED4ext::CGlobalFunction::Create("DumpRuntimeClassFunctions", "DumpRuntimeClassFunctions", &DumpRuntimeClassFunctions);
    f22->flags = flags; f22->SetReturnType("Int32"); rtti->RegisterFunction(f22);

    auto f23 = RED4ext::CGlobalFunction::Create("SetVRIKAnimInputTestMode", "SetVRIKAnimInputTestMode", &SetVRIKAnimInputTestMode);
    f23->flags = flags; f23->AddParam("Int32", "mode"); rtti->RegisterFunction(f23);

    auto f24 = RED4ext::CGlobalFunction::Create("UpdateVRIKAnimInputs", "UpdateVRIKAnimInputs", &UpdateVRIKAnimInputs);
    f24->flags = flags; f24->SetReturnType("Int32"); rtti->RegisterFunction(f24);

    auto f25 = RED4ext::CGlobalFunction::Create("DumpAnimControllerFunctionDetails", "DumpAnimControllerFunctionDetails", &DumpAnimControllerFunctionDetails);
    f25->flags = flags; f25->SetReturnType("Int32"); rtti->RegisterFunction(f25);

    auto f26 = RED4ext::CGlobalFunction::Create("DumpAnimControllerListeners", "DumpAnimControllerListeners", &DumpAnimControllerListeners);
    f26->flags = flags; f26->SetReturnType("Int32"); rtti->RegisterFunction(f26);

    auto f27 = RED4ext::CGlobalFunction::Create("DumpInterestingAnimClassProperties", "DumpInterestingAnimClassProperties", &DumpInterestingAnimClassProperties);
    f27->flags = flags; f27->SetReturnType("Int32"); rtti->RegisterFunction(f27);

    auto f28 = RED4ext::CGlobalFunction::Create("DumpAnimationSystemCandidates", "DumpAnimationSystemCandidates", &DumpAnimationSystemCandidates);
    f28->flags = flags; f28->SetReturnType("Int32"); rtti->RegisterFunction(f28);

    auto f29 = RED4ext::CGlobalFunction::Create("RunIKTargetAddTest", "RunIKTargetAddTest", &RunIKTargetAddTest);
    f29->flags = flags; f29->SetReturnType("Int32"); f29->AddParam("Int32", "mode"); rtti->RegisterFunction(f29);

    auto f30 = RED4ext::CGlobalFunction::Create("TestAnimFloatInput", "TestAnimFloatInput", &TestAnimFloatInput);
    f30->flags = flags; f30->SetReturnType("Int32");
    f30->AddParam("CName", "inputName");
    f30->AddParam("Float", "value");
    f30->AddParam("Int32", "route");
    rtti->RegisterFunction(f30);

    auto f31 = RED4ext::CGlobalFunction::Create("TestAnimFloatPreset", "TestAnimFloatPreset", &TestAnimFloatPreset);
    f31->flags = flags; f31->SetReturnType("Int32");
    f31->AddParam("Int32", "mode");
    f31->AddParam("Float", "value");
    f31->AddParam("Int32", "route");
    rtti->RegisterFunction(f31);

    auto f32 = RED4ext::CGlobalFunction::Create("SetPlayerAnimParameter", "SetPlayerAnimParameter", &SetPlayerAnimParameter);
    f32->flags = flags; f32->SetReturnType("Int32");
    f32->AddParam("CName", "inputName");
    f32->AddParam("Float", "value");
    rtti->RegisterFunction(f32);

    auto f33 = RED4ext::CGlobalFunction::Create("SetPlayerAnimParameterPreset", "SetPlayerAnimParameterPreset", &SetPlayerAnimParameterPreset);
    f33->flags = flags; f33->SetReturnType("Int32");
    f33->AddParam("Int32", "mode");
    f33->AddParam("Float", "value");
    rtti->RegisterFunction(f33);

    auto f34 = RED4ext::CGlobalFunction::Create("SetPlayerAnimParameterPersistentPreset", "SetPlayerAnimParameterPersistentPreset", &SetPlayerAnimParameterPersistentPreset);
    f34->flags = flags; f34->SetReturnType("Int32");
    f34->AddParam("Int32", "mode");
    f34->AddParam("Float", "value");
    rtti->RegisterFunction(f34);

    auto f35 = RED4ext::CGlobalFunction::Create("GetPlayerAnimParameterPersistentLastResult", "GetPlayerAnimParameterPersistentLastResult", &GetPlayerAnimParameterPersistentLastResult);
    f35->flags = flags; f35->SetReturnType("Int32");
    rtti->RegisterFunction(f35);

    auto f36 = RED4ext::CGlobalFunction::Create("TestWeaponUserFeatureRoute", "TestWeaponUserFeatureRoute", &TestWeaponUserFeatureRoute);
    f36->flags = flags; f36->SetReturnType("Int32");
    f36->AddParam("Int32", "route");
    rtti->RegisterFunction(f36);

    auto f37 = RED4ext::CGlobalFunction::Create("TestIKFeatureRoute", "TestIKFeatureRoute", &TestIKFeatureRoute);
    f37->flags = flags; f37->SetReturnType("Int32");
    f37->AddParam("Int32", "mode");
    f37->AddParam("Int32", "route");
    rtti->RegisterFunction(f37);

    auto f38 = RED4ext::CGlobalFunction::Create("TestMeleeIKDataFeatureRoute", "TestMeleeIKDataFeatureRoute", &TestMeleeIKDataFeatureRoute);
    f38->flags = flags; f38->SetReturnType("Int32");
    f38->AddParam("Int32", "route");
    rtti->RegisterFunction(f38);

    auto f39 = RED4ext::CGlobalFunction::Create("DumpRootGraphVariables", "DumpRootGraphVariables", &DumpRootGraphVariables);
    f39->flags = flags; f39->SetReturnType("Int32");
    rtti->RegisterFunction(f39);

    auto f40 = RED4ext::CGlobalFunction::Create("SetRootGraphBoolVariable", "SetRootGraphBoolVariable", &SetRootGraphBoolVariableByName);
    f40->flags = flags; f40->SetReturnType("Int32");
    f40->AddParam("CName", "name");
    f40->AddParam("Bool", "value");
    rtti->RegisterFunction(f40);

    auto f41 = RED4ext::CGlobalFunction::Create("SetRootGraphFloatVariable", "SetRootGraphFloatVariable", &SetRootGraphFloatVariableByName);
    f41->flags = flags; f41->SetReturnType("Int32");
    f41->AddParam("CName", "name");
    f41->AddParam("Float", "value");
    rtti->RegisterFunction(f41);

    auto f42 = RED4ext::CGlobalFunction::Create("SetRootGraphVectorVariable", "SetRootGraphVectorVariable", &SetRootGraphVectorVariableByName);
    f42->flags = flags; f42->SetReturnType("Int32");
    f42->AddParam("CName", "name");
    f42->AddParam("Vector4", "value");
    rtti->RegisterFunction(f42);

    auto f43 = RED4ext::CGlobalFunction::Create("TestRootGraphFloatPreset", "TestRootGraphFloatPreset", &TestRootGraphFloatPreset);
    f43->flags = flags; f43->SetReturnType("Int32");
    f43->AddParam("Int32", "mode");
    f43->AddParam("Float", "value");
    rtti->RegisterFunction(f43);

    auto f44 = RED4ext::CGlobalFunction::Create("TestRootGraphVectorPreset", "TestRootGraphVectorPreset", &TestRootGraphVectorPreset);
    f44->flags = flags; f44->SetReturnType("Int32");
    f44->AddParam("Int32", "mode");
    f44->AddParam("Vector4", "value");
    rtti->RegisterFunction(f44);

    auto f45 = RED4ext::CGlobalFunction::Create("SetRootGraphFloatPresetPersistent", "SetRootGraphFloatPresetPersistent", &SetRootGraphFloatPresetPersistent);
    f45->flags = flags; f45->SetReturnType("Int32");
    f45->AddParam("Int32", "mode");
    f45->AddParam("Float", "value");
    rtti->RegisterFunction(f45);

    auto f46 = RED4ext::CGlobalFunction::Create("SetRootGraphVectorPresetPersistent", "SetRootGraphVectorPresetPersistent", &SetRootGraphVectorPresetPersistent);
    f46->flags = flags; f46->SetReturnType("Int32");
    f46->AddParam("Int32", "mode");
    f46->AddParam("Vector4", "value");
    rtti->RegisterFunction(f46);

    auto f47 = RED4ext::CGlobalFunction::Create("GetRootGraphPersistentLastResult", "GetRootGraphPersistentLastResult", &GetRootGraphPersistentLastResult);
    f47->flags = flags; f47->SetReturnType("Int32");
    rtti->RegisterFunction(f47);

    auto f48 = RED4ext::CGlobalFunction::Create("DumpPlayerAnimatedObjectRuntime", "DumpPlayerAnimatedObjectRuntime", &DumpPlayerAnimatedObjectRuntime);
    f48->flags = flags; f48->SetReturnType("Int32");
    rtti->RegisterFunction(f48);

    auto f49 = RED4ext::CGlobalFunction::Create("DumpAnimationSystemLookup", "DumpAnimationSystemLookup", &DumpAnimationSystemLookup);
    f49->flags = flags; f49->SetReturnType("Int32");
    rtti->RegisterFunction(f49);

    auto f50 = RED4ext::CGlobalFunction::Create("DumpRootMetaRigTracks", "DumpRootMetaRigTracks", &DumpRootMetaRigTracks);
    f50->flags = flags; f50->SetReturnType("Int32");
    rtti->RegisterFunction(f50);

    auto f51 = RED4ext::CGlobalFunction::Create("TestRootMetaRigTrackPreset", "TestRootMetaRigTrackPreset", &TestRootMetaRigTrackPreset);
    f51->flags = flags; f51->SetReturnType("Int32");
    f51->AddParam("Int32", "mode");
    f51->AddParam("Float", "value");
    rtti->RegisterFunction(f51);

    auto f52 = RED4ext::CGlobalFunction::Create("SetRootMetaRigTrackPresetPersistent", "SetRootMetaRigTrackPresetPersistent", &SetRootMetaRigTrackPresetPersistent);
    f52->flags = flags; f52->SetReturnType("Int32");
    f52->AddParam("Int32", "mode");
    f52->AddParam("Float", "value");
    rtti->RegisterFunction(f52);

    auto f53 = RED4ext::CGlobalFunction::Create("GetRootMetaRigTrackPersistentLastResult", "GetRootMetaRigTrackPersistentLastResult", &GetRootMetaRigTrackPersistentLastResult);
    f53->flags = flags; f53->SetReturnType("Int32");
    rtti->RegisterFunction(f53);

    auto f54 = RED4ext::CGlobalFunction::Create("DumpRootAnimatedObjectFloatArrayCandidates", "DumpRootAnimatedObjectFloatArrayCandidates", &DumpRootAnimatedObjectFloatArrayCandidates);
    f54->flags = flags; f54->SetReturnType("Int32");
    rtti->RegisterFunction(f54);

    auto f55 = RED4ext::CGlobalFunction::Create("TestRootLiveTrackPreset", "TestRootLiveTrackPreset", &TestRootLiveTrackPreset);
    f55->flags = flags; f55->SetReturnType("Int32");
    f55->AddParam("Int32", "mode");
    f55->AddParam("Float", "value");
    f55->AddParam("Int32", "arrayMode");
    rtti->RegisterFunction(f55);

    auto f56 = RED4ext::CGlobalFunction::Create("SetRootLiveTrackPresetPersistent", "SetRootLiveTrackPresetPersistent", &SetRootLiveTrackPresetPersistent);
    f56->flags = flags; f56->SetReturnType("Int32");
    f56->AddParam("Int32", "mode");
    f56->AddParam("Float", "value");
    f56->AddParam("Int32", "arrayMode");
    rtti->RegisterFunction(f56);

    auto f57 = RED4ext::CGlobalFunction::Create("GetRootLiveTrackPersistentLastResult", "GetRootLiveTrackPersistentLastResult", &GetRootLiveTrackPersistentLastResult);
    f57->flags = flags; f57->SetReturnType("Int32");
    rtti->RegisterFunction(f57);

    auto f58 = RED4ext::CGlobalFunction::Create("ReadRootLiveTrackPreset", "ReadRootLiveTrackPreset", &ReadRootLiveTrackPreset);
    f58->flags = flags; f58->SetReturnType("Int32");
    f58->AddParam("Int32", "mode");
    f58->AddParam("Int32", "arrayMode");
    rtti->RegisterFunction(f58);

    // Weapon-aim native hook (M1 instrumentation).
    auto f59 = RED4ext::CGlobalFunction::Create("InstallWeaponAimHook", "InstallWeaponAimHook", &InstallWeaponAimHook);
    f59->flags = flags; f59->SetReturnType("Int32"); rtti->RegisterFunction(f59);

    auto f60 = RED4ext::CGlobalFunction::Create("DumpWeaponAimHookStats", "DumpWeaponAimHookStats", &DumpWeaponAimHookStats);
    f60->flags = flags; f60->SetReturnType("Int32"); rtti->RegisterFunction(f60);

    auto fProjRtti = RED4ext::CGlobalFunction::Create("DumpVRProjectileRtti", "DumpVRProjectileRtti", &DumpVRProjectileRtti);
    fProjRtti->flags = flags; fProjRtti->SetReturnType("Int32"); rtti->RegisterFunction(fProjRtti);

    auto fPaInstall = RED4ext::CGlobalFunction::Create("InstallVRPrepareAttack", "InstallVRPrepareAttack", &InstallVRPrepareAttack);
    fPaInstall->flags = flags; fPaInstall->SetReturnType("Int32"); rtti->RegisterFunction(fPaInstall);
    auto fPaSwap = RED4ext::CGlobalFunction::Create("SetVRPrepareAttackSwap", "SetVRPrepareAttackSwap", &SetVRPrepareAttackSwap);
    fPaSwap->flags = flags; fPaSwap->AddParam("Int32", "on"); rtti->RegisterFunction(fPaSwap);
    auto fPaDump = RED4ext::CGlobalFunction::Create("GetVRPADump", "GetVRPADump", &GetVRPADump);
    fPaDump->flags = flags; fPaDump->AddParam("Int32", "idx"); fPaDump->SetReturnType("Float"); rtti->RegisterFunction(fPaDump);

    auto fProjFind = RED4ext::CGlobalFunction::Create("DumpVRLiveProjectile", "DumpVRLiveProjectile", &DumpVRLiveProjectile);
    fProjFind->flags = flags; fProjFind->SetReturnType("Int32"); rtti->RegisterFunction(fProjFind);
    auto fProjSteer = RED4ext::CGlobalFunction::Create("SetVRProjSteer", "SetVRProjSteer", &SetVRProjSteer);
    fProjSteer->flags = flags; fProjSteer->AddParam("Int32", "on"); rtti->RegisterFunction(fProjSteer);
    auto fProjTick = RED4ext::CGlobalFunction::Create("VRProjSteerTick", "VRProjSteerTick", &VRProjSteerTick);
    fProjTick->flags = flags; rtti->RegisterFunction(fProjTick);
    auto fProjLD = RED4ext::CGlobalFunction::Create("GetVRProjLiveDump", "GetVRProjLiveDump", &GetVRProjLiveDump);
    fProjLD->flags = flags; fProjLD->AddParam("Int32", "idx"); fProjLD->SetReturnType("Float"); rtti->RegisterFunction(fProjLD);

    auto fProvInst = RED4ext::CGlobalFunction::Create("InstallVRProvInstrument", "InstallVRProvInstrument", &InstallVRProvInstrument);
    fProvInst->flags = flags; fProvInst->SetReturnType("Int32"); rtti->RegisterFunction(fProvInst);
    auto fProvOvr = RED4ext::CGlobalFunction::Create("SetVRProvOverrideSlot", "SetVRProvOverrideSlot", &SetVRProvOverrideSlot);
    fProvOvr->flags = flags; fProvOvr->AddParam("Int32", "slot"); rtti->RegisterFunction(fProvOvr);
    auto fProvDump = RED4ext::CGlobalFunction::Create("GetVRProvDump", "GetVRProvDump", &GetVRProvDump);
    fProvDump->flags = flags; fProvDump->AddParam("Int32", "idx"); fProvDump->SetReturnType("Float"); rtti->RegisterFunction(fProvDump);
    auto fProvReset = RED4ext::CGlobalFunction::Create("ResetVRProvCounts", "ResetVRProvCounts", &ResetVRProvCounts);
    fProvReset->flags = flags; rtti->RegisterFunction(fProvReset);
    auto fProvQM = RED4ext::CGlobalFunction::Create("SetVRProvQuatMode", "SetVRProvQuatMode", &SetVRProvQuatMode);
    fProvQM->flags = flags; fProvQM->AddParam("Int32", "mode"); fProvQM->AddParam("Int32", "axis"); rtti->RegisterFunction(fProvQM);
    auto fMuz = RED4ext::CGlobalFunction::Create("SetVRMuzzleQuat", "SetVRMuzzleQuat", &SetVRMuzzleQuat);
    fMuz->flags = flags; fMuz->AddParam("Float","i"); fMuz->AddParam("Float","j"); fMuz->AddParam("Float","k"); fMuz->AddParam("Float","r"); rtti->RegisterFunction(fMuz);
    auto fZoom = RED4ext::CGlobalFunction::Create("SetVRZoomLevel", "SetVRZoomLevel", &SetVRZoomLevel);
    fZoom->flags = flags; fZoom->AddParam("Float","zoom"); rtti->RegisterFunction(fZoom);
    auto fMeleeFire = RED4ext::CGlobalFunction::Create("SetVRMeleeFire", "SetVRMeleeFire", &SetVRMeleeFire);
    fMeleeFire->flags = flags; fMeleeFire->AddParam("Int32","fire"); rtti->RegisterFunction(fMeleeFire);
    auto fMeleeTrig = RED4ext::CGlobalFunction::Create("GetVRMeleeTrigger", "GetVRMeleeTrigger", &GetVRMeleeTrigger);
    fMeleeTrig->flags = flags; fMeleeTrig->SetReturnType("Int32"); rtti->RegisterFunction(fMeleeTrig);
    // Generic shared-slot getter (for the hand-to-holster CET mod + other VR mods that need raw poses).
    auto fGetSlot = RED4ext::CGlobalFunction::Create("GetVRSharedSlot", "GetVRSharedSlot", &GetVRSharedSlot);
    fGetSlot->flags = flags; fGetSlot->AddParam("Int32", "idx"); fGetSlot->SetReturnType("Float");
    rtti->RegisterFunction(fGetSlot);

    auto f61 = RED4ext::CGlobalFunction::Create("SetVRWeaponAim", "SetVRWeaponAim", &SetVRWeaponAim);
    f61->flags = flags; f61->SetReturnType("Int32");
    f61->AddParam("Float", "fx"); f61->AddParam("Float", "fy"); f61->AddParam("Float", "fz");
    f61->AddParam("Float", "px"); f61->AddParam("Float", "py"); f61->AddParam("Float", "pz");
    f61->AddParam("Int32", "enable"); f61->AddParam("Int32", "mode"); f61->AddParam("Float", "gate");
    rtti->RegisterFunction(f61);

    auto f62 = RED4ext::CGlobalFunction::Create("GetWeaponAimStat", "GetWeaponAimStat", &GetWeaponAimStat);
    f62->flags = flags; f62->SetReturnType("Int32"); f62->AddParam("Int32", "which");
    rtti->RegisterFunction(f62);

    auto f63 = RED4ext::CGlobalFunction::Create("SetVRHeadingTest", "SetVRHeadingTest", &SetVRHeadingTest);
    f63->flags = flags; f63->SetReturnType("Int32");
    f63->AddParam("Int32", "force"); f63->AddParam("Float", "yaw"); f63->AddParam("Float", "pitch");
    rtti->RegisterFunction(f63);

    auto f64 = RED4ext::CGlobalFunction::Create("SetVRShotCamera", "SetVRShotCamera", &SetVRShotCamera);
    f64->flags = flags; f64->AddParam("handle:IScriptable", "cam"); rtti->RegisterFunction(f64);

    auto f65 = RED4ext::CGlobalFunction::Create("SetVRShotSnap", "SetVRShotSnap", &SetVRShotSnap);
    f65->flags = flags; f65->SetReturnType("Int32");
    f65->AddParam("Int32", "enable"); f65->AddParam("Int32", "mode"); f65->AddParam("Float", "testYaw");
    rtti->RegisterFunction(f65);

    auto f66 = RED4ext::CGlobalFunction::Create("StartVRCamTrace", "StartVRCamTrace", &StartVRCamTrace);
    f66->flags = flags; f66->SetReturnType("Int32"); f66->AddParam("Int32", "offsetSel"); f66->AddParam("Int32", "gated"); f66->AddParam("Int32", "writeOnly");
    rtti->RegisterFunction(f66);
    auto f67 = RED4ext::CGlobalFunction::Create("StopVRCamTrace", "StopVRCamTrace", &StopVRCamTrace);
    f67->flags = flags; f67->SetReturnType("Int32"); rtti->RegisterFunction(f67);
    auto f68 = RED4ext::CGlobalFunction::Create("DumpVRCamTrace", "DumpVRCamTrace", &DumpVRCamTrace);
    f68->flags = flags; f68->SetReturnType("Int32"); rtti->RegisterFunction(f68);

    auto f70 = RED4ext::CGlobalFunction::Create("DumpVRCamAddr", "DumpVRCamAddr", &DumpVRCamAddr);
    f70->flags = flags; f70->SetReturnType("Int32"); rtti->RegisterFunction(f70);

    auto f71 = RED4ext::CGlobalFunction::Create("SetVRGetOrient", "SetVRGetOrient", &SetVRGetOrient);
    f71->flags = flags; f71->SetReturnType("Int32"); f71->AddParam("Int32", "mode"); f71->AddParam("Float", "testYaw"); f71->AddParam("Int32", "plane");
    rtti->RegisterFunction(f71);

    auto f72 = RED4ext::CGlobalFunction::Create("SetVRSkipHmdTest", "SetVRSkipHmdTest", &SetVRSkipHmdTest);
    f72->flags = flags; f72->SetReturnType("Int32"); f72->AddParam("Int32", "mode"); rtti->RegisterFunction(f72);

    auto fMenu = RED4ext::CGlobalFunction::Create("SetVRMenuOpen", "SetVRMenuOpen", &SetVRMenuOpen);
    fMenu->flags = flags; fMenu->SetReturnType("Int32"); fMenu->AddParam("Int32", "open"); rtti->RegisterFunction(fMenu);

    auto f73 = RED4ext::CGlobalFunction::Create("SetVRHeadLocal", "SetVRHeadLocal", &SetVRHeadLocal);
    f73->flags = flags; f73->SetReturnType("Int32"); f73->AddParam("Int32", "enable"); f73->AddParam("Int32", "conv");
    rtti->RegisterFunction(f73);

    auto f69 = RED4ext::CGlobalFunction::Create("SetVRXformOverride", "SetVRXformOverride", &SetVRXformOverride);
    f69->flags = flags; f69->SetReturnType("Int32"); f69->AddParam("Int32", "mode"); f69->AddParam("Float", "testYaw"); f69->AddParam("Int32", "plane");
    rtti->RegisterFunction(f69);

    // FIRE-SHOT direction lever.
    auto f74 = RED4ext::CGlobalFunction::Create("SetVRFireMode", "SetVRFireMode", &SetVRFireMode);
    f74->flags = flags; f74->SetReturnType("Int32"); f74->AddParam("Int32", "mode"); f74->AddParam("Int32", "plane"); f74->AddParam("Float", "angle"); f74->AddParam("Int32", "neg");
    rtti->RegisterFunction(f74);

    auto f75 = RED4ext::CGlobalFunction::Create("GetVRFireDump", "GetVRFireDump", &GetVRFireDump);
    f75->flags = flags; f75->SetReturnType("Float"); f75->AddParam("Int32", "idx");
    rtti->RegisterFunction(f75);

    auto f76 = RED4ext::CGlobalFunction::Create("SetVRFireScan", "SetVRFireScan", &SetVRFireScan);
    f76->flags = flags; f76->SetReturnType("Int32"); f76->AddParam("Int32", "src"); f76->AddParam("Int32", "base");
    rtti->RegisterFunction(f76);

    auto f77 = RED4ext::CGlobalFunction::Create("SetVRFireOverrideTarget", "SetVRFireOverrideTarget", &SetVRFireOverrideTarget);
    f77->flags = flags; f77->SetReturnType("Int32"); f77->AddParam("Int32", "src"); f77->AddParam("Int32", "off");
    rtti->RegisterFunction(f77);

    auto f78 = RED4ext::CGlobalFunction::Create("GetVRFireHit", "GetVRFireHit", &GetVRFireHit);
    f78->flags = flags; f78->SetReturnType("Float"); f78->AddParam("Int32", "hit"); f78->AddParam("Int32", "field");
    rtti->RegisterFunction(f78);

    // TRACE-DISPATCHER funnel instrumentation.
    auto f79 = RED4ext::CGlobalFunction::Create("SetVRTraceOverride", "SetVRTraceOverride", &SetVRTraceOverride);
    f79->flags = flags; f79->SetReturnType("Int32"); f79->AddParam("Int32", "on"); f79->AddParam("Int32", "writeOff"); f79->AddParam("Int32", "force"); f79->AddParam("Int32", "neg"); f79->AddParam("Int32", "gateRet");
    rtti->RegisterFunction(f79);

    auto f80 = RED4ext::CGlobalFunction::Create("ResetVRTrace", "ResetVRTrace", &ResetVRTrace);
    f80->flags = flags; f80->SetReturnType("Int32"); rtti->RegisterFunction(f80);

    auto f81 = RED4ext::CGlobalFunction::Create("GetVRTrace", "GetVRTrace", &GetVRTrace);
    f81->flags = flags; f81->SetReturnType("Float"); f81->AddParam("Int32", "idx");
    rtti->RegisterFunction(f81);

    auto f82 = RED4ext::CGlobalFunction::Create("GetVRTraceCaller", "GetVRTraceCaller", &GetVRTraceCaller);
    f82->flags = flags; f82->SetReturnType("Float"); f82->AddParam("Int32", "caller"); f82->AddParam("Int32", "field");
    rtti->RegisterFunction(f82);

    auto f83 = RED4ext::CGlobalFunction::Create("SetVRTargetCtrl", "SetVRTargetCtrl", &SetVRTargetCtrl);
    f83->flags = flags; f83->SetReturnType("Int32"); f83->AddParam("Int32", "on"); f83->AddParam("Int32", "neg");
    rtti->RegisterFunction(f83);

    auto f84 = RED4ext::CGlobalFunction::Create("SetVRProjCtrl", "SetVRProjCtrl", &SetVRProjCtrl);
    f84->flags = flags; f84->SetReturnType("Int32"); f84->AddParam("Int32", "on"); f84->AddParam("Int32", "neg"); f84->AddParam("Int32", "unguide"); f84->AddParam("Int32", "always");
    rtti->RegisterFunction(f84);

    auto f86 = RED4ext::CGlobalFunction::Create("GetVRProjDump", "GetVRProjDump", &GetVRProjDump);
    f86->flags = flags; f86->SetReturnType("Float"); f86->AddParam("Int32", "idx");
    rtti->RegisterFunction(f86);

    auto f87 = RED4ext::CGlobalFunction::Create("SetVRProjOriginRow", "SetVRProjOriginRow", &SetVRProjOriginRow);
    f87->flags = flags; f87->SetReturnType("Int32"); f87->AddParam("Int32", "row");
    rtti->RegisterFunction(f87);

    auto f90 = RED4ext::CGlobalFunction::Create("SetVRProjGateRva", "SetVRProjGateRva", &SetVRProjGateRva);
    f90->flags = flags; f90->SetReturnType("Int32"); f90->AddParam("Int32", "rva");
    rtti->RegisterFunction(f90);

    auto f88 = RED4ext::CGlobalFunction::Create("SetVRShotOrigin", "SetVRShotOrigin", &SetVRShotOrigin);
    f88->flags = flags; f88->SetReturnType("Int32"); f88->AddParam("Float", "x"); f88->AddParam("Float", "y"); f88->AddParam("Float", "z");
    rtti->RegisterFunction(f88);

    auto f89 = RED4ext::CGlobalFunction::Create("SetVRFireCamSnap", "SetVRFireCamSnap", &SetVRFireCamSnap);
    f89->flags = flags; f89->SetReturnType("Int32"); f89->AddParam("Int32", "on"); f89->AddParam("Int32", "off");
    rtti->RegisterFunction(f89);

    auto f85 = RED4ext::CGlobalFunction::Create("SetVRFireXform", "SetVRFireXform", &SetVRFireXform);
    f85->flags = flags; f85->SetReturnType("Int32"); f85->AddParam("Int32", "mode"); f85->AddParam("Int32", "off");
    rtti->RegisterFunction(f85);
}

RED4EXT_C_EXPORT void RED4EXT_CALL RegisterTypes() {}

RED4EXT_C_EXPORT bool RED4EXT_CALL Main(RED4ext::v1::PluginHandle aHandle, RED4ext::v1::EMainReason aReason, const RED4ext::v1::Sdk* aSdk) {
    RED4EXT_UNUSED_PARAMETER(aHandle); RED4EXT_UNUSED_PARAMETER(aSdk);
    if (aReason == RED4ext::v1::EMainReason::Load) {
        auto rtti = RED4ext::CRTTISystem::Get();
        rtti->AddRegisterCallback(RegisterTypes);
        rtti->AddPostRegisterCallback(PostRegisterTypes);
    }
    return true;
}

RED4EXT_C_EXPORT void RED4EXT_CALL Query(RED4ext::v1::PluginInfo* aInfo) {
    aInfo->name = L"CyberpunkVR_Hands";
    aInfo->author = L"VRPort";
    aInfo->version = RED4EXT_V1_SEMVER(1, 0, 0);
    aInfo->runtime = RED4EXT_V1_RUNTIME_VERSION_INDEPENDENT;
    aInfo->sdk = RED4EXT_V1_SDK_VERSION_1_0_0_COMPAT_0_5_0;
}

RED4EXT_C_EXPORT uint32_t RED4EXT_CALL Supports() {
    return RED4EXT_API_VERSION_1_COMPAT_0;
}
