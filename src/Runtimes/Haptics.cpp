// Haptics -- see Runtimes/Haptics.hpp for the shape of it.
//
// WHY THE FRAME THREAD PLAYS EVERYTHING. A round is reported from whichever worker ran the shot, a
// block from the script thread, a belt touch from CET. xrApplyHapticFeedback is not documented as
// needing external synchronisation, but it is an action call, and this port's runtimes (Virtual
// Desktop, SteamVR, Link) only ever see action calls from the thread that syncs them. One frame of
// latency on a tap is below what a hand can tell.
//
// WHY ENVELOPES AND NOT FREQUENCIES. The controllers this port runs on answer frequency very
// differently -- Index takes it, most Touch controllers resonate in one band and go weak outside it --
// so the frequency stays XR_FREQUENCY_UNSPECIFIED and the weapons differ in strength, length and
// decay instead, which every motor can render.
//
// ONE ENVELOPE PER HAND. A new event replaces what is playing unless that has a higher priority and
// is still running: full-auto restarts the rifle tap every round, and a belt tick never cuts a
// shotgun's tail.

#include "Runtimes/Haptics.hpp"
#include "Anim/LeftHanded.hpp"

#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/Psvr2HapticChannel.hpp"

#include <windows.h>
#include <algorithm>
#include <cstdint>
#include <mutex>

