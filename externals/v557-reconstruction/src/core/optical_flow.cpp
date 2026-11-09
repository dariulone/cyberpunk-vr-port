#include "xrfg/optical_flow.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <stdexcept>

namespace xrfg {
namespace {

struct AccumulatedPixel {
    float red{};
    float green{};
    float blue{};
    float alpha{};
    float weight{};
};

[[nodiscard]] std::size_t pixel_index(
    std::size_t x,
    std::size_t y,
    std::size_t width) noexcept {
    return y * width + x;
}

void validate_image_pair(
    std::span<const Rgba8> previous,
    std::span<const Rgba8> current,
    std::size_t width,
    std::size_t height) {
    if (width == 0 || height == 0 || width > std::numeric_limits<std::size_t>::max() / height) {
        throw std::invalid_argument("image dimensions must be non-zero and representable");
    }
    if (width > static_cast<std::size_t>(std::numeric_limits<int>::max()) ||
        height > static_cast<std::size_t>(std::numeric_limits<int>::max())) {
        throw std::invalid_argument("image dimensions exceed the CPU reference coordinate range");
    }
    const std::size_t expected_size = width * height;
    if (previous.size() != expected_size || current.size() != expected_size) {
        throw std::invalid_argument("image spans must match width times height");
    }
}

[[nodiscard]] float color_distance(const Rgba8& left, const Rgba8& right) noexcept {
    return static_cast<float>(
        std::abs(static_cast<int>(left.r) - static_cast<int>(right.r)) +
        std::abs(static_cast<int>(left.g) - static_cast<int>(right.g)) +
        std::abs(static_cast<int>(left.b) - static_cast<int>(right.b)));
}

[[nodiscard]] float patch_cost(
    std::span<const Rgba8> source,
    std::span<const Rgba8> target,
    std::size_t width,
    std::size_t height,
    int source_x,
    int source_y,
    int delta_x,
    int delta_y,
    int patch_radius) noexcept {
    float total = 0.0F;
    int samples = 0;
    int source_samples = 0;
    const int width_i = static_cast<int>(width);
    const int height_i = static_cast<int>(height);
    for (int patch_y = -patch_radius; patch_y <= patch_radius; ++patch_y) {
        const int from_y = source_y + patch_y;
        if (from_y < 0 || from_y >= height_i) {
            continue;
        }
        for (int patch_x = -patch_radius; patch_x <= patch_radius; ++patch_x) {
            const int from_x = source_x + patch_x;
            if (from_x < 0 || from_x >= width_i) {
                continue;
            }
            ++source_samples;
            const int to_x = from_x + delta_x;
            const int to_y = from_y + delta_y;
            if (to_x < 0 || to_x >= width_i || to_y < 0 || to_y >= height_i) {
                continue;
            }
            total += color_distance(
                source[pixel_index(
                    static_cast<std::size_t>(from_x),
                    static_cast<std::size_t>(from_y),
                    width)],
                target[pixel_index(
                    static_cast<std::size_t>(to_x),
                    static_cast<std::size_t>(to_y),
                    width)]);
            ++samples;
        }
    }

    if (samples == 0 || source_samples == 0) {
        return std::numeric_limits<float>::infinity();
    }

    const float average = total / static_cast<float>(samples);
    const float missing_sample_penalty =
        static_cast<float>(source_samples - samples) * 765.0F;
    const float displacement_tie_break =
        0.001F * static_cast<float>(delta_x * delta_x + delta_y * delta_y);
    return average + missing_sample_penalty + displacement_tie_break;
}

[[nodiscard]] std::vector<FlowVector> estimate_one_direction(
    std::span<const Rgba8> source,
    std::span<const Rgba8> target,
    std::size_t width,
    std::size_t height,
    const OpticalFlowSettings& settings) {
    constexpr std::uint32_t kMaximumReferenceSearchRadius = 512;
    constexpr std::uint32_t kMaximumReferencePatchRadius = 64;
    if (settings.search_radius > kMaximumReferenceSearchRadius ||
        settings.patch_radius > kMaximumReferencePatchRadius) {
        throw std::invalid_argument("optical-flow radii exceed the CPU reference limits");
    }
    const int search_radius = static_cast<int>(settings.search_radius);
    const int patch_radius = static_cast<int>(settings.patch_radius);
    std::vector<FlowVector> output(width * height);

    for (std::size_t y = 0; y < height; ++y) {
        for (std::size_t x = 0; x < width; ++x) {
            float best_cost = std::numeric_limits<float>::infinity();
            float second_cost = std::numeric_limits<float>::infinity();
            int best_x = 0;
            int best_y = 0;
            for (int delta_y = -search_radius; delta_y <= search_radius; ++delta_y) {
                const int target_y = static_cast<int>(y) + delta_y;
                if (target_y < 0 || target_y >= static_cast<int>(height)) {
                    continue;
                }
                for (int delta_x = -search_radius; delta_x <= search_radius; ++delta_x) {
                    const int target_x = static_cast<int>(x) + delta_x;
                    if (target_x < 0 || target_x >= static_cast<int>(width)) {
                        continue;
                    }
                    const float cost = patch_cost(
                        source,
                        target,
                        width,
                        height,
                        static_cast<int>(x),
                        static_cast<int>(y),
                        delta_x,
                        delta_y,
                        patch_radius);
                    if (cost < best_cost) {
                        second_cost = best_cost;
                        best_cost = cost;
                        best_x = delta_x;
                        best_y = delta_y;
                    } else if (cost < second_cost) {
                        second_cost = cost;
                    }
                }
            }

            float confidence = 0.0F;
            if (std::isfinite(best_cost) && std::isfinite(second_cost)) {
                confidence = std::clamp(
                    (second_cost - best_cost) / std::max(second_cost, 1.0F),
                    0.0F,
                    1.0F);
            } else if (std::isfinite(best_cost)) {
                confidence = 1.0F;
            }
            output[pixel_index(x, y, width)] = {
                static_cast<float>(best_x),
                static_cast<float>(best_y),
                confidence,
            };
        }
    }
    return output;
}

[[nodiscard]] float consistency_weight(
    const FlowVector& primary,
    std::span<const FlowVector> reverse,
    std::size_t x,
    std::size_t y,
    std::size_t width,
    std::size_t height,
    float threshold) noexcept {
    const long reverse_x = std::lround(static_cast<float>(x) + primary.horizontal);
    const long reverse_y = std::lround(static_cast<float>(y) + primary.vertical);
    if (reverse_x < 0 || reverse_y < 0 ||
        reverse_x >= static_cast<long>(width) || reverse_y >= static_cast<long>(height)) {
        return 0.05F;
    }

    const FlowVector& opposite = reverse[pixel_index(
        static_cast<std::size_t>(reverse_x),
        static_cast<std::size_t>(reverse_y),
        width)];
    const float residual_x = primary.horizontal + opposite.horizontal;
    const float residual_y = primary.vertical + opposite.vertical;
    const float residual = std::sqrt(residual_x * residual_x + residual_y * residual_y);
    const float safe_threshold = std::max(threshold, 0.01F);
    const float consistency = residual <= safe_threshold
                                  ? 1.0F
                                  : std::max(0.05F, safe_threshold / residual);
    const float match_quality = 0.25F + 0.75F * primary.confidence;
    return consistency * match_quality;
}

void splat(
    std::vector<AccumulatedPixel>& accumulation,
    std::size_t width,
    std::size_t height,
    float target_x,
    float target_y,
    const Rgba8& color,
    float source_weight) noexcept {
    const int left = static_cast<int>(std::floor(target_x));
    const int top = static_cast<int>(std::floor(target_y));
    const float fraction_x = target_x - static_cast<float>(left);
    const float fraction_y = target_y - static_cast<float>(top);
    const std::array<float, 4> weights{
        (1.0F - fraction_x) * (1.0F - fraction_y),
        fraction_x * (1.0F - fraction_y),
        (1.0F - fraction_x) * fraction_y,
        fraction_x * fraction_y,
    };
    const std::array<int, 4> offsets_x{0, 1, 0, 1};
    const std::array<int, 4> offsets_y{0, 0, 1, 1};
    for (std::size_t sample = 0; sample < weights.size(); ++sample) {
        const int x = left + offsets_x[sample];
        const int y = top + offsets_y[sample];
        if (x < 0 || y < 0 || x >= static_cast<int>(width) || y >= static_cast<int>(height)) {
            continue;
        }
        const float weight = weights[sample] * source_weight;
        AccumulatedPixel& output = accumulation[pixel_index(
            static_cast<std::size_t>(x),
            static_cast<std::size_t>(y),
            width)];
        output.red += static_cast<float>(color.r) * weight;
        output.green += static_cast<float>(color.g) * weight;
        output.blue += static_cast<float>(color.b) * weight;
        output.alpha += static_cast<float>(color.a) * weight;
        output.weight += weight;
    }
}

[[nodiscard]] std::uint8_t resolve_channel(float sum, float weight) noexcept {
    return static_cast<std::uint8_t>(std::clamp(std::lround(sum / weight), 0L, 255L));
}

}  // namespace

BidirectionalFlow estimate_bidirectional_flow_reference(
    std::span<const Rgba8> previous,
    std::span<const Rgba8> current,
    std::size_t width,
    std::size_t height,
    const OpticalFlowSettings& settings) {
    validate_image_pair(previous, current, width, height);
    if (!std::isfinite(settings.consistency_threshold) ||
        settings.consistency_threshold <= 0.0F) {
        throw std::invalid_argument("consistency threshold must be finite and positive");
    }

    BidirectionalFlow result{};
    result.width = width;
    result.height = height;
    result.previous_to_current = estimate_one_direction(
        previous,
        current,
        width,
        height,
        settings);
    result.current_to_previous = estimate_one_direction(
        current,
        previous,
        width,
        height,
        settings);
    return result;
}

std::vector<Rgba8> synthesize_bidirectional_reference(
    std::span<const Rgba8> previous,
    std::span<const Rgba8> current,
    const BidirectionalFlow& flow,
    float alpha,
    float consistency_threshold) {
    validate_image_pair(previous, current, flow.width, flow.height);
    const std::size_t pixel_count = flow.width * flow.height;
    if (flow.previous_to_current.size() != pixel_count ||
        flow.current_to_previous.size() != pixel_count) {
        throw std::invalid_argument("flow fields must match their declared dimensions");
    }
    if (!std::isfinite(consistency_threshold) || consistency_threshold <= 0.0F) {
        throw std::invalid_argument("consistency threshold must be finite and positive");
    }

    const float clamped_alpha = std::clamp(alpha, 0.0F, 1.0F);
    std::vector<AccumulatedPixel> accumulation(pixel_count);
    for (std::size_t y = 0; y < flow.height; ++y) {
        for (std::size_t x = 0; x < flow.width; ++x) {
            const std::size_t index = pixel_index(x, y, flow.width);
            const FlowVector& forward = flow.previous_to_current[index];
            const float forward_weight = consistency_weight(
                forward,
                flow.current_to_previous,
                x,
                y,
                flow.width,
                flow.height,
                consistency_threshold);
            splat(
                accumulation,
                flow.width,
                flow.height,
                static_cast<float>(x) + forward.horizontal * clamped_alpha,
                static_cast<float>(y) + forward.vertical * clamped_alpha,
                previous[index],
                forward_weight * (1.0F - clamped_alpha));

            const FlowVector& backward = flow.current_to_previous[index];
            const float backward_weight = consistency_weight(
                backward,
                flow.previous_to_current,
                x,
                y,
                flow.width,
                flow.height,
                consistency_threshold);
            splat(
                accumulation,
                flow.width,
                flow.height,
                static_cast<float>(x) + backward.horizontal * (1.0F - clamped_alpha),
                static_cast<float>(y) + backward.vertical * (1.0F - clamped_alpha),
                current[index],
                backward_weight * clamped_alpha);
        }
    }

    std::vector<Rgba8> output(pixel_count);
    for (std::size_t index = 0; index < pixel_count; ++index) {
        const AccumulatedPixel& value = accumulation[index];
        if (value.weight > 1.0e-6F) {
            output[index] = {
                resolve_channel(value.red, value.weight),
                resolve_channel(value.green, value.weight),
                resolve_channel(value.blue, value.weight),
                resolve_channel(value.alpha, value.weight),
            };
        } else {
            // Only unresolved holes use a same-coordinate fallback. It is not
            // the interpolation path for pixels covered by either flow field.
            output[index] = interpolate_pixel(previous[index], current[index], clamped_alpha);
        }
    }
    return output;
}

}  // namespace xrfg
