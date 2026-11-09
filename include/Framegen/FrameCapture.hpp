#pragma once
#include "Framegen/Generator.hpp"

namespace cvr::framegen {
// Diagnostic: with CyberpunkVR_FramegenCapture set to 1, the next three consecutive real frames are
// written to the plugin log/framegen-capture/<time>/ -- both eyes' colour, the engine depth and motion
// vectors, the engine camera and the eye cameras -- so every generator can be replayed offline
// against a real frame in between (tools/framegen_tests: replay). The export reads 2 when written,
// -1 on failure. Costs nothing while disarmed.
// UI-triggered, non-persistent capture. Delay is serviced by capture calls, not UI draws.
bool ScheduleFrameCapture(unsigned delaySeconds=10);
void CancelFrameCapture(); // cancels a scheduled/incomplete capture; never frees pending GPU copies
void FrameCaptureStatus(char* text,size_t capacity,bool* busy=nullptr);
void CaptureFrame(ID3D12Device*,ID3D12CommandQueue*,ID3D12Resource* const colors[2],
    const std::shared_ptr<const Inputs> (&inputs)[2],const EyePair (&cameras)[2]);
}
