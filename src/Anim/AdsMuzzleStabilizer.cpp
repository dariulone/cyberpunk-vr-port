#include "Utils/DebugGate.hpp"
// AdsMuzzleStabilizer -- the muzzle keeps pointing where it pointed, through the vanilla ADS raise.
//
// Ported from dabinn's TofuExpress (797a2a95, "fix(ads): prevent laser dot shifting during non-VRIK
// ADS"). The problem is specific to the NON-VRIK arms: raising the sights plays the game's authored
// aim-in animation, and that animation does not merely move the weapon closer to the eye, it CHANGES
// THE DIRECTION the barrel points. On a flat screen nobody notices, because the game then draws its
// reticle wherever the weapon ends up. In VR the aim point is derived from the real muzzle transform,
// so the dot -- and the bullet -- slide away from where the player was aiming a moment earlier.
//
// The shape of the fix is a closed loop rather than an override, and that is the whole trick: the
// vanilla animation is left to play, and only the residual DIRECTION error it introduces is taken
// back out.
//
//   HIP FIRE records the muzzle direction that is trustworthy, in GAME-HEADING space. Heading space
//   is the right frame because it follows mouse/stick turning while excluding physical HMD rotation:
//   a hip aim should travel with the right stick and stay put when only the player's head moves.
//
//   WHILE AIMING the recorded direction is rotated back into the world, the angular error against the
//   live muzzle is measured, and 35% of it is applied per tick to the weapon bone -- in MODEL
//   space, converted through the entity quaternion. Bounded at 15 degrees total, so a wrong reference
//   can never throw the weapon across the screen.
//
//   WHEN AIM-IN ENDS the accumulated correction is converted into a WEAPON-LOCAL delta and frozen.
//   That conversion is what keeps ADS alive: a frozen model-space rotation would fight breathing,
//   sway and right-stick aiming, while the same rotation expressed in the weapon's own frame rides
//   along with all three.
//
// Two traps are handled explicitly, both learned the hard way upstream:
//
//   * The pose hook visits the same buffer several times per entity tick. Composing the correction
//     onto an already-corrected pass multiplies it, and the skew accumulates until it is permanent --
//     so the tick's RAW weapon bone local rotation is cached and every pass composes from that.
//   * The hip reference must not be learned from the lowered weapon or from the raise transition, or
//     it records a direction the player never aimed. Weapon PSM 5 is the real ranged Ready state; Safe
//     and PublicSafeToReady are excluded by the redscript that publishes those states.
//
// DEPARTURES FROM THE ORIGINAL. His version carries the redscript values and the enable flag in
// shared-memory slots [158..162]; those numbers are already taken in this tree (the B and Y buttons,
// the trigger channel) and, more to the point, everything here lives in ONE plugin now -- the natives
// that receive the redscript values and this consumer are the same DLL, so they are plain globals.
// The aiming flag is g_isAiming, which the camera hook already refreshes, rather than a slot. And the
// heading comes from the LATCHED view packet instead of a live slot read, because this tree has
// measured that mixing a latched view with a directly-read heading is what produced the snap-turn arm
// double.

#include "Anim/AdsMuzzleStabilizer.hpp"

#include "Anim/CharacterRig.hpp"
#include "Anim/AdsEyeAlign.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include "Anim/LeftHanded.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Camera/CameraState.hpp"
#include "Core/VrCoreShared.hpp"   // g_isAiming
#include "Utils/SharedSlots.hpp"

#include <cmath>
#include <cstdint>

extern float* g_pSharedHands;

// 1 = correct the non-VRIK ADS muzzle drift (default). The knob exists because this writes a bone
// every tick while aiming, and a feature that writes bones should be switchable without a rebuild.
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_NonVrikAdsStabilizer = 1;
// How often the correction is actually applied, and how often the reference was refused. Both are
// answers to "is it running at all", which cost a round trip to guess at.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugAdsStabApplies = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugAdsStabHipCaptures = 0;

