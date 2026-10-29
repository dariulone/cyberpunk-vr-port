#include "Hooks/BodyHeading.hpp"
#include "Utils/DebugGate.hpp"
// CharacterRig -- the solve for the PLAYER'S OWN bones.
//
// Forward kinematics, the two-bone arm and leg solves, the hand target and its stop, the torso
// damping and girdle pinning, the arm scaling that fits the avatar to a measured player, and the body
// placement under the HMD. Every function here answers the same question: given a target in some
// space, what local rotation does this character bone take?
//
// It does NOT contain the WEAPON bones. Those are written inside the pose-apply detour, at brace
// depth three to seven and inside its single 1,980-line __try -- see the note at the top of
// src/Hooks/AnimPose.cpp for why they have not been lifted out yet and what it would take.

#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/SkeletonPolicy.hpp"
#include "Anim/UpperBodyPosture.hpp"
#include "Anim/BodyBend.hpp"
#include "Anim/LadderGripProfile.hpp"
#include "Anim/HandGestures.hpp"
#include "Anim/LadderFingerTuning.hpp"
#include "Anim/LadderRungPose.hpp"
#include "Camera/NeckCameraMount.hpp"
// The elbow-policy flag lives with the other live controls.
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include <algorithm>
#include <cstring>
#include <mutex>

static inline void VRIK_QuatExtractTwist(const float* q,const float* axis,float* outT);

bool g_VRUpperOwned[VRIK_MAX_BONES]{};
int g_VRShadowSource[VRIK_MAX_BONES]{};
bool g_VRHeadReferenceValid=false;
bool g_VRIKVehicleUpperActive=false;
float g_VRHeadReferenceModelRot[4]={0,0,0,1};
namespace {
bool s_referenceValid[VRIK_MAX_BONES]{};
float s_referenceLocalRot[VRIK_MAX_BONES][4]{};
float s_referenceLocalPos[VRIK_MAX_BONES][3]{};
float s_referenceModelRot[VRIK_MAX_BONES][4]{};
float s_referenceModelPos[VRIK_MAX_BONES][3]{};
float s_referenceCameraHeight=1.6f;
bool s_referenceCameraValid{};
std::mutex s_neckMountMutex;
cvr::camera::AnchorVector s_neckMount{};
cvr::camera::AnchorVector s_neckCameraEyeOffset{};
bool s_neckMountValid{};
float s_neutralEyeModel[3]{},s_eyeInHead[3]{};
bool s_eyeReferenceValid{};
int s_ladderFingers[2][19]{};
int s_scapula[2][4]{},s_scapulaCount[2]{},s_upperSkin[2][2]{{-1,-1},{-1,-1}};
struct ShoulderBase {
    uint8_t* buffer{};
    uint64_t revision{};
    int upper{-1},clavicle{-1};
    float clavicleLocalRot[4]{},upperLocalPos[3]{};
};
ShoulderBase s_shoulderBase[2]{};
float s_modelCachePos[VRIK_MAX_BONES][3]{};
float s_modelCacheRot[VRIK_MAX_BONES][4]{};
bool s_modelCacheOwned[VRIK_MAX_BONES]{};
uint64_t s_solveCacheCapturedMs{};
uint64_t s_rigFingerprint{},s_rigRevision{};
bool s_bendLegOwned[VRIK_MAX_BONES]{};
int s_bendLegShadowSource[VRIK_MAX_BONES]{};
struct BendLegRestore {
    uint8_t* buffer{};
    uint64_t revision{};
    bool vehicle{};
    int count{},bones[14]{}; // six joints + shadows; Combat also saves pelvis + shadow
    float native[14][12]{},solved[14][12]{};
};
BendLegRestore s_bendLegRestore[8]{};
unsigned s_bendRestoreCursor{};

bool SameLocal(const uint8_t* buffer,int bone,const float* value) {
    const auto* local=reinterpret_cast<const float*>(buffer+bone*48);
    for(int k=0;k<12;++k)if(!std::isfinite(local[k]) || std::abs(local[k]-value[k])>1e-6f)return false;
    return true;
}
BendLegRestore* SaveVehicleLower(uint8_t* buffer) {
    int bones[14]={g_VRRightUpLegIdx,g_VRRightLegIdx,g_VRRightFootIdx,
                   g_VRLeftUpLegIdx,g_VRLeftLegIdx,g_VRLeftFootIdx};
    int count=6;
    for(int bone=0;bone<VRIK_FKCount() && count<12;++bone)
        if(s_bendLegShadowSource[bone]>=0)bones[count++]=bone;
    bones[count++]=g_VRHipsIdx;
    for(int bone=0;bone<VRIK_FKCount() && count<14;++bone)
        if(g_VRShadowSource[bone]==g_VRHipsIdx)bones[count++]=bone;
    BendLegRestore* saved=nullptr;
    for(auto& entry:s_bendLegRestore)if(entry.buffer==buffer) { saved=&entry;break; }
    if(!saved)saved=&s_bendLegRestore[s_bendRestoreCursor++%8];
    const bool previous=saved->buffer==buffer && saved->revision==s_rigRevision;
    for(int i=0;i<count;++i) {
        // Cache replay can arrive on another animation buffer. Keep its native
        // lower pose before replay, but never recapture our own output as native.
        if(!previous || i>=saved->count || saved->bones[i]!=bones[i] ||
            !SameLocal(buffer,bones[i],saved->solved[i]))
            std::memcpy(saved->native[i],buffer+bones[i]*48,48);
        saved->bones[i]=bones[i];
    }
    saved->buffer=buffer;saved->revision=s_rigRevision;saved->count=count;saved->vehicle=true;
    return saved;
}
}
bool g_solveCacheModel=false;
float g_VRIKBodyBendAngle=0;
float g_VRIKBodyBendPelvis[3]{};
float g_VRIKBodyEyeTarget[3]{},g_VRIKBodyEyeSolved[3]{};
bool g_VRIKBodyEyeBound=false;
float g_VRIKGirdleReach[2][4]{}; // weight, clavicle degrees, joint advance(m), scapula travel(m)
// GRIP REACH. A hand closed on a weapon -- the support hand of the two-hand hold, a wrist the reload has locked to a
// slide or a fore-end -- has to ARRIVE at what it is drawn holding; a hand stopped a few centimetres short of its
// own grip by the arm's length caps is the one thing everybody sees. A real arm gets there from behind: the
// scapula protracts round the ribcage, the clavicle swings forward, and when that is still not enough the thorax
// turns into the reach. So while a side is in a grip its girdle reaches further (VRIK_ReachGirdle), and whatever
// the LEFT wrist is still short after the caps feeds a thorax turn on the next solve (VRIK_GripTorso).
extern "C" int CyberpunkVR_TwoHandActive;                                       // Anim/TwoHandGrip.cpp
extern "C" __declspec(dllexport) int   CyberpunkVR_GripReach = 1;
extern "C" __declspec(dllexport) float CyberpunkVR_GripProtract = 0.085f;      // m of protraction at full reach
extern "C" __declspec(dllexport) float CyberpunkVR_GripTorsoMaxDeg = 20.0f;     // the thorax turn's limit
// R, L: how far the wrist fell short of its target after the caps, mm (negative = slack); the turn, degrees
extern "C" __declspec(dllexport) float CyberpunkVR_DebugGripShortMm[2] = {0.0f, 0.0f};
extern "C" __declspec(dllexport) float CyberpunkVR_DebugGripTorsoDeg = 0.0f;
// the left side: how far the turn carried the shoulder's anchor (mm); the girdle's weight, clavicle degrees and
// joint advance (mm); the reach as a fraction of the arm; 1 while that girdle ran in its grip profile
extern "C" __declspec(dllexport) float CyberpunkVR_DebugGripReach[6] = {0,0,0,0,0,0};
bool  g_VRIKGrip[2]{};            // R, L: that hand was in a grip on the last solve
float g_VRIKShort[2]{};           // ...and its wrist's shortfall, metres (negative = slack)
float g_VRIKTorsoShift[3]{};      // where this solve's thorax turn carried the LEFT shoulder, model space
float g_VRIKTorsoQ[2][4]{{0,0,0,1},{0,0,0,1}};   // the turn's two rotations, model space, lower bone first...
float g_VRIKTorsoP[2][3]{};                      // ...and the point each one turns about
bool  g_VRIKTorsoOn=false;
int   g_VRIKTorsoBones[2]{-1,-1}; // the two spine bones the turn is written on, for the replay cache
int g_VRCyberwareFamily=0;
extern "C" __declspec(dllexport) int CyberpunkVR_NativeCyberwarePasses=1;
uint64_t g_VRCyberGraphHashes[16]{},g_VRCyberGraphCalls[16]{};
bool VRIK_UsesNativeCyberware() {return CyberpunkVR_NativeCyberwarePasses!=0 && g_VRCyberwareFamily!=0;}

bool cvr::camera::ReadNeckCameraMount(AnchorVector* offset) {
    std::lock_guard lock(s_neckMountMutex);
    if(!offset || !s_neckMountValid)return false;
    *offset=s_neckMount;return true;
}
bool cvr::camera::ReadNeckCameraEyeOffset(AnchorVector* offset) {
    std::lock_guard lock(s_neckMountMutex);
    if(!offset || !s_eyeReferenceValid)return false;
    *offset=s_neckCameraEyeOffset;return true;
}

void VRIK_ConfigureRigPolicy(const char* const* names,int count,const uint8_t* reference,int referenceCount,bool female) {
    using namespace cvr::vrik;
    count=std::clamp(count,0,VRIK_MAX_BONES);
    g_VRCyberwareFamily=0;
    for(int i=0;i<count;++i)if(names[i]) {
        if(EqualBoneName(names[i],"l_plate_up_a"))g_VRCyberwareFamily=1;
        if(EqualBoneName(names[i],"l_wristSuspension01_JNT"))g_VRCyberwareFamily=2;
        if(EqualBoneName(names[i],"l_nanoWire01_grp"))g_VRCyberwareFamily=3;
        if(EqualBoneName(names[i],"launcher_base"))g_VRCyberwareFamily=4;
    }
    VRIK_ConfigureCyberwareRig(names,count,reference,referenceCount);
    // A meta-rig can change when arms/cyberware are replaced. Neither indices
    // nor captured anatomical axes from the previous layout may survive it.
    uint64_t fingerprint=14695981039346656037ull;
    auto hashByte=[&](uint8_t b) { fingerprint=(fingerprint^b)*1099511628211ull; };
    hashByte(female ? 1:0);
    for(int i=0;i<count;++i) {
        if(names[i])for(const char* c=names[i];*c;++c)hashByte(static_cast<uint8_t>(*c));
        hashByte(0);hashByte(static_cast<uint8_t>(g_VRBoneParent[i]));
        hashByte(static_cast<uint8_t>(g_VRBoneParent[i]>>8));
        if(reference && i<referenceCount) {
            for(int b=0;b<12;++b)hashByte(reference[i*48+b]);
            for(int b=16;b<32;++b)hashByte(reference[i*48+b]);
        }
    }
    if(fingerprint!=s_rigFingerprint) { s_rigFingerprint=fingerprint;++s_rigRevision; }
    std::fill_n(g_VRForeTwistR,3,-1);std::fill_n(g_VRForeTwistL,3,-1);
    std::fill_n(s_scapulaCount,2,0);
    for(auto& side:s_upperSkin)std::fill_n(side,2,-1);
    for(auto& base:s_shoulderBase)base={};
    for(auto& side:s_ladderFingers)std::fill_n(side,19,-1);
    for(int bone=0;bone<count;++bone)if(names[bone])for(int side=0;side<2;++side) {
        const std::string_view prefix=side ? "Right":"Left",name=names[bone];
        if(!StartsBoneName(name,prefix))continue;
        const int hand=side ? g_VRRightBoneIdx:g_VRLeftBoneIdx;
        int ancestor=bone;
        for(int guard=0;ancestor>=0 && ancestor<count && ancestor!=hand && guard<count;++guard)ancestor=g_VRBoneParent[ancestor];
        if(ancestor!=hand)continue;
        for(int slot=0;slot<19;++slot)if(EqualBoneName(name.substr(prefix.size()),cvr::ladder::profile::fingers[slot].suffix))
            s_ladderFingers[side][slot]=bone;
    }
    for(int i=0;i<count;++i)if(names[i]) {
        for(bool left:{false,true}) {
            const int slot=ForearmTwistSlot(names[i],left);
            const int fore=left ? g_VRLeftForeArmIdx : g_VRRightForeArmIdx;
            // Supported helpers are siblings parented to the forearm. Checking
            // the topology also rejects stale names/indices from another rig.
            if(slot>=0 && fore>=0 && fore<count && g_VRBoneParent[i]==fore)
                (left ? g_VRForeTwistL : g_VRForeTwistR)[slot]=i;
        }
    }
    std::fill_n(g_VRUpperOwned,VRIK_MAX_BONES,false);
    std::fill_n(g_VRShadowSource,VRIK_MAX_BONES,-1);
    std::fill_n(s_bendLegShadowSource,VRIK_MAX_BONES,-1);
    std::fill_n(s_bendLegOwned,VRIK_MAX_BONES,false);
    std::fill_n(s_referenceValid,VRIK_MAX_BONES,false);
    g_VRHeadReferenceValid=false;
    g_solveCacheN=0;g_solveCacheModel=false;g_solveCacheTick=0xFFFFFFFFu;
    for(int i=0;i<count;++i) {
        if(!names[i] || IsRigControl(names[i]))continue;
        if(EqualBoneName(names[i],"Hips"))g_VRUpperOwned[i]=true;
        for(int ancestor=i,guard=0;ancestor>=0 && ancestor<count && guard++<count;) {
            if(names[ancestor] && IsPrimarySpine(names[ancestor])) { g_VRUpperOwned[i]=true;break; }
            const int parent=g_VRBoneParent[ancestor];
            if(parent>=ancestor)break;
            ancestor=parent;
        }
    }
    for(int i=0;i<count;++i) {
        if(!names[i] || !StartsBoneName(names[i],"shadow_"))continue;
        for(int source=0;source<count;++source) {
            if(g_VRUpperOwned[source] && names[source] && EqualBoneName(names[i]+7,names[source])) {
                g_VRShadowSource[i]=source;break;
            }
        }
        for(int source:{static_cast<int>(g_VRRightUpLegIdx),static_cast<int>(g_VRRightLegIdx),
                        static_cast<int>(g_VRRightFootIdx),static_cast<int>(g_VRLeftUpLegIdx),
                        static_cast<int>(g_VRLeftLegIdx),static_cast<int>(g_VRLeftFootIdx)}) {
            if(source>=0 && source<count && names[source] && EqualBoneName(names[i]+7,names[source])) {
                s_bendLegShadowSource[i]=source;break;
            }
        }
    }
    // Do not use global working FK arrays while arming a rig.
    referenceCount=reference ? std::clamp(referenceCount,0,count) : 0;
    for(int i=0;i<referenceCount;++i) {
        const float* q=reinterpret_cast<const float*>(reference+i*48+VRIK_ROT_OFF);
        const float norm=q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3];
        const int parent=g_VRBoneParent[i];
        if(!std::isfinite(norm) || norm<.5f || norm>1.5f || parent>=i ||
           (parent>=0 && !s_referenceValid[parent]))continue;
        std::copy_n(q,4,s_referenceLocalRot[i]);VRIK_QuatNorm(s_referenceLocalRot[i]);
        const float* p=reinterpret_cast<const float*>(reference+i*48+VRIK_TRANS_OFF);
        if(!std::isfinite(p[0]) || !std::isfinite(p[1]) || !std::isfinite(p[2]))continue;
        std::copy_n(p,3,s_referenceLocalPos[i]);
        if(parent>=0)VRIK_QuatMul(s_referenceModelRot[parent],s_referenceLocalRot[i],s_referenceModelRot[i]);
        else std::copy_n(s_referenceLocalRot[i],4,s_referenceModelRot[i]);
        if(parent>=0) {
            VRIK_QuatRotateVec(s_referenceModelRot[parent],s_referenceLocalPos[i],s_referenceModelPos[i]);
            for(int k=0;k<3;++k)s_referenceModelPos[i][k]+=s_referenceModelPos[parent][k];
        } else std::copy_n(s_referenceLocalPos[i],3,s_referenceModelPos[i]);
        VRIK_QuatNorm(s_referenceModelRot[i]);s_referenceValid[i]=true;
    }
    const int head=g_VRHeadBoneIdx;
    if(head>=0 && head<referenceCount && s_referenceValid[head]) {
        std::copy_n(s_referenceModelRot[head],4,g_VRHeadReferenceModelRot);
        g_VRHeadReferenceValid=true;
    }
    // Resolve the camera and eyes from this rig, then evaluate the same neutral
    // spine posture used below. The upper neck is sampled with a neutral HMD.
    int camera=-1,leftEye=-1,rightEye=-1;
    for(int i=0;i<count;++i)if(names[i]) {
        if(EqualBoneName(names[i],"Torso_fppCamera_Control_GRP"))camera=i;
        if(EqualBoneName(names[i],"l_J_eye_JNT") || (leftEye<0 && EqualBoneName(names[i],"LeftEye")))leftEye=i;
        if(EqualBoneName(names[i],"r_J_eye_JNT") || (rightEye<0 && EqualBoneName(names[i],"RightEye")))rightEye=i;
    }
    float neutralPos[VRIK_MAX_BONES][3]{},neutralRot[VRIK_MAX_BONES][4]{};
    for(int i=0;i<referenceCount;++i)if(s_referenceValid[i]) {
        const int parent=g_VRBoneParent[i];
        if(parent>=0) {
            VRIK_QuatRotateVec(neutralRot[parent],s_referenceLocalPos[i],neutralPos[i]);
            for(int k=0;k<3;++k)neutralPos[i][k]+=neutralPos[parent][k];
            VRIK_QuatMul(neutralRot[parent],s_referenceLocalRot[i],neutralRot[i]);
        } else {
            std::copy_n(s_referenceLocalPos[i],3,neutralPos[i]);
            std::copy_n(s_referenceLocalRot[i],4,neutralRot[i]);
        }
        float flex=0;bool override=false;
        for(int c=0;c<g_VRSpineCount && c<8;++c)if(g_VRSpineIdx[c]==i) {
            flex=cvr::vrik::SpineFlex(c,g_VRSpineCount);override=true;
            if(c==g_VRSpineCount-1)neutralPos[i][1]-=cvr::vrik::ChestRetraction;
        }
        if(i==g_VRNeckIdx) { flex=cvr::vrik::NeckFlex;override=true; }
        if(i==g_VRNeck1Idx) { flex=cvr::vrik::UpperNeckFlex;override=true; }
        if(i==head) { flex=0;override=true; }
        if(override) {
            const float bend[4]={std::sin(flex*.5f),0,0,std::cos(flex*.5f)};
            VRIK_QuatMul(bend,s_referenceModelRot[i],neutralRot[i]);
        }
    }
    const int neck=g_VRNeck1Idx>=0 ? g_VRNeck1Idx:g_VRNeckIdx;
    s_referenceCameraValid=camera>=0 && camera<referenceCount && s_referenceValid[camera];
    s_referenceCameraHeight=s_referenceCameraValid ? s_referenceModelPos[camera][2]:0;
    std::lock_guard lock(s_neckMountMutex);
    s_neckMountValid=neck>=0 && neck<referenceCount && camera>=0 && camera<referenceCount &&
        leftEye>=0 && leftEye<referenceCount && rightEye>=0 && rightEye<referenceCount &&
        s_referenceValid[neck] && s_referenceValid[camera] && s_referenceValid[leftEye] && s_referenceValid[rightEye];
    if(s_neckMountValid) {
        s_neckMount=cvr::camera::NeckCameraOffset({neutralPos[neck][0],neutralPos[neck][1],neutralPos[neck][2]},
            .5f*(neutralPos[leftEye][2]+neutralPos[rightEye][2]),
            {s_referenceModelPos[camera][0],s_referenceModelPos[camera][1],s_referenceModelPos[camera][2]},female);
    }
    s_eyeReferenceValid=s_neckMountValid && head>=0 && head<referenceCount && s_referenceValid[head];
    if(s_eyeReferenceValid) {
        float delta[3],inverse[4];VRIK_QuatConj(neutralRot[head],inverse);
        for(int k=0;k<3;++k) {
            s_neutralEyeModel[k]=.5f*(neutralPos[leftEye][k]+neutralPos[rightEye][k]);
            delta[k]=s_neutralEyeModel[k]-neutralPos[head][k];
        }
        VRIK_QuatRotateVec(inverse,delta,s_eyeInHead);
        s_neckCameraEyeOffset=cvr::camera::NeckCameraEyeOffset(
            {neutralPos[neck][0],neutralPos[neck][1],neutralPos[neck][2]},
            {s_neutralEyeModel[0],s_neutralEyeModel[1],s_neutralEyeModel[2]},female);
    }
    const int chest=g_VRSpineCount>0 ? g_VRSpineIdx[g_VRSpineCount-1]:-1;
    for(int i=0;i<count;++i)if(names[i] && s_referenceValid[i])for(int side=0;side<2;++side) {
        const bool left=side==1;const int upper=left ? g_VRLeftUpperArmIdx:g_VRRightUpperArmIdx;
        if(g_VRBoneParent[i]==chest && StartsBoneName(names[i],left ? "l_scapula_":"r_scapula_") && s_scapulaCount[side]<4)
            s_scapula[side][s_scapulaCount[side]++]=i;
        if(g_VRBoneParent[i]==upper)for(int slot=0;slot<2;++slot) {
            const char* n=left ? (slot==0 ? "l_SHL_0_JNT":"l_SHL_1_JNT"):(slot==0 ? "r_SHL_0_JNT":"r_SHL_1_JNT");
            if(EqualBoneName(names[i],n))s_upperSkin[side][slot]=i;
        }
    }
}


// Rotate vector v by quaternion q (q = i,j,k,r == x,y,z,w). o = q * v * q^-1.
void VRIK_QuatRotateVec(const float* q, const float* v, float* o) {
    const float tx = 2.0f * (q[1] * v[2] - q[2] * v[1]);
    const float ty = 2.0f * (q[2] * v[0] - q[0] * v[2]);
    const float tz = 2.0f * (q[0] * v[1] - q[1] * v[0]);
    o[0] = v[0] + q[3] * tx + (q[1] * tz - q[2] * ty);
    o[1] = v[1] + q[3] * ty + (q[2] * tx - q[0] * tz);
    o[2] = v[2] + q[3] * tz + (q[0] * ty - q[1] * tx);
}

// Hamilton product o = a * b (both i,j,k,r == x,y,z,w).
void VRIK_QuatMul(const float* a, const float* b, float* o) {
    o[0] = a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1];
    o[1] = a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0];
    o[2] = a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3];
    o[3] = a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2];
}

// Conjugate (== inverse for a unit quaternion).
void VRIK_QuatConj(const float* q, float* o) {
    o[0] = -q[0]; o[1] = -q[1]; o[2] = -q[2]; o[3] = q[3];
}

void VRIK_QuatNorm(float* q) {
    float n = std::sqrt(q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3]);
    if (n > 1e-8f) { float inv = 1.0f / n; q[0]*=inv; q[1]*=inv; q[2]*=inv; q[3]*=inv; }
    else { q[0]=0.0f; q[1]=0.0f; q[2]=0.0f; q[3]=1.0f; }
}

float VRIK_Dot3(const float* a, const float* b) {
    return a[0]*b[0] + a[1]*b[1] + a[2]*b[2];
}
void VRIK_Cross3(const float* a, const float* b, float* o) {
    o[0] = a[1]*b[2] - a[2]*b[1];
    o[1] = a[2]*b[0] - a[0]*b[2];
    o[2] = a[0]*b[1] - a[1]*b[0];
}
float VRIK_Norm3(float* v) {
    float n = std::sqrt(v[0]*v[0] + v[1]*v[1] + v[2]*v[2]);
    if (n > 1e-8f) { float inv = 1.0f/n; v[0]*=inv; v[1]*=inv; v[2]*=inv; }
    return n;
}
static inline float VRIK_Dist3(const float* a, const float* b) {
    float dx=a[0]-b[0], dy=a[1]-b[1], dz=a[2]-b[2];
    return std::sqrt(dx*dx + dy*dy + dz*dz);
}

// slerp(identity, q, t): a fraction t of the rotation q (used to distribute a spine bend
// across several bones so the curve is gradual instead of a single sharp joint).
void VRIK_QuatScale(const float* q, float t, float* o) {
    float qq[4] = { q[0], q[1], q[2], q[3] };
    if (qq[3] < 0.0f) { qq[0]=-qq[0]; qq[1]=-qq[1]; qq[2]=-qq[2]; qq[3]=-qq[3]; } // shortest arc
    float s = std::sqrt(qq[0]*qq[0] + qq[1]*qq[1] + qq[2]*qq[2]);
    if (s < 1e-6f) { o[0]=0.0f; o[1]=0.0f; o[2]=0.0f; o[3]=1.0f; return; } // ~no rotation
    float w = qq[3]; if (w > 1.0f) w = 1.0f;
    float half = std::acos(w) * t;     // scaled half-angle
    float sn = std::sin(half), cn = std::cos(half);
    o[0] = qq[0]/s*sn; o[1] = qq[1]/s*sn; o[2] = qq[2]/s*sn; o[3] = cn;
}

// Shortest-arc unit quaternion rotating unit vector a onto unit vector b.
void VRIK_QuatFromTo(const float* a, const float* b, float* o) {
    float d = VRIK_Dot3(a, b);
    if (d >= 1.0f - 1e-6f) { o[0]=0.0f; o[1]=0.0f; o[2]=0.0f; o[3]=1.0f; return; }
    if (d <= -1.0f + 1e-6f) {
        // Antiparallel: rotate 180 deg about any axis orthogonal to a.
        float ax[3] = { 1.0f, 0.0f, 0.0f };
        if (std::fabs(a[0]) > 0.9f) { ax[0]=0.0f; ax[1]=1.0f; ax[2]=0.0f; }
        float axis[3]; VRIK_Cross3(a, ax, axis); VRIK_Norm3(axis);
        o[0]=axis[0]; o[1]=axis[1]; o[2]=axis[2]; o[3]=0.0f;
        return;
    }
    float c[3]; VRIK_Cross3(a, b, c);
    o[0]=c[0]; o[1]=c[1]; o[2]=c[2]; o[3]=1.0f + d;
    VRIK_QuatNorm(o);
}

