#pragma once

namespace cvr::blade {
// A temporary displacement of the virtual wrist servo target, never of raw tracking.
void ApplyPose(int side, float target[3]);
void RegisterPoseNative();
}
