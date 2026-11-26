// The camera state src/Core/VrCore.cpp keeps for PatchCamera and LocateCamera, at the plugin's defaults, and the
// services the camera hooks ask (src/Hooks/HookServices.cpp in the plugin), scripted by the traces -- for every bench
// program, since the Rust half links all of them.
#define _ALLOW_KEYWORD_MACROS 1
#include <windows.h>
#include "Camera/CameraState.hpp"
#include "Camera/GazeCenter.hpp"
#include "Anim/WeaponAimState.hpp"
#include "Hooks/SightLine.hpp"
#include "Camera/SurveillanceFollow.hpp"
#include "Core/LiveControls.hpp"
#include "Core/Telemetry.hpp"
#include "Core/VrCoreShared.hpp"
#include "Fbt/Game/Body.hpp"
#include "Render/Afw.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <atomic>
#include <cstdint>
#include <cstring>
#include <map>
#include <vector>

extern "C" {
uint64_t g_patchCameraHits=0,g_locateCameraHits=0;
std::atomic<uintptr_t> g_lensComp{0},g_camObjMain{0},g_camObjVrcam{0},g_takeoverEntity{0};
float g_devCamLastWritten[4]={0,0,0,0},g_devCamBase[4]={0,0,0,1},g_devCamViewQuat[4]={0,0,0,1};
float g_devCamAimYaw=0,g_devCamAimPitch=0,g_devCamFovOrig=0;
std::atomic<int> g_devCamAimValid{0},g_devCamBaseValid{0},g_devCamFovSaved{0},g_devCamViewValid{0},g_devCamPosValid{0};
std::atomic<int32_t> g_devCamPosFP[3]={};
volatile float g_engineCamQuat[4]={0,0,0,1};volatile uint32_t g_engineCamQuatValid=0;
std::atomic<uint64_t> g_camComposedForPresent{~0ull};
volatile float g_headingSy=0,g_headingCy=1;
volatile float g_viewYawUsedRad=0;volatile int g_viewYawUsedValid=0;
float g_bdSceneQuat[4]={0,0,0,1};std::atomic<int> g_bdScenePoseValid{0};std::atomic<int32_t> g_bdScenePosFP[3]={};
volatile float g_headQuatComposed[4]={0,0,0,1};
std::atomic<int32_t> g_locatePosValid{0},g_locatePosFP[3]={},g_locateCenterFP[3]={};
std::atomic<uint32_t> g_headDeltaValid{0};std::atomic<int32_t> g_headDeltaFP[3]={};
volatile float g_normalFovOverrideValue=90.0f;
float CyberpunkVR_SceneStickYawDps=0,CyberpunkVR_DebugSceneStickYawDeg=0;
float CyberpunkVR_SightApproach[3]={};
uint32_t CyberpunkVR_DebugTidPatchCam=0;
uint64_t CyberpunkVR_DebugCamThreadSwitches=0,CyberpunkVR_DebugPatchCamMain=0;
int32_t CyberpunkVR_DeviceCamOrient=1;
int CyberpunkVR_PoseLocateAtWrite=1;
float CyberpunkVR_DebugHeadingStepDeg=0;
int32_t CyberpunkVR_BdSceneBaseInPatch=1;
uint64_t CyberpunkVR_DebugBdSceneBaseInPatch=0,CyberpunkVR_DebugPoseLocatedAtWrite=0,CyberpunkVR_DebugPoseFromCache=0;
uint64_t CyberpunkVR_DebugCamComposed=0,CyberpunkVR_DebugCamVrcamFirst=0,CyberpunkVR_DebugCamNoHmd=0;
int CyberpunkVR_CamWriteOrientInVehicle=1;
uint64_t CyberpunkVR_DebugBdEditorAlign=0;
int32_t CyberpunkVR_BdPushTransform=1;
uint64_t CyberpunkVR_DebugVrcamPosFromMain=0;
int32_t CyberpunkVR_EngineCamPosFP[3]={0,0,0};int CyberpunkVR_EngineCamPosValid=0;
float CyberpunkVR_DevCamOffsetX=0,CyberpunkVR_DevCamOffsetY=0,CyberpunkVR_DevCamOffsetZ=0;
int CyberpunkVR_CamMountCompensate=0;
float CyberpunkVR_DebugCamMountM=0;
uint64_t CyberpunkVR_DebugDeltaRebuilt=0,CyberpunkVR_DebugVrcamPosWrites=0,CyberpunkVR_DebugMainPosWrites=0;
int32_t CyberpunkVR_BdIpdInLocate=1;
uint64_t CyberpunkVR_DebugIpdWorldWrites=0;
int CyberpunkVR_IpdInPosAB=0;
float g_dbgEntryYaw=0,g_dbgEntryPosX=0,g_dbgEntryPosY=0,g_dbgEntryPosZ=0;
volatile int32_t g_lastLocatePosFP[3]={},g_lastIpdShiftFP[3]={};volatile float g_lastLocateQuat[4]={0,0,0,1};
// LocateCamera's switches and counters (src/Core/VrCore.cpp, the stereo module and the optics native in the plugin).
float CyberpunkVR_MainAdsZoomFactor=0.f;int32_t CyberpunkVR_SightZoom=0;
int32_t CyberpunkVR_BdQuatFromBuffer=0,CyberpunkVR_BdBaseFromLocate=1,CyberpunkVR_BdQuatFromWriteSite=0,CyberpunkVR_BdMainPosFromScene=0,CyberpunkVR_BdPushBase=1;
int CyberpunkVR_VehicleAnchorFromViewYaw=1;
uint64_t CyberpunkVR_DebugBdQuatFromBuffer=0,CyberpunkVR_DebugBdBaseFromLocate=0,CyberpunkVR_DebugBdBaseFromScript=0,CyberpunkVR_DebugBdQuatFromWriteSite=0;
uint64_t CyberpunkVR_DebugBdMainPos=0,CyberpunkVR_DebugBdIpdLocate=0,CyberpunkVR_DebugTakeoverPush=0,CyberpunkVR_DebugBdPushTransform=0,CyberpunkVR_DebugBdPushMain=0;
uint32_t CyberpunkVR_DebugTidLocateCam=0;
// WeaponAim's inputs (src/Natives/OrientationProvider.cpp, NativeState.cpp in the plugin).
volatile uint32_t g_provMuzzleSeq=0;volatile long g_provRecoilSeqSeen=-1;
__declspec(dllexport) volatile uint32_t CyberpunkVR_DualWieldRecords[2]={};
__declspec(dllexport) float CyberpunkVR_SightLineOffset[3]={};__declspec(dllexport) uint64_t CyberpunkVR_SightLineTick=0;
__declspec(dllexport) int32_t CyberpunkVR_HitscanFromSightLine=1;
}
volatile uintptr_t g_waExeBase=0;
std::atomic<int> g_remoteCamOn{0};std::atomic<uint64_t> g_takeoverEntityId{0};
void* volatile g_dbgFovCamState=nullptr;volatile float g_dbgLastOriginalFov=0;

