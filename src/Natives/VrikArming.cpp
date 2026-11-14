// VrikArming -- natives lifted out of src/Natives/Natives.cpp, which held every family at once.
//
// The VRIK arming core: turning the engine's own IK on and pointing it at a
// world-space target, which is what makes the avatar's arms follow the controllers.
//
// The cut was placed by the seam map and then SNAPPED to the nearest point at brace depth zero.
// Boundaries taken from line numbers alone are how a split lands in the middle of a function; the
// check is cheap and it is the same lesson as every other generator in this restructure.
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/GameEngine.hpp>
#include <sstream>
#include <locale>
#include <clocale>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <RED4ext/Containers/StaticArray.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Functions.hpp>
#include <RED4ext/Scripting/CProperty.hpp>
#include <RED4ext/Scripting/Natives/Generated/WorldPosition.hpp>
#include <RED4ext/Scripting/Natives/Transform.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <RED4ext/Scripting/Natives/Generated/Quaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimGraph.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_IK.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_MeleeIKData.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimFeature_WeaponUser.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableBool.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableContainer.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableInt.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableQuaternion.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableTransform.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimVariableVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimationControlBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterAnimFeature.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterFloat.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimInputSetterVector.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/IKTargetAddEvent.hpp>
#include <RED4ext/Scripting/Natives/Generated/red/Event.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/entIPlacedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimatedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/StaticOrientationProvider.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystem.hpp>
#include <RED4ext/Scripting/Natives/worldAnimationSystemScriptInterface.hpp>
#include <RED4ext/Scripting/Natives/entSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/entAnimationControllerComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/GarmentSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/MeshComponent.hpp>
#include <windows.h>
#include <algorithm>
#include <cstdio>
#include "Core/LiveControls.hpp"
#include <cctype>
#include <cstring>
#include <atomic>
#include <cstdint>
#include <fstream>
#include <utility>
#include <iomanip>
#include <string>
#include <vector>
#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/SkeletonPolicy.hpp"
#include "Anim/WeaponAim.hpp"
#include "Natives/NativeState.hpp"
#include "Natives/NativeHelpers.hpp"
#include <MinHook.h>
#include "Natives/NativeFunctions.hpp"
#include "Natives/NativeHelpers.hpp"
#include "Natives/NativeState.hpp"




