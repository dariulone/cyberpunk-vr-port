#include "Utils/DebugGate.hpp"
// The weapon-aim path: the native hitscan from the muzzle, and the two engine functions the camera
// code needs this family to own.
//
// WHAT IS LEFT, AND WHY ONLY THIS. The family grew while the shot was being searched for: a hardware
// breakpoint tracer on the camera, detours on TargetHelper, the shot classifier, the generic trace
// funnel, PlayerShotCaller, the camera transform and orientation getters, the heading offset, two
// Normalize call sites and the secondary physics trace, counters on four more shot functions, and a
// write of the VR muzzle into the crosshair targeting cache. The bullet was found -- it is the
// PhysicalRay evaluator below, and the projectile launch slot in Natives/OrientationProvider.cpp --
// and the rest stayed installed at boot as instrumentation, or as levers behind switches nobody set.
// Not harmless: the targeting cache (+0x350 of the cache at this+0x30CC8 of the update at 0x19C75C)
// is read by every look-at query, not only by the shot, so its origin moved to the muzzle made
// interaction prompts disappear -- q303's suit duffle had no Take/Equip, Jackie's ofrenda items
// neither (found by a tester, 2026-10-17). The muzzle no longer writes that cache.
// Head gaze instead adjusts only its direction to the binocular view centre;
// the original camera/interaction origin and the physical shot path stay intact.
//
//   PhysicalRay   gameEffectObjectProvider_PhysicalRay::Execute's origin and forward: bullets from the
//                 muzzle, along the barrel, and the hitscan hand recoil. The shot path itself.
//   0x21297C      a transform copy the camera pose bookkeeping (Camera/PoseIdentity) follows across;
//                 installed here because this family owned the address first.
//   0x19C75C      the targeting update, for one thing only: re-aiming the targeting at the device
//                 camera the player looks through, after the engine's own update (Camera/TakeoverTargeting).
//
// The ABI is Anim/WeaponAimState.hpp; what anything outside calls is declared in Anim/WeaponAim.hpp.

#include "Anim/WeaponAim.hpp"
#include "Hooks/Hook.hpp"   // CVR_HOOK: this family installs at boot, see below
#include "Camera/PoseIdentity.hpp"
#include "Camera/CameraState.hpp"
#include "Camera/TakeoverTargeting.hpp"
#include "Camera/GazeCenter.hpp"
#include "Core/VrCoreShared.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Utils/MemorySafe.hpp"
#include "Hooks/SightLine.hpp"
#include "Utils/SharedSlots.hpp"

#include <windows.h>
#include <cstdint>
#include <cstring>
#include <cmath>
#include <algorithm>
#include <intrin.h>
#include <MinHook.h>

extern "C" __declspec(dllexport) int CyberpunkVR_DebugWaInstalled = 0;
extern "C" __declspec(dllexport) uintptr_t CyberpunkVR_GazeTargetState = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_GazeTargetUpdates = 0;

// ------------------------------------------------------------------------------------------------
// FILE-SCOPE OBJECTS HERE ARE `inline`, NOT `static`, AND THAT IS LOAD-BEARING. This file was a
// header carrying its implementation; with `static`, a second including TU got its own copy of every
// trampoline original -- null -- and called through it the first time that path ran.
// ------------------------------------------------------------------------------------------------

// ============================================================================
// TARGETING UPDATE @0x19C75C, per frame (this = rcx). The original refreshes the
// cache first. Device takeover and the head-gaze direction are applied afterward.
// ============================================================================
typedef void* (*WaTargetingUpdateFn)(void*, void*, void*, void*);
inline WaTargetingUpdateFn OrigWaTargetingUpdate = nullptr;

extern "C" inline void* Hooked_WaTargetingUpdate(void* rcx, void* rdx, void* r8, void* r9) {
    void* result=OrigWaTargetingUpdate(rcx, rdx, r8, r9);
    if(g_remoteCamOn.load(std::memory_order_acquire) &&
       g_takeoverEntityId.load(std::memory_order_acquire) && DeviceCamActive() &&
       g_menuModeValue==0 && !cvr::vrui::CapturesInput() && OpenXRManager::Get().IsSessionRunning()) {
        const auto player=cvr::roomscale::PlayerIdentity();
        uint64_t entity{};
        if(player && ReadU64Safe(player+0x48,&entity))
            cvr::camera::RefreshTakeoverTargeting(rcx,player,entity);
    }
    if(g_menuModeValue==0 && !cvr::vrui::CapturesInput() && OpenXRManager::Get().IsSessionRunning()) {
        const auto player=cvr::roomscale::PlayerIdentity();uint64_t entity{};
        if(player && ReadU64Safe(player+0x48,&entity)) {
            const bool centered=cvr::camera::CenterHeadGaze(rcx,player,entity,cvr::gaze::Read().tangentY);
            CVR_DIAGNOSTIC(CyberpunkVR_GazeTargetState=reinterpret_cast<uintptr_t>(rcx));
            CVR_DIAGNOSTIC(CyberpunkVR_GazeTargetUpdates+=centered?1:0);
        }
    }
    return result;
}

// ============================================================================
// POSE COPY @0x21297C (dest = rcx, source = rdx). The camera pose bookkeeping follows a pose it
// knows from the source address to the destination.
// ============================================================================
typedef void* (*WaPoseCopyFn)(void*, void*, void*, void*);
inline WaPoseCopyFn OrigWaPoseCopy = nullptr;

extern "C" inline void* Hooked_WaPoseCopy(void* rcx, void* rdx, void* r8, void* r9) {
    const auto poseSource=cvr::camera::CapturePoseAddress(reinterpret_cast<uintptr_t>(rdx));
    void* result = OrigWaPoseCopy(rcx, rdx, r8, r9);
    cvr::camera::TransferPoseAddress(poseSource,reinterpret_cast<uintptr_t>(rcx));
    return result;
}

static inline void* WaAllocRelayNear(uint8_t* target) {
    SYSTEM_INFO si{}; GetSystemInfo(&si);
    const uintptr_t gran = si.dwAllocationGranularity ? si.dwAllocationGranularity : 0x10000;
    const uintptr_t ta = reinterpret_cast<uintptr_t>(target);
    for (uintptr_t dist = gran; dist < 0x70000000ull; dist += gran) {
        uintptr_t cand[2] = { ta + dist, ta > dist ? ta - dist : 0 };
        for (uintptr_t c : cand) {
            if (!c) continue;
            c &= ~(gran - 1);
            void* m = VirtualAlloc(reinterpret_cast<void*>(c), gran, MEM_RESERVE | MEM_COMMIT, PAGE_EXECUTE_READWRITE);
            if (m) return m;
        }
    }
    return nullptr;
}

