// TwoHandGrip -- the support hand on the weapon, and the aim that follows from it.
//
// A pistol held in one hand is what the game does because a flat shooter has one aim vector and no second
// hand to speak of. In VR the second hand is real, it is empty, and bringing it to the gun is the first
// thing anyone does -- so this makes that mean something: reach for the grip and the fingers preview the
// hold; squeeze and the hand LOCKS to the weapon, the barrel starts pointing along the line between the
// two hands, and the recoil the hand takes drops to a fifth.
//
// NOTHING HERE IS AUTHORED. The hold is one frame of the game's own two-handed animation, captured with
// VRIK off (when the animation actually reaches the arms) by VRTwoHandCapture(): where the left wrist sits
// relative to the RIGHT wrist, how it is turned, and how its fingers are curled. Stored the same way and in
// the same place as every other recorded pose in this port -- CyberpunkVR_TwoHandGrip_Left.ini, keyed by
// bone name -- so it survives a rig change and reads like the smoke and rest grips beside it.
//
// WHY THE OFFSET IS RELATIVE TO THE RIGHT WRIST rather than to the weapon: the weapon is parented to the
// right hand bone, so the two are the same statement, and the wrist is a bone this code already has in
// both frames it needs (animated at capture, solved at replay). Going through the weapon would add its
// attachment transform to both sides of the equation for nothing.
//
// THE AIM. The weapon is the right controller's, and the left hand steers it only by how it moves AGAINST
// the right one once the grip has closed (CyberpunkVR_TwoHandAim). So squeezing turns nothing and moving
// both hands together turns nothing -- the drawn hands show the controllers -- while pushing the
// handguard with the left hand turns the barrel, with the roll left to the right controller, which is
// what a wrist does. The drawn left hand is placed on the weapon by the offset above. A preview NEVER
// moves a wrist: only a held grip does, which is the same rule the reload module follows for slides and
// magazines.

#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/TwoHandGrip.hpp"
#include "Core/VrCoreShared.hpp"    // g_hasWeaponEquipped
#include "Natives/NativeState.hpp"  // VRDiagPath
#include "Anim/ScenePolicy.hpp"     // the carried body's left arm
#include "Anim/VrikState.hpp"       // g_VRBodyCarryingState
#include "Anim/LeftHanded.hpp"

#include <windows.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>

// Published by the weapon module on each draw (src/Natives/OrientationProvider.cpp).
extern void Log(const char* fmt, ...);   // the ungated project logger
extern "C" __declspec(dllexport) extern char CyberpunkVR_WeaponName[64];

// ---- the switches, and every one of them is a fact rather than a taste ----
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandGrip    = 1;      // the feature
// 1 = the support-hand weld is OFF. Raised by the weapon module while the weapon is carried in the
// LEFT hand: the gun is parented to that hand then, so there is no support hand to weld -- and, more
// to the point, the weld has to be gone BEFORE the carry measures the bone it attaches to, or the
// wrist unbends afterwards and takes the weapon with it.
// A MASK, bit 0 being that. Bit k+1 takes away only the weapon's k-th hold (see g_cand): the reload module
// raises it for a hold on a magazine that is not in the gun -- with no magazine there is
// nothing to hold on to -- while the weapon's other holds stay on offer.
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandSuppress = 0;
// Which of the weapon's holds is the one in use (index into g_cand), -1 = none; for tests and the log.
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandHoldIdx = -1;
extern "C" __declspec(dllexport) extern int CyberpunkVR_VrikBodyCarryHandOff;   // Hooks/AnimPose.cpp
// 1 = the weapon is being carried in the LEFT hand (the weapon module says so, on the frame it moves
// the item into the left slot).
extern "C" __declspec(dllexport) int   CyberpunkVR_CarryLeft      = 0;
// How long the wrist takes to come out of the welded hold and back to the controller, milliseconds.
// The weapon is attached to that wrist, so this is also how long the gun takes to settle -- 200 ms is
// the same order as the recoil settle and the finger-preview ramp.
extern "C" __declspec(dllexport) float CyberpunkVR_CarryBlendMs   = 320.0f;
// The damping of that blend. Under 1 it overshoots slightly before settling, and that overshoot is
// what makes a heavy thing feel heavy -- the same argument and the same shape as the hand recoil's
// spring, which runs at 0.55. A little stiffer here: this is a hand-over, not a kick.
extern "C" __declspec(dllexport) float CyberpunkVR_CarryBlendZeta = 0.65f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugCarryBlend = 0.0f;
// How near the support point the hand has to be for the fingers to offer the hold. It shipped at 12 cm
// on the argument that that is the reach of a hand which means it -- and in play that was too wide: the
// offer appeared for a hand merely passing the weapon, which is the failure the same note predicted for
// a larger value. 6 cm by feel. It is an ini key now (xr_two_hand_radius) because this is a
// number to settle by feel, and settling it by feel must not cost a rebuild per attempt.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandRadius  = 0.06f;
// What a second hand takes off the kick. Two hands roughly triple the effective mass resisting the same
// impulse and add a second lever against the muzzle rise, so a fifth of the one-handed flip is what the
// physics of it says.
// 0.286, not 0.2, and the change is arithmetic rather than taste: the one-handed angle came down by
// 30% and the two-handed result was to stay where it was, so the fraction goes up by the same 30%.
// 0.2 / 0.7 = 0.286, and a two-handed Lexington still peaks at the 4.4 deg it did before.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandRecoil  = 0.286f;
// How fast the finger preview fades in and out, seconds. Matches the reload module's own ramp so the two
// previews feel like one system.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandFadeS   = 0.15f;
// HOW LONG THE HOLD TAKES TO ARRIVE AND TO LEAVE, milliseconds, and how it settles. The hand and the
// aim used to switch on the frame the grip closed; a hand does not arrive instantly and neither does
// the authority it has over the weapon. Same spring shape as the recoil and the carry blend, so the
// three feel like one system.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandEngageMs = 220.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandEngageZeta = 0.75f;
// The live weight: 0 = the hand is not on the weapon at all, 1 = it fully is. Published so the ini
// tuner and the overlay can see it.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandWeight  = 0.0f;
// HOW MUCH LEVERAGE THE SUPPORT HAND HAS, 0..1: the captured hold's own offset length over
// TwoHandLeverFull. A pistol's second hand sits 7 cm from the wrist and a rifle's is out at 35, and
// that ratio is the whole difference between steadying a weapon and merely touching it. The aim
// correction below has always used it; the recoil reads it too, so a hand under a pistol grip stops
// damping the flip as hard as a hand on a handguard.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandLever   = 0.0f;
// LETTING GO IS QUICKER THAN TAKING HOLD, and not for symmetry's sake: a hand closing on a rifle
// settles onto it, while a hand opening simply stops steadying it. A release should be minimal,
// so the release gets its own, much shorter span.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandReleaseMs = 110.0f;
// The weapon coming back into the right hand: how long it takes to settle out of the left hand's hold,
// and how that settle damps. See the note above CarryReturnRight for what is being faded.
// 0 = the plugin does NOT move the right wrist on the way back. The weapon now travels by its own
// slot offset, eased in the right bone's frame by the weapon module, so the hand must not be displaced
// as well -- two springs on one hand-over fight each other. The derivation above and the code below are
// kept because they are the only record of how the pose relation cancels out, and because the moment
// the carry ever goes back to moving the item, this is the way to make the return smooth again.
extern "C" __declspec(dllexport) int   CyberpunkVR_CarryReturnHand = 1;
extern "C" __declspec(dllexport) float CyberpunkVR_CarryReturnMs   = 260.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_CarryReturnZeta = 0.70f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugCarryReturn = 0.0f;
// REACHING BACK FOR THE CARRIED WEAPON. Inside this radius the right hand is offered the weapon: its
// fingers close into the captured grip pose and the grip button takes the gun back. Same number for
// both, deliberately -- a preview that promises a take the button will not perform is a lie the player
// finds at the edge. The Lua gate reads it through GetVRCarryReach().
// 9 cm, halved from 18 by feel. An ini key (xr_carry_radius), because it is a number to
// settle by feel and that must not cost a rebuild per attempt.
extern "C" __declspec(dllexport) float CyberpunkVR_CarryGripRadius = 0.09f;
// ...and how the offer arrives: the same spring as everything else in this file, because a hand closing
// on a grip settles onto it. The pose preview blends in on that spring too.
extern "C" __declspec(dllexport) float CyberpunkVR_CarryGripMs     = 200.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_CarryGripZeta   = 0.80f;
extern "C" __declspec(dllexport) float CyberpunkVR_CarryGripBlend  = 0.0f;   // 0..1, read by the finger pass
extern "C" __declspec(dllexport) float CyberpunkVR_DebugCarryReach = -1.0f;  // metres, -1 = not carrying
// HOW MUCH OF THE AIM THE SUPPORT HAND OWNS. A third of the error, capped at 15 degrees, with the first
// two degrees ignored: enough that bringing the left hand up visibly settles and steers the weapon, far
// too little for it to take the weapon over and lay it on its side. The right hand stays the one holding
// the gun -- which is what it is doing.
// HOW MUCH OF THE AIM THE SUPPORT HAND OWNS -- AND IT IS DECIDED BY LEVERAGE, not by preference.
//
// The two ways this is done in VR are well known: most games snap the off hand to the weapon and let it
// affect nothing, while games with long guns (Pavlov and its like) aim the weapon along the line from the
// rear hand to the front one. Both are right, for different weapons, and the thing that separates them is
// the BASELINE -- how far apart the hands are.
//
// On a pistol both hands are on the same grip: measured here, 74 mm apart. A centimetre of tracking noise
// across 74 mm is 8 degrees, so aiming along that line is aiming along the noise -- which is exactly what
// "the pistol teleports sideways when I move my palm" was. On a rifle the support hand is out at the
// handguard, 350-450 mm away, where the same centimetre is under two degrees and the hand really does
// steer the weapon, because it really does have the leverage.
//
// So the gain SCALES with the captured baseline, in proportion to it: a pistol hold at 74 mm keeps
// about a fifth of the authority a rifle hold at 350 mm gets -- the first school and the second out of
// one rule, with no weapon list and no switch to set. The recoil the second hand absorbs is not gated on any of this -- holding a
// pistol with both hands steadies it whether or not it steers it.
// 1.0 AT FULL LEVERAGE: on a rifle the left hand controls the front and the right hand the back,
// as in life and in native VR games. At a rifle's baseline that is
// the whole error, i.e. the barrel really does follow the line between the hands -- the second school
// named above, in full, rather than six tenths of it. A pistol is untouched by this: its own leverage
// (74 mm of 350) still leaves it about a fifth, which is the first school.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimGain   = 1.00f;   // at full leverage
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandLeverFull = 0.35f;   // m, at this: all of it
// How quickly the push is allowed to become a correction, seconds. This is what makes a jump
// impossible: the aim can only ramp toward the hand, never step to it.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimTau    = 0.12f;
// THE CEILING AND THE DEADZONE ARE SCALED BY THE SAME LEVERAGE, and that is the point of them.
// Both existed to protect a PISTOL: 7 cm between the hands turns a centimetre of tracking noise into
// several degrees, so a hard 10 deg ceiling and a 4 deg deadzone were what kept it from shaking. On a
// rifle the same centimetre is under two degrees, the noise argument is gone, and those two numbers are
// simply a wall in front of the aim. So the ceiling is multiplied by the leverage and the deadzone is
// faded out by it: a pistol keeps 12.7 deg and 3.4, a rifle gets 60 deg and 1.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimMaxDeg = 60.0f;  // x leverage
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimDeadDeg = 4.0f;  // faded out by leverage
// HOW THE SUPPORT HAND STEERS THE BARREL. 0 = not at all: the weapon is the right controller's, one to
// one, and the support hand only closes on it and takes the kick. 1 = RELATIVE (the default): the barrel
// follows the left hand by how far it has moved AGAINST the right one since the grip closed. 2 = ABSOLUTE,
// the old behaviour: the barrel is laid on the line to the left hand from the first frame.
//
// 2 is what was seen: releasing the left grip made the weapon jump out onto the controllers'
// real pose. Laid on the line from the start, the weapon is turned by however far the player's hands
// sit from the captured animation's -- a constant the player never asked for -- so the drawn right hand is
// off the real one for the whole hold and the weapon travels back by all of it on release. 0 cured that
// and took the steering with it (the left hand could no longer correct the weapon). 1 keeps both: the squeeze
// steers nothing, hands moved together steer nothing, and what the weapon gives back on release is exactly
// what the left hand steered. ini: xr_two_hand_aim.
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandAim = 1;
// How long the steer takes to leave once the hand has, milliseconds -- critically damped, so the weapon
// settles onto the controller rather than swinging past it. Longer than the hand's own release
// (TwoHandReleaseMs): the hand simply lets go, while the weapon has a deliberate steer to give back.
// 400, doubled from 200 (the weapon came back too slowly). An ini key
// (xr_two_hand_aim_release_ms), because it is a number to settle by feel.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimReleaseMs = 400.0f;
// The steer last applied, degrees -- for the overlay and the tuner.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugTwoHandAimDeg = 0.0f;
// A HELD GRIP NEEDS AN END. The support point now follows the hand only by direction, so a squeeze held on
// a hand pulled half a metre away would leave the drawn one welded to the handguard. Beyond this distance
// from the support point the hand comes off -- held that far for BreakMs, so a recoil throw (a sniper's
// reaches ~14 cm for a moment) never counts. Still squeezing and back inside the grip radius, it takes
// hold again by itself. 0 = never. Not applied to mode 2, which keeps its old semantics. ini:
// xr_two_hand_break.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandBreakRadius = 0.60f;
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandBreakMs     = 250.0f;

