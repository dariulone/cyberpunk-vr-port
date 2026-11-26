// NpcArmGrip -- a hand taking a living person: by an arm, a leg, the head or the chest.
//
// There is no switch for one limb of a person's ragdoll: the "Animated" activation was tried (2026-11-04), its bodies appear and the person goes on as before. What is drawn comes from the pose, though,
// and this port's pose hook sees every skeleton's apply. So the part is solved there, as the player's own arm is:
//
//   an arm or a leg: two bones, the hand or foot to where the grip has it, elbow or knee in the plane they were in. An arm pulled past its stretch brings the shoulder along, and the spine less and less.
//   the head: the neck to the grip, and the spine a little; it turns with the hand that holds it, as far as a neck lets it.
//   the chest: the spine bends to the grip.
//
// The hips stay where they are, so the feet do too, and the person goes on walking and doing as they do: what they do with what is held is overridden only while it is held. It blends in and out.
//
// IDENTIFIED BY THE INSTANCE'S POSE DESCRIPTOR (AnimatedObject+0x18). Reference
// float tracks belong to the shared MetaRig and cannot identify an NPC instance.
// The buffer is indexed as the rig's bone names, when a pass has all of them; a pass of a
// lower level of detail has fewer and is left alone. Bone names and parents read from base\characters\base_entities\man_base\man_base.rig (Hips 2, Spine 4, Spine1 7, Spine2 10, Spine3 13, Neck 16, Neck1 19, Head 22).
#include "Anim/NpcArmGrip.hpp"
#include "Anim/NpcJointLimits.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Natives/PersonGrip.hpp"
#include "Natives/PhysHands.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/Entity.hpp>
#include <RED4ext/Scripting/Natives/animAnimatedObject.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
#include <RED4ext/Scripting/Natives/entIPlacedComponent.hpp>
#include <windows.h>
#include <algorithm>
#include <cmath>
#include <cstring>

extern void Log(const char* fmt, ...);
RED4ext::anim::AnimatedObject* FindAnimatedObjectOf(RED4ext::ent::Entity* entity);

