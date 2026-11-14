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
// IDENTIFIED BY TRACK BUFFER, as the player is: from the entity's animated object, the same two pointers (see ArmPlayerLocked). The buffer is indexed as the rig's bone names, when a pass has all of them; a pass of a
// lower level of detail has fewer and is left alone. Bone names and parents read from base\characters\base_entities\man_base\man_base.rig (Hips 2, Spine 4, Spine1 7, Spine2 10, Spine3 13, Neck 16, Neck1 19, Head 22).
#include "Anim/NpcArmGrip.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Natives/NativeFunctions.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/Entity.hpp>
#include <RED4ext/Scripting/Natives/animAnimatedObject.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
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
struct DonePose {uint8_t* buf;float q[4];};
struct Grip {
    bool on,letting,seeded;
    int part;
    uintptr_t trackA,trackB;
    uint32_t bones;
    int upper,mid,end;       // the two bones and what they carry; for a chain only `end`
    Joint joints[kMostJoints];   // from the root end. What the part's own bones cannot reach
    int jointCount;
    int16_t parents[VRIK_MAX_BONES];
    float target[3],turn[4],offset[3];
    uint64_t begun,let,asked;
    uint32_t passes,again;
    float miss;
    int chest,watch;         // the head is kept from the chest; the bone that tells a pose already solved
    float rest[4];           // the head as the chest had it when the grip closed
    DonePose done[4];
    int doneNext;
};
Grip grips[2]{};
SRWLOCK lock=SRWLOCK_INIT;
volatile LONG anyOn=0;
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
    const float la=std::sqrt(Dot(a,a)),lb=std::sqrt(Dot(b,b));
    q[0]=q[1]=q[2]=0;q[3]=1;
    if(la<1e-5f || lb<1e-5f)return;
    const float x[3]={a[0]/la,a[1]/la,a[2]/la},y[3]={b[0]/lb,b[1]/lb,b[2]/lb};
    float c[3];Cross(x,y,c);
    const float d=Dot(x,y);
    if(d<-.9999f)return;
    q[0]=c[0];q[1]=c[1];q[2]=c[2];q[3]=1+d;
    VRIK_QuatNorm(q);
}
void Conj(const float* q,float* o){o[0]=-q[0];o[1]=-q[1];o[2]=-q[2];o[3]=q[3];}
void BlendInto(float* out,const float* from,const float* to,float w) {
    const float s=(from[0]*to[0]+from[1]*to[1]+from[2]*to[2]+from[3]*to[3])<0?-1.f:1.f;
    for(int k=0;k<4;++k)out[k]=from[k]+(to[k]*s-from[k])*w;
    VRIK_QuatNorm(out);
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
    for(int b=bone;b>=0 && n<kMostChain;b=g.parents[b])chain[n++]=b;
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
    // TWO BONES, the elbow or knee staying in its plane
    const float toT[3]={t[0]-up.p[0],t[1]-up.p[1],t[2]-up.p[2]};
    float d=std::sqrt(Dot(toT,toT));
    const float lo=std::fabs(la-lb)+.001f,hi=(la+lb)*.999f;
    if(d<1e-4f)return;
    const float dir[3]={toT[0]/d,toT[1]/d,toT[2]/d};
    d=d<lo?lo:(d>hi?hi:d);
    const float reached[3]={up.p[0]+dir[0]*d,up.p[1]+dir[1]*d,up.p[2]+dir[2]*d};
    g.miss=Apart(t,reached);
    const float along=(la*la-lb*lb+d*d)/(2*d);
    const float h2=la*la-along*along;
    const float height=h2>0?std::sqrt(h2):0.f;
    const float bendWas[3]={mid.p[0]-up.p[0],mid.p[1]-up.p[1],mid.p[2]-up.p[2]};
    const float on=Dot(bendWas,dir);
    float side[3]={bendWas[0]-dir[0]*on,bendWas[1]-dir[1]*on,bendWas[2]-dir[2]*on};
    const float ls=std::sqrt(Dot(side,side));
    if(ls<1e-4f){side[0]=0;side[1]=0;side[2]=-1;}else for(auto& v:side)v/=ls;
    const float bend[3]={dir[0]*along+side[0]*height,dir[1]*along+side[1]*height,dir[2]*along+side[2]*height};
    float a1[4];Arc(bendWas,bend,a1);
    float upQ[4];VRIK_QuatMul(a1,up.q,upQ);VRIK_QuatNorm(upQ);
    const float endFromMid[3]={end.p[0]-mid.p[0],end.p[1]-mid.p[1],end.p[2]-mid.p[2]};
    float rotated[3];VRIK_QuatRotateVec(a1,endFromMid,rotated);
    const float wantEnd[3]={dir[0]*d-bend[0],dir[1]*d-bend[1],dir[2]*d-bend[2]};
    float a2[4];Arc(rotated,wantEnd,a2);
    float midBase[4];VRIK_QuatMul(a1,mid.q,midBase);
    float midQ[4];VRIK_QuatMul(a2,midBase,midQ);VRIK_QuatNorm(midQ);
    // A hand goes on as it was held and turns with the hand holding it; a foot stays as it was.
    float endQ[4];std::copy_n(endWas,4,endQ);
    if(g.part==ArmL || g.part==ArmR) {
        float turn[4];HeldTurn(g.turn,turn);
        VRIK_QuatMul(turn,endWas,endQ);VRIK_QuatNorm(endQ);
    }
    SetModelRot(g,buf,g.upper,upQ,touched);
    SetModelRot(g,buf,g.mid,midQ,touched);
    SetModelRot(g,buf,g.end,endQ,touched);
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
void Solve(Grip& g,uint8_t* buf,float weight) {
    // Where the grip took it is where it is: the slots the script goes by are not quite the bones.
    if(!g.seeded) {
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
    if(g.upper>=0)SolveLimb(g,buf,t,touched);
    else SolveChain(g,buf,t,touched);
    if(weight<.999f)for(int i=0;i<touched.count;++i) {
        float* now=LocalR(buf,touched.bone[i]);
        BlendInto(now,touched.was[i],now,weight);
    }
}
// ONCE ON A POSE. The hook sees a person's pose four times a frame (every grip's passes in the log of 2026-11-05 were a multiple of 4, some 165 a second). A pass finding the bones as the last one left them is on a pose already
// solved, and solving it again adds the turn again: a limit of 46 degrees let a head go 184. Told by the bone itself, not by the clock, so a new pose is never missed.
bool SolveOnce(Grip& g,uint8_t* buf,float weight) {
    const int watch=g.watch>=0?g.watch:g.end;
    DonePose* slot=nullptr;
    for(auto& d:g.done)if(d.buf==buf) {
        if(std::memcmp(LocalR(buf,watch),d.q,sizeof(d.q))==0) {++g.again;return false;}
        slot=&d;
        break;
    }
    Solve(g,buf,weight);
    if(!slot) {
        slot=&g.done[g.doneNext];
        g.doneNext=(g.doneNext+1)%4;
        slot->buf=buf;
    }
    std::memcpy(slot->q,LocalR(buf,watch),sizeof(slot->q));
    return true;
}
void ApplyInner(void* a2) {
    void* poseDesc=reinterpret_cast<void**>(a2)[7];
    if(!poseDesc)return;
    auto* boneBuf=reinterpret_cast<uint8_t**>(poseDesc)[0];
    const uintptr_t trackBuf=reinterpret_cast<uintptr_t*>(poseDesc)[3];
    if(!boneBuf || !trackBuf)return;
    const uint32_t slots=*reinterpret_cast<uint32_t*>(static_cast<uint8_t*>(poseDesc)+8);
    // What moves the trunk first: a limb hangs from it.
    for(int order=0;order<2;++order)for(auto& g:grips) {
        if(!g.on || (trackBuf!=g.trackA && trackBuf!=g.trackB) || slots<g.bones)continue;
        if((g.upper<0)!=(order==0))continue;
        const uint64_t tick=GetTickCount64();
        float weight;
        if(g.letting || tick-g.asked>kUnaskedMs) {
            if(!g.let)g.let=tick;
            weight=1.f-(tick-g.let)*.001f/kBlendOut;
            if(weight<=0) {g.on=false;Log("[npcgrip] let go of the %s after %u passes, and %u more on a pose already solved\n",kPartNames[g.part],g.passes,g.again);continue;}
        } else {
            weight=(tick-g.begun)*.001f/kBlendIn;
            if(weight>1)weight=1;
        }
        if(SolveOnce(g,boneBuf,weight))++g.passes;
    }
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
    g.trackA=ownerA?*reinterpret_cast<uintptr_t*>(ownerA+0x40):0;
    g.trackB=ownerB?*reinterpret_cast<uintptr_t*>(ownerB+0x18):0;
    if(!g.trackA && !g.trackB)return false;
    g.bones=rig->boneNames.Size();
    for(uint32_t i=0;i<g.bones;++i)g.parents[i]=rig->parentIndeces[i];
    g.part=part;
    g.upper=g.mid=g.end=-1;
    g.jointCount=0;
    g.chest=g.watch=-1;
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
        g.watch=BoneNamed(rig,"Spine2");   // the chest's own bone is not one it writes
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
    InterlockedExchange(&anyOn,1);
    Log("[npcgrip] holding the %s: %u bones, end %d, %d joints above it\n",kPartNames[part],g.bones,g.end,g.jointCount);
    return true;
}
bool BeginGuarded(int side,int part,RED4ext::ent::Entity* entity,const float* target) {
    __try {return Begin(side,part,entity,target);}
    __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
}  // namespace

void NpcArmGripApply(void* a2) {
    if(!anyOn)return;
    __try {
        AcquireSRWLockExclusive(&lock);
        ApplyInner(a2);
        ReleaseSRWLockExclusive(&lock);
    } __except(EXCEPTION_EXECUTE_HANDLER) {
        ReleaseSRWLockExclusive(&lock);
        grips[0].on=grips[1].on=false;
    }
    if(!grips[0].on && !grips[1].on)InterlockedExchange(&anyOn,0);
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
    Grip& g=grips[side];
    const float t[3]={target.X,target.Y,target.Z};
    AcquireSRWLockExclusive(&lock);
    float answer=-1.f;
    if(target.W<.5f) {
        if(g.on)g.letting=true;
        answer=0.f;
    } else if(g.on && !g.letting && g.part==part) {
        std::copy_n(t,3,g.target);
        g.turn[0]=turn.X;g.turn[1]=turn.Y;g.turn[2]=turn.Z;g.turn[3]=turn.W;
        g.asked=GetTickCount64();
        answer=g.miss;
    } else if(entity.instance && !g.on) {
        if(BeginGuarded(side,part,reinterpret_cast<RED4ext::ent::Entity*>(entity.instance),t))answer=0.f;
    }
    ReleaseSRWLockExclusive(&lock);
    if(out)*out=answer;
}