extern "C" { uint64_t CyberpunkVR_DebugPatchCamOther=0,CyberpunkVR_DebugPatchCamVrcam=0;unsigned int CyberpunkVR_DebugPatchCamDevice=0; }
TelemetryData* g_telemetry=nullptr;
// src/Core/VrCore.cpp's quaternion helpers and span test, as there.
bool IsPlausiblePositionVec4(const float* v);   // src/Camera/CameraMath.cpp (or its Rust port)
void NormalizeQuat(float& x,float& y,float& z,float& w) {
    const float lenSq=x*x+y*y+z*z+w*w;
    if(lenSq<=0.000001f) { x=0.0f;y=0.0f;z=0.0f;w=1.0f;return; }
    const float invLen=1.0f/sqrtf(lenSq);x*=invLen;y*=invLen;z*=invLen;w*=invLen;
}
void MulQuat(float ax,float ay,float az,float aw,float bx,float by,float bz,float bw,float& ox,float& oy,float& oz,float& ow) {
    ox=aw*bx+ax*bw+ay*bz-az*by;oy=aw*by-ax*bz+ay*bw+az*bx;oz=aw*bz+ax*by-ay*bx+az*bw;ow=aw*bw-ax*bx-ay*by-az*bz;
}
bool IsPlausibleCameraSpan(const float* a,const float* b) {
    if(!a || !b)return false;
    if(!IsPlausiblePositionVec4(a) || !IsPlausiblePositionVec4(b))return false;
    const float dx=b[0]-a[0],dy=b[1]-a[1],dz=b[2]-a[2];
    return dx*dx+dy*dy+dz*dz<25.0f;
}

