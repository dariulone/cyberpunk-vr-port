#pragma once

// ================================================================================================
// ReloadPose: called from the pose-apply detour, each already guarded there.
// See src/Anim/ReloadPose.cpp for why the guards stayed behind.
// ================================================================================================

#include "Anim/VrikState.hpp"

#include <cstdint>

namespace cvr {
namespace anim {

// The resting left hand: captures it while unarmed, replays it while armed. Must run BEFORE
// VrikReloadFingerPose so a preview blends out of the resting fingers, not over them.
void VrikRestFingerPose(uint8_t* boneBuf);
// Disk half of the same feature, called from the frame loop: never from the pose path.
void RestFingerTick();
// The same, for the RIGHT hand: applied while the weapon is carried in the left one, where this hand is
// empty but the game still poses it around a grip.
void VrikRestFingerPoseRight(uint8_t* boneBuf);
void RestFingerRightTick();
// The right hand's GRIP fingers: learned from the hand itself every pass it holds a weapon, and
// offered back as a preview while the left hand carries it. Runs AFTER the resting pose, which it fades
// in on top of. No disk half -- there is nothing to record and nothing to load.
void VrikCarryGripPoseRight(uint8_t* boneBuf);
void VrikReloadFingerPose(uint8_t* boneBuf);

}  // namespace anim
}  // namespace cvr
