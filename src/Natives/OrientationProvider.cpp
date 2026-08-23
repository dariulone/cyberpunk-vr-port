// OrientationProvider -- lifted out of src/Natives/Natives.cpp, where it was one of four instrumentation
// subsystems sharing the tail of an 8,400-line file behind nothing but a banner comment.
//
// Instruments GetOrientation through the provider VMT -- standing at the
// register rather than guessing what passes through it.
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
#include <cctype>
#include <cstring>
#include <atomic>
#include <cstdint>
#include <fstream>
#include <utility>
#include <iomanip>
#include <string>
#include "Anim/VrikHook.hpp"
#include "Anim/WeaponAim.hpp"
#include "Natives/NativeState.hpp"
#include "Natives/NativeHelpers.hpp"
#include <MinHook.h>
#include "Natives/NativeFunctions.hpp"
#include "Natives/NativeHelpers.hpp"
#include "Natives/NativeState.hpp"



// ============================================================================
// ORIENTATION-PROVIDER GetOrientation VMT INSTRUMENT (the user's "stand at the register" plan).
// We can't pin the GetOrientation vtable slot statically (auto-analysis off, CClass vs instance
// vtable confusion). So: CreateInstance the provider class -> read its REAL instance vtable -> VMT-
// instrument the interface-tail slots (read-only: each stub bumps a per-slot counter + records the
// output quaternion, then calls the original). Fire a projectile -> the slot that fires == the one
// the launch calls; its output is the camera-forward quat. Then we flip that stub to OVERRIDE the
// output with the controller aim. No shared-memory writes (avoids the worldTransform hang).
// ============================================================================
static constexpr int kProvSlotLo = 3;      // first vtable slot to instrument
static constexpr int kProvNSlots = 48;     // slots [3..50]
static constexpr int kProvNCls   = 3;      // entEntity / entStatic / entFunc
static const char*   kProvNames[kProvNCls] = {
    "entEntityOrientationProvider", "entStaticOrientationProvider", "entFuncOrientationProvider" };
static uintptr_t g_provVtbl[kProvNCls] = {0};
static void*     g_provOrig[kProvNCls][kProvNSlots] = {0};
volatile uint64_t g_provCalls[kProvNCls][kProvNSlots] = {0};
// EXPORTED VIEW of the pistol class's slot counters (entFunc = class 2). Which vtable slot actually
// fires on a shot is a question the dump answers only after the fact, and only if the instrument was
// installed; this lets a live probe watch it while the trigger is pulled.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugProvEntFunc[16] = {0};

// The hand-recoil spring (src/Anim/Recoil.cpp).
extern "C" void RecoilOnShot();
// The last 16 bytes each slot handed back. The launch ORIENTATION was found by testing for a unit
// quaternion; the launch ORIGIN is findable the same way and for the same reason -- a world
// position in Night City is thousands of metres out, which nothing else in these buffers is. One
// run of the census names the slot instead of another guess.
volatile float    g_provLastOut[kProvNCls][kProvNSlots][4] = {};
volatile uint8_t  g_provOutSeen[kProvNCls][kProvNSlots] = {};

extern volatile float    g_provMuzzlePos[3];      // defined with the other muzzle state below
extern volatile uint32_t g_provMuzzlePosSeq;

void DumpProviderSlots(std::ofstream& out) {
    // Which provider slot carries the launch ORIGIN. Classified by content, not by position in
    // the table: a unit quaternion is an orientation, three components of a thousand metres or
    // more are a world position, and everything else is neither.
    out << "PROVIDER SLOTS (realSlot = " << kProvSlotLo << " + index)\n";
    out << "  muzzlePos=(" << g_provMuzzlePos[0] << ", " << g_provMuzzlePos[1] << ", "
        << g_provMuzzlePos[2] << ") seq=" << g_provMuzzlePosSeq << "\n";
    for (int c = 0; c < kProvNCls; ++c) {
        for (int sl = 0; sl < kProvNSlots; ++sl) {
            if (!g_provCalls[c][sl] || !g_provOutSeen[c][sl]) continue;
            const float a0 = g_provLastOut[c][sl][0], a1 = g_provLastOut[c][sl][1];
            const float a2 = g_provLastOut[c][sl][2], a3 = g_provLastOut[c][sl][3];
            const float n = a0*a0 + a1*a1 + a2*a2 + a3*a3;
            const float big = (std::fabs(a0) > 100.0f) + (std::fabs(a1) > 100.0f)
                            + (std::fabs(a2) > 100.0f);
            const char* tag = (n > 0.9f && n < 1.1f) ? "QUAT"
                            : (big >= 2.0f)          ? "WORLDPOS"
                                                     : "-";
            out << "  C" << c << " S" << sl << " (slot " << (kProvSlotLo + sl) << ") "
                << tag << " calls=" << g_provCalls[c][sl]
                << " out=(" << a0 << ", " << a1 << ", " << a2 << ", " << a3 << ")\n";
        }
    }
}

volatile int      g_provOverrideCls  = -1;
volatile int      g_provOverrideSlot = -1; // (cls,slot) whose stub overwrites the out-quat
volatile uint64_t g_provOverrides = 0;
volatile float    g_provLastQ[4] = {0,0,0,1};
volatile float    g_provOrigQ[4] = {0,0,0,1};   // provider's ORIGINAL out-quat (=camera) for the override slot
volatile float    g_provCtrlQ[4] = {0,0,0,1};   // controller quat shared[12..15] captured at the same call
volatile float    g_provHmdQ[4]  = {0,0,0,1};   // hmd quat shared[16..19] captured at the same call
volatile float    g_provMuzzleQ[4] = {0,0,0,1}; // weapon muzzle WORLD orientation (CET publishes it)
// The muzzle WORLD POSITION. The launch override has always replaced the shot's direction with
// this muzzle's orientation while leaving its ORIGIN at the game camera -- which is the left eye,
// because MAIN is both the gameplay camera and the left render camera. The bullet therefore flew
// parallel to the barrel but started 65 mm away from the eye that was aiming, so the sight was
// exact for the left eye and off by a constant 65 mm for the right. Measured: identical geometry
// in both views, and a miss that does not change with range.
// 1 = the aim ray starts at the muzzle instead of the head. Live switch, so the two can be
// compared by shooting rather than by argument.
volatile int      g_waXhPosFromMuzzle = 1;
volatile float    g_provMuzzlePos[3] = {0,0,0};
volatile uint32_t g_provMuzzlePosSeq = 0;
volatile uint32_t g_provMuzzleSeq = 0;          // freshness
volatile float    g_provDeltaQ[4] = {0,0,0,1};  // mode6 cone-rotation delta (recomputed per muzzle seq)
volatile uint32_t g_provDeltaSeq = 0xFFFFFFFF;  // seq the delta was computed for
volatile int      g_provQuatMode = 1;      // 0=raw shared[53..56], 1=build from controller fwd shared[60..62]
volatile int      g_provFwdAxis  = 1;      // which body axis is "forward": 0=+X 1=+Y 2=+Z (RED uses +Y)

