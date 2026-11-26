#pragma once

// ================================================================================================
// The weapon-aim ABI: the site offsets of Hooks/WeaponAim.cpp, and the muzzle and aim state it shares
// with the orientation provider (Natives/OrientationProvider.cpp defines the g_prov* globals;
// g_waExeBase lives in Natives/NativeState.cpp).
//
// The kWa*Offset constants are engine RVAs. They are here rather than beside each detour because a
// table of sites is easier to check against a game build when it is a table.
//
// The ~150 experiment globals that used to sit here (TargetHelper, classifier, trace funnel,
// orientation getters, heading offset, tracer, projectile redirect) went with their hooks on
// 2026-10-17 -- see the header of Hooks/WeaponAim.cpp.
// ================================================================================================

#include "Anim/VrikState.hpp"
#include "Utils/SharedSlots.hpp"

#include <windows.h>
#include <atomic>
#include <cstdint>

inline constexpr uintptr_t kWaTargetingUpdateOffset = 0x19C75C;
inline constexpr uintptr_t kWaPoseCopyOffset        = 0x21297C;

// gameEffectObjectProvider_PhysicalRay::Execute -- the native hitscan ray. It evaluates its origin
// and its direction through one shared effect-input evaluator, and the port replaces those two exact
// calls. See docs/cp2077-hitscan-physicalray-muzzle.md for how each address was established.
inline constexpr uintptr_t kWaPhysicalRayEvalFn          = 0x474BCC;
inline constexpr uintptr_t kWaPhysicalRayOriginCallsite  = 0x9507B7;
inline constexpr uintptr_t kWaPhysicalRayForwardCallsite = 0x9507EC;

// Published by the orientation-provider side: the frame counter the muzzle is published on, and the
// latch that keeps one round to one hand kick across both shot paths.
extern "C" volatile uint32_t g_provMuzzleSeq;       // C linkage: rust/crates/cvr-rig/src/vrik/weapon_aim.rs
extern "C" volatile long     g_provRecoilSeqSeen;

// The aim line's hit point and surface normal, published once a frame by the weapon module from the
// game's SpatialQueriesSystem. Read by the overlay's barrel dot; see SetVRAimHit.
extern volatile float    g_provAimHit[3];
extern volatile float    g_provAimNormal[3];
extern volatile uint32_t g_provAimHitSeq;
extern volatile uint64_t g_provAimHitStampMs;

// The muzzle's world position (SetVRMuzzlePos).
extern volatile float    g_provMuzzlePos[3];
extern volatile uint32_t g_provMuzzlePosSeq;

extern volatile uintptr_t g_waExeBase;         // Cyberpunk2077.exe base