// ---- live state, published so it can be read from outside ----
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandActive  = 0;      // 1 = the hand is on the gun
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandBlend   = 0.0f;   // finger preview ramp 0..1
extern "C" __declspec(dllexport) float CyberpunkVR_DebugTwoHandDist = -1.0f;
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandHave = 0;
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandCaptureReq = 0;
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandRefused = 0;
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandSaved = 0;
// How many captured holds were found at startup -- one per weapon anyone has recorded.
extern "C" __declspec(dllexport) int   CyberpunkVR_DebugTwoHandLoaded = 0;
// The baseline the last capture saw, millimetres -- the number that tells a hold from a hand at rest.
extern "C" __declspec(dllexport) float CyberpunkVR_DebugTwoHandBaseMm = -1.0f;

// A PUMP'S FORE-END RUN BACK UNDER THE SUPPORT HAND: a MODEL-space offset of the support point, set by the
// reload mod (SetVRTwoHandSlide) while the player racks the gun with the hand still closed on it. Racking a
// pump IS the support hand's job, so the hold stays shut through the stroke and its drawn wrist rides the
// fore-end. The steer and the reach are measured with the stroke taken back out of the hand, so working the
// action neither turns the barrel nor reads as letting go. A deadline rather than a flag: a mod that stops
// refreshing it cannot leave the hand displaced.
float    g_VRTwoHandSlide[3]   = {0.0f, 0.0f, 0.0f};
uint64_t g_VRTwoHandSlideUntil = 0;
// ...and THE LEFT CONTROLLER ITSELF, model space, as this hold read it. While the hold is closed every published
// left wrist is the weld -- it rides the gun -- so a stroke measured from it against the gun is always zero. The
// pump measures the real hand from this instead (GetVRTwoHandLeftCtrl).
float g_VRTwoHandCtrlL[3]   = {0.0f, 0.0f, 0.0f};
int   g_VRTwoHandCtrlLValid = 0;

namespace {

// ONE FILE PER WEAPON, AND EVERY FILE READ ONCE. A hold is a property of the weapon, not of the port: a
// pistol's support hand is on the same grip, a rifle's is out on the handguard 35 cm away. So the capture
// writes CyberpunkVR_TwoHandGrip_<weapon>.ini, and at startup every one of those that exists is read into
// the table below. Drawing a weapon then costs a name comparison -- no file is opened while playing, and a
// weapon nobody has captured simply has no two-hand hold rather than the previous weapon's.
struct Hold {
    char  weapon[64];
    // ONE WEAPON CAN BORROW ANOTHER'S HOLD, and some do: the Tsunami Kappa is an Arasaka Yukimura with a
    // different shell (it has no rig or anims of its own -- see the reload module's signature table), and
    // the Tamayura is a Nue the same way. A COPY of the file would work until the original is recaptured
    // and the copy quietly keeps the old hold, so the borrowed one is a REFERENCE: "ALIAS <weapon>" on
    // its own line, resolved when the weapon is selected. Recapture the original and both follow.
    char  alias[64];
    float off[3];
    float rot[4];
    float finger[32][4];
    char  fingerName[32][48];
    int   fingerCount;
};

// Room for primary, magazine and additional fore-end/handle points, plus captured user holds.
// Reject saturation explicitly rather than silently dropping later weapon families.
constexpr int kMaxHolds = 256;
Hold g_holds[kMaxHolds] = {};
int  g_holdCount = 0;
int  g_scanned   = 0;
int  g_saveReq   = 0;

// The row the weapon in hand selects, and the name it was selected for.
const Hold* g_cur = nullptr;
// A WEAPON MAY BE HELD IN MORE THAN ONE PLACE. Its other holds are files of their own,
// CyberpunkVR_TwoHandGrip_<weapon>@<where>.ini -- the Militech Crusher is held on its fore-end and, @mag, on
// its magazine (two holds: the fore-end handle and the magazine). All of them are picked up with the weapon, the
// plain one first and the rest by name; an open hand is offered the NEAREST, and the one that closed keeps
// the hand until it opens. `g_cur` is always the one in use, so nothing downstream had to learn about this.
// Twelve: a long gun has its fore-end, 35 mm either side of it, its magazine and the receiver ahead of the trigger
// (@trigger); a katana its handle every 25 mm down to the pommel (tools/hand_tests/generate_support_points.cpp). At
// four, the last ones read were never offered.
constexpr int kMaxCand = 12;
const Hold* g_cand[kMaxCand] = {};
int g_candN = 0;
const Hold* g_primary = nullptr;       // the plain one: what the left-hand carry is measured from
char g_curFor[64] = {1, 0};      // deliberately not a valid name, so the first pass always selects

// The support point, recomputed every pass the right hand is solved and read by the left one, which is
// solved after it. A frame-local hand-off, not state: both live inside one pose apply.
float g_supPos[3] = {0.0f, 0.0f, 0.0f};
float g_supRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
int   g_supValid  = 0;

// ...AND THE LAST ONE THAT WAS VALID, which is what a release has to fade toward. The pose above is
// cleared at the top of every pass, and the passes that end the hold -- the reload taking the hand, the
// carry taking the weapon -- return before setting it again. Without a copy that outlives them there is
// nothing left to ease out of, and the hand snaps instead.
float g_supPrevPos[3] = {0.0f, 0.0f, 0.0f};
float g_supPrevRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
int   g_supPrevHave = 0;

// The right grip as a squeeze rather than a level -- see the note in CarryReturnRight. Kept beside the
// left hand's pair so the two rules are read together.
int g_rGripWas   = 0;
int g_rGripFresh = 0;

// WHOSE SQUEEZE THE LEFT GRIP IS -- see the note at the top of TwoHandRight for what this fixes. A button
// is a LEVEL, and a level cannot tell a squeeze that has just been made from one that is merely still down.
int g_gripWas   = 0;      // the button as of the last pass, so a press can be told from a hold
int g_gripFresh = 0;      // 1 = the squeeze now down has not been spent by anyone yet

const char* WeaponKey() {
    return (CyberpunkVR_WeaponName[0]) ? CyberpunkVR_WeaponName : "default";
}

Hold* FindHold(const char* weapon) {
    for (int i = 0; i < g_holdCount; ++i)
        if (std::strncmp(g_holds[i].weapon, weapon, 63) == 0) return &g_holds[i];
    return nullptr;
}

Hold* AddHold(const char* weapon) {
    Hold* h = FindHold(weapon);
    if (h) return h;
    if (g_holdCount >= kMaxHolds) return nullptr;
    h = &g_holds[g_holdCount++];
    std::memset(h, 0, sizeof(*h));
    std::strncpy(h->weapon, weapon, 63);
    h->rot[3] = 1.0f;
    return h;
}

// SELECTION IS A NAME COMPARISON, done wherever it is needed, because it must be right on the very first
// pass after a draw -- the pose path cannot wait for a background tick to catch up.
void SelectHold() {
    const char* w = WeaponKey();
    if (std::strncmp(g_curFor, w, 63) == 0) return;
    std::strncpy(g_curFor, w, 63);
    g_curFor[63] = '\0';
    g_cur = FindHold(w);
    // Follow a borrowed hold, once: a chain would be a mistake worth catching rather than supporting, and
    // one hop covers every real case (a re-shelled weapon points at the original, never at another alias).
    if (g_cur && g_cur->alias[0] && g_cur->fingerCount == 0) {
        const Hold* src = FindHold(g_cur->alias);
        if (src) g_cur = src;
    }
    g_primary = g_cur;
    g_candN = 0;
    if (g_cur) g_cand[g_candN++] = g_cur;
    {
        // "<weapon>@..." rows, in name order (a handful of rows; a selection sort is plenty)
        const size_t wl = std::strlen(w);
        const Hold* alt[kMaxCand] = {};
        int altN = 0;
        for (int i = 0; i < g_holdCount && altN < kMaxCand; ++i) {
            const char* nm = g_holds[i].weapon;
            if (std::strncmp(nm, w, wl) == 0 && nm[wl] == '@' && g_holds[i].fingerCount > 0) alt[altN++] = &g_holds[i];
        }
        for (int a = 0; a < altN; ++a)
            for (int b = a + 1; b < altN; ++b)
                if (std::strcmp(alt[b]->weapon, alt[a]->weapon) < 0) { const Hold* t = alt[a]; alt[a] = alt[b]; alt[b] = t; }
        for (int a = 0; a < altN && g_candN < kMaxCand; ++a) g_cand[g_candN++] = alt[a];
    }
    if (!g_cur && g_candN > 0) g_cur = g_cand[0];
    CyberpunkVR_DebugTwoHandHoldIdx = -1;
    CyberpunkVR_DebugTwoHandHave = g_cur ? 1 : 0;
    CyberpunkVR_TwoHandActive = 0;
}

void ParseInto(Hold* h, FILE* f) {
    char line[256];
    while (std::fgets(line, sizeof(line), f)) {
        if (line[0] == '#' || line[0] == '\n' || line[0] == '\r') continue;
        float a, b, c, d, e, g, i;
        char nm[64] = {0};
        {
            char al[64] = {0};
            if (std::sscanf(line, "ALIAS %63s", al) == 1 && al[0]) {
                std::strncpy(h->alias, al, 63);
                continue;
            }
        }
        if (std::sscanf(line, "W %g %g %g %g %g %g %g", &a, &b, &c, &d, &e, &g, &i) == 7) {
            h->off[0]=a; h->off[1]=b; h->off[2]=c;
            h->rot[0]=d; h->rot[1]=e; h->rot[2]=g; h->rot[3]=i;
            VRIK_QuatNorm(h->rot);
            continue;
        }
        if (std::sscanf(line, "F %63s %g %g %g %g", nm, &a, &b, &c, &d) == 5 && h->fingerCount < 32) {
            std::strncpy(h->fingerName[h->fingerCount], nm, 47);
            h->finger[h->fingerCount][0]=a; h->finger[h->fingerCount][1]=b;
            h->finger[h->fingerCount][2]=c; h->finger[h->fingerCount][3]=d;
            ++h->fingerCount;
        }
    }
}

// THE HOLD AS THE WEAPON HAND HAS IT. Captured with the weapon in the right hand -- the left wrist in the right
// wrist's frame -- and, with the weapon in the left hand (Anim/LeftHanded.cpp), its mirror: the right wrist in the
// left wrist's frame. Everything below measures and places the support hand through this, so the hold, the steer
// and the release are the same code for both hands.
void HoldPose(const Hold* h, float* off, float* rot) {
    off[0] = h->off[0]; off[1] = h->off[1]; off[2] = h->off[2];
    rot[0] = h->rot[0]; rot[1] = h->rot[1]; rot[2] = h->rot[2]; rot[3] = h->rot[3];
    if (cvr::anim::WeaponInLeftHand()) cvr::anim::MirrorHeldHand(off, rot);
}

// The support hand in the reload module's numbering (0 left, 1 right).
int SupportHand() { return cvr::anim::WeaponInLeftHand() ? 1 : 0; }

// THE SUPPORT HAND'S GRIP. [155] is the left controller's, published by the input merge per physical hand; the
// right grip lives at the legacy [49] (the two were once read off one slot, which is how a lighter used to ignite
// itself). With the weapon in the left hand the support hand is the right one.
float LeftGripPressed() {
    return (g_pSharedHands ? g_pSharedHands[SupportHand() ? 49 : 155] : 0.0f);
}

// ...and the right one, for the carry: the hand that gives the weapon away and later takes it back.
float RightGripPressed() {
    return (g_pSharedHands ? g_pSharedHands[49] : 0.0f);
}

}  // namespace