// Build a world orientation quaternion whose chosen forward axis = the controller forward vector.
// up = world +Z; matrix [right, fwd, up] -> quaternion. Falls back gracefully near-parallel.
static void BuildOrientFromFwd(float fx, float fy, float fz, int fwdAxis, float* q) {
    float fl = std::sqrt(fx*fx+fy*fy+fz*fz); if (fl < 1e-4f) { q[0]=0;q[1]=0;q[2]=0;q[3]=1; return; }
    fx/=fl; fy/=fl; fz/=fl;
    float ux=0, uy=0, uz=1;
    if (std::fabs(fz) > 0.95f) { ux=0; uy=1; uz=0; }            // forward near world-up -> use +Y as ref
    // right = normalize(cross(fwd, up))
    float rx = fy*uz - fz*uy, ry = fz*ux - fx*uz, rz = fx*uy - fy*ux;
    float rl = std::sqrt(rx*rx+ry*ry+rz*rz); if (rl<1e-4f){rx=1;ry=0;rz=0;rl=1;} rx/=rl;ry/=rl;rz/=rl;
    // recompute up = cross(right, fwd)
    float vx = ry*fz - rz*fy, vy = rz*fx - rx*fz, vz = rx*fy - ry*fx;
    // columns of rotation matrix depend on which axis is forward (RED forward = +Y)
    float m00,m01,m02, m10,m11,m12, m20,m21,m22;
    // X column=right, the forward axis column=fwd, remaining=up
    if (fwdAxis == 1) { // +Y = forward
        m00=rx; m10=ry; m20=rz;      // X = right
        m01=fx; m11=fy; m21=fz;      // Y = forward
        m02=vx; m12=vy; m22=vz;      // Z = up
    } else if (fwdAxis == 0) { // +X = forward
        m00=fx; m10=fy; m20=fz;
        m01=rx; m11=ry; m21=rz;
        m02=vx; m12=vy; m22=vz;
    } else { // +Z = forward
        m00=rx; m10=ry; m20=rz;
        m01=vx; m11=vy; m21=vz;
        m02=fx; m12=fy; m22=fz;
    }
    // matrix -> quaternion (Shepperd)
    float tr = m00+m11+m22;
    if (tr > 0.0f) { float s=std::sqrt(tr+1.0f)*2.0f; q[3]=0.25f*s; q[0]=(m21-m12)/s; q[1]=(m02-m20)/s; q[2]=(m10-m01)/s; }
    else if (m00>m11 && m00>m22) { float s=std::sqrt(1.0f+m00-m11-m22)*2.0f; q[3]=(m21-m12)/s; q[0]=0.25f*s; q[1]=(m01+m10)/s; q[2]=(m02+m20)/s; }
    else if (m11>m22) { float s=std::sqrt(1.0f+m11-m00-m22)*2.0f; q[3]=(m02-m20)/s; q[0]=(m01+m10)/s; q[1]=0.25f*s; q[2]=(m12+m21)/s; }
    else { float s=std::sqrt(1.0f+m22-m00-m11)*2.0f; q[3]=(m10-m01)/s; q[0]=(m02+m20)/s; q[1]=(m12+m21)/s; q[2]=0.25f*s; }
}

// Rotate vector v by quaternion q (q=x,y,z,w): o = q * v * q^-1.
static inline void ProvRotVec(const float* q, const float* v, float* o) {
    const float x=q[0],y=q[1],z=q[2],w=q[3];
    const float tx=2.0f*(y*v[2]-z*v[1]);
    const float ty=2.0f*(z*v[0]-x*v[2]);
    const float tz=2.0f*(x*v[1]-y*v[0]);
    o[0]=v[0]+w*tx+(y*tz-z*ty);
    o[1]=v[1]+w*ty+(z*tx-x*tz);
    o[2]=v[2]+w*tz+(x*ty-y*tx);
}

// THE ROUND'S OWN SEQUENCE, shared by every stub. Each ProvStub<C,S> is a separate function, so a
// function-local static would give each slot its own counter -- and a hitscan round goes through two
// slots, which would then fire the recoil twice. One round is one kick whichever slots it passes.
volatile LONG g_provRecoilSeqSeen = -1;

// ------------------------------------------------------------------------------------------------
// WHOSE ATTACK IS THIS. FINDING THE FIELD, NOT YET USING IT.
//
// The port VMT-hooks the provider CLASS, so slot 33 runs for every shooter in the world -- measured,
// 1200 calls during one firefight. The hand recoil and the muzzle override behind it therefore fire
// on other people's shots: an NPC shooting the player kicks the player's hands, and an NPC's
// projectile gets aimed down the player's barrel.
//
// The chain to the shooter, measured live and confirmed against the original slot-33 implementation
// (RVA 0x405524, which does exactly `src = *(this+0x78); src->vtable[0x10](src, outQuat)`):
//
//     provider + 0x78  ->  orientation source
//     source   + 0x10  ->  gameAttack_Projectile        (class name read from its live CClass)
//
// What is NOT usable: the orientation source's class. A conditional breakpoint that fired only on a
// class other than the player's took 1200 hits and never matched -- everyone's source is the same
// class. And gameAttack_Projectile plus its whole parent chain declare ZERO RTTI properties, so the
// instigator's offset cannot be read out of the type; the script-visible GetInstigator belongs to
// Effector and EffectScriptContext, not to the attack.
//
// So it was measured instead, by recording every qword of the attack that equals the player's entity
// pointer. Two runs on the live build:
//
//     the player firing        6 calls, 6 with the player present, at +0xC8 and +0xD8 (and +0xB8)
//     NPCs firing at the player  156 calls, 156 with the player present NOWHERE
//
// The second run settles the question a single offset could not. The worry was that an NPC shooting
// the player would carry the player as its TARGET and pass any "does this attack mention the player"
// test. It does not: across a whole firefight the player's pointer never appeared in anyone else's
// attack at all.
//
// Static analysis says why, and turns the measurement into structure. gameIAttack's base destructor
// (RVA 0x128108) releases exactly four WeakHandles, through the same weak-handle destructor:
//
//     sub_1402185E8(this + 21)   // +0xA8
//     sub_1402185E8(this + 23)   // +0xB8
//     sub_1402185E8(this + 25)   // +0xC8
//     sub_1402185E8(this + 27)   // +0xD8
//
// Four 16-byte weak handles in a row, which is exactly what the live dump showed: +0xC8 and +0xD8
// both held the player and SHARED one refcount block (two handles to one object -- instigator and
// source are the same entity when the player shoots), +0xB8 held a different object, +0xA8 was null.
// These are the attack's participants, named the way gameAttackInitContext names them: its RTTI
// registration (RVA 0xFB8C50) declares record at +0, instigator at +0x10, source at +0x20 and weapon
// at +0x30. There is no TARGET among them, which is precisely why 156 NPC attacks mentioned the
// player nowhere.
//
// So the gate reads those four handles and nothing else -- narrower than "somewhere in the object",
// and every one of them means the same thing: this attack is the player's.
static constexpr int kProvAttackHandleOffsets[4] = { 0xA8, 0xB8, 0xC8, 0xD8 };
volatile uintptr_t g_playerEntityPtr = 0;
// The launcher DEBUG box. Used below to decide whether the out-quaternion of a slot that does
// nothing functional is worth a VirtualQuery.
extern "C" __declspec(dllexport) extern int CyberpunkVR_XrDeepDiag;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugAtkCalls   = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugAtkNoMatch = 0;
// bit i = the handle at attack+8*i held the player. Kept because it is the cheapest way to see the
// gate still working: a shot of the player's should light one of the four, and nobody else's should
// light any. The 64-qword dump this was found with is gone -- copying the attack object out on every
// call was for the investigation, not for shipping.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugAtkMask    = 0;

