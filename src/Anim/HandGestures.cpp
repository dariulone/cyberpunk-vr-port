#include "Anim/HandGestures.hpp"
#include "Anim/HandGesturePose.hpp"
#include "Anim/HandGestureCalibration.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/ReloadPose.hpp"
#include "Anim/Cyberware.hpp"
#include "Runtimes/FingerGrip.hpp"
#include <cmath>
#include "Core/VrCoreShared.hpp"
#include <atomic>
#include <cstring>

extern "C" int CyberpunkVR_CarryLeft;
extern char g_VRSmokeFingerName[32][48],g_VRSmokeFingerNameL[32][48];
extern "C" {
__declspec(dllexport) int CyberpunkVR_GestureMode[2]{};
__declspec(dllexport) float CyberpunkVR_GestureClosed[2][5]{};
__declspec(dllexport) float CyberpunkVR_GestureStraight[2][5]{};
// 1 = with finger-tracking gloves (xr_grip_from_fingers), each of V's fingers follows its own (xr_finger_bones).
__declspec(dllexport) int CyberpunkVR_FingerBones=1;
// With finger tracking, the open thumb: 1 = the rig's reference pose, like the other four fingers; 0 = the thumb laid
// flat along the index (ThumbReference, made for the button gestures) (xr_thumb_rig_open).
__declspec(dllexport) int CyberpunkVR_ThumbRigOpen=1;
// Finger spread from hand tracking (xr_finger_spread): 0 = off (fingers only curl), 1 = default, up to 2.
__declspec(dllexport) float CyberpunkVR_FingerSpread=1.0f;
}
namespace cvr::gestures {
namespace {
std::atomic<uint64_t> input{},equipment{};
struct State {Targets pose;CalibrationWeights calibration{};uint32_t epoch=~0u;uint64_t stamp{};};
State state[2];
uint64_t Byte(float v){return uint64_t(std::clamp(std::isfinite(v)?v:0.f,0.f,1.f)*255.f+.5f);}
void Blend(const float* a,const float* b,float t,float* out) {
    const float sign=(a[0]*b[0]+a[1]*b[1]+a[2]*b[2]+a[3]*b[3])<0?-1.f:1.f;
    for(int k=0;k<4;++k)out[k]=a[k]*(1-t)+b[k]*t*sign;
    VRIK_QuatNorm(out);
}
}
void ApplyCalibration(int side,const char* suffix,const CalibrationWeights& weights,float* rotation) {
    if(side<0 || side>1 || !suffix || !rotation)return;
    float delta[4]={0,0,0,1};bool changed=false;
    for(const auto& entry:CalibratedGestures) {
        const float weight=weights[entry.pose];
        if(weight<=0 || !(entry.hands&(1u<<side)) || std::strcmp(entry.suffix,suffix))continue;
        for(int i=0;i<4;++i)delta[i]+=weight*(entry.delta[i]-(i==3?1.f:0.f));
        changed=true;
    }
    if(!changed)return;
    VRIK_QuatNorm(delta);float result[4];VRIK_QuatMul(rotation,delta,result);VRIK_QuatNorm(result);
    std::copy_n(result,4,rotation);
}
void PublishInput(float lg,float rg,float lt,float rt,bool thumb,bool enabled) {
    const uint64_t now=GetTickCount64()&0x3fffffffull;
    input.store(Byte(lg)|(Byte(rg)<<8)|(Byte(lt)<<16)|(Byte(rt)<<24)|
        (uint64_t(thumb)<<32)|(uint64_t(enabled)<<33)|(now<<34),std::memory_order_release);
}
void PublishEquipment(int mask) {
    equipment.store((GetTickCount64()<<2)|uint64_t(mask&3),std::memory_order_release);
}
namespace {
// Writes one hand's digits: from the bone's current local towards straight, then towards the native fist, per
// digit group (thumb..little). Returns how many bones each digit got.
void PoseDigits(uint8_t* buffer,int side,const Targets& pose,const CalibrationWeights* calibration,int posed[5],bool thumbRigOpen=false) {
    const int count=side==0?g_VRSmokeFingerCountL:g_VRSmokeFingerCount;
    const int* indices=side==0?g_VRSmokeFingerIdxL:g_VRSmokeFingerIdx;
    const char (*names)[48]=side==0?g_VRSmokeFingerNameL:g_VRSmokeFingerName;
    const size_t prefix=side==0?4:5;
    for(int k=0;k<std::min(count,32);++k) {
        const int bone=indices[k];float reference[4];
        if(!VRIK_ReferenceFingerRotation(bone,reference) || std::strlen(names[k])<=prefix)continue;
        for(const auto& fist:NativeFist)if(std::strcmp(names[k]+prefix,fist.suffix)==0) {
            float inverse[4],delta[4],closed[4],rest[4],straight[4],straightGoal[4];
            auto* q=reinterpret_cast<float*>(buffer+bone*48+VRIK_ROT_OFF);
            std::copy_n(q,4,rest);
            VRIK_QuatConj(fist.reference[side],inverse);VRIK_QuatMul(fist.closed[side],inverse,delta);
            VRIK_QuatMul(delta,reference,closed);VRIK_QuatNorm(closed);
            std::copy_n(reference,4,straightGoal);
            if(!thumbRigOpen && std::strcmp(fist.suffix,"InHandThumb")==0)ThumbReference(side,straightGoal);
            Blend(rest,straightGoal,pose.straight[fist.group],straight);
            Blend(straight,closed,pose.closed[fist.group],q);
            ++posed[fist.group];
            break;
        }
        if(calibration)ApplyCalibration(side,names[k]+prefix,*calibration,reinterpret_cast<float*>(buffer+bone*48+VRIK_ROT_OFF));
    }
}
struct GloveState {Targets pose;float splay[4]{.5f,.5f,.5f,.5f};bool splayValid=false;uint32_t epoch=~0u;uint64_t stamp{};};
// FINGER SPREAD. Each pair of neighbouring digits has a separation, 0 touching .. 1 apart (fingergrip::TrackedSplay). The middle finger
// stays; the others turn sideways in the palm plane (SpreadAxis), relative to the rig's own pose at kSplayNeutral:
// index by its pair with the middle, ring by its pair, little by both pairs out from the middle, thumb by its pair
// with the index. Degrees per unit of splay, times xr_finger_spread; clamped.
constexpr float kSplayNeutral=.5f;
// Degrees per unit of splay, tuned in VR.
constexpr float kSplayDeg[5]={19.6f,14.7f,0.f,11.76f,11.76f};   // thumb, index, (middle), ring, little per pair
void SpreadDigits(uint8_t* buffer,int side,const float splay[4],float strength) {
    float degrees[5]{};
    degrees[0]=(splay[0]-kSplayNeutral)*kSplayDeg[0];
    degrees[1]=(splay[1]-kSplayNeutral)*kSplayDeg[1];
    degrees[3]=(splay[2]-kSplayNeutral)*kSplayDeg[3];
    degrees[4]=degrees[3]+(splay[3]-kSplayNeutral)*kSplayDeg[4];
    for(int digit:{0,1,3,4}) {
        const float limit=digit==0?40.f:30.f;
        const float angle=std::clamp(degrees[digit]*strength,-limit,limit)*0.01745329252f;
        int bone=-1;float axis[3];
        if(std::abs(angle)<1e-4f || !SpreadAxis(side,digit,&bone,axis))continue;
        const float h=std::sin(angle*.5f);
        const float turn[4]={axis[0]*h,axis[1]*h,axis[2]*h,std::cos(angle*.5f)};
        auto* q=reinterpret_cast<float*>(buffer+bone*48+VRIK_ROT_OFF);
        float out[4];VRIK_QuatMul(turn,q,out);VRIK_QuatNorm(out);std::copy_n(out,4,q);
    }
}
GloveState gloves[2];
}
void Apply(uint8_t* buffer) {
    CyberpunkVR_GestureMode[0]=CyberpunkVR_GestureMode[1]=0;
    if(!buffer || g_VRBind!=4 || g_isInVehicle)return;
    const auto controls=input.load(std::memory_order_acquire),held=equipment.load(std::memory_order_acquire);
    const auto now=GetTickCount64();
    if(!((controls>>33)&1) || ((now-(controls>>34))&0x3fffffffull)>250 ||
        !held || now<(held>>2) || now-(held>>2)>500)return;
    const int kind=cyberware::SelectedKind();
    const auto epoch=g_VrikFrameEpoch.load(std::memory_order_relaxed);
    float unused[5];
    for(int side=0;side<2;++side) {
        // Gloves drive this hand in ApplyGloves, after every other finger layer.
        if(CyberpunkVR_FingerBones!=0 && fingergrip::TrackedCurl(side,unused)){state[side]={};continue;}
        const bool owned=((held>>side)&1)!=0 || (side==0 && CyberpunkVR_CarryLeft!=0) ||
            g_VRReloadFingerActive[side] || (side==0?g_VRSmokeFingerActiveL:g_VRSmokeFingerActive);
        if(owned){state[side]={};CyberpunkVR_GestureMode[side]=0;continue;}
        const float grip=float((controls>>(side*8))&255)/255.f;
        const float trigger=float((controls>>(16+side*8))&255)/255.f;
        // R3 belongs only to the right hand. L3 remains the game's ability.
        const bool thumb=side==1 && ((controls>>32)&1)!=0,wire=kind==3;
        auto& s=state[side];
        if(s.epoch!=epoch) {
            const auto wanted=Evaluate(grip,trigger,thumb,wire,cyberware::WireDeployment(side));
            const auto calibration=CalibrationTargets(grip,trigger,thumb,wire);
            const float dt=s.stamp && now>=s.stamp?std::min(float(now-s.stamp)*.001f,.05f):.016f;
            const float blend=wire?1.f:1-std::exp(-dt/.04f);
            for(int digit=0;digit<5;++digit) {
                s.pose.closed[digit]+=(wanted.closed[digit]-s.pose.closed[digit])*blend;
                s.pose.straight[digit]+=(wanted.straight[digit]-s.pose.straight[digit])*blend;
                CyberpunkVR_GestureClosed[side][digit]=s.pose.closed[digit];
                CyberpunkVR_GestureStraight[side][digit]=s.pose.straight[digit];
            }
            for(size_t i=0;i<s.calibration.size();++i)s.calibration[i]+=(calibration[i]-s.calibration[i])*blend;
            s.stamp=now;s.epoch=epoch;
        }
        CyberpunkVR_GestureMode[side]=wire?6:(thumb && grip>.5f?5:(grip>.5f?(trigger>.5f?3:2):(trigger>.5f?4:1)));
        // Captured rest locals are the baseline, below reload/held-object layers.
        if(side==0)anim::VrikRestFingerPose(buffer,true);else anim::VrikRestFingerPoseRight(buffer,true);
        int posed[5]{};
        PoseDigits(buffer,side,s.pose,&s.calibration,posed);
    }
}
void ApplyGloves(uint8_t* buffer) {
    if(!buffer || g_VRBind!=4 || CyberpunkVR_FingerBones==0)return;
    const auto now=GetTickCount64();
    const auto epoch=g_VrikFrameEpoch.load(std::memory_order_relaxed);
    for(int side=0;side<2;++side) {
        float curl[5];
        if(!fingergrip::TrackedCurl(side,curl)){gloves[side]={};continue;}
        auto& s=gloves[side];
        if(s.epoch!=epoch) {
            const auto wanted=FromTracking(curl);
            const float dt=s.stamp && now>=s.stamp?std::min(float(now-s.stamp)*.001f,.05f):.016f;
            const float blend=s.stamp?1-std::exp(-dt/.04f):1.f;
            for(int digit=0;digit<5;++digit) {
                s.pose.closed[digit]+=(wanted.closed[digit]-s.pose.closed[digit])*blend;
                s.pose.straight[digit]+=(wanted.straight[digit]-s.pose.straight[digit])*blend;
            }
            s.stamp=now;s.epoch=epoch;
        }
        int posed[5]{};
        PoseDigits(buffer,side,s.pose,nullptr,posed,CyberpunkVR_ThumbRigOpen!=0);
        float splay[4];
        if(CyberpunkVR_FingerSpread>0.0f && fingergrip::TrackedSplay(side,splay)) {
            const float blend=s.splayValid?1-std::exp(-.016f/.06f):1.f;
            for(int k=0;k<4;++k)s.splay[k]+=(splay[k]-s.splay[k])*blend;
            s.splayValid=true;
            SpreadDigits(buffer,side,s.splay,std::min(CyberpunkVR_FingerSpread,2.0f));
        } else s.splayValid=false;
    }
}
bool OwnsHand(int side){return side>=0 && side<2 && CyberpunkVR_GestureMode[side]!=0;}
}