// Resolves the player's live track buffers (a2[7][3] candidates) so the hook can
// identify the player call. Returns bitmask: 1=bufA set, 2=bufB set.
namespace {
struct PlayerBinding {
    uintptr_t object{},rig{},names{},parents{},reference{},trackA{},trackB{};
    uint64_t id{};
    uint32_t bones{},parentCount{},referenceCount{};
    bool operator==(const PlayerBinding&) const = default;
};
PlayerBinding s_playerBinding{};
bool ReadPlayerBinding(RED4ext::anim::AnimatedObject* object,PlayerBinding& key) {
    __try {
        if(!object || !object->metaRig)return false;
        auto* rig=object->metaRig;
        if(!rig->boneNames.Size() || !rig->parentIndeces.Size() || !rig->boneTransforms.Size())return false;
        auto* base=reinterpret_cast<uint8_t*>(object);
        const auto ownerB=*reinterpret_cast<uintptr_t*>(base+0x18);
        key={reinterpret_cast<uintptr_t>(object),reinterpret_cast<uintptr_t>(rig),
            reinterpret_cast<uintptr_t>(&rig->boneNames[0]),reinterpret_cast<uintptr_t>(&rig->parentIndeces[0]),
            reinterpret_cast<uintptr_t>(&rig->boneTransforms[0]),
            rig->referenceTracks.Size() ? reinterpret_cast<uintptr_t>(&rig->referenceTracks[0]):0,
            ownerB ? *reinterpret_cast<uintptr_t*>(ownerB+0x18):0,object->metaRigID,
            rig->boneNames.Size(),rig->parentIndeces.Size(),rig->boneTransforms.Size()};
        return key.bones>0 && key.bones<=VRIK_MAX_BONES && key.parentCount==key.bones &&
            key.referenceCount>=key.bones && key.names && key.parents && key.reference && (key.trackA || key.trackB);
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
}

bool VRIK_PlayerBindingChanged() {
    PlayerBinding current{};
    return !ReadPlayerBinding(FindPlayerAnimatedObjectByComponentName("root"),current) || !(current==s_playerBinding);
}

// THE MIRROR DOUBLE. A mirror (V's bathroom, game 2.5) does not reflect the player. Looking into it starts a
// workspot and spawns an NPC copy of V (Character.q110_tpp_male_v / _female_v) exactly where the player stands,
// with V's head, hair and clothes, and the scene animates that copy. The reflection shows the copy, so nothing
// VRIK solves ever reaches it, and the player's own first-person body has no head to show anyway.
//
// While a double is bound (VRMirrorDouble, from CET), every player pass -- VRIK already written into it -- is
// cached, and every pass of the double's root rig takes the cached LOCAL transforms of the bones whose names
// both rigs share. The double then stands in the reflection with the tracked arms and the head turned where the
// headset looks, and its head mesh is the one drawn. VRIK is not suspended for the scene while the double is
// bound (VRIK_EffectiveCutsceneSuspendTier), otherwise there would be no tracked pose to copy.
//
// The double is told apart by its track buffers, read the way ArmPlayerLocked reads the player's. Buffer B is
// the instance's own; buffer A is taken too only when CET asks for it (flags bit 0), until it is shown not to
// be shared with other skeletons. The copy runs after the original apply and after the player's own work.
//
// THE POSE COPIED IS THE LAST PASS OF A BATCH. The engine applies the player's pose 4-5 times per animation batch,
// back to back; VRIK solves once and replays only what it owns (arms, spine, head -- Hooks/AnimPose.cpp), so the
// legs of each pass are the graph's own output for that pass, and they differ from pass to pass. A double pass
// lands anywhere among them: copying "the player's latest pass" gave it the legs of a different pass every frame,
// and the legs shook in place while the arms held still. Each player pass now goes to a pending copy, and the
// pending copy is published when the next batch starts (the gap the VRIK batch clock uses): the double always takes
// the pose the player's batch ended with, one batch late.
extern "C" __declspec(dllexport) extern float CyberpunkVR_VrikBatchGapMs;
namespace {
SRWLOCK s_mirrorLock=SRWLOCK_INIT;
std::atomic<int> s_mirrorActive{0};
std::atomic<uintptr_t> s_mirrorTrackA{0},s_mirrorTrackB{0};
uintptr_t s_mirrorObject=0,s_mirrorPlayerRig=0;
uint32_t s_mirrorBones=0;                       // the double's root rig, bones
int16_t s_mirrorMap[VRIK_MAX_BONES];            // double bone -> player bone, -1 = no bone of that name
alignas(16) uint8_t s_mirrorPose[VRIK_MAX_BONES*48];
uint32_t s_mirrorPoseBones=0;                   // player bones published; 0 = nothing published yet
alignas(16) uint8_t s_mirrorPending[VRIK_MAX_BONES*48];
uint32_t s_mirrorPendingBones=0;                // the current batch's latest player pass
int64_t s_mirrorLastPassQpc=0;                  // the player's previous pass

double MirrorQpcMs(int64_t ticks) {
    static const double perMs=[] {LARGE_INTEGER f;QueryPerformanceFrequency(&f);return double(f.QuadPart)/1000.0;}();
    return double(ticks)/perMs;
}

void MirrorDoubleClear() {
    s_mirrorActive.store(0,std::memory_order_release);
    AcquireSRWLockExclusive(&s_mirrorLock);
    s_mirrorTrackA.store(0,std::memory_order_relaxed);
    s_mirrorTrackB.store(0,std::memory_order_relaxed);
    s_mirrorObject=s_mirrorPlayerRig=0;
    s_mirrorBones=0;
    s_mirrorPoseBones=0;
    s_mirrorPendingBones=0;
    s_mirrorLastPassQpc=0;
    ReleaseSRWLockExclusive(&s_mirrorLock);
}

uintptr_t MirrorTrackA(RED4ext::anim::AnimatedObject* object) {
    __try {
        auto* ownerA=*reinterpret_cast<uint8_t**>(reinterpret_cast<uint8_t*>(object)+0x8);
        return ownerA ? *reinterpret_cast<uintptr_t*>(ownerA+0x40):0;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return 0;}
}

// Names are CName hashes, 8 bytes each. Returns the number of double bones that found a player bone.
int MirrorBuildMap(const uint64_t* doubleNames,uint32_t doubleBones,const uint64_t* playerNames,uint32_t playerBones) {
    int mapped=0;
    __try {
        for(uint32_t i=0;i<doubleBones;++i) {
            s_mirrorMap[i]=-1;
            const uint64_t h=doubleNames[i];
            if(!h)continue;
            // Same rig family first: the bone is usually at the same index.
            if(i<playerBones && playerNames[i]==h) {s_mirrorMap[i]=static_cast<int16_t>(i);++mapped;continue;}
            for(uint32_t j=0;j<playerBones;++j) {
                if(playerNames[j]==h) {s_mirrorMap[i]=static_cast<int16_t>(j);++mapped;break;}
            }
        }
    } __except(EXCEPTION_EXECUTE_HANDLER) {return -1;}
    return mapped;
}
}

// [0] binds  [1] bind failures  [2] player passes cached  [3] double passes written  [4] double passes skipped
// [5] bones mapped  [6] double bones  [7] double passes through buffer A
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DebugMirrorDouble[8]{};

bool VRIK_MirrorDoubleActive() {return s_mirrorActive.load(std::memory_order_relaxed)!=0;}

int VRIK_EffectiveCutsceneSuspendTier() {
    return VRIK_MirrorDoubleActive() ? -1 : g_liveControls.xrCutsceneSuspendTier;
}

// Runs inside the pose detour's __try, after the original apply and after the player's own work.
void VRIK_MirrorDoublePass(uint8_t* boneBuf,uintptr_t trackBuf,uint32_t slots) {
    if(trackBuf==g_PlayerTrackBufA || trackBuf==g_PlayerTrackBufB) {
        const int n=g_VRBoneCount;
        if(n<=0 || n>VRIK_MAX_BONES || slots<static_cast<uint32_t>(n))return;
        LARGE_INTEGER now;QueryPerformanceCounter(&now);
        AcquireSRWLockExclusive(&s_mirrorLock);
        __try {
            const bool newBatch=s_mirrorLastPassQpc==0 ||
                MirrorQpcMs(now.QuadPart-s_mirrorLastPassQpc)>CyberpunkVR_VrikBatchGapMs;
            s_mirrorLastPassQpc=now.QuadPart;
            if(newBatch && s_mirrorPendingBones>0) {
                std::memcpy(s_mirrorPose,s_mirrorPending,static_cast<size_t>(s_mirrorPendingBones)*48);
                s_mirrorPoseBones=s_mirrorPendingBones;
            }
            std::memcpy(s_mirrorPending,boneBuf,static_cast<size_t>(n)*48);
            s_mirrorPendingBones=static_cast<uint32_t>(n);
        } __finally {ReleaseSRWLockExclusive(&s_mirrorLock);}
        CyberpunkVR_DebugMirrorDouble[2].fetch_add(1,std::memory_order_relaxed);
        return;
    }
    const uintptr_t a=s_mirrorTrackA.load(std::memory_order_relaxed);
    const bool viaA=a && trackBuf==a;
    if(!viaA && trackBuf!=s_mirrorTrackB.load(std::memory_order_relaxed))return;
    bool written=false;
    AcquireSRWLockShared(&s_mirrorLock);
    __try {
        const uint32_t bones=s_mirrorBones,cached=s_mirrorPoseBones;
        if(bones>0 && cached>0 && slots>=bones) {
            for(uint32_t i=0;i<bones;++i) {
                const int j=s_mirrorMap[i];
                if(j<0 || static_cast<uint32_t>(j)>=cached)continue;
                std::memcpy(boneBuf+static_cast<size_t>(i)*48,s_mirrorPose+static_cast<size_t>(j)*48,48);
            }
            written=true;
        }
    } __finally {ReleaseSRWLockShared(&s_mirrorLock);}
    CyberpunkVR_DebugMirrorDouble[written ? 3:4].fetch_add(1,std::memory_order_relaxed);
    if(viaA)CyberpunkVR_DebugMirrorDouble[7].fetch_add(1,std::memory_order_relaxed);
}

// VRMirrorDouble(entity, flags) -> Bool: the entity (the scene's NPC copy of V) takes the player's pose, bone by
// bone, until VRMirrorDoubleClear. Calling it again with the same entity is cheap: the map is rebuilt only when the
// double's or the player's rig object changed. flags bit 0: identify its passes by buffer A as well.
void VRMirrorDouble(RED4ext::IScriptable*,RED4ext::CStackFrame* aFrame,bool* aOut,int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> entity;
    int32_t flags=0;
    RED4ext::GetParameter(aFrame,&entity);
    RED4ext::GetParameter(aFrame,&flags);
    aFrame->code++;
    if(aOut)*aOut=false;
    auto* rtti=RED4ext::CRTTISystem::Get();
    auto* entityClass=rtti ? rtti->GetClass("entEntity"):nullptr;
    if(!entity.instance || !entityClass || !entity.instance->GetType()->IsA(entityClass)) {
        CyberpunkVR_DebugMirrorDouble[1].fetch_add(1,std::memory_order_relaxed);
        return;
    }
    auto* object=FindAnimatedObjectForEntity(reinterpret_cast<RED4ext::ent::Entity*>(entity.instance),"root",false);
    auto* playerObject=FindPlayerAnimatedObjectByComponentName("root");
    PlayerBinding dbl{},player{};
    if(!object || object==playerObject || !ReadPlayerBinding(object,dbl) || !ReadPlayerBinding(playerObject,player)) {
        CyberpunkVR_DebugMirrorDouble[1].fetch_add(1,std::memory_order_relaxed);
        return;
    }
    const uintptr_t trackA=(flags&1) ? MirrorTrackA(object):0;
    const uintptr_t trackB=dbl.trackB;
    const bool sharedWithPlayer=(trackA && (trackA==g_PlayerTrackBufA || trackA==g_PlayerTrackBufB)) ||
        (trackB && (trackB==g_PlayerTrackBufA || trackB==g_PlayerTrackBufB));
    if(!trackB || sharedWithPlayer) {
        CyberpunkVR_DebugMirrorDouble[1].fetch_add(1,std::memory_order_relaxed);
        return;
    }
    if(s_mirrorActive.load(std::memory_order_acquire) && s_mirrorObject==dbl.object && s_mirrorPlayerRig==player.rig &&
        s_mirrorTrackB.load(std::memory_order_relaxed)==trackB && s_mirrorTrackA.load(std::memory_order_relaxed)==trackA) {
        if(aOut)*aOut=true;
        return;
    }
    AcquireSRWLockExclusive(&s_mirrorLock);
    const int mapped=MirrorBuildMap(reinterpret_cast<const uint64_t*>(dbl.names),dbl.bones,
        reinterpret_cast<const uint64_t*>(player.names),player.bones);
    if(mapped>0) {
        s_mirrorBones=dbl.bones;
        s_mirrorObject=dbl.object;
        s_mirrorPlayerRig=player.rig;
        s_mirrorPoseBones=0;
        s_mirrorTrackA.store(trackA,std::memory_order_relaxed);
        s_mirrorTrackB.store(trackB,std::memory_order_relaxed);
    }
    ReleaseSRWLockExclusive(&s_mirrorLock);
    if(mapped<=0) {
        MirrorDoubleClear();
        CyberpunkVR_DebugMirrorDouble[1].fetch_add(1,std::memory_order_relaxed);
        return;
    }
    CyberpunkVR_DebugMirrorDouble[5].store(static_cast<uint64_t>(mapped),std::memory_order_relaxed);
    CyberpunkVR_DebugMirrorDouble[6].store(dbl.bones,std::memory_order_relaxed);
    CyberpunkVR_DebugMirrorDouble[0].fetch_add(1,std::memory_order_relaxed);
    s_mirrorActive.store(1,std::memory_order_release);
    if(aOut)*aOut=true;
}

// VRMirrorDoubleClear(): no double; the scene suspends VRIK again as configured.
void VRMirrorDoubleClear(RED4ext::IScriptable*,RED4ext::CStackFrame* aFrame,void*,int64_t) {
    aFrame->code++;
    MirrorDoubleClear();
}

// VRMirrorDoubleStats() -> String: the counters above, for the harness.
void VRMirrorDoubleStats(RED4ext::IScriptable*,RED4ext::CStackFrame* aFrame,RED4ext::CString* aOut,int64_t) {
    aFrame->code++;
    char text[256];
    std::snprintf(text,sizeof(text),"active=%d binds=%llu fails=%llu cached=%llu written=%llu skipped=%llu mapped=%llu bones=%llu viaA=%llu",
        s_mirrorActive.load(),
        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorDouble[0].load()),
        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorDouble[1].load()),
        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorDouble[2].load()),
        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorDouble[3].load()),
        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorDouble[4].load()),
        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorDouble[5].load()),
        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorDouble[6].load()),
        static_cast<unsigned long long>(CyberpunkVR_DebugMirrorDouble[7].load()));
    if(aOut)*aOut=RED4ext::CString(text);
}