// ================================================================================================
// NATIVE HITSCAN, FROM THE MUZZLE.
//
// gameEffectObjectProvider_PhysicalRay::Execute builds the firearm's damage ray. The port replaces
// its origin with the VR muzzle and rotates its direction from the camera axis onto the barrel,
// at the two exact evaluator callsites inside that function. Everything about how those addresses,
// the owner test and the cone centre were established is in docs/cp2077-hitscan-physicalray-muzzle.md.
// ================================================================================================
extern "C" __declspec(dllexport) int CyberpunkVR_HitscanFromMuzzle = 1;

extern "C" __declspec(dllexport) int CyberpunkVR_DebugPhysicalRayPatched = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayCalls = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayMine = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayOriginWrites = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayForwardWrites = 0;
// Rays redirected with the camera cone centre available.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayConeRays = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayRecoil = 0;
// Rays that reached the callsite but are not the local player's weapon: the character's own ground
// probes, world effects, anyone else's gun. Expected to be far larger than the shot counts.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayForeign = 0;
// Player shots sent out of the LEFT gun's barrel (two guns in hand).
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayLeftGun = 0;
// The last two-gun shot as WaLeftGunFired saw it: left (1) or right (0), the path that decided (1 record, 2 one
// trigger, 3 both triggers in turn, 0 none), left and right trigger; the shot's record and the two published.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugDualShot[4] = {};
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugDualShotRecord[3] = {};
// Ours by the game's own mark, but the published muzzle was nowhere near the ray. Should stay 0.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayStaleMuzzle = 0;
// Rays started on the sight line (Hooks/SightLine.hpp) instead of at the muzzle, and the last lift.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRaySightLine = 0;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugPhysicalRaySightLift[4] = {};
extern "C" __declspec(dllexport) float CyberpunkVR_DebugPhysicalRayRawForward[4] = {};
extern "C" __declspec(dllexport) float CyberpunkVR_DebugPhysicalRayOutForward[4] = {};

// NO READABILITY PROBE IN THIS FILE ANY MORE. There was one -- WaReadable, a VirtualQuery wrapper --
// and behind it a per-thread region cache, both there to make three syscalls per ray affordable. The
// syscalls are gone instead: see the note above WaPlayerOwnedWeapon for why the engine's own successful
// call on the same object is the guarantee they were standing in for. If a probe is ever needed here
// again it belongs on a one-shot path, never on one that runs per ray.

// The barrel, as the rest of the port publishes it: the muzzle slot's world position (shared[200..202],
// valid at [203]) and the muzzle transform's world +Y axis (shared[24..26], valid at [27]). The latter
// is published by SetVRMuzzleQuat and is the same basis used by the working entFunc projectile slots.
// shared[60..62] belongs to the older controller/projectile bridge and does not include the weapon
// model's muzzle-axis correction, so it must not be used for native hitscan.
inline bool WaMuzzle(float* pos, float* fwd) {
    if (!g_pSharedHands) return false;
    if (g_pSharedHands[203] < 0.5f || g_pSharedHands[27] < 0.5f) return false;
    pos[0] = g_pSharedHands[200]; pos[1] = g_pSharedHands[201]; pos[2] = g_pSharedHands[202];
    float fx = g_pSharedHands[24], fy = g_pSharedHands[25], fz = g_pSharedHands[26];
    float l2 = fx*fx + fy*fy + fz*fz;
    if (l2 < 0.25f) return false;
    const float inv = 1.0f / std::sqrt(l2);
    fwd[0] = fx * inv; fwd[1] = fy * inv; fwd[2] = fz * inv;
    return true;
}

// PhysicalRay::Execute evaluates its camera-space origin and forward into stack locals, then builds
// the combat query from those values. These are the two exact evaluator callsites reached by the
// player's pistol immediately before the verified query at 0x950912.
typedef char (*WaPhysicalRayEvalFn)(void*, void*, float*);
inline WaPhysicalRayEvalFn s_waPhysicalRayEvalOrig = nullptr;
inline uint8_t* s_waPhysicalRayOriginRelay = nullptr;
inline uint8_t* s_waPhysicalRayForwardRelay = nullptr;
inline thread_local bool s_waPhysicalRayMine = false;
inline thread_local uint32_t s_waPhysicalRayMuzzleSeq = 0xFFFFFFFFu;
inline thread_local void* s_waPhysicalRayContext = nullptr;      // the blackboard the origin was written for
inline thread_local float s_waPhysicalRayOrigin[3] = {};
inline thread_local float s_waPhysicalRayMuzzleForward[3] = {};
inline thread_local float s_waPhysicalRayConeCenter[3] = {};
inline thread_local bool s_waPhysicalRayHaveCentre = false;

extern "C" void RecoilOnShot();
extern "C" void RecoilOnShotLeft();
// The left gun's round latch while two guns are in hand: its own, so a frame in which both guns fire
// kicks both hands (g_provRecoilSeqSeen is the weapon in hand's).
inline volatile LONG s_waRecoilSeqSeenLeft = -1;
// 1 = centre the cone on the CAMERA forward, which is what this hook did originally and what keeps
// the game's accuracy roll on a single shot. 0 = centre it on the shot's FIRST RAY, the way the
// projectile path already centres on the first pellet, so a single shot lands on the barrel and a
// pattern keeps its shape.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ConeCentreFromCamera = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugConeFirstRay = 0;

// The camera as the LOCATE site published it. Declared here the way the overlay declares it, which
// is the same value the barrel dot is drawn from -- so the dot and the bullet share one frame.
extern volatile float g_lastLocateQuat[4];

