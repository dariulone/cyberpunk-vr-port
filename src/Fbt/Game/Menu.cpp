#include "Fbt/Game/Menu.hpp"
#include "Fbt/Game/Body.hpp"
#include "Fbt/Game/Calibration.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Overlay/UiRec.hpp"

using namespace wrpuppets::fbt;   // the IK library (externals/wrpuppets-fbt-ik)

namespace cvr::fbt::game {

bool DrawSettingsSection(LiveControlsUiState& state) {
    bool changed = false;
    if (UI::CollapsingHeader("FULL-BODY TRACKING", ImGuiTreeNodeFlags_DefaultOpen)) {
        // The extension is chosen when the OpenXR instance is created: the switch applies at the next start.
        bool trackers = state.xrViveTrackers != 0;
        if (UI::Checkbox("Enable Vive trackers", &trackers)) {
            state.xrViveTrackers = trackers ? 1 : 0;
            changed = true;
            // Turning the trackers on is asking for the body: the switches below start on, and can be turned off.
            if (trackers) { SetLegsEnabled(true); SetSpineEnabled(true); SetArmsEnabled(true); }
        }
        if (UI::IsItemHovered()) UI::SetTooltip("Reads your Vive Trackers. Give them the roles Waist, Left Foot and Right Foot in SteamVR.\nTurning it on applies at the next game start.");
        const int step = CalibrationStep();
        if (step == 0) {
            const bool available = CalibrationAvailable();
            UI::BeginDisabled(!available);
            if (UI::Button("Calibrate full-body tracking")) StartCalibration();
            if (UI::IsItemHovered()) UI::SetTooltip("Three steps: look ahead at the horizon line, press both triggers;\nstand straight, arms along the body, look down, press both triggers;\nthen the T-pose on the cross until the countdown ends.\nNeeds the Vive Trackers on the waist and both feet.");
            UI::EndDisabled();
            if (!available)
                UI::TextColored(ImVec4(1.0f, 0.75f, 0.2f, 1.0f), "Full-body calibration works in game only: load a save first.");
        } else if (step == 1) {
            if (UI::Button("Step 1: look ahead at the horizon line, then press both triggers (press to cancel)")) CancelCalibration();
            if (UI::IsItemHovered()) UI::SetTooltip("Release both triggers, take the pose, then pull both together.");
        } else if (step == 2) {
            if (UI::Button("Step 2: arms down, look down, then press both triggers (press to cancel)")) CancelCalibration();
            if (UI::IsItemHovered()) UI::SetTooltip("Release both triggers, take the pose, then pull both together.");
        } else {
            if (UI::Button("Step 3: face the cross: head on its top, arms straight out along its arms, until the countdown ends (press to keep V's arms)")) CancelCalibration();
            if (UI::IsItemHovered()) UI::SetTooltip("Stand facing the cross; the dots turn green on their marks, then the countdown runs.");
        }
        bool legs = LegsEnabled();
        if (UI::Checkbox("V's legs follow the trackers", &legs)) SetLegsEnabled(legs);
        if (UI::IsItemHovered()) UI::SetTooltip("V's hips and legs follow your waist and foot trackers (needs a calibration).\nThe spine, head and arms stay as before.");
        UI::BeginDisabled(!legs);
        bool spine = SpineEnabled();
        if (UI::Checkbox("V's spine and head follow the headset", &spine)) SetSpineEnabled(spine);
        if (UI::IsItemHovered()) UI::SetTooltip("V's spine bends from your hips to your headset, V's eyes on your view (with the legs on).\nThe arms stay as before.");
        UI::BeginDisabled(!spine);
        bool arms = ArmsEnabled();
        if (UI::Checkbox("V's arms follow the controllers", &arms)) SetArmsEnabled(arms);
        if (UI::IsItemHovered()) UI::SetTooltip("V's arms reach your controllers with V's arm length set by the calibration (with the spine on).\nOff: the arms as before.");
        UI::EndDisabled();
        UI::EndDisabled();
    }
    return changed;
}

}  // namespace cvr::fbt::game
