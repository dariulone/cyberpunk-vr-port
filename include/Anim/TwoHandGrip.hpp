#pragma once

// ================================================================================================
// TwoHandGrip: the support hand on the weapon. See src/Anim/TwoHandGrip.cpp for what is captured
// and why the offset hangs off the right WRIST rather than off the weapon.
//
// Call order inside one pose apply, and it is load-bearing:
//   TwoHandCapture(boneBuf)                     -- only when asked, and only with VRIK off
//   TwoHandRight(targetR, hm, wristR, leftCtrl) -- in the right arm's branch, BEFORE handRot is built
//   TwoHandLeft(target, handRot)                 -- in the left arm's branch, before the stop zone
//   TwoHandFingers(boneBuf)                      -- after the resting pose, before the reload layer
// ================================================================================================

#include "Anim/VrikState.hpp"

#include <cstdint>

namespace cvr {
namespace anim {

void TwoHandCapture(uint8_t* boneBuf);
void TwoHandRight(const float* targetR, float* hm, const float* wristR, const float* leftCtrlModel);
bool TwoHandLeft(float* target, float* handRot);
// Selects the hold for the weapon currently in hand -- needed by the carry offset getter, which runs
// outside the pose pass.
void SelectHoldForCarry();
void TwoHandFingers(uint8_t* boneBuf);
// Disk half, called from the frame loop: never from the pose path.
void TwoHandTick();

}  // namespace anim
}  // namespace cvr