// NO VirtualQuery ON THIS PATH -- the same trap WeaponRig.cpp already documents. IsReadable is a
// kernel transition, this runs once per pellet per shooter, and three of them per call cost enough
// frame time to be visible: the FPS dropped the moment a projectile weapon fired. The __try is the
// guard; this is only here to keep it from being exercised on every call.
static inline bool ProvPlausiblePtr(uintptr_t p) {
    return p >= 0x10000ull && p < 0x00007FFFFFFFFFFFull && (p & 7ull) == 0;
}

// Whether this provider's attack belongs to the local player. Returns false when it cannot tell --
// no cached player, an implausible chain -- so an unknown shot is left alone rather than claimed.
//
// THE CLASS DECIDES WHERE TO LOOK, AND THE CLASSES ARE DIFFERENT SIZES.
//
// The +0x78 chain below was reversed on entFuncOrientationProvider and belongs to it alone. The
// override this gates was always applied to every provider class -- a grenade comes through
// entEntity, not entFunc -- so the first version ran the entFunc chain on all three and crashed the
// player while they were simply running: entStaticOrientationProvider is 0x50 bytes, +0x78 is 0x28
// past its end, and the garbage read from there was dereferenced. The dump named the line.
//
// Sizes and fields are the generated SDK's:
//     entEntityOrientationProvider  0x90   WeakHandle<entEntity> entity at +0x58
//     entStaticOrientationProvider  0x50   a quaternion and nothing else -- no owner to ask about
//     entFuncOrientationProvider    0x80   the shot's source at +0x78, see below
static bool ProvIsPlayersAttack(int aClass, uintptr_t aProvider) {
    const uintptr_t player = g_playerEntityPtr;
    if (!player || !ProvPlausiblePtr(aProvider)) return false;

    // entStatic has no owner: it is a fixed orientation, so it can never be the player's shot.
    if (aClass == 1) return false;

    // entEntity names its owner outright.
    if (aClass == 0) {
        bool mine = false;
        __try {
            mine = (*reinterpret_cast<uintptr_t*>(aProvider + 0x58) == player);
        } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
        return mine;
    }

    bool mine = false;
    __try {
        const uintptr_t src = *reinterpret_cast<uintptr_t*>(aProvider + 0x78);
        if (!ProvPlausiblePtr(src)) return false;
        const uintptr_t atk = *reinterpret_cast<uintptr_t*>(src + 0x10);
        if (!ProvPlausiblePtr(atk)) return false;
        InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugAtkCalls));
        const uintptr_t* words = reinterpret_cast<const uintptr_t*>(atk);
        // NOTHING IS DEREFERENCED HERE BUT THE ATTACK ITSELF, and only at the four handle offsets,
        // all inside gameIAttack (0x128) which every attack object has. An earlier version walked
        // every plausible-looking qword in here as if it were a pointer, chasing the back-link to
        // gamedamageAttackData, and crashed the game on the first shot: a word that passes a
        // range-and-alignment test is not a pointer -- the faulting address was 0x3F800078, which is
        // the float 1.0f plus a field offset -- and __try is no licence to read arbitrary addresses
        // in this process, because the engine's vectored handler takes the violation first.
        unsigned long long found = 0;
        for (int h = 0; h < 4; ++h) {
            const int off = kProvAttackHandleOffsets[h];
            if (words[off / 8] == player) found |= (1ull << (off / 8));
        }
        if (found) {
            mine = true;
            InterlockedOr64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugAtkMask),
                            static_cast<LONG64>(found));
        } else {
            InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugAtkNoMatch));
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
    return mine;
}

