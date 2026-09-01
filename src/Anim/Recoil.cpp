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

#include <windows.h>
#include <atomic>
#include <cmath>
#include <cstdint>

// ---- controls -----------------------------------------------------------------------------------
//
// Four numbers, and each one is a thing you can feel: how far the hand is thrown back, how far the
// muzzle rises, how long it takes to come home, and how much of it the second hand takes.
extern "C" __declspec(dllexport) int   CyberpunkVR_HandRecoil          = 1;
// ZERO BY DEFAULT, and that is the correction that matters.
//
// Throwing the wrist backwards is what a free-floating gun does, not what a held one does: the
// player's hand is on a controller and does not move, so pulling the IK target off it breaks the one
// rule this whole rig is built on -- the hand IS the controller. Reported from the headset as "it
// jerks the arm somehow, and that is wrong, my hand is steady", which is exactly what a translated
// wrist looks like from the inside.
//
// What a held weapon actually does with the impulse is ROTATE: the muzzle flips up about the wrist
// while the hand stays where it is being held. That is the whole of it below, and the weapon follows
// because it is parented to the hand bone. The travel stays as a slider for anyone who wants a looser
// grip, and it is the first thing to try if the rotation alone reads as too light.
//
// AND IT WAS ASKED FOR: "recoil kick сделаем на плечо или чтобы рука чуть назад дергалась". So the
// travel is on now, deliberately small. The direction needs no new geometry -- minus the hand's own
// forward axis, on an arm holding a gun out, points back INTO the shoulder, which is where a real
// recoil goes. The old warning above stands: this pulls the IK target off the controller, so the
// number is a couple of centimetres rather than the several a free-floating gun would take, and it is
// scaled per weapon and damped by a second hand exactly like the rise is.
// 3 cm, up from 2 now that the travel is purely backwards: a backward slide runs along the line of
// sight and foreshortens, so it reads weaker than a dip of the same size. An ini key
// (xr_recoil_back_cm) with its ceiling, because it is the number this feature is judged by.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilBackCm    = 3.0f;   // backwards, in the body
// The ceiling for that travel, on the same argument as the angle's: a heavy weapon should shove the
// hand further than a light one, but not far enough to look like the arm was yanked.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilBackMaxCm = 4.0f;
// THE PEAK ANGLE OF ONE SHOT, and it is computed rather than chosen -- 22 deg is what a 9 mm pistol
// held at the wrist actually produces:
//
//     J  = m_b*v_b + m_powder*v_gas = 0.008*360 + 0.00033*540      = 3.06 N s
//     L  = J * h,  h = 0.07 m from the bore axis down to the wrist = 0.214 N m s
//     I  = 1.0*0.08^2 + 0.004 + 0.4*0.05^2                         = 0.0114 kg m^2
//     w0 = L / I                                                   = 18.8 rad/s
//     peak = 0.523 * w0 / w_spring,  w_spring = 2*pi/0.25 s        = 0.39 rad = 22 deg
//
// AN EARLIER VERSION OF THIS COMMENT SAID 7.8 deg AND WAS WRONG: it used h = 0.03 m, which is the
// bore-to-top-of-hand distance, not the distance to the joint the flip actually turns about. The
// error made the recoil invisible in the headset, and the fix for that was briefly a taste number
// (15 deg) -- which is exactly the kind of thing this file is not supposed to contain. The moment arm
// is the physical quantity; the angle follows from it.
//
// This is also the number the per-weapon table will write: J and I are the only things that change
// between a Kenshin and an Overture, so each weapon gets its own peak from the same formula.
// 15.4 deg: the computed 22 taken down by 30% on the user's call after firing every weapon in the
// set. The physics above still holds -- it says what a 9 mm does to a free wrist -- and a hand that
// is braced for the shot, which a player's is, gives less than a free one.
// TUNED DOWN 20% IN THE HEADSET, in two steps and this is the second one. The derivation above is
// left exactly as it was written -- it says where 15.4 came from and it is still right. Half (7.7)
// was tried first and read as too short a throw, so the reduction settled at a fifth.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilRiseDeg   = 12.3f;  // 9 mm reference, -20%
// 180 ms, DOWN FROM 250: a hand holding a gun snaps back, it does not sink. The settle is a property
// of the wrist -- a spring of roughly fixed stiffness -- while the cartridge decides how FAR the muzzle
// goes, which is the per-weapon angle above. An ini key (xr_recoil_return_ms), because this and the
// exponent below are the two numbers that decide how the recoil feels.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilReturnMs  = 180.0f; // full settle
// THE OFF HAND'S SHARE IS GONE, and its absence is the answer rather than an oversight. It said how much
// of the kick a second hand takes when it is also on the weapon -- but a hand ON the weapon now rides the
// weapon: the support point is built from the already-kicked right hand, so the left one inherits exactly
// the motion the gun makes, about the gun's axis rather than its own. Sampling the spring for it again
// was the double kick that tore the hand off the grip. And a hand that is NOT on the weapon feels nothing
// at all. That leaves no case for a fraction, so there is no fraction.

