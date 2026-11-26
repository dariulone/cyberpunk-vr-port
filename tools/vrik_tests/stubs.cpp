// Headless boundaries only. Solver, FK and packet readers are production code.
#include "Camera/GazeCenter.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/BladeContactPose.hpp"
#include "Camera/CameraState.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include "Runtimes/Haptics.hpp"
#include "Camera/CameraLink.hpp"
#include "Camera/PoseIdentity.hpp"
#include "Hooks/LadderInput.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Lobby/MenuLobby.hpp"
#include "Fbt/Game/Body.hpp"
#include "Fbt/Game/Calibration.hpp"
#include <vector>
#include "test_state.hpp"
#include "Hooks/HookServices.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Hooks/LadderInput.hpp"
#include "Hooks/KeypadInput.hpp"
#include "Hooks/CyberwareChord.hpp"
#include "Natives/Reach.hpp"
#include "Natives/VrMenu.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Quest/StoryAttention.hpp"
#include "Lobby/Intro.hpp"
#include "Lobby/MenuLobby.hpp"
#include <memory>
#include "Hooks/Hook.hpp"
#include "Runtimes/TrackedBodyYaw.hpp"
#include "Anim/VrikState.hpp"
#include <MinHook.h>
#include <cstdlib>
#include <cstring>
#include <cstdarg>
#include <cstdio>
#include <string>
#include <string_view>

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
volatile int g_VRBodyFemale=0;
volatile int g_VRBind=4;
bool testCyberwareWorldFrame=false;
// The body hooks (src/Hooks/RoomscaleMove.cpp, BodyHeading.cpp, BodyYawFollow.cpp, or their Rust port) stand in here, except
// in the room trace's programs (VRIK_ROOM_TESTS), which run them.
#ifndef VRIK_ROOM_TESTS
bool VRIK_ReadCurrentBodyFrame(float* p,float* q,float* yaw){if(!testCyberwareWorldFrame)return false;std::fill_n(p,3,0.f);std::fill_n(q,4,0.f);q[3]=1;*yaw=0;return true;}
#endif
int g_VRForeTwistR[3]{-1,-1,-1},g_VRForeTwistL[3]{-1,-1,-1};
volatile float g_VRHeadDrop=.08f,g_VRSquatThreshold=.20f;
volatile float g_VRBodyBone[11][3]{};
volatile int g_VRBodyBoneOk[11]{};
volatile float g_VRIKDbgChest[3]{},g_VRIKDbgChestTgt[3]{};
volatile float g_VRIKDbgClav[2][8]{},g_VRIKDbgShModel[3]{},g_VRIKDbgHipsYaw{};

std::string VRDiagPath(const char* name) { return name; }
// The log goes nowhere; the trace counts the finger filter's lines (spike, jump, zero dropout) for its coverage.
int testFingerLogCounts[4]{};
void Log(const char* format,...) {
    char text[512];va_list args;va_start(args,format);vsnprintf(text,sizeof(text),format,args);va_end(args);
    if(!std::strstr(text,"[finger-"))return;
    testFingerLogCounts[std::strstr(text,"spike") ? 0:std::strstr(text,"jump") ? 1:std::strstr(text,"zero dropout") ? 2:3]++;
}
// Fixture camera frames have no outstanding body-yaw follow offset.
#ifndef VRIK_ROOM_TESTS
float BodyYawFollowOffset() { return 0; }
float BodyTrackingYaw(float nativeYaw) { return nativeYaw; }
#endif

