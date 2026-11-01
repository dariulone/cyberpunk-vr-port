// The hand's reach in the game: the point the targeting looks at, and the interaction button the hand presses
// (Natives/Reach.hpp).
#include "Natives/Reach.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Anim/HandGestures.hpp"
#include "Anim/LeftHanded.hpp"
#include <RED4ext/Scripting/Utils.hpp>
#include <windows.h>
#include <algorithm>
#include <cmath>
#include <mutex>

extern "C" int CyberpunkVR_TwoHandActive;  // Anim/TwoHandGrip.cpp

namespace cvr::reach {
namespace {
std::mutex g_mutex;
float g_point[3]{};
ULONGLONG g_pointTick = 0;
ULONGLONG g_useUntil = 0;

bool SetAim(const RED4ext::Vector4& point) {
    std::lock_guard lock(g_mutex);
    if (point.W < 0.5f || !std::isfinite(point.X) || !std::isfinite(point.Y) || !std::isfinite(point.Z)) {
        g_pointTick = 0;
        return false;
    }
    g_point[0] = point.X;
    g_point[1] = point.Y;
    g_point[2] = point.Z;
    g_pointTick = GetTickCount64();
    return true;
}

bool SetUse(float seconds) {
    std::lock_guard lock(g_mutex);
    if (!(seconds > 0.0f)) {
        g_useUntil = 0;
        return false;
    }
    g_useUntil = GetTickCount64() + static_cast<ULONGLONG>(std::clamp(seconds, 0.05f, 2.0f) * 1000.0f);
    return true;
}
}  // namespace

bool AimPoint(float point[3]) {
    std::lock_guard lock(g_mutex);
    if (!g_pointTick || GetTickCount64() - g_pointTick > kStaleMs) return false;
    std::copy(g_point, g_point + 3, point);
    return true;
}

bool UseHeld() {
    std::lock_guard lock(g_mutex);
    return g_useUntil && GetTickCount64() < g_useUntil;
}

bool HandHolds(int side) {
    if (side < 0 || side > 1) return false;
    const int support = cvr::anim::WeaponInLeftHand() ? 1 : 0;
    return cvr::gestures::PortHolds(side) || (CyberpunkVR_TwoHandActive != 0 && side == support);
}
}  // namespace cvr::reach

void VRReachAim(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    RED4ext::Vector4 point{};
    RED4ext::GetParameter(frame, &point);
    ++frame->code;
    const bool aimed = cvr::reach::SetAim(point);
    if (out) *out = aimed;
}

void VRReachUse(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    float seconds = 0.0f;
    RED4ext::GetParameter(frame, &seconds);
    ++frame->code;
    const bool held = cvr::reach::SetUse(seconds);
    if (out) *out = held;
}

void VRHandHolds(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    int32_t side = 0;
    RED4ext::GetParameter(frame, &side);
    ++frame->code;
    const bool holds = cvr::reach::HandHolds(side);
    if (out) *out = holds;
}
