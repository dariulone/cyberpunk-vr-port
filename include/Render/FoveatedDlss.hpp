#pragma once
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <string>
#include <d3d12.h>

namespace cvr::foveated {
struct Settings {
    int mode = 0; // Off, Quality, Balanced, Performance
    float widthPercent = 70.0f;
    float heightPercent = 65.0f;
    bool showBoundary = false;
    float offsetXPercent = 0.0f;
    float offsetYPercent = -10.0f;
    uint8_t parsedDimensions = 0; // INI migration only; explicit axes take precedence.
    bool eyeTracked = false;
    bool SameAllocation(const Settings& other) const {
        return mode == other.mode && widthPercent == other.widthPercent &&
            heightPercent == other.heightPercent;
    }
    bool SameReconstruction(const Settings& other) const {
        return SameAllocation(other) && offsetXPercent == other.offsetXPercent &&
            offsetYPercent == other.offsetYPercent && eyeTracked == other.eyeTracked;
    }
    bool operator==(const Settings& other) const { return SameReconstruction(other) && showBoundary == other.showBoundary; }
};
struct Region {
    unsigned x{}, y{}, width{}, height{}, inputW{}, inputH{}, sourceX{}, sourceY{};
    bool operator==(const Region&) const = default;
};
struct CropStep {
    float deltaX = 0.0f;
    float deltaY = 0.0f;
    bool saccadeReset = false;
    bool centralReset = false;
};
inline CropStep ComputeCropStep(bool engineOrFirstReset, bool havePrevCrop,
                                unsigned prevSourceX, unsigned prevSourceY,
                                const Region& center) {
    CropStep step{};
    if (!engineOrFirstReset && havePrevCrop) {
        step.deltaX = float(int(center.sourceX) - int(prevSourceX));
        step.deltaY = float(int(center.sourceY) - int(prevSourceY));
        step.saccadeReset =
            std::fabs(step.deltaX) > 0.6f * float(center.inputW) ||
            std::fabs(step.deltaY) > 0.6f * float(center.inputH);
    }
    step.centralReset = engineOrFirstReset || step.saccadeReset;
    if (step.centralReset) {
        step.deltaX = 0.0f;
        step.deltaY = 0.0f;
    }
    return step;
}
// Explicit time makes release/hold behavior testable without sleeping.
class SettingsDelay {
    Settings requested, applied;
    uint64_t applyAfter{};
    bool editing{};
public:
    void Request(Settings, uint64_t now);
    void SetEditing(bool, uint64_t now);
    void Update(uint64_t now);
    Settings Requested() const { return requested; }
    Settings Applied() const { return applied; }
};
struct Status {
    bool loaded{}, ready{}, active{};
    uint64_t evaluations{}, processed{};
    float nativeMs{}, foveatedMs{}, peripheralMs{};
    float totalMs{};
    unsigned views{};
    char text[192] = "Off";
};
Settings Normalize(Settings);
bool Enabled(Settings);
Settings GetSettings();
Settings GetAppliedSettings();
void SetSettings(Settings);
void SetUiEditing(bool);
void UpdateSettings();
Region CenterRegion(Settings, unsigned renderW, unsigned renderH, unsigned outputW, unsigned outputH);
bool ParseSetting(const char*, Settings&);
void WriteSettings(FILE*, Settings);
Status GetStatus();
// Called once per game Present, outside the image-submission lock.
void Tick(ID3D12Device*, ID3D12CommandQueue*);
void Shutdown();
void BeforeSubmit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void ResetList(ID3D12GraphicsCommandList*);
}