VrikTransformSnapshot testNativePair{},testLuaPair{};
bool testNativeAvailable=false,testLuaAvailable=false;
bool VRIK_ReadTransformSnapshot(VrikTransformSnapshot* out) { if(!testLuaAvailable)return false;*out=testLuaPair;return true; }
#ifndef VRIK_ROOM_TESTS
bool VRIK_ReadNativeTransformSnapshot(VrikTransformSnapshot* out) { if(!testNativeAvailable)return false;*out=testNativePair;return true; }
#endif
// These external runtime boundaries are deliberately fatal if a test reaches
// them without supplying a pose. No GPU/runtime is silently simulated as success.
// OpenXRManager::Get() is tools/vrik_tests/xr_fake.cpp's: an object nobody constructed, with only its shared block
// pointed at testXrShared, so the inline GetSharedSlot / SetSharedSlot work; every other member a test reaches is a stub.
bool testSwimming=false,testLadder=false;
#ifndef VRIK_WATER_TESTS
namespace cvr::swimming { bool Active() { return testSwimming; } }
namespace cvr::ladder { bool Active() { return testLadder; } }
#endif
// Head aim, the left hand and the ADS layers are production code now (src/Anim/HeadAimWeapon.cpp and its neighbours,
// or their Rust port): the bench holds no weapon and does not aim.
extern "C" bool g_hasLeftWeaponEquipped=false,g_hasRightWeaponEquipped=false,g_isAiming=false;
volatile int g_VRSmokeLighterIdx=-1,g_VRSmokeCigIdx=-1,g_VRWeaponRaiseTransition=0;
volatile float g_VRWeaponPsmState=0,g_VRAimInRemaining=0;
bool OpenXRManager::GetHeadPose(OpenXRHeadPose*) const { std::abort(); }
bool OpenXRManager::GetNativeFrameHead(OpenXRHeadPose*,uint64_t*,uint64_t*,XrTime*,cvr::roomscale::Vec2*) const { std::abort(); }
bool OpenXRManager::LocateHeadPoseAt(XrTime,OpenXRHeadPose*) { std::abort(); }
void OpenXRManager::UpdateBodyPosture(OpenXRHeadPose&) { std::abort(); }
#if CVR_RUST_VRIK
// The header's inline frame-aim and frame-sample methods call rust/crates/cvr-xr/src/frame.rs in the Rust build; the
// bench has no XR session, so these stand in for it (and keep the OpenXR half of the Rust library out of the link).
extern "C" bool cvr_cpp_xr_acquire_frame_head(OpenXRHeadPose*);
extern "C" bool cvr_cpp_xr_acquire_frame_head_sample(OpenXRHeadPose*);
// cvr-rig asks for the XR cycle's own sample (prefer_native false) or the native frame's: the adapters' stubs.
extern "C" bool cvr_xr_acquire_frame_head_sample(OpenXRHeadPose* out,uint64_t*,uint64_t*,XrTime*,bool preferNative) {
    return preferNative ? cvr_cpp_xr_acquire_frame_head_sample(out) : cvr_cpp_xr_acquire_frame_head(out);
}
extern "C" int64_t cvr_xr_frame_aim_time_now() { return 0; }
extern "C" uint64_t cvr_cpp_xr_frame_aim_epoch();
extern "C" int64_t cvr_cpp_xr_frame_aim_time();
extern "C" void cvr_xr_frame_aim_read(cvr::tracking::FrameAimSnapshot* out) {
    if (out) { *out = {}; out->time = cvr_cpp_xr_frame_aim_time(); out->epoch = cvr_cpp_xr_frame_aim_epoch(); }
}
extern "C" void cvr_xr_frame_aim_publish(int64_t,uint64_t) {}
// The gaze centre's shared state (include/Camera/GazeCenter.hpp: rust/crates/cvr-xr/src/gaze.rs in the DLL), as it starts
// there -- no centre, the head looking along it -- so the bench's header reads keep cvr-xr out of the link too.
extern "C" cvr::gaze::XrGazeShared* cvr_xr_gaze_shared() {
    static cvr::gaze::XrGazeShared gaze{0,true};
    return &gaze;
}
// The OpenXR switches' block (rust/crates/cvr-xr/src/switches.rs in the DLL), as it starts there, its shared slots the
// bench's own (testXrShared): the input adapters read and write them through it.
extern float testXrShared[256];
extern "C" CvrXrSwitchesShared* cvr_xr_switches() {
    static CvrXrSwitchesShared switches{{4},{1},{1},{0},{0},testXrShared};
    return &switches;
}
// The target handoff's exported state and the held resource's cookie (src/Render/NativeTargetHandoff.cpp's in the DLL),
// as it starts there; the water bench's hooks reach them, nothing on its path writes them.
struct ID3D12Resource;
extern "C" {
__declspec(dllexport) uint64_t CyberpunkVR_TargetHandoffStats[16]{2};
__declspec(dllexport) uint64_t CyberpunkVR_TargetHandoffSources[32]{},CyberpunkVR_TargetHandoffSourceLocal[16]{},
    CyberpunkVR_TargetHandoffOwners[16]{},CyberpunkVR_TargetHandoffTargets[192]{},CyberpunkVR_TargetHandoffStates[80]{};
bool cvr_cpp_held_resource_cookie(ID3D12Resource*,uint64_t* cookie) { if (cookie) *cookie = 0; return false; }
}
#endif

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
extern "C" bool g_isInVehicle=false;   // C linkage, as Core/VrCoreShared.hpp declares it
extern "C" bool g_hasWeaponEquipped=false;
volatile int g_VRSmokeFingerCount=0,g_VRSmokeFingerCountL=0;
int g_VRSmokeFingerIdx[32]{},g_VRSmokeFingerIdxL[32]{};
volatile int g_VRSmokeFingerActive=0,g_VRSmokeFingerActiveL=0;
volatile int g_VRRestFingerHave=0,g_VRRestFingerCount=0,g_VRRestFingerApply=1;
float g_VRRestFingerRot[32][4]{};
volatile int g_VRReloadFingerActive[2]{};
float g_VRReloadFingerRot[2][32][4]{};
volatile int g_VRReloadFingerSet[2][32]{};
volatile float g_VRReloadFingerBlend[2]{};
extern "C" { char g_VRSmokeFingerName[32][48]{},g_VRSmokeFingerNameL[32][48]{}; }
extern "C" {
int CyberpunkVR_RuntimeDiagnostics=1; // tests explicitly inspect diagnostic counters

int CyberpunkVR_CamComposeAtWrite=1,CyberpunkVR_CamWriteInPatch=1,CyberpunkVR_HeadTranslationInPatch=1;
int CyberpunkVR_HeadingFromPreWrite=0,CyberpunkVR_ViewYawFromEngine=0;   // the body hooks' exports: src/Hooks/BodyHooksState.cpp
int CyberpunkVR_VrikTransformsFromPlugin=1,CyberpunkVR_VrikNativeFramePair=1,CyberpunkVR_VrikVehicleFullEntityQuat=0;
uint64_t CyberpunkVR_DebugVrikNativePairUsed=0,CyberpunkVR_DebugVrikLuaPairFallback=0;
int CyberpunkVR_VrikElbowPolicy=0;
// The camera link's placement switches (src/Core/VrCore.cpp in the plugin), at the plugin's defaults.
int CyberpunkVR_OneSamplePerFrame=1,CyberpunkVR_CamWriteInFinal=0,CyberpunkVR_VrcamHeadTranslation=1,CyberpunkVR_DeltaFromFreshSample=1;
int32_t CyberpunkVR_VrcamPosFromMain=0;
}
bool testWeaponInLeftHand=false;
namespace cvr::anim {
}
volatile int g_VRLocomotionState = -1;
volatile int g_VRLocomotionDetailed = -1;
extern "C" {

volatile uint64_t CyberpunkVR_DualWieldUntil = 0;
}
#include <string>
namespace cvr::paths { enum class Folder; std::wstring File(Folder, const wchar_t*) { return {}; } }
// The grips folder: none, except while the trace points it at its own scratch folder (the two-hand holds).
std::string testGripDir;
std::string VRGripPath(const char* name) { return testGripDir.empty() ? std::string{} : testGripDir+"\\"+name; }
// Solver dependencies added since the stubs were last brought up to date (the build stopped linking on these four).
// The config folder: none, except while the trace points it at its own scratch folder (vrport.ini for finger tracking).
std::string testConfigDir;
namespace cvr::paths { std::string FileA(Folder, const char* name) { return testConfigDir.empty() ? std::string{} : testConfigDir+"\\"+name; } }
namespace cvr::fbt::game { bool BodyActive() { return false; } }
// The smoking poses (src/Anim/SmokingPose.cpp, or its Rust port) are production code on the bench too.
float g_VRSmokeFingerRot[32][4]{},g_VRSmokeFingerRotL[32][4]{},g_VRSmokeFingerRotLC[32][4]{};
volatile int g_VRSmokeFingerCapture=0,g_VRSmokeFingerCaptureL=0,g_VRSmokeFingerHave=0,g_VRSmokeFingerHaveL=0;
volatile int g_VRSmokeMouthBoneIdx=-1,g_VRSmokeCigHave=0,g_VRSmokeCigEnable=0,g_VRSmokeMouthAnchor=0,g_VRSmokeAnchorValid=0;
float g_VRSmokeCigPos[3]{},g_VRSmokeCigRot[4]{0,0,0,1},g_VRSmokeLighterPos[3]{},g_VRSmokeLighterRot[4]{0,0,0,1};
float g_VRSmokeCigLPos[3]{},g_VRSmokeCigLRot[4]{0,0,0,1};
volatile float g_VRSmokeCigOffP[3]{},g_VRSmokeCigOffQ[4]{0,0,0,1},g_VRSmokeCigScaleY=1;
volatile float g_VRSmokeAnchorLocalPos[3]{},g_VRSmokeAnchorLocalRot[4]{0,0,0,1};
volatile int g_VRSmokeLighterHave=0,g_VRSmokeLighterEnable=0,g_VRSmokeLeftUseCig=0,g_VRSmokeRightLighter=0,g_VRSmokeCigLHave=0;
volatile float g_VRSmokeLighterOffP[3]{},g_VRSmokeLighterOffQ[4]{0,0,0,1},g_VRSmokeThumbFlickL[4]{0,0,0,1};
volatile float g_VRSmokeThumbPressManualL=0;
int g_VRSmokeThumbIsL[32]{};

// The weapon rig's state (src/Natives/NativeState.cpp) and the main image's range (src/Core/VrCore.cpp), for
// src/Anim/WeaponRig.cpp and its Rust port.
uintptr_t g_gameModuleBase=0;
size_t    g_gameModuleSize=0;
volatile int       g_WeaponPartCount=0;
volatile int       g_WeaponPartIdx[24]{};
volatile float     g_WeaponPartPos[24][3]{};
volatile float     g_WeaponPartRot[24][4]{};
volatile int       g_WeaponPartHave=0;
volatile uint64_t  g_WeaponMatchCalls=0;
volatile float     g_WeaponPartOff[24][3]{};
volatile int       g_WeaponPartWriteOn=0;
volatile int       g_PoseCensusN=0;
volatile uintptr_t g_PoseCensusBuf[VRPOSE_CENSUS_MAX]{};
volatile uint32_t  g_PoseCensusA4[VRPOSE_CENSUS_MAX]{};
volatile uint32_t  g_PoseCensusHits[VRPOSE_CENSUS_MAX]{};
volatile uint8_t   g_PoseCensusArmed[VRPOSE_CENSUS_MAX]{};
volatile uint32_t  g_PoseCensusHitsOut[VRPOSE_CENSUS_MAX]{};
volatile uint32_t  g_PoseCensusHitsIn[VRPOSE_CENSUS_MAX]{};
volatile int       g_PoseCensusWeaponOut=0;
volatile int       g_PoseCensusFull=0;
volatile uintptr_t g_RigBuf[VRRIG_N]{};
volatile uint32_t  g_RigSeen[VRRIG_N]{};
volatile uint32_t  g_RigBones[VRRIG_N]{};
volatile uint32_t  g_RigTracks[VRRIG_N]{};
volatile int       g_RigWriteN=0;
volatile int       g_RigWriteWhich[VRRIG_WRITES]{};
volatile int       g_RigWriteBone[VRRIG_WRITES]{};
volatile float     g_RigWriteOff[VRRIG_WRITES][3]{};
volatile float     g_RigWriteBase[VRRIG_WRITES][3]{};
volatile int       g_RigWriteHaveBase[VRRIG_WRITES]{};
volatile uint32_t  g_RigWriteApplied[VRRIG_WRITES]{};
volatile int       g_RigWriteSlot[VRRIG_WRITES]{};
volatile float     g_RigWriteRotAxis[VRRIG_WRITES][3]{};
volatile float     g_RigWriteRotAngle[VRRIG_WRITES]{};
volatile float     g_RigWriteBaseRot[VRRIG_WRITES][4]{};
volatile int       g_RigWriteHaveBaseRot[VRRIG_WRITES]{};
volatile int       g_RigWritePin[VRRIG_WRITES]{};
volatile float     g_RigPose[VRRIG_N][20][7]{};
volatile int       g_RigPoseHave[VRRIG_N]{};
volatile int       g_RigSigN=0;
volatile int       g_RigSigWhich[VRRIG_SIG_MAX]{};
volatile uint32_t  g_RigSigBones[VRRIG_SIG_MAX]{};
volatile int       g_RigSigIdx[VRRIG_SIG_MAX][VRRIG_SIG_NAMES]{};
volatile uint64_t  g_RigSigHash[VRRIG_SIG_MAX][VRRIG_SIG_NAMES]{};
volatile float     g_RigTrackVal[VRRIG_N][VRRIG_TRACKS]{};
volatile int       g_RigTrackHave[VRRIG_N]{};
volatile float     g_RigTrackSet[VRRIG_N][VRRIG_TRACKS]{};
volatile int       g_RigTrackOn[VRRIG_N][VRRIG_TRACKS]{};
volatile float     g_RigWriteScale[VRRIG_WRITES]{};
volatile int       g_RigWriteAbs[VRRIG_WRITES]{};
volatile float     g_RigWriteQuat[VRRIG_WRITES][4]{};
volatile int       g_RigWriteQuatOn[VRRIG_WRITES]{};
volatile int       g_RigWriteEnabled[VRRIG_WRITES]{};
volatile uint32_t  g_RigPassSeen[4]{};
volatile int       g_RigPassRn[4]{};
volatile int       g_RigPassDst[4]{};
volatile int       g_RigPassSlots[4]{};
volatile int       g_RigPassBufLo[4]{};
volatile int       g_RigPassMap0[8]{};

