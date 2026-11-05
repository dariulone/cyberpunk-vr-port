// The first-launch setup's natives: the hand that holds the weapon, the T-pose calibration and the player's height,
// for the lobby's pages that walk a new player through them (mods/cet/CyberpunkVRPort_Lobby modules/setup.lua).
//
// The calibration itself is OpenXRManager's (Runtimes/OpenXRCalibration.cpp): the T-pose measures which way is
// right, the eyes' height over the floor and the arms' span; the height gives the arms and the shoulders their
// anatomical lengths (Runtimes/BodyProportions.hpp). Both are saved to vrik_calibration.ini, with the mark that
// the setup has been through.

#include "Natives/NativeFunctions.hpp"
#include "Core/LiveControls.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Runtimes/BodyProportions.hpp"
#include "Runtimes/OpenXRManager.hpp"

#include <RED4ext/Scripting/Utils.hpp>

#include <algorithm>
#include <cmath>

// VRFirstSetupNeeded() -> Bool: the setup has never been through on this install.
void VRFirstSetupNeeded(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    stack->code++;
    if (out) *out = !OpenXRManager::Get().IsFirstSetupDone();
}

// VRSetLeftHanded(left): the hand that holds the weapon -- xr_left_handed, saved as the settings panel saves it.
void VRSetLeftHanded(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    bool left = false;
    RED4ext::GetParameter(stack, &left);
    stack->code++;
    LiveControlsUiState state{};
    GetLiveControlsUiState(&state);
    state.xrLeftHanded = left ? 1 : 0;
    SetLiveControlsUiState(&state, 1);
    if (out) *out = true;
}

// VRCalibrationStart(seconds): the T-pose sample begins (arms straight out to the sides, standing straight).
void VRCalibrationStart(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    float seconds = 4.0f;
    RED4ext::GetParameter(stack, &seconds);
    stack->code++;
    if (!std::isfinite(seconds)) seconds = 4.0f;
    OpenXRManager::Get().StartAutoCalibration(std::clamp(seconds, 1.0f, 10.0f));
    if (out) *out = true;
}

// VRCalibrationState() -> Int32: 0 idle, 1 sampling, 2 done, 3 failed (no T-pose seen).
void VRCalibrationState(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, int32_t* out, int64_t) {
    stack->code++;
    if (out) *out = OpenXRManager::Get().GetCalibrationState();
}

// VRCalibrationProgress() -> Float: 0 .. 1 through the sample.
void VRCalibrationProgress(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, float* out, int64_t) {
    stack->code++;
    if (out) *out = OpenXRManager::Get().GetCalibrationProgress();
}

// VRSuggestedHeight() -> Float: the height to offer -- the one given before, else the one whose eyes are as high
// as the last T-pose measured them (to the centimetre), else 1.75 m.
void VRSuggestedHeight(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, float* out, int64_t) {
    stack->code++;
    auto& xr = OpenXRManager::Get();
    float height = xr.GetUserHeight();
    if (!(height > 0.0f)) {
        const float eyes = xr.GetMeasuredEyeHeight();
        height = eyes > 0.0f ? std::round(cvr::body::HeightFromEyes(eyes) * 100.0f) / 100.0f : 1.75f;
    }
    if (out) *out = std::clamp(height, cvr::body::kMinHeight, cvr::body::kMaxHeight);
}

// VRSetPlayerHeight(meters): the body's proportions from it, saved; the setup is through.
void VRSetPlayerHeight(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    float meters = 0.0f;
    RED4ext::GetParameter(stack, &meters);
    stack->code++;
    const bool ok = std::isfinite(meters) && meters > 0.0f;
    if (ok) OpenXRManager::Get().ApplyUserHeight(meters);
    if (out) *out = ok;
}

// VRPlayerHeight() -> Float: the height given, 0 if none.
void VRPlayerHeight(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, float* out, int64_t) {
    stack->code++;
    if (out) *out = OpenXRManager::Get().GetUserHeight();
}
