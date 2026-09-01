// ReloadPose -- the physical reload's FINGER poses, out of the VRIK solve.
//
// Not VRIK. VRIK solves arms from a controller position; this replays a captured finger pose while
// the reload module has a hand on a magazine, and the two share nothing but the bone buffer they
// write into. They shared a file because the detour that reaches the buffer lives there.
//
// THE POSES ARE DELTAS, never absolute finger rotations -- that is the fact this module cost the most
// to learn. An absolute pose fights whatever the animation graph is doing that frame; a delta rides
// it. See the reload module's own notes.
//
// The GUARD stays in src/Hooks/AnimPose.cpp, verbatim, exactly as it does for the weapon rig: this
// function is entered only when the caller has already decided a reload hand is active. And it needs
// no __try of its own -- SEH is dynamic, so the detour's __except covers it across the file boundary.

#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/WeaponRig.hpp"
#include "Hooks/Hook.hpp"
#include <MinHook.h>
#include "Anim/ReloadPose.hpp"
#include "Core/VrCoreShared.hpp"   // g_hasWeaponEquipped, g_isInVehicle
#include "Natives/NativeState.hpp"  // VRDiagPath
#include <cstdio>
#include <cstring>

// TWO NUMBERS THAT SEPARATE THE TWO WAYS THIS CAN LOOK WRONG. A hand that still wears the two-handed
// claw is either a pose that never latched (caps == 0: the hands were never both empty and still) or a
// pose that latched the wrong moment (caps > 0, and the fix is to stand still with empty hands once
// more). Without these the two are the same picture.
extern "C" __declspec(dllexport) int                CyberpunkVR_DebugRestFingerHave = 0;
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugRestFingerCaps = 0;
extern "C" __declspec(dllexport) int                CyberpunkVR_DebugRestFingerRefused = 0;
extern "C" __declspec(dllexport) int                CyberpunkVR_DebugRestFingerSaved = 0;
extern "C" __declspec(dllexport) int                CyberpunkVR_DebugRestFingerLoadTried = 0;
extern "C" __declspec(dllexport) int                CyberpunkVR_DebugRestFingerLoaded = 0;
// Raised by the overlay button (or a probe write); cleared by the pose path once it has the frame.
extern "C" __declspec(dllexport) int                CyberpunkVR_RestFingerCaptureReq = 0;
extern "C" __declspec(dllexport) int                CyberpunkVR_RestFingerSaveReq = 0;
// The apply half, measured the same way: how many passes wrote the pose, how many bones the last one
// touched, and the gate that decides. Plus a force switch, so the write path can be proven with empty
// hands instead of waiting to find out whether the weapon flag is the problem.
extern "C" __declspec(dllexport) unsigned long long CyberpunkVR_DebugRestApplyCalls = 0;
extern "C" __declspec(dllexport) int                CyberpunkVR_DebugRestBones = 0;
extern "C" __declspec(dllexport) int                CyberpunkVR_DebugRestGate = 0;
extern "C" __declspec(dllexport) int                CyberpunkVR_RestFingerForce = 0;

// Named and shaped like the smoke module's grip files (CyberpunkVR_SmokeGrip_right.ini,
// CyberpunkVR_LighterGrip_Left.ini) and kept beside them: one convention for every recorded pose
// in the port, and one place to look for them.
static const char* kRestGripIni = "CyberpunkVR_RestGrip_Left.ini";

