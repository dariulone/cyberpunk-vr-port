// ReloadHandOff -- the native reload, played by both arms.
//
// Pressing Reload runs the game's own reload: the weapon state machine enters ReloadEvents and the
// animation graph plays the reload on the arms -- the left hand pulls the magazine, the right hand tips
// the gun. VRIK solved both arms to the controllers straight over it, so the magazine moved through
// the gun on its own while the hands stayed still. So both hands are handed to the animation with a
// seamless transition.
//
// NOT A HAND-OVER IN THE WHEEL'S SENSE. The wheel and the carried body stop solving an arm and give it
// the rig's own segment lengths and clavicle (WheelHandsOff / armToAnim in AnimPose.cpp). Switching to
// that at the end of a blend jumps the hand by the difference between the player's arm and the rig's,
// and the shoulder by the clavicle anchoring -- a step in a transition that exists to have none. So
// the arms keep being solved, and only their TARGET moves: from the controller to the animated hand
// and back, eased. At full weight the IK reaches exactly the hand the animation authored.
//
// ...RE-PLACED ON THE SOLVED CHEST. The animated pose is captured before anything of ours is written
// (beside WheelCaptureAnim), with the animation's own torso; VRIK then moves the torso to the player's
// body. Each hand is recorded relative to the bone both clavicles hang from and put back onto that bone
// as solved, so the reload happens in front of the player's chest wherever it is, and the two hands
// keep the relation the animation gave them -- the left hand still meets the magazine well.
//
// WHEN TO GIVE THE HANDS BACK. The redscript (vrport_reload_anim.reds) passes the seconds until the
// state can end -- its logical duration plus the end time IsReloadDurationComplete waits for. The hands
// start back CyberpunkVR_ReloadHandOffLeadSec before that and arrive over the out ramp, so they are on
// the controllers as the reload completes instead of after it: the end of the animation is the weapon
// coming back up, and the blend carries it into the hand. Interrupted (fire, sprint, quick melee), the
// state exits early and the blend goes back from wherever it is.

#include "Anim/ReloadHandOff.hpp"

#include "Anim/CharacterRig.hpp"   // g_fkRot
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Core/VrCoreShared.hpp"   // g_hasWeaponEquipped

#include <windows.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdint>
#include <cstring>

// The switches and probes, defined in src/Anim/CharacterRigState.cpp (the Rust port shares them).
extern "C" {
__declspec(dllexport) extern int   CyberpunkVR_VrikReloadHandOff;
__declspec(dllexport) extern float CyberpunkVR_ReloadHandOffInSec,CyberpunkVR_ReloadHandOffOutSec,CyberpunkVR_ReloadHandOffLeadSec;
__declspec(dllexport) extern float CyberpunkVR_ReloadHandOffStats[8];
}

