#pragma once

// ================================================================================================
// The weapon-aim ABI: the globals this path shares with the natives, and the site offsets.
//
// Same story as Anim/VrikState.hpp -- these were interleaved through a 1,400-line header that
// carried its own implementation. They are the contract between src/Anim/WeaponAim.cpp and
// src/Natives/Natives.cpp, which DEFINES them.
//
// The kWa*Offset constants are engine RVAs. They are here rather than beside each detour because
// they are read by the install pass as a set, and a table of sites is easier to check against a game
// build when it is a table.
// ================================================================================================

#include "Anim/VrikState.hpp"
#include "Utils/SharedSlots.hpp"

#include <windows.h>
#include <atomic>
#include <cstdint>

inline constexpr uintptr_t kWaXhUpdOffset = 0x4B9D84;
inline constexpr uintptr_t kWaHeadOffset = 0x4E8A1C;
inline constexpr uintptr_t kWaProjOffset           = 0x28D4B8;
inline constexpr uintptr_t kWaTargetHelperOffset   = 0x46F774;
inline constexpr uintptr_t kWaTargetShotRet        = 0x46F017; // TargetHelper call from the player-shot wrapper

// gameEffectObjectProvider_PhysicalRay::Execute -- the native hitscan ray. It evaluates its origin
// and its direction through one shared effect-input evaluator, and the port replaces those two exact
// calls. See docs/cp2077-hitscan-physicalray-muzzle.md for how each address was established.
inline constexpr uintptr_t kWaPhysicalRayEvalFn          = 0x1203B0;
inline constexpr uintptr_t kWaPhysicalRayOriginCallsite  = 0x84E31F;
inline constexpr uintptr_t kWaPhysicalRayForwardCallsite = 0x84E354;

// Published by the orientation-provider side: the frame counter the muzzle is published on, and the
// latch that keeps one round to one hand kick across both shot paths.
extern volatile uint32_t g_provMuzzleSeq;
extern volatile long     g_provRecoilSeqSeen;

// The aim line's hit point and surface normal, published once a frame by the weapon module from the
// game's SpatialQueriesSystem. Read by the overlay's barrel dot; see SetVRAimHit.
extern volatile float    g_provAimHit[3];
extern volatile float    g_provAimNormal[3];
extern volatile uint32_t g_provAimHitSeq;
extern volatile uint64_t g_provAimHitStampMs;
inline constexpr uintptr_t kWaShotClassifyOffset   = 0x291FDE0;
inline constexpr uintptr_t kWaClassifyRetProc      = 0x292279E; // return into ShotVectorProcessor
inline constexpr uintptr_t kWaClassifyRetAltProc   = 0x2923292;
inline constexpr uintptr_t kWaCandAOffset = 0x291D9C8;
inline constexpr uintptr_t kWaCandBOffset = 0x291DD54;
inline constexpr uintptr_t kWaSVPOffset   = 0x292263C; // ShotVectorProcessor
inline constexpr uintptr_t kWaSFVWOffset  = 0x29216D0; // ShotFinalVectorWrite
inline constexpr uintptr_t kWaPhysCallsite = 0x46F1EA;
inline constexpr uint8_t   kWaExpectedCall[7] = { 0x41, 0xFF, 0x92, 0x50, 0x01, 0x00, 0x00 };
inline constexpr int kWaMaxSaved = 256;
inline constexpr uintptr_t kWaNormCallsite = 0x46F0E5;
inline constexpr uintptr_t kWaFireNormCallsite = 0x84C968;
inline constexpr uintptr_t kWaNormalizeFn  = 0x13DE80;
inline constexpr uintptr_t kSsOffset = 0x79ACA0;
inline constexpr uintptr_t kGoOffset = 0x802390;
inline constexpr uintptr_t kXfOffset = 0x1D92A0;
inline constexpr uintptr_t kFireOffset = 0x4E4AFC;
inline constexpr uintptr_t kTraceOffset = 0x1303EC;