// THE EQUIPPED WEAPON'S OWN KICK, in the game's degrees, published on each draw by the weapon module
// (SetVRWeaponKick). Zero means "unknown weapon" and the reference angle is used unscaled.
//
// The reference is the Lexington at 1.0, because 22 deg on it is the number that was tried in the
// headset and kept. Everything else follows its own ratio: an Overture at 4.0 flips four times as far,
// a Kappa at 0.24 barely moves -- which is the impulse ratio, not a taste ladder.
extern "C" __declspec(dllexport) float CyberpunkVR_WeaponKickDeg = 0.0f;
// THE PER-WEAPON KICK, SCALED. Tuned in the headset alongside the spring above: 0.7 with a halved
// spring was too little kick and too short a throw, so both settled at a fifth off. Kept as a
// separate multiplier rather than folded into the per-weapon table so the ORDERING that table encodes
// -- a .44 kicking four times a 9 mm -- stays visible and stays measured from the game's own
// RecoilKickMax.
// BACK TO 1.0 ON THE USER'S CALL. The 20% came off twice: once here and once in the reference angle
// above (15.4 -> 12.3), so a pistol was flipping at 0.64 of the number that had been tuned in the
// headset and read as weak. The angle keeps its reduction; this multiplier gives back the other one.
extern "C" __declspec(dllexport) float CyberpunkVR_WeaponKickScale = 1.0f;
// WHICH WEAPON IS THIS, as the record itself says: 0 unknown, 1 handgun/revolver, 2 rifle-ish
// (SMG, assault, MG), 3 shotgun, 4 sniper/precision rifle. Published per draw by the weapon module,
// which already reads the kick out of the same record.
extern "C" __declspec(dllexport) int   CyberpunkVR_WeaponClass = 0;
// HOW THE IMPULSE IS SPLIT, per class -- and the split IS the realism, not the size. One shot puts the
// same momentum into the shooter either way; where it goes depends on how the thing is held. A rifle is
// against a shoulder with a hand well out on the handguard: the muzzle barely lifts and the shove goes
// straight back. A pistol is out on an arm with a 7 cm moment arm: it flips. A shotgun does both, hard.
//
// Multipliers rather than three tables, so the per-weapon ladder read out of the game's own
// RecoilKickMax still orders everything inside a class.
// WHICH WAY THE HAND IS THROWN -- and it is not purely backwards, which is the correction here.
//
// The travel was minus the hand's forward axis, straight back along the pointing direction, and what it
// reads as in the headset is DOWN. The physical picture says the observation is the right one: the
// muzzle flips UP about the wrist, so the GRIP -- below the bore, inside the hand -- dips. A pistol's
// visible answer to a shot is the butt dropping in the palm, not the fist sliding backwards. A
// shouldered weapon is the other way round: the shove goes into the shoulder and the grip barely moves.
//
// 0 = all backwards, 1 = all down. Per class, and ini keys, because this is a feel number.
//
// ALL THREE ARE ZERO, tried in the headset and rejected: "да вниз не надо. надо назад. сейчас она
// только вниз дергается". The dip stays as a knob because the argument above is still true of a real
// pistol; what it is not is what this port should be doing to the hand. Back is what a shot reads as.
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilDownPistol  = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilDownRifle   = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilDownShotgun = 0.0f;
// A PISTOL'S THROW IS THE SMALLEST OF THE FOUR. Its energy goes into the flip -- which is at full
// strength -- and not into pushing the hand back: there is no shoulder behind it and no mass to drive.
// Asked for as "минимальный дерг назад". Rise is untouched: the reference IS the pistol.
extern "C" __declspec(dllexport) float CyberpunkVR_PistolBackMul   = 0.30f;
extern "C" __declspec(dllexport) float CyberpunkVR_RifleRiseMul    = 0.35f;
extern "C" __declspec(dllexport) float CyberpunkVR_RifleBackMul    = 2.00f;
extern "C" __declspec(dllexport) float CyberpunkVR_ShotgunRiseMul  = 1.00f;
extern "C" __declspec(dllexport) float CyberpunkVR_ShotgunBackMul  = 2.00f;
// SNIPERS ARE NOT RIFLES, as far as the hand is concerned: the one shouldered weapon whose impulse is
// big enough to move the shooter, so it takes MORE flip than the pistol reference rather than less, and
// by far the longest throw. Both numbers are the user's.
extern "C" __declspec(dllexport) float CyberpunkVR_SniperRiseMul   = 2.00f;
extern "C" __declspec(dllexport) float CyberpunkVR_SniperBackMul   = 5.00f;
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
// WHAT A SECOND HAND LEAVES OF THE BACKWARD THROW -- and it is deliberately NOT the angle's factor.
//
// Sharing that factor (0.286) is what this session already got wrong: it cut a rifle's shoulder shove
// to a fifth on exactly the weapon that is always held with two hands, and the shove is most of what a
// rifle should feel like. Two hands and a braced arm do reduce how far the shooter is moved -- a second
// arm's mass is real -- but they reduce the FLIP far more, because that is leverage against a moment
// arm and this is only mass against an impulse. 0.6 on the user's call, and an ini key
// (xr_recoil_twohand_back), because the previous round proved one number cannot serve both.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandBackMul = 0.60f;
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilRefKick   = 1.0f;   // the Lexington
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilMaxDeg    = 28.0f;  // a hand has limits
// HOW LONG THE HEAVY ONES TAKE TO COME BACK. The settle time of a hand holding a weapon is the period
// of its own spring, 2*pi/sqrt(k/I) -- it grows with the inertia, so a magnum has to come home slower
// than a smart pistol. The weapon's kick stands in for that inertia, mildly (a power of 0.35), which
// spreads a 250 ms reference across roughly 180..400 ms.
//
// THE GAME'S OWN RecoilRecoveryTime WAS READ FIRST AND REJECTED, which is worth writing down: it is a
// gameplay pacing number, not a physical one, and it runs the wrong way -- Kenshin 0.08 s, Kappa 0.30,
// Overture 0.15, Liberty up to 0.80. Taking it would have made the heaviest revolver settle faster
// than the lightest smart pistol.
// 0.15 -- NEARLY FLAT, and that is the correction that matters here. At 0.60 an Overture computed
// 575 ms and sat on the ceiling, so the heaviest weapons were the SLOWEST to come home: a wallow, when
// what a .50 does to a wrist is a crack. Reported against Bodycam ("тяжелые пистолеты типа deagle это
// очень резко дергает").
//
// A trace of dependence is kept rather than none, because a heavier weapon really does stretch the
// hand's spring a little: Kappa 0.24 -> 145 ms, Lexington 1.0 -> 180, Overture 4.0 -> 221. The WEIGHT
// of the shot lives in the angle, which is already per-weapon and knee-compressed near the cap.
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilReturnPow  = 0.15f;
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilReturnMinMs = 80.0f;
// MUZZLE CLIMB ACROSS A BURST -- the piece a per-shot spring cannot express.
//
// One spring that returns to zero makes a burst N identical kicks around one point of aim. A real burst
// WALKS the muzzle up and comes back only part of the way, slowly: the shooter is fighting an
// accumulation, not a sequence of taps. So each shot adds a fraction of its own peak angle to a climb
// term that decays with a long time constant, on top of the fast spring.
//
// First order, not a spring: a climb settles, it does not bounce. Capped, because a held trigger must
// not be able to bend the wrist past what the model says a wrist does.
// OFF BY DEFAULT, AND THE REASON IS VR RATHER THAN BALLISTICS. Tried in the headset and rejected at
// once: "чет какая-то фигня что он медленно типо опускается в конце, это вообще нереалистично".
//
// In a flat shooter the slow walk back down is the sight picture recovering, and it is the game's job.
// Here the weapon is the player's HANDS: they never left, they are steady, and a drift that eases the
// muzzle down over most of a second is the game moving them -- the one thing this whole rig refuses to
// do everywhere else. Accumulation across a burst is still there and still physical, but it lives where
// it belongs: the fast spring takes a velocity impulse per round, so held fire stacks by construction
// and stops the instant the shooting does.
//
// The knob stays, with a short time constant, for anyone who wants a trace of walk-up: at 250 ms it is
// part of the kick rather than a separate drift.
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilClimbFrac   = 0.0f;    // of one shot's peak
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilClimbMaxDeg = 8.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_RecoilClimbMs     = 250.0f;  // time constant of the walk-back
extern "C" __declspec(dllexport) float CyberpunkVR_DebugRecoilClimbDeg = 0.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_HandRecoilReturnMaxMs = 420.0f;
// 40, set from the headset: the ratio keeps a revolver clearly heavier than a 9 mm, and past this
// the wrist reads as broken rather than kicked. It bites on the top four -- Silverhand, Liberty,
// Unity, Omaha, Nue and the Overture all land here, so those six are told apart by their recovery
// rather than by their peak.
// Readable live: how many shots the pose path has answered, and the current displacement.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRecoilShots = 0;
extern "C" __declspec(dllexport) float    CyberpunkVR_DebugRecoilCm    = 0.0f;
// Advances every fresh solve. Zero here means the pose path never reached the spring at all.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRecoilTicks = 0;
// What the SOLVE actually applied to the right hand this frame, degrees of muzzle rise. The spring
// can be perfect and still reach nothing -- everything between the sample and the arm solve is
// downstream, and this is the only number that sees it.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugRecoilAppliedDeg = 0.0f;
// Max-hold of the same number; write 0 to it to re-arm.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugRecoilPeakDeg = 0.0f;

