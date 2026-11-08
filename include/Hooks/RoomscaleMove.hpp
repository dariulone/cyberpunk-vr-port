#pragma once
#include "Runtimes/RoomscaleMovement.hpp"

namespace cvr::roomscale {
// Identity comes from GetPlayer in the existing camera-state refresh, never
// from proximity to the camera. No runtime addresses survive a player change.
void SetPlayer(uintptr_t player);
uintptr_t PlayerIdentity();
bool IsPlayerMovement(const void* state);
bool IsPlayerProvider(const void* provider);
void ObserveMovement(void* state);
bool GameplayAllowed();
bool PhysicalBodyEnabled(); // VRIK/scene ownership, independent of the pause menu
bool PhysicalBodyHeadingOwned(); // retains same-frame view cancellation during rig rebinding
bool PhysicalBodyAllowed();
bool PoseFrameAllowed();
Vec2 CameraConsumed(uint64_t origin);
Vec2 CameraConsumed();
// The player's neck at rest in the tracked residual's frame (Movement::RestNeck); false without a neck to give (no
// head sampled yet, another origin, 3DoF movement: the ledger walks the eyes then).
bool CameraRestNeck(uint64_t origin, Vec2* out);
bool CameraRestNeck(Vec2* out);
// The body is walking after the head this tick (the bend tracker reads a bob as a step then, Anim/BodyBend.hpp).
bool BodyFollowing();
// The visible body's dead zones (Runtimes/BodyDeadZones.hpp): the body less the neck, tracking metres; false when the
// mode is not on (zones off, not head-led body rotation, not strict roomscale, swimming, full-body tracking) or for
// another origin.
bool BodyDeadZoneOffset(Vec2* out);
// Strict roomscale only: the eye as the last capsule move saw it, in the tracked residual's frame (Movement::TickEye).
bool StrictTickEye(Vec2* out);
}
