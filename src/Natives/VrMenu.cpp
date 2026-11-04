// The VR menu's natives: the summon gesture, the two pointers, and the input the menu holds back.
//
// The menu itself is the game's own UI on a plane in the world (vrport\ui\vr_menu.ent, built and run by
// mods/cet/CyberpunkVRPort_Menu). What script cannot see is the hands: their tracking history for the
// gesture, and where the rendered index fingertip and the hand's pointing ray are this frame. These
// hand that over, and the Lua decides what a touch or a click means.
//
// THE GESTURE is Sword Art Online's swipe, made deliberate: the right hand, GRIP HELD, swept straight
// down (an open hand dropping after an action is not a swipe). A grip that holds something is not held for
// the swipe, and stays out of it until it is released (Natives/MenuSwipe.hpp) -- the steering wheel (turning it
// sweeps that hand down), a ladder (climbing is the same pull down, and the last pull goes on after the ladder
// lets the player off), a thing the fingers are closed on (Anim/GraspPose: the lobby's items) -- and the setting
// xr_menu_gesture turns the swipe off (L3 + R3 still opens the menu). Read in TRACKING space, from the same published hand frame the melee reads, so a turn
// of the body or the camera never makes a swipe. Fired once; the Lua checks whether the moment allows
// a menu (no weapon in hand, no scene) before it opens one.
//
// THE POINTERS are measured on the hand the player SEES: the solved rig's snapshot (g_VRFKSnap*, filled
// while diag capture is on -- the hand collision runs it, and the menu turns it on while open), put in
// the world by the same model-to-world pair VRPalmWorldPos uses. The fingertip is the end of
// RightHandIndex3 / LeftHandIndex3; the ray starts at the index knuckle and runs along the HAND bone,
// which a trigger press does not bend -- so clicking does not move the ray off what it points at.

#include "Natives/NativeFunctions.hpp"
#include "Audio/PortSound.hpp"
#include "Natives/VrMenu.hpp"
#include "Natives/MenuSwipe.hpp"
#include "Camera/CameraState.hpp"   // InBraindance
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/GraspPose.hpp"
#include "Anim/WheelGrab.hpp"
#include "Hooks/LadderInput.hpp"
#include "Core/LiveControls.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Overlay/UiRec.hpp"
#include "Runtimes/FingerGrip.hpp"
#include "Runtimes/OpenXRManager.hpp"

#include <RED4ext/Scripting/Utils.hpp>

#include <windows.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <string>
#include <vector>

namespace cvr::vrmenu {
namespace {
std::atomic<float> g_blink{0.0f};
std::atomic<ULONGLONG> g_blinkUpdated{0};
}
float BlinkClosure() {
    if (GetTickCount64() - g_blinkUpdated.load(std::memory_order_acquire) > 2000) return 0.0f;
    return g_blink.load(std::memory_order_relaxed);
}
namespace {
std::atomic<bool> g_capture{false};
// The wheel script's last call (it calls every frame it can open the wheel), and the hold as the input
// merge last decided it: 0 none, 1 pressed and undecided, 2 the wheel's hold.
std::atomic<ULONGLONG> g_wheelBeat{0};
std::atomic<int> g_wheelHold{0};
}
bool CapturesButtons() { return g_capture.load(std::memory_order_relaxed); }
bool WheelArmed(uint64_t now) {
    const ULONGLONG beat = g_wheelBeat.load(std::memory_order_relaxed);
    return beat != 0 && now >= beat && now - beat < 500;
}
void PublishWheelHold(bool pending, bool held) {
    g_wheelHold.store(held ? 2 : (pending ? 1 : 0), std::memory_order_relaxed);
}
}