// Writes one VR hand into the destination bone buffer (48-byte QsTransform:
// rotation/quaternion @ +0, translation @ +16, scale @ +32 -- confirmed via IDA
// current pose-apply implementation). When head-relative is active the controller's
// head-local offset is rotated by the head bone's orientation and added to the
// head bone's position, so the result lands in the same buffer space as the head.
// This mirrors the working CET gizmo (worldPos = camPos + camQuat * localPos) and
// is what stops the hand from swinging when the head turns.
void VRIK_WriteHand(uint8_t* boneBuf, int bIdx,
                                  const float* headPos, const float* headQuat, bool headOk,
                                  const float* vrPos, const float* vrQuat, bool writeRot) {
    if (bIdx < 0) return;

    const float s = g_VRBindScale;
    float local[3];
    VRIK_RemapAxis(g_VRBindAxis, vrPos, local);
    local[0] *= s; local[1] *= s; local[2] *= s;

    float pos[3];
    if (g_VRUseHeadRelative && headOk) {
        float rotated[3];
        VRIK_QuatRotateVec(headQuat, local, rotated);
        pos[0] = headPos[0] + rotated[0];
        pos[1] = headPos[1] + rotated[1];
        pos[2] = headPos[2] + rotated[2];
    } else {
        pos[0] = local[0];
        pos[1] = local[1];
        pos[2] = local[2];
    }
    pos[0] += g_VRBindOffX; pos[1] += g_VRBindOffY; pos[2] += g_VRBindOffZ;

    // Translation @ +0 (QsTransform), Rotation @ +16 -- see file header.
    float* t = reinterpret_cast<float*>(boneBuf + bIdx * 48 + 0);
    t[0] = pos[0]; t[1] = pos[1]; t[2] = pos[2];

    if (writeRot) {
        // VR->game axis swap, same as the gizmo's mapLocalQuat: (i, -k, j, r).
        float localQuat[4] = { vrQuat[0], -vrQuat[2], vrQuat[1], vrQuat[3] };
        float* r = reinterpret_cast<float*>(boneBuf + bIdx * 48 + 16);
        if (g_VRUseHeadRelative && headOk) {
            VRIK_QuatMul(headQuat, localQuat, r);
        } else {
            r[0] = localQuat[0]; r[1] = localQuat[1]; r[2] = localQuat[2]; r[3] = localQuat[3];
        }
    }
}

// QsTransform field offsets inside each 48-byte bone slot.

// Model-space FK scratch (recomputed each matched frame). Sized generously.
float g_fkPos[VRIK_MAX_BONES][3];
float g_fkRot[VRIK_MAX_BONES][4];

// FK SNAPSHOT -- the measurement channel for capsule geometry.
//
// Scripts could only see eleven hand-picked bones (spine and legs), and the arms were not among them, so
// "does the forearm capsule match the forearm" and "where do the fingers end" were unanswerable without a
// plugin rebuild each time. This copies the whole solved rig once per pose pass so any bone can be measured
// by index; DumpPlayerBoneNames() writes the index table.
//
// Filled only while diag capture is on, which is also the only time the post-solve FK is computed at all --
// on normal frames the arms in g_fkPos are still the ENGINE'S ANIMATED pose, the exact trap that once put
// the ball at the player's hip. Count is published LAST so a reader either sees a complete snapshot or the
// previous one.
volatile float g_VRFKSnapPos[VRIK_MAX_BONES][3];
volatile float g_VRFKSnapRot[VRIK_MAX_BONES][4];
volatile int   g_VRFKSnapCount = 0;

void VRIK_PublishNativeCyberwarePose(uint8_t* buffer,uint64_t graph) {
    // Read-only diagnostic of the actual downstream result, rather than the
    // primary wrist alone (which may only drive a shadow on a cyberware rig).
    for(int i=0;i<16;++i)if(!g_VRCyberGraphHashes[i] || g_VRCyberGraphHashes[i]==graph) {
        g_VRCyberGraphHashes[i]=graph;++g_VRCyberGraphCalls[i];break;
    }
    if(!g_VRDiagCapture)return;
    const int count=VRIK_FKCount();VRIK_ComputeFK(buffer,count);
    for(int i=0;i<count;++i) {
        for(int k=0;k<3;++k)g_VRFKSnapPos[i][k]=g_fkPos[i][k];
        for(int k=0;k<4;++k)g_VRFKSnapRot[i][k]=g_fkRot[i][k];
    }
    g_VRFKSnapCount=count;
}

// FK walk length: only the prefix of the rig the solver actually reads
// (parents precede children, so the prefix is self-contained). Falls back to
// the full bone count until the resolve publishes the prefix.
int VRIK_FKCount() {
    const int n = g_VRFKCount;
    return (n > 0) ? n : g_VRBoneCount;
}

bool VRIK_ReferenceFingerRotation(int bone,float* rotation) {
    if(!rotation || bone<0 || bone>=g_VRBoneCount || !s_referenceValid[bone])return false;
    std::copy_n(s_referenceLocalRot[bone],4,rotation);return true;
}
bool cvr::gestures::ThumbReference(int side,float* rotation) {
    if(side<0 || side>1 || !rotation)return false;
    const int root=s_ladderFingers[side][0],tip=s_ladderFingers[side][10];
    const int index=s_ladderFingers[side][1],indexTip=s_ladderFingers[side][15];
    for(int bone:{root,tip,index,indexTip})if(bone<0 || bone>=g_VRBoneCount || !s_referenceValid[bone])return false;
    const int parent=g_VRBoneParent[root];if(parent<0 || !s_referenceValid[parent])return false;
    float from[3],to[3],inverse[4],a[3],b[3],delta[4];
    for(int k=0;k<3;++k){from[k]=s_referenceModelPos[tip][k]-s_referenceModelPos[root][k];to[k]=s_referenceModelPos[indexTip][k]-s_referenceModelPos[index][k];}
    VRIK_QuatConj(s_referenceModelRot[parent],inverse);
    VRIK_QuatRotateVec(inverse,from,a);VRIK_QuatRotateVec(inverse,to,b);
    if(VRIK_Dot3(a,a)<1e-8f || VRIK_Dot3(b,b)<1e-8f)return false;
    VRIK_Norm3(a);VRIK_Norm3(b);
    VRIK_QuatFromTo(a,b,delta);VRIK_QuatMul(delta,s_referenceLocalRot[root],rotation);VRIK_QuatNorm(rotation);
    return true;
}

bool cvr::gestures::SpreadAxis(int side,int digit,int* bone,float axis[3]) {
    if(side<0 || side>1 || !bone || !axis || digit<0 || digit>4 || digit==2)return false;
    const int* slot=s_ladderFingers[side];
    // Slots: 0-4 InHand*, 5-9 Hand*1 (knuckles), 10-14 Hand*2, 15-18 Hand*3 (no thumb 3).
    const int root=digit==0 ? slot[0] : slot[5+digit], next=digit==0 ? slot[10] : slot[10+digit];
    const int refFrom=digit==0 ? slot[1] : slot[7], refTo=digit==0 ? slot[15] : slot[12];
    const int indexKnuckle=slot[6],littleKnuckle=slot[9],palm=slot[2];
    for(int b:{root,next,refFrom,refTo,indexKnuckle,littleKnuckle,palm})if(b<0 || b>=g_VRBoneCount || !s_referenceValid[b])return false;
    const int parent=g_VRBoneParent[root];if(parent<0 || !s_referenceValid[parent])return false;
    const auto* p=s_referenceModelPos;
    float dir[3],ref[3],across[3],toPalm[3],normal[3],turn[3];
    for(int k=0;k<3;++k){dir[k]=p[next][k]-p[root][k];ref[k]=p[refTo][k]-p[refFrom][k];
        across[k]=p[littleKnuckle][k]-p[indexKnuckle][k];toPalm[k]=p[palm][k]-p[indexKnuckle][k];}
    normal[0]=across[1]*toPalm[2]-across[2]*toPalm[1];normal[1]=across[2]*toPalm[0]-across[0]*toPalm[2];normal[2]=across[0]*toPalm[1]-across[1]*toPalm[0];
    turn[0]=ref[1]*dir[2]-ref[2]*dir[1];turn[1]=ref[2]*dir[0]-ref[0]*dir[2];turn[2]=ref[0]*dir[1]-ref[1]*dir[0];
    if(VRIK_Dot3(normal,normal)<1e-12f)return false;
    VRIK_Norm3(normal);
    // Turning dir further around +normal increases its angle from ref when ref x dir points along +normal.
    if(VRIK_Dot3(turn,normal)<0)for(float& v:normal)v=-v;
    float inverse[4];VRIK_QuatConj(s_referenceModelRot[parent],inverse);VRIK_QuatRotateVec(inverse,normal,axis);
    if(VRIK_Dot3(axis,axis)<1e-12f)return false;
    VRIK_Norm3(axis);*bone=root;return true;
}

// Forward kinematics: accumulate parent-local transforms into model space.
// Requires parent index < child index (true for these rigs / topological order).
void VRIK_ComputeFK(uint8_t* boneBuf, int count) {
    if (count > VRIK_MAX_BONES) count = VRIK_MAX_BONES;
    for (int i = 0; i < count; ++i) {
        const float* lt = reinterpret_cast<float*>(boneBuf + i * 48 + VRIK_TRANS_OFF);
        const float* lr = reinterpret_cast<float*>(boneBuf + i * 48 + VRIK_ROT_OFF);
        float lpos[3] = { lt[0], lt[1], lt[2] };
        float lrot[4] = { lr[0], lr[1], lr[2], lr[3] };
        int p = g_VRBoneParent[i];
        if (p >= 0 && p < i) {
            VRIK_QuatMul(g_fkRot[p], lrot, g_fkRot[i]);
            VRIK_QuatNorm(g_fkRot[i]);
            float rp[3];
            VRIK_QuatRotateVec(g_fkRot[p], lpos, rp);
            g_fkPos[i][0] = g_fkPos[p][0] + rp[0];
            g_fkPos[i][1] = g_fkPos[p][1] + rp[1];
            g_fkPos[i][2] = g_fkPos[p][2] + rp[2];
        } else {
            g_fkRot[i][0]=lrot[0]; g_fkRot[i][1]=lrot[1]; g_fkRot[i][2]=lrot[2]; g_fkRot[i][3]=lrot[3];
            VRIK_QuatNorm(g_fkRot[i]);
            g_fkPos[i][0]=lpos[0]; g_fkPos[i][1]=lpos[1]; g_fkPos[i][2]=lpos[2];
        }
    }
}

// Writes a model-space rotation back into a bone as a LOCAL rotation, given the
// (already updated) model rotation of its parent. localRot = parentModel^-1 * modelRot.
void VRIK_WriteLocalRot(uint8_t* boneBuf, int idx,
                                      const float* parentModelRot, const float* modelRot) {
    float pInv[4]; VRIK_QuatConj(parentModelRot, pInv);
    float local[4]; VRIK_QuatMul(pInv, modelRot, local);
    VRIK_QuatNorm(local);
    float* r = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_ROT_OFF);
    r[0]=local[0]; r[1]=local[1]; r[2]=local[2]; r[3]=local[3];
}

void VRIK_WriteLocalPos(uint8_t* boneBuf, int idx,
                                      const float* parentModelPos, const float* parentModelRot,
                                      const float* modelPos) {
    float delta[3] = {
        modelPos[0] - parentModelPos[0],
        modelPos[1] - parentModelPos[1],
        modelPos[2] - parentModelPos[2]
    };
    float pInv[4]; VRIK_QuatConj(parentModelRot, pInv);
    float local[3]; VRIK_QuatRotateVec(pInv, delta, local);
    float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
    t[0]=local[0]; t[1]=local[1]; t[2]=local[2];
}

bool VRIK_AlignViewToHands(const float* viewWorldRot,const float* viewHeadXr,
                          const float* handHeadXr,float outWorldRot[4]) {
    float viewHead[4],handHead[4],view[4];
    std::copy_n(viewHeadXr,4,viewHead);std::copy_n(handHeadXr,4,handHead);
    std::copy_n(viewWorldRot,4,view);
    for(const float* q:{viewHead,handHead,view}) {
        const float n=q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3];
        if(!std::isfinite(n) || n<1e-6f)return false;
    }
    VRIK_QuatNorm(viewHead);VRIK_QuatNorm(handHead);VRIK_QuatNorm(view);
    float inverse[4],deltaXr[4];VRIK_QuatConj(viewHead,inverse);
    VRIK_QuatMul(inverse,handHead,deltaXr);
    const float deltaGame[4]={deltaXr[0],-deltaXr[2],deltaXr[1],deltaXr[3]};
    VRIK_QuatMul(view,deltaGame,outWorldRot);VRIK_QuatNorm(outWorldRot);
    return true;
}

void VRIK_WorldToModelRotation(float fallbackYaw,const float* entityRotation,float* result) {
    if(entityRotation) {
        VRIK_QuatConj(entityRotation,result);
        VRIK_QuatNorm(result);
    } else {
        result[0]=result[1]=0;
        result[2]=-std::sin(fallbackYaw*.5f);result[3]=std::cos(fallbackYaw*.5f);
    }
}

void VRIK_BuildViewHandTarget(const float* viewModelPos,const float* viewModelRot,
                             const float* handLocalPos,const float* handLocalRot,
                             float scale,const float* offset,float target[3],float controllerModelRot[4]) {
    const float mapped[3]={handLocalPos[0]*scale,-handLocalPos[2]*scale,handLocalPos[1]*scale};
    float delta[3];VRIK_QuatRotateVec(viewModelRot,mapped,delta);
    for(int k=0;k<3;++k)target[k]=viewModelPos[k]+delta[k]+offset[k];
    const float mappedRot[4]={handLocalRot[0],-handLocalRot[2],handLocalRot[1],handLocalRot[3]};
    VRIK_QuatMul(viewModelRot,mappedRot,controllerModelRot);VRIK_QuatNorm(controllerModelRot);
}

void VRIK_SyncShadowUpper(uint8_t* boneBuf,bool preserveLowerBody) {
    const int count=std::min(VRIK_FKCount(),VRIK_MAX_BONES);
    const float id[4]={0,0,0,1},zero[3]={};
    VRIK_ComputeFK(boneBuf,count);
    for(int bone=0;bone<count;++bone) {
        const int leg=s_bendLegShadowSource[bone];
        const int source=g_VRShadowSource[bone]>=0 ? g_VRShadowSource[bone]:
            (leg>=0 && s_bendLegOwned[leg] ? leg:-1);
        if(source<0 || source>=count)continue;
        if(preserveLowerBody && (source==g_VRHipsIdx || !g_VRUpperOwned[source]))continue;
        const int parent=g_VRBoneParent[bone];
        VRIK_WriteLocalPos(boneBuf,bone,parent>=0 ? g_fkPos[parent] : zero,
                          parent>=0 ? g_fkRot[parent] : id,g_fkPos[source]);
        VRIK_WriteLocalRot(boneBuf,bone,parent>=0 ? g_fkRot[parent] : id,g_fkRot[source]);
        std::copy_n(g_fkPos[source],3,g_fkPos[bone]);
        std::copy_n(g_fkRot[source],4,g_fkRot[bone]);
    }
    // Also refresh unmapped children of the shadow chain.
    VRIK_ComputeFK(boneBuf,count);
    for(auto& saved:s_bendLegRestore)if(saved.buffer==boneBuf && saved.revision==s_rigRevision &&
                                       saved.count>=6 && s_bendLegOwned[saved.bones[0]]) {
        for(int i=6;i<saved.count;++i)std::memcpy(saved.solved[i],boneBuf+saved.bones[i]*48,48);
        break;
    }
}

void VRIK_CaptureModelCache(bool preserveLowerBody) {
    const int count=std::min(VRIK_FKCount(),VRIK_MAX_BONES);
    g_solveCacheN=0;g_solveCacheModel=true;
    s_solveCacheCapturedMs=GetTickCount64();
    std::fill_n(s_modelCacheOwned,VRIK_MAX_BONES,false);
    for(int bone=0;bone<count;++bone) {
        const int leg=s_bendLegShadowSource[bone];
        if(preserveLowerBody && (bone==g_VRHipsIdx || g_VRShadowSource[bone]==g_VRHipsIdx ||
            (g_VRShadowSource[bone]>=0 && !g_VRUpperOwned[g_VRShadowSource[bone]]) ||
            (!g_VRUpperOwned[bone] && g_VRShadowSource[bone]<0)))continue;
        if(!g_VRUpperOwned[bone] && g_VRShadowSource[bone]<0 && !s_bendLegOwned[bone] &&
           !(leg>=0 && s_bendLegOwned[leg]))continue;
        s_modelCacheOwned[bone]=true;
        std::copy_n(g_fkPos[bone],3,s_modelCachePos[bone]);
        std::copy_n(g_fkRot[bone],4,s_modelCacheRot[bone]);
        g_solveCacheIdx[g_solveCacheN]=bone;
        std::copy_n(g_fkPos[bone],3,g_solveCacheVal[g_solveCacheN]);
        std::copy_n(g_fkRot[bone],4,g_solveCacheVal[g_solveCacheN]+3);
        ++g_solveCacheN;
    }
}

void VRIK_CaptureLocalArmCache(uint8_t* boneBuf,bool right,bool left) {
    g_solveCacheN=0;g_solveCacheModel=false;
    s_solveCacheCapturedMs=GetTickCount64();
    const int count=std::min(static_cast<int>(g_VRBoneCount),VRIK_MAX_BONES);
    auto push=[&](int bone) {
        if(!boneBuf || bone<0 || bone>=count || g_solveCacheN>=96)return;
        for(int i=0;i<g_solveCacheN;++i)if(g_solveCacheIdx[i]==bone)return;
        g_solveCacheIdx[g_solveCacheN]=bone;
        const auto* p=reinterpret_cast<const float*>(boneBuf+bone*48+VRIK_TRANS_OFF);
        const auto* q=reinterpret_cast<const float*>(boneBuf+bone*48+VRIK_ROT_OFF);
        std::copy_n(p,3,g_solveCacheVal[g_solveCacheN]);
        std::copy_n(q,4,g_solveCacheVal[g_solveCacheN]+3);
        ++g_solveCacheN;
    };
    auto arm=[&](int upper,int fore,int hand,const int* twist) {
        if(upper>=0 && upper<count)push(g_VRBoneParent[upper]);
        push(upper);push(fore);push(hand);
        for(int i=0;i<3;++i)push(twist[i]);
    };
    if(right)arm(g_VRRightUpperArmIdx,g_VRRightForeArmIdx,g_VRRightBoneIdx,g_VRForeTwistR);
    if(left)arm(g_VRLeftUpperArmIdx,g_VRLeftForeArmIdx,g_VRLeftBoneIdx,g_VRForeTwistL);
    // ...and the spine the arms hang from, while a grip has turned it: replayed without it, the next pass would
    // put both arms back on the untuned thorax and the hands would sit a turn away from their targets
    if(g_VRIKTorsoOn)for(const int bone:g_VRIKTorsoBones)push(bone);
}

// THE THORAX TURNS INTO A GRIP THE ARM CANNOT REACH (see CyberpunkVR_GripReach). The left wrist's shortfall from the
// last solve drives a turn of the upper spine about the body's vertical -- the left shoulder forward -- with a third
// as much forward lean, integrated so it settles at the turn that leaves the wrist a centimetre inside its cap, and
// unwinding once the grip opens. The two top spine bones carry it, 40/60. The LEFT shoulder's anchor then follows
// the turned thorax (g_VRIKTorsoShift); the right one is anchored where it always is, so the gun, which is built
// off the right hand, does not move with it.
void VRIK_GripTorso(uint8_t* boneBuf,const float* bodyRight,const float* bodyUp) {
    g_VRIKTorsoOn=false;
    g_VRIKTorsoShift[0]=g_VRIKTorsoShift[1]=g_VRIKTorsoShift[2]=0.0f;
    // elapsed time, measured here: this runs once per fresh solve, and the passes in between replay it
    float dt=.016f;
    {
        static LARGE_INTEGER f{},prev{};
        if(f.QuadPart==0)QueryPerformanceFrequency(&f);
        LARGE_INTEGER now{};QueryPerformanceCounter(&now);
        if(prev.QuadPart!=0 && f.QuadPart)dt=float(double(now.QuadPart-prev.QuadPart)/double(f.QuadPart));
        prev=now;
        if(!(dt>0.0f) || dt>.1f)dt=.016f;
    }
    static float s_turn=0.0f;
    const float maxTurn=std::clamp(CyberpunkVR_GripTorsoMaxDeg,0.0f,35.0f)*.01745329252f;
    if(CyberpunkVR_GripReach!=0 && g_VRIKGrip[1] && !g_VRIKVehicleUpperActive) {
        // the shoulder sits ~22 cm off the turn's pivot; settles in about a sixth of a second, slew-limited
        const float rate=std::clamp((g_VRIKShort[1]+.01f)/.22f*6.0f,-1.5f,1.5f);
        s_turn=std::clamp(s_turn+rate*dt,0.0f,maxTurn);
    } else {
        s_turn-=s_turn*std::min(1.0f,dt/.20f);
        if(s_turn<1e-4f)s_turn=0.0f;
    }
    CyberpunkVR_DebugGripTorsoDeg=s_turn*57.2957795f;
    const int count=std::clamp(static_cast<int>(g_VRSpineCount),0,8);
    const int upper=g_VRLeftUpperArmIdx;
    if(s_turn<=0.0f || count<2 || upper<0 || upper>=g_VRBoneCount || !bodyRight || !bodyUp)return;
    const int bones[2]={g_VRSpineIdx[count-2],g_VRSpineIdx[count-1]};
    const float share[2]={.4f,.6f};
    // about -up turns the left shoulder forward; about -right leans the chest over the reach
    float axis[3]={-bodyUp[0]-.33f*bodyRight[0],-bodyUp[1]-.33f*bodyRight[1],-bodyUp[2]-.33f*bodyRight[2]};
    if(VRIK_Norm3(axis)<1e-4f)return;
    VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    const float identity[4]={0,0,0,1};
    for(int i=0;i<2;++i) {
        const int bone=bones[i];
        if(bone<0 || bone>=g_VRBoneCount)return;
        const float half=s_turn*share[i]*.5f,sn=std::sin(half);
        const float q[4]={axis[0]*sn,axis[1]*sn,axis[2]*sn,std::cos(half)};
        // kept, so the shoulder's ANCHOR can be carried by the same turn: the animated shoulder this pose holds is
        // not where the anchor puts it (a rifle animation has it 18 cm forward of the spine, at the midline), and a
        // shift measured off that point swung the anchor sideways instead of forward
        std::copy_n(q,4,g_VRIKTorsoQ[i]);std::copy_n(g_fkPos[bone],3,g_VRIKTorsoP[i]);
        float model[4];VRIK_QuatMul(q,g_fkRot[bone],model);VRIK_QuatNorm(model);
        const int parent=g_VRBoneParent[bone];
        VRIK_WriteLocalRot(boneBuf,bone,parent>=0 ? g_fkRot[parent]:identity,model);
        VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    }
    g_VRIKTorsoBones[0]=bones[0];g_VRIKTorsoBones[1]=bones[1];
    g_VRIKTorsoOn=true;
}

// A model-space point carried by this solve's thorax turn: each rotation about its own pivot, lower bone first --
// exactly what the turn did to everything hanging from the chest.
static void VRIK_TorsoCarry(float* p) {
    for(int i=0;i<2;++i) {
        float r[3]={p[0]-g_VRIKTorsoP[i][0],p[1]-g_VRIKTorsoP[i][1],p[2]-g_VRIKTorsoP[i][2]},o[3];
        VRIK_QuatRotateVec(g_VRIKTorsoQ[i],r,o);
        for(int k=0;k<3;++k)p[k]=g_VRIKTorsoP[i][k]+o[k];
    }
}

void VRIK_ReplaySolveCache(uint8_t* boneBuf) {
    if(g_solveCacheModel) {VRIK_ReplayModelCache(boneBuf);return;}
    for(int i=0;i<g_solveCacheN;++i) {
        const int bone=g_solveCacheIdx[i];
        if(bone<0 || bone>=std::min(static_cast<int>(g_VRBoneCount),VRIK_MAX_BONES))continue;
        auto* p=reinterpret_cast<float*>(boneBuf+bone*48+VRIK_TRANS_OFF);
        auto* q=reinterpret_cast<float*>(boneBuf+bone*48+VRIK_ROT_OFF);
        std::copy_n(g_solveCacheVal[i],3,p);
        std::copy_n(g_solveCacheVal[i]+3,4,q);
    }
}

bool VRIK_RestoreMissingCamera(uint8_t* boneBuf,VrikCameraStatus status,uint32_t tick,uint64_t nowMs) {
    if(status==VrikCameraStatus::Unavailable && g_solveCacheN>0 &&
       nowMs>=s_solveCacheCapturedMs && nowMs-s_solveCacheCapturedMs<=250) {
        VRIK_ReplaySolveCache(boneBuf);
        g_solveCacheTick=tick;
        return true;
    }
    g_solveCacheN=0;g_solveCacheModel=false;g_solveCacheTick=0xFFFFFFFFu;
    return false;
}

void VRIK_ReplayModelCache(uint8_t* boneBuf) {
    const int count=std::min(VRIK_FKCount(),VRIK_MAX_BONES);
    auto* vehicleLower=g_VRIKVehicleUpperActive ? SaveVehicleLower(boneBuf) : nullptr;
    const float id[4]={0,0,0,1},zero[3]={};
    for(int bone=0;bone<count;++bone) {
        const int parent=g_VRBoneParent[bone];
        const bool haveParent=parent>=0 && parent<bone;
        const float* parentPos=haveParent ? g_fkPos[parent] : zero;
        const float* parentRot=haveParent ? g_fkRot[parent] : id;
        if(s_modelCacheOwned[bone]) {
            VRIK_WriteLocalPos(boneBuf,bone,parentPos,parentRot,s_modelCachePos[bone]);
            VRIK_WriteLocalRot(boneBuf,bone,parentRot,s_modelCacheRot[bone]);
            std::copy_n(s_modelCachePos[bone],3,g_fkPos[bone]);
            std::copy_n(s_modelCacheRot[bone],4,g_fkRot[bone]);
        } else {
            const float* p=reinterpret_cast<const float*>(boneBuf+bone*48+VRIK_TRANS_OFF);
            const float* q=reinterpret_cast<const float*>(boneBuf+bone*48+VRIK_ROT_OFF);
            VRIK_QuatMul(parentRot,q,g_fkRot[bone]);VRIK_QuatNorm(g_fkRot[bone]);
            VRIK_QuatRotateVec(parentRot,p,g_fkPos[bone]);
            for(int k=0;k<3;++k)g_fkPos[bone][k]+=parentPos[k];
        }
    }
    if(vehicleLower)for(int i=0;i<vehicleLower->count;++i)
        std::memcpy(vehicleLower->solved[i],boneBuf+vehicleLower->bones[i]*48,48);
    VRIK_PublishBodyBones();
}

void VRIK_PublishBodyBones() {
    const int count=std::clamp(static_cast<int>(g_VRSpineCount),0,8);
    const int chest=count ? g_VRSpineIdx[count-1] : -1;
    const int mid=count>1 ? g_VRSpineIdx[count/2] : chest;
    const int bones[11]={g_VRHipsIdx,mid,chest,g_VRNeckIdx,g_VRHeadBoneIdx,
        g_VRLeftUpLegIdx,g_VRLeftLegIdx,g_VRLeftFootIdx,g_VRRightUpLegIdx,g_VRRightLegIdx,g_VRRightFootIdx};
    for(int slot=0;slot<11;++slot) {
        const int bone=bones[slot];
        g_VRBodyBoneOk[slot]=bone>=0 && bone<VRIK_FKCount();
        if(g_VRBodyBoneOk[slot])for(int k=0;k<3;++k)g_VRBodyBone[slot][k]=g_fkPos[bone][k];
    }
}

void VRIK_DampenTorsoWeaponPose(uint8_t* boneBuf) {
    // Only the anatomical spine here. Pelvis placement happens below; controls
    // and leg local animation stay native. Identity is not a reference basis.
    for(int i=0;i<g_VRSpineCount && i<8;++i) {
        const int bone=g_VRSpineIdx[i];
        if(bone<0 || bone>=g_VRBoneCount || !s_referenceValid[bone])continue;
        std::copy_n(s_referenceLocalRot[bone],4,
                    reinterpret_cast<float*>(boneBuf+bone*48+VRIK_ROT_OFF));
    }
}

