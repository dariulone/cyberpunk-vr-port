#pragma once

#include <cstdint>
#include <optional>

namespace xrfg {

struct Vec3 {
    float x{};
    float y{};
    float z{};
};

struct Quaternion {
    float x{};
    float y{};
    float z{};
    float w{1.0F};
};

struct Pose {
    Quaternion orientation{};
    Vec3 position{};
};

struct TimedPose {
    std::int64_t time_ns{};
    Pose pose{};
};

[[nodiscard]] Quaternion normalize(Quaternion value) noexcept;
[[nodiscard]] Quaternion slerp_shortest(Quaternion from, Quaternion to, float alpha) noexcept;
[[nodiscard]] Pose interpolate_pose(const Pose& from, const Pose& to, float alpha) noexcept;
[[nodiscard]] std::optional<TimedPose> midpoint(const TimedPose& previous, const TimedPose& current) noexcept;

}  // namespace xrfg
