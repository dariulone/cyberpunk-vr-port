#pragma once
#include <algorithm>
#include <cmath>
#include <cstdint>

namespace cvr::input {
inline float StickDeadzone(float value) {
    return std::isfinite(value) && value>=0 && value<=.30f ? value : .15f;
}
inline float StickFullInput(float value) {
    return std::isfinite(value) && value>=.80f && value<=1 ? value : .90f;
}
// TE6-testing-highlight's ApplyStickRange: independently remap each axis from
// its centre deadzone to the chosen full-travel point, preserving its sign.
inline float AnalogAxis(float value,float deadzone,float fullInput) {
    if(!std::isfinite(value))return 0;
    deadzone=StickDeadzone(deadzone);fullInput=StickFullInput(fullInput);
    const float magnitude=std::abs(value);
    if(magnitude<=deadzone)return 0;
    return std::copysign(std::min(1.f,(magnitude-deadzone)/(fullInput-deadzone)),value);
}
inline bool UseAnalogMovement(int mode,bool vehicle) {return mode==1 && !vehicle;}
// The caller supplies raw travel in analog mode and the legacy deadzoned axis
// in fixed mode. Scanner ownership can zero that value before testing it.
inline bool AtFullTravel(float value,bool analog,float fullInput,bool negative=false) {
    if(!std::isfinite(value))return false;
    if(negative)value=-value;
    return analog?value>=StickFullInput(fullInput):value>.90f;
}
// ONE PRESS PER PUSH. A stick held at its stop becomes a single short button press, re-armed only once
// the stick comes back under half travel in the same direction. The game's ToggleCrouch reacts to a tap
// alone (inputContexts.xml gives it no hold timeout), so a level-held R3 did nothing past a quick flick.
// A stick still held when a blocking context (the scanner's zoom, a device screen) ends must come back
// first, or leaving that context would press by itself.
struct StickPulse {
    bool armed=true;
    uint64_t until=0;
    bool Update(bool atStop,float travel,uint64_t nowMs,uint32_t pulseMs,bool allowed) {
        if(atStop) {
            if(armed && allowed)until=nowMs+pulseMs;
            armed=false;
        } else if(!(travel>=.5f)) armed=true;
        if(!allowed)until=0;
        return nowMs<until;
    }
};
}