// ---- the shared globals ----
extern volatile uintptr_t g_exeBaseTrace;
extern volatile uint32_t  g_traceRvas[128];
extern volatile uint32_t  g_traceRvaCounts[128];
extern volatile int       g_traceCount;
extern volatile uint64_t  g_traceHits;
extern volatile uintptr_t g_traceAddr;   // watched address
extern volatile int       g_traceActive;
extern volatile int       g_traceGated;  // 1 = only record while g_shotInProgress
extern volatile int       g_shotInProgress;
extern volatile int g_traceWriteOnly;  // 1 = DR0 watches WRITE only, 0 = read/write
extern volatile uint64_t g_waProjCalls;        // projectile copy hook invocations
extern volatile uint64_t g_waProjMutated;      // projectile shoot events we redirected
extern volatile int      g_waProjCtrl;         // 1 = redirect startVelocity to controller fwd (g_fireInShot-gated)
extern volatile int      g_waProjNeg;          // flip sign
extern volatile int      g_waProjUnguide;      // 1 = clear smartGunIsProjectileGuided (defeat homing)
extern volatile float    g_waProjRange;        // targetPosition distance along controller (def 1000)
extern volatile int      g_waProjAlways;       // 1 = bypass the g_fireInShot gate
extern volatile int      g_waProjOriginRow;    // localToWorld row (0..3) used as the world muzzle origin
extern volatile uint32_t g_waProjLastRetRva;   // caller return RVA for the last copy call
extern volatile uint32_t g_waProjRejectReason; // bitmask: 1 ctrlOff, 2 gate, 4 nullRcx, 8 noShared, 16 targetLocal, 32 badCtrl
extern volatile uint32_t g_waProjGateRva;      // only mutate this copy caller; 0 = all callers
extern volatile uint64_t g_waProjRet36F9FF;    // queue/state copy spam path
extern volatile uint64_t g_waProjRet36FD7C;    // manager insert/update path
extern volatile uint64_t g_waProjRet4E5109;    // active projectile consumer path
extern volatile uint64_t g_waProjRet4E615F;    // alternate projectile consumer path
extern volatile float    g_shotOrigin[3];      // player/camera WORLD position (pumped from CET)
extern volatile float    g_projDump[64];       // diag: l2w(0-23) tgtPre(24-26) guided(27) origin(28-30) ctrlDir(31-33) svPost(34-36) tgtPost(37-39) reason/control(40+)
extern volatile uint64_t g_waTargetCalls;      // TargetHelper hook invocations (all callers)
extern volatile uint64_t g_waTargetFromShot;   // ...called from the shot wrapper (ret 0x46F017)
extern volatile uint64_t g_waClassifyCalls;    // classify hook invocations (all)
extern volatile uint64_t g_waClassifyFromShot; // ...called from the shot-vector processor
extern volatile uint64_t g_waRedirects;        // shot directions we actually rewrote
extern volatile int      g_waInstalled;
extern volatile float    g_waTargetOrigin[4];  // last shot origin (source+0x50)
extern volatile float    g_waTargetDir[4];     // last original aim delta (target-origin)
extern volatile float    g_xfTestYaw;          // shared sanity-yaw (also used by TargetHelper test)
extern volatile uint64_t g_shotTick;           // tick of last shot-frame flag set
extern volatile int      g_xfTestPlane;        // 0=around Z(yaw) 1=around Y 2=around X
extern volatile uint32_t g_waLastRetRva;       // last TargetHelper caller return rva (diag)
extern volatile int      g_shotInProgress;
extern volatile int      g_fireInShot;         // nest-safe shot-window counter (trace gate)
extern volatile uintptr_t g_waExeBase;         // Cyberpunk2077.exe base
extern volatile int      g_waTgtCtrl;  // 1 = redirect TargetHelper target to controller forward
extern volatile int      g_waTgtNeg;   // flip sign
extern volatile uint64_t g_waTgtOvr;   // override count (confirms it applied)
extern volatile int      g_waEnable;   // 0 = instrument only; 1 = redirect to g_waFwd
extern volatile int      g_waMode;     // bit0: also move shot origin to g_waPos (muzzle)
extern volatile float    g_waFwd[3];   // unit barrel forward, WORLD space
extern volatile float    g_waPos[3];   // weapon/muzzle WORLD position
extern volatile float    g_waGateDist; // (unused by classify; kept for compat)
extern volatile uint32_t g_waFwdSeq;
extern volatile uint64_t g_waXhCalls;
extern volatile uint64_t g_waXhMutated;
extern volatile int      g_waXhSnapped;
extern volatile float    g_waXhPos[4];   // captured cache+0x350 (pre-write)
extern volatile float    g_waXhDir[4];   // captured cache+0x370 (pre-write)
extern volatile float    g_provMuzzlePos[3];
extern volatile uint32_t g_provMuzzlePosSeq;
extern volatile int      g_waXhPosFromMuzzle;
extern volatile uint64_t  g_waHeadCalls;
extern volatile uintptr_t g_waHeadObj;     // captured camObj (this)
extern volatile int       g_waHeadForce;   // 1 = force flags + write offset
extern volatile float     g_waHeadYaw;     // heading yaw offset to write @+0x4E4
extern volatile float     g_waHeadPitch;   // heading pitch offset to write @+0x4E8
extern volatile float     g_waHeadOrig4E4; // captured original values (diag)
extern volatile float     g_waHeadOrig4E8;
extern volatile float     g_waHeadVal4B8;
extern volatile int       g_waHeadFlag474;
extern volatile uint64_t g_waCandA, g_waCandB, g_waSVP, g_waSFVW;
extern volatile uint64_t g_waPhysCalls;
extern volatile uint64_t g_waPhysMutated;
extern volatile int      g_waPhysPatched;
extern volatile int      g_waDbgSnapped;
extern volatile float    g_waDbgArg3[72];  // arg3 (r8) first 0x120 bytes as floats
extern volatile float    g_waDbgRay[40];   // rayList (r9) first 0xA0 bytes as floats
extern volatile float    g_waDbgRayEntry[28]; // first ray-list entry (deref *rayList) 0x70 bytes
extern volatile uint64_t g_waNormShot;
extern volatile uint64_t g_waNormMutated;
extern volatile int      g_waNormPatched;
extern volatile uint64_t g_waFireNormShot;
extern volatile uint64_t g_waFireNormMutated;
extern volatile int      g_waFireNormPatched;
extern volatile uint64_t  g_ssCalls;
extern volatile uint64_t  g_ssSnapped;
extern volatile uintptr_t g_ssCamPtr;     // FPP camera obj (set by CET SetVRShotCamera)
extern volatile int       g_ssEnable;
extern volatile int       g_ssMode;       // bracket offset selector: 0=+0xF0(world) 1=+0xD0(local) 2=+0x110
extern volatile float     g_ssTestYaw;    // static yaw (rad) sanity test; 0 = use controller
extern volatile float     g_ssCamQuat[4]; // last read bracket-offset quat (diag)
extern volatile float     g_ssDiagD0[4];  // diag: cam+0xD0 (local orient)
extern volatile float     g_ssDiagF0[4];  // diag: cam+0xF0 (world orient)
extern volatile float     g_ssDiag110[4]; // diag: cam+0x110
extern volatile uint64_t g_goCalls;
extern volatile uint64_t g_goMutated;
extern volatile int      g_goMode;
extern volatile float    g_goTestYaw;
extern volatile int      g_goPlane;
extern volatile float    g_goLastQuat[4];
extern volatile uint64_t g_xfCalls;
extern volatile uint64_t g_xfMutated;
extern volatile int      g_xfMode;          // 0 off, 1 always, 2 gated-by-shot
extern volatile float    g_xfTestYaw;       // !=0 -> rotate output orient by this (sanity); 0 -> controller quat
extern volatile int      g_shotInProgress;
extern volatile float    g_xfLastOut[4];    // diag: output orient we saw
extern volatile uint64_t g_fireCalls;
extern volatile uint64_t g_fireMutated;
extern volatile int      g_fireMode;      // 0 scan-only, 1 bend-test, 2 controller override
extern volatile int      g_firePlane;     // bend axis: 0=Z(yaw) 1=Y 2=X
extern volatile float    g_fireTestAng;   // bend angle (radians)
extern volatile int      g_fireNeg;       // mode2: 0 = write +fwd, 1 = write -fwd
extern volatile float    g_fireDir[4];    // captured override-target (pre-write) for UI
extern volatile float    g_fireDirOut[4]; // what we wrote (post)
extern volatile int      g_fireScanSrc;   // 0=r8  1=rdx  2=*(rdx+0x10)  3=*(rdx)
extern volatile int      g_fireScanRange; // bytes to scan from the source base
extern volatile int      g_fireOvrSrc;    // override source (same enum)
extern volatile int      g_fireOvrOff;    // override byte offset
extern volatile int      g_fireXform;     // transform-quat override: 0 off,1 +F0,2 +D0,3 both
extern volatile int      g_fireXformOff;  // world-orient quat offset (default 0xF0)
extern volatile int      g_fireCamSnap;   // 1 = snap FPP cam orientation to controller during the shot
extern volatile int      g_fireCamSnapOff;// cam quat offset (default 0xF0 world; try 0xD0 local)
extern volatile int      g_fireHitCount;
extern volatile int      g_fireHitOff[24];
extern volatile float    g_fireHitVec[24*3];
extern volatile float    g_fireHitDot[24];
extern volatile uint64_t g_trShotCalls;       // dispatcher calls during the shot window
extern volatile int      g_trRetCount;        // distinct return RVAs captured
extern volatile uint32_t g_trRetRing[16];     // their RVAs
extern volatile uint32_t g_trCallerRay[16*12];// per-caller ray-struct (arg5) dump (12 dwords each)
extern volatile float    g_trCallerDir[16*4]; // per-caller arg3 (r8) = the DIRECTION vector dump
extern volatile uint32_t g_trCallerHits[16];  // how many times each caller fired this shot
extern volatile int      g_trOverride;        // 0 off, 1 override (ONLY the gated caller)
extern volatile uint32_t g_trGateRet;         // REQUIRED non-zero ret-RVA to override (no override-all)
extern volatile int      g_trWriteOff;        // byte offset in the ray struct (arg5) to write the unit dir
extern volatile int      g_trForce;           // 1 = write even if current value isn't a unit vector
extern volatile int      g_trNeg;
extern volatile uint64_t g_trOvrCount;
extern volatile int      g_fireInShot;