namespace {

double g_lastShotMs = 0.0;

// The shot is signalled from the weapon-aim detour, which runs on a game thread; the spring is
// integrated in the pose hook, which runs on another. One counter crossing between them is the whole
// handover: the pose path compares it against what it last saw and converts the difference into
// impulses. A missed shot is then impossible, and a double-counted one is impossible too -- which a
// bool flag could not promise, because two shots inside one animation frame would collapse into one.
std::atomic<uint64_t> g_shotSeq{0};

struct Spring {
    float x = 0.0f;   // displacement, 0..1 of the configured amplitude
    float v = 0.0f;   // and its rate, per second
};
Spring g_spring[2];
uint64_t g_seenSeq = 0;
uint64_t g_pendingShots = 0;
double g_lastMs = 0.0;

double NowMs() {
    static LARGE_INTEGER f = {};
    if (f.QuadPart == 0) QueryPerformanceFrequency(&f);
    if (f.QuadPart == 0) return 0.0;
    LARGE_INTEGER t{};
    QueryPerformanceCounter(&t);
    return static_cast<double>(t.QuadPart) * 1000.0 / static_cast<double>(f.QuadPart);
}

float g_climbDeg = 0.0f;

// THE WEAPON'S OWN PEAK ANGLE: the reference, scaled by this weapon's kick and compressed by the knee.
// Shared, because the climb has to accumulate a fraction of exactly the angle the kick produces -- two
// copies of this arithmetic would drift apart the first time either is touched.
float WeaponRiseDeg() {
    float rise = CyberpunkVR_HandRecoilRiseDeg;
    if (CyberpunkVR_WeaponKickDeg > 0.0f && CyberpunkVR_HandRecoilRefKick > 0.01f) {
        rise *= (CyberpunkVR_WeaponKickDeg * CyberpunkVR_WeaponKickScale)
                / CyberpunkVR_HandRecoilRefKick;
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
float GripRiseFactor() {
    float f = 1.0f;
    if (CyberpunkVR_WeaponClass == 2)      f = CyberpunkVR_RifleRiseMul;
    else if (CyberpunkVR_WeaponClass == 3) f = CyberpunkVR_ShotgunRiseMul;
    else if (CyberpunkVR_WeaponClass == 4) f = CyberpunkVR_SniperRiseMul;
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

// Called from the shot bracket in src/Hooks/WeaponAim.cpp, once per round that actually leaves the
// barrel -- so burst and full-auto stack by construction, exactly as a real one does.
extern "C" void RecoilOnShot() {
    // ONE ROUND, ONE IMPULSE -- and the site cannot promise that on its own.
    //
    // The provider slot is called several times per round (measured: 42 calls for 8 rounds fired), and
    // the muzzle sequence only separates FRAMES, so a round whose calls straddle two frames still
    // counted twice -- 14 impulses for those same 8 rounds. A refractory window is what actually
    // matches a round: 40 ms is longer than the 2-3 frames one round's calls span at 72 fps, and
    // shorter than the gap between rounds even at 1500 rpm.
    static double s_lastMs = 0.0;
    const double now = NowMs();
    if (s_lastMs > 0.0 && (now - s_lastMs) < 40.0) return;
    s_lastMs = now;
    g_lastShotMs = now;
    g_shotSeq.fetch_add(1, std::memory_order_release);
}

// Advance both springs to now. Called once per fresh solve, before either arm is built.
extern "C" void RecoilTick() {
    ++CyberpunkVR_DebugRecoilTicks;
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

    const uint64_t seq = g_shotSeq.load(std::memory_order_acquire);
    if (seq != g_seenSeq) {
        const uint64_t shots = seq - g_seenSeq;
        g_seenSeq = seq;
        CyberpunkVR_DebugRecoilShots += shots;
        // ...and the burst walks the muzzle up. A fraction of the angle THIS shot will reach, per
        // round, so a magnum climbs faster than a smart pistol for the same reason it kicks harder.
        g_climbDeg += WeaponRiseDeg() * GripRiseFactor() * CyberpunkVR_RecoilClimbFrac
                      * static_cast<float>(shots);
        if (g_climbDeg > CyberpunkVR_RecoilClimbMaxDeg) g_climbDeg = CyberpunkVR_RecoilClimbMaxDeg;
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
        g_pendingShots += shots;
    }

    // Per-weapon settle: the reference return time scaled by the weapon's own kick, so the six that
    // share the 40 deg ceiling are still told apart -- by how long they take to come home rather than
    // by how far they go.
    float ret = CyberpunkVR_HandRecoilReturnMs;
    if (CyberpunkVR_WeaponKickDeg > 0.0f && CyberpunkVR_HandRecoilRefKick > 0.01f) {
        const float r = (CyberpunkVR_WeaponKickDeg * CyberpunkVR_WeaponKickScale)
                        / CyberpunkVR_HandRecoilRefKick;
        ret *= std::pow(r, CyberpunkVR_HandRecoilReturnPow);
        if (ret < CyberpunkVR_HandRecoilReturnMinMs) ret = CyberpunkVR_HandRecoilReturnMinMs;
        if (ret > CyberpunkVR_HandRecoilReturnMaxMs) ret = CyberpunkVR_HandRecoilReturnMaxMs;
    }
    if (ret < 20.0f) ret = 20.0f;
    const float w = 6.28318531f / (ret * 0.001f);
    const float zeta = 0.55f;
    // The impulse is issued HERE, with the same w the spring is about to be integrated with -- the peak
    // is 0.523*v0/w, so any other w would break the promise that the configured angle is the angle.
    if (g_pendingShots) {
        g_spring[0].v += 1.914f * w * static_cast<float>(g_pendingShots);
        g_spring[1].v += 1.914f * w * static_cast<float>(g_pendingShots);
        g_pendingShots = 0;
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
        for (int i = 0; i < steps; ++i) {
            const float a = -w * w * sp.x - 2.0f * zeta * w * sp.v;
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
    if (!weaponHand && !CyberpunkVR_TwoHandActive) return;
    // The weapon's own peak angle -- the per-weapon ratio and the knee that keeps the heavy ones apart,
    // both in WeaponRiseDeg() so the climb accumulates a fraction of exactly this.
    float rise = WeaponRiseDeg();
    // WHAT THE WEAPON ITSELF DOES, before the grip and the class have their say: the per-weapon ratio
    // and the knee above, nothing else. Both the travel and the angle are built from this, which is
    // what keeps them from disagreeing about how hard the shot was.
    const float riseW = rise;

    // HOW THE IMPULSE IS SPLIT, by class. See the knobs: same momentum, three different geometries
    // holding it.
    // How far the hand is thrown, per class. The ANGLE's own factors live in GripRiseFactor().
    float backMul = 1.0f;
    if (CyberpunkVR_WeaponClass == 1)      backMul = CyberpunkVR_PistolBackMul;
    else if (CyberpunkVR_WeaponClass == 2) backMul = CyberpunkVR_RifleBackMul;
    else if (CyberpunkVR_WeaponClass == 3) backMul = CyberpunkVR_ShotgunBackMul;
    else if (CyberpunkVR_WeaponClass == 4) backMul = CyberpunkVR_SniperBackMul;

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
    *outBackM = x * backCm * 0.01f;

    // ...AND THE ANGLE, which is where both the class and the second hand belong -- see GripRiseFactor:
    // the class says how the impulse is split, and the second hand takes what its leverage is worth.
    // Applied to the angle rather than to the impulse, so the settle time is unchanged: the same
    // spring, held better.
    rise = riseW * GripRiseFactor();
    // THE BURST'S CLIMB RIDES ON TOP, and only on the hand that holds the weapon: the support hand
    // inherits it through the weapon, the way it inherits the kick.
    float deg = x * rise + (weaponHand ? g_climbDeg : 0.0f);
    // THE CEILING HAS TO HOLD FOR STACKED SHOTS TOO. Rounds add velocity to the spring, so a burst drives
    // the envelope past 1 and the angle past the cap -- measured at 49.9 deg against a 40 deg ceiling. The
    // cap is a statement about a wrist, and a wrist does not bend further because the trigger was held.
    const float capDeg = CyberpunkVR_HandRecoilMaxDeg;
    if (deg >  capDeg) deg =  capDeg;
    if (deg < -capDeg) deg = -capDeg;
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
    }
}

// When the last round left, on the same clock the rest of this file uses. The heading hook reads it to
// ask a question it cannot answer alone: is the sideways camera jerk on a shot the GAME's own recoil
// arriving through the heading delta, or something of ours.
extern "C" double RecoilLastShotMs() { return g_lastShotMs; }
