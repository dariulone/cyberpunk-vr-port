#include "Anim/Cyberware.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/SkeletonPolicy.hpp"
#include "Anim/MantisDeployment.hpp"
#include "Anim/CyberwareMuscles.hpp"
#include "Anim/HandGestures.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include <iterator>
#include <algorithm>
#include <cmath>
#include <string>
#include <array>
#include <mutex>
#include <cstdio>
#include "Utils/SharedSlots.hpp"

namespace {
struct MantisSide {
    int wrist=-1,index=-1,sourceIndex=-1,forearm=-1,hand=-1;
    int bladeJoint=-1;
    float dorsalHand[3]{},dorsalForearm[3]{};
    bool frameValid=false;
};
MantisSide sides[2];
struct FingerLink {int source=-1,target=-1,side=0;};
std::array<FingerLink,38> skinFingers{};
int skinFingerCount=0;
int deploymentIndices[std::size(cvr::vrik::MantisDeployment)]{};
float deploymentScale[2]{1,1};
bool Valid(int i) { return i>=0 && i<g_VRBoneCount; }
using cvr::cyberware::Vec3;
// The monowire's own route out of the left hand in the unmodified game (bind 0,
// FK snapshot of nanoWire01/03/04/06/08, 2026-10-16), fixed-point 1/131072 m:
// the root on the forearm housing, across the wrist crease on the palm side,
// then along the palm under the curled fingers. The first three ride the
// distal twist joint that carries the housing's skin, the rest the hand.
// The right arm mirrors it (every axis negated, like the outlet light).
struct LeadPoint {bool hand;float x,y,z;};
constexpr LeadPoint WireLead[]={
    {false,-3998.f,-317.f,-2910.f},{false,622.f,677.f,-3734.f},{false,2085.f,1115.f,-3663.f},
    {true,2226.f,264.f,-3619.f},{true,5789.f,-581.f,-4246.f},
};
struct CombatSide {
    int hand=-1,knuckle=-1;
    int wireAnchor=-1,wireRoot=-1,wireControl=-1,wireSource=-1;
    std::array<int,cvr::cyberware::Wire::Count> wire{};
    std::array<Vec3,cvr::cyberware::Wire::Count> wireAxis{};
    float length{};
    cvr::cyberware::Wire dynamics;
};
CombatSide combat[2];
int wireTrack[2]={-1,-1};
const float* wireTrackReference{};
int launcherAmmo=-1,launcherMuzzle=-1,launcherBase=-1;
struct WristBinding {int bone=-1;float position[3]{},rotation[4]{};};
WristBinding launcherWrist[2];
uint32_t wireEpoch=~0u;uint64_t wireStamp{};
// The body origin at the last wire step: the line is carried by its movement (Wire::Step).
Vec3 wireOrigin{};bool wireOriginValid{};
std::mutex frameMutex;
cvr::cyberware::Frame frame;
std::array<cvr::cyberware::Plane,cvr::cyberware::Wire::Count> collisionPlanes[2];
uint64_t collisionStamp[2]{};
// One atomic packet: start milliseconds in the high bits, duration in the low16.
std::atomic<uint64_t> musclePulse[2]{};
int muscleIndices[std::size(cvr::cyberware::MuscleCurves)]{};
float muscleReferenceScale[std::size(cvr::cyberware::MuscleCurves)][3]{};
std::atomic<int> selectedKind{};
std::atomic<uint64_t> selectedStamp{};
std::atomic<bool> rawGrip[2]{};
Vec3 Position(int i){return {g_fkPos[i][0],g_fkPos[i][1],g_fkPos[i][2]};}
Vec3 Rotated(const float* q,Vec3 v){float a[3]={v.x,v.y,v.z},b[3];VRIK_QuatRotateVec(q,a,b);return {b[0],b[1],b[2]};}
bool Grip(int side){return rawGrip[side].load(std::memory_order_relaxed);}
bool DescendsFrom(int bone,int parent) {
    for(int steps=0;Valid(bone) && steps++<g_VRBoneCount;bone=g_VRBoneParent[bone])
        if(bone==parent)return true;
    return false;
}
void ModelPosition(uint8_t* buffer,int bone,const float* position) {
    const int parent=g_VRBoneParent[bone];
    if(Valid(parent))VRIK_WriteLocalPos(buffer,bone,g_fkPos[parent],g_fkRot[parent],position);
}
bool MountFrame(const MantisSide& s,float* mount) {
    if(!s.frameValid || !Valid(s.hand) || !Valid(s.forearm))return false;
    float axis[3],dorsal[3],swing[4],normal[3];
    for(int k=0;k<3;++k)axis[k]=g_fkPos[s.hand][k]-g_fkPos[s.forearm][k];
    if(VRIK_Dot3(axis,axis)<1e-6f)return false;
    VRIK_Norm3(axis);VRIK_QuatRotateVec(g_fkRot[s.hand],s.dorsalHand,dorsal);
    auto project=[&] {const float d=VRIK_Dot3(dorsal,axis);for(int k=0;k<3;++k)dorsal[k]-=axis[k]*d;};
    project();
    if(VRIK_Dot3(dorsal,dorsal)<1e-5f) {VRIK_QuatRotateVec(g_fkRot[s.forearm],s.dorsalForearm,dorsal);project();}
    if(VRIK_Dot3(dorsal,dorsal)<1e-5f)return false;
    VRIK_Norm3(dorsal);
    const float y[3]={0,1,0},z[3]={0,0,1};VRIK_QuatFromTo(y,axis,swing);
    VRIK_QuatRotateVec(swing,z,normal);
    float cross[3];VRIK_Cross3(normal,dorsal,cross);
    const float half=.5f*std::atan2(VRIK_Dot3(cross,axis),VRIK_Dot3(normal,dorsal));
    const float turn[4]={axis[0]*std::sin(half),axis[1]*std::sin(half),axis[2]*std::sin(half),std::cos(half)};
    VRIK_QuatMul(turn,swing,mount);VRIK_QuatNorm(mount);return true;
}
}

