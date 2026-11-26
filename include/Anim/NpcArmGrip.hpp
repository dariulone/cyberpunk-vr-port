#pragma once
namespace cvr::anim {
// Called after the AnimatedObject's complete native graph chain has finished.
void NpcArmGripObjectUpdated(void* animatedObject);
// Alternative completion path, immediately before in-place local-to-model conversion.
void NpcArmGripPoseCompleted(void* descriptor,bool beforeModelConversion);
}
