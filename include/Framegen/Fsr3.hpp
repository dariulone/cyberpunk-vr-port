#pragma once
#include "Framegen/Inputs.hpp"
#include <cstdint>
#include <memory>

// iPowerTech's per-eye FSR3 generator, using the port's pinned FidelityFX SDK.
namespace cvr::fsr3 {
enum Mode : int { kInterpolation = 0, kExtrapolation = 1 };
struct Settings {
    bool enabled=false;
    int mode=kInterpolation;   // ini xr_fsr3_mode
    bool opticalFlow=true;   // FidelityFX optical flow for what the engine's vectors miss
    // The interpolated frames go to the headset. Off, FSR3 still interpolates and the headset gets only
    // the real frames: what showing them costs, apart from what computing them costs. Not saved.
    bool show=true;
};
void SetSettings(Settings);
bool ShowGenerated();
bool Enabled();   // switched on and not stopped by a fault
struct GpuWait {ID3D12Fence* fence;uint64_t value;};
// Native presentation supplies the producer queue and the fence for the last headset copy.
bool Generate(ID3D12Device*,ID3D12CommandQueue*,ID3D12Resource* const colors[2],
    const std::shared_ptr<const framegen::Inputs> (&inputs)[2],uint64_t serial,double frameMs,std::shared_ptr<const void> captureLease);
void ConsumerFence(ID3D12Fence*,uint64_t);
// A snap turn of `degrees` was handed to the game (Hooks/OnFootDeltaHead.cpp; the braindance turn, Core/VrCore.cpp).
// The engine does not flag it as a cut: among the next few captures, one whose camera turned more than half of it
// is treated as one. Head turns alone never are.
void NoteSnapTurn(float degrees);
// `queue` waits on the GPU for the last Interpolate or Extrapolate: its frames, and its reads of the capture.
void QueueWaitForLast(ID3D12CommandQueue* queue);
// The interpolation's GPU time, both eyes, smoothed (0 until measured). An interpolated frame fits between two real
// ones only if it is made in half the time between them; for game frames shorter than twice this none is inserted
//.
double GenerationMs();
// The frame interpolated just before `serial`'s capture -- in mode Extrapolation, the one extrapolated from it --
// per eye (resting in COPY_SOURCE, the capture's own size and format), or null when there is none.
ID3D12Resource* Output(unsigned eye,uint64_t serial);
struct Status {
    bool ready{},faulted{};
    uint64_t interpolated{},primed{},skipped{},extrapolated{};
    char text[160]{};
};
Status GetStatus();
void Poll();
void ResetHistory();
bool ReleaseResources();
}
