#pragma once
#include <atomic>
#include <cstdint>
#include "Framegen/HardwareTelemetry.hpp"
#include "Framegen/History.hpp"

namespace cvr::framegen {
// FidelityFX: FidelityFX frame interpolation, with FidelityFX optical flow unless flowScale is 0 (any GPU).
// Nvidia: NVIDIA optical flow (OFA) + FidelityFX frame interpolation.
// DlssG: native NVIDIA DLSS Frame Generation through NGX (RTX 40/50).
// Anr: ANR FG (Apex Neural Rendering), a DLSS-G provider dll run through the DlssG path (Framegen/AnrFrameGen.hpp).
enum class Backend:int { FidelityFX=0,Nvidia=1,DlssG=2,Anr=3 };
inline bool NgxBackend(Backend b) {return b==Backend::DlssG || b==Backend::Anr;}
const char* BackendName(Backend);
struct Settings {
    bool enabled=false,overlay=false,autoPace=true;
    // Real frames three display periods apart while the game cannot make two (CadenceLock). Off: always two, the
    // rate the headset's refresh was chosen for; a slow frame is absorbed by the XR side instead.
    bool cadenceLock=false;
    // NVIDIA's optical flow (OFA) at half resolution under FidelityFX interpolation; a GPU without OFA
    // falls back to FidelityFX's own flow (Generator.cpp).
    Backend backend=Backend::Nvidia;
    int quality=2,flowScale=50,overlayCorner=1;
    int overlayLayout=0,overlayFollow=1,historySeconds=30;
    float overlayCone=60,overlayScale=1,overlayDistance=1.25f,overlayX=0,overlayY=0;
};
struct Statistics {
    double realFps{},generatedFps{},outputFps{},repeatFps{};
    // generationMs: the GPU time of a pair's generation (its command lists); generationSpanMs: from its start to its
    // end on the GPU timeline, the optical flow engine's wait and other work in between included.
    double cpuMs{},cpuPeakMs{},gpuMs{},gpuPeakMs{},generationMs{},generationSpanMs{};
    double averageFps{},minimumFps{},maximumFps{},low1Fps{},low01Fps{},cpuAverageMs{},gpuAverageMs{},cpuP99Ms{},cpuP999Ms{},gpuP99Ms{},gpuP999Ms{};
    double elapsedSeconds{},budgetMs{},repeatPercent{};
    uint32_t historySamples{};
    std::array<float,GraphPoints> cpuGraph{},gpuGraph{};
    HardwareStatistics hardware;
    uint64_t realFrames{},generatedFrames{},repeatedFrames{},skipped{},vramBytes{};
    uint64_t realSubmissions{};
    uint64_t inputFrames[2]{};
    uint32_t renderWidth[2]{},renderHeight[2]{};
    bool active{},gpuValid{},inputsReady{};
    char status[160]="Disabled";
};
Settings GetSettings();
// The optical flow in use: the NVIDIA setting falls back to FidelityFX on other GPUs.
Backend ActiveBackend();
void PublishActiveBackend(Backend);
bool Enabled();
// Depth submission uses the same camera-labelled snapshots even with framegen off.
bool DepthCaptureEnabled();
bool CaptureInputsEnabled();
// The motion vectors are collected with the depth: for framegen, and for AFW's frame warp (Render/Afw.hpp).
bool MotionWanted();
void SetDepthCaptureEnabled(bool enabled);
bool MetricsEnabled();
void SetSettings(Settings value);
Statistics GetStatistics();
struct RuntimeStatus {bool active{};char text[160]{};};
RuntimeStatus GetRuntimeStatus();
void ResetStatistics();
void Status(const char* text,bool active=false);
void OnRealFrame(uint64_t serial,double nowMs);
void OnSubmitted(uint64_t serial,bool generated,bool repeat,double nowMs);
void OnGpuFrame(double milliseconds);
void OnGeneration(double milliseconds,double spanMilliseconds);
void OnInput(unsigned eye,uint32_t width,uint32_t height);
void OnSkip();
void SetVram(uint64_t bytes);
double NowMs();
// Called when xrWaitFrame returns. SteamVR scheduling multiples are filtered only for producer pacing;
// the caller retains the original predicted period for OpenXR display-time calculations.
void DisplayPeriod(int64_t nanoseconds,bool steamVr=false);
// xr_fps_cap: the game held to the headset's refresh rate when framegen is not pacing it (default on).
bool FpsCap();
void SetFpsCap(bool);
void PacingReady(bool);
void OnPresentationCycle(bool missedAdjacentFrame);
void PaceRenderedFrame();
bool ParseSetting(const char*,Settings&);
void WriteSettings(void* file,const Settings&);
}
