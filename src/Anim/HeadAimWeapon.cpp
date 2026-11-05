// HeadAimWeapon -- the weapon follows the head, and the shot still leaves the muzzle.
//
// Ported from dabinn's TofuExpress (d002d314 "feat(head-aim): add enhanced head aim with
// muzzle-based firing", plus the vehicle allowance from 73bdf668).
//
// The port has always had two aiming models, and the weapon-aim toggle chooses between them:
//
//   HAND AIM (toggle on, the default) -- the controller points the weapon, VRIK drives the arms,
//   and the projectile leaves the muzzle. This is unchanged.
//
//   HEAD AIM (toggle off) -- what the toggle used to mean was only "send the bullet at the camera
//   crosshair instead", which left the weapon itself wherever the animation put it: the player aimed
//   with a reticle that had no relationship to the thing in their hands. Now the WEAPON follows the
//   head: the final HMD orientation becomes WeaponRight's absolute model rotation, so what is held
//   points where the player looks, and the projectile leaves the same live muzzle as in hand aim.
//   One firing path for both modes -- which is also why the launch override no longer keys on the
//   toggle at all, only on "a weapon is out and its muzzle is published".
//
// THREE THINGS ARE DELIBERATELY LEFT TO THE GAME. Position: the vanilla pose owns where the weapon
// sits, including the ADS raise and every authored animation, so only the rotation is replaced.
// The arms: VRIK is suspended while head aim owns the weapon, because a controller-driven arm solve
// and a head-driven weapon are two different answers to the same question and the visible result is
// the arms fighting the gun. And the ADS animations, which keep playing normally.
//
// The view quaternion is already in game world axes (+Y forward), so reaching WeaponRight's model
// frame is the ordinary world-to-entity conversion and nothing more. It is taken from the LATCHED
// view packet, the same instant the rest of the solve uses.

#include "Anim/HeadAimWeapon.hpp"

#include "Anim/AdsEyeAlign.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/LeftHanded.hpp"
#include "Anim/VrikHook.hpp"
#include "Anim/VrikState.hpp"
#include "Utils/SharedSlots.hpp"

#include <cstdint>

extern float* g_pSharedHands;

namespace cvr::anim {

bool IsHeadAimWeaponActive() {
    // These are single-float state flags well outside the [0..126] coherent pose snapshot, so they
    // are read directly rather than through the latch.
    //
    // NOT gated on the vehicle flag: head aim is if anything more useful while driving, where the
    // hands are on the wheel (dabinn dropped that condition himself in 73bdf668).
    return g_pSharedHands &&
           g_pSharedHands[58] <= 0.5f &&                       // weapon-aim toggle OFF = head aim
           g_pSharedHands[vrshared::kWeaponFlag] > 0.5f &&     // a weapon is out
           g_pSharedHands[27] > 0.5f;                          // and its muzzle is published
}

void ApplyHeadAimWeaponOrientation(uint8_t* boneBuf) {
    if (!IsHeadAimWeaponActive()) return;

    // The weapon in hand's bone: WeaponRight, or WeaponLeft with the weapon in the left hand.
    const int weaponIdx = HeldWeaponBone();
    if (weaponIdx < 0 || weaponIdx >= VRIK_FKCount() || !g_viewPktValid) return;

    float viewQ[4] = { g_viewPkt[0], g_viewPkt[1], g_viewPkt[2], g_viewPkt[3] };
    VRIK_QuatNorm(viewQ);
    float entQ[4] = { g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR };
    VRIK_QuatNorm(entQ);
    float invEnt[4];
    VRIK_QuatConj(entQ, invEnt);
    float viewModel[4];
    VRIK_QuatMul(invEnt, viewQ, viewModel);
    VRIK_QuatNorm(viewModel);

    // THROUGH THE HAND, not onto the weapon bone (dabinn, TofuExpress 73bdf668). Writing the
    // weapon bone directly replaced the authored GRIP with the aim rotation, which is how a pistol
    // ends up held sideways; rotating its parent moves the weapon by the same amount and leaves the
    // grip exactly as animated.
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    WriteWeaponModelRotViaHand(boneBuf, weaponIdx, viewModel);
}

}  // namespace cvr::anim
