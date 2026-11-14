#include "Hooks/LadderInput.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/SwimmingGesture.hpp"
#include "Camera/HandViewFrame.hpp"
#include "Camera/CameraLink.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/LadderRungWrist.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Camera/CameraState.hpp"
#include "Overlay/ImGuiOverlay.hpp"
#include <mutex>

namespace cvr::ladder {
namespace {
std::mutex s_mutex;
uintptr_t s_player{};
uint64_t s_stamp{},s_inputStamp{},s_testUntil{};
int s_detailed=-1;
bool s_paused{};
Geometry s_geometry;
Climber s_climber;
float s_axis{},s_testGrip[2]{};
Vec s_handWorld[2]{};
bool IsFresh(uint64_t now) { return Fresh(s_player,cvr::roomscale::PlayerIdentity(),s_stamp,now,s_detailed); }
bool ContextAllowed() {
    const int tier=g_sceneTier.load(std::memory_order_relaxed);
    return GripEnabled(g_liveControls.xrHandPhysics!=0,g_liveControls.xrLadderGripClimb!=0) && !g_isInVehicle && g_menuModeValue==0 &&
        !DeviceCamActive() && !g_bdActive.load(std::memory_order_relaxed) &&
        !g_uiPopupOpen.load(std::memory_order_relaxed) && !OverlayIsVisible() && tier>0 && tier<4;
}
void Clear() { s_climber.Reset();s_axis=0;s_inputStamp=0; }
Vec V(XrVector3f v) { return {v.x,v.y,v.z}; }
}
bool Active() { std::lock_guard lock(s_mutex);return IsFresh(XrDiagNowUs()); }
void ReadHandKinds(int* kinds) {
    kinds[0]=kinds[1]=0;if(!ContextAllowed())return;
    std::lock_guard lock(s_mutex);const auto now=XrDiagNowUs();
    if(s_paused || !IsFresh(now) || !s_inputStamp || now<s_inputStamp || now-s_inputStamp>100000)return;
    for(int side=0;side<2;++side)if(s_climber.Held(side))kinds[side]=s_climber.Kind(side);
}
void Publish(uintptr_t player,int detailed,bool paused,const Geometry* geometry) {
    std::lock_guard lock(s_mutex);
    if(player!=s_player || !OnLadder(detailed) || paused)Clear();
    Geometry next=geometry ? *geometry:Geometry{};
    if(player==s_player && next.valid && s_geometry.identity==next.identity) {
        next.measuredTop=s_geometry.measuredTop;
        next.topRailCount=s_geometry.topRailCount;
        std::copy_n(s_geometry.topRails,next.topRailCount,next.topRails);
    }
    s_player=player;s_detailed=detailed;s_paused=paused;s_stamp=XrDiagNowUs();
    s_geometry=next;
}
bool PublishTop(uintptr_t player,Vec ladderPosition,Vec origin,Vec right,Vec normal,Vec up) {
    std::lock_guard lock(s_mutex);
    if(player!=s_player || !IsFresh(XrDiagNowUs()) || Length(ladderPosition-s_geometry.position)>.01f)return false;
    return s_geometry.SetTopRails(origin,right,normal,up);
}
bool PublishMeasuredTop(uintptr_t player,Vec ladderPosition,float top) {
    std::lock_guard lock(s_mutex);
    if(player!=s_player || !IsFresh(XrDiagNowUs()) || Length(ladderPosition-s_geometry.position)>.01f)return false;
    return s_geometry.SetMeasuredTop(top);
}
void ResetInput() { std::lock_guard lock(s_mutex);Clear(); }
bool SetSimulatorGrip(float left,float right,int milliseconds) {
    if(!GetModuleHandleW(L"openxr_simulator.dll"))return false;
    std::lock_guard lock(s_mutex);
    s_testGrip[0]=std::clamp(left,0.0f,1.0f);s_testGrip[1]=std::clamp(right,0.0f,1.0f);
    s_testUntil=milliseconds>0 ? XrDiagNowUs()+uint64_t(std::min(milliseconds,2000))*1000:0;
    return true;
}
void UpdateInput(const VRControllerState& controllers) {
    const bool context=ContextAllowed();Geometry geometry;float grips[2]={controllers.leftGrip,controllers.rightGrip};
    {
        std::lock_guard lock(s_mutex);const auto now=XrDiagNowUs();
        if(!context || s_paused || !IsFresh(now) || !s_geometry.valid) { Clear();return; }
        geometry=s_geometry;
        if(now<s_testUntil)for(int i=0;i<2;++i)grips[i]=s_testGrip[i];
    }
    Frame sample{};sample.grip[0]=grips[0];sample.grip[1]=grips[1];
    sample.autoFinish=g_liveControls.xrLadderAutoFinish!=0;
    sample.finishDistance=g_liveControls.xrLadderFinishDistance;
    OpenXRHeadPose head{},hands[2]{};uint64_t sequence{},stamp{};
    float entityPos[3],entityRot[4],trackingYaw{};VrikTransformSnapshot pair{};
    cvr::camera::AnchorRecipe recipe{};
    const bool valid=OpenXRManager::Get().GetGestureHandFrame(&head,hands,&sequence,&stamp) &&
        VRIK_ReadCurrentBodyFrame(entityPos,entityRot,&trackingYaw) &&
        VRIK_ReadNativeTransformSnapshot(&pair) && pair.valid && !pair.unavailable &&
        cvr::camera::AnchorRecipeRead(&recipe);
    if(valid) {
        const XrQuaternionf entityQ{entityRot[0],entityRot[1],entityRot[2],entityRot[3]};
        const auto base=RotateVector(ConjugateQuat(entityQ),
            {pair.bodyCameraMinusEntity[0],pair.bodyCameraMinusEntity[1],pair.bodyCameraMinusEntity[2]});
        const auto consumed=cvr::roomscale::CameraConsumed(head.originSerial);
        const float bodyYaw=2*std::atan2(entityRot[2],entityRot[3]);
        const XrQuaternionf headQ{head.oriX,head.oriY,head.oriZ,head.oriW};
        const auto view=cvr::camera::BuildHandViewFrame({base.x,base.y,base.z},recipe,
            {head.posX-consumed.x,head.posY,head.posZ+consumed.y},headQ,bodyYaw,trackingYaw,
            {g_headingPitchS,0,0,g_headingPitchC});
        const Vec entity{entityPos[0],entityPos[1],entityPos[2]};
        for(int side=0;side<2;++side) {
            const float scale=side ? g_VRScaleR:g_VRScaleL;
            const auto local=RotateVector(view.rotation,{hands[side].posX*scale,-hands[side].posZ*scale,hands[side].posY*scale});
            const XrQuaternionf controller{hands[side].oriX,-hands[side].oriZ,hands[side].oriY,hands[side].oriW};
            const XrQuaternionf correction=side ? XrQuaternionf{g_VRWristR_I,g_VRWristR_J,g_VRWristR_K,g_VRWristR_R}:
                XrQuaternionf{g_VRWristL_I,g_VRWristL_J,g_VRWristL_K,g_VRWristL_R};
            const auto wrist=MultiplyQuat(view.rotation,MultiplyQuat(controller,correction));
            const auto palm=profile::hands[side].contact;
            const auto contact=RotateVector(wrist,{palm[0],palm[1],palm[2]});
            const Vec tuning=side ? Vec{g_VROffRX,g_VROffRY,g_VROffRZ}:Vec{g_VROffLX,g_VROffLY,g_VROffLZ};
            sample.world[side]=entity+V(RotateVector(entityQ,
                {view.position.x+local.x+tuning.x+contact.x,view.position.y+local.y+tuning.y+contact.y,
                 view.position.z+local.z+tuning.z+contact.z}));
            const auto tracking=cvr::swimming::HandInTrackingSpace({head.posX,head.posY,head.posZ},headQ,
                {hands[side].posX,hands[side].posY,hands[side].posZ});
            sample.tracking[side]={tracking.x*recipe.scale,-tracking.z*recipe.scale,tracking.y*recipe.scale};
            sample.handValid[side]=hands[side].valid && (side ? controllers.rightHandValid:controllers.leftHandValid);
        }
        const float c=std::cos(trackingYaw),s=std::sin(trackingYaw);
        sample.trackingUp={c*geometry.up.x+s*geometry.up.y,-s*geometry.up.x+c*geometry.up.y,geometry.up.z};
        sample.height=Dot(entity-geometry.position,geometry.up);
        sample.sequence=sequence;sample.origin=head.originSerial;sample.stamp=stamp;sample.valid=head.valid;
    }
    std::lock_guard lock(s_mutex);const auto now=XrDiagNowUs();
    if(!IsFresh(now) || s_paused || geometry.identity!=s_geometry.identity) { Clear();return; }
    for(int i=0;i<2;++i)s_handWorld[i]=sample.world[i];
    s_axis=s_climber.Update(geometry,sample,now,valid);s_inputStamp=now;
}
float NativeAction() {
    if(!ContextAllowed())return 0;
    std::lock_guard lock(s_mutex);const auto now=XrDiagNowUs();
    if(s_paused || !IsFresh(now) || !s_inputStamp || now<s_inputStamp || now-s_inputStamp>100000)return 0;
    return s_axis;
}
float MixMove(float original,float rawX,float rawY) {
    if(std::abs(rawX)>.15f || std::abs(rawY)>.15f) { ResetInput();return original; }
    const float axis=NativeAction();return axis!=0 ? axis:original;
}
void ConstrainHand(int side,float* targetModel,float* handRotation,const float* position,const float* rotation) {
    if(!targetModel || !handRotation || !position || !rotation || side<0 || side>1 || !ContextAllowed())return;
    std::lock_guard lock(s_mutex);const auto now=XrDiagNowUs();
    if(s_paused || !IsFresh(now) || !s_inputStamp || now<s_inputStamp || now-s_inputStamp>100000)return;
    // The caller supplies the one body frame latched for both hands and the
    // view. Reading the newest transform independently here would mix ticks.
    Vec point{targetModel[0],targetModel[1],targetModel[2]};
    Rotation wrist{handRotation[0],handRotation[1],handRotation[2],handRotation[3]};
    if(s_climber.Constrain(side,{position[0],position[1],position[2]},
        {rotation[0],rotation[1],rotation[2],rotation[3]},point,wrist)) {
        if(s_climber.Kind(side)==2) {
            Rotation body{rotation[0],rotation[1],rotation[2],rotation[3]};body.Normalize();
            CorrectRungWrist(side,body.Inverse().Rotate(s_geometry.right),body.Inverse().Rotate(s_geometry.up),point,wrist);
        }
        targetModel[0]=point.x;targetModel[1]=point.y;targetModel[2]=point.z;
        handRotation[0]=wrist.x;handRotation[1]=wrist.y;handRotation[2]=wrist.z;handRotation[3]=wrist.w;
    }
}
float DebugValue(int index) {
    if(index==20)return NativeAction();
    std::lock_guard lock(s_mutex);
    if(index>=10 && index<16) {
        const auto p=s_handWorld[(index-10)/3];return (index-10)%3==0 ? p.x : (index-10)%3==1 ? p.y:p.z;
    }
    switch(index) {
    case 0:return float(s_detailed);
    case 1:return IsFresh(XrDiagNowUs()) ? 1.0f:0.0f;
    case 2:return s_axis;
    case 3:return s_climber.Held(0) ? 1.0f:0.0f;
    case 4:return s_climber.Held(1) ? 1.0f:0.0f;
    case 5:return s_climber.Debt();
    case 6:return s_geometry.valid ? 1.0f:0.0f;
    case 7:return float(s_climber.Kind(0));
    case 8:return float(s_climber.Kind(1));
    case 9:return s_stamp ? float(XrDiagNowUs()-s_stamp)*.001f:-1;
    case 16:return float(s_geometry.topRailCount);
    case 17:return s_geometry.PhysicalTop();
    case 18:return s_climber.Finishing() ? 1.0f:0.0f;
    case 21:return cvr::swimming::ScriptHooksActive() ? 1.0f:0.0f;   // MoveY gets the axis natively (Natives/ActionReads.cpp)
    default:return -1;
    }
}
}
