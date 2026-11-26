#pragma once
#include <cstdint>

namespace cvr::anim {
struct RifleElbowPose {
    uint64_t poseId{};
    int mode{};
    float joints[9]{};  // final model-space shoulder, elbow and wrist, in metres
};
RifleElbowPose FindRifleElbowPose(uint64_t poseId);
}
