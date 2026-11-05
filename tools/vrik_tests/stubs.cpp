// Headless boundaries only. Solver, FK and packet readers are production code.
#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Camera/CameraState.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "test_state.hpp"
#include <cstdlib>
#include <string>

float* g_pSharedHands=nullptr;
int16_t g_VRBoneParent[VRIK_MAX_BONES]{};
volatile int g_VRBoneCount=0,g_VRFKCount=0;
volatile int g_VRHipsIdx=2,g_VRNeckIdx=16,g_VRNeck1Idx=19,g_VRHeadBoneIdx=22;
volatile int g_VRRightUpLegIdx=6,g_VRRightLegIdx=9,g_VRRightFootIdx=12;
volatile int g_VRLeftUpLegIdx=5,g_VRLeftLegIdx=8,g_VRLeftFootIdx=11;
volatile int g_VRRightUpperArmIdx=18,g_VRRightForeArmIdx=21,g_VRRightBoneIdx=24;
volatile int g_VRLeftUpperArmIdx=17,g_VRLeftForeArmIdx=20,g_VRLeftBoneIdx=23;
int g_VRSpineIdx[8]{4,7,10,13,91,92,93,94};
volatile int g_VRSpineCount=8,g_VRPoseCapGen=0;
volatile int g_VRDiagCapture=0;
volatile int g_VRBind=4;
std::atomic<uint32_t> g_VrikFrameEpoch{};
bool testCyberwareWorldFrame=false;
bool VRIK_ReadCurrentBodyFrame(float* p,float* q,float* yaw){if(!testCyberwareWorldFrame)return false;std::fill_n(p,3,0.f);std::fill_n(q,4,0.f);q[3]=1;*yaw=0;return true;}
int g_VRForeTwistR[3]{-1,-1,-1},g_VRForeTwistL[3]{-1,-1,-1};
volatile float g_VRHeadDrop=.08f,g_VRSquatThreshold=.20f;
volatile float g_VRBodyBone[11][3]{};
volatile int g_VRBodyBoneOk[11]{};
volatile float g_VRIKDbgChest[3]{},g_VRIKDbgChestTgt[3]{};
volatile float g_VRIKDbgClav[2][8]{},g_VRIKDbgShModel[3]{},g_VRIKDbgHipsYaw{};

std::string VRDiagPath(const char* name) { return name; }
void Log(const char*,...) {}
// Fixture camera frames have no outstanding body-yaw follow offset.
float BodyYawFollowOffset() { return 0; }
float BodyTrackingYaw(float nativeYaw) { return nativeYaw; }

VrikTransformSnapshot testNativePair{},testLuaPair{};
bool testNativeAvailable=false,testLuaAvailable=false;
bool VRIK_ReadTransformSnapshot(VrikTransformSnapshot* out) { if(!testLuaAvailable)return false;*out=testLuaPair;return true; }
bool VRIK_ReadNativeTransformSnapshot(VrikTransformSnapshot* out) { if(!testNativeAvailable)return false;*out=testNativePair;return true; }
// These external runtime boundaries are deliberately fatal if a test reaches
// them without supplying a pose. No GPU/runtime is silently simulated as success.
OpenXRManager& OpenXRManager::Get() { std::abort(); }
namespace cvr::swimming { bool Active() { return false; } }
namespace cvr::ladder { bool Active() { return false; } }
bool OpenXRManager::GetHeadPose(OpenXRHeadPose*) const { std::abort(); }
bool OpenXRManager::GetNativeFrameHead(OpenXRHeadPose*,uint64_t*,uint64_t*,XrTime*,cvr::roomscale::Vec2*) const { std::abort(); }
bool OpenXRManager::LocateHeadPoseAt(XrTime,OpenXRHeadPose*) { std::abort(); }
void OpenXRManager::UpdateBodyPosture(OpenXRHeadPose&) { std::abort(); }

volatile float g_VRBindScale=1,g_VRBindOffX=0,g_VRBindOffY=0,g_VRBindOffZ=0;
volatile int g_VRBindAxis=0,g_VRUseHeadRelative=0;
volatile float g_VRCamI=0,g_VRCamJ=0,g_VRCamK=0,g_VRCamR=1;
volatile float g_VRCamPosX=0,g_VRCamPosY=0,g_VRCamPosZ=0;
volatile float g_VREntityPosX=0,g_VREntityPosY=0,g_VREntityPosZ=0;
volatile float g_VREntityQI=0,g_VREntityQJ=0,g_VREntityQK=0,g_VREntityQR=1;
volatile float g_VRCamPairLocalX=0,g_VRCamPairLocalY=0,g_VRCamPairLocalZ=0;
volatile int g_VRCamPairValid=0,g_VRCamPosValid=0;
volatile float g_VRUserArmLenR=0,g_VRUserArmLenL=0,g_VRPlayerYaw=0;
volatile float g_VRIKDbgTarget[3]{},g_VRIKDbgShoulder[3]{},g_VRIKDbgElbow[3]{},g_VRIKDbgLens[2]{},g_VRIKDbgLocal[4]{};
volatile float g_VRIKDbgTargetL[3]{},g_VRIKDbgShoulderL[3]{},g_VRIKDbgElbowL[3]{},g_VRIKDbgLensL[2]{},g_VRIKDbgLocalL[4]{};
volatile float g_headingPitchS=0,g_headingPitchC=1;
volatile uint32_t g_headingValid=0;
bool g_isInVehicle=false;
bool g_hasWeaponEquipped=false;
volatile int g_VRSmokeFingerCount=0,g_VRSmokeFingerCountL=0;
int g_VRSmokeFingerIdx[32]{},g_VRSmokeFingerIdxL[32]{};
volatile int g_VRSmokeFingerActive=0,g_VRSmokeFingerActiveL=0;
volatile int g_VRRestFingerHave=0,g_VRRestFingerCount=0,g_VRRestFingerApply=1;
float g_VRRestFingerRot[32][4]{};
volatile int g_VRReloadFingerActive[2]{};
float g_VRReloadFingerRot[2][32][4]{};
volatile int g_VRReloadFingerSet[2][32]{};
volatile float g_VRReloadFingerBlend[2]{};
char g_VRSmokeFingerName[32][48]{},g_VRSmokeFingerNameL[32][48]{};
extern "C" {
int CyberpunkVR_RuntimeDiagnostics=1; // tests explicitly inspect diagnostic counters
int CyberpunkVR_CarryLeft=0;
float CyberpunkVR_CarryGripBlend=0;
int CyberpunkVR_CamComposeAtWrite=1,CyberpunkVR_CamWriteInPatch=1,CyberpunkVR_HeadTranslationInPatch=1;
int CyberpunkVR_HeadingFromPreWrite=0,CyberpunkVR_EngineBodyYawValid=0,CyberpunkVR_ViewYawFromEngine=0;
float CyberpunkVR_EngineBodyYawZ=0,CyberpunkVR_EngineBodyYawW=1,CyberpunkVR_BodyYawRealignRad=0;
int CyberpunkVR_VrikTransformsFromPlugin=1,CyberpunkVR_VrikNativeFramePair=1,CyberpunkVR_VrikVehicleFullEntityQuat=0;
uint64_t CyberpunkVR_DebugVrikNativePairUsed=0,CyberpunkVR_DebugVrikLuaPairFallback=0;
int CyberpunkVR_VrikElbowPolicy=0;
}
