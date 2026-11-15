#pragma once

#include <cstring>
#include <initializer_list>

namespace cvr::lobby::detail {
// Identify the 432-byte menu composition constants before modifying their weights.
// Return values are the existing CyberpunkVR_LobbyComposeStats rejection indices:
// 0 accepted, 2 weights, 3 target size, 4 repeated grading, 5 grading values.
inline int CompositionBlock(const float* b, bool drawHudNode = false) {
    const auto weight = [](float v) { return v == 0.0f || (v >= 1e-3f && v <= 1.0f); };
    for (const int i : {4, 5, 6, 7, 8})
        if (!weight(b[i])) return 2;
    const float w = b[66], h = b[67];
    if (!(w >= 64.0f && w <= 16384.0f && h >= 64.0f && h <= 16384.0f) ||
        w != float(int(w)) || h != float(int(h))) return 3;
    // The original layout repeats 18 grading floats in rows 22..26.xy.
    // DrawHUD also emits a layout with only rows 22..23 initialized. A live
    // game-2.5 capture showed its remaining rows alternating between zeros and
    // stack pointers/counters/pose data. Testing that tail rejected 826 of 864
    // frames and repeatedly expired WorldShown's 500 ms deadline.
    // Accept the shorter prefix only from the known DrawHUD node, still with
    // exact duplicated bytes, real weights, a pixel target and valid grading.
    // Other nodes retain the original full-layout fingerprint. Row 26.zw is
    // unused padding in both layouts.
    if (std::memcmp(b + 16, b + 88, 18 * sizeof(float)) != 0 &&
        (!drawHudNode || std::memcmp(b + 16, b + 88, 8 * sizeof(float)) != 0)) return 4;
    const float sat = b[26], ab = b[27];
    return sat >= 1e-3f && sat <= 4.0f && ab >= 0.0f && ab < 0.01f ? 0 : 5;
}
}  // namespace cvr::lobby::detail
