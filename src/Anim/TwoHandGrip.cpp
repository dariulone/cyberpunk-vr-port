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
// THE AIM. While the grip is held, the barrel is aimed at the LEFT CONTROLLER -- the real one, the thing
// the player is actually pointing -- and the roll about the barrel is left to the right controller, which
// is what a wrist does. The drawn left hand is then placed on the weapon by the offset above, so it lands
// where the player is holding, without ever being dragged there. A preview NEVER moves a wrist: only a
// held grip does, which is the same rule the reload module follows for slides and magazines.

#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/TwoHandGrip.hpp"
#include "Core/VrCoreShared.hpp"    // g_hasWeaponEquipped
#include "Natives/NativeState.hpp"  // VRDiagPath

#include <windows.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>

// Published by the weapon module on each draw (src/Natives/OrientationProvider.cpp).
extern "C" __declspec(dllexport) extern char CyberpunkVR_WeaponName[64];

// ---- the switches, and every one of them is a fact rather than a taste ----
extern "C" __declspec(dllexport) int   CyberpunkVR_TwoHandGrip    = 1;      // the feature
// How near the support point the hand has to be for the fingers to offer the hold. It shipped at 12 cm
// on the argument that that is the reach of a hand which means it -- and in play that was too wide: the
// offer appeared for a hand merely passing the weapon, which is the failure the same note predicted for
// a larger value. 6 cm on the user's call. It is an ini key now (xr_two_hand_radius) because this is a
// number to settle by feel, and settling it by feel must not cost a rebuild per attempt.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandRadius  = 0.06f;
// What a second hand takes off the kick. Two hands roughly triple the effective mass resisting the same
// impulse and add a second lever against the muzzle rise, so a fifth of the one-handed flip is what the
// physics of it says -- and it is the number asked for.
// 0.286, not 0.2, and the change is arithmetic rather than taste: the one-handed angle came down by
// 30% and the two-handed result was to stay where it was, so the fraction goes up by the same 30%.
// 0.2 / 0.7 = 0.286, and a two-handed Lexington still peaks at the 4.4 deg it did before.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandRecoil  = 0.286f;
// How fast the finger preview fades in and out, seconds. Matches the reload module's own ramp so the two
// previews feel like one system.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandFadeS   = 0.15f;
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
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimGain   = 0.60f;   // at full leverage
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandLeverFull = 0.35f;   // m, at this: all of it
// How quickly the push is allowed to become a correction, seconds. This is what makes a jump
// impossible: the aim can only ramp toward the hand, never step to it.
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimTau    = 0.12f;
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimMaxDeg = 10.0f;
extern "C" __declspec(dllexport) float CyberpunkVR_TwoHandAimDeadDeg = 4.0f;

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

Hold g_holds[32] = {};
int  g_holdCount = 0;
int  g_scanned   = 0;
int  g_saveReq   = 0;

// The row the weapon in hand selects, and the name it was selected for.
const Hold* g_cur = nullptr;
char g_curFor[64] = {1, 0};      // deliberately not a valid name, so the first pass always selects

// The support point, recomputed every pass the right hand is solved and read by the left one, which is
// solved after it. A frame-local hand-off, not state: both live inside one pose apply.
float g_supPos[3] = {0.0f, 0.0f, 0.0f};
float g_supRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
int   g_supValid  = 0;

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
    if (g_holdCount >= 32) return nullptr;
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

float LeftGripPressed() {
    // [155] is the left grip, published by the input merge. The right grip lives at the legacy [49]; the
    // two were once read off one slot, which is how a lighter used to ignite itself.
    return (g_pSharedHands ? g_pSharedHands[155] : 0.0f);
}

}  // namespace

namespace cvr {
namespace anim {

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
    if (why) { CyberpunkVR_DebugTwoHandRefused = why; return; }

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
        if (base > 0.60f) { CyberpunkVR_DebugTwoHandRefused = 16; return; }
    }

