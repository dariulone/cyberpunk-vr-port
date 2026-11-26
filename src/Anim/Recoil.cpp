#include "Utils/DebugGate.hpp"
// Recoil that reaches the HANDS -- the kick the weapon gives the shooter, not the camera.
//
// The game already kicks the camera (an additive spine animation, base\animations\weapon\firearms\*,
// `add_camera_recoil_single_shot_*`). It never reaches the arms in VR for a structural reason: VRIK
// writes the wrist and the hand rotation from the controller every solve, so whatever the animation
// system does to those bones is overwritten in the same frame. The weapon, being parented to the
// hand, does not move either. So the kick has to be added where the arm is decided -- to the IK
// TARGET, before the solve -- or it does not exist at all.
//
// THE IMPULSE LIVES IN THE HAND'S OWN FRAME, not the world's and not the weapon's. Back is -Y of the
// hand and rise is a rotation about its right axis, so the same numbers work for a pistol, a rifle,
// either hand, any grip, and no part of this needs to know what is being held or where the muzzle is.
//
// The motion is a damped spring given a velocity impulse, which is what a shoulder actually does: the
// hand leaves fast, comes back, and settles. A pure decay (x *= k each frame) reads as a soft push
// because it has no overshoot and no return time of its own -- the whole character of recoil is in
// how it comes BACK.
//
//     x'' = -w^2 x - 2*zeta*w x'          w = 2*pi / returnTime, zeta = 0.55 (a little overshoot)
//
// Everything is integrated per solve with the real elapsed time, so the feel does not change with
// frame rate -- a rate-dependent decay was the first version and it made recoil weaker the faster the
// machine ran.

#include "Anim/CharacterRig.hpp"
#include "Camera/CameraState.hpp"
#include "Runtimes/Haptics.hpp"     // the round in the hand
#include "Anim/LeftHanded.hpp"

#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstdint>

#include "Anim/Recoil.hpp"

// The controls -- the kick's size, its split by class, its settle, the climb -- and the debug read-outs are defined in
// src/Anim/CharacterRigState.cpp, with the reasoning behind each number; the Rust port (rust/crates/cvr-rig/src/vrik/
// recoil.rs, CVR_RUST_VRIK) reads the same objects.
// Held up by the reload mod while two guns are in hand (src/Hooks/XInput.cpp).
extern "C" volatile uint64_t CyberpunkVR_DualWieldUntil;
// The two-hand grip (src/Anim/TwoHandGrip.cpp): its state, and what it leaves of the kick.
extern "C" __declspec(dllexport) extern int   CyberpunkVR_TwoHandActive;
extern "C" __declspec(dllexport) extern float CyberpunkVR_TwoHandRecoil;
// ...and HOW MUCH LEVERAGE that hand has, 0..1, which is what decides how much of the flip it can
// actually take. TwoHandRecoil is therefore what a hand AT FULL LEVERAGE leaves; a pistol's second hand
// is worth a fraction of that, because torque is force times lever and its lever is 7 cm.
extern "C" __declspec(dllexport) extern float CyberpunkVR_TwoHandLever;
// ...and how much of the hold has actually arrived, 0..1, so the damping fades in with the hand rather
// than appearing on the frame the grip button goes down.
extern "C" __declspec(dllexport) extern float CyberpunkVR_TwoHandWeight;