// Each stub knows its (class C, slot S): bump counter; sample/override the out-quat; call original.
template <int C, int S>
static uintptr_t __fastcall ProvStub(uintptr_t rcx, uintptr_t rdx, uintptr_t r8, uintptr_t r9) {
    g_provCalls[C][S]++;
    // Slots 30..45 of the entFunc class, mirrored out. S is already the slot minus kProvSlotLo.
    if (C == 2 && S >= 30 && S < 46) ++CyberpunkVR_DebugProvEntFunc[S - 30];
    using Fn = uintptr_t(__fastcall*)(uintptr_t,uintptr_t,uintptr_t,uintptr_t);
    Fn orig = reinterpret_cast<Fn>(g_provOrig[C][S]);
    uintptr_t ret = orig ? orig(rcx, rdx, r8, r9) : 0;
    uintptr_t outp = rdx ? rdx : ret;   // out-quat: rdx buffer (or returned rax)
    // DO NOT TOUCH THE OUT-BUFFER ON A SLOT THAT DOES NOT NEED IT.
    //
    // IsReadable is a VirtualQuery -- a kernel transition -- and this stub is installed over 48
    // vtable slots on three provider classes, so it runs for every entity in the world that has an
    // orientation provider. A gun pays it once per pellet; a SECURITY CAMERA sweeping its lens and a
    // TURRET tracking a target evaluate their provider EVERY FRAME, so the cost scaled with how many
    // of those were active nearby. That is the frame rate collapsing around cameras and turrets in
    // 0.1.2, which is the release this VMT hook landed in. The note above ProvPlausiblePtr already
    // says a VirtualQuery on this path costs visible frame time; the fix went to the sibling
    // function and never reached the hottest line in the file.
    //
    // Removing the check is NOT the answer -- tried, and it crashes during loading, because the
    // engine's vectored handler takes the violation before our __try ever sees it (the same trap
    // documented at the attack walk below). So the read stays guarded and the GUARD stays; what goes
    // is the read itself, everywhere it was only ever feeding a diagnostic.
    //
    // Three ways a slot can still need it:
    //   * it is one of the three that carry the shot -- entFunc real slots 33/36/37 -- which is a
    //     compile-time constant per instantiation, so the other 141 stubs fold this to false;
    //   * the manual native override is pointed at exactly this (class, slot);
    //   * DEBUG is on, and the sampling that FOUND slots 33/36/37 in the first place is wanted.
    // SLOT 30 ON EVERY CLASS, and that is not a detail: the launch override below is gated on
    // `S == 30` with NO class test -- its own comment says "on ANY provider class (pistol=entFunc,
    // grenade=entEntity)". A first version of this gate read `C == 2 && ...`, which is entFunc
    // alone, so for a grenade or any projectile coming through entEntity the out-buffer was never
    // read and the whole override block never ran. Slots 33 and 34 are the hitscan orientation
    // reads and those really are entFunc-only.
    constexpr bool kSlotCarriesTheShot = (S == 30) || (C == 2 && (S == 33 || S == 34));
    const bool needOut = kSlotCarriesTheShot
                      || (g_provOverrideCls == C && g_provOverrideSlot == S)
                      || (CyberpunkVR_XrDeepDiag != 0);
    if (needOut && IsReadable(outp, 16)) {
        __try {
            float* q = reinterpret_cast<float*>(outp);
            // Record BEFORE the unit-quaternion test, so the slots that are not orientations --
            // the one we are actually looking for -- get sampled too.
            g_provLastOut[C][S][0] = q[0]; g_provLastOut[C][S][1] = q[1];
            g_provLastOut[C][S][2] = q[2]; g_provLastOut[C][S][3] = q[3];
            g_provOutSeen[C][S] = 1;
            float m = q[0]*q[0]+q[1]*q[1]+q[2]*q[2]+q[3]*q[3];
            if (m > 0.9f && m < 1.1f) {  // unit quaternion -> this slot returns an orientation
                g_provLastQ[0]=q[0]; g_provLastQ[1]=q[1]; g_provLastQ[2]=q[2]; g_provLastQ[3]=q[3];
                // DIAGNOSTIC capture, ALWAYS read-only, hardwired to entFunc(C==2) vtable slot 33
                // (S = 33 - kProvSlotLo = 30) = the PISTOL launch orientation. Capture the original
                // (=camera) quat + the controller quat so the exact transform can be derived offline.
                // SLOTS 33, 36 AND 37 -- one weapon system, three ways in.
                //
                // Slot 33 (S==30) is the PROJECTILE launch orientation, and for a long time it was the
                // only one this port needed: with a mod converting the guns to projectiles, every shot
                // came through it. With that mod off the game is back to its own hitscan, slot 33 goes
                // completely silent, and the round is read through slots 36 and 37 instead -- measured,
                // 8 rounds fired, S=33 and S=34 called exactly 8 times each and S=30 not once.
                if (C == 2 && (S == 30 || S == 33 || S == 34)) {
                    // HAND RECOIL FIRES HERE, because this is the slot that actually runs.
                    //
                    // Measured live while the trigger was held: this slot took 42 calls in ten seconds
                    // of firing, while the Normalize callsite patches -- the ones the old hook dump
                    // credited with 811 shots each -- took ZERO. Those offsets belong to an older exe;
                    // this vtable slot is where the shot is in the build that is running.
                    //
                    // COALESCED PER FRAME. A shotgun calls this once per PELLET, and eight pellets are
                    // one kick, not eight. The muzzle sequence already counts frames for exactly this
                    // reason (the spread delta is recomputed on the first pellet), so the same edge
                    // separates a new round from the rest of its own pattern.
                    //
                    // S==30 ONLY, and that is real vtable slot 33 -- the PROJECTILE launch. The other
                    // two here are real slots 36 and 37, the hitscan orientation reads, and hitscan
                    // recoil now comes from the PhysicalRay origin wrapper, which knows whose shot it
                    // is (playerOwnedWeapon on the effect's shared data). A second, ungated edge on
                    // those slots could only do harm: they run for every shooter in the world, so an
                    // NPC firing at the player was winning this latch and kicking the player's hands.
                    //
                    // And it only fires for the player's own attack, which ProvIsPlayersAttack settles
                    // from the attack's participant handles.
                    if (S == 30 && ProvIsPlayersAttack(C, rcx)) {
                        const LONG recoilSeq = static_cast<LONG>(g_provMuzzleSeq);
                        const LONG recoilSeen = g_provRecoilSeqSeen;
                        if (recoilSeen != recoilSeq &&
                            InterlockedCompareExchange(&g_provRecoilSeqSeen, recoilSeq, recoilSeen) == recoilSeen) {
                            RecoilOnShot();
                        }
                    }
                    g_provOrigQ[0]=q[0]; g_provOrigQ[1]=q[1]; g_provOrigQ[2]=q[2]; g_provOrigQ[3]=q[3];
                    if (g_pSharedHands) {
                        g_provCtrlQ[0]=g_pSharedHands[12]; g_provCtrlQ[1]=g_pSharedHands[13]; g_provCtrlQ[2]=g_pSharedHands[14]; g_provCtrlQ[3]=g_pSharedHands[15];
                        g_provHmdQ[0]=g_pSharedHands[16]; g_provHmdQ[1]=g_pSharedHands[17]; g_provHmdQ[2]=g_pSharedHands[18]; g_provHmdQ[3]=g_pSharedHands[19];
                    }
                }
                // ENABLE: VR-overlay-driven via shared[58] -> override slot 33 (S==30) on ANY provider
                // class (pistol=entFunc, grenade=entEntity), mode shared[59] (default 5 = game muzzle).
                // Native SetVRProvOverrideSlot path kept as a manual fallback.
                //
                // S==30 ONLY. It used to cover the two hitscan orientation slots (S==33 and S==34, real
                // vtable 36 and 37) as well, from when this was the only way a bullet left the barrel.
                // It is not any more: the hitscan ray is rewritten at PhysicalRay::Execute, where the
                // shot's own shared data says whether the weapon is the player's. These slots carry no
                // such marker and run for EVERY shooter, so overriding them aimed other people's shots
                // down the player's barrel.
                //
                // What may still want them is the tracer and the muzzle flash, which read the shot
                // orientation from here rather than from the ray. If those go back to following the
                // camera, the answer is to restore these two slots behind the owner test -- not to
                // drop the test.
                // ...and only for the player's own attack. Without that test this aimed everyone
                // else's projectiles down the player's barrel.
                // NO LONGER KEYED ON THE AIM TOGGLE (dabinn, TofuExpress d002d314). Both aiming
                // models now launch from the live muzzle -- hand aim because the controller points the
                // weapon, head aim because the head does -- so the condition is simply "the player has
                // a weapon out and its muzzle is published". Keying this on shared[58] is what made
                // head aim mean "shoot at the crosshair instead", i.e. a reticle with no relationship
                // to the thing in the player's hands.
                bool sharedOn = (g_pSharedHands &&
                                 g_pSharedHands[vrshared::kWeaponFlag] > 0.5f &&
                                 g_pSharedHands[27] > 0.5f && S == 30 &&
                                 ProvIsPlayersAttack(C, rcx));
                bool nativeOn = (g_provOverrideCls == C && g_provOverrideSlot == S);
                if ((sharedOn || nativeOn) && g_pSharedHands) {
                    // shared (VR-overlay) path defaults to mode 6 = muzzle + preserved spread.
                    const int mode = sharedOn ? 6 : g_provQuatMode;
                    if (mode == 6) {
                        // MUZZLE + PRESERVE SPREAD (shotguns): rotate the whole shot CONE so its center
                        // lands on the barrel, keeping each pellet's relative offset.
                        //   delta = muzzle (X) conj(qCenter)  -- qCenter = first pellet's quat this frame
                        //   qNew  = delta (X) q               -- per pellet
                        // 1 pellet (pistol) -> qNew = muzzle exactly. N pellets -> pattern around muzzle.
                        float mq[4]={g_provMuzzleQ[0],g_provMuzzleQ[1],g_provMuzzleQ[2],g_provMuzzleQ[3]};
                        if (mq[0]*mq[0]+mq[1]*mq[1]+mq[2]*mq[2]+mq[3]*mq[3] > 0.5f) {
                            if (g_provDeltaSeq != g_provMuzzleSeq) {   // recompute once per frame (first pellet)
                                float cqx=-q[0],cqy=-q[1],cqz=-q[2],cqw=q[3];           // conj(q)
                                g_provDeltaQ[0]=mq[3]*cqx+mq[0]*cqw+mq[1]*cqz-mq[2]*cqy; // mq (X) conj(q)
                                g_provDeltaQ[1]=mq[3]*cqy-mq[0]*cqz+mq[1]*cqw+mq[2]*cqx;
                                g_provDeltaQ[2]=mq[3]*cqz+mq[0]*cqy-mq[1]*cqx+mq[2]*cqw;
                                g_provDeltaQ[3]=mq[3]*cqw-mq[0]*cqx-mq[1]*cqy-mq[2]*cqz;
                                g_provDeltaSeq = g_provMuzzleSeq;
                            }
                            float dx=g_provDeltaQ[0],dy=g_provDeltaQ[1],dz=g_provDeltaQ[2],dw=g_provDeltaQ[3];
                            float ax=q[0],ay=q[1],az=q[2],aw=q[3];                       // delta (X) q
                            q[0]=dw*ax+dx*aw+dy*az-dz*ay;
                            q[1]=dw*ay-dx*az+dy*aw+dz*ax;
                            q[2]=dw*az+dx*ay-dy*ax+dz*aw;
                            q[3]=dw*aw-dx*ax-dy*ay-dz*az;
                            g_provOverrides++;
                        }
                    } else if (mode == 5) {
                        // MUZZLE: use the game's own muzzle WORLD orientation (CET publishes it via
                        // weapon:GetMuzzleSlotWorldTransform). Pure game-world, NO controller-space math.
                        // g_provMuzzleFwdAxis selects which muzzle local axis = barrel (default +Y).
                        float mq[4]={g_provMuzzleQ[0],g_provMuzzleQ[1],g_provMuzzleQ[2],g_provMuzzleQ[3]};
                        if (mq[0]*mq[0]+mq[1]*mq[1]+mq[2]*mq[2]+mq[3]*mq[3] > 0.5f) {
                            if (g_provFwdAxis == 1) {            // muzzle orientation used directly
                                q[0]=mq[0]; q[1]=mq[1]; q[2]=mq[2]; q[3]=mq[3];
                            } else {                            // rebuild +Y-forward from the chosen muzzle axis
                                float ax[3]={0,0,0}; ax[g_provFwdAxis==0?0:2]=1.0f;
                                float fw[3]; ProvRotVec(mq, ax, fw);
                                float nq[4]; BuildOrientFromFwd(fw[0],fw[1],fw[2],1,nq);
                                q[0]=nq[0];q[1]=nq[1];q[2]=nq[2];q[3]=nq[3];
                            }
                            g_provOverrides++;
                        }
                    } else if (mode == 4) {
                        // DATA-DERIVED correct aim. The controller BARREL = its local -Z axis (not +Y!).
                        //   barrel_base = rot(ctrl, (0,0,-1));  barrel_head = rot(conj(hmd), barrel_base)
                        //   fwd_world   = rot(camera/origQ, barrel_head);  qNew = lookQuat(fwd_world,+Y)
                        // Identity when controller points where the head looks -> crosshair; tracks the
                        // controller as it deviates. (Verified against the aligned-sample quaternions.)
                        float cq[4]={g_pSharedHands[12],g_pSharedHands[13],g_pSharedHands[14],g_pSharedHands[15]};
                        float hq[4]={g_pSharedHands[16],g_pSharedHands[17],g_pSharedHands[18],g_pSharedHands[19]};
                        if (cq[0]*cq[0]+cq[1]*cq[1]+cq[2]*cq[2]+cq[3]*cq[3]>0.5f &&
                            hq[0]*hq[0]+hq[1]*hq[1]+hq[2]*hq[2]+hq[3]*hq[3]>0.5f) {
                            float zaxis[3]={0,0,-1}, cb[3], ch[3], cw[3];
                            ProvRotVec(cq, zaxis, cb);                 // barrel in base
                            float hconj[4]={-hq[0],-hq[1],-hq[2],hq[3]};
                            ProvRotVec(hconj, cb, ch);                 // -> head-local
                            float oq[4]={q[0],q[1],q[2],q[3]};
                            ProvRotVec(oq, ch, cw);                    // -> world via camera basis
                            float nq[4]; BuildOrientFromFwd(cw[0], cw[1], cw[2], 1 /*+Y*/, nq);
                            q[0]=nq[0]; q[1]=nq[1]; q[2]=nq[2]; q[3]=nq[3]; g_provOverrides++;
                        }
                    } else if (mode == 3) {
                        // qNew = original(camera) * [ inv(swap(hmd)) * swap(controller) ]
                        // = camera rotated by the controller's offset-from-head, in game-local space.
                        // swap=(x,-z,y,w) is the VR->game axis map (matches VRIK). Identity when the
                        // controller points where the head looks -> bullet stays on crosshair; tracks
                        // the controller as it deviates. (Derived from the aligned-sample numbers.)
                        float cqx=g_pSharedHands[12], cqy=g_pSharedHands[13], cqz=g_pSharedHands[14], cqw=g_pSharedHands[15];
                        float hqx=g_pSharedHands[16], hqy=g_pSharedHands[17], hqz=g_pSharedHands[18], hqw=g_pSharedHands[19];
                        if (cqx*cqx+cqy*cqy+cqz*cqz+cqw*cqw>0.5f && hqx*hqx+hqy*hqy+hqz*hqz+hqw*hqw>0.5f) {
                            // swapped controller, swapped head
                            float sc[4]={cqx,-cqz,cqy,cqw};
                            float sh[4]={hqx,-hqz,hqy,hqw};
                            float ih[4]={-sh[0],-sh[1],-sh[2],sh[3]};           // inv(swap(hmd))
                            // delta = ih (X) sc
                            float dx=ih[3]*sc[0]+ih[0]*sc[3]+ih[1]*sc[2]-ih[2]*sc[1];
                            float dy=ih[3]*sc[1]-ih[0]*sc[2]+ih[1]*sc[3]+ih[2]*sc[0];
                            float dz=ih[3]*sc[2]+ih[0]*sc[1]-ih[1]*sc[0]+ih[2]*sc[3];
                            float dw=ih[3]*sc[3]-ih[0]*sc[0]-ih[1]*sc[1]-ih[2]*sc[2];
                            // qNew = original (X) delta
                            float ax=q[0],ay=q[1],az=q[2],aw=q[3];
                            q[0]=aw*dx+ax*dw+ay*dz-az*dy;
                            q[1]=aw*dy-ax*dz+ay*dw+az*dx;
                            q[2]=aw*dz+ax*dy-ay*dx+az*dw;
                            q[3]=aw*dw-ax*dx-ay*dy-az*dz;
                            g_provOverrides++;
                        }
                    } else if (mode == 2) {
                        // EXACTLY VRIK_WriteHand: handWorld = headQuat * swap(controllerQuat).
                        // The provider's ORIGINAL out-quat == the camera/head orientation, so it plays
                        // headQuat; delta = swap(shared[12..15]) = (x,-z,y,w) of the controller quat.
                        // qNew = original (X) delta  -> the VRIK-correct world hand aim (head-stable).
                        float hx=g_pSharedHands[12], hy=g_pSharedHands[13], hz=g_pSharedHands[14], hw=g_pSharedHands[15];
                        if (hx*hx+hy*hy+hz*hz+hw*hw > 0.5f) {
                            float dx=hx, dy=-hz, dz=hy, dw=hw;          // VR->game axis swap (i,-k,j,r)
                            float ax=q[0],ay=q[1],az=q[2],aw=q[3];      // original (head/camera)
                            q[0]=aw*dx+ax*dw+ay*dz-az*dy;               // Hamilton original (X) delta
                            q[1]=aw*dy-ax*dz+ay*dw+az*dx;
                            q[2]=aw*dz+ax*dy-ay*dx+az*dw;
                            q[3]=aw*dw-ax*dx-ay*dy-az*dz;
                            g_provOverrides++;
                        }
                    } else if (mode == 1) {
                        float fx=g_pSharedHands[60], fy=g_pSharedHands[61], fz=g_pSharedHands[62];
                        if (fx*fx+fy*fy+fz*fz > 0.25f) {
                            float nq[4]; BuildOrientFromFwd(fx, fy, fz, g_provFwdAxis, nq);
                            q[0]=nq[0]; q[1]=nq[1]; q[2]=nq[2]; q[3]=nq[3]; g_provOverrides++;
                        }
                    } else {
                        float cx=g_pSharedHands[53],cy=g_pSharedHands[54],cz=g_pSharedHands[55],cw=g_pSharedHands[56];
                        if (cx*cx+cy*cy+cz*cz+cw*cw > 0.5f) { q[0]=cx; q[1]=cy; q[2]=cz; q[3]=cw; g_provOverrides++; }
                    }
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return ret;
}
static void* g_provStubTbl[kProvNCls][kProvNSlots] = {0};
template <int C, int... I> static void FillRow(std::integer_sequence<int, I...>) {
    ((g_provStubTbl[C][I] = reinterpret_cast<void*>(&ProvStub<C, I>)), ...);
}
volatile int g_provInstalled = 0;
static int InstallProvClass(int c) {
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* cls = rtti ? rtti->GetClass(kProvNames[c]) : nullptr;
    if (!cls) return -1;
    void* inst = cls->CreateInstance(true);
    if (!inst) return -2;
    uintptr_t vt = 0;
    __try { vt = *reinterpret_cast<uintptr_t*>(inst); } __except (EXCEPTION_EXECUTE_HANDLER) {}
    if (!vt) return -3;
    g_provVtbl[c] = vt;
    DWORD oldp = 0;
    if (!VirtualProtect(reinterpret_cast<void*>(vt + kProvSlotLo*8), kProvNSlots*8, PAGE_EXECUTE_READWRITE, &oldp)) return -4;
    for (int i = 0; i < kProvNSlots; ++i) {
        uintptr_t* slot = reinterpret_cast<uintptr_t*>(vt + (kProvSlotLo + i)*8);
        g_provOrig[c][i] = reinterpret_cast<void*>(*slot);
        *slot = reinterpret_cast<uintptr_t>(g_provStubTbl[c][i]);
    }
    VirtualProtect(reinterpret_cast<void*>(vt + kProvSlotLo*8), kProvNSlots*8, oldp, &oldp);
    return 1;
}
void InstallVRProvInstrument(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    if (g_provInstalled) { if (aOut) *aOut = 2; return; }
    FillRow<0>(std::make_integer_sequence<int, kProvNSlots>{});
    FillRow<1>(std::make_integer_sequence<int, kProvNSlots>{});
    FillRow<2>(std::make_integer_sequence<int, kProvNSlots>{});
    int ok = 0;
    for (int c = 0; c < kProvNCls; ++c) if (InstallProvClass(c) == 1) ++ok;
    g_provInstalled = ok > 0 ? 1 : 0;
    if (aOut) *aOut = ok;   // number of provider classes instrumented (expect 3)
}
// arg = cls*1000 + REAL vtable slot (e.g. 0*1000+33 for entEntity slot 33); -1 = off.
// Stored override slot is the STUB INDEX (realSlot - kProvSlotLo) to match ProvStub<C,S>.
void SetVRProvOverrideSlot(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t v = -1; RED4ext::GetParameter(aFrame, &v); aFrame->code++;
    if (v < 0) { g_provOverrideCls = -1; g_provOverrideSlot = -1; }
    else {
        int realSlot = v % 1000;
        g_provOverrideCls = v / 1000;
        g_provOverrideSlot = realSlot - kProvSlotLo;   // -> stub index
    }
}
void SetVRProvQuatMode(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t mode = 1, axis = 1; RED4ext::GetParameter(aFrame, &mode); RED4ext::GetParameter(aFrame, &axis);
    aFrame->code++; g_provQuatMode = mode; g_provFwdAxis = axis;
}
void SetVRMuzzleQuat(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float i=0,j=0,k=0,r=1; RED4ext::GetParameter(aFrame,&i); RED4ext::GetParameter(aFrame,&j);
    RED4ext::GetParameter(aFrame,&k); RED4ext::GetParameter(aFrame,&r); aFrame->code++;
    g_provMuzzleQ[0]=i; g_provMuzzleQ[1]=j; g_provMuzzleQ[2]=k; g_provMuzzleQ[3]=r; ++g_provMuzzleSeq;
    // THE PLAYER'S ENTITY, CACHED ON A THREAD THAT MAY ASK FOR IT.
    // FindPlayerEntity runs a script global; the provider stubs run on engine worker threads and must
    // never call it. This native arrives from Lua on the script thread once a frame while a weapon is
    // out, which is the only place in this file that is allowed to look it up. Refreshed rarely because
    // the pointer only changes across a load.
    {
        static uint32_t s_playerRefresh = 0;
        if ((s_playerRefresh++ % 180u) == 0u) {
            if (auto* player = FindPlayerEntity())
                g_playerEntityPtr = reinterpret_cast<uintptr_t>(player);
        }
    }
    // Publish the muzzle WORLD forward (+Y of the quat) to shared[24..26] (FREE slots; [33..47] are
    // VRIK IK-calib, [50..62] weapon-aim) so the dxgi overlay can project an EXACT barrel crosshair
    // (this dir through the located camera = the eye view).
    if (g_pSharedHands) {
        g_pSharedHands[24] = 2.0f*(i*j - k*r);
        g_pSharedHands[25] = 1.0f - 2.0f*(i*i + k*k);
        g_pSharedHands[26] = 2.0f*(j*k + i*r);
        g_pSharedHands[27] = 1.0f;  // valid
    }
}
// Publish the current ADS/scope zoom factor to shared[28] so the dxgi overlay can scale the
// barrel laser-dot's screen offset by it (the scope magnifies the image but the bullet still
// leaves the barrel). CET pushes PlayerStateMachine.ZoomLevel each frame; 1.0 = no zoom.
// Companion to SetVRMuzzleQuat: the same GetMuzzleSlotWorldTransform the CET weapon mod already
// reads, minus the half it used to throw away.
void SetVRMuzzlePos(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float x = 0.0f, y = 0.0f, z = 0.0f;
    RED4ext::GetParameter(aFrame, &x);
    RED4ext::GetParameter(aFrame, &y);
    RED4ext::GetParameter(aFrame, &z);
    aFrame->code++;
    // The transform returns WORLD coordinates on most frames and something local on others --
    // measured alternating (3187.26, -376.40, 134.16) and (0.00, 0.29, 0.04) frame to frame.
    // A local value is not a muzzle position and must not overwrite a good one: keep the last
    // world-space sample instead, or the consumer flips between two answers every other frame.
    if (x*x + y*y + z*z < 1.0f) return;
    g_provMuzzlePos[0] = x; g_provMuzzlePos[1] = y; g_provMuzzlePos[2] = z;
    ++g_provMuzzlePosSeq;
    if (g_pSharedHands) {
        g_pSharedHands[200] = x; g_pSharedHands[201] = y; g_pSharedHands[202] = z;
        g_pSharedHands[203] = 1.0f;   // valid
    }
}

// THE WEAPON'S OWN RECOIL, HANDED TO THE HAND SPRING. Called on every draw by the weapon module, with
// the value it reads off the equipped weapon BEFORE it zeroes the camera kick.
//
// This is what makes the hand recoil per-weapon without a table and without identifying the weapon in
// the plugin at all. The number is the game's own RecoilKickMax in degrees, and its ordering is exactly
// the physics: Kappa 0.24, Chao 0.40, Yukimura 0.70, Kenshin 0.80, Lexington 1.00, Silverhand 1.90,
// Liberty 2.00, Unity 2.25, Omaha 2.80, Nue 3.20, Overture 4.00 -- a .44 revolver kicking four times a
// 9 mm is what the impulse ratio says it should. Modded weapons come along for free.
//
// It is a per-DRAW value, not per-frame data: nothing here is in the pose path's timing, so it does not
// touch the rule that the solve takes nothing from CET.
extern "C" __declspec(dllexport) float CyberpunkVR_WeaponKickDeg;

// WHICH WEAPON IS IN HAND, by family name, published on each draw beside its kick.
//
// The two-hand hold is weapon-specific -- a pistol's support hand sits on the same grip, a rifle's is out
// on the handguard 35 cm away -- so its captured file has to be per weapon, and the plugin needs a name to
// key it by. It has no way to ask: the rig signature identifies the thirteen weapons the reload knows and
// says nothing about the rest, while the family is already resolved script-side for the recoil table.
//
// This is per-DRAW identity, not per-frame data, so it does not touch the rule that the pose path takes
// nothing from CET: the pose path reads a string that was fixed the moment the weapon came out.
extern "C" __declspec(dllexport) char CyberpunkVR_WeaponName[64] = {0};

// The console instrumentation that found the hitscan path -- the targeting-system lookup, the
// call counters, the argument spy, and the look-at / ray-query / aim-point levers -- lived here.
// All of it was scaffolding, and every one of those levers was applied live without moving the
// impact. What does is documented in docs/cp2077-hitscan-physicalray-muzzle.md.

void SetVRWeaponName(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    RED4ext::CString s;
    RED4ext::GetParameter(aFrame, &s);
    aFrame->code++;
    const char* p = s.c_str();
    int j = 0;
    if (p) {
        for (int i = 0; p[i] && j < 63; ++i) {
            const char c = p[i];
            // Only what can safely become a file name, and lower case so the same weapon is one file
            // however the caller spells it.
            if ((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_') CyberpunkVR_WeaponName[j++] = c;
            else if (c >= 'A' && c <= 'Z') CyberpunkVR_WeaponName[j++] = static_cast<char>(c - 'A' + 'a');
        }
    }
    CyberpunkVR_WeaponName[j] = '\0';
}

void SetVRWeaponKick(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float k = 0.0f; RED4ext::GetParameter(aFrame, &k); aFrame->code++;
    CyberpunkVR_WeaponKickDeg = (k > 0.0f && k < 100.0f) ? k : 0.0f;
}

// IS THE GAME SPRINTING -- published from its own SprintEvents wraps, because the blackboard does not
// say (measured: PlayerStateMachine.Locomotion stays Default through a player sprint). The sprint gesture
// drives a TOGGLE, so it needs to know whether the toggle is currently on; without this it kept pressing
// and turned its own sprint off again.
void SetVRSprintActive(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t active = 0;
    RED4ext::GetParameter(aFrame, &active);
    aFrame->code++;
    g_VRSprintActive = (active != 0) ? 1 : 0;
    if (aOut) *aOut = 1;
}

// The player's LOCOMOTION state machine value (gamePSMLocomotionStates). Only script can see it, and
// the plugin needs it for one decision: a sprint detent must not stand a crouching player up. Anything
// negative means "unknown", which the consumer treats as no gate rather than as "not crouched".
void SetVRLocomotionState(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t state = -1;
    RED4ext::GetParameter(aFrame, &state);
    aFrame->code++;
    g_VRLocomotionState = state;
    if (aOut) *aOut = 1;
}

// ---- the weapon state machine, for the non-VRIK ADS muzzle stabilizer --------------------------
//
// Redscript is the only place these are visible: the weapon PSM value and the ADS AimInTimeRemaining
// live on the player's local blackboard, and the PublicSafeToReady raise is a state transition rather
// than a value at all. Both natives return Int32 so the redscript side can be written as a plain
// expression, matching the upstream form (dabinn, TofuExpress 797a2a95).
void SetVRWeaponPoseState(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t weaponState = 0;
    float aimInRemaining = 0.0f;
    RED4ext::GetParameter(aFrame, &weaponState);
    RED4ext::GetParameter(aFrame, &aimInRemaining);
    aFrame->code++;
    g_VRWeaponPsmState = static_cast<float>(weaponState);
    g_VRAimInRemaining = aimInRemaining;
    if (aOut) *aOut = 1;
}

void SetVRWeaponRaiseTransition(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    int32_t active = 0;
    RED4ext::GetParameter(aFrame, &active);
    aFrame->code++;
    g_VRWeaponRaiseTransition = (active != 0) ? 1 : 0;
    if (aOut) *aOut = 1;
}

// The wrist the physical reload owns, from the reload CET mod to the collision one -- see
// vrshared::kReloadOwnedHand for why this crosses through the plugin at all. Anything that is not
// 0 or 1 publishes -1, so a mod that stops updating cannot leave a wrist excluded forever.
void SetVRReloadOwnedHand(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t hand = -1; RED4ext::GetParameter(aFrame, &hand); aFrame->code++;
    if (g_pSharedHands) {
        g_pSharedHands[vrshared::kReloadOwnedHand] =
            (hand == 0 || hand == 1) ? static_cast<float>(hand) : -1.0f;
    }
}

// DIAGNOSTIC ONLY: the live camera GetZoom, published to shared[28] for telemetry.
//
// DO NOT scale an overlay or a projection with this. MAIN's render projection already contains
// ADS magnification, so applying this on top double-zooms every ordinary weapon (measured
// 1.3x * 1.3x), and the value is sampled by CET on its own schedule so it can also be a frame
// out of step. See GetOverlayProjTans, which takes the factor from the projection itself.
void SetVRZoomLevel(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float z = 1.0f; RED4ext::GetParameter(aFrame, &z); aFrame->code++;
    if (g_pSharedHands) g_pSharedHands[28] = (z > 0.01f && z < 64.0f) ? z : 1.0f;
}
// Melee fire pulse -> shared[29]. The CET weapon mod pulses this on a VR swing; the dxgi XInput merge
// reads it and forces the right trigger so the GAME performs its own native melee attack (native
// damage / crits / numbers / armor). A swing doesn't press RT by itself, hence the inject.
// Melee RT IMPULSE: the CET mod calls this on a detected VR swing with a small frame count (e.g. 4).
// dxgi (HookedXInputGetState) sees shared[29] > 0 and taps RT for that many frames, so the game enters
// its NATIVE melee-attack PSM state вЂ” dealing fully native damage / combo / numbers / markers вЂ” then
// decrements it back to 0. (Not a sustained hold, so it's a single attack press, not spam.)
void SetVRMeleeFire(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t v = 0; RED4ext::GetParameter(aFrame, &v); aFrame->code++;
    if (g_pSharedHands) g_pSharedHands[29] = (float)v;
}
// THE PORT'S SAY OVER THE RIGHT TRIGGER -> shared[161], read by the XInput merge in the stereo module.
// 0 = pass it through, 1 = swallow it, 2 = press it fully. The physical reload uses both ends: a revolver with its
// cylinder swung out swallows the trigger (it has nothing under the hammer), and a cocked one presses it fully as
// soon as the finger has moved far enough, which is what single action means.
//
// Written every frame while a revolver is in hand and cleared to 0 the moment it is not -- a latched 1 here would
// be a gun that never fires again, so the Lua side sends 0 on teardown rather than trusting a state flag.
void SetVRTriggerMode(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t v = 0; RED4ext::GetParameter(aFrame, &v); aFrame->code++;
    EnsureSharedMemory();
    if (g_pSharedHands) g_pSharedHands[161] = (float)((v < 0) ? 0 : ((v > 2) ? 2 : v));
}
// Live MODE of the Aim_JNT shake kill (g_VRCamBoneFreeze: 0 stock / 1 yaw-live / 2 full /
// 3 swing-only).
void SetVRCamBoneFreeze(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    int32_t v = 0; RED4ext::GetParameter(aFrame, &v); aFrame->code++;
    g_VRCamBoneFreeze = (v < 0) ? 0 : ((v > 4) ? 4 : v);
}
// Live tuning of the clean-pair XY slew rate (m/s), CET: SetVRPairSlew(rate).
// The rate trades the two faces of the SAME sprint-transient artifact: too low
// (0.5) = body/hands visibly float/drag on sprint start/stop and snap turns;
// raw/high (10) = the old flash-lurch (sprint dive + the sprint snap "double",
// which the frozen camTrace proved is the 20cm camera lead swinging, not yaw).
// Sweet spot expected around 0.8..1.5.
void SetVRPairSlew(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float v = 1.0f; RED4ext::GetParameter(aFrame, &v); aFrame->code++;
    g_VRPairSlewRate = (v < 0.1f) ? 0.1f : ((v > 10.0f) ? 10.0f : v);
}
// Clean-pair prediction lead in ticks (0..2), CET: SetVRPairLead(t). See g_VRPairLeadTicks.
void SetVRPairLead(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float v = 0.0f; RED4ext::GetParameter(aFrame, &v); aFrame->code++;
    g_VRPairLeadTicks = (v < 0.0f) ? 0.0f : ((v > 2.0f) ? 2.0f : v);
}
// [CAMWRITE] Lua ack: echo the consumed publish seq ([151]) into [152]. The dxgi
// locate hook uses an advancing ack as the "component write path is ALIVE" gate;
// without it (mod removed, CET dead, menus) dxgi falls back to the legacy stomp.
void SetVRCamAck(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float v = 0.0f; RED4ext::GetParameter(aFrame, &v); aFrame->code++;
    EnsureSharedMemory();
    if (g_pSharedHands) g_pSharedHands[152] = v;
}

// Read the held-trigger power flag (shared[30], published by the dxgi XInput merge while in melee
// mode). The CET weapon mod uses it as the power-attack modifier for the next swing.
void GetVRMeleeTrigger(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, int32_t* aOut, int64_t) {
    aFrame->code++;
    if (aOut) *aOut = (g_pSharedHands && g_pSharedHands[30] > 0.5f) ? 1 : 0;
}
// Generic shared-slot read for CET mods that need raw values (hand HMD-local poses, grip analog,
// etc.). idx must be 0..255 -- the shared block is 256 floats (CyberpunkVR_Hands_Shared, 1024
// bytes; all three modules map the same 1024). Full slot map: src/shared_slots.h.
void GetVRSharedSlot(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t) {
    int32_t idx = 0; RED4ext::GetParameter(aFrame, &idx); aFrame->code++;
    if (aOut) *aOut = (g_pSharedHands && idx >= 0 && idx < 256) ? g_pSharedHands[idx] : 0.0f;
}
void GetVRProvDump(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, float* aOut, int64_t) {
    int32_t idx = 0; RED4ext::GetParameter(aFrame, &idx); aFrame->code++;
    double v = 0.0;
    if (idx == 103) v = g_provQuatMode;
    else if (idx == 104) v = g_provFwdAxis;
    else if (idx == 100) v = g_provInstalled;
    else if (idx == 101) v = (double)g_provOverrides;
    else if (idx == 102) v = (g_provOverrideCls < 0) ? -1 : (g_provOverrideCls*1000 + g_provOverrideSlot);
    else if (idx >= 200 && idx < 204) v = g_provLastQ[idx-200];
    else if (idx >= 210 && idx < 214) v = g_provOrigQ[idx-210];   // original (camera) quat
    else if (idx >= 214 && idx < 218) v = g_provCtrlQ[idx-214];   // controller quat shared[12..15]
    else if (idx >= 218 && idx < 222) v = g_provHmdQ[idx-218];    // hmd quat shared[16..19]
    else if (idx >= 0 && idx < kProvNCls*kProvNSlots) v = (double)g_provCalls[idx / kProvNSlots][idx % kProvNSlots];
    if (aOut) *aOut = (float)v;
}
// Clear the per-(class,slot) GetOrientation fire counters + sampled quats. Lets the user check each
// weapon fresh: clear -> equip+fire one weapon -> read which entFunc/entEntity slot fired.
void ResetVRProvCounts(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    aFrame->code++;
    for (int c = 0; c < kProvNCls; ++c)
        for (int s = 0; s < kProvNSlots; ++s) g_provCalls[c][s] = 0;
    g_provOverrides = 0;
    for (int i = 0; i < 4; ++i) { g_provLastQ[i]=0; g_provOrigQ[i]=0; g_provCtrlQ[i]=0; g_provHmdQ[i]=0; }
}