// ------------------------------------------------------------------------------------------------
// WHOSE RAY IS THIS. THE GAME ANSWERS IT ITSELF, SO DO NOT GUESS.
//
// PhysicalRay is generic. The player's own CHARACTER fires service rays through it -- a ground check
// starts 0.5 m above the player's feet, points straight down and runs 2 m, and there is more than
// one such effect. The filter this replaces asked "does the ray start within 1.5 m of the muzzle",
// which that ground check passes with a weapon in hand (muzzle to feet+0.5 m is about 1.0 m), so
// every frame of movement was rotating the character's own ground probe onto the barrel. An NPC
// firing at contact range would have passed it too.
//
// Distance cannot answer the question. The game already carries the answer: the effect's shared
// data has a Bool field named playerOwnedWeapon, registered next to muzzlePosition, raycastEnd,
// forward and range in the same table (registrar sub_140824108, entry 148, type Bool). Our two
// patched callsites are handed that container in rdx.
//
// The container, read out of blackboard vtable +0x108 (RVA 0x276368) and verified live:
//     keys  = *(uint64_t**)(bb + 0x48)     CName array, searched linearly
//     count = *(uint32_t*)(bb + 0x54)
//     vals  = *(uint8_t**)(bb + 0x58)      24 bytes per entry
//     entry +0x00 = type pointer, low bit is a tag; the pointer part must be non-zero
//     entry +0x08 = the value, inline for a small type
//
// Measured on the live build, one player pistol shot and one ground-check ray. The keys are CNames,
// and a CName here is FNV1a64 of the name, so both sets resolve back to text:
//
//     shot, 24 keys:  weaponItemRecord  range  attackStatModList  ricochetData  ricochetCount
//                     spreadingData  spreadingCount  spreadingTargets  hitCooldown  muzzlePosition
//                     charge  fxPackage  PLAYEROWNEDWEAPON=1  inTPP  ricochetInRow  minRayAngleDiff
//                     lastRayDir  lastRayPos  shootTime  randRoll  ignoreMountedVehicleCollision
//                     attackData  position  forward
//     ground check, 4 keys:  range  position  forward  attackId
//
// That list is also what settles what the flag MEANS. It is not "the player is holding a gun" --
// a global player state would not be sitting between spreadingData and attackData in the shared
// data of one shot. It is a property of the weapon that fired THIS ray, next to weaponItemRecord,
// which is the record of that same weapon. Not yet confirmed against an NPC's shot; if that ever
// reads true, weaponItemRecord alone will not save us and the instigator has to be dug out instead.
//
// The entry after playerOwnedWeapon shared its type pointer and held 0, which is what confirms the
// value is read from +0x08 and not from the tag.
//
// weaponItemRecord is required as well. On its own it only proves the ray came out of a weapon --
// an NPC's shot carries it too -- but it costs nothing in the same pass and keeps a hypothetical
// non-weapon player effect that happened to carry the flag from being treated as a shot.
//
// Reading the container directly rather than calling the virtual getter is deliberate: the getter
// takes the container's lock and hands back a variant that has to be released, and neither is worth
// doing from a detour on a worker thread for three loads. Every failure -- unreadable memory, an
// absent field, a torn count -- falls out as "not ours", which leaves the shot vanilla.
static constexpr uint64_t kWaPlayerOwnedWeaponCName = 0x8CB4C0891BD255EDull;  // FNV1a64
static constexpr uint64_t kWaWeaponItemRecordCName  = 0x8DBDD9257C33612Full;  // FNV1a64
// The blackboard's typed Vector4 setter, sub_140475078 (blackboard, &key, &value, flag): the one the
// PhysicalRay family itself calls (sub_14084CFC8) to publish "position" and "forward", the keys the
// impact consumers read back through sub_1401208D8 (sub_14084CD94, sub_140D17100). Refused if a game
// update moved it.
static constexpr uintptr_t kWaSharedVector4SetRva   = 0x475078;
static constexpr uint8_t kWaSharedVector4SetBytes[] = {0x4C, 0x8B, 0xDC, 0x49, 0x89, 0x5B, 0x08};
// The shot's "position" and "forward" (EffectSharedData), plain name hashes like the two keys above.
// NOT the entries sub_14084CFC8 publishes: the key table it uses at 0x143472650/+8 resolves live to
// "lastRayDir"/"lastRayPos" (2026-10-16), the piercing continuation, which is left alone.
static constexpr uint64_t kWaPositionCName          = 0x4CBF3A26FCA1D74Aull;  // FNV1a64("position")
static constexpr uint64_t kWaForwardCName           = 0x1E65942E132F4F7Aull;  // FNV1a64("forward")

// Range and alignment only, copied from ProvPlausiblePtr in src/Natives/OrientationProvider.cpp rather
// than reinvented: it is the same question asked in the same process, and this file needs the cheap half
// of it on a path that runs per ray.
static inline bool WaPlausiblePtr(uintptr_t p) {
    return p >= 0x10000ull && p < 0x00007FFFFFFFFFFFull && (p & 7ull) == 0;
}

// NO SYSCALL ON THIS PATH, and that is sound rather than brave.
//
// The caller runs the ENGINE'S OWN evaluator first and returns early unless it succeeded, so by the time
// this function looks at the blackboard the engine has just read that same object and reported success:
// it is a live container of the expected class, and keys/count/vals are its own documented fields. The
// three VirtualQuery calls that used to guard them were guarding a possibility the successful original
// call already excludes -- and they were the whole cost. Measured under one camera with a weapon drawn:
// 17538 rays, 15222 of them foreign and all of them past the geometric bound, 9053 reaching the kernel
// even with a region cache in front, because the container and its arrays are allocated PER EFFECT and so
// land in regions no cache has seen.
//
// This is NOT the act that crashed the orientation provider. That note is about walking EVERY qword of a
// struct as if it were a pointer -- "a word that passes a range-and-alignment test is not a pointer" --
// which is a different thing from reading three named fields of a container the engine just used. What is
// kept from it is the cheap half: non-null, aligned, user space, and a bounded count. Any torn or absent
// field still falls out as "not ours", which leaves the shot vanilla.
static bool WaPlayerOwnedWeapon(const void* aBlackboard) {
    if (!aBlackboard) return false;
    const uint8_t* bb = static_cast<const uint8_t*>(aBlackboard);
    if (!WaPlausiblePtr(reinterpret_cast<uintptr_t>(bb))) return false;

    const uint64_t* keys = *reinterpret_cast<const uint64_t* const*>(bb + 0x48);
    const uint32_t count = *reinterpret_cast<const uint32_t*>(bb + 0x54);
    const uint8_t* vals = *reinterpret_cast<const uint8_t* const*>(bb + 0x58);
    // 256 is a bound on nonsense, not on the game: the shot's own container holds 24 entries -- read live
    // in the debugger, with weaponItemRecord at key 0 and playerOwnedWeapon at key 12.
    if (!keys || !vals || count == 0 || count > 256) return false;
    if (!WaPlausiblePtr(reinterpret_cast<uintptr_t>(keys))) return false;
    if (!WaPlausiblePtr(reinterpret_cast<uintptr_t>(vals))) return false;

    bool fromWeapon = false;
    bool playerOwned = false;
    for (uint32_t i = 0; i < count; ++i) {
        const uint64_t key = keys[i];
        if (key != kWaPlayerOwnedWeaponCName && key != kWaWeaponItemRecordCName) continue;
        const uint8_t* entry = vals + static_cast<size_t>(i) * 24;
        if ((*reinterpret_cast<const uint64_t*>(entry) & ~1ull) == 0) continue;   // empty slot
        if (key == kWaWeaponItemRecordCName) fromWeapon = true;
        else playerOwned = entry[8] != 0;
    }
    return fromWeapon && playerOwned;
}