namespace {
// Bone indices of the player rig (the snapshot is indexed like it; see the hand collision's capsules.lua).
constexpr int kHand[2] = {23, 24};      // LeftHand, RightHand
constexpr int kIndex1[2] = {46, 51};    // LeftHandIndex1, RightHandIndex1
constexpr int kIndex3[2] = {65, 69};    // LeftHandIndex3, RightHandIndex3
// From the bone to the end of the finger, along the bone's X: the last capsule's length and radius.
constexpr float kTip = 0.024f;

cvr::vrmenu::SwipeDetector g_swipe;
uint64_t g_lastSequence = 0;

bool ModelToWorldFrame(float* m2w) {
    if (!g_VRPalmModelValid || !g_VRViewWorldValid) return false;
    const float camRot[4] = {g_VRCamModelRot[0], g_VRCamModelRot[1], g_VRCamModelRot[2], g_VRCamModelRot[3]};
    const float viewRot[4] = {g_VRViewWorldRot[0], g_VRViewWorldRot[1], g_VRViewWorldRot[2], g_VRViewWorldRot[3]};
    float camInv[4];
    VRIK_QuatConj(camRot, camInv);
    VRIK_QuatMul(viewRot, camInv, m2w);
    VRIK_QuatNorm(m2w);
    return true;
}

void ModelToWorld(const float* m2w, const float* model, float* world) {
    const float rel[3] = {model[0] - g_VRCamModelPos[0], model[1] - g_VRCamModelPos[1], model[2] - g_VRCamModelPos[2]};
    float w[3];
    VRIK_QuatRotateVec(m2w, rel, w);
    for (int k = 0; k < 3; ++k) world[k] = g_VRViewWorldPos[k] + w[k];
}

// A bone of the snapshot in the world: position, and one of its axes (bone-local `axis`).
bool BoneWorld(const float* m2w, int bone, const float* offset, const float* axis, float* pos, float* dir) {
    if (bone < 0 || bone >= g_VRFKSnapCount) return false;
    const float p[3] = {g_VRFKSnapPos[bone][0], g_VRFKSnapPos[bone][1], g_VRFKSnapPos[bone][2]};
    const float q[4] = {g_VRFKSnapRot[bone][0], g_VRFKSnapRot[bone][1], g_VRFKSnapRot[bone][2], g_VRFKSnapRot[bone][3]};
    float model[3] = {p[0], p[1], p[2]};
    if (offset) {
        float o[3];
        VRIK_QuatRotateVec(q, offset, o);
        for (int k = 0; k < 3; ++k) model[k] += o[k];
    }
    ModelToWorld(m2w, model, pos);
    if (axis) {
        float a[3];
        VRIK_QuatRotateVec(q, axis, a);
        VRIK_QuatRotateVec(m2w, a, dir);
    }
    return true;
}

// THE POINTER LEAVES THE HAND THE PLAYER HOLDS, not the drawn one. With the controller beyond the arm's reach --
// pointing at a panel a metre away, the arm out -- the solve stops the drawn hand short, and where it stops moves with
// the shoulders: the body's yaw steps under a turning head carried the pointer along (measured in the lobby: the ray's
// origin 12 cm in three steps over a 35 degree head turn, the hand still in the room). The controller's own wrist
// (g_VRIKHandAim, before the reach, the stops and the blends) is where the hand is; the index finger's base and the
// hand's axis are put on it the way the snapshot has them on the drawn hand. Within reach the two are the same wrist.
bool AimWorld(const float* m2w, int side, const float* along, float* origin, float* dir) {
    const float* aim = g_VRIKHandAim[side ? 0 : 1];
    const int hand = kHand[side], index = kIndex1[side];
    if (aim[7] == 0.0f || hand >= g_VRFKSnapCount || index >= g_VRFKSnapCount) return false;
    const float handQ[4] = {g_VRFKSnapRot[hand][0], g_VRFKSnapRot[hand][1], g_VRFKSnapRot[hand][2], g_VRFKSnapRot[hand][3]};
    const float rel[3] = {g_VRFKSnapPos[index][0] - g_VRFKSnapPos[hand][0], g_VRFKSnapPos[index][1] - g_VRFKSnapPos[hand][1],
                          g_VRFKSnapPos[index][2] - g_VRFKSnapPos[hand][2]};
    float handInv[4], local[3];
    VRIK_QuatConj(handQ, handInv);
    VRIK_QuatRotateVec(handInv, rel, local);
    float q[4] = {aim[3], aim[4], aim[5], aim[6]};
    VRIK_QuatNorm(q);
    float o[3], a[3];
    VRIK_QuatRotateVec(q, local, o);
    const float model[3] = {aim[0] + o[0], aim[1] + o[1], aim[2] + o[2]};
    ModelToWorld(m2w, model, origin);
    VRIK_QuatRotateVec(q, along, a);
    VRIK_QuatRotateVec(m2w, a, dir);
    return true;
}

float Dot(const float* a, const float* b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; }
} // namespace

bool cvr::vrmenu::FingerWorld(int side, float* world) {
    VRControllerState input{};
    if (side < 0 || side > 1 || !OpenXRManager::Get().GetControllerState(&input) ||
        !(side ? input.rightHandValid : input.leftHandValid)) return false;
    float m2w[4];
    const float end[3] = {side ? -kTip : kTip, 0, 0};
    return ModelToWorldFrame(m2w) && BoneWorld(m2w, kIndex3[side], end, nullptr, world, nullptr);
}