    Hold* h = AddHold(WeaponKey());
    if (!h) { CyberpunkVR_DebugTwoHandRefused = 8; return; }   // 8 = the table is full

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
}

// THE RIGHT HAND'S HALF, called with the hand already built from its own controller.
//
// `hm` is the controller's orientation in model space, whose +Y is the barrel (the weapon rides this hand
// bone). Engaged, it is turned so that barrel points at the left controller -- the smallest rotation that
// does it, so the roll the right wrist has is untouched. Then the support point is stored for the left
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
        if (down && g_VRReloadFingerActive[0]) g_gripFresh = 0;   // ...and the reload has taken it
        g_gripWas = down ? 1 : 0;
    }

    // THE RELOAD OWNS THIS HAND WHILE IT IS WORKING, and that is not a courtesy -- the two features want the
    // same wrist for opposite reasons. Bringing a magazine to the well passes straight through the support
    // point, so without this the grip snapped shut on the way in and the magazine was carried by a hand
    // welded to the pistol ("срабатывает магнит на two handed"). `g_VRReloadFingerActive[0]` is the reload
    // module saying it has the left hand: a magazine held, a slide gripped, or a preview being offered.
    if (g_VRReloadFingerActive[0]) {
        CyberpunkVR_TwoHandActive = 0;
        CyberpunkVR_TwoHandBlend = 0.0f;
        return;
    }
    SelectHold();
    if (!CyberpunkVR_TwoHandGrip || !g_cur || !g_hasWeaponEquipped || !targetR || !hm) {
        CyberpunkVR_TwoHandActive = 0;
        CyberpunkVR_TwoHandBlend = 0.0f;
        return;
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

    float off[3]; VRIK_QuatRotateVec(bone, g_cur->off, off);
    float sup[3] = { targetR[0] + off[0], targetR[1] + off[1], targetR[2] + off[2] };

    float dist = -1.0f;
    if (leftCtrlModel) {
        const float dx = leftCtrlModel[0] - sup[0];
        const float dy = leftCtrlModel[1] - sup[1];
        const float dz = leftCtrlModel[2] - sup[2];
        dist = std::sqrt(dx*dx + dy*dy + dz*dz);
    }
    CyberpunkVR_DebugTwoHandDist = dist;

    // ENGAGE on a squeeze inside the radius; HOLD until it is released, wherever the hand then goes. A
    // grip that let go the moment the hand drifted out of a sphere would be a grip that cannot be used.
    // `g_gripFresh` is what makes "a squeeze" mean a squeeze rather than a button that happens to be down --
    // see the note at the top of this function.
    const bool inRange = (dist >= 0.0f && dist <= CyberpunkVR_TwoHandRadius);
    if (CyberpunkVR_TwoHandActive) {
        if (!pressed) CyberpunkVR_TwoHandActive = 0;
    } else if (pressed && inRange && g_gripFresh) {
        CyberpunkVR_TwoHandActive = 1;
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

    if (CyberpunkVR_TwoHandActive && leftCtrlModel) {
        // WHAT THE CORRECTION IS MEASURED AGAINST: the support point itself, not an assumed barrel axis.
        //
        // The first version rotated a hard-coded local +Y onto the line between the hands. If that axis is
        // not exactly the barrel -- and nothing here can promise it is -- the error never goes to zero, the
        // correction sits pinned at its ceiling, and its AXIS swings with every small hand movement. The
        // weapon then appears to jump sideways for a centimetre of palm motion, which is what was
        // reported. Referencing the support point removes the assumption entirely: the direction from the
        // wrist to where the support hand IS ATTACHED is known exactly (it is the captured offset), so the
        // error is zero when the player's hand is where the weapon says it is, and grows only as he pushes
        // it away. Nothing about the weapon's own axes is assumed.
        //
        // AND IT IS LOW-PASSED IN THE BONE'S OWN FRAME. Smoothing a model-space direction would fight the
        // hand's motion (the frame moves under the filter); in the wrist's frame the reference is a
        // constant and only the player's push varies, so the filter has nothing to chase. A 0.12 s time
        // constant is slower than tracking noise and faster than a deliberate push -- and, more to the
        // point, it makes a jump impossible: the correction can only ever ramp.
        const float rl = std::sqrt(g_cur->off[0]*g_cur->off[0] + g_cur->off[1]*g_cur->off[1]
                                + g_cur->off[2]*g_cur->off[2]);
        float d[3] = { leftCtrlModel[0] - targetR[0],
                       leftCtrlModel[1] - targetR[1],
                       leftCtrlModel[2] - targetR[2] };
        const float dl = std::sqrt(d[0]*d[0] + d[1]*d[1] + d[2]*d[2]);
        if (rl > 1e-4f && dl > 1e-4f) {
            const float refL[3] = { g_cur->off[0]/rl, g_cur->off[1]/rl, g_cur->off[2]/rl };   // constant, in the bone frame
            float bc[4]; VRIK_QuatConj(bone, bc);
            const float dm[3] = { d[0]/dl, d[1]/dl, d[2]/dl };
            float dL[3]; VRIK_QuatRotateVec(bc, dm, dL);                       // the push, in the same frame

            static float s_dL[3] = {0.0f, 0.0f, 0.0f};
            static int   s_have  = 0;
            if (!s_have) { s_dL[0]=dL[0]; s_dL[1]=dL[1]; s_dL[2]=dL[2]; s_have = 1; }
            const float tau = (CyberpunkVR_TwoHandAimTau > 0.01f) ? CyberpunkVR_TwoHandAimTau : 0.01f;
            const float a = 1.0f - std::exp(-dt / tau);
            for (int i = 0; i < 3; ++i) s_dL[i] += (dL[i] - s_dL[i]) * a;
            const float sl = std::sqrt(s_dL[0]*s_dL[0] + s_dL[1]*s_dL[1] + s_dL[2]*s_dL[2]);
            if (sl > 1e-5f) {
                const float ds[3] = { s_dL[0]/sl, s_dL[1]/sl, s_dL[2]/sl };
                const float dot = refL[0]*ds[0] + refL[1]*ds[1] + refL[2]*ds[2];
                float axisL[3] = { refL[1]*ds[2] - refL[2]*ds[1],
                                   refL[2]*ds[0] - refL[0]*ds[2],
                                   refL[0]*ds[1] - refL[1]*ds[0] };
                const float al = std::sqrt(axisL[0]*axisL[0] + axisL[1]*axisL[1] + axisL[2]*axisL[2]);
                if (al > 1e-6f && dot > -0.999f) {
                    float ang = std::atan2(al, dot);
                    // THE OFF HAND STEADIES, IT DOES NOT AIM. A fraction of the error, with a ceiling and a
                    // deadzone: the right hand holds the grip and IS the weapon's pose, while the left rests
                    // on it and can only push within the give of a wrist. The deadzone matters on its own --
                    // the two hands are barely 7 cm apart on a pistol, so a centimetre of tracking noise is
                    // several degrees, and correcting noise is shake.
                    const float dead = CyberpunkVR_TwoHandAimDeadDeg * 0.01745329252f;
                    const float cap  = CyberpunkVR_TwoHandAimMaxDeg  * 0.01745329252f;
                    ang = (ang > dead) ? (ang - dead) : 0.0f;
                    // The leverage the captured hold actually has, measured from the hold itself.
                    // PROPORTIONAL TO THE ARM, with no floor and no threshold. Torque is force times
                    // lever, so a hand 74 mm from the wrist has 74/350 of the authority the same hand has
                    // out on a rifle handguard -- about a fifth, not zero. The first version cut everything
                    // below 12 cm off entirely and the pistol stopped answering the second hand at all,
                    // which is a different wrong answer: a supporting hand on a pistol DOES move the point
                    // of aim, that is how muzzle flip is controlled. What made it jump was never the short
                    // lever -- it was aiming at an assumed barrel axis, and that is fixed above.
                    float lev = (CyberpunkVR_TwoHandLeverFull > 1e-3f)
                              ? (rl / CyberpunkVR_TwoHandLeverFull) : 1.0f;
                    if (lev > 1.0f) lev = 1.0f;
                    ang *= CyberpunkVR_TwoHandAimGain * lev;
                    if (ang > cap) ang = cap;
                    if (ang > 1e-5f) {
                        // Built in the BONE frame and applied there, so it is a push on the weapon rather
                        // than a rotation about some world axis that happens to be nearby.
                        const float s = std::sin(ang * 0.5f) / al;
                        const float qL[4] = { axisL[0]*s, axisL[1]*s, axisL[2]*s, std::cos(ang * 0.5f) };
                        float nb[4]; VRIK_QuatMul(bone, qL, nb); VRIK_QuatNorm(nb);
                        bone[0]=nb[0]; bone[1]=nb[1]; bone[2]=nb[2]; bone[3]=nb[3];
                        // ...and back out to the controller frame, which is what the caller composes the
                        // hand from: hm = bone * conj(wristR).
                        if (wristR) {
                            const float wc[4] = { -wristR[0], -wristR[1], -wristR[2], wristR[3] };
                            float nh[4]; VRIK_QuatMul(bone, wc, nh); VRIK_QuatNorm(nh);
                            hr[0]=nh[0]; hr[1]=nh[1]; hr[2]=nh[2]; hr[3]=nh[3];
                        } else {
                            hr[0]=bone[0]; hr[1]=bone[1]; hr[2]=bone[2]; hr[3]=bone[3];
                        }
                        hm[0]=hr[0]; hm[1]=hr[1]; hm[2]=hr[2]; hm[3]=hr[3];
                    }
                }
            }
        }
        // ...and the support point is recomputed from the AIMED hand: the hand is drawn where the weapon
        // now is, which is where the player's own hand is.
        VRIK_QuatRotateVec(bone, g_cur->off, off);
        sup[0] = targetR[0] + off[0]; sup[1] = targetR[1] + off[1]; sup[2] = targetR[2] + off[2];
    }

    g_supPos[0] = sup[0]; g_supPos[1] = sup[1]; g_supPos[2] = sup[2];
    // The wrist rotation follows the same relation as the offset, so the hand lands on the grip the way
    // the animation had it rather than merely near it.
    VRIK_QuatMul(bone, g_cur->rot, g_supRot); VRIK_QuatNorm(g_supRot);
    g_supValid = 1;
}

// THE LEFT HAND'S HALF: only a held grip moves the wrist.
bool TwoHandLeft(float* target, float* handRot) {
    if (!CyberpunkVR_TwoHandActive || !g_supValid || !target) return false;
    target[0] = g_supPos[0]; target[1] = g_supPos[1]; target[2] = g_supPos[2];
    if (handRot) { handRot[0]=g_supRot[0]; handRot[1]=g_supRot[1]; handRot[2]=g_supRot[2]; handRot[3]=g_supRot[3]; }
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
    for (int k = 0; k < g_cur->fingerCount && k < 32; ++k) {
        int bi = -1;
        for (int s = 0; s < g_VRSmokeFingerCountL && s < 32; ++s) {
            if (std::strcmp(g_cur->fingerName[k], g_VRSmokeFingerNameL[s]) == 0) { bi = g_VRSmokeFingerIdxL[s]; break; }
        }
        if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
        float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
        const float* tq = g_cur->finger[k];
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
        const std::string pat = VRDiagPath("CyberpunkVR_TwoHandGrip_*.ini");
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
                if (fopen_s(&f, VRDiagPath(nm).c_str(), "r") == 0 && f) {
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
    if (fopen_s(&f, VRDiagPath(path).c_str(), "w") == 0 && f) {
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
