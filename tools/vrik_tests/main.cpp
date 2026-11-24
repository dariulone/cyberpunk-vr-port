#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/CyberwareMuscles.hpp"
#include "Anim/ReloadPose.hpp"
#include "Anim/HandGestures.hpp"
#include "Anim/HandGesturePose.hpp"
#include "Anim/HandGestureCalibration.hpp"
#include "Anim/LadderFingerTuning.hpp"
#include "Runtimes/LadderClimbing.hpp"
#include "Anim/SkeletonPolicy.hpp"
#include "Anim/VehiclePosePolicy.hpp"
#include "test_state.hpp"
#include "Utils/XrMath.hpp"
#include "Camera/AnchorTranslation.hpp"
#include "Camera/NeckCameraMount.hpp"
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Runtimes/RoomscaleMovement.hpp"
#include <array>
#include <limits>
#include <cmath>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>
#include <thread>
#include <cstdlib>

extern "C" int CyberpunkVR_RestFingerCaptureReqR;
extern "C" int CyberpunkVR_RestFingerApplyR;
extern "C" int CyberpunkVR_VrikElbowPolicy;
extern "C" int CyberpunkVR_GestureMode[2];
extern char g_VRSmokeFingerName[32][48],g_VRSmokeFingerNameL[32][48];

namespace {
struct Bone { float p[4],q[4],s[4]; };
static_assert(sizeof(Bone)==48);
std::array<float,256> shared{};
std::vector<std::string> names;
std::string fixturePrefix;
void Check(bool ok,const char* what) { if(!ok)throw std::runtime_error(what); }
void Near(float a,float b,float tolerance,const char* what) { Check(std::isfinite(a)&&std::abs(a-b)<tolerance,what); }
template<class T> std::vector<T> Read(const char* name) {
    std::ifstream file(std::filesystem::path(VR_CHAIN_FIXTURE_DIR)/fixturePrefix/name,std::ios::binary|std::ios::ate);
    Check(bool(file),"fixture cannot be opened");
    const auto bytes=file.tellg();Check(bytes>0 && bytes%sizeof(T)==0,"fixture size mismatch");
    std::vector<T> data(static_cast<size_t>(bytes)/sizeof(T));file.seekg(0);
    file.read(reinterpret_cast<char*>(data.data()),bytes);Check(bool(file),"fixture read failed");return data;
}
void Setup(const char* prefix="") {
    fixturePrefix=prefix;names.clear();
    const auto parents=Read<int16_t>("player_parents.bin");
    Check(parents.size()==(fixturePrefix=="gorilla" ? 768:fixturePrefix=="mantis" ? 766:fixturePrefix=="launcher" ? 720:(fixturePrefix.empty() || fixturePrefix=="ladder" ? 619:620)),"captured rig size changed");
    for(size_t i=0;i<parents.size();++i)g_VRBoneParent[i]=parents[i];
    g_VRBoneCount=g_VRFKCount=static_cast<int>(parents.size());
    g_pSharedHands=shared.data();
    std::memset(shared.data(),0,sizeof(shared));
    shared[19]=1;shared[31]=0;
    std::ifstream nameFile(std::filesystem::path(VR_CHAIN_FIXTURE_DIR)/fixturePrefix/"player_names.txt");
    std::string line;std::vector<const char*> namePointers;
    while(std::getline(nameFile,line)) {
        if(!line.empty() && line.back()=='\r')line.pop_back();
        names.push_back(line.substr(line.find('\t')+1));
    }
    Check(names.size()==parents.size(),"name/parent fixture mismatch");
    g_VRSpineCount=0;
    for(size_t i=0;i<names.size();++i) {
        namePointers.push_back(names[i].c_str());
        if(cvr::vrik::IsPrimarySpine(names[i]) && g_VRSpineCount<8)
            g_VRSpineIdx[g_VRSpineCount++]=static_cast<int>(i);
    }
    // Reproduce a layout change after cyberware: these old slots are leg bones
    // in the current rig and must be replaced by a fresh name/topology resolve.
    g_VRForeTwistR[0]=171;g_VRForeTwistR[1]=172;g_VRForeTwistR[2]=173;
    g_VRForeTwistL[0]=157;g_VRForeTwistL[1]=158;g_VRForeTwistL[2]=159;
    const auto reference=Read<Bone>("reference_full.bin");
    VRIK_ConfigureRigPolicy(namePointers.data(),static_cast<int>(names.size()),
        reinterpret_cast<const uint8_t*>(reference.data()),static_cast<int>(reference.size()));
}
void QuatNear(const float* a,const float* b,const char* what) {
    float dot=0;for(int k=0;k<4;++k)dot+=a[k]*b[k];
    Near(std::abs(dot),1.0f,2e-5f,what);
}
void SolveBody(std::vector<Bone>& pose,const float* rotation=nullptr,const float* headDelta=nullptr,const float* orbit=nullptr) {
    VRIK_DampenTorsoWeaponPose(reinterpret_cast<uint8_t*>(pose.data()));
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),static_cast<int>(pose.size()));
    const float camera[3]={0,0,1.6f},id[4]={0,0,0,1},forward[3]={0,1,0};
    VRIK_PlaceBodyUnderHMD(reinterpret_cast<uint8_t*>(pose.data()),camera,rotation ? rotation : id,22,forward,headDelta,orbit);
}
void BodyCentreOrbit() {
    using namespace cvr::camera;
    const auto native=Read<Bone>("player_local.bin");
    auto neutral=native;SolveBody(neutral);
    std::vector<std::array<float,3>> positions(native.size());
    for(size_t i=0;i<native.size();++i)std::copy_n(g_fkPos[i],3,positions[i].begin());
    AnchorVector mount{};Check(ReadNeckCameraMount(&mount),"missing neutral mount");
    auto recipe=MakeAnchorRecipe({}, {}, mount,false,1,0,0);recipe.bodyOrbit=true;
    for(float angle:{-3.14159265f,-1.57079633f,-.2f,0.f,.7f,1.57079633f,3.14159265f}) {
        const auto offset=BodyOrbitModel(recipe,angle,0);const float orbit[3]={offset.x,offset.y,0};
        auto pose=native;SolveBody(pose,nullptr,nullptr,orbit);
        for(size_t i=0;i<native.size();++i) {
            if(!g_VRUpperOwned[i])Check(std::memcmp(&pose[i],&native[i],sizeof(Bone))==0,"orbit rewrote native leg animation");
            bool descendant=false;
            for(int p=int(i);p>=0;p=g_VRBoneParent[p])if(p==g_VRHipsIdx){descendant=true;break;}
            if(descendant)for(int k=0;k<3;++k)Near(g_fkPos[i][k],positions[i][k]+orbit[k],2e-5f,"orbit distorted a skeleton segment");
        }
        // Deliberate view turning and recenter may change tracking yaw too;
        // neither rotates the calibrated origin about the capsule.
        const auto view=ComposeViewTranslation({},recipe,angle*.4f,angle);
        Near(view.x,mount.x,1e-6f,"body yaw moved optical centre X");
        Near(view.y,mount.y,1e-6f,"body yaw moved optical centre Y");
        // A physical turn keeps the NECK in place, not the eye: the eye's travel round the neck is the tracked residual's.
        const auto pivot=BodyOrbitPivot(recipe);
        const auto world=RotateAnchorYaw({pivot.x+offset.x,pivot.y+offset.y,0},angle);
        Near(world.x,pivot.x,1e-6f,"body orbit lost the calibrated neck X");
        Near(world.y,pivot.y,1e-6f,"body orbit lost the calibrated neck Y");
        // A stick turn (the view's yaw turning with the body) keeps the EYE in place: the world turns about the eyes.
        const auto stick=BodyOrbitModel(recipe,angle,angle);
        const auto eyeWorld=RotateAnchorYaw({mount.x+stick.x,mount.y+stick.y,0},angle);
        Near(eyeWorld.x,mount.x,1e-6f,"a stick turn swung the body round the eyes X");
        Near(eyeWorld.y,mount.y,1e-6f,"a stick turn swung the body round the eyes Y");
        // Both at once: after a stick turn of .6 rad and a physical one of `angle`, the body's neck is the player's --
        // the eye less the lever along the view's forward.
        const auto both=BodyOrbitModel(recipe,.6f+angle,.6f);
        const auto neckWorld=RotateAnchorYaw({pivot.x+both.x,pivot.y+both.y,0},.6f+angle);
        const auto lever=RotateAnchorYaw(BodyOrbitLever(recipe),.6f);
        Near(neckWorld.x,mount.x-lever.x,1e-6f,"stick and physical turns put the body's neck off the player's X");
        Near(neckWorld.y,mount.y-lever.y,1e-6f,"stick and physical turns put the body's neck off the player's Y");
        // The player's neck from the roomscale ledger: the body's neck goes there, a physical turn keeps it there, and the
        // assumed neck given as the ledger's changes nothing.
        for(const AnchorVector rest:{AnchorVector{.012f,-.081f,0},AnchorVector{-.009f,-.104f,0}}) {
            const auto ledger=BodyOrbitModel(recipe,.6f+angle,.6f,&rest);
            const auto ledgerNeck=RotateAnchorYaw({pivot.x+ledger.x,pivot.y+ledger.y,0},.6f+angle);
            const auto restWorld=RotateAnchorYaw(rest,.6f);
            Near(ledgerNeck.x,mount.x+restWorld.x,1e-6f,"the body's neck left the ledger's neck X");
            Near(ledgerNeck.y,mount.y+restWorld.y,1e-6f,"the body's neck left the ledger's neck Y");
        }
        const AnchorVector assumed{0,-kOrbitEyeAheadOfNeck,0};
        const auto same=BodyOrbitModel(recipe,.6f+angle,.6f,&assumed);
        Near(same.x,both.x,1e-6f,"the assumed neck through the ledger moved the body X");
        Near(same.y,both.y,1e-6f,"the assumed neck through the ledger moved the body Y");
        // Far from the assumed neck is no neck: clamped to kOrbitRestNeckReach.
        const AnchorVector distant{1.f,-kOrbitEyeAheadOfNeck,0};
        const auto clamped=BodyOrbitModel(recipe,angle,0,&distant);
        const auto clampedNeck=RotateAnchorYaw({pivot.x+clamped.x,pivot.y+clamped.y,0},angle);
        Near(clampedNeck.x,pivot.x+kOrbitRestNeckReach,1e-6f,"a far ledger neck was not clamped");
        const auto first=pose;
        for(int repeat=0;repeat<100;++repeat)SolveBody(pose,nullptr,nullptr,orbit);
        for(size_t i=0;i<pose.size();++i)for(int k=0;k<3;++k)Near(pose[i].p[k],first[i].p[k],1e-5f,"orbit accumulated across repeated poses");
    }
    std::cout<<"PASS fixed optical centre and rigid avatar orbit, native local legs preserved\n";
}
void RigPolicy() {
    Check(g_VRSpineCount==4,"service nodes were selected as spine");
    Check(g_VRHeadReferenceValid,"reference head basis missing");
    for(int i=0;i<619;++i)if(g_VRUpperOwned[i])
        Check(!cvr::vrik::IsRigControl(names[i]),"control node owned by VRIK");
    Check(g_VRUpperOwned[19],"Neck1 omitted");
    Check(g_VRUpperOwned[2] && !g_VRUpperOwned[5],"pelvis/leg ownership incorrect");
    Check(g_VRShadowSource[257]==22 && g_VRShadowSource[259]==24,"shadow upper aliases unresolved");
    Check(g_VRShadowSource[236]==2 && g_VRShadowSource[238]<0,"shadow pelvis/leg ownership incorrect");
    for(int i=0;i<3;++i) {
        Check(g_VRForeTwistL[i]==162+i && g_VRForeTwistR[i]==165+i,"forearm helper resolves to stale/missing rig slot");
    }
    int n=0,shadow=0;
    for(int i=0;i<619;++i) { n+=g_VRUpperOwned[i];shadow+=g_VRShadowSource[i]>=0; }
    std::cout<<"primary_upper="<<n<<" shadow_aliases="<<shadow<<'\n';
}
void RigRebind() {
    const auto reference=Read<Bone>("reference_full.bin");
    auto changedNames=names;std::vector<const char*> ptrs;
    for(auto& name:changedNames)if(cvr::vrik::ForearmTwistSlot(name,true)>=0 ||
        cvr::vrik::ForearmTwistSlot(name,false)>=0)name="absent_helper";
    // A plausible name on a leg is still not a forearm deformation bone.
    changedNames[172]="r_forearmTwist01_JNT";
    for(auto& name:changedNames)ptrs.push_back(name.c_str());
    VRIK_ConfigureRigPolicy(ptrs.data(),619,reinterpret_cast<const uint8_t*>(reference.data()),619);
    for(int i=0;i<3;++i)Check(g_VRForeTwistL[i]<0 && g_VRForeTwistR[i]<0,"rig rebind retained old helper or accepted leg topology");
    ptrs.clear();for(auto& name:names)ptrs.push_back(name.c_str());
    VRIK_ConfigureRigPolicy(ptrs.data(),619,reinterpret_cast<const uint8_t*>(reference.data()),619);
    for(int i=0;i<3;++i)Check(g_VRForeTwistL[i]==162+i && g_VRForeTwistR[i]==165+i,"rig rebind did not recover current helpers");
}
void ShoulderReference() {
    auto pose=Read<Bone>("armed_local.bin");const auto reference=Read<Bone>("reference_full.bin");
    Check(std::abs(pose[14].p[2]-reference[14].p[2])>.1f,"armed fixture no longer reproduces displaced clavicle");
    for(int pass=0;pass<100;++pass) {
        // Start already armed, then feed previous VR writes back into the next
        // pass. No number of those poses should redefine a shoulder pivot.
        pose[14].p[2]+=.08f;pose[15].p[1]+=.07f;
        VRIK_PinGirdleTranslations(reinterpret_cast<uint8_t*>(pose.data()));
        for(int bone:{14,15,17,18,20,21,23,24})for(int k=0;k<3;++k)
            Near(pose[bone].p[k],reference[bone].p[k],1e-6f,"animated pose was captured as girdle reference");
        SolveBody(pose);
        VRIK_ResetShoulderReference(reinterpret_cast<uint8_t*>(pose.data()),17);
        VRIK_ResetShoulderReference(reinterpret_cast<uint8_t*>(pose.data()),18);
        for(int clavicle:{14,15})QuatNear(pose[clavicle].q,reference[clavicle].q,"clavicle aim starts at previous weapon/VR rotation");
        VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));
        for(int shadow:{249,250})for(int k=0;k<3;++k)
            Near(pose[shadow].p[k],reference[shadow].p[k],2e-5f,"shadow inherited displaced clavicle pivot");
    }
}
void WeaponStance(bool armedStart,bool bending=false) {
    Setup("weapon_stance");
    if(bending) {
        shared[114]=.7f;shared[115]=2;
        shared[89]=-cvr::body::HingeDrop(.7f);shared[90]=shared[89]-.08f;
        *reinterpret_cast<uint32_t*>(&shared[127])=2;RefreshHandsSnapshot();
    }
    const auto input=Read<float>("solve_inputs.bin");
    Check(input.size()==21,"stance inputs changed");
    const auto unarmed=Read<Bone>("unarmed-2.bin"),armed=Read<Bone>("shotgun-2.bin");
    std::vector<std::array<float,7>> expected;
    auto relevant=[](int i) {
        const int source=g_VRShadowSource[i]>=0 ? g_VRShadowSource[i]:i;
        return (source>=13 && source<=24) || names[source].find("SHL_")!=std::string::npos ||
            names[source].find("deltoid_")!=std::string::npos || names[source].find("scapula_")!=std::string::npos ||
            names[source].find("trapezius_")!=std::string::npos || names[source].find("Wrist_")!=std::string::npos;
    };
    float maxPosition=0,maxRotation=0,shoulderDelta=0;
    for(int pass=0;pass<12;++pass) {
        const bool useArmed=(pass%2==0)==armedStart;
        auto pose=pass<2 ? (useArmed ? armed:unarmed):Read<Bone>(useArmed ? "shotgun-3.bin":"unarmed-3.bin");
        // A bent pelvis also satisfies native foot contacts. Hold the lower
        // stance constant here to isolate weapon UPPER animation; independent
        // contact/gait preservation is checked by BodyBendGeometry.
        if(bending)for(int leg:{5,6,8,9,11,12})pose[leg]=unarmed[leg];
        const auto native=pose;auto* buf=reinterpret_cast<uint8_t*>(pose.data());
        VRIK_DampenTorsoWeaponPose(buf);VRIK_PinGirdleTranslations(buf);VRIK_ComputeFK(buf,620);
        float right[3],up[3],forward[3];
        Check(VRIK_BodyAxesFromRig(right,up,forward),"missing pre-placement arm frame");
        VRIK_PlaceBodyUnderHMD(buf,input.data(),input.data()+3,22,forward);
        Check(VRIK_BodyAxesFromRig(right,up,forward),"missing post-placement arm frame");
        for(bool left:{false,true}) {
            const int upper=left ? 17:18,fore=left ? 20:21,hand=left ? 23:24,side=left ? 0:1;
            float anchor[3],joint[3];std::copy_n(g_fkPos[16],3,anchor);
            VRIK_AnchorShoulder(buf,upper,anchor,left,right,up,.14f,joint);
            VRIK_SolveArm(buf,upper,fore,hand,input.data()+7+side*3,input.data()+13+side*4,right,up,forward,0,1,left,false);
            VRIK_ComputeFK(buf,620);
        }
        VRIK_SyncShadowUpper(buf);VRIK_ComputeFK(buf,620);
        for(int i=0;i<620;++i) {
            const bool bendLeg=bending && (names[i]=="LeftUpLeg" || names[i]=="RightUpLeg" ||
                names[i]=="LeftLeg" || names[i]=="RightLeg" || names[i]=="LeftFoot" || names[i]=="RightFoot" ||
                names[i]=="shadow_LeftUpLeg" || names[i]=="shadow_RightUpLeg" || names[i]=="shadow_LeftLeg" ||
                names[i]=="shadow_RightLeg" || names[i]=="shadow_LeftFoot" || names[i]=="shadow_RightFoot");
            if(!g_VRUpperOwned[i] && g_VRShadowSource[i]<0 && !bendLeg)
                Check(std::memcmp(&pose[i],&native[i],48)==0,"stance solve overwrote native leg/control animation");
            Check(std::memcmp(pose[i].s,native[i].s,16)==0,"stance solve changed skin scale");
        }
        if(expected.empty()) {
            expected.resize(620);
            for(int i=0;i<620;++i) {
                std::copy_n(g_fkPos[i],3,expected[i].begin());
                std::copy_n(g_fkRot[i],4,expected[i].begin()+3);
            }
        } else for(int i=0;i<620;++i)if(relevant(i)) {
            float distance=0,dot=0;
            for(int k=0;k<3;++k) {
                const float d=g_fkPos[i][k]-expected[i][k];distance+=d*d;
            }
            distance=std::sqrt(distance);maxPosition=std::max(maxPosition,distance);
            if(i==17 || i==18)shoulderDelta=std::max(shoulderDelta,distance);
            for(int k=0;k<4;++k)dot+=g_fkRot[i][k]*expected[i][k+3];
            for(int k=0;k<4;++k)maxRotation=std::max(maxRotation,
                std::abs(g_fkRot[i][k]-(dot<0 ? -1:1)*expected[i][k+3]));
        }
        // Replay on an independently evaluated native weapon pose must retain
        // the solved upper body and its shadow aliases in the same model frame.
        std::vector<std::array<float,3>> cached(620);
        for(int i=0;i<620;++i)std::copy_n(g_fkPos[i],3,cached[i].begin());
        VRIK_CaptureModelCache();auto replay=useArmed ? unarmed:armed;
        VRIK_ReplayModelCache(reinterpret_cast<uint8_t*>(replay.data()));
        VRIK_ComputeFK(reinterpret_cast<uint8_t*>(replay.data()),620);
        for(int i=0;i<620;++i)if(relevant(i)) {
            Check(std::isfinite(g_fkPos[i][0]),"nonfinite replay");
            for(int k=0;k<3;++k)Near(g_fkPos[i][k],cached[i][k],1e-4f,"stance replay changed cached position");
        }
    }
    std::cout<<"shoulder_stance_delta_mm="<<shoulderDelta*1000
        <<" upper_and_helpers_delta_mm="<<maxPosition*1000<<" rotation_component_delta="<<maxRotation<<'\n';
    Check(maxPosition<.00025f,"weapon animation changes solved upper-body positions");
    Check(maxRotation<2e-5f,"weapon animation changes solved upper-body rotations");
}
void BodyArmFrame() {
    Setup("weapon_stance");const auto native=Read<Bone>("shotgun-2.bin");
    const auto reference=Read<Bone>("reference_full.bin");
    const float expected[3][3]={{1,0,0},{0,0,1},{0,1,0}};
    for(float angle:{-1.2f,0.0f,1.4f}) {
        auto pose=native;float axes[3][3];
        const float head[4]={std::sin(angle*.5f),0,0,std::cos(angle*.5f)};
        SolveBody(pose,head);
        Check(VRIK_BodyAxesFromRig(axes[0],axes[1],axes[2]),"head pose invalidated body frame");
        for(int i=0;i<3;++i)for(int k=0;k<3;++k)
            Near(axes[i][k],expected[i][k],1e-5f,"head pitch rotates shoulder frame");
        // A rotated body must still rotate the arm frame. Returning a fixed
        // identity basis would pass the weapon comparison but fail this test.
        const float turn[4]={0,0,std::sin(angle*.5f),std::cos(angle*.5f)};
        VRIK_QuatMul(turn,reference[2].q,pose[2].q);
        VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),620);
        Check(VRIK_BodyAxesFromRig(axes[0],axes[1],axes[2]),"body turn invalidated arm frame");
        for(int i=0;i<3;++i) {
            float turned[3];VRIK_QuatRotateVec(turn,expected[i],turned);
            for(int k=0;k<3;++k)Near(axes[i][k],turned[k],1e-5f,"arm frame does not follow body turn");
        }
    }
    float right[3]={7,8,9},up[3]={7,8,9},forward[3]={7,8,9};
    g_VRHipsIdx=g_VRBoneCount;
    Check(!VRIK_BodyAxesFromRig(right,up,forward),"invalid pelvis accepted as body frame");
    g_VRHipsIdx=2;g_fkRot[2][0]=std::numeric_limits<float>::quiet_NaN();
    Check(!VRIK_BodyAxesFromRig(right,up,forward),"nonfinite pelvis accepted as body frame");
    Check(right[0]==7 && up[1]==8 && forward[2]==9,"failed frame partially overwrote caller axes");
}
void BodyBendGeometry() {
    Setup("weapon_stance");const auto native=Read<Bone>("unarmed-2.bin");auto pose=native;
    SolveBody(pose);
    const auto standing=pose;
    const float hipsZ=g_fkPos[2][2],hipsY=g_fkPos[2][1],headY=g_fkPos[22][1];
    float inverse[4],backMarker[3];VRIK_QuatConj(g_fkRot[2],inverse);
    const float modelBack[3]={0,-.1f,0};VRIK_QuatRotateVec(inverse,modelBack,backMarker);
    const int legs[6]={6,9,12,5,8,11};
    float feet[2][3],rot[2][4];
    for(int side=0;side<2;++side) { std::copy_n(g_fkPos[legs[side*3+2]],3,feet[side]);std::copy_n(g_fkRot[legs[side*3+2]],4,rot[side]); }
    auto publish=[&](float angle) {
        shared[114]=angle;shared[115]=2;
        shared[89]=-cvr::body::HingeDrop(angle);shared[90]=shared[89]-.08f;
        auto& seq=*reinterpret_cast<uint32_t*>(&shared[127]);seq+=2;RefreshHandsSnapshot();
    };
    publish(.7f);const float head[4]={-std::sin(.35f),0,0,std::cos(.35f)};
    // the body's bend angle eases in (0.06 s) -- a solve is a 90 Hz frame here: a second of them is the settled bend
    for(int frame=0;frame<90;++frame)SolveBody(pose,head);
    std::cout<<"bend_pelvis_delta_mm="<<(g_fkPos[2][1]-hipsY)*1000<<','<<(g_fkPos[2][2]-hipsZ)*1000
        <<" head_forward_mm="<<(g_fkPos[22][1]-headY)*1000<<'\n';
    Check(g_fkPos[2][1]<hipsY-.03f,"bending did not move pelvis back");
    float raisedBack[3];VRIK_QuatRotateVec(g_fkRot[2],backMarker,raisedBack);
    Check(g_fkPos[2][2]>=hipsZ-.001f && g_fkPos[2][2]+raisedBack[2]>hipsZ+.015f,
        "bending did not raise the rear of the pelvis within planted leg reach");
    Check(g_fkPos[22][1]>headY+.20f,"torso did not hinge forward");
    for(int side=0;side<2;++side) {
        const int* j=legs+side*3;
        for(int k=0;k<3;++k)Near(g_fkPos[j[2]][k],feet[side][k],1e-4f,"bending moved native foot contact");
        QuatNear(g_fkRot[j[2]],rot[side],"bending tilted planted foot");
        float up[3],lo[3];for(int k=0;k<3;++k) { up[k]=g_fkPos[j[1]][k]-g_fkPos[j[0]][k];lo[k]=g_fkPos[j[2]][k]-g_fkPos[j[1]][k]; }
        VRIK_Norm3(up);VRIK_Norm3(lo);
        Check(VRIK_Dot3(up,lo)<std::cos(.1f),"knees remain locked while bending");
        for(int bone:{j[0],j[1],j[2]})for(int k=0;k<3;++k)Near(pose[bone].p[k],native[bone].p[k],1e-6f,"bend stretched a leg segment");
    }
    const auto bent=pose;
    for(int repeat=0;repeat<50;++repeat)SolveBody(pose,head);
    for(int i=0;i<620;++i)for(int k=0;k<3;++k)Near(pose[i].p[k],bent[i].p[k],1e-5f,"bending accumulates translations");
    for(int bone:legs)QuatNear(pose[bone].q,bent[bone].q,"repeated bend solve changes native stance");
    VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));
    for(int source:legs) {
        const auto it=std::find(names.begin(),names.end(),"shadow_"+names[source]);Check(it!=names.end(),"missing captured shadow leg");
        const int shadow=static_cast<int>(it-names.begin());
        for(int k=0;k<3;++k)Near(g_fkPos[shadow][k],g_fkPos[source][k],2e-5f,"shadow leg ignores bend");
    }
    std::array<std::array<float,3>,6> cached{};
    for(int i=0;i<6;++i)std::copy_n(g_fkPos[legs[i]],3,cached[i].begin());
    VRIK_CaptureModelCache();auto replay=native;
    VRIK_ReplayModelCache(reinterpret_cast<uint8_t*>(replay.data()));
    for(int i=0;i<6;++i)for(int k=0;k<3;++k)Near(g_fkPos[legs[i]][k],cached[i][k],2e-5f,"pose replay lost bent knees");
    // the bend eases out and its limit fades over 0.35 s (a solve is a 90 Hz frame here): the legs are native after
    publish(0);for(int frame=0;frame<200;++frame)SolveBody(pose);
    for(int bone:legs)for(int k=0;k<12;++k)Near(reinterpret_cast<float*>(&pose[bone])[k],reinterpret_cast<const float*>(&native[bone])[k],1e-5f,"standing did not release native legs");
    VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));
    for(int source:legs) {
        const int shadow=static_cast<int>(std::find(names.begin(),names.end(),"shadow_"+names[source])-names.begin());
        for(int k=0;k<12;++k)Near(reinterpret_cast<float*>(&pose[shadow])[k],reinterpret_cast<const float*>(&native[shadow])[k],1e-5f,"standing left the shadow knees bent");
    }
    VRIK_CaptureModelCache();
    for(int i=0;i<g_solveCacheN;++i)for(int bone:legs)Check(g_solveCacheIdx[i]!=bone,"neutral cache still owns legs");
    cvr::camera::AnchorVector mount{};Check(cvr::camera::ReadNeckCameraMount(&mount),"bend lost neutral camera calibration");
    // A fresh native lower-body evaluation replaces the retained input. On
    // release we must return that new gait, not the first standing snapshot.
    const auto newNative=Read<Bone>("shotgun-3.bin");
    for(int bone:legs)pose[bone]=newNative[bone];
    publish(.5f);SolveBody(pose,head);publish(0);for(int frame=0;frame<200;++frame)SolveBody(pose);
    for(int bone:legs)for(int k=0;k<12;++k)Near(reinterpret_cast<float*>(&pose[bone])[k],reinterpret_cast<const float*>(&newNative[bone])[k],1e-5f,"bend release restored a stale native gait");
}
void FloorReachGeometry() {
    Setup("weapon_stance");const auto native=Read<Bone>("unarmed-2.bin");auto pose=native;
    auto reference=Read<Bone>("reference_full.bin");VRIK_ComputeFK(reinterpret_cast<uint8_t*>(reference.data()),620);
    float inverseChest[4];VRIK_QuatConj(g_fkRot[13],inverseChest);
    SolveBody(pose);float feet[2][3];
    std::copy_n(g_fkPos[11],3,feet[0]);std::copy_n(g_fkPos[12],3,feet[1]);
    const float standingHip=g_fkPos[2][2];
    const float head[4]={-std::sin(65*.01745329252f*.5f),0,0,std::cos(65*.01745329252f*.5f)};
    auto publish=[&](float bend,float height) {
        shared[114]=bend;shared[115]=2;shared[89]=height;shared[90]=height-.08f;
        auto& seq=*reinterpret_cast<uint32_t*>(&shared[127]);seq+=2;RefreshHandsSnapshot();
    };
    publish(cvr::body::MaxBend,-.8f);
    for(int pass=0;pass<60;++pass)SolveBody(pose,head);
    float delta[4],front[3];VRIK_QuatMul(g_fkRot[13],inverseChest,delta);
    const float axis[3]={0,1,0};VRIK_QuatRotateVec(delta,axis,front);
    Check(front[2]<-.93f,"floor reach did not bring chest toward the floor");
    Check(g_fkPos[2][2]<standingHip-.1f,"deep physical descent did not move into the knees");
    for(int side=0;side<2;++side)for(int k=0;k<3;++k)
        Near(g_fkPos[side ? 12:11][k],feet[side][k],1e-4f,"deep reach pushed foot below native ground contact");
    // Both hands can reach a low object, with normal arm-length caps intact.
    float right[3],up[3],forward[3];Check(VRIK_BodyAxesFromRig(right,up,forward),"missing bent body frame");
    for(bool left:{false,true}) {
        const int upper=left ? 17:18,fore=left ? 20:21,hand=left ? 23:24;
        const float target[3]={left ? -.2f:.2f,.45f,.15f},rotation[4]={0,0,0,1};
        float anchor[3],joint[3];std::copy_n(g_fkPos[16],3,anchor);
        VRIK_AnchorShoulder(reinterpret_cast<uint8_t*>(pose.data()),upper,anchor,left,right,up,.14f,joint);
        VRIK_SolveArm(reinterpret_cast<uint8_t*>(pose.data()),upper,fore,hand,target,rotation,right,up,forward,0,1,left,false);
        VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),620);
        float error=0;for(int k=0;k<3;++k)error+=(g_fkPos[hand][k]-target[k])*(g_fkPos[hand][k]-target[k]);
        std::cout<<(left ? "left":"right")<<"_floor_target_error_mm="<<std::sqrt(error)*1000<<'\n';
        Check(error<1e-6f,"hand cannot reach the low floor object");
    }
    publish(0,0);
    for(int pass=0;pass<70;++pass)SolveBody(pose);
    Near(g_fkPos[2][2],standingHip,1e-5f,"standing retains the final crouch EMA deadband");
    std::cout<<"floor_chest_down="<<-front[2]<<'\n';
}
// The bend limit fades by this clock: one 90 Hz frame per body solve (g_VRBendLimitClockUs).
int64_t g_testClockUs=0,g_testClockStepUs=11111;
int64_t TestClock() { return g_testClockUs+=g_testClockStepUs; }
void HeadBoundBend(bool bind) {
    Setup("weapon_stance");const auto reference=Read<Bone>("reference_full.bin");
    auto find=[](const char* a,const char* b) {
        for(const char* preferred:{b,a})for(int i=0;i<static_cast<int>(names.size());++i)if(names[i]==preferred)return i;
        throw std::runtime_error("missing eye fixture");
    };
    const int le=find("LeftEye","l_J_eye_JNT"),re=find("RightEye","r_J_eye_JNT");
    auto neutral=reference;SolveBody(neutral);float neutralEye[3];
    for(int k=0;k<3;++k)neutralEye[k]=.5f*(g_fkPos[le][k]+g_fkPos[re][k]);
    // The limit fades in and out over 0.35 s (32 frames at 90 Hz): it binds once it is in.
    const int fadeFrames=33;
    float maxLead=0,maxAbove=0,maxDeepError=0;int constrained=0;
    float pelvisStep[2]{};
    // pass 0: a clock of a second a solve, the limit at once (as it was); pass 1: 90 Hz, faded
    for(int pass=0;pass<2;++pass) {
    g_testClockStepUs=pass ? 11111:1000000;
    for(float forwardFraction:{0.0f,.25f,1.0f}) {
        cvr::body::BendTracker tracker;
        // the bend's geometry under the head, not the reach's handover to the squat (BendTracker::Update; tested in
        // roomscale floor_reach_tracking): the deepest squat depth, and a straight-down reach kept within a reach
        tracker.SetSquatDepth(.55f);
        int bentFor=0;float lastPelvis=std::numeric_limits<float>::quiet_NaN();
        for(int frame=0;frame<=200;++frame) {
            // Include a deep reach hold: the accepted 24cm bend threshold
            // intentionally leaves more of the approach upright now. (The 20 cm below the hinge go with the neck's
            // forward travel: straight down they are a squat's depth, 0.56 m, where the reach hands over to the squat.)
            const float t=frame<=80 ? float(frame)/80 : (frame<=120 ? 1.0f:float(200-frame)/80);
            const float a=1.134464f*t,q[4]={-std::sin(a*.5f),0,0,std::cos(a*.5f)};
            const float optical[3]={0,cvr::body::EyeAboveNeck,-cvr::body::EyeAheadOfNeck};float rotated[3];VRIK_QuatRotateVec(q,optical,rotated);
            const float xr[3]={0,-.63f*(1-std::cos(a))-(forwardFraction>=1 ? .2f*t:0.0f)+rotated[1]-cvr::body::EyeAboveNeck,
                -.63f*std::sin(a)*forwardFraction+rotated[2]+cvr::body::EyeAheadOfNeck};
            const auto bend=tracker.Update({xr[0],xr[1],xr[2]},{q[0],q[1],q[2],q[3]},1);
            shared[114]=bend.angle;shared[115]=2;shared[89]=xr[1];shared[90]=xr[1]-rotated[1];
            auto& seq=*reinterpret_cast<uint32_t*>(&shared[127]);seq+=2;RefreshHandsSnapshot();
            const float delta[3]={xr[0],-xr[2],xr[1]};auto pose=reference;
            SolveBody(pose,q,bind ? delta:nullptr);
            const float pelvis=g_fkPos[g_VRHipsIdx][2];
            // the bend's edges -- coming in and going out, under 6 degrees -- not the fast hinge's own geometry between
            if(std::isfinite(lastPelvis) && bend.angle<.1f)
                pelvisStep[pass]=std::max(pelvisStep[pass],std::abs(pelvis-lastPelvis));
            lastPelvis=pelvis;
            bentFor=bend.angle>1e-5f ? bentFor+1:0;
            if(pass==1 && bentFor>fadeFrames) {
                ++constrained;
                float error[3];for(int k=0;k<3;++k)error[k]=.5f*(g_fkPos[le][k]+g_fkPos[re][k])-neutralEye[k]-delta[k];
                maxLead=std::max(maxLead,error[1]);maxAbove=std::max(maxAbove,error[2]);
                if(t>.8f)maxDeepError=std::max(maxDeepError,std::sqrt(VRIK_Dot3(error,error)));
            }
        }
    }
    }
    g_testClockStepUs=11111;
    std::cout<<"head_anchor_forward_lead_mm="<<maxLead*1000<<" above_mm="<<maxAbove*1000
        <<" deep_error_mm="<<maxDeepError*1000<<" pelvis_frame_step_mm instant="<<pelvisStep[0]*1000
        <<" faded="<<pelvisStep[1]*1000<<'\n';
    Check(constrained>200,"head anchor trajectory failed to exercise bending");
    if(bind) {
        // the jump between bent and upright (2026-11-06): at once, the limit moved the pelvis by the whole gap in a frame
        Check(pelvisStep[1]<.015f,"the bend's limit still moves the pelvis in a jump");
        Check(pelvisStep[1]<pelvisStep[0]*.3f,"fading the bend's limit did not soften its edge");
    }
    Check(maxLead<=.0101f,"bending upper body overtakes the HMD forward position");
    Check(maxAbove<=.0151f,"bending upper body stays above the lowered HMD");
    Check(maxDeepError<.001f,"deep bend is not built under the paired HMD");
}

