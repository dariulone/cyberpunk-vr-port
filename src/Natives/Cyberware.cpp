#include "Natives/NativeFunctions.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/HandGestures.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/WeaponTrigger.hpp"
#include "Anim/LeftHanded.hpp"
#include "Core/LiveControls.hpp"
#include "Hooks/CyberwareChord.hpp"
#include <RED4ext/Scripting/Utils.hpp>

extern "C" int CyberpunkVR_TwoHandActive;
extern "C" int CyberpunkVR_CarryLeft;
namespace {
// weaponLast: the hands in the melee order -- the other hand, then the weapon's (the left one while the weapon is in
// the left hand, Anim/LeftHanded.cpp); otherwise left then right, as the implants' frames have them.
void AppendMeleeTracking(RED4ext::DynArray<RED4ext::Vector4>* out,bool header,bool weaponLast=false) {
    // Append actual tracking-space motion, before any game camera/body pose,
    // wrist offset or IK. A native hit reaction/aim turn must not become another
    // punch merely because the rendered fist moved in the game's world.
    OpenXRHeadPose head{},hands[2]{};uint64_t sequence{},stamp{};
    const bool valid=OpenXRManager::Get().GetGestureHandFrame(&head,hands,&sequence,&stamp,header);
    // Time belongs to this tracking sample, not the later CET read. Adjacent
    // reads can straddle publication only a millisecond apart even though the
    // two poses span a full XR frame; read time would invent a fast swing.
    if(header)out->PushBack({float((stamp/1000u)&0xffffffu),float(sequence&0xffffffu),CyberpunkVR_TwoHandActive?1.f:0.f,0.f});
    const float headQ[4]={head.oriX,head.oriY,head.oriZ,head.oriW};
    RED4ext::Vector4 tracking[2]{},rotations[2]{};
    for(int side=0;side<2;++side)if(valid && hands[side].valid) {
        const auto& hand=hands[side];const float local[3]={hand.posX,hand.posY,hand.posZ};
        float p[3],q[4];VRIK_QuatRotateVec(headQ,local,p);
        const float localQ[4]={hand.oriX,hand.oriY,hand.oriZ,hand.oriW};VRIK_QuatMul(headQ,localQ,q);VRIK_QuatNorm(q);
        tracking[side]={p[0]+head.posX,p[1]+head.posY,p[2]+head.posZ,float((head.originSerial&0xffffffu)+1u)};
        rotations[side]={q[0],q[1],q[2],q[3]};
    }
    const int first=weaponLast && cvr::anim::WeaponInLeftHand() ? 1 : 0;
    out->PushBack(tracking[first]);out->PushBack(tracking[1-first]);
    out->PushBack(rotations[first]);out->PushBack(rotations[1-first]);
}
}
void VRMeleeInputFrame(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,RED4ext::DynArray<RED4ext::Vector4>* out,int64_t) {
    stack->code++;if(!out)return;out->Clear();
    if(g_VRBind!=4 || cvr::anim::IsHeadAimWeaponActive())return;
    AppendMeleeTracking(out,true,true);
}
void VRMeleeThrowEnabled(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    stack->code++;if(out)*out=g_VRBind==4 && !cvr::anim::IsHeadAimWeaponActive();
}
void VRWeaponTriggerEnabled(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    stack->code++;if(out)*out=cvr::input::WeaponTriggerEnabled();
}
// The weapon in hand is drawn in the LEFT hand (Anim/LeftHanded.cpp): the scripts' support hand is the right one.
void VRWeaponInLeftHand(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    stack->code++;if(out)*out=cvr::anim::WeaponInLeftHand();
}
// The left-handed controls are on (xr_left_handed): the hand that draws and holsters is the left one, weapon or not.
void VRLeftHanded(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    stack->code++;if(out)*out=g_liveControls.xrLeftHanded!=0;
}
void VRMeleeThrowFrame(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,RED4ext::DynArray<RED4ext::Vector4>* out,int64_t) {
    stack->code++;if(!out)return;out->Clear();
    if(g_VRBind!=4 || g_menuModeValue!=0 || g_isInVehicle || CyberpunkVR_CarryLeft || cvr::anim::IsHeadAimWeaponActive())return;
    OpenXRHeadPose head{},hands[2]{};uint64_t sequence{},stamp{};VRControllerState input{};
    float p[3],q[4],yaw{};
    auto& xr=OpenXRManager::Get();
    // The hand holding the knife throws it: the left one while the weapon is in the left hand.
    const bool left=cvr::anim::WeaponInLeftHand();
    const int hand=left ? 0 : 1;
    if(!xr.GetGestureHandFrame(&head,hands,&sequence,&stamp,true) || !hands[hand].valid ||
       !xr.GetControllerState(&input) || !(left ? input.leftHandValid : input.rightHandValid) ||
       !VRIK_ReadCurrentBodyFrame(p,q,&yaw))return;
    const float headQ[4]={head.oriX,head.oriY,head.oriZ,head.oriW};
    const float local[3]={hands[hand].posX,hands[hand].posY,hands[hand].posZ};float tracking[3];
    VRIK_QuatRotateVec(headQ,local,tracking);
    // Measure in metres in tracking space. Map the resulting velocity to game
    // axes only at release, so camera/body turns never create throwing energy.
    out->PushBack({float((stamp/1000u)&0xffffffu),float(sequence&0xffffffu),float(head.originSerial&0xffffffu),0});
    out->PushBack({tracking[0]+head.posX,tracking[1]+head.posY,tracking[2]+head.posZ,1});
    // the other hand's trigger and the throwing hand's grip
    out->PushBack({std::cos(yaw),std::sin(yaw),left ? input.rightTrigger : input.leftTrigger,
                   left ? input.leftGrip : input.rightGrip});
    const float handQ[4]={hands[hand].oriX,hands[hand].oriY,hands[hand].oriZ,hands[hand].oriW};
    float fullQ[4],forward[3],gaze[3];const float ray[3]={0,0,-1};
    VRIK_QuatMul(headQ,handQ,fullQ);VRIK_QuatRotateVec(fullQ,ray,forward);
    out->PushBack({forward[0],forward[1],forward[2],0});
    // Where the head looks and where it looks from, in the same tracking space: the throw aims
    // mostly along it.
    VRIK_QuatRotateVec(headQ,ray,gaze);
    out->PushBack({gaze[0],gaze[1],gaze[2],0});
    out->PushBack({head.posX,head.posY,head.posZ,1});
}
void VRMeleeRealTime(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,float* out,int64_t) {
    stack->code++;static const uint64_t start=GetTickCount64();
    if(out)*out=float(GetTickCount64()-start)*.001f;
}
void VRMeleeTwoHandedActive(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    stack->code++;if(out)*out=g_VRBind==4 && CyberpunkVR_TwoHandActive!=0 && !cvr::anim::IsHeadAimWeaponActive();
}

