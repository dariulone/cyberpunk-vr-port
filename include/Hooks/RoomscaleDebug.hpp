#pragma once
#include <cstddef>
#include <cstdint>
// POD snapshot for x64dbg / offline live probes (CyberpunkVR_RoomscaleDebug, src/Hooks/BodyHooksState.cpp). Read while
// paused, or bracket reads with the even, unchanged sequence. Diagnostics do not write game state.
struct RoomscaleDebug {
    uint64_t calls{}, injected{}, feedbackReads{}, resets{};
    uintptr_t player{}, movement{}, backend{};
    uint64_t poseSequence{}, origin{};
    uint32_t reason{}, gates{}; // native binding / CCT enabled / gameplay / tracking
    float dt{}, yaw{}, native[3]{}, requested[3]{}, before[3]{}, after[3]{}, velocity[3]{};
    float consumed[2]{}, feedback[2]{};
    float accepted[2]{}, lead[2]{}; // controller's share of the physical step; view lead over the capsule (m)
    float leadRadius{}, eyeBase[2]{};
    float reported[2]{};          // strict: the physical velocity the readers other than the solver see (m/s)
    uint32_t strict{}, walking{}; // strict mode on this move; PhysicalWalkGate's walk
};
static_assert(sizeof(RoomscaleDebug)==208 && offsetof(RoomscaleDebug,reason)==72 && offsetof(RoomscaleDebug,consumed)==148 &&
              offsetof(RoomscaleDebug,leadRadius)==180 && offsetof(RoomscaleDebug,strict)==200,
              "RoomscaleDebug is mirrored in rust/crates/cvr-rig/src/vrik/roomscale_move.rs");