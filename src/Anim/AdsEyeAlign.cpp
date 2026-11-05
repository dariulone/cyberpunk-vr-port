// AdsEyeAlign -- the game's aim-down-sights pose is centred between the eyes, and you look through one.
//
// Ported from dabinn's TofuExpress (73bdf668, "feat(aiming): align non-VRIK and Head Aim ADS poses
// with the right eye").
//
// THE PROBLEM. The authored ADS animation puts the sight in front of the CAMERA, which in this port is
// the cyclopean point between the eyes. On a flat screen that is the screen centre and it is correct.
// In VR the player sights with one eye, half an IPD to the side, so the sight sits beside the line of
// sight rather than on it -- and the closer the sight is to the face, the larger the miss. Nothing
// about the weapon's own pose is wrong; it is aligned to a viewpoint nobody is looking from.
//
// THE FIX, and it is a re-anchoring rather than a correction: take the whole authored arm pose and
// move it so that its camera-relative geometry hangs off the RIGHT EYE instead of the centre. The
// pose keeps its shape -- grip, wrist, elbow bend, the animation's own motion -- and only the frame
// it is expressed in changes.
//
//   THE PIVOT IS THE CYCLOPEAN POINT, NOT THE EYE. Hand and elbow positions are taken relative to the
//   camera (the centre), rotated by the live head delta, and re-anchored at the eye. Pre-shifting the
//   source by half an IPD as well would cancel exactly the dominant-eye correction this exists for.
//
//   THE ANCHOR IS LATCHED, the live IPD orbit is not. The eye offset is captured in the vanilla
//   camera's frame when ADS begins, so walking, crouching and vehicle motion still carry the arms
//   with the player, while PHYSICAL head translation afterwards stays free -- that is how a player
//   fine-adjusts a sight, by moving their head, and it must not drag the weapon along.
//
//   HEAD AIM ALSO ROTATES, non-VRIK does not. Under head aim the weapon follows the HMD, so the arms
//   must orbit the eye with it (delta = the live rotation since the anchor). In non-VRIK hand aim the
//   game still owns the direction, so only the translation is re-anchored (delta = identity).
//
// THE CENTRED CAMERA FRAME IS RECOVERED BY DIVIDING OUT THE HMD, not by reading the engine camera:
// the render view is composed as gameHeading * mappedHmd, and the engine's camera orientation may
// already contain the HMD. The raw head orientation published beside the view packet is the exact
// factor to remove, so this holds even when the player is looking away from their body.
//
// AND THE GRIP IS NEVER OVERWRITTEN. Where a weapon rotation has to be applied, it is applied to the
// hand and the authored weapon bone's local transform is left alone:
//
//     desiredWeaponModel = desiredHandModel * weaponLocal
//         =>  desiredHandModel = desiredWeaponModel * inverse(weaponLocal)
//
// Writing the weapon bone directly would have replaced the authored grip with whatever rotation the
// aim wanted, which is how a pistol ends up held sideways.
//
// TWO DEPARTURES FROM THE ORIGINAL. His repeat-pass cache is keyed on SharedPose(13) -- a component
// of the right controller quaternion, which happens to change most frames; here it is keyed on the
// entity tick, the quantity that actually means "a new pose". And the arm solve is rotation-only in
// both versions, which is deliberate: writing a hand TRANSLATION would stretch the wrist off the
// forearm.
//
// THE WEAPON IN THE LEFT HAND (Anim/LeftHanded.hpp) is the same pose mirrored: its animations are the
// right hand's mirrored onto the left, so it is sighted with the LEFT eye, and the arm that holds the
// weapon -- the one whose rotation the weapon writer owns -- is the left one. The arms are kept by role,
// weapon arm and support arm, not by side.

#include "Anim/AdsEyeAlign.hpp"

#include "Anim/AdsMuzzleStabilizer.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include "Anim/LeftHanded.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Core/VrCoreShared.hpp"   // g_isAiming
#include "Utils/SharedSlots.hpp"