// Animation translations are not rest geometry: the first frame may already
// contain an armed/VR pose. Use the rig reference before calibration and IK.
void VRIK_PinGirdleTranslations(uint8_t* boneBuf) {
    int idx[8];
    idx[0] = (g_VRRightUpperArmIdx >= 0 && g_VRRightUpperArmIdx < VRIK_MAX_BONES)
             ? g_VRBoneParent[g_VRRightUpperArmIdx] : -1;   // right clavicle
    idx[1] = g_VRRightUpperArmIdx;
    idx[2] = g_VRRightForeArmIdx;
    idx[3] = g_VRRightBoneIdx;                              // right hand
    idx[4] = (g_VRLeftUpperArmIdx >= 0 && g_VRLeftUpperArmIdx < VRIK_MAX_BONES)
             ? g_VRBoneParent[g_VRLeftUpperArmIdx] : -1;    // left clavicle
    idx[5] = g_VRLeftUpperArmIdx;
    idx[6] = g_VRLeftForeArmIdx;
    idx[7] = g_VRLeftBoneIdx;                               // left hand
    for (int k = 0; k < 8; ++k) {
        if(idx[k]<0 || idx[k]>=g_VRBoneCount || !s_referenceValid[idx[k]])continue;
        float* t = reinterpret_cast<float*>(boneBuf + idx[k] * 48 + VRIK_TRANS_OFF);
        std::copy_n(s_referenceLocalPos[idx[k]],3,t);
    }
}

void VRIK_ResetShoulderReference(uint8_t* boneBuf,int upperIdx) {
    if(upperIdx<0 || upperIdx>=g_VRBoneCount)return;
    const int clavicle=g_VRBoneParent[upperIdx];
    if(clavicle<0 || clavicle>=upperIdx || !s_referenceValid[clavicle] ||
       !s_referenceValid[upperIdx])return;
    std::copy_n(s_referenceLocalPos[clavicle],3,reinterpret_cast<float*>(boneBuf+clavicle*48));
    std::copy_n(s_referenceLocalRot[clavicle],4,reinterpret_cast<float*>(boneBuf+clavicle*48+VRIK_ROT_OFF));
    std::copy_n(s_referenceLocalPos[upperIdx],3,reinterpret_cast<float*>(boneBuf+upperIdx*48));
    VRIK_ComputeFK(boneBuf,VRIK_FKCount());
}

bool VRIK_BodyAxesFromRig(float* bodyRight,float* bodyUp,float* bodyFwd,bool upperTorso) {
    // The shoulder line still contains native weapon animation when this runs.
    // Use the pelvis relative to its immutable rig basis; body placement owns
    // that pose on foot. Neither head motion nor clavicle aim can feed back into
    // the axes used to solve the shoulders and elbows.
    // A passenger leaning through the window keeps an authored, twisted trunk.
    // Its shoulders follow the chest; the seated pelvis is not their frame.
    const int basis=upperTorso && g_VRSpineCount>0
        ? g_VRSpineIdx[g_VRSpineCount-1] : g_VRHipsIdx;
    if(basis<0 || basis>=VRIK_FKCount() || !s_referenceValid[basis])return false;
    float inverse[4],body[4];
    VRIK_QuatConj(s_referenceModelRot[basis],inverse);
    VRIK_QuatMul(g_fkRot[basis],inverse,body);
    float norm=0;
    for(float q:body) { if(!std::isfinite(q))return false;norm+=q*q; }
    if(norm<1e-8f)return false;
    VRIK_QuatNorm(body);
    const float right[3]={1,0,0},up[3]={0,0,1},forward[3]={0,1,0};
    VRIK_QuatRotateVec(body,right,bodyRight);
    VRIK_QuatRotateVec(body,up,bodyUp);
    VRIK_QuatRotateVec(body,forward,bodyFwd);
    return true;
}

void VRIK_AnchorShoulder(uint8_t* boneBuf,int upperIdx,const float* anchor,bool isLeft,
                         const float* bodyRight,const float* bodyUp,float halfWidth,float* outJoint,bool neckAnchor) {
    float half = std::fabs(halfWidth);
    if (half < 0.13f) half = 0.13f;
    if (half > 0.19f) half = 0.19f;
    const float drop = 0.17f;
    float side = isLeft ? -1.0f : 1.0f;
    float desired[3] = {
        anchor[0] + bodyRight[0]*(side*half) - bodyUp[0]*drop,
        anchor[1] + bodyRight[1]*(side*half) - bodyUp[1]*drop,
        anchor[2] + bodyRight[2]*(side*half) - bodyUp[2]*drop };
    if(neckAnchor && upperIdx>=0 && upperIdx<g_VRBoneCount &&
       g_VRNeckIdx>=0 && g_VRNeckIdx<g_VRBoneCount &&
       s_referenceValid[upperIdx] && s_referenceValid[g_VRNeckIdx]) {
        // The body anchor is the neck on foot as well as in Combat. The legacy
        // head-anchor drop put shoulders 17cm below the neck and shortened reach.
        // Use the rig's actual neck-to-shoulder height/depth with calibrated width.
        float forward[3];VRIK_Cross3(bodyUp,bodyRight,forward);
        const float height=s_referenceModelPos[upperIdx][2]-s_referenceModelPos[g_VRNeckIdx][2];
        const float depth=s_referenceModelPos[upperIdx][1]-s_referenceModelPos[g_VRNeckIdx][1];
        for(int k=0;k<3;++k)desired[k]=anchor[k]+bodyRight[k]*(side*half)+bodyUp[k]*height+forward[k]*depth;
    }
    // ...carried with the thorax while a grip has turned it (VRIK_GripTorso) -- the LEFT shoulder only. The joint
    // handed back for the hand target stays where it would have been: the turn is there to REACH the target, and a
    // target built off the turned shoulder would move away by exactly as much.
    const bool turned=isLeft && g_VRIKTorsoOn;
    if(turned) {
        float carried[3]={desired[0],desired[1],desired[2]};VRIK_TorsoCarry(carried);
        for(int k=0;k<3;++k) { g_VRIKTorsoShift[k]=carried[k]-desired[k];desired[k]=carried[k]; }
        CyberpunkVR_DebugGripReach[0]=std::sqrt(VRIK_Dot3(g_VRIKTorsoShift,g_VRIKTorsoShift))*1000.0f;
    }
    int clavi = (upperIdx >= 0 && upperIdx < VRIK_MAX_BONES) ? g_VRBoneParent[upperIdx] : -1;
    const int dbgSide = isLeft ? 1 : 0;
    g_VRIKDbgClav[dbgSide][0]=desired[0]; g_VRIKDbgClav[dbgSide][1]=desired[1]; g_VRIKDbgClav[dbgSide][2]=desired[2];
    g_VRIKDbgClav[dbgSide][6]=0.0f; g_VRIKDbgClav[dbgSide][7]=0.0f;
    if (clavi >= 0 && clavi < VRIK_MAX_BONES) {
        // Start the clavicle aim at the rig's actual pivot/basis.
        // A widest animated pose can already contain weapon or VR writes.
        VRIK_ResetShoulderReference(boneBuf,upperIdx);
        const float* pv = g_fkPos[clavi];
        float cur[3] = { g_fkPos[upperIdx][0]-pv[0], g_fkPos[upperIdx][1]-pv[1], g_fkPos[upperIdx][2]-pv[2] };
        float des[3] = { desired[0]-pv[0], desired[1]-pv[1], desired[2]-pv[2] };
        if (VRIK_Norm3(cur) > 1e-4f && VRIK_Norm3(des) > 1e-4f) {
            float d4[4]; VRIK_QuatFromTo(cur, des, d4);
            if (d4[3] < 0.0f) { d4[0]=-d4[0]; d4[1]=-d4[1]; d4[2]=-d4[2]; d4[3]=-d4[3]; }
            float ang = 2.0f * std::acos(std::fmin(1.0f, d4[3]));
            // Keep the existing angular limit, now measured from
            // a fixed clavicle basis instead of an armed animation.
            const float kMaxClav = 1.3090f;   // 75 deg
            float applied = ang;
            if (ang > kMaxClav && ang > 1e-4f) { VRIK_QuatScale(d4, kMaxClav/ang, d4); applied = kMaxClav; }
            g_VRIKDbgClav[dbgSide][6] = ang * 57.29578f;
            g_VRIKDbgClav[dbgSide][7] = applied * 57.29578f;
            float nm[4]; VRIK_QuatMul(d4, g_fkRot[clavi], nm); VRIK_QuatNorm(nm);
            int pp = g_VRBoneParent[clavi];
            float idq[4] = { 0,0,0,1 };
            VRIK_WriteLocalRot(boneBuf, clavi, (pp>=0&&pp<VRIK_MAX_BONES)?g_fkRot[pp]:idq, nm);
            VRIK_ComputeFK(boneBuf, VRIK_FKCount());
        }
    }
    outJoint[0]=g_fkPos[upperIdx][0]; outJoint[1]=g_fkPos[upperIdx][1]; outJoint[2]=g_fkPos[upperIdx][2];
    if(turned)for(int k=0;k<3;++k)outJoint[k]-=g_VRIKTorsoShift[k];
    if(clavi>=0 && clavi<upperIdx) {
        auto& base=s_shoulderBase[dbgSide];base.buffer=boneBuf;base.revision=s_rigRevision;
        base.upper=upperIdx;base.clavicle=clavi;
        std::copy_n(reinterpret_cast<const float*>(boneBuf+clavi*48+VRIK_ROT_OFF),4,base.clavicleLocalRot);
        std::copy_n(reinterpret_cast<const float*>(boneBuf+upperIdx*48),3,base.upperLocalPos);
    }
    g_VRIKDbgClav[dbgSide][3]=outJoint[0]; g_VRIKDbgClav[dbgSide][4]=outJoint[1]; g_VRIKDbgClav[dbgSide][5]=outJoint[2];
    if (!isLeft) {
        // Trace probes: right shoulder joint (model) + hips MODEL yaw.
        g_VRIKDbgShModel[0] = outJoint[0];
        g_VRIKDbgShModel[1] = outJoint[1];
        g_VRIKDbgShModel[2] = outJoint[2];
        const int hb = g_VRHipsIdx;
        if (hb >= 0 && hb < VRIK_MAX_BONES) {
            // Yaw of the hips bone in model space: heading of its
            // local +X axis (rig lateral) projected to the ground.
            const float* q = g_fkRot[hb];
            const float axX = 1.0f - 2.0f*(q[1]*q[1] + q[2]*q[2]);
            const float axY = 2.0f*(q[0]*q[1] + q[2]*q[3]);
            g_VRIKDbgHipsYaw = std::atan2(axY, axX) * 57.29578f;
        }
    }
}

void VRIK_UpdateForearmDeformation(uint8_t* boneBuf,int foreIdx,int handIdx,bool left) {
    if(VRIK_UsesNativeCyberware())return;
    if(foreIdx<0 || handIdx<0 || foreIdx>=g_VRBoneCount || handIdx>=g_VRBoneCount ||
       g_VRBoneParent[handIdx]!=foreIdx || !s_referenceValid[handIdx])return;
    float axis[3];std::copy_n(s_referenceLocalPos[handIdx],3,axis);
    const float restLength=VRIK_Norm3(axis);
    if(restLength<1e-4f)return;
    const float* hand=reinterpret_cast<const float*>(boneBuf+handIdx*48);
    float inverse[4],delta[4],twist[4];
    VRIK_QuatConj(s_referenceLocalRot[handIdx],inverse);
    VRIK_QuatMul(hand+4,inverse,delta);VRIK_QuatNorm(delta);
    VRIK_QuatExtractTwist(delta,axis,twist);
    if(twist[3]<0)for(float& v:twist)v=-v;
    const float angle=std::clamp(2*std::atan2(VRIK_Dot3(twist,axis),twist[3]),-2.0944f,2.0944f);
    const float length=std::sqrt(VRIK_Dot3(hand,hand));
    const float weights[3]={.2f,.4f,.6f};
    const int* helpers=left ? g_VRForeTwistL : g_VRForeTwistR;
    for(int slot=0;slot<3;++slot) {
        const int bone=helpers[slot];
        if(bone<0 || bone>=g_VRBoneCount || g_VRBoneParent[bone]!=foreIdx || !s_referenceValid[bone])continue;
        float* local=reinterpret_cast<float*>(boneBuf+bone*48);
        const float half=angle*weights[slot]*.5f,s=std::sin(half);
        const float roll[4]={axis[0]*s,axis[1]*s,axis[2]*s,std::cos(half)};
        VRIK_QuatMul(roll,s_referenceLocalRot[bone],local+4);VRIK_QuatNorm(local+4);
        // Calibrated segment length changes must also move its skin helpers.
        // Keep their radial offset and scale only distance along the segment.
        const float along=VRIK_Dot3(s_referenceLocalPos[bone],axis);
        for(int k=0;k<3;++k)local[k]=s_referenceLocalPos[bone][k]+axis[k]*along*(length/restLength-1);
    }
}

// Two-bone arm IK in model space. Rotates upper arm + forearm so the wrist reaches the model-space
// target. Algorithm follows a production-grade VR full-body solver (a baseline skeleton solver,
// `setArms`), cross-validated against established arm-IK and VR-IK conventions.
//
// Approach (different from a naive "law of cosines at the shoulder"):
//   * xDir   = unit vector from HAND -> SHOULDER (so anchored at the player's actual hand position).
//   * yDir   = unit vector PERPENDICULAR to xDir, in the plane the elbow should sit; built from
//              anatomical hints (down, back, side) projected onto the plane normal to xDir.
//   * Cosine law gives the WRIST angle (the angle of the upper/forearm/hand triangle measured at
//     the hand), so elbow = handPos + xDir * cos(wrist)*foreLen + yDir * sin(wrist)*foreLen.
//   * If hand is out of reach (hsLen > upLen+foreLen), the upper+forearm lengths are stretched
//     proportionally to cover the gap. This is what allows the arm to go COMPLETELY STRAIGHT when
//     extended — clamping the distance back to armLen-eps locks in a permanent bend (the F4VR
//     code explicitly does this stretch instead, see comments below).
//
// The elbow-direction logic uses a body-frame anatomical hint that's stable everywhere:
//   * Default: elbow points DOWN (project -bodyUp onto plane perpendicular to xDir).
//   * Fallback when arm is vertical (axis nearly parallel to -bodyUp): use -bodyFwd (back).
//   * Cross-body correction: when hand reaches across midline, swing elbow OUT to the own side.
//   * Near-straight fade: as |handToShoulder| approaches armLen, fade the cross-body swing to 0
//     so the elbow doesn't twitch sideways during a fully-extended pose.
// Quaternion from an orthonormal basis built on X (primary) + Y-hint (Gram-Schmidt).
static inline void VRIK_QuatFromAxes(const float* x, const float* yHint, float* outQ) {
    float X[3] = { x[0], x[1], x[2] }; VRIK_Norm3(X);
    float Y[3] = { yHint[0], yHint[1], yHint[2] };
    float d = VRIK_Dot3(Y, X); Y[0]-=X[0]*d; Y[1]-=X[1]*d; Y[2]-=X[2]*d;
    if (VRIK_Norm3(Y) < 1e-5f) {
        Y[0]=X[1]; Y[1]=X[2]; Y[2]=X[0];
        d = VRIK_Dot3(Y, X); Y[0]-=X[0]*d; Y[1]-=X[1]*d; Y[2]-=X[2]*d; VRIK_Norm3(Y);
    }
    float Z[3]; VRIK_Cross3(X, Y, Z);
    const float m00=X[0], m01=Y[0], m02=Z[0];
    const float m10=X[1], m11=Y[1], m12=Z[1];
    const float m20=X[2], m21=Y[2], m22=Z[2];
    float tr = m00 + m11 + m22;
    if (tr > 0.0f) {
        float s = std::sqrt(tr + 1.0f) * 2.0f;
        outQ[3]=0.25f*s; outQ[0]=(m21-m12)/s; outQ[1]=(m02-m20)/s; outQ[2]=(m10-m01)/s;
    } else if (m00 > m11 && m00 > m22) {
        float s = std::sqrt(1.0f + m00 - m11 - m22) * 2.0f;
        outQ[3]=(m21-m12)/s; outQ[0]=0.25f*s; outQ[1]=(m01+m10)/s; outQ[2]=(m02+m20)/s;
    } else if (m11 > m22) {
        float s = std::sqrt(1.0f + m11 - m00 - m22) * 2.0f;
        outQ[3]=(m02-m20)/s; outQ[0]=(m01+m10)/s; outQ[1]=0.25f*s; outQ[2]=(m12+m21)/s;
    } else {
        float s = std::sqrt(1.0f + m22 - m00 - m11) * 2.0f;
        outQ[3]=(m10-m01)/s; outQ[0]=(m02+m20)/s; outQ[1]=(m12+m21)/s; outQ[2]=0.25f*s;
    }
    VRIK_QuatNorm(outQ);
}
// Model rotation R with R*aLoc = aW and R*hLoc = hW (both pairs orthonormalised on their
// primary): R = Basis(aW,hW) * Basis(aLoc,hLoc)^-1. Fully determines a bone's SWING AND TWIST
// from its anatomical axis (aLoc: bone-local segment axis, a rig CONSTANT captured at runtime)
// and its anatomical hinge (hLoc: bone-local elbow-hinge axis, also a rig constant).
static inline void VRIK_QuatAlignTwo(const float* aLoc, const float* hLoc,
                                     const float* aW, const float* hW, float* outQ) {
    float qw[4]; VRIK_QuatFromAxes(aW, hW, qw);
    float ql[4]; VRIK_QuatFromAxes(aLoc, hLoc, ql);
    float qlc[4] = { -ql[0], -ql[1], -ql[2], ql[3] };
    VRIK_QuatMul(qw, qlc, outQ); VRIK_QuatNorm(outQ);
}
// Swing/twist decomposition: the twist component of q around a unit axis.
static inline void VRIK_QuatExtractTwist(const float* q, const float* axis, float* outT) {
    float d = q[0]*axis[0] + q[1]*axis[1] + q[2]*axis[2];
    outT[0]=axis[0]*d; outT[1]=axis[1]*d; outT[2]=axis[2]*d; outT[3]=q[3];
    float n = std::sqrt(outT[0]*outT[0]+outT[1]*outT[1]+outT[2]*outT[2]+outT[3]*outT[3]);
    if (n < 1e-6f) { outT[0]=outT[1]=outT[2]=0.0f; outT[3]=1.0f; return; }
    outT[0]/=n; outT[1]/=n; outT[2]/=n; outT[3]/=n;
    if (outT[3] < 0.0f) { outT[0]=-outT[0]; outT[1]=-outT[1]; outT[2]=-outT[2]; outT[3]=-outT[3]; }
}

// REVERTED TO THE 0.1.18 SOLVER: with it the shoulders sat right, the elbows
// behaved better and the biceps did not stretch. The elbow direction is built the VRArmIK way -- a
// rest pose (armRest/bendRest) swung onto the current shoulder->hand axis, then deviated by a formula
// in body-normalised coordinates -- instead of being derived from one anatomical hint per case; the
// forward-bend strip and the outward reference are part of that construction. The min-flex clamp that
// the replacement added is gone with it.
// Upper-arm REST local translation, per side (0 = right, 1 = left). Captured on the first solve and
// used by the shoulder protraction inside VRIK_SolveArm, which writes that translation ABSOLUTE
// against it. File scope rather than a static inside the function because the WHEEL GRAB has to be
// able to put the same rest value BACK when it hands the arm to the driving animation -- the
// protraction write is not reverted by anything else (iPowerTech, 425d4262).
float g_vrikUpArmRest[2][3] = {};
bool  g_vrikUpArmRestCap[2] = { false, false };

static bool VRIK_ReachGirdle(uint8_t* buffer,int upper,const float* target,
                             const float* up,const float* forward,
                             float armLength,bool left,bool ladderReach,bool grip) {
    const int side=left ? 1:0;const auto& base=s_shoulderBase[side];
    std::fill_n(g_VRIKGirdleReach[side],4,0.0f);
    if(base.buffer!=buffer || base.revision!=s_rigRevision || base.upper!=upper ||
       base.clavicle<0 || armLength<.1f)return false;
    const int clavicle=base.clavicle;
    // Always start at this solve's calibrated clavicle aim, not our last reach.
    std::copy_n(base.clavicleLocalRot,4,reinterpret_cast<float*>(buffer+clavicle*48+VRIK_ROT_OFF));
    std::copy_n(base.upperLocalPos,3,reinterpret_cast<float*>(buffer+upper*48));
    std::copy_n(base.upperLocalPos,3,g_vrikUpArmRest[side]);g_vrikUpArmRestCap[side]=true;
    VRIK_ComputeFK(buffer,VRIK_FKCount());
    float start[3],dir[3];std::copy_n(g_fkPos[upper],3,start);
    for(int k=0;k<3;++k)dir[k]=target[k]-start[k];const float distance=VRIK_Norm3(dir);
    // A GRIP has to arrive (see CyberpunkVR_GripReach): the girdle starts earlier, protracts further -- forward
    // and, reaching across, round the ribcage toward the middle -- and the blade carries more of it. On foot only;
    // the ladder and the seat keep their own profiles.
    const bool g=grip && !ladderReach && !g_VRIKVehicleUpperActive;
    const float weight=g ? cvr::body::SmoothRange(distance/armLength,.84f,1.06f)
                         : cvr::body::SmoothRange(distance/armLength,ladderReach ? .88f:.94f,ladderReach ? 1.10f:1.14f);
    const float front=std::clamp(VRIK_Dot3(dir,forward),-.35f,1.0f);
    const float rise=std::clamp(VRIK_Dot3(dir,up),-.2f,1.0f);
    // Combat's explicit camera mount puts the optical centre above/in front of
    // the avatar's eyes. Let the seated girdle protract for long reaches; keep
    // the sternum pivot, clavicle length and humeral translation limit intact.
    const float protract=ladderReach ? .06f:(g_VRIKVehicleUpperActive ? .07f:
        (g ? std::clamp(CyberpunkVR_GripProtract,0.0f,.12f):.035f));
    // Raising an extended arm rotates the clavicle upward. The old 15mm/12deg
    // forward-reach profile left the shoulder down even above the head, so the
    // wrist hit the segment caps before reaching the tracked hand. Blend only
    // the on-foot elevated workspace; forward/downward and mounted poses keep
    // their existing geometry and neither arm segment gets a larger stretch cap.
    const float overhead=cvr::body::SmoothRange(rise,.15f,.9f);
    const float downward=(!ladderReach && !g_VRIKVehicleUpperActive)
        ? cvr::body::SmoothRange(-VRIK_Dot3(dir,up),.3f,.9f):0.0f;
    const float elevate=ladderReach ? .035f:(g_VRIKVehicleUpperActive ? .03f:(.015f+.085f*overhead));
    float maxClavicle=ladderReach ? .34906585f:(g_VRIKVehicleUpperActive ? .38397244f:(.20943951f+.40142573f*overhead+.17453293f*downward));
    if(g)maxClavicle=std::max(maxClavicle,.40142573f);                 // 23 deg of forward swing
    const float shoulderGlide=ladderReach ? .022f:(g ? .03f:(.015f+.02f*downward));
    const float bladeShare=ladderReach ? .75f:(g ? .8f:.6f);
    // ...and the share of the reach that runs ACROSS the body, which a protracting scapula follows round the ribs
    float across[3]={0,0,0};
    if(g)for(int k=0;k<3;++k)across[k]=dir[k]-forward[k]*VRIK_Dot3(dir,forward)-up[k]*VRIK_Dot3(dir,up);
    float desired[3],current[3],arc[3];
    for(int k=0;k<3;++k) {
        desired[k]=start[k]+weight*(forward[k]*protract*front+across[k]*protract*.4f+up[k]*(elevate*rise-.045f*downward));
        current[k]=start[k]-g_fkPos[clavicle][k];arc[k]=desired[k]-g_fkPos[clavicle][k];
    }
    float rotationDelta[4]={0,0,0,1};
    if(VRIK_Norm3(current)>1e-4f && VRIK_Norm3(arc)>1e-4f) {
        VRIK_QuatFromTo(current,arc,rotationDelta);
        if(rotationDelta[3]<0)for(float& q:rotationDelta)q=-q;
        float angle=2*std::acos(std::clamp(rotationDelta[3],0.0f,1.0f));
        if(angle>maxClavicle) { VRIK_QuatScale(rotationDelta,maxClavicle/angle,rotationDelta);angle=maxClavicle; }
        float model[4];VRIK_QuatMul(rotationDelta,g_fkRot[clavicle],model);VRIK_QuatNorm(model);
        const int parent=g_VRBoneParent[clavicle];const float identity[4]={0,0,0,1};
        VRIK_WriteLocalRot(buffer,clavicle,parent>=0 ? g_fkRot[parent]:identity,model);
        VRIK_ComputeFK(buffer,VRIK_FKCount());
        g_VRIKGirdleReach[side][1]=angle*57.2957795f;
    }
    // A small glenohumeral translation supplements the clavicle's arc. Keep
    // the sternum pivot and clavicle radius fixed instead of stretching them.
    float joint[3];
    for(int k=0;k<3;++k)joint[k]=g_fkPos[upper][k]+dir[k]*(shoulderGlide*weight);
    VRIK_WriteLocalPos(buffer,upper,g_fkPos[clavicle],g_fkRot[clavicle],joint);
    VRIK_ComputeFK(buffer,VRIK_FKCount());
    float movement[3];for(int k=0;k<3;++k)movement[k]=g_fkPos[upper][k]-start[k];
    float bladeRotation[4];VRIK_QuatScale(rotationDelta,bladeShare,bladeRotation);
    for(int slot=0;slot<s_scapulaCount[side];++slot) {
        const int bone=s_scapula[side][slot],parent=g_VRBoneParent[bone];
        if(parent<0 || parent>=bone)continue;
        float position[3],rotation[4],model[4];
        VRIK_QuatRotateVec(g_fkRot[parent],s_referenceLocalPos[bone],position);
        for(int k=0;k<3;++k)position[k]+=g_fkPos[parent][k]+movement[k]*bladeShare;
        VRIK_QuatMul(g_fkRot[parent],s_referenceLocalRot[bone],rotation);
        VRIK_QuatMul(bladeRotation,rotation,model);VRIK_QuatNorm(model);
        VRIK_WriteLocalPos(buffer,bone,g_fkPos[parent],g_fkRot[parent],position);
        VRIK_WriteLocalRot(buffer,bone,g_fkRot[parent],model);
    }
    VRIK_ComputeFK(buffer,VRIK_FKCount());
    g_VRIKGirdleReach[side][0]=weight;
    g_VRIKGirdleReach[side][2]=std::sqrt(VRIK_Dot3(movement,movement));
    g_VRIKGirdleReach[side][3]=g_VRIKGirdleReach[side][2]*bladeShare;
    if(left) {
        CyberpunkVR_DebugGripReach[1]=weight;
        CyberpunkVR_DebugGripReach[2]=g_VRIKGirdleReach[side][1];
        CyberpunkVR_DebugGripReach[3]=g_VRIKGirdleReach[side][2]*1000.0f;
        CyberpunkVR_DebugGripReach[4]=distance/armLength;
        CyberpunkVR_DebugGripReach[5]=g ? 1.0f : 0.0f;
    }
    return true;
}

