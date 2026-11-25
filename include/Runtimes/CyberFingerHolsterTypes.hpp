#pragma once

namespace cvr::cyberfinger::holsters {
struct Category {
    const char* label;
    const char* key;
    int bit;
};
// These bits are also used by the CET holster placement table.
inline constexpr Category categories[] = {
    {"Handguns", "xr_cyberfinger_holster_handguns", 1},
    {"Machine guns", "xr_cyberfinger_holster_machine_guns", 2},
    {"Shotguns", "xr_cyberfinger_holster_shotguns", 4},
    {"Knives", "xr_cyberfinger_holster_knives", 8},
    {"Grenades", "xr_cyberfinger_holster_grenades", 16},
    {"Swords", "xr_cyberfinger_holster_swords", 32},
    {"Rifles", "xr_cyberfinger_holster_rifles", 64},
    {"Other melee", "xr_cyberfinger_holster_other_melee", 128},
};
inline constexpr int all = 255;
inline constexpr int defaults = 8 | 16 | 32 | 128;
}