// The steering wheel's boundaries (src/Anim/WheelGrab.cpp and its Rust port): the live controls, the driver's seat,
// the menu and the overlay. The tracking sample and the clock come from the trace's hooks (g_VRIKWheelFrame,
// g_VRIKClockUs); the real OpenXR read must never be reached.
LiveControls g_liveControls{};
#if CVR_RUST_VRIK
// The solve reads the live controls from the block the plugin attaches (rust/crates/cvr-rig/src/vrik/live_reads.rs).
extern "C" void cvr_live_controls_block_attach(LiveControls* block);
static const bool kLiveControlsAttached = (cvr_live_controls_block_attach(&g_liveControls), true);
#endif
std::atomic<bool> g_isDriving{false};
volatile int g_menuModeValue=0;
bool testOverlayVisible=false;
bool OverlayIsVisible() { return testOverlayVisible; }
OpenXRHeadPose testGestureHead{},testGestureHands[2]{};bool testGestureOk=false;uint64_t testGestureSequence=7,testGestureStamp=11;
bool OpenXRManager::GetGestureHandFrame(OpenXRHeadPose* head,OpenXRHeadPose* hands,uint64_t* sequence,uint64_t* stamp,bool) const {
    if(!testGestureOk)return false;*head=testGestureHead;hands[0]=testGestureHands[0];hands[1]=testGestureHands[1];
    if(sequence)*sequence=testGestureSequence;if(stamp)*stamp=testGestureStamp;return true;
}

// Hand recoil (src/Anim/Recoil.cpp and its Rust port): every round's haptic envelope is recorded for the trace, and the
// two-hand grip's share of the kick (src/Anim/TwoHandGrip.cpp) is the trace's to set.
// The haptics' device call (src/Runtimes/HapticsDevice.cpp): recorded, and counted as an applied segment.
std::vector<float> testHapticPlays;
extern "C" __declspec(dllexport) int CyberpunkVR_HapticStats[8];
extern "C" void cvr_cpp_haptics_play(int hand,float amplitude,float seconds) {
    testHapticPlays.push_back(float(hand));testHapticPlays.push_back(amplitude);testHapticPlays.push_back(seconds);
    CyberpunkVR_HapticStats[3]=0;++CyberpunkVR_HapticStats[hand==0 ? 1:2];
}
// The support hand (src/Anim/TwoHandGrip.cpp and its Rust port): the weapon's name, the carried body and its hand-off
// switch; its own state is src/Anim/CharacterRigState.cpp's.
extern "C" {
char CyberpunkVR_WeaponName[64]{};
volatile int g_VRBodyCarryingState=-1;
}