// WHICH GUN FIRED, WITH TWO IN HAND. The published muzzle is the RIGHT gun's; the left gun's barrel is
// published beside it while the game reports dual wielding (kLeftMuzzle*, valid only with kDualWieldActive).
// NOT BY muzzlePosition: the key is in the shot's shared data, but a hitscan shot leaves it at 0,0,0 (measured
// 2026-10-27 on Liberty + Nue), so the left barrel was never recognised. The shot names its gun by
// weaponItemRecord (a handle to the record; its TweakDBID hash) against the two records the reload mod
// publishes (CyberpunkVR_DualWieldRecords). Two guns of one model look the same that way; then the trigger
// decides -- each trigger fires only its own hand's gun (vrport_dual_wield.reds) -- and with both squeezed the
// pair's shots go out right, then left. Anything unreadable keeps the right gun, as every shot did before.
extern "C" volatile uint32_t CyberpunkVR_DualWieldRecords[2];
static std::atomic<uint32_t> s_waDualAlternate{0};
static bool WaLeftGunFired(const void* aBlackboard, float* pos, float* fwd) {
    if (!g_pSharedHands) return false;
    if (g_pSharedHands[vrshared::kDualWieldActive] < 0.5f || g_pSharedHands[vrshared::kLeftMuzzleValid] < 0.5f)
        return false;
    const uint8_t* bb = static_cast<const uint8_t*>(aBlackboard);
    const uint64_t* keys = *reinterpret_cast<const uint64_t* const*>(bb + 0x48);
    const uint32_t count = *reinterpret_cast<const uint32_t*>(bb + 0x54);
    const uint8_t* vals = *reinterpret_cast<const uint8_t* const*>(bb + 0x58);
    if (!keys || !vals || count == 0 || count > 256) return false;
    // The entry holds a handle to the WeaponItem record (instance at +8); the record's TweakDBID is at +0x40
    // (gamedataTweakDBRecord), the hash in its low 32 bits. Called inside the evaluator hook's __try.
    uint32_t record = 0;
    for (uint32_t i = 0; i < count; ++i) {
        if (keys[i] != kWaWeaponItemRecordCName) continue;
        const uint8_t* entry = vals + static_cast<size_t>(i) * 24;
        const uintptr_t instance = *reinterpret_cast<const uintptr_t*>(entry + 8);
        if ((*reinterpret_cast<const uint64_t*>(entry) & ~1ull) != 0 && WaPlausiblePtr(instance))
            record = *reinterpret_cast<const uint32_t*>(instance + 0x40);
        break;
    }
    const uint32_t leftRecord = CyberpunkVR_DualWieldRecords[0], rightRecord = CyberpunkVR_DualWieldRecords[1];
    const int hands = static_cast<int>(g_pSharedHands[vrshared::kDualWieldActive]);
    const bool leftTrigger = (hands & vrshared::kDualWieldLeftFires) != 0;
    const bool rightTrigger = (hands & vrshared::kDualWieldRightFires) != 0;
    bool left = false;
    int path = 0;
    if (record && leftRecord && rightRecord && leftRecord != rightRecord) {left = record == leftRecord; path = 1;}
    else if (leftTrigger != rightTrigger) {left = leftTrigger; path = 2;}
    else if (leftTrigger) {left = (s_waDualAlternate.fetch_add(1, std::memory_order_relaxed) & 1u) != 0; path = 3;}
    if (cvr::RuntimeDiagnosticsEnabled()) {
        CyberpunkVR_DebugDualShot[0] = left ? 1.0f : 0.0f; CyberpunkVR_DebugDualShot[1] = static_cast<float>(path);
        CyberpunkVR_DebugDualShot[2] = leftTrigger ? 1.0f : 0.0f; CyberpunkVR_DebugDualShot[3] = rightTrigger ? 1.0f : 0.0f;
        CyberpunkVR_DebugDualShotRecord[0] = record;
        CyberpunkVR_DebugDualShotRecord[1] = leftRecord; CyberpunkVR_DebugDualShotRecord[2] = rightRecord;
    }
    if (!left) return false;
    const float* muzzle = &g_pSharedHands[vrshared::kLeftMuzzlePos];
    const float* f = &g_pSharedHands[vrshared::kLeftMuzzleFwd];
    const float l2 = f[0]*f[0] + f[1]*f[1] + f[2]*f[2];
    if (!(l2 > 0.25f)) return false;
    const float inv = 1.0f / std::sqrt(l2);
    pos[0] = muzzle[0]; pos[1] = muzzle[1]; pos[2] = muzzle[2];
    fwd[0] = f[0] * inv; fwd[1] = f[1] * inv; fwd[2] = f[2] * inv;
    return true;
}

// ONE SHOT, ONE IMPACT. The two evaluator hooks move the ray the query is cast along, but the effect's
// blackboard keeps the "position" and "forward" the engine published for the camera ray, and the impact
// consumers read those back: the hit landed twice, once where the barrel pointed and once where the
// camera did. Publish the ray actually cast -- origin and the final forward, pellet spread included --
// through the engine's own typed setter. From the TofuExpress fork (2026-10-16).
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRayShared = 0;
// 1 (default) = publish the cast ray into the shot's shared data; 0 = leave the engine's camera ray (A/B).
extern "C" __declspec(dllexport) int CyberpunkVR_HitscanSharedRay = 1;
// Shots whose shared data did not carry both keys: nothing is written then, so no entry is ever ADDED.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugPhysicalRaySharedMissing = 0;
// The last player shot's shared-data keys, diagnostics only: [0] = count, then up to 63 keys.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugPhysicalRayKeys[64] = {};

