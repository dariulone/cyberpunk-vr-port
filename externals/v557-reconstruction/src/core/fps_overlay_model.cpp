#include "xrfg/fps_overlay_model.hpp"

#include <algorithm>
#include <cmath>
#include <cstdio>

namespace xrfg {

FpsOverlayPosition parse_overlay_position(std::string_view value) noexcept {
    if (value == "off") return FpsOverlayPosition::off;
    if (value == "upper_left") return FpsOverlayPosition::upper_left;
    if (value == "lower_left") return FpsOverlayPosition::lower_left;
    if (value == "lower_right") return FpsOverlayPosition::lower_right;
    return FpsOverlayPosition::upper_right;
}

const char* overlay_position_name(FpsOverlayPosition position) noexcept {
    switch (position) {
    case FpsOverlayPosition::off: return "off";
    case FpsOverlayPosition::upper_left: return "upper_left";
    case FpsOverlayPosition::lower_left: return "lower_left";
    case FpsOverlayPosition::lower_right: return "lower_right";
    default: return "upper_right";
    }
}

FpsOverlaySize parse_overlay_size(std::string_view value) noexcept {
    if (value == "medium") return FpsOverlaySize::medium;
    if (value == "small") return FpsOverlaySize::little;
    return FpsOverlaySize::big;
}

const char* overlay_size_name(FpsOverlaySize size) noexcept {
    switch (size) {
    case FpsOverlaySize::medium: return "medium";
    case FpsOverlaySize::little: return "small";
    default: return "big";
    }
}

FpsCounter::Bucket& FpsCounter::bucket(std::int64_t now) noexcept {
    now = std::max<std::int64_t>(now, 0);
    if (start_ns_ < 0) start_ns_ = now;
    const auto epoch = now / 100'000'000;
    auto& entry = buckets_[static_cast<std::size_t>(epoch) % buckets_.size()];
    if (entry.epoch != epoch) entry = Bucket{epoch};
    return entry;
}

void FpsCounter::submitted(std::int64_t now, bool synthetic, bool new_content) noexcept {
    auto& entry = bucket(now);
    // A repeat is a submission and not a frame. See the header for what
    // counting them cost.
    if (new_content) ++entry.output;
    ++entry.submissions;
    if (synthetic) last_synthetic_ns_ = now;
}

FpsSnapshot FpsCounter::snapshot(std::int64_t now) const noexcept {
    FpsSnapshot result;
    if (start_ns_ < 0 || now < start_ns_) return result;
    const auto epoch = now / 100'000'000;
    const auto window_start = std::max(start_ns_, (epoch - 9) * 100'000'000);
    const float seconds = std::max(0.1f, static_cast<float>(now - window_start) * 1e-9f);
    std::uint32_t output = 0, submissions = 0;
    for (const auto& entry : buckets_) {
        if (entry.epoch < 0 || entry.epoch > epoch || entry.epoch < epoch - 9) continue;
        output += entry.output;
        submissions += entry.submissions;
    }
    result.submitted_fps = static_cast<float>(output) / seconds;
    if (submissions != 0)
        result.new_content_share = static_cast<float>(output) / static_cast<float>(submissions);
    // Hysteresis across alternating S/B slots; no green merely because armed,
    // initialized, primed, repeated, or because an attempted S failed.
    result.active = last_synthetic_ns_ >= 0 && now >= last_synthetic_ns_ &&
        now - last_synthetic_ns_ < 500'000'000;
    return result;
}

float delivered_new_images(float delivered, const FpsSnapshot& snapshot) noexcept {
    return delivered * std::clamp(snapshot.new_content_share, 0.0f, 1.0f);
}

float displayed_rate(float rate, float refresh_hz) noexcept {
    if (!(refresh_hz > 0.0f) || !std::isfinite(rate)) return rate;
    const float band = refresh_hz * 0.02f;
    return rate >= refresh_hz - band && rate <= refresh_hz + band ? refresh_hz : rate;
}

OverlayPlacement overlay_placement(
    FpsOverlayPosition position, float left, float right, float down, float up) noexcept {
    left = std::clamp(left, -1.0f, -0.1f);
    right = std::clamp(right, 0.1f, 1.0f);
    down = std::clamp(down, -1.0f, -0.1f);
    up = std::clamp(up, 0.1f, 1.0f);
    const float span_x = right - left, span_y = up - down;
    const float anchor_width = std::min(span_x * 0.06f, span_y * 0.15f);
    const float anchor_height = anchor_width / 2.0f;
    constexpr float size_scale = 0.80f;
    const float width = anchor_width * size_scale;
    const float height = anchor_height * size_scale;
    // Keep the small number comfortably inside the headset's visible area.
    // Mirror the same inset for all corners. Anchor with the V065 footprint so
    // reducing the visible size does not move the counter centre back outward.
    constexpr float horizontal_inset = 0.25f;
    constexpr float vertical_inset = 0.29f;
    const bool on_left = position == FpsOverlayPosition::upper_left ||
        position == FpsOverlayPosition::lower_left;
    const bool on_top = position == FpsOverlayPosition::upper_left ||
        position == FpsOverlayPosition::upper_right;
    return {2.0f * (on_left ? left + span_x * horizontal_inset + anchor_width * 0.5f
                           : right - span_x * horizontal_inset - anchor_width * 0.5f),
            2.0f * (on_top ? up - span_y * vertical_inset - anchor_height * 0.5f
                          : down + span_y * vertical_inset + anchor_height * 0.5f),
            -2.0f, width * 2.0f, height * 2.0f};
}

std::uint32_t overlay_texture_width(std::uint32_t eye_width) noexcept {
    return std::clamp((eye_width / 16 + 23) / 24 * 24, 96u, 336u);
}

namespace {
// Original 5x7 bitmap glyphs, rows from top to bottom. No font dependency.
std::array<unsigned, 7> glyph(char c) noexcept {
    switch (c) {
    case '0': return {14,17,19,21,25,17,14};
    case '1': return {4,12,4,4,4,4,14};
    case '2': return {14,17,1,2,4,8,31};
    case '3': return {30,1,1,14,1,1,30};
    case '4': return {2,6,10,18,31,2,2};
    case '5': return {31,16,16,30,1,1,30};
    case '6': return {14,16,16,30,17,17,14};
    case '7': return {31,1,2,4,8,8,8};
    case '8': return {14,17,17,14,17,17,14};
    case '9': return {14,17,17,15,1,1,14};
    case 'p': return {27,27,27,27,27,27,27}; // pause: two vertical bars
    case 'H': return {17,17,17,31,17,17,17};
    case 'Y': return {17,17,10,4,4,4,4};
    case 'B': return {30,17,17,30,17,17,30};
    case 'R': return {30,17,17,30,20,18,17};
    case 'I': return {14,4,4,4,4,4,14};
    case 'D': return {28,18,17,17,17,18,28};
    case 'O': return {14,17,17,17,17,17,14};
    case 'K': return {17,18,20,24,20,18,17};
    case '(': return {2,4,8,8,8,4,2};
    case ')': return {8,4,2,2,2,4,8};
    case 'm': return {0,0,26,21,21,21,21};
    case 's': return {0,0,15,16,14,1,30};
    case 'X': return {17,17,10,4,10,17,17};
    default: return {};
    }
}
}

std::vector<std::uint32_t> rasterize_fps_overlay(
    std::uint32_t width, std::uint32_t height, const FpsSnapshot& snapshot, bool bgra,
    FpsOverlaySize size) {
    if (width < 24 || width > 2048 || height < 12 || height > 1024) return {};
    const auto color = [bgra](unsigned r, unsigned g, unsigned b) {
        return 0xff000000u | (bgra ? b | (g << 8) | (r << 16)
                                          : r | (g << 8) | (b << 16));
    };
    // Premultiplied alpha: transparent black outside opaque digit strokes.
    std::vector<std::uint32_t> pixels(static_cast<std::size_t>(width) * height, 0);
    const auto status = snapshot.active ? color(64, 235, 112) : color(255, 80, 80);
    const unsigned scale = std::min(width / 24, height / 12);
    const float rate = std::isfinite(snapshot.submitted_fps) ? snapshot.submitted_fps : 0.0f;
    // Four glyphs at most, 23 columns of the 24: the pause symbol and three
    // digits.
    char text[5]{};
    const auto length = static_cast<unsigned>(std::snprintf(text, sizeof(text), "%s%d",
        snapshot.paused ? "p" : "",
        static_cast<int>(std::clamp(rate, 0.0f, 999.0f) + 0.5f)));
    // The tray's pause symbol is the same orange.
    const auto pause = color(255, 150, 0);
    // One centred line of glyphs, each glyph pixel `cell` texture pixels on a
    // side; fractional below big, where each texture pixel takes the glyph
    // pixel under its centre. Returns the row below the line. `ink`, when
    // non-zero, colours the whole line.
    const auto draw = [&](const char* line, unsigned count, float cell, unsigned top,
                          std::uint32_t ink = 0) {
        const auto line_width = static_cast<unsigned>(std::ceil((count * 6 - 1) * cell));
        const auto line_height = static_cast<unsigned>(std::ceil(7 * cell));
        const unsigned left = (width - line_width) / 2;
        for (unsigned py = top; py < top + line_height && py < height; ++py) {
            const auto y = static_cast<unsigned>((py - top + 0.5f) / cell);
            if (y >= 7) continue;
            for (unsigned px = left; px < left + line_width && px < width; ++px) {
                const auto cell_column = static_cast<unsigned>((px - left + 0.5f) / cell);
                const unsigned index = cell_column / 6, column = cell_column % 6;
                if (index >= count || column == 5) continue;
                if ((glyph(line[index])[y] & (1u << (4 - column))) == 0) continue;
                pixels[static_cast<std::size_t>(py) * width + px] =
                    ink != 0 ? ink : line[index] == 'p' ? pause : status;
            }
        }
        return top + line_height;
    };
    const float cell = std::max(1.0f, static_cast<float>(scale) *
        (size == FpsOverlaySize::little ? 1.0f / 3.0f
         : size == FpsOverlaySize::medium ? 0.5f : 1.0f));
    // The number keeps the 12-row grid's centre in a taller image too: rows
    // 0-12 of a 16-row image, rows 4-16 of a 20-row one.
    const unsigned above = height >= 20 * scale ? 4 * scale : 0;
    const unsigned band = height >= 16 * scale ? 12 * scale : height;
    const auto text_height = static_cast<unsigned>(std::ceil(7 * cell));
    const unsigned number_top = above + (band - text_height) / 2;
    const unsigned below = draw(text, length, cell, number_top);
    if (snapshot.hybrid >= 0 && above != 0) {
        char hybrid[24]{};
        const auto hybrid_length = static_cast<unsigned>(snapshot.hybrid == 0
            ? std::snprintf(hybrid, sizeof(hybrid), "HYBRID OK")
            : std::snprintf(hybrid, sizeof(hybrid), "HYBRID KO (%d)",
                  std::min(snapshot.hybrid, 999)));
        // Half the number's size where the longest form, "HYBRID KO (999)",
        // fits nine tenths of the panel's width, smaller otherwise, so the
        // line keeps its size between OK and KO; one of its own glyph pixels
        // above the number.
        constexpr unsigned longest = 15;
        const float hybrid_cell = std::max(1.0f, std::min(cell * 0.5f,
            static_cast<float>(width) * 0.9f / static_cast<float>(longest * 6 - 1)));
        const auto hybrid_height = static_cast<unsigned>(std::ceil(7 * hybrid_cell));
        const auto gap = static_cast<unsigned>(std::ceil(hybrid_cell));
        if (number_top >= hybrid_height + gap) {
            draw(hybrid, hybrid_length, hybrid_cell, number_top - hybrid_height - gap,
                snapshot.hybrid == 0 ? color(64, 235, 112) : color(255, 80, 80));
        }
    }
    // The lines below the number: half its size, each one of its own glyph
    // pixels below the one above it.
    const float small_cell = std::max(1.0f, cell * 0.5f);
    const auto small_height = static_cast<unsigned>(std::ceil(7 * small_cell));
    const auto small_gap = static_cast<unsigned>(std::ceil(small_cell));
    unsigned next_top = below + small_gap;
    if (snapshot.latency_ms >= 0) {
        char latency[8]{};
        const auto latency_length = static_cast<unsigned>(std::snprintf(
            latency, sizeof(latency), "%dms", std::min(snapshot.latency_ms, 999)));
        if (next_top + small_height <= height) {
            draw(latency, latency_length, small_cell, next_top);
        }
        next_top += small_height + small_gap;
    }
    // V550: the multiplier below the latency, in the number's colour.
    if (snapshot.multiplier >= 2 && snapshot.multiplier <= 9 && next_top + small_height <= height) {
        const char multiplier[3]{static_cast<char>('0' + snapshot.multiplier), 'X', '\0'};
        draw(multiplier, 2, small_cell, next_top);
    }
    return pixels;
}

} // namespace xrfg
