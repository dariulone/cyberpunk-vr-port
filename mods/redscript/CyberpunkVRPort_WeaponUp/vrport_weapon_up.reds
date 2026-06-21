// CyberpunkVRPort — keep the player's melee weapon out of the game's auto-lower / auto-unequip path.
//
// THE PROBLEM. The game state machine watches for weapon interactions (RangedAttack / native MeleeAttack
// inputs). If none arrive for a while it concludes the player isn't using the weapon and runs two
// chained transitions:
//   1) MeleeSafeDecisions (meleeTransitions.script:1873)  -> enters the "Safe" (lowered) state
//      via the shared DefaultTransition.ShouldEnterSafe check.
//   2) MeleePublicSafeEvents.OnTick (meleeTransitions.script:1863) ->
//      after `timeToUnequipMeleeware` / `timeToAutoUnequipWeapon` seconds (default 15s) calls
//      SendEquipmentSystemWeaponManipulationRequest(UnequipWeapon) and the katana literally vanishes
//      from the player's hands.
//
// Our VR motion-melee bypasses the game's attack input entirely (no RT trigger, no MeleeAttack press —
// damage is dealt by VRMeleeBladeHit + the DamageSystem wrap), so the game has no way to tell that the
// player IS actively using the weapon. This module shuts off both transitions for melee weapons.
//
// Modeled on Rivarez's ControlLowerAndHolsterWeapons (which only covers firearms via an IsFirearms gate
// — the author explicitly states it doesn't work for melee). We add melee coverage on the SAME two
// hook points (just gated on a melee weapon being equipped instead of a firearm).

module CyberpunkVRPort.WeaponUp

// True if the player has a melee weapon equipped in the right-hand slot (covers katana, knives, bats,
// hammers, mantis blades / gorilla arms — anything tagged Melee in the weapon record).
func VRWeaponUpHasMelee(scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let ts = scriptInterface.GetTransactionSystem();
  if !IsDefined(ts) { return false; }
  let weapon = ts.GetItemInSlot(scriptInterface.executionOwner, t"AttachmentSlots.WeaponRight") as WeaponObject;
  if !IsDefined(weapon) { return false; }
  return ts.HasTag(scriptInterface.executionOwner, WeaponObject.GetMeleeWeaponTag(), weapon.GetItemID());
}

// (1) Block the SHARED gateway: DefaultTransition.ShouldEnterSafe is called from MeleeSafeDecisions
// (and from ranged SafeDecisions for firearms). When a melee weapon is equipped, refuse to enter Safe.
@wrapMethod(DefaultTransition)
protected final const func ShouldEnterSafe(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>, const opt maxDistanceSquared: Float) -> Bool {
  if VRWeaponUpHasMelee(scriptInterface) {
    return false;
  }
  return wrappedMethod(stateContext, scriptInterface, maxDistanceSquared);
}

// (2) Belt-and-suspenders: even if (1) is somehow bypassed by another path, kill the 15-second
// auto-unequip timer that runs in MeleePublicSafeEvents.OnTick. The function is replaced with a no-op
// so the SendEquipmentSystemWeaponManipulationRequest(UnequipWeapon) call never fires.
@replaceMethod(MeleePublicSafeEvents)
protected final func OnTick(timeDelta: Float, stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
}