void GirdleReach() {
    Setup("weapon_stance");const auto native=Read<Bone>("unarmed-2.bin"),reference=Read<Bone>("reference_full.bin");
    const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0},handQ[4]={0,0,0,1};
    for(bool left:{false,true}) {
        const int side=left ? 1:0,upper=left ? 17:18,fore=left ? 20:21,hand=left ? 23:24,clav=left ? 14:15;
        const std::string scapName=left ? "l_scapula_A_bot_out_JNT":"r_scapula_A_bot_out_JNT";
        const int scap=static_cast<int>(std::find(names.begin(),names.end(),scapName)-names.begin());
        Check(scap<620,"scapula fixture missing");
        const float rest=std::sqrt(VRIK_Dot3(reference[fore].p,reference[fore].p))+std::sqrt(VRIK_Dot3(reference[hand].p,reference[hand].p));
        auto pose=native;std::array<float,3> firstFar{};
        for(int pass=0;pass<30;++pass) {
            const bool farTarget=pass>0 && pass<29;
            SolveBody(pose);auto* buf=reinterpret_cast<uint8_t*>(pose.data());VRIK_PinGirdleTranslations(buf);VRIK_ComputeFK(buf,620);
            float anchor[3],joint[3],pivot[3];std::copy_n(g_fkPos[16],3,anchor);
            VRIK_AnchorShoulder(buf,upper,anchor,left,right,up,.14f,joint);std::copy_n(g_fkPos[clav],3,pivot);
            const float target[3]={joint[0],joint[1]+rest*(farTarget ? 1.16f:.5f),joint[2]};
            VRIK_SolveArm(buf,upper,fore,hand,target,handQ,right,up,forward,0,1,left,false);VRIK_ComputeFK(buf,620);
            const float* diagnostic=g_VRIKGirdleReach[side];
            for(int k=0;k<3;++k)Near(g_fkPos[clav][k],pivot[k],1e-5f,"reach displaced sternum/clavicle pivot");
            if(farTarget) {
                Check(diagnostic[0]>.99f && diagnostic[1]>.5f && diagnostic[1]<=12.01f,"reach did not use bounded clavicle rotation");
                Check(diagnostic[2]>.02f && diagnostic[2]<.06f,"shoulder reach is absent or excessive");
                float bladeLocalDelta[3];for(int k=0;k<3;++k)bladeLocalDelta[k]=pose[scap].p[k]-reference[scap].p[k];
                const float blade=std::sqrt(VRIK_Dot3(bladeLocalDelta,bladeLocalDelta));
                Check(blade>.005f && blade<.04f,"scapula did not participate within the bounded glide");
                if(pass==1)std::copy_n(g_fkPos[upper],3,firstFar.begin());
                else for(int k=0;k<3;++k)Near(g_fkPos[upper][k],firstFar[k],1e-5f,"girdle reach accumulates across solves");
                const int skin=left ? 159:161;
                Near(std::abs(pose[skin].p[0]/pose[fore].p[0]),std::abs(reference[skin].p[0]/reference[fore].p[0]),1e-4f,"upper-arm skin does not share segment stretch");
            } else {
                Near(diagnostic[2],0,1e-5f,"near target retained the old shoulder advance");
                for(int k=0;k<3;++k)Near(pose[scap].p[k],reference[scap].p[k],1e-5f,"near target retained scapula glide");
            }
            for(int bone:{upper,fore,hand,scap})Check(std::memcmp(pose[bone].s,native[bone].s,16)==0,"reach changed skin scale");
            if(pass==1) {
                VRIK_SyncShadowUpper(buf);const std::array<int,3> tracked{upper,scap,left ? 159:161};
                float captured[3][3];for(int i=0;i<3;++i)std::copy_n(g_fkPos[tracked[i]],3,captured[i]);
                VRIK_CaptureModelCache();auto replay=native;VRIK_ReplayModelCache(reinterpret_cast<uint8_t*>(replay.data()));
                for(int i=0;i<3;++i)for(int k=0;k<3;++k)Near(g_fkPos[tracked[i]][k],captured[i][k],2e-5f,"replay lost shoulder or scapula reach");
                VRIK_ComputeFK(buf,620);
            }
        }
    }
}

void OverheadReach() {
    Setup("weapon_stance");
    const auto native=Read<Bone>("unarmed-2.bin"),reference=Read<Bone>("reference_full.bin");
    const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0},q[4]={0,0,0,1};
    float worst=0;
    for(bool left:{false,true})for(float elevation:{0.f,30.f,50.f,70.f,85.f,90.f,-70.f}) {
        auto pose=native;SolveBody(pose);auto* buf=reinterpret_cast<uint8_t*>(pose.data());
        const int upper=left?17:18,fore=left?20:21,hand=left?23:24,clav=left?14:15;
        VRIK_PinGirdleTranslations(buf);VRIK_ComputeFK(buf,620);
        float anchor[3],joint[3],pivot[3];std::copy_n(g_fkPos[16],3,anchor);
        VRIK_AnchorShoulder(buf,upper,anchor,left,right,up,.14f,joint);
        std::copy_n(g_fkPos[clav],3,pivot);
        const float length=std::sqrt(VRIK_Dot3(reference[fore].p,reference[fore].p))+std::sqrt(VRIK_Dot3(reference[hand].p,reference[hand].p));
        const float angle=elevation*.01745329252f;
        // A lifted shoulder adds reach above the head while arm length stays fixed.
        const float distance=length*1.14f+.065f*std::max(0.f,std::sin(angle));
        const float target[3]={joint[0],joint[1]+std::cos(angle)*distance,joint[2]+std::sin(angle)*distance};
        for(int pass=0;pass<12;++pass) {
            VRIK_SolveArm(buf,upper,fore,hand,target,q,right,up,forward,0,1,left,false);
            VRIK_ComputeFK(buf,620);
            float error[3];for(int k=0;k<3;++k)error[k]=g_fkPos[hand][k]-target[k];
            worst=std::max(worst,std::sqrt(VRIK_Dot3(error,error)));
            for(int k=0;k<3;++k)Near(g_fkPos[clav][k],pivot[k],1e-5f,"overhead reach moved clavicle pivot");
            for(int bone:{fore,hand}) {
                const float scale=std::sqrt(VRIK_Dot3(pose[bone].p,pose[bone].p)/VRIK_Dot3(reference[bone].p,reference[bone].p));
                Check(scale<=1.1501f,"overhead reach stretched the arm beyond existing limits");
            }
        }
    }
    std::cout<<"overhead_worst_wrist_error_mm="<<worst*1000<<'\n';
    Check(worst<.002f,"raised wrist stops before the controller");
}

