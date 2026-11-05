#pragma once

// The solve for the player's own bones, and the shared-block reader that feeds it. Declared here so
// the pose-apply detour in src/Hooks/AnimPose.cpp can call them without either file including the
// other's implementation.
//
// Signatures are copied from the definitions, never retyped.

#include "Anim/VrikState.hpp"

#include <cstdint>
#include <cstddef>
#include <atomic>

// ONE RENDERED FRAME, COUNTED INSIDE THIS DLL. Bumped once per Present (src/Runtimes/
// OpenXRPresent.cpp), read by the solve as the key its cache is valid for.
//
// It replaces g_pSharedHands[99], which was pushed from CET Lua's onUpdate through the shared block.
// That made the arms' freshness depend on a Lua callback: a fresh solve happened only when Lua got
// round to advancing the counter, and every pass in between replayed the previous pose. Two thread
// hops and a script scheduler stood between a hand moving and the arm following it, which is what
// "VRIK looks laggy" was.
//
// WHY A FRAME AND NOT THE XR SAMPLE. The engine applies the player pose 4-5 times per rendered frame,
// and every pass must leave the buffer identical or mid-frame consumers (shadow, reflection, the
// render snapshot) catch two different arm positions inside one image -- measured, and it looked like
// a second arm on snap turns. Keying on the XR publish (72/s against ~52 frames) would reintroduce
// exactly that. One epoch per frame keeps the passes coherent while taking the script out of the path.
extern std::atomic<uint32_t> g_VrikFrameEpoch;

// Peak second difference per hand, per stage (0 ctrl as the solve reads it, 1 anchor, 2 target,
// 3 the same controller sampled uniformly on the XR thread), in mm. See the definition in
// src/Anim/CharacterRig.cpp -- stage 3 against stage 0 is what separates aliasing from noise.
// Peak mm the engine had moved our bones before we wrote them back -- the width of the
// window in which the skeleton is not ours. See src/Hooks/AnimPose.cpp.
extern int   g_VrikOverwritePassHist[8];
extern int   g_VrikBatchGapNew;
extern int   g_VrikBatchGapSame;
extern float g_VrikBatchGapNewMin;
extern float g_VrikBatchGapSameMax;
extern float g_VrikEngineOverwriteMm;
extern int   g_VrikEngineOverwriteBone;
extern int   g_VrikEngineOverwriteHits;
extern int   g_VrikEngineOverwritePasses;
extern float g_VrikShakePeakMm[2][4];   // any motion
extern float g_VrikShakeSlowMm[2][4];   // slow motion only -- the number that answers the report
extern int   g_VrikShakeSlowN[2][4];
void VRIK_NoteShake(int hand, int stage, const float* pos);

extern bool  g_viewPktValid;
extern float g_fkRot[VRIK_MAX_BONES][4];
extern float g_solveCacheSnapCtr;
extern uint32_t g_solveCacheTick;
extern float g_solveCacheVal[VRIK_MAX_BONES][7];
extern float g_solveCacheYaw;
extern float g_viewPkt[17];
extern float s_vrSharedSquatDrop;
extern int   g_solveCacheIdx[VRIK_MAX_BONES];
extern int   g_solveCacheN;
extern uint32_t g_handsStableSeq;
// outEntityQuat receives the exact world->model basis source used for outPos.  Keeping it with the
// result prevents callers from converting another world-space offset with a newer entity yaw.
// outPairedRot receives the camera rotation from that same camera/entity publication: the stable
// base frame expected by the existing controller-packet composition.  outRot may be replaced by
// the fresh XR sample for the head bone and must not then be used with controller-packet data.
// outBodyBase receives the native camera base from THAT SAME publication, in
// model space, with no physical/bake/IPD translation. Never subtract a later
// g_headDeltaFP from outPos to reconstruct it.
enum class VrikCameraStatus { Unavailable, Valid, Invalidated };
bool VRIK_ComputeCamModel(float* outPos, float* outRot, float* outEntityQuat = nullptr,
                          float* outPairedRot = nullptr, float* outBodyBase = nullptr,
                          VrikCameraStatus* status = nullptr);
// Called before any fresh-solve writes. A brief publication miss may replay the
// last complete upper pose; explicit invalidation or expiry releases ownership.
bool VRIK_RestoreMissingCamera(uint8_t* boneBuf,VrikCameraStatus status,uint32_t tick,uint64_t nowMs);
void VRIK_CaptureLocalArmCache(uint8_t* boneBuf,bool right,bool left);
void VRIK_ReplaySolveCache(uint8_t* boneBuf);
bool VRIK_ResolveViewPos(float out[3]);
void VRIK_HeadResidual(float fallbackConsumedRight,float fallbackConsumedForward,float out[3]);
// Same paired model origin used by the torso, without a round trip through Lua
// world-position mirrors. The view delta is added exactly once.
bool VRIK_ResolveViewModelPos(const float* pairedCamModel,const float* pairedEntityQuat,float out[3]);
bool VRIK_AlignViewToHands(const float* viewWorldRot,const float* viewHeadXr,
                          const float* handHeadXr,float outWorldRot[4]);
