#include "Anim/NpcJointLimits.hpp"
#include "Anim/VrikHook.hpp"
LONG TestGateExchange(volatile LONG*,LONG);
#undef InterlockedExchange
#define InterlockedExchange TestGateExchange
#include NPC_GRIP_SOURCE
#undef InterlockedExchange
#include <array>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <nlohmann/json.hpp>
#include <stdexcept>
#include <string>
#include <vector>

namespace jl=cvr::anim::jointlimits;
void VRIK_QuatConj(const float* q,float* o){jl::Conjugate(q,o);}
void VRIK_QuatMul(const float* a,const float* b,float* o){jl::Multiply(a,b,o);}
void VRIK_QuatNorm(float* q){if(!jl::Normalize(q))throw std::runtime_error("non-finite or zero quaternion");}
void VRIK_QuatRotateVec(const float* q,const float* v,float* o){jl::Rotate(q,v,o);}
void Log(const char*,...){}
RED4ext::anim::AnimatedObject* FindAnimatedObjectOf(RED4ext::ent::Entity*){return nullptr;}
namespace cvr::physhands {
bool Tracing(){return false;}
void BeginPhysicsPoseFrame(){}
void EndPhysicsPoseFrame(){}
bool ReadGripControl(int,GripControl&){return false;}
bool ReadPersonContact(int,PersonContact&){return false;}
bool ReadPersonAttachment(int,PersonAttachment&){return false;}
void PublishPersonAttachment(int,const PersonAttachment&){}
}