// The pose hook (src/Hooks/AnimPoseBody.cpp and its Rust port): its globals (src/Natives, src/Runtimes, the camera) and
// the C++ services it reads, each one the trace's to set.
extern "C" {
float CyberpunkVR_DebugHandMissMm=0;
int CyberpunkVR_VrikArmAnchorFromBody=1,CyberpunkVR_VrikBatchClock=1,CyberpunkVR_VrikSplitHeadHandRot=0,CyberpunkVR_XrDeepDiag=0;
float CyberpunkVR_VrikBatchGapMs=3.0f;
volatile int CyberpunkVR_VrikHandFrameAlign=1,CyberpunkVR_VrikHandFrameOneLatch=1;
std::atomic<uint64_t> CyberpunkVR_PoseIdImageWrites[2]{};   // the image pose labels (cvr-rig image_pose)
volatile uint64_t g_AnimPoseTotalCalls=0,g_AnimPoseMatchCalls=0;
volatile uintptr_t g_AnimPoseLastBoneBuf=0,g_PlayerTrackBufA=0,g_PlayerTrackBufB=0,g_WeaponTrackBufA=0,g_WeaponTrackBufB=0;
volatile int g_PoseCensusOn=0,g_WeaponRigActive=0,g_VRRecordFK=0;
volatile int g_VRTakedownState=-1,g_VRBodyDisposalState=-1,g_VRBodyCarryBothHands=-1,g_VRFinisherActive=0;
volatile uint64_t g_VRFinisherSinceMs=0;
volatile int g_VRBodyUnderHMD=1,g_VRCamBoneFreeze=0;
volatile int g_VRFppCamIdx[5]={-1,-1,-1,-1,-1};
volatile float g_VRCamModelPos[3]{},g_VRCamModelRot[4]{0,0,0,1};
volatile float g_VRElbowPoleR=0,g_VRElbowPoleL=0,g_VRElbowSwingR=0,g_VRElbowSwingL=0;
volatile float g_VRIKDbgHandFK[3]{},g_VRIKDbgTargetTrace[3]{};
volatile int g_VRIKFreshTotal=0,g_VRIKReplayTotal=0;
volatile float g_VROffLX=0,g_VROffLY=0,g_VROffLZ=0,g_VROffRX=0,g_VROffRY=0,g_VROffRZ=0;
volatile float g_VRPalmModelL[3]{},g_VRPalmModelR[3]{},g_VRPalmModelRotL[4]{0,0,0,1},g_VRPalmModelRotR[4]{0,0,0,1};
volatile int g_VRPalmModelValid=0;
volatile float g_VRScaleL=1,g_VRScaleR=1;
volatile float g_VRShoulderLX=.18f,g_VRShoulderLY=-.15f,g_VRShoulderLZ=.05f,g_VRShoulderRX=.18f,g_VRShoulderRY=-.15f,g_VRShoulderRZ=.05f;
volatile float g_VRSmokeAltAnchorLocalPos[3]{},g_VRSmokeAltAnchorLocalRot[4]{0,0,0,1};
volatile int g_VRSmokeAltAnchorValid=0,g_VRSmokeAnchorBoneIdx=-1,g_VRSmokeAnchorBoneSel=0;
volatile float g_VRSmokeMouthDist=0,g_VRSmokeMouthDistL=0;
volatile float g_VRSmokeMouthPos[3]{0,.08f,-.06f},g_VRSmokeMouthRot[4]{0,0,0,1};
volatile float g_VRSmokeMouthWorldPos[3]{},g_VRSmokeMouthWorldRot[4]{0,0,0,1};
volatile int g_VRSmokeMouthWorldValid=0;
volatile float g_VRSmokeSmokePos[3]{0,.1f,-.05f},g_VRSmokeSmokeRot[4]{0,0,0,1};
volatile float g_VRViewWorldPos[3]{},g_VRViewWorldRot[4]{0,0,0,1};
volatile int g_VRViewWorldValid=0;
volatile float g_VRWristL_I=0,g_VRWristL_J=0,g_VRWristL_K=0,g_VRWristL_R=1,g_VRWristR_I=0,g_VRWristR_J=0,g_VRWristR_K=0,g_VRWristR_R=1;
}
std::atomic<int> g_sceneTier{1},g_vehicleState{0};
bool testDeviceCam=false,testBodyYawFollow=false,testFbtCalibrating=false,testFbtArm=false,testLobby=false;
bool testRecipeOk=false,testRestNeckOk=false;int testCutsceneTier=-1;int testLadderKinds[2]{};
cvr::camera::AnchorRecipe testRecipe{};cvr::roomscale::Vec2 testConsumed{},testRestNeck{};
std::vector<float> testServiceCalls;
bool DeviceCamActive() { return testDeviceCam; }
#ifndef VRIK_ROOM_TESTS
bool BodyYawFollowActive() { return testBodyYawFollow; }
#endif
#if defined(CVR_RUST_VRIK)
// The tier rust/crates/cvr-rig/src/vrik/vrik_arming.rs reads (the live controls' in the plugin).
extern "C" int cvr_cpp_cutscene_suspend_tier() { return testCutsceneTier; }
// The keypad, world-touch and entity-attach natives' exports and services (rust/crates/cvr-rig/src/vrik/keypad.rs,
// world_touch.rs, entity_attach.rs); the benches never call them.
extern "C" {
uint64_t CyberpunkVR_KeypadRoot{}, CyberpunkVR_KeypadLayer{};
std::atomic<uint64_t> CyberpunkVR_KeypadHeadHits{}, CyberpunkVR_KeypadHeldHits{}, CyberpunkVR_DebugEntityAttach[4]{};
float CyberpunkVR_KeypadCursor[2]{};
bool cvr_cpp_vrmenu_finger_world(int, float*) { return false; }
// The VR menu's natives (rust/crates/cvr-rig/src/vrik/vr_menu.rs): the overlay, the settings recorder, the raw
// controllers and the menu's live controls are the DLL's.
bool cvr_cpp_vrui_consume_menu_summon() { return false; }
bool cvr_cpp_vrui_visible() { return false; }
void cvr_cpp_vrui_toggle() {}
void cvr_cpp_menu_controls(int32_t* gesture, int32_t* weaponWheel) { *gesture = *weaponWheel = 0; }
bool cvr_cpp_xr_raw_controller_state(void*) { return false; }
void cvr_cpp_uirec_request(int32_t) {}
size_t cvr_cpp_uirec_latest(int32_t, char*, size_t) { return 0; }
void cvr_cpp_uirec_queue_edit(int32_t, const char*, double) {}
void CvrWorldTouchTriangleThunk() {}
int CyberpunkVR_BodyCapsulePrefilter = 1;
int cvr_cpp_vrcam_enable_request() { return -1; }
// rust/crates/cvr-rig/src/vrik/remote_camera.rs.
int CyberpunkVR_BdFovWrite = 1, CyberpunkVR_BdHeadWrite = 1, CyberpunkVR_HudInBraindance = 0;
void cvr_cpp_remote_camera_state(void* out) { std::memset(out, 0, 7 * sizeof(void*)); }
void cvr_cpp_takeover_lens_release() {}
void cvr_cpp_braindance_camera_release() {}
void cvr_cpp_patch_fast_disarm() {}
void cvr_cpp_device_cam_restore_fov() {}
// rust/crates/cvr-rig/src/vrik/menu_surface.rs.
std::atomic<uint64_t> CyberpunkVR_MenuSurfaceStats[8]{};
// rust/crates/cvr-rig/src/vrik/os_input.rs.
volatile long CyberpunkVR_DebugCursorMap[4]{};
bool testCursorClipped = false;
bool cvr_cpp_launcher_open() { return false; }
bool cvr_overlay_host_open() { return false; }
void* cvr_cpp_game_hwnd() { return nullptr; }
bool* cvr_cpp_cursor_clipped() { return &testCursorClipped; }
unsigned GetForcedDisplayModeWidth() { return 0; }
unsigned GetForcedDisplayModeHeight() { return 0; }
unsigned GetForcedWindowWidth() { return 0; }
// rust/crates/cvr-rig/src/vrik/camera_fov.rs.
void cvr_cpp_launcher_size(int* width, int* height) { *width = *height = 0; }
bool cvr_cpp_xr_asym_eye_windows(void*) { return false; }
float cvr_cpp_xr_runtime_hfov() { return 0.0f; }
void cvr_cpp_fov_publish(void*, float, float, float) {}
bool cvr_cpp_xr_eye_fov(int, void*) { return false; }
float GetForcedFov() { return 0.0f; }
int CyberpunkVR_AsymProjection = 0;
// rust/crates/cvr-rig/src/vrik/lod_fov.rs.
int CyberpunkVR_FixLodEnable = 1;
uint64_t CyberpunkVR_DebugBdHeadWrites = 0, CyberpunkVR_DebugBdViewSeen = 0, CyberpunkVR_DebugBdViewByFov = 0, CyberpunkVR_DebugBdFovWrites = 0;
uint32_t CyberpunkVR_DebugFixLodCoverage = 0;
float CyberpunkVR_DebugVrcamWantFov = 0.0f;
float cvr_cpp_bd_scene_fov() { return 0.0f; }
// rust/crates/cvr-rig/src/vrik/left_handed_animation.rs's native.
std::atomic<uint32_t> CyberpunkVR_LeftHandedGraphsAttached{};
// rust/crates/cvr-rig/src/vrik/photo_capture.rs.
std::atomic<uint64_t> CyberpunkVR_PhotoCaptureSizeOverrides{}, CyberpunkVR_PhotoCaptureClampOverrides{};
unsigned GetForcedWindowHeight() { return 0; }
void CvrPhotoClampDetour() {}
void CvrPhotoSizeDetour() {}
// rust/crates/cvr-rig/src/vrik/port_sound.rs (stb_vorbis is the plugin's).
int stb_vorbis_decode_memory(const unsigned char*, int, int*, int*, short**) { return -1; }
}
#else
int VRIK_EffectiveCutsceneSuspendTier() { return testCutsceneTier; }
#endif
// The camera link's and camera math's services (src/Camera/CameraServices.cpp in the plugin).
uint64_t testOriginSerial=1,testPoseIdentity=0;float testRuntimeIpd=.064f;
std::atomic<int> g_bdActive{0};
extern "C" {
uint64_t cvr_cpp_xr_tracking_origin_serial() { return testOriginSerial; }
// The pose identity's OpenXR questions: the rendered pose a scripted function of the sample, or none.
bool testResolveRenderedPose=true,testExternalPoseReset=false;
bool cvr_cpp_xr_resolve_rendered_pose(const OpenXRHeadPose* head,XrPosef* out) {
    if(!testResolveRenderedPose || !head || !out)return false;
    *out={{head->oriX,head->oriY,head->oriZ,head->oriW},{head->posX+.01f,head->posY,head->posZ-.02f}};return true;
}
bool cvr_cpp_xr_external_pose_reset_pending() { return testExternalPoseReset; }
float cvr_cpp_xr_runtime_ipd() { return testRuntimeIpd; }
uint64_t cvr_cpp_next_pose_identity() { return (++testPoseIdentity)|(uint64_t{1}<<62); }
bool cvr_cpp_device_cam_active() { return testDeviceCam; }
bool cvr_cpp_braindance_active() { return g_bdActive.load()!=0; }
void cvr_cpp_camera_scales(float* out) { out[0]=g_liveControls.xrWorldScale;out[1]=g_liveControls.xrIpdScale;out[2]=g_liveControls.xrStereoScale; }
}
namespace cvr::fbt::game {
bool Calibrating() { return testFbtCalibrating; }
bool SolveArm(uint8_t*,int side,const float target[3],const float handRot[4]) {
    if(!testFbtArm)return false;testServiceCalls.push_back(float(side));for(int k=0;k<3;++k)testServiceCalls.push_back(target[k]);
    for(int k=0;k<4;++k)testServiceCalls.push_back(handRot[k]);return true;
}
void SolveLowerBody(uint8_t*,const float camModelPos[3],const float camModelRot[4]) {
    testServiceCalls.push_back(-1.f);for(int k=0;k<3;++k)testServiceCalls.push_back(camModelPos[k]);for(int k=0;k<4;++k)testServiceCalls.push_back(camModelRot[k]);
}
}
namespace cvr::lobby { bool WorldShown() { return testLobby; } }
#ifndef VRIK_ROOM_TESTS
namespace cvr::roomscale {
Vec2 CameraConsumed() { return testConsumed; }
bool CameraRestNeck(Vec2* out) { if(testRestNeckOk)*out=testRestNeck;return testRestNeckOk; }
}
#endif
#ifndef VRIK_WATER_TESTS
namespace cvr::ladder {
void ReadHandKinds(int* kinds) { kinds[0]=testLadderKinds[0];kinds[1]=testLadderKinds[1]; }
void ConstrainHand(int side,float* target,float* rot,const float* entityPos,const float*) {
    target[2]+=.01f*float(side+1)+entityPos[0]*.001f;rot[3]+=.001f;
}
}
#endif

