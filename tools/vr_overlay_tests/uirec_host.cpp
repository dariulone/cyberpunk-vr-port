// What src/Overlay/UiRec.cpp reads and writes in the plugin, for the test programs: the live controls are a
// zeroed snapshot here and nothing is persisted.
#include "Overlay/LiveControlsUi.hpp"
#include "Core/LiveControls.hpp"
LiveControls g_liveControls{};
namespace cvr::fingergrip { bool GripFromFingers(){return false;} }

extern "C" void GetLiveControlsUiState(LiveControlsUiState* outState) {
    if (outState) *outState = LiveControlsUiState{};
}
extern "C" void SetLiveControlsUiState(const LiveControlsUiState*, int) {}