void CyberwarePose(const char* rig="gorilla") {
    Setup(rig);const auto native=Read<Bone>("player_local.bin");auto pose=native;
    auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
    Check(VRIK_UsesNativeCyberware(),"cyberware rig not detected");
    Check(!VRIK_IsNativeCyberwareConsumer(VRIK_PlayerBaseGraph),"VRIK skipped the primary pose producer");
    for(bool left:{false,true}) {
        const int upper=left?17:18,fore=left?20:21,hand=left?23:24;
        const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0};
        auto first=native;SolveBody(first);
        float target[3],rotation[4];std::copy_n(g_fkPos[hand],3,target);std::copy_n(g_fkRot[hand],4,rotation);
        VRIK_SolveArm(reinterpret_cast<uint8_t*>(first.data()),upper,fore,hand,target,rotation,right,up,forward,0,1,left,false);
        VRIK_ComputeFK(reinterpret_cast<uint8_t*>(first.data()),g_VRBoneCount);
        float axis[3],before[4];std::copy_n(g_fkRot[fore],4,before);
        for(int k=0;k<3;++k)axis[k]=g_fkPos[hand][k]-g_fkPos[fore][k];VRIK_Norm3(axis);
        const float roll[4]={axis[0]*.70710678f,axis[1]*.70710678f,axis[2]*.70710678f,.70710678f};
        float turned[4];VRIK_QuatMul(roll,rotation,turned);
        auto second=native;SolveBody(second);
        VRIK_SolveArm(reinterpret_cast<uint8_t*>(second.data()),upper,fore,hand,target,turned,right,up,forward,0,1,left,false);
        VRIK_ComputeFK(reinterpret_cast<uint8_t*>(second.data()),g_VRBoneCount);
        QuatNear(g_fkRot[hand],turned,"cyberware roll changed the tracked wrist");
        float dot=0;for(int k=0;k<4;++k)dot+=before[k]*g_fkRot[fore][k];
        const float degrees=2*std::acos(std::clamp(std::abs(dot),0.0f,1.0f))*57.2957795f;
        std::cout<<rig<<(left?" left":" right")<<" forearm_roll_degrees="<<degrees<<'\n';
        // Combat now deliberately has a different elbow plane from relaxed
        // ordinary hands; compare the share of wrist roll, not full orientation.
        Check(degrees>20.f && degrees<26.f,"cyberware stopped sharing approximately one quarter of wrist roll");
    }
    SolveBody(pose);
    const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0},q[4]={0,.70710678f,0,.70710678f};
    for(bool left:{false,true}) {
        const int upper=left?17:18,fore=left?20:21,hand=left?23:24;
        const float target[3]={left?-.24f:.24f,.48f,1.65f};
        VRIK_SolveArm(buffer,upper,fore,hand,target,q,right,up,forward,0,1,left,false);
    }
    VRIK_ComputeFK(buffer,g_VRBoneCount);VRIK_CaptureModelCache();
    const char* marker=fixturePrefix=="mantis" ? "l_wrist01_GRP":fixturePrefix=="launcher" ? "LeftHand_setup":"l_plate_up_a";
    const int bone=int(std::find(names.begin(),names.end(),marker)-names.begin());
    Check(bone<g_VRBoneCount,"missing native deformation marker");
    // Native graph results arrive AFTER the primary IK producer. They must
    // survive the dispatch intact, including a fresh grip and deployment pose.
    for(uint64_t graph:{0xa3e7441ba96a1b41ull,0x16320cecc15a325cull,0xebb9897efd33f895ull,0x1234ull}) {
        pose[bone].p[1]+=.013f;pose[bone].q[0]=.2f;
        pose[45].q[1]=.3f; // authored left thumb grip (wire/cyberware)
        const auto expected=pose;
        if(!VRIK_IsNativeCyberwareConsumer(graph))VRIK_ReplaySolveCache(buffer);
        Check(!std::memcmp(pose.data(),expected.data(),pose.size()*sizeof(Bone)),"VRIK replay overwrote a downstream cyberware graph");
    }
    Setup("weapon_stance");
    Check(!VRIK_UsesNativeCyberware(),"cyberware policy survived a rig replacement");
    Check(!VRIK_IsNativeCyberwareConsumer(0x16320cecc15a325cull),"ordinary weapons lost their existing replay path");
    std::cout<<"native_cyberware_consumers_preserved=true normal_rig_reset=true\n";
}

void MantisWrist() {
    Setup("mantis");g_VRBind=4;
    const auto native=Read<Bone>("player_local.bin");
    const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0};
    auto find=[](const std::string& name){return int(std::find(names.begin(),names.end(),name)-names.begin());};
    auto reference=Read<Bone>("reference_full.bin");
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(reference.data()),int(reference.size()));
    float localDorsal[2][3]{},neutralHand[2][4]{};
    for(int side=0;side<2;++side) {
        const std::string h=side?"Right":"Left";const int hand=find(h+"Hand");
        float a[3],b[3],normal[3],inverse[4];
        for(int k=0;k<3;++k){a[k]=g_fkPos[find(h+"HandIndex1")][k]-g_fkPos[hand][k];b[k]=g_fkPos[find(h+"HandPinky1")][k]-g_fkPos[hand][k];}
        VRIK_Cross3(a,b,normal);if(side)for(float& v:normal)v=-v;VRIK_Norm3(normal);
        VRIK_QuatConj(g_fkRot[hand],inverse);VRIK_QuatRotateVec(inverse,normal,localDorsal[side]);
        float aim[3],turn[4];for(int k=0;k<3;++k)aim[k]=g_fkPos[hand][k]-g_fkPos[find(h+"ForeArm")][k];
        VRIK_Norm3(aim);VRIK_QuatFromTo(aim,forward,turn);VRIK_QuatMul(turn,g_fkRot[hand],neutralHand[side]);
    }
    for(int axis=0;axis<3;++axis)for(float angle:{-1.2f,0.f,1.2f}) {
        auto pose=native;SolveBody(pose);auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        for(int side=0;side<2;++side) {
            const bool left=side==0;const int upper=left?17:18,fore=left?20:21,hand=left?23:24;
            const float target[3]={left?-.24f:.24f,.45f,1.55f};
            // Reflect the target pose itself; the captured native attack had
            // different left/right wrist orientations and was not a mirror.
            const float mirrored=side && axis!=0?-angle:angle;
            float roll[4]={0,0,0,std::cos(mirrored*.5f)},q[4];roll[axis]=std::sin(mirrored*.5f);
            VRIK_QuatMul(roll,neutralHand[side],q);
            VRIK_SolveArm(buffer,upper,fore,hand,target,q,right,up,forward,0,1,left,false);
        }
        VRIK_ApplyCyberwarePose(buffer);
        if(const char* capture=std::getenv("VRP_MANTIS_CAPTURE_DIR")) {
            const auto dir=std::filesystem::path(capture);std::filesystem::create_directories(dir);
            const auto path=dir/("mantis-"+std::to_string(axis)+"-"+std::to_string(int(std::round(angle*100)))+".bin");
            std::ofstream file(path,std::ios::binary);
            for(int i=0;i<g_VRBoneCount;++i){file.write(reinterpret_cast<const char*>(g_fkPos[i]),12);file.write(reinterpret_cast<const char*>(g_fkRot[i]),16);}
        }
        float anatomy[2][3]{};
        for(int side=0;side<2;++side) {
            const std::string p=side?"r_":"l_",h=side?"Right":"Left",e=side?"right_":"left_";
            const int skin=find(p+"handIndex01_JNT"),insert=find(h+"InHandIndex");
            // These are the actual equal bind matrices in the flesh/mechanism
            // meshes. They must remain welded under wrist flexion AND roll.
            for(int k=0;k<3;++k)Near(g_fkPos[skin][k],g_fkPos[insert][k],2e-5f,"Mantis skin hand detached from its index insert");
            QuatNear(g_fkRot[skin],g_fkRot[insert],"Mantis skin ignores controller wrist rotation");
            const int wrist=find(p+"innerWrist01_JNT"),socket=g_VRBoneParent[wrist];float joint[3];
            VRIK_QuatRotateVec(g_fkRot[socket],reference[wrist].p,joint);
            for(int k=0;k<3;++k)Near(joint[k]+g_fkPos[socket][k],g_fkPos[wrist][k],.00005f,"Mantis blade-base socket separates from the palm pivot");
            float targetDirection[3],forearmDirection[3];
            for(int k=0;k<3;++k) {
                targetDirection[k]=g_fkPos[find(p+"blade05_JNT")][k]-g_fkPos[find(p+"blade01_JNT")][k];
                forearmDirection[k]=g_fkPos[find(h+"Hand")][k]-g_fkPos[find(h+"ForeArm")][k];
            }
            VRIK_Norm3(targetDirection);VRIK_Norm3(forearmDirection);
            Check(VRIK_Dot3(targetDirection,forearmDirection)>.7f,"deployed blade does not point forward along its arm");
            float dorsal[3],lateral[3],tip[3],edge[3];
            VRIK_QuatRotateVec(g_fkRot[find(h+"Hand")],localDorsal[side],dorsal);
            const float parallel=VRIK_Dot3(dorsal,forearmDirection);
            for(int k=0;k<3;++k)dorsal[k]-=parallel*forearmDirection[k];
            VRIK_Norm3(dorsal);VRIK_Cross3(forearmDirection,dorsal,lateral);
            for(int k=0;k<3;++k) {
                tip[k]=g_fkPos[find(p+"blade05_JNT")][k]-g_fkPos[find(h+"Hand")][k];
                edge[k]=g_fkPos[find(p+"blade05_JNT")][k]-g_fkPos[find(p+"blade02_JNT")][k];
            }
            anatomy[side][0]=VRIK_Dot3(tip,lateral);
            anatomy[side][1]=VRIK_Dot3(tip,forearmDirection);
            anatomy[side][2]=VRIK_Dot3(tip,dorsal);
            // The mating pivot moves around Hand during wrist flexion. Bound
            // the resulting placement against the arm, rather than requiring
            // the old disconnected profile's constant Hand-relative offset.
            Check(anatomy[side][2]>.10f,"Mantis blade is not above the knuckle side of the arm");
            Check(std::abs(anatomy[side][0])<.065f,"Mantis blade is mounted beside the forearm");
            Check(anatomy[side][1]>.30f,"Mantis blade is not extended ahead of the hand");
            Near(std::sqrt(VRIK_Dot3(edge,edge)),.57144f,.002f,"wrist joint repair changed native cutting-edge length");
            VRIK_Norm3(edge);Check(VRIK_Dot3(edge,forearmDirection)>.99f,"Mantis cutting edge is not parallel to the forearm");
            // Native hand articulation can shift the two actual wrist pivots
            // by millimetres. Compare the mirrored mechanism about its mating
            // socket, while the checks above still bound it against the arm.
            for(int k=0;k<3;++k)tip[k]=g_fkPos[find(p+"blade05_JNT")][k]-g_fkPos[wrist][k];
            anatomy[side][0]=VRIK_Dot3(tip,lateral);
            anatomy[side][1]=VRIK_Dot3(tip,forearmDirection);
            anatomy[side][2]=VRIK_Dot3(tip,dorsal);
        }
        Near(anatomy[0][0],-anatomy[1][0],.001f,"Mantis left/right lateral mount differs");
        for(int k=1;k<3;++k)Near(anatomy[0][k],anatomy[1][k],.001f,"Mantis left/right deployment differs");
        const auto first=pose;
        for(int repeat=0;repeat<20;++repeat)VRIK_ApplyCyberwarePose(buffer);
        for(size_t i=0;i<pose.size();++i)for(int k=0;k<3;++k)
            Near(pose[i].p[k],first[i].p[k],2e-5f,"Mantis correction accumulates across pose passes");
    }
    Setup("weapon_stance");Check(!VRIK_UsesNativeCyberware(),"Mantis policy survives rig replacement");
    std::cout<<"mantis_wrist_axes=3 poses=9 shared_skinning_anchor_welded=true\n";
}

void CyberwareWire() {
    using namespace cvr::cyberware;
    Wire line;const Vec3 anchor{0,0,1.5f},forward{0,1,0};
    for(int i=0;i<60;++i)line.Step(anchor,forward,true,1.f/120,Wire::Reach);
    Check(line.deployed==1.f,"monowire did not fully deploy");
    Near(Length(line.point[0]-anchor),0,1e-6f,"wire emitter detached");
    const float segment=Wire::Reach/(Wire::Count-1);
    float worst=0;
    for(int frame=0;frame<180;++frame) {
        const Vec3 moving{.18f*std::sin(frame*.07f),0,1.5f};
        line.Step(moving,forward,true,1.f/120,Wire::Reach);
        for(int i=1;i<Wire::Count;++i) {
            Check(Finite(line.point[i]),"wire simulation diverged");
            worst=std::max(worst,std::abs(Length(line.point[i]-line.point[i-1])-segment));
        }
    }
    Check(worst<.015f,"wire segment constraints stretch excessively");
    Check(line.point.back().z<1.4f,"wire has no gravity/inertial sag");
    Wire still;
    for(int i=0;i<600;++i)still.Step(anchor,forward,true,1.f/120,Wire::Reach);
    const auto mid=still.point[Wire::Count/2],tip=still.point.back();
    std::cout<<"wire_rest_mid="<<mid.x<<','<<mid.y<<','<<mid.z<<" tip="<<tip.x<<','<<tip.y<<','<<tip.z<<'\n';
    Check(mid.y>1.8f && mid.z>1.35f,"wire hangs from its middle instead of extending ahead");
    Check(tip.y>2.7f && tip.z<mid.z-.20f,"wire tip does not drop after its supported section");
    for(const Vec3 sweep:{Vec3{1,0,0},Vec3{-1,0,0},Vec3{0,0,1},Vec3{0,0,-1}}) {
        Wire whip;const Vec3 high{0,0,5};
        for(int i=0;i<300;++i)whip.Step(high,forward,true,1.f/120,Wire::Reach);
        Vec3 moving=high;
        for(int i=1;i<=60;++i){moving=high+sweep*(3.6f*i/120);whip.Step(moving,forward,true,1.f/120,Wire::Reach);}
        const Vec3 end=whip.point.back(),middle=whip.point[Wire::Count/2];
        const float trailing=-Dot(middle-(moving+end)*.5f,sweep);
        const float tangent=Dot(Unit(end-whip.point[Wire::Count-2]),forward);
        std::cout<<"wire_sweep="<<sweep.x<<','<<sweep.z<<" tip_forward="<<end.y-moving.y<<" trailing_arc="<<trailing<<" tip_tangent="<<tangent<<'\n';
        Check(end.y-moving.y>4.6f,"wire tip stays folded down during a swing");
        Check(trailing>.015f && trailing<.65f,"wire does not form a small trailing arc in this sweep direction");
        Check(tangent>.93f,"wire end does not straighten during a swing");
    }
    for(int i=0;i<30;++i)line.Step(anchor,forward,false,1.f/120,Wire::Reach);
    Check(line.deployed==0,"wire does not retract on grip release");
    for(const auto p:line.point)Near(Length(p-anchor),0,1e-6f,"retracted wire remains outside emitter");
    line.Step({100,100,100},forward,true,.3f,Wire::Reach);
    for(const auto p:line.point)Check(Finite(p)&&Length(p-Vec3{100,100,100})<5.1f,"teleport leaves a trailing hit segment");
    line.Step({NAN,0,0},forward,true,.01f,Wire::Reach);Check(!line.valid,"invalid tracking did not reset wire");
    line.Reset();
    for(int frame=0;frame<500;++frame) {
        for(int i=1;i<Wire::Count;++i)line.planes[i]={{line.point[i].x,line.point[i].y,0},{0,0,1},true};
        line.Step({0,0,.35f},forward,true,1.f/120,Wire::Reach);
        for(int i=1;i<Wire::Count;++i)Check(Finite(line.point[i])&&line.point[i].z>=.0059f,"wire penetrated its floor contact plane");
    }
    const auto restingTip=line.point.back();
    float tipTravel=0;
    for(int frame=0;frame<180;++frame) {
        const float t=frame/120.f;
        const Vec3 moving{.65f*std::sin(t*9.f),0,1.5f+.45f*std::sin(t*4.f)};
        for(int i=1;i<Wire::Count;++i)line.planes[i]={{line.point[i].x,line.point[i].y,0},{0,0,1},true};
        line.Step(moving,Unit({std::sin(t*9),1,.6f}),true,1.f/120,Wire::Reach);
        tipTravel=std::max(tipTravel,Length(line.point.back()-restingTip));
    }
    Check(tipTravel>.45f,"only the start of the wire follows a hand flick");
    Wire left,rightLine;
    for(int frame=0;frame<300;++frame) {
        const float x=.2f+.3f*std::sin(frame*.04f);
        left.Step({-x,0,1.5f},{-.3f,1,.2f},true,1.f/90,Wire::Reach);
        rightLine.Step({x,0,1.5f},{.3f,1,.2f},true,1.f/90,Wire::Reach);
        for(int i=0;i<Wire::Count;++i) {
            Near(left.point[i].x,-rightLine.point[i].x,.0001f,"left/right wire dynamics are asymmetric");
            Near(left.point[i].y,rightLine.point[i].y,.0001f,"left/right wire lengths disagree");
            Near(left.point[i].z,rightLine.point[i].z,.0001f,"left/right wire sag disagrees");
        }
    }
    std::cout<<"wire_flick_tip_travel="<<tipTravel<<"m mirrored_motion=true\n";
    std::cout<<"wire_length="<<Wire::Reach<<"m nodes="<<Wire::Count<<" max_segment_error="<<worst<<"m\n";
    const char* trackNames[]={"other0","rightWireSlideFactor","other2","leftWireSlideFactor","other4"};
    float defaults[]={7,.3f,8,.8f,9},values[]={7,.3f,8,.8f,9};
    const int family=g_VRCyberwareFamily;g_VRCyberwareFamily=3;g_VRBind=4;
    VRIK_ConfigureCyberwareTracks(trackNames,5,defaults);
    VRIK_ApplyWireTracks(defaults,5);
    Check(defaults[1]==.3f && defaults[3]==.8f,"wire wrote read-only reference tracks");
    VRIK_ApplyWireTracks(values,2);
    Check(values[1]==0 && values[3]==.8f && values[0]==7,"wire track write exceeded this pose's bounds");
    VRIK_ApplyWireTracks(values,5);
    Check(values[3]==0 && values[2]==8 && values[4]==9,"wire overwrote unrelated animation tracks");
    VRIK_ConfigureCyberwareTracks(nullptr,0,nullptr);g_VRCyberwareFamily=family;
}

void CombatElbow() {
    const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0};
    CyberpunkVR_VrikElbowPolicy=1;g_VRBind=4;
    for(const char* rig:{"gorilla","mantis"})for(bool left:{false,true}) {
        Setup(rig);const auto reference=Read<Bone>("reference_full.bin");
        auto pose=Read<Bone>("player_local.bin");auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        const int upper=left?17:18,fore=left?20:21,hand=left?23:24;
        auto ref=reference;VRIK_ComputeFK(reinterpret_cast<uint8_t*>(ref.data()),int(ref.size()));
        float axis[3],refHand[4];for(int k=0;k<3;++k)axis[k]=g_fkPos[hand][k]-g_fkPos[fore][k];VRIK_Norm3(axis);
        std::copy_n(g_fkRot[hand],4,refHand);
        const float aim[3]={left?1.f:-1.f,0,0};float turn[4],rotation[4];VRIK_QuatFromTo(axis,aim,turn);VRIK_QuatMul(turn,refHand,rotation);
        SolveBody(pose);float shoulder[3];std::copy_n(g_fkPos[upper],3,shoulder);
        const float target[3]={shoulder[0]+(left?.02f:-.02f),shoulder[1]+.32f,shoulder[2]-.02f};
        VRIK_SolveArm(buffer,upper,fore,hand,target,rotation,right,up,forward,0,1,left,false);
        VRIK_ComputeFK(buffer,g_VRBoneCount);
        const float out=(g_fkPos[fore][0]-shoulder[0])*(left?-1.f:1.f);
        const float drop=shoulder[2]-g_fkPos[fore][2];
        Check(out>.12f,"hook elbow remains pinned against the torso");
        Check(drop<.12f,"hook elbow is forced into the relaxed hanging pose");
        for(int k=0;k<3;++k)Near(g_fkPos[hand][k],target[k],.002f,"combat elbow moved wrist off controller");
        std::cout<<rig<<(left?" left":" right")<<" hook_elbow_out="<<out<<"m drop="<<drop<<"m\n";
    }
    CyberpunkVR_VrikElbowPolicy=0;
}

