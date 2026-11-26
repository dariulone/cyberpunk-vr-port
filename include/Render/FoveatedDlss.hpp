#pragma once
#include <cstddef>
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
    bool SameReconstruction(const Settings& other) const {
        return mode == other.mode && widthPercent == other.widthPercent &&
            heightPercent == other.heightPercent && offsetXPercent == other.offsetXPercent &&
            offsetYPercent == other.offsetYPercent;
    }
    bool operator==(const Settings& other) const { return SameReconstruction(other) && showBoundary == other.showBoundary; }
};
struct Region {
    unsigned x{}, y{}, width{}, height{}, inputW{}, inputH{}, sourceX{}, sourceY{};
    bool operator==(const Region&) const = default;
};
// Explicit time makes release/hold behavior testable without sleeping.
class SettingsDelay;
#if CVR_RUST_VRIK
}
// The policy: rust/crates/cvr-render/src/foveated.rs.
extern "C" {
void cvr_foveated_delay_request(cvr::foveated::SettingsDelay*, cvr::foveated::Settings, uint64_t now);
void cvr_foveated_delay_set_editing(cvr::foveated::SettingsDelay*, bool active, uint64_t now);
void cvr_foveated_delay_update(cvr::foveated::SettingsDelay*, uint64_t now);
}
namespace cvr::foveated {
#endif
class SettingsDelay {
    Settings requested, applied;
    uint64_t applyAfter{};
    bool editing{};
public:
#if CVR_RUST_VRIK
    void Request(Settings value, uint64_t now) { cvr_foveated_delay_request(this, value, now); }
    void SetEditing(bool active, uint64_t now) { cvr_foveated_delay_set_editing(this, active, now); }
    void Update(uint64_t now) { cvr_foveated_delay_update(this, now); }
#else
    void Request(Settings, uint64_t now);
    void SetEditing(bool, uint64_t now);
    void Update(uint64_t now);
#endif
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
#if CVR_RUST_VRIK
}
extern "C" {
cvr::foveated::Settings cvr_foveated_normalize(cvr::foveated::Settings);
bool cvr_foveated_enabled(cvr::foveated::Settings);
cvr::foveated::Settings cvr_foveated_get_settings();
cvr::foveated::Settings cvr_foveated_get_applied_settings();
void cvr_foveated_set_settings(cvr::foveated::Settings);
void cvr_foveated_set_ui_editing(bool);
void cvr_foveated_update_settings();
cvr::foveated::Region cvr_foveated_center_region(cvr::foveated::Settings, unsigned renderW, unsigned renderH, unsigned outputW,
                                                 unsigned outputH);
bool cvr_foveated_parse_setting(const char*, cvr::foveated::Settings*);
void cvr_foveated_write_settings(FILE*, cvr::foveated::Settings);
}
namespace cvr::foveated {
static_assert(sizeof(Settings) == 28 && offsetof(Settings, offsetXPercent) == 16 && sizeof(Region) == 32 &&
              sizeof(SettingsDelay) == 72, "mirrored by rust/crates/cvr-render/src/foveated.rs");
inline Settings Normalize(Settings v) { return cvr_foveated_normalize(v); }
inline bool Enabled(Settings v) { return cvr_foveated_enabled(v); }
inline Settings GetSettings() { return cvr_foveated_get_settings(); }
inline Settings GetAppliedSettings() { return cvr_foveated_get_applied_settings(); }
inline void SetSettings(Settings v) { cvr_foveated_set_settings(v); }
inline void SetUiEditing(bool active) { cvr_foveated_set_ui_editing(active); }
inline void UpdateSettings() { cvr_foveated_update_settings(); }
inline Region CenterRegion(Settings v, unsigned renderW, unsigned renderH, unsigned outputW, unsigned outputH) {
    return cvr_foveated_center_region(v, renderW, renderH, outputW, outputH);
}
inline bool ParseSetting(const char* line, Settings& v) { return cvr_foveated_parse_setting(line, &v); }
inline void WriteSettings(FILE* file, Settings v) { cvr_foveated_write_settings(file, v); }
#else
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
#endif
Status GetStatus();
// Called once per game Present, outside the image-submission lock.
void Tick(ID3D12Device*, ID3D12CommandQueue*);
void Shutdown();
void BeforeSubmit(ID3D12CommandQueue*,UINT,ID3D12CommandList* const*);
void ResetList(ID3D12GraphicsCommandList*);
}