static void VRIK_UpdateUpperArmDeformation(uint8_t* buffer,int fore,bool left) {
    if(fore<0 || fore>=g_VRBoneCount || !s_referenceValid[fore])return;
    float axis[3];std::copy_n(s_referenceLocalPos[fore],3,axis);const float rest=VRIK_Norm3(axis);
    if(rest<1e-4f)return;
    const float* end=reinterpret_cast<const float*>(buffer+fore*48);
    const float scale=std::sqrt(VRIK_Dot3(end,end))/rest;
    for(int bone:s_upperSkin[left ? 1:0])if(bone>=0 && s_referenceValid[bone]) {
        float* local=reinterpret_cast<float*>(buffer+bone*48);
        const float along=VRIK_Dot3(s_referenceLocalPos[bone],axis);
        for(int k=0;k<3;++k)local[k]=s_referenceLocalPos[bone][k]+axis[k]*along*(scale-1);
    }
}

void VRIK_SolveArm(uint8_t* boneBuf, int upperIdx, int foreIdx, int handIdx,
                                 const float* targetModel, const float* handModelRot,
                                 const float* bodyRight, const float* bodyUp, const float* bodyFwd,
                                 float poleAngleRad, float swingGain, bool isLeft,
                                 bool storeDbg,bool ladderReach) {
    if (upperIdx < 0 || foreIdx < 0 || handIdx < 0) return;
    if (upperIdx >= VRIK_MAX_BONES || foreIdx >= VRIK_MAX_BONES || handIdx >= VRIK_MAX_BONES) return;

    const float* sh = g_fkPos[upperIdx];
    const float* el = g_fkPos[foreIdx];
    const float* wr = g_fkPos[handIdx];
    // Mix the free combat swivel with the existing relaxed elbow policy.
    // Only the swivel changes: the wrist target and both segment lengths stay fixed.
    const float combatBlend=g_VRBind!=4 || ladderReach || !s_referenceValid[foreIdx] || !s_referenceValid[handIdx] ? 0.f :
        VRIK_UsesNativeCyberware() && (g_VRCyberwareFamily==1 || g_VRCyberwareFamily==2) ? 1.f :
        VRIK_UsesNativeCyberware() && g_VRCyberwareFamily==3 ? .5f : .25f;
    const bool combatElbow=combatBlend>0.f;

    float curUp[3]   = { el[0]-sh[0], el[1]-sh[1], el[2]-sh[2] };
    float curFore[3] = { wr[0]-el[0], wr[1]-el[1], wr[2]-el[2] };
    float upLen   = VRIK_Norm3(curUp);
    float foreLen = VRIK_Norm3(curFore);
    if (upLen < 1e-4f || foreLen < 1e-4f) return;

    // STABLE SEGMENT LENGTHS (anti-ratchet). The HAND PIN below writes the hand bone's local
    // translation, and the engine does NOT reset it next frame -- so reading segment lengths from
    // the live FK let the pin cap feed on its own output and ratchet the forearm out to 0.5-0.9m
    // (measured). Use deterministic lengths instead: the native rest lengths cached on the very
    // first solve of the session, or the calibrated per-segment length (userArmLen * 0.5, matching
    // VRIK_ScaleArmBonesFromRest's 50/50 split) when calibration is present.
    static float s_restUpLen[2] = { 0.0f, 0.0f }, s_restForeLen[2] = { 0.0f, 0.0f };
    static uint64_t s_lengthRevision[2]{};
    const int sideJ = isLeft ? 1 : 0;
    if(s_lengthRevision[sideJ]!=s_rigRevision) {
        s_lengthRevision[sideJ]=s_rigRevision;
        s_restUpLen[sideJ]=s_restForeLen[sideJ]=0;
        g_vrikUpArmRestCap[sideJ]=false;
    }
    if (s_restUpLen[sideJ] <= 0.0f) { s_restUpLen[sideJ] = upLen; s_restForeLen[sideJ] = foreLen; }
    {
        const float ual = isLeft ? g_VRUserArmLenL : g_VRUserArmLenR;
        const float calibHalf = (ual >= 0.45f) ? ual * 0.5f : 0.0f;
        upLen   = (calibHalf > 0.0f) ? calibHalf : s_restUpLen[sideJ];
        foreLen = (calibHalf > 0.0f) ? calibHalf : s_restForeLen[sideJ];
    }
    // A hand closed on something: the two-hand hold's support hand, or a wrist the reload has locked to a grip.
    const bool grip=CyberpunkVR_GripReach!=0 &&
        ((isLeft && CyberpunkVR_TwoHandActive!=0) || g_VRHandRotLockOn[isLeft ? 0 : 1]!=0);
    g_VRIKGrip[sideJ]=grip;
    const bool girdle=VRIK_ReachGirdle(boneBuf,upperIdx,targetModel,bodyUp,bodyFwd,upLen+foreLen,isLeft,ladderReach,grip);
    if(girdle) {
        for(int k=0;k<3;++k) { curUp[k]=g_fkPos[foreIdx][k]-g_fkPos[upperIdx][k];curFore[k]=g_fkPos[handIdx][k]-g_fkPos[foreIdx][k]; }
        VRIK_Norm3(curUp);VRIK_Norm3(curFore);
    }
    // Legacy anchor modes retain their old protraction. The calibrated body
    // anchor above distributes the reach through the girdle instead.
    // SHOULDER PROTRACTION (VRArmIK ShoulderPoser-lite). Reaching far FORWARD a real
    // shoulder slides several cm forward (scapula protracts); the avatar's fixed shoulder
    // made forward reaches land ~5cm short, so the hand pin stretched the forearm
    // rubber-style. Slide the upper-arm root toward the reach (mostly
    // forward, a touch out), written ABSOLUTE against the captured rest local translation
    // (the engine keeps our writes; incremental would creep).
    float shP[3] = { sh[0], sh[1], sh[2] };
    {
        const int par = g_VRBoneParent[upperIdx];
        if (!girdle && par >= 0 && par < upperIdx) {
            float* tl = reinterpret_cast<float*>(boneBuf + upperIdx * 48 + VRIK_TRANS_OFF);
            if (!g_vrikUpArmRestCap[sideJ]) {
                g_vrikUpArmRest[sideJ][0]=tl[0]; g_vrikUpArmRest[sideJ][1]=tl[1]; g_vrikUpArmRest[sideJ][2]=tl[2];
                g_vrikUpArmRestCap[sideJ] = true;
            }
            // Clean base shoulder = parent FK * rest local (undo last frame's write, which
            // is already baked into g_fkPos[upperIdx]).
            float base[3]; VRIK_QuatRotateVec(g_fkRot[par], g_vrikUpArmRest[sideJ], base);
            base[0] += g_fkPos[par][0]; base[1] += g_fkPos[par][1]; base[2] += g_fkPos[par][2];
            float toT[3] = { targetModel[0]-base[0], targetModel[1]-base[1], targetModel[2]-base[2] };
            const float dist = std::sqrt(toT[0]*toT[0] + toT[1]*toT[1] + toT[2]*toT[2]);
            const float armL0 = upLen + foreLen;
            if (dist > 1e-4f && armL0 > 1e-4f) {
                float fwdness = (toT[0]*bodyFwd[0] + toT[1]*bodyFwd[1] + toT[2]*bodyFwd[2]) / dist;
                if (fwdness < 0.0f) fwdness = 0.0f;
                float need = (dist / armL0 - 0.90f) * (1.0f / 0.12f);
                if (need < 0.0f) need = 0.0f; if (need > 1.0f) need = 1.0f;
                const float pr = need * fwdness;
                const float outS = isLeft ? 1.0f : -1.0f;   // own-side outward = -sideSign*right
                float delta[3] = {
                    (bodyFwd[0]*0.06f - outS*bodyRight[0]*0.015f) * pr,
                    (bodyFwd[1]*0.06f - outS*bodyRight[1]*0.015f) * pr,
                    (bodyFwd[2]*0.06f - outS*bodyRight[2]*0.015f) * pr,
                };
                shP[0] = base[0] + delta[0]; shP[1] = base[1] + delta[1]; shP[2] = base[2] + delta[2];
                float pc[4] = { -g_fkRot[par][0], -g_fkRot[par][1], -g_fkRot[par][2], g_fkRot[par][3] };
                float ld[3]; VRIK_QuatRotateVec(pc, delta, ld);
                tl[0] = g_vrikUpArmRest[sideJ][0] + ld[0];
                tl[1] = g_vrikUpArmRest[sideJ][1] + ld[1];
                tl[2] = g_vrikUpArmRest[sideJ][2] + ld[2];
            }
        }
    }

    // Hand -> shoulder (F4VR convention; xDir is along this).
    float handToShoulder[3] = { shP[0]-targetModel[0], shP[1]-targetModel[1], shP[2]-targetModel[2] };
    float hsLen = std::sqrt(handToShoulder[0]*handToShoulder[0]
                          + handToShoulder[1]*handToShoulder[1]
                          + handToShoulder[2]*handToShoulder[2]);
    if (hsLen < 1e-4f) return; // hand on top of shoulder; no defined direction
    float xDir[3] = { handToShoulder[0]/hsLen, handToShoulder[1]/hsLen, handToShoulder[2]/hsLen };

    // STRETCH instead of clamp (per F4VR). If the player reaches farther than the rest-pose arm
    // length, we keep the arm STRAIGHT by lengthening upLen+foreLen proportionally to cover hsLen.
    // The cosine law then computes a wrist angle of 0 (cos=1, sin=0) -> elbow lies on the
    // hand-shoulder line -> arm is perfectly straight. NOT clamping with an epsilon is the fix
    // for an arm that could not straighten downwards — that bug came from forcing hsLen < upLen+foreLen.
    float upL = upLen, foreL = foreLen;
    if (hsLen > upL + foreL) {
        float diff = hsLen - upL - foreL;
        float ratio = foreL / (foreL + upL);
        foreL += ratio * diff;
        upL   += (1.0f - ratio) * diff;
    }
    // Hand far closer than |up-fore|: math has no solution; equalise to keep solver stable.
    if (hsLen < std::fabs(upL - foreL) + 1e-3f) {
        float avg = (upL + foreL) * 0.5f;
        upL = foreL = avg;
    }

    // EXTENSION SNAP. With arms hanging at the sides the real shoulder->controller distance
    // comes up a few cm short of the calibrated arm length (shoulder drop/adduction, palm-
    // center grip), so the cosine law held a permanent 10-15deg micro-bend (the arm never
    // straightened at the sides). Within the last reach percents, ease the segment lengths toward exactly
    // the distance -> the arm straightens fully; max shrink a few cm, visually invisible.
    {
        const float sum0 = upL + foreL;
        if (sum0 > 1e-4f && hsLen < sum0) {
            const float r = hsLen / sum0;
            float s = (r - 0.90f) * (1.0f / 0.06f);
            if (s > 1.0f) s = 1.0f;
            if (s > 0.0f) {
                const float f = 1.0f + (r - 1.0f) * s;
                upL *= f; foreL *= f;
            }
        }
    }

    // ANATOMICAL TWIST RIG CONSTANTS (captured once per side, pose-invariant; used by the
    // geometric bend below AND the twist basis further down). The bone-local segment axis
    // conj(rot)*worldAxis and the bone-local ELBOW-HINGE axis conj(rot)*worldHinge are rigid
    // rig data. Sampled from the native pose whenever the elbow is visibly bent
    // (|upDir x foreDir| > ~10deg), hinge sign = upDir x foreDir (anatomically positive).
    // s_palmLoc = hand-bone-local PALM/finger axis (forearm continuation at capture): rotating
    // the wrist swings handRot*s_palmLoc, which feeds the FinalIK-style bend vector so
    // pronation/supination pulls the elbow naturally.
    static float s_axLocUp[2][3], s_hgLocUp[2][3], s_axLocFore[2][3], s_hgLocFore[2][3];
    static float s_palmLoc[2][3];
    static float s_armRest[2][3], s_bendRest[2][3];  // native arm axis + elbow bend dir (model)
    static bool  s_rigCap[2] = { false, false };
    static uint64_t s_axisRevision[2]{};
    const int sideI = isLeft ? 1 : 0;
    if(s_axisRevision[sideI]!=s_rigRevision) {
        s_axisRevision[sideI]=s_rigRevision;s_rigCap[sideI]=false;
    }
    if (!s_rigCap[sideI]) {
        float cx[3]; VRIK_Cross3(curUp, curFore, cx);
        float cl = std::sqrt(cx[0]*cx[0] + cx[1]*cx[1] + cx[2]*cx[2]);
        if (cl > 0.17f) {
            cx[0]/=cl; cx[1]/=cl; cx[2]/=cl;
            const float* ur = g_fkRot[upperIdx];
            const float* fr = g_fkRot[foreIdx];
            const float* hr = g_fkRot[handIdx];
            float uc[4] = { -ur[0], -ur[1], -ur[2], ur[3] };
            float fc[4] = { -fr[0], -fr[1], -fr[2], fr[3] };
            float hc[4] = { -hr[0], -hr[1], -hr[2], hr[3] };
            VRIK_QuatRotateVec(uc, curUp,   s_axLocUp[sideI]);
            VRIK_QuatRotateVec(uc, cx,      s_hgLocUp[sideI]);
            VRIK_QuatRotateVec(fc, curFore, s_axLocFore[sideI]);
            VRIK_QuatRotateVec(fc, cx,      s_hgLocFore[sideI]);
            VRIK_QuatRotateVec(hc, curFore, s_palmLoc[sideI]);
            // Anatomical NEUTRAL for the VRArmIK swivel model: the native animation's arm
            // axis + the direction the ELBOW POKES. Convention check (triangle S-E-W): the
            // solver's yDir is the direction the elbow is DISPLACED from the shoulder-hand
            // line; the forearm's perpendicular component points the OPPOSITE way (from the
            // elbow back across the line). So store the NEGATED perpendicular -- capturing
            // it un-negated bent every elbow backwards.
            s_armRest[sideI][0] = curUp[0]; s_armRest[sideI][1] = curUp[1]; s_armRest[sideI][2] = curUp[2];
            const float dUF = VRIK_Dot3(curFore, curUp);
            float bnd[3] = { curUp[0]*dUF - curFore[0], curUp[1]*dUF - curFore[1], curUp[2]*dUF - curFore[2] };
            if (VRIK_Norm3(bnd) > 1e-3f) {
                s_bendRest[sideI][0]=bnd[0]; s_bendRest[sideI][1]=bnd[1]; s_bendRest[sideI][2]=bnd[2];
                s_rigCap[sideI] = true;
            }
        }
    }

    // yDir: anatomical elbow bend direction (perpendicular to xDir = hand->shoulder axis).
    //
    // Anatomical reality: a human elbow NEVER bends forward. The bend always has a back+down
    // component relative to the body, never forward. So we BLEND down + back (NOT either-or),
    // and explicitly clamp out any forward component before normalising.
    //
    // The cross-body swing (when hand reaches across midline) blends yDir toward the own-side
    // OUT-direction; that does NOT introduce forward bend either because we re-project after.
    float sideSign = isLeft ? 1.0f : -1.0f;
    float downRef[3] = { -bodyUp[0],            -bodyUp[1],            -bodyUp[2]            };
    float backRef[3] = { -bodyFwd[0],           -bodyFwd[1],           -bodyFwd[2]           };
    float outRef[3]  = { -sideSign*bodyRight[0],-sideSign*bodyRight[1],-sideSign*bodyRight[2] };

    auto projectPerp = [](const float* v, const float* axis, float* out) {
        float d = v[0]*axis[0] + v[1]*axis[1] + v[2]*axis[2];
        out[0] = v[0] - axis[0]*d;
        out[1] = v[1] - axis[1]*d;
        out[2] = v[2] - axis[2]*d;
    };
    auto stripForwardBend = [&](float* dir) {
        if(combatBlend>=1.f)return;
        float fc = VRIK_Dot3(dir, bodyFwd);
        if (fc > 0.0f) {
            dir[0] -= bodyFwd[0] * fc;
            dir[1] -= bodyFwd[1] * fc;
            dir[2] -= bodyFwd[2] * fc;
        }
        projectPerp(dir, xDir, dir);
    };
    float oPerp[3];
    projectPerp(outRef,  xDir, oPerp);
    (void)downRef; (void)backRef;

    // FinalIK-STYLE GEOMETRIC BEND (VRIK ArmSolver.GetBendNormal — the formula VRChat-class
    // products ship). Replaces the manual down+back+out blend: the bend vector is DERIVED from
    // (a) the target direction in chest space, (b) the current upper-arm axis, and (c) the hand
    // PALM axis (wrist pronation/supination pulls the elbow, FinalIK's `b -= handRot*palmAxis`).
    // Neutral rest: elbow back+down; forward reach: elbow down; high reach: elbow swings out —
    // all emergent, no per-zone constants. Chest coords (right,fwd,up), X mirrored for LEFT.
    // Every safety net below (anti-forward clamp, degenerate fallback, cross-body swing, pole
    // slider trim, straight-arm fade) still applies on top.
    float yDir[3];
    bool yFromVRArmIK = false;
    // VRArmIK SWIVEL MODEL (Parger et al., zone constants re-tuned below).
    // Zero-reference = a SYNTHETIC SYMMETRIC NEUTRAL swung onto the live target direction.
    // (v1 captured the neutral from the native animation per side -- but the two arms
    // captured in DIFFERENT animation frames, so the neutrals differed and mirrored inputs
    // produced visibly different elbows: the arms out of step. The analytic neutral is mirror-
    // identical by construction.) Relaxed arm: hangs mostly down, slightly forward; elbow
    // pokes back, slightly out, slightly down. Deviation = zone model, a PURE function of
    // the normalized hand position in shoulder space; all safety clamps below still apply.
    {
        float dirN0[3] = { -xDir[0], -xDir[1], -xDir[2] };
        float armRest[3] = { -bodyUp[0] + 0.15f*bodyFwd[0] + 0.10f*outRef[0],
                             -bodyUp[1] + 0.15f*bodyFwd[1] + 0.10f*outRef[1],
                             -bodyUp[2] + 0.15f*bodyFwd[2] + 0.10f*outRef[2] };
        VRIK_Norm3(armRest);
        float bendRest[3] = { -0.90f*bodyFwd[0] + 0.35f*outRef[0] - 0.25f*bodyUp[0],
                              -0.90f*bodyFwd[1] + 0.35f*outRef[1] - 0.25f*bodyUp[1],
                              -0.90f*bodyFwd[2] + 0.35f*outRef[2] - 0.25f*bodyUp[2] };
        VRIK_Norm3(bendRest);
        float qsw[4]; VRIK_QuatFromTo(armRest, dirN0, qsw);
        float ref[3]; VRIK_QuatRotateVec(qsw, bendRest, ref);
        float refP[3]; projectPerp(ref, xDir, refP);
        if (VRIK_Norm3(refP) > 0.2f) {
            const float axLen = upLen + foreLen;
            float v[3] = { -handToShoulder[0], -handToShoulder[1], -handToShoulder[2] };
            const float nx = VRIK_Dot3(v, bodyRight) / axLen;
            const float ny = VRIK_Dot3(v, bodyUp)    / axLen;
            const float nz = VRIK_Dot3(v, bodyFwd)   / axLen;
            const float inward = nx * (isLeft ? 1.0f : -1.0f);   // + when crossing midline
            float dev = -60.0f * ny;
            float zGate = 0.6f - nz; if (zGate < 0.0f) zGate = 0.0f;
            // zWeightTop reduced 260 -> 70: at face height the stock top-zone flared the
            // elbow far outward (high shoulder strain). 70 UNDER-cancels the -60*y term
            // there -> the elbow eases slightly DOWN from neutral, only a touch out.
            dev += (ny > 0.0f) ? (70.0f * zGate * ny) : (-100.0f * zGate * (-ny));
            float xT = inward + 0.1f; if (xT < 0.0f) xT = 0.0f;
            dev += -50.0f * xT;
            if (dev >  25.0f) dev =  25.0f;   // asymmetric cap: outward/up flare is the risky side
            if (dev < -60.0f) dev = -60.0f;
            const float a = dev * 0.01745329252f * (isLeft ? 1.0f : -1.0f);
            float cxv[3]; VRIK_Cross3(xDir, refP, cxv);
            const float ca = std::cos(a), sa = std::sin(a);
            yDir[0] = refP[0]*ca + cxv[0]*sa;
            yDir[1] = refP[1]*ca + cxv[1]*sa;
            yDir[2] = refP[2]*ca + cxv[2]*sa;
            yFromVRArmIK = true;
        }
    }
    if (!yFromVRArmIK) {
        const float mir = isLeft ? -1.0f : 1.0f;
        float dirN[3] = { -xDir[0], -xDir[1], -xDir[2] };        // shoulder -> hand (unit)
        auto toChest = [&](const float* v, float* o) {
            o[0] = VRIK_Dot3(v, bodyRight) * mir;
            o[1] = VRIK_Dot3(v, bodyFwd);
            o[2] = VRIK_Dot3(v, bodyUp);
        };
        auto fromChest = [&](const float* c, float* o) {
            o[0] = bodyRight[0]*c[0]*mir + bodyFwd[0]*c[1] + bodyUp[0]*c[2];
            o[1] = bodyRight[1]*c[0]*mir + bodyFwd[1]*c[1] + bodyUp[1]*c[2];
            o[2] = bodyRight[2]*c[0]*mir + bodyFwd[2]*c[1] + bodyUp[2]*c[2];
        };
        float dC[3]; toChest(dirN, dC);
        // b = FromTo(down, dir+fwd) * back   (chest space)
        float f0[3] = { 0.0f, 0.0f, -1.0f };
        float t0[3] = { dC[0], dC[1] + 1.0f, dC[2] };
        if (VRIK_Norm3(t0) < 1e-3f) { t0[0]=dC[0]; t0[1]=dC[1]; t0[2]=dC[2]; }
        float q0[4]; VRIK_QuatFromTo(f0, t0, q0);
        float back0[3] = { 0.0f, -1.0f, 0.0f };
        float bC[3]; VRIK_QuatRotateVec(q0, back0, bC);
        float b[3]; fromChest(bC, b);
        // + target direction (NOT the live arm axis: feeding the CURRENT pose back in made
        // the bend depend on our own previous solve -> two attractors -> elbow TELEPORT).
        // b is now a PURE function of the target position: continuous, deterministic.
        b[0] += dirN[0]; b[1] += dirN[1]; b[2] += dirN[2];
        // Palm/wrist term DISABLED (rotating the wrist must not move the elbow).
        // Kept for future tuning: kPalmW ~0.3 would give FinalIK-style pronation coupling.
        const float kPalmW = 0.0f;
        if (kPalmW > 0.0f && s_rigCap[sideI]) {
            float pw[3]; VRIK_QuatRotateVec(handModelRot, s_palmLoc[sideI], pw);
            b[0] -= pw[0]*kPalmW; b[1] -= pw[1]*kPalmW; b[2] -= pw[2]*kPalmW;
        }
        // Elbow direction = b rejected onto the plane perpendicular to the arm axis
        // (cross(dir, cross(b, dir)) collapses to exactly this).
        projectPerp(b, xDir, yDir);
    }

    // CRITICAL anti-forward-bend clamp: project out any +bodyFwd component. The elbow MUST
    // never point in the forward direction; if the blend ended up with a forward component
    // (e.g. axis is mostly down and dPerp picked up forward), strip it here.
    stripForwardBend(yDir);

    if (VRIK_Norm3(yDir) < 0.3f) {
        // The blend degenerated (axis parallel to both bodyUp and bodyFwd, which is impossible
        // for a real body). Fall back to own-side outward.
        if (VRIK_Norm3(oPerp) > 0.3f) {
            yDir[0]=oPerp[0]; yDir[1]=oPerp[1]; yDir[2]=oPerp[2];
        } else {
            float fb[3] = { 0.0f, 0.0f, 1.0f };
            if (std::fabs(xDir[2]) > 0.9f) { fb[0]=1.0f; fb[2]=0.0f; }
            projectPerp(fb, xDir, yDir);
            VRIK_Norm3(yDir);
        }
    }

    // Cross-body swing: if the hand reaches across the body's midline, blend yDir toward the
    // own-side outward direction so the elbow follows out (anatomically correct). Fade off as
    // the arm approaches full extension (reach01 -> 1) — at straight extension yDir doesn't
    // matter, but if you keep blending you'll see a sideways elbow twitch right at the limit.
    float armLen = upL + foreL;
    float reach01 = (armLen > 1e-4f) ? (hsLen / armLen) : 0.0f;
    if (reach01 > 1.0f) reach01 = 1.0f;
    float straightFade = (reach01 - 0.92f) * (1.0f / 0.08f);
    if (straightFade < 0.0f) straightFade = 0.0f;
    if (straightFade > 1.0f) straightFade = 1.0f;
    float poleWeight = 1.0f - straightFade;

    float shoulderToHand[3] = { -handToShoulder[0], -handToShoulder[1], -handToShoulder[2] };
    float lateral = (shoulderToHand[0]*bodyRight[0]
                   + shoulderToHand[1]*bodyRight[1]
                   + shoulderToHand[2]*bodyRight[2]) * sideSign;
    float crossAmount = (lateral > 0.0f) ? (lateral / armLen) : 0.0f;
    if (VRIK_Norm3(oPerp) > 1e-3f && poleWeight > 0.0f) {
        float bendFactor = (0.90f - reach01) * (1.0f / 0.35f);
        if (bendFactor < 0.0f) bendFactor = 0.0f; if (bendFactor > 1.0f) bendFactor = 1.0f;
        float crossGate = (crossAmount - 0.05f) * (1.0f / 0.10f);
        if (crossGate < 0.0f) crossGate = 0.0f; if (crossGate > 1.0f) crossGate = 1.0f;
        // HEIGHT GATE: the cross-body swing is a CHEST-level aid. Raised to face level it
        // flared the elbow far out (strained pose, humans keep the elbow down there) --
        // fade the swing off as the hand rises above the shoulder line.
        float upAmt = (shoulderToHand[0]*bodyUp[0] + shoulderToHand[1]*bodyUp[1]
                     + shoulderToHand[2]*bodyUp[2]) / armLen;
        float hGate = 1.0f - (upAmt - 0.05f) * (1.0f / 0.30f);
        if (hGate < 0.0f) hGate = 0.0f; if (hGate > 1.0f) hGate = 1.0f;
        float f = swingGain * crossGate * bendFactor * poleWeight * hGate;
        if (f < 0.0f) f = 0.0f; if (f > 1.0f) f = 1.0f;
        yDir[0] = (1.0f-f)*yDir[0] + f*oPerp[0];
        yDir[1] = (1.0f-f)*yDir[1] + f*oPerp[1];
        yDir[2] = (1.0f-f)*yDir[2] + f*oPerp[2];
    }
    // Final anti-forward-bend clamp after blending. Elbow must never point forward.
    stripForwardBend(yDir);
    VRIK_Norm3(yDir);

    // Optional swivel (Elbow pole slider): rotate yDir around xDir by poleAngleRad. Faded by
    // poleWeight so the spin can't yank the elbow near full extension.
    {
        float swivelRad = poleAngleRad * poleWeight;
        if (std::fabs(swivelRad) > 1e-5f) {
            float c = std::cos(swivelRad), s = std::sin(swivelRad);
            float cr[3]; VRIK_Cross3(xDir, yDir, cr);
            float ad = VRIK_Dot3(xDir, yDir);
            float y2[3] = {
                yDir[0]*c + cr[0]*s + xDir[0]*ad*(1.0f-c),
                yDir[1]*c + cr[1]*s + xDir[1]*ad*(1.0f-c),
                yDir[2]*c + cr[2]*s + xDir[2]*ad*(1.0f-c),
            };
            yDir[0]=y2[0]; yDir[1]=y2[1]; yDir[2]=y2[2];
            VRIK_Norm3(yDir);
        }
    }
    stripForwardBend(yDir);
    if (VRIK_Norm3(yDir) < 1e-3f) {
        projectPerp(backRef, xDir, yDir);
        if (VRIK_Norm3(yDir) < 1e-3f) {
            projectPerp(outRef, xDir, yDir);
            VRIK_Norm3(yDir);
        }
    }

    // NEAR-EXTENSION RELAX: as the arm approaches full stretch, ease the residual bend
    // direction to anatomical DOWN(+slightly back). A nearly-straight arm hangs its tiny
    // elbow sag downward (T-pose!); holding the zone-model direction there read as
    // an arm that would not straighten -- the elbow poked sideways/up at 97% extension.
    {
        float rf = (reach01 - 0.88f) * (1.0f / 0.10f);
        if (rf > 1.0f) rf = 1.0f;
        if (rf > 0.0f) {
            float dn[3] = { -bodyUp[0] - 0.3f*bodyFwd[0],
                            -bodyUp[1] - 0.3f*bodyFwd[1],
                            -bodyUp[2] - 0.3f*bodyFwd[2] };
            float dnP[3]; projectPerp(dn, xDir, dnP);
            if (VRIK_Norm3(dnP) > 1e-3f) {
                yDir[0] = yDir[0]*(1.0f-rf) + dnP[0]*rf;
                yDir[1] = yDir[1]*(1.0f-rf) + dnP[1]*rf;
                yDir[2] = yDir[2]*(1.0f-rf) + dnP[2]*rf;
                if (VRIK_Norm3(yDir) < 1e-3f) { yDir[0]=dnP[0]; yDir[1]=dnP[1]; yDir[2]=dnP[2]; }
            }
        }
    }

    // F4VR formula: wristAngle = acos((fore^2 + hs^2 - up^2) / (2*fore*hs)). This is the angle of
    // the triangle measured AT THE HAND, not the shoulder. Elbow position is then offset from the
    // hand by (cos*fore) along xDir + (sin*fore) along yDir.
    float cosWrist = (foreL*foreL + hsLen*hsLen - upL*upL) / (2.0f * foreL * hsLen);
    if (cosWrist < -1.0f) cosWrist = -1.0f; if (cosWrist > 1.0f) cosWrist = 1.0f;
    float sinWrist = std::sqrt(std::fmax(0.0f, 1.0f - cosWrist*cosWrist));

    float xDist = cosWrist * foreL;
    float yDist = sinWrist * foreL;

    auto relaxedElbowPolicy = [&](float* bend) {
        if(!CyberpunkVR_VrikElbowPolicy || yDist<=1e-4f)return;
        const float armLenTot = upL + foreL;
        if (armLenTot > 1e-3f) {
            float w = 1.0f - hsLen / (0.5f * armLenTot);
            if (w < 0.0f) w = 0.0f;
            if (w > 1.0f) w = 1.0f;
            w *= 0.75f;
            if (w > 0.0f) {
                const float rest[3] = {
                    0.30f * outRef[0] - bodyUp[0] - 2.0f * bodyFwd[0],
                    0.30f * outRef[1] - bodyUp[1] - 2.0f * bodyFwd[1],
                    0.30f * outRef[2] - bodyUp[2] - 2.0f * bodyFwd[2] };
                float rp[3]; projectPerp(rest, xDir, rp);
                if (VRIK_Norm3(rp) > 1e-3f) {
                    bend[0] = bend[0]*(1.0f-w) + rp[0]*w;
                    bend[1] = bend[1]*(1.0f-w) + rp[1]*w;
                    bend[2] = bend[2]*(1.0f-w) + rp[2]*w;
                    projectPerp(bend, xDir, bend);
                    if (VRIK_Norm3(bend) < 1e-3f) { bend[0]=rp[0]; bend[1]=rp[1]; bend[2]=rp[2]; }
                }
            }
        }
        const float handUp = VRIK_Dot3(targetModel, bodyUp);
        const float shUp   = VRIK_Dot3(shP, bodyUp);
        if (handUp <= shUp + 0.05f) {                       // arm not raised above the shoulder
            const float capUp = (handUp < shUp) ? handUp : shUp;
            float u[3]; projectPerp(bodyUp, xDir, u);
            const float r = VRIK_Norm3(u);                  // normalises u, returns the old length
            if (r > 1e-3f) {
                const float baseUp = handUp + xDist * VRIK_Dot3(xDir, bodyUp);
                float aMax = (capUp - baseUp) / (yDist * r);
                if (aMax < -1.0f) aMax = -1.0f;
                if (aMax <  1.0f) {                         // >= 1 cannot be violated
                    float v[3]; VRIK_Cross3(xDir, u, v); VRIK_Norm3(v);
                    const float a = VRIK_Dot3(bend, u);
                    if (a > aMax) {
                        const float b = VRIK_Dot3(bend, v);
                        float bNew = std::sqrt(std::fmax(0.0f, 1.0f - aMax * aMax));
                        if (b < 0.0f) bNew = -bNew;
                        bend[0] = u[0]*aMax + v[0]*bNew;
                        bend[1] = u[1]*aMax + v[1]*bNew;
                        bend[2] = u[2]*aMax + v[2]*bNew;
                        VRIK_Norm3(bend);
                    }
                }
            }
        }
    };

    auto separateElbow = [&](float* bend) {
        float oP[3]; projectPerp(outRef, xDir, oP);
        if (VRIK_Norm3(oP) > 1e-3f) {
            const float minOut = 0.12f;                // ~sin(7 deg)
            const float dOut = VRIK_Dot3(bend, oP);
            if (dOut < minOut) {
                bend[0] += oP[0] * (minOut - dOut);
                bend[1] += oP[1] * (minOut - dOut);
                bend[2] += oP[2] * (minOut - dOut);
                projectPerp(bend, xDir, bend);
                if (VRIK_Norm3(bend) < 1e-3f) {
                    bend[0]=oP[0]; bend[1]=oP[1]; bend[2]=oP[2];
                }
            }
        }
    };

    if(combatElbow && s_referenceValid[foreIdx] && s_referenceValid[handIdx]) {
        // A hook needs the elbow beside the fist, not pinned below it. Infer
        // forearm continuation from the wrist's aim (NOT its roll), then choose
        // the point on the shoulder/wrist elbow circle closest to that aim.
        // Guard/uppercut -> elbow down; a sideways fist -> elbow out. The same
        // rule gives Mantis a free cutting plane without moving the wrist target.
        float baseBend[3];std::copy_n(yDir,3,baseBend);
        if(combatBlend<1.f){separateElbow(baseBend);relaxedElbowPolicy(baseBend);}
        float continuation[3],inverseHand[4],local[3],aim[3],guide[3],relaxed[3];
        for(int k=0;k<3;++k)continuation[k]=s_referenceModelPos[handIdx][k]-s_referenceModelPos[foreIdx][k];
        VRIK_Norm3(continuation);VRIK_QuatConj(s_referenceModelRot[handIdx],inverseHand);
        VRIK_QuatRotateVec(inverseHand,continuation,local);VRIK_QuatRotateVec(handModelRot,local,aim);
        for(int k=0;k<3;++k) { guide[k]=-aim[k];relaxed[k]=-bodyUp[k]+.30f*outRef[k]; }
        projectPerp(guide,xDir,guide);projectPerp(relaxed,xDir,relaxed);
        const float leverage=VRIK_Norm3(guide);
        if(VRIK_Norm3(relaxed)<1e-4f)std::copy_n(oPerp,3,relaxed);
        const float weight=std::clamp((leverage-.10f)/.55f,0.f,1.f)*.90f;
        for(int k=0;k<3;++k)yDir[k]=relaxed[k]*(1-weight)+guide[k]*weight;
        if(VRIK_Norm3(yDir)<1e-4f)std::copy_n(relaxed,3,yDir);
        if(combatBlend<1.f) {
            separateElbow(yDir);VRIK_Norm3(baseBend);VRIK_Norm3(yDir);
            float cross[3];VRIK_Cross3(baseBend,yDir,cross);
            const float angle=std::atan2(VRIK_Dot3(cross,xDir),VRIK_Dot3(baseBend,yDir))*combatBlend;
            VRIK_Cross3(xDir,baseBend,cross);
            for(int k=0;k<3;++k)yDir[k]=baseBend[k]*std::cos(angle)+cross[k]*std::sin(angle);
            VRIK_Norm3(yDir);
        }
    }

    // TEMPORAL SMOOTHING (anti-teleport). The bend direction is now a pure function of the
    // target, but crossing workspace zones (chest reach, overhead) can still swing it fast --
    // the elbow visibly TELEPORTED between poses. A real arm re-poses smoothly: exponentially
    // smooth yDir over time (80ms relaxed -> 25ms combat) and interpolate its rate cap, then re-project
    // perpendicular to the arm axis (the cosine-law placement assumes yDir ⊥ xDir).
    {
        static float s_ySm[2][3];
        static bool  s_yInit[2] = { false, false };
        static long long s_yT[2] = { 0, 0 };
        static uint64_t s_yRevision[2]{};
        static float s_yBlend[2]={-1.f,-1.f};
        if(s_yRevision[sideI]!=s_rigRevision || s_yBlend[sideI]!=combatBlend) {
            s_yRevision[sideI]=s_rigRevision;s_yBlend[sideI]=combatBlend;s_yInit[sideI]=false;s_yT[sideI]=0;
        }
        LARGE_INTEGER qn, qf;
        QueryPerformanceCounter(&qn); QueryPerformanceFrequency(&qf);
        float dt = 0.016f;
        if (s_yT[sideI] != 0) {
            dt = static_cast<float>(static_cast<double>(qn.QuadPart - s_yT[sideI]) / static_cast<double>(qf.QuadPart));
            if (dt < 0.0f) dt = 0.0f;
            if (dt > 0.05f) dt = 0.05f;
        }
        s_yT[sideI] = qn.QuadPart;
        if (!s_yInit[sideI]) {
            s_ySm[sideI][0]=yDir[0]; s_ySm[sideI][1]=yDir[1]; s_ySm[sideI][2]=yDir[2];
            s_yInit[sideI] = true;
        } else {
            float alpha = 1.0f - std::exp(-dt / (.08f+(.025f-.08f)*combatBlend));
            float dotp = s_ySm[sideI][0]*yDir[0] + s_ySm[sideI][1]*yDir[1] + s_ySm[sideI][2]*yDir[2];
            if (dotp < -1.0f) dotp = -1.0f;
            if (dotp >  1.0f) dotp =  1.0f;
            const float ang = std::acos(dotp);
            const float maxStep = (9.42477f+(25.13274f-9.42477f)*combatBlend) * dt;
            if (ang > 1e-4f && ang * alpha > maxStep) alpha = maxStep / ang;
            float ns[3] = {
                s_ySm[sideI][0] + (yDir[0]-s_ySm[sideI][0])*alpha,
                s_ySm[sideI][1] + (yDir[1]-s_ySm[sideI][1])*alpha,
                s_ySm[sideI][2] + (yDir[2]-s_ySm[sideI][2])*alpha,
            };
            if (VRIK_Norm3(ns) > 1e-3f) {
                s_ySm[sideI][0]=ns[0]; s_ySm[sideI][1]=ns[1]; s_ySm[sideI][2]=ns[2];
            } else {
                // Exactly antiparallel lerp degenerated: snap (next frames smooth from here).
                s_ySm[sideI][0]=yDir[0]; s_ySm[sideI][1]=yDir[1]; s_ySm[sideI][2]=yDir[2];
            }
        }
        float yP[3]; projectPerp(s_ySm[sideI], xDir, yP);
        if (VRIK_Norm3(yP) > 1e-3f) { yDir[0]=yP[0]; yDir[1]=yP[1]; yDir[2]=yP[2]; }
    }

    separateElbow(yDir);

    if(!combatElbow)relaxedElbowPolicy(yDir);

    float newElbow[3] = {
        targetModel[0] + xDir[0]*xDist + yDir[0]*yDist,
        targetModel[1] + xDir[1]*xDist + yDir[1]*yDist,
        targetModel[2] + xDir[2]*xDist + yDir[2]*yDist,
    };

    float desUp[3] = { newElbow[0]-shP[0], newElbow[1]-shP[1], newElbow[2]-shP[2] };
    VRIK_Norm3(desUp);
    float desFore[3] = { targetModel[0]-newElbow[0], targetModel[1]-newElbow[1], targetModel[2]-newElbow[2] };
    VRIK_Norm3(desFore);

    // (Rig constants captured above, before the geometric bend — see s_rigCap block.)

    // Desired hinge in model space: perpendicular to the solved bend plane (spanned by desUp and
    // desFore), anatomical sign = desUp x desFore. Near full extension the cross degenerates ->
    // fall back to the pole plane (hinge = yDir x desUp... i.e. cross(desUp, -yDir)): the forearm
    // flexes toward -yDir from the shoulder-hand line, matching the capture convention.
    float hDes[3]; VRIK_Cross3(desUp, desFore, hDes);
    {
        float hl = std::sqrt(hDes[0]*hDes[0] + hDes[1]*hDes[1] + hDes[2]*hDes[2]);
        if (hl < 0.05f) {
            float negY[3] = { -yDir[0], -yDir[1], -yDir[2] };
            VRIK_Cross3(desUp, negY, hDes);
            VRIK_Norm3(hDes);
        } else { hDes[0]/=hl; hDes[1]/=hl; hDes[2]/=hl; }
    }

    // Upper arm: swing from the rig reference in the solved clavicle frame.
    // Starting from the animated upper-arm rotation retained weapon-specific
    // bicep roll, moving SHL/deltoid skin even with identical shoulder/hand targets.
    // The anatomical basis is used only as a LIMIT REFERENCE: extract how
    // far the basis solution would twist the segment vs the plain swing, and apply that roll
    // only in EXTREMES -- 25deg dead zone, then eased, capped at 60deg (the biceps should turn
    // only as a last resort).
    float swingSource[4],swingAxis[3];
    std::copy_n(g_fkRot[upperIdx],4,swingSource);std::copy_n(curUp,3,swingAxis);
    const int referenceParent=g_VRBoneParent[upperIdx];
    if(s_referenceValid[upperIdx] && s_referenceValid[foreIdx] &&
       g_VRBoneParent[foreIdx]==upperIdx && referenceParent>=0 && referenceParent<upperIdx) {
        VRIK_QuatMul(g_fkRot[referenceParent],s_referenceLocalRot[upperIdx],swingSource);
        VRIK_QuatNorm(swingSource);
        VRIK_QuatRotateVec(swingSource,s_referenceLocalPos[foreIdx],swingAxis);
        VRIK_Norm3(swingAxis);
    }
    float delta1[4]; VRIK_QuatFromTo(swingAxis, desUp, delta1);
    float swingUp[4]; VRIK_QuatMul(delta1, swingSource, swingUp); VRIK_QuatNorm(swingUp);
    float newUpModel[4] = { swingUp[0], swingUp[1], swingUp[2], swingUp[3] };
    if (s_rigCap[sideI]) {
        float basis[4]; VRIK_QuatAlignTwo(s_axLocUp[sideI], s_hgLocUp[sideI], desUp, hDes, basis);
        float swc[4] = { -swingUp[0], -swingUp[1], -swingUp[2], swingUp[3] };
        float d[4]; VRIK_QuatMul(basis, swc, d); VRIK_QuatNorm(d);
        float tw[4]; VRIK_QuatExtractTwist(d, desUp, tw);
        float proj = tw[0]*desUp[0] + tw[1]*desUp[1] + tw[2]*desUp[2];
        float angS = 2.0f * std::atan2(proj, tw[3]);           // signed twist vs plain swing
        const float kDead = 0.4363f;                           // 25 deg dead zone
        const float kCap  = 1.0472f;                           // 60 deg max applied roll
        float mag = std::fabs(angS);
        float eff = (mag <= kDead) ? 0.0f : std::fmin(mag - kDead, kCap);
        if (eff > 0.0f) {
            float s = (angS >= 0.0f ? 1.0f : -1.0f) * std::sin(eff * 0.5f);
            float aa[4] = { desUp[0]*s, desUp[1]*s, desUp[2]*s, std::cos(eff * 0.5f) };
            VRIK_QuatMul(aa, swingUp, newUpModel); VRIK_QuatNorm(newUpModel);
        }
    }

    int upParent = g_VRBoneParent[upperIdx];
    const float* upParentModel = (upParent >= 0 && upParent < VRIK_MAX_BONES) ? g_fkRot[upParent] : nullptr;
    float identity[4] = { 0,0,0,1 };
    VRIK_WriteLocalRot(boneBuf, upperIdx, upParentModel ? upParentModel : identity, newUpModel);

    // Share pronation with the wrist in the same way for skin and cyberware.
    // A mechanical hand must follow its wrist target independently; rotating
    // the whole forearm to compensate for a disconnected wrist is incorrect.
    float newForeModel[4];
    if (s_rigCap[sideI]) {
        VRIK_QuatAlignTwo(s_axLocFore[sideI], s_hgLocFore[sideI], desFore, hDes, newForeModel);
    } else {
        // Until the hinge can be identified, transport the native forearm.
        float nativeDelta[4];VRIK_QuatFromTo(curUp,desUp,nativeDelta);
        float foreBase[3]; VRIK_QuatRotateVec(nativeDelta, curFore, foreBase);
        float delta2[4]; VRIK_QuatFromTo(foreBase, desFore, delta2);
        float tmp[4]; VRIK_QuatMul(delta2, nativeDelta, tmp);
        VRIK_QuatMul(tmp, g_fkRot[foreIdx], newForeModel); VRIK_QuatNorm(newForeModel);
    }
    if (s_rigCap[sideI]) {
        float inverse[4];VRIK_QuatConj(newForeModel,inverse);
        float relative[4];VRIK_QuatMul(handModelRot,inverse,relative);VRIK_QuatNorm(relative);
        float twist[4];VRIK_QuatExtractTwist(relative,desFore,twist);
        const float projected=twist[0]*desFore[0]+twist[1]*desFore[1]+twist[2]*desFore[2];
        const float angle=2.0f*std::atan2(projected,twist[3]);
        const float applied=std::clamp(angle*.25f,-.7854f,.7854f);
        const float sine=std::sin(applied*.5f);
        const float roll[4]={desFore[0]*sine,desFore[1]*sine,desFore[2]*sine,std::cos(applied*.5f)};
        float rotated[4];VRIK_QuatMul(roll,newForeModel,rotated);
        std::copy_n(rotated,4,newForeModel);VRIK_QuatNorm(newForeModel);
    }
    VRIK_WriteLocalRot(boneBuf, foreIdx, newUpModel, newForeModel);

    // DISTRIBUTED STRETCH (elbow half of the hand pin). Any residual length mismatch used
    // to materialize ONLY in the hand pin -> 100% of the stretch showed on the forearm
    // (rubber-arm look). Write the ELBOW bone's parent-local translation too, so the upper
    // arm carries its share; caps keep it within anatomical looks.
    float elbowW[3] = { newElbow[0], newElbow[1], newElbow[2] };
    {
        float relU[3] = { newElbow[0]-shP[0], newElbow[1]-shP[1], newElbow[2]-shP[2] };
        const float rlU = std::sqrt(relU[0]*relU[0] + relU[1]*relU[1] + relU[2]*relU[2]);
        if (rlU > 1e-4f) {
            float clU = rlU;
            const float loU = upLen * 0.85f, hiU = upLen * 1.15f;
            if (clU < loU) clU = loU; if (clU > hiU) clU = hiU;
            elbowW[0] = shP[0] + relU[0]*(clU/rlU);
            elbowW[1] = shP[1] + relU[1]*(clU/rlU);
            elbowW[2] = shP[2] + relU[2]*(clU/rlU);
            VRIK_WriteLocalPos(boneBuf, foreIdx, shP, newUpModel, elbowW);
        }
    }

    // Hand orientation written local to the new forearm.
    VRIK_WriteLocalRot(boneBuf, handIdx, newForeModel, handModelRot);

    // HAND PIN (hand == target EXACTLY: the hand is the gizmo, the rest adapts).
    // The two-bone solve lands short whenever the avatar arm and the real reach disagree; instead
    // of letting the wrist float off the gizmo, write the hand bone's parent-local TRANSLATION so
    // the wrist sits ON the target. The visual forearm segment stretches/shrinks within anatomical
    // caps (0.7..1.3 of the native segment) -- the F4VR trade: exact hand presence over a few cm
    // of forearm skin stretch.
    {
        // Anchored at the WRITTEN elbow (elbowW), not the ideal one -- the hand's parent
        // model position is wherever the fore bone actually went.
        float rel[3] = { targetModel[0]-elbowW[0], targetModel[1]-elbowW[1], targetModel[2]-elbowW[2] };
        float rl = std::sqrt(rel[0]*rel[0] + rel[1]*rel[1] + rel[2]*rel[2]);
        if (rl > 1e-4f) {
            float cl = rl;
            const float lo = foreLen * 0.7f, hi = foreLen * 1.15f;   // foreLen is rest/calibrated (stable)
            // what the caps leave between the wrist and its target -- the thorax turn's input (VRIK_GripTorso)
            g_VRIKShort[sideJ] = rl - hi;
            CyberpunkVR_DebugGripShortMm[sideJ] = (rl - hi) * 1000.0f;
            if (cl < lo) cl = lo; if (cl > hi) cl = hi;
            float pin[3] = { elbowW[0]+rel[0]*(cl/rl), elbowW[1]+rel[1]*(cl/rl), elbowW[2]+rel[2]*(cl/rl) };
            VRIK_WriteLocalPos(boneBuf, handIdx, elbowW, newForeModel, pin);
        }
    }

    VRIK_UpdateUpperArmDeformation(boneBuf,foreIdx,isLeft);
    VRIK_UpdateForearmDeformation(boneBuf,foreIdx,handIdx,isLeft);

    if (storeDbg) {
        volatile float* L = isLeft ? g_VRIKDbgLocalL : g_VRIKDbgLocal;
        L[0]=lateral; L[1]=crossAmount; L[2]=reach01; L[3]=0.0f;
        volatile float* T = isLeft ? g_VRIKDbgTargetL   : g_VRIKDbgTarget;
        volatile float* S = isLeft ? g_VRIKDbgShoulderL : g_VRIKDbgShoulder;
        volatile float* E = isLeft ? g_VRIKDbgElbowL    : g_VRIKDbgElbow;
        volatile float* N = isLeft ? g_VRIKDbgLensL     : g_VRIKDbgLens;
        T[0]=targetModel[0]; T[1]=targetModel[1]; T[2]=targetModel[2];
        S[0]=shP[0]; S[1]=shP[1]; S[2]=shP[2];
        E[0]=newElbow[0]; E[1]=newElbow[1]; E[2]=newElbow[2];
        N[0]=upL; N[1]=foreL;
    }
}