// The hand-tracking runtime (src/Runtimes/FingerGrip.cpp, or its Rust port): xrGetInstanceProcAddr hands out three
// fakes whose answers the trace scripts; every call and what it was given goes to testHand.calls.
TestHandRuntime testHand;
static XrResult XRAPI_CALL TestCreateHandTracker(XrSession session,const XrHandTrackerCreateInfoEXT* info,XrHandTrackerEXT* tracker) {
    const int h=info->hand==XR_HAND_LEFT_EXT ? 0:1;
    testHand.calls.insert(testHand.calls.end(),{1,int64_t(reinterpret_cast<uintptr_t>(session)),info->type,int64_t(reinterpret_cast<uintptr_t>(info->next)),info->hand,info->handJointSet});
    if(XR_FAILED(testHand.createResult[h]))return testHand.createResult[h];
    *tracker=reinterpret_cast<XrHandTrackerEXT>(uintptr_t(0x7100+h));
    return testHand.createResult[h];
}
static XrResult XRAPI_CALL TestDestroyHandTracker(XrHandTrackerEXT tracker) {
    testHand.calls.insert(testHand.calls.end(),{2,int64_t(reinterpret_cast<uintptr_t>(tracker))});
    return XR_SUCCESS;
}
static XrResult XRAPI_CALL TestLocateHandJoints(XrHandTrackerEXT tracker,const XrHandJointsLocateInfoEXT* info,XrHandJointLocationsEXT* out) {
    int64_t range=-1;
    if(info->next) { const auto* r=static_cast<const XrHandJointsMotionRangeInfoEXT*>(info->next);range=int64_t(r->type)*16+r->handJointsMotionRange; }
    testHand.calls.insert(testHand.calls.end(),{3,int64_t(reinterpret_cast<uintptr_t>(tracker)),info->type,range,
        int64_t(reinterpret_cast<uintptr_t>(info->baseSpace)),info->time,out->type,int64_t(reinterpret_cast<uintptr_t>(out->next)),out->jointCount});
    if(XR_FAILED(testHand.locateResult))return testHand.locateResult;
    out->isActive=testHand.active;
    std::memcpy(out->jointLocations,testHand.joints,sizeof(testHand.joints));
    return testHand.locateResult;
}
extern "C" XrResult XRAPI_CALL xrGetInstanceProcAddr(XrInstance instance,const char* name,PFN_xrVoidFunction* function) {
    testHand.calls.insert(testHand.calls.end(),{4,int64_t(reinterpret_cast<uintptr_t>(instance)),int64_t(std::strlen(name))});
    if(std::strcmp(name,"xrCreateHandTrackerEXT")==0)*function=testHand.haveCreate ? reinterpret_cast<PFN_xrVoidFunction>(&TestCreateHandTracker):nullptr;
    else if(std::strcmp(name,"xrDestroyHandTrackerEXT")==0)*function=testHand.haveDestroy ? reinterpret_cast<PFN_xrVoidFunction>(&TestDestroyHandTracker):nullptr;
    else if(std::strcmp(name,"xrLocateHandJointsEXT")==0)*function=testHand.haveLocate ? reinterpret_cast<PFN_xrVoidFunction>(&TestLocateHandJoints):nullptr;
    else *function=nullptr;
    return *function ? XR_SUCCESS:XR_ERROR_FUNCTION_UNSUPPORTED;
}
// src/Core/Log.cpp's guarded float copies, as the plugin has them (src/Utils/MemorySafeC.cpp does the same per float).
extern "C" bool cvr_cpp_read_floats_safe(const float*,float*,size_t);extern "C" bool cvr_cpp_write_floats_safe(float*,const float*,size_t);
bool ReadFloatArraySafe(const float* src,float* out,size_t count) { return cvr_cpp_read_floats_safe(src,out,count); }
bool WriteFloatArraySafe(float* dst,const float* values,size_t count) { return cvr_cpp_write_floats_safe(dst,values,count); }

// The body hooks' services (src/Hooks/HookServices.cpp and RoomscaleTweaks.cpp in the plugin): every answer scripted by
// the room trace, every flush of the hands recorded.
cvr::roomscale::Sample testRoomSample{};OpenXRHeadPose testFrameHead{};int64_t testFrameTime=0;bool testRoomSampleOk=false;
OpenXRHeadPose testAcquireHead{};bool testAcquireOk=false;
cvr::body::TrackingFrame testBodyFrame{};bool testBodyFrameOk=false;
float testCapsuleRadius=0;int testPlayerChanged=0;
float testSteer[3]{};int testSteerState=0;bool testSteerOk=false;
std::vector<float> testFlushes;
extern "C" {
bool cvr_cpp_xr_roomscale_sample(cvr::roomscale::Sample* out,OpenXRHeadPose* head,XrTime* time) {
    *out=testRoomSample;if(head)*head=testFrameHead;if(time)*time=testFrameTime;return testRoomSampleOk;
}
void cvr_cpp_xr_flush_hands_to_shared(const OpenXRHeadPose* head,XrTime time,const cvr::roomscale::Vec2* consumed) {
    testFlushes.push_back(head ? head->posX:-1.f);testFlushes.push_back(float(time));
    if(consumed) { testFlushes.push_back(consumed->x);testFlushes.push_back(consumed->y); }
}
bool cvr_cpp_xr_acquire_frame_head(OpenXRHeadPose* out) { if(!testAcquireOk)return false;*out=testAcquireHead;return true; }
bool cvr_cpp_xr_body_tracking_frame(cvr::body::TrackingFrame* out) { if(!testBodyFrameOk)return false;*out=testBodyFrame;return true; }
float cvr_cpp_tweak_capsule_radius() { return testCapsuleRadius; }
// The Rust solve asks rust/crates/cvr-ffi/src/native_tweaks.rs (the DLL) for the same radius.
float cvr_tweak_capsule_radius() { return testCapsuleRadius; }
void cvr_cpp_camera_player_changed() { ++testPlayerChanged; }
int cvr_cpp_menu_mode() { return g_menuModeValue; }
bool cvr_cpp_swimming_read_steering(float* direction,int* state) {
    direction[0]=testSteer[0];direction[1]=testSteer[1];direction[2]=testSteer[2];*state=testSteerState;return testSteerOk;
}
bool cvr_cpp_pose_heading_ready() { return g_PlayerPoseHeadingReady.load(); }
void cvr_cpp_body_controls(CvrBodyControls* out) {
    out->roomscaleMovement=g_liveControls.xrRoomscaleMovement;out->cutsceneSuspendTier=g_liveControls.xrCutsceneSuspendTier;
    out->bodyRotationMode=g_liveControls.xrBodyRotationMode;out->bodyMoveRadius=g_liveControls.xrBodyMoveRadius;
    out->bodyFreeLookDeg=g_liveControls.xrBodyFreeLookDeg;out->bodyFreeLookDownDeg=g_liveControls.xrBodyFreeLookDownDeg;
    out->bodyFreeLookSwimDeg=g_liveControls.xrBodyFreeLookSwimDeg;
}
int Get3DofMovement() { return g_liveControls.xr3DofMovement; }
}
// The hook registry: the hook files register themselves, nothing installs on the bench.
cvr::hooks::Hook::Hook(const char* n,Stage s,int o,InstallFn f,EnabledFn g):name(n),stage(s),order(o),install(f),enabled(g),next(nullptr),installed(false) {}
#if defined(VRIK_ROOM_TESTS) && !defined(VRIK_WATER_TESTS)
// What the C++ body hooks call directly: the steering the shim above gives Rust.
namespace cvr::swimming {
bool ReadSteering(XrVector3f* direction,int* state) {
    direction->x=testSteer[0];direction->y=testSteer[1];direction->z=testSteer[2];*state=testSteerState;return testSteerOk;
}
}
#endif
#if defined(VRIK_ROOM_TESTS) && !defined(CVR_RUST_VRIK)
extern "C" {
MH_STATUS WINAPI MH_Initialize() { return MH_ERROR_NOT_INITIALIZED; }
MH_STATUS WINAPI MH_CreateHook(LPVOID,LPVOID,LPVOID*) { return MH_ERROR_NOT_INITIALIZED; }
MH_STATUS WINAPI MH_RemoveHook(LPVOID) { return MH_ERROR_NOT_INITIALIZED; }
MH_STATUS WINAPI MH_EnableHook(LPVOID) { return MH_ERROR_NOT_INITIALIZED; }
MH_STATUS WINAPI MH_DisableHook(LPVOID) { return MH_ERROR_NOT_INITIALIZED; }
}
#endif