#include <cmath>
#include <cstdint>

extern float* g_pSharedHands;

namespace cvr::anim {

namespace {

// One entry per pose buffer the hook visits in a tick.
struct AimArmPose {
    uint8_t* boneBuf = nullptr;
    float tick = -1.0f;
    int bone[6] = {-1, -1, -1, -1, -1, -1};   // weapon arm upper/fore/hand, support arm upper/fore/hand
    float localRot[6][4] = {};
    float rawPos[6][3] = {};
    float rawRot[6][4] = {};
    float targetHand[2][3] = {};
    float targetElbow[2][3] = {};
    float targetSupportRot[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    bool valid = false;
};
AimArmPose g_aimArmPose[4];

// The sighting eye (side +1 the right, -1 the left), and the frames needed to move a pose onto it: eye
// in world and in model space, the view rotation in model space, the camera's model position, and the
// CENTRED camera rotation with the HMD divided out.
bool CurrentSightingEye(float side, float* outWorld, float* outModel, float* outViewModel,
                        float* outCamModelPos, float* outCentreModelRot) {
    float unusedCamRot[4];
    if (!g_viewPktValid || !VRIK_ComputeCamModel(outCamModelPos, unusedCamRot)) return false;
    float headWorld[3];
    if (!VRIK_ResolveViewPos(headWorld)) return false;

    float viewQ[4] = { g_viewPkt[0], g_viewPkt[1], g_viewPkt[2], g_viewPkt[3] };
    VRIK_QuatNorm(viewQ);
    const float rightAxis[3] = {1.0f, 0.0f, 0.0f};
    float rightWorld[3];
    VRIK_QuatRotateVec(viewQ, rightAxis, rightWorld);
    const float halfIpd = SharedPose(95) * side;
    outWorld[0] = headWorld[0] + rightWorld[0] * halfIpd;
    outWorld[1] = headWorld[1] + rightWorld[1] * halfIpd;
    outWorld[2] = headWorld[2] + rightWorld[2] * halfIpd;

    float entQ[4] = { g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR };
    VRIK_QuatNorm(entQ);
    float invEnt[4];
    VRIK_QuatConj(entQ, invEnt);
    const float eyeDeltaWorld[3] = { outWorld[0] - g_VREntityPosX,
                                     outWorld[1] - g_VREntityPosY,
                                     outWorld[2] - g_VREntityPosZ };
    VRIK_QuatRotateVec(invEnt, eyeDeltaWorld, outModel);
    VRIK_QuatMul(invEnt, viewQ, outViewModel);
    VRIK_QuatNorm(outViewModel);

    // Divide out the exact raw HMD quaternion published beside this view -- axis-mapped the same way
    // the compose maps it -- rather than comparing against the engine camera, which may already carry
    // the HMD. What remains is the centred game-camera frame the authored weapon pose was made in.
    float hmdGame[4] = { g_viewPkt[13], -g_viewPkt[15], g_viewPkt[14], g_viewPkt[16] };
    if ((hmdGame[0] * hmdGame[0] + hmdGame[1] * hmdGame[1] +
         hmdGame[2] * hmdGame[2] + hmdGame[3] * hmdGame[3]) < 1e-6f) {
        hmdGame[0] = 0.0f; hmdGame[1] = 0.0f; hmdGame[2] = 0.0f; hmdGame[3] = 1.0f;
    } else {
        VRIK_QuatNorm(hmdGame);
    }
    float invHmdGame[4];
    VRIK_QuatConj(hmdGame, invHmdGame);
    float centreWorld[4];
    VRIK_QuatMul(viewQ, invHmdGame, centreWorld);
    VRIK_QuatMul(invEnt, centreWorld, outCentreModelRot);
    VRIK_QuatNorm(outCentreModelRot);
    return true;
}

// Rotation-only two-bone solve for an authored arm pose. Segment translations are untouched, so this
// cannot stretch the wrist away from the forearm the way writing the hand's position would.
void SolveAimArm(uint8_t* boneBuf, int upperIdx, int foreIdx, int handIdx,
                 const float rawPos[3][3], const float rawRot[3][4],
                 const float* targetHand, const float* elbowHint, const float* targetHandRot) {
    float upVec[3] = { rawPos[1][0] - rawPos[0][0], rawPos[1][1] - rawPos[0][1],
                       rawPos[1][2] - rawPos[0][2] };
    float foreVec[3] = { rawPos[2][0] - rawPos[1][0], rawPos[2][1] - rawPos[1][1],
                         rawPos[2][2] - rawPos[1][2] };
    const float upLen = VRIK_Norm3(upVec), foreLen = VRIK_Norm3(foreVec);
    float toHand[3] = { targetHand[0] - rawPos[0][0], targetHand[1] - rawPos[0][1],
                        targetHand[2] - rawPos[0][2] };
    const float dist = VRIK_Norm3(toHand);
    if (upLen < 1e-4f || foreLen < 1e-4f || dist < 1e-4f) return;

    const float minD = std::fabs(upLen - foreLen) + 1e-4f, maxD = upLen + foreLen - 1e-4f;
    float reach = dist;
    if (reach < minD) reach = minD;
    if (reach > maxD) reach = maxD;
    const float along = (upLen * upLen - foreLen * foreLen + reach * reach) / (2.0f * reach);
    const float height = std::sqrt(std::fmax(0.0f, upLen * upLen - along * along));
    const float linePoint[3] = { rawPos[0][0] + toHand[0] * along,
                                 rawPos[0][1] + toHand[1] * along,
                                 rawPos[0][2] + toHand[2] * along };
    float bend[3] = { elbowHint[0] - linePoint[0], elbowHint[1] - linePoint[1],
                      elbowHint[2] - linePoint[2] };
    float proj = VRIK_Dot3(bend, toHand);
    bend[0] -= toHand[0] * proj; bend[1] -= toHand[1] * proj; bend[2] -= toHand[2] * proj;
    if (VRIK_Norm3(bend) < 1e-4f) {
        // The hint collapsed onto the shoulder-to-hand line: fall back to the authored elbow, which
        // is the pose's own bend direction.
        bend[0] = rawPos[1][0] - linePoint[0];
        bend[1] = rawPos[1][1] - linePoint[1];
        bend[2] = rawPos[1][2] - linePoint[2];
        proj = VRIK_Dot3(bend, toHand);
        bend[0] -= toHand[0] * proj; bend[1] -= toHand[1] * proj; bend[2] -= toHand[2] * proj;
        if (VRIK_Norm3(bend) < 1e-4f) return;
    }
    const float newElbow[3] = { linePoint[0] + bend[0] * height,
                                linePoint[1] + bend[1] * height,
                                linePoint[2] + bend[2] * height };
    float desiredUp[3] = { newElbow[0] - rawPos[0][0], newElbow[1] - rawPos[0][1],
                           newElbow[2] - rawPos[0][2] };
    VRIK_Norm3(desiredUp);
    float desiredFore[3] = { targetHand[0] - newElbow[0], targetHand[1] - newElbow[1],
                             targetHand[2] - newElbow[2] };
    VRIK_Norm3(desiredFore);

    float dUp[4], newUp[4];
    VRIK_QuatFromTo(upVec, desiredUp, dUp);
    VRIK_QuatMul(dUp, rawRot[0], newUp);
    VRIK_QuatNorm(newUp);
    float dFore[4], newFore[4];
    VRIK_QuatFromTo(foreVec, desiredFore, dFore);
    VRIK_QuatMul(dFore, rawRot[1], newFore);
    VRIK_QuatNorm(newFore);

    const int upParent = g_VRBoneParent[upperIdx];
    const float identity[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    VRIK_WriteLocalRot(boneBuf, upperIdx,
                       (upParent >= 0 && upParent < VRIK_FKCount()) ? g_fkRot[upParent] : identity,
                       newUp);
    VRIK_WriteLocalRot(boneBuf, foreIdx, newUp, newFore);
    VRIK_WriteLocalRot(boneBuf, handIdx, newFore, targetHandRot);
}

}  // namespace

bool WriteWeaponModelRotViaHand(uint8_t* boneBuf, int weaponIdx,
                                const float* desiredWeaponModel,
                                const float* weaponLocalOverride) {
    if (weaponIdx < 0 || weaponIdx >= VRIK_FKCount()) return false;
    const int handIdx = g_VRBoneParent[weaponIdx];
    if (handIdx < 0 || handIdx >= VRIK_FKCount() ||
        (handIdx != g_VRRightBoneIdx && handIdx != g_VRLeftBoneIdx)) {
        return false;
    }
    const int handParent = g_VRBoneParent[handIdx];
    if (handParent < 0 || handParent >= handIdx) return false;

    float weaponLocal[4];
    if (weaponLocalOverride) {
        weaponLocal[0] = weaponLocalOverride[0]; weaponLocal[1] = weaponLocalOverride[1];
        weaponLocal[2] = weaponLocalOverride[2]; weaponLocal[3] = weaponLocalOverride[3];
    } else {
        const float* local =
            reinterpret_cast<const float*>(boneBuf + weaponIdx * 48 + VRIK_ROT_OFF);
        weaponLocal[0] = local[0]; weaponLocal[1] = local[1];
        weaponLocal[2] = local[2]; weaponLocal[3] = local[3];
    }
    VRIK_QuatNorm(weaponLocal);

    float invWeaponLocal[4];
    VRIK_QuatConj(weaponLocal, invWeaponLocal);
    float desiredHandModel[4];
    VRIK_QuatMul(desiredWeaponModel, invWeaponLocal, desiredHandModel);
    VRIK_QuatNorm(desiredHandModel);
    VRIK_WriteLocalRot(boneBuf, handIdx, g_fkRot[handParent], desiredHandModel);
    return true;
}

void PrepareAimArmTargets(uint8_t* boneBuf) {
    static bool s_prevHeadAim = false, s_prevAiming = false;
    static bool s_headAnchorValid = false, s_nonVrikAnchorValid = false;
    static float s_headCentreOffsetCam[3] = {0.0f, 0.0f, 0.0f};
    static float s_nonVrikEyeOffsetCam[3] = {0.0f, 0.0f, 0.0f};

    const bool headAim = IsHeadAimWeaponActive();
    const bool nonVrik = g_pSharedHands && g_VRBind <= 0 && CyberpunkVR_NonVrikAdsStabilizer &&
                         g_pSharedHands[vrshared::kWeaponFlag] > 0.5f;
    const bool aiming = g_isAiming;
    const bool active = aiming && (headAim || nonVrik);

    AimArmPose* pose = nullptr;
    for (auto& entry : g_aimArmPose) {
        entry.valid = false;
        if (entry.boneBuf == boneBuf) { pose = &entry; break; }
        if (!pose && entry.boneBuf == nullptr) pose = &entry;
    }
    if (!active || !pose) {
        if (!aiming) { s_headAnchorValid = false; s_nonVrikAnchorValid = false; }
        s_prevHeadAim = headAim;
        s_prevAiming = aiming;
        return;
    }
    if (pose->boneBuf != boneBuf) { pose->boneBuf = boneBuf; pose->tick = -1.0f; }

    const bool leftWeapon = WeaponInLeftHand();
    const int right[3] = { g_VRRightUpperArmIdx, g_VRRightForeArmIdx, g_VRRightBoneIdx };
    const int left[3] = { g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx, g_VRLeftBoneIdx };
    const int* weaponArm = leftWeapon ? left : right;
    const int* supportArm = leftWeapon ? right : left;
    const int bone[6] = { weaponArm[0], weaponArm[1], weaponArm[2],
                          supportArm[0], supportArm[1], supportArm[2] };
    for (int i = 0; i < 6; ++i) if (bone[i] < 0 || bone[i] >= VRIK_FKCount()) return;

    // THE TICK, not a controller quaternion component. The hook visits this buffer several times per
    // tick: the first pass records the authored rotations, the repeats restore them, so every pass
    // re-anchors the same original pose instead of re-anchoring its own output.
    const float tick = g_pSharedHands[vrshared::kEntitySeq];
    bool samePose = (pose->tick == tick);
    for (int i = 0; i < 6; ++i) samePose = samePose && pose->bone[i] == bone[i];
    if (samePose) {
        for (int i = 0; i < 6; ++i) {
            float* q = reinterpret_cast<float*>(boneBuf + bone[i] * 48 + VRIK_ROT_OFF);
            q[0] = pose->localRot[i][0]; q[1] = pose->localRot[i][1];
            q[2] = pose->localRot[i][2]; q[3] = pose->localRot[i][3];
        }
    } else {
        for (int i = 0; i < 6; ++i) {
            const float* q =
                reinterpret_cast<const float*>(boneBuf + bone[i] * 48 + VRIK_ROT_OFF);
            pose->bone[i] = bone[i];
            pose->localRot[i][0] = q[0]; pose->localRot[i][1] = q[1];
            pose->localRot[i][2] = q[2]; pose->localRot[i][3] = q[3];
        }
        pose->tick = tick;
    }

    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    for (int i = 0; i < 6; ++i) {
        for (int k = 0; k < 3; ++k) pose->rawPos[i][k] = g_fkPos[bone[i]][k];
        for (int k = 0; k < 4; ++k) pose->rawRot[i][k] = g_fkRot[bone[i]][k];
    }

    const float eyeSide = leftWeapon ? -1.0f : 1.0f;
    float eyeWorld[3], eyeModel[3], viewModel[4], camPos[3], centreRot[4];
    if (!CurrentSightingEye(eyeSide, eyeWorld, eyeModel, viewModel, camPos, centreRot)) return;
    float invCentre[4];
    VRIK_QuatConj(centreRot, invCentre);
    float liveDelta[4];
    VRIK_QuatMul(viewModel, invCentre, liveDelta);
    VRIK_QuatNorm(liveDelta);

    // The eye, and the cyclopean point it orbits, both expressed in the centred camera's frame so an
    // anchor captured now survives the player walking and turning.
    const float eyeFromCentre[3] = { eyeModel[0] - camPos[0], eyeModel[1] - camPos[1],
                                     eyeModel[2] - camPos[2] };
    float eyeOffsetCam[3];
    VRIK_QuatRotateVec(invCentre, eyeFromCentre, eyeOffsetCam);
    const float rightAxis[3] = {1.0f, 0.0f, 0.0f};
    float eyeSideOffset[3];
    VRIK_QuatRotateVec(viewModel, rightAxis, eyeSideOffset);
    const float halfIpd = SharedPose(95) * eyeSide;
    eyeSideOffset[0] *= halfIpd; eyeSideOffset[1] *= halfIpd; eyeSideOffset[2] *= halfIpd;
    const float headCentre[3] = { eyeModel[0] - eyeSideOffset[0], eyeModel[1] - eyeSideOffset[1],
                                  eyeModel[2] - eyeSideOffset[2] };
    const float headFromCam[3] = { headCentre[0] - camPos[0], headCentre[1] - camPos[1],
                                   headCentre[2] - camPos[2] };
    float headOffsetCam[3];
    VRIK_QuatRotateVec(invCentre, headFromCam, headOffsetCam);

    if (headAim && (!s_prevHeadAim || !s_prevAiming || !s_headAnchorValid)) {
        for (int k = 0; k < 3; ++k) s_headCentreOffsetCam[k] = headOffsetCam[k];
        s_headAnchorValid = true;
    }
    if (nonVrik && aiming && (!s_prevAiming || s_prevHeadAim || !s_nonVrikAnchorValid)) {
        for (int k = 0; k < 3; ++k) s_nonVrikEyeOffsetCam[k] = eyeOffsetCam[k];
        s_nonVrikAnchorValid = true;
    }
    if (!headAim) s_headAnchorValid = false;
    if (!aiming) s_nonVrikAnchorValid = false;
    s_prevHeadAim = headAim;
    s_prevAiming = aiming;

    float anchor[3], delta[4];
    if (headAim && s_headAnchorValid) {
        // Head aim: the latched head centre plus the LIVE sighting-eye orbit, and the arms rotate with
        // the head so the sight stays on the eye through a head turn.
        float headOff[3];
        VRIK_QuatRotateVec(centreRot, s_headCentreOffsetCam, headOff);
        for (int k = 0; k < 3; ++k) anchor[k] = camPos[k] + headOff[k] + eyeSideOffset[k];
        for (int k = 0; k < 4; ++k) delta[k] = liveDelta[k];
    } else if (nonVrik && s_nonVrikAnchorValid) {
        // Hand aim: the game still owns the direction, so only the translation moves onto the eye.
        float eyeOff[3];
        VRIK_QuatRotateVec(centreRot, s_nonVrikEyeOffsetCam, eyeOff);
        for (int k = 0; k < 3; ++k) anchor[k] = camPos[k] + eyeOff[k];
        delta[0] = 0.0f; delta[1] = 0.0f; delta[2] = 0.0f; delta[3] = 1.0f;
    } else {
        return;
    }

    const int handSlot[2] = {2, 5}, elbowSlot[2] = {1, 4};
    for (int side = 0; side < 2; ++side) {
        const float relH[3] = { pose->rawPos[handSlot[side]][0] - camPos[0],
                                pose->rawPos[handSlot[side]][1] - camPos[1],
                                pose->rawPos[handSlot[side]][2] - camPos[2] };
        const float relE[3] = { pose->rawPos[elbowSlot[side]][0] - camPos[0],
                                pose->rawPos[elbowSlot[side]][1] - camPos[1],
                                pose->rawPos[elbowSlot[side]][2] - camPos[2] };
        float rotH[3], rotE[3];
        VRIK_QuatRotateVec(delta, relH, rotH);
        VRIK_QuatRotateVec(delta, relE, rotE);
        for (int k = 0; k < 3; ++k) {
            pose->targetHand[side][k] = anchor[k] + rotH[k];
            pose->targetElbow[side][k] = anchor[k] + rotE[k];
        }
    }
    VRIK_QuatMul(delta, pose->rawRot[5], pose->targetSupportRot);
    VRIK_QuatNorm(pose->targetSupportRot);
    pose->valid = true;
}

void SolvePreparedAimArms(uint8_t* boneBuf) {
    AimArmPose* pose = nullptr;
    for (auto& entry : g_aimArmPose) {
        if (entry.boneBuf == boneBuf) { pose = &entry; break; }
    }
    if (!pose || !pose->valid) return;

    // The WEAPON hand keeps whatever rotation the weapon writer just gave it -- head aim's view
    // rotation, or the stabilizer's correction -- so it is read back from the FK rather than
    // re-derived here. The support hand takes the rotated authored grip.
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    const float weaponHandRot[4] = { g_fkRot[pose->bone[2]][0], g_fkRot[pose->bone[2]][1],
                                     g_fkRot[pose->bone[2]][2], g_fkRot[pose->bone[2]][3] };
    SolveAimArm(boneBuf, pose->bone[0], pose->bone[1], pose->bone[2],
                &pose->rawPos[0], &pose->rawRot[0],
                pose->targetHand[0], pose->targetElbow[0], weaponHandRot);
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    SolveAimArm(boneBuf, pose->bone[3], pose->bone[4], pose->bone[5],
                &pose->rawPos[3], &pose->rawRot[3],
                pose->targetHand[1], pose->targetElbow[1], pose->targetSupportRot);
}

}  // namespace cvr::anim