void ElbowBlendProfiles() {
    const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0};
    CyberpunkVR_VrikElbowPolicy=1;
    for(bool left:{false,true})for(float height:{-.02f,.12f}) {
        Setup("gorilla");const auto reference=Read<Bone>("reference_full.bin");
        auto ref=reference;VRIK_ComputeFK(reinterpret_cast<uint8_t*>(ref.data()),int(ref.size()));
        const int upper=left?17:18,fore=left?20:21,hand=left?23:24;
        float axis[3],turn[4],rotation[4];for(int k=0;k<3;++k)axis[k]=g_fkPos[hand][k]-g_fkPos[fore][k];VRIK_Norm3(axis);
        const float aim[3]={left?1.f:-1.f,0,0};VRIK_QuatFromTo(axis,aim,turn);VRIK_QuatMul(turn,g_fkRot[hand],rotation);
        auto native=Read<Bone>("player_local.bin");auto base=native;SolveBody(base);
        float shoulder[3];std::copy_n(g_fkPos[upper],3,shoulder);
        const float target[3]={shoulder[0]+(left?.02f:-.02f),shoulder[1]+.32f,shoulder[2]+height};
        float normals[7][3]{},lengths[7][2]{};
        // Revisit wire/ordinary after a full-combat profile to cover rearming's smoothing state.
        const int families[]={0,1,3,0,2,3,0};
        for(int profile=0;profile<7;++profile) {
            auto pose=native;g_VRCyberwareFamily=1;SolveBody(pose);
            g_VRCyberwareFamily=families[profile];g_VRBind=profile==0?0:4;
            auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
            VRIK_SolveArm(buffer,upper,fore,hand,target,rotation,right,up,forward,0,1,left,false);
            VRIK_ComputeFK(buffer,g_VRBoneCount);
            float line[3],fromWrist[3],upperSegment[3];
            for(int k=0;k<3;++k) {
                Near(g_fkPos[hand][k],target[k],.002f,"elbow blend moves wrist off its controller");
                line[k]=g_fkPos[upper][k]-target[k];fromWrist[k]=g_fkPos[fore][k]-target[k];
                upperSegment[k]=g_fkPos[fore][k]-g_fkPos[upper][k];
            }
            lengths[profile][0]=std::sqrt(VRIK_Dot3(upperSegment,upperSegment));
            lengths[profile][1]=std::sqrt(VRIK_Dot3(fromWrist,fromWrist));
            VRIK_Norm3(line);const float along=VRIK_Dot3(fromWrist,line);
            for(int k=0;k<3;++k)normals[profile][k]=fromWrist[k]-line[k]*along;
            VRIK_Norm3(normals[profile]);
        }
        const auto angle=[&](int profile){return std::acos(std::clamp(VRIK_Dot3(normals[0],normals[profile]),-1.f,1.f));};
        const float full=angle(1);Check(full>.3f,"blend test needs distinct relaxed and combat endpoints");
        for(int profile=1;profile<7;++profile) {
            const float expected=families[profile]==0?.25f:families[profile]==3?.5f:1.f;
            Near(angle(profile)/full,expected,.01f,"elbow swivel does not match the requested percentage");
            for(int k=0;k<2;++k)Near(lengths[profile][k],lengths[0][k],.00005f,"elbow percentage changes arm length");
        }
        std::cout<<(left?"left":"right")<<" height="<<height<<" wire_fraction="<<angle(2)/full<<" ordinary_fraction="<<angle(3)/full<<'\n';
    }
    CyberpunkVR_VrikElbowPolicy=0;g_VRBind=4;Setup();
}

void CyberwareMuscles() {
    using namespace cvr::cyberware;
    float mantisMove=0,gorillaScale=1;
    for(const auto& curve:MuscleCurves) {
        for(int endpoint:{0,MuscleSampleCount-1}) {
            const auto& value=curve.samples[endpoint];
            for(int k=0;k<3;++k){Near(value.position[k],0,1e-7f,"muscle pulse fails to return to its pose");Near(value.scale[k],1,1e-7f,"muscle pulse fails to restore thickness");}
            Near(std::abs(value.rotation[3]),1,1e-7f,"muscle pulse leaves a rotation offset");
        }
        for(const auto& value:curve.samples) {
            float norm=0;for(float v:value.rotation){Check(std::isfinite(v),"invalid muscle rotation");norm+=v*v;}
            Near(norm,1,1e-5f,"muscle curve rotation is not normalized");
            if(curve.family==2)mantisMove=std::max(mantisMove,std::sqrt(VRIK_Dot3(value.position,value.position)));
            if(curve.family==1)gorillaScale=std::max(gorillaScale,value.scale[1]);
            for(float v:value.scale)Check(std::isfinite(v)&&v>.8f&&v<2.1f,"native muscle scale out of captured bounds");
        }
    }
    Check(mantisMove>.005f&&mantisMove<.04f,"Mantis native muscle motion missing");
    Check(gorillaScale>1.5f,"Gorilla native contraction missing");
    std::cout<<"native_mantis_motion="<<mantisMove<<"m gorilla_thickness="<<gorillaScale<<"\n";
}

void LauncherAxis() {
    Setup("launcher");auto pose=Read<Bone>("player_local.bin");g_VRBind=4;
    testCyberwareWorldFrame=true;VRIK_ApplyCyberwarePose(reinterpret_cast<uint8_t*>(pose.data()));testCyberwareWorldFrame=false;
    cvr::cyberware::Frame frame;Check(cvr::cyberware::ReadFrame(frame)&&frame.count[0]==2,"launcher muzzle not published");
    auto find=[](const std::string& n){return int(std::find(names.begin(),names.end(),n)-names.begin());};
    float actual[3],expected[3],forearm[3];const float local[3]={0,0,1};
    const auto delta=frame.points[0][1]-frame.points[0][0];actual[0]=delta.x;actual[1]=delta.y;actual[2]=delta.z;VRIK_Norm3(actual);
    VRIK_QuatRotateVec(g_fkRot[find("launcher_base")],local,expected);
    for(int k=0;k<3;++k)forearm[k]=g_fkPos[find("LeftHand")][k]-g_fkPos[find("LeftForeArm")][k];VRIK_Norm3(forearm);
    Near(VRIK_Dot3(actual,expected),1,1e-5f,"launcher aims across the housing axis");
    Check(VRIK_Dot3(actual,forearm)>.98f,"native deployed launcher points sideways");
    const auto before=delta;
    const int ammo=find("launcher_ammo_base");const float turn[4]={.70710678f,0,0,.70710678f};
    float rotated[4];VRIK_QuatMul(pose[ammo].q,turn,rotated);std::copy_n(rotated,4,pose[ammo].q);
    testCyberwareWorldFrame=true;VRIK_ApplyCyberwarePose(reinterpret_cast<uint8_t*>(pose.data()));testCyberwareWorldFrame=false;
    Check(cvr::cyberware::ReadFrame(frame),"launcher frame missing after cartridge movement");
    Near(cvr::cyberware::Length((frame.points[0][1]-frame.points[0][0])-before),0,.00001f,"cartridge animation steers the shot");
    // The idle animation sways the fire mechanism and its marker: neither may carry the shot's direction.
    const float sway[4]={0,.08715574f,0,.9961947f};
    for(const char* bone:{"fire_mechanism_b_part","launcher_fragment_fire"}) {
        const int part=find(bone);VRIK_QuatMul(pose[part].q,sway,rotated);std::copy_n(rotated,4,pose[part].q);
    }
    testCyberwareWorldFrame=true;VRIK_ApplyCyberwarePose(reinterpret_cast<uint8_t*>(pose.data()));testCyberwareWorldFrame=false;
    Check(cvr::cyberware::ReadFrame(frame),"launcher frame missing after mechanism sway");
    Near(cvr::cyberware::Length((frame.points[0][1]-frame.points[0][0])-before),0,.00001f,"mechanism sway steers the shot");
    std::cout<<"launcher_barrel_axis_stable_during_cartridge_animation=true forward_dot="<<VRIK_Dot3(actual,forearm)<<"\n";
}

void LauncherWrist() {
    Setup("launcher");g_VRBind=4;
    const auto native=Read<Bone>("player_local.bin");
    auto base=native;VRIK_ApplyCyberwarePose(reinterpret_cast<uint8_t*>(base.data()));
    const auto find=[](const char* n){return int(std::find(names.begin(),names.end(),n)-names.begin());};
    const int hand=find("LeftHand"),skin=find("LeftHand_setup"),barrel=find("launcher_fragment_fire");
    float baseHand[4],baseSkin[4],baseBarrel[4],offset[3];
    std::copy_n(g_fkRot[hand],4,baseHand);std::copy_n(g_fkRot[skin],4,baseSkin);std::copy_n(g_fkRot[barrel],4,baseBarrel);
    for(int k=0;k<3;++k)offset[k]=g_fkPos[skin][k]-g_fkPos[hand][k];
    for(int axis=0;axis<3;++axis)for(float angle:{-.8f,.8f}) {
        auto pose=native;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        VRIK_ComputeFK(buffer,g_VRBoneCount);
        float turn[4]={0,0,0,std::cos(angle/2)},target[4],expected[4],expectedOffset[3];turn[axis]=std::sin(angle/2);
        VRIK_QuatMul(turn,baseHand,target);VRIK_QuatMul(turn,baseSkin,expected);VRIK_QuatRotateVec(turn,offset,expectedOffset);
        VRIK_WriteLocalRot(buffer,hand,g_fkRot[g_VRBoneParent[hand]],target);
        VRIK_ApplyCyberwarePose(buffer);
        QuatNear(g_fkRot[skin],expected,"launcher visible wrist ignores controller rotation");
        QuatNear(g_fkRot[barrel],baseBarrel,"wrist flex steers forearm barrel");
        for(int k=0;k<3;++k)Near(g_fkPos[skin][k]-g_fkPos[hand][k],expectedOffset[k],1e-5f,"launcher wrist bind socket separated");
        for(size_t i=0;i<pose.size();++i)if(names[i].find("Hand")!=std::string::npos && names[i].find("_setup")!=std::string::npos && int(i)!=skin)
            Check(std::memcmp(&pose[i],&native[i],sizeof(Bone))==0,"launcher wrist changed native finger articulation");
        const auto once=pose;VRIK_ApplyCyberwarePose(buffer);
        for(size_t i=0;i<pose.size();++i)for(int k=0;k<4;++k)Near(pose[i].q[k],once[i].q[k],1e-6f,"launcher wrist accumulates across consumers");
    }
    std::cout<<"launcher_visible_wrist_axes=3 native_fingers_and_barrel_preserved=true\n";
}

void HandGestures() {
    using cvr::gestures::Evaluate;
    auto point=Evaluate(1,0,false,false,0);Check(point.closed[2]==1 && point.closed[1]==0 && point.straight[1]==1,"point gesture");
    auto index=Evaluate(0,1,false,false,0);Check(index.closed[1]==1 && index.closed[0]==0 && index.closed[2]==0,"trigger-only index");
    auto fist=Evaluate(1,1,false,false,0);for(float x:fist.closed)Check(x==1,"fist digit open");
    auto thumb=Evaluate(1,1,true,false,0);Check(thumb.straight[0]==1 && thumb.closed[0]==0 && thumb.closed[1]==1,"thumbs up priority");
    for(float phase:{0.f,.25f,.5f,.75f,1.f}) {
        auto wire=Evaluate(1,1,true,true,phase);
        for(float v:wire.closed)Near(v,phase<.35f?0.f:phase>.65f?1.f:.5f,.00001f,"wire catch not synchronized to payout");
    }
    for(const char* rig:{"","gorilla","mantis"}) {
        Setup(rig);auto rest=Read<Bone>("reference_full.bin"),pose=rest;g_VRBind=4;g_hasWeaponEquipped=false;
        g_VRSmokeFingerCount=g_VRSmokeFingerCountL=19;
        for(int side=0;side<2;++side)for(int k=0;k<19;++k) {
            const std::string name=std::string(side?"Right":"Left")+cvr::gestures::NativeFist[k].suffix;
            const auto it=std::find(names.begin(),names.end(),name);Check(it!=names.end(),"gesture finger missing");
            const int bone=int(it-names.begin());(side?g_VRSmokeFingerIdx:g_VRSmokeFingerIdxL)[k]=bone;
            std::strncpy((side?g_VRSmokeFingerName:g_VRSmokeFingerNameL)[k],name.c_str(),47);
            if(!side)std::copy_n(rest[bone].q,4,g_VRRestFingerRot[k]);
        }
        g_VRRestFingerCount=19;g_VRRestFingerHave=1;
        for(int side=0;side<2;++side) {
            pose=rest;const std::string hand=side?"Right":"Left";
            auto finger=[&](const char* suffix){return int(std::find(names.begin(),names.end(),hand+suffix)-names.begin());};
            VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),g_VRBoneCount);
            float expected[3];for(int k=0;k<3;++k)expected[k]=g_fkPos[finger("HandIndex3")][k]-g_fkPos[finger("InHandIndex")][k];VRIK_Norm3(expected);
            Check(cvr::gestures::ThumbReference(side,pose[finger("InHandThumb")].q),"thumb reference missing");
            VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),g_VRBoneCount);
            float actual[3];for(int k=0;k<3;++k)actual[k]=g_fkPos[finger("HandThumb2")][k]-g_fkPos[finger("InHandThumb")][k];VRIK_Norm3(actual);
            Check(VRIK_Dot3(actual,expected)>.995f,"thumb points sideways instead of up along the palm");
        }
        pose=rest;
        CyberpunkVR_RestFingerCaptureReqR=1;cvr::anim::VrikRestFingerPoseRight(reinterpret_cast<uint8_t*>(pose.data()));
        cvr::gestures::PublishInput(1,1,1,1,true,true);cvr::gestures::PublishEquipment(3);
        cvr::gestures::Apply(reinterpret_cast<uint8_t*>(pose.data()));
        Check(!std::memcmp(pose.data(),rest.data(),pose.size()*sizeof(Bone)),"held weapon fingers changed");
        cvr::gestures::PublishEquipment(0);
        for(int i=0;i<20;++i) {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));++g_VrikFrameEpoch;
            cvr::gestures::PublishInput(1,1,0,0,false,true);pose=rest;
            cvr::gestures::Apply(reinterpret_cast<uint8_t*>(pose.data()));
        }
        for(int side=0;side<2;++side)for(int k=0;k<19;++k) {
            const auto& finger=cvr::gestures::NativeFist[k];const int bone=(side?g_VRSmokeFingerIdx:g_VRSmokeFingerIdxL)[k];
            if(finger.group==1)QuatNear(pose[bone].q,rest[bone].q,"pointing index bent");
            Check(!std::memcmp(pose[bone].p,rest[bone].p,16),"gesture changed finger length");
        }
        if(std::string(rig)=="mantis" || std::string(rig)=="gorilla") {
            testCyberwareWorldFrame=true;VRIK_ApplyCyberwarePose(reinterpret_cast<uint8_t*>(pose.data()));testCyberwareWorldFrame=false;
            for(const char* prefix:{"l_","r_"}) {
                const std::string canonical=prefix[0]=='l'?"LeftHandMiddle1":"RightHandMiddle1";
                const auto source=std::find(names.begin(),names.end(),canonical)-names.begin();
                const auto target=std::string(rig)=="gorilla" ? canonical+"_setup" : std::string(prefix)+"handMiddle02_JNT";
                const auto skin=std::find(names.begin(),names.end(),target)-names.begin();
                QuatNear(g_fkRot[source],g_fkRot[skin],"Cyberware visible finger chain ignored the controlled finger");
            }
        }
        // R3 must never command the left thumb, even with both grips held.
        ++g_VrikFrameEpoch;cvr::gestures::PublishInput(1,1,0,0,true,true);pose=rest;
        cvr::gestures::Apply(reinterpret_cast<uint8_t*>(pose.data()));
        Check(CyberpunkVR_GestureMode[0]==2 && CyberpunkVR_GestureMode[1]==5,"right thumbs-up leaked into the left hand");
        ++g_VrikFrameEpoch;cvr::gestures::PublishInput(1,0,0,0,true,true);pose=rest;
        cvr::gestures::Apply(reinterpret_cast<uint8_t*>(pose.data()));
        Check(CyberpunkVR_GestureMode[0]==2 && CyberpunkVR_GestureMode[1]==1,"R3 without right grip triggered a gesture");
        // The authored trigger-only pose must reach both canonical and visible
        // cyberware fingers without changing the other finger groups' lengths.
        for(int sample=0;sample<25;++sample) {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));++g_VrikFrameEpoch;
            cvr::gestures::PublishInput(0,0,1,1,false,true);cvr::gestures::PublishEquipment(0);pose=rest;
            cvr::gestures::Apply(reinterpret_cast<uint8_t*>(pose.data()));
        }
        const cvr::gestures::CalibrationWeights indexPose={0,0,0,1};
        for(int hand=0;hand<2;++hand)for(int k=0;k<19;++k) {
            const auto& finger=cvr::gestures::NativeFist[k];const int bone=(hand?g_VRSmokeFingerIdx:g_VRSmokeFingerIdxL)[k];
            float expected[4];std::copy_n(rest[bone].q,4,expected);
            if(finger.group==1) {
                float inverse[4],delta[4];VRIK_QuatConj(finger.reference[hand],inverse);
                VRIK_QuatMul(finger.closed[hand],inverse,delta);VRIK_QuatMul(delta,rest[bone].q,expected);VRIK_QuatNorm(expected);
            }
            for(const auto& authored:cvr::gestures::CalibratedGestures)if(authored.pose==3 &&
                (authored.hands&(1u<<hand)) && std::strcmp(authored.suffix,finger.suffix)==0) {
                float corrected[4];VRIK_QuatMul(expected,authored.delta,corrected);VRIK_QuatNorm(corrected);std::copy_n(corrected,4,expected);
            }
            QuatNear(pose[bone].q,expected,"runtime gesture does not include the baked export");
            Check(!std::memcmp(pose[bone].p,rest[bone].p,16),"calibration changed finger length");
        }
        if(std::string(rig)=="mantis" || std::string(rig)=="gorilla") {
            const auto once=pose;testCyberwareWorldFrame=true;VRIK_ApplyCyberwarePose(reinterpret_cast<uint8_t*>(pose.data()));testCyberwareWorldFrame=false;
            for(int hand=0;hand<2;++hand) {
                const std::string source=std::string(hand?"Right":"Left")+"HandIndex1";
                const std::string skin=std::string(rig)=="gorilla" ? source+"_setup":std::string(hand?"r_":"l_")+"handIndex02_JNT";
                const auto a=std::find(names.begin(),names.end(),source)-names.begin(),b=std::find(names.begin(),names.end(),skin)-names.begin();
                QuatNear(pose[a].q,once[a].q,"baked corrections accumulate across consumer passes");
                QuatNear(g_fkRot[a],g_fkRot[b],"baked corrections did not reach the visible cyberware fingers");
            }
        }
    }
    for(float grip:{0.f,.25f,1.f})for(float trigger:{0.f,.5f,1.f}) {
        const auto w=cvr::gestures::CalibrationTargets(grip,trigger,false,false);
        float sum=0;for(float v:w){Check(v>=0 && v<=1,"invalid calibration blend weight");sum+=v;}
        Check(sum<=1.000001f,"gesture calibrations overlap instead of blending");
        Check(cvr::gestures::CalibrationTargets(grip,trigger,true,true)==cvr::gestures::CalibrationWeights{},"wire inherited a normal gesture correction");
    }
    const float neutral[4]={0,0,0,1};float left[4],right[4];std::copy_n(neutral,4,left);std::copy_n(neutral,4,right);
    cvr::gestures::ApplyCalibration(0,"InHandThumb",{0,0,1,0},left);
    cvr::gestures::ApplyCalibration(1,"InHandThumb",{0,0,1,0},right);
    QuatNear(left,right,"the thumbs-up calibration differs between the hands");
    Near(2*std::acos(right[3])*57.2957795f,49.8069992f,.0001f,"right thumb differs from user export");
    std::cout<<"hand_gestures_baked_both_hands_right_only_thumb_wire_and_weapon_protection=true\n";
}