void VRIK_ConfigureCyberwareRig(const char* const* names,int count,const uint8_t* reference,int referenceCount) {
    wireTrack[0]=wireTrack[1]=-1;wireTrackReference=nullptr;
    sides[0]={};sides[1]={};std::fill(std::begin(deploymentIndices),std::end(deploymentIndices),-1);
    skinFingerCount=0;
    deploymentScale[0]=deploymentScale[1]=1;
    for(auto& c:combat){c={};c.wire.fill(-1);}
    launcherAmmo=launcherMuzzle=launcherBase=-1;wireEpoch=~0u;wireStamp=0;wireOriginValid=false;
    for(auto& binding:launcherWrist)binding={};
    for(auto& pulse:musclePulse)pulse.store(0,std::memory_order_relaxed);
    std::fill(std::begin(muscleIndices),std::end(muscleIndices),-1);
    {std::lock_guard lock(frameMutex);frame={};collisionStamp[0]=collisionStamp[1]=0;}
    if(g_VRCyberwareFamily==0)return;
    auto find=[&](const std::string& name) {
        for(int i=0;i<count;++i)if(names[i] && cvr::vrik::EqualBoneName(names[i],name))return i;
        return -1;
    };
    if(g_VRCyberwareFamily==1 || g_VRCyberwareFamily==2)for(int side=0;side<2;++side) {
        const std::string hand=side?"Right":"Left",prefix=side?"r_":"l_";
        for(int joint=0;joint<4;++joint)for(const char* digit:{"Thumb","Index","Middle","Ring","Pinky"}) {
            if(joint==3 && std::string_view(digit)=="Thumb")continue;
            const std::string canonical=joint==0 ? hand+"InHand"+digit:hand+"Hand"+digit+std::to_string(joint);
            const auto actual=g_VRCyberwareFamily==1 ? canonical+"_setup" : prefix+"hand"+digit+"0"+std::to_string(joint+1)+"_JNT";
            const int source=find(canonical),target=find(actual);
            if(Valid(source)&&Valid(target)&&skinFingerCount<int(skinFingers.size()))skinFingers[skinFingerCount++]={source,target,side};
        }
    }
    // Arming cannot borrow the animation job's working FK arrays. Static rather than stack (VRIK_MAX_BONES
    // entries, ~60 KB): arming runs under g_PlayerPoseStateLock held exclusively.
    static std::array<std::array<float,3>,VRIK_MAX_BONES> rp;
    static std::array<std::array<float,4>,VRIK_MAX_BONES> rq;
    static std::array<bool,VRIK_MAX_BONES> valid;
    rp.fill({});rq.fill({});valid.fill(false);
    for(int i=0;reference && i<std::min({count,referenceCount,VRIK_MAX_BONES});++i) {
        const auto* p=reinterpret_cast<const float*>(reference+i*48);const int parent=g_VRBoneParent[i];
        if(parent>=i || (parent>=0 && !valid[parent]))continue;
        bool finite=true;for(int k=0;k<8;++k)finite=finite && std::isfinite(p[k]);
        const float norm=p[4]*p[4]+p[5]*p[5]+p[6]*p[6]+p[7]*p[7];
        if(!finite || norm<.5f || norm>1.5f)continue;
        if(parent>=0) {
            VRIK_QuatRotateVec(rq[parent].data(),p,rp[i].data());
            for(int k=0;k<3;++k)rp[i][k]+=rp[parent][k];
            VRIK_QuatMul(rq[parent].data(),p+4,rq[i].data());
        } else {std::copy_n(p,3,rp[i].data());std::copy_n(p+4,4,rq[i].data());}
        VRIK_QuatNorm(rq[i].data());valid[i]=true;
    }
    for(int side=0;side<2;++side) {
        const std::string p=side ? "r_":"l_",h=side ? "Right":"Left";
        auto& s=sides[side];
        auto& c=combat[side];c.hand=find(h+"Hand");c.knuckle=find(h+"HandMiddle1_setup");
        if(!Valid(c.knuckle))c.knuckle=find(h+"HandMiddle1");
        // The outlet light in a0_002_ma__monowire_whip.ent is bound to
        // InHandThumb. nanoWire01_off is an animation control in the forearm,
        // not the personal-link housing from which the visible line emerges.
        c.wireAnchor=find(h+"InHandThumb");c.wireRoot=find(p+"nanoWire01_grp");
        c.wireControl=find(p+"nanoWire01_off");
        // The housing on the inner forearm is skinned ~70% to the distal
        // twist joint; the rendered line starts there (see the apply pass).
        c.wireSource=find(p+"Wrist_2_JNT");
        for(int k=0;k<cvr::cyberware::Wire::Count;++k) {
            char suffix[32];std::snprintf(suffix,sizeof(suffix),"nanoWire%02d_jnt",k+1);
            c.wire[k]=find(p+suffix);
        }
        for(int k=0;k<cvr::cyberware::Wire::Count;++k) {
            const int bone=c.wire[k],next=c.wire[k+1<cvr::cyberware::Wire::Count?k+1:k-1];
            if(!Valid(bone)||!Valid(next)||!valid[bone]||!valid[next])continue;
            float axis[3],inverse[4];
            for(int d=0;d<3;++d)axis[d]=(rp[next][d]-rp[bone][d])*(k+1<cvr::cyberware::Wire::Count?1.f:-1.f);
            if(k+1<cvr::cyberware::Wire::Count)c.length+=std::sqrt(VRIK_Dot3(axis,axis));
            VRIK_Norm3(axis);VRIK_QuatConj(rq[bone].data(),inverse);float local[3];VRIK_QuatRotateVec(inverse,axis,local);
            c.wireAxis[k]={local[0],local[1],local[2]};
        }
        s.wrist=find(p+"innerWrist01_JNT");s.index=find(p+"handIndex01_JNT");
        s.sourceIndex=find(h+"InHandIndex");s.forearm=find(h+"ForeArm");s.hand=find(h+"Hand");
        s.bladeJoint=find(p+"blade02_JNT");
        if(!DescendsFrom(s.index,s.wrist) || !DescendsFrom(s.sourceIndex,s.hand))s={};
        const int index=find(h+"HandIndex1"),pinky=find(h+"HandPinky1");
        if(Valid(index) && Valid(pinky) && Valid(s.hand) && Valid(s.forearm) &&
           valid[index] && valid[pinky] && valid[s.hand] && valid[s.forearm]) {
            float a[3],b[3],dorsal[3],inverse[4];
            for(int k=0;k<3;++k){a[k]=rp[index][k]-rp[s.hand][k];b[k]=rp[pinky][k]-rp[s.hand][k];}
            VRIK_Cross3(a,b,dorsal);if(side)for(float& v:dorsal)v=-v;
            if(VRIK_Dot3(dorsal,dorsal)>1e-10f) {
                VRIK_Norm3(dorsal);VRIK_QuatConj(rq[s.hand].data(),inverse);
                VRIK_QuatRotateVec(inverse,dorsal,s.dorsalHand);
                VRIK_QuatConj(rq[s.forearm].data(),inverse);VRIK_QuatRotateVec(inverse,dorsal,s.dorsalForearm);
                s.frameValid=true;
            }
        }
        if(reference && Valid(s.bladeJoint) && s.bladeJoint<referenceCount) {
            const float* hinge=reinterpret_cast<const float*>(reference+s.bladeJoint*48);
            // Native reference blade hinge length of the sampled rig. Scale
            // the deployed local translations to this rig's actual geometry.
            const float length=std::sqrt(VRIK_Dot3(hinge,hinge));
            if(std::isfinite(length))deploymentScale[side]=std::clamp(length/cvr::vrik::MantisDeploymentReferenceLength[side],.5f,2.f);
        }

    }
    // Both arms pay out the same authored line. Do not inherit a one-sided
    // equip/attack animation's extension or scale into the physical rig.
    if(combat[0].length>.2f && combat[1].length>.2f) {
        const float length=std::min(cvr::cyberware::Wire::Reach,std::min(combat[0].length,combat[1].length));
        combat[0].length=combat[1].length=length;
    }
    for(size_t i=0;i<std::size(cvr::vrik::MantisDeployment);++i)
        deploymentIndices[i]=find(cvr::vrik::MantisDeployment[i].name);
    launcherAmmo=find("launcher_ammo_base");launcherMuzzle=find("launcher_fragment_fire");launcherBase=find("launcher_base");
    if(g_VRCyberwareFamily==4 && Valid(combat[0].hand) && valid[combat[0].hand]) {
        const int hand=combat[0].hand;float inverse[4];VRIK_QuatConj(rq[hand].data(),inverse);
        const char* roots[]={"LeftHand_setup","l_Wrist_2_JNT_setup"};
        for(int i=0;i<2;++i) {
            const int bone=find(roots[i]);if(!Valid(bone)||!valid[bone])continue;
            auto& binding=launcherWrist[i];binding.bone=bone;
            float offset[3];for(int k=0;k<3;++k)offset[k]=rp[bone][k]-rp[hand][k];
            VRIK_QuatRotateVec(inverse,offset,binding.position);
            VRIK_QuatMul(inverse,rq[bone].data(),binding.rotation);
        }
    }
    for(size_t i=0;i<std::size(cvr::cyberware::MuscleCurves);++i) {
        const auto& curve=cvr::cyberware::MuscleCurves[i];
        if(curve.family!=g_VRCyberwareFamily)continue;
        const int bone=find(curve.name);muscleIndices[i]=bone;
        for(int k=0;k<3;++k)muscleReferenceScale[i][k]=reference && bone>=0 && bone<referenceCount
            ? reinterpret_cast<const float*>(reference+bone*48)[8+k]:1.f;
    }
}
static void ApplySkinFingers(uint8_t* buffer,int count) {
    if(!skinFingerCount)return;
    // Cyberware has a second, skinned finger chain. Its phalange offsets/axes are
    // the canonical ones, but its native idle curl is otherwise independent.
    // Transfer the controlled model rotations, retaining all authored lengths.
    for(int i=0;i<skinFingerCount;++i) {
        const auto& link=skinFingers[i];const int parent=g_VRBoneParent[link.target];
        if(!cvr::gestures::OwnsHand(link.side) || !Valid(parent))continue;
        VRIK_WriteLocalRot(buffer,link.target,g_fkRot[parent],g_fkRot[link.source]);
        std::copy_n(g_fkRot[link.source],4,g_fkRot[link.target]);
    }
    VRIK_ComputeFK(buffer,count);
}