// The on-foot input hooks' services (src/Hooks/HookServices.cpp in the plugin): the bench has no telemetry block, no
// pending snap and no shared block for them; nothing calls their callbacks.
extern "C" {
float CyberpunkVR_DebugShotHeadingKickDeg=0;
uintptr_t cvr_cpp_telemetry_free_delta() { return 0; }
void cvr_cpp_telemetry_move_xy(void*) {}
void cvr_cpp_telemetry_delta_head(float*) {}
int32_t cvr_cpp_take_pending_snap_yaw() { return InterlockedExchange(&g_pendingSnapYawDeltaBits,0); }
float* cvr_cpp_shot_shared() { return GetShotShared(); }
void cvr_cpp_swimming_bind_native_input(void*) {}
int cvr_cpp_movement_source() { return g_liveControls.xrMovementSource; }
float cvr_cpp_xr_hmd_yaw_rel_to_body() { return 0; }
float cvr_cpp_xr_hand_yaw_rel_to_body(int) { return 0; }
}

// The XInput merge's world (src/Hooks/XInput.cpp, or its Rust port through src/Hooks/InputServices.cpp): every answer
// the input trace scripts, every call it records -- the keys it would have typed among them.
float testXrShared[256]{},testShotShared[256]{};
VRControllerState testController{};bool testControllerOk=false,testCrouchRequested=false;
bool testClueConsume=false,testVruiCaptures=false,testReachHeld=false,testMenuCaptures=false,testWheelArmed=false;
bool testKeypad=false,testInBraindance=false,testSceneYawArmed=false,testIntroRunning=false,testSwimSprint=false;
float testCurtain=0;cvr::swimming::Input testSwimInput{};
std::vector<float> testInputCalls;
volatile LONG g_pendingSnapYawDeltaBits=0;
std::atomic<int> g_uiPopupOpen{0};std::atomic<unsigned long long> g_uiPopupClosedMs{0};
std::atomic<float> g_lookStickX{0};volatile int g_verboseLog=0;HWND g_gameHwnd=nullptr;
float* GetShotShared() { return testShotShared; }
bool InBraindance() { return testInBraindance; }
bool SceneStickYawArmed() { return testSceneYawArmed; }
bool OpenXRManager::GetControllerState(VRControllerState* out) const { if(!testControllerOk)return false;*out=testController;return true; }
bool OpenXRManager::PhysicalCrouchRequested() { return testCrouchRequested; }
// The water benches link the real ones (src/Hooks/StoryAttention.cpp, or its Rust port).
#ifndef VRIK_WATER_TESTS
namespace cvr::quest {
void SuspendManualClueInput() { testInputCalls.push_back(101); }
bool ConsumeManualClueButton(bool pressed,bool allowed) { testInputCalls.insert(testInputCalls.end(),{102,float(pressed),float(allowed)});return testClueConsume && pressed; }
}
#endif
#ifndef VRIK_WATER_TESTS
namespace cvr::swimming {
void ResetInput() { testInputCalls.push_back(103); }
Input UpdateInput(const VRControllerState& c,bool manualBack) { testInputCalls.insert(testInputCalls.end(),{104,c.leftThumbY,float(manualBack)});return testSwimInput; }
bool SprintInput(bool wanted) { testInputCalls.insert(testInputCalls.end(),{105,float(wanted)});return wanted!=testSwimSprint; }
}
namespace cvr::ladder {
void ResetInput() { testInputCalls.push_back(106); }
void UpdateInput(const VRControllerState& c) { testInputCalls.insert(testInputCalls.end(),{107,c.leftGrip,c.rightGrip,float(c.buttons)}); }
}
#endif
namespace cvr::vrui { bool CapturesInput() { return testVruiCaptures; } }
namespace cvr::reach { bool UseHeld() { return testReachHeld; } }
namespace cvr::input {
#ifndef VRIK_WATER_TESTS
void DispatchCyberwareChord() { testInputCalls.push_back(108); }
#endif
bool KeypadInputActive() { return testKeypad; }
void PublishKeypadStick(float x,float y,float deadzone) { testInputCalls.insert(testInputCalls.end(),{109,x,y,deadzone}); }
}
namespace cvr::vrmenu {
bool CapturesButtons() { return testMenuCaptures; }
bool WheelArmed(uint64_t) { return testWheelArmed; }
void PublishWheelHold(bool pending,bool held) { testInputCalls.insert(testInputCalls.end(),{110,float(pending),float(held)}); }
}
namespace cvr::lobby {
float Curtain() { return testCurtain; }
namespace intro {
std::shared_ptr<const Picture> Latest() { return testIntroRunning ? std::make_shared<const Picture>():nullptr; }
bool Running() { return testIntroRunning; }
}
}
extern "C" uint32_t cvr_cpp_send_input(uint32_t count,const void* inputs,int size) {
    const auto* in=static_cast<const INPUT*>(inputs);
    for(uint32_t k=0;k<count;++k)testInputCalls.insert(testInputCalls.end(),{111,float(in[k].type),float(in[k].ki.wVk),float(in[k].ki.dwFlags),float(size)});
    return count;
}
// The camera hooks' plugin state (src/Core/VrCore.cpp and src/Hooks/HookServices.cpp in the plugin): FinalCamera,
// CameraPitch and WorldSpaceBlendCamera read and write these; the traces script them.
int testVrcamView=0,testMainView=0;uintptr_t testCamObjMain=0;uint32_t testTelemetryHits=0;uintptr_t testTelemetryRsi=0;
std::vector<std::pair<OpenXRHeadPose,bool>> testRenderedPoses;
extern "C" {
uint64_t g_finalCameraHits=0;volatile uint32_t g_lastLocateSeq=0,g_renderedSeq=0,g_headQuatValid=0;volatile float g_gamePitchRadians=0;
int CyberpunkVR_StereoModuleLoaded=0,CyberpunkVR_PoseReadBack=1,CyberpunkVR_CamFinalViewScope=1;
uint32_t CyberpunkVR_DebugFinalAge=0,CyberpunkVR_DebugFinalTies=0;
uint64_t CyberpunkVR_DebugFinalTieHits=0,CyberpunkVR_DebugFinalMatch=0,CyberpunkVR_DebugFinalNoMatch=0;
uint64_t CyberpunkVR_DebugViewCamMain=0,CyberpunkVR_DebugViewCamVrcam=0,CyberpunkVR_DebugViewCamOther=0;
int CyberpunkVR_IsVrcamViewActive() { return testVrcamView; }
int CyberpunkVR_IsMainViewActive() { return testMainView; }
void CvrWorldBlendNotifyDetour() {}
void cvr_cpp_telemetry_final(uint32_t hits,uintptr_t rsi) { testTelemetryHits=hits;testTelemetryRsi=rsi; }
void cvr_cpp_xr_push_rendered_frame_pose(const OpenXRHeadPose* pose,bool vrcam) { if(pose)testRenderedPoses.push_back({*pose,vrcam}); }
uintptr_t cvr_cpp_cam_obj_main() { return testCamObjMain; }
}
// CameraPoseTransport's services (src/Hooks/HookServices.cpp in the plugin), scripted by its trace; the asm thunks of
// the setup copy and default (src/Hooks/CameraPoseLeafThunks.asm), which the bench never enters.
bool testSessionRunning=true,testBdScenePose=false,testEnsureCopyHook=true;uintptr_t testVrcamComp=0;
OpenXRHeadPose testCameraPoseFrame{};uint64_t testCameraPoseSequence=0;bool testCameraPoseOk=false;
float testNormalFov=90.f;int testVrcamLive=0;std::vector<uintptr_t> testTransportCalls;
extern "C" {
int32_t CyberpunkVR_BdEditorAlign=0;int CyberpunkVR_IpdInWorldPos=1,CyberpunkVR_MainIsRightEye=0;
bool cvr_cpp_lobby_world_shown() { return cvr::lobby::WorldShown(); }
bool cvr_cpp_xr_session_running() { return testSessionRunning; }
bool cvr_cpp_bd_scene_pose_valid() { return testBdScenePose; }
uintptr_t cvr_cpp_vrcam_comp() { return testVrcamComp; }
bool cvr_cpp_xr_acquire_camera_pose_frame(OpenXRHeadPose* head,cvr::roomscale::Vec2* consumed,uint64_t* sequence) {
    testTransportCalls.push_back(201);if(head)*head=testCameraPoseFrame;if(consumed)*consumed={.01f,-.02f};
    if(sequence)*sequence=testCameraPoseSequence;return testCameraPoseOk;
}
void cvr_cpp_refresh_player_camera_state() { testTransportCalls.push_back(202); }
float cvr_cpp_normal_fov_override() { return testNormalFov; }
bool cvr_cpp_ensure_camera_pose_copy_hook() { return testEnsureCopyHook; }
void cvr_cpp_external_vrcam_observe(uintptr_t component) { testTransportCalls.insert(testTransportCalls.end(),{203,component}); }
int cvr_cpp_external_vrcam_notify(uintptr_t eyeCamera,void(*notify)(uintptr_t)) {
    testTransportCalls.insert(testTransportCalls.end(),{204,eyeCamera,uintptr_t(testVrcamLive)});
    if(testVrcamLive==0 && notify)notify(eyeCamera);
    return testVrcamLive;
}
uintptr_t CvrPoseSetupCopyDetour(uintptr_t,uintptr_t) { return 0; }
uintptr_t CvrPoseSetupDefaultDetour(uintptr_t) { return 0; }
}
#if defined(VRIK_WATER_TESTS) && !defined(CVR_RUST_VRIK)
// The camera hooks' installers and their probe logging, which the bench never runs (src/Hooks/Trampoline.cpp,
// src/Core/Log.cpp).
void* AllocateTrampoline(void*,size_t) { return nullptr; }
void WriteMovRaxImm64(uint8_t*,int&,uintptr_t) {}
bool LooksProjectionLike(const float*,size_t) { return false; }
void LogMatrix4x4(const char*,const float*) {}
bool WriteU32Safe(uintptr_t addr,uint32_t value) { __try { *reinterpret_cast<uint32_t*>(addr)=value;return true; } __except(EXCEPTION_EXECUTE_HANDLER) { return false; } }
#endif
// The cyberware chord's and BackgroundPad's services (src/Hooks/HookServices.cpp in the plugin).
bool testForegroundInProcess=true;
extern "C" bool cvr_cpp_foreground_in_process() { return testForegroundInProcess; }
extern "C" bool cvr_cpp_session_focused() { return false; }
#if defined(CVR_RUST_VRIK)
// The loading screen's and the rebind census's exports (src/Hooks/SeamlessLoading.cpp, RebindCensus.cpp in the plugin),
// which the Rust hooks name and the bench never runs.
extern "C" { int32_t CyberpunkVR_SeamlessLoading=1;std::atomic<uint64_t> CyberpunkVR_DebugSeamlessLoads{0};
    volatile uint64_t CyberpunkVR_DebugRebindLoopCalls=0;
    uintptr_t cvr_cpp_settings_res_apply(uintptr_t) { return 0; }
    void cvr_cpp_forced_render_size(uint32_t* width,uint32_t* height) { *width=*height=0; }
    uint64_t g_unifixHits=0;float g_unifixProjDump[9]{};volatile uintptr_t g_unifixRenderObj=0;
    uint64_t g_projStageHits=0;float g_projStageFov=0,g_projStageAspect=0,g_projStageExtra=0;bool g_projStagePatched=false;
    // rust/crates/cvr-rig/src/vrik/hand_body.rs's C++ half (src/Natives/AnimLookup.cpp in the DLL).
    void* cvr_anim_player_animated_object(const char*) { return nullptr; }
    bool cvr_cpp_xr_hand_motion(int,float*,float*) { return false; }
    volatile int g_VRPalmRIdx=-1,g_VRPalmLIdx=-1;
    // rust/crates/cvr-rig/src/vrik/pose_rig.rs's C++ half (the pose hook's install is anim_pose_hook.rs's own).
    bool cvr_cpp_install_weapon_aim_hooks() { return false; }
    void cvr_ensure_shared_memory() {}   // rust/crates/cvr-ffi/src/hand_natives.rs in the DLL
    void cvr_cpp_persist_live_controls() {}
    void* cvr_anim_object_for_entity(void*,const char*,bool) { return nullptr; }
    void* cvr_anim_player_entity() { return nullptr; }
    int cvr_anim_list_components(void*,char*,size_t,void**) { return 0; }
    bool cvr_anim_qword_is_heap(uint64_t) { return false; }
    int g_CalibrationBoneIndex=-1;float CyberpunkVR_ScannerSlots[21]{};
    volatile int g_PairN=0,g_PairFull=0;volatile uint32_t g_PairBones[24]{},g_PairTracks[24]{},g_PairOut[24]{},g_PairIn[24]{};
    volatile uintptr_t g_PairBuf[24]{};
    volatile int g_SmallN=0,g_SmallFull=0;volatile uintptr_t g_SmallBuf[16]{};
    volatile uint32_t g_SmallA4[16]{},g_SmallBones[16]{},g_SmallOut[16]{},g_SmallIn[16]{};
    volatile int g_WeaponRigDiag[8]{};char g_WeaponRigNames[512]{};const char* kWeaponPartNames[9]{};
    volatile int g_VREyeLeftIdx=-1,g_VREyeRightIdx=-1;
    // rust/crates/cvr-rig/src/vrik/calibration.rs's reads.
    volatile float g_VRPairSlewRate=1,g_VRPairLeadTicks=0,g_VRUserEyeHeight=0,g_VRChestDrop=0,g_VRChestFwd=0;
    float g_VRDiagBones[32*7]{};volatile int g_VRIKSolvesLastTick=0,g_VRIKSolvesMaxTick=0;
    volatile uintptr_t g_VRIKLastBufA=0,g_VRIKLastBufB=0;
    // rust/crates/cvr-rig/src/vrik/ui_popup.rs's C++ half.
    int CyberpunkVR_PopupVisualStateFilter=1;uint64_t CyberpunkVR_PopupVisualStateStats[3]{};
    int cvr_cpp_ui_popup_exchange(int) { return 0; }
    void cvr_cpp_ui_popup_closed_store(uint64_t) {}
    // rust/crates/cvr-rig/src/vrik/provider.rs's C++ half.
    void cvr_cpp_xr_set_physical_crouch_bridge_ready(bool) {}
    bool cvr_cpp_physical_optics_stereo_ready() { return false; }
    void cvr_cpp_set_physical_optics_path(int32_t) {}
    bool CvrLuaHostGcWanted(bool) { return false; }
    // rust/crates/cvr-rig/src/vrik/menu_source.rs's C++ half (src/Hooks/MenuMode.cpp's exports).
    std::atomic<uint32_t> CyberpunkVR_MenuSourceSeq{},CyberpunkVR_MenuMethodsLength{};
    uint8_t CyberpunkVR_MenuSource[168]{};char CyberpunkVR_MenuMethods[32768]{};
    uint64_t CyberpunkVR_DebugViewKeyMainNodes=0,CyberpunkVR_DebugViewKeyOtherNodes=0; }
