#include "Anim/HandGestures.hpp"
#include "Anim/HandGesturePose.hpp"
#include "Anim/HandGestureCalibration.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/ReloadPose.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/WheelGrab.hpp"
#include "Core/VrCoreShared.hpp"
#include <atomic>
#include <chrono>
#include <cstring>

extern "C" int CyberpunkVR_CarryLeft;
extern char g_VRSmokeFingerName[32][48],g_VRSmokeFingerNameL[32][48];
extern "C" {
__declspec(dllexport) int CyberpunkVR_GestureMode[2]{};
__declspec(dllexport) float CyberpunkVR_GestureClosed[2][5]{};
__declspec(dllexport) float CyberpunkVR_GestureStraight[2][5]{};
}
namespace cvr::gestures {
namespace {
std::atomic<uint64_t> input{},equipment{},fingers[2]{};
struct State {Targets pose;CalibrationWeights calibration{};uint32_t epoch=~0u;uint64_t stamp{};};
// How much each finger is held back from closing, so that zero (the start, and a hand long unreported) means free.
std::atomic<float> contactHeld[2][5]{};
std::atomic<uint64_t> contactStamp[2]{};
std::atomic<uint64_t> contactAsked[2][5]{};
std::atomic<bool> contactForce[2][5]{};
std::atomic<float> contactBack[2][5]{};
std::atomic<uint64_t> contactBackAt[2][5]{};
std::atomic<float> gestureWanted[2][5]{};
float contactWaited[2][5]{};
constexpr uint64_t kContactFreshMs=400;
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
void PublishFingers(int side,const float* closed) {
    if(side<0 || side>1)return;
    if(!closed){fingers[side].store(0,std::memory_order_release);return;}
    uint64_t packed=(GetTickCount64()&0xffffffull)<<40;
    for(int digit=0;digit<5;++digit)packed|=Byte(closed[digit])<<(digit*8);
    fingers[side].store(packed,std::memory_order_release);
}
void PublishEquipment(int mask) {
    equipment.store((GetTickCount64()<<2)|uint64_t(mask&3),std::memory_order_release);
}
void FingerContact(int side,int digit,float limit,bool hold) {
    if(side<0 || side>1 || digit<0 || digit>4 || !std::isfinite(limit))return;
    limit=std::clamp(limit,0.f,1.f);
    const auto now=GetTickCount64();
    const auto asked=contactAsked[side][digit].exchange(now,std::memory_order_relaxed);
    const bool wasHeld=contactForce[side][digit].exchange(hold,std::memory_order_relaxed);
    // long unasked, the finger has been free; and letting go of a thing opens the hand at its own pace, not the surface's
    const bool free=!asked || now-asked>kContactFreshMs || (wasHeld && !hold);
    const float was=free?1.f:1-contactHeld[side][digit].load(std::memory_order_relaxed);
    const float next=hold?limit:StepFingerLimit(was,limit,std::min(float(now-asked)*.001f,.1f),contactWaited[side][digit]);
    contactHeld[side][digit].store(1-next,std::memory_order_relaxed);
    contactStamp[side].store(now,std::memory_order_relaxed);
}
void FingerBend(int side,int digit,float angle) {
    if(side<0 || side>1 || digit<1 || digit>4 || !std::isfinite(angle))return;
    // A real clock: the tick count moves in 16 ms steps, and the calls come 2 and 21 ms apart. It eases in and out, and the rate is capped, so a wall found late does not snap a finger back.
    constexpr float riseTau=.07f,fallTau=.18f,mostRate=7.f,most=1.5f;
    static thread_local std::chrono::steady_clock::time_point last[2][5];
    const auto clock=std::chrono::steady_clock::now();
    const float wanted=std::clamp(angle,0.f,most);
    const auto now=GetTickCount64();
    const auto asked=contactBackAt[side][digit].exchange(now,std::memory_order_relaxed);
    const bool fresh=asked && now-asked<=kContactFreshMs;
    const float was=fresh?contactBack[side][digit].load(std::memory_order_relaxed):0.f;
    const float dt=fresh?std::min(std::chrono::duration<float>(clock-last[side][digit]).count(),.1f):0.f;
    last[side][digit]=clock;
    const float pull=(wanted-was)*(1-std::exp(-dt/(wanted>was?riseTau:fallTau)));
    const float next=was+std::clamp(pull,-mostRate*dt,mostRate*dt);
    contactBack[side][digit].store(next,std::memory_order_relaxed);
}
float FingerClosed(int side,int digit) {
    return side<0 || side>1 || digit<0 || digit>4 ? 0.f:CyberpunkVR_GestureClosed[side][digit];
}
float FingerWanted(int side,int digit) {
    return side<0 || side>1 || digit<0 || digit>4 ? 0.f:gestureWanted[side][digit].load(std::memory_order_relaxed);
}
namespace {
bool MeasuredFingers(int side,uint64_t now,float* closed) {
    const auto packed=fingers[side].load(std::memory_order_acquire);
    if(!packed || ((now-(packed>>40))&0xffffffull)>250)return false;
    for(int digit=0;digit<5;++digit)closed[digit]=float((packed>>(digit*8))&255)/255.f;
    return true;
}
}
void Apply(uint8_t* buffer) {
    CyberpunkVR_GestureMode[0]=CyberpunkVR_GestureMode[1]=0;
    if(!buffer || g_VRBind!=4)return;
    const auto controls=input.load(std::memory_order_acquire),held=equipment.load(std::memory_order_acquire);
    const auto now=GetTickCount64();
    if(!((controls>>33)&1) || ((now-(controls>>34))&0x3fffffffull)>250 ||
        !held || now<(held>>2) || now-(held>>2)>500)return;
    const int kind=cyberware::SelectedKind();
    const auto epoch=g_VrikFrameEpoch.load(std::memory_order_relaxed);
    for(int side=0;side<2;++side) {
        const bool owned=((held>>side)&1)!=0 || PortHolds(side);
        if(owned){state[side]={};CyberpunkVR_GestureMode[side]=0;continue;}
        const float grip=float((controls>>(side*8))&255)/255.f;
        const float trigger=float((controls>>(16+side*8))&255)/255.f;
        const bool wire=kind==3;
        // The wire's catch is timed to its payout, whatever the fingers do.
        float measured[5];
        const bool tracked=!wire && MeasuredFingers(side,now,measured);
        // SEATED, a hand's fingers are its own while they are measured and it is not on the wheel. On the wheel they are the driving
        // animation's, and without a measurement WheelFingers gives it the relaxed hand it always had.
        if(g_isInVehicle && (!tracked || (side==0 ? anim::g_wheelBlendLeft:anim::g_wheelBlendRight).load(std::memory_order_relaxed)>.01f)) {
            state[side]={};CyberpunkVR_GestureMode[side]=0;continue;
        }
        // R3 belongs only to the right hand. L3 remains the game's ability. Measured, a thumb is simply up.
        const bool thumb=side==1 && !tracked && ((controls>>32)&1)!=0;
        auto& s=state[side];
        if(s.epoch!=epoch) {
            auto wanted=tracked?FromFingers(measured):Evaluate(grip,trigger,thumb,wire,cyberware::WireDeployment(side));
            for(int digit=0;digit<5;++digit)gestureWanted[side][digit].store(wanted.closed[digit],std::memory_order_relaxed);
            const auto contactAt=contactStamp[side].load(std::memory_order_relaxed);
            float mayClose[5]={1,1,1,1,1};
            if(!wire && contactAt && now>=contactAt && now-contactAt<=kContactFreshMs)
                for(int digit=0;digit<5;++digit) {
                    mayClose[digit]=1-contactHeld[side][digit].load(std::memory_order_relaxed);
                    wanted.closed[digit]=contactForce[side][digit].load(std::memory_order_relaxed)?mayClose[digit]:
                        std::min(wanted.closed[digit],mayClose[digit]);
                }
            const auto calibration=tracked?FingerCalibration(measured):CalibrationTargets(grip,trigger,thumb,wire);
            const float dt=s.stamp && now>=s.stamp?std::min(float(now-s.stamp)*.001f,.05f):.016f;
            const float blend=wire?1.f:1-std::exp(-dt/.04f);
            for(int digit=0;digit<5;++digit) {
                s.pose.closed[digit]=tracked?FollowFinger(s.pose.closed[digit],wanted.closed[digit],blend,digit,dt):
                    s.pose.closed[digit]+(wanted.closed[digit]-s.pose.closed[digit])*blend;
                // a surface moves a finger now, without the pace of a hand opening
                s.pose.closed[digit]=std::min(s.pose.closed[digit],mayClose[digit]);
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
        const int count=side==0?g_VRSmokeFingerCountL:g_VRSmokeFingerCount;
        const int* indices=side==0?g_VRSmokeFingerIdxL:g_VRSmokeFingerIdx;
        const char (*names)[48]=side==0?g_VRSmokeFingerNameL:g_VRSmokeFingerName;
        const size_t prefix=side==0?4:5;
        for(int k=0;k<std::min(count,32);++k) {
            const int bone=indices[k];float reference[4];
            if(!VRIK_ReferenceFingerRotation(bone,reference) || std::strlen(names[k])<=prefix)continue;
            for(const auto& pose:NativeFist)if(std::strcmp(names[k]+prefix,pose.suffix)==0) {
                float inverse[4],delta[4],closed[4],rest[4],straight[4],straightGoal[4];
                auto* q=reinterpret_cast<float*>(buffer+bone*48+VRIK_ROT_OFF);
                std::copy_n(q,4,rest);
                VRIK_QuatConj(pose.reference[side],inverse);VRIK_QuatMul(pose.closed[side],inverse,delta);
                VRIK_QuatMul(delta,reference,closed);VRIK_QuatNorm(closed);
                std::copy_n(reference,4,straightGoal);
                if(std::strcmp(pose.suffix,"InHandThumb")==0)ThumbReference(side,straightGoal);
                Blend(rest,straightGoal,s.pose.straight[pose.group],straight);
                Blend(straight,closed,s.pose.closed[pose.group],q);
                // PRESSED BACK, at the first joint and against the way this joint closes. Its closing is a turn on the parent's side, so this is too.
                if(pose.group>0 && !std::strncmp(pose.suffix,"Hand",4) && pose.suffix[std::strlen(pose.suffix)-1]=='1') {
                    const auto backAt=contactBackAt[side][pose.group].load(std::memory_order_relaxed);
                    const float back=backAt && now-backAt<=kContactFreshMs?contactBack[side][pose.group].load(std::memory_order_relaxed):0.f;
                    const float length=std::sqrt(delta[0]*delta[0]+delta[1]*delta[1]+delta[2]*delta[2]);
                    if(back>.001f && length>1e-4f) {
                        const float sign=delta[3]<0?1.f:-1.f;   // the short way round is the way it closes
                        const float half=back*.5f,sine=std::sin(half)*sign/length;
                        const float turn[4]={delta[0]*sine,delta[1]*sine,delta[2]*sine,std::cos(half)};
                        float result[4];
                        VRIK_QuatMul(turn,q,result);VRIK_QuatNorm(result);
                        std::copy_n(result,4,q);
                    }
                }
                break;
            }
            ApplyCalibration(side,names[k]+prefix,s.calibration,reinterpret_cast<float*>(buffer+bone*48+VRIK_ROT_OFF));
        }
    }
}
bool OwnsHand(int side){return side>=0 && side<2 && CyberpunkVR_GestureMode[side]!=0;}
bool PortHolds(int side) {
    if(side<0 || side>1)return false;
    return (side==0 && CyberpunkVR_CarryLeft!=0) || g_VRReloadFingerActive[side] ||
        (side==0?g_VRSmokeFingerActiveL:g_VRSmokeFingerActive);
}
}
