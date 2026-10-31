#pragma once
#include <cstdint>
#include <cctype>
#include <string>

namespace cvr::input {
inline bool ThumbrestProfileAllowed(const char* systemName) {
    std::string name = systemName ? systemName : "";
    for (auto& c : name) c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    // Sense uses Touch emulation, but its modifier is Triangle touch, not thumbrest.
    return name.find("playstation_vr2") == std::string::npos &&
           name.find("playstation vr2") == std::string::npos && name.find("psvr2") == std::string::npos;
}
inline bool ThumbrestDpad(bool held, float& x, float& y, uint16_t& buttons) {
    if (!held) return false;
    uint16_t directions = 0;
    if (y > .5f) directions |= 0x0001;
    if (y < -.5f) directions |= 0x0002;
    if (x < -.5f) directions |= 0x0004;
    if (x > .5f) directions |= 0x0008;
    buttons |= directions;
    x = y = 0; // Suppress look even inside the D-pad dead zone.
    return directions != 0;
}
}