// Builds the model-space hand target + orientation from the VR controller.
//
// The controller pose in shared memory is HMD-local, exactly like the visible gizmo hands.
// hmdRel rotates it back into the recenter/body frame so the hand target stays in place when
// the player turns their head. The shoulder pivot comes from auto-calibration in the same body
// frame (OpenXR axes: X right, Y up, Z back) instead of from the animated head bone; deriving
// it from the head bone made the wrist target inherit small head-animation arcs.
//
// Math:
//   controller_body         = hmdRel * vrPos
//   hand_from_shoulder_body = controller_body - calibratedShoulderBody
//   mapLocal (game)         = (hfs.x, -hfs.z, hfs.y) * scale
//   outTarget               = shoulderModel + mapLocal + off
// HAND STOP: a hand position resolved by PHYSICS, pushed in from script, in MODEL space.
//
// Why it has to come back through the IK target at all: the drawn hand's position is produced here, by
// VRIK_SolveArm from `target`. Physics cannot move a drawn bone. So a hand that stops at a wall means a
// target that stops at the wall -- whatever holds the collider, the stop returns through this one value.
//
// The collider itself is one of the player's own capsules flipped solver-owned at runtime
// (entPhysicalBodyInterface::SetIsKinematic) and driven by bounded velocity, exactly the way the basketball
// is carried. Authoring it Dynamic in the asset was measured and rejected: a solver-owned collider component
// is not held by its bone binding, and both hand capsules free-fell to Z = -8100.
//
// Index 0 is LEFT, 1 is RIGHT.
volatile float g_VRHandStopModel[2][3] = { {0.0f,0.0f,0.0f}, {0.0f,0.0f,0.0f} };
volatile int   g_VRHandStopValid[2]    = { 0, 0 };
// A driven body always trails its target a little, and echoing that lag into the arm would read as input
// latency rather than as contact. Only a miss LARGER than this counts as being blocked, and only the part
// beyond it is applied -- so unobstructed motion is untouched and a real wall moves the hand by exactly how
// far the body could not go.
volatile float g_VRHandStopDeadband = 0.02f;

// WRIST ROTATION LOCK. While on, the hand's MODEL-space rotation is replaced with this quat right before the arm
// solve -- the same slot the controller's twist normally rides in -- so a glued hand (the reload grip) keeps the
// recorded orientation no matter how the controller rotates. Raw tracking (g_VRHandRawRot below) is published
// BEFORE the override, so scripts keep seeing pure tracking.
volatile float g_VRHandRotLock[2][4] = { {0.0f,0.0f,0.0f,1.0f}, {0.0f,0.0f,0.0f,1.0f} };
volatile int   g_VRHandRotLockOn[2]  = { 0, 0 };

// The target BEFORE any stop is applied, i.e. where tracking alone would put the hand. The driving loop must
// steer the body towards THIS, never towards the clamped result: every palm readout a script can reach is
// published after the clamp, so driving the body at that would close a loop on itself -- the hand would stay
// wherever it stopped even after the wall was gone.
volatile float g_VRHandRawModel[2][3] = { {0.0f,0.0f,0.0f}, {0.0f,0.0f,0.0f} };
volatile int   g_VRHandRawValid[2]    = { 0, 0 };
// The TRACKED hand rotation, published for the same reason: a collision shape oriented by the solved bone
// depends on the previous frame's clamp, which is a feedback loop that shakes the hand in a cluster of capsules.
volatile float g_VRHandRawRot[2][4]   = { {0.0f,0.0f,0.0f,1.0f}, {0.0f,0.0f,0.0f,1.0f} };