void ForearmRoll() {
    const auto reference=Read<Bone>("reference_full.bin");
    for(bool left:{false,true}) {
        const int fore=left ? 20:21,hand=left ? 23:24;
        const int* helpers=left ? g_VRForeTwistL : g_VRForeTwistR;
        float axis[3];std::copy_n(reference[hand].p,3,axis);VRIK_Norm3(axis);
        float previousMotion=0;
        for(float angle:{-1.04719755f,0.0f,1.04719755f}) {
            auto pose=reference;
            const float roll[4]={axis[0]*std::sin(angle/2),axis[1]*std::sin(angle/2),axis[2]*std::sin(angle/2),std::cos(angle/2)};
            VRIK_QuatMul(roll,reference[hand].q,pose[hand].q);
            for(int k=0;k<3;++k)pose[hand].p[k]*=1.15f;
            const auto before=pose;
            VRIK_UpdateForearmDeformation(reinterpret_cast<uint8_t*>(pose.data()),fore,hand,left);
            previousMotion=0;
            for(int slot=0;slot<3;++slot) {
                const int bone=helpers[slot];const float radial[3]={0,1,0};float v[3];
                VRIK_QuatRotateVec(pose[bone].q,radial,v);
                // Follow a skin marker, not the implementation's angle formula:
                // it must turn with the wrist, farther at the distal helper.
                const float motion=std::abs(v[2]);
                if(angle!=0) {
                    Check(v[2]*angle*axis[0]>0,"forearm skin marker turns against the wrist");
                    Check(motion>previousMotion+.05f,"forearm skin does not follow wrist progressively");
                } else Near(v[2],0,1e-5f,"neutral wrist retains a captured animation twist");
                previousMotion=motion;
                float radialLength=0;for(float x:v)radialLength+=x*x;
                Near(radialLength,1,1e-5f,"twist shrinks forearm cross section");
                Near(pose[bone].p[0]/pose[hand].p[0],reference[bone].p[0]/reference[hand].p[0],1e-5f,"skin helper no longer follows calibrated segment length");
            }
            for(int i=0;i<619;++i)if(i!=helpers[0] && i!=helpers[1] && i!=helpers[2])
                Check(std::memcmp(&pose[i],&before[i],48)==0,"forearm deformation wrote another bone");
            const auto solved=pose;
            for(int repeat=0;repeat<100;++repeat)VRIK_UpdateForearmDeformation(reinterpret_cast<uint8_t*>(pose.data()),fore,hand,left);
            Check(std::memcmp(pose.data(),solved.data(),pose.size()*48)==0,"forearm twist accumulates across passes");
            VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),619);
            VRIK_CaptureModelCache();pose=reference;
            VRIK_ReplayModelCache(reinterpret_cast<uint8_t*>(pose.data()));
            for(int slot=0;slot<3;++slot)QuatNear(pose[helpers[slot]].q,solved[helpers[slot]].q,"model replay lost wrist deformation");
        }
    }
}
void ForearmSolve() {
    const auto native=Read<Bone>("player_local.bin");
    const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0};
    for(bool left:{false,true}) {
        const int upper=left ? 17:18,fore=left ? 20:21,hand=left ? 23:24;
        const int* helpers=left ? g_VRForeTwistL : g_VRForeTwistR;
        float rotations[2][3][4]{};
        for(int sign=0;sign<2;++sign) {
            auto pose=native;SolveBody(pose);
            float target[3],axis[3],handRotation[4];std::copy_n(g_fkPos[hand],3,target);
            for(int k=0;k<3;++k)axis[k]=g_fkPos[hand][k]-g_fkPos[fore][k];VRIK_Norm3(axis);
            const float a=sign ? .5f:-.5f,roll[4]={axis[0]*std::sin(a),axis[1]*std::sin(a),axis[2]*std::sin(a),std::cos(a)};
            VRIK_QuatMul(roll,g_fkRot[hand],handRotation);
            const auto before=pose;
            VRIK_SolveArm(reinterpret_cast<uint8_t*>(pose.data()),upper,fore,hand,target,handRotation,right,up,forward,0,1,left,false);
            VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),619);
            QuatNear(g_fkRot[hand],handRotation,"wrist target changed while distributing forearm twist");
            for(int i=0;i<619;++i)if(!g_VRUpperOwned[i])Check(std::memcmp(&pose[i],&before[i],48)==0,"arm solver overwrote native lower body");
            for(int slot=0;slot<3;++slot)std::copy_n(pose[helpers[slot]].q,4,rotations[sign][slot]);
        }
        float previous=0;
        for(int slot=0;slot<3;++slot) {
            float dot=0;for(int k=0;k<4;++k)dot+=rotations[0][slot][k]*rotations[1][slot][k];
            const float change=2*std::acos(std::clamp(std::abs(dot),0.0f,1.0f));
            Check(change>previous+.05f,"full arm solve did not drive forearm helpers with wrist roll");previous=change;
        }
    }
}
extern "C" float CyberpunkVR_DebugToes[2][3];
// The knee's pole in a squat follows the toes (CyberpunkVR_SquatKneeMode): the toes must be the ToeBase bones, ahead of
// the ankles -- the foot's other child, LeftHeel, is behind them and bent the knees backward (2026-11-07).
void ToesAhead() {
    auto pose=Read<Bone>("player_local.bin");SolveBody(pose);VRIK_PublishBodyBones();
    const int feet[2]={g_VRRightFootIdx,g_VRLeftFootIdx};
    for(int side=0;side<2;++side) {
        Check(feet[side]>=0,"no foot");
        const float* toe=CyberpunkVR_DebugToes[side];const float* foot=g_fkPos[feet[side]];
        const float ahead=toe[1]-foot[1],below=foot[2]-toe[2];
        std::cout<<(side ? "left":"right")<<" toe ahead of the ankle "<<ahead*1000<<" mm, below "<<below*1000<<" mm\n";
        Check(ahead>.05f,"the toe is not ahead of the ankle");
    }
    std::cout<<"PASS toes ahead of the ankles\n";
}
void NativeLowerPreserved() {
    auto pose=Read<Bone>("player_local.bin");const auto native=pose;SolveBody(pose);
    for(size_t i=0;i<pose.size();++i) {
        if(!g_VRUpperOwned[i])Check(std::memcmp(&pose[i],&native[i],sizeof(Bone))==0,"non-upper native bone changed");
        Check(pose[i].p[3]==native[i].p[3] && std::memcmp(pose[i].s,native[i].s,16)==0,"QsTransform padding/scale changed");
    }
    const auto reference=Read<Bone>("reference_full.bin");
    Near(g_fkPos[2][2],reference[2].p[2]+1.6f-1.60963654f,1e-5f,"pelvis lost native camera height offset");
    Check(g_fkPos[13][1]<g_fkPos[22][1]-.01f,"chest should be slightly behind head");
    std::cout<<"chest_behind_head_mm="<<(g_fkPos[22][1]-g_fkPos[13][1])*1000<<'\n';
    const auto solved=pose;SolveBody(pose);
    for(size_t i=0;i<pose.size();++i)for(int k=0;k<3;++k)
        Near(pose[i].p[k],solved[i].p[k],1e-5f,"body solve accumulates translation");
}
void HeadBasis() {
    auto pose=Read<Bone>("player_local.bin");
    for(float a : {0.0f,.7f,-1.2f}) {
        const float head[4]={std::sin(a/2),0,0,std::cos(a/2)};SolveBody(pose,head);
        float expected[4];VRIK_QuatMul(head,g_VRHeadReferenceModelRot,expected);
        QuatNear(g_fkRot[22],expected,"head lost reference axes");
        const float skullUp[3]={1,0,0};float actualUp[3],desiredUp[3];
        const float cameraUp[3]={0,0,1};VRIK_QuatRotateVec(head,cameraUp,desiredUp);
        VRIK_QuatRotateVec(g_fkRot[22],skullUp,actualUp);
        for(int k=0;k<3;++k)Near(actualUp[k],desiredUp[k],1e-5f,"skull up axis does not follow HMD");
    }
}
void NeckMount() {
    auto reference=Read<Bone>("reference_full.bin");auto pose=reference;
    cvr::camera::AnchorVector mount{};
    Check(cvr::camera::ReadNeckCameraMount(&mount),"neutral neck camera mount unavailable");
    SolveBody(pose);
    Near(mount.y,g_fkPos[19][1]+.15f,1e-5f,"camera is not fifteen centimetres forward of neutral neck");
    Near(1.6f+mount.z,.5f*(g_fkPos[264][2]+g_fkPos[265][2])+.10f,1e-5f,"camera height is not ten centimetres above neutral eyes");
    cvr::camera::AnchorVector eyeOffset{};
    Check(cvr::camera::ReadNeckCameraEyeOffset(&eyeOffset),"neutral camera/eye offset unavailable");
    Near(eyeOffset.y,g_fkPos[19][1]+.15f-.5f*(g_fkPos[264][1]+g_fkPos[265][1]),1e-5f,
         "passenger mount differs from standing neutral neck forward offset");
    Near(eyeOffset.z,.10f,1e-5f,"passenger mount lost ten centimetres above the eyes");
    const float tilted[4]={std::sin(.4f),0,0,std::cos(.4f)};
    for(int i=0;i<100;++i) {
        pose=Read<Bone>("player_local.bin");pose[11].p[0]+=.15f;pose[12].p[1]-=.2f;
        pose[2].p[1]+=.3f;SolveBody(pose,tilted);
        cvr::camera::AnchorVector next{};Check(cvr::camera::ReadNeckCameraMount(&next),"mount lost during head motion");
        Check(std::memcmp(&mount,&next,sizeof(mount))==0,"animated stance/head changed camera calibration");
        Check(shared[119]==0 && shared[116]==0 && shared[117]==0 && shared[118]==0,"old head EMA adds a second camera offset");
    }
    std::cout<<"neutral_neck_mount="<<mount.x<<','<<mount.y<<','<<mount.z<<'\n';
    std::vector<const char*> ptrs;for(auto& name:names)ptrs.push_back(name.c_str());
    VRIK_ConfigureRigPolicy(ptrs.data(),619,reinterpret_cast<const uint8_t*>(reference.data()),103);
    cvr::camera::AnchorVector unavailable{};
    Check(!cvr::camera::ReadNeckCameraMount(&unavailable),"partial rig retained the previous camera mount");
}
void FemaleNeckMount() {
    const auto reference=Read<Bone>("reference_full.bin");
    std::vector<const char*> ptrs;for(auto& name:names)ptrs.push_back(name.c_str());
    cvr::camera::AnchorVector male{},maleEyes{};
    Check(cvr::camera::ReadNeckCameraMount(&male) && cvr::camera::ReadNeckCameraEyeOffset(&maleEyes),"male baseline mount missing");
    for(bool female:{true,false,true,false}) {
        VRIK_ConfigureRigPolicy(ptrs.data(),g_VRBoneCount,reinterpret_cast<const uint8_t*>(reference.data()),static_cast<int>(reference.size()),female);
        cvr::camera::AnchorVector mount{},eyes{};
        Check(cvr::camera::ReadNeckCameraMount(&mount) && cvr::camera::ReadNeckCameraEyeOffset(&eyes),"body rebind lost neutral neck mount");
        Near(mount.y,male.y-(female ? .05f:0),.00001f,"female body did not switch from15cm to10cm forward");
        Near(eyes.y,maleEyes.y-(female ? .05f:0),.00001f,"camera and VRIK eye anchors disagree on female forward offset");
        Near(mount.x,male.x,.00001f,"body selection changed lateral mount");
        Near(mount.z,male.z,.00001f,"body selection changed camera height");
        Near(eyes.z,.10f,.00001f,"body selection lost10cm eye height");
        auto pose=Read<Bone>("player_local.bin");auto* buf=reinterpret_cast<uint8_t*>(pose.data());
        const float camera[3]={.1f,.3f,1.5f},head[4]={0,0,0,1};
        Check(VRIK_PlaceSwimmingBody(buf,camera,head,g_VRHeadBoneIdx),"body fit failed after gender switch");
        Near(g_VRIKBodyEyeSolved[1],camera[1]-eyes.y,.00002f,"female water/upper-body eye anchor kept the old offset");
    }
    std::cout<<"female_forward_m=0.10 male_forward_m=0.15 up_m=0.10 rebind_restores_body_offset=true\n";
}
int ComputePoseWithBounds(uint8_t* buffer,uint32_t slots,bool guarded) {
    __try {
        if(guarded && !VRIK_PlayerPoseFits(slots,g_VRBoneCount,g_VRFKCount))return 0;
        VRIK_ComputeFK(buffer,VRIK_FKCount());return 1;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return -1;}
}
void PoseBufferRebind() {
    auto pose=Read<Bone>("player_local.bin");
    const uint32_t poseSlots=static_cast<uint32_t>(pose.size());
    const size_t bytes=pose.size()*sizeof(Bone),page=4096,committed=(bytes+page-1)&~(page-1);
    auto* allocation=static_cast<uint8_t*>(VirtualAlloc(nullptr,committed+page,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
    Check(allocation!=nullptr,"guarded pose allocation failed");DWORD old=0;
    const bool protectedPage=VirtualProtect(allocation+committed,page,PAGE_NOACCESS,&old)!=0;
    if(!protectedPage){VirtualFree(allocation,0,MEM_RELEASE);Check(false,"pose guard page failed");}
    auto* buffer=allocation+committed-bytes;std::memcpy(buffer,pose.data(),bytes);
    g_VRBoneCount=g_VRFKCount=768;
    const int unsafe=ComputePoseWithBounds(buffer,poseSlots,false);
    const int rejected=ComputePoseWithBounds(buffer,poseSlots,true);
    const bool untouched=std::memcmp(buffer,pose.data(),bytes)==0;
    // The actual production FK crosses the guard page with the old layout.
    // The entry gate must reject before touching a smaller replacement buffer.
    Check(unsafe==-1 && rejected==0 && untouched,"stale cyberware layout was not stopped at the pose boundary");
    HANDLE ready=CreateEventW(nullptr,TRUE,FALSE,nullptr),finished=CreateEventW(nullptr,TRUE,FALSE,nullptr);
    Check(ready && finished,"rebind events failed");int result=-2;
    AcquireSRWLockExclusive(&g_PlayerPoseStateLock);
    std::thread solve([&]{SetEvent(ready);AcquireSRWLockExclusive(&g_PlayerPoseStateLock);
        result=ComputePoseWithBounds(buffer,poseSlots,true);
        ReleaseSRWLockExclusive(&g_PlayerPoseStateLock);SetEvent(finished);});
    WaitForSingleObject(ready,1000);
    const bool blocked=WaitForSingleObject(finished,30)==WAIT_TIMEOUT;
    g_VRBoneCount=g_VRFKCount=static_cast<int>(poseSlots);
    ReleaseSRWLockExclusive(&g_PlayerPoseStateLock);solve.join();
    CloseHandle(ready);CloseHandle(finished);
    const bool valid=ComputePoseWithBounds(buffer,poseSlots,true)==1;
    VirtualFree(allocation,0,MEM_RELEASE);
    Check(blocked && result==1 && valid,"pose solve observed a partially rebound skeleton");
    Check(!VRIK_PlayerPoseFits(poseSlots,0,0),"unresolved rig accepted");
    Check(!VRIK_PlayerPoseFits(poseSlots,static_cast<int>(poseSlots),768),"stale FK prefix accepted");
    Check(VRIK_PlayerPoseFits(768,768,768),"full cyberware rig rejected");
}
void PelvisPose() {
    auto pose=Read<Bone>("player_local.bin");const auto native=pose;
    auto reference=Read<Bone>("reference_full.bin");
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(reference.data()),619);
    float referenceRotation[4];std::copy_n(g_fkRot[2],4,referenceRotation);
    SolveBody(pose);QuatNear(g_fkRot[2],referenceRotation,"pelvis kept animated rotation");
    Near(pose[4].p[0],reference[4].p[0],1e-6f,"spine root stretched instead of moving pelvis");
    for(int i=0;i<619;++i)if(!g_VRUpperOwned[i])Check(std::memcmp(&pose[i],&native[i],48)==0,"pelvis solve overwrote native leg/control local pose");
    const float standing=g_fkPos[2][2];
    const float camera[3]={0,0,1.2f},q[4]={0,0,0,1},forward[3]={0,1,0};
    VRIK_PlaceBodyUnderHMD(reinterpret_cast<uint8_t*>(pose.data()),camera,q,22,forward);
    Near(g_fkPos[2][2],standing-.4f,1e-5f,"pelvis ignores native crouch height");
    VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));
    for(int k=0;k<3;++k)Near(g_fkPos[236][k],g_fkPos[2][k],1e-5f,"shadow pelvis differs from body");
}
void UpperPosture() {
    auto pose=Read<Bone>("reference_full.bin");
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),619);
    float inverse[4],sternumLocal[3];VRIK_QuatConj(g_fkRot[13],inverse);
    const float sternumOffset[3]={0,.08f,.08f};
    VRIK_QuatRotateVec(inverse,sternumOffset,sternumLocal);
    const std::array<float,4> referenceRotation={g_fkRot[13][0],g_fkRot[13][1],g_fkRot[13][2],g_fkRot[13][3]};
    SolveBody(pose);
    float sternum[3];VRIK_QuatRotateVec(g_fkRot[13],sternumLocal,sternum);
    for(int k=0;k<3;++k)sternum[k]+=g_fkPos[13][k];
    // A surface point on the front of the chest must retreat, not just rotate
    // around its centre and project farther forward. The previous pose gives
    // Y=0.01660m on this captured rig; the new limit is below 0.010m.
    Check(sternum[1]<.010f,"rounded posture still protrudes the chest forward");
    Check(g_fkPos[19][1]-g_fkPos[13][1]>.065f,"upper spine is still too straight");
    float delta[4],normal[3];VRIK_QuatConj(referenceRotation.data(),inverse);
    VRIK_QuatMul(g_fkRot[13],inverse,delta);
    const float front[3]={0,1,0};VRIK_QuatRotateVec(delta,front,normal);
    Check(normal[2]<-.17f,"chest tilt does not round the upper back");
    std::cout<<"sternum_y_mm="<<sternum[1]*1000<<" chest_behind_neck_mm="
             <<(g_fkPos[19][1]-g_fkPos[13][1])*1000<<'\n';
}
void ShadowUpper() {
    auto pose=Read<Bone>("player_local.bin");const auto native=pose;SolveBody(pose);
    VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));
    for(size_t i=0;i<pose.size();++i) {
        const int source=g_VRShadowSource[i];
        if(source>=0) {
            for(int k=0;k<3;++k)Near(g_fkPos[i][k],g_fkPos[source][k],1e-5f,"shadow position differs from primary");
            QuatNear(g_fkRot[i],g_fkRot[source],"shadow rotation differs from primary");
        } else if(!g_VRUpperOwned[i])Check(std::memcmp(&pose[i],&native[i],sizeof(Bone))==0,"shadow sync overwrote native lower/control bone");
    }
}
void ModelReplay() {
    auto pose=Read<Bone>("player_local.bin");SolveBody(pose);
    VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));VRIK_CaptureModelCache();
    std::array<std::array<float,3>,619> saved{};
    for(int i=0;i<619;++i)std::copy_n(g_fkPos[i],3,saved[i].begin());
    // Simulate another engine pass with different locomotion below the upper body.
    pose[2].p[0]+=.13f;pose[2].q[0]=.15f;VRIK_QuatNorm(pose[2].q);
    pose[8].q[1]+=.22f;VRIK_QuatNorm(pose[8].q);
    const auto native=pose;
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),619);
    float legacyError=0;
    for(int k=0;k<3;++k)legacyError+=std::pow(g_fkPos[24][k]-saved[24][k],2.0f);
    legacyError=std::sqrt(legacyError);
    Check(legacyError>.05f,"test failed to disturb the native parent chain");
    std::cout<<"local_replay_under_changed_parent_error_mm="<<legacyError*1000<<'\n';
    VRIK_ReplayModelCache(reinterpret_cast<uint8_t*>(pose.data()));
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),619);
    for(int i=0;i<619;++i) {
        if(g_VRUpperOwned[i] || g_VRShadowSource[i]>=0)
            for(int k=0;k<3;++k)Near(g_fkPos[i][k],saved[i][k],2e-5f,"upper model pose moved during replay");
        else Check(std::memcmp(&pose[i],&native[i],48)==0,"replay rewrote native lower/control pose");
    }
    for(int k=0;k<3;++k)Near(g_VRBodyBone[6][k],g_fkPos[8][k],1e-6f,"body proxy still uses cached native leg");
}
void FkFixture() {
    auto local=Read<Bone>("player_local.bin");const auto model=Read<Bone>("player_model.bin");
    Check(local.size()==model.size(),"fixture pair count mismatch");
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(local.data()),static_cast<int>(local.size()));
    float worst=0;
    for(size_t i=0;i<local.size();++i)for(int k=0;k<3;++k)worst=std::max(worst,std::abs(g_fkPos[i][k]-model[i].p[k]));
    std::cout<<"production_fk_error_m="<<worst<<'\n';
    Check(worst<.00001f,"production FK disagrees with captured engine model pose");
}
void PacketAtomicity() {
    auto& seq=*reinterpret_cast<uint32_t*>(&shared[127]);
    seq=2;shared[1]=.2f;shared[9]=-.2f;shared[124]=.8f;RefreshHandsSnapshot();
    Near(SharedPose(1),.2f,1e-6f,"left hand packet");
    seq=3;shared[1]=9;shared[9]=8;shared[124]=7;RefreshHandsSnapshot();
    Near(SharedPose(1),.2f,1e-6f,"incomplete write must preserve the previous complete packet");
    Near(SharedPose(124),.8f,1e-6f,"head basis must stay with the retained hand packet");
    seq=4;RefreshHandsSnapshot();
    Near(SharedPose(1),9,1e-6f,"new complete packet must become visible");
    Near(SharedPose(9),8,1e-6f,"both hands advance together");
    Near(SharedPose(124),7,1e-6f,"reference advances with the hands");
}
void StartupPacket() {
    auto& seq=*reinterpret_cast<uint32_t*>(&shared[127]);
    seq=1;shared[0]=1;shared[1]=9;RefreshHandsSnapshot();
    Near(SharedPose(0),0,1e-6f,"unfinished first packet exposed a valid hand");
    Near(SharedPose(1),0,1e-6f,"unfinished first packet exposed partial position");
    seq=2;RefreshHandsSnapshot();Near(SharedPose(1),9,1e-6f,"first complete packet rejected");
}
void PairedConsumed() {
    shared[112]=.8f;shared[113]=.2f;shared[115]=2;
    shared[124]=.8f;shared[125]=.1f;shared[126]=-.2f;
    auto& seq=*reinterpret_cast<uint32_t*>(&shared[127]);seq=2;RefreshHandsSnapshot();
    float residual[3];VRIK_HeadResidual(.9f,.35f,residual);
    Near(residual[0],0,1e-7f,"a later native move shifted the old head packet");
    Near(residual[2],0,1e-7f,"a later forward consumption shifted the old head packet");
    Near(residual[1],.1f,1e-7f,"native consumption changed physical head height");
    seq=3;shared[112]=5;shared[124]=7;RefreshHandsSnapshot();
    VRIK_HeadResidual(.9f,.35f,residual);Near(residual[0],0,1e-7f,"torn consumed/head publication entered VRIK");
    seq=4;RefreshHandsSnapshot();VRIK_HeadResidual(.9f,.35f,residual);
    Near(residual[0],2,1e-7f,"complete head/consumption pair not advanced together");
}
void PairSelection() {
    const float q[4]={0,0,std::sin(.5f),std::cos(.5f)},model[3]={.1f,-.2f,1.6f};
    testNativeAvailable=testLuaAvailable=true;testNativePair.valid=testLuaPair.valid=1;
    std::copy_n(q,4,testNativePair.entityQuat);std::copy_n(q,4,testNativePair.camQuat);
    VRIK_QuatRotateVec(q,model,testNativePair.cameraMinusEntity);
    VRIK_QuatRotateVec(q,model,testNativePair.bodyCameraMinusEntity);
    testLuaPair.cameraMinusEntity[0]=.9f;testLuaPair.entityQuat[3]=testLuaPair.camQuat[3]=1;
    float p[3],r[4],basis[4],body[3];Check(VRIK_ComputeCamModel(p,r,basis,nullptr,body),"native pair unavailable");
    for(int k=0;k<3;++k)Near(p[k],model[k],1e-6f,"native camera mixed with Lua entity");
    QuatNear(basis,q,"pair basis changed");
    testNativePair.valid=0;
    Check(!VRIK_ComputeCamModel(p,r),"explicit native invalidation fell back to stale Lua pair");
    testNativeAvailable=false;
    Check(!VRIK_ComputeCamModel(p,r),"on-foot native camera must not switch to Lua coordinates");
    g_isInVehicle=1;
    Check(VRIK_ComputeCamModel(p,r),"vehicle lost its complete Lua camera/entity pair");
    Near(p[0],.9f,1e-6f,"vehicle selected another camera source");
    g_isInVehicle=0;CyberpunkVR_CamComposeAtWrite=0;
    Check(VRIK_ComputeCamModel(p,r),"legacy camera path lost its Lua pair");
    CyberpunkVR_CamComposeAtWrite=1;
}
void MissingCameraReplay() {
    auto pose=Read<Bone>("player_local.bin");SolveBody(pose);
    VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));VRIK_CaptureModelCache();
    const float saved[3]={g_fkPos[24][0],g_fkPos[24][1],g_fkPos[24][2]};
    // Live34212: native publication unavailable and expired Lua fallback. Neither
    // may switch the arm onto a differently calibrated, animated-head anchor.
    testNativeAvailable=false;testLuaAvailable=true;testLuaPair.valid=0;testLuaPair.unavailable=1;
    float p[3]={},q[4]={};VrikCameraStatus status{};
    Check(!VRIK_ComputeCamModel(p,q,nullptr,nullptr,nullptr,&status),"invalid fallback accepted");
    Check(status==VrikCameraStatus::Unavailable,"missing native camera did not report unavailability");
    pose=Read<Bone>("player_local.bin");pose[2].p[0]+=.1f;const auto native=pose;
    Check(VRIK_RestoreMissingCamera(reinterpret_cast<uint8_t*>(pose.data()),status,1234,GetTickCount64()),
          "missing camera frame did not replay the complete upper pose");
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),619);
    for(int k=0;k<3;++k)Near(g_fkPos[24][k],saved[k],2e-5f,"missing camera selected another hand anchor");
    Check(std::memcmp(&pose[8],&native[8],48)==0,"camera miss froze native leg animation");
    Check(g_solveCacheTick==1234,"replayed solve did not own the current batch");
}
void MissingNativeCentre() {
    testNativeAvailable=true;testNativePair.valid=0;testNativePair.unavailable=1;
    testLuaAvailable=true;testLuaPair.valid=1;
    float p[3],q[4];VrikCameraStatus status{};
    Check(!VRIK_ComputeCamModel(p,q,nullptr,nullptr,nullptr,&status),"incomplete native centre accepted");
    Check(status==VrikCameraStatus::Unavailable,"missing native centre was treated as camera detachment");
}
void InvalidatedCameraRelease() {
    auto pose=Read<Bone>("player_local.bin");SolveBody(pose);
    VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));VRIK_CaptureModelCache();
    testNativeAvailable=true;testNativePair.valid=0;testLuaAvailable=true;testLuaPair.valid=1;
    float p[3],q[4];VrikCameraStatus status{};
    Check(!VRIK_ComputeCamModel(p,q,nullptr,nullptr,nullptr,&status),"native invalidation fell through to Lua");
    Check(status==VrikCameraStatus::Invalidated,"native camera invalidation lost");
    pose=Read<Bone>("player_local.bin");const auto native=pose;
    Check(!VRIK_RestoreMissingCamera(reinterpret_cast<uint8_t*>(pose.data()),status,1234,GetTickCount64()),
          "detached camera retained VRIK ownership");
    Check(std::memcmp(pose.data(),native.data(),pose.size()*48)==0 && g_solveCacheN==0,"released camera rewrote native animation");
}
void MissingCameraExpiry() {
    auto pose=Read<Bone>("player_local.bin");SolveBody(pose);
    VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));VRIK_CaptureModelCache();
    pose=Read<Bone>("player_local.bin");const auto native=pose;
    Check(!VRIK_RestoreMissingCamera(reinterpret_cast<uint8_t*>(pose.data()),VrikCameraStatus::Unavailable,
          1234,GetTickCount64()+251),"unavailable camera retained stale pose indefinitely");
    Check(std::memcmp(pose.data(),native.data(),pose.size()*48)==0 && g_solveCacheN==0,"expired pose rewrote native animation");
}
void ModelOrigin() {
    const float native[3]={.1f,0,1.6f},basis[4]={0,0,.707106781f,.707106781f};
    shared[115]=0;*reinterpret_cast<uint32_t*>(&shared[127])=2;RefreshHandsSnapshot();
    g_viewPktValid=true;g_viewPkt[7]=2;g_viewPkt[4]=.02f;g_viewPkt[5]=-.03f;g_viewPkt[6]=.04f;
    float result[3];Check(VRIK_ResolveViewModelPos(native,basis,result),"model view origin unavailable");
    Near(result[0],.07f,1e-6f,"world delta rotated in wrong basis");
    Near(result[1],-.02f,1e-6f,"model offset was rotated twice");Near(result[2],1.64f,1e-6f,"view height");
    g_VRCamPosValid=g_VRCamPairValid=1;
    g_VRCamPairLocalX=.1f;g_VRCamPairLocalY=0;g_VRCamPairLocalZ=1.6f;
    float oldWorld[3],oldModel[3],inverse[4];
    Check(VRIK_ResolveViewPos(oldWorld),"legacy counterexample unavailable");
    VRIK_QuatConj(basis,inverse);VRIK_QuatRotateVec(inverse,oldWorld,oldModel);
    float oldError=0;for(int k=0;k<3;++k)oldError+=std::pow(oldModel[k]-result[k],2.0f);
    oldError=std::sqrt(oldError);Check(oldError>.14f,"mixed-pair counterexample did not move origin");
    std::cout<<"legacy_mixed_origin_error_mm="<<oldError*1000<<'\n';
    // Unrelated Lua globals must have no effect on the native model origin.
    g_VREntityPosX=800;g_VRCamPairLocalX=-.1f;g_VREntityQK=0;g_VREntityQR=1;
    float again[3];Check(VRIK_ResolveViewModelPos(native,basis,again),"model view origin lost");
    for(int k=0;k<3;++k)Near(again[k],result[k],1e-6f,"Lua scalar mutation moved native hand origin");
    g_viewPkt[4]=3;Check(!VRIK_ResolveViewModelPos(native,basis,result),"implausible delta accepted");
}
void NeutralViewReference() {
    const float neutral[4]={0,0,0,1},head[4]={0,std::sin(.4f),0,std::cos(.4f)};
    const float expected[4]={0,0,std::sin(.4f),std::cos(.4f)};float actual[4];
    Check(VRIK_AlignViewToHands(neutral,neutral,head,actual),"neutral reference rejected");
    QuatNear(actual,expected,"identity view head skipped rebase");
    const float bad[4]={};Check(!VRIK_AlignViewToHands(neutral,bad,head,actual),"invalid quaternion accepted");
}
XrPosef Compose(const XrPosef& a,const XrPosef& b) {
    const auto p=RotateVector(a.orientation,b.position);
    return {MultiplyQuat(a.orientation,b.orientation),{a.position.x+p.x,a.position.y+p.y,a.position.z+p.z}};
}
void PoseChain() {
    const auto native=Read<Bone>("player_local.bin");
    float worstTarget=0,worstWrist=0,worstShadow=0;
    int solved=0;
    // Same test sequence in two tracking origins: a recenter changes representation,
    // not the physical relationship between the HMD and controllers.
    for(int originCase=0;originCase<2;++originCase)for(int sample=0;sample<181;++sample) {
        const float t=static_cast<float>(sample)*.017f;
        const XrPosef origin=originCase ? XrPosef{{0,std::sin(.6f),0,std::cos(.6f)},{3,2,-5}} : XrPosef{{0,0,0,1},{0,0,0}};
        const XrQuaternionf yaw{0,std::sin(.3f*std::sin(t)),0,std::cos(.3f*std::sin(t))};
        const XrQuaternionf pitch{std::sin(.2f*std::sin(2*t)),0,0,std::cos(.2f*std::sin(2*t))};
        const XrPosef head{MultiplyQuat(yaw,pitch),{.05f*std::sin(t),.03f*std::sin(2*t),.04f*std::cos(t)}};
        const float entityQ[4]={0,0,std::sin(.4f),std::cos(.4f)};
        const float heading[4]={0,0,std::sin(.3f),std::cos(.3f)};
        const float hq[4]={head.orientation.x,head.orientation.y,head.orientation.z,head.orientation.w};
        const float hqGame[4]={hq[0],-hq[2],hq[1],hq[3]};
        const auto offset=cvr::camera::ComposeAnchorTranslation({head.position.x,-head.position.z,head.position.y},
            {},{.03f,-.06f,.02f},.6f,.8f);
        const float baseModel[3]={0,0,1.6f};float baseWorld[3];VRIK_QuatRotateVec(entityQ,baseModel,baseWorld);
        testNativeAvailable=true;testNativePair.valid=1;std::copy_n(entityQ,4,testNativePair.entityQuat);
        VRIK_QuatMul(heading,hqGame,testNativePair.camQuat);
        const float deltas[3]={offset.x,offset.y,offset.z};
        for(int k=0;k<3;++k) { testNativePair.cameraMinusEntity[k]=baseWorld[k]+deltas[k];testNativePair.bodyCameraMinusEntity[k]=baseWorld[k]; }
        for(int side=0;side<2;++side) {
            const XrPosef controller{{0,0,0,1},{side ? .23f : -.23f,-.3f,-.28f}};
            const auto local=RelativePose(Compose(origin,head),Compose(origin,controller));
            const int slot=side*8;shared[slot]=1;
            shared[slot+1]=local.position.x;shared[slot+2]=local.position.y;shared[slot+3]=local.position.z;
            shared[slot+4]=local.orientation.x;shared[slot+5]=local.orientation.y;shared[slot+6]=local.orientation.z;shared[slot+7]=local.orientation.w;
        }
        for(int k=0;k<4;++k)shared[16+k]=hq[k];
        shared[115]=0;*reinterpret_cast<uint32_t*>(&shared[127])=2u*static_cast<uint32_t>(++solved);
        RefreshHandsSnapshot();
        // A differently labelled view, including an exactly neutral head reference.
        const float oldHead[4]={sample%2 ? std::sin(.12f) : 0.0f,0,0,sample%2 ? std::cos(.12f) : 1.0f};
        const float oldGame[4]={oldHead[0],-oldHead[2],oldHead[1],oldHead[3]};float oldView[4];
        VRIK_QuatMul(heading,oldGame,oldView);
        for(int k=0;k<4;++k) { shared[104+k]=oldView[k];shared[227+k]=oldHead[k]; }
        shared[108]=shared[109]=shared[110]=0;shared[111]=2;shared[141]=.6f;shared[142]=1;
        *reinterpret_cast<uint32_t*>(&shared[143])=2u*static_cast<uint32_t>(solved);
        VRIK_LatchViewPacket();Check(g_viewPktValid,"view packet failed");
        float camera[3],cameraQ[4],pairQ[4],body[3],view[3],worldQ[4],modelQ[4],inverse[4];
        Check(VRIK_ComputeCamModel(camera,cameraQ,pairQ,nullptr,body),"camera pair failed");
        Check(VRIK_ResolveViewModelPos(camera,pairQ,view),"model origin failed");
        const float latchedHead[4]={SharedPose(16),SharedPose(17),SharedPose(18),SharedPose(19)};
        Check(VRIK_AlignViewToHands(g_viewPkt,&g_viewPkt[13],latchedHead,worldQ),"orientation alignment failed");
        VRIK_QuatConj(pairQ,inverse);VRIK_QuatMul(inverse,worldQ,modelQ);
        auto pose=native;SolveBody(pose,cameraQ);
        for(int side=0;side<2;++side) {
            const int slot=side*8,upper=side ? 18:17,fore=side ? 21:20,hand=side ? 24:23;
            float localPos[3],localQ[4];for(int k=0;k<3;++k)localPos[k]=SharedPose(slot+1+k);
            for(int k=0;k<4;++k)localQ[k]=SharedPose(slot+4+k);
            const float tuning[3]={.004f,-.003f,.002f};float target[3],controllerQ[4];
            VRIK_BuildViewHandTarget(view,modelQ,localPos,localQ,1,tuning,target,controllerQ);
            const float controllerBase[3]={side ? .23f : -.23f,.28f,-.3f};
            float expectedWorld[3],expected[3];VRIK_QuatRotateVec(heading,controllerBase,expectedWorld);
            VRIK_QuatRotateVec(inverse,expectedWorld,expected);
            for(int k=0;k<3;++k) {
                const float bake[3]={.03f,-.06f,.02f};expected[k]+=baseModel[k]+bake[k]+tuning[k];
                worstTarget=std::max(worstTarget,std::abs(target[k]-expected[k]));
            }
            const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0};
            float wrist[4]={0,std::sin(-.785398163f),0,std::cos(-.785398163f)},handQ[4];
            VRIK_QuatMul(controllerQ,wrist,handQ);
            VRIK_SolveArm(reinterpret_cast<uint8_t*>(pose.data()),upper,fore,hand,target,handQ,right,up,forward,0,1,side==0,false);
            VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),619);
            for(int k=0;k<3;++k)worstWrist=std::max(worstWrist,std::abs(g_fkPos[hand][k]-target[k]));
            QuatNear(g_fkRot[hand],handQ,"IK wrist orientation differs from target");
        }
        VRIK_SyncShadowUpper(reinterpret_cast<uint8_t*>(pose.data()));
        for(int bone=0;bone<619;++bone)if(g_VRShadowSource[bone]>=0)
            for(int k=0;k<3;++k)worstShadow=std::max(worstShadow,std::abs(g_fkPos[bone][k]-g_fkPos[g_VRShadowSource[bone]][k]));
    }
    std::cout<<"packets="<<solved<<" target_error_mm="<<worstTarget*1000<<" wrist_error_mm="<<worstWrist*1000<<" shadow_error_mm="<<worstShadow*1000<<'\n';
    Check(worstTarget<.00001f,"head rotation/translation moved a stationary controller target");
    Check(worstWrist<.002f,"production IK missed reachable wrist target");
    Check(worstShadow<.00001f,"shadow pose diverged during HMD motion");
}
void PassengerWindow() {
    using namespace cvr::anim;
    Check(IsPassengerWindowCombat(2),"passenger Combat state not recognized");
    for(int state : {-1,0,1,3,4,5,6,7})
        Check(!IsPassengerWindowCombat(state),"ordinary seat/driver/turret acquired window-combat pose");
    Check(IsDrivingVehicleState(1) && IsDrivingVehicleState(6),"driver states lost");
    for(int state : {-1,0,2,3,4,5,7})
        Check(!IsDrivingVehicleState(state),"passenger/unknown state treated as driver");
    Setup("passenger-window");
    float requested[2][2]{};
    for(int mode=0;mode<2;++mode) {
        auto pose=Read<Bone>("vehicle_pose.bin");const auto original=pose;
        auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        VRIK_BeginBodySolve(buffer);VRIK_ComputeFK(buffer,static_cast<int>(pose.size()));
        float right[3],up[3],forward[3];
        Check(VRIK_BodyAxesFromRig(right,up,forward,mode!=0),"vehicle shoulder basis unavailable");
        float anchor[3];std::copy_n(g_fkPos[g_VRNeckIdx],3,anchor);
        for(int side=0;side<2;++side) {
            float joint[3];
            VRIK_AnchorShoulder(buffer,side ? g_VRLeftUpperArmIdx:g_VRRightUpperArmIdx,
                                anchor,side!=0,right,up,.14f,joint);
            requested[mode][side]=g_VRIKDbgClav[side][6];
            if(mode)Near(g_VRIKDbgClav[side][7],requested[mode][side],.001f,"window shoulder hit angular clamp");
        }
        for(int bone : {g_VRHipsIdx,g_VRLeftUpLegIdx,g_VRRightUpLegIdx,g_VRLeftLegIdx,g_VRRightLegIdx,g_VRLeftFootIdx,g_VRRightFootIdx})
            Check(std::memcmp(&pose[bone],&original[bone],sizeof(Bone))==0,"window shoulders changed seated lower body");
    }
    Check(requested[0][1]>100,"captured passenger pose no longer reproduces old shoulder restriction");
    Check(requested[1][0]<55 && requested[1][1]<55,"chest basis did not free passenger shoulders");
    std::cout<<"clavicle_requested_before="<<requested[0][0]<<','<<requested[0][1]
             <<" after="<<requested[1][0]<<','<<requested[1][1]<<'\n';
}

