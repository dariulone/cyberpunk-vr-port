#pragma once
#include <cstdint>

namespace cvr::input {
// Two deliberate trigger breaks per hand within 600 ms request the game's
// Heywood Hurricane. Holding an automatic gun, one fast hand, and a pair of
// simultaneous first shots cannot activate it. Inputs already use WeaponTrigger
// hysteresis; this detector does not interpret analog noise a second time.
struct DualWieldRapidFire {
    static constexpr uint64_t WindowMs = 600;
    static constexpr uint64_t RequestMs = 200;
    bool down[2]{};
    uint64_t first[2]{}, last[2]{}, requestUntil{};
    unsigned count[2]{};

    bool Update(bool active, bool left, bool right, uint64_t now) {
        if (!active) {
            *this = {};
            // No request or partial cadence survives a gameplay-context change.
            down[0] = left; down[1] = right;
            return false;
        }
        const bool held[2]{left, right};
        for (int side = 0; side != 2; ++side) {
            if (count[side] && (now < first[side] || now - first[side] > WindowMs)) {
                if (count[side] == 2 && now >= last[side] && now - last[side] <= WindowMs) {
                    count[side] = 1;
                    first[side] = last[side];
                } else count[side] = 0;
            }
            if (held[side] && !down[side]) {
                if (count[side] == 0) first[side] = now;
                // Keep only the two most recent breaks from this hand.
                else if (count[side] >= 2) first[side] = last[side];
                last[side] = now;
                if (count[side] < 2) ++count[side];
            }
            down[side] = held[side];
        }
        if (count[0] >= 2 && count[1] >= 2) {
            requestUntil = now + RequestMs;
            count[0] = count[1] = 0;
        }
        return now < requestUntil;
    }
};
} // namespace cvr::input
