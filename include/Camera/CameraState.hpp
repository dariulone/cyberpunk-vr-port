#pragma once

// ================================================================================================
// The rest of the camera link: the arrangement flags, the diagnostic counters, the composed state
// and the small maths the three camera hooks share.
//
// Camera/CameraLink.hpp holds the two objects whose ACCESS PROTOCOL is not a load. This holds the
// plain ones -- same reasoning as Core/LiveControls.hpp, where forty volatile scalars are read
// directly because the `volatile` is the whole synchronisation and a getter per scalar buys nothing.
//
// The arrangement flags in particular MUST be single objects: two or three of the hook files each
// read them, and a per-translation-unit copy would let the files disagree about which site writes
// the camera. That is not a compile error and not a crash; it is two hooks both believing they own
// the write.
// ================================================================================================

#include "Runtimes/OpenXRManager.hpp"

#include <atomic>
#include <cstdint>

// Forward declaration rather than the SDK: three cached RTTI property pointers do not
// justify every translation unit that touches the camera including RED4ext.hpp.
namespace RED4ext { struct CProperty; struct CBaseFunction; }

// ---- state ----
extern RED4ext::CProperty* g_equippedWeaponProp;
extern RED4ext::CProperty* g_isAimingProp;
extern RED4ext::CProperty* g_mountedVehicleProp;
// PlayerPuppet::sceneTier (GameplayTier, int32): 0 = Tier1_FullGameplay .. 3 =
// Tier4_FPPCinematic, 4 = Tier5_Cinematic. Read straight off the player -- no blackboard and no
// CET involved, see the cutscene-suspend block in LocateCamera.cpp.
extern RED4ext::CProperty* g_sceneTierProp;
// VehicleComponent::IsDriver(GameInstance, GameObject) -- a STATIC script function, so it is executed
// with a null instance. Used only to tell the driver seat from a passenger one, for the wheel grab.
// Null when it cannot be resolved, and the caller then falls back to "mounted to anything": the
// feature degrades to slightly too permissive rather than to dead.
extern RED4ext::CBaseFunction* g_isDriverFunc;
// The player's live scene tier, refreshed with the other player flags in LocateCamera and read
// by the pose hook to suspend VRIK during cutscenes. A PLAIN GLOBAL, not a shared slot: both
// ends live in this DLL, so the mapping could only add ways to fail -- and it did. The first
// version published it in slots [157]/[158], which XInput.cpp already writes every input tick
// with the B and Y button flags, so the tier and the threshold were stomped to 0 between every
// write and every read. Atomic because the writer is the camera thread and the reader is the
// animation thread.
extern std::atomic<int> g_sceneTier;
// MAIN's camera object, latched by the PatchCamera owner classification. Needed outside that
// classification by the ADS weapon-zoom sync in LocateCamera.cpp.
extern std::atomic<uintptr_t> g_camObjMain;
// And VRCAM's, which the ADS weapon-zoom sync needs for the same reason it needs MAIN's: the
// weapon layer is magnified per camera object, so writing one eye and not the other is a
// difference between the eyes.
extern std::atomic<uintptr_t> g_camObjVrcam;
extern bool g_isRTTIInitialized;
extern "C" __declspec(dllexport) extern int CyberpunkVR_CamComposeAtWrite;
extern "C" __declspec(dllexport) extern int CyberpunkVR_CamFinalViewScope;
extern "C" __declspec(dllexport) extern int CyberpunkVR_CamWriteInFinal;
extern "C" __declspec(dllexport) extern int CyberpunkVR_CamWriteInPatch;
extern "C" __declspec(dllexport) extern int CyberpunkVR_IpdInPosAB;
extern "C" __declspec(dllexport) extern int CyberpunkVR_IpdInWorldPos;
extern "C" __declspec(dllexport) extern int CyberpunkVR_OneSamplePerFrame;
extern "C" __declspec(dllexport) extern int CyberpunkVR_PoseLocateAtWrite;
extern "C" __declspec(dllexport) extern int CyberpunkVR_PoseReadBack;
extern "C" __declspec(dllexport) extern int CyberpunkVR_VrcamHeadTranslation;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_DebugFinalAge;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_DebugFinalTies;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_DebugTidLocateCam;
extern "C" __declspec(dllexport) extern uint32_t CyberpunkVR_DebugTidPatchCam;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugCamComposed;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugCamNoHmd;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugCamThreadSwitches;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugCamVrcamFirst;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugFinalMatch;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugFinalNoMatch;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugFinalTieHits;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugIpdWorldWrites;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugPatchCamMain;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugPatchCamOther;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugPatchCamVrcam;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugPoseFromCache;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugPoseLocatedAtWrite;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugViewCamMain;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugViewCamOther;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugViewCamVrcam;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugVrcamPosWrites;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugMainPosWrites;
extern "C" __declspec(dllexport) extern int CyberpunkVR_HeadTranslationInPatch;
extern "C" __declspec(dllexport) extern int CyberpunkVR_HeadingFromPreWrite;
extern "C" __declspec(dllexport) extern int CyberpunkVR_DeltaFromFreshSample;
extern "C" __declspec(dllexport) extern float CyberpunkVR_HeadingLeadFrames;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugHeadingStepDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugHeadingPreWriteDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugHeadingCachedDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugBodyYawLagDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugBodyYawStepDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugHipsYawStepDeg;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugYawToSolveMaxMs;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugYawToSolveMinMs;
extern "C" __declspec(dllexport) extern float CyberpunkVR_DebugYawLagWriteDeg;
// Published by the yaw write site itself (src/Hooks/BodyYawCensus.cpp).
extern "C" __declspec(dllexport) extern double CyberpunkVR_DebugYawWriteMs;
extern "C" __declspec(dllexport) extern float  CyberpunkVR_EngineBodyYawZ;
extern "C" __declspec(dllexport) extern float  CyberpunkVR_EngineBodyYawW;
extern "C" __declspec(dllexport) extern int    CyberpunkVR_EngineBodyYawValid;
// 1 = the solve converts world->model with the engine body yaw above instead of the view packet's
// heading. Measured reason at the use site.
extern "C" __declspec(dllexport) extern int    CyberpunkVR_VrikYawFromEngine;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_ViewYawFromEngine;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_BodyYawFinalRad;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_BodyYawFinalValid;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_PlayerEntityPos[3];
extern "C" __declspec(dllexport) extern float    CyberpunkVR_PlayerEntityQuat[4];
extern "C" __declspec(dllexport) extern int      CyberpunkVR_PlayerEntityValid;
// 1 = the pose path uses plugin-owned, phase-coherent sources: a seqlocked relative camera/entity
// snapshot for position and AcquireFrameHeadSample for orientation.  It must never combine the
// current player entity with g_lastLocate*, because LocateCamera publishes that pose after the
// animation solve and therefore leaves a one-frame locomotion/head-turn error here.
extern "C" __declspec(dllexport) extern int      CyberpunkVR_VrikTransformsFromPlugin;
// Prefer the per-frame native camera(N-1)/entity(N-1) snapshot. The Lua snapshot above remains a
// startup/failure fallback and a live A/B path.
extern "C" __declspec(dllexport) extern int      CyberpunkVR_VrikNativeFramePair;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_VrikVehicleFullEntityQuat;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_VehicleAnchorFromViewYaw;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_CamWriteOrientInVehicle;
// The yaw the VIEW was composed with, published by PatchCamera at the instant it uses it and consumed by
// LocateCamera in the same frame (PatchCamera writes the camera; LocateCamera runs downstream of it).
extern "C" __declspec(dllexport) extern int CyberpunkVR_YawCatchUpOnSharedEpoch;
extern "C" __declspec(dllexport) extern unsigned long long CyberpunkVR_DebugYawCaughtUp;
extern volatile float g_viewYawUsedRad;
extern volatile int   g_viewYawUsedValid;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugVrikNativePairUsed;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugVrikLuaPairFallback;
// 1 = keep the fresh XR orientation for the head bone only; arms and model/world conversion use
// the stable camera/entity-push orientation expected by controller composition. Live A/B is
// independent of VrikTransformsFromPlugin so testing it cannot re-enable the old locomotion anchor.
extern "C" __declspec(dllexport) extern int      CyberpunkVR_VrikSplitHeadHandRot;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_VrikArmAnchorFromBody;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_VrikElbowPolicy;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_BodyYawFollow;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_BodyYawRealignRad;
extern "C" float BodyYawBridgeCurrentRealignRad();
extern "C" float BodyYawBridgeRealignForEpoch(uint64_t epoch);
extern "C" void BodyYawBridgeAcknowledgeRenderedCamera(uint64_t epoch, float bridgeUsed);
extern "C" bool BodyYawBridgeNeedsProtection();
extern "C" __declspec(dllexport) extern float    CyberpunkVR_BodyYawFollowDeadDeg;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_DebugBodyFollowOffsetDeg;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_DebugBodyFollowErrDeg;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_BodyYawFinalValid;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_BodyYawFinalRad;
extern "C" __declspec(dllexport) extern unsigned long long CyberpunkVR_DebugBodyFollowApplied;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_DebugHandWorldErrMm;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_DebugHandMissMm;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_WeaponKickDeg;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_DebugHandReachRatio;
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_EngineCamPosFP[3];
extern "C" __declspec(dllexport) extern int      CyberpunkVR_EngineCamPosValid;
extern "C" __declspec(dllexport) extern float    CyberpunkVR_DebugCamMountM;
extern "C" __declspec(dllexport) extern int      CyberpunkVR_CamMountCompensate;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugViewYawFromEngine;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugYawWritesAll;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugYawWritesPlayer;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugHeadingLedComps;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugDeltaRebuilt;
extern "C" extern int CyberpunkVR_MainIsRightEye;
extern float g_dbgEntryYaw;
extern std::atomic<int32_t> g_headDeltaFP[3];
extern std::atomic<uint32_t> g_headDeltaValid;
extern std::atomic<uint64_t> g_camComposedForPresent;
extern uint64_t g_finalCameraHits;
extern uint64_t g_locateCameraHits;
extern uint64_t g_patchCameraHits;
extern volatile float g_anchorCy;
extern volatile float g_anchorOff[3];
extern volatile float g_anchorScale;
extern volatile float g_engineCamQuat[4];
extern volatile float g_headQuatComposed[4];
extern volatile float g_headingCy;
extern volatile float g_headingSy;
// The GAME's own camera pitch, in radians, and the pitch quaternion (s,0,0,c) built from it.
//
// Published by the pitch hook and consumed by both compose sites. Zero whenever
// xr_disable_mouse_y is on, which is the default -- the headset supplies vertical look and the
// game's pitch also drives a constrained pivot offset that moves the head relative to the body.
// With the option OFF these carry the game pitch so that mouse and right-stick Y look up and down
// again, composed with the HMD orientation rather than replacing it (dabinn, TofuExpress 11974ee5).
extern volatile float g_gamePitchRadians;
extern volatile float g_headingPitchS;
extern volatile float g_headingPitchC;
extern volatile float g_lastLocateQuat[4];
extern volatile int g_anchorRecipeValid;
extern volatile int32_t g_lastIpdShiftFP[3];
extern volatile int32_t g_lastLocatePosFP[3];
extern volatile uint32_t g_engineCamQuatValid;
extern volatile uint32_t g_headingValid;
extern volatile uint32_t g_lastLocateSeq;
extern volatile uint32_t g_renderedSeq;