static void ApplyLauncherWrist(uint8_t* buffer) {
    const int hand=combat[0].hand;if(g_VRCyberwareFamily!=4 || !Valid(hand))return;
    // The launcher skin uses a sibling of LeftHand, not its descendant. Its
    // bind frame differs by ~45 degrees; preserve that binding while driving
    // the actual skin wrist from the tracked hand. Keep the forearm barrel
    // and native finger articulation independent of wrist flexion.
    for(const auto& binding:launcherWrist) {
        const int bone=binding.bone;if(!Valid(bone)||!Valid(g_VRBoneParent[bone]))continue;
        float position[3],rotation[4];
        VRIK_QuatRotateVec(g_fkRot[hand],binding.position,position);
        for(int k=0;k<3;++k)position[k]+=g_fkPos[hand][k];
        VRIK_QuatMul(g_fkRot[hand],binding.rotation,rotation);
        ModelPosition(buffer,bone,position);
        VRIK_WriteLocalRot(buffer,bone,g_fkRot[g_VRBoneParent[bone]],rotation);
    }
    VRIK_ComputeFK(buffer,VRIK_FKCount());
}

static void ApplyMantisPose(uint8_t* buffer) {
    if(!buffer || !VRIK_UsesNativeCyberware() || g_VRCyberwareFamily!=2 || g_VRBind!=4)return;
    const int count=VRIK_FKCount();VRIK_ComputeFK(buffer,count);
    float mounts[2][4]{};const bool mounted[2]={MountFrame(sides[0],mounts[0]),MountFrame(sides[1],mounts[1])};
    float wristPosition[2][3]{},wristRotation[2][4]{},mountOrigin[2][3]{};
    bool trackedWrist[2]{};
    for(int side=0;side<2;++side) {
        const auto& s=sides[side];if(!mounted[side])continue;
        std::copy_n(g_fkPos[s.hand],3,mountOrigin[side]);
        if(!Valid(s.wrist) || !Valid(s.index) || !Valid(s.sourceIndex))continue;
        float inverse[4],delta[4],offset[3],anchor[3];
        VRIK_QuatConj(g_fkRot[s.index],inverse);VRIK_QuatMul(g_fkRot[s.sourceIndex],inverse,delta);VRIK_QuatNorm(delta);
        for(int k=0;k<3;++k)offset[k]=g_fkPos[s.wrist][k]-g_fkPos[s.index][k];
        VRIK_QuatRotateVec(delta,offset,wristPosition[side]);
        for(int k=0;k<3;++k)wristPosition[side][k]+=g_fkPos[s.sourceIndex][k];
        VRIK_QuatMul(delta,g_fkRot[s.wrist],wristRotation[side]);
        VRIK_QuatRotateVec(mounts[side],cvr::vrik::MantisDeploymentWristAnchor[side],anchor);
        // Mount around the actual tracked wrist socket, not the canonical Hand
        // origin. Welding only the hand left the round blade-base joint 4-7cm
        // away from its native mating point when the wrist changed orientation.
        // Translate the complete mechanism together; preserve its native shape
        // and the blade's forearm-aligned cutting axis.
        for(int k=0;k<3;++k)mountOrigin[side][k]=wristPosition[side][k]-anchor[k]*deploymentScale[side];
        trackedWrist[side]=true;
    }
    for(size_t i=0;i<std::size(cvr::vrik::MantisDeployment);++i) {
        const int bone=deploymentIndices[i];if(!Valid(bone))continue;
        const auto& pose=cvr::vrik::MantisDeployment[i];
        const int side=pose.name[0]=='r';
        const int parent=g_VRBoneParent[bone];
        if(!mounted[side] || !Valid(parent))continue;
        float position[3],rotation[4];VRIK_QuatRotateVec(mounts[side],pose.position,position);
        for(int k=0;k<3;++k)position[k]=mountOrigin[side][k]+position[k]*deploymentScale[side];
        VRIK_QuatMul(mounts[side],pose.rotation,rotation);VRIK_QuatNorm(rotation);
        ModelPosition(buffer,bone,position);VRIK_WriteLocalRot(buffer,bone,g_fkRot[parent],rotation);
        // Profile order is parent-before-child. Keep FK current for each
        // dependent joint without a full-rig walk for every blade bone.
        std::copy_n(position,3,g_fkPos[bone]);std::copy_n(rotation,4,g_fkRot[bone]);
    }
    VRIK_ComputeFK(buffer,count);
    for(int side=0;side<2;++side) {
        const auto& s=sides[side];
        if(!trackedWrist[side])continue;
        // The actual skin and mechanism meshes use DIFFERENT names for the
        // same index metacarpal bind matrix: l/r_handIndex01_JNT and
        // Left/RightInHandIndex. The canonical wrist alone only moves the
        // mechanism's index insert. Transport the complete mechanical wrist
        // so this shared skinning anchor agrees, preserving its native local
        // finger articulation. A T-pose Hand->wrist offset is not this binding.
        const int parent=g_VRBoneParent[s.wrist];
        if(!Valid(parent))continue;
        ModelPosition(buffer,s.wrist,wristPosition[side]);
        VRIK_WriteLocalRot(buffer,s.wrist,g_fkRot[parent],wristRotation[side]);
        VRIK_ComputeFK(buffer,count);
    }

}