#endif
extern "C" { volatile long CyberpunkVR_LeftStickClicks=0;int CyberpunkVR_LtLighterGate=1;volatile int g_VRSprintActive=-1; }

// The ladder's and the swim strokes' services (src/Hooks/HookServices.cpp in the plugin), and the action reader's thunk
// (src/Hooks/SwimmingActionLeafThunk.asm), which the bench never runs.
bool testOverlayVisibleLadder=false,testSimulatorLoaded=false,testScriptHooks=false;
extern "C" {
bool cvr_cpp_overlay_visible() { return OverlayIsVisible(); }
bool cvr_cpp_simulator_loaded() { return testSimulatorLoaded; }
struct CvrLadderControls { int32_t gripClimb, autoFinish; float finishDistance; };
void cvr_cpp_ladder_controls(CvrLadderControls* out) {
    out->gripClimb=g_liveControls.xrLadderGripClimb;out->autoFinish=g_liveControls.xrLadderAutoFinish;
    out->finishDistance=g_liveControls.xrLadderFinishDistance;
}
int cvr_cpp_breaststroke_swim() { return g_liveControls.xrBreaststrokeSwim; }
bool cvr_cpp_swimming_script_hooks_active() { return testScriptHooks; }
uintptr_t cvr_cpp_swimming_player_input_object(void* locomotion) {
    if(!locomotion)return 0;
    const auto wrapper=static_cast<uint8_t*>(locomotion)+0x30;
    const auto table=*reinterpret_cast<uintptr_t*>(wrapper);
    if(!table)return 0;
    const auto getter=*reinterpret_cast<uintptr_t*>(table);
    if(!getter)return 0;
    const auto context=reinterpret_cast<uintptr_t(__fastcall*)(void*)>(getter)(wrapper);
#ifdef VRIK_ROOM_TESTS
    if(!context || !cvr::roomscale::IsPlayerProvider(*reinterpret_cast<void**>(context+8)))return 0;
#else
    if(!context)return 0;
#endif
    return *reinterpret_cast<uintptr_t*>(context+0x10);
}
float CvrSwimmingActionDetour(void*,uint64_t) { return 0; }
}
namespace cvr::swimming { bool ScriptHooksActive() { return testScriptHooks; } }