void VehicleUpperBody() {
    Setup("passenger-window");
    auto distance=[](const float* a,const float* b) { return std::hypot(a[0]-b[0],a[1]-b[1],a[2]-b[2]); };
    const auto native=Read<Bone>("vehicle_pose.bin");
    const float eye[3]={.4653063416f,.180765152f,1.21284008f};
    cvr::camera::AnchorVector cameraFromEyes{};
    Check(cvr::camera::ReadNeckCameraEyeOffset(&cameraFromEyes),"passenger camera mount unavailable");
    auto cameraBaseline=native;
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(cameraBaseline.data()),static_cast<int>(cameraBaseline.size()));
    const int feet[2]={g_VRRightFootIdx,g_VRLeftFootIdx};
    float footPosition[2][3],footRotation[2][4];
    for(int side=0;side<2;++side) {
        std::copy_n(g_fkPos[feet[side]],3,footPosition[side]);
        std::copy_n(g_fkRot[feet[side]],4,footRotation[side]);
    }
    auto checkFeet=[&] {
        for(int side=0;side<2;++side) {
            for(int k=0;k<3;++k)Near(g_fkPos[feet[side]][k],footPosition[side][k],.0001f,"Combat lost its native foot contact");
            QuatNear(g_fkRot[feet[side]],footRotation[side],"Combat rotated a planted foot");
        }
    };
    std::vector<std::pair<int,std::array<float,3>>> cameraControls;
    for(int i=0;i<g_VRBoneCount;++i)if(names[i].starts_with("Torso_fppCamera"))
        cameraControls.push_back({i,{g_fkPos[i][0],g_fkPos[i][1],g_fkPos[i][2]}});
    float worstEye=0,worstForward=0,worstReplay=0,worstHand=0;
    for(float yawDegrees : {-179.0f,-120.0f,-60.0f,0.0f,60.0f,120.0f,179.0f})
    for(float pitchDegrees : {-40.0f,0.0f,40.0f}) {
        const float yaw=yawDegrees*.01745329252f,pitch=pitchDegrees*.01745329252f;
        const float yawQ[4]={0,0,std::sin(yaw*.5f),std::cos(yaw*.5f)};
        const float pitchQ[4]={std::sin(pitch*.5f),0,0,std::cos(pitch*.5f)};
        float head[4];VRIK_QuatMul(yawQ,pitchQ,head);
        auto pose=native;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        VRIK_BeginBodySolve(buffer);
        Check(VRIK_PlaceVehicleCombatUpper(buffer,eye,head,g_VRHeadBoneIdx),"passenger upper solve rejected captured rig");
        Check(g_VRIKVehicleUpperActive && g_VRIKBodyEyeBound,"passenger upper solve did not bind the head");
        const auto mountedOffset=cvr::camera::RotateAnchorYaw(cameraFromEyes,yaw);
        const float expectedEyes[3]={eye[0]-mountedOffset.x,eye[1]-mountedOffset.y,eye[2]-.10f};
        float error=0;for(int k=0;k<3;++k)error+=(g_VRIKBodyEyeSolved[k]-expectedEyes[k])*(g_VRIKBodyEyeSolved[k]-expectedEyes[k]);
        worstEye=std::max(worstEye,std::sqrt(error));
        float right[3],up[3],forward[3];Check(VRIK_BodyAxesFromRig(right,up,forward,true),"passenger chest axes missing");
        const float horizontal=std::hypot(forward[0],forward[1]);
        const float alignment=(forward[0]*(-std::sin(yaw))+forward[1]*std::cos(yaw))/horizontal;
        worstForward=std::max(worstForward,1-alignment);
        Check(horizontal>.01f && alignment>.999f,"passenger chest did not face HMD yaw");
        for(int i=0;i<g_VRSpineCount;++i) {
            const int bone=g_VRSpineIdx[i];
            Check(std::isfinite(g_fkPos[bone][0]),"passenger spine produced invalid positions");
        }
        Check(g_fkPos[g_VRNeck1Idx][2]>g_fkPos[g_VRNeckIdx][2]+.025f &&
              g_fkPos[g_VRHeadBoneIdx][2]>g_fkPos[g_VRNeck1Idx][2]+.025f,"Combat folded the upper neck downward");
        checkFeet();
        const auto solved=pose;
        VRIK_BeginBodySolve(buffer);
        Check(VRIK_PlaceVehicleCombatUpper(buffer,eye,head,g_VRHeadBoneIdx),"repeated passenger solve failed");
        for(int i=0;i<g_VRSpineCount;++i)QuatNear(pose[g_VRSpineIdx[i]].q,solved[g_VRSpineIdx[i]].q,"passenger upper pose accumulates across calls");
        float anchor[3];std::copy_n(g_fkPos[g_VRNeckIdx],3,anchor);
        VRIK_BodyAxesFromRig(right,up,forward,true);
        for(int side=0;side<2;++side) {
            const int upper=side ? g_VRLeftUpperArmIdx:g_VRRightUpperArmIdx;
            const int fore=side ? g_VRLeftForeArmIdx:g_VRRightForeArmIdx;
            const int hand=side ? g_VRLeftBoneIdx:g_VRRightBoneIdx;
            float joint[3];VRIK_AnchorShoulder(buffer,upper,anchor,side!=0,right,up,.14f,joint);
            const float local[3]={side ? -.2f:.2f,-.3f,-.4f},id[4]={0,0,0,1},zero[3]{};
            float target[3],handRotation[4];
            VRIK_BuildViewHandTarget(eye,head,local,id,1.05f,zero,target,handRotation);
            VRIK_SolveArm(buffer,upper,fore,hand,target,handRotation,right,up,forward,0,1,side!=0,false);
            VRIK_ComputeFK(buffer,static_cast<int>(pose.size()));
            if(distance(g_fkPos[hand],target)>.002f)
                std::cout<<"vehicle hand miss yaw="<<yawDegrees<<" pitch="<<pitchDegrees<<" side="<<side
                    <<" target="<<target[0]<<','<<target[1]<<','<<target[2]
                    <<" shoulder="<<g_fkPos[upper][0]<<','<<g_fkPos[upper][1]<<','<<g_fkPos[upper][2]
                    <<" span="<<distance(g_fkPos[upper],target)
                    <<" arm="<<distance(g_fkPos[upper],g_fkPos[fore])+distance(g_fkPos[fore],g_fkPos[hand])<<'\n';
            for(int k=0;k<3;++k)worstHand=std::max(worstHand,std::abs(g_fkPos[hand][k]-target[k]));
        }
        VRIK_SyncShadowUpper(buffer);VRIK_CaptureModelCache();
        for(const auto& control:cameraControls) {
            Check(std::memcmp(&pose[control.first],&native[control.first],sizeof(Bone))==0,"passenger upper pose rewrote camera controls");
            for(int k=0;k<3;++k)Near(g_fkPos[control.first][k],control.second[k],.00001f,"passenger upper pose fed motion back into the camera rig");
        }
        for(int bone : {0,1})
            Check(std::memcmp(&pose[bone],&native[bone],sizeof(Bone))==0,"Combat pose moved the mount root");
        QuatNear(pose[g_VRHipsIdx].q,native[g_VRHipsIdx].q,"Combat changed the seated pelvis orientation");
        bool pelvisCached=false;
        for(int i=0;i<g_solveCacheN;++i)pelvisCached|=g_solveCacheIdx[i]==g_VRHipsIdx;
        Check(pelvisCached,"Combat replay omitted the HMD-fitted pelvis");
        const std::array<float,3> expectedHead{g_fkPos[g_VRHeadBoneIdx][0],g_fkPos[g_VRHeadBoneIdx][1],g_fkPos[g_VRHeadBoneIdx][2]};
        pose=native;VRIK_ReplayModelCache(reinterpret_cast<uint8_t*>(pose.data()));
        for(int k=0;k<3;++k)worstReplay=std::max(worstReplay,std::abs(g_fkPos[g_VRHeadBoneIdx][k]-expectedHead[k]));
        for(int bone : {0,1})
            Check(std::memcmp(&pose[bone],&native[bone],sizeof(Bone))==0,"Combat replay changed the mount root");
        checkFeet();
        for(int bone=0;bone<g_VRBoneCount;++bone)if(g_VRShadowSource[bone]>=0 && g_VRShadowSource[bone]!=g_VRHipsIdx)
            for(int k=0;k<3;++k)Near(g_fkPos[bone][k],g_fkPos[g_VRShadowSource[bone]][k],.00005f,"passenger upper shadow diverged");
        VRIK_BeginBodySolve(buffer);
        for(int bone : {g_VRHipsIdx,g_VRLeftUpLegIdx,g_VRRightUpLegIdx,g_VRLeftLegIdx,g_VRRightLegIdx,g_VRLeftFootIdx,g_VRRightFootIdx})
            Check(std::memcmp(&pose[bone],&native[bone],sizeof(Bone))==0,"leaving/re-solving Combat retained modified native lower pose");
    }
    std::cout<<"eye_error_mm="<<worstEye*1000<<" facing_error="<<worstForward<<" replay_error_mm="<<worstReplay*1000<<" hand_error_mm="<<worstHand*1000<<'\n';
    Check(worstEye<.002f,"passenger head missed reachable HMD target");
    Check(worstReplay<.00002f,"vehicle animation erased the upper solve on replay");
    Check(worstHand<.002f,"passenger hands cannot reach normal controller targets around HMD");
}