void VRIK_ApplyHandStop(int side, float* target, float* handRot) {
    if (side < 0 || side > 1 || !target) return;
    // published unconditionally, so the loop has a target even while no hold is active
    g_VRHandRawModel[side][0] = target[0];
    g_VRHandRawModel[side][1] = target[1];
    g_VRHandRawModel[side][2] = target[2];
    if (handRot) {
        for (int k = 0; k < 4; ++k) g_VRHandRawRot[side][k] = handRot[k];
        // the reload grip's wrist lock: the solve gets the recorded orientation instead of the controller's
        if (g_VRHandRotLockOn[side]) {
            handRot[0] = g_VRHandRotLock[side][0];
            handRot[1] = g_VRHandRotLock[side][1];
            handRot[2] = g_VRHandRotLock[side][2];
            handRot[3] = g_VRHandRotLock[side][3];
        }
    }
    g_VRHandRawValid[side] = 1;
    if (!g_VRHandStopValid[side]) return;
    const float d[3] = { g_VRHandStopModel[side][0] - target[0],
                         g_VRHandStopModel[side][1] - target[1],
                         g_VRHandStopModel[side][2] - target[2] };
    const float len = std::sqrt(d[0]*d[0] + d[1]*d[1] + d[2]*d[2]);
    const float dead = g_VRHandStopDeadband;
    if (len <= dead || len > 1.5f) return;   // 1.5 m: the body is lost, not blocked -- ignore it
    const float k = (len - dead) / len;
    target[0] += d[0] * k;
    target[1] += d[1] * k;
    target[2] += d[2] * k;
}

void VRIK_BuildHandTarget(const float* shoulderModelPos,
                                        const float* calibratedShoulderBody,
                                        const float* hmdRel,
                                        const float* vrPos, const float* vrQuat,
                                        const float* wristCorr,
                                        float scale,
                                        const float* off,
                                        float* outTarget, float* outHandRot) {
    const float s = scale;

    // Normalise hmdRel (producer composes it without renormalising; a non-unit quat would scale
    // the rotated vector by |q|^2 -> hand drifts when head moves).
    float hq[4] = { hmdRel ? hmdRel[0] : 0.0f,
                    hmdRel ? hmdRel[1] : 0.0f,
                    hmdRel ? hmdRel[2] : 0.0f,
                    hmdRel ? hmdRel[3] : 1.0f };
    if ((hq[0]*hq[0] + hq[1]*hq[1] + hq[2]*hq[2] + hq[3]*hq[3]) > 1e-4f) VRIK_QuatNorm(hq);
    else { hq[0]=0; hq[1]=0; hq[2]=0; hq[3]=1; }

    // 1. Controller in body-frame OpenXR (cancel head orientation).
    float controllerBody[3];
    VRIK_QuatRotateVec(hq, vrPos, controllerBody);

    // 2. Shoulder offset from HMD/body origin, calibrated from the same gizmo/controller poses.
    const float shoulderBody[3] = {
        calibratedShoulderBody ? calibratedShoulderBody[0] : 0.0f,
        calibratedShoulderBody ? calibratedShoulderBody[1] : 0.0f,
        calibratedShoulderBody ? calibratedShoulderBody[2] : 0.0f
    };

    // 3. Hand position relative to shoulder, body-frame OpenXR.
    float handFromShoulderBody[3] = {
        controllerBody[0] - shoulderBody[0],
        controllerBody[1] - shoulderBody[1],
        controllerBody[2] - shoulderBody[2]
    };

    // 4. Axis-swap OpenXR -> game body, then scale by the player's arm-length ratio.
    float mapLocal[3] = {
         handFromShoulderBody[0] * s,
        -handFromShoulderBody[2] * s,
         handFromShoulderBody[1] * s
    };

    // 5. Anchor at the shoulder model position.
    outTarget[0] = shoulderModelPos[0] + mapLocal[0] + off[0];
    outTarget[1] = shoulderModelPos[1] + mapLocal[1] + off[1];
    outTarget[2] = shoulderModelPos[2] + mapLocal[2] + off[2];

    // Hand orientation: cancel head rotation, axis-swap, apply per-hand wrist correction.
    float baseQuat[4];
    VRIK_QuatMul(hq, vrQuat, baseQuat);
    VRIK_QuatNorm(baseQuat);
    float mapQuat[4] = { baseQuat[0], -baseQuat[2], baseQuat[1], baseQuat[3] };
    VRIK_QuatMul(mapQuat, wristCorr, outHandRot);
    VRIK_QuatNorm(outHandRot);
}

// IK-CORRECT hand target: the controller's position in MODEL space, head-independent (via
// hmdRel), anchored at the avatar's EYE (head bone horizontal, HMD eye height). Unlike the old
// VRIK_BuildHandTarget it does NOT subtract a calibrated shoulder offset or apply a reach scale
// -- the hand goes EXACTLY where the controller is relative to the head, and the arm IK just
// pivots the (length-calibrated) arm from the shoulder to it. This matches the body-IK convention (hand target =
// controller position) and the baked view (which sits on the head), so the hand lands on the
// real controller instead of an offset-shoulder approximation.
static inline void VRIK_HandTargetModelSpace(const float* eyeAnchor, const float* hmdRel,
                                       const float* vrPos, const float* vrQuat,
                                       const float* wristCorr, const float* off,
                                       float* outTarget, float* outHandRot) {
    float hq[4] = { hmdRel ? hmdRel[0] : 0.0f, hmdRel ? hmdRel[1] : 0.0f,
                    hmdRel ? hmdRel[2] : 0.0f, hmdRel ? hmdRel[3] : 1.0f };
    if ((hq[0]*hq[0]+hq[1]*hq[1]+hq[2]*hq[2]+hq[3]*hq[3]) > 1e-4f) VRIK_QuatNorm(hq);
    else { hq[0]=0; hq[1]=0; hq[2]=0; hq[3]=1; }

    float ctrlBase[3]; VRIK_QuatRotateVec(hq, vrPos, ctrlBase);   // controller in body/base frame
    float mapLocal[3] = { ctrlBase[0], -ctrlBase[2], ctrlBase[1] }; // OpenXR -> game axes
    outTarget[0] = eyeAnchor[0] + mapLocal[0] + (off ? off[0] : 0.0f);
    outTarget[1] = eyeAnchor[1] + mapLocal[1] + (off ? off[1] : 0.0f);
    outTarget[2] = eyeAnchor[2] + mapLocal[2] + (off ? off[2] : 0.0f);

    float baseQuat[4]; VRIK_QuatMul(hq, vrQuat, baseQuat); VRIK_QuatNorm(baseQuat);
    float mapQuat[4] = { baseQuat[0], -baseQuat[2], baseQuat[1], baseQuat[3] };
    VRIK_QuatMul(mapQuat, wristCorr, outHandRot);
    VRIK_QuatNorm(outHandRot);
}

// ---------------------------------------------------------------------------
// GIZMO-EXACT (1:1) full-body anchoring helpers (Phase 1 of the full-body IK rework).
//
// The visible gizmo hands are drawn (init.lua getHandWorldPose) as
//   worldPos = camPos + camQuat * mapLocalPos(rawPos),   mapLocalPos = (x,-z,y)
// i.e. PURELY camera(HMD)-relative. The old IK built its target a different way
// (hmdRel + an animated-head-derived shoulder + a reach scale), so the hand never
// matched the gizmo and the arm anchor drifted with weapon/recoil animation.
//
// These helpers reproduce the gizmo formula in the bone buffer's MODEL space, so
// the hand lands EXACTLY on the gizmo and the shoulder is anchored to the stable
// HMD frame (not the animated spine). That single change fixes: hand!=gizmo, head
// drifting on weapon draw, and the shoulder kick on fire.

// FPP camera (HMD) pose in model space, from the world camera + entity transforms pushed by
// Lua. The bone buffer's model->world transform is the entity transform (entityPos, entityQuat),
// so world->model rotation = conjugate(entityQuat). Using the FULL entity quaternion (not a
// Rz(-yaw) guess) is what makes this exact and convention-proof -- the hand then renders exactly
// on the gizmo.
bool VRIK_ComputeCamModel(float* outPos, float* outRot, float* outEntityQuat,
                          float* outPairedRot, float* outBodyBase,VrikCameraStatus* status) {
    if(status)*status=VrikCameraStatus::Unavailable;
    const bool componentPair=CyberpunkVR_VrikNativeFramePair && CyberpunkVR_CamWriteInPatch &&
        CyberpunkVR_CamComposeAtWrite && CyberpunkVR_HeadTranslationInPatch;
    // PHASE-COHERENT MODEL TRANSFORM.
    //
    // The animation solve runs before LocateCamera for the frame it is building.  Consequently
    // g_lastLocate*(N-1) paired with CyberpunkVR_PlayerEntity*(N) was never a same-frame pair:
    //   * translation left camera(N-1)-entity(N), i.e. one whole frame of locomotion, in outPos;
    //   * rotation left the head bone one camera sample behind the view.
    // Forward motion mostly hid the positional error behind the eye; backwards motion put the old
    // avatar in front of it, and turns/running made the changing phase visible as judder.
    //
    // Position is a relative transform, so it does not need a current world position.  Consume the
    // camera/entity pair from ONE SetVRPlayerYaw push under its seqlock and convert it with the
    // entity quaternion from THAT push.  Pure world locomotion and yaw cancel before the solve sees
    // them, even if the push itself is a tick old.
    //
    // Orientation does need the current head.  AcquireFrameHeadSample is the latch PatchCamera uses
    // later in this same aim epoch, so composing from it here makes the head bone and rendered view
    // use the identical XR sample.  The fallback remains the coherent pushed relative orientation.
    if (CyberpunkVR_VrikNativeFramePair || CyberpunkVR_VrikTransformsFromPlugin) {
        VrikTransformSnapshot snap{};
        bool haveSnapshot = false;
        // MOUNTED, THE NATIVE PAIR IS THE WRONG FRAME -- and by this function's own stated principle.
        //
        // Its entity orientation is built in BodyYawFollowTick as a pure Rz(yaw) reconstructed from the
        // body-yaw store site. On foot that is exact: the player entity is upright, so yaw is all of it.
        // Seated in a car the body pitches and rolls with the shell, so a yaw-only frame is wrong by a
        // quantity that changes every frame the car moves -- which is the jitter. That same store site is
        // already known not to describe a mounted player: the [vehyaw] census caught it flat at 0.0 deg
        // while the car swung through 260, which is why the camera writer gates ViewYawFromEngine on
        // !g_isInVehicle.
        //
        // The Lua pair carries the FULL entity world quaternion (g_VREntityQ*, "world->model =
        // conjugate(this)") paired coherently with its own camera under one seqlock, so it is preferred
        // while mounted and only while mounted. Not a filter: the wobble is not noise, it is a frame
        // built from the wrong quantity.
        const bool preferFullEntityQuat = g_isInVehicle &&
                                          CyberpunkVR_VrikVehicleFullEntityQuat &&
                                          CyberpunkVR_VrikTransformsFromPlugin;
        if (preferFullEntityQuat) {
            haveSnapshot = VRIK_ReadTransformSnapshot(&snap);
            if (haveSnapshot) {
                CVR_DIAGNOSTIC(std::atomic_ref<uint64_t>(CyberpunkVR_DebugVrikLuaPairFallback)
                    .fetch_add(1u, std::memory_order_relaxed));
            }
            // A missed mounted pair must not switch to the on-foot yaw frame.
            // The caller retains its bounded previous arm solve through a miss.
            if(!haveSnapshot)return false;
        }
        if (!haveSnapshot && componentPair) {
            haveSnapshot = VRIK_ReadNativeTransformSnapshot(&snap);
            if (haveSnapshot) {
                CVR_DIAGNOSTIC(std::atomic_ref<uint64_t>(CyberpunkVR_DebugVrikNativePairUsed)
                    .fetch_add(1u, std::memory_order_relaxed));
            }
        }
        if (!haveSnapshot && CyberpunkVR_VrikTransformsFromPlugin &&
            (!componentPair || g_isInVehicle)) {
            haveSnapshot = VRIK_ReadTransformSnapshot(&snap);
            if (haveSnapshot) {
                CVR_DIAGNOSTIC(std::atomic_ref<uint64_t>(CyberpunkVR_DebugVrikLuaPairFallback)
                    .fetch_add(1u, std::memory_order_relaxed));
            }
        }
        if (haveSnapshot) {
            // An explicit invalidation is different from a transient seqlock miss: the native saw
            // a detached/cinematic camera, so feeding either its >3 m raw span or the last FPP pair
            // into the avatar would teleport the solve into a view it does not own.
            if (!snap.valid) {
                // An expired/unavailable Lua fallback does not revoke the native
                // camera. Its brief absence must not select legacy FK anchoring.
                if(status)*status=snap.unavailable ? VrikCameraStatus::Unavailable : VrikCameraStatus::Invalidated;
                return false;
            }
            float pushedEntQ[4] = { snap.entityQuat[0], snap.entityQuat[1],
                                    snap.entityQuat[2], snap.entityQuat[3] };
            VRIK_QuatNorm(pushedEntQ);
            if (outEntityQuat) {
                for (int i = 0; i < 4; ++i) outEntityQuat[i] = pushedEntQ[i];
            }
            float invPushedEnt[4]; VRIK_QuatConj(pushedEntQ, invPushedEnt);
            VRIK_QuatRotateVec(invPushedEnt, snap.cameraMinusEntity, outPos);
            if (outBodyBase) VRIK_QuatRotateVec(invPushedEnt, snap.bodyCameraMinusEntity, outBodyBase);

            float pushedCamQ[4] = { snap.camQuat[0], snap.camQuat[1],
                                    snap.camQuat[2], snap.camQuat[3] };
            VRIK_QuatNorm(pushedCamQ);
            VRIK_QuatMul(invPushedEnt, pushedCamQ, outRot);
            VRIK_QuatNorm(outRot);
            if (outPairedRot) {
                for (int i = 0; i < 4; ++i) outPairedRot[i] = outRot[i];
            }

            // Mirror PatchCamera's direct-composition gates.  In a menu/shot-frame it deliberately
            // leaves the engine orientation alone; using a fresh HMD here anyway would rotate the
            // skeleton head under a view that did not rotate.  Non-default heading modes fall back
            // to the coherent pushed relative orientation because animation has no pre-write camera
            // quaternion from which PatchCamera derives those modes.
            const bool cameraUsesDirectEngineYaw =
                CyberpunkVR_CamWriteInPatch && CyberpunkVR_CamComposeAtWrite && g_headingValid &&
                CyberpunkVR_HeadingFromPreWrite && CyberpunkVR_ViewYawFromEngine &&
                CyberpunkVR_EngineBodyYawValid && !g_isInVehicle;
            if (cameraUsesDirectEngineYaw) {
                OpenXRHeadPose head{};
                if (OpenXRManager::Get().AcquireFrameHeadSample(&head) && head.valid) {
                    float wz = CyberpunkVR_EngineBodyYawZ;
                    float ww = CyberpunkVR_EngineBodyYawW;
                    if (ww < 0.0f) { wz = -wz; ww = -ww; }
                    if (wz != 0.0f || ww != 0.0f) {
                        // The body quaternion and the view heading below come from this same census
                        // pair.  Do not re-read the four separately-published PlayerEntityQuat floats.
                        float currentEntQ[4] = { 0.0f, 0.0f, wz, ww };
                        VRIK_QuatNorm(currentEntQ);
                        float invCurrentEnt[4]; VRIK_QuatConj(currentEntQ, invCurrentEnt);

                        float viewYaw = BodyTrackingYaw(2.0f * std::atan2(wz, ww));
                        const float yawQ[4] = { 0.0f, 0.0f,
                                                std::sin(viewYaw * 0.5f),
                                                std::cos(viewYaw * 0.5f) };
                        const float pitchQ[4] = { g_headingPitchS, 0.0f, 0.0f, g_headingPitchC };
                        const float headQ[4] = { head.oriX, -head.oriZ, head.oriY, head.oriW };
                        float headingPitch[4]; VRIK_QuatMul(yawQ, pitchQ, headingPitch);
                        float worldCamQ[4]; VRIK_QuatMul(headingPitch, headQ, worldCamQ);
                        VRIK_QuatNorm(worldCamQ);
                        VRIK_QuatMul(invCurrentEnt, worldCamQ, outRot);
                        VRIK_QuatNorm(outRot);
                    }
                }
            }
            if(status)*status=VrikCameraStatus::Valid;
            return true;
        }
        // When this pipeline is enabled, a missing coherent centred pair is not
        // permission to read the unframed legacy scalar mirrors below. Let the
        // caller retain a recent complete solve or release it according to status.
        if (componentPair && !g_isInVehicle) return false;
    }
    if (!g_VRCamPosValid) return false;
    float entQ[4] = { g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR };
    if ((entQ[0]*entQ[0]+entQ[1]*entQ[1]+entQ[2]*entQ[2]+entQ[3]*entQ[3]) < 1e-6f) {
        entQ[0]=0; entQ[1]=0; entQ[2]=0; entQ[3]=1;
    }
    VRIK_QuatNorm(entQ);
    if (outEntityQuat) {
        for (int i = 0; i < 4; ++i) outEntityQuat[i] = entQ[i];
    }
    float invEnt[4]; VRIK_QuatConj(entQ, invEnt);   // world->model rotation
    float camQ[4] = { g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR };
    VRIK_QuatMul(invEnt, camQ, outRot); VRIK_QuatNorm(outRot);
    if (outPairedRot) {
        for (int i = 0; i < 4; ++i) outPairedRot[i] = outRot[i];
    }
    // WELDED BODY ANCHOR (single-filter architecture). g_VRCamPairLocal* is THE one
    // stabilized (cam - entity) offset computed in SetVRTransforms; the rendered view
    // applies the IDENTICAL value (shared [124..127] -> dxgi stabilizer). Same number
    // on both sides => body and view cannot move relative to each other, and bob/kick
    // are filtered out of both simultaneously.
    float d[3];
    if (g_VRCamPairValid) {
        d[0] = g_VRCamPairLocalX;
        d[1] = g_VRCamPairLocalY;
        d[2] = g_VRCamPairLocalZ;
    } else {
        d[0] = g_VRCamPosX - g_VREntityPosX;
        d[1] = g_VRCamPosY - g_VREntityPosY;
        d[2] = g_VRCamPosZ - g_VREntityPosZ;
    }
    VRIK_QuatRotateVec(invEnt, d, outPos);
    if (outBodyBase) for (int i=0;i<3;++i) outBodyBase[i]=outPos[i];
    if(status)*status=VrikCameraStatus::Valid;
    return true;
}

void VRIK_BodyAxesFromCamYaw(const float* camModelRot,
                                           float* bodyRight, float* bodyUp, float* bodyFwd) {
    bodyUp[0] = 0.0f; bodyUp[1] = 0.0f; bodyUp[2] = 1.0f;

    float fwdLocal[3] = { 0.0f, 1.0f, 0.0f }; // OpenXR -Z forward maps to game/model +Y.
    VRIK_QuatRotateVec(camModelRot, fwdLocal, bodyFwd);
    bodyFwd[2] = 0.0f;
    if (VRIK_Norm3(bodyFwd) < 1e-4f) { bodyFwd[0]=0.0f; bodyFwd[1]=1.0f; bodyFwd[2]=0.0f; }

    VRIK_Cross3(bodyFwd, bodyUp, bodyRight);
    if (VRIK_Norm3(bodyRight) < 1e-4f) { bodyRight[0]=1.0f; bodyRight[1]=0.0f; bodyRight[2]=0.0f; }
    VRIK_Cross3(bodyUp, bodyRight, bodyFwd);
    if (VRIK_Norm3(bodyFwd) < 1e-4f) { bodyFwd[0]=0.0f; bodyFwd[1]=1.0f; bodyFwd[2]=0.0f; }
}

// Gizmo-exact hand target + HMD-anchored shoulder, both in model space.
//   target   = camModelPos + camModelRot * mapLocalPos(controllerOpenXR)   (== gizmo)
//   shoulder = camModelPos + camModelRot * mapLocalPos(shoulderOpenXR)      (HMD-anchored)
//   handRot  = camModelRot * mapLocalQuat(controllerQuat) * wristCorr
// shoulderOpenXR is the calibrated HMD-local shoulder offset (OpenXR axes: X right,
// Y up, Z back) -> mapped (x,-z,y) the SAME way as the controller, so the whole arm
// frame is consistent with the gizmo.
static inline void VRIK_BuildHandTargetGizmo(const float* camModelPos, const float* camModelRot,
                                             const float* vrPos, const float* vrQuat,
                                             const float* shoulderOpenXR, const float* wristCorr,
                                             const float* off,
                                             float* outTarget, float* outHandRot,
                                             float* outShoulderModel) {
    float mapPos[3] = { vrPos[0], -vrPos[2], vrPos[1] };
    float rotP[3];  VRIK_QuatRotateVec(camModelRot, mapPos, rotP);
    outTarget[0] = camModelPos[0] + rotP[0] + (off ? off[0] : 0.0f);
    outTarget[1] = camModelPos[1] + rotP[1] + (off ? off[1] : 0.0f);
    outTarget[2] = camModelPos[2] + rotP[2] + (off ? off[2] : 0.0f);

    float mapSh[3] = { shoulderOpenXR[0], -shoulderOpenXR[2], shoulderOpenXR[1] };
    float rotS[3];  VRIK_QuatRotateVec(camModelRot, mapSh, rotS);
    outShoulderModel[0] = camModelPos[0] + rotS[0];
    outShoulderModel[1] = camModelPos[1] + rotS[1];
    outShoulderModel[2] = camModelPos[2] + rotS[2];

    float localQuat[4] = { vrQuat[0], -vrQuat[2], vrQuat[1], vrQuat[3] };
    float hm[4]; VRIK_QuatMul(camModelRot, localQuat, hm);
    VRIK_QuatMul(hm, wristCorr, outHandRot);
    VRIK_QuatNorm(outHandRot);
}

// Arm-length calibration. A bone's parent-local translation IS its segment length, so the
// avatar arm length = |foreArm.translation| (upper-arm) + |hand.translation| (forearm).
// We scale those two translations so the avatar arm length matches the player's T-pose-measured
// arm length: a straight real arm then yields a straight avatar arm at the 1:1 gizmo target
// (fixes "arm down but elbow still bent"). Hand + fingers keep their normal size.
static inline float VRIK_ArmScale(uint8_t* boneBuf, int foreIdx, int handIdx, float userArmLen) {
    // Reject implausible T-pose measurements (real shoulder->wrist is ~0.45..0.85 m). A bad
    // calibration frame must NOT shrink the avatar arm -> fall back to the avatar's own length.
    if (userArmLen < 0.45f || userArmLen > 0.85f || foreIdx < 0 || handIdx < 0
        || foreIdx >= VRIK_MAX_BONES || handIdx >= VRIK_MAX_BONES) return 1.0f;
    auto segLen = [&](int idx) -> float {
        const float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
        return std::sqrt(t[0]*t[0] + t[1]*t[1] + t[2]*t[2]);
    };
    float avatar = segLen(foreIdx) + segLen(handIdx);
    if (avatar < 0.05f) return 1.0f;
    float s = userArmLen / avatar;
    if (s < 0.7f) s = 0.7f; if (s > 1.3f) s = 1.3f;
    return s;
}

static inline void VRIK_ScaleArmBones(uint8_t* boneBuf, int foreIdx, int handIdx, float scale) {
    if (scale <= 0.0f || std::fabs(scale - 1.0f) < 1e-3f) return;
    if (foreIdx >= 0 && foreIdx < VRIK_MAX_BONES) {
        float* t = reinterpret_cast<float*>(boneBuf + foreIdx * 48 + VRIK_TRANS_OFF);
        t[0]*=scale; t[1]*=scale; t[2]*=scale;
    }
    if (handIdx >= 0 && handIdx < VRIK_MAX_BONES) {
        float* t = reinterpret_cast<float*>(boneBuf + handIdx * 48 + VRIK_TRANS_OFF);
        t[0]*=scale; t[1]*=scale; t[2]*=scale;
    }
}

static inline bool VRIK_ArmRestTrans(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount, int idx, float* out) {
    static uintptr_t s_trackBuf = 0;
    static int s_boneCount = 0;
    static bool s_valid[VRIK_MAX_BONES] = {};
    static float s_trans[VRIK_MAX_BONES][3] = {};

    if (!boneBuf || idx < 0 || idx >= VRIK_MAX_BONES || idx >= boneCount) return false;
    if (trackBuf != s_trackBuf || boneCount != s_boneCount) {
        for (int i = 0; i < VRIK_MAX_BONES; ++i) s_valid[i] = false;
        s_trackBuf = trackBuf;
        s_boneCount = boneCount;
    }
    if (!s_valid[idx]) {
        const float* t = reinterpret_cast<float*>(boneBuf + idx * 48 + VRIK_TRANS_OFF);
        s_trans[idx][0] = t[0]; s_trans[idx][1] = t[1]; s_trans[idx][2] = t[2];
        s_valid[idx] = true;
    }
    out[0] = s_trans[idx][0]; out[1] = s_trans[idx][1]; out[2] = s_trans[idx][2];
    return true;
}

void VRIK_ScaleArmBonesFromRest(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount,
                                              int foreIdx, int handIdx, float userArmLen) {
    if (userArmLen < 0.45f || userArmLen > 0.85f) return;
    float foreRest[3], handRest[3];
    if (!VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, foreIdx, foreRest)) return;
    if (!VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, handIdx, handRest)) return;

    float upperLen = std::sqrt(foreRest[0]*foreRest[0] + foreRest[1]*foreRest[1] + foreRest[2]*foreRest[2]);
    float foreLen  = std::sqrt(handRest[0]*handRest[0] + handRest[1]*handRest[1] + handRest[2]*handRest[2]);
    if (upperLen < 0.02f || foreLen < 0.02f) return;

    // Human shoulder->wrist split. The old uniform scale preserved the avatar's bad bicep/forearm
    // ratio; this gives each segment its own calibrated length while preserving its rest direction.
    // Even 50/50 split. The measured real bicep/forearm are near-equal (~33/~35cm); 0.515 forearm
    // read as "forearm too long" in game (amplified by the reach stretch), and the old 0.52 bicep
    // read as bicep too long. Neutral 50/50 avoids both; revisit only if a clear asymmetry shows.
    float targetUpper = userArmLen * 0.50f;
    float targetFore  = userArmLen * 0.50f;
    float upperScale = targetUpper / upperLen;
    float foreScale  = targetFore  / foreLen;
    if (upperScale < 0.65f) upperScale = 0.65f; if (upperScale > 1.45f) upperScale = 1.45f;
    if (foreScale  < 0.65f) foreScale  = 0.65f; if (foreScale  > 1.45f) foreScale  = 1.45f;

    float* foreT = reinterpret_cast<float*>(boneBuf + foreIdx * 48 + VRIK_TRANS_OFF);
    float* handT = reinterpret_cast<float*>(boneBuf + handIdx * 48 + VRIK_TRANS_OFF);
    foreT[0] = foreRest[0] * upperScale; foreT[1] = foreRest[1] * upperScale; foreT[2] = foreRest[2] * upperScale;
    handT[0] = handRest[0] * foreScale;  handT[1] = handRest[1] * foreScale;  handT[2] = handRest[2] * foreScale;
}

