#pragma once

#include <cstddef>
#include <cstdint>
namespace xrfg {

struct Rgba8 {
    std::uint8_t r{};
    std::uint8_t g{};
    std::uint8_t b{};
    std::uint8_t a{255};
};

[[nodiscard]] Rgba8 interpolate_pixel(Rgba8 previous, Rgba8 current, float alpha) noexcept;

}  // namespace xrfg
