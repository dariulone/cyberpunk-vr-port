#pragma once

#include "xrfg/image.hpp"

#include <cstddef>
#include <cstdint>
#include <span>
#include <vector>

namespace xrfg {

struct FlowVector {
    float horizontal{};
    float vertical{};
    float confidence{};
};

struct OpticalFlowSettings {
    std::uint32_t search_radius{8};
    std::uint32_t patch_radius{2};
    float consistency_threshold{1.5F};
};

struct BidirectionalFlow {
    std::size_t width{};
    std::size_t height{};
    std::vector<FlowVector> previous_to_current;
    std::vector<FlowVector> current_to_previous;
};

// Slow deterministic CPU block matching for offline validation. This is not a
// production VR backend and does not make any GPU or real-time guarantees.
[[nodiscard]] BidirectionalFlow estimate_bidirectional_flow_reference(
    std::span<const Rgba8> previous,
    std::span<const Rgba8> current,
    std::size_t width,
    std::size_t height,
    const OpticalFlowSettings& settings = {});

// Forward-warps both input images toward alpha, rejects inconsistent flow, and
// fills any remaining holes. Alpha 0.5 produces the temporal midpoint.
[[nodiscard]] std::vector<Rgba8> synthesize_bidirectional_reference(
    std::span<const Rgba8> previous,
    std::span<const Rgba8> current,
    const BidirectionalFlow& flow,
    float alpha = 0.5F,
    float consistency_threshold = 1.5F);

}  // namespace xrfg