namespace {

double g_lastShotMs = 0.0;

// The shot is signalled from the weapon-aim detour, which runs on a game thread; the spring is
// integrated in the pose hook, which runs on another. One counter crossing between them is the whole
// handover: the pose path compares it against what it last saw and converts the difference into
// impulses. A missed shot is then impossible, and a double-counted one is impossible too -- which a
// bool flag could not promise, because two shots inside one animation frame would collapse into one.
// ONE COUNTER PER GUN: [1] is the weapon in hand, [0] the left gun while two are held.
std::atomic<uint64_t> g_shotSeq[2] = {};

struct Spring {
    float x = 0.0f;   // displacement, 0..1 of the configured amplitude
    float v = 0.0f;   // and its rate, per second
};
Spring g_spring[2];
uint64_t g_seenSeq[2] = {};
uint64_t g_pendingShots[2] = {};
double g_lastMs = 0.0;

bool DualLive() { return VRIK_ClockMs() < CyberpunkVR_DualWieldUntil; }

// Which gun a hand's spring answers to. With a gun in each hand the left one has its own weapon; otherwise
// both springs belong to the weapon in hand, exactly as before.
float KickOf(int side) {
    if (side == 0 && DualLive() && CyberpunkVR_WeaponKickDegLeft > 0.0f) return CyberpunkVR_WeaponKickDegLeft;
    return CyberpunkVR_WeaponKickDeg;
}
int ClassOf(int side) {
    if (side == 0 && DualLive() && CyberpunkVR_WeaponClassLeft > 0) return CyberpunkVR_WeaponClassLeft;
    return CyberpunkVR_WeaponClass;
}

// The performance counter in milliseconds (VRIK_ClockQpc: the tests' counter, or QueryPerformanceCounter).
double NowMs() {
    const double f = VRIK_ClockQpcFrequency();
    if (f == 0.0) return 0.0;
    return static_cast<double>(VRIK_ClockQpc()) * 1000.0 / f;
}

float g_climbDeg = 0.0f;

// THE WEAPON'S OWN PEAK ANGLE: the reference, scaled by this weapon's kick and compressed by the knee.
// Shared, because the climb has to accumulate a fraction of exactly the angle the kick produces -- two
// copies of this arithmetic would drift apart the first time either is touched.
float WeaponRiseDeg(int side) {
    float rise = CyberpunkVR_HandRecoilRiseDeg;
    const float kick = KickOf(side);
    if (kick > 0.0f && CyberpunkVR_HandRecoilRefKick > 0.01f) {
        rise *= (kick * CyberpunkVR_WeaponKickScale) / CyberpunkVR_HandRecoilRefKick;
        const float knee = CyberpunkVR_HandRecoilRiseDeg;
        const float cap  = CyberpunkVR_HandRecoilMaxDeg;
        if (rise > knee && cap > knee) {
            rise = knee + (cap - knee) * std::tanh((rise - knee) / (cap - knee));
        }
        if (rise > cap) rise = cap;
    }
    return rise;
}

// WHAT THE GRIP AND THE CLASS LEAVE OF IT. The class decides how the impulse is split (a rifle barely
// lifts), and the second hand takes what its LEVERAGE is worth -- full damping out on a handguard,
// almost none cupped under a pistol grip, because torque is force times lever.
float GripRiseFactor(int side) {
    float f = 1.0f;
    const int cls = ClassOf(side);
    if (cls == 2)      f = CyberpunkVR_RifleRiseMul;
    else if (cls == 3) f = CyberpunkVR_ShotgunRiseMul;
    else if (cls == 4) f = CyberpunkVR_SniperRiseMul;
    if (CyberpunkVR_TwoHandActive) {
        float lev = CyberpunkVR_TwoHandLever;
        if (lev < 0.0f) lev = 0.0f;
        if (lev > 1.0f) lev = 1.0f;
        float keep = CyberpunkVR_TwoHandRecoil;      // what full leverage leaves
        if (keep < 0.0f) keep = 0.0f;
        if (keep > 1.0f) keep = 1.0f;
        f *= 1.0f - (1.0f - keep) * lev;
    }
    return f;
}

}  // namespace