static void ApplyMuscles(uint8_t* buffer) {
    using namespace cvr::cyberware;
    const uint64_t now=GetTickCount64();
    for(size_t i=0;i<std::size(MuscleCurves);++i) {
        const auto& curve=MuscleCurves[i];const int bone=muscleIndices[i],side=curve.name[0]=='r';
        if(curve.family!=g_VRCyberwareFamily || !Valid(bone))continue;
        const uint64_t pulse=musclePulse[side].load(std::memory_order_acquire),start=pulse>>16,duration=pulse&0xffffu;
        const bool active=duration && now>=start && now-start<duration && SelectedKind()==curve.family;
        float* transform=reinterpret_cast<float*>(buffer+bone*48);
        if(curve.family==1)for(int k=0;k<3;++k)transform[8+k]=muscleReferenceScale[i][k];
        if(!active)continue;
        const float phase=float(now-start)/float(duration)*(MuscleSampleCount-1);
        const int a=std::clamp(int(phase),0,MuscleSampleCount-2);const float weight=phase-a;
        const auto& first=curve.samples[a];const auto& next=curve.samples[a+1];
        const float sign=VRIK_Dot3(first.rotation,next.rotation)+first.rotation[3]*next.rotation[3]<0?-1.f:1.f;
        float delta[4],rotation[4];
        for(int k=0;k<4;++k)delta[k]=first.rotation[k]*(1-weight)+next.rotation[k]*weight*sign;
        VRIK_QuatNorm(delta);
        if(curve.family==2) {
            for(int k=0;k<3;++k)transform[k]+=(first.position[k]*(1-weight)+next.position[k]*weight)*deploymentScale[side];
            VRIK_QuatMul(delta,transform+4,rotation);VRIK_QuatNorm(rotation);std::copy_n(rotation,4,transform+4);
        }
        for(int k=0;k<3;++k)transform[8+k]=muscleReferenceScale[i][k]*(first.scale[k]*(1-weight)+next.scale[k]*weight);
    }
}

