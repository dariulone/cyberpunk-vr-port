// HeldBody -- a carried thing in the hand, the same frame as the hand: held by its own body (HoldBody, below), or its
// picture drawn on the hand while its body follows by velocity (Hold, this first part).
//
// WHY. A body can only follow the hand from one physics step to the next, so it is a frame behind at best: 7 to 10 cm at 3 m/s (measured 2026-11-04). The body has to stay a
// body, for walls and for the throw. Only its picture has to be on the hand, and the picture is a separate thing.
//
// HOW THE PICTURE GETS THERE, read in the 2.5 exe. Loose props are worldInstancedDestructibleMeshNodeInstance. Each has a listener at +0x100, registered with the physics system
// (at 0x143BFCDD0) in the table at +0x222018, by the low 16 bits of the proxy id; its pose records are at +0x1A2018, {records, count}, 64 bytes each: position as a 32-bit
// fixed point number at +0, quaternion at +0x10. sub_1405C3454(instance, span of records, count) turns those into one transform per instance and sends them to the render proxy.
// It keeps nothing of them and reads the span only, so what is in the span is what is drawn, and nobody else sees it.
//   1. A detour on that function gives it a copy of the span with the carried instance changed, so the engine's own message never undoes the picture.
//   2. It may run before the hand is animated, and does not run for a body at rest. So once more at the frame submit (sub_1403D6D84, game thread, before the proxy flush), with
//      the hand as animated for this frame.
// A wrong picture is the most this can do: nothing here writes to the game's memory.
//
// NOT PROVEN until it has run: that the message sent at the submit reaches this frame, and what motion vectors and ray tracing do. The [picture] lines in the log are for that.
#include "Natives/HeldBody.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Natives/DebugView.hpp"
#include "Natives/FrameOrderProbe.hpp"
#include "Natives/PhysXStep.hpp"
#include "Natives/PhysHands.hpp"
#include "Anim/VrikState.hpp"
#include "Anim/GraspPose.hpp"
#include "Core/GameBuild.hpp"
#include "Core/LiveControls.hpp"
#include "Hooks/Hook.hpp"
#include "MinHook.h"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/Vector4.hpp>
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstring>

extern void Log(const char* fmt, ...);
extern "C" {
    extern volatile float CyberpunkVR_DebugGraspApproach[2][4];
    extern float CyberpunkVR_PlayerEntityPos[3];
    extern float CyberpunkVR_PlayerEntityQuat[4];
    extern int   CyberpunkVR_PlayerEntityValid;
}

namespace cvr::heldbody {
namespace {
constexpr uintptr_t kUpdateRva=0x5C3454,kListenerEntryRva=0x5C3010,kSystemRva=0x3BFCDD0,kListeners=0x222018,kRecords=0x1A2018,kProxyId=0x160,kListenerAt=0x100;
constexpr uint32_t kRecordSize=64,kMostInstances=256;      // the instance's alive mask has 4 words
constexpr double kFixed=131072.0;
constexpr float kBlendIn=.08f,kBlendOut=.1f;              // s
constexpr float kFarFromBody=3.f;                         // m: a picture further than this means this file has misread something, or the player has been moved
constexpr uint64_t kUnaskedMs=150;
constexpr float kMostSpin=50.f;                           // rad/s: a wrist flick is above PhysX's 7, measured held to exactly that

struct Pose {float p[3];float q[4];};
struct Slot {
    bool used,asked;
    uint32_t proxy,body;
    int bone;
    float offset[3],turn[4];
    bool blocked;
    float reach;
    // The setting is off: the slot only measures (the debug view, the log), the picture stays on the body and nothing is changed.
    bool measureOnly;
    // Held by its own body (HoldBody): its own PhysX actor, dynamic, given at each step the velocities that bring it into the
    // hand by the step's end, as far as a hand's force can (Steer). The actor, its scene and that scene's origin in the
    // game's world; whether it is being steered (its damping and spin ceiling changed, kept in `saved`); the throw it gets
    // when let go (w 1: given, 0: it keeps the velocity it has).
    bool ownBody,driving;
    float saved[3];
    float throwGiven;
    void* actor;
    void* scene;
    float origin[3];
    float throwV[3],throwW[3];
    uint32_t targets,driveFailed;
    // Its shapes' simulation filter data as it was, put back when it is let go (see Filter).
    void* shapes[8];
    uint32_t filters[8][4];
    uint32_t shapeCount;
    // IN ONE HAND, SEATED FOR ITS GRASP (src/Anim/GraspPose.cpp): the script asks for it while one hand holds; the hand's
    // fingers close on its shape the way the hand came to it, and it eases from where it was taken (seatFrom) to where that
    // grasp has it (seat), both in the hand's frame (x y z, then x y z w). The script's offset and turn are not used while
    // it is seated.
    bool graspWanted,seated,graspTried;
    int graspSide,graspKind;
    float seatFrom[7],seat[7],seatT;
    uint64_t askedAt;
    float weight;     // 0 the body, 1 the hand
    bool posed;
    Pose hand;
    // Let go: what is between the picture and the body, carried along with the body and closing.
    bool letGo,relSet,spun;
    float relP[3],relQ[4];
    // what was seen, for the log
    uint32_t frames,sends,detours,tooFar,blockedFrames,instances;
    uint64_t firstTick;
    DWORD detourThread;
    uint64_t lastDetour,lastSubmit;
    double behindSum,behindMost,detourGapSum;
    uint32_t detourGaps;
};
SRWLOCK lock=SRWLOCK_INIT;
Slot slots[2]{};
volatile LONG anyUsed=0;
uintptr_t base=0;
// Reading memory that may not be there without a fault (the engine's vectored handler would take one first).
bool Peek(uintptr_t at,void* out,size_t n) {
    SIZE_T got=0;
    return at>0x10000 && ReadProcessMemory(GetCurrentProcess(),reinterpret_cast<const void*>(at),out,n,&got) && got==n;
}
bool installed=false;
using UpdateFn=void(*)(uint8_t*,void*,uint32_t);
UpdateFn originalUpdate=nullptr;
struct Span {uint8_t* begin;uint8_t* end;};

void Mul(const float* a,const float* b,float* o) {
    o[0]=a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1];
    o[1]=a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0];
    o[2]=a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3];
    o[3]=a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2];
}
void Rotate(const float* q,const float* v,float* o) {
    const float t[3]={2*(q[1]*v[2]-q[2]*v[1]),2*(q[2]*v[0]-q[0]*v[2]),2*(q[0]*v[1]-q[1]*v[0])};
    o[0]=v[0]+q[3]*t[0]+q[1]*t[2]-q[2]*t[1];
    o[1]=v[1]+q[3]*t[1]+q[2]*t[0]-q[0]*t[2];
    o[2]=v[2]+q[3]*t[2]+q[0]*t[1]-q[1]*t[0];
}
bool UnitLike(const float* q) {
    const float n=q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3];
    return std::isfinite(n) && n>.9f && n<1.1f;
}
// PxRigidBody / PxRigidDynamic slots (PhysX 3.4, counted from its headers; setMaxAngularVelocity at 60 is the one
// Natives/DebugView.cpp already calls).
constexpr int kGetShapes=26,kShapeSetSimFilter=20,kShapeGetSimFilter=21;
constexpr int kGetGlobalPose=20,kGetCMassLocalPose=30,kGetMass=32,kGetLinearVelocity=37,kSetLinearVelocity=38,
              kGetAngularVelocity=39,kSetAngularVelocity=40,kSetLinearDamping=56,kGetLinearDamping=57,kSetAngularDamping=58,
              kGetAngularDamping=59,kSetMaxAngularVelocity=60,kGetMaxAngularVelocity=61;