static int ArmPlayerLocked() {
    g_PlayerTrackBufA = 0;
    g_PlayerTrackBufB = 0;
    g_VRBoneCount = g_VRFKCount = 0;
    g_solveCacheN = 0;
    g_solveCacheModel = false;
    g_solveCacheTick = 0xFFFFFFFFu;
    auto* animObj = FindPlayerAnimatedObjectByComponentName("root");
    if (!animObj || !VRIK_IsReadable(animObj, 0x40)) return -1;
    PlayerBinding binding{};
    if(!ReadPlayerBinding(animObj,binding))return -1;
    uint8_t* base = reinterpret_cast<uint8_t*>(animObj);

    uintptr_t trackA=0,trackB=0;

    // A: *(*(animObj+0x8) + 0x40)
    void* ownerA = *reinterpret_cast<void**>(base + 0x8);
    if (VRIK_IsReadable(ownerA, 0x48))
        trackA = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(ownerA) + 0x40);

    // B: *(*(animObj+0x18) + 0x18)
    void* ownerB = *reinterpret_cast<void**>(base + 0x18);
    if (VRIK_IsReadable(ownerB, 0x20))
        trackB = *reinterpret_cast<uintptr_t*>(reinterpret_cast<uint8_t*>(ownerB) + 0x18);

    // Resolve the head + hand bone indices from the metaRig bone names so the
    // pose hook does not rely on hard-coded guesses. The buffer the hook writes
    // (a2[7][0]) is indexed the same as metaRig->boneNames. Prefer an exact name
    // match, fall back to the shortest name containing the needles (so we get the
    // hand root, not a finger like "RightHandThumb1").
    auto* metaRig = animObj->metaRig;
    // The rig comes from the player's typed AnimatedObject; low Windows allocations are valid too.
    // Keep the address-range heuristic for untyped scans, and check this known object's memory directly.
    if (VRIK_IsReadable(metaRig, sizeof(*metaRig)))
    {
        const uint32_t boneCount = metaRig->boneNames.Size();
        if (boneCount > 0 && boneCount < 8192)
        {
            int head = -1, rightHand = -1, leftHand = -1;
            int rightArm = -1, rightFore = -1, leftArm = -1, leftFore = -1;
            // Palm centre for the VR basketball. RightHand is the WRIST; the ball has to sit in
            // the hand the player can see, and the base of the middle finger is where a palm
            // actually cups a ball. Exact names in this rig: RightHandMiddle1 / LeftHandMiddle1.
            int rightPalm = -1, leftPalm = -1;
            int spineTmp[8] = {-1,-1,-1,-1,-1,-1,-1,-1};
            int spineTmpCount = 0;
            int smokeFingerTmp[32]; int smokeFingerTmpCount = 0;
            char smokeFingerNameTmp[32][48] = {};
            int smokeCigTmp = -1;
    int smokeMouthBoneTmp = -1;
            int smokeFingerTmpL[32]; int smokeFingerTmpCountL = 0;
            char smokeFingerNameTmpL[32][48] = {};
            int smokeLighterTmp = -1;
            const size_t kNoMatch = static_cast<size_t>(-1);
            size_t headLen = kNoMatch, rightLen = kNoMatch, leftLen = kNoMatch;
            for (uint32_t i = 0; i < boneCount; ++i)
            {
                const char* nm = metaRig->boneNames[i].ToString();
                if (!nm || !nm[0])
                    continue;
                const size_t len = std::strlen(nm);

                if (EqualsInsensitive(nm, "Head")) { head = static_cast<int>(i); headLen = 0; }
                else if (headLen != 0 && ContainsInsensitive(nm, "head") && len < headLen)
                { head = static_cast<int>(i); headLen = len; }

                // "_setup" copies exist for the same names (RightHandMiddle1_setup) and are not
                // part of the animated chain, so they must not win the match.
                if (!ContainsInsensitive(nm, "_")) {
                    if (EqualsInsensitive(nm, "RightHandMiddle1")) rightPalm = static_cast<int>(i);
                    if (EqualsInsensitive(nm, "LeftHandMiddle1"))  leftPalm  = static_cast<int>(i);
                }

                const bool isHand = ContainsInsensitive(nm, "hand");
                if (isHand && ContainsInsensitive(nm, "right"))
                {
                    if (EqualsInsensitive(nm, "RightHand")) { rightHand = static_cast<int>(i); rightLen = 0; }
                    else if (rightLen != 0 && len < rightLen) { rightHand = static_cast<int>(i); rightLen = len; }
                }
                if (isHand && ContainsInsensitive(nm, "left"))
                {
                    if (EqualsInsensitive(nm, "LeftHand")) { leftHand = static_cast<int>(i); leftLen = 0; }
                    else if (leftLen != 0 && len < leftLen) { leftHand = static_cast<int>(i); leftLen = len; }
                }

                // Arm-chain joints for the full IK (exact names on the player rig).
                if (EqualsInsensitive(nm, "RightArm"))     rightArm  = static_cast<int>(i);
                if (EqualsInsensitive(nm, "RightForeArm")) rightFore = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftArm"))      leftArm   = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftForeArm"))  leftFore  = static_cast<int>(i);
                // Torso chain. Weapon-ready poses mostly bend Spine* backward, which moves the
                // shoulders before our arm IK runs. Keep only the spine bones, not hips/head.
                if (cvr::vrik::IsPrimarySpine(nm) && spineTmpCount < 8) {
                    spineTmp[spineTmpCount++] = static_cast<int>(i);
                }
                // Hip + leg bones: holster proximity AND the full-body lower chain (move hips
                // under the HMD, keep feet planted via leg IK).
                if (EqualsInsensitive(nm, "RightUpLeg"))   g_VRRightUpLegIdx = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftUpLeg"))    g_VRLeftUpLegIdx  = static_cast<int>(i);
                if (EqualsInsensitive(nm, "RightLeg"))     g_VRRightLegIdx   = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftLeg"))      g_VRLeftLegIdx    = static_cast<int>(i);
                if (EqualsInsensitive(nm, "RightFoot"))    g_VRRightFootIdx  = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftFoot"))     g_VRLeftFootIdx   = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Hips"))         g_VRHipsIdx       = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Neck"))         g_VRNeckIdx       = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Neck1"))        g_VRNeck1Idx      = static_cast<int>(i);
                if (EqualsInsensitive(nm, "LeftEye"))      g_VREyeLeftIdx    = static_cast<int>(i);
                if (EqualsInsensitive(nm, "RightEye"))     g_VREyeRightIdx   = static_cast<int>(i);
                // FPP-camera control chain (see g_VRFppCamIdx above; frozen by the pose hook).
                if (EqualsInsensitive(nm, "Torso_fppCamera_Control_GRP"))  g_VRFppCamIdx[0] = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Torso_fppCamera_Aim_JNT"))      g_VRFppCamIdx[1] = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Torso_fppCamera_Target_JNT"))   g_VRFppCamIdx[2] = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Torso_fppCamera_UpOffset_GRP")) g_VRFppCamIdx[3] = static_cast<int>(i);
                if (EqualsInsensitive(nm, "Torso_fppCamera_Up_GRP"))       g_VRFppCamIdx[4] = static_cast<int>(i);
                // Deformation helpers are resolved after the parent table below.
                // The current rig uses l/r_Wrist_0..2; cyberware can replace it.

                // Right-hand FINGER bones for the smoke "hold cigarette" fingers-only grip.
                // Deform finger + metacarpal joints have "right"+"hand" in the name but are
                // NOT the wrist ("RightHand"), NOT control joints ("_setup") and NOT the hand
                // tip ("RightHandEnd"). Muscle joints ("r_thumb_*") lack the word "right", so
                // the isHand+"right" gate skips them. Captures RightInHand{Thumb..Pinky} and
                // RightHand{Thumb,Index,Middle,Ring,Pinky}{1..3}.
                // Real deform finger bones (RightHandThumb1, RightInHandIndex, ...) have NO
                // underscore; the false matches that slipped in (Torso_*Arm_Hand_IK_JNT control
                // joints, shadow_RightHand) all contain '_'. Excluding '_' cleanly keeps only the
                // finger bones. "end" still filters RightHandEnd (no underscore).
                if (isHand && ContainsInsensitive(nm, "right")
                    && !EqualsInsensitive(nm, "RightHand")
                    && !ContainsInsensitive(nm, "_")
                    && !ContainsInsensitive(nm, "end")
                    && smokeFingerTmpCount < 32) {
                    std::strncpy(smokeFingerNameTmp[smokeFingerTmpCount], nm, 47);
                    smokeFingerTmp[smokeFingerTmpCount++] = static_cast<int>(i);
                }
                // Cigarette slot bone (child of RightHand); moving its local transform
                // positions the attached cig into the finger pinch.
                // In-hand grip rides WeaponRight (same as the weapon). The MOUTH pin rides WeaponRight1
                // -- a custom LEAF copy of WeaponRight (child of RightHand) added to the rig -- so the cig
                // at the lips leaves WeaponRight free for a pistol, and pinning the leaf moves only it.
                if (EqualsInsensitive(nm, "WeaponRight"))  smokeCigTmp = static_cast<int>(i);
                if (EqualsInsensitive(nm, "WeaponRight1")) smokeMouthBoneTmp = static_cast<int>(i);

                // LEFT-hand mirror (lighter grip).
                if (isHand && ContainsInsensitive(nm, "left")
                    && !EqualsInsensitive(nm, "LeftHand")
                    && !ContainsInsensitive(nm, "_")
                    && !ContainsInsensitive(nm, "end")
                    && smokeFingerTmpCountL < 32) {
                    std::strncpy(smokeFingerNameTmpL[smokeFingerTmpCountL], nm, 47);
                    smokeFingerTmpL[smokeFingerTmpCountL++] = static_cast<int>(i);
                }
                if (EqualsInsensitive(nm, "WeaponLeft")) smokeLighterTmp = static_cast<int>(i);
            }

            if (head >= 0)      g_VRHeadBoneIdx  = head;
            if (rightHand >= 0) g_VRRightBoneIdx = rightHand;
            if (leftHand >= 0)  g_VRLeftBoneIdx  = leftHand;
            // Fall back to the wrist if the rig has no Middle1 (the ball is then a few cm off,
            // rather than absent).
            g_VRPalmRIdx = (rightPalm >= 0) ? rightPalm : rightHand;
            g_VRPalmLIdx = (leftPalm  >= 0) ? leftPalm  : leftHand;
            if (rightArm >= 0)  g_VRRightUpperArmIdx = rightArm;
            if (rightFore >= 0) g_VRRightForeArmIdx  = rightFore;
            if (leftArm >= 0)   g_VRLeftUpperArmIdx  = leftArm;
            if (leftFore >= 0)  g_VRLeftForeArmIdx   = leftFore;
            g_VRSpineCount = spineTmpCount;
            for (int s = 0; s < 8; ++s) g_VRSpineIdx[s] = (s < spineTmpCount) ? spineTmp[s] : -1;

            // Publish the right-hand finger bone set. This resolve re-runs on every VRIK desync;
            // only (re)init rotations to identity BEFORE we have a pose. Otherwise a desync would
            // wipe the loaded/captured pose back to identity (straight/flat fingers) -> the
            // "loads flat, capture works until the next desync" bug. Once have==1 the (stable
            // same-skeleton) idx/name mapping is unchanged, so rot[] stays valid.
            const bool initRotR = (g_VRSmokeFingerHave == 0);
            for (int s = 0; s < 32; ++s) {
                g_VRSmokeFingerIdx[s] = (s < smokeFingerTmpCount) ? smokeFingerTmp[s] : -1;
                if (s < smokeFingerTmpCount) std::strncpy(g_VRSmokeFingerName[s], smokeFingerNameTmp[s], 47);
                else                         g_VRSmokeFingerName[s][0] = '\0';
                if (initRotR) { g_VRSmokeFingerRot[s][0]=0.0f; g_VRSmokeFingerRot[s][1]=0.0f;
                                g_VRSmokeFingerRot[s][2]=0.0f; g_VRSmokeFingerRot[s][3]=1.0f; }
            }
            g_VRSmokeFingerCount = smokeFingerTmpCount;
            g_VRSmokeCigIdx = smokeCigTmp;
            g_VRSmokeMouthBoneIdx = smokeMouthBoneTmp;

            // LEFT-hand publish (mirror). Same desync-preservation guard as the right hand.
            const bool initRotL = (g_VRSmokeFingerHaveL == 0);
            for (int s = 0; s < 32; ++s) {
                g_VRSmokeFingerIdxL[s] = (s < smokeFingerTmpCountL) ? smokeFingerTmpL[s] : -1;
                if (s < smokeFingerTmpCountL) std::strncpy(g_VRSmokeFingerNameL[s], smokeFingerNameTmpL[s], 47);
                else                          g_VRSmokeFingerNameL[s][0] = '\0';
                if (initRotL) { g_VRSmokeFingerRotL[s][0]=0.0f; g_VRSmokeFingerRotL[s][1]=0.0f;
                                g_VRSmokeFingerRotL[s][2]=0.0f; g_VRSmokeFingerRotL[s][3]=1.0f; }
                // Left thumb bones get the trigger-driven press flick.
                g_VRSmokeThumbIsL[s] = (s < smokeFingerTmpCountL
                                        && ContainsInsensitive(smokeFingerNameTmpL[s], "thumb")) ? 1 : 0;
            }
            g_VRSmokeFingerCountL = smokeFingerTmpCountL;
            g_VRSmokeLighterIdx = smokeLighterTmp;

            // AUTO-LOAD the baked grip poses once (so no re-capture per session). Two files next
            // to Cyberpunk2077.exe (bin\x64\): CyberpunkVR_SmokeGrip_right.ini (cigarette) and
            // CyberpunkVR_LighterGrip_Left.ini (lighter). Name-keyed by bone -> survives index
            // shifts / male-female skeletons. F=finger rot; C=slot pos+rot (WeaponRight cig /
            // WeaponLeft lighter); K=LeftThumbFlick delta. Both files parsed with the same logic.
            static bool s_smokeGripLoaded = false;
            if (!s_smokeGripLoaded && (smokeFingerTmpCount > 0 || smokeFingerTmpCountL > 0)) {
                s_smokeGripLoaded = true;
                const char* files[2] = { "CyberpunkVR_SmokeGrip_right.ini", "CyberpunkVR_LighterGrip_Left.ini" };
                int rFing = 0, lFing = 0; bool cigLoaded = false, ltrLoaded = false;
                for (int fi = 0; fi < 2; ++fi) {
                    std::ifstream f(VRGripPath(files[fi]));
                    if (!f.is_open()) continue;
                    std::string line;
                    while (std::getline(f, line)) {
                        if (line.empty() || line[0] == '#') continue;
                        // Locale-INDEPENDENT parse. The file always uses '.' as the decimal, but the
                        // CRT locale may be ','-decimal at load time -> sscanf("%f") would misparse
                        // "0.5" and drop the value (loads flat while an in-session capture works,
                        // because capture is a pure in-memory float copy). classic() forces '.'.
                        std::istringstream iss(line);
                        iss.imbue(std::locale::classic());
                        std::string tag, nm;
                        if (!(iss >> tag >> nm)) continue;
                        float a=0,b=0,c=0,d=0,e=0,g=0,h=0;
                        if (tag == "F") {
                            if (iss >> a >> b >> c >> d) {
                                bool hit = false;
                                for (int k = 0; k < g_VRSmokeFingerCount && k < 32; ++k) {
                                    if (EqualsInsensitive(g_VRSmokeFingerName[k], nm.c_str())) {
                                        g_VRSmokeFingerRot[k][0]=a; g_VRSmokeFingerRot[k][1]=b;
                                        g_VRSmokeFingerRot[k][2]=c; g_VRSmokeFingerRot[k][3]=d;
                                        ++rFing; hit = true; break;
                                    }
                                }
                                if (!hit) for (int k = 0; k < g_VRSmokeFingerCountL && k < 32; ++k) {
                                    if (EqualsInsensitive(g_VRSmokeFingerNameL[k], nm.c_str())) {
                                        g_VRSmokeFingerRotL[k][0]=a; g_VRSmokeFingerRotL[k][1]=b;
                                        g_VRSmokeFingerRotL[k][2]=c; g_VRSmokeFingerRotL[k][3]=d;
                                        ++lFing; break;
                                    }
                                }
                            }
                        } else if (tag == "C") {
                            if (iss >> a >> b >> c >> d >> e >> g >> h) {
                                if (EqualsInsensitive(nm.c_str(), "WeaponLeft")) {
                                    g_VRSmokeLighterPos[0]=a; g_VRSmokeLighterPos[1]=b; g_VRSmokeLighterPos[2]=c;
                                    g_VRSmokeLighterRot[0]=d; g_VRSmokeLighterRot[1]=e; g_VRSmokeLighterRot[2]=g; g_VRSmokeLighterRot[3]=h;
                                    ltrLoaded = true;
                                } else {
                                    g_VRSmokeCigPos[0]=a; g_VRSmokeCigPos[1]=b; g_VRSmokeCigPos[2]=c;
                                    g_VRSmokeCigRot[0]=d; g_VRSmokeCigRot[1]=e; g_VRSmokeCigRot[2]=g; g_VRSmokeCigRot[3]=h;
                                    cigLoaded = true;
                                }
                            }
                        } else if (tag == "K") {
                            if (iss >> a >> b >> c >> d) {
                                g_VRSmokeThumbFlickL[0]=a; g_VRSmokeThumbFlickL[1]=b;
                                g_VRSmokeThumbFlickL[2]=c; g_VRSmokeThumbFlickL[3]=d;
                            }
                        }
                    }
                }
                if (rFing > 0)    g_VRSmokeFingerHave  = 1;
                if (lFing > 0)    g_VRSmokeFingerHaveL = 1;
                if (cigLoaded)    g_VRSmokeCigHave     = 1;
                if (ltrLoaded)    g_VRSmokeLighterHave = 1;
                // THIRD file: LEFT-hand CIGARETTE grip (CyberpunkVR_SmokeGrip_Left.ini) -> LC buffer.
                // Same left finger bones as the lighter, different pose. Seed LC from the lighter pose
                // so any bone the file omits (or a missing file) falls back to the lighter hold.
                {
                    for (int k = 0; k < 32; ++k) {
                        g_VRSmokeFingerRotLC[k][0]=g_VRSmokeFingerRotL[k][0]; g_VRSmokeFingerRotLC[k][1]=g_VRSmokeFingerRotL[k][1];
                        g_VRSmokeFingerRotLC[k][2]=g_VRSmokeFingerRotL[k][2]; g_VRSmokeFingerRotLC[k][3]=g_VRSmokeFingerRotL[k][3];
                    }
                    g_VRSmokeCigLPos[0]=g_VRSmokeLighterPos[0]; g_VRSmokeCigLPos[1]=g_VRSmokeLighterPos[1]; g_VRSmokeCigLPos[2]=g_VRSmokeLighterPos[2];
                    g_VRSmokeCigLRot[0]=g_VRSmokeLighterRot[0]; g_VRSmokeCigLRot[1]=g_VRSmokeLighterRot[1]; g_VRSmokeCigLRot[2]=g_VRSmokeLighterRot[2]; g_VRSmokeCigLRot[3]=g_VRSmokeLighterRot[3];
                    if (ltrLoaded) g_VRSmokeCigLHave = 1;   // fallback availability = lighter pose
                    std::ifstream fc(VRGripPath("CyberpunkVR_SmokeGrip_Left.ini"));
                    if (fc.is_open()) {
                        int lcFing = 0; bool lcSlot = false;
                        std::string line;
                        while (std::getline(fc, line)) {
                            if (line.empty() || line[0] == '#') continue;
                            std::istringstream iss(line);
                            iss.imbue(std::locale::classic());
                            std::string tag, nm;
                            if (!(iss >> tag >> nm)) continue;
                            float a=0,b=0,c=0,d=0,e=0,g2=0,h=0;
                            if (tag == "F") {
                                if (iss >> a >> b >> c >> d) {
                                    for (int k = 0; k < g_VRSmokeFingerCountL && k < 32; ++k) {
                                        if (EqualsInsensitive(g_VRSmokeFingerNameL[k], nm.c_str())) {
                                            g_VRSmokeFingerRotLC[k][0]=a; g_VRSmokeFingerRotLC[k][1]=b;
                                            g_VRSmokeFingerRotLC[k][2]=c; g_VRSmokeFingerRotLC[k][3]=d;
                                            ++lcFing; break;
                                        }
                                    }
                                }
                            } else if (tag == "C") {
                                if (iss >> a >> b >> c >> d >> e >> g2 >> h) {
                                    g_VRSmokeCigLPos[0]=a; g_VRSmokeCigLPos[1]=b; g_VRSmokeCigLPos[2]=c;
                                    g_VRSmokeCigLRot[0]=d; g_VRSmokeCigLRot[1]=e; g_VRSmokeCigLRot[2]=g2; g_VRSmokeCigLRot[3]=h;
                                    lcSlot = true;
                                }
                            }
                        }
                        if (lcFing > 0 || lcSlot) g_VRSmokeCigLHave = 1;
                    }
                }
                {   // diag: confirm how many lines actually parsed+matched from the .ini
                    std::ofstream dbg(VRDiagPath("CyberpunkVR_SmokeGrip_loadinfo.txt"), std::ios::trunc);
                    if (dbg.is_open())
                        dbg << "resolvedR=" << g_VRSmokeFingerCount << " resolvedL=" << g_VRSmokeFingerCountL
                            << " loadedR=" << rFing << " loadedL=" << lFing
                            << " cig=" << (cigLoaded ? 1 : 0) << " lighter=" << (ltrLoaded ? 1 : 0) << "\n";
                }
            }

            // Copy the parent-index table so the pose hook can run FK each frame.
            const uint32_t pc = metaRig->parentIndeces.Size();
            const uint32_t copyN = (pc < boneCount ? pc : boneCount);
            int written = 0;
            for (uint32_t i = 0; i < copyN && i < static_cast<uint32_t>(VRIK_MAX_BONES); ++i) { g_VRBoneParent[i] = metaRig->parentIndeces[i]; ++written; }
            g_VRBoneCount = written;

            // Static: the rebind holds g_PlayerPoseStateLock exclusively, and VRIK_MAX_BONES pointers are 16 KB.
            static const char* policyNames[VRIK_MAX_BONES];
            std::fill(std::begin(policyNames), std::end(policyNames), nullptr);
            for(int i=0;i<written;++i)policyNames[i]=metaRig->boneNames[i].ToString();
            const int referenceCount=static_cast<int>(metaRig->boneTransforms.Size());
            const auto* reference=referenceCount>0
                ? reinterpret_cast<const uint8_t*>(&metaRig->boneTransforms[0]) : nullptr;
            const int referenceLimit=std::min(referenceCount,written);
            const bool referenceReadable=reference && referenceLimit>0 &&
                VRIK_IsReadable(reference,static_cast<size_t>(referenceLimit)*48);
            // Resolve the current body on every rig bind, including save/body
            // changes. This is the game thread, not the animation hot path.
            RED4ext::CName gender;
            bool female=false;
            if(auto* player=FindPlayerEntity()) {
                auto* cls=player->GetType();
                auto* getGender=cls ? cls->GetFunction("GetResolvedGenderName"):nullptr;
                female=getGender && RED4ext::ExecuteFunction(player,getGender,&gender) && gender==RED4ext::CName("Female");
            }
            VRIK_ConfigureRigPolicy(policyNames,written,
                referenceReadable ? reference : nullptr,referenceReadable ? referenceLimit : 0,female);
            std::vector<const char*> trackNames;
            if(metaRig->trackNames.Size()<=8192) {
                for(uint32_t i=0;i<metaRig->trackNames.Size();++i)trackNames.push_back(metaRig->trackNames[i].ToString());
            }
            VRIK_ConfigureCyberwareTracks(trackNames.data(),static_cast<int>(trackNames.size()),
                metaRig->referenceTracks.Size()?&metaRig->referenceTracks[0]:nullptr);

            // PERF (audit, session 3): per-solve FK used to walk the FULL rig (up to
            // 256 bones) 3+ times per fresh solve, while the solver only ever reads
            // model-space transforms up to the highest resolved bone index (parents
            // precede children in this rig, so a prefix walk is complete). Publish
            // that prefix length; the hook falls back to the full count if 0.
            {
                int mx = 0;
                auto acc = [&](int v) { if (v > mx) mx = v; };
                acc(g_VRHeadBoneIdx);      acc(g_VRRightBoneIdx);     acc(g_VRLeftBoneIdx);
                acc(g_VRRightUpperArmIdx); acc(g_VRRightForeArmIdx);
                acc(g_VRLeftUpperArmIdx);  acc(g_VRLeftForeArmIdx);
                for (int s = 0; s < 8; ++s) acc(g_VRSpineIdx[s]);
                acc(g_VRRightUpLegIdx);    acc(g_VRLeftUpLegIdx);
                acc(g_VRRightLegIdx);      acc(g_VRLeftLegIdx);
                acc(g_VRRightFootIdx);     acc(g_VRLeftFootIdx);
                acc(g_VRHipsIdx);          acc(g_VRNeckIdx);          acc(g_VRNeck1Idx);
                acc(g_VREyeLeftIdx);       acc(g_VREyeRightIdx);
                for (int s = 0; s < 3; ++s) { acc(g_VRForeTwistR[s]); acc(g_VRForeTwistL[s]); }
                for(int i=0;i<written;++i)if(g_VRUpperOwned[i] || g_VRShadowSource[i]>=0)acc(i);
                const int lim = mx + 1;
                g_VRFKCount = (lim < g_VRBoneCount) ? lim : g_VRBoneCount;
            }

            // Write the bone-resolve diagnostic ONCE only. VRIK_DoArmPlayer() now only runs on
            // initial bootstrap and when UpdateVRIKAnimInputs detects a real desync (see the
            // g_AnimPoseMatchCalls stall check there), so a per-call file write is no longer a
            // hot-path concern either way -- kept as one snapshot since the bone indices don't
            // change after the first successful resolve (same skeleton).
            static bool s_boneDiagWritten = false;
            if (!s_boneDiagWritten) {
                std::ofstream out(VRDiagPath("vrik_bone_resolve.txt"), std::ios::trunc);
                if (out.is_open()) {
                    out << "boneCount=" << boneCount
                        << " parentCount=" << pc
                        << " head=" << g_VRHeadBoneIdx
                        << " rightHand=" << g_VRRightBoneIdx
                        << " leftHand=" << g_VRLeftBoneIdx
                        << " rightArm=" << g_VRRightUpperArmIdx
                        << " rightForeArm=" << g_VRRightForeArmIdx
                        << " leftArm=" << g_VRLeftUpperArmIdx
                        << " leftForeArm=" << g_VRLeftForeArmIdx
                        << " spineCount=" << g_VRSpineCount << "\n";
                    s_boneDiagWritten = true;
                }
            }
        }
    }

    // Publish only after indices, hierarchy, reference pose and replay state
    // belong to this rig. A failed resolve leaves the player branch disarmed.
    PlayerBinding finished{};
    if(g_VRBoneCount>0 && ReadPlayerBinding(animObj,finished) && finished==binding) {
        g_PlayerTrackBufA=trackA;g_PlayerTrackBufB=trackB;s_playerBinding=binding;
    }
    return (g_PlayerTrackBufA ? 1 : 0) | (g_PlayerTrackBufB ? 2 : 0);
}

int VRIK_DoArmPlayer() {
    int result=0;
    AcquireSRWLockExclusive(&g_PlayerPoseStateLock);
    __try { result=ArmPlayerLocked();return result; }
    __finally {
        g_PlayerPoseHeadingReady.store(result>0 && g_VRHeadBoneIdx>=0,std::memory_order_release);
        ReleaseSRWLockExclusive(&g_PlayerPoseStateLock);
    }
}
