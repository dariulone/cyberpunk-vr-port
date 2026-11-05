#include "Runtimes/HudClearance.hpp"
#include "Natives/NativeFunctions.hpp"
#include <RED4ext/RED4ext.hpp>
#include <Windows.h>
#include <atomic>
#include <cmath>

// 1 = keep the HUD panels in front of the world (xr_hud_clearance). See Runtimes/HudClearance.hpp.
extern "C" __declspec(dllexport) int CyberpunkVR_HudClearance = 0;
// How much a panel grows as it comes in (xr_hud_clearance_grow), 0..1. See GrowFactor.
extern "C" __declspec(dllexport) float CyberpunkVR_HudClearanceGrow = 0.5f;
// 1 = a tracked hand in front of a HUD panel pushes it in too (xr_hud_clearance_hands). See HandFront.
extern "C" __declspec(dllexport) int CyberpunkVR_HudClearanceHands = 0;
// How far a hand pushes the HUD out (xr_hud_clearance_hands_scale): its scale while a hand is in front, 1..3.
extern "C" __declspec(dllexport) float CyberpunkVR_HudClearanceHandsScale = cvr::hud::clearance::kHandOutScale;

namespace {
std::atomic<float>    g_clearance{0.0f};
std::atomic<uint64_t> g_stampMs{0};
constexpr uint64_t kFreshMs = 500;   // published every ~33 ms; a gap this long means the module stopped
}

namespace cvr::hud::clearance {
float Published(uint64_t nowMs) {
    const uint64_t stamp = g_stampMs.load(std::memory_order_acquire);
    if (!stamp || nowMs - stamp > kFreshMs) return 0.0f;
    return g_clearance.load(std::memory_order_relaxed);
}
}

namespace cvr::hud::clearance {
float GrowFactor(float configured, float distance, float grow) {
    if (!(grow > 0.0f) || !(distance > 0.0f) || !(configured > distance)) return 1.0f;
    if (grow > 1.0f) grow = 1.0f;
    return std::pow(configured / distance, grow);
}
}

// SetVRHudClearance(depth: Float) -- nearest depth in metres along the camera's forward axis; 99 = open.
void SetVRHudClearance(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float depth = 0.0f;
    RED4ext::GetParameter(aFrame, &depth);
    aFrame->code++;
    if (!std::isfinite(depth) || depth < 0.0f) depth = 0.0f;
    g_clearance.store(depth, std::memory_order_relaxed);
    g_stampMs.store(GetTickCount64(), std::memory_order_release);
}
