// HeldPicture -- a carried thing is drawn on the drawn hand, in the same frame, and its body stays in PhysX.
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
#include "Natives/HeldPicture.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Natives/DebugView.hpp"
#include "Anim/VrikState.hpp"
#include "Core/GameBuild.hpp"
#include "Core/LiveControls.hpp"
#include "Hooks/Hook.hpp"
#include "MinHook.h"
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/Vector4.hpp>
#include <windows.h>
#include <algorithm>
#include <cmath>
#include <cstring>

extern void Log(const char* fmt, ...);
extern "C" {
    extern float CyberpunkVR_PlayerEntityPos[3];
    extern float CyberpunkVR_PlayerEntityQuat[4];
    extern int   CyberpunkVR_PlayerEntityValid;
}

namespace cvr::heldpicture {
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
    Log("[picture] proxy %u body %u of %u let go: %.0f frames a second; %u frames, %u sent at the submit, the engine's own %u (thread %lu), body behind the hand mean %.3f most %.3f m, blocked %u frames, too far %u, submit after engine's own %.1f ms\n",
        s.proxy,s.body,s.instances,took?s.frames*1000.0/took:0.,s.frames,s.sends,s.detours,s.detourThread,s.frames?s.behindSum/s.frames:0.,s.behindMost,s.blockedFrames,s.tooFar,s.detourGaps?s.detourGapSum/s.detourGaps:0.);
}
}  // namespace

bool Hold(uint32_t proxy,uint32_t body,int bone,const float offset[3],const float turn[4],bool blocked,float reach) {
    if(!installed || !g_liveControls.xrCarryPicture || body>=kMostInstances)return false;
    if(!InstanceOf(proxy))return false;
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
        slot->letGo=false;slot->relSet=false;
        std::copy_n(offset,3,slot->offset);
        std::copy_n(turn,4,slot->turn);
    }
    ReleaseSRWLockExclusive(&lock);
    return slot!=nullptr;
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

void AtFrameSubmit() {
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
        if(!s.spun) {
            s.spun=true;
            float was=0;
            const bool raised=cvr::debugview::RaiseSpin(s.proxy,s.body,kMostSpin,&was);
            Log("[picture] proxy %u body %u: most spin %s, was %.1f rad/s\n",s.proxy,s.body,raised?"raised":"left alone",was);
        }
        if(s.blocked && s.asked)++s.blockedFrames;
        // After the last ask the picture stays where the hand last had it, and goes to the body: the hand is off elsewhere by then.
        if(s.asked) {
            Pose pose;
            if(HandPose(s,pose)) {s.hand=pose;s.posed=true;}
        }
        const bool toHand=s.asked && !s.blocked && s.posed;
        s.weight=toHand?std::min(1.f,s.weight+dt/kBlendIn):std::max(0.f,s.weight-dt/kBlendOut);
        uint8_t* instance=InstanceOf(s.proxy);
        if(!instance || (!s.asked && s.weight<=0)) {
            Report(s);
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
            if(s.frames==1)Log("[picture] proxy %u body %u of %u: first frame, body at (%.3f %.3f %.3f), hand has it at (%.3f %.3f %.3f), quat %.3f %.3f %.3f %.3f and %.3f %.3f %.3f %.3f, submit thread %lu\n",
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
bool Install() {
    if(!cvr::game_build::Supported())return false;
    base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    auto* target=reinterpret_cast<void*>(base+kUpdateRva);
    if(std::memcmp(target,"\x48\x8b\xc4\x48\x89\x58\x18\x48\x89\x50\x10\x55\x56\x57\x41\x54",16) ||
       std::memcmp(reinterpret_cast<void*>(base+kListenerEntryRva),"\x48\x83\xec\x38\x4c\x8b\x42\x08\x48\x81\xc1\x00\xff\xff\xff",15)) {
        Log("[picture] these are not the functions this was read from: carried things stay as they were\n");
        return false;
    }
    if(MH_CreateHook(target,reinterpret_cast<void*>(&Update),reinterpret_cast<void**>(&originalUpdate))!=MH_OK)return false;
    if(MH_EnableHook(target)!=MH_OK) {MH_RemoveHook(target);return false;}
    installed=true;
    return true;
}
CVR_HOOK("HeldPicture",::cvr::hooks::Stage::Boot,84,Install);
}  // namespace
}  // namespace cvr::heldpicture

void VRHeldPictureLetGo(RED4ext::IScriptable*,RED4ext::CStackFrame* stack,bool* out,int64_t) {
    int32_t proxy=0,body=0;
    RED4ext::GetParameter(stack,&proxy);
    RED4ext::GetParameter(stack,&body);
    stack->code++;
    if(proxy>0 && body>=0)cvr::heldpicture::LetGo(uint32_t(proxy),uint32_t(body));
    if(out)*out=true;
}

// THE GAME'S OWN OUTLINE ON A THING WITHOUT AN ENTITY. entRenderHighlightEvent ends in sub_14079FE8C(render object, params) and that in a command of the render scene: slot 0x2D8 (sub_1407A0190), arguments &object+8, &object+0x10,
// on, mask, opacity, 0. Read in the exe, all callers (sub_14079FE8C, sub_140402428, sub_1403DA0F4) alike. Mask: 1 an outline, 2 a fill, the index <<2, 0x40 through walls. The render side (sub_14067AAB4) calls slot 0xD0 of what +0x10 holds
// and goes on only for what that answers with a type (+0x7D >>3) of 1, 5 or 7. A prop's object is at instance+0xC8 and is of the same make: sub_14020A038 reads it the same way, +8 a thing with a vtable, +0x10 the handle. The first try passed
// +0x10 first and null second, read as fill and outline: a crash on the render thread at sub_14067AAFF. The whole object lights up: a node's instances together.
namespace {
bool Readable(uintptr_t at,size_t size) {
    if(at<0x10000)return false;
    MEMORY_BASIC_INFORMATION m{};
    if(!VirtualQuery(reinterpret_cast<void*>(at),&m,sizeof(m)) || m.State!=MEM_COMMIT || (m.Protect&(PAGE_NOACCESS|PAGE_GUARD)))return false;
    return at+size<=reinterpret_cast<uintptr_t>(m.BaseAddress)+m.RegionSize;
}
// In the image, in .text: a slot of a vtable that has it.
bool CodeOfImage(uintptr_t at) {
    static uintptr_t lo=0,hi=0;
    if(!lo) {
        const auto* dos=reinterpret_cast<const IMAGE_DOS_HEADER*>(cvr::heldpicture::base);
        const auto* nt=reinterpret_cast<const IMAGE_NT_HEADERS64*>(cvr::heldpicture::base+dos->e_lfanew);
        const auto* section=IMAGE_FIRST_SECTION(nt);
        for(int i=0;i<nt->FileHeader.NumberOfSections;++i,++section)if(!std::memcmp(section->Name,".text",5)) {
            lo=cvr::heldpicture::base+section->VirtualAddress;
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
    using namespace cvr::heldpicture;
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
            Log("[picture] outline on proxy %d: render object type %d, %s\n",proxy,type,type==1 || type==5 || type==7?"it takes one":"it does not take one: its lines instead");
        } else if(type<0) {
            static bool said=false;
            if(!said) {said=true;Log("[picture] outline on proxy %d: no render object answers\n",proxy);}
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
        if(!said) {said=true;Log("[picture] the scene's highlight is not where it was read: no highlight\n");}
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
        cvr::heldpicture::Hold(uint32_t(proxy),uint32_t(body),bone,o,t,blocked,std::isfinite(offset.W) && offset.W>=0?offset.W:1000.f);
    if(out)*out=on;
}
