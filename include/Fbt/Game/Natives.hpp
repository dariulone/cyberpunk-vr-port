#pragma once
// The full-body tracking natives for the game's scripts (the CET menu): VRFbtCalibrating, VRFbtCalibrationStep,
// VRFbtSetCalibrationText. Registered from Natives/Registration.cpp.
#include <RED4ext/RED4ext.hpp>

namespace cvr::fbt::game {
void RegisterNatives(RED4ext::CRTTISystem* rtti);
}
