#include "xrfg/image.hpp"

#include <algorithm>
#include <cmath>

namespace xrfg {
namespace {

[[nodiscard]] std::uint8_t interpolate_channel(std::uint8_t from, std::uint8_t to, float alpha) noexcept {
    const float value = static_cast<float>(from) +
                        (static_cast<float>(to) - static_cast<float>(from)) * alpha;
    return static_cast<std::uint8_t>(std::clamp(std::lround(value), 0L, 255L));
}

}  // namespace

Rgba8 interpolate_pixel(Rgba8 previous, Rgba8 current, float alpha) noexcept {
    const float clamped_alpha = std::clamp(alpha, 0.0F, 1.0F);
    return {
        interpolate_channel(previous.r, current.r, clamped_alpha),
        interpolate_channel(previous.g, current.g, clamped_alpha),
        interpolate_channel(previous.b, current.b, clamped_alpha),
        interpolate_channel(previous.a, current.a, clamped_alpha),
    };
}

}  // namespace xrfg
