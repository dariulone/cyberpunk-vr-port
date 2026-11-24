#pragma once
namespace cvr::anim {
// On every skeleton's pose apply, after the engine's own. Costs a compare unless a person's arm is held.
void NpcArmGripApply(void* poseArgument);
}