// One round out of one gun: side 1 is the weapon in hand, side 0 the left gun while two are held.
static void RecoilOnShotSide(int side) {
    // ONE ROUND, ONE IMPULSE -- and the site cannot promise that on its own.
    //
    // The provider slot is called several times per round (measured: 42 calls for 8 rounds fired), and
    // the muzzle sequence only separates FRAMES, so a round whose calls straddle two frames still
    // counted twice -- 14 impulses for those same 8 rounds. A refractory window is what actually
    // matches a round: 40 ms is longer than the 2-3 frames one round's calls span at 72 fps, and
    // shorter than the gap between rounds even at 1500 rpm. Per gun, so two guns fired together are
    // two rounds.
    static double s_lastMs[2] = {};
    const double now = NowMs();
    if (s_lastMs[side] > 0.0 && (now - s_lastMs[side]) < 40.0) return;
    s_lastMs[side] = now;
    g_lastShotMs = now;
    g_shotSeq[side].fetch_add(1, std::memory_order_release);
    // ...and the round is felt: this is the one place that already knows a round left the barrel,
    // so the controller gets one envelope per round, by weapon class, in the hand holding that gun -- and
    // a weaker copy in the support hand when it is really on the gun.
    // (Left-handed the weapon in hand is the LEFT controller's: the guns' sides are the game's, the hands are not.)
    const int hand = cvr::anim::WeaponInLeftHand() ? 1 - side : side;
    cvr::haptics::Shot(ClassOf(side), side == 1 && CyberpunkVR_TwoHandActive != 0, hand);
}

// Called from the shot bracket in src/Hooks/WeaponAim.cpp, once per round that actually leaves the
// barrel -- so burst and full-auto stack by construction, exactly as a real one does.
extern "C" void RecoilOnShot() { RecoilOnShotSide(1); }

// ...and the same for a round out of the LEFT gun while the game reports a gun in each hand.
extern "C" void RecoilOnShotLeft() { RecoilOnShotSide(0); }

