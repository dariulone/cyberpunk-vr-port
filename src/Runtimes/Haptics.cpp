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


#include <windows.h>
#include <algorithm>
#include <cstdint>
#include <mutex>

extern "C" {
// The controls and read-outs (src/Runtimes/HapticsState.cpp): the master switch, the scale, the support hand's share,
// the profiles (per row three segments of amplitude 0..1 and milliseconds) and the counters.
__declspec(dllexport) extern int   CyberpunkVR_Haptics;
__declspec(dllexport) extern float CyberpunkVR_HapticScale;
__declspec(dllexport) extern float CyberpunkVR_HapticSupportMul;
__declspec(dllexport) extern float CyberpunkVR_HapticProfiles[16][6];
__declspec(dllexport) extern int   CyberpunkVR_HapticStats[8];
// The off hand is on the weapon (src/Anim/TwoHandGrip.cpp).
__declspec(dllexport) extern int CyberpunkVR_TwoHandActive;
// The one device call (src/Runtimes/HapticsDevice.cpp): OpenXR, or the PSVR2 bridge.
void cvr_cpp_haptics_play(int hand, float amplitude, float seconds);
// The tests' counter (ticks at 1 MHz), src/Anim/CharacterRigState.cpp; null = QueryPerformanceCounter.
extern int64_t (*g_VRIKClockQpc)();
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

int64_t Qpc() {
    if (g_VRIKClockQpc) return g_VRIKClockQpc();
    LARGE_INTEGER c{}; QueryPerformanceCounter(&c); return c.QuadPart;
}
int64_t QpcFreq() {
    if (g_VRIKClockQpc) return 1000000;
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
    // The device call (src/Runtimes/HapticsDevice.cpp): OpenXR, or the PSVR2 bridge's own channel.
    for (int h = 0; h < 2; ++h) {
        if (due[h].amp <= 0.0f) continue;
        cvr_cpp_haptics_play(h, due[h].amp, due[h].sec);
    }
}

}  // namespace cvr::haptics