// Declarators the generator could not see because they share a line with a sibling -- and one
// (g_headQuatValid) shared a line with a whole second DEFINITION, which is exactly why this split
// is done by symbol and not by line range. That line is now two lines.
extern volatile float g_anchorSy;
extern float g_dbgEntryPosX;
extern float g_dbgEntryPosY;
extern float g_dbgEntryPosZ;
extern volatile uint32_t g_headQuatValid;

// ---- shared helpers ----
bool IsPlausibleCameraSpan(const float* a, const float* b);
bool IsPlausibleUnitQuaternion(const float* q);
bool IsPlausibleUnitVector3(const float* v);
bool LooksProjectionLike(const float* values, size_t count);
bool ReadFloatArraySafe(const float* src, float* out, size_t count);
bool WriteU32Safe(uintptr_t addr, uint32_t value);
// REPAIRED. This was ONE 2,154-character line: a whole block of declarations and their comments,
// collapsed together by the generator that produced this header. Everything after its first `//` was a
// comment, so NINE declarations were silently switched off. Nothing failed to build, because every
// user of them had its own local `extern` -- which is exactly the state this header exists to end.

// 1 = give VRCAM the same head translation MAIN gets. Live-switchable, to isolate it.
extern "C" __declspec(dllexport) extern int CyberpunkVR_VrcamHeadTranslation;