#if CVR_RUST_VRIK
// cvr-rig calls the XR runtime's Rust functions by their names (rust/crates/cvr-rig/src/vrik/xr_calls.rs): the bench
// answers them with the adapters' stubs above and in camera_state.cpp.
extern "C" {
bool cvr_cpp_xr_locate_head_pose_at(int64_t,OpenXRHeadPose*);
bool cvr_cpp_xr_get_head_pose(OpenXRHeadPose*);
void cvr_cpp_xr_store_render_eye_pose(int,const OpenXRHeadPose*,uint32_t);
bool cvr_cpp_xr_hand_pose(int,OpenXRHeadPose*);
bool cvr_xr_get_roomscale_sample(cvr::roomscale::Sample* out,OpenXRHeadPose* head,XrTime* time) { return cvr_cpp_xr_roomscale_sample(out,head,time); }
void cvr_xr_flush_hands(const OpenXRHeadPose* head,XrTime time,const cvr::roomscale::Vec2* consumed) { cvr_cpp_xr_flush_hands_to_shared(head,time,consumed); }
bool cvr_xr_body_tracking_frame(cvr::body::TrackingFrame* out) { return cvr_cpp_xr_body_tracking_frame(out); }
bool cvr_xr_acquire_camera_pose_frame(OpenXRHeadPose* head,cvr::roomscale::Vec2* consumed,uint64_t* sequence) {
    return cvr_cpp_xr_acquire_camera_pose_frame(head,consumed,sequence);
}
bool cvr_xr_locate_head_pose_at(int64_t time,OpenXRHeadPose* out) { return cvr_cpp_xr_locate_head_pose_at(time,out); }
bool cvr_xr_get_head_pose(OpenXRHeadPose* out) { return cvr_cpp_xr_get_head_pose(out); }
void cvr_xr_store_render_eye_pose(int eye,const OpenXRHeadPose* pose,uint32_t seq) { cvr_cpp_xr_store_render_eye_pose(eye,pose,seq); }
bool cvr_xr_get_hand_pose(int hand,OpenXRHeadPose* out) { return cvr_cpp_xr_hand_pose(hand,out); }
float cvr_xr_hmd_yaw_rel_to_body() { return cvr_cpp_xr_hmd_yaw_rel_to_body(); }
float cvr_xr_hand_yaw_rel_to_body(int side) { return cvr_cpp_xr_hand_yaw_rel_to_body(side); }
void cvr_xr_push_rendered_frame_pose(bool vrcam,const OpenXRHeadPose* pose) { cvr_cpp_xr_push_rendered_frame_pose(pose,vrcam); }
bool cvr_xr_controller_state(VRControllerState* out,bool raw) { return !raw && OpenXRManager::Get().GetControllerState(out); }
bool cvr_xr_physical_crouch_requested() { return OpenXRManager::Get().PhysicalCrouchRequested(); }
bool cvr_xr_published_hand_frame(OpenXRHeadPose* head,OpenXRHeadPose* hands,int which,uint64_t* sequence,uint64_t* stamp) {
    return (which==1 || which==2) && OpenXRManager::Get().GetGestureHandFrame(head,hands,sequence,stamp,which==2);
}
CvrXrSessionShared* cvr_xr_session_shared() {
    static CvrXrSessionShared session{};
    session.running.store(cvr_cpp_xr_session_running());session.focused.store(cvr_cpp_session_focused());
    return &session;
}
}
#endif

#if CVR_RUST_VRIK
// What src/Anim/CharacterRigRust.cpp and src/Hooks/InputServices.cpp defined for the Rust solve until the plugin's
// Rust build took them over (rust/crates/cvr-render/src/cpp_services.rs): the forwards into this program's stand-ins.
#include "Hooks/BodyHeading.hpp"
extern "C" {
bool cvr_cpp_pose_device_cam_active() { return DeviceCamActive(); }
void cvr_cpp_ladder_read_hand_kinds(int* kinds) { cvr::ladder::ReadHandKinds(kinds); }
void cvr_cpp_ladder_constrain_hand(int side,float* target,float* rot,const float* entityPos,const float* entityRot) {
    cvr::ladder::ConstrainHand(side,target,rot,entityPos,entityRot);
}
void cvr_cpp_roomscale_camera_consumed(float* out) { const auto v=cvr::roomscale::CameraConsumed();out[0]=v.x;out[1]=v.y; }
bool cvr_cpp_roomscale_camera_rest_neck(float* out) {
    cvr::roomscale::Vec2 v{};const bool ok=cvr::roomscale::CameraRestNeck(&v);out[0]=v.x;out[1]=v.y;return ok;
}
bool cvr_cpp_body_yaw_follow_active() { return BodyYawFollowActive(); }
float cvr_cpp_vrik_body_tracking_yaw(float nativeYaw) { return BodyTrackingYaw(nativeYaw); }
void cvr_cpp_xr_set_shared_slot(int slot, float value) {
    float* p = cvr_xr_switches()->sharedHands; if (p && slot >= 0 && slot < 256) p[slot] = value;
}
float cvr_cpp_xr_get_shared_slot(int slot) {
    float* p = cvr_xr_switches()->sharedHands; return (p && slot >= 0 && slot < 256) ? p[slot] : 0.0f;
}
void cvr_cpp_quest_suspend_manual_clue_input() { cvr::quest::SuspendManualClueInput(); }
bool cvr_cpp_quest_consume_manual_clue_button(bool pressed, bool allowed) { return cvr::quest::ConsumeManualClueButton(pressed, allowed); }
void cvr_cpp_swimming_reset_input() { cvr::swimming::ResetInput(); }
bool cvr_cpp_swimming_sprint_input(bool wanted) { return cvr::swimming::SprintInput(wanted); }
void cvr_cpp_ladder_reset_input() { cvr::ladder::ResetInput(); }
void cvr_cpp_ladder_update_input(const VRControllerState* c) { cvr::ladder::UpdateInput(*c); }
void cvr_cpp_cyberware_chord_dispatch() { cvr::input::DispatchCyberwareChord(); }
bool cvr_cpp_vrui_captures_input() { return cvr::vrui::CapturesInput(); }
bool cvr_cpp_reach_use_held() { return cvr::reach::UseHeld(); }
int cvr_cpp_ui_popup_open() { return g_uiPopupOpen.load(std::memory_order_relaxed); }
uint64_t cvr_cpp_ui_popup_closed_ms() { return g_uiPopupClosedMs.load(std::memory_order_relaxed); }
int cvr_cpp_verbose_log() { return g_verboseLog; }
bool cvr_cpp_is_driving() { return g_isDriving.load(std::memory_order_relaxed); }
void cvr_cpp_set_pending_snap_yaw(int32_t bits) { InterlockedExchange(&g_pendingSnapYawDeltaBits, bits); }
void cvr_cpp_set_look_stick_x(float x) { g_lookStickX.store(x, std::memory_order_relaxed); }
bool cvr_cpp_game_window_in_front() { return GetForegroundWindow() == g_gameHwnd; }
bool cvr_cpp_vrmenu_captures_buttons() { return cvr::vrmenu::CapturesButtons(); }
bool cvr_cpp_vrmenu_wheel_armed(uint64_t now) { return cvr::vrmenu::WheelArmed(now); }
void cvr_cpp_vrmenu_publish_wheel_hold(bool pending, bool held) { cvr::vrmenu::PublishWheelHold(pending, held); }
bool cvr_cpp_keypad_input_active() { return cvr::input::KeypadInputActive(); }
void cvr_cpp_keypad_publish_stick(float x, float y, float deadzone) { cvr::input::PublishKeypadStick(x, y, deadzone); }
float cvr_cpp_lobby_curtain() { return cvr::lobby::Curtain(); }
bool cvr_cpp_lobby_intro_running() { return cvr::lobby::intro::Running(); }
bool cvr_cpp_in_braindance() { return InBraindance(); }
bool cvr_cpp_scene_stick_yaw_armed() { return SceneStickYawArmed(); }
}
#endif