void CombatHmdBody() {
    Setup("passenger-window");
    const auto original=Read<Bone>("low_camera_pose.bin");
    auto pose=original;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
    VRIK_ComputeFK(buffer,static_cast<int>(pose.size()));
    const float before=g_fkPos[g_VRHeadBoneIdx][2]-g_fkPos[g_VRNeck1Idx][2];
    Check(before<-.04f,"paused capture no longer reproduces the inverted upper neck");
    // PID20928: midpoint of the two actual eye components, in their common
    // owner's frame. The previous Lua target was MAIN's eye, 32.57mm aside.
    const float centre[3]={.492536226f,.114632202f,1.17155708f};
    const float oldEye[3]={.460008025f,.114009172f,1.1730845f};
    const float head[4]={.024741674f,-.007709608f,.999661922f,-.002139449f};
    const float oldNeckClearance=centre[2]-g_fkPos[g_VRNeckIdx][2];
    const int feet[2]={g_VRRightFootIdx,g_VRLeftFootIdx};
    float contacts[2][3];for(int side=0;side<2;++side)std::copy_n(g_fkPos[feet[side]],3,contacts[side]);
    VRIK_BeginBodySolve(buffer);VRIK_PinGirdleTranslations(buffer);
    Check(VRIK_PlaceVehicleCombatUpper(buffer,centre,head,g_VRHeadBoneIdx),"captured low-camera Combat solve failed");
    const float after=g_fkPos[g_VRHeadBoneIdx][2]-g_fkPos[g_VRNeck1Idx][2];
    const float clearance=centre[2]-g_fkPos[g_VRNeckIdx][2];
    Check(after>.04f && clearance>.20f,"HMD remains at the folded neck/body surface");
    const float yaw=std::atan2(2*(head[2]*head[3]-head[0]*head[1]),1-2*(head[0]*head[0]+head[2]*head[2]));
    cvr::camera::AnchorVector offset{};Check(cvr::camera::ReadNeckCameraEyeOffset(&offset),"mount disappeared");
    offset=cvr::camera::RotateAnchorYaw(offset,yaw);
    const float expected[3]={centre[0]-offset.x,centre[1]-offset.y,centre[2]-offset.z};
    for(int k=0;k<3;++k)Near(g_VRIKBodyEyeSolved[k],expected[k],.00001f,"body did not bind to the centred 15/10 mount");
    for(int side=0;side<2;++side)for(int k=0;k<3;++k)
        Near(g_fkPos[feet[side]][k],contacts[side][k],.0001f,"low-camera pose moved a native foot contact");
    for(int i=0;i<g_VRBoneCount;++i)if(names[i].starts_with("Torso_fppCamera") || i<2)
        Check(std::memcmp(&pose[i],&original[i],sizeof(Bone))==0,"fitting Combat body altered the camera/mount root");
    VRIK_SyncShadowUpper(buffer);VRIK_CaptureModelCache();
    // A different animation buffer must retain its own native lower pose, so
    // replay followed by a fresh solve or leaving Combat cannot ratchet feet.
    auto other=original;auto* replay=reinterpret_cast<uint8_t*>(other.data());
    VRIK_ReplayModelCache(replay);VRIK_BeginBodySolve(replay);
    for(int bone:{g_VRHipsIdx,g_VRRightUpLegIdx,g_VRRightLegIdx,g_VRRightFootIdx,
                  g_VRLeftUpLegIdx,g_VRLeftLegIdx,g_VRLeftFootIdx})
        Check(std::memcmp(&other[bone],&original[bone],sizeof(Bone))==0,"cross-buffer replay lost the native lower pose");
    std::cout<<"old_eye_centre_error_mm="<<std::hypot(centre[0]-oldEye[0],centre[1]-oldEye[1],centre[2]-oldEye[2])*1000
        <<" neck_rise_before_mm="<<before*1000<<" after_mm="<<after*1000
        <<" camera_above_neck_before_mm="<<oldNeckClearance*1000<<" after_mm="<<clearance*1000<<'\n';
}
void VehiclePairMiss() {
    g_isInVehicle=true;CyberpunkVR_VrikVehicleFullEntityQuat=1;
    testLuaAvailable=testNativeAvailable=true;
    testLuaPair.valid=testNativePair.valid=1;
    testLuaPair.entityQuat[3]=testLuaPair.camQuat[3]=1;
    testLuaPair.cameraMinusEntity[2]=.72176f;
    testNativePair.entityQuat[3]=testNativePair.camQuat[3]=1;
    testNativePair.cameraMinusEntity[2]=1.7f;
    float p[3]{},q[4]{};VrikCameraStatus status{};
    Check(VRIK_ComputeCamModel(p,q,nullptr,nullptr,nullptr,&status),"mounted pair missing");
    Near(p[2],.72176f,1e-6f,"mounted pair used the on-foot camera basis");
    testLuaAvailable=false;g_VRCamPosValid=1;g_VRCamPairValid=1;
    g_VRCamPairLocalZ=2.5f;
    Check(!VRIK_ComputeCamModel(p,q,nullptr,nullptr,nullptr,&status),"vehicle switched to native/scalar frame on a Lua miss");
    Check(status==VrikCameraStatus::Unavailable,"transient vehicle miss revoked ownership");
    testLuaAvailable=true;testLuaPair.valid=0;testLuaPair.unavailable=1;
    Check(!VRIK_ComputeCamModel(p,q,nullptr,nullptr,nullptr,&status),"expired vehicle pair accepted");
    Check(status==VrikCameraStatus::Unavailable,"expired vehicle pair selected another anchor");
    testLuaPair.unavailable=0;
    Check(!VRIK_ComputeCamModel(p,q,nullptr,nullptr,nullptr,&status) && status==VrikCameraStatus::Invalidated,
          "detached mounted camera retained ownership");
}
void VehicleMissingCamera() {
    for(int mask:{1,2,3}) {
        auto pose=Read<Bone>("player_local.bin");SolveBody(pose);
        const auto saved=pose;
        VRIK_CaptureLocalArmCache(reinterpret_cast<uint8_t*>(pose.data()),(mask&1)!=0,(mask&2)!=0);
        Check(!g_solveCacheModel && g_solveCacheN>0,"seated arm cache missing");
        std::array<bool,VRIK_MAX_BONES> owned{};
        for(int i=0;i<g_solveCacheN;++i)owned[g_solveCacheIdx[i]]=true;
        Check(owned[g_VRRightBoneIdx]==bool(mask&1) && owned[g_VRLeftBoneIdx]==bool(mask&2),
              "cache captured an arm owned by the wheel animation");
        for(size_t i=0;i<pose.size();++i)pose[i].p[0]+=.01f;
        const auto native=pose;const auto stamp=GetTickCount64();
        Check(VRIK_RestoreMissingCamera(reinterpret_cast<uint8_t*>(pose.data()),VrikCameraStatus::Unavailable,1234,stamp),
              "seated camera miss did not preserve the previous arm solve");
        for(size_t i=0;i<pose.size();++i)Check(std::memcmp(&pose[i],owned[i] ? &saved[i]:&native[i],48)==0,
              "seated fallback changed a native bone or lost a solved arm");
        pose=native;
        Check(!VRIK_RestoreMissingCamera(reinterpret_cast<uint8_t*>(pose.data()),VrikCameraStatus::Unavailable,1235,stamp+251),
              "seated fallback refreshed its own lifetime");
        Check(std::memcmp(pose.data(),native.data(),pose.size()*48)==0,"expired seated fallback modified the native pose");
    }
}
void VehicleHandFrame() {
    // Paused prologue: entity yaw is 51.657deg; the on-foot census is still zero.
    const float entity[4]={.00381906028f,.00161385909f,.4356752932f,.9000944495f};
    const float view[4]={0,0,-.8978280425f,.4403462708f};
    const float cameraRelative[3]={.154296875f,.47021484375f,1.2144699097f};
    const float handQ[4]={-.1494381428f,0,0,.988771081f},zero[3]{};
    float inverse[4],modelView[4],modelOrigin[3],legacy[4];
    VRIK_WorldToModelRotation(0,entity,inverse);
    VRIK_WorldToModelRotation(0,nullptr,legacy);
    VRIK_QuatMul(inverse,view,modelView);VRIK_QuatRotateVec(inverse,cameraRelative,modelOrigin);
    for(int side=0;side<2;++side) {
        const float scale=side ? 1.083f:1.05f;
        const float hand[3]={side ? -.2f:.2f,-.3f,-.4f};
        float target[3],rotation[4],expected[3],expectedQ[4];
        VRIK_BuildViewHandTarget(modelOrigin,modelView,hand,handQ,scale,zero,target,rotation);
        VRIK_BuildViewHandTarget(cameraRelative,view,hand,handQ,scale,zero,expected,expectedQ);
        float world[3],worldQ[4];VRIK_QuatRotateVec(entity,target,world);VRIK_QuatMul(entity,rotation,worldQ);
        for(int k=0;k<3;++k)Near(world[k],expected[k],.00001f,"mounted hand target used stale on-foot heading");
        QuatNear(worldQ,expectedQ,"mounted wrist orientation lost full entity basis");
        float wrong[3];VRIK_QuatRotateVec(entity,expected,wrong);
        float error=0;for(int k=0;k<3;++k)error+=(wrong[k]-expected[k])*(wrong[k]-expected[k]);
        Check(error>.04f,"control no longer exposes stale-yaw frame error");
    }
    const float id[4]={0,0,0,1};QuatNear(legacy,id,"on-foot fallback yaw changed");
}