void VRIK_ConfigureCyberwareTracks(const char* const* names,int count,const float* reference) {
    wireTrack[0]=wireTrack[1]=-1;wireTrackReference=reference;
    if(!names || count<0 || count>8192)return;
    for(int i=0;i<count;++i)if(names[i]) {
        if(std::string_view(names[i])=="leftWireSlideFactor")wireTrack[0]=i;
        if(std::string_view(names[i])=="rightWireSlideFactor")wireTrack[1]=i;
    }
}

void VRIK_ApplyWireTracks(float* tracks,uint32_t count) {
    if(!tracks || tracks==wireTrackReference || count>8192 || g_VRBind!=4 || g_VRCyberwareFamily!=3)return;
    // Mesh ExtensionFloatTrackId differs left/right. Native idle retracts one
    // side through this shader input even when both physical chains are equal.
    // Geometry already owns payout; render the full current curve, not a second
    // independently shortened animation curve. Never write resource defaults.
    for(int side=0;side<2;++side)if(wireTrack[side]>=0 && uint32_t(wireTrack[side])<count)
        tracks[wireTrack[side]]=combat[side].dynamics.deployed>.0001f?1.f:0.f;
}

void VRIK_ApplyCyberwarePose(uint8_t* buffer,float* tracks,uint32_t trackCount) {
    if(!buffer || !VRIK_UsesNativeCyberware() || g_VRBind!=4)return;
    cvr::gestures::Apply(buffer);
    if(g_VRCyberwareFamily==2)ApplyMantisPose(buffer);
    else VRIK_ComputeFK(buffer,VRIK_FKCount());
    ApplyLauncherWrist(buffer);
    ApplySkinFingers(buffer,VRIK_FKCount());
    ApplyMuscles(buffer);
    if(g_VRCyberwareFamily==2)VRIK_ComputeFK(buffer,VRIK_FKCount());
    float worldPos[3],worldRot[4],trackingYaw{};
    if(!VRIK_ReadCurrentBodyFrame(worldPos,worldRot,&trackingYaw))return;
    const Vec3 origin{worldPos[0],worldPos[1],worldPos[2]};float inverse[4];VRIK_QuatConj(worldRot,inverse);
    const uint64_t now=GetTickCount64();const uint32_t epoch=g_VrikFrameEpoch.load(std::memory_order_relaxed);
    const bool advance=epoch!=wireEpoch;
    const float dt=wireStamp && now>=wireStamp ? float(now-wireStamp)*.001f:0.f;
    // Locomotion carries the wire instead of dragging it: only the arm's own motion swings it.
    const Vec3 carry=wireOriginValid?origin-wireOrigin:Vec3{};
    cvr::cyberware::Frame output{};output.family=g_VRCyberwareFamily;output.epoch=epoch;output.stamp=now;output.bodyOrigin=origin;
    for(int side=0;side<2;++side) {
        auto& c=combat[side];if(!Valid(c.hand))continue;
        auto world=[&](int bone){return origin+Rotated(worldRot,Position(bone));};
        const Vec3 raw{g_VRHandRawModel[side][0],g_VRHandRawModel[side][1],g_VRHandRawModel[side][2]};
        output.wrists[side]=origin+Rotated(worldRot,raw);
        const float rawRotation[4]={g_VRHandRawRot[side][0],g_VRHandRawRot[side][1],g_VRHandRawRot[side][2],g_VRHandRawRot[side][3]};
        VRIK_QuatMul(worldRot,rawRotation,output.handRotation[side]);VRIK_QuatNorm(output.handRotation[side]);
        if(g_VRCyberwareFamily==3 && Valid(c.wire[0]) && Valid(c.wire.back()) && c.length>.2f) {
            // The simulated first joint is an OUTPUT, never its own anchor.
            // The native emitter control/forearm mount remains independent.
            const int anchorBone=c.wireAnchor;
            if(!Valid(anchorBone))continue;
            // Exact native light_slots offset in REDengine fixed-point units.
            // The light is centred on the glowing opening of the link housing.
            const float sign=side?-1.f:1.f;
            const Vec3 outlet{sign*3128.f/131072.f,-sign*3244.f/131072.f,(side?-3218.f:3217.f)/131072.f};
            const Vec3 anchor=world(anchorBone)+Rotated(worldRot,Rotated(g_fkRot[anchorBone],outlet));
            const Vec3 direction=Valid(c.knuckle)?world(c.knuckle)-world(c.hand):Rotated(worldRot,{0,1,0});
            for(int root:{c.wireRoot,c.wireControl})if(Valid(root)) {
                float* scale=reinterpret_cast<float*>(buffer+root*48+32);scale[0]=scale[1]=scale[2]=1.f;
            }
            if(advance) {
                {std::lock_guard lock(frameMutex);
                    if(now>=collisionStamp[side] && now-collisionStamp[side]<100)c.dynamics.planes=collisionPlanes[side];
                    else c.dynamics.planes={};
                }
                // Grip155 is outside the 127-float pose snapshot. Read input
                // slots directly, like WheelGrab; SharedPose(155) is invalid.
                c.dynamics.Step(anchor,direction,Grip(side),dt,c.length,carry);
            }
            if(!c.dynamics.valid)continue;
            output.deployment[side]=c.dynamics.deployed;
            output.count[side]=c.dynamics.deployed>.1f && Grip(side)?cvr::cyberware::Wire::Count:0;
            // THE LINE LEAVES THE FOREARM HOUSING, THEN THE FIST (2026-10-16: it used
            // to come out of the fist instead of the forearm housing).
            // The simulated chain keeps its pin at the fist outlet: combat,
            // collision planes and the swing direction read it unchanged. Only
            // the rendered joints change: the first five follow the game's own
            // route (WireLead) from the forearm housing over the wrist and the
            // palm into the fist -- the reference is the unmodified
            // left hand -- the next one is the outlet, and the 46 simulated
            // points are resampled by arc length onto the remaining joints, so
            // the tip stays the tip.
            // THE LINE IS SHOWN WHERE THE BODY IS NOW (2026-10-17: on the run the wire doubled, the
            // copy did not move with the run). The line steps once per tracking epoch, but this runs for
            // every pose evaluation of that epoch, and the body moves between them. The simulated points
            // ride along by the same carry the next step will apply, so every evaluation shows one line.
            std::array<Vec3,cvr::cyberware::Wire::Count> points=c.dynamics.point;
            if(!advance && cvr::cyberware::Finite(carry))for(auto& p:points)p=p+carry;
            std::array<Vec3,cvr::cyberware::Wire::Count> shown=points;
            if(Valid(c.wireSource)) {
                constexpr int n=cvr::cyberware::Wire::Count,lead=int(std::size(WireLead));
                bool finite=true;
                for(int k=0;k<lead;++k) {
                    const auto& p=WireLead[k];const int bone=p.hand?c.hand:c.wireSource;
                    const Vec3 offset{sign*p.x/131072.f,sign*p.y/131072.f,sign*p.z/131072.f};
                    shown[k]=world(bone)+Rotated(worldRot,Rotated(g_fkRot[bone],offset));
                    finite=finite && cvr::cyberware::Finite(shown[k]);
                }
                const float reach=cvr::cyberware::Length(shown[0]-anchor);
                if(!finite || reach<.005f || reach>.25f)shown=points;
                else {
                    const auto& point=points;
                    std::array<float,n> arc{};
                    for(int i=1;i<n;++i)arc[i]=arc[i-1]+cvr::cyberware::Length(point[i]-point[i-1]);
                    for(int k=lead,i=1;k<n;++k) {
                        const float s=arc[n-1]*float(k-lead)/float(n-1-lead);
                        while(i<n-1 && arc[i]<s)++i;
                        const float span=arc[i]-arc[i-1];
                        const float f=span>1e-6f?std::clamp((s-arc[i-1])/span,0.f,1.f):0.f;
                        shown[k]=point[i-1]+(point[i]-point[i-1])*f;
                    }
                }
            }
            for(int k=0;k<cvr::cyberware::Wire::Count;++k) {
                const int bone=c.wire[k];if(!Valid(bone)||!Valid(g_VRBoneParent[bone]))continue;
                const Vec3 model=Rotated(inverse,shown[k]-origin);
                const Vec3 a=shown[k?k-1:k],b=shown[k+1<cvr::cyberware::Wire::Count?k+1:k];
                Vec3 directionModel=Rotated(inverse,cvr::cyberware::Unit(b-a));
                const Vec3 nativeAxis=Rotated(g_fkRot[bone],c.wireAxis[k]);
                float from[3]={nativeAxis.x,nativeAxis.y,nativeAxis.z},to[3]={directionModel.x,directionModel.y,directionModel.z};
                float delta[4],rotation[4],position[3]={model.x,model.y,model.z};VRIK_QuatFromTo(from,to,delta);
                VRIK_QuatMul(delta,g_fkRot[bone],rotation);VRIK_QuatNorm(rotation);
                ModelPosition(buffer,bone,position);VRIK_WriteLocalRot(buffer,bone,g_fkRot[g_VRBoneParent[bone]],rotation);
                float* scale=reinterpret_cast<float*>(buffer+bone*48+32);scale[0]=scale[1]=scale[2]=1.f;
                std::copy_n(position,3,g_fkPos[bone]);std::copy_n(rotation,4,g_fkRot[bone]);
                output.points[side][k]=c.dynamics.point[k];
            }
        } else if(g_VRCyberwareFamily==2) {
            const auto& s=sides[side];int tip=-1;
            for(size_t i=0;i<std::size(cvr::vrik::MantisDeployment);++i)
                if(std::string_view(cvr::vrik::MantisDeployment[i].name)==(side?"r_blade05_JNT":"l_blade05_JNT"))tip=deploymentIndices[i];
            if(Valid(s.bladeJoint)&&Valid(tip)){output.count[side]=2;output.points[side][0]=world(s.bladeJoint);output.points[side][1]=world(tip);}
        } else if(g_VRCyberwareFamily==1 && Valid(c.knuckle)) {
            output.count[side]=2;output.points[side][0]=world(c.hand);output.points[side][1]=world(c.knuckle);
        } else if(g_VRCyberwareFamily==4 && side==0 && Valid(launcherAmmo)&&Valid(launcherMuzzle)&&Valid(launcherBase)) {
            output.count[side]=2;output.points[side][0]=world(launcherMuzzle);
            // The housing's +Z, not the barrel's: the fire mechanism and its marker sway
            // through the idle animation and carried the shot with them, and the cartridge
            // joint rotates during the action/reload. The housing lies along the forearm.
            const Vec3 direction=Rotated(worldRot,Rotated(g_fkRot[launcherBase],{0,0,1}));
            output.points[side][1]=output.points[side][0]+direction*.4f;
        }
    }
    if(advance){wireEpoch=epoch;wireStamp=now;wireOrigin=origin;wireOriginValid=true;}
    VRIK_ApplyWireTracks(tracks,trackCount);
    if(g_VRCyberwareFamily==3)VRIK_ComputeFK(buffer,VRIK_FKCount());
    std::lock_guard lock(frameMutex);frame=output;
}