extern "C" {
// Master switch (ini xr_haptics) and the strength of everything (ini xr_haptic_scale, 0..2).
__declspec(dllexport) int   CyberpunkVR_Haptics = 1;
__declspec(dllexport) float CyberpunkVR_HapticScale = 1.0f;
// THE SECOND HAND IN A TWO-HANDED GRIP feels the round as the trigger hand does: in 2H the weapon is
// in both hands, held in one it is in one (2026-10-17). This is its share; 1 = the same.
__declspec(dllexport) float CyberpunkVR_HapticSupportMul = 1.0f;
// THE PROFILES, live-tunable: per profile three segments of (amplitude 0..1, milliseconds); an
// amplitude of 0 is a gap. Rows: 0 handgun, 1 rifle/SMG/LMG, 2 shotgun, 3 sniper/precision,
// 4 belt touch, 5 melee block, 6 melee parry, 7 bullet block, 8 bullet deflect, 9 grenade pin (the
// pulling hand), 10 grenade pin (the holding hand), 11 VR menu click, 12 VR menu hover, 13 VR menu
// slider step, 14 melee hit, 15 melee swing. A segment under ~12 ms can fall between two XR frames and never
// play, so none is shorter.
__declspec(dllexport) float CyberpunkVR_HapticProfiles[16][6] = {
    { 0.85f, 28.0f,  0.25f, 30.0f,  0.00f,  0.0f },   // handgun: one crisp snap
    { 0.60f, 22.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // rifle: short, so full-auto stays a rattle of rounds
    { 1.00f, 55.0f,  0.55f, 70.0f,  0.20f, 60.0f },   // shotgun: a heavy thump that rolls off
    { 1.00f, 45.0f,  0.60f, 60.0f,  0.25f, 90.0f },   // sniper: the hardest kick and the longest tail
    { 0.30f, 14.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // belt: a light tick
    { 0.90f, 40.0f,  0.40f, 50.0f,  0.00f,  0.0f },   // melee block: a dull thud
    { 1.00f, 20.0f,  0.00f, 35.0f,  0.75f, 25.0f },   // melee parry: a double "clang"
    { 0.65f, 30.0f,  0.20f, 30.0f,  0.00f,  0.0f },   // bullet block: a short knock
    { 1.00f, 15.0f,  0.45f, 25.0f,  0.00f,  0.0f },   // bullet deflect: a sharp ping
    { 1.00f, 18.0f,  0.35f, 30.0f,  0.00f,  0.0f },   // grenade pin: the snap of the ring coming free
    { 0.45f, 25.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // grenade pin, felt through the grenade
    { 0.80f, 16.0f,  0.00f, 12.0f,  0.35f, 12.0f },   // menu click: a press and its release
    { 0.15f, 12.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // menu hover: a faint tick on entering a control
    { 0.22f, 12.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // menu slider: one tick per step crossed
    { 1.00f, 35.0f,  0.50f, 55.0f,  0.00f,  0.0f },   // melee hit: the blow landing, then its weight
    { 0.50f, 45.0f,  0.00f,  0.0f,  0.00f,  0.0f },   // melee swing: the weapon cutting the air
};
// The off hand is on the weapon (src/Anim/TwoHandGrip.cpp).
__declspec(dllexport) extern int CyberpunkVR_TwoHandActive;
// [0] events accepted, [1] segments applied left, [2] right, [3] last xrApplyHapticFeedback result,
// [4] last profile row, [5] dropped (switched off, no action, no session), [6] shots, [7] events.
__declspec(dllexport) int   CyberpunkVR_HapticStats[8] = {};
}

namespace cvr::haptics {
namespace {

constexpr int kSegs = 3;
constexpr int kProfileCount = 16;

struct Envelope {
    float   amp[kSegs] = {};
    int64_t start[kSegs] = {};   // QPC
    int64_t end[kSegs] = {};
    int     next = kSegs;        // first segment not yet started; kSegs = idle
    int     priority = 0;
    int64_t done = 0;            // QPC when the last segment ends
};

std::mutex g_lock;
Envelope   g_hand[2];            // 0 left, 1 right

int64_t Qpc() { LARGE_INTEGER c{}; QueryPerformanceCounter(&c); return c.QuadPart; }
int64_t QpcFreq() {
    static const int64_t f = [] { LARGE_INTEGER q{}; QueryPerformanceFrequency(&q); return q.QuadPart; }();
    return f;
}
int64_t MsToQpc(float ms) { return static_cast<int64_t>(static_cast<double>(ms) * 0.001 * static_cast<double>(QpcFreq())); }

int PriorityOf(int row) {
    switch (row) {
        case 4: case 11: case 12: case 13: case 15: return 1;   // belt, menu, a swing
        case 0: case 1: case 2: case 3: return 2;   // shots
        default: return 3;           // block / parry / deflect / pin
    }
}

void Queue(int row, int handMask, float mul) {
    if (row < 0 || row >= kProfileCount) return;
    if (!CyberpunkVR_Haptics) { ++CyberpunkVR_HapticStats[5]; return; }
    const float scale = std::clamp(CyberpunkVR_HapticScale, 0.0f, 2.0f) * mul;
    if (scale <= 0.0f) return;
    const float* p = CyberpunkVR_HapticProfiles[row];
    const int64_t now = Qpc();
    const int pri = PriorityOf(row);
    std::lock_guard lock(g_lock);
    for (int h = 0; h < 2; ++h) {
        if (!(handMask & (h == 0 ? kLeft : kRight))) continue;
        Envelope& e = g_hand[h];
        if (now < e.done && e.priority > pri) continue;
        Envelope n{};
        int64_t t = now;
        for (int s = 0; s < kSegs; ++s) {
            const float ms = std::clamp(p[s * 2 + 1], 0.0f, 500.0f);
            n.amp[s] = std::clamp(p[s * 2] * scale, 0.0f, 1.0f);
            n.start[s] = t;
            n.end[s] = t + MsToQpc(ms);
            t = n.end[s];
        }
        n.next = 0;
        n.priority = pri;
        n.done = t;
        e = n;
    }
    ++CyberpunkVR_HapticStats[0];
    CyberpunkVR_HapticStats[4] = row;
}

}  // namespace

void Shot(int weaponClass, bool supportHandOnGun, int hand) {
    if (weaponClass < 1 || weaponClass > 4) return;   // melee and "unknown" fire no rounds of their own
    ++CyberpunkVR_HapticStats[6];
    const int row = weaponClass - 1;
    const int trigger = (hand == 0) ? kLeft : kRight;
    Queue(row, trigger, 1.0f);
    if (supportHandOnGun) {
        Queue(row, (trigger == kLeft) ? kRight : kLeft, std::clamp(CyberpunkVR_HapticSupportMul, 0.0f, 1.0f));
    }
}

void Play(int event, int handMask) {
    int row = -1;
    switch (event) {
        case kBeltTouch:     row = 4; break;
        case kGrenadePin:    row = 9; break;
        case kGrenadePinHold: row = 10; break;
        case kMeleeBlock:    row = 5; break;
        case kMeleeParry:    row = 6; break;
        case kBulletBlock:   row = 7; break;
        case kBulletDeflect: row = 8; break;
        case kUiClick:       row = 11; break;
        case kUiHover:       row = 12; break;
        case kUiStep:        row = 13; break;
        case kMeleeHit:      row = 14; break;
        case kMeleeSwing:    row = 15; break;
        default: return;
    }
    // A BLOCK IS TAKEN BY BOTH HANDS when both are on the weapon -- the same test the two-handed melee
    // damage uses (CyberpunkVR_TwoHandActive) -- so the support hand feels it too: the left one, or the right
    // one with the weapon in the left hand.
    if (((row >= 5 && row <= 8) || row == 14 || row == 15) && CyberpunkVR_TwoHandActive != 0)
        handMask |= cvr::anim::WeaponInLeftHand() ? kRight : kLeft;
    ++CyberpunkVR_HapticStats[7];
    Queue(row, handMask, 1.0f);
}

void Tick() {
    struct Due { float amp; float sec; };
    Due due[2] = {};
    {
        std::lock_guard lock(g_lock);
        const int64_t now = Qpc();
        for (int h = 0; h < 2; ++h) {
            Envelope& e = g_hand[h];
            int started = -1;
            while (e.next < kSegs && e.start[e.next] <= now) started = e.next++;
            if (started < 0 || e.amp[started] <= 0.0f) continue;
            const double left = static_cast<double>(e.end[started] - now) / static_cast<double>(QpcFreq());
            if (left > 0.003) due[h] = { e.amp[started], static_cast<float>(left) };
        }
    }
    // PSVR2: the Sense controllers' actuators belong to the PlayStation VR2 Toolkit bridge, which plays what the
    // port sends over its own channel (Runtimes/Psvr2HapticChannel.hpp); there is no OpenXR vibration to drive.
    const bool psvr2 = OpenXRManager::Get().IsRuntimePsvr2();
    for (int h = 0; h < 2; ++h) {
        if (due[h].amp <= 0.0f) continue;
        if (psvr2) {
            cvr::psvr2::Haptics().Pulse(h, due[h].amp, std::clamp(static_cast<int>(due[h].sec * 1000.0f + 0.5f), 1, 1000));
            ++CyberpunkVR_HapticStats[h == 0 ? 1 : 2];
            continue;
        }
        const int r = OpenXRManager::Get().ApplyHaptic(h, due[h].amp, due[h].sec);
        CyberpunkVR_HapticStats[3] = r;
        if (r == 0) ++CyberpunkVR_HapticStats[h == 0 ? 1 : 2];
        else ++CyberpunkVR_HapticStats[5];
    }
}

}  // namespace cvr::haptics