namespace cvr {
namespace anim {

// THE LEFT HAND AT REST -- RECORDED ONCE, then replayed for good.
//
// While a weapon is out the game poses the left hand as the SUPPORT half of a two-handed grip: correct for
// a flat shooter, wrong here, because in VR that hand is empty and in plain sight, wearing a claw around
// nothing. The pose that belongs there exists only as the game's own empty-handed animation, so it is
// taken from there -- but on COMMAND, not by watching.
//
// WATCHING WAS TRIED AND DOES NOT WORK, which is worth recording rather than re-attempting. The first
// version latched by itself whenever the hands were empty and every finger held within a quarter of a
// degree for a third of a second. It never fired once -- `caps == 0` across a whole session -- because an
// idle animation is never still: it breathes, and the fingers drift with it. Loosening the threshold only
// moves the failure, since a slow gesture then qualifies as rest. Motion alone cannot tell the two apart.
//
// So the moment is CHOSEN, once, with empty hands, and that frame is written to disk beside the game and
// loaded at boot. One frame of the game's own animation, kept exactly, in the units it already speaks:
// parent-local quaternions straight out of the pose buffer. Nothing is converted, so nothing can be
// converted wrongly -- which is the whole reason this is captured here rather than off a recorder take.
//
// A BASE LAYER, never an override: it is written before the reload/preview layer, so a preview still fades
// in FROM these fingers over its own blend, exactly as it faded in from the animation before.
void VrikRestFingerPose(uint8_t* boneBuf) {
    const int  cnt = g_VRSmokeFingerCountL;
    const int* idx = g_VRSmokeFingerIdxL;
    if (cnt <= 0 || !idx) return;

    // CAPTURE, on request. Refused while a weapon is out -- that is the pose being replaced -- and while
    // another layer owns this hand, because what is in the buffer then is that layer's, not the game's.
    if (CyberpunkVR_RestFingerCaptureReq) {
        // A REFUSAL NAMES ITSELF. Four conditions can block a capture and they need different answers
        // from the player -- holster the gun, let go of the magazine, put the cigarette out, get out of
        // the car -- so a single 'refused' flag sends him guessing. Bits: 1 weapon, 2 reload grip,
        // 4 cigarette, 8 vehicle.
        {
            int why = 0;
            if (g_hasWeaponEquipped)        why |= 1;
            if (g_VRReloadFingerActive[0])  why |= 2;
            // ...ACTIVE, not HAVE. `g_VRSmokeCigLHave` says a cigarette pose EXISTS -- it is set when the
            // smoke module's ini loads at arming and then stands for the whole session -- so guarding on
            // it refused every capture with "there is a cigarette in your hand" while the hand was empty.
            // `g_VRSmokeFingerActiveL` is the one that means the left hand is being posed right now.
            if (g_VRSmokeFingerActiveL)     why |= 4;
            if (g_isInVehicle)              why |= 8;
            if (why) {
                CyberpunkVR_RestFingerCaptureReq = 0;
                CyberpunkVR_DebugRestFingerRefused = why;
                return;
            }
        }
        for (int k = 0; k < 32; ++k) {
            const int bi = (k < cnt) ? idx[k] : -1;
            if (bi < 0 || bi >= VRIK_MAX_BONES) {
                g_VRRestFingerRot[k][0] = 0.0f; g_VRRestFingerRot[k][1] = 0.0f;
                g_VRRestFingerRot[k][2] = 0.0f; g_VRRestFingerRot[k][3] = 1.0f;
                continue;
            }
            const float* q = reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
            g_VRRestFingerRot[k][0] = q[0]; g_VRRestFingerRot[k][1] = q[1];
            g_VRRestFingerRot[k][2] = q[2]; g_VRRestFingerRot[k][3] = q[3];
        }
        g_VRRestFingerCount = cnt;
        g_VRRestFingerHave = 1;
        CyberpunkVR_DebugRestFingerHave = 1;
        ++CyberpunkVR_DebugRestFingerCaps;
        CyberpunkVR_DebugRestFingerRefused = 0;
        CyberpunkVR_RestFingerCaptureReq = 0;
        CyberpunkVR_RestFingerSaveReq = 1;      // written to disk off this thread; see RestFingerTick
        return;
    }

    // THE GATE, PUBLISHED. "It does not apply" has four different causes and they are indistinguishable
    // from outside: no weapon seen, no pose recorded, the feature switched off, or no finger list. One
    // bitfield per pass says which, so the next report is a reading rather than a guess.
    //   1 weapon seen   2 pose recorded   4 apply on   8 finger list resolved
    CyberpunkVR_DebugRestGate = (g_hasWeaponEquipped ? 1 : 0)
                              | (g_VRRestFingerHave  ? 2 : 0)
                              | (g_VRRestFingerApply ? 4 : 0)
                              | ((cnt > 0)           ? 8 : 0);
    if (!g_hasWeaponEquipped && !CyberpunkVR_RestFingerForce) return;   // empty-handed: the game's pose IS the right one
    if (!g_VRRestFingerHave || !g_VRRestFingerApply) return;
    ++CyberpunkVR_DebugRestApplyCalls;

    // Written flat, at full strength, and FIRST -- the reload/preview layer runs after this one and nlerps
    // onto whatever is here, so a preview now grows out of the resting hand instead of out of the grip
    // pose. That ordering is the whole reason this is a separate pass and not a branch inside one.
    //
    // The recorded count decides, not the live one: a pose taken against 19 bones must not be sprayed over
    // a rig that resolved more.
    const int n = (g_VRRestFingerCount > 0 && g_VRRestFingerCount < cnt) ? g_VRRestFingerCount : cnt;
    int wrote = 0;
    for (int k = 0; k < n && k < 32; ++k) {
        const int bi = idx[k];
        if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
        const float* r = g_VRRestFingerRot[k];
        if (r[0] == 0.0f && r[1] == 0.0f && r[2] == 0.0f && r[3] == 0.0f) continue;   // never recorded
        float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
        q[0] = r[0]; q[1] = r[1]; q[2] = r[2]; q[3] = r[3];
        ++wrote;
    }
    CyberpunkVR_DebugRestBones = wrote;
}

// DISK, OFF THE ANIMATION THREAD. The pose path runs inside the game's own pose apply, several times a
// tick; opening a file there would put an unbounded wait in the middle of it. So the capture only raises a
// flag and the frame loop does the writing, and the load happens once, on the first call.
//
// THE FILE IS KEYED BY BONE NAME, not by slot. The slot order comes from the plugin's own finger resolver,
// which rebuilds itself per rig -- and the male and female skeletons are different objects. Saving slot
// numbers would make a pose recorded on one body silently land on the wrong fingers of the other. Names
// cost a string compare once, at load.
void RestFingerTick() {
    if (CyberpunkVR_RestFingerSaveReq) {
        CyberpunkVR_RestFingerSaveReq = 0;
        FILE* f = nullptr;
        if (fopen_s(&f, VRDiagPath(kRestGripIni).c_str(), "w") == 0 && f) {
            std::fprintf(f, "# CyberpunkVR rest grip pose v1 (auto-generated by VRRestFingerCapture)\n"
                            "# The LEFT hand's resting fingers, one frame of the game's own empty-handed\n"
                            "# animation. Replayed while a weapon is out, where the game would otherwise\n"
                            "# pose this hand as the support half of a two-handed grip.\n"
                            "# F <bone> qx qy qz qw           finger: parent-local rotation only\n");
            for (int k = 0; k < g_VRRestFingerCount && k < 32; ++k) {
                const char* nm = g_VRSmokeFingerNameL[k];
                if (!nm || !nm[0]) continue;
                std::fprintf(f, "F %s %.9g %.9g %.9g %.9g\n", nm,
                             g_VRRestFingerRot[k][0], g_VRRestFingerRot[k][1],
                             g_VRRestFingerRot[k][2], g_VRRestFingerRot[k][3]);
            }
            std::fclose(f);
            CyberpunkVR_DebugRestFingerSaved = 1;
        } else {
            CyberpunkVR_DebugRestFingerSaved = -1;
        }
        return;
    }

    // LOAD once the resolver has names to match against -- not at boot, when it has none yet.
    if (g_VRRestFingerHave || CyberpunkVR_DebugRestFingerLoadTried) return;
    if (g_VRSmokeFingerCountL <= 0) return;
    CyberpunkVR_DebugRestFingerLoadTried = 1;
    FILE* f = nullptr;
    if (fopen_s(&f, VRDiagPath(kRestGripIni).c_str(), "r") == 0 && f) {
        char line[256];
        int  hit = 0;
        while (std::fgets(line, sizeof(line), f)) {
            if (line[0] == '#' || line[0] == '\n' || line[0] == '\r') continue;
            char nm[64] = {0};
            float a = 0.0f, b = 0.0f, c = 0.0f, d = 0.0f;
            // `F <bone> qx qy qz qw`, the smoke module's own line shape -- one format for every recorded
            // grip in the port, so a pose file is readable wherever it came from.
            if (std::sscanf(line, "F %63s %g %g %g %g", nm, &a, &b, &c, &d) != 5) continue;
            for (int k = 0; k < g_VRSmokeFingerCountL && k < 32; ++k) {
                if (std::strcmp(nm, g_VRSmokeFingerNameL[k]) != 0) continue;
                g_VRRestFingerRot[k][0] = a; g_VRRestFingerRot[k][1] = b;
                g_VRRestFingerRot[k][2] = c; g_VRRestFingerRot[k][3] = d;
                ++hit;
                break;
            }
        }
        std::fclose(f);
        if (hit > 0) {
            g_VRRestFingerCount = g_VRSmokeFingerCountL;
            g_VRRestFingerHave = 1;
            CyberpunkVR_DebugRestFingerHave = 1;
            CyberpunkVR_DebugRestFingerLoaded = hit;
        }
    }
}

void VrikReloadFingerPose(uint8_t* boneBuf) {
                    for (int hnd = 0; hnd < 2; ++hnd) {
                        if (!g_VRReloadFingerActive[hnd]) continue;
                        const int  cnt = (hnd == 0) ? g_VRSmokeFingerCountL : g_VRSmokeFingerCount;
                        const int* idx = (hnd == 0) ? g_VRSmokeFingerIdxL   : g_VRSmokeFingerIdx;
                        // Blend factor 0..1 (the preview ramp): 1 writes the target pose outright; below 1 the
                        // target is nlerp-mixed onto the LIVE tracked locals still in boneBuf at this point, so
                        // the fingers GLIDE into the grip over the ramp instead of teleporting.
                        const float b = g_VRReloadFingerBlend[hnd];
                        for (int k = 0; k < cnt && k < 32; ++k) {
                            if (!g_VRReloadFingerSet[hnd][k]) continue;
                            const int bi = idx[k];
                            if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                            float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                            const float tx = g_VRReloadFingerRot[hnd][k][0], ty = g_VRReloadFingerRot[hnd][k][1],
                                        tz = g_VRReloadFingerRot[hnd][k][2], tw = g_VRReloadFingerRot[hnd][k][3];
                            if (b >= 0.999f) {
                                q[0] = tx; q[1] = ty; q[2] = tz; q[3] = tw;
                            } else if (b > 0.001f) {
                                const float dot = q[0]*tx + q[1]*ty + q[2]*tz + q[3]*tw;
                                const float s = (dot < 0.0f) ? -b : b;      // nearer hemisphere
                                float nx = q[0]*(1.0f-b) + tx*s, ny = q[1]*(1.0f-b) + ty*s,
                                      nz = q[2]*(1.0f-b) + tz*s, nw = q[3]*(1.0f-b) + tw*s;
                                const float nl = std::sqrt(nx*nx + ny*ny + nz*nz + nw*nw);
                                if (nl > 1e-6f) { q[0] = nx/nl; q[1] = ny/nl; q[2] = nz/nl; q[3] = nw/nl; }
                            }
                        }
                    }
}

}  // namespace anim
}  // namespace cvr