// Advance both springs to now. Called once per fresh solve, before either arm is built.
extern "C" void RecoilTick() {
    CVR_DIAGNOSTIC(++CyberpunkVR_DebugRecoilTicks);
    if (!CyberpunkVR_HandRecoil) {
        g_spring[0] = Spring{};
        g_spring[1] = Spring{};
        CyberpunkVR_DebugRecoilCm = 0.0f;
        return;
    }
    const double now = NowMs();
    float dt = (g_lastMs > 0.0) ? static_cast<float>((now - g_lastMs) * 0.001) : 0.0f;
    g_lastMs = now;
    if (dt < 0.0f) dt = 0.0f;
    if (dt > 0.1f) dt = 0.1f;          // a hitch or a pause must not launch the spring

    for (int g = 0; g < 2; ++g) {
        const uint64_t seq = g_shotSeq[g].load(std::memory_order_acquire);
        if (seq == g_seenSeq[g]) continue;
        const uint64_t shots = seq - g_seenSeq[g];
        g_seenSeq[g] = seq;
        CVR_DIAGNOSTIC(CyberpunkVR_DebugRecoilShots += shots);
        // ...and the burst walks the muzzle up. A fraction of the angle THIS shot will reach, per
        // round, so a magnum climbs faster than a smart pistol for the same reason it kicks harder.
        // Only the weapon in hand climbs: the climb is applied to that hand alone (RecoilSample).
        if (g == 1) {
            g_climbDeg += WeaponRiseDeg(1) * GripRiseFactor(1) * CyberpunkVR_RecoilClimbFrac
                          * static_cast<float>(shots);
            if (g_climbDeg > CyberpunkVR_RecoilClimbMaxDeg) g_climbDeg = CyberpunkVR_RecoilClimbMaxDeg;
        }
        // Velocity, not position: a position step teleports the hand and reads as a glitch, while a
        // velocity impulse is a throw the spring then has to catch. Stacked shots add velocity, so a
        // held trigger climbs -- which is the behaviour, not a bug.
        //
        // THE SIZE IS DERIVED, NOT PICKED. With x(0)=0 and x'(0)=v0 this spring peaks at
        //     x_peak = 0.523 * v0 / w        (zeta = 0.55)
        // so one round reaching exactly the configured amplitude means v0 = 1.914*w, and the numbers
        // in the controls then mean what they say. A flat 6.0 sat here first and measured 0.34 cm of
        // travel against 3.5 asked for -- eleven times short, in a way no slider could have found,
        // because the error was in the units and not in the value.
        g_pendingShots[g] += shots;
    }

    // Per-weapon settle: the reference return time scaled by the weapon's own kick, so the six that
    // share the 40 deg ceiling are still told apart -- by how long they take to come home rather than
    // by how far they go. Per hand, because with a gun in each the two can be different weapons.
    float w[2];
    for (int s = 0; s < 2; ++s) {
        float ret = CyberpunkVR_HandRecoilReturnMs;
        const float kick = KickOf(s);
        if (kick > 0.0f && CyberpunkVR_HandRecoilRefKick > 0.01f) {
            const float r = (kick * CyberpunkVR_WeaponKickScale) / CyberpunkVR_HandRecoilRefKick;
            ret *= std::pow(r, CyberpunkVR_HandRecoilReturnPow);
            if (ret < CyberpunkVR_HandRecoilReturnMinMs) ret = CyberpunkVR_HandRecoilReturnMinMs;
            if (ret > CyberpunkVR_HandRecoilReturnMaxMs) ret = CyberpunkVR_HandRecoilReturnMaxMs;
        }
        if (ret < 20.0f) ret = 20.0f;
        w[s] = 6.28318531f / (ret * 0.001f);
    }
    const float zeta = 0.55f;
    // The impulse is issued HERE, with the same w the spring is about to be integrated with -- the peak
    // is 0.523*v0/w, so any other w would break the promise that the configured angle is the angle.
    // WHICH SPRING A ROUND DRIVES: with a gun in each hand, each gun its own hand's; otherwise the weapon
    // in hand drives both, as it always has (the left spring is read only for a hand on that weapon).
    const bool dual = DualLive();
    if (g_pendingShots[1]) {
        g_spring[1].v += 1.914f * w[1] * static_cast<float>(g_pendingShots[1]);
        if (!dual) g_spring[0].v += 1.914f * w[0] * static_cast<float>(g_pendingShots[1]);
        g_pendingShots[1] = 0;
    }
    if (g_pendingShots[0]) {
        g_spring[0].v += 1.914f * w[0] * static_cast<float>(g_pendingShots[0]);
        g_pendingShots[0] = 0;
    }
    // FIXED 2 ms SUBSTEPS, because a frame is far too coarse for this spring. Integrated once per
    // frame at 72 Hz the scheme reaches only a fraction of the analytic peak, and the fraction depends
    // on the stiffness: 0.53 at a 180 ms return, 0.67 at 250, 0.79 at 406. That silently compressed the
    // per-weapon ladder -- every light weapon lost half its kick while a heavy one kept four fifths --
    // and it made "the configured angle is the angle" false. At 2 ms the same weapons land at
    // 0.93 / 0.95 / 0.97, and the ladder is the one the numbers describe.
    int steps = static_cast<int>(dt / 0.002f) + 1;
    if (steps > 64) steps = 64;
    const float sdt = dt / static_cast<float>(steps);
    for (int s = 0; s < 2; ++s) {
        Spring& sp = g_spring[s];
        const float ws = w[s];
        for (int i = 0; i < steps; ++i) {
            const float a = -ws * ws * sp.x - 2.0f * zeta * ws * sp.v;
            sp.v += a * sdt;
            sp.x += sp.v * sdt;
        }
        if (std::fabs(sp.x) < 1e-5f && std::fabs(sp.v) < 1e-4f) { sp.x = 0.0f; sp.v = 0.0f; }
    }
    // THE WALK BACK DOWN, first order and slow: the aim recovers over most of a second, and it is a
    // recovery rather than a return -- while the trigger is held the additions outrun it, which is the
    // whole behaviour.
    {
        const float tau = (CyberpunkVR_RecoilClimbMs > 1.0f) ? (CyberpunkVR_RecoilClimbMs * 0.001f) : 0.001f;
        g_climbDeg *= std::exp(-dt / tau);
        if (g_climbDeg < 0.0005f) g_climbDeg = 0.0f;
    }
    CyberpunkVR_DebugRecoilClimbDeg = g_climbDeg;
    CyberpunkVR_DebugRecoilCm = g_spring[0].x * CyberpunkVR_HandRecoilBackCm;
}