// ---- the scripted services ----
std::map<uintptr_t,int> testCameraKinds;           // ClassifyPatchCameraOwner: the owners the trace names
bool testLocateOwnsTakeover=false,testBdPushOwns=false,testSurveillanceOk=false,testFbtEyeOk=false,testAfwActive=false;
float testSurveillanceYaw=0,testFbtEyeZ=1.7f;int testAfwEye=0,testMenuRect=0;
std::vector<float> testCameraCalls;                 // what the camera hooks did to the rest of the plugin
OpenXRHeadPose testHeadPose{},testLocatedPose{},testFrameSample{};bool testHeadPoseOk=false,testLocatedOk=false,testFrameSampleOk=false;
uint64_t testAimEpoch=0;int64_t testAimTime=0;
// LocateCamera's OpenXR answers: the render eye, the baked camera offset and the right hand.
int testRenderEye=0;float testCamOffset[3]={};OpenXRHeadPose testHandPose{};bool testHandOk=false;
// The tank hooks' answers: the mounted vehicle and whether it is a tank.
uintptr_t testMountedVehicle=0;bool testIsTank=false;
uintptr_t CurrentMountedVehicleIdentity() { return testMountedVehicle; }
PatchFastPath g_patchFast{};

int ClassifyPatchCameraOwner(void* owner) {
    const auto found=testCameraKinds.find(reinterpret_cast<uintptr_t>(owner));
    if(found==testCameraKinds.end())return 0;
    if(found->second==1)++CyberpunkVR_DebugPatchCamMain;
    return found->second;
}
void RefreshLensFromComponent() { testCameraCalls.push_back(401); }
bool BraindanceCameraMatch(uintptr_t,const float* quat) {
    testCameraCalls.insert(testCameraCalls.end(),{402,quat[0],quat[1],quat[2],quat[3]});return false;
}
bool LocateOwnsTakeover() { return testLocateOwnsTakeover; }
bool BdPushOwnsComposition() { return testBdPushOwns; }
#if !defined(VRIK_WATER_TESTS)
// The scene comfort is LocateCamera's (stateful there; the water programs link it); the other benches hand the
// scene's quaternion back normalised.
bool SceneComfortApply(const float* raw,float* out) {
    const float n=std::sqrt(raw[0]*raw[0]+raw[1]*raw[1]+raw[2]*raw[2]+raw[3]*raw[3]);
    for(int k=0;k<4;++k)out[k]=n>1e-6f ? raw[k]/n:(k==3 ? 1.f:0.f);
    return true;
}
#endif
void PushLensHeadTransform(const float* quat) { testCameraCalls.insert(testCameraCalls.end(),{403,quat[0],quat[1],quat[2],quat[3]}); }
// The surveillance view PatchCamera asks (its trace scripts it; the water benches also link the real one).
static bool TestSurveillanceViewYaw(uintptr_t,uintptr_t,uint64_t,float ownerYaw,float& yaw) {
    if(!testSurveillanceOk)return false;yaw=testSurveillanceYaw+ownerYaw*.5f;return true;
}
namespace cvr::fbt::game { bool CameraEyeHeight(float* z) { if(!testFbtEyeOk)return false;*z=testFbtEyeZ;return true; } }
namespace cvr::afw { bool Active() { return testAfwActive; } int FrameEye() { return testAfwEye; } }
extern "C" int GetMenuRectMode() { return testMenuRect; }
extern "C" int GetMenuMode() { return g_menuModeValue; }