using namespace cvr::anim;
bool gateSchedule=false,gateVisited=false,gateWasLocked=false;
LONG TestGateExchange(volatile LONG* destination,LONG value){
    if(gateSchedule&&destination==&anyOn&&value==0){
        gateSchedule=false;gateVisited=true;
        const bool acquired=TryAcquireSRWLockExclusive(&lock)!=0;gateWasLocked=!acquired;
        if(acquired){
            // Deterministic competing Begin at the old check/clear window.
            grips[0].on=true;_InterlockedExchange(&anyOn,1);ReleaseSRWLockExclusive(&lock);
        }
    }
    return _InterlockedExchange(destination,value);
}
namespace {
struct Bone {float p[4],q[4],s[4];};static_assert(sizeof(Bone)==48);
void Check(bool ok,const std::string& text){if(!ok)throw std::runtime_error(text);}
void Near(float a,float b,float e,const char* why){Check(std::isfinite(a)&&std::abs(a-b)<e,why);}
struct Rig {
    std::vector<Bone> pose;std::vector<int> parents;std::vector<std::string> names;
    explicit Rig(const char* name){
        std::ifstream f(std::filesystem::path(NPC_FIXTURES)/(std::string(name)+".json"));nlohmann::json doc;f>>doc;
        for(const auto& row:doc.at("bones")){
            Bone b{};names.push_back(row.at("name"));parents.push_back(row.at("parent"));
            for(int k=0;k<4;++k){b.p[k]=row.at("p").at(k);b.q[k]=row.at("q").at(k);b.s[k]=row.at("s").at(k);}jl::Normalize(b.q);pose.push_back(b);
        }
    }
    int find(const char* name)const{for(size_t i=0;i<names.size();++i)if(names[i]==name)return int(i);throw std::runtime_error(name);}
    Grip grip(int part)const{
        const char* namesByPart[4][3]{{"LeftArm","LeftForeArm","LeftHand"},{"RightArm","RightForeArm","RightHand"},
            {"LeftUpLeg","LeftLeg","LeftFoot"},{"RightUpLeg","RightLeg","RightFoot"}};
        Grip g{};g.part=part;g.bones=uint32_t(pose.size());for(size_t i=0;i<parents.size();++i)g.parents[i]=int16_t(parents[i]);
        g.upper=find(namesByPart[part][0]);g.mid=find(namesByPart[part][1]);g.end=find(namesByPart[part][2]);
        g.chest=-1;g.turn[3]=1;g.seeded=true;
#ifdef NPC_TEST_LEGACY
        g.watch=-1;
#endif
#ifndef NPC_TEST_LEGACY
        std::copy_n(pose[g.upper].q,4,g.upperRest);std::copy_n(pose[g.end].q,4,g.endRest);
        g.bounded=jl::Configure(g.hinge,pose[g.mid].q,pose[g.mid].p,pose[g.end].p,part>=2);Check(g.bounded,"reference hinge not recognized");
#endif
        return g;
    }
};
Tr Model(const Grip& g,std::vector<Bone>& pose,int bone){Tr result{};Check(ModelOf(g,reinterpret_cast<uint8_t*>(pose.data()),bone,result),"FK failed");return result;}
void ConeInside(const float* reference,const float* local,const float* axis,jl::Cone cone,const char* label){
    float limited[4];Check(jl::Clamp(reference,local,axis,cone,limited),"invalid constrained joint");
    float dot=0;for(int k=0;k<4;++k)dot+=limited[k]*local[k];Check(std::abs(dot)>1-2e-5f,label);
}
size_t Exercise(const Rig& rig){
    size_t count=0;
    const std::array<std::array<float,3>,14> directions{{{1,0,0},{-1,0,0},{0,1,0},{0,-1,0},{0,0,1},{0,0,-1},
        {1,1,1},{-1,1,1},{1,-1,1},{-1,-1,-1},{.00001f,0,1},{0,.00001f,-1},{1,0,.00001f},{-1,0,-.00001f}}};
    for(int part=0;part<4;++part)for(float worldTurn:{0.f,1.37f})for(const auto& direction:directions)for(float distance:{.03f,.15f,.4f,.7f,2.f})for(float handTurn:{-2.9f,0.f,2.9f}){
        auto g=rig.grip(part);auto pose=rig.pose;
        const float rootAxis[3]{0,0,1};jl::AxisAngle(rootAxis,worldTurn,pose[0].q);
        const auto before=pose;auto up=Model(g,pose,g.upper),mid=Model(g,pose,g.mid),end=Model(g,pose,g.end);
        const float length1=Apart(up.p,mid.p),length2=Apart(mid.p,end.p);
        float d[3]{direction[0],direction[1],direction[2]},target[3];jl::Unit(d);jl::Rotate(pose[0].q,d,d);
        for(int k=0;k<3;++k)target[k]=up.p[k]+d[k]*distance;
        const float turnAxis[3]{.577350269f,.577350269f,.577350269f};jl::AxisAngle(turnAxis,handTurn,g.turn);
        Touched touched;SolveLimb(g,reinterpret_cast<uint8_t*>(pose.data()),target,touched);
        up=Model(g,pose,g.upper);mid=Model(g,pose,g.mid);end=Model(g,pose,g.end);
        Near(Apart(up.p,mid.p),length1,3e-5f,"upper bone length changed");Near(Apart(mid.p,end.p),length2,3e-5f,"lower bone length changed");
        Near(g.miss,Apart(target,end.p),3e-5f,"reported grip miss ignores joint limits");
        float a[3],b[3],hinge[3],cross[3];for(int k=0;k<3;++k){a[k]=mid.p[k]-up.p[k];b[k]=end.p[k]-mid.p[k];}jl::Unit(a);jl::Unit(b);
        const float z[3]{0,0,1};float localAxis[3];jl::Rotate(rig.pose[g.mid].q,z,localAxis);jl::Rotate(up.q,localAxis,hinge);jl::Cross(a,b,cross);
        const float flex=std::atan2(jl::Dot(cross,hinge),jl::Dot(a,b))*(part>=2?1:-1);
        Check(flex>=1.99f*jl::Deg&&flex<=(part>=2?150.01f:145.01f)*jl::Deg,"elbow/knee folds backward or beyond its flexion envelope");
        Check(std::abs(jl::Dot(b,hinge))<.0005f,"elbow/knee bends sideways");
        ConeInside(rig.pose[g.upper].q,pose[g.upper].q,rig.pose[g.mid].p,part>=2?jl::Hip:jl::Shoulder,"shoulder/hip escaped its cone");
        ConeInside(rig.pose[g.end].q,pose[g.end].q,rig.pose[g.end].p,part>=2?jl::Ankle:jl::Wrist,"wrist/ankle escaped its local cone");
        for(size_t i=0;i<pose.size();++i){
            Check(std::memcmp(pose[i].p,before[i].p,sizeof(pose[i].p))==0&&std::memcmp(pose[i].s,before[i].s,sizeof(pose[i].s))==0,"IK translated or scaled a bone");
            if(i!=size_t(g.upper)&&i!=size_t(g.mid)&&i!=size_t(g.end))Check(std::memcmp(pose[i].q,before[i].q,sizeof(pose[i].q))==0,"IK wrote an unrelated bone");
            float norm=0;for(float v:pose[i].q){Check(std::isfinite(v),"non-finite pose");norm+=v*v;}Near(norm,1,1e-4f,"non-unit pose");
        }
        ++count;
    }
    return count;
}
void Boundaries(){
    const float x[3]{1,0,0},opposite[3]{-1,0,0};float q[4],out[3];Check(jl::Arc(x,opposite,q),"opposite arc rejected");jl::Rotate(q,x,out);Near(out[0],-1,1e-6f,"opposite direction silently ignored");
    const float id[4]{0,0,0,1},axis[3]{0,0,1};jl::AxisAngle(axis,1.5f,q);float limited[4];BlendInto(limited,id,q,.8f/1.5f);
    Near(2*std::atan2(std::abs(limited[2]),std::abs(limited[3])),.8f,1e-5f,"blend exceeded its angular bound");
    cvr::physhands::GripControl control{};control.shoulder[0]=100;control.shoulder[1]=-200;control.reach=.6f;
    float wanted[3]{101,-200,0};Check(cvr::physhands::LimitGripReach(control,wanted),"reachable control rejected");
    Near(wanted[0],100.6f,1e-5f,"bone goal escaped the player arm reach");
    wanted[0]=100.2f;Check(cvr::physhands::LimitGripReach(control,wanted),"inside reach rejected");Near(wanted[0],100.2f,1e-5f,"inside reach goal moved");
    control.reach=0;Check(!cvr::physhands::LimitGripReach(control,wanted),"unknown reach accepted");
}
void GatePublication(){
    for(auto& g:grips)g.on=false;
    _InterlockedExchange(&anyOn,1);gateSchedule=true;gateVisited=false;gateWasLocked=false;
    void* args[8]{};NpcArmGripApply(args);
    Check(gateVisited,"empty-grip publication path was not exercised");
    Check(gateWasLocked&&(!grips[0].on||anyOn),"new grab lost its active gate after the pose lock was released");
    for(auto& g:grips)g.on=false;_InterlockedExchange(&anyOn,0);
}
void NewPoseWithSameWrist(const Rig& rig){
    auto g=rig.grip(ArmR);auto pose=rig.pose;auto end=Model(g,pose,g.end);
    for(int k=0;k<3;++k)g.target[k]=end.p[k];g.target[1]+=.1f;
    auto* buf=reinterpret_cast<uint8_t*>(pose.data());
    Check(SolveOnce(g,buf,1),"first pose skipped");const auto solved=pose;
    Check(!SolveOnce(g,buf,1),"same pose solved twice");
    Check(std::memcmp(pose.data(),solved.data(),pose.size()*sizeof(Bone))==0,"duplicate pass accumulated rotation");
    // A new game animation can change ancestors while retaining its wrist's
    // local quaternion. The old one-bone cache incorrectly skipped this pose.
    const float axis[3]{0,1,0};float q[4];jl::AxisAngle(axis,.1f,q);jl::Multiply(q,pose[g.upper].q,pose[g.upper].q);
    Check(SolveOnce(g,buf,1),"new shoulder pose with the same wrist was skipped");
    Check(!SolveOnce(g,buf,1),"new pose not cached after solving");
}
void StraightLimbContinuity(const Rig& rig){
    for(int part=0;part<4;++part){
        auto g=rig.grip(part);auto base=rig.pose;const auto up=Model(g,base,g.upper),end=Model(g,base,g.end);
        float direction[3],hinge[3],basis[3];for(int k=0;k<3;++k)direction[k]=end.p[k]-up.p[k];
        const float distance=std::sqrt(jl::Dot(direction,direction))*.7f;jl::Unit(direction);
        const float z[3]{0,0,1};jl::Rotate(up.q,z,hinge);jl::Cross(hinge,direction,basis);jl::Unit(basis);
        float previous[4]{};bool have=false;
        for(int step=-100;step<=100;++step){
            auto pose=base;float target[3];for(int k=0;k<3;++k)target[k]=up.p[k]+direction[k]*distance+basis[k]*step*.0001f;
            Touched touched;SolveLimb(g,reinterpret_cast<uint8_t*>(pose.data()),target,touched);
            if(have){float dot=0;for(int k=0;k<4;++k)dot+=previous[k]*pose[g.upper].q[k];
                Check(2*std::acos(std::min(1.f,std::abs(dot)))<.02f,"straight limb flips its bend plane across a tiny target movement");}
            std::copy_n(pose[g.upper].q,4,previous);have=true;
        }
    }
}
void MovingTargetOnCachedPose(const Rig& rig){
    auto g=rig.grip(ArmR);auto pose=rig.pose;const auto end=Model(g,pose,g.end);
    for(int k=0;k<3;++k)g.target[k]=end.p[k];g.target[1]+=.1f;
    Check(SolveOnce(g,reinterpret_cast<uint8_t*>(pose.data()),1),"initial cached pose missing");
    for(float weight:{1.f,1.f,.6f,.2f}){
        g.target[2]+=.025f;
        auto expected=rig.pose;auto expectedGrip=g;Solve(expectedGrip,reinterpret_cast<uint8_t*>(expected.data()),weight);
        Check(SolveOnce(g,reinterpret_cast<uint8_t*>(pose.data()),weight),"moving controller was ignored until the NPC animated again");
        for(int bone:{g.upper,g.mid,g.end}){float dot=0;for(int k=0;k<4;++k)dot+=pose[bone].q[k]*expected[bone].q[k];
            Check(std::abs(dot)>1-2e-5f,"cached pose reapplied IK to its own output instead of the native animation");}
        auto copied=pose;
        Check(!SolveOnce(g,reinterpret_cast<uint8_t*>(copied.data()),weight),"copied solved pose accumulated another IK pass");
        Check(std::memcmp(pose.data(),copied.data(),pose.size()*sizeof(Bone))==0,"copied pose changed without a new command");
    }
}
void TwoHandsOneActor(const Rig& rig){
    auto pose=rig.pose;
    for(int side=0;side<2;++side){
        grips[side]=rig.grip(side);auto& g=grips[side];g.on=true;g.trackA=77;
        g.begun=GetTickCount64()-1000;g.asked=GetTickCount64();
        const char* names[]{"Spine","Spine1","Spine2","Spine3",side?"RightShoulder":"LeftShoulder"};
        const float share[]{.08f,.12f,.18f,.25f,.6f},limit[]{.08f,.12f,.15f,.2f,.45f};
        for(int j=0;j<5;++j)g.joints[g.jointCount++]={rig.find(names[j]),share[j],limit[j]};
        const auto end=Model(g,pose,g.end);for(int k=0;k<3;++k)g.target[k]=end.p[k];g.target[1]+=1;g.target[2]+=.5f;
    }
    struct Descriptor{uint8_t* buffer;uint32_t capacity,count;uintptr_t other,track;};
    Descriptor descriptor{reinterpret_cast<uint8_t*>(pose.data()),uint32_t(pose.size()),uint32_t(pose.size()),0,77};
    for(auto& g:grips)g.poseOwner=reinterpret_cast<uintptr_t>(&descriptor);
    void* args[8]{};args[7]=&descriptor;ApplyInner(args);const auto solved=pose;
    for(int repeat=0;repeat<6;++repeat){for(auto& g:grips)g.asked=GetTickCount64();ApplyInner(args);
        Check(std::memcmp(pose.data(),solved.data(),pose.size()*sizeof(Bone))==0,"two hands accumulated spine rotation on the same animation pose");}
    for(auto& g:grips){g.target[0]+=.07f;g.asked=GetTickCount64();}
    auto expected=rig.pose;
    for(auto& g:grips){auto copy=g;Solve(copy,reinterpret_cast<uint8_t*>(expected.data()),1);}
    ApplyInner(args);
    Check(std::memcmp(pose.data(),expected.data(),pose.size()*sizeof(Bone))==0,"two moving hands did not replay the same native pose");
    grips[0].letting=true;grips[0].let=GetTickCount64()-300;grips[1].asked=GetTickCount64();
    expected=rig.pose;auto remaining=grips[1];Solve(remaining,reinterpret_cast<uint8_t*>(expected.data()),1);
    ApplyInner(args);Check(!grips[0].on&&grips[1].on,"one-hand release disabled the wrong grip");
    Check(std::memcmp(pose.data(),expected.data(),pose.size()*sizeof(Bone))==0,"one-hand release left a stale spine or limb correction");
    for(auto& g:grips)g.on=false;
}
void ContinuousTwistStops(){
    const float reference[4]{0,0,0,1},axis[3]{1,0,0};float q[4],a[4],b[4];
    jl::AxisAngle(axis,179.99f*jl::Deg,q);jl::Clamp(reference,q,axis,jl::Shoulder,a);
    jl::AxisAngle(axis,180.01f*jl::Deg,q);jl::Clamp(reference,q,axis,jl::Shoulder,b);
    const float oldJump=AngleBetween(a,b)/jl::Deg;Check(oldJump>150,"stateless twist seam did not reproduce the old jump");
    size_t samples=0;
    for(float direction:{-1.f,1.f})for(const auto cone:{jl::Shoulder,jl::Hip,jl::Wrist,jl::Ankle}){
        jl::TwistHistory history;float previous[4]{0,0,0,1};
        for(int step=0;step<=1080;++step){
            const float requested=direction*float(step<=540?step:1080-step)*jl::Deg;
            jl::AxisAngle(axis,requested,q);if(step&1)for(float& v:q)v=-v;
            float raw;Check(jl::ReferenceTwist(reference,q,axis,raw),"invalid requested twist");history.Observe(raw);
            Check(jl::Clamp(reference,q,axis,cone,a,&history.angle),"continuous clamp failed");
            Near(jl::Twist(a,axis),std::clamp(requested,-cone.twist,cone.twist),.00002f,"limited twist crossed to the opposite stop");
            Check(AngleBetween(previous,a)<1.05f*jl::Deg,"one-degree input caused a discontinuous cone output");std::copy_n(a,4,previous);
            jl::Hinge hinge;hinge.twist=cone.twist;float bend[4],inverse[4],relative[4];jl::AxisAngle(hinge.axis,-1.f,bend);jl::Conjugate(bend,inverse);
            jl::Pose(hinge,1.f,q,b,&history.angle);jl::Multiply(inverse,b,relative);
            Near(jl::Twist(relative,axis),std::clamp(requested,-cone.twist,cone.twist),.00002f,"forearm pronation jumped across its twist stop");++samples;
        }
    }
    std::cout<<samples<<" continuous twist samples; old seam jump "<<oldJump<<" degrees\n";
}
void SharedRigIsolation(const Rig& rig){
    for(auto& p:nativePoses)p=NativePose{};for(auto& g:grips)g=Grip{};
    auto held=rig.pose,neighbor=rig.pose;auto& g=grips[0];g=rig.grip(ArmR);g.on=true;g.side=0;g.trackA=171;g.trackB=172;
    g.begun=GetTickCount64()-1000;g.asked=GetTickCount64();
    const auto end=Model(g,held,g.end);std::copy_n(end.p,3,g.target);g.target[1]+=.25f;g.target[2]+=.1f;
    struct Descriptor{uint8_t* buffer;uint32_t capacity,count;uintptr_t other,track;};
    Descriptor target{reinterpret_cast<uint8_t*>(held.data()),uint32_t(held.size()),uint32_t(held.size()),0,172};
    Descriptor other{reinterpret_cast<uint8_t*>(neighbor.data()),uint32_t(neighbor.size()),uint32_t(neighbor.size()),0,171};
    g.poseOwner=reinterpret_cast<uintptr_t>(&target);void* args[8]{};args[7]=&other;
    ApplyInner(args);Check(g.passes==0&&std::memcmp(neighbor.data(),rig.pose.data(),neighbor.size()*48)==0,"another actor with shared reference tracks received this grip");
    args[7]=&target;--target.count;ApplyInner(args);
    Check(g.passes==0&&std::memcmp(held.data(),rig.pose.data(),held.size()*48)==0,"LOD pose capacity was mistaken for its live bone count");
    target.count=target.capacity+1;ApplyInner(args);Check(g.passes==0,"invalid pose count was accepted");target.count=target.capacity;
    void* object[4]{};object[3]=&target;PublishActivity();NpcArmGripObjectUpdated(object);Check(g.passes>0,"completed own object pose was not solved");
    const auto passes=g.passes;other.track=172;args[7]=&other;ApplyInner(args);
    Check(g.passes==passes&&std::memcmp(neighbor.data(),rig.pose.data(),neighbor.size()*48)==0,"a different pose descriptor reused the active track identity");
    neighbor=held;other.buffer=reinterpret_cast<uint8_t*>(neighbor.data());const auto untouched=neighbor;
    for(auto& p:nativePoses)if(p.descriptor==g.poseOwner)p.retired=true;
    RestoreRetired(reinterpret_cast<uintptr_t>(&other),other.buffer,other.count);
    Check(std::memcmp(neighbor.data(),untouched.data(),neighbor.size()*48)==0,"retired grip restored another actor with identical bone values");
    RestoreRetired(g.poseOwner,target.buffer,target.count);
    Check(std::memcmp(held.data(),rig.pose.data(),held.size()*48)==0,"retired grip failed to restore its own actor");
    for(auto& value:grips)value.on=false;
}
void PartialNativePose(const Rig& rig,bool retired){
    for(auto& p:nativePoses)p=NativePose{};for(auto& g:grips)g=Grip{};
    auto pose=rig.pose;auto& g=grips[0];g=rig.grip(ArmR);g.on=true;g.side=0;g.trackA=171;
    g.begun=GetTickCount64()-1000;g.asked=GetTickCount64();
    const auto end=Model(g,pose,g.end);std::copy_n(end.p,3,g.target);g.target[1]+=.25f;g.target[2]+=.1f;
    struct Descriptor{uint8_t* buffer;uint32_t capacity,count;uintptr_t other,track;};
    Descriptor descriptor{reinterpret_cast<uint8_t*>(pose.data()),uint32_t(pose.size()),uint32_t(pose.size()),0,171};void* args[8]{};args[7]=&descriptor;
    g.poseOwner=reinterpret_cast<uintptr_t>(&descriptor);
    ApplyInner(args);Check(std::memcmp(pose[g.upper].q,rig.pose[g.upper].q,16)!=0,"partial-update fixture did not change the upper arm");
    auto expected=rig.pose;
    // A later graph pass can refresh translation/scale and one joint while
    // retaining the other rotations. Those retained rotations are still ours.
    pose[g.upper].p[0]+=.000001f;expected[g.upper].p[0]=pose[g.upper].p[0];
    pose[g.upper].s[1]=expected[g.upper].s[1]=1.001f;
    jl::Pose(g.hinge,.9f,rig.pose[g.mid].q,pose[g.mid].q);std::copy_n(pose[g.mid].q,4,expected[g.mid].q);
    if(retired){
        for(auto& p:nativePoses)if(p.buf==descriptor.buffer)p.retired=true;
        RestoreRetired(g.poseOwner,descriptor.buffer,descriptor.count);
    }else BeginNativePose(g.poseOwner,descriptor.buffer,descriptor.count);
    for(size_t b=0;b<pose.size();++b)Check(std::memcmp(&pose[b],&expected[b],48)==0,"partial native pose retained an earlier IK rotation or lost a fresh native channel");
    for(auto& value:grips)value.on=false;
}
void BonePointGrips(const Rig& rig){
    float worst=0,legacyWorst=0;double microseconds=0;size_t solves=0;
    for(int part=0;part<4;++part)for(int which=0;which<3;++which)for(float bend:{.5f,1.3f,2.2f})for(float turn:{-.2f,0.f,.2f}){
        auto g=rig.grip(part);auto initial=rig.pose;
        const int bone=which==0?g.upper:which==1?g.mid:g.end;
        const float local[3]{g.hinge.outgoing[0]*(which==2?.04f:.12f),.04f,.02f},axis[3]{0,1,0};
        const auto start=Model(g,initial,bone);float localQ[4],point[3],handQ[4];jl::AxisAngle(axis,.3f,localQ);jl::Rotate(start.q,local,point);
        for(int k=0;k<3;++k)point[k]+=start.p[k];jl::Multiply(start.q,localQ,handQ);
        g.anchor.on=true;g.anchor.bone=bone;std::copy_n(point,3,g.anchor.capture);std::copy_n(handQ,4,g.anchor.captureQ);
        auto wanted=initial;float delta[4],q[4];jl::AxisAngle(axis,turn,delta);jl::Multiply(g.upperRest,delta,q);
        jl::Clamp(g.upperRest,q,wanted[g.mid].p,part>=2?jl::Hip:jl::Shoulder,wanted[g.upper].q);
        jl::Pose(g.hinge,bend,wanted[g.mid].q,wanted[g.mid].q);
        const auto goalBone=Model(g,wanted,bone);jl::Rotate(goalBone.q,local,g.target);for(int k=0;k<3;++k)g.target[k]+=goalBone.p[k];
        jl::Multiply(goalBone.q,localQ,g.turn);
        auto pose=initial;const auto started=std::chrono::steady_clock::now();Solve(g,reinterpret_cast<uint8_t*>(pose.data()),1);
        microseconds+=std::chrono::duration<double,std::micro>(std::chrono::steady_clock::now()-started).count();++solves;
        const float error=Apart(g.anchor.point,g.target);worst=std::max(worst,error);
        Check(error<.004f,"reachable bone-local grasp point did not converge: "+std::to_string(error));
        for(int k=0;k<3;++k)Near(g.anchor.local[k],local[k],2e-5f,"grip point was recaptured/slid on its bone");
        const auto actual=Model(g,pose,bone);float attached[3];jl::Rotate(actual.q,local,attached);for(int k=0;k<3;++k)attached[k]+=actual.p[k];
        Near(Apart(attached,g.anchor.point),0,2e-5f,"published hand is not on its bone-local anchor");
        for(size_t i=0;i<pose.size();++i)Check(std::memcmp(pose[i].p,initial[i].p,sizeof(pose[i].p))==0,"bone-point solve stretched a bone");
        ConeInside(g.upperRest,pose[g.upper].q,pose[g.mid].p,part>=2?jl::Hip:jl::Shoulder,"anchor shoulder escaped bound");
        ConeInside(g.endRest,pose[g.end].q,pose[g.end].p,part>=2?jl::Ankle:jl::Wrist,"anchor wrist escaped bound");
        if(which==1){
            auto old=rig.grip(part);auto oldPose=initial;const auto tip=Model(old,oldPose,old.end);
            for(int k=0;k<3;++k)old.target[k]=tip.p[k]+g.target[k]-point[k];
            float inverse[4];jl::Conjugate(handQ,inverse);jl::Multiply(g.turn,inverse,old.turn);
            Solve(old,reinterpret_cast<uint8_t*>(oldPose.data()),1);const auto oldBone=Model(old,oldPose,bone);
            jl::Rotate(oldBone.q,local,attached);for(int k=0;k<3;++k)attached[k]+=oldBone.p[k];legacyWorst=std::max(legacyWorst,Apart(attached,g.target));
        }
    }
    Check(legacyWorst>.02f,"fixed distal-endpoint offset did not expose the old contact slip");
    std::cout<<"bone-local anchors worst "<<worst<<" m; old endpoint offset worst "<<legacyWorst<<" m; mean solve "<<microseconds/solves<<" us\n";
}
void TwoBoneContacts(const Rig& rig){
    auto pose=rig.pose;
    for(int side=0;side<2;++side){auto& g=grips[side];g=rig.grip(ArmR);g.on=true;g.trackA=91;g.begun=GetTickCount64()-1000;g.asked=GetTickCount64();
        const auto bone=Model(g,pose,g.upper);const float local[3]{-.10f-float(side)*.1f,side?.04f:-.04f,.02f};float delta[3];jl::Rotate(bone.q,local,delta);
        g.anchor.on=true;g.anchor.bone=g.upper;for(int k=0;k<3;++k)g.anchor.capture[k]=bone.p[k]+delta[k];std::copy_n(bone.q,4,g.anchor.captureQ);
        std::copy_n(g.anchor.capture,3,g.target);g.target[1]+=side?.06f:-.04f;g.target[2]+=.03f;std::copy_n(bone.q,4,g.turn);
    }
    struct Descriptor{uint8_t* buffer;uint32_t capacity,count;uintptr_t other,track;};Descriptor descriptor{reinterpret_cast<uint8_t*>(pose.data()),uint32_t(pose.size()),uint32_t(pose.size()),0,91};
    for(auto& g:grips)g.poseOwner=reinterpret_cast<uintptr_t>(&descriptor);
    void* args[8]{};args[7]=&descriptor;ApplyInner(args);
    for(int side=0;side<2;++side){auto& g=grips[side];float p[3],q[4];AnchorPose(g,descriptor.buffer,p,q);
        Near(Apart(p,g.anchor.point),0,1e-6f,"first hand published an intermediate NPC pose");
        Near(g.anchor.local[0],-.10f-float(side)*.1f,1e-5f,"second contact was captured after the first solve moved its bone");g.on=false;
    }
}
void MissingActorRelease(const Rig& rig){
    for(auto& p:nativePoses){p.owner.Reset();p.ownerKey=0;p.retired=false;p.count=0;}
    for(auto& v:retiredOwners)v.store(0);anyRetired.store(false);
    for(auto& g:grips)g.on=false;
    auto pose=rig.pose;auto& g=grips[0];g=rig.grip(ArmR);g.on=true;g.trackA=123;
    g.begun=GetTickCount64()-1000;g.asked=GetTickCount64();const auto end=Model(g,pose,g.end);
    std::copy_n(end.p,3,g.target);g.target[1]+=.12f;
    struct Descriptor{uint8_t* buffer;uint32_t capacity,count;uintptr_t other,track;};Descriptor desc{reinterpret_cast<uint8_t*>(pose.data()),uint32_t(pose.size()),uint32_t(pose.size()),0,123};
    g.poseOwner=reinterpret_cast<uintptr_t>(&desc);
    void* args[8]{};args[7]=&desc;ApplyInner(args);
    Check(std::memcmp(pose.data(),rig.pose.data(),pose.size()*sizeof(Bone))!=0,"release fixture never posed the NPC");
    StopAnchor(g);g.let=GetTickCount64()-300;
    ReapGrips(999,1);PublishActivity();
    Check(!g.on&&!anyOn&&anyRetired.load(),"missing actor kept its hand slot occupied");
    g=rig.grip(ArmL);g.on=true;g.trackA=456;g.poseOwner=456;g.asked=GetTickCount64();
    RestoreRetired(reinterpret_cast<uintptr_t>(&desc),desc.buffer,desc.count);
    Check(std::memcmp(pose.data(),rig.pose.data(),pose.size()*sizeof(Bone))==0,"returning actor kept its stale grip pose");
    Check(g.on&&g.trackA==456,"retired pose cleanup cancelled a new grab");
    RestoreRetired(reinterpret_cast<uintptr_t>(&desc),desc.buffer,desc.count);PublishActivity();Check(!anyRetired.load(),"fresh pose did not retire the repair entry");
    g.on=false;PublishActivity();
    Grip pending{};pending.on=true;pending.anchor.on=true;StopAnchor(pending);Check(!pending.on,"uninitialized attachment remained active after release");
}
using Json=nlohmann::json;
std::array<float,4> Row(const Json& rows,int index){return rows.at(index).get<std::array<float,4>>();}
int MakeLive(const char* input,const char* output){
    std::ifstream file(input);Json doc;file>>doc;const auto& rows=doc.at("pose");
    const auto up=Row(rows,1),end=Row(rows,3);float direction[3];for(int k=0;k<3;++k)direction[k]=end[k]-up[k];Check(jl::Unit(direction),"live limb length zero");
    const float diagonal[3]{.577350269f,.577350269f,.577350269f};
    Json samples=Json::array();std::array<float,3> previous{end[0],end[1],end[2]};
    const std::array<std::array<float,3>,6> destinations{{{up[0]+direction[0]*.03f,up[1]+direction[1]*.03f,up[2]+direction[2]*.03f},
        {up[0]+direction[0]*1.2f,up[1]+direction[1]*1.2f,up[2]+direction[2]*1.2f},
        {up[0],up[1],up[2]+.7f},{up[0],up[1]-.7f,up[2]},{up[0]+.6f,up[1]+.4f,up[2]-.4f},{end[0],end[1],end[2]}}};
    for(size_t phase=0;phase<destinations.size();++phase){
        for(int step=1;step<=16;++step){const float t=float(step)/16,u=t*t*(3-2*t);float p[3],q[4];
            for(int k=0;k<3;++k)p[k]=previous[k]+(destinations[phase][k]-previous[k])*u-end[k]+(k==2?1.f:0.f);
            jl::AxisAngle(diagonal,(phase%2?1:-1)*2.8f*std::sin(t*jl::Pi),q);
            samples.push_back(Json{{"phase",phase},{"target",p},{"turn",q}});
        }previous=destinations[phase];
    }
    std::ofstream out(output);out<<samples.dump(2)<<'\n';return 0;
}
int VerifyLive(const char* input,bool allLimbs=true){
    std::ifstream file(input);Json doc;file>>doc;size_t count=0;float maximumMiss=0,minimumFlex=180,maximumFlex=0;std::array<bool,4> parts{};
    for(const auto& sample:doc){
        if(!sample.contains("pose")||sample.at("pose").size()<14)continue;
        const auto& rows=sample.at("pose");const auto meta=Row(rows,0);const int part=int(meta[0]);Check(part>=0&&part<4,"bad part in live sample");
        const auto upper=Row(rows,4),middle=Row(rows,5),tip=Row(rows,6),upperRef=Row(rows,7),midRef=Row(rows,8),tipRef=Row(rows,9);
        const auto target=Row(rows,10),outgoing=Row(rows,12),incoming=Row(rows,13);
        ConeInside(upperRef.data(),upper.data(),incoming.data(),part>=2?jl::Hip:jl::Shoulder,"live shoulder/hip escaped bound");
        ConeInside(tipRef.data(),tip.data(),outgoing.data(),part>=2?jl::Ankle:jl::Wrist,"live wrist/ankle escaped bound");
        float inverse[4],relative[4],twist[4],swing[4];jl::Conjugate(midRef.data(),inverse);jl::Multiply(inverse,middle.data(),relative);
        const float pronation=jl::Twist(relative,outgoing.data());Check(std::abs(pronation)<=incoming[3]+.001f,"live forearm/knee twist escaped bound");
        jl::AxisAngle(outgoing.data(),pronation,twist);jl::Conjugate(twist,inverse);jl::Multiply(relative,inverse,swing);if(swing[3]<0)for(float& v:swing)v=-v;
        Check(std::abs(swing[0])<.001f&&std::abs(swing[1])<.001f,"live hinge bends sideways");
        const float flex=(2*std::atan2(swing[2],swing[3])+target[3])*(part>=2?1:-1);
        Check(flex>=1.8f*jl::Deg&&flex<=outgoing[3]+.001f,"live hinge flex escaped bound");
        minimumFlex=std::min(minimumFlex,flex/jl::Deg);maximumFlex=std::max(maximumFlex,flex/jl::Deg);maximumMiss=std::max(maximumMiss,meta[1]);parts[part]=true;++count;
    }
    const auto partCount=std::count(parts.begin(),parts.end(),true);
    Check(count>(allLimbs?50u:10u)&&partCount>=(allLimbs?4:1),"live report lacks required limbs or enough samples");
    std::cout<<Json{{"samples",count},{"parts",partCount},{"flex_min_degrees",minimumFlex},{"flex_max_degrees",maximumFlex},{"max_unreachable_distance_m",maximumMiss},{"bounded",true}}.dump()<<'\n';return 0;
}
int VerifyAttachment(const char* input){
    std::ifstream file(input);Json doc;file>>doc;size_t count=0;float worst=0,angleWorst=0,sum=0;std::array<float,4> identity{},anchor{};bool have=false;
    for(const auto& sample:doc){
        const std::string label=sample.value("label",std::string{});
        if(label!="held"&&label!="moved"&&label!="turned"&&label!="returned")continue;
        const auto& rows=sample.at("pose");Check(rows.size()>=21,"native bone attachment not active");
        const auto meta=Row(rows,14),local=Row(rows,16),world=Row(rows,17),rotation=Row(rows,20);
        const auto hand=Row(sample.at("hand"),0),handQ=Row(sample.at("hand"),1);
        Check(meta[3]>.5f,"bone attachment stopped during normal hold");
        if(have){Check(meta[0]==identity[0]&&meta[1]==identity[1]&&meta[2]==identity[2],"held bone/owner changed");Near(Apart(local.data(),anchor.data()),0,1e-5f,"grip slid on its bone");}
        else{identity=meta;anchor=local;have=true;}
        const float gap=Apart(hand.data(),world.data());worst=std::max(worst,gap);sum+=gap;
        float dot=0;for(int k=0;k<4;++k)dot+=handQ[k]*rotation[k];angleWorst=std::max(angleWorst,2*std::acos(std::clamp(std::abs(dot),0.f,1.f))/jl::Deg);++count;
    }
    Check(count>=24,"too few attachment samples");
    std::cout<<Json{{"samples",count},{"mean_wrist_anchor_gap_m",sum/count},{"max_wrist_anchor_gap_m",worst},{"max_rotation_gap_degrees",angleWorst},{"bone",identity[0]},{"owner",identity[1]}}.dump()<<'\n';
    Check(worst<.015f&&angleWorst<5,"drawn wrist still diverges from the bone attachment");
    return VerifyLive(input,false);
}
}
int main(int argc,char** argv){try{
    if(argc==4&&std::string(argv[1])=="--make-live")return MakeLive(argv[2],argv[3]);
    if(argc==3&&std::string(argv[1])=="--check-live")return VerifyLive(argv[2]);
    if(argc==3&&std::string(argv[1])=="--check-grip")return VerifyLive(argv[2],false);
    if(argc==3&&std::string(argv[1])=="--check-anchor")return VerifyAttachment(argv[2]);
    const std::string mode=argc>1?argv[1]:"all";
    if(mode=="all")Boundaries();if(mode=="all"||mode=="gate")GatePublication();size_t n=0;
    for(const char* name:{"man","woman","man_apose","woman_apose"}){Rig rig(name);if(mode=="all"||mode=="limbs")n+=Exercise(rig);if(mode=="all"||mode=="cache")NewPoseWithSameWrist(rig);if(mode=="all"||mode=="continuity")StraightLimbContinuity(rig);if(mode=="all"||mode=="moving")MovingTargetOnCachedPose(rig);if(mode=="all"||mode=="coupled")TwoHandsOneActor(rig);}
    if(mode=="all"||mode=="anchors")for(const char* name:{"man","woman","man_apose","woman_apose"}){Rig rig(name);BonePointGrips(rig);TwoBoneContacts(rig);}
    if(mode=="all"||mode=="retire"){Rig rig("man_apose");MissingActorRelease(rig);}
    if(mode=="all"||mode=="partial")for(const char* name:{"man","woman","man_apose","woman_apose"}){Rig rig(name);PartialNativePose(rig,false);PartialNativePose(rig,true);}
    if(mode=="all"||mode=="isolation")for(const char* name:{"man","woman","man_apose","woman_apose"}){Rig rig(name);SharedRigIsolation(rig);}
    if(mode=="all"||mode=="twist")ContinuousTwistStops();
    std::cout<<n<<" real-rig limb cases: hinge direction, flexion, local joint cones, lengths, pose isolation and pose-cache regression PASS\n";return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}