// 1 = hold the gamepad LT back on foot with empty hands, so striking the smoking lighter does not also
// pull the camera into aim-zoom. Driving is never gated: no weapon is equipped in a car, and that is
// where LT is the brake.
extern "C" __declspec(dllexport) extern int CyberpunkVR_LtLighterGate;

// 1 = put the eye separation into the component WORLD POSITION (component+0xE0), above the view
// producer, so culling, shadows, the distant pass and motion vectors all see the eye they are drawn for.
extern "C" __declspec(dllexport) extern int CyberpunkVR_IpdInWorldPos;
extern "C" extern int CyberpunkVR_MainIsRightEye;

// The legacy write into component+0x100/0x110 ("posA/posB"), OFF: measured to have no effect on the
// rendered viewpoint -- the two render cameras stayed 23 MICROMETRES apart with it enabled. Kept
// switchable only so the old behaviour can be restored in one session if something turns out to depend
// on those fields for a reason we have not found.
extern "C" __declspec(dllexport) extern int CyberpunkVR_IpdInPosAB;

extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugIpdWorldWrites;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugVrcamPosWrites;

// How often the camera write arrives on a DIFFERENT thread than the previous one. A value that stays
// near 1 means the site is effectively single-threaded for cameras; one that climbs with the frame count
// means it is not -- which is why the composition is a compare-exchange and the quaternion a seqlock
// rather than four plain stores.
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugCamThreadSwitches;