extern "C" {
int cvr_classify_patch_camera_owner(void* owner) { return ClassifyPatchCameraOwner(owner); }
void cvr_refresh_lens_from_component() { RefreshLensFromComponent(); }
bool cvr_braindance_camera_match(uintptr_t object,const float* quat) { return BraindanceCameraMatch(object,quat); }
bool cvr_locate_owns_takeover() { return LocateOwnsTakeover(); }
bool cvr_bd_push_owns_composition() { return BdPushOwnsComposition(); }
bool cvr_cpp_scene_comfort_apply(const float* raw,float* out) { return SceneComfortApply(raw,out); }
void cvr_push_lens_head_transform(const float* quat) { PushLensHeadTransform(quat); }
bool cvr_cpp_surveillance_view_yaw(uintptr_t lens,uintptr_t owner,uint64_t origin,float ownerYaw,float* yaw) {
    return yaw && TestSurveillanceViewYaw(lens,owner,origin,ownerYaw,*yaw);
}
bool cvr_cpp_fbt_camera_eye_height(float* z) { return z && cvr::fbt::game::CameraEyeHeight(z); }
bool cvr_afw_active() { return cvr::afw::Active(); }
int cvr_afw_frame_eye() { return cvr::afw::FrameEye(); }
float cvr_cpp_scene_cam_comfort_deg() { return g_liveControls.xrSceneCamComfortDeg; }
uint64_t cvr_cpp_xr_frame_aim_epoch() { return testAimEpoch; }
int64_t cvr_cpp_xr_frame_aim_time() { return testAimTime; }
bool cvr_cpp_xr_locate_head_pose_at(int64_t time,OpenXRHeadPose* out) {
    testCameraCalls.insert(testCameraCalls.end(),{404,float(time)});if(!testLocatedOk)return false;*out=testLocatedPose;return true;
}
bool cvr_cpp_xr_get_head_pose(OpenXRHeadPose* out) { if(!testHeadPoseOk)return false;*out=testHeadPose;return true; }
void cvr_cpp_xr_push_render_head_pose(const OpenXRHeadPose* pose) {
    testCameraCalls.insert(testCameraCalls.end(),{405,pose->posX,pose->posY,pose->posZ,pose->oriW,float(pose->originSerial)});
}
bool cvr_cpp_xr_acquire_frame_head_sample(OpenXRHeadPose* out) { if(!testFrameSampleOk)return false;*out=testFrameSample;return true; }
uintptr_t cvr_cpp_patch_fast_path() { return reinterpret_cast<uintptr_t>(&g_patchFast); }
uintptr_t cvr_cpp_telemetry_patch_hits() { return 0; }
void cvr_cpp_telemetry_locate(uint32_t,uint64_t,float) {}
bool cvr_cpp_remote_cam_on() { return g_remoteCamOn.load()!=0; }
uint64_t cvr_cpp_takeover_entity_id() { return g_takeoverEntityId.load(); }
float cvr_cpp_gaze_tangent_y() { return cvr::gaze::Read().tangentY; }
uintptr_t cvr_mounted_vehicle_identity() { return testMountedVehicle; }
bool cvr_cpp_is_tank(uintptr_t) { testCameraCalls.push_back(600);return testIsTank; }
void cvr_cpp_camera_offsets(float* out) {
    out[0]=g_liveControls.xrHeadOffsetX;out[1]=g_liveControls.xrHeadOffsetY;out[2]=g_liveControls.xrHeadOffsetZ;
    out[3]=g_liveControls.xrVehHeadOffsetX;out[4]=g_liveControls.xrVehHeadOffsetY;out[5]=g_liveControls.xrVehHeadOffsetZ;
}
int cvr_cpp_xr_render_eye_index() { return testRenderEye; }
void cvr_cpp_xr_store_render_eye_pose(int eye,const OpenXRHeadPose* pose,uint32_t seq) {
    testCameraCalls.insert(testCameraCalls.end(),{406,float(eye),pose->posX,pose->oriW,float(seq)});
}
void cvr_cpp_xr_camera_offset(float* out) { for(int k=0;k<3;++k)out[k]=testCamOffset[k]; }
bool cvr_cpp_xr_hand_pose(int hand,OpenXRHeadPose* out) {
    testCameraCalls.insert(testCameraCalls.end(),{407,float(hand)});if(!testHandOk)return false;*out=testHandPose;return true;
}
}