void VRIK_BuildViewHandTarget(const float* viewModelPos,const float* viewModelRot,
                             const float* handLocalPos,const float* handLocalRot,
                             float scale,const float* offset,float target[3],float controllerModelRot[4]);
float VRIK_Dot3(const float* a, const float* b);
float VRIK_Norm3(float* v);
int VRIK_FKCount();
bool VRIK_ReferenceFingerRotation(int bone,float* rotation);
// A bone's parent-local transform in the rig's reference pose; false when the rig has none for it.
bool VRIK_ReferenceLocal(int bone,float* position,float* rotation);
// ...and its model-space transform there.
bool VRIK_ReferenceModel(int bone,float* position,float* rotation);
int VRIK_LadderFingerBone(int side,int slot);
void VRIK_ApplyHandStop(int side, float* target, float* handRot);
void VRIK_BodyAxesFromCamYaw(const float* camModelRot, float* bodyRight, float* bodyUp, float* bodyFwd);
void VRIK_ComputeFK(uint8_t* boneBuf, int count);
// Runtime name/parent tables, never hardcoded indices. Reference transforms use
// the engine's48-byte local QsTransform layout.
void VRIK_ConfigureRigPolicy(const char* const* names,int count,const uint8_t* reference,int referenceCount,bool female=false);
// V's reference pose for full-body tracking calibration (Fbt/Game/Calibration.cpp), model space (game axes: x right,
// y forward, z up, metres): the eye midpoint (V's view point), the eye midpoint in the Head bone's frame, and the
// bones the trackers drive. Taken from the loaded rig's reference pose, without the port's neutral spine flex.
struct VrikFbtReference {
    bool valid{},female{};
    float eye[3]{},eyeInHead[3]{};
    float hipsPos[3]{},hipsRot[4]{0,0,0,1},headPos[3]{},headRot[4]{0,0,0,1};
    float footPos[2][3]{},footRot[2][4]{{0,0,0,1},{0,0,0,1}};   // [0] left, [1] right
    float upperArmPos[2][3]{},foreArmPos[2][3]{};               // the arms' reference direction (T or A pose)
    float handPos[2][3]{};                                       // the wrists (the hand bones)
};
bool VRIK_ReadFbtReference(VrikFbtReference* out);
// The rig file last written for offline IK tools (fbt_rig_<male|female>_<bones>.txt next to the game) for that body:
// the shared full-body IK (src/Fbt) loads V's skeleton from it. False until such a rig has loaded.
bool VRIK_FbtRigPath(bool female,char* out,size_t size);
// Full-body tracking (Fbt/Game/Body.cpp) wrote this leg bone in this body solve: the model cache replays it, as it
// does the crouch bend's legs.
void VRIK_MarkLegOwned(int bone);
// While full-body tracking calibrates: V's body (Hips and everything under it) in the rig's reference pose (T-pose),
// written over whatever the animation and the solve left. The fppCamera bones keep their model pose, so the view does
// not move. Called at the end of every player pose pass.
void VRIK_HoldReferencePose(uint8_t* boneBuf);
extern bool g_VRUpperOwned[VRIK_MAX_BONES];
extern int g_VRShadowSource[VRIK_MAX_BONES];
extern bool g_VRHeadReferenceValid;
extern float g_VRHeadReferenceModelRot[4];
extern bool g_solveCacheModel;
void VRIK_SyncShadowUpper(uint8_t* boneBuf,bool preserveLowerBody=false);
void VRIK_CaptureModelCache(bool preserveLowerBody=false);
void VRIK_ReplayModelCache(uint8_t* boneBuf);
void VRIK_PublishBodyBones();
void VRIK_BeginBodySolve(uint8_t* boneBuf);
extern float g_VRIKBodyBendAngle,g_VRIKBodyBendPelvis[3];
extern float g_VRIKBodyEyeTarget[3],g_VRIKBodyEyeSolved[3];
extern bool g_VRIKBodyEyeBound;
extern bool g_VRIKVehicleUpperActive;
bool VRIK_PlaceSwimmingBody(uint8_t* boneBuf,const float* cameraTarget,const float* headRotation,int headIdx);
void VRIK_PlaceLadderUpper(uint8_t* boneBuf,const float* cameraTarget,const float* headRotation,int headIdx);
// No tuning arguments in gameplay: use the baked, approved rung pose.
// Explicit arguments remain for offline authoring/equivalence checks.
void VRIK_ApplyLadderFingers(uint8_t* boneBuf,int side,int kind,const float* curls=nullptr,const float (*adjustments)[6]=nullptr);
extern float g_VRIKGirdleReach[2][4];
extern float g_VRIKHandPin[2][3];   // R, L: the solved (drawn) wrist, model space; the target when not short
extern float g_VRIKHandAim[2][8];   // R, L: the controller's wrist target and rotation (x y z, qx qy qz qw, valid),
                                    // model space, before the reach, the stops and the blends move it
