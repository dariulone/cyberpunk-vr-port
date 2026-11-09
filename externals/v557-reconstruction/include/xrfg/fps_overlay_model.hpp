#pragma once

#include <array>
#include <cstdint>
#include <string_view>
#include <vector>

namespace xrfg {

enum class FpsOverlayPosition { off, upper_left, upper_right, lower_left, lower_right };
[[nodiscard]] FpsOverlayPosition parse_overlay_position(std::string_view value) noexcept;
[[nodiscard]] const char* overlay_position_name(FpsOverlayPosition position) noexcept;

// The digits' size within the counter's panel: big is the original, medium
// half of it and small a third. The panel itself keeps its size and place, so
// the counter's centre does not move and the text keeps the big size's pixel
// density instead of being minified by the compositor.
// "little" because windows.h defines small as a macro; its ini value is "small".
enum class FpsOverlaySize { big, medium, little };
[[nodiscard]] FpsOverlaySize parse_overlay_size(std::string_view value) noexcept;
[[nodiscard]] const char* overlay_size_name(FpsOverlaySize size) noexcept;

struct FpsSnapshot {
    float submitted_fps{};
    bool active{};
    // Share of the window's submissions that carried a new image; 1 when none
    // were repeats or nothing was submitted.
    float new_content_share{1.0f};
    // Generation paused from the tray: drawn as a pause symbol before the
    // number.
    bool paused{};
    // The latency the layer adds to the game's frames, in whole
    // milliseconds, drawn as "11ms" below the number; negative draws nothing.
    int latency_ms{-1};
    // Hybrid's state, drawn above the number while Hybrid is selected:
    // negative nothing, 0 "HYBRID OK" in green, otherwise "HYBRID KO (n)" in
    // red with n the flight-log record that explains it (V506).
    int hybrid{-1};
    // The frame generation multiplier the session runs, drawn as "2X" or
    // "3X" below the latency (where the latency would be when it is off); 0
    // draws nothing.
    int multiplier{0};
};

// The compositor's delivered rate, less the share of it that was repeats.
//
// Each source sees one loss the other cannot. The compositor counts every
// submission it scanned out on time, and a repeat is one, so below half the
// display rate it reads the refresh rate: Callisto Protocol at 36.6 frames a
// second on a 90 Hz display delivered 89.3 by the compositor's count while 16.8
// of those were repeats and 73 new images reached the eye. The counter sees
// the repeats but not a frame the compositor showed on the wrong vsync.
// Scaling one by the other assumes a mispresent is as likely to hit a repeat as
// a new image; where it favours repeats this reads slightly low, never high.
[[nodiscard]] float delivered_new_images(float delivered, const FpsSnapshot& snapshot) noexcept;

// The figure the overlay draws for `rate` on a headset refreshing at
// `refresh_hz`: the refresh rate itself when the rate is within 2% of it,
// the rate otherwise. Both sources read one frame short every few windows
// while the headset shows every frame: the compositor's count is a 0.7 s
// window, so one flagged frame reads 88.6 at 90 Hz, and the counter's own
// second is quantised to the frame. The number then alternated 89/90 on a
// session that was whole. One frame in fifty is below what the eye sees;
// 3% and more stays visible, because that is a real loss.
[[nodiscard]] float displayed_rate(float rate, float refresh_hz) noexcept;

// Caller serializes access. Fixed storage, monotonic wall-clock measurements;
// successful downstream submissions are NOT evidence of physical scanout.
//
// Counts *distinct images*, not submissions. The presenter hands the runtime
// one frame per display period whether or not the application produced one,
// resubmitting what it already holds to keep the cadence. Those repeats carry
// nothing new, and counting them made the overlay report the headset's refresh
// rate rather than the frame rate: measured on The Witcher 3 through VDXR at
// 144 Hz, 144.0 submissions a second of which 65.8 were repeats, against 78.1
// distinct images actually reaching the eye. The reporter saw 144 and a
// picture that visibly was not.
//
// On SteamVR the displayed figure is the compositor's own delivered count
// instead, scaled by this counter's new-content share (delivered_new_images).
// Before that scaling the fix above never applied there, and fpsVR, which
// reads the same compositor counters, cannot apply it at all.
class FpsCounter {
public:
    // `new_content` is false for a repeat.
    void submitted(std::int64_t now_ns, bool synthetic, bool new_content = true) noexcept;
    [[nodiscard]] FpsSnapshot snapshot(std::int64_t now_ns) const noexcept;
    void reset() noexcept { *this = {}; }
private:
    struct Bucket {
        std::int64_t epoch{-1};
        std::uint32_t output{};
        std::uint32_t submissions{};
    };
    Bucket& bucket(std::int64_t now_ns) noexcept;
    std::array<Bucket, 12> buckets_{};
    std::int64_t start_ns_{-1};
    std::int64_t last_synthetic_ns_{-1};
};

struct OverlayPlacement {
    float x{}, y{}, z{-2.0f}, width{}, height{};
};
// Tangent-space bounds common to both eyes. Inset AND capped to the central
// 90 degrees so wide-FOV headsets do not push the panel into peripheral optics.
[[nodiscard]] OverlayPlacement overlay_placement(
    FpsOverlayPosition position, float left, float right, float down, float up) noexcept;
[[nodiscard]] std::uint32_t overlay_texture_width(std::uint32_t eye_width) noexcept;
// The number is centred in a 24x12 grid. An image 20 rows of that grid tall
// keeps it in rows 4-16 and draws Hybrid's state in the four rows above and
// the latency, at half the number's size, in the four below; a 16-row image
// has no rows above (the layout before V506). A 24-row image (V550) has
// eight rows below: the latency, then the multiplier at the latency's size.
[[nodiscard]] std::vector<std::uint32_t> rasterize_fps_overlay(
    std::uint32_t width, std::uint32_t height, const FpsSnapshot& snapshot, bool bgra,
    FpsOverlaySize size = FpsOverlaySize::big);

} // namespace xrfg