// ================================================================================================
// THE RESTING RIGHT HAND -- the mirror of the block above, for the one case that did not exist until
// the weapon could be carried in the left hand: a right hand that is empty while a weapon is out.
//
// Everything here is the left implementation with the right finger list and one different gate. The
// reasoning above applies unchanged -- why the moment is chosen rather than detected, why the file is
// keyed by bone name, why it is a base layer written before the reload preview -- and is not repeated.
// ================================================================================================
static const char* kRestGripIniR = "CyberpunkVR_RestGrip_Right.ini";

extern "C" __declspec(dllexport) int CyberpunkVR_RestFingerCaptureReqR = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_RestFingerSaveReqR    = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_RestFingerApplyR      = 1;
extern "C" __declspec(dllexport) int CyberpunkVR_RestFingerForceR      = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugRestFingerHaveR  = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugRestFingerSavedR = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugRestFingerRefusedR = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugRestFingerCapsR  = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugRestBonesR       = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugRestGateR        = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugRestFingerLoadTriedR = 0;

// Raised by the weapon module while the weapon is carried in the left hand.
extern "C" __declspec(dllexport) extern int CyberpunkVR_CarryLeft;

namespace {
float g_restRotR[32][4] = {};
int   g_restCountR = 0;
int   g_restHaveR  = 0;
}  // namespace