// The controller's own wrist (g_VRIKHandAim), noted where the solve has built it from tracking: what a pointer aims
// with, the hand the player holds rather than the one the arm could reach.
void VRIK_NoteHandAim(int pin, const float* target, const float* rotation);
void VRIK_Cross3(const float* a, const float* b, float* o);
void VRIK_DampenTorsoWeaponPose(uint8_t* boneBuf);
void VRIK_LatchViewPacket();
void VRIK_PinGirdleTranslations(uint8_t* boneBuf);
void VRIK_ResetShoulderReference(uint8_t* boneBuf,int upperIdx);
bool VRIK_BodyAxesFromRig(float* bodyRight,float* bodyUp,float* bodyFwd,bool upperTorso=false);
void VRIK_WorldToModelRotation(float fallbackYaw,const float* entityRotation,float* result);
void VRIK_AnchorShoulder(uint8_t* boneBuf,int upperIdx,const float* anchor,bool isLeft,
                         const float* bodyRight,const float* bodyUp,float halfWidth,float* outJoint,bool neckAnchor=true);
// The thorax turning into a grip the left arm cannot reach -- once per fresh solve, after the shoulders' anchor
// point is read and before either shoulder is anchored. See the definition.
void VRIK_GripTorso(uint8_t* boneBuf,const float* bodyRight,const float* bodyUp);
extern bool g_VRIKTorsoOn;
void VRIK_UpdateForearmDeformation(uint8_t* boneBuf,int foreIdx,int handIdx,bool left);
void VRIK_PlaceBodyUnderHMD(uint8_t* boneBuf, const float* camModelPos, const float* camModelRot,
                          int headIdx, const float* bodyFwd,const float* pairedHeadDeltaModel=nullptr,
                          const float* bodyOrbitModel=nullptr);
bool VRIK_PlaceVehicleCombatUpper(uint8_t* boneBuf,const float* cameraTarget,
                                 const float* headRotation,int headIdx);
void VRIK_QuatFromTo(const float* a, const float* b, float* o);
void VRIK_QuatScale(const float* q, float t, float* o);
void VRIK_RemapAxis(int preset, const float* v, float* o);
void VRIK_ScaleArmBonesFromRest(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount, int foreIdx, int handIdx, float userArmLen);
// The inverse, for an arm handed to the driving animation: the rig's own rest translations back, the
// captured shoulder protraction rest included. See the definition for why the engine will not do it.
void VRIK_RestoreArmRestTrans(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount, int upperIdx, int foreIdx, int handIdx, bool isLeft);
void VRIK_SolveArm(uint8_t* boneBuf, int upperIdx, int foreIdx, int handIdx, const float* targetModel, const float* handModelRot, const float* bodyRight, const float* bodyUp, const float* bodyFwd, float poleAngleRad, float swingGain, bool isLeft, bool storeDbg,bool ladderReach=false);
void VRIK_TwistAbout(const float* q, const float* a, float* outT);
void VRIK_WriteLocalPos(uint8_t* boneBuf, int idx, const float* parentModelPos, const float* parentModelRot, const float* modelPos);
void VRIK_WriteLocalRot(uint8_t* boneBuf, int idx, const float* parentModelRot, const float* modelRot);
void VRIK_WriteHand(uint8_t* boneBuf, int bIdx, const float* headPos, const float* headQuat, bool headOk, const float* vrPos, const float* vrQuat, bool writeRot);

// Copied from the definition. The generator's pattern let a greedy character class swallow the
// name itself, so this one came back "not found" three passes in a row -- a reminder that a
// generator is only worth what its failures cost to notice.
void VRIK_BuildHandTarget(const float* shoulderModelPos,
                          const float* calibratedShoulderBody,
                          const float* hmdRel,
                          const float* vrPos, const float* vrQuat,
                          const float* wristCorr,
                          float scale,
                          const float* off,
                          float* outTarget, float* outHandRot);
