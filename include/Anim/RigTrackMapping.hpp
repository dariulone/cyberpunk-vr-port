#pragma once
#include <cstdint>
#include <cmath>

namespace cvr::anim {
// Track indices belong to the source rig. The pose buffer has a separate
// namespace and capacity, just as the bone buffer does.
inline bool ApplyRigTracks(const uint32_t* pairs, uint32_t count,
                           uint32_t sourceCount, float* destination, uint32_t capacity,
                           volatile float* observed, volatile float* requested,
                           volatile int* enabled, uint32_t limit) {
    for (uint32_t i = 0; i < limit; ++i) observed[i] = -1.0f;
    if (!pairs || !destination || !count || !capacity || count > sourceCount) return false;
    bool any = false;
    for (uint32_t i = 0; i < count; ++i) {
        const auto src = pairs[i * 2], dst = pairs[i * 2 + 1];
        if (src >= sourceCount || src >= limit || dst >= capacity) continue;
        const float before = destination[dst];
        if (!std::isfinite(before)) continue;
        observed[src] = before;
        any = true;
        const float value = requested[src];
        if (enabled[src] && std::isfinite(value)) destination[dst] = value;
    }
    return any;
}
}