bool cvr::cyberware::ReadFrame(Frame& out) {
    std::lock_guard lock(frameMutex);const uint64_t now=GetTickCount64();
    if(g_VRBind!=4 || !frame.family || frame.family!=g_VRCyberwareFamily || now<frame.stamp || now-frame.stamp>150)return false;
    out=frame;return true;
}
bool cvr::cyberware::LauncherLine(float* origin,float* direction) {
    // The rig keeps the launcher's bones while it is installed: selected is what makes the left trigger fire it.
    Frame value;
    if(SelectedKind()!=4 || cvr::anim::IsHeadAimWeaponActive() || !ReadFrame(value) || value.family!=4 || value.count[0]!=2)return false;
    const Vec3 start=value.points[0][0],end=value.points[0][1];
    const float d[3]={end.x-start.x,end.y-start.y,end.z-start.z};
    const float length=std::sqrt(d[0]*d[0]+d[1]*d[1]+d[2]*d[2]);
    if(!(length>.01f))return false;
    origin[0]=start.x;origin[1]=start.y;origin[2]=start.z;
    for(int k=0;k<3;++k)direction[k]=d[k]/length;
    return true;
}
float cvr::cyberware::WireDeployment(int side) {
    Frame value;
    return side>=0 && side<2 && ReadFrame(value) && value.family==3 ? value.deployment[side]:0.f;
}
void cvr::cyberware::SetWirePlanes(int side,uint32_t epoch,const std::array<Plane,Wire::Count>& planes) {
    if(side<0||side>1)return;
    std::lock_guard lock(frameMutex);
    if(frame.family!=3 || static_cast<uint32_t>(frame.epoch-epoch)>8u)return;
    collisionPlanes[side]=planes;collisionStamp[side]=GetTickCount64();
}
void cvr::cyberware::MusclePulse(int side,float duration) {
    if(side<0||side>1||!std::isfinite(duration))return;
    const auto milliseconds=static_cast<uint64_t>(std::clamp(duration,0.f,1.5f)*1000);
    musclePulse[side].store((GetTickCount64()<<16)|milliseconds,std::memory_order_release);
}
void cvr::cyberware::Select(int kind) {
    selectedKind.store(kind>=0&&kind<=5?kind:0,std::memory_order_relaxed);
    selectedStamp.store(GetTickCount64(),std::memory_order_release);
}
int cvr::cyberware::SelectedKind() {
    const auto stamp=selectedStamp.load(std::memory_order_acquire),now=GetTickCount64();
    return g_VRBind==4 && now>=stamp && now-stamp<250 ? selectedKind.load(std::memory_order_relaxed):0;
}
void cvr::cyberware::PublishGrips(bool left,bool right) {
    rawGrip[0].store(left,std::memory_order_relaxed);rawGrip[1].store(right,std::memory_order_relaxed);
}
void cvr::cyberware::CollisionInfo(float* values) {
    std::lock_guard lock(frameMutex);values[0]=values[1]=0;
    for(int side=0;side<2;++side)for(const auto& plane:collisionPlanes[side])if(plane.valid)values[side]+=1;
    const auto stamp=std::max(collisionStamp[0],collisionStamp[1]);
    values[2]=stamp?float(GetTickCount64()-stamp):-1.f;
    values[3]=float((Grip(0)?1:0)|(Grip(1)?2:0));
}