// Whether the shot's container already holds both keys. Same three fields WaPlayerOwnedWeapon reads,
// on the same object the engine's evaluator has just used successfully.
static bool WaSharedHasPositionForward(const void* aBlackboard) {
    const uint8_t* bb = static_cast<const uint8_t*>(aBlackboard);
    const uint64_t* keys = *reinterpret_cast<const uint64_t* const*>(bb + 0x48);
    const uint32_t count = *reinterpret_cast<const uint32_t*>(bb + 0x54);
    if (!keys || count == 0 || count > 256 || !WaPlausiblePtr(reinterpret_cast<uintptr_t>(keys))) return false;
    if (::cvr::RuntimeDiagnosticsEnabled()) {
        const uint32_t n = count < 63 ? count : 63;
        for (uint32_t i = 0; i < n; ++i) CyberpunkVR_DebugPhysicalRayKeys[1 + i] = keys[i];
        CyberpunkVR_DebugPhysicalRayKeys[0] = count;
    }
    bool position = false, forward = false;
    for (uint32_t i = 0; i < count; ++i) {
        position |= keys[i] == kWaPositionCName;
        forward |= keys[i] == kWaForwardCName;
    }
    return position && forward;
}

static bool WaWriteSharedVector4(void* aBlackboard, uint64_t aKey, const float* aValue, float aW) {
    if (!aBlackboard || !aValue || !g_waExeBase) return false;
    if (!std::isfinite(aValue[0]) || !std::isfinite(aValue[1]) || !std::isfinite(aValue[2])) return false;
    static const bool s_setterMatches =
        std::memcmp(reinterpret_cast<const void*>(g_waExeBase + kWaSharedVector4SetRva),
                    kWaSharedVector4SetBytes, sizeof(kWaSharedVector4SetBytes)) == 0;
    if (!s_setterMatches) return false;
    using Setter = bool (*)(void*, const uint64_t*, const float*, uint8_t);
    alignas(16) const float value[4] = {aValue[0], aValue[1], aValue[2], aW};
    const auto setter = reinterpret_cast<Setter>(g_waExeBase + kWaSharedVector4SetRva);
    return setter(aBlackboard, &aKey, value, 0);
}

// A COUNTER READ FROM SIX THREADS MUST BE INCREMENTED FROM SIX THREADS.
// A shotgun round runs one PhysicalRay per pellet, each on its own worker thread, and a plain ++
// on a shared global loses increments: one measured round reported 6 origin writes against 4
// forward writes, and the missing two read as "two pellets flew unredirected". They had not --
// the counter had. Diagnostics that can be wrong by a race are worse than no diagnostics.
static inline void WaCount(unsigned long long& aCounter) {
    if(!cvr::RuntimeDiagnosticsEnabled())return;
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(&aCounter));
}

// THE CONE CENTRE IS THE GAME CAMERA'S FORWARD. MEASURED, not assumed.
//
// One shotgun round, stopped in the debugger with the redirect off, at the point where both of
// PhysicalRay::Execute's branches meet: six rays, ONE shared origin (equal to the rendered camera
// position we publish in slot 204), six unit forwards, range 50. Their mean sits 0.47 deg from the
// forward of g_lastLocateQuat -- and the pellets' own offsets from that forward were 0.38, 0.53,
// 1.50, 1.15, 0.80 and 0.74 deg, so the mean of six samples of that cone is expected to miss the
// true axis by about that much. The camera forward IS the axis the game spreads around.
//
// Which is why this needs no shot sequence, no first-pellet latch and no cross-thread handshake:
// the centre is a property of the FRAME, identical for every pellet, and every worker can read it
// for itself. The three previous attempts each tried to carry a per-round value between threads
// (muzzle sequence, then TargetHelper's counter behind a seqlock) and each failed for the same
// reason -- there was no per-round value to carry. TargetHelper is called once per PELLET.
//
// Game forward is +Y; this is the same expansion LocateCamera uses to publish slot 60.
static bool WaCameraForward(float* aOut) {
    const float x = g_lastLocateQuat[0], y = g_lastLocateQuat[1];
    const float z = g_lastLocateQuat[2], w = g_lastLocateQuat[3];
    const float n = x*x + y*y + z*z + w*w;
    if (!std::isfinite(n) || n < 0.98f || n > 1.02f) return false;
    aOut[0] = 2.0f * (x*y - z*w);
    aOut[1] = 1.0f - 2.0f * (x*x + z*z);
    aOut[2] = 2.0f * (y*z + x*w);
    return std::isfinite(aOut[0]) && std::isfinite(aOut[1]) && std::isfinite(aOut[2]);
}

static bool WaRotationBetween(const float* from, const float* to, float* q) {
    float ax = from[0], ay = from[1], az = from[2];
    float bx = to[0], by = to[1], bz = to[2];
    const float al2 = ax*ax + ay*ay + az*az;
    const float bl2 = bx*bx + by*by + bz*bz;
    if (al2 < 1e-6f || bl2 < 1e-6f) return false;
    const float ai = 1.0f / std::sqrt(al2);
    const float bi = 1.0f / std::sqrt(bl2);
    ax *= ai; ay *= ai; az *= ai;
    bx *= bi; by *= bi; bz *= bi;

    const float dot = std::clamp(ax*bx + ay*by + az*bz, -1.0f, 1.0f);
    if (dot > 0.999999f) {
        q[0] = 0.0f; q[1] = 0.0f; q[2] = 0.0f; q[3] = 1.0f;
        return true;
    }
    if (dot < -0.999999f) {
        float rx = (std::fabs(ax) < 0.9f) ? 1.0f : 0.0f;
        float ry = (rx == 0.0f) ? 1.0f : 0.0f;
        float rz = 0.0f;
        const float qx = ay*rz - az*ry;
        const float qy = az*rx - ax*rz;
        const float qz = ax*ry - ay*rx;
        const float qi = 1.0f / std::sqrt(qx*qx + qy*qy + qz*qz);
        q[0] = qx * qi; q[1] = qy * qi; q[2] = qz * qi; q[3] = 0.0f;
        return true;
    }

    q[0] = ay*bz - az*by;
    q[1] = az*bx - ax*bz;
    q[2] = ax*by - ay*bx;
    q[3] = 1.0f + dot;
    const float qi = 1.0f / std::sqrt(q[0]*q[0] + q[1]*q[1] + q[2]*q[2] + q[3]*q[3]);
    q[0] *= qi; q[1] *= qi; q[2] *= qi; q[3] *= qi;
    return true;
}

static void WaRotateVector(const float* q, const float* v, float* out) {
    const float tx = 2.0f * (q[1]*v[2] - q[2]*v[1]);
    const float ty = 2.0f * (q[2]*v[0] - q[0]*v[2]);
    const float tz = 2.0f * (q[0]*v[1] - q[1]*v[0]);
    out[0] = v[0] + q[3]*tx + (q[1]*tz - q[2]*ty);
    out[1] = v[1] + q[3]*ty + (q[2]*tx - q[0]*tz);
    out[2] = v[2] + q[3]*tz + (q[0]*ty - q[1]*tx);
}