extern "C" __declspec(dllexport) int CyberpunkVR_IsVrcamViewActive();
extern "C" uint32_t GetRenderedCameraSeq();
float GetDesiredHalfIpd();
float GetWorldScale();
int ClassifyPatchCameraOwner(void* ownerState);
// Is the player looking through a device camera right now -- a surveillance camera it has taken over.
// True for 300 ms after the last write to such a camera component, so it arms and clears itself with no
// polling and no script call (the periodic poll runs on the worker thread, where the script VM is not
// safe to touch). Read by the VRIK suspend in src/Hooks/AnimPose.cpp.
bool DeviceCamActive();
// The device camera's own aim and place, latched once per takeover in ClassifyPatchCameraOwner's
// neighbourhood and consumed by the camera writer: the base the head pose is composed onto, and the
// position the second eye is moved to.
extern float g_devCamBase[4];
extern std::atomic<int> g_devCamBaseValid;
extern std::atomic<int32_t> g_devCamPosFP[3];
extern std::atomic<int> g_devCamPosValid;
// The gate and the target the script side publishes: 1 while the player controls a remote camera, and
// that camera's world position in 1/131072 m. Written by the VRRemoteCamera native.
// The head-steering experiment for a device camera: off by default, see src/Core/VrCore.cpp for why.
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_DeviceCamOrient;
extern float g_devCamLastWritten[4];
extern float g_devCamViewQuat[4];
extern std::atomic<int> g_devCamViewValid;
// The lens heading, as yaw and pitch, published by the device camera's own write and consumed by every
// camera's composition while a takeover is live -- one base for all three, which is what stops the
// per-epoch composer race from handing one camera another's base.
extern float g_devCamAimYaw;
extern float g_devCamAimPitch;
extern std::atomic<int> g_devCamAimValid;
// The camera's authored FOV and whether it has been saved, so the object is handed back unchanged.
extern float g_devCamFovOrig;
extern std::atomic<int> g_devCamFovSaved;
void DeviceCamRestoreFov();
// How many times a device camera has been patched, so its rate can be compared against the other two in
// the census line -- a view that is not patched in a frame keeps whatever the engine left in it.
extern "C" __declspec(dllexport) extern unsigned int CyberpunkVR_DebugPatchCamDevice;
extern std::atomic<int> g_remoteCamOn;
extern std::atomic<int32_t> g_remoteCamPosFP[3];
void ApplyFinalCameraOrientationFromQuat(float* rsiPtr, const float* q);
void ComputeRightVectorFromQuaternion(const float* q, float* outRight);
void InitializeMountedVehicleCache();
void LogMatrix4x4(const char* prefix, const float* values);
void MulQuat(float ax, float ay, float az, float aw, float bx, float by, float bz, float bw, float& ox, float& oy, float& oz, float& ow);
void NormalizeQuat(float& x, float& y, float& z, float& w);
void WriteRenderCameraBasis(float* rsiPtr, const float* q);
