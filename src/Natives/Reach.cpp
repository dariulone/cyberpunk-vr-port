// The hand's reach in the game: the point the targeting looks at, and the interaction button the hand presses
// (Natives/Reach.hpp).
#include "Natives/Reach.hpp"
#include "Hooks/CyberFingerKatana.hpp"
#include "Camera/CameraState.hpp"
#include "Core/LiveControls.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Anim/HandGestures.hpp"
#include "Runtimes/FingerGrip.hpp"
#include "Core/VrCoreShared.hpp"
#include "Natives/VrMenu.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Overlay/ModUi.hpp"
#include <atomic>
#include "Anim/LeftHanded.hpp"
#include "Runtimes/CyberFingerInput.hpp"
#include "Runtimes/CyberFingerUi.hpp"
#include <RED4ext/Scripting/Utils.hpp>
#include <windows.h>
#include <algorithm>
#include <cmath>
#include <mutex>

extern "C" int CyberpunkVR_TwoHandActive;  // Anim/TwoHandGrip.cpp

namespace cvr::reach {
namespace {
std::mutex g_mutex;
std::atomic<uint64_t> g_scabbardLease{};  // expiry in ms, with physical side in bit zero
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

static std::atomic<uint64_t> offhandBladeLease{};
bool OffhandBlade(int side) {
    const auto lease=offhandBladeLease.load(std::memory_order_relaxed);
    return (lease >> 1) > GetTickCount64() && int(lease & 1) == side;
}

bool BracingScabbard(int side) {
    const auto lease = g_scabbardLease.load(std::memory_order_relaxed);
    return (lease >> 1) > GetTickCount64() && int(lease & 1) == side;
}

bool HandHolds(int side) {
    if (side < 0 || side > 1) return false;
    const int support = cvr::anim::WeaponInLeftHand() ? 1 : 0;
    return BracingScabbard(side) || OffhandBlade(side) || cvr::gestures::PortHolds(side) || (CyberpunkVR_TwoHandActive != 0 && side == support);
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

void VRCyberFingerEnabled(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    ++frame->code;
    if (out) *out = cvr::cyberfinger::Enabled();
}
// Returns -1 when gameplay or the tracked hand is unavailable, 0 when open,
// 1 for a closed fist. Only a drawn secondary sword reserves the hand.
void VRCyberFingerMeleeHand(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    int32_t side = -1;
    bool blade = false;
    RED4ext::GetParameter(frame,&side);
    RED4ext::GetParameter(frame,&blade);
    ++frame->code;
    if (out) *out = -1;
    const bool active = (side == 0 || side == 1) && cvr::cyberfinger::Enabled();
    cvr::reach::offhandBladeLease.store(active && blade ? ((GetTickCount64()+250)<<1)|uint64_t(side) : 0,
                                        std::memory_order_relaxed);
    if (!active || !out || g_menuModeValue != 0 || g_isInVehicle || g_uiPopupOpen.load() ||
        g_sceneTier.load() > 1 || cvr::vrui::CapturesInput() || cvr::modui::CapturesInput() ||
        cvr::vrmenu::CapturesButtons() || cvr::cyberfinger::InteractionActive() ||
        cvr::cyberfinger::ConversationActive() || cvr::reach::BracingScabbard(side) ||
        cvr::gestures::PortHolds(side) || CyberpunkVR_TwoHandActive || !cvr::fingergrip::BothGripsFresh()) return;
    float curl[5];
    if (cvr::fingergrip::TrackedCurl(side,curl))
        *out = curl[1] >= .55f && curl[2] >= .55f && curl[3] >= .55f && curl[4] >= .55f ? 1 : 0;
}
void VRCyberFingerScabbardHand(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    int32_t side = -1;
    RED4ext::GetParameter(frame, &side);
    ++frame->code;
    const bool tracked = cvr::cyberfinger::Enabled() && cvr::fingergrip::BothGripsFresh() &&
        g_menuModeValue == 0 && !cvr::vrui::CapturesInput() && !cvr::vrmenu::CapturesButtons();
    const uint64_t lease = tracked && (side == 0 || side == 1)
        ? ((GetTickCount64() + cvr::reach::kStaleMs) << 1) | static_cast<uint64_t>(side) : 0;
    cvr::reach::g_scabbardLease.store(lease, std::memory_order_relaxed);
    if (out) *out = tracked;
}

void VRCyberFingerFistBlock(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    bool fists=false;
    RED4ext::GetParameter(frame,&fists);
    ++frame->code;
    const bool block=cvr::cyberfinger::NoteFistsEquipped(fists && cvr::cyberfinger::Enabled());
    if (out) *out=block;
}

void VRCyberFingerKatanaGrip(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    int32_t side=-1, equipment=0, support=0;
    RED4ext::GetParameter(frame,&side);
    RED4ext::GetParameter(frame,&equipment);
    RED4ext::GetParameter(frame,&support);
    ++frame->code;
    const bool enabled=cvr::cyberfinger::Enabled();
    const bool tapped=side>=0 && side<2 && cvr::cyberfinger::katanaGrips[side].Offer(
        enabled && equipment>0 ? equipment : 0, enabled && support>0 ? support : 0, GetTickCount());
    if (out) *out=tapped;
}
void VRCyberFingerHolsterTypes(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    ++frame->code;
    if (out) *out = cvr::cyberfinger::Enabled() ? g_liveControls.xrCyberFingerHolsterTypes : cvr::cyberfinger::holsters::all;
}
void VRCyberFingerRevealModeNotice(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, void*, int64_t) {
    ++frame->code;
    cvr::cyberfinger::RevealModeNotice();
}
void VRCyberFingerShowModeNotice(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, void*, int64_t) {
    int32_t message = 0;
    RED4ext::GetParameter(frame, &message);
    ++frame->code;
    if (message >= 1 && message <= 3)
        cvr::cyberfinger::RevealModeNotice(static_cast<cvr::cyberfinger::ModeNotice>(message));
}
void VRCyberFingerRightHandSwords(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    frame->code++;
    if (out) *out = cvr::cyberfinger::Enabled() && g_liveControls.xrCyberFingerRightHandSwords != 0;
}
void VRAllowsGestureHolstering(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    bool melee = false;
    RED4ext::GetParameter(frame, &melee);
    ++frame->code;
    if (out) *out = !cvr::cyberfinger::Enabled() || (melee ? g_liveControls.xrCyberFingerMeleeGestureHolstering
                                                          : g_liveControls.xrCyberFingerGunGestureHolstering) != 0;
}
void VRCyberFingerSetOffensiveMode(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,void*,int64_t) {
    bool enabled = true;
    RED4ext::GetParameter(frame, &enabled);
    ++frame->code;
    cvr::cyberfinger::SetOffensiveMode(enabled);
}
void VRCyberFingerCommand(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    ++frame->code;
    using namespace cvr::cyberfinger;
    auto action = TakeCommand();
    if (action == Command::CloseShard &&
        (Context() != MenuContext::ShardReader || cvr::vrui::CapturesInput() || cvr::vrmenu::CapturesButtons()))
        action = Command::None;
    if (out) *out = static_cast<int32_t>(action);
}
void VRCyberFingerConversation(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    bool visible=false;RED4ext::GetParameter(frame,&visible);++frame->code;
    const int action=cvr::cyberfinger::ConversationTick(visible);if(out)*out=action;
}
void VRCyberFingerUiState(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,void*,int64_t) {
    int32_t context=0;bool interaction=false;
    RED4ext::GetParameter(frame,&context);RED4ext::GetParameter(frame,&interaction);++frame->code;
    using cvr::cyberfinger::MenuContext;
    const auto menu = context >= 0 && context <= static_cast<int>(MenuContext::ShardReader)
        ? static_cast<MenuContext>(context) : MenuContext::General;
    cvr::cyberfinger::UiTick(menu,interaction);
}
