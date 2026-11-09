#pragma once

#include <algorithm>
#include <chrono>

namespace xrfg {

// How much of the compositor's frame the SteamVR presenter leaves in hand when
// a real frame's submission lands - the setpoint the acquisition step holds the
// grid at - given where the compositor starts composing.
//
// Two bounds enclose the margin. Too little and the hand-over's own jitter
// pushes a frame past the vsync. Too much and the submission lands before the
// compositor's running start: it is taken for the frame already being
// assembled, the next one collides with it, and about half the pair is never
// scanned out. Between them the margin is free.
//
// The running start was taken for a property of the machine: 4.0 to 5.0 ms
// before vsync on the one it was measured on, so a fixed 3 ms cleared it by a
// millisecond or more and nothing read it. On a PSVR2 through SteamVR it sits
// 3.1 ms before vsync when the scene is light (issue 44): the 3 ms margin
// landed the hand-over 20 microseconds from it, every real frame was shown
// twice and every synthetic that followed was dropped - 62 frames a second
// delivered from 90 submitted, with every layer-side reading converged. The
// same compositor widened to 4.1 ms after a few seconds of losses and the
// same margin then delivered 85-90. The compositor reports where it started
// composing each frame (m_flCompositorRenderStartMs, measured from that
// frame's vsync), so the bound is read rather than assumed.
//
// The margin is the fixed value where the running start leaves it room, and
// `clearance` inside the running start where it does not. 1 ms of clearance
// is what every measured working session had: 0.96 to 1.72 ms on the
// reference machine across V554-V556, 1.0 ms in issue 44's recovered state
// against 0.0 in its failing one. The floor keeps the hand-over off the vsync
// itself: the call runs 0.6 ms with a p99 of about 1 ms.
//
// Where the start is unknown or implausible the fixed margin stands, which is
// the behaviour every session had before this existed.
[[nodiscard]] constexpr std::chrono::nanoseconds submit_margin_for_running_start(
    std::chrono::nanoseconds scanout,
    std::chrono::nanoseconds running_start_after_vsync,
    std::chrono::nanoseconds fixed_margin = std::chrono::nanoseconds(3'000'000),
    std::chrono::nanoseconds clearance = std::chrono::nanoseconds(1'000'000),
    std::chrono::nanoseconds floor = std::chrono::nanoseconds(1'500'000)) noexcept {
    if (scanout <= std::chrono::nanoseconds::zero() ||
        running_start_after_vsync <= std::chrono::nanoseconds::zero() ||
        running_start_after_vsync >= scanout || floor > fixed_margin) {
        return fixed_margin;
    }
    const auto running_start_before_vsync = scanout - running_start_after_vsync;
    const auto inside = running_start_before_vsync - clearance;
    return std::clamp(inside, floor, fixed_margin);
}

}  // namespace xrfg