// VRMenuGesture() -> array<Vector4>: empty, or once per swipe
//   [0] (1, sequence, 0, 0)
//   [1] where the swipe started, relative to the head in its heading frame: (right, up, forward, 1) metres
//   [2] where it ended, the same way
// or, once per L3 + R3 hold (Overlay/VrOverlay.cpp), the single item (2, 0, 0, 0): open or close the menu
// in front of the head.
void VRMenuGesture(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, RED4ext::DynArray<RED4ext::Vector4>* out, int64_t) {
    stack->code++;
    if (!out) return;
    out->Clear();
    // NOT IN A BRAINDANCE, by either way in: the grips are its LB and RB there (Hooks/XInput.cpp), and an L3 + R3
    // hold is dropped rather than kept for after it.
    if (InBraindance()) {
        cvr::vrui::ConsumeMenuSummon();
        g_swipe.Clear();
        return;
    }
    if (cvr::vrui::ConsumeMenuSummon()) {
        out->PushBack({2.0f, 0.0f, 0.0f, 0.0f});
        return;
    }
    OpenXRHeadPose head{}, hands[2]{};
    uint64_t sequence{}, stamp{};
    VRControllerState input{};
    auto& xr = OpenXRManager::Get();
    // With the grip read from the fingers a hand closed on the controller grips all the time, and any move down would be
    // a swipe: the menu then opens with L3 + R3 only.
    if (!g_liveControls.xrMenuGesture || cvr::fingergrip::GripFromFingers() || !xr.GetGestureHandFrame(&head, hands, &sequence, &stamp, false) || !hands[1].valid ||
        !xr.GetControllerState(&input) || !input.rightHandValid) {
        g_swipe.Clear();
        return;
    }
    if (sequence == g_lastSequence) return;
    g_lastSequence = sequence;
    const float headQ[4] = {head.oriX, head.oriY, head.oriZ, head.oriW};
    const float local[3] = {hands[1].posX, hands[1].posY, hands[1].posZ};
    float p[3];
    VRIK_QuatRotateVec(headQ, local, p);
    const double t = double(stamp) * 1e-6;
    // The right hand's grip on something is not a swipe: the steering wheel (owned, or still easing onto it), a
    // ladder's rung, a thing in its grasp.
    const bool holding = (cvr::anim::WheelControlState() & 1) != 0 ||
                         cvr::anim::g_wheelBlendRight.load(std::memory_order_relaxed) > 0.0f ||
                         cvr::ladder::Active() || cvr::anim::GraspHolding(1);
    const float hand[3] = {p[0] + head.posX, p[1] + head.posY, p[2] + head.posZ};
    cvr::vrmenu::SwipeSample start{}, end{};
    if (!g_swipe.Push(t, hand, input.rightGrip > 0.70f, holding, &start, &end)) return;

    // The head's heading frame in tracking space (OpenXR: Y up, -Z ahead).
    const float ahead[3] = {0.0f, 0.0f, -1.0f};
    float f[3];
    VRIK_QuatRotateVec(headQ, ahead, f);
    f[1] = 0.0f;
    const float n = std::sqrt(f[0] * f[0] + f[2] * f[2]);
    if (n < 1e-4f) return;
    f[0] /= n;
    f[2] /= n;
    const float r[3] = {-f[2], 0.0f, f[0]};
    auto relative = [&](const float* q) {
        const float d[3] = {q[0] - head.posX, q[1] - head.posY, q[2] - head.posZ};
        return RED4ext::Vector4{Dot(d, r), d[1], Dot(d, f), 1.0f};
    };
    out->PushBack({1.0f, float(sequence & 0xffffffu), 0.0f, 0.0f});
    out->PushBack(relative(start.p));
    out->PushBack(relative(end.p));
}