namespace cvr::anim {
namespace {
constexpr float kBlendIn=.15f,kBlendOut=.2f,kTurnMost=1.2f;      // s, s, rad a held hand is turned from what it was doing
constexpr float kHeadFollow=.7f,kHeadBend=.5f,kHeadTwist=.8f,kHeadNod=.45f,kHeadRoll=.3f;   // of the hand's turn, of the neck's bend; rad about the neck, forward and back, ear to shoulder
constexpr float kStretch=.97f;
constexpr uint64_t kUnaskedMs=400;
constexpr int kMostChain=64,kMostJoints=8,kMostTouched=16;
enum Part {ArmL,ArmR,LegL,LegR,PartHead,PartChest,PartCount};
const char* const kPartNames[PartCount]={"left arm","right arm","left leg","right leg","head","chest"};
struct Joint {int bone;float share,most;};
struct DonePose {
    uint8_t* buf;int count;int bone[kMostChain];
    uint8_t before[kMostChain][48],after[kMostChain][48];
    float target[3],turn[4],weight;
};
struct GripPose {float point[3][3],local[3][4];uint64_t at;};
struct BoneGrip {
    bool on=false,seeded=false,live=false;
    int bone=-1;
    int side=-1,owner=0;
    uint64_t serial=0;
    float capture[3]{},captureQ[4]{0,0,0,1};
    float local[3]{},localQ[4]{0,0,0,1};
    float point[3]{},rotation[4]{0,0,0,1};
    float controlOffset[3]{},controlRotation[4]{0,0,0,1};
    float capsuleSeed[3]{},capsuleSeedQ[4]{0,0,0,1};
    float capsuleLocal[3]{},capsuleLocalQ[4]{0,0,0,1},radius{},half{};
    float entityP[3]{},entityQ[4]{0,0,0,1},worldPoint[3]{},worldGoal[3]{},worldRotation[4]{0,0,0,1};
    cvr::physhands::GripControl control{};
    jointlimits::TwistHistory twist[3]{};
};
struct Grip {
    bool on,letting,seeded;
    int side=-1;
    uintptr_t entityKey=0;
    int part;
    uintptr_t trackA,trackB;
    uintptr_t poseOwner{};
    uint32_t bones;
    int upper,mid,end;       // the two bones and what they carry; for a chain only `end`
    Joint joints[kMostJoints];   // from the root end. What the part's own bones cannot reach
    int jointCount;
    int16_t parents[VRIK_MAX_BONES];
    float target[3],turn[4],offset[3];
    uint64_t begun,let,asked;
    uint32_t passes,again;
    float miss;
    int chest;              // the head is kept from the chest
    float rest[4];           // the head as the chest had it when the grip closed
    jointlimits::Hinge hinge;
    float upperRest[4],endRest[4];
    bool bounded;
    GripPose solved;
    BoneGrip anchor;
    DonePose done[4];
    int doneNext;
};
Grip grips[2]{};
RED4ext::WeakHandle<RED4ext::IScriptable> gripEntities[2];
// Both hands may pull the same torso. Preserve one native input around the
// complete pair of solves, otherwise each hand mistakes the other's spine
// changes for a new animation and repeatedly adds its own reach correction.
struct NativePose {
    uintptr_t descriptor;uint8_t* buf;int count;int bone[2*kMostChain];
    uint8_t before[2*kMostChain][48],after[2*kMostChain][48];
    RED4ext::WeakHandle<RED4ext::IScriptable> owner;
    uintptr_t ownerKey{};
    bool retired{};
};
NativePose nativePoses[8]{};
int nextNativePose=0;
SRWLOCK lock=SRWLOCK_INIT;
volatile LONG anyOn=0;
std::atomic<uintptr_t> activeOwners[2]{},retiredOwners[8]{};
std::atomic<bool> anyRetired{};
std::atomic<uint64_t> nextCleanupAt{};
void PublishActivity(){
    for(int side=0;side<2;++side)activeOwners[side].store(grips[side].on?grips[side].poseOwner:0,std::memory_order_release);
    InterlockedExchange(&anyOn,grips[0].on||grips[1].on?1:0);
    bool retired=false;for(const auto& p:nativePoses)retired|=p.retired;anyRetired.store(retired,std::memory_order_release);
}
struct Tr {float p[3];float q[4];};
void Child(const Tr& parent,const float* lp,const float* lq,Tr& out) {
    VRIK_QuatMul(parent.q,lq,out.q);
    VRIK_QuatNorm(out.q);
    float r[3];
    VRIK_QuatRotateVec(parent.q,lp,r);
    for(int k=0;k<3;++k)out.p[k]=parent.p[k]+r[k];
}
float Dot(const float* a,const float* b){return a[0]*b[0]+a[1]*b[1]+a[2]*b[2];}
void Cross(const float* a,const float* b,float* o){o[0]=a[1]*b[2]-a[2]*b[1];o[1]=a[2]*b[0]-a[0]*b[2];o[2]=a[0]*b[1]-a[1]*b[0];}
float Apart(const float* a,const float* b){const float d[3]={a[0]-b[0],a[1]-b[1],a[2]-b[2]};return std::sqrt(Dot(d,d));}
// The turn taking direction a to direction b.
void Arc(const float* a,const float* b,float* q) {
    q[0]=q[1]=q[2]=0;q[3]=1;
    jointlimits::Arc(a,b,q);
}
void Conj(const float* q,float* o){o[0]=-q[0];o[1]=-q[1];o[2]=-q[2];o[3]=q[3];}
void BlendInto(float* out,const float* from,const float* to,float w) {
    jointlimits::Slerp(from,to,w,out);
}
float AngleBetween(const float* a,const float* b) {
    const float d=std::fabs(a[0]*b[0]+a[1]*b[1]+a[2]*b[2]+a[3]*b[3]);
    return 2.f*std::acos(d<1.f?d:1.f);
}
inline float* LocalT(uint8_t* buf,int i){return reinterpret_cast<float*>(buf+i*48+VRIK_TRANS_OFF);}
inline float* LocalR(uint8_t* buf,int i){return reinterpret_cast<float*>(buf+i*48+VRIK_ROT_OFF);}

// A bone's place in the model, as this pass's buffer has it now.
bool ModelOf(const Grip& g,uint8_t* buf,int bone,Tr& out) {
    int chain[kMostChain],n=0;
    for(int b=bone;b>=0 && n<kMostChain;b=g.parents[b]) {
        if(uint32_t(b)>=g.bones)return false;
        chain[n++]=b;
    }
    if(n==0 || n>=kMostChain)return false;
    Tr at{{0,0,0},{0,0,0,1}};
    for(int i=n-1;i>=0;--i) {
        Tr next;
        Child(at,LocalT(buf,chain[i]),LocalR(buf,chain[i]),next);
        at=next;
    }
    out=at;
    return true;
}
// What was changed, as it was: the animation it is blended with at the end.
struct Touched {
    int count=0;
    int bone[kMostTouched];
    float was[kMostTouched][4];
    const float* Note(uint8_t* buf,int b) {
        for(int i=0;i<count;++i)if(bone[i]==b)return was[i];
        if(count>=kMostTouched)return nullptr;
        bone[count]=b;
        std::copy_n(LocalR(buf,b),4,was[count]);
        return was[count++];
    }
};
// A bone to have this rotation in the model.
void SetModelRot(const Grip& g,uint8_t* buf,int bone,const float* model,Touched& t) {
    Tr parent;
    if(!t.Note(buf,bone) || !ModelOf(g,buf,g.parents[bone],parent))return;
    float inv[4],local[4];
    Conj(parent.q,inv);VRIK_QuatMul(inv,model,local);VRIK_QuatNorm(local);
    std::copy_n(local,4,LocalR(buf,bone));
}
// The joints turn so `tip` comes to `to`, the nearest first, each its share of the way and no further from what the animation had than it can go.
void ReachWith(const Grip& g,uint8_t* buf,int tip,const float* to,Touched& t) {
    for(int pass=0;pass<3;++pass) {
        for(int i=g.jointCount-1;i>=0;--i) {
            const Joint& j=g.joints[i];
            Tr at,end,parent;
            if(!ModelOf(g,buf,j.bone,at) || !ModelOf(g,buf,tip,end) || !ModelOf(g,buf,g.parents[j.bone],parent))return;
            const float from[3]={end.p[0]-at.p[0],end.p[1]-at.p[1],end.p[2]-at.p[2]},want[3]={to[0]-at.p[0],to[1]-at.p[1],to[2]-at.p[2]};
            float arc[4],part[4];
            Arc(from,want,arc);
            const float id[4]={0,0,0,1};
            BlendInto(part,id,arc,j.share);
            float q[4],inv[4],local[4];
            VRIK_QuatMul(part,at.q,q);VRIK_QuatNorm(q);
            Conj(parent.q,inv);VRIK_QuatMul(inv,q,local);VRIK_QuatNorm(local);
            const float* was=t.Note(buf,j.bone);
            if(!was)return;
            const float apart=AngleBetween(was,local);
            if(apart>j.most)BlendInto(local,was,local,j.most/apart);
            std::copy_n(local,4,LocalR(buf,j.bone));
        }
    }
}
// A chain to `to`, the nearest joint first. A joint turns only the way that takes the tip across to the target, its axis from where the tip has to go: never round the far way, whichever side of the joint the target has
// passed. Going by the direction from the joint to the target, a head brought down below the neck turned the trunk right round.
void BendChain(const Grip& g,uint8_t* buf,int tip,const float* to,Touched& t) {
    for(int pass=0;pass<3;++pass) {
        for(int i=g.jointCount-1;i>=0;--i) {
            const Joint& j=g.joints[i];
            Tr at,end,parent;
            if(!ModelOf(g,buf,j.bone,at) || !ModelOf(g,buf,tip,end) || !ModelOf(g,buf,g.parents[j.bone],parent))return;
            const float arm[3]={end.p[0]-at.p[0],end.p[1]-at.p[1],end.p[2]-at.p[2]},gap[3]={to[0]-end.p[0],to[1]-end.p[1],to[2]-end.p[2]};
            const float length=std::sqrt(Dot(arm,arm));
            if(length<1e-3f)continue;
            const float along=Dot(gap,arm)/length;
            const float across[3]={gap[0]-arm[0]*along/length,gap[1]-arm[1]*along/length,gap[2]-arm[2]*along/length};
            const float size=std::sqrt(Dot(across,across));
            if(size<1e-4f)continue;
            float axis[3];Cross(arm,across,axis);
            const float axisSize=std::sqrt(Dot(axis,axis));
            if(axisSize<1e-7f)continue;
            const float depth=length+along>length*.25f?length+along:length*.25f;
            const float half=std::atan2(size,depth)*j.share*.5f,sine=std::sin(half)/axisSize;
            const float part[4]={axis[0]*sine,axis[1]*sine,axis[2]*sine,std::cos(half)};
            float q[4],inv[4],local[4];
            VRIK_QuatMul(part,at.q,q);VRIK_QuatNorm(q);
            Conj(parent.q,inv);VRIK_QuatMul(inv,q,local);VRIK_QuatNorm(local);
            const float* was=t.Note(buf,j.bone);
            if(!was)return;
            const float apart=AngleBetween(was,local);
            if(apart>j.most)BlendInto(local,was,local,j.most/apart);
            std::copy_n(local,4,LocalR(buf,j.bone));
        }
    }
}
// A turn kept within what a neck does: so far about the neck's own line, and away from it less to the side than forward. The model faces +Y: about X is a nod, about Y an ear to a shoulder.
void LimitTurn(float* q,const float* axis,float twistMost,float nodMost,float rollMost) {
    const float id[4]={0,0,0,1};
    const float along=q[0]*axis[0]+q[1]*axis[1]+q[2]*axis[2];
    float twist[4]={axis[0]*along,axis[1]*along,axis[2]*along,q[3]};
    if(twist[0]*twist[0]+twist[1]*twist[1]+twist[2]*twist[2]+twist[3]*twist[3]<1e-6f)std::copy_n(id,4,twist);
    else VRIK_QuatNorm(twist);
    float inv[4],swing[4];
    Conj(twist,inv);VRIK_QuatMul(q,inv,swing);VRIK_QuatNorm(swing);
    const float a=AngleBetween(id,twist),b=AngleBetween(id,swing);
    float keptTwist[4],keptSwing[4];
    BlendInto(keptTwist,id,twist,a>twistMost?twistMost/a:1.f);
    const float sizeOf=std::sqrt(swing[0]*swing[0]+swing[1]*swing[1]+swing[2]*swing[2]);
    float over=0.f;
    if(sizeOf>1e-5f) {
        const float ux=swing[0]/sizeOf/nodMost,uy=swing[1]/sizeOf/rollMost,uz=swing[2]/sizeOf/nodMost;
        over=b*std::sqrt(ux*ux+uy*uy+uz*uz);
    }
    BlendInto(keptSwing,id,swing,over>1.f?1.f/over:1.f);
    VRIK_QuatMul(keptSwing,keptTwist,q);VRIK_QuatNorm(q);
}
// How the hand has turned since it closed, as far as a wrist goes along.
void HeldTurn(const float* turn,float* out) {
    const float id[4]={0,0,0,1};
    const float angle=AngleBetween(id,turn);
    BlendInto(out,id,turn,angle>kTurnMost?kTurnMost/angle:1.f);
}
// An arm or a leg.
void SolveLimb(Grip& g,uint8_t* buf,const float* t,Touched& touched) {
    if(!g.bounded)return;
    Tr up,mid,end;
    if(!ModelOf(g,buf,g.upper,up) || !ModelOf(g,buf,g.mid,mid) || !ModelOf(g,buf,g.end,end))return;
    float endWas[4];std::copy_n(end.q,4,endWas);
    const float la=Apart(mid.p,up.p),lb=Apart(end.p,mid.p);
    if(la<1e-3f || lb<1e-3f)return;
    // THE FALLOFF: past the limb's stretch the shoulder is brought along so far as it goes, and the spine less and less.
    const float stretch=(la+lb)*kStretch;
    if(g.jointCount>0 && Apart(t,up.p)>stretch) {
        const float d=Apart(t,up.p);
        const float shoulder[3]={t[0]+(up.p[0]-t[0])*stretch/d,t[1]+(up.p[1]-t[1])*stretch/d,t[2]+(up.p[2]-t[2])*stretch/d};
        ReachWith(g,buf,g.upper,shoulder,touched);
        if(!ModelOf(g,buf,g.upper,up) || !ModelOf(g,buf,g.mid,mid) || !ModelOf(g,buf,g.end,end))return;
    }
    // Solve with the rig's actual hinge. An unconstrained two-bone arc can fold
    // a knee sideways, and keeping the old world hand rotation breaks the wrist.
    const float toT[3]={t[0]-up.p[0],t[1]-up.p[1],t[2]-up.p[2]};
    float d=std::sqrt(Dot(toT,toT));
    if(!std::isfinite(d))return;
    float dir[3];
    if(d<1e-5f){for(int k=0;k<3;++k)dir[k]=end.p[k]-up.p[k];if(!jointlimits::Unit(dir))return;}
    else for(int k=0;k<3;++k)dir[k]=toT[k]/d;
    const float flex=std::acos(std::clamp((d*d-la*la-lb*lb)/(2*la*lb),-1.f,1.f));
    float midLocal[4];jointlimits::Pose(g.hinge,flex,LocalR(buf,g.mid),midLocal);
    float fore[3],localEnd[3],worldEnd[3];VRIK_QuatRotateVec(midLocal,LocalT(buf,g.end),fore);
    for(int k=0;k<3;++k)localEnd[k]=LocalT(buf,g.mid)[k]+fore[k];
    VRIK_QuatRotateVec(up.q,localEnd,worldEnd);
    float aim[4],upQ[4];Arc(worldEnd,dir,aim);VRIK_QuatMul(aim,up.q,upQ);VRIK_QuatNorm(upQ);

    // Transport the animated upper bone's roll with the aim rotation. Its rig
    // hinge already chooses the bend plane. Projecting the old upper segment
    // onto the NEW target instead invents opposite poles either side of a
    // straight limb: a sub-millimetre movement could flip the elbow 180 degrees.
    const bool leg=g.part==LegL||g.part==LegR;
    Tr parent;
    if(!ModelOf(g,buf,g.parents[g.upper],parent))return;
    float inv[4],local[4],bounded[4];Conj(parent.q,inv);VRIK_QuatMul(inv,upQ,local);
    if(!jointlimits::Clamp(g.upperRest,local,LocalT(buf,g.mid),leg?jointlimits::Hip:jointlimits::Shoulder,bounded))return;
    if(!touched.Note(buf,g.upper)||!touched.Note(buf,g.mid))return;
    std::copy_n(bounded,4,LocalR(buf,g.upper));std::copy_n(midLocal,4,LocalR(buf,g.mid));
    if(!ModelOf(g,buf,g.mid,mid))return;

    // Keep the requested world turn only as far as the wrist/ankle can follow
    // its NEW parent. Position and orientation share the constrained chain.
    float endQ[4];std::copy_n(endWas,4,endQ);
    if(!leg) {
        float turn[4];HeldTurn(g.turn,turn);
        VRIK_QuatMul(turn,endWas,endQ);VRIK_QuatNorm(endQ);
    }
    Conj(mid.q,inv);VRIK_QuatMul(inv,endQ,local);
    if(jointlimits::Clamp(g.endRest,local,LocalT(buf,g.end),leg?jointlimits::Ankle:jointlimits::Wrist,bounded)&&touched.Note(buf,g.end))
        std::copy_n(bounded,4,LocalR(buf,g.end));
    if(ModelOf(g,buf,g.end,end))g.miss=Apart(t,end.p);
}
// The head or the chest: a chain, no bones of its own.
void SolveChain(Grip& g,uint8_t* buf,const float* t,Touched& touched) {
    Tr was;
    if(!ModelOf(g,buf,g.end,was))return;
    float endWas[4];std::copy_n(was.q,4,endWas);
    Tr below;
    const bool hasBelow=ModelOf(g,buf,g.parents[g.end],below);
    BendChain(g,buf,g.end,t,touched);
    Tr now;
    if(ModelOf(g,buf,g.end,now))g.miss=Apart(t,now.p);
    if(g.part==PartHead) {
        const float id[4]={0,0,0,1};
        float inv[4],bend[4],half[4],turn[4],total[4],q[4];
        // half of how the neck bent it, so a head brought down looks down, and most of how the hand turned; all of that no further than a neck goes
        Conj(endWas,inv);VRIK_QuatMul(now.q,inv,bend);VRIK_QuatNorm(bend);
        BlendInto(half,id,bend,kHeadBend);
        BlendInto(turn,id,g.turn,kHeadFollow);
        VRIK_QuatMul(turn,half,total);VRIK_QuatNorm(total);
        if(hasBelow) {
            float axis[3]={was.p[0]-below.p[0],was.p[1]-below.p[1],was.p[2]-below.p[2]};
            const float size=std::sqrt(Dot(axis,axis));
            if(size>1e-4f) {
                for(auto& v:axis)v/=size;
                LimitTurn(total,axis,kHeadTwist,kHeadNod,kHeadRoll);
            }
        }
        VRIK_QuatMul(total,endWas,q);VRIK_QuatNorm(q);
        // AND NO FURTHER FROM THE CHEST, whatever the pose it started from had in it: the limit above is on what this pass adds.
        Tr chest;
        if(hasBelow && g.chest>=0 && ModelOf(g,buf,g.chest,chest)) {
            float axis[3]={was.p[0]-below.p[0],was.p[1]-below.p[1],was.p[2]-below.p[2]};
            const float size=std::sqrt(Dot(axis,axis));
            if(size>1e-4f) {
                for(auto& v:axis)v/=size;
                float carried[4],away[4],kept[4];
                VRIK_QuatMul(chest.q,g.rest,carried);VRIK_QuatNorm(carried);
                Conj(carried,inv);VRIK_QuatMul(q,inv,away);VRIK_QuatNorm(away);
                LimitTurn(away,axis,kHeadTwist,kHeadNod,kHeadRoll);
                VRIK_QuatMul(away,carried,kept);VRIK_QuatNorm(kept);
                std::copy_n(kept,4,q);
            }
        }
        SetModelRot(g,buf,g.end,q,touched);
    }
}
bool Ancestor(const Grip& g,int bone,int descendant){
    for(int depth=0;descendant>=0&&uint32_t(descendant)<g.bones&&depth<kMostChain;++depth,descendant=g.parents[descendant])
        if(bone==descendant)return true;
    return false;
}
bool AnchorPose(const Grip& g,uint8_t* buf,float* point,float* rotation){
    Tr at;if(!ModelOf(g,buf,g.anchor.bone,at))return false;
    float offset[3];VRIK_QuatRotateVec(at.q,g.anchor.local,offset);
    for(int k=0;k<3;++k)point[k]=at.p[k]+offset[k];
    VRIK_QuatMul(at.q,g.anchor.localQ,rotation);VRIK_QuatNorm(rotation);return true;
}
bool BoundLocal(const Grip& g,uint8_t* buf,int bone,const float* wanted,Touched& touched,float* out){
    const float* native=touched.Note(buf,bone);if(!native)return false;
    const bool leg=g.part==LegL||g.part==LegR;
    const int index=bone==g.upper?0:bone==g.mid?1:bone==g.end?2:-1;
    const float* winding=index>=0&&g.anchor.on&&g.anchor.twist[index].valid?&g.anchor.twist[index].angle:nullptr;
    if(bone==g.upper)return jointlimits::Clamp(g.upperRest,wanted,LocalT(buf,g.mid),leg?jointlimits::Hip:jointlimits::Shoulder,out,winding);
    if(bone==g.end)return jointlimits::Clamp(g.endRest,wanted,LocalT(buf,g.end),leg?jointlimits::Ankle:jointlimits::Wrist,out,winding);
    if(bone==g.mid){
        float direction[3],cross[3];VRIK_QuatRotateVec(wanted,g.hinge.outgoing,direction);
        Cross(g.hinge.incoming,direction,cross);
        const float flex=std::atan2(Dot(cross,g.hinge.axis),Dot(g.hinge.incoming,direction))*g.hinge.sign;
        jointlimits::Pose(g.hinge,flex,wanted,out,winding);return true;
    }
    for(int i=0;i<g.jointCount;++i)if(g.joints[i].bone==bone){
        const float angle=AngleBetween(native,wanted);
        BlendInto(out,native,wanted,angle>g.joints[i].most?g.joints[i].most/angle:1.f);return true;
    }
    return false;
}
bool BoundModel(const Grip& g,uint8_t* buf,int bone,const float* wanted,Touched& touched){
    Tr parent;if(!ModelOf(g,buf,g.parents[bone],parent))return false;
    float inverse[4],local[4],bounded[4];Conj(parent.q,inverse);VRIK_QuatMul(inverse,wanted,local);VRIK_QuatNorm(local);
    if(!BoundLocal(g,buf,bone,local,touched,bounded))return false;
    std::copy_n(bounded,4,LocalR(buf,bone));return true;
}
// An actual wrist-to-bone attachment, not a fixed offset to the limb's distal
// endpoint. Moving a forearm grip must solve that point even when the elbow
// bends; the wrist/foot can then continue to move naturally beyond it.
void StopAnchor(Grip& g);
bool SeedAnchor(Grip& g,uint8_t* buf){
    auto& a=g.anchor;
    if(!g.bounded||!a.on||!Ancestor(g,g.upper,a.bone))return false;
    if(!a.seeded){
        Tr bone;if(!ModelOf(g,buf,a.bone,bone))return false;
        // A mismatched entity/model basis must never create a metre-long lever
        // on a limb. A normal capsule grip is close to its selected bone.
        if(Apart(a.capture,bone.p)>.8f){
            if(a.live){Log("[npcgrip] rejected anchor %.3f m from bone %d\n",Apart(a.capture,bone.p),a.bone);StopAnchor(g);g.on=false;}
            return false;
        }
        float inverse[4],delta[3];Conj(bone.q,inverse);
        for(int k=0;k<3;++k)delta[k]=a.capture[k]-bone.p[k];VRIK_QuatRotateVec(inverse,delta,a.local);
        VRIK_QuatMul(inverse,a.captureQ,a.localQ);VRIK_QuatNorm(a.localQ);a.seeded=true;
        if(a.live){
            for(int k=0;k<3;++k)delta[k]=a.capsuleSeed[k]-bone.p[k];VRIK_QuatRotateVec(inverse,delta,a.capsuleLocal);
            VRIK_QuatMul(inverse,a.capsuleSeedQ,a.capsuleLocalQ);VRIK_QuatNorm(a.capsuleLocalQ);
        }
    }
    return true;
}
void SolveAnchor(Grip& g,uint8_t* buf,Touched& touched){
    if(!SeedAnchor(g,buf))return;
    auto& a=g.anchor;
    float inverse[4],wanted[4];Conj(a.localQ,inverse);VRIK_QuatMul(g.turn,inverse,wanted);VRIK_QuatNorm(wanted);
    // Observe each external requested twist once, before CCD changes any parent.
    // Iterative trial poses are not new tracking input and must not wind up the
    // history. The held bone follows the controller; other joints keep their
    // native animation's requested twist.
    const int limb[3]{g.upper,g.mid,g.end};
    for(int i=0;i<3;++i){
        const int bone=limb[i];float requested[4];std::copy_n(LocalR(buf,bone),4,requested);
        if(bone==a.bone){Tr parent;if(!ModelOf(g,buf,g.parents[bone],parent))return;
            float inv[4];Conj(parent.q,inv);VRIK_QuatMul(inv,wanted,requested);VRIK_QuatNorm(requested);}
        const float* reference=i==0?g.upperRest:i==1?g.hinge.rest:g.endRest;
        const float* axis=i==0?LocalT(buf,g.mid):i==1?g.hinge.outgoing:LocalT(buf,g.end);
        float angle;if(!jointlimits::ReferenceTwist(reference,requested,axis,angle))return;a.twist[i].Observe(angle);
    }
    int chain[kMostTouched],count=0;
    for(int bone:{g.end,g.mid,g.upper})if(Ancestor(g,bone,a.bone))chain[count++]=bone;
    for(int i=g.jointCount-1;i>=0;--i)if(Ancestor(g,g.joints[i].bone,a.bone)&&count<kMostTouched)chain[count++]=g.joints[i].bone;
    // Establish each limit's native reference once before any CCD pass.
    for(int i=0;i<count;++i)touched.Note(buf,chain[i]);
    for(int bone:{g.upper,g.mid,g.end}){float bounded[4];if(BoundLocal(g,buf,bone,LocalR(buf,bone),touched,bounded))std::copy_n(bounded,4,LocalR(buf,bone));}
    BoundModel(g,buf,a.bone,wanted,touched);
    float best[kMostTouched][4]{};float bestCost=INFINITY;
    const auto save=[&](){
        float p[3],q[4];if(!AnchorPose(g,buf,p,q))return INFINITY;
        const float distance=Apart(p,g.target),angle=AngleBetween(q,g.turn);
        // Position is the hard contact goal. Orientation selects among nearby
        // solutions and yields to the anatomical limits when both cannot fit.
        const float cost=distance*distance+angle*angle*1e-6f;
        if(cost<bestCost){bestCost=cost;for(int i=0;i<touched.count;++i)std::copy_n(LocalR(buf,touched.bone[i]),4,best[i]);}
        return distance;
    };
    save();
    for(int pass=0;pass<32;++pass){
        const float before=bestCost;
        for(int i=0;i<count;++i){
            Tr joint;float p[3],q[4];if(!ModelOf(g,buf,chain[i],joint)||!AnchorPose(g,buf,p,q))return;
            float from[3],to[3],arc[4],model[4];for(int k=0;k<3;++k){from[k]=p[k]-joint.p[k];to[k]=g.target[k]-joint.p[k];}
            Arc(from,to,arc);VRIK_QuatMul(arc,joint.q,model);VRIK_QuatNorm(model);
            BoundModel(g,buf,chain[i],model,touched);save();
        }
        float p[3],q[4];AnchorPose(g,buf,p,q);
        if(Apart(p,g.target)<.00025f || before-bestCost<1e-11f)break;
    }
    if(std::isfinite(bestCost))for(int i=0;i<touched.count;++i)std::copy_n(best[i],4,LocalR(buf,touched.bone[i]));
    AnchorPose(g,buf,a.point,a.rotation);g.miss=Apart(a.point,g.target);
}
void Solve(Grip& g,uint8_t* buf,float weight) {
    // Where the grip took it is where it is: the slots the script goes by are not quite the bones.
    if(!g.seeded && !g.anchor.on) {
        Tr end;
        if(!ModelOf(g,buf,g.end,end))return;
        for(int k=0;k<3;++k)g.offset[k]=end.p[k]-g.target[k];
        Tr chest;
        if(g.chest>=0 && ModelOf(g,buf,g.chest,chest)) {
            float inv[4];
            Conj(chest.q,inv);VRIK_QuatMul(inv,end.q,g.rest);VRIK_QuatNorm(g.rest);
        }
        g.seeded=true;
    }
    const float t[3]={g.target[0]+g.offset[0],g.target[1]+g.offset[1],g.target[2]+g.offset[2]};
    Touched touched;
    if(g.anchor.on)SolveAnchor(g,buf,touched);
    else if(g.upper>=0)SolveLimb(g,buf,t,touched);
    else SolveChain(g,buf,t,touched);
    if(weight<.999f)for(int i=0;i<touched.count;++i) {
        float* now=LocalR(buf,touched.bone[i]);
        BlendInto(now,touched.was[i],now,weight);
    }
    if(g.upper>=0){
        const int bone[3]{g.upper,g.mid,g.end};bool ok=true;
        for(int i=0;i<3;++i){Tr model;if(!ModelOf(g,buf,bone[i],model)){ok=false;break;}
            std::copy_n(model.p,3,g.solved.point[i]);std::copy_n(LocalR(buf,bone[i]),4,g.solved.local[i]);}
        g.solved.at=ok?GetTickCount64():0;
        if(ok){
            if(g.anchor.on){AnchorPose(g,buf,g.anchor.point,g.anchor.rotation);g.miss=Apart(g.target,g.anchor.point);}
            else g.miss=Apart(t,g.solved.point[2]);
        }
    }
}
// Keep the native input as well as the solved output. The same animation pose
// can be reused, or copied to another buffer, while a controller keeps moving.
// Re-solve from its native input on a new command; never add a turn to our own
// output, and never wait for the idle NPC's next animation to move the grip.
bool SolveOnce(Grip& g,uint8_t* buf,float weight) {
    DonePose* slot=nullptr;DonePose* previous=nullptr;
    for(auto& d:g.done) {
        if(d.buf==buf)slot=&d;
        bool same=d.count>0;
        for(int i=0;i<d.count&&same;++i)same=std::memcmp(buf+d.bone[i]*48,d.after[i],48)==0;
        if(same){previous=&d;break;}
    }
    if(previous){
        const bool commandSame=std::memcmp(previous->target,g.target,sizeof(g.target))==0&&
            std::memcmp(previous->turn,g.turn,sizeof(g.turn))==0&&previous->weight==weight;
        if(commandSame){++g.again;if(g.bounded)g.solved.at=GetTickCount64();return false;}
        for(int i=0;i<previous->count;++i)std::memcpy(buf+previous->bone[i]*48,previous->before[i],48);
    }
    if(!slot) {
        slot=&g.done[g.doneNext];
        g.doneNext=(g.doneNext+1)%4;
        slot->buf=buf;
    }
    slot->count=0;
    for(int b=g.end;b>=0&&uint32_t(b)<g.bones&&slot->count<kMostChain;b=g.parents[b]){
        const int i=slot->count++;slot->bone[i]=b;std::memcpy(slot->before[i],buf+b*48,48);
    }
    Solve(g,buf,weight);
    for(int i=0;i<slot->count;++i)std::memcpy(slot->after[i],buf+slot->bone[i]*48,48);
    std::copy_n(g.target,3,slot->target);std::copy_n(g.turn,4,slot->turn);slot->weight=weight;
    return true;
}
void RestoreRetainedRotations(const NativePose& pose,uint8_t* buf,uint32_t slots){
    // A graph pass can refresh only some channels of this buffer. A changed
    // translation, scale, or neighboring joint does not turn our retained
    // rotation into native animation. Restore only rotations we changed and
    // can still identify exactly; preserve every freshly written channel.
    for(int i=0;i<pose.count;++i){
        const int bone=pose.bone[i];if(uint32_t(bone)>=slots)continue;
        const auto* before=pose.before[i]+16;const auto* after=pose.after[i]+16;
        if(std::memcmp(before,after,16)!=0&&std::memcmp(buf+bone*48+16,after,16)==0)
            std::memcpy(buf+bone*48+16,before,16);
    }
}
NativePose* BeginNativePose(uintptr_t descriptor,uint8_t* buf,uint32_t slots){
    bool applies=false;
    for(const auto& g:grips)applies|=g.on&&descriptor==g.poseOwner&&slots>=g.bones;
    if(!applies)return nullptr;
    NativePose* own=nullptr;NativePose* previous=nullptr;
    for(auto& p:nativePoses){
        if(p.descriptor!=descriptor)continue;
        if(p.buf==buf)own=&p;
        bool same=p.count>0;
        for(int i=0;i<p.count&&same;++i)same=uint32_t(p.bone[i])<slots&&std::memcmp(buf+p.bone[i]*48,p.after[i],48)==0;
        if(same)previous=&p;
    }
    if(previous)for(int i=0;i<previous->count;++i)std::memcpy(buf+previous->bone[i]*48,previous->before[i],48);
    else if(own)RestoreRetainedRotations(*own,buf,slots);
    if(!own){
        for(int tries=0;tries<8;++tries){own=&nativePoses[nextNativePose];nextNativePose=(nextNativePose+1)%8;if(!own->retired)break;}
    }
    own->retired=false;retiredOwners[own-nativePoses].store(0,std::memory_order_release);
    own->owner.Reset();own->ownerKey=0;
    own->descriptor=descriptor;own->buf=buf;own->count=0;
    for(const auto& g:grips){
        if(!g.on||descriptor!=g.poseOwner||slots<g.bones)continue;
        if(g.side>=0&&g.entityKey){own->owner=gripEntities[g.side];own->ownerKey=g.entityKey;}
        int depth=0;
        for(int b=g.end;b>=0&&uint32_t(b)<g.bones&&depth++<kMostChain;b=g.parents[b]){
            bool known=false;for(int i=0;i<own->count;++i)known|=own->bone[i]==b;if(known)continue;
            if(own->count>=2*kMostChain)break;
            const int i=own->count++;own->bone[i]=b;std::memcpy(own->before[i],buf+b*48,48);
        }
    }
    return own;
}
bool EntityFrameGuarded(RED4ext::IScriptable* object,float* p,float* q){
    __try {
        auto* entity=static_cast<RED4ext::ent::Entity*>(object);
        if(!entity||entity->status!=RED4ext::EntityStatus::Attached||!entity->transformComponent)return false;
        const auto first=entity->transformComponent->worldTransform;
        const auto second=entity->transformComponent->worldTransform;
        if(std::memcmp(&first,&second,sizeof(first))!=0)return false;
        const auto position=first.Position.AsVector3();p[0]=position.X;p[1]=position.Y;p[2]=position.Z;
        q[0]=first.Orientation.i;q[1]=first.Orientation.j;q[2]=first.Orientation.k;q[3]=first.Orientation.r;
        return std::isfinite(p[0])&&std::isfinite(p[1])&&std::isfinite(p[2])&&jointlimits::Normalize(q);
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
bool EntityAttachedGuarded(RED4ext::IScriptable* object){
    __try {return object&&static_cast<RED4ext::ent::Entity*>(object)->status==RED4ext::EntityStatus::Attached;}
    __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
void ClearRetired(NativePose& p){p.retired=false;retiredOwners[&p-nativePoses].store(0,std::memory_order_release);p.owner.Reset();p.ownerKey=0;p.count=0;}
void RestoreRetired(uintptr_t descriptor,uint8_t* buf,uint32_t slots){
    for(auto& p:nativePoses){
        if(!p.retired)continue;
        if(p.ownerKey){const auto owner=p.owner.Lock();if(!owner||!EntityAttachedGuarded(owner.instance)){ClearRetired(p);continue;}}
        if(!descriptor||!buf||p.descriptor!=descriptor)continue;
        bool same=p.count>0;
        for(int i=0;i<p.count&&same;++i)same=uint32_t(p.bone[i])<slots&&std::memcmp(buf+p.bone[i]*48,p.after[i],48)==0;
        if(same){for(int i=0;i<p.count;++i)std::memcpy(buf+p.bone[i]*48,p.before[i],48);}
        else if(p.buf==buf){RestoreRetainedRotations(p,buf,slots);ClearRetired(p);} // fresh or partially refreshed native pose
    }
}
void StopAnchor(Grip& g){
    if(g.anchor.side>=0&&g.anchor.side<2){
        auto& a=g.anchor;
        cvr::physhands::PersonAttachment ended{};ended.owner=a.owner;ended.serial=a.serial;ended.release=true;
        if(!ended.serial){cvr::physhands::PersonContact c{};if(cvr::physhands::ReadPersonContact(a.side,c)&&c.owner==a.owner)ended.serial=c.serial;}
        cvr::physhands::PublishPersonAttachment(a.side,ended);
    }
    g.anchor.live=false;g.letting=true;
    if(!g.let)g.let=GetTickCount64();
    if(g.anchor.on&&!g.anchor.seeded){g.on=false;if(g.side>=0)gripEntities[g.side].Reset();}
}
void ReapGrips(uintptr_t currentOwner,uint32_t slots){
    const auto now=GetTickCount64();
    for(auto& g:grips){
        if(!g.on)continue;
        bool detached=false;
        if(g.side>=0&&g.entityKey){const auto owner=gripEntities[g.side].Lock();detached=!owner||!EntityAttachedGuarded(owner.instance);}
        if(!g.letting&&now-g.asked<=kUnaskedMs&&!detached)continue;
        if(!g.let)g.let=g.letting?now:g.asked+kUnaskedMs;
        if(!g.letting)StopAnchor(g);
        const bool thisPose=currentOwner==g.poseOwner&&slots>=g.bones;
        if(!detached&&(now-g.let<uint64_t(kBlendOut*1000)||thisPose))continue;
        // No matching pose callbacks after a menu/streaming/LOD transition:
        // free the hand now and restore any reused output when that actor next
        // submits a pose. Never dereference an old bone-buffer pointer here.
        for(auto& p:nativePoses)if(p.count&&p.descriptor==g.poseOwner){
            if(detached)ClearRetired(p);
            else{p.retired=true;retiredOwners[&p-nativePoses].store(p.descriptor,std::memory_order_release);}
        }
        StopAnchor(g);g.on=false;if(g.side>=0)gripEntities[g.side].Reset();
        Log("[npcgrip] released inactive %s (%s)\n",kPartNames[g.part],detached?"entity detached":"pose stopped arriving");
    }
}
bool RefreshAnchor(Grip& g){
    auto& a=g.anchor;if(!a.live)return true;
    if(g.letting||GetTickCount64()-g.asked>kUnaskedMs){StopAnchor(g);return a.seeded;}
    cvr::physhands::PersonContact contact{};
    const auto entity=gripEntities[a.side].Lock();
    const bool entityReady=entity&&EntityFrameGuarded(entity.instance,a.entityP,a.entityQ);
    const bool contactReady=cvr::physhands::ReadPersonContact(a.side,contact)&&contact.owner==a.owner;
    const bool controlReady=cvr::physhands::ReadGripControl(a.side,a.control);
    if(!entityReady||!contactReady||!controlReady){
        Log("[npcgrip] bone drive unavailable: owner %d entity %d contact %d (owner %d) control %d\n",a.owner,entityReady,contactReady,contact.owner,controlReady);
        StopAnchor(g);return a.seeded;
    }
    if(!contact.ready&&!a.seeded)return false; // let the initial finger seat settle first
    float inverse[4],delta[3];Conj(a.entityQ,inverse);
    if(a.serial!=contact.serial||!a.seeded){
        a.serial=contact.serial;a.seeded=false;std::memset(g.done,0,sizeof(g.done));g.doneNext=0;
        for(int k=0;k<3;++k)delta[k]=contact.position[k]-a.entityP[k];VRIK_QuatRotateVec(inverse,delta,a.capture);
        VRIK_QuatMul(inverse,contact.rotation,a.captureQ);VRIK_QuatNorm(a.captureQ);
        for(int k=0;k<3;++k)delta[k]=contact.capsule[k]-a.entityP[k];VRIK_QuatRotateVec(inverse,delta,a.capsuleSeed);
        VRIK_QuatMul(inverse,contact.capsuleRotation,a.capsuleSeedQ);VRIK_QuatNorm(a.capsuleSeedQ);a.radius=contact.radius;a.half=contact.half;
        float controlInverse[4];Conj(a.control.rotation,controlInverse);
        for(int k=0;k<3;++k)delta[k]=contact.position[k]-a.control.position[k];VRIK_QuatRotateVec(controlInverse,delta,a.controlOffset);
        VRIK_QuatMul(controlInverse,contact.rotation,a.controlRotation);VRIK_QuatNorm(a.controlRotation);
        Log("[npcgrip] bone contact ready: owner %d bone %d serial %llu wrist %.3f %.3f %.3f reach %.3f\n",a.owner,a.bone,
            static_cast<unsigned long long>(a.serial),contact.position[0],contact.position[1],contact.position[2],a.control.reach);
    }
    VRIK_QuatRotateVec(a.control.rotation,a.controlOffset,a.worldGoal);
    for(int k=0;k<3;++k)a.worldGoal[k]+=a.control.position[k];
    if(!cvr::physhands::LimitGripReach(a.control,a.worldGoal)){StopAnchor(g);return a.seeded;}
    for(int k=0;k<3;++k)delta[k]=a.worldGoal[k]-a.entityP[k];VRIK_QuatRotateVec(inverse,delta,g.target);
    float worldQ[4];VRIK_QuatMul(a.control.rotation,a.controlRotation,worldQ);VRIK_QuatMul(inverse,worldQ,g.turn);VRIK_QuatNorm(g.turn);
    return true;
}
void PublishAnchor(Grip& g,uint8_t* buf){
    auto& a=g.anchor;if(!a.live||!a.seeded)return;
    Tr bone;if(!ModelOf(g,buf,a.bone,bone))return;
    cvr::physhands::PersonAttachment result{};result.owner=a.owner;result.serial=a.serial;result.on=true;
    VRIK_QuatRotateVec(a.entityQ,a.point,result.position);for(int k=0;k<3;++k)result.position[k]+=a.entityP[k];
    std::copy_n(result.position,3,a.worldPoint);VRIK_QuatMul(a.entityQ,a.rotation,result.rotation);VRIK_QuatNorm(result.rotation);
    std::copy_n(result.rotation,4,a.worldRotation);
    float localCap[3],modelCap[3],modelQ[4];VRIK_QuatRotateVec(bone.q,a.capsuleLocal,localCap);
    for(int k=0;k<3;++k)modelCap[k]=bone.p[k]+localCap[k];VRIK_QuatRotateVec(a.entityQ,modelCap,result.capsule);
    for(int k=0;k<3;++k)result.capsule[k]+=a.entityP[k];
    VRIK_QuatMul(bone.q,a.capsuleLocalQ,modelQ);VRIK_QuatMul(a.entityQ,modelQ,result.capsuleRotation);VRIK_QuatNorm(result.capsuleRotation);
    result.radius=a.radius;result.half=a.half;result.controlAt=a.control.at;
    if(Apart(result.position,a.control.shoulder)>a.control.reach+.03f){result.on=false;result.release=true;}
    cvr::physhands::PublishPersonAttachment(a.side,result);
}
void ApplyInner(void* a2) {
    void* poseDesc=reinterpret_cast<void**>(a2)[7];
    if(!poseDesc)return;
    auto* boneBuf=reinterpret_cast<uint8_t**>(poseDesc)[0];
    const uintptr_t owner=reinterpret_cast<uintptr_t>(poseDesc);
    if(!boneBuf)return;
    const uint32_t capacity=*reinterpret_cast<uint32_t*>(static_cast<uint8_t*>(poseDesc)+8);
    const uint32_t slots=*reinterpret_cast<uint32_t*>(static_cast<uint8_t*>(poseDesc)+12);
    if(slots>capacity)return;
    NativePose* native=BeginNativePose(owner,boneBuf,slots);
    bool ready[2]{true,true};
    for(int side=0;side<2;++side){auto& g=grips[side];
        if(!g.on||owner!=g.poseOwner||slots<g.bones)continue;
        if(g.anchor.live)ready[side]=RefreshAnchor(g);
        // Simultaneous contacts share the same source pose. The first hand's
        // solve must not change where the second contact is captured on a bone.
        if(ready[side]&&g.anchor.on)ready[side]=SeedAnchor(g,boneBuf);
    }
    // What moves the trunk first: a limb hangs from it.
    for(int order=0;order<2;++order)for(auto& g:grips) {
        if(!g.on || owner!=g.poseOwner || slots<g.bones)continue;
        if((g.upper<0)!=(order==0))continue;
        if(!ready[&g-grips])continue;
        const uint64_t tick=GetTickCount64();
        float weight;bool finished=false;
        if(g.letting || tick-g.asked>kUnaskedMs) {
            if(!g.let)g.let=tick;
            weight=1.f-(tick-g.let)*.001f/kBlendOut;
            if(weight<=0){weight=0;finished=true;}
        } else {
            weight=(tick-g.begun)*.001f/kBlendIn;
            if(weight>1)weight=1;
        }
        if(SolveOnce(g,boneBuf,weight))++g.passes;
        if(finished){StopAnchor(g);g.on=false;if(g.side>=0)gripEntities[g.side].Reset();Log("[npcgrip] let go of the %s after %u passes, and %u more on a pose already solved\n",kPartNames[g.part],g.passes,g.again);}
    }
    // The other hand can still move a shared ancestor (or the same bone).
    // Publish attachments only from the final combined pose, never from the
    // intermediate pose produced by the first grip.
    for(auto& g:grips){
        if(!g.on||owner!=g.poseOwner||slots<g.bones)continue;
        if(g.anchor.on&&g.anchor.seeded){
            AnchorPose(g,boneBuf,g.anchor.point,g.anchor.rotation);g.miss=Apart(g.target,g.anchor.point);
            const int bone[3]{g.upper,g.mid,g.end};
            for(int i=0;i<3;++i){Tr model;if(ModelOf(g,boneBuf,bone[i],model)){std::copy_n(model.p,3,g.solved.point[i]);std::copy_n(LocalR(boneBuf,bone[i]),4,g.solved.local[i]);}}
        }
        PublishAnchor(g,boneBuf);
    }
    if(native)for(int i=0;i<native->count;++i)std::memcpy(native->after[i],boneBuf+native->bone[i]*48,48);
}
template<class RigT> int BoneNamed(RigT* rig,const char* name) {
    const RED4ext::CName wanted(name);
    for(uint32_t i=0;i<rig->boneNames.Size();++i)if(rig->boneNames[i]==wanted)return int(i);
    return -1;
}
bool Begin(int side,int part,RED4ext::ent::Entity* entity,const float* target) {
    auto* object=FindAnimatedObjectOf(entity);
    if(!object || !object->metaRig)return false;
    auto* rig=object->metaRig;
    if(rig->parentIndeces.Size()!=rig->boneNames.Size() || rig->boneNames.Size()>VRIK_MAX_BONES)return false;
    const auto* base=reinterpret_cast<uint8_t*>(object);
    const auto ownerA=*reinterpret_cast<const uintptr_t*>(base+8),ownerB=*reinterpret_cast<const uintptr_t*>(base+0x18);
    Grip& g=grips[side];
    if(g.on)return false;
    g.side=side;g.entityKey=reinterpret_cast<uintptr_t>(entity);
    g.trackA=ownerA?*reinterpret_cast<uintptr_t*>(ownerA+0x40):0;
    g.trackB=ownerB?*reinterpret_cast<uintptr_t*>(ownerB+0x18):0;
    g.poseOwner=ownerB;
    if(!g.poseOwner)return false;
    g.bones=rig->boneNames.Size();
    for(uint32_t i=0;i<g.bones;++i)g.parents[i]=rig->parentIndeces[i];
    g.part=part;
    g.bounded=false;
    g.solved={};
    g.anchor={};
    g.upper=g.mid=g.end=-1;
    g.jointCount=0;
    g.chest=-1;
    std::memset(g.done,0,sizeof(g.done));
    g.doneNext=0;g.again=0;
    const auto add=[&](const char* name,float share,float most) {
        const int b=BoneNamed(rig,name);
        if(b>=0 && g.jointCount<kMostJoints)g.joints[g.jointCount++]=Joint{b,share,most};
    };
    if(part==ArmL || part==ArmR || part==LegL || part==LegR) {
        const char* const names[4][3]={{"LeftArm","LeftForeArm","LeftHand"},{"RightArm","RightForeArm","RightHand"},{"LeftUpLeg","LeftLeg","LeftFoot"},{"RightUpLeg","RightLeg","RightFoot"}};
        g.upper=BoneNamed(rig,names[part][0]);g.mid=BoneNamed(rig,names[part][1]);g.end=BoneNamed(rig,names[part][2]);
        if(g.upper<0 || g.mid<0 || g.end<0) {
            Log("[npcgrip] this rig has no %s (%d %d %d of %u bones)\n",kPartNames[part],g.upper,g.mid,g.end,g.bones);
            return false;
        }
        if(g.parents[g.mid]!=g.upper || g.parents[g.end]!=g.mid) {
            Log("[npcgrip] bones stand between those of the %s: not solved\n",kPartNames[part]);
            return false;
        }
        if(rig->boneTransforms.Size()!=g.bones)return false;
        const auto& upper=rig->boneTransforms[g.upper];const auto& mid=rig->boneTransforms[g.mid];const auto& end=rig->boneTransforms[g.end];
        std::memcpy(g.upperRest,&upper.Rotation,16);std::memcpy(g.endRest,&end.Rotation,16);
        g.bounded=jointlimits::Normalize(g.upperRest)&&jointlimits::Normalize(g.endRest)&&
            jointlimits::Configure(g.hinge,reinterpret_cast<const float*>(&mid.Rotation),
                reinterpret_cast<const float*>(&mid.Translation),reinterpret_cast<const float*>(&end.Translation),part==LegL||part==LegR);
        if(!g.bounded){
            static uint64_t lastUnsupported=0;const auto now=GetTickCount64();
            if(now-lastUnsupported>3000){lastUnsupported=now;
                const auto* m=reinterpret_cast<const float*>(&mid);const auto* e=reinterpret_cast<const float*>(&end);
                Log("[npcgrip] unsupported reference hinge for %s: mid p(%.5f %.5f %.5f) q(%.5f %.5f %.5f %.5f), end p(%.5f %.5f %.5f); not solved\n",
                    kPartNames[part],m[0],m[1],m[2],m[4],m[5],m[6],m[7],e[0],e[1],e[2]);}
            return false;
        }
        if(part==ArmL || part==ArmR) {
            add("Spine",.08f,.08f);add("Spine1",.12f,.12f);add("Spine2",.18f,.15f);add("Spine3",.25f,.2f);
            add(part==ArmL?"LeftShoulder":"RightShoulder",.6f,.45f);
        }
    } else if(part==PartHead) {
        g.end=BoneNamed(rig,"Head");
        add("Spine",.02f,.02f);add("Spine1",.03f,.03f);add("Spine2",.05f,.05f);add("Spine3",.08f,.07f);add("Neck",.5f,.25f);add("Neck1",.7f,.3f);
        g.chest=BoneNamed(rig,"Spine3");
    } else {
        g.end=BoneNamed(rig,"Spine3");
        add("Spine",.3f,.3f);add("Spine1",.5f,.35f);add("Spine2",.7f,.4f);
    }
    if(g.end<0 || (g.upper<0 && g.jointCount==0)) {
        Log("[npcgrip] this rig has no %s (%u bones)\n",kPartNames[part],g.bones);
        return false;
    }
    std::copy_n(target,3,g.target);
    g.turn[0]=g.turn[1]=g.turn[2]=0;g.turn[3]=1;
    const uint64_t now=GetTickCount64();
    g.begun=now;g.asked=now;g.let=0;g.letting=false;g.passes=0;g.miss=0;g.seeded=false;
    g.on=true;
    PublishActivity();
    Log("[npcgrip] holding the %s: %u bones, end %d, %d joints above it; pose owner %p reference tracks %p live tracks %p\n",
        kPartNames[part],g.bones,g.end,g.jointCount,reinterpret_cast<void*>(g.poseOwner),reinterpret_cast<void*>(g.trackA),reinterpret_cast<void*>(g.trackB));
    return true;
}
bool BeginGuarded(int side,int part,RED4ext::ent::Entity* entity,const float* target) {
    __try {return Begin(side,part,entity,target);}
    __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
}  // namespace

bool PoseArguments(void* a2,uintptr_t& owner,uintptr_t& track,uint8_t*& buf,uint32_t& slots){
    __try {if(!a2)return false;auto* desc=reinterpret_cast<void**>(a2)[7];if(!desc)return false;
        owner=reinterpret_cast<uintptr_t>(desc);buf=reinterpret_cast<uint8_t**>(desc)[0];track=reinterpret_cast<uintptr_t*>(desc)[3];
        const auto capacity=*reinterpret_cast<uint32_t*>(static_cast<uint8_t*>(desc)+8);slots=*reinterpret_cast<uint32_t*>(static_cast<uint8_t*>(desc)+12);
        if(slots>capacity){buf=nullptr;slots=0;return false;}return buf!=nullptr;}
    __except(EXCEPTION_EXECUTE_HANDLER){return false;}
}
void NpcArmGripApply(void* a2) {
    if(!anyOn&&!anyRetired.load(std::memory_order_acquire))return;
    uintptr_t owner=0,track=0;uint8_t* buf=nullptr;uint32_t slots=0;PoseArguments(a2,owner,track,buf,slots);
    bool interested=false;for(const auto& value:activeOwners)interested|=owner&&value.load(std::memory_order_acquire)==owner;
    for(const auto& value:retiredOwners)interested|=owner&&value.load(std::memory_order_acquire)==owner;
    const auto now=GetTickCount64();auto next=nextCleanupAt.load(std::memory_order_relaxed);
    const bool cleanup=now>=next&&nextCleanupAt.compare_exchange_strong(next,now+125);
    if(!interested&&!cleanup)return;
    AcquireSRWLockExclusive(&lock);
    bool scope=false;
    __try {
        if(!interested&&cvr::physhands::Tracing()){
            static unsigned reported=0;
            for(const auto& g:grips)if(g.on&&owner&&owner!=g.poseOwner&&track&&(track==g.trackA||track==g.trackB)&&reported<8){
                ++reported;Log("[npcgrip] rejected shared-track pose %p (held owner %p), track %p\n",reinterpret_cast<void*>(owner),reinterpret_cast<void*>(g.poseOwner),reinterpret_cast<void*>(track));}
        }
        ReapGrips(owner,slots);RestoreRetired(owner,buf,slots);
        if(anyOn&&owner&&buf){cvr::physhands::BeginPhysicsPoseFrame();scope=true;ApplyInner(a2);}
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        for(auto& g:grips){StopAnchor(g);if(g.side>=0)gripEntities[g.side].Reset();}
        grips[0].on=grips[1].on=false;
    }
    // Begin publishes on/anyOn under this same lock. Clearing the gate after
    // unlocking could erase a new grab between the empty check and exchange.
    PublishActivity();
    if(scope)cvr::physhands::EndPhysicsPoseFrame();
    ReleaseSRWLockExclusive(&lock);
}
void NpcArmGripPoseCompleted(void* descriptor,bool beforeModelConversion){
    if(!anyOn&&!anyRetired.load(std::memory_order_acquire))return;
    const auto owner=reinterpret_cast<uintptr_t>(descriptor);
    bool active=false;for(const auto& value:activeOwners)active|=owner&&value.load(std::memory_order_acquire)==owner;
    static std::atomic<unsigned> reported{0};const unsigned bit=beforeModelConversion?1u:2u;
    if(active&&!(reported.load(std::memory_order_relaxed)&bit)&&cvr::physhands::Tracing()&&!(reported.fetch_or(bit)&bit))
        Log("[npcgrip] completed held pose %p at %s\n",descriptor,beforeModelConversion?"local-to-model boundary":"object update return");
    void* args[8]{};args[7]=descriptor;NpcArmGripApply(args);
}
void NpcArmGripObjectUpdated(void* object){
    if(!anyOn&&!anyRetired.load(std::memory_order_acquire))return;
    // CP2077 2.5 AnimatedObject update (exe+0x1B3E00) runs its linked graph
    // controllers before returning. Each controller's pose-copy call is only
    // an intermediate result. The instance owns the completed descriptor here.
    void* descriptor=nullptr;
    __try {if(object)descriptor=*reinterpret_cast<void**>(static_cast<uint8_t*>(object)+0x18);}
    __except(EXCEPTION_EXECUTE_HANDLER){return;}
    NpcArmGripPoseCompleted(descriptor,false);
}
}  // namespace cvr::anim

// VRNpcGrip(entity, the hand that has it, the part 0 left arm 1 right arm 2 left leg 3 right leg 4 head 5 chest, where the grip has it in the entity's model space (w 0 lets go), how the hand has turned since it closed, in that
// space) -> how far the grip is from what the part can reach, in m. Below 0: it could not be held.
void VRNpcGrip(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,float* out,int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> entity;
    int32_t side=0,part=0;
    RED4ext::Vector4 target{},turn{};
    RED4ext::GetParameter(stack,&entity);
    RED4ext::GetParameter(stack,&side);
    RED4ext::GetParameter(stack,&part);
    RED4ext::GetParameter(stack,&target);
    RED4ext::GetParameter(stack,&turn);
    stack->code++;
    if(out)*out=-1.f;
    using namespace cvr::anim;
    if(side<0 || side>1 || part<0 || part>=PartCount)return;
    if(!std::isfinite(target.W))return;
    Grip& g=grips[side];
    const float t[3]={target.X,target.Y,target.Z};
    float rotation[4]{turn.X,turn.Y,turn.Z,turn.W};
    if(target.W>=.5f && (!std::isfinite(t[0])||!std::isfinite(t[1])||!std::isfinite(t[2])||!jointlimits::Normalize(rotation)))return;
    AcquireSRWLockExclusive(&lock);
    ReapGrips(0,0);
    float answer=-1.f;
    if(target.W<.5f) {
        if(g.on){StopAnchor(g);g.letting=true;}
        answer=0.f;
    } else if(g.on && !g.letting && g.part==part) {
        std::copy_n(t,3,g.target);
        std::copy_n(rotation,4,g.turn);
        g.asked=GetTickCount64();
        answer=g.miss;
    } else if(entity.instance && !g.on) {
        if(BeginGuarded(side,part,reinterpret_cast<RED4ext::ent::Entity*>(entity.instance),t)){gripEntities[side]=entity;answer=0.f;}
    }
    PublishActivity();
    ReleaseSRWLockExclusive(&lock);
    if(out)*out=answer;
}

// Bind a PhysX grasp to the selected segment's bone. All driving and world
// conversion then happens natively from paired controller and NPC poses.
void VRNpcGripBind(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,float* out,int64_t){
    RED4ext::Handle<RED4ext::IScriptable> entity;int32_t side=-1,part=-1,segment=-1,owner=0;
    RED4ext::GetParameter(stack,&entity);RED4ext::GetParameter(stack,&side);RED4ext::GetParameter(stack,&part);
    RED4ext::GetParameter(stack,&segment);RED4ext::GetParameter(stack,&owner);stack->code++;
    if(out)*out=-1;if(!entity||side<0||side>1||part<0||part>3||segment<0||segment>2||owner<=0)return;
    using namespace cvr::anim;AcquireSRWLockExclusive(&lock);ReapGrips(0,0);auto& g=grips[side];float answer=-1;
    const auto current=gripEntities[side].Lock();
    if(g.on&&!g.letting&&g.anchor.live&&g.anchor.owner==owner&&g.part==part&&current.instance==entity.instance){g.asked=GetTickCount64();answer=g.miss;}
    else if(!g.on){const float zero[3]{};
        if(BeginGuarded(side,part,static_cast<RED4ext::ent::Entity*>(entity.instance),zero)){
            g.anchor.on=g.anchor.live=true;g.anchor.side=side;g.anchor.owner=owner;
            g.anchor.bone=segment==0?g.upper:segment==1?g.mid:g.end;gripEntities[side]=entity;answer=0;
            Log("[npcgrip] %s hand binds owner %d to bone %d (segment %d)\n",side?"right":"left",owner,g.anchor.bone,segment);
        }
    }
    PublishActivity();ReleaseSRWLockExclusive(&lock);if(out)*out=answer;
}

// Read-only diagnostic: one locked snapshot of the actually drawn limb,
// including its reference frames. No actor/rig pointers escape to Lua.
void VRNpcGripState(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,RED4ext::DynArray<RED4ext::Vector4>* out,int64_t) {
    int32_t side=-1;RED4ext::GetParameter(stack,&side);stack->code++;
    if(!out)return;out->Clear();if(side<0||side>1)return;
    using namespace cvr::anim;
    AcquireSRWLockShared(&lock);
    const Grip& g=grips[side];
    if(g.on&&g.bounded&&g.solved.at&&GetTickCount64()-g.solved.at<250){
        out->EmplaceBack(RED4ext::Vector4{float(g.part),g.miss,float(g.passes),1});
        for(const auto& p:g.solved.point)out->EmplaceBack(RED4ext::Vector4{p[0],p[1],p[2],1});
        for(const auto& q:g.solved.local)out->EmplaceBack(RED4ext::Vector4{q[0],q[1],q[2],q[3]});
        for(const float* q:{g.upperRest,g.hinge.rest,g.endRest})out->EmplaceBack(RED4ext::Vector4{q[0],q[1],q[2],q[3]});
        out->EmplaceBack(RED4ext::Vector4{g.target[0]+(g.anchor.on?0:g.offset[0]),g.target[1]+(g.anchor.on?0:g.offset[1]),g.target[2]+(g.anchor.on?0:g.offset[2]),g.hinge.restAngle});
        out->EmplaceBack(RED4ext::Vector4{g.hinge.axis[0],g.hinge.axis[1],g.hinge.axis[2],g.hinge.sign});
        out->EmplaceBack(RED4ext::Vector4{g.hinge.outgoing[0],g.hinge.outgoing[1],g.hinge.outgoing[2],g.hinge.maximum});
        out->EmplaceBack(RED4ext::Vector4{g.hinge.incoming[0],g.hinge.incoming[1],g.hinge.incoming[2],g.hinge.twist});
        if(g.anchor.on&&g.anchor.seeded){const auto& a=g.anchor;
            out->EmplaceBack(RED4ext::Vector4{float(a.bone),float(a.owner),float(a.serial),a.live?1.f:0.f});
            out->EmplaceBack(RED4ext::Vector4{a.point[0],a.point[1],a.point[2],g.miss});
            out->EmplaceBack(RED4ext::Vector4{a.local[0],a.local[1],a.local[2],1});
            out->EmplaceBack(RED4ext::Vector4{a.worldPoint[0],a.worldPoint[1],a.worldPoint[2],1});
            out->EmplaceBack(RED4ext::Vector4{a.worldGoal[0],a.worldGoal[1],a.worldGoal[2],1});
            out->EmplaceBack(RED4ext::Vector4{a.rotation[0],a.rotation[1],a.rotation[2],a.rotation[3]});
            out->EmplaceBack(RED4ext::Vector4{a.worldRotation[0],a.worldRotation[1],a.worldRotation[2],a.worldRotation[3]});
        }
    }
    ReleaseSRWLockShared(&lock);
}
