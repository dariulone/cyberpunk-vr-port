#pragma once
#include <algorithm>
#include <cmath>
#if CVR_RUST_INPUT
#include "Rust/CvrRust.h"
#endif

namespace cvr::input {
// The C++ implementation, kept as the reference the Rust one (rust/crates/cvr-input) is compared against on the same
// inputs (tools/analog_input_tests). With CVR_RUST_INPUT the names below call Rust instead.
namespace cpp {
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
}

#if CVR_RUST_INPUT
inline float StickDeadzone(float value) {return cvr_stick_deadzone(value);}
inline float StickFullInput(float value) {return cvr_stick_full_input(value);}
inline float AnalogAxis(float value,float deadzone,float fullInput) {return cvr_analog_axis(value,deadzone,fullInput);}
inline bool UseAnalogMovement(int mode,bool vehicle) {return cvr_use_analog_movement(mode,vehicle);}
inline bool AtFullTravel(float value,bool analog,float fullInput,bool negative=false) {
    return cvr_at_full_travel(value,analog,fullInput,negative);
}
#else
using cpp::StickDeadzone;
using cpp::StickFullInput;
using cpp::AnalogAxis;
using cpp::UseAnalogMovement;
using cpp::AtFullTravel;
#endif
}