namespace cvr {
namespace anim {

void VrikRestFingerPoseRight(uint8_t* boneBuf) {
    const int  cnt = g_VRSmokeFingerCount;
    const int* idx = g_VRSmokeFingerIdx;
    if (cnt <= 0 || !idx) return;

    // CAPTURE, on request and with empty hands: a weapon in the right hand is the pose being replaced,
    // and the cigarette owns these very fingers when it is lit.
    if (CyberpunkVR_RestFingerCaptureReqR) {
        int why = 0;
        if (g_hasWeaponEquipped)        why |= 1;
        if (g_VRSmokeFingerActive)      why |= 4;
        if (why) {
            CyberpunkVR_DebugRestFingerRefusedR = why;
            CyberpunkVR_RestFingerCaptureReqR = 0;
            return;
        }
        for (int k = 0; k < 32; ++k) {
            const int bi = (k < cnt) ? idx[k] : -1;
            if (bi < 0 || bi >= VRIK_MAX_BONES) {
                g_restRotR[k][0] = 0.0f; g_restRotR[k][1] = 0.0f;
                g_restRotR[k][2] = 0.0f; g_restRotR[k][3] = 1.0f;
                continue;
            }
            const float* q = reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
            g_restRotR[k][0] = q[0]; g_restRotR[k][1] = q[1];
            g_restRotR[k][2] = q[2]; g_restRotR[k][3] = q[3];
        }
        g_restCountR = cnt;
        g_restHaveR = 1;
        CyberpunkVR_DebugRestFingerHaveR = 1;
        ++CyberpunkVR_DebugRestFingerCapsR;
        CyberpunkVR_DebugRestFingerRefusedR = 0;
        CyberpunkVR_RestFingerCaptureReqR = 0;
        CyberpunkVR_RestFingerSaveReqR = 1;
        return;
    }

    //   1 carried in the left hand   2 pose recorded   4 apply on   8 finger list resolved
    CyberpunkVR_DebugRestGateR = ((CyberpunkVR_CarryLeft != 0) ? 1 : 0)
                               | (g_restHaveR ? 2 : 0)
                               | (CyberpunkVR_RestFingerApplyR ? 4 : 0)
                               | ((cnt > 0) ? 8 : 0);
    // THE ONE CASE: the weapon is in the LEFT hand, so this one is empty while the game still poses it
    // around a grip. With the weapon in this hand, or with both hands empty, the game is already right.
    if (CyberpunkVR_CarryLeft == 0 && !CyberpunkVR_RestFingerForceR) return;
    if (!g_restHaveR || !CyberpunkVR_RestFingerApplyR) return;

    const int n = (g_restCountR > 0 && g_restCountR < cnt) ? g_restCountR : cnt;
    int wrote = 0;
    for (int k = 0; k < n && k < 32; ++k) {
        const int bi = idx[k];
        if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
        const float* r = g_restRotR[k];
        if (r[0] == 0.0f && r[1] == 0.0f && r[2] == 0.0f && r[3] == 0.0f) continue;
        float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
        q[0] = r[0]; q[1] = r[1]; q[2] = r[2]; q[3] = r[3];
        ++wrote;
    }
    CyberpunkVR_DebugRestBonesR = wrote;
}

// Disk half, off the animation thread -- same reason as the left one: a file open inside the pose
// apply would put an unbounded wait in the middle of it.
void RestFingerRightTick() {
    if (CyberpunkVR_RestFingerSaveReqR) {
        CyberpunkVR_RestFingerSaveReqR = 0;
        FILE* f = nullptr;
        if (fopen_s(&f, VRDiagPath(kRestGripIniR).c_str(), "w") == 0 && f) {
            std::fprintf(f, "# CyberpunkVR rest grip pose v1, RIGHT hand (VRRestFingerCaptureRight)\n"
                            "# One frame of the game's own empty-handed animation. Replayed while the\n"
                            "# weapon is carried in the LEFT hand, where this one would otherwise be\n"
                            "# posed around a grip it is no longer holding.\n"
                            "# F <bone> qx qy qz qw           finger: parent-local rotation only\n");
            for (int k = 0; k < g_restCountR && k < 32; ++k) {
                const char* nm = g_VRSmokeFingerName[k];
                if (!nm || !nm[0]) continue;
                std::fprintf(f, "F %s %.9g %.9g %.9g %.9g\n", nm,
                             g_restRotR[k][0], g_restRotR[k][1],
                             g_restRotR[k][2], g_restRotR[k][3]);
            }
            std::fclose(f);
            CyberpunkVR_DebugRestFingerSavedR = 1;
        } else {
            CyberpunkVR_DebugRestFingerSavedR = -1;
        }
        return;
    }

    if (g_restHaveR || CyberpunkVR_DebugRestFingerLoadTriedR) return;
    if (g_VRSmokeFingerCount <= 0) return;
    CyberpunkVR_DebugRestFingerLoadTriedR = 1;
    FILE* f = nullptr;
    if (fopen_s(&f, VRDiagPath(kRestGripIniR).c_str(), "r") == 0 && f) {
        char line[256];
        int  hit = 0;
        while (std::fgets(line, sizeof(line), f)) {
            if (line[0] == '#' || line[0] == '\n' || line[0] == '\r') continue;
            char nm[64] = {0};
            float a = 0.0f, b = 0.0f, c = 0.0f, d = 0.0f;
            if (std::sscanf(line, "F %63s %g %g %g %g", nm, &a, &b, &c, &d) != 5) continue;
            for (int k = 0; k < g_VRSmokeFingerCount && k < 32; ++k) {
                if (std::strcmp(nm, g_VRSmokeFingerName[k]) != 0) continue;
                g_restRotR[k][0] = a; g_restRotR[k][1] = b;
                g_restRotR[k][2] = c; g_restRotR[k][3] = d;
                ++hit;
                break;
            }
        }
        std::fclose(f);
        if (hit > 0) {
            g_restCountR = g_VRSmokeFingerCount;
            g_restHaveR = 1;
            CyberpunkVR_DebugRestFingerHaveR = 1;
        }
    }
}

}  // namespace anim
}  // namespace cvr