// VRMenuPointer(center, right, up, size) -> array<Vector4>, the menu plane given in the world (unit
// right and up axes, size.X x size.Y metres; its front faces right x up), for each hand (0 left, 1 right):
//   [4h+0] fingertip: (u, v, distance in front of the plane, valid)   u, v in 0..1 across / down the plane
//   [4h+1] ray:       (u, v, length to the plane, hits)               hits = 1 only when it meets the front
//   [4h+2] ray origin in the world (x, y, z, trigger 0..1)
//   [4h+3] ray hit point in the world (x, y, z, grip 0..1)
void VRMenuPointer(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, RED4ext::DynArray<RED4ext::Vector4>* out, int64_t) {
    RED4ext::Vector4 center{}, right{}, up{}, size{};
    RED4ext::GetParameter(stack, &center);
    RED4ext::GetParameter(stack, &right);
    RED4ext::GetParameter(stack, &up);
    RED4ext::GetParameter(stack, &size);
    stack->code++;
    if (!out) return;
    out->Clear();
    const float c[3] = {center.X, center.Y, center.Z}, rx[3] = {right.X, right.Y, right.Z}, uy[3] = {up.X, up.Y, up.Z};
    const float nz[3] = {rx[1] * uy[2] - rx[2] * uy[1], rx[2] * uy[0] - rx[0] * uy[2], rx[0] * uy[1] - rx[1] * uy[0]};
    const float w = size.X > 0.01f ? size.X : 1.0f, h = size.Y > 0.01f ? size.Y : 1.0f;
    VRControllerState input{};
    const bool haveInput = OpenXRManager::Get().GetControllerState(&input);
    float m2w[4];
    const bool frame = ModelToWorldFrame(m2w) && g_VRFKSnapCount > kIndex3[1];
    for (int side = 0; side < 2; ++side) {
        RED4ext::Vector4 tip{-1, -1, 0, 0}, ray{-1, -1, 0, 0}, origin{0, 0, 0, 0}, hit{0, 0, 0, 0};
        const float trigger = haveInput ? (side ? input.rightTrigger : input.leftTrigger) : 0.0f;
        const float grip = haveInput ? (side ? input.rightGrip : input.leftGrip) : 0.0f;
        const float along[3] = {side ? -1.0f : 1.0f, 0.0f, 0.0f};
        const float end[3] = {along[0] * kTip, 0.0f, 0.0f};
        float p[3], d[3];
        if (frame && BoneWorld(m2w, kIndex3[side], end, nullptr, p, nullptr)) {
            const float l[3] = {p[0] - c[0], p[1] - c[1], p[2] - c[2]};
            tip = {Dot(l, rx) / w + 0.5f, 0.5f - Dot(l, uy) / h, Dot(l, nz), 1.0f};
        }
        float o[3];
        if (frame && AimWorld(m2w, side, along, o, d)) {
            origin = {o[0], o[1], o[2], trigger};
            const float l[3] = {o[0] - c[0], o[1] - c[1], o[2] - c[2]};
            const float front = Dot(l, nz), into = Dot(d, nz);
            if (front > 0.0f && into < -1e-3f) {
                const float t = -front / into;
                const float q[3] = {o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t};
                const float lq[3] = {q[0] - c[0], q[1] - c[1], q[2] - c[2]};
                ray = {Dot(lq, rx) / w + 0.5f, 0.5f - Dot(lq, uy) / h, t, t < 6.0f ? 1.0f : 0.0f};
                hit = {q[0], q[1], q[2], grip};
            } else {
                hit = {o[0] + d[0] * 2.0f, o[1] + d[1] * 2.0f, o[2] + d[2] * 2.0f, grip};
            }
        }
        out->PushBack(tip);
        out->PushBack(ray);
        out->PushBack(origin);
        out->PushBack(hit);
    }
}

// VRMenuOpenSettings(): the port's own settings panel, as F10 opens it.
void VRMenuOpenSettings(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    stack->code++;
    const bool open = !cvr::vrui::Visible();
    if (open) cvr::vrui::Toggle();
    if (out) *out = open;
}

// VRGripFromFingers() -> Bool: the grip is read from the fingers (xr_grip_from_fingers), so a hand closed on the controller
// reads as gripping before it does anything; what waits for a grip to close asks here.
void VRGripFromFingers(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    stack->code++;
    if (out) *out = cvr::fingergrip::GripFromFingers();
}

// VRMenuCapture(open): while the menu is open the triggers, grips and face buttons stop reaching the game.
void VRMenuCapture(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    bool open = false;
    RED4ext::GetParameter(stack, &open);
    stack->code++;
    cvr::vrmenu::g_capture.store(open, std::memory_order_relaxed);
    if (out) *out = open;
}

