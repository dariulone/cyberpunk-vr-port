#pragma once
// The full-body tracking section of the VR settings (the ImGui panel, which the in-world VR menu records): the
// switch that reads the trackers, the calibration button, showing the step while it runs. Called from
// Overlay/OverlayPanels.cpp; true when the panel's state changed.
struct LiveControlsUiState;
namespace cvr::fbt::game {
bool DrawSettingsSection(LiveControlsUiState& state);
}
