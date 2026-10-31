#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>

namespace cvr::haptics {
// Recovered from the earlier fork's QueueOpenXRHapticPulse/RecoilOnShot.
struct PulseRequest {
    float amplitude = 0;
    int durationMs = 0;
    uint64_t queuedMs = 0;
    void Merge(float amp, int ms, uint64_t now) {
        if (!std::isfinite(amp) || amp <= 0 || amp > 1 || ms < 1 || ms > 1000) return;
        if (!Fresh(now)) *this = {};
        amplitude = std::max(amplitude, amp);
        durationMs = std::max(durationMs, ms);
        queuedMs = now;
    }
    bool Fresh(uint64_t now) const {
        return durationMs > 0 && now >= queuedMs && now - queuedMs <= 100;
    }
};
inline PulseRequest LegacyShot(float kick) {
    if (!std::isfinite(kick) || kick <= 0 || kick > 16) kick = 1;
    const float root = std::sqrt(kick);
    return {std::min(1.0f, .45f + .25f * root), std::min(65, static_cast<int>(35 + 8 * root)), 0};
}
// Match the established dual Toolkit impact strength without coupling it to
// adaptive-trigger selection. A heavy gun keeps the longer impact envelope.
inline PulseRequest DualToolkitShot(float kick) {
    const bool heavy = std::isfinite(kick) && kick >= 3.5f;
    return {heavy ? 1.f : .85f, heavy ? 125 : 75, 0};
}
inline float SafeGain(float value, float fallback = 1.0f) {
    return std::isfinite(value) ? std::clamp(value, 0.0f, 2.0f) : fallback;
}
inline bool VehicleFresh(bool driving, uint64_t now, uint64_t received) {
    return driving && received != 0 && now >= received && now - received <= 100;
}
}