// PUT THE ARM'S TRANSLATIONS BACK, for an arm handed to the driving animation (iPowerTech,
// 425d4262). Two of our writes are translations the engine does NOT revert: the segment lengths
// scaled to the player's real arm, and the shoulder protraction. Left in place, the animation's own
// rotations then put the hand BESIDE the wheel instead of on it -- so an arm we stop solving has to
// get the rig's own rest values back. Lives here because VRIK_ArmRestTrans above and the protraction
// rest pair are both this file's.
void VRIK_RestoreArmRestTrans(uint8_t* boneBuf, uintptr_t trackBuf, int boneCount,
                                            int upperIdx, int foreIdx, int handIdx, bool isLeft) {
    float rest[3];
    if (foreIdx >= 0 && foreIdx < VRIK_MAX_BONES
        && VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, foreIdx, rest)) {
        float* t = reinterpret_cast<float*>(boneBuf + foreIdx * 48 + VRIK_TRANS_OFF);
        t[0] = rest[0]; t[1] = rest[1]; t[2] = rest[2];
    }
    if (handIdx >= 0 && handIdx < VRIK_MAX_BONES
        && VRIK_ArmRestTrans(boneBuf, trackBuf, boneCount, handIdx, rest)) {
        float* t = reinterpret_cast<float*>(boneBuf + handIdx * 48 + VRIK_TRANS_OFF);
        t[0] = rest[0]; t[1] = rest[1]; t[2] = rest[2];
    }
    const int side = isLeft ? 1 : 0;
    if (upperIdx >= 0 && upperIdx < VRIK_MAX_BONES && g_vrikUpArmRestCap[side]) {
        float* t = reinterpret_cast<float*>(boneBuf + upperIdx * 48 + VRIK_TRANS_OFF);
        t[0] = g_vrikUpArmRest[side][0];
        t[1] = g_vrikUpArmRest[side][1];
        t[2] = g_vrikUpArmRest[side][2];
    }
}

void VRIK_BeginBodySolve(uint8_t* boneBuf) {
    for(auto& base:s_shoulderBase)base={};
    g_VRIKBodyEyeBound=false;
    g_VRIKVehicleUpperActive=false;
    std::fill_n(s_bendLegOwned,VRIK_MAX_BONES,false);
    g_VRIKBodyBendAngle=0;std::fill_n(g_VRIKBodyBendPelvis,3,0.0f);
    // Some pose passes feed our own output back in. Undo only a complete match
    // of the six joints we last wrote to this buffer; a new native gait wins.
    for(auto& saved:s_bendLegRestore)if(saved.buffer==boneBuf && saved.revision==s_rigRevision) {
        if(saved.vehicle) {
            for(int i=0;i<saved.count;++i)if(SameLocal(boneBuf,saved.bones[i],saved.solved[i]))
                std::memcpy(boneBuf+saved.bones[i]*48,saved.native[i],48);
            break;
        }
        for(int group=0;group<saved.count;group+=6) {
            const int end=std::min(group+6,saved.count);bool ownOutput=true;
            for(int i=group;i<end && ownOutput;++i) {
                const float* local=reinterpret_cast<const float*>(boneBuf+saved.bones[i]*48);
                for(int k=0;k<12;++k)if(!std::isfinite(local[k]) ||
                    std::abs(local[k]-saved.solved[i][k])>1e-6f) { ownOutput=false;break; }
            }
            if(ownOutput)for(int i=group;i<end;++i)std::memcpy(boneBuf+saved.bones[i]*48,saved.native[i],48);
        }
        break;
    }
}

// Generic 2-bone limb IK (hip->knee->foot). Rotation-only writes (no stretch). The knee bends
// toward poleDir (projected perpendicular to the hip->foot axis). Used to keep the feet planted
// on their captured ground targets after the hips move under the HMD.
static inline void VRIK_SolveLeg(uint8_t* boneBuf, int upIdx, int midIdx, int endIdx,
                                 const float* target, const float* poleDir) {
    if (upIdx < 0 || midIdx < 0 || endIdx < 0
        || upIdx >= VRIK_MAX_BONES || midIdx >= VRIK_MAX_BONES || endIdx >= VRIK_MAX_BONES) return;
    const float* hip = g_fkPos[upIdx];
    float curUp[3] = { g_fkPos[midIdx][0]-hip[0], g_fkPos[midIdx][1]-hip[1], g_fkPos[midIdx][2]-hip[2] };
    float curLo[3] = { g_fkPos[endIdx][0]-g_fkPos[midIdx][0], g_fkPos[endIdx][1]-g_fkPos[midIdx][1], g_fkPos[endIdx][2]-g_fkPos[midIdx][2] };
    float upLen = VRIK_Norm3(curUp), loLen = VRIK_Norm3(curLo);
    if (upLen < 1e-4f || loLen < 1e-4f) return;

    float toTarget[3] = { target[0]-hip[0], target[1]-hip[1], target[2]-hip[2] };
    float dist = VRIK_Norm3(toTarget);
    if (dist < 1e-4f) return;
    float maxLen = upLen + loLen;
    dist=std::clamp(dist,std::fabs(upLen-loLen)+1e-6f,maxLen*.999999f);

    float pole[3] = { poleDir[0], poleDir[1], poleDir[2] };
    float pd = VRIK_Dot3(pole, toTarget);
    pole[0]-=toTarget[0]*pd; pole[1]-=toTarget[1]*pd; pole[2]-=toTarget[2]*pd;
    if (VRIK_Norm3(pole) < 1e-3f) { pole[0]=0; pole[1]=0; pole[2]=1; }

    float cosHip = (upLen*upLen + dist*dist - loLen*loLen) / (2.0f*upLen*dist);
    if (cosHip < -1.0f) cosHip = -1.0f; if (cosHip > 1.0f) cosHip = 1.0f;
    float hipAng = std::acos(cosHip);
    float kneePos[3] = {
        hip[0] + toTarget[0]*(std::cos(hipAng)*upLen) + pole[0]*(std::sin(hipAng)*upLen),
        hip[1] + toTarget[1]*(std::cos(hipAng)*upLen) + pole[1]*(std::sin(hipAng)*upLen),
        hip[2] + toTarget[2]*(std::cos(hipAng)*upLen) + pole[2]*(std::sin(hipAng)*upLen),
    };
    float desUp[3] = { kneePos[0]-hip[0], kneePos[1]-hip[1], kneePos[2]-hip[2] }; VRIK_Norm3(desUp);
    float d1[4]; VRIK_QuatFromTo(curUp, desUp, d1);
    float newUp[4]; VRIK_QuatMul(d1, g_fkRot[upIdx], newUp); VRIK_QuatNorm(newUp);
    int up_p = g_VRBoneParent[upIdx]; float id[4] = {0,0,0,1};
    VRIK_WriteLocalRot(boneBuf, upIdx, (up_p>=0 && up_p<VRIK_MAX_BONES)?g_fkRot[up_p]:id, newUp);
    float loBase[3]; VRIK_QuatRotateVec(d1, curLo, loBase);
    float desLo[3] = { target[0]-kneePos[0], target[1]-kneePos[1], target[2]-kneePos[2] }; VRIK_Norm3(desLo);
    float d2[4]; VRIK_QuatFromTo(loBase, desLo, d2);
    float tmp[4]; VRIK_QuatMul(d2, d1, tmp);
    float newLo[4]; VRIK_QuatMul(tmp, g_fkRot[midIdx], newLo); VRIK_QuatNorm(newLo);
    VRIK_WriteLocalRot(boneBuf, midIdx, newUp, newLo);
}

struct VrikTorsoCandidate {
    int bones[12]{},count{};
    float positions[12][3]{},rotations[12][4]{},eye[3]{};
    bool valid{};
};
// Evaluate only the torso path while fitting the pose. Full800-bone FK is
// needed just for the final writes, not for each trial angle.
static VrikTorsoCandidate VRIK_EvaluateTorso(uint8_t* buffer,float flexion,const float* headRotation) {
    VrikTorsoCandidate pose;const int hips=g_VRHipsIdx,head=g_VRHeadBoneIdx;
    pose.bones[pose.count++]=hips;
    const int spineCount=std::clamp(static_cast<int>(g_VRSpineCount),0,8);
    for(int i=0;i<spineCount;++i)pose.bones[pose.count++]=g_VRSpineIdx[i];
    if(g_VRNeckIdx>=0)pose.bones[pose.count++]=g_VRNeckIdx;
    if(g_VRNeck1Idx>=0 && pose.count<11)pose.bones[pose.count++]=g_VRNeck1Idx;
    if(pose.count>=12)return pose;
    pose.bones[pose.count++]=head;
    for(int slot=0;slot<pose.count;++slot) {
        const int bone=pose.bones[slot];
        if(bone<0 || bone>=VRIK_FKCount() || !s_referenceValid[bone])return pose;
        if(slot==0) {
            const float tilt[4]={-std::sin(flexion*.3f),0,0,std::cos(flexion*.3f)};
            VRIK_QuatMul(tilt,s_referenceModelRot[hips],pose.rotations[slot]);continue;
        }
        const int parent=g_VRBoneParent[bone];int parentSlot=-1;
        for(int i=0;i<slot;++i)if(pose.bones[i]==parent)parentSlot=i;
        if(parentSlot<0)return pose;
        VRIK_QuatRotateVec(pose.rotations[parentSlot],reinterpret_cast<const float*>(buffer+bone*48),pose.positions[slot]);
        for(int k=0;k<3;++k)pose.positions[slot][k]+=pose.positions[parentSlot][k];
        if(bone==head) { VRIK_QuatMul(headRotation,g_VRHeadReferenceModelRot,pose.rotations[slot]);continue; }
        float flex=0,share=0,hinge=flexion;
        if(slot<=spineCount) {
            const int i=slot-1;const float t=spineCount>1 ? float(i)/float(spineCount-1):1;
            flex=cvr::vrik::SpineFlex(i,spineCount);hinge=flexion*(.85f+.15f*t);
        }
        if(bone==g_VRNeckIdx) { flex=cvr::vrik::NeckFlex;share=.25f; }
        if(bone==g_VRNeck1Idx) { flex=cvr::vrik::UpperNeckFlex;share=.55f; }
        const float pitch=flex-hinge*(1-share);
        const float bend[4]={std::sin(pitch*.5f),0,0,std::cos(pitch*.5f)};
        float headPart[4],combined[4];VRIK_QuatScale(headRotation,share,headPart);
        VRIK_QuatMul(headPart,bend,combined);VRIK_QuatMul(combined,s_referenceModelRot[bone],pose.rotations[slot]);
    }
    VRIK_QuatRotateVec(pose.rotations[pose.count-1],s_eyeInHead,pose.eye);
    for(int k=0;k<3;++k)pose.eye[k]+=pose.positions[pose.count-1][k];
    pose.valid=true;return pose;
}

static void VRIK_ApplyBodyBend(uint8_t* boneBuf,float angle,const float* headRotation,float squatDrop,
                              const float* headDeltaModel) {
    if(!std::isfinite(angle) || angle<=1e-5f)return;
    angle=std::clamp(angle,0.0f,cvr::body::MaxBend);
    // Keep the floor-reaching range, but do not amplify the onset of a bend.
    const float gain=1+.2f*cvr::body::SmoothRange(angle,.52359878f,1.04719755f);
    const float flexion=std::min(1.30899694f,angle*gain);
    const int hips=g_VRHipsIdx,head=g_VRHeadBoneIdx;
    if(hips<0 || hips>=VRIK_FKCount() || !s_referenceValid[hips])return;
    const int joints[6]={g_VRRightUpLegIdx,g_VRRightLegIdx,g_VRRightFootIdx,
                         g_VRLeftUpLegIdx,g_VRLeftLegIdx,g_VRLeftFootIdx};
    bool legs=true;
    for(int bone:joints)legs=legs && bone>=0 && bone<VRIK_FKCount();
    if(legs)for(int side=0;side<2;++side) {
        const int* j=joints+side*3;
        legs=legs && g_VRBoneParent[j[0]]==hips && g_VRBoneParent[j[1]]==j[0] && g_VRBoneParent[j[2]]==j[1];
    }
    float feet[2][3]{},footRot[2][4]{},length[2][2]{};
    BendLegRestore* saved=nullptr;
    if(legs) {
        for(auto& entry:s_bendLegRestore)if(entry.buffer==boneBuf) { saved=&entry;break; }
        if(!saved)saved=&s_bendLegRestore[s_bendRestoreCursor++%8];
        saved->buffer=boneBuf;saved->revision=s_rigRevision;saved->count=6;saved->vehicle=false;
        for(int i=0;i<6;++i) { saved->bones[i]=joints[i];std::memcpy(saved->native[i],boneBuf+joints[i]*48,48); }
        for(int bone=0;bone<VRIK_FKCount() && saved->count<12;++bone)if(s_bendLegShadowSource[bone]>=0) {
            const int slot=saved->count++;saved->bones[slot]=bone;
            std::memcpy(saved->native[slot],boneBuf+bone*48,48);
        }
        for(int side=0;side<2;++side) {
            const int* j=joints+side*3;
            std::copy_n(g_fkPos[j[2]],3,feet[side]);std::copy_n(g_fkRot[j[2]],4,footRot[side]);
            length[side][0]=VRIK_Dist3(g_fkPos[j[0]],g_fkPos[j[1]]);
            length[side][1]=VRIK_Dist3(g_fkPos[j[1]],g_fkPos[j[2]]);
        }
    }
    const float id[4]={0,0,0,1},zero[3]={};
    const float weight=cvr::body::BendWeight(angle);
    const float initialPelvis[3]={g_fkPos[hips][0],g_fkPos[hips][1],g_fkPos[hips][2]};
    const float preferred[3]={initialPelvis[0],initialPelvis[1]-.045f*weight,initialPelvis[2]+.02f*weight-squatDrop};
    bool headBound=s_eyeReferenceValid && headDeltaModel;
    if(headBound)for(int k=0;k<3;++k)headBound=headBound && std::isfinite(headDeltaModel[k]);
    float eyeTarget[3]{};
    if(headBound)for(int k=0;k<3;++k)eyeTarget[k]=s_neutralEyeModel[k]+headDeltaModel[k]+
        initialPelvis[k]-s_referenceModelPos[hips][k];
    auto fit=[&](const VrikTorsoCandidate& candidate,float* pelvis) {
        if(!candidate.valid)return 1e6f;
        for(int k=0;k<3;++k) {
            pelvis[k]=preferred[k];
            if(headBound)pelvis[k]+=(eyeTarget[k]-candidate.eye[k]-pelvis[k])*weight;
        }
        if(headBound) {
            // Continuous one-sided limits also protect the beginning of a bend,
            // before full anchoring fades in. The body must not get ahead of or
            // above the same HMD that drives the view, exposing the upper back.
            pelvis[1]=std::min(pelvis[1],eyeTarget[1]-candidate.eye[1]+.01f);
            pelvis[2]=std::min(pelvis[2],eyeTarget[2]-candidate.eye[2]+.015f);
        }
        float excess=0;
        if(legs)for(int side=0;side<2;++side) {
            const float u=length[side][0],l=length[side][1];
            const float reach=std::min((u+l)*.999999f,std::sqrt(u*u+l*l+2*u*l*std::cos(.20f*weight)));
            float socket[3];VRIK_QuatRotateVec(candidate.rotations[0],reinterpret_cast<const float*>(boneBuf+joints[side*3]*48),socket);
            for(int k=0;k<3;++k)socket[k]+=pelvis[k];
            excess=std::max(excess,VRIK_Dist3(socket,feet[side])-reach);
        }
        return excess;
    };
    VrikTorsoCandidate selected=VRIK_EvaluateTorso(boneBuf,flexion,headRotation);
    if(!selected.valid)return;
    float pelvis[3],excess=fit(selected,pelvis);
    if(headBound && legs && excess>1e-5f) {
        // Pick the closest feasible angle instead of sinking the entire body
        // below the headset when a stronger requested hinge overextends a leg.
        float previous=flexion,bestExcess=excess;
        for(int step=1;step<=32;++step) {
            const float trialAngle=flexion*(1-float(step)/32);
            auto trial=VRIK_EvaluateTorso(boneBuf,trialAngle,headRotation);float trialPelvis[3];
            const float trialExcess=fit(trial,trialPelvis);
            if(trialExcess<bestExcess) { selected=trial;std::copy_n(trialPelvis,3,pelvis);bestExcess=trialExcess; }
            if(trialExcess<=1e-5f) {
                float low=trialAngle,high=previous;
                selected=trial;std::copy_n(trialPelvis,3,pelvis);
                for(int iteration=0;iteration<10;++iteration) {
                    const float mid=(low+high)*.5f;auto middle=VRIK_EvaluateTorso(boneBuf,mid,headRotation);float at[3];
                    if(fit(middle,at)<=1e-5f) { low=mid;selected=middle;std::copy_n(at,3,pelvis); }
                    else high=mid;
                }
                break;
            }
            previous=trialAngle;
        }
    }
    const int parent=g_VRBoneParent[hips];
    auto placePelvis=[&]() {
        VRIK_WriteLocalPos(boneBuf,hips,parent>=0 ? g_fkPos[parent]:zero,parent>=0 ? g_fkRot[parent]:id,pelvis);
        VRIK_WriteLocalRot(boneBuf,hips,parent>=0 ? g_fkRot[parent]:id,selected.rotations[0]);
        VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    };
    placePelvis();
    if(legs) {
        float lower=0;
        for(int side=0;side<2;++side) {
            const float u=length[side][0],l=length[side][1];
            const float reach=std::min((u+l)*.999999f,std::sqrt(u*u+l*l+2*u*l*std::cos(.20f*weight)));
            const float* hip=g_fkPos[joints[side*3]];
            const float dx=hip[0]-feet[side][0],dy=hip[1]-feet[side][1];
            const float maxZ=feet[side][2]+std::sqrt(std::max(0.0f,reach*reach-dx*dx-dy*dy));
            lower=std::min(lower,maxZ-hip[2]);
        }
        pelvis[2]+=lower;placePelvis();
    }
    for(int slot=1;slot<selected.count;++slot) {
        const int bone=selected.bones[slot],par=g_VRBoneParent[bone];
        VRIK_WriteLocalRot(boneBuf,bone,par>=0 ? g_fkRot[par]:id,selected.rotations[slot]);
        VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    }
    if(headBound) {
        std::copy_n(eyeTarget,3,g_VRIKBodyEyeTarget);
        VRIK_QuatRotateVec(g_fkRot[head],s_eyeInHead,g_VRIKBodyEyeSolved);
        for(int k=0;k<3;++k)g_VRIKBodyEyeSolved[k]+=g_fkPos[head][k];
        g_VRIKBodyEyeBound=true;
    }
    if(legs)for(int side=0;side<2;++side) {
        const int* j=joints+side*3;const float forward[3]={0,1,0};
        VRIK_SolveLeg(boneBuf,j[0],j[1],j[2],feet[side],forward);
        VRIK_ComputeFK(boneBuf,VRIK_FKCount());
        VRIK_WriteLocalRot(boneBuf,j[2],g_fkRot[j[1]],footRot[side]);
        for(int k=0;k<3;++k)s_bendLegOwned[j[k]]=true;
        VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    }
    if(saved)for(int i=0;i<saved->count;++i)std::memcpy(saved->solved[i],boneBuf+saved->bones[i]*48,48);
    g_VRIKBodyBendAngle=angle;
    for(int i=0;i<3;++i)g_VRIKBodyBendPelvis[i]=g_fkPos[hips][i]-initialPelvis[i];
}

// Passenger combat is fitted downward from the HMD centre. A fixed seated
// pelvis plus a low camera target folded the neck below its own base. Move the
// skeleton pelvis to fit the neutral upper posture; keep native foot contacts.
static bool VRIK_PlaceHeadAnchoredBody(uint8_t* boneBuf,const float* cameraTarget,
                                 const float* headRotation,int headIdx,bool vehicle) {
    if(!boneBuf || !cameraTarget || !headRotation || !g_VRHeadReferenceValid || !s_eyeReferenceValid ||
       headIdx<0 || headIdx>=g_VRBoneCount || g_VRSpineCount<=0)return false;
    for(int k=0;k<3;++k)if(!std::isfinite(cameraTarget[k]))return false;
    float headQ[4],norm=0;
    for(int k=0;k<4;++k) { if(!std::isfinite(headRotation[k]))return false;norm+=headRotation[k]*headRotation[k]; }
    if(norm<.5f || norm>1.5f)return false;
    VRIK_QuatMul(headRotation,g_VRHeadReferenceModelRot,headQ);VRIK_QuatNorm(headQ);
    const int hips=g_VRHipsIdx;
    int chain[12],count=0;
    for(int bone=headIdx;bone>=0 && bone<g_VRBoneCount && count<12;bone=g_VRBoneParent[bone]) {
        if(!s_referenceValid[bone])return false;
        chain[count++]=bone;if(bone==hips)break;
    }
    if(count<3 || chain[count-1]!=hips)return false;
    std::reverse(chain,chain+count);
    const int joints[6]={g_VRRightUpLegIdx,g_VRRightLegIdx,g_VRRightFootIdx,
                         g_VRLeftUpLegIdx,g_VRLeftLegIdx,g_VRLeftFootIdx};
    for(int side=0;vehicle && side<2;++side) {
        const int* j=joints+side*3;
        for(int k=0;k<3;++k)if(j[k]<0 || j[k]>=VRIK_FKCount())return false;
        if(g_VRBoneParent[j[0]]!=hips || g_VRBoneParent[j[1]]!=j[0] || g_VRBoneParent[j[2]]!=j[1])return false;
    }
    VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    const float forward[3]={0,1,0},rightAxis[3]={1,0,0};
    float facing[3];VRIK_QuatRotateVec(headRotation,forward,facing);facing[2]=0;
    if(VRIK_Norm3(facing)<1e-4f) {
        float right[3];VRIK_QuatRotateVec(headRotation,rightAxis,right);
        facing[0]=-right[1];facing[1]=right[0];facing[2]=0;
        if(VRIK_Norm3(facing)<1e-4f)return false;
    }
    // Swimming has its own physical body-yaw cone. Do not bypass it by making
    // the entire upper body follow every head turn as passenger combat does.
    const float yaw=vehicle ? std::atan2(-facing[0],facing[1]):0.0f;
    if(!vehicle) { facing[0]=0;facing[1]=1;facing[2]=0; }
    const float heading[4]={0,0,std::sin(yaw*.5f),std::cos(yaw*.5f)};
    float inverseHeading[4],relativeHead[4];VRIK_QuatConj(heading,inverseHeading);
    VRIK_QuatMul(inverseHeading,headRotation,relativeHead);VRIK_QuatNorm(relativeHead);
    const auto cameraFromEyes=cvr::camera::RotateAnchorYaw(s_neckCameraEyeOffset,yaw);
    const float eyeTarget[3]={cameraTarget[0]-cameraFromEyes.x,
        cameraTarget[1]-cameraFromEyes.y,cameraTarget[2]-cameraFromEyes.z};

    float positions[12][3]{},rotations[12][4]{};
    std::copy_n(vehicle ? g_fkRot[hips]:s_referenceModelRot[hips],4,rotations[0]);
    for(int i=1;i<count;++i) {
        const int bone=chain[i];
        VRIK_QuatRotateVec(rotations[i-1],s_referenceLocalPos[bone],positions[i]);
        for(int k=0;k<3;++k)positions[i][k]+=positions[i-1][k];
        if(bone==headIdx) { std::copy_n(headQ,4,rotations[i]);continue; }
        float flex=0,headShare=0;
        for(int c=0;c<g_VRSpineCount;++c)if(g_VRSpineIdx[c]==bone) {
            flex=cvr::vrik::SpineFlex(c,g_VRSpineCount);
            if(c==g_VRSpineCount-1)for(int k=0;k<3;++k)
                positions[i][k]-=facing[k]*cvr::vrik::ChestRetraction;
        }
        if(bone==g_VRNeckIdx) { flex=cvr::vrik::NeckFlex;headShare=.25f; }
        if(bone==g_VRNeck1Idx) { flex=cvr::vrik::UpperNeckFlex;headShare=.55f; }
        const float bend[4]={std::sin(flex*.5f),0,0,std::cos(flex*.5f)};
        float partial[4],tilted[4],world[4];VRIK_QuatScale(relativeHead,headShare,partial);
        VRIK_QuatMul(partial,bend,tilted);VRIK_QuatMul(heading,tilted,world);
        VRIK_QuatMul(world,s_referenceModelRot[bone],rotations[i]);VRIK_QuatNorm(rotations[i]);
    }
    float eye[3],pelvis[3];VRIK_QuatRotateVec(headQ,s_eyeInHead,eye);
    for(int k=0;k<3;++k)pelvis[k]=eyeTarget[k]-positions[count-1][k]-eye[k];

    float feet[2][3],footRot[2][4],poles[2][3];
    auto* saved=vehicle ? SaveVehicleLower(boneBuf):nullptr;
    for(int side=0;vehicle && side<2;++side) {
        const int* j=joints+side*3;
        std::copy_n(g_fkPos[j[2]],3,feet[side]);std::copy_n(g_fkRot[j[2]],4,footRot[side]);
        for(int k=0;k<3;++k)poles[side][k]=g_fkPos[j[1]][k]-g_fkPos[j[0]][k];
        if(VRIK_Norm3(poles[side])<1e-6f)std::copy_n(forward,3,poles[side]);
    }
    const float id[4]={0,0,0,1},zero[3]={};
    for(int i=0;i<count;++i) {
        const int bone=chain[i],parent=g_VRBoneParent[bone];
        float target[3];for(int k=0;k<3;++k)target[k]=pelvis[k]+positions[i][k];
        VRIK_WriteLocalPos(boneBuf,bone,parent>=0 ? g_fkPos[parent]:zero,parent>=0 ? g_fkRot[parent]:id,target);
        VRIK_WriteLocalRot(boneBuf,bone,parent>=0 ? g_fkRot[parent]:id,rotations[i]);
        std::copy_n(target,3,g_fkPos[bone]);std::copy_n(rotations[i],4,g_fkRot[bone]);
    }
    VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    for(int side=0;vehicle && side<2;++side) {
        const int* j=joints+side*3;
        VRIK_SolveLeg(boneBuf,j[0],j[1],j[2],feet[side],poles[side]);
        VRIK_ComputeFK(boneBuf,VRIK_FKCount());
        VRIK_WriteLocalRot(boneBuf,j[2],g_fkRot[j[1]],footRot[side]);
        for(int k=0;k<3;++k)s_bendLegOwned[j[k]]=true;
        VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    }
    if(saved)for(int i=0;i<saved->count;++i)std::memcpy(saved->solved[i],boneBuf+saved->bones[i]*48,48);
    std::copy_n(eyeTarget,3,g_VRIKBodyEyeTarget);
    VRIK_QuatRotateVec(g_fkRot[headIdx],s_eyeInHead,g_VRIKBodyEyeSolved);
    for(int k=0;k<3;++k)g_VRIKBodyEyeSolved[k]+=g_fkPos[headIdx][k];
    g_VRIKBodyEyeBound=true;g_VRIKVehicleUpperActive=vehicle;
    VRIK_PublishBodyBones();return true;
}

bool VRIK_PlaceVehicleCombatUpper(uint8_t* boneBuf,const float* cameraTarget,
                                 const float* headRotation,int headIdx) {
    return VRIK_PlaceHeadAnchoredBody(boneBuf,cameraTarget,headRotation,headIdx,true);
}

