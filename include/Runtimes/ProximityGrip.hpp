#pragma once
#include <cstdint>

namespace cvr::cyberfinger {
// The caller supplies a filtered fist with hysteresis. Target selection remains
// with the port's holsters, magazines, support grip and nearby world interaction.
// A fresh open hand is required on startup and after loss, so reacquisition with
// closed fingers cannot equip/use something. A brief dropout does not drop it.
class ProximityGrip {
public:
    bool Update(bool valid, bool closed, uint64_t now) {
        if (!valid) {
            if (!seen_ || now - lastValid_ >= kLossMs) {
                held_ = armed_ = releasing_ = false;
            }
            return held_;
        }
        // A gap can occur without intermediate invalid samples (focus/pause).
        if (seen_ && now - lastValid_ >= kLossMs) held_ = armed_ = releasing_ = false;
        lastValid_ = now;
        seen_ = true;
        if (!closed) {
            armed_ = true;
            if (held_) {
                if (!releasing_) { releaseAt_ = now; releasing_ = true; }
                if (now - releaseAt_ >= kReleaseMs) held_ = releasing_ = false;
            }
        } else {
            releasing_ = false;
            if (armed_) held_ = true;
        }
        return held_;
    }
    static constexpr uint64_t kReleaseMs = 100, kLossMs = 250;
private:
    bool held_ = false, armed_ = false, releasing_ = false, seen_ = false;
    uint64_t lastValid_ = 0, releaseAt_ = 0;
};
}