// ================================================================================================
// THE RIGHT HAND'S GRIP -- REMEMBERED AS IT IS WORN, OFFERED BACK WHILE THE LEFT HAND CARRIES.
//
// NOTHING IS RECORDED BY HAND HERE, and that is the point: there are fifty weapons and the pose is
// already on the hand every time one of them is held. So every pass the weapon is genuinely in this
// hand, the game's own grip fingers are copied into one slot, and the carry has an exact pose to offer
// back without the player ever recording anything.
//
// A ROLLING SNAPSHOT ALSO ANSWERS EVERY QUESTION A ONE-OFF CAPTURE WOULD HAVE HAD TO. There is no draw
// animation to skip -- its frames are overwritten by the settled grip a moment later. There is no
// per-weapon file to key -- the weapon last held IS the one being carried. There is no staleness -- the
// last write lands on the frame before the carry begins. And there is nothing to persist: drawing the
// weapon teaches it again.
//
// What it does need is honest gates: the weapon actually in this hand (not carried in the other), no
// cigarette, and no reload working these fingers -- in each of those cases the pose in the buffer is
// not a grip.
//
// Replayed on top of the resting pose and faded in by CyberpunkVR_CarryGripBlend, which the reach
// spring in TwoHandGrip.cpp raises as the hand comes back inside the carry radius. Fingers only, never
// the wrist -- the rule every preview in this port follows.
// ================================================================================================
extern "C" __declspec(dllexport) int CyberpunkVR_CarryGripApply      = 1;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugCarryGripHave  = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugCarryGripSnaps = 0;
extern "C" __declspec(dllexport) int CyberpunkVR_DebugCarryGripBones = 0;

