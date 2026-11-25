#include "Runtimes/CyberFingerInput.hpp"
#include "Core/Paths.hpp"
#include <atomic>
#include <cstdio>
#include <windows.h>
#include <cmath>
#include <algorithm>

extern void Log(const char*, ...);
namespace cvr::cyberfinger {
namespace { std::atomic<bool> g_enabled{false};std::atomic<uint64_t> g_activeUntil{};std::atomic<float> g_hudScale{1.25f},g_stickActionThreshold{.8f}; }
void Configure() {
    FILE* file = nullptr;
    int enabled = 0;
    float hudScale=1.25f;
    float stickActionThreshold=.8f;
    const auto path = cvr::paths::FileA(cvr::paths::Folder::Config, "cyberfinger.ini");
    if (fopen_s(&file, path.c_str(), "r") == 0 && file) {
        char line[256];
        while (fgets(line, sizeof(line), file)) {
            int value = 0;
            if (sscanf_s(line, " enabled = %d", &value) == 1) enabled = value;
            float scale{};if(sscanf_s(line," hud_scale = %f",&scale)==1 && std::isfinite(scale))hudScale=std::clamp(scale,1.f,2.f);
            float threshold{};if(sscanf_s(line," stick_action_threshold = %f",&threshold)==1)stickActionThreshold=SanitizeStickActionThreshold(threshold);
        }
        fclose(file);
    }
    g_enabled.store(enabled == 1, std::memory_order_relaxed);
    g_hudScale.store(hudScale,std::memory_order_relaxed);
    g_stickActionThreshold.store(stickActionThreshold,std::memory_order_relaxed);
    g_activeUntil.store(0,std::memory_order_release);
    Log("[cyberfinger] profile %s (config/cyberfinger.ini; restart to change)\n", enabled == 1 ? "enabled" : "disabled");
}
bool Configured() { return g_enabled.load(std::memory_order_relaxed); }
float StickActionThreshold(){return g_stickActionThreshold.load(std::memory_order_relaxed);}
void NoteBindingActivity(bool active) { g_activeUntil.store(active?GetTickCount64()+500:0,std::memory_order_release); }
bool Enabled() { return Configured() && GetTickCount64()<g_activeUntil.load(std::memory_order_acquire); }
float HudScale(){return Enabled()?g_hudScale.load(std::memory_order_relaxed):1.f;}
}