namespace cvr::anim {
namespace {

std::atomic<bool>    g_active{false};
std::atomic<float>   g_finishIn{-1.0f};
std::atomic<int64_t> g_enterQpc{0};

struct AnimHand {
    int   chest = -1;
    float pos[3] = {};
    float rot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    bool  valid = false;
};
AnimHand g_hand[2];          // 0 right, 1 left -- the wheel's numbering
float    g_blend = 0.0f;     // linear ramp; the weight is its ease
float    g_weight = 0.0f;
float    g_rate = 0.0f;      // the ramp's slope at the last update, per second
std::atomic<int64_t> g_lastQpc{0};

float Ease(float b) { return b * b * (3.0f - 2.0f * b); }
// The hands' layers' counter (Anim/CharacterRig.hpp): QPC, or the tests' clock.
int64_t Qpc() { return VRIK_ClockQpc(); }
double  QpcSeconds(int64_t ticks) {
    const double freq = VRIK_ClockQpcFrequency();
    return freq > 0.0 ? static_cast<double>(ticks) / freq : 0.0;
}

// The bone both clavicles hang from: the upper arm's parent is the clavicle, its parent the chest.
int ChestOf(int upperArm) {
    if (upperArm < 0 || upperArm >= VRIK_MAX_BONES) return -1;
    const int clavicle = g_VRBoneParent[upperArm];
    if (clavicle < 0 || clavicle >= VRIK_MAX_BONES) return -1;
    const int chest = g_VRBoneParent[clavicle];
    return (chest >= 0 && chest < VRIK_MAX_BONES) ? chest : -1;
}

void Capture(AnimHand& h, int handIdx, int upperArm) {
    h.valid = false;
    h.chest = ChestOf(upperArm);
    if (h.chest < 0 || handIdx < 0 || handIdx >= VRIK_MAX_BONES) return;
    float inv[4]; VRIK_QuatConj(g_fkRot[h.chest], inv);
    const float d[3] = { g_fkPos[handIdx][0] - g_fkPos[h.chest][0],
                         g_fkPos[handIdx][1] - g_fkPos[h.chest][1],
                         g_fkPos[handIdx][2] - g_fkPos[h.chest][2] };
    VRIK_QuatRotateVec(inv, d, h.pos);
    VRIK_QuatMul(inv, g_fkRot[handIdx], h.rot); VRIK_QuatNorm(h.rot);
    h.valid = std::isfinite(h.pos[0] + h.pos[1] + h.pos[2] + h.rot[0] + h.rot[1] + h.rot[2] + h.rot[3]);
}

}  // namespace

void ReloadHandOffSet(bool active, float finishInSec) {
    if (active) {
        g_finishIn.store(std::isfinite(finishInSec) ? finishInSec : -1.0f, std::memory_order_relaxed);
        g_enterQpc.store(Qpc(), std::memory_order_relaxed);
        CyberpunkVR_ReloadHandOffStats[7] += 1.0f;
    }
    g_active.store(active, std::memory_order_release);
}

void ReloadHandOffCapture() {
    Capture(g_hand[0], g_VRRightBoneIdx, g_VRRightUpperArmIdx);
    Capture(g_hand[1], g_VRLeftBoneIdx, g_VRLeftUpperArmIdx);
    if (g_hand[0].valid) std::memcpy(&CyberpunkVR_ReloadHandOffStats[4], g_hand[0].pos, sizeof(g_hand[0].pos));
}

void ReloadHandOffUpdate() {
    const int64_t now = Qpc();
    const int64_t last = g_lastQpc.load(std::memory_order_relaxed);
    float dt = last ? static_cast<float>(QpcSeconds(now - last)) : 0.016f;
    dt = std::clamp(dt, 0.0f, 0.1f);

    const bool active = g_active.load(std::memory_order_acquire);
    const float finishIn = g_finishIn.load(std::memory_order_relaxed);
    const float elapsed = active ? static_cast<float>(QpcSeconds(now - g_enterQpc.load(std::memory_order_relaxed))) : 0.0f;
    // A reload whose exit never reaches us (the weapon state machine detached with the weapon) must not
    // keep the arms: a second past its expected end, or ten seconds into a shell whose end is unknown --
    // every following shell enters again and restarts that clock.
    const bool overdue = finishIn > 0.0f ? elapsed > finishIn + 1.0f : elapsed > 10.0f;
    const bool wanted = active && !overdue && CyberpunkVR_VrikReloadHandOff != 0 && g_hasWeaponEquipped &&
        !(finishIn > 0.0f && elapsed >= finishIn - CyberpunkVR_ReloadHandOffLeadSec);

    const float ramp = wanted ? CyberpunkVR_ReloadHandOffInSec : CyberpunkVR_ReloadHandOffOutSec;
    const float step = ramp > 0.001f ? dt / ramp : 1.0f;
    g_blend = std::clamp(g_blend + (wanted ? step : -step), 0.0f, 1.0f);
    // Smoothstep: no velocity step where the hands leave the controllers or arrive back on them.
    g_weight = Ease(g_blend);
    g_rate = (wanted ? (g_blend < 1.0f) : (g_blend > 0.0f)) && ramp > 0.001f ? (wanted ? 1.0f : -1.0f) / ramp : 0.0f;
    g_lastQpc.store(now, std::memory_order_relaxed);

    CyberpunkVR_ReloadHandOffStats[0] = g_weight;
    CyberpunkVR_ReloadHandOffStats[1] = wanted ? 1.0f : 0.0f;
    CyberpunkVR_ReloadHandOffStats[2] = elapsed;
    CyberpunkVR_ReloadHandOffStats[3] = active ? finishIn : -1.0f;
}

// THE FINGERS ARE WRITTEN ON EVERY PASS, the arms only on the fresh one. A tick's first pass reaches the
// finger layers before its fresh solve has updated the ramp, so a stored value would give that pass the
// previous tick's weight and the next passes this tick's -- finger bones that differ between passes
// flicker. The ramp is extrapolated to the moment of the call instead: passes a millisecond apart get
// weights a millisecond apart.
float ReloadHandOffWeight() {
    if (g_rate == 0.0f) return g_weight;
    const int64_t last = g_lastQpc.load(std::memory_order_relaxed);
    const float ahead = last ? std::clamp(static_cast<float>(QpcSeconds(Qpc() - last)), 0.0f, 0.1f) : 0.0f;
    return Ease(std::clamp(g_blend + g_rate * ahead, 0.0f, 1.0f));
}

void ReloadHandOffBlendTarget(int hand, float* target, float* handRot) {
    if (hand < 0 || hand > 1 || !target || !handRot || g_weight <= 0.0f) return;
    const AnimHand& h = g_hand[hand];
    if (!h.valid || h.chest < 0) return;
    // The chest as solved: the arm block runs after the body solve and its FK.
    float animPos[3], animRot[4];
    VRIK_QuatRotateVec(g_fkRot[h.chest], h.pos, animPos);
    animPos[0] += g_fkPos[h.chest][0]; animPos[1] += g_fkPos[h.chest][1]; animPos[2] += g_fkPos[h.chest][2];
    VRIK_QuatMul(g_fkRot[h.chest], h.rot, animRot); VRIK_QuatNorm(animRot);
    const float w = g_weight;
    for (int k = 0; k < 3; ++k) target[k] += (animPos[k] - target[k]) * w;
    if (handRot[0]*animRot[0] + handRot[1]*animRot[1] + handRot[2]*animRot[2] + handRot[3]*animRot[3] < 0.0f)
        for (float& c : animRot) c = -c;
    for (int k = 0; k < 4; ++k) handRot[k] += (animRot[k] - handRot[k]) * w;
    VRIK_QuatNorm(handRot);
}

}  // namespace cvr::anim
