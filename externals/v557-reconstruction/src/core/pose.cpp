#include "xrfg/pose.hpp"

#include <algorithm>
#include <cmath>

namespace xrfg {
namespace {

[[nodiscard]] float dot(const Quaternion& left, const Quaternion& right) noexcept {
    return left.x * right.x + left.y * right.y + left.z * right.z + left.w * right.w;
}

[[nodiscard]] Quaternion scale_add(
    const Quaternion& left,
    float left_scale,
    const Quaternion& right,
    float right_scale) noexcept {
    return {
        left.x * left_scale + right.x * right_scale,
        left.y * left_scale + right.y * right_scale,
        left.z * left_scale + right.z * right_scale,
        left.w * left_scale + right.w * right_scale,
    };
}

}  // namespace

Quaternion normalize(Quaternion value) noexcept {
    const float length_squared = dot(value, value);
    if (!(length_squared > 0.0F) || !std::isfinite(length_squared)) {
        return {};
    }

    const float inverse_length = 1.0F / std::sqrt(length_squared);
    return {
        value.x * inverse_length,
        value.y * inverse_length,
        value.z * inverse_length,
        value.w * inverse_length,
    };
}

Quaternion slerp_shortest(Quaternion from, Quaternion to, float alpha) noexcept {
    const float clamped_alpha = std::clamp(alpha, 0.0F, 1.0F);
    from = normalize(from);
    to = normalize(to);

    float cosine = dot(from, to);
    if (cosine < 0.0F) {
        to = {-to.x, -to.y, -to.z, -to.w};
        cosine = -cosine;
    }

    cosine = std::clamp(cosine, -1.0F, 1.0F);
    if (cosine > 0.9995F) {
        return normalize(scale_add(from, 1.0F - clamped_alpha, to, clamped_alpha));
    }

    const float angle = std::acos(cosine);
    const float sine = std::sin(angle);
    if (!(std::abs(sine) > 1.0e-7F)) {
        return from;
    }

    const float from_scale = std::sin((1.0F - clamped_alpha) * angle) / sine;
    const float to_scale = std::sin(clamped_alpha * angle) / sine;
    return normalize(scale_add(from, from_scale, to, to_scale));
}

Pose interpolate_pose(const Pose& from, const Pose& to, float alpha) noexcept {
    const float clamped_alpha = std::clamp(alpha, 0.0F, 1.0F);
    return {
        slerp_shortest(from.orientation, to.orientation, clamped_alpha),
        {
            from.position.x + (to.position.x - from.position.x) * clamped_alpha,
            from.position.y + (to.position.y - from.position.y) * clamped_alpha,
            from.position.z + (to.position.z - from.position.z) * clamped_alpha,
        },
    };
}

std::optional<TimedPose> midpoint(const TimedPose& previous, const TimedPose& current) noexcept {
    if (current.time_ns <= previous.time_ns) {
        return std::nullopt;
    }

    const std::int64_t delta = current.time_ns - previous.time_ns;
    return TimedPose{
        previous.time_ns + delta / 2,
        interpolate_pose(previous.pose, current.pose, 0.5F),
    };
}

}  // namespace xrfg