void VRCyberwareFrame(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,RED4ext::DynArray<RED4ext::Vector4>* out,int64_t) {
    stack->code++;if(!out)return;out->Clear();
    cvr::cyberware::Frame frame;if(!cvr::cyberware::ReadFrame(frame))return;
    // Header, body origin, two raw wrists, two raw rotations, then both chains.
    out->PushBack({float(frame.family),float(frame.count[0]),float(frame.count[1]),float(frame.epoch)});
    // W is a modulo-2^24 millisecond clock (exact in Float), not game time.
    // Physical swing speed must not multiply under Sandevistan/time dilation.
    out->PushBack({frame.bodyOrigin.x,frame.bodyOrigin.y,frame.bodyOrigin.z,float(frame.stamp&0xffffffu)});
    for(const auto p:frame.wrists)out->PushBack({p.x,p.y,p.z,1});
    for(const auto& q:frame.handRotation)out->PushBack({q[0],q[1],q[2],q[3]});
    for(int side=0;side<2;++side)for(int i=0;i<frame.count[side];++i) {
        const auto p=frame.points[side][i];out->PushBack({p.x,p.y,p.z,1});
    }
    AppendMeleeTracking(out,false);
}
void VRCyberwarePoint(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,RED4ext::Vector4* out,int64_t) {
    int32_t side=0,index=0;RED4ext::GetParameter(stack,&side);RED4ext::GetParameter(stack,&index);stack->code++;
    if(!out)return;*out={};cvr::cyberware::Frame frame;
    if(side<0 || side>1 || !cvr::cyberware::ReadFrame(frame) || index<0 || index>=frame.count[side])return;
    const auto p=frame.points[side][index];*out={p.x,p.y,p.z,1};
}
void VRCyberwareWirePlanes(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,void*,int64_t) {
    int32_t side{},epoch{};RED4ext::DynArray<RED4ext::Vector4> values;
    RED4ext::GetParameter(stack,&side);RED4ext::GetParameter(stack,&epoch);RED4ext::GetParameter(stack,&values);stack->code++;
    std::array<cvr::cyberware::Plane,cvr::cyberware::Wire::Count> planes{};
    if(values.size()!=2*cvr::cyberware::Wire::Count)return;
    for(int i=0;i<cvr::cyberware::Wire::Count;++i) {
        const auto p=values[i*2],n=values[i*2+1];
        auto& plane=planes[i];plane.position={p.X,p.Y,p.Z};plane.normal={n.X,n.Y,n.Z};
        const float len=cvr::cyberware::Length(plane.normal);
        plane.valid=p.W>.5f&&cvr::cyberware::Finite(plane.position)&&std::isfinite(len)&&len>.5f&&len<1.5f;
        if(plane.valid)plane.normal=plane.normal*(1/len);
    }
    cvr::cyberware::SetWirePlanes(side,static_cast<uint32_t>(epoch),planes);
}
void VRCyberwareMusclePulse(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,void*,int64_t) {
    int32_t side{};float duration{};
    RED4ext::GetParameter(stack,&side);RED4ext::GetParameter(stack,&duration);stack->code++;
    cvr::cyberware::MusclePulse(side,duration);
}
void VRCyberwareSelect(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,void*,int64_t) {
    int32_t kind{};RED4ext::GetParameter(stack,&kind);stack->code++;cvr::cyberware::Select(kind);
}
void VRHandEquipment(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,void*,int64_t) {
    int32_t mask{};RED4ext::GetParameter(stack,&mask);stack->code++;
    cvr::gestures::PublishEquipment(mask);
}
void VRCyberwareChordTake(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    stack->code++;
    const bool taken=cvr::input::TakeCyberwareChord();
    if(out)*out=taken;
}
void VRLauncherControllerAim(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    stack->code++;
    if(out)*out=g_VRBind==4 && !cvr::anim::IsHeadAimWeaponActive();
}
void VRCyberwareCollisionInfo(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,RED4ext::Vector4* out,int64_t) {
    stack->code++;if(!out)return;float values[4]{};cvr::cyberware::CollisionInfo(values);*out={values[0],values[1],values[2],values[3]};
}