void CoordinateRoundtrip() {
    // Physical reference and model basis are different frames, including after recenter.
    const float p[3]={.32f,-.19f,.71f};
    for(float yaw : {0.0f,.8f,-2.3f,3.14159265f}) {
        float q[4]={0,0,std::sin(yaw/2),std::cos(yaw/2)},inv[4],world[3],back[3];
        VRIK_QuatConj(q,inv);VRIK_QuatRotateVec(q,p,world);VRIK_QuatRotateVec(inv,world,back);
        for(int k=0;k<3;++k)Near(back[k],p[k],1e-6f,"world/model conversion is not invertible");
    }
}
void BodyWriteCensus() {
    auto pose=Read<Bone>("player_local.bin");const auto before=pose;
    VRIK_DampenTorsoWeaponPose(reinterpret_cast<uint8_t*>(pose.data()));
    VRIK_ComputeFK(reinterpret_cast<uint8_t*>(pose.data()),static_cast<int>(pose.size()));
    const float camera[3]={0,0,1.60f},rotation[4]={0,0,0,1},forward[3]={0,1,0};
    VRIK_PlaceBodyUnderHMD(reinterpret_cast<uint8_t*>(pose.data()),camera,rotation,22,forward);
    std::cout<<"changed_bones=";
    for(size_t i=0;i<pose.size();++i)if(std::memcmp(&pose[i],&before[i],sizeof(Bone))!=0)std::cout<<i<<',';
    std::cout<<"\nhead_model_q=";for(float q:g_fkRot[22])std::cout<<q<<',';std::cout<<'\n';
    Check(std::isfinite(g_fkPos[22][2]),"solver produced nonfinite head");
}
void SwimmingBody() {
    const auto original=Read<Bone>("player_local.bin");auto pose=original;
    auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
    cvr::camera::AnchorVector mount{};
    Check(cvr::camera::ReadNeckCameraEyeOffset(&mount),"neutral neck mount is unavailable");
    shared[115]=2;shared[114]=1.1f;shared[89]=shared[90]=-.7f;
    g_pSharedHands=shared.data();*reinterpret_cast<uint32_t*>(&shared[127])=2;RefreshHandsSnapshot();
    // Native swimming moves its camera far below/forward of the land base.
    // That translation and physical descent must move the whole fitted body,
    // never reactivate a land squat/hinge or detach the neck from the HMD.
    for(float z:{.4f,.93f,1.6f})for(float pitch:{-.9f,0.0f,.7f}) {
        pose=original;VRIK_ComputeFK(buffer,static_cast<int>(pose.size()));
        const float centre[3]={.19f,.48f,z},head[4]={std::sin(pitch*.5f),0,0,std::cos(pitch*.5f)};
        Check(VRIK_PlaceSwimmingBody(buffer,centre,head,22),"water body placement failed");
        Check(g_VRIKBodyBendAngle==0 && s_vrSharedSquatDrop==0,"water retained a crouch/bend");
        Check(g_VRIKBodyEyeBound && !g_VRIKVehicleUpperActive,"water selected a vehicle posture");
        const float expected[3]={centre[0]-mount.x,centre[1]-mount.y,centre[2]-mount.z};
        for(int k=0;k<3;++k)Near(g_VRIKBodyEyeSolved[k],expected[k],.00002f,"water lost neutral 15/10 camera mount");
        for(int bone:{0,1,5,6,8,9,11,12})
            Check(std::memcmp(&pose[bone],&original[bone],sizeof(Bone))==0,"water bent native legs or wrote the root");
        const auto once=pose;
        Check(VRIK_PlaceSwimmingBody(buffer,centre,head,22),"repeat water solve failed");
        for(int bone=0;bone<g_VRBoneCount;++bone)for(int k=0;k<3;++k)
            Near(pose[bone].p[k],once[bone].p[k],.00001f,"repeated water solve accumulated a translation");
    }
    shared[114]=shared[89]=shared[90]=0;*reinterpret_cast<uint32_t*>(&shared[127])=4;RefreshHandsSnapshot();
    SolveBody(pose);Check(g_VRIKBodyBendAngle==0 && s_vrSharedSquatDrop==0,"water-to-land transition retained a posture offset");
}
void LadderGrips() {
    Setup("ladder");const auto reference=Read<Bone>("reference_full.bin");
    for(int side=0;side<2;++side) {
        const auto native=Read<Bone>(side ? "side-grip-right.bin":"side-grip-left.bin");
        auto pose=native;auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
        const int hand=side ? g_VRRightBoneIdx:g_VRLeftBoneIdx;
        std::vector<int> fingers;
        const std::string prefix=side ? "Right":"Left";
        for(const auto& f:cvr::ladder::profile::fingers) {
            const auto it=std::find(names.begin(),names.end(),prefix+f.suffix);
            Check(it!=names.end(),"captured ladder finger missing in rig");fingers.push_back(int(it-names.begin()));
        }
        VRIK_ApplyLadderFingers(buffer,side,1,nullptr);
        for(int i=0;i<g_VRBoneCount;++i) {
            if(std::find(fingers.begin(),fingers.end(),i)!=fingers.end())QuatNear(pose[i].q,native[i].q,"native side finger was changed");
            else Check(!std::memcmp(&pose[i],&native[i],sizeof(Bone)),"grip wrote outside its hand");
            Check(!std::memcmp(pose[i].p,native[i].p,16) && !std::memcmp(pose[i].s,native[i].s,16),"grip stretched fingers");
        }
        VRIK_ComputeFK(buffer,VRIK_FKCount());float originalWrist[3],originalRot[4];
        std::copy_n(g_fkPos[hand],3,originalWrist);std::copy_n(g_fkRot[hand],4,originalRot);
        float height=0;
        for(const char* name:{"HandIndex1","HandMiddle1","HandRing1","HandPinky1"})
            height+=g_fkPos[std::find(names.begin(),names.end(),prefix+name)-names.begin()][2]*.25f;
        cvr::ladder::Geometry geometry;geometry.position={0,.5f,0};geometry.normal={0,-1,0};geometry.up={0,0,1};geometry.height=8;geometry.identity=1;geometry.Prepare();
        cvr::ladder::Frame frame;frame.valid=true;frame.handValid[side]=true;frame.sequence=frame.origin=1;frame.stamp=1000000;
        frame.world[side]={side ? .3f:-.3f,.475f,height};
        cvr::ladder::Climber climb;climb.Update(geometry,frame,frame.stamp,true);
        frame.grip[side]=1;frame.sequence++;frame.stamp+=20000;climb.Update(geometry,frame,frame.stamp,true);
        cvr::ladder::Vec target;cvr::ladder::Rotation rotation;
        Check(climb.Constrain(side,{},cvr::ladder::Rotation{},target,rotation),"native side grip did not attach");
        const float result[3]={target.x,target.y,target.z},q[4]={rotation.x,rotation.y,rotation.z,rotation.w};
        for(int k=0;k<3;++k)Near(result[k],originalWrist[k],.00002f,"palm anchor does not reproduce the native wrist position");
        QuatNear(q,originalRot,"side rail changed native wrist orientation");
        float curls[5]={1,1,1,1,1};curls[1]=0;
        VRIK_ApplyLadderFingers(buffer,side,2,curls);
        for(int slot=0;slot<19;++slot)QuatNear(pose[fingers[slot]].q,
            cvr::ladder::profile::fingers[slot].group==1 ? reference[fingers[slot]].q:native[fingers[slot]].q,
            "independent rung finger slider changed the wrong finger");
        const auto once=pose;for(int i=0;i<20;++i)VRIK_ApplyLadderFingers(buffer,side,2,curls);
        for(int bone:fingers)QuatNear(pose[bone].q,once[bone].q,"rung slider accumulated finger curl");
        VRIK_ApplyLadderFingers(buffer,side,3,curls);
        for(int bone:fingers)QuatNear(pose[bone].q,native[bone].q,"rung tuning leaked into top/side rail grip");
    }
    std::cout<<"separate_native_hands=2 named_finger_joints=38 palm_anchor_error_under_20um=true\n";
}
void LadderBakedRung() {
    Setup("ladder");float curls[5]{1,1,1,1,1},adjustments[2][5][6]{};int settings=0;
    std::ifstream input(std::filesystem::path(VR_CHAIN_FIXTURE_DIR)/"ladder"/"tuned-rung.ini");
    Check(bool(input),"approved user preset fixture missing");std::string line;
    while(std::getline(input,line)) {
        if(line.empty() || line[0]=='#')continue;
        int hand,finger,channel;float value;
        if(cvr::ladder::ParseFingerAdjustment(line.c_str(),hand,finger,channel,value)) {
            adjustments[hand][finger][channel]=value;++settings;continue;
        }
        for(int f=0;f<5;++f) {
            const std::string key=std::string("xr_ladder_rung_")+cvr::ladder::FingerNames[f]+"=";
            if(line.starts_with(key)) { curls[f]=std::stof(line.substr(key.size()));++settings;break; }
        }
    }
    Check(settings==63,"approved preset did not contain all 63 saved settings");
    for(int side=0;side<2;++side) {
        const auto native=Read<Bone>(side ? "side-grip-right.bin":"side-grip-left.bin");
        auto expected=native,actual=native;
        // The previous authoring path consumes the archived player settings;
        // gameplay consumes no settings and must produce the same skeleton.
        VRIK_ApplyLadderFingers(reinterpret_cast<uint8_t*>(expected.data()),side,2,curls,adjustments[side]);
        auto* buffer=reinterpret_cast<uint8_t*>(actual.data());
        for(int pass=0;pass<20;++pass)VRIK_ApplyLadderFingers(buffer,side,2);
        for(int bone=0;bone<g_VRBoneCount;++bone) {
            QuatNear(actual[bone].q,expected[bone].q,"baked pose differs from the user's tuned grip");
            Check(!std::memcmp(actual[bone].p,native[bone].p,16) && !std::memcmp(actual[bone].s,native[bone].s,16),"baking changed finger geometry");
        }
        for(int kind:{1,3}) {
            VRIK_ApplyLadderFingers(buffer,side,kind);
            for(int bone=0;bone<g_VRBoneCount;++bone)QuatNear(actual[bone].q,native[bone].q,"baked rung changed the native rail grip");
        }
        const auto before=actual;VRIK_ApplyLadderFingers(buffer,side,0);
        Check(!std::memcmp(actual.data(),before.data(),actual.size()*sizeof(Bone)),"free hand inherited a baked rung pose");
    }
    std::cout<<"approved_settings=63 matching_hands=2 live_configuration_required=false\n";
}
void LadderFingerAdjustments() {
    Setup("ladder");const float curls[5]={1,1,1,1,1};int checked=0;
    for(int side=0;side<2;++side) {
        const auto native=Read<Bone>(side ? "side-grip-right.bin":"side-grip-left.bin");
        for(int group=0;group<5;++group)for(int channel=0;channel<6;++channel)if(cvr::ladder::HasAdjustment(group,channel)) {
            float adjustments[5][6]{};adjustments[group][channel]=20;
            auto pose=native;auto* buf=reinterpret_cast<uint8_t*>(pose.data());
            VRIK_ApplyLadderFingers(buf,side,2,curls,adjustments);
            std::string suffix;
            if(channel==5 || (group==0 && (channel==3 || channel==4)))suffix="InHand";
            else suffix="Hand";
            const char* fingers[]={"Thumb","Index","Middle","Ring","Pinky"};suffix+=fingers[group];
            if(suffix.starts_with("Hand"))suffix+=char('1'+(channel<3 ? channel:0));
            const std::string target=std::string(side ? "Right":"Left")+suffix;
            const int joint=int(std::find(names.begin(),names.end(),target)-names.begin());
            Check(joint<g_VRBoneCount,"tuning targeted an absent joint");
            for(int bone=0;bone<g_VRBoneCount;++bone) {
                Check(!std::memcmp(pose[bone].p,native[bone].p,16) && !std::memcmp(pose[bone].s,native[bone].s,16),"finger editing stretched the rig");
                if(bone!=joint)QuatNear(pose[bone].q,native[bone].q,"one joint slider modified another joint or hand");
            }
            float dot=0;for(int k=0;k<4;++k)dot+=pose[joint].q[k]*native[joint].q[k];
            Near(2*std::acos(std::clamp(std::abs(dot),0.0f,1.0f))*57.2957795f,20,.01f,"joint slider did not apply its angle");
            const auto once=pose;
            for(int pass=0;pass<10;++pass)VRIK_ApplyLadderFingers(buf,side,2,curls,adjustments);
            QuatNear(pose[joint].q,once[joint].q,"joint tuning accumulated across pose replays");
            for(int kind:{1,3}) {
                VRIK_ApplyLadderFingers(buf,side,kind,curls,adjustments);
                for(int bone=0;bone<g_VRBoneCount;++bone)QuatNear(pose[bone].q,native[bone].q,"fine tuning leaked into native rail fingers");
            }
            ++checked;
        }
    }
    std::cout<<"independent_hand_joint_controls="<<checked<<" native_side_grips_preserved=true\n";
}
void LadderReach() {
    Setup("ladder");const auto native=Read<Bone>("native_local.bin"),reference=Read<Bone>("reference_full.bin");
    const float head[4]={0,0,0,1};float minimumGain=1;
    for(bool left:{false,true})for(float vertical:{0.0f,.6f}) {
        const int side=left ? 1:0,upper=left ? 17:18,fore=left ? 20:21,hand=left ? 23:24,clav=left ? 14:15;
        const float length=std::sqrt(VRIK_Dot3(reference[fore].p,reference[fore].p))+std::sqrt(VRIK_Dot3(reference[hand].p,reference[hand].p));
        auto run=[&](bool ladder,float ratio) {
            auto pose=native;auto* buf=reinterpret_cast<uint8_t*>(pose.data());
            std::array<float,4> result{};std::array<float,3> first{};
            for(int pass=0;pass<8;++pass) {
                VRIK_BeginBodySolve(buf);VRIK_PinGirdleTranslations(buf);VRIK_PlaceLadderUpper(buf,nullptr,head,g_VRHeadBoneIdx);
                float right[3],up[3],forward[3],anchor[3],joint[3],pivot[3];
                Check(VRIK_BodyAxesFromRig(right,up,forward),"ladder body frame missing");
                std::copy_n(g_fkPos[g_VRNeckIdx],3,anchor);
                VRIK_AnchorShoulder(buf,upper,anchor,left,right,up,.14f,joint);std::copy_n(g_fkPos[clav],3,pivot);
                float target[3];for(int k=0;k<3;++k)target[k]=joint[k]+length*ratio*(forward[k]*std::sqrt(1-vertical*vertical)+up[k]*vertical);
                VRIK_SolveArm(buf,upper,fore,hand,target,head,right,up,forward,0,1,left,false,ladder);
                VRIK_SyncShadowUpper(buf,true);
                std::copy_n(g_VRIKGirdleReach[side],4,result.begin());
                for(int k=0;k<3;++k)Near(g_fkPos[clav][k],pivot[k],.00001f,"ladder reach moved clavicle root");
                Check(result[1]<(ladder ? 20.01f:35.01f) && result[2]<.10f && result[3]<.075f,"girdle adjustment exceeded anatomical limits");
                for(int bone:{0,1,2,5,6,8,9,11,12})Check(!std::memcmp(&pose[bone],&native[bone],sizeof(Bone)),"ladder shoulder reach moved native support");
                if(pass==0)std::copy_n(g_fkPos[upper],3,first.begin());
                else for(int k=0;k<3;++k)Near(g_fkPos[upper][k],first[k],.00001f,"ladder protraction accumulated");
                if(pass==7) {
                    VRIK_CaptureModelCache(true);auto replay=native;
                    VRIK_ReplayModelCache(reinterpret_cast<uint8_t*>(replay.data()));
                    for(int k=0;k<3;++k)Near(g_fkPos[upper][k],first[k],.00002f,"replay lost stronger ladder shoulder");
                }
            }
            return result;
        };
        const auto foot=run(false,1.16f),ladder=run(true,1.16f);
        const float gain=ladder[2]-foot[2];minimumGain=std::min(minimumGain,gain);
        if(vertical==0)Check(gain>.01f && ladder[3]>foot[3]+.01f,"ladder did not gain useful forward shoulder and scapula motion");
        Check(run(true,.95f)[0]>run(false,.95f)[0]+.1f,"ladder assistance did not start earlier");
        Check(run(true,.55f)[2]<.00001f,"relaxed arm retained extra protraction");
        Near(run(false,1.16f)[2],foot[2],.00001f,"ladder profile leaked into ordinary reach");
    }
    std::cout<<"minimum_extra_shoulder_travel_m="<<minimumGain<<" native_support_preserved=true\n";
}
void LadderRest() {
    Setup("ladder");const auto rest=Read<Bone>("reference_full.bin");auto pose=Read<Bone>("side-grip-left.bin");
    auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
    // Exercise the real rest-grip writers with empty hands and both ordinary
    // apply toggles off. Ladder force is local to this call, not global state.
    g_VRSmokeFingerCount=g_VRSmokeFingerCountL=19;
    for(int side=0;side<2;++side)for(int slot=0;slot<19;++slot) {
        const std::string name=std::string(side ? "Right":"Left")+cvr::ladder::profile::fingers[slot].suffix;
        const int bone=int(std::find(names.begin(),names.end(),name)-names.begin());
        (side ? g_VRSmokeFingerIdx:g_VRSmokeFingerIdxL)[slot]=bone;
        if(!side)std::copy_n(rest[bone].q,4,g_VRRestFingerRot[slot]);
        else std::copy_n(rest[bone].q,4,pose[bone].q);
    }
    g_VRRestFingerHave=1;g_VRRestFingerCount=19;g_VRRestFingerApply=0;CyberpunkVR_RestFingerApplyR=0;
    CyberpunkVR_RestFingerCaptureReqR=1;cvr::anim::VrikRestFingerPoseRight(buffer);
    pose=Read<Bone>("side-grip-right.bin");buffer=reinterpret_cast<uint8_t*>(pose.data());const auto native=pose;
    cvr::anim::VrikRestFingerPose(buffer);cvr::anim::VrikRestFingerPoseRight(buffer);
    Check(!std::memcmp(pose.data(),native.data(),pose.size()*sizeof(Bone)),"ordinary empty hands were forced into ladder rest");
    cvr::anim::VrikRestFingerPose(buffer,true);cvr::anim::VrikRestFingerPoseRight(buffer,true);
    for(int side=0;side<2;++side)for(int slot=0;slot<19;++slot) {
        const int bone=(side ? g_VRSmokeFingerIdx:g_VRSmokeFingerIdxL)[slot];
        QuatNear(pose[bone].q,rest[bone].q,"free ladder hand did not receive its own rest grip");
        Check(!std::memcmp(pose[bone].p,native[bone].p,16),"rest grip changed a finger length");
    }
    Check(g_VRRestFingerApply==0 && CyberpunkVR_RestFingerApplyR==0,"ladder rest changed persistent user toggles");
    VRIK_ApplyLadderFingers(buffer,0,1,nullptr);
    for(int slot=0;slot<19;++slot) {
        QuatNear(pose[g_VRSmokeFingerIdxL[slot]].q,cvr::ladder::profile::fingers[slot].rotation[0],"left grip did not replace rest");
        QuatNear(pose[g_VRSmokeFingerIdx[slot]].q,rest[g_VRSmokeFingerIdx[slot]].q,"left grip disturbed right rest");
    }
    std::cout<<"left_right_rest_and_grip_priority=true\n";
}
void LadderUpper() {
    Setup("ladder");const auto native=Read<Bone>("native_local.bin");auto pose=native;
    auto* buffer=reinterpret_cast<uint8_t*>(pose.data());
    const int lower[]={0,1,g_VRHipsIdx,g_VRLeftUpLegIdx,g_VRLeftLegIdx,g_VRLeftFootIdx,
        g_VRRightUpLegIdx,g_VRRightLegIdx,g_VRRightFootIdx};
    shared[114]=1.1f;shared[115]=2;shared[89]=shared[90]=-.7f;
    *reinterpret_cast<uint32_t*>(&shared[127])=2;RefreshHandsSnapshot();
    cvr::camera::AnchorVector mount{};
    Check(cvr::camera::ReadNeckCameraEyeOffset(&mount),"ladder has no neutral camera mount");
    float minReachGain=100;
    for(float pitch:{-.8f,0.0f,.6f}) {
        pose=native;const float head[4]={std::sin(pitch*.5f),0,0,std::cos(pitch*.5f)};
        VRIK_BeginBodySolve(buffer);VRIK_PinGirdleTranslations(buffer);
        VRIK_PlaceLadderUpper(buffer,nullptr,head,g_VRHeadBoneIdx);
        const auto neutral=pose;
        float neck[3];std::copy_n(g_fkPos[g_VRNeckIdx],3,neck);
        const float camera[3]={g_VRIKBodyEyeSolved[0]+mount.x,
            g_VRIKBodyEyeSolved[1]+mount.y+.20f,g_VRIKBodyEyeSolved[2]+mount.z-.06f};
        VRIK_PlaceLadderUpper(buffer,camera,head,g_VRHeadBoneIdx);VRIK_SyncShadowUpper(buffer,true);
        const float gain=g_fkPos[g_VRNeckIdx][1]-neck[1];minReachGain=std::min(minReachGain,gain);
        Check(gain>.12f,"ladder upper spine did not bring shoulder bases toward the reach");
        Check(std::abs(g_VRIKBodyEyeSolved[1]-g_VRIKBodyEyeTarget[1])<.045f,"ladder head fit missed forward HMD movement");
        for(int i=0;i<g_VRSpineCount;++i) {
            const int bone=g_VRSpineIdx[i];float dot=0;
            for(int k=0;k<4;++k)dot+=pose[bone].q[k]*neutral[bone].q[k];
            Check(2*std::acos(std::clamp(std::abs(dot),0.0f,1.0f))<.36f,"ladder bent one vertebra too far");
            for(int k=0;k<3;++k)Near(pose[bone].p[k],neutral[bone].p[k],.00001f,"ladder stretched a vertebra");
        }
        for(int bone:lower)Check(!std::memcmp(&pose[bone],&native[bone],sizeof(Bone)),"ladder solve changed native pelvis/root/legs");
        for(int i=0;i<g_VRBoneCount;++i)if(names[i].starts_with("shadow_") &&
            (names[i].find("Leg")!=std::string::npos || names[i].find("Foot")!=std::string::npos || names[i]=="shadow_Hips"))
            Check(!std::memcmp(&pose[i],&native[i],sizeof(Bone)),"ladder solve changed a lower shadow bone");
        Check(g_VRIKBodyBendAngle==0 && s_vrSharedSquatDrop==0,"ladder retained physical squat/bend");
        float expected[4];VRIK_QuatMul(head,g_VRHeadReferenceModelRot,expected);
        QuatNear(g_fkRot[g_VRHeadBoneIdx],expected,"ladder head did not follow HMD");
        const auto once=pose;
        for(int pass=0;pass<20;++pass)VRIK_PlaceLadderUpper(buffer,camera,head,g_VRHeadBoneIdx);
        for(int i=0;i<g_VRSpineCount;++i)QuatNear(pose[g_VRSpineIdx[i]].q,once[g_VRSpineIdx[i]].q,"ladder bend accumulated across pose passes");
        float eyeTarget[3];std::copy_n(g_VRIKBodyEyeTarget,3,eyeTarget);
        for(float yaw:{-1.2f,1.2f}) {
            const float turn[4]={0,0,std::sin(yaw/2),std::cos(yaw/2)};float turned[4];
            VRIK_QuatMul(turn,head,turned);VRIK_PlaceLadderUpper(buffer,camera,turned,g_VRHeadBoneIdx);
            for(int k=0;k<3;++k)Near(g_VRIKBodyEyeTarget[k],eyeTarget[k],.000001f,"free head yaw rotated ladder camera mount and pulled torso sideways");
        }
        VRIK_PlaceLadderUpper(buffer,camera,head,g_VRHeadBoneIdx);
        VRIK_CaptureModelCache(true);
        auto fresh=native;fresh[g_VRHipsIdx].p[2]+=.07f;
        const auto before=fresh;auto* next=reinterpret_cast<uint8_t*>(fresh.data());
        VRIK_ComputeFK(next,VRIK_FKCount());VRIK_ReplayModelCache(next);
        for(int bone:lower)Check(!std::memcmp(&fresh[bone],&before[bone],sizeof(Bone)),"cached upper replay overwrote fresh native ladder support");
    }
    std::cout<<"native_ladder_bones="<<native.size()<<" pelvis_and_legs_preserved=true min_neck_reach_gain="<<minReachGain<<"\n";
}
}
int main(int argc,char** argv) {
    try {
        g_VRBendLimitClockUs=TestClock;
        Setup();const std::string name=argc>1?argv[1]:"";
        if(name=="fk_fixture")FkFixture();
        else if(name=="packet_atomicity")PacketAtomicity();
        else if(name=="coordinate_roundtrip")CoordinateRoundtrip();
        else if(name=="passenger_window")PassengerWindow();
        else if(name=="vehicle_hand_frame")VehicleHandFrame();
        else if(name=="vehicle_upper_body")VehicleUpperBody();
        else if(name=="body_write_census")BodyWriteCensus();
        else if(name=="swimming_body")SwimmingBody();
        else if(name=="ladder_upper")LadderUpper();
        else if(name=="ladder_grips")LadderGrips();
        else if(name=="ladder_finger_adjustments")LadderFingerAdjustments();
        else if(name=="ladder_baked_rung")LadderBakedRung();
        else if(name=="ladder_reach")LadderReach();
        else if(name=="ladder_rest")LadderRest();
        else if(name=="rig_policy")RigPolicy();
        else if(name=="rig_rebind")RigRebind();
        else if(name=="pose_buffer_rebind")PoseBufferRebind();
        else if(name=="shoulder_reference")ShoulderReference();
        else if(name=="weapon_stance")WeaponStance(false);
        else if(name=="weapon_stance_armed_start")WeaponStance(true);
        else if(name=="weapon_stance_bending")WeaponStance(false,true);
        else if(name=="body_arm_frame")BodyArmFrame();
        else if(name=="body_bend_geometry")BodyBendGeometry();
        else if(name=="floor_reach_geometry")FloorReachGeometry();
        else if(name=="head_bound_bend")HeadBoundBend(true);
        else if(name=="head_bound_bend_unbound")HeadBoundBend(false);
        else if(name=="girdle_reach")GirdleReach();
        else if(name=="overhead_reach")OverheadReach();
        else if(name=="cyberware_pose")CyberwarePose();
        else if(name=="mantis_pose")CyberwarePose("mantis");
        else if(name=="mantis_wrist")MantisWrist();
        else if(name=="cyberware_wire")CyberwareWire();
        else if(name=="combat_elbow")CombatElbow();
        else if(name=="elbow_blend")ElbowBlendProfiles();
        else if(name=="cyberware_muscles")CyberwareMuscles();
        else if(name=="launcher_axis")LauncherAxis();
        else if(name=="launcher_wrist")LauncherWrist();
        else if(name=="hand_gestures")HandGestures();
        else if(name=="launcher_pose")CyberwarePose("launcher");
        else if(name=="forearm_roll")ForearmRoll();
        else if(name=="forearm_solve")ForearmSolve();
        else if(name=="native_lower")NativeLowerPreserved();
        else if(name=="toes_ahead")ToesAhead();
        else if(name=="head_basis")HeadBasis();
        else if(name=="neck_mount")NeckMount();
        else if(name=="body_centre_orbit")BodyCentreOrbit();
        else if(name=="female_neck_mount")FemaleNeckMount();
        else if(name=="combat_hmd_body")CombatHmdBody();
        else if(name=="pelvis_pose")PelvisPose();
        else if(name=="upper_posture")UpperPosture();
        else if(name=="shadow_upper")ShadowUpper();
        else if(name=="model_replay")ModelReplay();
        else if(name=="startup_packet")StartupPacket();
        else if(name=="pair_selection")PairSelection();
        else if(name=="model_origin")ModelOrigin();
        else if(name=="neutral_view_reference")NeutralViewReference();
        else if(name=="pose_chain")PoseChain();
        else if(name=="missing_camera_replay")MissingCameraReplay();
        else if(name=="vehicle_pair_miss")VehiclePairMiss();
        else if(name=="vehicle_missing_camera")VehicleMissingCamera();
        else if(name=="invalidated_camera_release")InvalidatedCameraRelease();
        else if(name=="missing_camera_expiry")MissingCameraExpiry();
        else if(name=="missing_native_centre")MissingNativeCentre();
        else if(name=="paired_consumed")PairedConsumed();
        else throw std::runtime_error("unknown test");
        return 0;
    }catch(const std::exception& e){std::cerr<<"FAIL: "<<e.what()<<'\n';return 1;}
}