// HOW THE THROW IS SPLIT between backwards and down, unit length, so the magnitude stays entirely in
// backM and the ini number stays in centimetres. The CALLER supplies the frame -- see the note above --
// because "back" is a direction in the body and not in the wrist: taken from the hand's own axis it is
// back along however the wrist happens to be tilted, and the part of that the eye notices is the drop.
extern "C" void RecoilTravelMix(float* outBack, float* outDown) {
    float d = CyberpunkVR_RecoilDownPistol;
    // a sniper is shouldered like a rifle, so it shares the rifle's split between back and down
    if (CyberpunkVR_WeaponClass == 2 || CyberpunkVR_WeaponClass == 4) d = CyberpunkVR_RecoilDownRifle;
    else if (CyberpunkVR_WeaponClass == 3) d = CyberpunkVR_RecoilDownShotgun;
    if (d < 0.0f) d = 0.0f;
    if (d > 1.0f) d = 1.0f;
    const float back = 1.0f - d;
    const float len = std::sqrt(back * back + d * d);
    const float k = (len > 1e-6f) ? (1.0f / len) : 0.0f;
    if (outBack) *outBack = back * k;
    if (outDown) *outDown = d * k;
}

// The displacement for one hand, in that hand's own frame: metres along RecoilTravelDirLocal, and
// radians of rise about its right axis. Only the hand that HOLDS the weapon is ever given anything:
// see the note on the removed share above.
extern "C" void RecoilSample(int side, int weaponHand, float* outBackM, float* outRiseRad) {
    *outBackM = 0.0f;
    *outRiseRad = 0.0f;
    if (!CyberpunkVR_HandRecoil || side < 0 || side > 1) return;
    // A HAND THAT IS NOT ON THE WEAPON FEELS NOTHING. It was being kicked anyway, every shot, whether or
    // not it was holding anything -- reported from the headset exactly that way. The two-hand grip is what
    // decides, because it is the only thing that puts this hand on the gun; and when it does, the kick
    // arrives through the weapon rather than from here (see AnimPose's left branch).
    // ...AND A GUN IN EACH HAND makes the left one a weapon hand too, kicked by its own gun's rounds.
    if (side == 0 && DualLive()) weaponHand = 1;
    if (!weaponHand && !CyberpunkVR_TwoHandActive) return;
    // The weapon's own peak angle -- the per-weapon ratio and the knee that keeps the heavy ones apart,
    // both in WeaponRiseDeg() so the climb accumulates a fraction of exactly this.
    float rise = WeaponRiseDeg(side);
    const int cls = ClassOf(side);
    // WHAT THE WEAPON ITSELF DOES, before the grip and the class have their say: the per-weapon ratio
    // and the knee above, nothing else. Both the travel and the angle are built from this, which is
    // what keeps them from disagreeing about how hard the shot was.
    const float riseW = rise;

    // HOW THE IMPULSE IS SPLIT, by class. See the knobs: same momentum, three different geometries
    // holding it.
    // How far the hand is thrown, per class. The ANGLE's own factors live in GripRiseFactor().
    float backMul = 1.0f;
    if (cls == 1)      backMul = CyberpunkVR_PistolBackMul;
    else if (cls == 2) backMul = CyberpunkVR_RifleBackMul;
    else if (cls == 3) backMul = CyberpunkVR_ShotgunBackMul;
    else if (cls == 4) backMul = CyberpunkVR_SniperBackMul;

    const float x = g_spring[side].x;
    // THE TRAVEL RIDES THE WEAPON'S SHAPING, NOT THE GRIP'S -- and that correction is most of what
    // made a rifle feel wrong. It used to ride `rise` AFTER the two-hand damping, so the shoulder
    // shove was cut to a fifth on exactly the weapons that are always held with two hands. Two hands
    // hold the muzzle DOWN, by leverage; they do not absorb the momentum, which still has to go
    // somewhere. So the damping stays on the angle, below, and the travel keeps the weapon's own
    // severity -- scaled by the class, ceiling included, or a rifle would be clipped at a pistol's
    // limit.
    float backCm = CyberpunkVR_HandRecoilBackCm * backMul;
    if (CyberpunkVR_HandRecoilRiseDeg > 0.01f) backCm *= riseW / CyberpunkVR_HandRecoilRiseDeg;
    // ...and a second hand on the weapon shortens the throw -- by its own factor, faded in by how much
    // of the hold has arrived. The ceiling comes down with it, or the damping would be invisible on
    // exactly the heavy weapons that reach the ceiling in the first place.
    {
        float w = CyberpunkVR_TwoHandWeight;
        if (w < 0.0f) w = 0.0f;
        if (w > 1.0f) w = 1.0f;
        float m = CyberpunkVR_TwoHandBackMul;
        if (m < 0.0f) m = 0.0f;
        if (m > 1.0f) m = 1.0f;
        backMul *= 1.0f + (m - 1.0f) * w;
        backCm  *= 1.0f + (m - 1.0f) * w;
    }
    const float backMax = CyberpunkVR_HandRecoilBackMaxCm * backMul;
    if (backCm > backMax) backCm = backMax;
    *outBackM = x * backCm * 0.01f * CyberpunkVR_RecoilScale;

    // ...AND THE ANGLE, which is where both the class and the second hand belong -- see GripRiseFactor:
    // the class says how the impulse is split, and the second hand takes what its leverage is worth.
    // Applied to the angle rather than to the impulse, so the settle time is unchanged: the same
    // spring, held better.
    rise = riseW * GripRiseFactor(side);
    // THE BURST'S CLIMB RIDES ON TOP, and only on the hand that holds the weapon: the support hand
    // inherits it through the weapon, the way it inherits the kick.
    float deg = x * rise + ((weaponHand && side == 1) ? g_climbDeg : 0.0f);
    // THE CEILING HAS TO HOLD FOR STACKED SHOTS TOO. Rounds add velocity to the spring, so a burst drives
    // the envelope past 1 and the angle past the cap -- measured at 49.9 deg against a 40 deg ceiling. The
    // cap is a statement about a wrist, and a wrist does not bend further because the trigger was held.
    const float capDeg = CyberpunkVR_HandRecoilMaxDeg;
    if (deg >  capDeg) deg =  capDeg;
    if (deg < -capDeg) deg = -capDeg;
    deg *= CyberpunkVR_RecoilScale;
    *outRiseRad = deg * 0.01745329252f;
    if (side == 1) {
        const float deg = *outRiseRad * 57.2957795f;
        CyberpunkVR_DebugRecoilAppliedDeg = deg;
        // PEAK HELD, because sampling a 180 ms spring from outside the process means catching it in
        // the act -- three measurement windows in a row missed every shot simply because the trigger
        // was not being pulled during them. A latch the reader clears makes the question "did the
        // hand ever get the kick" answerable without standing next to the trigger.
        const float a = deg < 0.0f ? -deg : deg;
        if (a > CyberpunkVR_DebugRecoilPeakDeg) CyberpunkVR_DebugRecoilPeakDeg = a;
    } else {
        const float a = std::fabs(*outRiseRad) * 57.2957795f;
        if (a > CyberpunkVR_DebugRecoilPeakDegLeft) CyberpunkVR_DebugRecoilPeakDegLeft = a;
    }
}

// When the last round left, on the same clock the rest of this file uses. The heading hook reads it to
// ask a question it cannot answer alone: is the sideways camera jerk on a shot the GAME's own recoil
// arriving through the heading delta, or something of ours.
extern "C" double RecoilLastShotMs() { return g_lastShotMs; }