#if defined(VRIK_WATER_TESTS)
// The ini parse's subsystems (src/Core/LiveControlsParse.cpp): each takes the lines of its own test key, logged in the
// order they were offered -- the framegen one also takes xr_mono_cascades, which the loop asks only after it.
#include "Core/LiveControlsParse.hpp"
#include <string>
std::vector<std::pair<int,std::string>> testSubsystemLines;
static bool TestSubsystem(int which,const char* line,const char* key) {
    if(std::strncmp(line,key,std::strlen(key))!=0)return false;
    testSubsystemLines.emplace_back(which,line);return true;
}
namespace cvr::hud { bool ParseLayoutSetting(const char* line,LayoutSettings&) { return TestSubsystem(0,line,"xr_hud_element_"); } }
namespace cvr::framegen {
bool ParseSetting(const char* line,Settings&) { return TestSubsystem(1,line,"xr_framegen_") || TestSubsystem(1,line,"xr_mono_cascades"); }
}
// Foveated DLSS, fixed VRS and the hidden-area mask parse their own lines (src/Render/*Settings.cpp, or their Rust
// policies): both builds route those keys to the real parsers, so the settings trace compares the same thing.
namespace cvr::vrui { bool ParseSetting(const char* line,Settings&) { return TestSubsystem(5,line,"xr_vrui_"); } }
int ClampRuntimeMode(int value) { return value==1 ? 1:0; }   // src/Core/VrCore.cpp
// The menu mode hook's lobby: it holds the menu up for the states the trace names.
std::vector<uintptr_t> testLobbyHolds;
extern "C" bool cvr_cpp_lobby_holds_render(uintptr_t menuState) {
    for(const auto s:testLobbyHolds)if(s==menuState)return true;
    return false;
}
// The mirror's item handlings: the features the trace names, with their state and type.
std::map<uintptr_t,uint32_t> testItemHandlings;
extern "C" uint32_t cvr_cpp_item_handling_bits(const void* feature) {
    const auto found=testItemHandlings.find(reinterpret_cast<uintptr_t>(feature));
    return found==testItemHandlings.end() ? 0xFFFFu:found->second;
}
// StoryAttention's reach: a point while the trace gives one.
bool testReachOk=false;float testReachPoint[3]{};
extern "C" bool cvr_cpp_reach_aim_point(float* point) { if(!testReachOk)return false;for(int k=0;k<3;++k)point[k]=testReachPoint[k];return true; }
// SurveillanceBody's component: what the trace says of it.
struct SurveillanceControl {uintptr_t owner{};uint64_t entity{};bool enabled{};uint8_t kind{};};
std::map<uintptr_t,SurveillanceControl> testSurveillanceControls;
extern "C" bool cvr_cpp_surveillance_control(uintptr_t component,SurveillanceControl* out) {
    const auto found=testSurveillanceControls.find(component);if(found==testSurveillanceControls.end())return false;*out=found->second;return true;
}
#else
// The other benches link the Rust menu mode hook too (src/Hooks/MenuModeCallback.cpp defines these in the water ones).
extern "C" {
#if !CVR_RUST_VRIK   // the Rust variants: rust/crates/cvr-render/src/rig_test_exports.rs (the plugin's values, the same)
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_MenuTransitionSeq{},CyberpunkVR_MenuTransitionCount{};
#endif
__declspec(dllexport) uint64_t CyberpunkVR_MenuTransitions[64*4]{};
bool cvr_cpp_lobby_holds_render(uintptr_t) { return false; }
#if !CVR_RUST_VRIK   // the Rust variants: rust/crates/cvr-render/src/rig_test_exports.rs (the plugin's values, the same)
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_LeftHandedNamesMirrored{};
__declspec(dllexport) std::atomic<uintptr_t> CyberpunkVR_LeftHandedController{};
#endif
__declspec(dllexport) uint64_t CyberpunkVR_LeftHandedNameLog[64*2]{};
#if !CVR_RUST_VRIK   // the Rust variants: rust/crates/cvr-render/src/rig_test_exports.rs (the plugin's values, the same)
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_LeftHandedNameLogCount{};
#endif
uint32_t cvr_cpp_item_handling_bits(const void*) { return 0xFFFF; }
#if !CVR_RUST_VRIK   // the Rust variants: rust/crates/cvr-render/src/rig_test_exports.rs (the plugin's values, the same)
__declspec(dllexport) int CyberpunkVR_StoryAttentionHookReady{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StoryAttentionCounters[5]{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ReachTurns{};
#endif
bool cvr_cpp_reach_aim_point(float*) { return false; }
#if !CVR_RUST_VRIK   // the Rust variants: rust/crates/cvr-render/src/rig_test_exports.rs (the plugin's values, the same)
__declspec(dllexport) int CyberpunkVR_SurveillanceBodyFollow=1;
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_SurveillanceBodyUpdates{},CyberpunkVR_SurveillanceBodyInjected{},CyberpunkVR_SurveillanceAutoFiltered{};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SurveillanceBodyDebugSeq{};
__declspec(dllexport) float CyberpunkVR_SurveillanceBodyDebug[9]{},CyberpunkVR_SurveillanceAutoDebug[6]{};
#endif
void* CvrSurveillanceInputOriginal{};
bool cvr_cpp_surveillance_control(uintptr_t,void*) { return false; }
}
#endif
// The look-at ray's thunk (src/Hooks/StoryAttentionThunk.asm in the plugin): only its address is taken here.
extern "C" void CvrStoryAttentionDetour() {}
// The surveillance thunks (src/Hooks/SurveillanceInputThunk.asm in the plugin): only their addresses are taken here.
extern "C" void CvrSurveillanceInputDetour() {}
extern "C" void CvrSurveillanceFinishDetour() {}
