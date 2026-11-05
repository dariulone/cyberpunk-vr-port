#include "Runtimes/InteractionDepth.hpp"
#include "Natives/NativeFunctions.hpp"
#include <RED4ext/RED4ext.hpp>
#include <Windows.h>
#include <atomic>
#include <cmath>

// 1 = place the interaction prompt at the depth of its object (xr_interaction_anchor). See the header.
extern "C" __declspec(dllexport) int CyberpunkVR_InteractionAnchor = 0;

namespace {
std::atomic<float>    g_depth{0.0f};
std::atomic<uint64_t> g_stampMs{0};
constexpr uint64_t kFreshMs = 500;   // the module publishes every ~33 ms; a gap this long means it stopped
}

namespace cvr::interaction {
float TargetDepth(uint64_t nowMs) {
    const uint64_t stamp = g_stampMs.load(std::memory_order_acquire);
    if (!stamp || nowMs - stamp > kFreshMs) return 0.0f;
    return g_depth.load(std::memory_order_relaxed);
}
}

// SetVRInteractionDepth(depth: Float) -- metres along the camera's forward axis; 0 or less = no target.
void SetVRInteractionDepth(RED4ext::IScriptable*, RED4ext::CStackFrame* aFrame, void*, int64_t) {
    float depth = 0.0f;
    RED4ext::GetParameter(aFrame, &depth);
    aFrame->code++;
    if (!std::isfinite(depth) || depth < 0.0f) depth = 0.0f;
    g_depth.store(depth, std::memory_order_relaxed);
    g_stampMs.store(GetTickCount64(), std::memory_order_release);
}
