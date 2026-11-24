#pragma once

namespace cvr::haptics {

// Captures the Windows default render endpoint in loopback mode and converts its low-frequency
// vehicle audio/transients into a small per-hand OpenXR amplitude envelope. The worker is started
// only for native output (Quest or PSVR2 without the bridge). Toolkit retains its PCM path.
void StartVehicleAudioCapture();
void StopVehicleAudioCapture();

} // namespace cvr::haptics