struct PxPose {float q[4];float p[3];};
// HOW HARD A HAND PULLS, and how fast it turns a thing: the cap on each step's change of velocity is this force over the
// thing's mass (a light thing follows any swing exactly, a heavy one lags it), and never more than kMostAccel. What it
// runs into answers with its own mass: a can pushes a can aside, a crate stops it, a wall stops it.
constexpr float kHandForce=250.f;      // N
constexpr float kMostAccel=1500.f;     // m/s^2
constexpr float kMostSpinAccel=4000.f; // rad/s^2
constexpr float kHeldSpin=200.f;       // rad/s: PhysX's spin ceiling while held (a wrist flick turns ~20 deg a frame)
struct SteerIn {float target[7];float origin[3];float gravity[3];float dt;bool first;};
// One step's velocities: those that bring the body's centre of mass and turn from where they are to the target's by the end
// of the step (PhysX adds gravity before it moves the body: taken off; damping is set to none while held), their change
// held to what a hand can give. `gap`: how far the body is from the target now (m). False if PhysX faulted.
bool Steer(void* actor,const SteerIn& in,float* saved,float* gap,float* mass) {
    __try {
        void** table=*reinterpret_cast<void***>(actor);
        if(in.first) {
            saved[0]=reinterpret_cast<float(*)(void*)>(table[kGetLinearDamping])(actor);
            saved[1]=reinterpret_cast<float(*)(void*)>(table[kGetAngularDamping])(actor);
            saved[2]=reinterpret_cast<float(*)(void*)>(table[kGetMaxAngularVelocity])(actor);
            reinterpret_cast<void(*)(void*,float)>(table[kSetLinearDamping])(actor,0.f);
            reinterpret_cast<void(*)(void*,float)>(table[kSetAngularDamping])(actor,0.f);
            reinterpret_cast<void(*)(void*,float)>(table[kSetMaxAngularVelocity])(actor,kHeldSpin);
        }
        PxPose pose{},cm{};
        reinterpret_cast<void(*)(void*,PxPose*)>(table[kGetGlobalPose])(actor,&pose);
        // THE FIRST STEP ONLY TAKES IT. Its filter was changed this step (held apart from the loose things), and PhysX
        // lets go of its pairs with its neighbours in the next simulate: moved at once, a plate taken out of the middle of a
        // stack struck the ones on it at 1.3 m/s before they had let go of it. Held still this step, steered from the next.
        if(in.first) {
            const float still[3]={-in.gravity[0]*in.dt,-in.gravity[1]*in.dt,-in.gravity[2]*in.dt},none[3]={0.f,0.f,0.f};
            reinterpret_cast<void(*)(void*,const float*,bool)>(table[kSetLinearVelocity])(actor,still,true);
            reinterpret_cast<void(*)(void*,const float*,bool)>(table[kSetAngularVelocity])(actor,none,true);
            *mass=reinterpret_cast<float(*)(void*)>(table[kGetMass])(actor);
            float g2=0.f;
            for(int k=0;k<3;++k) {const float d=in.target[k]-in.origin[k]-pose.p[k];g2+=d*d;}
            *gap=std::sqrt(g2);
            return true;
        }
        reinterpret_cast<void(*)(void*,PxPose*)>(table[kGetCMassLocalPose])(actor,&cm);
        float v[3]{},w[3]{};
        reinterpret_cast<void(*)(void*,float*)>(table[kGetLinearVelocity])(actor,v);
        reinterpret_cast<void(*)(void*,float*)>(table[kGetAngularVelocity])(actor,w);
        const float m=reinterpret_cast<float(*)(void*)>(table[kGetMass])(actor);
        *mass=m;
        // the centre of mass now and where the target puts it, PhysX's frame
        float at[3],to[3],off[3];
        Rotate(pose.q,cm.p,off);
        for(int k=0;k<3;++k)at[k]=pose.p[k]+off[k];
        Rotate(in.target+3,cm.p,off);
        for(int k=0;k<3;++k)to[k]=in.target[k]-in.origin[k]+off[k];
        const float dt=in.dt>1e-4f?in.dt:1.f/60.f;
        float need[3],dv[3];
        float gap2=0.f;
        for(int k=0;k<3;++k) {
            const float d=in.target[k]-in.origin[k]-pose.p[k];
            gap2+=d*d;
            need[k]=(to[k]-at[k])/dt;
            dv[k]=need[k]-v[k];
        }
        *gap=std::sqrt(gap2);
        const float most=std::min(kMostAccel,kHandForce/std::max(m,1e-3f))*dt;
        const float dvn=std::sqrt(dv[0]*dv[0]+dv[1]*dv[1]+dv[2]*dv[2]);
        const float kv=dvn>most?most/dvn:1.f;
        float set[3];
        for(int k=0;k<3;++k)set[k]=v[k]+dv[k]*kv-in.gravity[k]*dt;
        // the turn: from the body's to the target's, about the shortest way, in the step
        const float conj[4]={-pose.q[0],-pose.q[1],-pose.q[2],pose.q[3]};
        float dq[4];
        Mul(in.target+3,conj,dq);
        if(dq[3]<0)for(auto& c:dq)c=-c;
        const float s=std::sqrt(dq[0]*dq[0]+dq[1]*dq[1]+dq[2]*dq[2]);
        const float angle=2.f*std::atan2(s,dq[3]);
        float spin[3]{};
        if(s>1e-7f)for(int k=0;k<3;++k)spin[k]=dq[k]/s*angle/dt;
        float dw[3];
        for(int k=0;k<3;++k)dw[k]=spin[k]-w[k];
        const float mostW=kMostSpinAccel*dt;
        const float dwn=std::sqrt(dw[0]*dw[0]+dw[1]*dw[1]+dw[2]*dw[2]);
        const float kw=dwn>mostW?mostW/dwn:1.f;
        float wset[3];
        for(int k=0;k<3;++k)wset[k]=w[k]+dw[k]*kw;
        reinterpret_cast<void(*)(void*,const float*,bool)>(table[kSetLinearVelocity])(actor,set,true);
        reinterpret_cast<void(*)(void*,const float*,bool)>(table[kSetAngularVelocity])(actor,wset,true);
        return std::isfinite(set[0]+set[1]+set[2]+wset[0]+wset[1]+wset[2]);
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
// PxScene::getGravity (slot 67, as Natives/DebugView.cpp calls it).
void SceneGravity(void* scene,float* out) {
    out[0]=0.f;out[1]=0.f;out[2]=-9.81f;
    __try {
        float g[3]{};
        reinterpret_cast<void*(*)(void*,float*)>((*reinterpret_cast<void***>(scene))[67])(scene,g);
        if(std::isfinite(g[0]+g[1]+g[2]) && std::fabs(g[2])>1.f && std::fabs(g[2])<30.f) {out[0]=g[0];out[1]=g[1];out[2]=g[2];}
    } __except(EXCEPTION_EXECUTE_HANDLER) {}
}
// Let go: damping and the spin ceiling as they were, and the throw when one is given.
bool Unsteer(void* actor,const float* saved,const float* velocity,const float* spin,bool thrown) {
    __try {
        void** table=*reinterpret_cast<void***>(actor);
        reinterpret_cast<void(*)(void*,float)>(table[kSetLinearDamping])(actor,saved[0]);
        reinterpret_cast<void(*)(void*,float)>(table[kSetAngularDamping])(actor,saved[1]);
        reinterpret_cast<void(*)(void*,float)>(table[kSetMaxAngularVelocity])(actor,std::max(saved[2],50.f));
        if(thrown) {
            reinterpret_cast<void(*)(void*,const float*,bool)>(table[kSetLinearVelocity])(actor,velocity,true);
            reinterpret_cast<void(*)(void*,const float*,bool)>(table[kSetAngularVelocity])(actor,spin,true);
        }
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
// LOOSE PROPS DO NOT MEET EACH OTHER. Their shapes' simulation filter data (read 2026-11-08, cups, plates, cans) is group
// 0x1000 in word 0 and 0xEC in word 2, and 0x1000 is not in 0xEC: one carried by velocity, or held kinematic, went
// through another hung in its way six times out of six, either way. AND THAT IS KEPT, by default: the bits below are
// added only when asked for (VRHeldBodyFilter; word 2 0xFFFFFFFF there means "its own group"). A held box let meet its
// own kind crashed the game (2026-11-08 20:52, ReportQueue ...-205204): instances of one InstancedDestructibleNode
// stand in each other, since they never collide, so the box taken from a stack met its neighbours inside them, PhysX
// threw them apart, two of them broke, and the player's character controller ran its next query into one of the
// actors the game had just freed for that (PhysX3CharacterKinematic +0xA592, getKinematicTarget through a dead
// table). Let go, the shapes get back what they had.
uint32_t heldFilterOr[2]={0,0};
// A HELD THING MEETS NO LOOSE THING (Natives/PhysHands.cpp, HeldAlone): the stall's plates (group 1c008, mask 13cf) do meet
// each other, and one pulled out of the middle of a stack threw the rest off the counter. Held, it is in the hands' group
// with the world's mask: taken alone, it still stops at a wall and a table. Let go, it has its own filter again.
bool Filter(void* actor,void** shapes,uint32_t (*saved)[4],uint32_t* count) {
    const bool adding=heldFilterOr[0] || heldFilterOr[1];
    __try {
        void** table=*reinterpret_cast<void***>(actor);
        const uint32_t n=reinterpret_cast<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(table[kGetShapes])(actor,shapes,8,0);
        *count=n<8?n:8;
        bool any=false;
        for(uint32_t i=0;i<*count;++i) {
            void** st=*reinterpret_cast<void***>(shapes[i]);
            reinterpret_cast<void*(*)(void*,uint32_t*)>(st[kShapeGetSimFilter])(shapes[i],saved[i]);
            uint32_t data[4]={saved[i][0],saved[i][1],saved[i][2],saved[i][3]};
            if(adding) {
                data[0]|=heldFilterOr[0];
                data[2]|=heldFilterOr[1]==0xFFFFFFFFu?saved[i][0]:heldFilterOr[1];
            } else if(!cvr::physhands::HeldAlone(saved[i],data)) continue;
            reinterpret_cast<void(*)(void*,const uint32_t*)>(st[kShapeSetSimFilter])(shapes[i],data);
            any=true;
        }
        if(!any)*count=0;
        return any;
    } __except(EXCEPTION_EXECUTE_HANDLER) {*count=0;return false;}
}
bool Unfilter(void** shapes,uint32_t (*saved)[4],uint32_t count) {
    __try {
        for(uint32_t i=0;i<count;++i) {
            void** st=*reinterpret_cast<void***>(shapes[i]);
            reinterpret_cast<void(*)(void*,const uint32_t*)>(st[kShapeSetSimFilter])(shapes[i],saved[i]);
        }
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
// Where PhysX has an actor, in the game's world: x y z, then the rotation x y z w.
bool ActorPose(void* actor,const float* origin,float* out) {
    __try {
        PxPose pose{};
        reinterpret_cast<void(*)(void*,PxPose*)>((*reinterpret_cast<void***>(actor))[kGetGlobalPose])(actor,&pose);
        for(int k=0;k<3;++k)out[k]=pose.p[k]+origin[k];
        for(int k=0;k<4;++k)out[3+k]=pose.q[k];
        return std::isfinite(out[0]+out[1]+out[2]+out[3]+out[4]+out[5]+out[6]);
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
// ---- THE SHAPE A HAND CLOSES ON: the actor's own shapes, as one primitive in the actor's frame.
// PxShape slots, read off NpShape's table in PhysX3_x64.dll (getConcreteTypeName's "PxShape" at 1, getReferenceCount at 6,
// so one more than the 3.4 header counts): getGeometryType 7, getBoxGeometry 10, getSphereGeometry 11, getCapsuleGeometry 12,
// getConvexMeshGeometry 14, getLocalPose 19 (returned through a hidden pointer). Gu::ConvexMesh (PhysX3Common_x64.dll):
// getLocalBounds 13, the same way; getNbVertices 5 and getVertices 6 (PxBase's five slots, then the mesh's own).
// Geometry types: 0 sphere, 2 capsule (along its X), 3 box, 4 convex mesh. PxConvexMeshGeometry: its scale at +4 (the
// scale's own turn at +16), the mesh at +32.
constexpr int kShapeGeometryType=7,kShapeGetBox=10,kShapeGetSphere=11,kShapeGetCapsule=12,kShapeGetConvex=14,kShapeLocalPose=19,
              kConvexLocalBounds=13,kConvexNbVertices=5,kConvexVertices=6;
constexpr int kHullMost=256;            // a convex mesh has at most 255 points
// A convex shape's mesh and how it is scaled into the shape, and the middle of its bounds there (the box's centre).
struct Hull {void* mesh=nullptr;float scale[3]{1.f,1.f,1.f};float turn[4]{0.f,0.f,0.f,1.f};float centre[3]{};};
constexpr float kGraspLargest=.3f;      // m: a thing with a half size past this is not one hand's (a crate): no grasp
constexpr float kRound=.15f;            // two half sizes this near (share of the larger) are a round section...
constexpr float kLong=1.2f;             // ...and the third this much longer: a can, a bottle, a cylinder
constexpr float kSeatSec=.12f;          // s from where it was taken to its seat
// One shape as a box (or a ball, a capsule) in the actor's frame. kind: 0 box, 1 sphere, 2 capsule along its X.
bool ShapeBox(void* shape,int& kind,float* half,PxPose& local,Hull* hull=nullptr) {
    __try {
        void** st=*reinterpret_cast<void***>(shape);
        const int type=reinterpret_cast<int(*)(void*)>(st[kShapeGeometryType])(shape);
        reinterpret_cast<void*(*)(void*,PxPose*)>(st[kShapeLocalPose])(shape,&local);
        if(type==3) {
            float g[4]{};
            if(!reinterpret_cast<bool(*)(void*,float*)>(st[kShapeGetBox])(shape,g))return false;
            kind=0;std::copy_n(g+1,3,half);
            return true;
        }
        if(type==0) {
            float g[2]{};
            if(!reinterpret_cast<bool(*)(void*,float*)>(st[kShapeGetSphere])(shape,g))return false;
            kind=1;half[0]=half[1]=half[2]=g[1];
            return true;
        }
        if(type==2) {
            float g[3]{};
            if(!reinterpret_cast<bool(*)(void*,float*)>(st[kShapeGetCapsule])(shape,g))return false;
            kind=2;half[0]=g[2]+g[1];half[1]=half[2]=g[1];
            return true;
        }
        if(type==4) {
            alignas(8) uint8_t g[64]{};
            if(!reinterpret_cast<bool(*)(void*,void*)>(st[kShapeGetConvex])(shape,g))return false;
            void* mesh=*reinterpret_cast<void**>(g+32);
            const float* scale=reinterpret_cast<const float*>(g+4);
            if(!mesh)return false;
            float b[6]{};
            reinterpret_cast<void*(*)(void*,float*)>((*reinterpret_cast<void***>(mesh))[kConvexLocalBounds])(mesh,b);
            float centre[3],moved[3];
            for(int k=0;k<3;++k) {
                half[k]=.5f*(b[3+k]-b[k])*std::fabs(scale[k]);
                centre[k]=.5f*(b[3+k]+b[k])*scale[k];
            }
            Rotate(local.q,centre,moved);
            for(int k=0;k<3;++k)local.p[k]+=moved[k];
            if(hull) {
                hull->mesh=mesh;
                std::copy_n(scale,3,hull->scale);
                std::copy_n(reinterpret_cast<const float*>(g+16),4,hull->turn);
                std::copy_n(centre,3,hull->centre);
            }
            kind=0;
            return true;
        }
        return false;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
// The points of a convex mesh, scaled into its shape (PxMeshScale: turned into the scale's frame, scaled, turned back) and
// taken from the middle of its bounds. Their count, 0 when there are none to read or they make no sense.
int HullPoints(const Hull& h,float* out) {
    __try {
        void** mt=*reinterpret_cast<void***>(h.mesh);
        const uint32_t n=reinterpret_cast<uint32_t(*)(void*)>(mt[kConvexNbVertices])(h.mesh);
        if(n<4 || n>kHullMost)return 0;
        const float* v=reinterpret_cast<const float*(*)(void*)>(mt[kConvexVertices])(h.mesh);
        if(!v)return 0;
        const float inv[4]={-h.turn[0],-h.turn[1],-h.turn[2],h.turn[3]};
        for(uint32_t i=0;i<n;++i) {
            float a[3],b[3];
            Rotate(h.turn,v+3*i,a);
            for(int k=0;k<3;++k)a[k]*=h.scale[k];
            Rotate(inv,a,b);
            for(int k=0;k<3;++k) {
                out[3*i+k]=b[k]-h.centre[k];
                if(!std::isfinite(out[3*i+k]) || std::fabs(out[3*i+k])>1.f)return 0;
            }
        }
        return static_cast<int>(n);
    } __except(EXCEPTION_EXECUTE_HANDLER) {return 0;}
}
// The actor's shapes as one primitive for the grasp (src/Anim/GraspPose.cpp: 0 box, 1 sphere, 2 cylinder along its Z), its half
// sizes, and its pose in the actor's frame (x y z, x y z w). One shape keeps its own frame; more are one box round them all. A box
// whose section is round and which is longer than that is a cylinder: a can's hull, a bottle's. A cylinder that is one convex
// shape also gives its hull's points (`points`, kHullMost of them at most, their count in `nPoints`) in the cylinder's frame:
// the part a hand closes on is read from them (GraspPartOf: a bottle's neck or its body).
bool GraspShape(void* actor,int& shape,float* half,float* local,float* points=nullptr,int* nPoints=nullptr) {
    if(nPoints)*nPoints=0;
    void* shapes[8]{};
    uint32_t n=0;
    __try {
        void** table=*reinterpret_cast<void***>(actor);
        n=reinterpret_cast<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(table[kGetShapes])(actor,shapes,8,0);
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
    if(n==0)return false;
    if(n>8)n=8;
    PxPose pose{};
    float h[3]{};
    int kind=0;
    Hull hull;
    if(n==1) {
        if(!ShapeBox(shapes[0],kind,h,pose,&hull))return false;
    } else {
        float lo[3]={1e9f,1e9f,1e9f},hi[3]={-1e9f,-1e9f,-1e9f};
        for(uint32_t i=0;i<n;++i) {
            int k=0;float sh[3];PxPose sp{};
            if(!ShapeBox(shapes[i],k,sh,sp))return false;
            for(int c=0;c<8;++c) {
                const float corner[3]={c&1?sh[0]:-sh[0],c&2?sh[1]:-sh[1],c&4?sh[2]:-sh[2]};
                float at[3];
                Rotate(sp.q,corner,at);
                for(int a=0;a<3;++a) {lo[a]=std::min(lo[a],sp.p[a]+at[a]);hi[a]=std::max(hi[a],sp.p[a]+at[a]);}
            }
        }
        for(int a=0;a<3;++a) {h[a]=.5f*(hi[a]-lo[a]);pose.p[a]=.5f*(hi[a]+lo[a]);}
        pose.q[0]=pose.q[1]=pose.q[2]=0.f;pose.q[3]=1.f;
        kind=0;
    }
    if(!(std::max(h[0],std::max(h[1],h[2]))<=kGraspLargest) || !(std::min(h[0],std::min(h[1],h[2]))>0.f))return false;
    // the long axis of a capsule, or of a box with a round section; a cylinder's axis is its Z
    int axis=-1;
    if(kind==2)axis=0;
    else if(kind==0) {
        for(int a=0;a<3 && axis<0;++a) {
            const float u=h[(a+1)%3],v=h[(a+2)%3];
            if(std::fabs(u-v)<=kRound*std::max(u,v) && h[a]>=kLong*std::max(u,v))axis=a;
        }
    }
    const float r=.70710678f;
    float turn[4]={0.f,0.f,0.f,1.f};
    if(axis==0) {turn[1]=r;turn[3]=r;}             // its Z onto the actor's X: a quarter turn about Y
    else if(axis==1) {turn[0]=-r;turn[3]=r;}       // its Z onto Y: a quarter turn back about X
    if(axis>=0) {
        shape=2;
        half[0]=h[(axis+1)%3];half[1]=h[(axis+2)%3];half[2]=h[axis];
    } else {
        shape=kind==1?1:0;
        std::copy_n(h,3,half);
    }
    float q[4];
    Mul(pose.q,turn,q);
    std::copy_n(pose.p,3,local);
    std::copy_n(q,4,local+3);
    // the hull in the cylinder's frame: the box's frame turned back by `turn`
    if(axis>=0 && hull.mesh && points && nPoints) {
        const int m=HullPoints(hull,points);
        const float back[4]={-turn[0],-turn[1],-turn[2],turn[3]};
        for(int i=0;i<m;++i) {
            float v[3];
            Rotate(back,points+3*i,v);
            std::copy_n(v,3,points+3*i);
        }
        *nPoints=m;
    }
    return true;
}
// THE DRAWN SHAPE OF A ROUND PROP. A bottle's collider is a capsule round the whole of it (a can's, a cup's too): it has no
// neck, and it is thicker than the glass. Where the neck is and how thin, and how thick the body really is, are the drawn
// mesh's: their radius profiles, made from the game's cooked meshes by tools/grasp_tests/profiles (profilegen.exe), by the
// mesh's depot path's hash. The mesh is the instance's CMesh (a RED object among those it points at: its weak self handle
// at +8, its table in the game's image, its class a CMesh; CResource's path at +0x30). Its frame is the body's: the
// instance's record is the body's pose (a bottle's capsule lies along the mesh's Z, from its base at 0 to its mouth).
struct MeshProfile {uint64_t hash;int axis;float lo,hi,cb,cc;float r[16];};
const MeshProfile kMeshProfiles[]={
#include "../Anim/GraspProfiles.inc"
};
const MeshProfile* ProfileOf(uint64_t hash) {
    if(!hash)return nullptr;
    for(const MeshProfile& m:kMeshProfiles)if(m.hash==hash)return &m;
    return nullptr;
}
uint8_t* InstanceOf(uint32_t proxy);
uint64_t MeshOf(uint32_t proxy) {
    uint8_t* instance=InstanceOf(proxy);
    auto* rtti=RED4ext::CRTTISystem::Get();
    static RED4ext::CClass* meshClass=rtti?rtti->GetClass("CMesh"):nullptr;
    if(!instance || !meshClass)return 0;
    for(int off=8;off<0x200;off+=8) {
        uintptr_t v=0,self=0,table=0;
        if(!Peek(uintptr_t(instance)+off,&v,8) || !v)continue;
        if(!Peek(v+8,&self,8) || self!=v)continue;
        if(!Peek(v,&table,8) || table<base || table>base+0x8000000)continue;
        RED4ext::CClass* type=reinterpret_cast<RED4ext::ISerializable*>(v)->GetType();
        if(!type || !type->IsA(meshClass))continue;
        uint64_t hash=0;
        return Peek(v+0x30,&hash,8)?hash:0;
    }
    return 0;
}
constexpr int kProfileRing=8;
// A profile's rings (kProfileRing points round each slice's middle) in the cylinder's frame (`local`: its pose in the body's,
// that is the mesh's), each taken `mz` along the cylinder's axis toward the profile's own middle; their count, 0 when the
// mesh's axis is not the cylinder's or it is not as thick (another scale, another thing).
int ProfilePoints(const MeshProfile& m,const float* local,const float* half,float* out,float& mz,float& halfLength,float& widest) {
    const float inv[4]={-local[3],-local[4],-local[5],local[6]};
    float axis[3]={0.f,0.f,0.f},dir[3];
    axis[m.axis]=1.f;
    Rotate(inv,axis,dir);
    if(std::fabs(dir[2])<.95f)return 0;
    widest=0.f;
    for(float r:m.r)widest=std::max(widest,r);
    const float capsule=std::max(half[0],half[1]);
    if(!(widest>.004f) || widest>1.4f*capsule || widest<.6f*capsule)return 0;
    const int b=(m.axis+1)%3,c=(m.axis+2)%3;
    float centre[3],rel[3],mid[3];
    centre[m.axis]=.5f*(m.lo+m.hi);centre[b]=m.cb;centre[c]=m.cc;
    for(int k=0;k<3;++k)rel[k]=centre[k]-local[k];
    Rotate(inv,rel,mid);
    if(std::hypot(mid[0],mid[1])>.01f)return 0;      // its axis is not the cylinder's
    mz=mid[2];
    halfLength=.5f*(m.hi-m.lo);
    const float w=(m.hi-m.lo)/16.f;
    int n=0;
    for(int k=0;k<16;++k)
        for(int a=0;a<kProfileRing;++a) {
            const float t=6.2831853f*a/kProfileRing;
            float p[3];
            p[m.axis]=m.lo+w*(k+.5f);p[b]=m.cb+m.r[k]*std::cos(t);p[c]=m.cc+m.r[k]*std::sin(t);
            for(int i=0;i<3;++i)rel[i]=p[i]-local[i];
            Rotate(inv,rel,out+3*n);
            out[3*n+2]-=mz;
            ++n;
        }
    return n;
}
// Fingers closed on a held thing, per hand: on while it is seated, eased off when it is let go or the other hand takes it too.
struct Fingers {float blend;bool on,used;uint64_t at;};
Fingers fingers[2]{};
constexpr float kFingersIn=.12f,kFingersOut=.18f;     // s
void EaseFingers() {
    const uint64_t now=GetTickCount64();
    for(int h=0;h<2;++h) {
        Fingers& f=fingers[h];
        if(!f.used)continue;
        const float dt=f.at?std::min(float(now-f.at)*.001f,.1f):0.f;
        f.at=now;
        f.blend=f.on?std::min(1.f,f.blend+dt/kFingersIn):std::max(0.f,f.blend-dt/kFingersOut);
        if(!f.on && f.blend<=0.f) {
            cvr::anim::GraspLetGo(h);
            f.used=false;f.at=0;
            continue;
        }
        cvr::anim::GraspEase(h,f.blend);
    }
}
void Unseat(Slot& s);
// Seated for its grasp, or not: the slot's hand and its offset and turn (the caller holds the lock).
void SeatFor(Slot& s,float dt) {
    const int side=s.bone==g_VRRightBoneIdx?1:s.bone==g_VRLeftBoneIdx?0:-1;
    if(s.graspWanted && !s.seated && !s.graspTried && side>=0 && s.actor) {
        s.graspTried=true;
        int shape=0;
        float half[3],local[7];
        float hull[3*kHullMost];
        int hullN=0;
        if(!GraspShape(s.actor,shape,half,local,hull,&hullN)) {
            Log("[heldbody] proxy %u body %u: no shape for the grasp (or too big for one hand)\n",s.proxy,s.body);
            return;
        }
        // the thing in the hand's frame: where the actor is there, then the shape in the actor
        float p[3],q[4],moved[3];
        Rotate(s.turn,local,moved);
        for(int k=0;k<3;++k)p[k]=s.offset[k]+moved[k];
        Mul(s.turn,local+3,q);
        // a bottle: its neck or its body, whichever the palm is at (the palm in the cylinder's frame), a cylinder of its own,
        // and the whole of it round that for the fingers -- from its convex hull, or from its drawn mesh's profile
        int ends=15;
        bool part=false;
        cvr::anim::GraspPart gp;
        float skin[3],mz=0.f,hl=half[2],widest=0.f;
        uint64_t mesh=0;
        if(shape==2 && hullN<6) {
            mesh=MeshOf(s.proxy);
            if(const MeshProfile* mp=ProfileOf(mesh))hullN=ProfilePoints(*mp,local,half,hull,mz,hl,widest);
        }
        if(shape==2 && hullN>=6 && cvr::anim::GraspPalm(side,skin)) {
            const float rel[3]={skin[0]-p[0],skin[1]-p[1],skin[2]-p[2]};
            const float inv[4]={-q[0],-q[1],-q[2],q[3]};
            float hand[3];
            Rotate(inv,rel,hand);
            hand[2]-=mz;
            const bool found=cvr::anim::GraspPartOf(hull,hullN,hl,hand,gp);
            float dz[3]={0.f,0.f,mz};
            if(found) {
                dz[2]+=gp.z;
                half[0]=half[1]=gp.radius;half[2]=gp.half;
                ends=gp.ends;part=true;
            } else if(gp.lathe.knots>=2 && widest>0.f) {
                half[0]=half[1]=widest;half[2]=hl;           // the whole, as thick as it is drawn
            }
            if(found || (gp.lathe.knots>=2 && widest>0.f)) {
                float m[3];
                Rotate(local+3,dz,m);
                for(int k=0;k<3;++k)local[k]+=m[k];
                Rotate(q,dz,m);
                for(int k=0;k<3;++k)p[k]+=m[k];
            }
        }
        float seatP[3],seatQ[4];
        int kind=-1;
        const bool lathe=gp.lathe.knots>=2 && (part || widest>0.f);
        if(!cvr::anim::GraspTake(side,shape,half,p,q,seatP,seatQ,&kind,ends,part,lathe?&gp.lathe:nullptr)) {
            s.graspTried=false;     // the palm is not measured yet: again at the next step
            return;
        }
        // back to the actor: the seat, the shape's own pose in the actor taken off
        const float inv[4]={-local[3],-local[4],-local[5],local[6]};
        float aq[4],back[3];
        Mul(seatQ,inv,aq);
        Rotate(aq,local,back);
        for(int k=0;k<3;++k)s.seat[k]=seatP[k]-back[k];
        std::copy_n(aq,4,s.seat+3);
        std::copy_n(s.offset,3,s.seatFrom);
        std::copy_n(s.turn,4,s.seatFrom+3);
        s.seatT=0.f;s.seated=true;s.graspSide=side;s.graspKind=kind;
        fingers[side].on=true;fingers[side].used=true;
        static const char* kKinds[8]={"flat","wrap","ball","pinch","edge","from above","from below","three fingers"};
        const volatile float* how=CyberpunkVR_DebugGraspApproach[side];
        Log("[heldbody] proxy %u body %u: seated in the %s hand for a %s grasp (shape %d%s, half %.3f %.3f %.3f, hull %d points, mesh %016llx%s), %.3f m from where it was taken; "
            "palm to its axis %.2f, along it %.2f of its half, that end to up %.2f\n",
            s.proxy,s.body,side?"right":"left",kind>=0 && kind<8?kKinds[kind]:"?",shape,part?(gp.neck?" its neck":" its body"):"",
            half[0],half[1],half[2],hullN,static_cast<unsigned long long>(mesh),lathe?", the whole drawn round it":"",
            std::sqrt((s.seat[0]-s.offset[0])*(s.seat[0]-s.offset[0])+(s.seat[1]-s.offset[1])*(s.seat[1]-s.offset[1])+(s.seat[2]-s.offset[2])*(s.seat[2]-s.offset[2])),
            how[0],how[1],how[2]);
    }
    if(!s.graspWanted) {
        Unseat(s);
        return;
    }
    if(!s.seated)return;
    s.seatT=std::min(1.f,s.seatT+dt/kSeatSec);
    const float w=s.seatT*s.seatT*(3.f-2.f*s.seatT);
    for(int k=0;k<3;++k)s.offset[k]=s.seatFrom[k]+(s.seat[k]-s.seatFrom[k])*w;
    const float* a=s.seatFrom+3;
    const float* b=s.seat+3;
    const float sign=a[0]*b[0]+a[1]*b[1]+a[2]*b[2]+a[3]*b[3]<0.f?-1.f:1.f;
    float n=0.f;
    for(int k=0;k<4;++k) {s.turn[k]=a[k]+(sign*b[k]-a[k])*w;n+=s.turn[k]*s.turn[k];}
    n=std::sqrt(n);
    if(n>1e-4f)for(float& v:s.turn)v/=n;
}
void Unseat(Slot& s) {
    if(s.seated && s.graspSide>=0)fingers[s.graspSide].on=false;
    s.seated=false;s.graspTried=false;
}
// The record of this instance, blended from where its body is to the hand. False if it is not to be touched.
bool Picture(Slot& s,uint8_t* record) {
    if(!s.posed || s.weight<=0)return false;
    auto* fixed=reinterpret_cast<int32_t*>(record);
    auto* quat=reinterpret_cast<float*>(record+0x10);
    if(!UnitLike(quat))return false;
    float body[3];
    for(int k=0;k<3;++k)body[k]=float(fixed[k]/kFixed);
    const float gap[3]={s.hand.p[0]-body[0],s.hand.p[1]-body[1],s.hand.p[2]-body[2]};
    if(gap[0]*gap[0]+gap[1]*gap[1]+gap[2]*gap[2]>kFarFromBody*kFarFromBody) {
        ++s.tooFar;
        return false;
    }
    if(!s.asked) {
        // No longer on the hand: the body flies, and the picture with it. Holding it at the hand's last place would stop it dead for the length of the blend.
        const float conj[4]={-quat[0],-quat[1],-quat[2],quat[3]};
        if(!s.relSet) {
            std::copy_n(gap,3,s.relP);
            Mul(s.hand.q,conj,s.relQ);
            if(s.relQ[3]<0)for(auto& v:s.relQ)v=-v;
            s.relSet=true;
        }
        for(int k=0;k<3;++k)fixed[k]=int32_t(std::lround((body[k]+s.relP[k]*s.weight)*kFixed));
        float part[4]={s.relQ[0]*s.weight,s.relQ[1]*s.weight,s.relQ[2]*s.weight,1+(s.relQ[3]-1)*s.weight};
        const float size=std::sqrt(part[0]*part[0]+part[1]*part[1]+part[2]*part[2]+part[3]*part[3]);
        if(size<1e-4f)return false;
        for(auto& v:part)v/=size;
        float out[4];
        Mul(part,quat,out);
        std::copy_n(out,4,quat);
        return true;
    }
    // Short of a wall: no further from the body than the wall leaves room for.
    const float size=std::sqrt(gap[0]*gap[0]+gap[1]*gap[1]+gap[2]*gap[2]);
    const float keep=size>s.reach && size>1e-5f?s.reach/size:1.f;
    for(int k=0;k<3;++k)fixed[k]=int32_t(std::lround((body[k]+gap[k]*keep*s.weight)*kFixed));
    const float sign=(quat[0]*s.hand.q[0]+quat[1]*s.hand.q[1]+quat[2]*s.hand.q[2]+quat[3]*s.hand.q[3])<0?-1.f:1.f;
    float out[4],n=0;
    for(int k=0;k<4;++k) {
        out[k]=quat[k]+(s.hand.q[k]*sign-quat[k])*s.weight;
        n+=out[k]*out[k];
    }
    n=std::sqrt(n);
    if(n<1e-4f)return false;
    for(int k=0;k<4;++k)quat[k]=out[k]/n;
    return true;
}
// Where the hand has the thing now. The player as the engine has it this frame, the bone as animated for it.
bool HandPose(const Slot& s,Pose& out) {
    if(!CyberpunkVR_PlayerEntityValid || s.bone<0 || s.bone>=g_VRFKSnapCount)return false;
    float bone[3],rot[4],q[4],at[3],world[4],off[3];
    for(int k=0;k<3;++k)bone[k]=g_VRFKSnapPos[s.bone][k];
    for(int k=0;k<4;++k)rot[k]=g_VRFKSnapRot[s.bone][k];
    for(int k=0;k<4;++k)q[k]=CyberpunkVR_PlayerEntityQuat[k];
    if(!UnitLike(rot) || !UnitLike(q))return false;
    Rotate(q,bone,at);
    Mul(q,rot,world);
    Rotate(world,s.offset,off);
    for(int k=0;k<3;++k)out.p[k]=CyberpunkVR_PlayerEntityPos[k]+at[k]+off[k];
    Mul(world,s.turn,out.q);
    const float n=std::sqrt(out.q[0]*out.q[0]+out.q[1]*out.q[1]+out.q[2]*out.q[2]+out.q[3]*out.q[3]);
    if(!std::isfinite(n) || n<1e-4f)return false;
    for(int k=0;k<4;++k)out.q[k]/=n;
    return std::isfinite(out.p[0]) && std::isfinite(out.p[1]) && std::isfinite(out.p[2]);
}
uintptr_t System() {return *reinterpret_cast<uintptr_t*>(base+kSystemRva);}
// The instance behind a proxy, if the listener registered for it is one of these. A listener in the table is alive; the id it knows says it is still this proxy's.
uint8_t* InstanceOf(uint32_t proxy) {
    const uintptr_t system=System();
    if(!system)return nullptr;
    const uintptr_t listener=*reinterpret_cast<uintptr_t*>(system+kListeners+uintptr_t(proxy&0xFFFF)*8);
    if(!listener)return nullptr;
    const uintptr_t table=*reinterpret_cast<uintptr_t*>(listener);
    if(!table || *reinterpret_cast<uintptr_t*>(table+8)!=base+kListenerEntryRva)return nullptr;
    auto* instance=reinterpret_cast<uint8_t*>(listener-kListenerAt);
    return *reinterpret_cast<uint32_t*>(instance+kProxyId)==proxy?instance:nullptr;
}
void Update(uint8_t* instance,void* spanArg,uint32_t count) {
    cvr::frameorder::Note('R');
    if(!anyUsed || count==0 || count>kMostInstances) {originalUpdate(instance,spanArg,count);return;}
    const uint32_t proxy=*reinterpret_cast<uint32_t*>(instance+kProxyId);
    static thread_local uint8_t copy[kRecordSize*kMostInstances];
    bool changed=false;
    AcquireSRWLockExclusive(&lock);
    for(auto& s:slots) {
        if(!s.used || s.proxy!=proxy || s.body>=count)continue;
        if(!changed)std::memcpy(copy,static_cast<Span*>(spanArg)->begin,size_t(count)*kRecordSize);
        if(Picture(s,copy+size_t(s.body)*kRecordSize))changed=true;
        ++s.detours;
        s.detourThread=GetCurrentThreadId();
        LARGE_INTEGER now;QueryPerformanceCounter(&now);
        s.lastDetour=uint64_t(now.QuadPart);
    }
    ReleaseSRWLockExclusive(&lock);
    if(!changed) {originalUpdate(instance,spanArg,count);return;}
    Span span{copy,copy+size_t(count)*kRecordSize};
    originalUpdate(instance,&span,count);
}
void Report(const Slot& s) {
    const uint64_t took=s.firstTick?GetTickCount64()-s.firstTick:0;
    Log("[heldbody] proxy %u body %u of %u let go (%s): %.0f frames a second; %u frames, %u sent at the submit, the engine's own %u (thread %lu), body behind the hand mean %.3f most %.3f m, blocked %u frames, too far %u, submit after engine's own %.1f ms\n",
        s.proxy,s.body,s.instances,s.ownBody?"held by its own body":s.measureOnly?"picture off, measured only":"pictured",took?s.frames*1000.0/took:0.,s.frames,s.sends,s.detours,s.detourThread,s.frames?s.behindSum/s.frames:0.,s.behindMost,s.blockedFrames,s.tooFar,s.detourGaps?s.detourGapSum/s.detourGaps:0.);
}
}  // namespace

bool Hold(uint32_t proxy,uint32_t body,int bone,const float offset[3],const float turn[4],bool blocked,float reach) {
    if(!installed || body>=kMostInstances)return false;
    if(!InstanceOf(proxy))return false;
    const bool picture=g_liveControls.xrCarryPicture!=0;
    const auto now=GetTickCount64();
    AcquireSRWLockExclusive(&lock);
    Slot* slot=nullptr;
    for(auto& s:slots)if(s.used && s.proxy==proxy && s.body==body)slot=&s;
    if(!slot)for(auto& s:slots)if(!s.used) {
        slot=&s;
        std::memset(&s,0,sizeof(s));
        s.used=true;s.proxy=proxy;s.body=body;
        InterlockedExchange(&anyUsed,1);
        break;
    }
    if(slot) {
        slot->asked=true;slot->askedAt=now;slot->bone=bone;slot->blocked=blocked;slot->reach=reach;
        slot->letGo=false;slot->relSet=false;slot->measureOnly=!picture;
        std::copy_n(offset,3,slot->offset);
        std::copy_n(turn,4,slot->turn);
    }
    ReleaseSRWLockExclusive(&lock);
    return slot!=nullptr && picture;
}

void LetGo(uint32_t proxy,uint32_t body) {
    AcquireSRWLockExclusive(&lock);
    for(auto& s:slots)if(s.used && s.proxy==proxy && s.body==body) {
        s.letGo=true;
        s.asked=false;
        s.relSet=false;
    }
    ReleaseSRWLockExclusive(&lock);
}


bool HoldBody(uint32_t proxy,uint32_t body,int bone,const float offset[3],const float turn[4]) {
    LARGE_INTEGER t0,t1,t2,frequency;
    QueryPerformanceCounter(&t0);
    // A broken thing is not looked for again: each look walks every scene (0.3 to 2 ms), and the script asks every frame.
    if(!installed || body>=kMostInstances || cvr::debugview::BodyGone(proxy,body) || !InstanceOf(proxy) || !cvr::physxstep::Ensure())return false;
    QueryPerformanceCounter(&t1);
    const auto now=GetTickCount64();
    bool known=false;
    AcquireSRWLockExclusive(&lock);
    for(const auto& s:slots)if(s.used && s.proxy==proxy && s.body==body && s.ownBody && s.actor)known=true;
    ReleaseSRWLockExclusive(&lock);
    void* actor=nullptr;
    void* scene=nullptr;
    float origin[3]{};
    if(!known) {
        // One that was not found is not looked for again for a second: the script asks every frame it is carried.
        static uint32_t missProxy=0,missBody=0;
        static uint64_t missAt=0;
        if(proxy==missProxy && body==missBody && now-missAt<1000)return false;
        actor=cvr::debugview::RigidDynamicOf(proxy,body,&scene,origin);
        // What taking a thing costs the frame it is taken in: the step's hooks (installed at boot, so nothing), then the
        // walk over the scenes' actors for this one. Said when it is felt.
        QueryPerformanceCounter(&t2);
        QueryPerformanceFrequency(&frequency);
        const double lookMs=double(t2.QuadPart-t1.QuadPart)*1000.0/double(frequency.QuadPart);
        static int slowSaid=0;
        if(lookMs>0.5 && slowSaid++<16)
            Log("[heldbody] proxy %u body %u: looked up in %.2f ms (the step's hooks %.2f ms)\n",proxy,body,lookMs,
                double(t1.QuadPart-t0.QuadPart)*1000.0/double(frequency.QuadPart));
        if(!actor) {
            missProxy=proxy;missBody=body;missAt=now;
            static int said=0;
            if(said++<8)Log("[heldbody] proxy %u body %u: no PxRigidDynamic of a scene with a known origin; it is carried the old way\n",proxy,body);
            return false;
        }
    }
    AcquireSRWLockExclusive(&lock);
    Slot* slot=nullptr;
    for(auto& s:slots)if(s.used && s.proxy==proxy && s.body==body)slot=&s;
    if(!slot)for(auto& s:slots)if(!s.used) {
        slot=&s;
        std::memset(&s,0,sizeof(s));
        s.used=true;s.proxy=proxy;s.body=body;
        InterlockedExchange(&anyUsed,1);
        break;
    }
    bool held=false;
    if(slot && (actor || (slot->ownBody && slot->actor))) {
        if(actor) {
            slot->ownBody=true;slot->actor=actor;slot->scene=scene;slot->driving=false;
            std::copy_n(origin,3,slot->origin);
            Log("[heldbody] proxy %u body %u: held by its own body, actor %p of scene %p, origin (%.2f %.2f %.2f)\n",proxy,body,actor,scene,origin[0],origin[1],origin[2]);
            cvr::debugview::NoteActor(proxy,body,actor);   // its body interface refused the moment this actor is released
        }
        slot->asked=true;slot->askedAt=now;slot->bone=bone;slot->blocked=false;slot->reach=1000.f;
        slot->letGo=false;slot->relSet=false;slot->measureOnly=true;
        if(!slot->seated) {
            std::copy_n(offset,3,slot->offset);
            std::copy_n(turn,4,slot->turn);
        }
        held=true;
    }
    ReleaseSRWLockExclusive(&lock);
    return held;
}

void SetHeldFilter(uint32_t word0,uint32_t word2) {heldFilterOr[0]=word0;heldFilterOr[1]=word2;}

int HoldingMask() {
    int mask=0;
    AcquireSRWLockShared(&lock);
    for(const auto& s:slots)
        if(s.used && s.ownBody && s.actor && s.driving && !s.letGo)
            mask|=s.bone==g_VRRightBoneIdx?2:s.bone==g_VRLeftBoneIdx?1:0;
    ReleaseSRWLockShared(&lock);
    return mask;
}

// How many shapes an actor has, the first one's geometry type, and its convex mesh's point count (-1 when not read).
void ShapesOf(void* actor,uint32_t& n,int& type,int& points) {
    n=0;type=-1;points=-1;
    __try {
        void* shapes[8]{};
        n=reinterpret_cast<uint32_t(*)(void*,void**,uint32_t,uint32_t)>((*reinterpret_cast<void***>(actor))[kGetShapes])(actor,shapes,8,0);
        if(n<1)return;
        void** st=*reinterpret_cast<void***>(shapes[0]);
        type=reinterpret_cast<int(*)(void*)>(st[kShapeGeometryType])(shapes[0]);
        if(type!=4)return;
        alignas(8) uint8_t g[64]{};
        if(!reinterpret_cast<bool(*)(void*,void*)>(st[kShapeGetConvex])(shapes[0],g))return;
        void* mesh=*reinterpret_cast<void**>(g+32);
        if(!mesh)return;
        points=int(reinterpret_cast<uint32_t(*)(void*)>((*reinterpret_cast<void***>(mesh))[kConvexNbVertices])(mesh));
    } __except(EXCEPTION_EXECUTE_HANDLER) {}
}
// What the grasp makes of a body, for VRBodyGraspShape (a test tool): its primitive and pose in the body, its hull or its
// drawn mesh's profile, and the part a palm at either end and at its middle would close on.
void GraspShapeText(uint32_t proxy,uint32_t body,char* text,int size) {
    text[0]=0;
    void* actor=cvr::debugview::ActorOf(proxy,body);
    if(!actor)return;
    int shape=0,hullN=0;
    float half[3]{},local[7]{};
    static float hull[3*kHullMost];
    if(!GraspShape(actor,shape,half,local,hull,&hullN)) {std::snprintf(text,size,"no grasp shape");return;}
    uint32_t nShapes=0;
    int type=-1,points=-1;
    ShapesOf(actor,nShapes,type,points);
    const uint64_t mesh=MeshOf(proxy);
    const MeshProfile* mp=ProfileOf(mesh);
    float mz=0.f,hl=half[2],widest=0.f;
    if(shape==2 && hullN<6 && mp)hullN=ProfilePoints(*mp,local,half,hull,mz,hl,widest);
    int at=std::snprintf(text,size,"shape %d half %.3f %.3f %.3f hull %d (shapes %u, type %d, mesh points %d) at %.3f %.3f %.3f turn %.3f %.3f %.3f %.3f; "
                         "mesh %016llx %s, its middle %.3f along, half %.3f, widest %.3f",shape,half[0],half[1],half[2],hullN,nShapes,type,points,
                         local[0],local[1],local[2],local[3],local[4],local[5],local[6],static_cast<unsigned long long>(mesh),mp?"profiled":"no profile",mz,hl,widest);
    if(shape!=2 || hullN<6)return;
    static const char* kWhere[3]={"-Z end","middle","+Z end"};
    for(int w=0;w<3 && at>0 && at<size;++w) {
        const float hand[3]={half[0]+.02f,0.f,(w-1)*hl*.9f};
        cvr::anim::GraspPart gp;
        if(cvr::anim::GraspPartOf(hull,hullN,hl,hand,gp))
            at+=std::snprintf(text+at,size-at,"; %s: %s z %.3f half %.3f r %.3f ends %d",kWhere[w],gp.neck?"neck":"body",gp.z,gp.half,gp.radius,gp.ends);
        else
            at+=std::snprintf(text+at,size-at,"; %s: whole",kWhere[w]);
    }
}
bool GraspBody(uint32_t proxy,uint32_t body,bool on) {
    bool known=false;
    AcquireSRWLockExclusive(&lock);
    for(auto& s:slots)if(s.used && s.proxy==proxy && s.body==body && s.ownBody) {
        s.graspWanted=on;
        known=true;
    }
    ReleaseSRWLockExclusive(&lock);
    return known;
}

void LetGoBody(uint32_t proxy,uint32_t body,const float velocity[3],const float spin[3],bool thrown) {
    AcquireSRWLockExclusive(&lock);
    for(auto& s:slots)if(s.used && s.proxy==proxy && s.body==body) {
        s.throwGiven=thrown?1.f:0.f;
        std::copy_n(velocity,3,s.throwV);
        std::copy_n(spin,3,s.throwW);
        s.letGo=true;
        s.asked=false;
    }
    ReleaseSRWLockExclusive(&lock);
}

// A HELD ACTOR CAN DIE IN THE HAND. A bowl knocked on the table breaks, and the game releases its actor; PhysX puts the
// memory back in its pool, whose free list lives in the actor's first word, where the table was. Steering it on through
// the pointer kept here called a function out of that free list (2026-11-08 21:08, ReportQueue ...-210801: Steer, a
// call through a heap "table"), and __try does not save it: the engine's vectored handler takes the fault first. So
// before anything touches it, each step: the table is still PxRigidDynamic's and the userData still this body's
// (proxy, body), the words FindActorIn matched it by. A pool keeps its slabs, so reading them is safe. Not asked of
// VirtualQuery: that cost 7 to 12 ms a call on this heap (DebugView.cpp, ActorIs), on the physics step.
constexpr uintptr_t kRigidDynamicTableRva=0x20B2E8;
bool Alive(const Slot& s) {
    static const uintptr_t table=reinterpret_cast<uintptr_t>(GetModuleHandleA("PhysX3_x64.dll"))+kRigidDynamicTableRva;
    const auto* a=static_cast<const uint8_t*>(s.actor);
    if(!a || table==kRigidDynamicTableRva)return false;
    return *reinterpret_cast<const uintptr_t*>(a)==table && *reinterpret_cast<const uint32_t*>(a+0x10)==s.proxy &&
           *reinterpret_cast<const uint16_t*>(a+0x14)==uint16_t(s.body);
}
// Gone: nothing of it is touched again, not even to put back what holding it changed (the actor that had it is no more).
void Forget(Slot& s,const char* why) {
    Log("[heldbody] proxy %u body %u: its actor %p is %s; let go of it without touching it\n",s.proxy,s.body,s.actor,why);
    cvr::debugview::NoteGone(s.proxy,s.body);   // and its body interface is refused from now on (HandBody.cpp)
    Unseat(s);
    s.graspWanted=false;
    s.actor=nullptr;
    s.driving=false;
    s.shapeCount=0;
    s.targets=0;
    s.letGo=true;
    s.asked=false;
}

// QUERIES OFF, SET AT THE STEP. A carried thing is hidden from the player's movement and from rays (queries off) while it is
// carried, and given them back a while after it is let go. Through the game's SetIsQueryable that was queued for the proxy
// from the script thread and done whenever the game got to it: a stack of cups thrown out of the hand broke as it landed,
// the queries given back in that same moment put the freed actor's shape back into the scene's query tree, and the next
// ray through that place called through the freed shape (2026-11-10 16:27 and 21:39, ReportQueue ...-162755,
// ...-213948). No check on the script thread closes that: the game frees the actor between the check and the call. So a
// body with a PhysX actor of its own is hidden here, just before PxScene::simulate, when nothing else
// may touch the scene and a release of this frame has already happened (the actor's first word then the pool's free
// link, not the table): only on an actor that is still this body's. A gone one is let be, and its body interface refused.
namespace {
constexpr int kQueryWants=16,kQueryShapes=8,kShapeSetQueryFilter=22,kShapeGetQueryFilter=23;
struct QueryWant {
    bool used,wantOn,off;
    uint32_t proxy,body;
    void* actor;
    void* scene;
    uint32_t shapeCount;
    uint32_t own[kQueryShapes][4];   // each shape's own query filter data, kept while it is hidden
};
QueryWant queryWants[kQueryWants]{};
SRWLOCK queryLock=SRWLOCK_INIT;
std::atomic<bool> anyQuery{false};

bool ActorStill(const void* actor,uint32_t proxy,uint32_t body) {
    static const uintptr_t table=reinterpret_cast<uintptr_t>(GetModuleHandleA("PhysX3_x64.dll"))+kRigidDynamicTableRva;
    const auto* a=static_cast<const uint8_t*>(actor);
    if(!a || table==kRigidDynamicTableRva)return false;
    return *reinterpret_cast<const uintptr_t*>(a)==table && *reinterpret_cast<const uint32_t*>(a+0x10)==proxy &&
           *reinterpret_cast<const uint16_t*>(a+0x14)==uint16_t(body);
}
// Hidden from the game's queries: a shape is hit only where words 2 and 3 of its query filter data meet the query's
// (the game's pre-filter, exe+0x21BD88), so those are cleared, each shape's own kept in `w`; or put back from it. Not
// PxShapeFlag::eSCENE_QUERY_SHAPE: PhysX 3.4 keeps the shape in its query tree whatever that flag says, and rays went on
// hitting it. A plain write of the shape's data: nothing is put into or taken out of the tree. False if not read.
bool SetQueries(QueryWant& w,bool on) {
    __try {
        void* shapes[kQueryShapes]{};
        void** table=*reinterpret_cast<void***>(w.actor);
        const uint32_t n=reinterpret_cast<uint32_t(*)(void*,void**,uint32_t,uint32_t)>(table[kGetShapes])(w.actor,shapes,kQueryShapes,0);
        if(!on)w.shapeCount=n<kQueryShapes?n:kQueryShapes;
        for(uint32_t i=0;i<w.shapeCount && i<n;++i) {
            void** st=*reinterpret_cast<void***>(shapes[i]);
            if(!on) {
                reinterpret_cast<void*(*)(void*,uint32_t*)>(st[kShapeGetQueryFilter])(shapes[i],w.own[i]);
                const uint32_t hidden[4]={w.own[i][0],w.own[i][1],0u,0u};
                reinterpret_cast<void(*)(void*,const uint32_t*)>(st[kShapeSetQueryFilter])(shapes[i],hidden);
            } else {
                reinterpret_cast<void(*)(void*,const uint32_t*)>(st[kShapeSetQueryFilter])(shapes[i],w.own[i]);
            }
        }
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
void QueriesAtStep(void* scene) {
    if(!anyQuery.load(std::memory_order_acquire))return;
    AcquireSRWLockExclusive(&queryLock);
    bool any=false;
    for(auto& w:queryWants) {
        if(!w.used)continue;
        if(w.scene!=scene) {any=true;continue;}
        if(!ActorStill(w.actor,w.proxy,w.body)) {
            Log("[heldbody] proxy %u body %u: its actor %p is gone; its queries let be\n",w.proxy,w.body,w.actor);
            cvr::debugview::NoteGone(w.proxy,w.body);
            w.used=false;
            continue;
        }
        if(w.wantOn) {
            if(w.off)SetQueries(w,true);
            w.used=false;
            continue;
        }
        if(!w.off) {
            if(SetQueries(w,false)) {
                w.off=true;
            } else {
                w.used=false;
                continue;
            }
        }
        any=true;
    }
    anyQuery.store(any,std::memory_order_release);
    ReleaseSRWLockExclusive(&queryLock);
}
}  // namespace

int QueryWanted(uint32_t proxy,uint32_t body,bool on) {
    if(!proxy)return 0;
    AcquireSRWLockExclusive(&queryLock);
    for(auto& w:queryWants)
        if(w.used && w.proxy==proxy && w.body==body) {
            w.wantOn=on;
            anyQuery.store(true,std::memory_order_release);
            ReleaseSRWLockExclusive(&queryLock);
            return 1;
        }
    ReleaseSRWLockExclusive(&queryLock);
    // queries back on for a body never set off here: the game's, as before (a gone one is refused by its interface)
    if(on || !cvr::physxstep::Ensure())return 0;
    void* scene=nullptr;
    float origin[3]{};
    void* actor=cvr::debugview::RigidDynamicOf(proxy,body,&scene,origin);
    if(!actor || !scene)return 0;
    AcquireSRWLockExclusive(&queryLock);
    QueryWant* free=nullptr;
    for(auto& w:queryWants)
        if(!w.used) {free=&w;break;}
    if(free) {
        *free=QueryWant{};
        free->used=true;
        free->proxy=proxy;
        free->body=body;
        free->actor=actor;
        free->scene=scene;
        anyQuery.store(true,std::memory_order_release);
    }
    ReleaseSRWLockExclusive(&queryLock);
    if(!free) {
        static bool said=false;
        if(!said) {said=true;Log("[heldbody] queries: all %d places taken; proxy %u body %u left to the game\n",kQueryWants,proxy,body);}
        return 0;
    }
    cvr::debugview::NoteActor(proxy,body,actor);   // its body interface refused the moment this actor is released
    return 1;
}

void BeforeSimulate(void* scene,float dt) {
    QueriesAtStep(scene);
    if(!installed || !anyUsed)return;
    const auto tick=GetTickCount64();
    AcquireSRWLockExclusive(&lock);
    for(auto& s:slots) {
        if(!s.used || !s.ownBody || !s.actor || s.scene!=scene)continue;
        if(!InstanceOf(s.proxy)) {s.actor=nullptr;s.driving=false;continue;}   // gone with its sector, and its actor with it
        if(!Alive(s)) {Forget(s,"released (broken?)");continue;}
        const bool holding=!s.letGo && tick-s.askedAt<=kUnaskedMs;
        if(holding) {
            SeatFor(s,dt);
            Pose pose;
            if(!HandPose(s,pose))continue;
            SteerIn in{};
            std::copy_n(pose.p,3,in.target);
            std::copy_n(pose.q,4,in.target+3);
            std::copy_n(s.origin,3,in.origin);
            SceneGravity(scene,in.gravity);
            in.dt=dt;
            in.first=!s.driving;
            // taken off a stack: what stood on it comes down (before this step moves it)
            if(in.first)cvr::physhands::Taken(s.actor);
            if(in.first && Filter(s.actor,s.shapes,s.filters,&s.shapeCount) && s.shapeCount && (cvr::physhands::FilterAgain(scene,s.actor),true))
                Log("[heldbody] proxy %u body %u: %u shapes held apart from the loose things, or as asked (was %08x %08x %08x %08x)\n",
                    s.proxy,s.body,s.shapeCount,s.filters[0][0],s.filters[0][1],s.filters[0][2],s.filters[0][3]);
            float gap=0.f,mass=0.f;
            if(Steer(s.actor,in,s.saved,&gap,&mass)) {
                if(in.first)Log("[heldbody] proxy %u body %u: steered into the hand, %.2f kg, %.3f m off at the first step (%.1f ms)\n",s.proxy,s.body,mass,gap,dt*1000.f);
                s.driving=true;
                ++s.targets;
            } else if(s.driveFailed++<4) Log("[heldbody] proxy %u body %u: PhysX refused\n",s.proxy,s.body);
        } else if(s.driving) {
            Unseat(s);
            s.graspWanted=false;
            const bool freed=Unsteer(s.actor,s.saved,s.throwV,s.throwW,s.throwGiven>0.5f);
            if(s.shapeCount && Unfilter(s.shapes,s.filters,s.shapeCount))cvr::physhands::FilterAgain(scene,s.actor);
            s.shapeCount=0;
            Log("[heldbody] proxy %u body %u let go after %u steps in the hand: %s, %s (%.2f %.2f %.2f) m/s\n",
                s.proxy,s.body,s.targets,freed?"its own again":"PhysX refused",s.throwGiven>0.5f?"thrown at":"as it went, not",s.throwV[0],s.throwV[1],s.throwV[2]);
            s.driving=false;
            s.targets=0;
        }
    }
    ReleaseSRWLockExclusive(&lock);
}

void AtFrameSubmit() {
    cvr::frameorder::NoteSubmit();
    if(installed) {
        AcquireSRWLockExclusive(&lock);
        EaseFingers();
        ReleaseSRWLockExclusive(&lock);
    }
    if(!installed || !anyUsed)return;
    static uint64_t last=0;
    LARGE_INTEGER now,frequency;
    QueryPerformanceCounter(&now);QueryPerformanceFrequency(&frequency);
    const float dt=last?std::min(float(double(now.QuadPart-last)/double(frequency.QuadPart)),.1f):0.f;
    last=now.QuadPart;
    const auto tick=GetTickCount64();
    static uint8_t copy[kRecordSize*kMostInstances];
    AcquireSRWLockExclusive(&lock);
    LONG used=0;
    for(auto& s:slots) {
        if(!s.used)continue;
        s.asked=!s.letGo && tick-s.askedAt<=kUnaskedMs;
        if(!s.spun && !s.measureOnly) {
            s.spun=true;
            float was=0;
            const bool raised=cvr::debugview::RaiseSpin(s.proxy,s.body,kMostSpin,&was);
            Log("[heldbody] proxy %u body %u: most spin %s, was %.1f rad/s\n",s.proxy,s.body,raised?"raised":"left alone",was);
        }
        if(s.blocked && s.asked)++s.blockedFrames;
        // After the last ask the picture stays where the hand last had it, and goes to the body: the hand is off elsewhere by then.
        if(s.asked) {
            Pose pose;
            if(HandPose(s,pose)) {s.hand=pose;s.posed=true;}
        }
        const bool toHand=s.asked && !s.blocked && s.posed && !s.measureOnly;
        s.weight=toHand?std::min(1.f,s.weight+dt/kBlendIn):std::max(0.f,s.weight-dt/kBlendOut);
        uint8_t* instance=InstanceOf(s.proxy);
        // a body held by itself is not given up before the step has put its damping back, unless it is gone
        if(!instance || (!s.asked && s.weight<=0 && !(s.ownBody && s.driving))) {
            Report(s);
            Unseat(s);
            s.used=false;
            continue;
        }
        ++used;
        const uintptr_t holder=*reinterpret_cast<uintptr_t*>(System()+kRecords+uintptr_t(s.proxy&0xFFFF)*8);
        if(!holder)continue;
        auto* records=*reinterpret_cast<uint8_t**>(holder);
        const uint32_t count=*reinterpret_cast<uint32_t*>(holder+8);
        if(!records || count==0 || count>kMostInstances || s.body>=count)continue;
        std::memcpy(copy,records,size_t(count)*kRecordSize);
        uint8_t* record=copy+size_t(s.body)*kRecordSize;
        // what the eye will see of the body alone
        const auto* fixed=reinterpret_cast<int32_t*>(record);
        const double gap[3]={s.hand.p[0]-fixed[0]/kFixed,s.hand.p[1]-fixed[1]/kFixed,s.hand.p[2]-fixed[2]/kFixed};
        const double behind=std::sqrt(gap[0]*gap[0]+gap[1]*gap[1]+gap[2]*gap[2]);
        if(s.posed) {
            ++s.frames;
            s.behindSum+=behind;
            s.behindMost=std::max(s.behindMost,behind);
            if(s.frames==1) {s.instances=count;s.firstTick=GetTickCount64();}
            if(s.frames==1)Log("[heldbody] proxy %u body %u of %u: first frame, body at (%.3f %.3f %.3f), hand has it at (%.3f %.3f %.3f), quat %.3f %.3f %.3f %.3f and %.3f %.3f %.3f %.3f, submit thread %lu\n",
                s.proxy,s.body,count,fixed[0]/kFixed,fixed[1]/kFixed,fixed[2]/kFixed,s.hand.p[0],s.hand.p[1],s.hand.p[2],
                reinterpret_cast<float*>(record+0x10)[0],reinterpret_cast<float*>(record+0x10)[1],reinterpret_cast<float*>(record+0x10)[2],reinterpret_cast<float*>(record+0x10)[3],
                s.hand.q[0],s.hand.q[1],s.hand.q[2],s.hand.q[3],GetCurrentThreadId());
        }
        if(s.lastDetour && s.lastDetour!=s.lastSubmit) {
            s.detourGapSum+=double(now.QuadPart-s.lastDetour)*1000.0/double(frequency.QuadPart);
            ++s.detourGaps;
            s.lastSubmit=s.lastDetour;
        }
        if(!Picture(s,record))continue;
        ++s.sends;
        // Two things on one instance are rare, and each is put right by the other's next frame.
        ReleaseSRWLockExclusive(&lock);
        Span span{copy,copy+size_t(count)*kRecordSize};
        originalUpdate(instance,&span,count);
        AcquireSRWLockExclusive(&lock);
    }
    if(!used)InterlockedExchange(&anyUsed,0);
    ReleaseSRWLockExclusive(&lock);
}

namespace {
// The record the physics system has for a body now. A torn read gives a wrong box for one frame, nothing worse.
bool ReadRecord(uint32_t proxy,uint32_t body,uint8_t* out) {
    __try {
        const uintptr_t system=System();
        if(!system)return false;
        const uintptr_t holder=*reinterpret_cast<uintptr_t*>(system+kRecords+uintptr_t(proxy&0xFFFF)*8);
        if(!holder)return false;
        auto* records=*reinterpret_cast<uint8_t**>(holder);
        const uint32_t count=*reinterpret_cast<uint32_t*>(holder+8);
        if(!records || count==0 || count>kMostInstances || body>=count)return false;
        std::memcpy(out,records+size_t(body)*kRecordSize,kRecordSize);
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
void PoseOf(const uint8_t* record,float* out) {
    const auto* fixed=reinterpret_cast<const int32_t*>(record);
    for(int k=0;k<3;++k)out[k]=float(fixed[k]/kFixed);
    std::memcpy(out+3,record+0x10,16);
}
}  // namespace

bool View(uint32_t proxy,uint32_t body,float bodyPose[7],float picture[7],float hand[7],bool* handKnown,float* weight) {
    if(!installed || !anyUsed)return false;
    Slot s{};
    bool found=false;
    AcquireSRWLockExclusive(&lock);
    for(const auto& t:slots)if(t.used && t.proxy==proxy && t.body==body) {s=t;found=true;break;}
    ReleaseSRWLockExclusive(&lock);
    if(!found || !InstanceOf(proxy))return false;
    // the hand as animated now, as the next submit will have it
    if(s.asked) {
        Pose pose;
        if(HandPose(s,pose)) {s.hand=pose;s.posed=true;}
    }
    uint8_t record[kRecordSize],drawn[kRecordSize];
    if(!ReadRecord(proxy,body,record))return false;
    std::memcpy(drawn,record,kRecordSize);
    PoseOf(record,bodyPose);
    // a copy of the slot: what Picture() keeps of a let-go thing stays the submit's own
    PoseOf(Picture(s,drawn)?drawn:record,picture);
    *handKnown=s.posed;
    if(s.posed) {std::copy_n(s.hand.p,3,hand);std::copy_n(s.hand.q,4,hand+3);}
    else std::copy_n(bodyPose,7,hand);
    *weight=s.weight;
    return true;
}

namespace {
bool Install() {
    if(!cvr::game_build::Supported())return false;
    base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    auto* target=reinterpret_cast<void*>(base+kUpdateRva);
    if(std::memcmp(target,"\x48\x8b\xc4\x48\x89\x58\x18\x48\x89\x50\x10\x55\x56\x57\x41\x54",16) ||
       std::memcmp(reinterpret_cast<void*>(base+kListenerEntryRva),"\x48\x83\xec\x38\x4c\x8b\x42\x08\x48\x81\xc1\x00\xff\xff\xff",15)) {
        Log("[heldbody] these are not the functions this was read from: carried things stay as they were\n");
        return false;
    }
    if(MH_CreateHook(target,reinterpret_cast<void*>(&Update),reinterpret_cast<void**>(&originalUpdate))!=MH_OK)return false;
    if(MH_EnableHook(target)!=MH_OK) {MH_RemoveHook(target);return false;}
    installed=true;
    return true;
}
CVR_HOOK("HeldBody",::cvr::hooks::Stage::Boot,84,Install);
}  // namespace
}  // namespace cvr::heldbody

void VRHeldPictureLetGo(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    int32_t proxy=0,body=0;
    RED4ext::GetParameter(stack,&proxy);
    RED4ext::GetParameter(stack,&body);
    stack->code++;
    if(proxy>0 && body>=0)cvr::heldbody::LetGo(uint32_t(proxy),uint32_t(body));
    if(out)*out=true;
}

// THE GAME'S OWN OUTLINE ON A THING WITHOUT AN ENTITY. entRenderHighlightEvent ends in sub_14079FE8C(render object, params) and that in a command of the render scene: slot 0x2D8 (sub_1407A0190), arguments &object+8, &object+0x10,
// on, mask, opacity, 0. Read in the exe, all callers (sub_14079FE8C, sub_140402428, sub_1403DA0F4) alike. Mask: 1 an outline, 2 a fill, the index <<2, 0x40 through walls. The render side (sub_14067AAB4) calls slot 0xD0 of what +0x10 holds
// and goes on only for what that answers with a type (+0x7D >>3) of 1, 5 or 7. A prop's object is at instance+0xC8 and is of the same make: sub_14020A038 reads it the same way, +8 a thing with a vtable, +0x10 the handle. The first try passed
// +0x10 first and null second, read as fill and outline: a crash on the render thread at sub_14067AAFF. The whole object lights up: a node's instances together.
namespace {
// The regions VirtualQuery said were readable are kept (it costs 7 to 12 ms a call on this heap: DebugView.cpp, Readable).
bool Readable(uintptr_t at,size_t size) {
    if(at<0x10000 || at>=0x00007FFFFFFFFFFFull)return false;
    struct Region {uintptr_t lo,hi;};
    static thread_local Region known[8]{};
    static thread_local int knownNext=0;
    for(const auto& r:known)if(at>=r.lo && at+size<=r.hi)return true;
    MEMORY_BASIC_INFORMATION m{};
    if(!VirtualQuery(reinterpret_cast<void*>(at),&m,sizeof(m)) || m.State!=MEM_COMMIT || (m.Protect&(PAGE_NOACCESS|PAGE_GUARD)))return false;
    const uintptr_t lo=reinterpret_cast<uintptr_t>(m.BaseAddress),end=lo+m.RegionSize;
    known[knownNext]=Region{lo,end};
    knownNext=(knownNext+1)%8;
    return at+size<=end;
}
// In the image, in .text: a slot of a vtable that has it.
bool CodeOfImage(uintptr_t at) {
    static uintptr_t lo=0,hi=0;
    if(!lo) {
        const auto* dos=reinterpret_cast<const IMAGE_DOS_HEADER*>(cvr::heldbody::base);
        const auto* nt=reinterpret_cast<const IMAGE_NT_HEADERS64*>(cvr::heldbody::base+dos->e_lfanew);
        const auto* section=IMAGE_FIRST_SECTION(nt);
        for(int i=0;i<nt->FileHeader.NumberOfSections;++i,++section)if(!std::memcmp(section->Name,".text",5)) {
            lo=cvr::heldbody::base+section->VirtualAddress;
            hi=lo+section->Misc.VirtualSize;
        }
    }
    return at>=lo && at<hi;
}
void* AnswerOf(uintptr_t held) {
    __try {
        using Get=void*(*)(void*);
        return reinterpret_cast<Get>((*reinterpret_cast<void***>(held))[0xD0/8])(reinterpret_cast<void*>(held));
    } __except(EXCEPTION_EXECUTE_HANDLER) {return nullptr;}
}
}  // namespace
void VRThingHighlight(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    int32_t proxy=0,fill=0,outline=0;
    bool through=false;
    RED4ext::GetParameter(stack,&proxy);
    RED4ext::GetParameter(stack,&fill);
    RED4ext::GetParameter(stack,&outline);
    RED4ext::GetParameter(stack,&through);
    stack->code++;
    if(out)*out=false;
    using namespace cvr::heldbody;
    if(!base || proxy<=0)return;
    uint8_t* instance=InstanceOf(uint32_t(proxy));
    if(!instance)return;
    const uintptr_t object=*reinterpret_cast<uintptr_t*>(instance+0xC8);
    if(!Readable(object,0x18))return;
    const uintptr_t owner=*reinterpret_cast<uintptr_t*>(object+8),held=*reinterpret_cast<uintptr_t*>(object+0x10);
    if(!Readable(owner,16) || !Readable(held,16))return;
    const uintptr_t table=*reinterpret_cast<uintptr_t*>(held);
    if(!Readable(table,0xD8) || !CodeOfImage(*reinterpret_cast<uintptr_t*>(table+0xD0)))return;
    void* answer=AnswerOf(held);
    const int type=answer && Readable(reinterpret_cast<uintptr_t>(answer)+0x7D,1)?*(reinterpret_cast<uint8_t*>(answer)+0x7D)>>3:-1;
    {
        static int saidType[64]={};
        if(type>=0 && type<64 && saidType[type]<1) {
            ++saidType[type];
            Log("[heldbody] outline on proxy %d: render object type %d, %s\n",proxy,type,type==1 || type==5 || type==7?"it takes one":"it does not take one: its lines instead");
        } else if(type<0) {
            static bool said=false;
            if(!said) {said=true;Log("[heldbody] outline on proxy %d: no render object answers\n",proxy);}
        }
    }
    if(type!=1 && type!=5 && type!=7)return;
    void* renderer=*reinterpret_cast<void**>(base+0x3BBCAE8);
    if(!renderer)return;
    using GetScene=void*(*)(void*);
    void* scene=reinterpret_cast<GetScene>((*reinterpret_cast<void***>(renderer))[0x4D0/8])(renderer);
    if(!scene)return;
    void* entry=(*reinterpret_cast<void***>(scene))[0x2D8/8];
    if(reinterpret_cast<uintptr_t>(entry)!=base+0x7A0190) {
        static bool said=false;
        if(!said) {said=true;Log("[heldbody] the scene's highlight is not where it was read: no highlight\n");}
        return;
    }
    const bool on=fill>0 || outline>0;
    uint8_t mask=through?0x40:0;
    if(fill>0)mask|=uint8_t(fill<<2)|2;
    else if(outline>0)mask|=uint8_t(outline<<2);
    if(outline>0)mask|=1;
    using Command=void(*)(void*,void*,void*,uint8_t,uint8_t,float,uint8_t);
    reinterpret_cast<Command>(entry)(scene,reinterpret_cast<void*>(object+8),reinterpret_cast<void*>(object+0x10),on?1:0,mask,1.f,0);
    if(out)*out=true;
}

// VRHeldBody(proxy, body, bone, offset, turn) -> Bool: the thing held by its own body, steered at each PhysX step to where the hand
// has it (offset and turn in that hand's frame, as for VRHeldPicture). False: it cannot be, carry it as before.
void VRHeldBody(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    int32_t proxy=0,body=0,bone=-1;
    RED4ext::Vector4 offset{},turn{};
    RED4ext::GetParameter(stack,&proxy);
    RED4ext::GetParameter(stack,&body);
    RED4ext::GetParameter(stack,&bone);
    RED4ext::GetParameter(stack,&offset);
    RED4ext::GetParameter(stack,&turn);
    stack->code++;
    const float o[3]={offset.X,offset.Y,offset.Z},t[4]={turn.X,turn.Y,turn.Z,turn.W};
    const bool on=proxy>0 && body>=0 && bone>=0 && std::isfinite(o[0]+o[1]+o[2]+t[0]+t[1]+t[2]+t[3]) &&
        cvr::heldbody::HoldBody(uint32_t(proxy),uint32_t(body),bone,o,t);
    if(out)*out=on;
}

// VRHeldBodyGrasp(proxy, body, on) -> Bool: a body held by VRHeldBody is (on) or is not (off) in one hand alone: seated there
// for the grasp its shape and the way the hand came to it make, the fingers closed on it (src/Anim/GraspPose.cpp). False if it is
// not held by its own body.
void VRHeldBodyGrasp(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    int32_t proxy=0,body=0;
    bool on=false;
    RED4ext::GetParameter(stack,&proxy);
    RED4ext::GetParameter(stack,&body);
    RED4ext::GetParameter(stack,&on);
    stack->code++;
    const bool known=proxy>0 && body>=0 && cvr::heldbody::GraspBody(uint32_t(proxy),uint32_t(body),on);
    if(out)*out=known;
}

// VRBodyGraspShape(proxy, body) -> String: the shape a hand would close on (src/Anim/GraspPose.cpp's primitive, its half sizes,
// its hull's points) and, for a long round one, the part a palm at either end and at its middle would take (a test tool's look
// at a thing before taking it; empty when it has no actor).
void VRBodyGraspShape(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,RED4ext::CString* out,int64_t) {
    int32_t proxy=0,body=0;
    RED4ext::GetParameter(stack,&proxy);
    RED4ext::GetParameter(stack,&body);
    stack->code++;
    char text[512]="";
    if(proxy>0 && body>=0)cvr::heldbody::GraspShapeText(uint32_t(proxy),uint32_t(body),text,int(sizeof(text)));
    if(out)*out=RED4ext::CString(text);
}

// VRHeldBodyFilter(word0, word2): the bits a held body's shapes get added to their simulation filter, from the next hold.
// word2 0xFFFFFFFF is "its own group"; 0, 0 (the default) leaves the game's filter alone. For finding what the game's
// filter wants, not for play: see heldFilterOr.
void VRHeldBodyFilter(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    int32_t w0=0,w2=-1;
    RED4ext::GetParameter(stack,&w0);
    RED4ext::GetParameter(stack,&w2);
    stack->code++;
    cvr::heldbody::SetHeldFilter(uint32_t(w0),uint32_t(w2));
    if(out)*out=true;
}

// VRHeldBodyLetGo(proxy, body, velocity, spin) -> Bool: lets a body held by VRHeldBody go at the next step, with that throw when
// velocity.w is 1 (0: it goes on as it was going).
void VRHeldBodyLetGo(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    int32_t proxy=0,body=0;
    RED4ext::Vector4 velocity{},spin{};
    RED4ext::GetParameter(stack,&proxy);
    RED4ext::GetParameter(stack,&body);
    RED4ext::GetParameter(stack,&velocity);
    RED4ext::GetParameter(stack,&spin);
    stack->code++;
    float v[3]={velocity.X,velocity.Y,velocity.Z},w[3]={spin.X,spin.Y,spin.Z};
    bool thrown=velocity.W>0.5f;
    if(!std::isfinite(v[0]+v[1]+v[2]+w[0]+w[1]+w[2]))thrown=false;
    if(proxy>0 && body>=0)cvr::heldbody::LetGoBody(uint32_t(proxy),uint32_t(body),v,w,thrown);
    if(out)*out=proxy>0 && body>=0;
}

// VRHeldPictureView(proxy, body, part): a carried thing, for a debug view. 0 its body's position and 1 its rotation, as the physics
// system has them (what hits things); 2 its picture's position with the picture's weight as w (0 on the body, 1 on the hand) and
// 3 its rotation, as the next submit will send them; 4 where the hand has it, w 1 once known, and 5 that rotation. With the
// picture setting off the thing is still measured, and the picture is its body. All zero: not a carried thing.
void VRHeldPictureView(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,RED4ext::Vector4* out,int64_t) {
    int32_t proxy=0,body=0,part=0;
    RED4ext::GetParameter(stack,&proxy);
    RED4ext::GetParameter(stack,&body);
    RED4ext::GetParameter(stack,&part);
    stack->code++;
    if(!out)return;
    out->X=out->Y=out->Z=out->W=0.f;
    float b[7],p[7],h[7],w=0.f;
    bool known=false;
    if(proxy<=0 || body<0 || !cvr::heldbody::View(uint32_t(proxy),uint32_t(body),b,p,h,&known,&w))return;
    const float* v=part<2?b:part<4?p:h;
    if(part%2==0) {out->X=v[0];out->Y=v[1];out->Z=v[2];out->W=part==0?1.f:part==2?w:(known?1.f:0.f);}
    else {out->X=v[3];out->Y=v[4];out->Z=v[5];out->W=v[6];}
}

// VRHeldPicture(proxy, body, bone, offset with the reach as its w, turn as x y z w, blocked)
void VRHeldPicture(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    int32_t proxy=0,body=0,bone=-1;
    RED4ext::Vector4 offset{},turn{};
    bool blocked=false;
    RED4ext::GetParameter(stack,&proxy);
    RED4ext::GetParameter(stack,&body);
    RED4ext::GetParameter(stack,&bone);
    RED4ext::GetParameter(stack,&offset);
    RED4ext::GetParameter(stack,&turn);
    RED4ext::GetParameter(stack,&blocked);
    stack->code++;
    const float o[3]={offset.X,offset.Y,offset.Z},t[4]={turn.X,turn.Y,turn.Z,turn.W};
    const bool on=proxy>0 && body>=0 && std::isfinite(o[0]+o[1]+o[2]+t[0]+t[1]+t[2]+t[3]) &&
        cvr::heldbody::Hold(uint32_t(proxy),uint32_t(body),bone,o,t,blocked,std::isfinite(offset.W) && offset.W>=0?offset.W:1000.f);
    if(out)*out=on;
}
