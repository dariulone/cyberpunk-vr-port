#pragma once
#include <cstdint>
#include <string>
#include <cctype>

namespace cvr::input {
inline bool IsPsvr2System(const char* name) {
    std::string value = name ? name : "";
    for (auto& c : value) c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    return value.find("playstation_vr2") != std::string::npos ||
           value.find("playstation vr2") != std::string::npos || value.find("psvr2") != std::string::npos;
}

// Triangle is capacitive: clicking still remains Y. Zero look for the whole
// shift, including below the direction threshold, to prevent accidental turns.
inline bool TriangleDpad(bool enabled, float& x, float& y, uint16_t& buttons) {
    if (!enabled) return false;
    uint16_t dpad = 0;
    if (y > .5f) dpad |= 0x0001;
    if (y < -.5f) dpad |= 0x0002;
    if (x < -.5f) dpad |= 0x0004;
    if (x > .5f) dpad |= 0x0008;
    buttons |= dpad;
    x = y = 0;
    return dpad != 0;
}

// Called at the actual game poll, never as a one-XR-frame pulse. Losing focus
// or an input provider cancels the gesture rather than turning it into a tap.
class SenseSystemButton {
public:
    uint16_t Update(bool pressed, uint64_t now, bool available) {
        if (!available) { held = fired = false; return 0; }
        uint16_t event = 0;
        if (pressed) {
            if (!held) { began = now; fired = false; }
            else if (!fired && now - began >= 500) { event = 0x0020; fired = true; }
        } else if (held && !fired) { event = now - began >= 500 ? 0x0020 : 0x0010; }
        held = pressed;
        return event;
    }
private:
    bool held = false, fired = false;
    uint64_t began = 0;
};
}
