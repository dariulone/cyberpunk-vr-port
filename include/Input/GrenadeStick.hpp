#pragma once

#include <atomic>

namespace cvr::input {
// CET selects the physical controller while holding a pin-type belt prop.
inline std::atomic<int> grenadeStickHand{-1};
inline std::atomic<unsigned> grenadeStickClicks{0};

struct GrenadeStick {
    bool down[2]{};
    int claimed = -1;

    int Update(int owner, bool eligible, bool left, bool right) {
        const bool pressed[] = {left, right};
        if (claimed >= 0 && !pressed[claimed]) claimed = -1;
        if (eligible && owner >= 0 && owner < 2 && pressed[owner] && !down[owner] && !pressed[1-owner]) {
            claimed = owner;
            grenadeStickClicks.fetch_add(1, std::memory_order_relaxed);
        }
        down[0] = left;
        down[1] = right;
        return claimed;
    }
};
}
