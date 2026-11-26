#include "Render/FoveatedDlss.hpp"
#include <algorithm>
#include <cmath>
#include <mutex>

#if !CVR_RUST_VRIK
// The C++ build's policy; the Rust build's is rust/crates/cvr-render/src/foveated.rs (the header calls it).
namespace cvr::foveated {
namespace { std::mutex mutex; SettingsDelay settingsDelay; }
Settings Normalize(Settings value) {
    const Settings defaults;
    value.mode = std::clamp(value.mode, 0, 3);
    value.widthPercent = std::isfinite(value.widthPercent) ? std::clamp(value.widthPercent, 20.0f, 100.0f) : defaults.widthPercent;
    value.heightPercent = std::isfinite(value.heightPercent) ? std::clamp(value.heightPercent, 20.0f, 100.0f) : defaults.heightPercent;
    const float maxX = (100.0f-value.widthPercent)*0.5f, maxY = (100.0f-value.heightPercent)*0.5f;
    value.offsetXPercent = std::clamp(std::isfinite(value.offsetXPercent) ? value.offsetXPercent : defaults.offsetXPercent, -maxX, maxX);
    value.offsetYPercent = std::clamp(std::isfinite(value.offsetYPercent) ? value.offsetYPercent : defaults.offsetYPercent, -maxY, maxY);
    value.parsedDimensions = 0;
    return value;
}
bool Enabled(Settings value) {
    value = Normalize(value);
    return value.mode != 0;
}
void SettingsDelay::Request(Settings value,uint64_t now) {
    value=Normalize(value);
    if(!value.SameReconstruction(requested))applyAfter=now+1000;
    requested=value;
    applied.showBoundary=value.showBoundary;
    if(!Enabled(value))applied=value; // Off never waits for a pending expensive rebuild.
}
void SettingsDelay::SetEditing(bool active,uint64_t now) {
    if(active || editing)applyAfter=now+1000;
    editing=active;
}
void SettingsDelay::Update(uint64_t now) {
    if(!editing && now>=applyAfter)applied=requested;
}
Settings GetSettings() { std::lock_guard lock(mutex); return settingsDelay.Requested(); }
Settings GetAppliedSettings() { std::lock_guard lock(mutex); return settingsDelay.Applied(); }
void SetSettings(Settings value) { std::lock_guard lock(mutex); settingsDelay.Request(value,GetTickCount64()); }
void SetUiEditing(bool active) { std::lock_guard lock(mutex); settingsDelay.SetEditing(active,GetTickCount64()); }
void UpdateSettings() { std::lock_guard lock(mutex); settingsDelay.Update(GetTickCount64()); }
Region CenterRegion(Settings settings,unsigned renderW,unsigned renderH,unsigned outputW,unsigned outputH) {
    if(!renderW || !renderH || !outputW || !outputH)return {};
    settings=Normalize(settings);
    Region r;
    r.inputW=std::min(renderW,std::max(32u,unsigned(std::lround(renderW*settings.widthPercent/100))));
    r.inputH=std::min(renderH,std::max(32u,unsigned(std::lround(renderH*settings.heightPercent/100))));
    const auto origin=[](unsigned full,unsigned size,float offset) {
        return unsigned(std::clamp(long((full-size)/2)+std::lround(full*offset/100),0L,long(full-size)));
    };
    r.sourceX=origin(renderW,r.inputW,settings.offsetXPercent);
    r.sourceY=origin(renderH,r.inputH,settings.offsetYPercent);
    r.width=unsigned(std::ceil(double(r.inputW)*outputW/renderW));
    r.height=unsigned(std::ceil(double(r.inputH)*outputH/renderH));
    r.x=unsigned(double(r.sourceX)*outputW/renderW);
    r.y=unsigned(double(r.sourceY)*outputH/renderH);
    return r;
}
bool ParseSetting(const char* line, Settings& value) {
    int number{}; float amount{};
    if (sscanf_s(line, "xr_foveated_dlss = %d", &number) == 1) { value.mode = number; return true; }
    if (sscanf_s(line, "xr_foveated_width = %f", &amount) == 1) {
        value.widthPercent = amount; value.parsedDimensions |= 1; return true;
    }
    if (sscanf_s(line, "xr_foveated_height = %f", &amount) == 1) {
        value.heightPercent = amount; value.parsedDimensions |= 2; return true;
    }
    if (sscanf_s(line, "xr_foveated_offset_x = %f", &amount) == 1) { value.offsetXPercent = amount; return true; }
    if (sscanf_s(line, "xr_foveated_offset_y = %f", &amount) == 1) { value.offsetYPercent = amount; return true; }
    if (sscanf_s(line, "xr_foveated_edges = %f", &amount) == 1) {
        const float size = 100.0f - 2.0f * std::clamp(amount, 0.0f, 40.0f);
        if (!(value.parsedDimensions & 1)) value.widthPercent = std::isfinite(amount) ? size : Settings{}.widthPercent;
        if (!(value.parsedDimensions & 2)) value.heightPercent = std::isfinite(amount) ? size : Settings{}.heightPercent;
        return true;
    }
    if (sscanf_s(line, "xr_foveated_boundary = %d", &number) == 1) { value.showBoundary = number != 0; return true; }
    return false;
}
void WriteSettings(FILE* file, Settings value) {
    value = Normalize(value);
    std::fprintf(file, "xr_foveated_dlss=%d\nxr_foveated_width=%.2f\nxr_foveated_height=%.2f\nxr_foveated_boundary=%d\nxr_foveated_offset_x=%.2f\nxr_foveated_offset_y=%.2f\n",
        value.mode, value.widthPercent, value.heightPercent, value.showBoundary ? 1 : 0, value.offsetXPercent, value.offsetYPercent);
}
}
#endif