extern "C" inline char Hooked_WaPhysicalRayOrigin(void* rcx, void* rdx, float* out) {
    s_waPhysicalRayMine = false;
    s_waPhysicalRayMuzzleSeq = 0xFFFFFFFFu;
    s_waPhysicalRayContext = nullptr;
    s_waPhysicalRayHaveCentre = false;
    s_waPhysicalRayMuzzleForward[0] = 0.0f;
    s_waPhysicalRayMuzzleForward[1] = 0.0f;
    s_waPhysicalRayMuzzleForward[2] = 0.0f;
    const char result = s_waPhysicalRayEvalOrig ? s_waPhysicalRayEvalOrig(rcx, rdx, out) : 0;
    WaCount(CyberpunkVR_DebugPhysicalRayCalls);
    if (!result || !out || !CyberpunkVR_HitscanFromMuzzle) return result;

    // NO SHOT-SCOPE GATE HERE, and that was tried. Gating this leaf on the shot-frame flag the old
    // TargetHelper / orientation-getter detours kept looked right, but that flag was stamped by the
    // PROJECTILE path, and hitscan does not go through it, so the window shut on our own rays and the
    // bullet went back to the game's native aim. What separates our ray from a camera's is the
    // ownership flag on the effect's own blackboard, and that test is now free: see WaPlayerOwnedWeapon.
    __try {
        // THE FREE CHECKS COME FIRST, AND THE ORDER IS THE WHOLE BUG.
        //
        // WaPlayerOwnedWeapon costs THREE VirtualQuery calls -- the blackboard, its key array, its
        // value array -- and a VirtualQuery is a kernel transition. It used to run before anything
        // else, so EVERY ray in the world paid it. PhysicalRay is not only the firearm: a security
        // camera sweeping for the player and a turret tracking one cast rays continuously, from
        // several worker threads, and the frame rate went from 60-70 to about 10 near either.
        // Confirmed by gating the whole family off and watching the drop disappear.
        //
        // This is the trap the note above ProvPlausiblePtr in OrientationProvider.cpp calls
        // "three of them per call". That note was written about THIS function. It was fixed on
        // that side and never came back here.
        //
        // Nothing is weakened. The conjunction is identical and every guard is still in place;
        // only the order changed. WaMuzzle reads our own shared block and the distance bound reads
        // the out-buffer the original evaluator just wrote, so both are free -- and both are false
        // for any ray that did not start at the player's own barrel. A camera across the street is
        // rejected before the first syscall.
        float muzzle[3], forward[3];
        if (!WaMuzzle(muzzle, forward)) return result;

        // A staleness bound, not identity. The muzzle slot keeps its last value when the weapon is
        // holstered, and one was measured 192 m from the camera that way. A published muzzle that far
        // from the ray's own origin cannot be the barrel that fired it, and firing along it would
        // throw the bullet sideways. 3 m is well past any real weapon-to-eye distance.
        const float dx = out[0] - muzzle[0];
        const float dy = out[1] - muzzle[1];
        const float dz = out[2] - muzzle[2];
        const float distanceSq = dx*dx + dy*dy + dz*dz;
        if (!std::isfinite(distanceSq) || distanceSq > 3.0f * 3.0f) {
            WaCount(CyberpunkVR_DebugPhysicalRayStaleMuzzle);
            return result;
        }

        // ...and only now the expensive one, on a ray that already starts at the player's barrel.
        if (!WaPlayerOwnedWeapon(rdx)) {
            WaCount(CyberpunkVR_DebugPhysicalRayForeign);
            return result;
        }

        // Two guns in hand: the left gun's shot leaves the left barrel, with no sight line and no kick
        // in the right hand.
        float leftMuzzle[3], leftForward[3];
        const bool leftGun = WaLeftGunFired(rdx, leftMuzzle, leftForward);
        if (leftGun) {
            muzzle[0] = leftMuzzle[0]; muzzle[1] = leftMuzzle[1]; muzzle[2] = leftMuzzle[2];
            forward[0] = leftForward[0]; forward[1] = leftForward[1]; forward[2] = leftForward[2];
            WaCount(CyberpunkVR_DebugPhysicalRayLeftGun);
        }

        s_waPhysicalRayMine = true;
        s_waPhysicalRayMuzzleSeq = g_provMuzzleSeq;
        // Both bases are sampled HERE, on the same instant, and carried to the forward callsite
        // that follows a few instructions later. Re-reading either one there would let a frame
        // land between the two halves of one ray's rotation.
        s_waPhysicalRayMuzzleForward[0] = forward[0];
        s_waPhysicalRayMuzzleForward[1] = forward[1];
        s_waPhysicalRayMuzzleForward[2] = forward[2];
        s_waPhysicalRayHaveCentre = WaCameraForward(s_waPhysicalRayConeCenter);
        WaCount(CyberpunkVR_DebugPhysicalRayMine);
        // With the eye on a sight the shot leaves the sight line, still along the barrel, so it
        // lands on the mark at every range (Hooks/SightLine.hpp).
        float lift[3] = {0.0f, 0.0f, 0.0f};
        if (!leftGun && cvr::sight::SightLineOffset(forward, lift)) {
            WaCount(CyberpunkVR_DebugPhysicalRaySightLine);
            CyberpunkVR_DebugPhysicalRaySightLift[0] = lift[0];
            CyberpunkVR_DebugPhysicalRaySightLift[1] = lift[1];
            CyberpunkVR_DebugPhysicalRaySightLift[2] = lift[2];
            CyberpunkVR_DebugPhysicalRaySightLift[3] = std::sqrt(lift[0]*lift[0] + lift[1]*lift[1] + lift[2]*lift[2]);
        }
        out[0] = muzzle[0] + lift[0]; out[1] = muzzle[1] + lift[1]; out[2] = muzzle[2] + lift[2]; out[3] = 1.0f;
        s_waPhysicalRayContext = rdx;
        s_waPhysicalRayOrigin[0] = out[0]; s_waPhysicalRayOrigin[1] = out[1]; s_waPhysicalRayOrigin[2] = out[2];
        WaCount(CyberpunkVR_DebugPhysicalRayOriginWrites);

        const LONG recoilSeq = static_cast<LONG>(s_waPhysicalRayMuzzleSeq);
        const LONG recoilSeen = g_provRecoilSeqSeen;
        if (!leftGun && recoilSeen != recoilSeq &&
            InterlockedCompareExchange(&g_provRecoilSeqSeen, recoilSeq, recoilSeen) == recoilSeen) {
            RecoilOnShot();
            WaCount(CyberpunkVR_DebugPhysicalRayRecoil);
        }
        // ...and a round out of the LEFT gun kicks the left hand and is felt in it.
        const LONG leftSeen = s_waRecoilSeqSeenLeft;
        if (leftGun && leftSeen != recoilSeq &&
            InterlockedCompareExchange(&s_waRecoilSeqSeenLeft, recoilSeq, leftSeen) == leftSeen) {
            RecoilOnShotLeft();
            WaCount(CyberpunkVR_DebugPhysicalRayRecoil);
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        s_waPhysicalRayMine = false;
        s_waPhysicalRayContext = nullptr;
    }
    return result;
}

extern "C" inline char Hooked_WaPhysicalRayForward(void* rcx, void* rdx, float* out) {
    const char result = s_waPhysicalRayEvalOrig ? s_waPhysicalRayEvalOrig(rcx, rdx, out) : 0;
    const bool redirect = s_waPhysicalRayMine;
    const bool haveCentre = s_waPhysicalRayHaveCentre;
    const float centre[3] = {
        s_waPhysicalRayConeCenter[0], s_waPhysicalRayConeCenter[1], s_waPhysicalRayConeCenter[2]
    };
    const float forward[3] = {
        s_waPhysicalRayMuzzleForward[0],
        s_waPhysicalRayMuzzleForward[1],
        s_waPhysicalRayMuzzleForward[2]
    };
    void* const sharedContext = s_waPhysicalRayContext;
    const float finalOrigin[3] = {s_waPhysicalRayOrigin[0], s_waPhysicalRayOrigin[1], s_waPhysicalRayOrigin[2]};
    s_waPhysicalRayMine = false;
    s_waPhysicalRayMuzzleSeq = 0xFFFFFFFFu;
    s_waPhysicalRayContext = nullptr;
    s_waPhysicalRayHaveCentre = false;
    if (!result || !redirect || !out || !CyberpunkVR_HitscanFromMuzzle) return result;

    __try {
        const float raw[3] = {out[0], out[1], out[2]};
        CyberpunkVR_DebugPhysicalRayRawForward[0] = raw[0];
        CyberpunkVR_DebugPhysicalRayRawForward[1] = raw[1];
        CyberpunkVR_DebugPhysicalRayRawForward[2] = raw[2];
        CyberpunkVR_DebugPhysicalRayRawForward[3] = out[3];

        // THE CENTRE OF THE CONE IS THE SHOT'S FIRST RAY, not the camera.
        //
        // Rotating by (camera -> barrel) preserves each ray's offset from the CAMERA axis, and for a
        // single shot that offset is the game's accuracy roll -- the flat-screen stand-in for an
        // unsteady hand. In VR the hand IS the aim, so that roll is just the bullet disagreeing with
        // the sight. Centring on the first ray of the shot instead makes both cases one rule with no
        // weapon to classify: a single ray is its own centre and lands exactly on the barrel, while a
        // pattern keeps every pellet's offset relative to its first pellet.
        //
        // The shot is keyed by the muzzle sequence, which is exactly what the projectile path keys
        // its own cone delta by -- see g_provDeltaSeq in OrientationProvider.cpp. Same key, so the
        // two paths cut a shot's rays into groups the same way.
        static uint32_t s_waConeSeq = 0xFFFFFFFFu;
        static float    s_waConeFirstRaw[3] = {0.0f, 0.0f, 0.0f};
        float coneCentre[3] = {centre[0], centre[1], centre[2]};
        bool haveCone = haveCentre;
        if (CyberpunkVR_ConeCentreFromCamera == 0) {
            const uint32_t shot = g_provMuzzleSeq;
            if (s_waConeSeq != shot) {
                s_waConeSeq = shot;
                s_waConeFirstRaw[0] = raw[0];
                s_waConeFirstRaw[1] = raw[1];
                s_waConeFirstRaw[2] = raw[2];
                WaCount(CyberpunkVR_DebugConeFirstRay);
            }
            coneCentre[0] = s_waConeFirstRaw[0];
            coneCentre[1] = s_waConeFirstRaw[1];
            coneCentre[2] = s_waConeFirstRaw[2];
            haveCone = (coneCentre[0]*coneCentre[0] + coneCentre[1]*coneCentre[1]
                        + coneCentre[2]*coneCentre[2]) > 1e-6f;
        }

        // ONE ROTATION, CONE AXIS -> BARREL AXIS, APPLIED TO THE PELLET AS IT CAME.
        float spreadDelta[4];
        if (haveCone) {
            if (!WaRotationBetween(coneCentre, forward, spreadDelta)) return result;
            WaCount(CyberpunkVR_DebugPhysicalRayConeRays);
        } else if (!WaRotationBetween(raw, forward, spreadDelta)) {
            // No camera this frame: put the ray on the barrel and lose the offset. Correct for one
            // ray, collapses a pattern -- but the alternative is a pellet still on the camera.
            return result;
        }

        float redirected[3];
        WaRotateVector(spreadDelta, raw, redirected);
        const float lengthSq = redirected[0]*redirected[0] + redirected[1]*redirected[1] + redirected[2]*redirected[2];
        if (lengthSq < 1e-6f || !std::isfinite(lengthSq)) return result;
        const float invLength = 1.0f / std::sqrt(lengthSq);
        out[0] = redirected[0] * invLength;
        out[1] = redirected[1] * invLength;
        out[2] = redirected[2] * invLength;
        out[3] = 0.0f;
        CyberpunkVR_DebugPhysicalRayOutForward[0] = out[0];
        CyberpunkVR_DebugPhysicalRayOutForward[1] = out[1];
        CyberpunkVR_DebugPhysicalRayOutForward[2] = out[2];
        CyberpunkVR_DebugPhysicalRayOutForward[3] = out[3];
        WaCount(CyberpunkVR_DebugPhysicalRayForwardWrites);

        // The same effect, the same ray: what the impact consumers read is what was cast.
        if (CyberpunkVR_HitscanSharedRay && sharedContext && sharedContext == rdx) {
            if (!WaSharedHasPositionForward(sharedContext)) WaCount(CyberpunkVR_DebugPhysicalRaySharedMissing);
            else if (WaWriteSharedVector4(sharedContext, kWaPositionCName, finalOrigin, 1.0f) &&
                     WaWriteSharedVector4(sharedContext, kWaForwardCName, out, 0.0f))
                WaCount(CyberpunkVR_DebugPhysicalRayShared);
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
    return result;
}

inline bool WaInstallPhysicalRayPatchAt(uintptr_t siteRva, void* hook, uint8_t*& relayOut) {
    uint8_t* callsite = reinterpret_cast<uint8_t*>(g_waExeBase + siteRva);
    if (callsite[0] != 0xE8) return false;
    int32_t rel = 0;
    std::memcpy(&rel, callsite + 1, sizeof(rel));
    if (callsite + 5 + rel != reinterpret_cast<uint8_t*>(g_waExeBase + kWaPhysicalRayEvalFn)) return false;

    relayOut = reinterpret_cast<uint8_t*>(WaAllocRelayNear(callsite));
    if (!relayOut) return false;
    uint8_t relay[12] = { 0x48, 0xB8, 0,0,0,0,0,0,0,0, 0xFF, 0xE0 };
    const uint64_t destination = reinterpret_cast<uint64_t>(hook);
    std::memcpy(relay + 2, &destination, sizeof(destination));
    std::memcpy(relayOut, relay, sizeof(relay));
    FlushInstructionCache(GetCurrentProcess(), relayOut, sizeof(relay));

    const intptr_t delta = relayOut - (callsite + 5);
    if (delta < INT32_MIN || delta > INT32_MAX) return false;
    uint8_t patch[5] = { 0xE8,
        static_cast<uint8_t>(delta & 0xFF), static_cast<uint8_t>((delta >> 8) & 0xFF),
        static_cast<uint8_t>((delta >> 16) & 0xFF), static_cast<uint8_t>((delta >> 24) & 0xFF) };
    DWORD oldProtect = 0;
    if (!VirtualProtect(callsite, sizeof(patch), PAGE_EXECUTE_READWRITE, &oldProtect)) return false;
    std::memcpy(callsite, patch, sizeof(patch));
    FlushInstructionCache(GetCurrentProcess(), callsite, sizeof(patch));
    DWORD ignored = 0;
    VirtualProtect(callsite, sizeof(patch), oldProtect, &ignored);
    return true;
}

inline bool InstallWeaponPhysicalRayHook() {
    if (!g_waExeBase) return false;
    if (CyberpunkVR_DebugPhysicalRayPatched == 1) return true;
    s_waPhysicalRayEvalOrig = reinterpret_cast<WaPhysicalRayEvalFn>(g_waExeBase + kWaPhysicalRayEvalFn);
    if (!WaInstallPhysicalRayPatchAt(kWaPhysicalRayOriginCallsite,
            reinterpret_cast<void*>(&Hooked_WaPhysicalRayOrigin), s_waPhysicalRayOriginRelay)) {
        CyberpunkVR_DebugPhysicalRayPatched = -1;
        return false;
    }
    if (!WaInstallPhysicalRayPatchAt(kWaPhysicalRayForwardCallsite,
            reinterpret_cast<void*>(&Hooked_WaPhysicalRayForward), s_waPhysicalRayForwardRelay)) {
        CyberpunkVR_DebugPhysicalRayPatched = -2;
        return false;
    }
    CyberpunkVR_DebugPhysicalRayPatched = 1;
    return true;
}

// The camera pose bookkeeping asks for the copy hook itself (Hooks/CameraPoseTransport.cpp), so it
// must not depend on the rest of this family having installed.
bool EnsureCameraPoseCopyHook() {
    if(!g_waExeBase)g_waExeBase=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    if(!g_waExeBase)return false;
    const auto status=MH_Initialize();
    if(status!=MH_OK && status!=MH_ERROR_ALREADY_INITIALIZED)return false;
    void* target=reinterpret_cast<void*>(g_waExeBase+kWaPoseCopyOffset);
    if(OrigWaPoseCopy) {
        const auto enabled=MH_EnableHook(target);
        return enabled==MH_OK || enabled==MH_ERROR_ENABLED;
    }
    if(MH_CreateHook(target,&Hooked_WaPoseCopy,reinterpret_cast<void**>(&OrigWaPoseCopy))!=MH_OK)return false;
    return MH_EnableHook(target)==MH_OK;
}
namespace {
bool InstallWeaponAimAtBoot() { return InstallWeaponAimHooks(); }

// KILL SWITCH for the whole family, kept because it earned its place.
//
// 0.1.2 shipped a frame-rate collapse from 60-70 to about 10 near security cameras and turrets.
// It was isolated by setting this to 0: with the family off the drop is gone, with it on the drop
// is back, and neither deleting every CET and redscript mod nor dropping to mono changed
// anything -- so it was in the DLL, on a path indifferent to the second view. This hook installs
// AT BOOT (see the note below), which is why removing the scripts proved nothing.
//
// The cause was the check order in Hooked_WaPhysicalRayOrigin, fixed there. Leave this at 1.
// Set it to 0 and the log says so -- [hooks] boot WeaponAim skipped (disabled) -- at the cost of
// bullets from the muzzle and the hitscan hand recoil.
constexpr int kEnableWeaponAimFamily = 1;
bool WeaponAimWanted() { return kEnableWeaponAimFamily != 0; }
}
CVR_HOOK_IF("WeaponAim", ::cvr::hooks::Stage::Boot, 80, InstallWeaponAimAtBoot, WeaponAimWanted);

// INSTALLED AT BOOT, not on a script's say-so: the CET Weapon module's call three seconds after load
// was measured not to happen at all in one session. Its native (InstallWeaponAimHook) stays, and a
// second call is harmless: MinHook refuses a duplicate and this code only enables what it created.
bool InstallWeaponAimHooks() {
    HMODULE h = GetModuleHandleA("Cyberpunk2077.exe");
    if (!h) return false;
    g_waExeBase = reinterpret_cast<uintptr_t>(h);
    MH_Initialize(); // no-op if already initialized by the VRIK hooks

    bool ok = EnsureCameraPoseCopyHook();
    InstallWeaponPhysicalRayHook();   // reports itself: CyberpunkVR_DebugPhysicalRayPatched
    void* targeting = reinterpret_cast<void*>(g_waExeBase + kWaTargetingUpdateOffset);
    if (MH_CreateHook(targeting, &Hooked_WaTargetingUpdate, reinterpret_cast<void**>(&OrigWaTargetingUpdate)) == MH_OK)
        MH_EnableHook(targeting);

    CyberpunkVR_DebugWaInstalled = ok ? 1 : 0;
    return ok;
}