// VRWeaponWheel(armed) -> Int32: called every frame by the port's weapon wheel. Armed, it takes the hold of
// Y from the game (Hooks/WeaponWheelHold.hpp) for the next half second; the answer is the hold as the input
// merge last decided it: 0 none, 1 pressed and undecided (time to get the wheel ready), 2 held (the wheel is
// up). Always 0 with the setting xr_vr_weapon_wheel off.
void VRWeaponWheel(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, int32_t* out, int64_t) {
    bool armed = false;
    RED4ext::GetParameter(stack, &armed);
    stack->code++;
    const bool on = armed && g_liveControls.xrVrWeaponWheel != 0;
    cvr::vrmenu::g_wheelBeat.store(on ? GetTickCount64() : 0, std::memory_order_relaxed);
    if (out) *out = on ? cvr::vrmenu::g_wheelHold.load(std::memory_order_relaxed) : 0;
}

void VRMenuBlink(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    float closure = 0.0f;
    RED4ext::GetParameter(stack, &closure);
    stack->code++;
    cvr::vrmenu::g_blink.store(std::isfinite(closure) ? std::clamp(closure, 0.0f, 1.0f) : 0.0f,
                              std::memory_order_relaxed);
    cvr::vrmenu::g_blinkUpdated.store(GetTickCount64(), std::memory_order_release);
    if (out) *out = true;
}

// VRSettingsPage(page) -> String: the port's settings page as the VR menu draws it (Overlay/UiRec.hpp),
// the latest one recorded -- "" until the first -- and a request for a fresh one, recorded on the next
// present. 0 GENERAL, 1 FRAMEGEN, 2 HUD, 3 CONTROLS, 4 STEREO, 5 AVATAR, 6 OVERLAY.
void VRSettingsPage(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, RED4ext::CString* out, int64_t) {
    int32_t page = 0;
    RED4ext::GetParameter(stack, &page);
    stack->code++;
    cvr::uirec::Request(page);
    if (out) *out = RED4ext::CString(cvr::uirec::Latest(page).c_str());
}

// VRSettingsEdit(page, id, value): what the VR menu did to the item `id` of that page -- a checkbox's
// 0 / 1, a slider's value, a combo's or tab's index, any value for a button. Applied through the panel
// code on the next present, persisted the way the panel persists.
void VRSettingsEdit(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    int32_t page = 0;
    RED4ext::CString id;
    float value = 0.0f;
    RED4ext::GetParameter(stack, &page);
    RED4ext::GetParameter(stack, &id);
    RED4ext::GetParameter(stack, &value);
    stack->code++;
    cvr::uirec::QueueEdit(page, id.c_str(), value);
    if (out) *out = true;
}

// THE MENU'S OWN SOUNDS. The game's UI events are Wwise random containers -- one event, a different
// variant each play -- so the variant the player picked can only be had as its own file: the mod ships
// it and the port plays it (Audio/PortSound.hpp), at the volume the Lua works out from the game's master
// and effects sliders.
//
// VRMenuPlayWav(file, volume) -> Bool: plays a sound file once (a WAV or an Ogg Vorbis, relative to the
// plugin's Lua mods folder), at `volume` (0..1). False when the file or the audio device is not there --
// the Lua then falls back to the game's event.
void VRMenuPlayWav(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    RED4ext::CString file;
    float volume = 1.0f;
    RED4ext::GetParameter(stack, &file);
    RED4ext::GetParameter(stack, &volume);
    stack->code++;
    const bool played = cvr::sound::PlayOnce(file.c_str(), volume);
    if (out) *out = played;
}

// VRSoundChannel(channel, file, volume, lowpass, loopFrom, loopTo, crossfade, fade) -> Bool: what the named channel plays
// now, reached over `fade` seconds (Audio/PortSound.hpp Channel); file "" ends it.
void VRSoundChannel(RED4ext::IScriptable*, RED4ext::CStackFrame* stack, bool* out, int64_t) {
    RED4ext::CString channel, file;
    cvr::sound::Sound sound;
    float fade = 0.0f;
    RED4ext::GetParameter(stack, &channel);
    RED4ext::GetParameter(stack, &file);
    RED4ext::GetParameter(stack, &sound.volume);
    RED4ext::GetParameter(stack, &sound.lowpass);
    RED4ext::GetParameter(stack, &sound.loopFrom);
    RED4ext::GetParameter(stack, &sound.loopTo);
    RED4ext::GetParameter(stack, &sound.crossfade);
    RED4ext::GetParameter(stack, &fade);
    stack->code++;
    sound.file = file.c_str();
    const bool ok = cvr::sound::Channel(channel.c_str(), sound, std::isfinite(fade) ? fade : 0.0f);
    if (out) *out = ok;
}