// Published by the carry: the reach spring's weight, and whether the weapon is in the left hand.
extern "C" __declspec(dllexport) extern float CyberpunkVR_CarryGripBlend;
extern "C" __declspec(dllexport) extern int   CyberpunkVR_CarryLeft;

namespace {
float g_gripRotR[32][4] = {};
int   g_gripCountR = 0;
int   g_gripHaveR  = 0;
}  // namespace

namespace cvr {
namespace anim {

void VrikCarryGripPoseRight(uint8_t* boneBuf) {
    const int  cnt = g_VRSmokeFingerCount;
    const int* idx = g_VRSmokeFingerIdx;
    if (cnt <= 0 || !idx) return;

    // LEARN IT, while the hand is really holding the weapon. This pass runs before the reload and
    // cigarette layers, so what is in the buffer here is the game's own pose and not one of ours.
    if (g_hasWeaponEquipped && CyberpunkVR_CarryLeft == 0
        && !g_VRSmokeFingerActive && !g_VRReloadFingerActive[1]) {
        for (int k = 0; k < cnt && k < 32; ++k) {
            const int bi = idx[k];
            if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
            const float* q = reinterpret_cast<const float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
            g_gripRotR[k][0] = q[0]; g_gripRotR[k][1] = q[1];
            g_gripRotR[k][2] = q[2]; g_gripRotR[k][3] = q[3];
        }
        g_gripCountR = cnt;
        g_gripHaveR = 1;
        CyberpunkVR_DebugCarryGripHave = 1;
        ++CyberpunkVR_DebugCarryGripSnaps;
        return;                     // and there is nothing to offer a hand that already has the gun
    }

    if (!g_gripHaveR || !CyberpunkVR_CarryGripApply) return;
    float b = CyberpunkVR_CarryGripBlend;
    if (b <= 0.001f) return;
    if (b > 1.0f) b = 1.0f;

    // The mixing is TwoHandFingers' own nlerp: a preview grows out of whatever the layer below left
    // (here the resting hand), it does not replace it.
    const int n = (g_gripCountR > 0 && g_gripCountR < cnt) ? g_gripCountR : cnt;
    int wrote = 0;
    for (int k = 0; k < n && k < 32; ++k) {
        const int bi = idx[k];
        if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
        const float* tq = g_gripRotR[k];
        if (tq[0] == 0.0f && tq[1] == 0.0f && tq[2] == 0.0f && tq[3] == 0.0f) continue;
        float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
        if (b >= 0.999f) {
            q[0]=tq[0]; q[1]=tq[1]; q[2]=tq[2]; q[3]=tq[3];
            ++wrote;
            continue;
        }
        const float dot = q[0]*tq[0] + q[1]*tq[1] + q[2]*tq[2] + q[3]*tq[3];
        const float s = (dot < 0.0f) ? -b : b;
        float nq[4] = { q[0]*(1.0f-b) + tq[0]*s, q[1]*(1.0f-b) + tq[1]*s,
                        q[2]*(1.0f-b) + tq[2]*s, q[3]*(1.0f-b) + tq[3]*s };
        const float nl = std::sqrt(nq[0]*nq[0] + nq[1]*nq[1] + nq[2]*nq[2] + nq[3]*nq[3]);
        if (nl > 1e-6f) { q[0]=nq[0]/nl; q[1]=nq[1]/nl; q[2]=nq[2]/nl; q[3]=nq[3]/nl; ++wrote; }
    }
    CyberpunkVR_DebugCarryGripBones = wrote;
}

}  // namespace anim
}  // namespace cvr