namespace { float s_squatEMA{};bool s_squatInit{}; }
void VRIK_ApplyLadderFingers(uint8_t* boneBuf,int side,int kind,const float* curls,const float (*adjustments)[6]) {
    if(!boneBuf || side<0 || side>1 || kind<1 || kind>3)return;
    if(kind==2 && !curls) {
        curls=cvr::ladder::profile::RungCurl;
        adjustments=cvr::ladder::profile::RungAdjust[side];
    }
    for(int slot=0;slot<19;++slot) {
        const int bone=s_ladderFingers[side][slot];
        if(bone<0 || bone>=g_VRBoneCount || !s_referenceValid[bone])continue;
        const auto& finger=cvr::ladder::profile::fingers[slot];
        auto* q=reinterpret_cast<float*>(boneBuf+bone*48+VRIK_ROT_OFF);
        if(kind!=2 || !curls) { std::copy_n(finger.rotation[side],4,q);continue; }
        const float value=curls[finger.group],amount=std::clamp(std::isfinite(value) ? value:1.0f,0.0f,1.5f);
        float inverse[4],delta[4],part[4];
        VRIK_QuatConj(s_referenceLocalRot[bone],inverse);VRIK_QuatMul(finger.rotation[side],inverse,delta);
        VRIK_QuatScale(delta,amount,part);VRIK_QuatMul(part,s_referenceLocalRot[bone],q);VRIK_QuatNorm(q);
        if(adjustments) {
            float angles[3];cvr::ladder::FingerAdjustmentAngles(finger.suffix,finger.group,adjustments[finger.group],angles);
            for(int axis=0;axis<3;++axis)if(angles[axis]!=0) {
                const float half=angles[axis]*.00872664626f;
                float offset[4]={0,0,0,std::cos(half)},result[4];offset[axis]=std::sin(half);
                VRIK_QuatMul(q,offset,result);VRIK_QuatNorm(result);std::copy_n(result,4,q);
            }
        }
    }
}
void VRIK_PlaceLadderUpper(uint8_t* boneBuf,const float* cameraTarget,const float* headRotation,int headIdx) {
    const int hips=g_VRHipsIdx;
    if(!boneBuf || !headRotation || hips<0 || hips>=g_VRBoneCount || headIdx<0 || headIdx>=g_VRBoneCount ||
       !s_referenceValid[hips] || !g_VRHeadReferenceValid)return;
    s_squatEMA=0;s_squatInit=false;s_vrSharedSquatDrop=0;
    g_VRIKBodyBendAngle=0;std::fill_n(g_VRIKBodyBendPelvis,3,0.0f);
    VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    float inverse[4],hipDelta[4],inverseDelta[4],relativeHead[4];
    VRIK_QuatConj(s_referenceModelRot[hips],inverse);VRIK_QuatMul(g_fkRot[hips],inverse,hipDelta);
    VRIK_QuatConj(hipDelta,inverseDelta);VRIK_QuatMul(inverseDelta,headRotation,relativeHead);
    int chain[12],count=0;
    for(int bone=headIdx;bone>=0 && bone<g_VRBoneCount && bone!=hips && count<12;bone=g_VRBoneParent[bone])
        chain[count++]=bone;
    if(!count || g_VRBoneParent[chain[count-1]]!=hips)return;
    std::reverse(chain,chain+count);
    for(int i=0;i<count;++i)if(!s_referenceValid[chain[i]])return;
    float local[12][4]{},baseLocal[12][4]{},positions[12][3]{},rotations[12][4]{},limits[12]{};
    // Start every solve from the same anatomical chain attached to the native
    // pelvis. Only its upper joints may rotate; lengths and support stay fixed.
    for(int i=0;i<count;++i) {
        const int bone=chain[i];
        float share=0,flex=0;
        for(int spine=0;spine<g_VRSpineCount;++spine)if(g_VRSpineIdx[spine]==bone) {
            flex=cvr::vrik::SpineFlex(spine,g_VRSpineCount);
            share=.15f*float(spine+1)/float(g_VRSpineCount);
            limits[i]=.14f+.21f*float(spine)/float(std::max(1,g_VRSpineCount-1));
        }
        if(bone==g_VRNeckIdx) { share=.35f;flex=cvr::vrik::NeckFlex; }
        if(bone==g_VRNeck1Idx) { share=.65f;flex=cvr::vrik::UpperNeckFlex; }
        if(bone==headIdx) { share=1;flex=0; }
        const float bend[4]={std::sin(flex*.5f),0,0,std::cos(flex*.5f)};
        float part[4],tilt[4],heading[4],model[4];
        VRIK_QuatScale(relativeHead,share,part);VRIK_QuatMul(part,bend,tilt);
        VRIK_QuatMul(hipDelta,tilt,heading);VRIK_QuatMul(heading,s_referenceModelRot[bone],model);
        VRIK_QuatConj(i ? rotations[i-1]:g_fkRot[hips],inverse);
        VRIK_QuatMul(inverse,model,local[i]);VRIK_QuatNorm(local[i]);
        std::copy_n(local[i],4,baseLocal[i]);std::copy_n(model,4,rotations[i]);
    }
    auto evaluate=[&]() {
        for(int i=0;i<count;++i) {
            const float* parentQ=i ? rotations[i-1]:g_fkRot[hips];
            const float* parentP=i ? positions[i-1]:g_fkPos[hips];
            VRIK_QuatRotateVec(parentQ,s_referenceLocalPos[chain[i]],positions[i]);
            for(int k=0;k<3;++k)positions[i][k]+=parentP[k];
            VRIK_QuatMul(parentQ,local[i],rotations[i]);VRIK_QuatNorm(rotations[i]);
        }
    };
    evaluate();
    const bool fit=cameraTarget && s_eyeReferenceValid &&
        std::isfinite(cameraTarget[0]) && std::isfinite(cameraTarget[1]) && std::isfinite(cameraTarget[2]);
    float headQ[4];VRIK_QuatMul(headRotation,s_referenceModelRot[headIdx],headQ);VRIK_QuatNorm(headQ);
    if(fit) {
        float eye[3],target[3];
        // The ladder keeps the native body heading. Its camera mount is in
        // model axes, as in AnchorRecipe; a free head turn must not rotate it.
        const auto mount=s_neckCameraEyeOffset;
        VRIK_QuatRotateVec(headQ,s_eyeInHead,eye);
        g_VRIKBodyEyeTarget[0]=cameraTarget[0]-mount.x;
        g_VRIKBodyEyeTarget[1]=cameraTarget[1]-mount.y;
        g_VRIKBodyEyeTarget[2]=cameraTarget[2]-mount.z;
        for(int k=0;k<3;++k)target[k]=g_VRIKBodyEyeTarget[k]-eye[k];
        // Bounded CCD over the four spine joints, not the pelvis or neck.
        // Fit positional HMD motion instead of interpreting looking down as
        // a whole-body bend. Upper thoracic joints have the larger allowance.
        // Evaluate only this short chain inside the iterations, then full FK once.
        for(int pass=0;pass<8;++pass)for(int i=count-2;i>=0;--i)if(limits[i]>0) {
            float current[3],desired[3];
            for(int k=0;k<3;++k) {
                current[k]=positions[count-1][k]-positions[i][k];desired[k]=target[k]-positions[i][k];
            }
            if(VRIK_Norm3(current)<.01f || VRIK_Norm3(desired)<.01f)continue;
            float delta[4],step[4],model[4],next[4],relative[4],limited[4],baseInverse[4];
            VRIK_QuatFromTo(current,desired,delta);VRIK_QuatScale(delta,.65f,step);
            VRIK_QuatMul(step,rotations[i],model);
            VRIK_QuatConj(i ? rotations[i-1]:g_fkRot[hips],inverse);VRIK_QuatMul(inverse,model,next);
            VRIK_QuatConj(baseLocal[i],baseInverse);VRIK_QuatMul(next,baseInverse,relative);VRIK_QuatNorm(relative);
            const float angle=2*std::acos(std::clamp(std::abs(relative[3]),0.0f,1.0f));
            VRIK_QuatScale(relative,angle>limits[i] ? limits[i]/angle:1.0f,limited);
            VRIK_QuatMul(limited,baseLocal[i],local[i]);VRIK_QuatNorm(local[i]);evaluate();
        }
    }
    // The head still follows HMD orientation exactly while the spine fits its
    // position. The wrist/arm solves below inherit these moved shoulder bases.
    VRIK_QuatConj(count>1 ? rotations[count-2]:g_fkRot[hips],inverse);
    VRIK_QuatMul(inverse,headQ,local[count-1]);
    for(int i=0;i<count;++i) {
        std::copy_n(s_referenceLocalPos[chain[i]],3,reinterpret_cast<float*>(boneBuf+chain[i]*48+VRIK_TRANS_OFF));
        std::copy_n(local[i],4,reinterpret_cast<float*>(boneBuf+chain[i]*48+VRIK_ROT_OFF));
    }
    VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    VRIK_QuatRotateVec(g_fkRot[headIdx],s_eyeInHead,g_VRIKBodyEyeSolved);
    for(int k=0;k<3;++k)g_VRIKBodyEyeSolved[k]+=g_fkPos[headIdx][k];
    g_VRIKBodyEyeBound=fit;
    // Root, Hips and every leg local remain exactly as native ladder animation
    // supplied them. No floor IK, squat, hinge or pelvis fitting runs here.
    VRIK_PublishBodyBones();
}
bool VRIK_PlaceSwimmingBody(uint8_t* boneBuf,const float* cameraTarget,const float* headRotation,int headIdx) {
    VRIK_BeginBodySolve(boneBuf);
    s_squatEMA=0;s_squatInit=false;s_vrSharedSquatDrop=0;
    // Fit from the actual eye centre, including the water camera's translation.
    // No floor contacts or physical crouch/bend are imposed while swimming.
    return VRIK_PlaceHeadAnchoredBody(boneBuf,cameraTarget,headRotation,headIdx,false);
}

// Camera alignment is a separate neutral-neck mount, not an animated measurement.
void VRIK_PlaceBodyUnderHMD(uint8_t* boneBuf,
                                          const float* camModelPos, const float* camModelRot,
                                          int headIdx, const float* /*bodyFwd*/,const float* pairedHeadDeltaModel,
                                          const float* bodyOrbitModel) {
    if(headIdx<0 || headIdx>=g_VRBoneCount || !g_VRHeadReferenceValid)return;
    VRIK_BeginBodySolve(boneBuf);
    const float bendAngle=SharedPose(115)==2.0f ? SharedPose(114):0;
    const float hingeDrop=cvr::body::HingeDrop(bendAngle);
    const float id[4]={0,0,0,1};
    // Native locomotion owns Root and leg local animation; VRIK owns Hips.
    // Real-life SQUAT: the game FPP camera height (camModelPos.z) is FIXED, so it can't tell when
    // the player physically crouches. shared[89] = the HMD's physical height rel the recenter base
    // (~0 standing, negative squatting); lower the upper-body anchor by that amount.
    // Squat height: use the NECK-PIVOT height (shared[90]), which removes the optical-centre arc
    // so looking DOWN no longer reads as a crouch (the #1 false-squat cause). Fall back to the raw
    // HMD height [89] if the producer hasn't written [90]. Deadzone g_VRSquatThreshold then ignores
    // small head bob; subtract it so the squat ramps from 0 smoothly.
    float squatDrop = 0.0f;
    if (g_pSharedHands) {
        float hy = SharedPose(90);
        if (hy == 0.0f) hy = SharedPose(89);
        const float ordinaryDrop=-hy-g_VRSquatThreshold-hingeDrop;
        // slot90 includes the neutral8cm neck-to-eye height. During a confirmed
        // floor reach, remove that bias and use a small crouch threshold: the
        // old20cm threshold kept the hips high enough to leave hands short.
        const float reachDrop=-(hy+.08f)-.02f-hingeDrop;
        const float reachWeight=cvr::body::BendWeight(bendAngle);
        float drop=ordinaryDrop+(reachDrop-ordinaryDrop)*reachWeight;
        if (drop > 0.0f) squatDrop = drop;
        if (squatDrop > 0.7f) squatDrop = 0.7f;
    }
    // Squat deadzone(2cm) + EMA(alpha=0.25) so sprint/jump head-bob doesn't twitch body+arms.
    // Small changes inside the deadband are frozen (ignore bob); larger real crouches ease in.
    // Shared via s_vrSharedSquatDrop so the ARM anchor uses the SAME smoothed squat as the body.
    {
        const float kSquatDead = 0.02f; const float kSquatA = 0.25f;
        if (!s_squatInit) { s_squatEMA = squatDrop; s_squatInit = true; }
        else if (squatDrop==0 || std::fabs(squatDrop - s_squatEMA) > kSquatDead) {
            s_squatEMA += (squatDrop - s_squatEMA) * kSquatA;
            if(squatDrop==0 && s_squatEMA<.001f)s_squatEMA=0;
        }
        squatDrop = s_squatEMA;
    }
    s_vrSharedSquatDrop = squatDrop;
    int chain[10],chainN=0;
    const int spineCount=std::clamp(static_cast<int>(g_VRSpineCount),0,8);
    for(int i=0;i<spineCount;++i)chain[chainN++]=g_VRSpineIdx[i];
    if(g_VRNeckIdx>=0)chain[chainN++]=g_VRNeckIdx;
    if(g_VRNeck1Idx>=0 && chainN<10)chain[chainN++]=g_VRNeck1Idx;
    // Own the pelvis, while keeping every native leg local transform. Native
    // camera height still supplies crouch/jump state; physical squat moves hips
    // rather than compressing the spine root away from its anatomical parent.
    const int hips=g_VRHipsIdx;
    if(hips>=0 && hips<g_VRBoneCount && s_referenceValid[hips] && s_referenceCameraValid) {
        const int parent=g_VRBoneParent[hips];
        const float zero[3]={};
        float target[3]={s_referenceModelPos[hips][0],s_referenceModelPos[hips][1],
            // Bent-leg contacts are captured at native standing height first.
            // Apply physical descent in the IK stage, so a floor reach bends
            // the knees instead of translating the feet below the ground.
            s_referenceModelPos[hips][2]+camModelPos[2]-s_referenceCameraHeight-
                (bendAngle>1e-5f ? 0.0f:squatDrop)};
        // Calibration stays at the physical tracking origin. Place the avatar
        // about that optical centre in model space; never move the camera to
        // follow a pelvis rotation. All native leg local animation is retained.
        if(bodyOrbitModel && std::isfinite(bodyOrbitModel[0]) && std::isfinite(bodyOrbitModel[1])) {
            target[0]+=bodyOrbitModel[0];target[1]+=bodyOrbitModel[1];
        }
        VRIK_WriteLocalPos(boneBuf,hips,parent>=0 ? g_fkPos[parent]:zero,
                          parent>=0 ? g_fkRot[parent]:id,target);
        VRIK_WriteLocalRot(boneBuf,hips,parent>=0 ? g_fkRot[parent]:id,s_referenceModelRot[hips]);
        VRIK_ComputeFK(boneBuf,VRIK_FKCount());
    }
    const int spineRoot=spineCount ? g_VRSpineIdx[0] : -1;
    if(spineRoot>=0 && spineRoot<g_VRBoneCount && s_referenceValid[spineRoot]) {
        // Reference translations make repeated solves idempotent. Using the last
        // written root offset here would accumulate a second body displacement.
        for(int c=0;c<chainN;++c) {
            const int bone=chain[c];
            if(bone<0 || bone>=g_VRBoneCount || !s_referenceValid[bone])continue;
            std::copy_n(s_referenceLocalPos[bone],3,
                        reinterpret_cast<float*>(boneBuf+bone*48+VRIK_TRANS_OFF));
        }
        VRIK_ComputeFK(boneBuf,VRIK_FKCount());
        for(int c=0;c<chainN;++c) {
            const int bone=chain[c];
            if(bone<0 || bone>=g_VRBoneCount || !s_referenceValid[bone])continue;
            // Rounded upper spine and a small chest retraction. Resetting local
            // translations above keeps the posterior shift absolute each solve.
            float flex=0.0f,headShare=0.0f;
            if(c<spineCount)flex=cvr::vrik::SpineFlex(c,spineCount);
            if(bone==g_VRNeckIdx) { flex=cvr::vrik::NeckFlex;headShare=.25f; }
            if(bone==g_VRNeck1Idx) { flex=cvr::vrik::UpperNeckFlex;headShare=.55f; }
            const float bend[4]={std::sin(flex*.5f),0,0,std::cos(flex*.5f)};
            float headPart[4],bendHead[4],modelRot[4];
            VRIK_QuatScale(camModelRot,headShare,headPart);
            VRIK_QuatMul(headPart,bend,bendHead);
            VRIK_QuatMul(bendHead,s_referenceModelRot[bone],modelRot);
            const int parent=g_VRBoneParent[bone];
            if(c==spineCount-1) {
                const float chest[3]={g_fkPos[bone][0],g_fkPos[bone][1]-cvr::vrik::ChestRetraction,g_fkPos[bone][2]};
                const float zero[3]={};
                VRIK_WriteLocalPos(boneBuf,bone,parent>=0 ? g_fkPos[parent]:zero,
                                  parent>=0 ? g_fkRot[parent]:id,chest);
            }
            VRIK_WriteLocalRot(boneBuf,bone,parent>=0 ? g_fkRot[parent] : id,modelRot);
            VRIK_ComputeFK(boneBuf,VRIK_FKCount());
        }

    }
    // A camera's axes are not a skull bone's axes. Preserve the reference basis.
    float headModelRot[4];VRIK_QuatMul(camModelRot,g_VRHeadReferenceModelRot,headModelRot);
    const int hp=g_VRBoneParent[headIdx];
    VRIK_WriteLocalRot(boneBuf,headIdx,hp>=0 ? g_fkRot[hp] : id,headModelRot);
    VRIK_ComputeFK(boneBuf,VRIK_FKCount());

    // NOTE: the palm publish used to live here and read g_fkPos[palmBone]. That is BEFORE
    // VRIK_SolveArm runs (lines ~3300 / ~3550), so it returned the ENGINE'S ANIMATED pose -- the
    // idle animation with the hands at the thighs -- and the ball sat at the player's right hip
    // no matter where the controller was. Exactly the trap the hand-to-holster code documents a
    // few hundred lines below. The publish now happens right after each arm is solved.

    VRIK_ApplyBodyBend(boneBuf,bendAngle,camModelRot,squatDrop,pairedHeadDeltaModel);
    VRIK_PublishBodyBones();

    // Camera alignment now comes from the immutable neutral-neck mount.
    // Retire the old animated-head EMA so it cannot add a second offset.
    if(g_pSharedHands) {
        g_pSharedHands[116]=g_pSharedHands[117]=g_pSharedHands[118]=0;
        g_pSharedHands[119]=0;
    }

    g_VRIKDbgChest[0]=g_fkPos[headIdx][0]; g_VRIKDbgChest[1]=g_fkPos[headIdx][1]; g_VRIKDbgChest[2]=g_fkPos[headIdx][2];
    for(int k=0;k<3;++k)g_VRIKDbgChestTgt[k]=g_fkPos[headIdx][k];
}


// Swing-twist: extract the TWIST of q about axis a (unit, same frame as q's vector part):
// twist = normalize((v·a)a, w); identity when the projection degenerates (q ⟂ a, 180° swing).
void VRIK_TwistAbout(const float* q, const float* a, float* outT) {
    const float d = q[0]*a[0] + q[1]*a[1] + q[2]*a[2];
    outT[0] = a[0]*d; outT[1] = a[1]*d; outT[2] = a[2]*d; outT[3] = q[3];
    float n = outT[0]*outT[0] + outT[1]*outT[1] + outT[2]*outT[2] + outT[3]*outT[3];
    if (n < 1e-10f) { outT[0]=0.0f; outT[1]=0.0f; outT[2]=0.0f; outT[3]=1.0f; return; }
    n = 1.0f / std::sqrt(n);
    outT[0]*=n; outT[1]*=n; outT[2]*=n; outT[3]*=n;
}

// AnimPoseFunc_t and OriginalAnimPose moved to src/Hooks/AnimPose.cpp: the trampoline the
// detour calls through is the hook's own state, not the character solve's.

// Solve cache (one solve per tick + replay).
// The frame the cache holds a solve for, and the frame counter itself. See the note in
// Anim/CharacterRig.hpp: this used to be the Lua-pushed tick from the shared block.
uint32_t g_solveCacheTick = 0xFFFFFFFFu;
std::atomic<uint32_t> g_VrikFrameEpoch{0};

// ---- WHERE DOES THE SHAKE ENTER? ---------------------------------------------------------------
//
// The SECOND DIFFERENCE of a position, |p[n] - 2p[n-1] + p[n-2]|, in millimetres. Smooth motion --
// however fast -- has a small second difference; noise has a large one, because noise reverses
// direction every sample and that is precisely what the second difference measures. The same number
// at three stages of the hand pipeline therefore says WHERE the shake is added:
//
//   0 ctrl    the controller position as the SOLVE reads it, HMD-local (~52 Hz, irregular)
//   1 anchor  the point the arm hangs off, model space (shared by both hands)
//   2 target  the hand target the IK actually solves for, model space
//   3 xr      the SAME controller position, sampled on the XR thread (72 Hz, uniform)
//
// STAGE 3 EXISTS TO CATCH ALIASING, and it is the difference between 3 and 0 that matters. The XR
// thread publishes at 72 Hz; the solve consumes at ~52 Hz and takes whatever the newest publish was, so
// its effective sampling interval alternates between one and two XR frames. For a MOVING hand that
// alternation alone produces |v * (dt_n - dt_n-1)| of apparent noise -- at 0.15 m/s and 14 ms of
// alternation, 2.1 mm, which is the order of the 3.5 mm measured at stage 0. Stage 3 samples the same
// number uniformly, on the producer's own clock, where that term cannot arise.
//
// Stage 3 quiet and stage 0 loud => the shake is our sampling, not the tracking, and no filter belongs
// anywhere: the solve should ask for the pose at ITS OWN instant instead of consuming a faster stream.
//
// Stage 0 large  -> the tracking is noisy and nothing downstream is at fault.
// Stage 0 quiet, 1 large -> the ANCHOR shakes: the composed camera, the entity, the bob.
// Stages 0 and 1 quiet, 2 large -> we add it ourselves, in the offset composition.
//
// ALL THREE ARE SAMPLED IN THE SAME PLACE, once per fresh solve. That is not a detail: a second
// difference depends on the sampling interval, so measuring stage 0 on the XR thread at 72 Hz and the
// others in the solve at ~52 Hz would make the three numbers incomparable and the comparison is the
// entire point.
//
// A PEAK, CLEARED EACH WINDOW, not a mean: shake is an excursion, and an average over a hundred
// frames hides one bad sample in ninety-nine good ones.
//
// This instrument replaces one that never existed. Slots [224..226] were read by the rate census and
// described in its comment as exactly this measurement, and nothing in the tree ever wrote them -- so
// it printed zeros from the day it was added.
// TWO PEAKS, and the difference between them is the whole reading.
//
// A second difference assumes a UNIFORM sampling interval. Ours is not uniform -- the game produces a
// 41 ms frame every couple of seconds, measured -- and with uneven dt a perfectly smooth hand still
// scores |v * (dt_n - dt_n-1)|. At 2 m/s a 17 ms hiccup alone is 34 mm, which is the size of the peaks
// the first version of this instrument reported. It was measuring frame-time jitter and calling it
// tracking noise.
//
// So the number that answers the symptom is the SLOW one: the same second difference, accumulated
// only while the hand is moving slower than kShakeSlowMm per sample. At that speed the v*dt term
// cannot manufacture millimetres, so what is left is really noise. The ALL peak stays beside it,
// because the pair is diagnostic: all >> slow means the peaks are motion and timing, not jitter.
// How far the ENGINE had moved a bone we own, between its graph evaluation and our write, in mm.
// Peak per window. See the measurement site in src/Hooks/AnimPose.cpp: this is the size of the only
// window in which the skeleton holds somebody else's arm, and therefore the size of the error any
// renderer that samples inside it will draw.
// The batch-gap census: how many pass gaps were counted as a NEW animation batch and how many as the
// same one, with the extremes of each. The two populations must stay far apart -- passes inside a batch
// are microseconds apart, batches a frame apart -- and the moment SameMax approaches NewMin the batch
// clock has stopped being safe.
// Which pass of the animation batch found a bone of ours already moved. Index 0 is the batch's first
// pass (which takes the fresh-solve path, so it never appears here), 1..6 the later ones, 7 a catch-all.
int   g_VrikOverwritePassHist[8] = {0,0,0,0,0,0,0,0};

int   g_VrikBatchGapNew = 0;
int   g_VrikBatchGapSame = 0;
float g_VrikBatchGapNewMin = 0.0f;
float g_VrikBatchGapSameMax = 0.0f;

float g_VrikEngineOverwriteMm = 0.0f;
// Which bone it was, and how often it happened at all: a quarter-metre peak that fires in one window
// out of three is a different problem from one that fires every pass, and the bone says whose arm --
// or whose body -- is being drawn in the wrong place.
int   g_VrikEngineOverwriteBone = -1;
int   g_VrikEngineOverwriteHits = 0;      // passes where the engine had moved a bone more than 10 mm
int   g_VrikEngineOverwritePasses = 0;    // replay passes examined
float g_VrikShakePeakMm[2][4] = {{0.0f,0.0f,0.0f,0.0f},{0.0f,0.0f,0.0f,0.0f}};   // any motion
float g_VrikShakeSlowMm[2][4] = {{0.0f,0.0f,0.0f,0.0f},{0.0f,0.0f,0.0f,0.0f}};   // slow motion only
int   g_VrikShakeSlowN[2][4]  = {{0,0,0,0},{0,0,0,0}};
namespace {
struct VrikShakeHist { float p1[3]; float p2[3]; int n; };
VrikShakeHist g_vrikShakeHist[2][4] = {};
// 3 mm per sample is ~0.15 m/s at 52 fps: a deliberate, watched, slow hand -- exactly the motion the
// shake was reported at, and slow enough that timing jitter contributes microns rather than millimetres.
constexpr float kShakeSlowMm = 3.0f;
}  // namespace

void VRIK_NoteShake(int hand, int stage, const float* pos) {
    if (hand < 0 || hand > 1 || stage < 0 || stage > 3 || !pos) return;
    VrikShakeHist& h = g_vrikShakeHist[hand][stage];
    if (h.n >= 2) {
        float acc2 = 0.0f, step2 = 0.0f;
        for (int i = 0; i < 3; ++i) {
            const float d2 = pos[i] - 2.0f * h.p1[i] + h.p2[i];
            acc2 += d2 * d2;
            const float d1 = pos[i] - h.p1[i];
            step2 += d1 * d1;
        }
        const float mm = std::sqrt(acc2) * 1000.0f;
        const float stepMm = std::sqrt(step2) * 1000.0f;
        if (mm < 500.0f) {
            if (mm > g_VrikShakePeakMm[hand][stage]) g_VrikShakePeakMm[hand][stage] = mm;
            if (stepMm < kShakeSlowMm) {
                if (mm > g_VrikShakeSlowMm[hand][stage]) g_VrikShakeSlowMm[hand][stage] = mm;
                ++g_VrikShakeSlowN[hand][stage];
            }
        }
    }
    for (int i = 0; i < 3; ++i) { h.p2[i] = h.p1[i]; h.p1[i] = pos[i]; }
    if (h.n < 2) ++h.n;
}

int   g_solveCacheN = 0;
int   g_solveCacheIdx[VRIK_MAX_BONES];
float g_solveCacheVal[VRIK_MAX_BONES][7];
float g_solveCacheYaw = 0.0f;   // heading the cached solve was built with
float g_solveCacheSnapCtr = -1.0f; // snap event counter [147] the cached solve consumed