namespace cvr::anim {

void ApplyNonVrikAdsMuzzleStabilizer(uint8_t* boneBuf) {
    // One entry per pose buffer the hook visits, so each buffer keeps its own raw rotation for the
    // tick. Four is one more than the three the player pass has been measured to use.
    struct RawWeaponPoseCache {
        uint8_t* boneBuf = nullptr;
        float tick = -1.0f;
        int weaponIdx = -1;
        int handIdx = -1;
        // BOTH the weapon's and the hand's authored local rotations. The correction is applied to the
        // hand, so the hand's raw value is as much a part of "the pose this tick started from" as the
        // weapon's (dabinn, TofuExpress 73bdf668).
        float weaponLocalRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
        float handLocalRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    };
    static float s_hipAimHeading[3] = {0.0f, 1.0f, 0.0f};
    static float s_totalModelCorrection[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    static float s_frozenWeaponLocalCorrection[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    static float s_lastTick = -1.0f;
    static bool s_hipAimValid = false;
    static bool s_prevAiming = false;
    static bool s_adsCorrectionFrozen = false;
    static bool s_adsSawAimIn = false;
    static bool s_freezeLocalCorrectionPending = false;
    static int s_adsTransitionTicks = 0;
    static int s_adsConvergedTicks = 0;
    static RawWeaponPoseCache s_rawPoseCache[4];
    static int s_rawPoseReplace = 0;

    // Off, VRIK-driven arms, no weapon out, or no muzzle published: reset to identity so nothing is
    // carried into the next time it applies. A stale correction is worse than none.
    // Head aim writes the weapon's rotation outright, so there is no vanilla direction error
    // left to correct -- and two writers on one bone is a fight, not a fix.
    if (!g_pSharedHands || IsHeadAimWeaponActive() || g_VRBind > 0 ||
        !CyberpunkVR_NonVrikAdsStabilizer ||
        g_pSharedHands[vrshared::kWeaponFlag] <= 0.5f ||
        g_pSharedHands[27] <= 0.5f) {
        s_totalModelCorrection[0] = 0.0f; s_totalModelCorrection[1] = 0.0f;
        s_totalModelCorrection[2] = 0.0f; s_totalModelCorrection[3] = 1.0f;
        s_frozenWeaponLocalCorrection[0] = 0.0f; s_frozenWeaponLocalCorrection[1] = 0.0f;
        s_frozenWeaponLocalCorrection[2] = 0.0f; s_frozenWeaponLocalCorrection[3] = 1.0f;
        s_prevAiming = false;
        s_adsCorrectionFrozen = false;
        s_freezeLocalCorrectionPending = false;
        return;
    }

    // The weapon in hand's bone: WeaponRight, or WeaponLeft with the weapon in the left hand.
    const int weaponIdx = HeldWeaponBone();
    if (weaponIdx < 0 || weaponIdx >= VRIK_FKCount()) return;

    float muzzle[3] = { g_pSharedHands[24], g_pSharedHands[25], g_pSharedHands[26] };
    if (VRIK_Norm3(muzzle) < 0.5f) return;
    float entQ[4] = { g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR };
    VRIK_QuatNorm(entQ);

    // The heading from the packet latched for THIS solve, not a live read -- see the file header.
    if (!g_viewPktValid) return;
    const float heading = g_viewPkt[8];

    const bool aiming = g_isAiming;
    const float tick = g_pSharedHands[vrshared::kEntitySeq];
    if (tick != s_lastTick) {
        s_lastTick = tick;
        const int weaponPsm = static_cast<int>(std::lround(g_VRWeaponPsmState));
        const bool weaponPsmReady = (weaponPsm == 5);
        const bool safeToReady = (g_VRWeaponRaiseTransition != 0);
        const bool aimInRunning = (g_VRAimInRemaining > 0.001f);

        if (aiming && !s_prevAiming) {
            s_totalModelCorrection[0] = 0.0f; s_totalModelCorrection[1] = 0.0f;
            s_totalModelCorrection[2] = 0.0f; s_totalModelCorrection[3] = 1.0f;
            s_frozenWeaponLocalCorrection[0] = 0.0f; s_frozenWeaponLocalCorrection[1] = 0.0f;
            s_frozenWeaponLocalCorrection[2] = 0.0f; s_frozenWeaponLocalCorrection[3] = 1.0f;
            s_adsCorrectionFrozen = false;
            s_adsSawAimIn = aimInRunning;
            s_freezeLocalCorrectionPending = false;
            s_adsTransitionTicks = 0;
            s_adsConvergedTicks = 0;
        } else if (!aiming && s_prevAiming) {
            s_adsCorrectionFrozen = false;
            s_adsSawAimIn = false;
            s_freezeLocalCorrectionPending = false;
            s_adsTransitionTicks = 0;
            s_adsConvergedTicks = 0;
        }

        if (!aiming) {
            s_totalModelCorrection[0] = 0.0f; s_totalModelCorrection[1] = 0.0f;
            s_totalModelCorrection[2] = 0.0f; s_totalModelCorrection[3] = 1.0f;

            // Weapon PSM 5 is the actual ranged Ready state. In Safe, and through the
            // PublicSafeToReady raise, the previous reference is preserved rather than replaced with
            // the lowered pose or a pose mid-transition.
            if (weaponPsmReady && !safeToReady) {
                const float hs = std::sin(heading * 0.5f);
                const float hc = std::cos(heading * 0.5f);
                const float invHeadingQ[4] = {0.0f, 0.0f, -hs, hc};
                float local[3];
                VRIK_QuatRotateVec(invHeadingQ, muzzle, local);
                if (VRIK_Norm3(local) > 0.5f) {
                    // Ready is the authority: whatever the barrel dot shows this tick IS the aim.
                    // Combat motion and turn sway are valid samples, not outliers to be filtered.
                    s_hipAimHeading[0] = local[0];
                    s_hipAimHeading[1] = local[1];
                    s_hipAimHeading[2] = local[2];
                    s_hipAimValid = true;
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugAdsStabHipCaptures);
                }
            }
        } else if (s_hipAimValid && !s_adsCorrectionFrozen) {
            if (aimInRunning) s_adsSawAimIn = true;
            const float headingQ[4] = {
                0.0f, 0.0f, std::sin(heading * 0.5f), std::cos(heading * 0.5f)};
            float desiredWorld[3];
            VRIK_QuatRotateVec(headingQ, s_hipAimHeading, desiredWorld);
            VRIK_Norm3(desiredWorld);

            float errorWorld[4];
            VRIK_QuatFromTo(muzzle, desiredWorld, errorWorld);
            float alignment = muzzle[0] * desiredWorld[0] + muzzle[1] * desiredWorld[1] +
                              muzzle[2] * desiredWorld[2];
            if (alignment > 1.0f) alignment = 1.0f;
            if (alignment < -1.0f) alignment = -1.0f;
            const float errorRadians = std::acos(alignment);
            ++s_adsTransitionTicks;
            if (errorRadians < 0.2f * 0.01745329252f) {
                if (s_adsConvergedTicks < 1000) ++s_adsConvergedTicks;
            } else {
                s_adsConvergedTicks = 0;
            }

            // 35% of the error per tick, in model space (entity-conjugated), accumulated and clamped.
            float stepWorld[4];
            VRIK_QuatScale(errorWorld, 0.35f, stepWorld);
            float invEnt[4];
            VRIK_QuatConj(entQ, invEnt);
            float tmp[4], stepModel[4];
            VRIK_QuatMul(invEnt, stepWorld, tmp);
            VRIK_QuatMul(tmp, entQ, stepModel);
            VRIK_QuatNorm(stepModel);

            float next[4];
            VRIK_QuatMul(stepModel, s_totalModelCorrection, next);
            VRIK_QuatNorm(next);
            constexpr float kMaxCorrectionRadians = 15.0f * 0.01745329252f;
            float w = std::fabs(next[3]);
            if (w > 1.0f) w = 1.0f;
            const float angle = 2.0f * std::acos(w);
            if (angle > kMaxCorrectionRadians) {
                VRIK_QuatScale(next, kMaxCorrectionRadians / angle, next);
            }
            s_totalModelCorrection[0] = next[0]; s_totalModelCorrection[1] = next[1];
            s_totalModelCorrection[2] = next[2]; s_totalModelCorrection[3] = next[3];

            // AimInTimeRemaining is authored by AimingStateEvents for the real weapon ADS
            // transition, so it is the exact end of the thing being corrected. The convergence
            // fallback is only for weapons whose AimInTime is zero or missing.
            if ((s_adsSawAimIn && !aimInRunning) ||
                (!s_adsSawAimIn && s_adsTransitionTicks >= 20 && s_adsConvergedTicks >= 3)) {
                s_adsCorrectionFrozen = true;
                s_freezeLocalCorrectionPending = true;
            }
        }
        s_prevAiming = aiming;
    }

    if (!aiming || !s_hipAimValid) return;

    // ALWAYS COMPOSE FROM THE TICK'S RAW ROTATION. The hook visits this buffer several times per
    // tick; composing onto an already-corrected pass multiplies the correction and the skew becomes
    // permanent -- in ADS and in the hip pose that follows it.
    RawWeaponPoseCache* rawPose = nullptr;
    for (auto& entry : s_rawPoseCache) {
        if (entry.boneBuf == boneBuf) { rawPose = &entry; break; }
    }
    if (!rawPose) {
        rawPose = &s_rawPoseCache[s_rawPoseReplace++ & 3];
        rawPose->boneBuf = boneBuf;
        rawPose->tick = -1.0f;
    }
    const int handIdx = static_cast<int>(WeaponInLeftHand() ? g_VRLeftBoneIdx : g_VRRightBoneIdx);
    if (handIdx < 0 || handIdx >= VRIK_FKCount() || g_VRBoneParent[weaponIdx] != handIdx) return;
    if (rawPose->tick != tick || rawPose->weaponIdx != weaponIdx || rawPose->handIdx != handIdx) {
        const float* rawWeaponLocal =
            reinterpret_cast<const float*>(boneBuf + weaponIdx * 48 + VRIK_ROT_OFF);
        const float* rawHandLocal =
            reinterpret_cast<const float*>(boneBuf + handIdx * 48 + VRIK_ROT_OFF);
        rawPose->weaponLocalRot[0] = rawWeaponLocal[0]; rawPose->weaponLocalRot[1] = rawWeaponLocal[1];
        rawPose->weaponLocalRot[2] = rawWeaponLocal[2]; rawPose->weaponLocalRot[3] = rawWeaponLocal[3];
        rawPose->handLocalRot[0] = rawHandLocal[0]; rawPose->handLocalRot[1] = rawHandLocal[1];
        rawPose->handLocalRot[2] = rawHandLocal[2]; rawPose->handLocalRot[3] = rawHandLocal[3];
        VRIK_QuatNorm(rawPose->weaponLocalRot);
        VRIK_QuatNorm(rawPose->handLocalRot);
        rawPose->tick = tick;
        rawPose->weaponIdx = weaponIdx;
        rawPose->handIdx = handIdx;
    }

    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    const int handParent = g_VRBoneParent[handIdx];
    if (handParent < 0 || handParent >= handIdx) return;
    // The weapon's model rotation this tick STARTED at: hand-from-its-parent, then the authored grip.
    float rawHandModel[4];
    VRIK_QuatMul(g_fkRot[handParent], rawPose->handLocalRot, rawHandModel);
    VRIK_QuatNorm(rawHandModel);
    float rawModel[4];
    VRIK_QuatMul(rawHandModel, rawPose->weaponLocalRot, rawModel);
    VRIK_QuatNorm(rawModel);

    float correctedModel[4];
    if (s_adsCorrectionFrozen && !s_freezeLocalCorrectionPending) {
        // Frozen: the correction rides in the WEAPON's frame, so breathing, sway and right-stick
        // aiming all still move the weapon normally.
        VRIK_QuatMul(rawModel, s_frozenWeaponLocalCorrection, correctedModel);
    } else {
        VRIK_QuatMul(s_totalModelCorrection, rawModel, correctedModel);
        if (s_freezeLocalCorrectionPending) {
            float invRawModel[4];
            VRIK_QuatConj(rawModel, invRawModel);
            VRIK_QuatMul(invRawModel, correctedModel, s_frozenWeaponLocalCorrection);
            VRIK_QuatNorm(s_frozenWeaponLocalCorrection);
            s_freezeLocalCorrectionPending = false;
        }
    }
    VRIK_QuatNorm(correctedModel);
    // Through the hand, with the tick's RAW grip as the factor to divide out -- so the grip survives
    // and repeated passes cannot compound.
    WriteWeaponModelRotViaHand(boneBuf, weaponIdx, correctedModel, rawPose->weaponLocalRot);
    CVR_DIAGNOSTIC(++CyberpunkVR_DebugAdsStabApplies);
}

}  // namespace cvr::anim