namespace cvr {
namespace anim {

// Pick the hold for the weapon in hand, from outside the pose pass. SelectHold() itself is in the
// anonymous namespace above; this is the one thing that needs it from elsewhere in the DLL.
void SelectHoldForCarry() { SelectHold(); }

// CAPTURE. Runs inside the pose apply, with VRIK OFF so the buffer holds the game's own two-handed
// animation. FK is computed here rather than taken from the recorder's snapshot: the snapshot exists only
// while the recorder mod is running, and this must work with nothing else installed.
void TwoHandCapture(uint8_t* boneBuf) {
    if (!CyberpunkVR_TwoHandCaptureReq) return;
    CyberpunkVR_TwoHandCaptureReq = 0;

    // 1 no weapon (there is no two-handed pose to capture), 2 VRIK is writing the arms (the buffer holds
    // the controller, not the animation), 4 the rig's bones are not resolved yet.
    int why = 0;
    if (!g_hasWeaponEquipped)                          why |= 1;
    if (g_VRBind != 0)                                 why |= 2;
    if (g_VRLeftBoneIdx < 0 || g_VRRightBoneIdx < 0)   why |= 4;
    // SAY IT OUT LOUD. Every refusal used to leave only a number in an exported int, which is invisible
    // from inside the headset -- the same silence that made a full table look like a broken feature.
    // Log() rather than a gated probe, because a refusal is rare and is the one thing worth a line.
    if (why) {
        CyberpunkVR_DebugTwoHandRefused = why;
        Log("TwoHandCapture REFUSED (%d):%s%s%s  weapon='%s' holds=%d/%d\n", why,
            (why & 1) ? " no-weapon" : "", (why & 2) ? " vrik-on(g_VRBind!=0)" : "",
            (why & 4) ? " bones-unresolved" : "", WeaponKey(), g_holdCount, kMaxHolds);
        return;
    }

    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    const float* pR = g_fkPos[g_VRRightBoneIdx];
    const float* qR = g_fkRot[g_VRRightBoneIdx];
    const float* pL = g_fkPos[g_VRLeftBoneIdx];
    const float* qL = g_fkRot[g_VRLeftBoneIdx];

    // 16 = THE HANDS ARE NOT ON THE SAME WEAPON. A two-handed hold puts the wrists centimetres apart --
    // 37 to 74 mm across the pistols measured here, and 350-450 mm on a rifle's handguard. When the game
    // is playing a ONE-handed stance the left arm hangs at the hip, and the capture then records that:
    // three weapons came back at 679, 690 and 691 mm, which would have planted the support hand two thirds
    // of a metre from the gun. Nothing about those numbers looks wrong from inside a single capture, which
    // is exactly why the check belongs here rather than in the eye.
    {
        const float dx = pL[0]-pR[0], dy = pL[1]-pR[1], dz = pL[2]-pR[2];
        const float base = std::sqrt(dx*dx + dy*dy + dz*dz);
        CyberpunkVR_DebugTwoHandBaseMm = base * 1000.0f;
        if (base > 0.60f) {
            CyberpunkVR_DebugTwoHandRefused = 16;
            Log("TwoHandCapture REFUSED (16): wrists %.0f mm apart -- that is a one-handed stance, not a "
                "hold. weapon='%s'\n", base * 1000.0f, WeaponKey());
            return;
        }
    }

    Hold* h = AddHold(WeaponKey());
    if (!h) {
        CyberpunkVR_DebugTwoHandRefused = 8;
        Log("TwoHandCapture REFUSED (8): the hold table is FULL -- %d of %d used, weapon='%s'\n",
            g_holdCount, kMaxHolds, WeaponKey());
        return;
    }

    float qRc[4]; VRIK_QuatConj(qR, qRc);
    const float d[3] = { pL[0] - pR[0], pL[1] - pR[1], pL[2] - pR[2] };
    VRIK_QuatRotateVec(qRc, d, h->off);
    VRIK_QuatMul(qRc, qL, h->rot); VRIK_QuatNorm(h->rot);

    h->fingerCount = 0;
    for (int k = 0; k < g_VRSmokeFingerCountL && k < 32; ++k) {
        const int bi = g_VRSmokeFingerIdxL[k];
        if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
        const float* q = reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
        h->finger[h->fingerCount][0] = q[0]; h->finger[h->fingerCount][1] = q[1];
        h->finger[h->fingerCount][2] = q[2]; h->finger[h->fingerCount][3] = q[3];
        std::strncpy(h->fingerName[h->fingerCount], g_VRSmokeFingerNameL[k], 47);
        ++h->fingerCount;
    }
    g_cur = h;
    std::strncpy(g_curFor, h->weapon, 63);
    CyberpunkVR_DebugTwoHandHave = 1;
    CyberpunkVR_DebugTwoHandRefused = 0;
    g_saveReq = 1;
    Log("TwoHandCapture OK: weapon='%s' base=%.0f mm fingers=%d holds=%d/%d\n",
        h->weapon, CyberpunkVR_DebugTwoHandBaseMm, h->fingerCount, g_holdCount, kMaxHolds);
}

// WHAT THE LEFT-HAND CARRY NEEDS, straight out of the captured hold.
//
// The weapon hangs at the RIGHT hand and `off` is where the left hand holds it, in the right wrist's
// frame. Moving the weapon to the left hand's slot means the attachment point becomes the LEFT hand,
// so the slot has to carry the weapon back by that same distance -- the opposite vector, in the left
// wrist's frame:  conj(rot) * (-off).
//
// Returns 0 when this weapon has no captured hold: there is then no recorded place to preserve, and
// inventing one would put the gun somewhere nobody chose.
extern "C" float VRTwoHandCarryOffset(int idx) {
    cvr::anim::SelectHoldForCarry();
    const Hold* h = g_primary ? g_primary : g_cur;
    if (!h || idx < 0 || idx > 2) return 0.0f;
    const float neg[3] = { -h->off[0], -h->off[1], -h->off[2] };
    float rc[4]; VRIK_QuatConj(h->rot, rc);
    float out[3]; VRIK_QuatRotateVec(rc, neg, out);
    return out[idx];
}

// THE STEER'S STATE, out here rather than in the function, because the early returns below have to be able
// to clear it: a hand taken by the reload or the carry ends the hold without ever reaching the release
// path, and a steer left standing would fade in out of nowhere on the next pass that does.
namespace {
float s_aimX = 0.0f, s_aimV = 0.0f;                // how much of the steer is on the weapon, and its rate
float s_steerAxis[3] = {0.0f, 0.0f, 1.0f};          // the steer, in the bone frame
float s_steerAng = 0.0f;
float s_refE[3] = {0.0f, 0.0f, 0.0f};               // where the left hand was, in the bone frame, at the squeeze
float s_dL[3] = {0.0f, 0.0f, 0.0f};
int   s_refHave = 0;

void SteerReset() {
    s_aimX = 0.0f; s_aimV = 0.0f;
    s_steerAng = 0.0f;
    s_refHave = 0;
    CyberpunkVR_DebugTwoHandAimDeg = 0.0f;
}
}  // namespace

// THE RIGHT HAND'S HALF, called with the hand already built from its own controller.
//
// `hm` is the controller's orientation in model space, whose +Y is the barrel (the weapon rides this hand
// bone). Engaged, it is turned by the left hand's steer (see CyberpunkVR_TwoHandAim) -- the smallest rotation
// that does it, so the roll the right wrist has is untouched. Then the support point is stored for the left
// hand, which is solved a few hundred lines later in the same pass.
void TwoHandRight(const float* targetR, float* hm, const float* wristR, const float* leftCtrlModel) {
    // The ramp needs elapsed time and this site is entered several times per tick, so it is measured here
    // rather than passed in: a per-pass dt taken from the caller would advance the fade once per PASS and
    // make the fade rate depend on how many passes the game happens to run.
    float dt = 0.016f;
    {
        static LARGE_INTEGER s_f = {};
        static LARGE_INTEGER s_prev = {};
        if (s_f.QuadPart == 0) QueryPerformanceFrequency(&s_f);
        LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
        if (s_prev.QuadPart != 0 && s_f.QuadPart)
            dt = (float)((double)(now.QuadPart - s_prev.QuadPart) / (double)s_f.QuadPart);
        s_prev = now;
        if (dt < 0.0f || dt > 0.25f) dt = 0.016f;
    }
    g_supValid = 0;
    if (leftCtrlModel) {
        g_VRTwoHandCtrlL[0] = leftCtrlModel[0]; g_VRTwoHandCtrlL[1] = leftCtrlModel[1]; g_VRTwoHandCtrlL[2] = leftCtrlModel[2];
    }
    g_VRTwoHandCtrlLValid = leftCtrlModel ? 1 : 0;

    // THE WEIGHT, INTEGRATED ONCE PER PASS. A second-order spring pulled toward 1 while the hold is
    // engaged and toward 0 while it is not: x'' = -w^2 (x - goal) - 2*zeta*w*x'. Under-damped on
    // purpose -- a hand closing on a rifle settles, it does not click into place.
    {
        static float s_x = 0.0f, s_v = 0.0f;
        const float goal = (CyberpunkVR_TwoHandActive != 0) ? 1.0f : 0.0f;
        const float askMs = (goal > 0.5f) ? CyberpunkVR_TwoHandEngageMs : CyberpunkVR_TwoHandReleaseMs;
        const float span = (askMs > 1.0f) ? askMs : 1.0f;
        float zeta = CyberpunkVR_TwoHandEngageZeta;
        if (zeta < 0.05f) zeta = 0.05f;
        if (zeta > 1.5f)  zeta = 1.5f;
        const float wn = 6.28318531f / (span * 0.001f);
        // fixed substeps for the same reason the recoil uses them: a frame is too coarse for a spring
        // this stiff, and integrating once per frame makes the settle depend on the frame rate
        int steps = static_cast<int>(dt / 0.002f) + 1;
        if (steps > 64) steps = 64;
        const float sdt = dt / static_cast<float>(steps);
        for (int i = 0; i < steps; ++i) {
            const float a = -wn * wn * (s_x - goal) - 2.0f * zeta * wn * s_v;
            s_v += a * sdt;
            s_x += s_v * sdt;
        }
        if (s_x < 0.0f) { s_x = 0.0f; if (s_v < 0.0f) s_v = 0.0f; }
        if (s_x > 1.2f) s_x = 1.2f;
        CyberpunkVR_TwoHandWeight = s_x;
    }

    // A SQUEEZE IS SPENT BY WHOEVER USED IT, AND THAT IS DECIDED BEFORE ANYTHING BELOW CAN RETURN.
    //
    // The rule this file states further down is "engage on a squeeze inside the radius". What it did was
    // `pressed && inRange`, which is not that rule: it engages on a squeeze that is merely STILL DOWN. So
    // seating a magazine and keeping hold of the grip handed the weapon the very squeeze that had just
    // carried the magazine in -- the reload let the left hand go, the button was still down, and the support
    // grip closed on the gun by itself with the player having asked for nothing.
    //
    // Hence: a squeeze becomes usable at the instant it goes down and stops being usable the moment the
    // reload takes the hand with it. Nothing else spends it, so the one thing the level rule was good for
    // survives -- squeeze off the weapon, bring the hand in, and it still engages when it arrives.
    //
    // AND IT IS TRACKED HERE, ABOVE THE EARLY RETURNS, because that is the whole difficulty. Read after the
    // reload check and the tracking has the same hole as the bug: through the entire carry this function
    // returned without looking, so the pass after the seat could not tell that squeeze from a new one.
    {
        const bool down = LeftGripPressed() > 0.5f;
        if (!down)                          g_gripFresh = 0;      // released: nothing is held to spend
        else if (!g_gripWas)                g_gripFresh = 1;      // just pressed: it belongs to no one yet
        if (down && g_VRReloadFingerActive[SupportHand()]) g_gripFresh = 0;   // ...and the reload has taken it
        g_gripWas = down ? 1 : 0;
    }

    // THE RELOAD OWNS THIS HAND WHILE IT IS WORKING, and that is not a courtesy -- the two features want the
    // same wrist for opposite reasons. Bringing a magazine to the well passes straight through the support
    // point, so without this the grip snapped shut on the way in and the magazine was carried by a hand
    // welded to the pistol (the two-handed magnet caught). `g_VRReloadFingerActive[0]` is the reload
    // module saying it has the left hand: a magazine held, a slide gripped, or a preview being offered.
    if (g_VRReloadFingerActive[SupportHand()]) {
        CyberpunkVR_TwoHandActive = 0;
        CyberpunkVR_TwoHandBlend = 0.0f;
        SteerReset();
        return;
    }
    // ...and the carry owns it too, for the same reason and by the same means -- the weapon's carry, and
    // a carried BODY, whose animation holds that arm under it (AnimPose.cpp).
    if ((CyberpunkVR_TwoHandSuppress & 1) ||
        cvr::anim::ShouldHandLeftArmToBodyCarry(g_VRBodyCarryingState, CyberpunkVR_VrikBodyCarryHandOff)) {
        CyberpunkVR_TwoHandActive = 0;
        CyberpunkVR_TwoHandBlend = 0.0f;
        SteerReset();
        return;
    }
    // AND WHILE THE WEAPON IS IN THE LEFT HAND THERE IS NO SUPPORT HAND AT ALL. The hold welds the left
    // wrist to a point measured from the RIGHT hand, on the assumption that the right hand is the one
    // holding the gun. Through a carry that assumption is false, so the weld dragged the left wrist --
    // and the weapon hanging off it -- toward a point beside an empty hand, which is what made the
    // return look like a teleport whenever the grip was still held.
    //
    // Note what is NOT done here: g_gripFresh is left alone. It is tracked above every early return, so
    // a squeeze held right through the carry is still unspent when the weapon comes back, and the hold
    // takes it again by itself instead of needing a new press.
    if (CyberpunkVR_CarryLeft) {
        CyberpunkVR_TwoHandActive = 0;
        CyberpunkVR_TwoHandBlend = 0.0f;
        SteerReset();
        return;
    }
    // NOR WITH A GUN IN EACH HAND: the left hand holds its own. The hold would open its fingers off that gun as soon
    // as the hands came close, and a squeeze would weld the hand to the right gun with the left one hanging off it.
    if (cvr::anim::TwoGunsInHand()) {
        CyberpunkVR_TwoHandActive = 0;
        CyberpunkVR_TwoHandBlend = 0.0f;
        SteerReset();
        return;
    }
    SelectHold();
    if (!CyberpunkVR_TwoHandGrip || !g_cur || !g_hasWeaponEquipped || !targetR || !hm) {
        CyberpunkVR_TwoHandActive = 0;
        CyberpunkVR_TwoHandBlend = 0.0f;
        SteerReset();
        return;
    }

    // THE FORE-END'S STROKE, taken out of the hand before anything is measured against it -- see
    // g_VRTwoHandSlide. Only while the hold is closed: an open hand is not on the fore-end.
    float slide[3] = {0.0f, 0.0f, 0.0f};
    if (CyberpunkVR_TwoHandActive && GetTickCount64() < g_VRTwoHandSlideUntil) {
        slide[0] = g_VRTwoHandSlide[0]; slide[1] = g_VRTwoHandSlide[1]; slide[2] = g_VRTwoHandSlide[2];
    }
    float leftHand[3] = {0.0f, 0.0f, 0.0f};
    const float* leftAt = nullptr;
    if (leftCtrlModel) {
        leftHand[0] = leftCtrlModel[0] - slide[0];
        leftHand[1] = leftCtrlModel[1] - slide[1];
        leftHand[2] = leftCtrlModel[2] - slide[2];
        leftAt = leftHand;
    }

    // Where the support hand WOULD sit with the weapon as it is now: that is what the player reaches for,
    // so it is what the distance is measured against.
    float hr[4] = { hm[0], hm[1], hm[2], hm[3] };
    const bool pressed = LeftGripPressed() > 0.5f;

    // THE OFFSET LIVES IN THE BONE'S FRAME, NOT THE CONTROLLER'S, and that distinction is the difference
    // between a hand ON the grip and a hand a few centimetres beside it -- which is exactly how the first
    // version looked. At capture the relation was measured between the two WRIST BONES; here the right
    // wrist bone is `hm * wristR`, the controller turned by the calibration that makes a real hand line up
    // with the rig's. Replaying a bone-frame offset on the controller frame rotates it by that correction
    // and misses by an arm's worth of it.
    float bone[4];
    if (wristR) { VRIK_QuatMul(hr, wristR, bone); VRIK_QuatNorm(bone); }
    else        { bone[0]=hr[0]; bone[1]=hr[1]; bone[2]=hr[2]; bone[3]=hr[3]; }

    // WHICH OF THE WEAPON'S HOLDS (see g_cand). Closed, the one that closed -- unless it has just been taken
    // away (a magazine hold whose magazine has been dropped), and then the hand lets go. Open, the nearest one
    // still on offer; changed only while nothing is being offered or when the new one is in reach itself, so a
    // hand passing between two holds is never shown one grip's fingers at the other's place.
    {
        int curK = -1;
        for (int k = 0; k < g_candN; ++k) if (g_cand[k] == g_cur) curK = k;
        auto avail = [](int k) { return ((CyberpunkVR_TwoHandSuppress >> (k + 1)) & 1) == 0; };
        if (CyberpunkVR_TwoHandActive && curK >= 0 && !avail(curK)) CyberpunkVR_TwoHandActive = 0;
        if (!CyberpunkVR_TwoHandActive && g_candN > 0) {
            int best = -1;
            float bestD = 1e9f;
            for (int k = 0; k < g_candN; ++k) {
                if (!avail(k)) continue;
                float ho[3], hq[4]; HoldPose(g_cand[k], ho, hq);
                float o[3]; VRIK_QuatRotateVec(bone, ho, o);
                float d = 0.0f;
                if (leftAt) {
                    const float dx = leftAt[0] - (targetR[0] + o[0]);
                    const float dy = leftAt[1] - (targetR[1] + o[1]);
                    const float dz = leftAt[2] - (targetR[2] + o[2]);
                    d = std::sqrt(dx*dx + dy*dy + dz*dz);
                }
                if (best < 0 || d < bestD) { best = k; bestD = d; }
            }
            if (best < 0) {                       // every hold this weapon has is taken away
                CyberpunkVR_TwoHandBlend = 0.0f;
                CyberpunkVR_DebugTwoHandHoldIdx = -1;
                CyberpunkVR_DebugTwoHandDist = -1.0f;
                SteerReset();
                return;
            }
            if (curK < 0 || !avail(curK) || CyberpunkVR_TwoHandBlend <= 0.01f || bestD <= CyberpunkVR_TwoHandRadius) {
                g_cur = g_cand[best];
                curK = best;
            }
        }
        CyberpunkVR_DebugTwoHandHoldIdx = curK;
    }

    float holdOff[3], holdRot[4]; HoldPose(g_cur, holdOff, holdRot);
    float off[3]; VRIK_QuatRotateVec(bone, holdOff, off);
    float sup[3] = { targetR[0] + off[0], targetR[1] + off[1], targetR[2] + off[2] };

    // The hold's leverage, published for the recoil. Same quantity the aim correction computes further
    // down as `lev`; hoisted here because it is a property of the hold and not of the correction.
    {
        const float rl = std::sqrt(g_cur->off[0]*g_cur->off[0] + g_cur->off[1]*g_cur->off[1]
                                 + g_cur->off[2]*g_cur->off[2]);
        float lev = (CyberpunkVR_TwoHandLeverFull > 1e-3f) ? (rl / CyberpunkVR_TwoHandLeverFull) : 1.0f;
        if (lev > 1.0f) lev = 1.0f;
        if (lev < 0.0f) lev = 0.0f;
        CyberpunkVR_TwoHandLever = lev;
    }

    float dist = -1.0f;
    if (leftAt) {
        const float dx = leftAt[0] - sup[0];
        const float dy = leftAt[1] - sup[1];
        const float dz = leftAt[2] - sup[2];
        dist = std::sqrt(dx*dx + dy*dy + dz*dz);
    }
    CyberpunkVR_DebugTwoHandDist = dist;

    // ENGAGE on a squeeze inside the radius; HOLD until it is released, wherever the hand then goes -- short
    // of being pulled clear of the weapon altogether (CyberpunkVR_TwoHandBreakRadius, further down). A
    // grip that let go the moment the hand drifted out of a sphere would be a grip that cannot be used.
    // `g_gripFresh` is what makes "a squeeze" mean a squeeze rather than a button that happens to be down --
    // see the note at the top of this function.
    const bool inRange = (dist >= 0.0f && dist <= CyberpunkVR_TwoHandRadius);
    bool engagedNow = false;      // this pass closed the hold: the steer is measured from here
    if (CyberpunkVR_TwoHandActive) {
        if (!pressed) CyberpunkVR_TwoHandActive = 0;
    } else if (pressed && inRange && g_gripFresh) {
        CyberpunkVR_TwoHandActive = 1;
        engagedNow = true;
    }

    // The finger ramp: full while held, and while merely offered it follows the same fade the reload
    // previews use, so the two systems look like one.
    //
    // A SPENT SQUEEZE IS OFFERED NOTHING. Held down and already used, the hand cannot take the grip however
    // near it is, and closing the fingers onto the hold anyway is the system saying it just did. That is the
    // half of the bug the player actually SEES; the offer only returns when the button does.
    const bool canTake = !pressed || g_gripFresh;
    const float step = (CyberpunkVR_TwoHandFadeS > 0.01f) ? (dt / CyberpunkVR_TwoHandFadeS) : 1.0f;
    const float want = CyberpunkVR_TwoHandActive ? 1.0f : ((inRange && canTake) ? 1.0f : 0.0f);
    if (CyberpunkVR_TwoHandBlend < want) {
        CyberpunkVR_TwoHandBlend += step;
        if (CyberpunkVR_TwoHandBlend > want) CyberpunkVR_TwoHandBlend = want;
    } else if (CyberpunkVR_TwoHandBlend > want) {
        CyberpunkVR_TwoHandBlend -= step;
        if (CyberpunkVR_TwoHandBlend < want) CyberpunkVR_TwoHandBlend = want;
    }

    // HOW MUCH OF THE STEER IS ON THE WEAPON, 0..1 -- its own spring, not the hand's weight.
    //
    // In with the hold, at the hold's pace. OUT ON ITS OWN, AND LATCHED: the steer last measured while the
    // hand was on the weapon is what fades, never a steer toward wherever the opening hand happens to be
    // going. Following the live hand through the release swung the barrel after a hand that had already
    // let go, on top of the return itself. Critically damped on the way out, so the weapon arrives on the
    // controller instead of swinging past it.
    {
        const bool on = CyberpunkVR_TwoHandAim != 0 && CyberpunkVR_TwoHandActive != 0;
        const float goal = on ? 1.0f : 0.0f;
        const float askMs = on ? CyberpunkVR_TwoHandEngageMs : CyberpunkVR_TwoHandAimReleaseMs;
        const float span = (askMs > 1.0f) ? askMs : 1.0f;
        float zeta = on ? CyberpunkVR_TwoHandEngageZeta : 1.0f;
        if (zeta < 0.05f) zeta = 0.05f;
        if (zeta > 1.5f)  zeta = 1.5f;
        const float wn = 6.28318531f / (span * 0.001f);
        int steps = static_cast<int>(dt / 0.002f) + 1;
        if (steps > 64) steps = 64;
        const float sdt = dt / static_cast<float>(steps);
        for (int i = 0; i < steps; ++i) {
            const float a = -wn * wn * (s_aimX - goal) - 2.0f * zeta * wn * s_aimV;
            s_aimV += a * sdt;
            s_aimX += s_aimV * sdt;
        }
        if (s_aimX < 0.0f) { s_aimX = 0.0f; if (s_aimV < 0.0f) s_aimV = 0.0f; }
        if (s_aimX > 1.2f) s_aimX = 1.2f;
    }

    // THE STEER, measured only while the hand is actually on the weapon. Kept as an axis and an angle in
    // the BONE's frame, so the release can scale it down without measuring anything again.
    if (CyberpunkVR_TwoHandAim && CyberpunkVR_TwoHandActive && leftAt) {
        // LOW-PASSED IN THE BONE'S OWN FRAME. Smoothing a model-space direction would fight the hand's
        // motion (the frame moves under the filter); in the wrist's frame only the player's push varies,
        // so the filter has nothing to chase. It makes a jump impossible: the steer can only ramp.
        //
        // SCALED BY THE LEVERAGE, like the deadzone and the ceiling below, and for their reason. This is all
        // the drawn left hand follows, so its whole constant reads as that hand trailing the right one. A
        // pistol needs it, 7 cm between the hands being mostly noise; at a rifle's baseline a quarter is plenty.
        const float rl = std::sqrt(g_cur->off[0]*g_cur->off[0] + g_cur->off[1]*g_cur->off[1]
                                + g_cur->off[2]*g_cur->off[2]);
        float d[3] = { leftAt[0] - targetR[0],
                       leftAt[1] - targetR[1],
                       leftAt[2] - targetR[2] };
        const float dl = std::sqrt(d[0]*d[0] + d[1]*d[1] + d[2]*d[2]);
        if (rl > 1e-4f && dl > 1e-4f) {
            float bc[4]; VRIK_QuatConj(bone, bc);
            const float dm[3] = { d[0]/dl, d[1]/dl, d[2]/dl };
            float dL[3]; VRIK_QuatRotateVec(bc, dm, dL);                       // the hand, in the bone frame
            if (engagedNow || !s_refHave) {
                s_refE[0]=dL[0]; s_refE[1]=dL[1]; s_refE[2]=dL[2];
                s_dL[0]=dL[0];   s_dL[1]=dL[1];   s_dL[2]=dL[2];
                s_refHave = 1;
            }
            float tauLever = (CyberpunkVR_TwoHandLeverFull > 1e-3f) ? (rl / CyberpunkVR_TwoHandLeverFull) : 1.0f;
            if (tauLever > 1.0f) tauLever = 1.0f;
            const float scaled = CyberpunkVR_TwoHandAimTau * (1.0f - 0.75f * tauLever);
            const float tau = (scaled > 0.01f) ? scaled : 0.01f;
            const float a = 1.0f - std::exp(-dt / tau);
            for (int i = 0; i < 3; ++i) s_dL[i] += (dL[i] - s_dL[i]) * a;

            // WHAT THE STEER IS MEASURED FROM -- and this is the whole difference between the two modes.
            //
            // 1 (relative): from where the left hand WAS when the grip closed. Nothing is steered at the
            // squeeze, so the weapon stays on the right controller exactly; moving both hands together --
            // tilting, turning, canting the gun -- keeps that relation and steers nothing either. Only
            // the left hand moving AGAINST the right turns the barrel, by what it moved. So the drawn
            // hands show the controllers until the player steers, and letting go returns only that steer.
            //
            // 2 (absolute, the old behaviour): from the captured hold. The barrel is put on the line to the
            // hand from the first frame, which turns the weapon by however far the player's hands sit from
            // the animation's -- the reported mismatch, and the jump on release that undid it.
            //
            // Either way the reference is the support point's direction, never an assumed barrel axis: an
            // axis that is not exactly the barrel leaves an error that never goes to zero, pins the steer at
            // its ceiling and swings its axis with every centimetre of palm motion.
            float refL[3];
            if (CyberpunkVR_TwoHandAim == 2) {
                refL[0] = holdOff[0]/rl; refL[1] = holdOff[1]/rl; refL[2] = holdOff[2]/rl;
            } else {
                const float el = std::sqrt(s_refE[0]*s_refE[0] + s_refE[1]*s_refE[1] + s_refE[2]*s_refE[2]);
                const float ei = (el > 1e-6f) ? 1.0f / el : 0.0f;
                refL[0] = s_refE[0]*ei; refL[1] = s_refE[1]*ei; refL[2] = s_refE[2]*ei;
            }
            s_steerAng = 0.0f;
            const float sl = std::sqrt(s_dL[0]*s_dL[0] + s_dL[1]*s_dL[1] + s_dL[2]*s_dL[2]);
            if (sl > 1e-5f) {
                const float ds[3] = { s_dL[0]/sl, s_dL[1]/sl, s_dL[2]/sl };
                const float dot = refL[0]*ds[0] + refL[1]*ds[1] + refL[2]*ds[2];
                const float axisL[3] = { refL[1]*ds[2] - refL[2]*ds[1],
                                         refL[2]*ds[0] - refL[0]*ds[2],
                                         refL[0]*ds[1] - refL[1]*ds[0] };
                const float al = std::sqrt(axisL[0]*axisL[0] + axisL[1]*axisL[1] + axisL[2]*axisL[2]);
                if (al > 1e-6f && dot > -0.999f) {
                    float ang = std::atan2(al, dot);
                    // THE OFF HAND STEERS IN PROPORTION TO ITS LEVERAGE. Torque is force times lever: a
                    // hand 74 mm from the wrist (a pistol) has about a fifth of the authority the same hand
                    // has out on a rifle handguard at 350 mm -- no floor and no threshold, because a
                    // supporting hand on a pistol DOES move the point of aim. The deadzone and the ceiling
                    // are functions of the same leverage: tracking noise across 7 cm is several degrees and
                    // correcting noise is shake, while on a rifle the same centimetre is under two.
                    float lev = (CyberpunkVR_TwoHandLeverFull > 1e-3f)
                              ? (rl / CyberpunkVR_TwoHandLeverFull) : 1.0f;
                    if (lev > 1.0f) lev = 1.0f;
                    if (lev < 0.0f) lev = 0.0f;
                    const float dead = CyberpunkVR_TwoHandAimDeadDeg * (1.0f - 0.75f * lev)
                                       * 0.01745329252f;
                    const float cap  = CyberpunkVR_TwoHandAimMaxDeg * lev * 0.01745329252f;
                    ang = (ang > dead) ? (ang - dead) : 0.0f;
                    ang *= CyberpunkVR_TwoHandAimGain * lev;
                    if (ang > cap) ang = cap;
                    s_steerAxis[0] = axisL[0]/al; s_steerAxis[1] = axisL[1]/al; s_steerAxis[2] = axisL[2]/al;
                    s_steerAng = ang;
                }
            }
        }
    }
    if (!CyberpunkVR_TwoHandActive) s_refHave = 0;      // the next squeeze measures from its own hand

    // ...AND THE STEER GOES ONTO THE WEAPON by however much of it has arrived (or is still leaving).
    // Built in the BONE frame and applied there, so it is a push on the weapon rather than a rotation
    // about some world axis that happens to be nearby; then back out to the controller frame the caller
    // composes the hand from: hm = bone * conj(wristR).
    {
        float aw = s_aimX;
        if (aw < 0.0f) aw = 0.0f;
        if (aw > 1.0f) aw = 1.0f;
        const float ang = s_steerAng * aw;
        CyberpunkVR_DebugTwoHandAimDeg = ang * 57.2957795f;
        if (ang > 1e-5f) {
            const float s = std::sin(ang * 0.5f);
            const float qL[4] = { s_steerAxis[0]*s, s_steerAxis[1]*s, s_steerAxis[2]*s, std::cos(ang * 0.5f) };
            float nb[4]; VRIK_QuatMul(bone, qL, nb); VRIK_QuatNorm(nb);
            bone[0]=nb[0]; bone[1]=nb[1]; bone[2]=nb[2]; bone[3]=nb[3];
            if (wristR) {
                const float wc[4] = { -wristR[0], -wristR[1], -wristR[2], wristR[3] };
                float nh[4]; VRIK_QuatMul(bone, wc, nh); VRIK_QuatNorm(nh);
                hr[0]=nh[0]; hr[1]=nh[1]; hr[2]=nh[2]; hr[3]=nh[3];
            } else {
                hr[0]=bone[0]; hr[1]=bone[1]; hr[2]=bone[2]; hr[3]=bone[3];
            }
            hm[0]=hr[0]; hm[1]=hr[1]; hm[2]=hr[2]; hm[3]=hr[3];
            // ...and the support point is recomputed from the AIMED hand: the hand is drawn where the
            // weapon now is.
            VRIK_QuatRotateVec(bone, holdOff, off);
            sup[0] = targetR[0] + off[0]; sup[1] = targetR[1] + off[1]; sup[2] = targetR[2] + off[2];
        }
    }

    // A HELD GRIP ENDS WHERE THE HAND DOES -- see CyberpunkVR_TwoHandBreakRadius. Measured against the
    // support point as drawn (steer included), so a hand that is steering is still on the weapon.
    // Effective from the next pass; this one has already been composed.
    {
        static float s_farMs = 0.0f;
        bool away = false;
        if (CyberpunkVR_TwoHandActive && CyberpunkVR_TwoHandAim != 2 && leftAt
            && CyberpunkVR_TwoHandBreakRadius > 0.0f) {
            const float dx = leftAt[0] - sup[0];
            const float dy = leftAt[1] - sup[1];
            const float dz = leftAt[2] - sup[2];
            away = std::sqrt(dx*dx + dy*dy + dz*dz) > CyberpunkVR_TwoHandBreakRadius;
        }
        s_farMs = away ? s_farMs + dt * 1000.0f : 0.0f;
        if (away && s_farMs >= CyberpunkVR_TwoHandBreakMs) { CyberpunkVR_TwoHandActive = 0; s_farMs = 0.0f; }
    }

    // ...and the hand is DRAWN where the fore-end is
    g_supPos[0] = sup[0] + slide[0]; g_supPos[1] = sup[1] + slide[1]; g_supPos[2] = sup[2] + slide[2];
    // The wrist rotation follows the same relation as the offset, so the hand lands on the grip the way
    // the animation had it rather than merely near it.
    VRIK_QuatMul(bone, holdRot, g_supRot); VRIK_QuatNorm(g_supRot);
    g_supValid = 1;
    g_supPrevPos[0] = g_supPos[0]; g_supPrevPos[1] = g_supPos[1]; g_supPrevPos[2] = g_supPos[2];
    g_supPrevRot[0] = g_supRot[0]; g_supPrevRot[1] = g_supRot[1];
    g_supPrevRot[2] = g_supRot[2]; g_supPrevRot[3] = g_supRot[3];
    g_supPrevHave = 1;
}

// COMING OUT OF THE HOLD WITHOUT A JUMP.
//
// While the weld is on, the drawn left wrist is turned to fit the weapon (g_supRot) while the player's
// controller is held however they like -- usually straight. The weapon is parented to that wrist, so
// letting the weld go teleports the gun. This latches the welded rotation on the frame the carry
// starts and fades it out, so the wrist -- and the weapon with it -- travels from the hold to the
// controller's own orientation.
//
// Smoothstep, so both ends have zero velocity: leaving the hold and arriving at the hand are equally
// free of corners. Position is untouched -- the controller's position was always right.
namespace {
float g_carryRot0[4] = {0.0f, 0.0f, 0.0f, 1.0f};
int   g_carryHave = 0;
double g_carryStartMs = 0.0;
int   g_carryWas = 0;

double CarryNowMs() {
    static LARGE_INTEGER f = {};
    if (f.QuadPart == 0) QueryPerformanceFrequency(&f);
    LARGE_INTEGER n{}; QueryPerformanceCounter(&n);
    return f.QuadPart ? (double)n.QuadPart * 1000.0 / (double)f.QuadPart : 0.0;
}
}  // namespace

extern "C" void CarryLeftBlend(float* handRot) {
    const int on = (CyberpunkVR_CarryLeft != 0) ? 1 : 0;
    if (on && !g_carryWas) {
        // The hold as it last stood. THE STICKY COPY, not the live one: the pass that sees this flag
        // raised is also the pass where the hold gives the weapon up, so the live pose has already been
        // cleared and latching from it caught nothing at all.
        if (g_supPrevHave) {
            g_carryRot0[0] = g_supPrevRot[0]; g_carryRot0[1] = g_supPrevRot[1];
            g_carryRot0[2] = g_supPrevRot[2]; g_carryRot0[3] = g_supPrevRot[3];
            g_carryHave = 1;
            g_carryStartMs = CarryNowMs();
        } else {
            g_carryHave = 0;
        }
    }
    if (!on) g_carryHave = 0;
    g_carryWas = on;
    CyberpunkVR_DebugCarryBlend = 0.0f;
    if (!on || !g_carryHave || !handRot) return;

    // A SPRING, NOT A CURVE. Released from 1 with zero velocity and pulled to 0: the analytic step
    // response of x'' = -w^2 x - 2*zeta*w*x', which is the same shape the recoil spring uses. Under
    // critical damping it crosses zero and comes back a little, and that small overshoot is the whole
    // difference between "it faded" and "something with mass moved".
    const float span = (CyberpunkVR_CarryBlendMs > 1.0f) ? CyberpunkVR_CarryBlendMs : 1.0f;
    const float t = (float)((CarryNowMs() - g_carryStartMs) / 1000.0);
    const float total = span / 1000.0f;
    if (t >= total * 1.6f) { g_carryHave = 0; return; }      // settled: 1.6 spans covers the ring-out
    float zeta = CyberpunkVR_CarryBlendZeta;
    if (zeta < 0.05f) zeta = 0.05f;
    if (zeta > 1.5f)  zeta = 1.5f;
    const float wn = 6.28318531f / total;                     // one period over the asked duration
    float w;
    if (zeta < 0.999f) {
        const float wd = wn * std::sqrt(1.0f - zeta * zeta);
        w = std::exp(-zeta * wn * t) * (std::cos(wd * t) + (zeta * wn / wd) * std::sin(wd * t));
    } else {
        w = std::exp(-wn * t) * (1.0f + wn * t);              // critically damped
    }
    if (w < 0.0f && w > -0.0005f) w = 0.0f;
    CyberpunkVR_DebugCarryBlend = w;

    // slerp from the controller's rotation toward the latched hold by w
    float a[4] = { handRot[0], handRot[1], handRot[2], handRot[3] };
    float b[4] = { g_carryRot0[0], g_carryRot0[1], g_carryRot0[2], g_carryRot0[3] };
    float dot = a[0]*b[0] + a[1]*b[1] + a[2]*b[2] + a[3]*b[3];
    if (dot < 0.0f) { b[0] = -b[0]; b[1] = -b[1]; b[2] = -b[2]; b[3] = -b[3]; dot = -dot; }
    if (dot > 0.9995f) {
        for (int i = 0; i < 4; ++i) handRot[i] = a[i] + (b[i] - a[i]) * w;
    } else {
        const float th0 = std::acos(dot);
        const float th = th0 * w;
        const float s0 = std::sin(th0);
        const float sa = std::sin(th0 - th) / s0;
        const float sb = std::sin(th) / s0;
        for (int i = 0; i < 4; ++i) handRot[i] = a[i] * sa + b[i] * sb;
    }
    VRIK_QuatNorm(handRot);
}


// ================================================================================================
// THE RETURN INTO THE RIGHT HAND, ON A SPRING -- and it needs no game-space geometry whatsoever.
//
// What goes wrong without it: on the way back the weapon is detached from the left hand and attached to
// the right at that slot's own origin, so it jumps from wherever the left hand was holding it straight
// into the right hand's grip.
//
// What makes this exact without a single frame conversion is that the unknown constants cancel. Write a
// wrist as a pose (position, rotation) in model space, and let C_R and C_L be the fixed transforms from
// a wrist to the weapon's attachment on that side -- both unknown (they are bone-local), both constant:
//
//   at the hand-over   L0 * C_L = R0 * C_R      the placement was chosen to preserve the weapon's pose
//   define             X = inv(L0) * R0         hence C_L = X * C_R
//   at the return      weapon = L1 * C_L = L1 * X * C_R
//   so the right wrist has to be drawn at   R_want = L1 * X   -- C_R cancels, and with it every
//                                                               bone-local unknown, which is the class
//                                                               of error this feature has already paid
//                                                               for twice.
//
// So X is captured on the frame the carry starts, D = inv(R) * R_want on the frame it ends, and the
// right wrist is then drawn at R * D faded to R * identity. The weapon, rigidly attached to that wrist,
// travels out of the left hand's hold and into the hand that now owns it.
//
// The left wrist's pose is one pass old (the right hand is solved first inside the same apply): a
// millisecond of hand travel, on a quantity that is sampled exactly once.
// ================================================================================================
namespace {
float g_leftDrawnPos[3] = {0.0f, 0.0f, 0.0f};
float g_leftDrawnRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
int   g_leftDrawnHave = 0;

float g_carryXPos[3] = {0.0f, 0.0f, 0.0f};   // X: the weapon's right-wrist pose, in the left wrist's frame
float g_carryXRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
int   g_carryXHave = 0;

float g_retPos[3] = {0.0f, 0.0f, 0.0f};      // D: the offset the right wrist starts the settle from
float g_retRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
int   g_retHave = 0;
double g_retStartMs = 0.0;

// Rigid-transform algebra, spelled out because getting the order wrong here fails silently.
void PoseMul(const float* pa, const float* qa, const float* pb, const float* qb, float* pc, float* qc) {
    float r[3]; VRIK_QuatRotateVec(qa, pb, r);
    pc[0] = pa[0] + r[0]; pc[1] = pa[1] + r[1]; pc[2] = pa[2] + r[2];
    VRIK_QuatMul(qa, qb, qc); VRIK_QuatNorm(qc);
}
void PoseInv(const float* p, const float* q, float* po, float* qo) {
    VRIK_QuatConj(q, qo);
    float r[3]; VRIK_QuatRotateVec(qo, p, r);
    po[0] = -r[0]; po[1] = -r[1]; po[2] = -r[2];
}
// Slerp from identity toward q by w, w outside [0,1] included -- the spring overshoots on purpose.
void QuatFromIdentity(const float* q, float w, float* out) {
    float b[4] = { q[0], q[1], q[2], q[3] };
    float dot = b[3];
    if (dot < 0.0f) { b[0]=-b[0]; b[1]=-b[1]; b[2]=-b[2]; b[3]=-b[3]; dot = -dot; }
    if (dot > 0.9995f) {
        out[0] = b[0]*w; out[1] = b[1]*w; out[2] = b[2]*w; out[3] = 1.0f + (b[3]-1.0f)*w;
    } else {
        const float th0 = std::acos(dot);
        const float th  = th0 * w;
        const float s0  = std::sin(th0);
        const float sa  = std::sin(th0 - th) / s0;
        const float sb  = std::sin(th) / s0;
        out[0] = b[0]*sb; out[1] = b[1]*sb; out[2] = b[2]*sb; out[3] = sa + b[3]*sb;
    }
    VRIK_QuatNorm(out);
}
}  // namespace

// The drawn left wrist, recorded at the end of the left hand's own solve. That is the hand the weapon
// hangs on while it is carried, so it is the only thing the return needs to know about it.
extern "C" void CarryRecordLeft(const float* pos, const float* rot) {
    if (!pos || !rot) return;
    g_leftDrawnPos[0] = pos[0]; g_leftDrawnPos[1] = pos[1]; g_leftDrawnPos[2] = pos[2];
    g_leftDrawnRot[0] = rot[0]; g_leftDrawnRot[1] = rot[1];
    g_leftDrawnRot[2] = rot[2]; g_leftDrawnRot[3] = rot[3];
    g_leftDrawnHave = 1;
}

// Called with the right hand's own target and controller orientation, BEFORE the two-hand hold, so the
// support point the hold computes follows the weapon while it settles rather than the hand it will end
// up in.
extern "C" void CarryReturnRight(float* target, float* hm, const float* wristR) {
    static int was = 0;
    const int on = (CyberpunkVR_CarryLeft != 0) ? 1 : 0;
    if (!target || !hm) { was = on; return; }

    float bone[4];
    if (wristR) { VRIK_QuatMul(hm, wristR, bone); VRIK_QuatNorm(bone); }
    else        { bone[0]=hm[0]; bone[1]=hm[1]; bone[2]=hm[2]; bone[3]=hm[3]; }

    if (on && !was) {
        // the carry has just begun: remember how the weapon sits on the hand that now holds it
        if (g_leftDrawnHave) {
            float li[3], lq[4];
            PoseInv(g_leftDrawnPos, g_leftDrawnRot, li, lq);
            PoseMul(li, lq, target, bone, g_carryXPos, g_carryXRot);
            g_carryXHave = 1;
        } else {
            g_carryXHave = 0;
        }
        g_retHave = 0;                      // a new carry cancels a return still settling
        g_rGripFresh = 0;                   // and this carry has SPENT the squeeze that started it
    } else if (!on && was) {
        // ...and it has just ended: where the weapon IS, expressed against the wrist it lands on
        if (g_carryXHave && g_leftDrawnHave) {
            float wp[3], wq[4];
            PoseMul(g_leftDrawnPos, g_leftDrawnRot, g_carryXPos, g_carryXRot, wp, wq);
            float ri[3], rq[4];
            PoseInv(target, bone, ri, rq);
            PoseMul(ri, rq, wp, wq, g_retPos, g_retRot);
            g_retHave = 1;
            g_retStartMs = CarryNowMs();
        }
        g_carryXHave = 0;
    }
    was = on;

    // HOW FAR THIS HAND IS FROM THE CARRIED WEAPON, and the offer that answers it.
    //
    // L * X is the weapon's grip expressed as a right-wrist pose -- see the derivation above -- so the
    // reach is a subtraction in the space the hand target already lives in. Both the finger preview and
    // the Lua take gate read what this publishes, so they cannot disagree.
    {
        float dt = 0.016f;
        {
            static LARGE_INTEGER s_f = {};
            static LARGE_INTEGER s_prev = {};
            if (s_f.QuadPart == 0) QueryPerformanceFrequency(&s_f);
            LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
            if (s_prev.QuadPart != 0 && s_f.QuadPart)
                dt = (float)((double)(now.QuadPart - s_prev.QuadPart) / (double)s_f.QuadPart);
            s_prev = now;
            if (dt < 0.0f || dt > 0.25f) dt = 0.016f;
        }
        // WHOSE SQUEEZE THE RIGHT GRIP IS, tracked the way the left one is a few hundred lines above and
        // for the same reason: a button is a LEVEL, and the take is an EDGE. The press that handed the
        // weapon over is spent (cleared at the transition), so holding it down offers nothing -- exactly
        // the rule the left hand's preview follows, and for the visible half of the same reason.
        const bool rDown = RightGripPressed() > 0.5f;
        if (!rDown)            g_rGripFresh = 0;
        else if (!g_rGripWas)  g_rGripFresh = 1;
        g_rGripWas = rDown ? 1 : 0;
        const bool canTake = !rDown || g_rGripFresh;

        float dist = -1.0f;
        if (on && g_carryXHave && g_leftDrawnHave) {
            float wp[3], wq[4];
            PoseMul(g_leftDrawnPos, g_leftDrawnRot, g_carryXPos, g_carryXRot, wp, wq);
            const float dx = wp[0] - target[0];
            const float dy = wp[1] - target[1];
            const float dz = wp[2] - target[2];
            dist = std::sqrt(dx*dx + dy*dy + dz*dz);
        }
        CyberpunkVR_DebugCarryReach = dist;

        static float s_g = 0.0f, s_gv = 0.0f;
        const float goal = (dist >= 0.0f && dist <= CyberpunkVR_CarryGripRadius && canTake) ? 1.0f : 0.0f;
        const float span = (CyberpunkVR_CarryGripMs > 1.0f) ? CyberpunkVR_CarryGripMs : 1.0f;
        float gz = CyberpunkVR_CarryGripZeta;
        if (gz < 0.05f) gz = 0.05f;
        if (gz > 1.5f)  gz = 1.5f;
        const float gwn = 6.28318531f / (span * 0.001f);
        int steps = static_cast<int>(dt / 0.002f) + 1;
        if (steps > 64) steps = 64;
        const float sdt = dt / static_cast<float>(steps);
        for (int i = 0; i < steps; ++i) {
            const float a = -gwn * gwn * (s_g - goal) - 2.0f * gz * gwn * s_gv;
            s_gv += a * sdt;
            s_g  += s_gv * sdt;
        }
        if (s_g < 0.0f) { s_g = 0.0f; if (s_gv < 0.0f) s_gv = 0.0f; }
        if (s_g > 1.0f) s_g = 1.0f;      // fingers do not curl PAST the pose they were captured in
        CyberpunkVR_CarryGripBlend = s_g;
    }

    CyberpunkVR_DebugCarryReturn = 0.0f;
    if (!g_retHave || !CyberpunkVR_CarryReturnHand) return;

    // The same spring as the wrist blend: released from 1, pulled to 0, a shade under-damped so it
    // arrives with weight instead of fading out.
    const float span  = (CyberpunkVR_CarryReturnMs > 1.0f) ? CyberpunkVR_CarryReturnMs : 1.0f;
    const float total = span / 1000.0f;
    const float t = (float)((CarryNowMs() - g_retStartMs) / 1000.0);
    if (t < 0.0f || t >= total * 1.6f) { g_retHave = 0; return; }
    float zeta = CyberpunkVR_CarryReturnZeta;
    if (zeta < 0.05f) zeta = 0.05f;
    if (zeta > 1.5f)  zeta = 1.5f;
    const float wn = 6.28318531f / total;
    float w;
    if (zeta < 0.999f) {
        const float wd = wn * std::sqrt(1.0f - zeta * zeta);
        w = std::exp(-zeta * wn * t) * (std::cos(wd * t) + (zeta * wn / wd) * std::sin(wd * t));
    } else {
        w = std::exp(-wn * t) * (1.0f + wn * t);
    }
    if (w > 1.2f)  w = 1.2f;
    if (w < -0.3f) w = -0.3f;
    CyberpunkVR_DebugCarryReturn = w;
    if (w > -0.0005f && w < 0.0005f) return;

    // Position first, in the wrist's own frame -- which is what makes the offset travel with the hand
    // instead of hanging in the world.
    const float off[3] = { g_retPos[0]*w, g_retPos[1]*w, g_retPos[2]*w };
    float offM[3]; VRIK_QuatRotateVec(bone, off, offM);
    target[0] += offM[0]; target[1] += offM[1]; target[2] += offM[2];

    // ...then the twist, and back out to the controller frame the caller composes the hand from.
    float dq[4]; QuatFromIdentity(g_retRot, w, dq);
    float nb[4]; VRIK_QuatMul(bone, dq, nb); VRIK_QuatNorm(nb);
    if (wristR) {
        const float wc[4] = { -wristR[0], -wristR[1], -wristR[2], wristR[3] };
        float nh[4]; VRIK_QuatMul(nb, wc, nh); VRIK_QuatNorm(nh);
        hm[0]=nh[0]; hm[1]=nh[1]; hm[2]=nh[2]; hm[3]=nh[3];
    } else {
        hm[0]=nb[0]; hm[1]=nb[1]; hm[2]=nb[2]; hm[3]=nb[3];
    }
}

// THE WEAPON HAND WAS STOPPED (hand collision) after the support point was built from it. The weapon rides that
// hand, so the point on the weapon moved with it, and the support hand has to go along.
void TwoHandWeaponHandMoved(const float* moved) {
    for (int k = 0; k < 3; ++k) {
        g_supPos[k] += moved[k];
        g_supPrevPos[k] += moved[k];
    }
}

// THE LEFT HAND'S HALF: only a held grip moves the wrist.
bool TwoHandLeft(float* target, float* handRot) {
    // MIXED, NOT REPLACED. The caller has already built this wrist from its own controller; the hold
    // pulls it toward the weapon by however much of the hold has arrived. Replacing it outright is what
    // made the hand teleport onto the gun the instant the grip closed -- and teleport back off it on
    // release, which is the same fault at the other end.
    //
    // The weight is still running while TwoHandActive is 0 (it is decaying), so this must not gate on
    // the flag -- only on there being a hold to mix toward and something left of the weight.
    if (!target) return false;
    // NOT WHILE THIS HAND IS THE ONE HOLDING THE WEAPON. The pose below is the support point, built from
    // the RIGHT hand on the assumption that the right hand holds the gun; through a carry that is false,
    // and a decaying weight then drags this wrist -- and the weapon hanging off it -- toward a point
    // beside an empty hand for the length of the release, on top of the carry blend already moving it.
    if (CyberpunkVR_CarryLeft != 0) return false;
    float w = CyberpunkVR_TwoHandWeight;
    if (w <= 0.0005f) return false;
    if (w > 1.0f) w = 1.0f;
    // The live pose while there is one, the last valid one while the weight runs out. A release is
    // precisely the case where this pass has no pose to offer, and easing toward the pose the hold last
    // had IS the release.
    const float* sp = g_supValid ? g_supPos : (g_supPrevHave ? g_supPrevPos : nullptr);
    const float* sq = g_supValid ? g_supRot : (g_supPrevHave ? g_supPrevRot : nullptr);
    if (!sp) return false;

    target[0] += (sp[0] - target[0]) * w;
    target[1] += (sp[1] - target[1]) * w;
    target[2] += (sp[2] - target[2]) * w;

    if (handRot) {
        float a[4] = { handRot[0], handRot[1], handRot[2], handRot[3] };
        float b[4] = { sq[0], sq[1], sq[2], sq[3] };
        float dot = a[0]*b[0] + a[1]*b[1] + a[2]*b[2] + a[3]*b[3];
        if (dot < 0.0f) { b[0]=-b[0]; b[1]=-b[1]; b[2]=-b[2]; b[3]=-b[3]; dot = -dot; }
        if (dot > 0.9995f) {
            for (int i = 0; i < 4; ++i) handRot[i] = a[i] + (b[i] - a[i]) * w;
        } else {
            const float th0 = std::acos(dot);
            const float th  = th0 * w;
            const float s0  = std::sin(th0);
            const float sa  = std::sin(th0 - th) / s0;
            const float sb  = std::sin(th) / s0;
            for (int i = 0; i < 4; ++i) handRot[i] = a[i] * sa + b[i] * sb;
        }
        VRIK_QuatNorm(handRot);
    }
    return true;
}

// The captured finger curl, mixed on by the ramp above. Written after the resting pose and before the
// reload layer, so a magazine grip still wins over a weapon grip -- the hand doing the more specific job
// keeps the fingers.
void TwoHandFingers(uint8_t* boneBuf) {
    SelectHold();
    if (!g_cur || g_cur->fingerCount <= 0) return;
    const float b = CyberpunkVR_TwoHandBlend;
    if (b <= 0.001f) return;
    // The hold's fingers are the left hand's; with the weapon in the left hand they go, mirrored, to the right one.
    const bool mirrored = cvr::anim::WeaponInLeftHand();
    for (int k = 0; k < g_cur->fingerCount && k < 32; ++k) {
        int bi = -1;
        for (int s = 0; s < g_VRSmokeFingerCountL && s < 32; ++s) {
            if (std::strcmp(g_cur->fingerName[k], g_VRSmokeFingerNameL[s]) == 0) { bi = g_VRSmokeFingerIdxL[s]; break; }
        }
        if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
        const float* tq = g_cur->finger[k];
        float mq[4];
        if (mirrored) {
            if (!cvr::anim::MirrorFingerToRight(bi, tq, &bi, mq)) continue;
            tq = mq;
        }
        float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
        if (b >= 0.999f) { q[0]=tq[0]; q[1]=tq[1]; q[2]=tq[2]; q[3]=tq[3]; continue; }
        const float dot = q[0]*tq[0] + q[1]*tq[1] + q[2]*tq[2] + q[3]*tq[3];
        const float s = (dot < 0.0f) ? -b : b;
        float nq[4] = { q[0]*(1.0f-b) + tq[0]*s, q[1]*(1.0f-b) + tq[1]*s,
                        q[2]*(1.0f-b) + tq[2]*s, q[3]*(1.0f-b) + tq[3]*s };
        const float nl = std::sqrt(nq[0]*nq[0] + nq[1]*nq[1] + nq[2]*nq[2] + nq[3]*nq[3]);
        if (nl > 1e-6f) { q[0]=nq[0]/nl; q[1]=nq[1]/nl; q[2]=nq[2]/nl; q[3]=nq[3]/nl; }
    }
}

// DISK, OFF THE ANIMATION THREAD, AND EVERY FILE READ ONCE. The pose path runs inside the game's own pose
// apply several times a tick; a file opened there is an unbounded wait in the middle of the animation. So
// the whole set is scanned at startup and nothing touches the disk again until a capture asks to be saved.
void TwoHandTick() {
    if (!g_scanned) {
        g_scanned = 1;
        WIN32_FIND_DATAA fd = {};
        const std::string pat = VRGripPath("CyberpunkVR_TwoHandGrip_*.ini");
        HANDLE hf = FindFirstFileA(pat.c_str(), &fd);
        if (hf != INVALID_HANDLE_VALUE) {
            do {
                // CyberpunkVR_TwoHandGrip_<weapon>.ini -> <weapon>
                const char* nm = fd.cFileName;
                const char* pre = "CyberpunkVR_TwoHandGrip_";
                const size_t pl = std::strlen(pre);
                if (std::strncmp(nm, pre, pl) != 0) continue;
                char weapon[64] = {0};
                std::strncpy(weapon, nm + pl, 63);
                char* dot = std::strrchr(weapon, '.');
                if (dot) *dot = '\0';
                if (!weapon[0]) continue;
                Hold* h = AddHold(weapon);
                if (!h) break;
                FILE* f = nullptr;
                if (fopen_s(&f, VRGripPath(nm).c_str(), "r") == 0 && f) {
                    ParseInto(h, f);
                    std::fclose(f);
                }
            } while (FindNextFileA(hf, &fd));
            FindClose(hf);
        }
        CyberpunkVR_DebugTwoHandLoaded = g_holdCount;
        g_curFor[0] = 1; g_curFor[1] = 0;      // force a re-select against the freshly loaded table
    }

    if (!g_saveReq) return;
    g_saveReq = 0;
    const Hold* h = g_cur;
    if (!h) { CyberpunkVR_DebugTwoHandSaved = -1; return; }
    char path[128];
    std::snprintf(path, sizeof(path), "CyberpunkVR_TwoHandGrip_%s.ini", h->weapon);
    FILE* f = nullptr;
    if (fopen_s(&f, VRGripPath(path).c_str(), "w") == 0 && f) {
        std::fprintf(f, "# CyberpunkVR two-hand grip pose v1 (auto-generated by VRTwoHandCapture)\n"
                        "# Weapon: %s. One frame of the game's own two-handed hold, captured with VRIK off.\n"
                        "# W px py pz qx qy qz qw        left wrist IN THE RIGHT WRIST'S FRAME\n"
                        "# F <bone> qx qy qz qw          finger: parent-local rotation only\n", h->weapon);
        std::fprintf(f, "W %.9g %.9g %.9g %.9g %.9g %.9g %.9g\n",
                     h->off[0], h->off[1], h->off[2], h->rot[0], h->rot[1], h->rot[2], h->rot[3]);
        for (int k = 0; k < h->fingerCount && k < 32; ++k) {
            std::fprintf(f, "F %s %.9g %.9g %.9g %.9g\n", h->fingerName[k],
                         h->finger[k][0], h->finger[k][1], h->finger[k][2], h->finger[k][3]);
        }
        std::fclose(f);
        CyberpunkVR_DebugTwoHandSaved = 1;
    } else {
        CyberpunkVR_DebugTwoHandSaved = -1;
    }
}

}  // namespace anim
}  // namespace cvr
