#pragma once
#include <atomic>
#include <cstdint>
#include "Framegen/HardwareTelemetry.hpp"
#include "Framegen/History.hpp"

namespace cvr::framegen {
// FidelityFX: FidelityFX frame interpolation, with FidelityFX optical flow unless flowScale is 0 (any GPU).
// Nvidia: private patched OFXR V557 with its own XR presentation schedule.
// DlssG: native NVIDIA DLSS Frame Generation through NGX (RTX 40/50).
enum class Backend:int { FidelityFX=0,Nvidia=1,DlssG=2 };
const char* BackendName(Backend);
struct Settings {
    bool enabled=false,overlay=false,autoPace=true;
    bool ofxrLogging=true; // V557 timing/error flight recorder, independent of FPS overlay
    // DLSS-G where the GPU has it (falls back to FidelityFX), and FidelityFX interpolation on the
    // engine's vectors alone: optical flow measured no difference in quality for its GPU time.
    Backend backend=Backend::DlssG;
    int quality=2,flowScale=0,overlayCorner=1;
    int overlayLayout=0,overlayFollow=1,historySeconds=30;
    float overlayCone=60,overlayScale=1,overlayDistance=1.25f,overlayX=0,overlayY=0;
};
struct Statistics {
    double realFps{},generatedFps{},outputFps{},repeatFps{};
    double cpuMs{},cpuPeakMs{},gpuMs{},gpuPeakMs{},generationMs{};
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
void OnGeneration(double milliseconds);
void OnInput(unsigned eye,uint32_t width,uint32_t height);
void OnSkip();
void SetVram(uint64_t bytes);
double NowMs();
void DisplayPeriod(int64_t nanoseconds);
void PacingReady(bool);
void OnPresentationCycle(bool missedAdjacentFrame);
void PaceRenderedFrame();
bool ParseSetting(const char*,Settings&);
void WriteSettings(void* file,const Settings&);
}
