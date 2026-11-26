#pragma once

namespace cvr {
// Persisted in vrport-launcher.ini. Missing and unrecognized values use the game's output.
enum class MonitorOutput { Default = 0, Left = 1, Right = 2 };

constexpr MonitorOutput ParseMonitorOutput(int value) {
    return value == 1 ? MonitorOutput::Left : value == 2 ? MonitorOutput::Right : MonitorOutput::Default;
}

MonitorOutput GetMonitorOutput();
void SetMonitorOutput(MonitorOutput output);
}
