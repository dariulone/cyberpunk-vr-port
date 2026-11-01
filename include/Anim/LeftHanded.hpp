#pragma once
// THE WEAPON IN THE LEFT HAND (xr_left_handed; the buttons are include/Hooks/Handedness.hpp). Drawn on its own, the
// weapon is the game's left hand's (mods/redscript/CyberpunkVRPort_LeftHanded): it hangs on WeaponLeft and the
// animation holds it there. What is left for the port is its own hands-on work. See src/Anim/LeftHanded.cpp.
#include <cstdint>

namespace cvr::anim {

// The only weapon in hand is in the left one: a weapon on AttachmentSlots.WeaponLeft, none on WeaponRight, not
// handed over by the carry and not an implanted arm. Whatever the controls are set to: it is where the weapon is.
bool WeaponInLeftHand();

// The weapon bone of the hand holding the weapon in hand: WeaponLeft while WeaponInLeftHand(), WeaponRight otherwise.
// -1 until the rig is armed.
int HeldWeaponBone();

// The rig was armed: each right bone's left twin and the reflection between their frames, from its reference pose.
// Called with the player pose lock held (src/Anim/CharacterRig.cpp).
void LeftHandedArmRig(const char* const* names, int count);

// Diagnostics: each hand's mean finger curl off the reference pose (CyberpunkVR_DebugFingerFlex), as the animation
// left it (drawn = false) or after every finger layer.
void NoteFingerFlex(const uint8_t* boneBuf, bool drawn);

// A hold captured with the weapon in the right hand -- the left wrist in the right wrist's frame -- as the same hold
// is with the weapon in the left hand: the right wrist in the left wrist's frame. In place; unchanged while the rig
// has no twins.
void MirrorHeldHand(float* pos, float* rot);

// A left finger's local rotation, mirrored onto its right twin. False when the bone has none.
bool MirrorFingerToRight(int leftBone, const float* rot, int* rightBone, float* out);

// WeaponLeft's local transform (offset and rotation in its parent's frame), mirrored onto WeaponRight: what a prop
// held in the left hand is in the right one. False while the rig has no weapon twin.
bool MirrorWeaponSlotToRight(const float* pos, const float* rot, float* outPos, float* outRot);

}  // namespace cvr::anim
