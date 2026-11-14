public native func VRWeaponTriggerEnabled() -> Bool;

// Public: read and written from WeaponTransition and the trigger wrap, which redscript 1.0 holds to field access.
@addField(WeaponObject) public let vrTriggerHasShot: Bool;
@addField(WeaponObject) public let vrTriggerLastShotReal: Float;

@addMethod(WeaponTransition)
protected final const func VRDirectSemiAuto(scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  // Native automatic cadence, burst spacing, charge, reload, ammo and weapon
  // safety remain game-owned. Single-shot repeat delay uses a uniform cap.
  return VRWeaponTriggerEnabled() && IsDefined(scriptInterface.executionOwner as PlayerPuppet) &&
    scriptInterface.IsTriggerModeActive(gamedataTriggerMode.SemiAuto);
}

@wrapMethod(WeaponTransition)
protected final const func CanPerformNextShotInSequence(const weaponObject: ref<WeaponObject>, const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if this.VRDirectSemiAuto(scriptInterface) {
    // Five deliberate pulls per real second. Use the physical-input clock so
    // slow motion does not turn a 200ms reset into a multi-second wait.
    let pullReset: Bool = !weaponObject.vrTriggerHasShot || VRMeleeRealTime() - weaponObject.vrTriggerLastShotReal >= 0.20;
    // SNIPER RIFLES KEEP THEIR OWN CYCLE (2026-10-16). The uniform 200ms
    // reset let a bolt gun fire five rounds a second; on top of it the weapon's
    // own CycleTime gate applies again, exactly as in the flat game.
    if Equals(weaponObject.GetWeaponRecord().ItemType().Type(), gamedataItemType.Wea_SniperRifle) {
      return pullReset && wrappedMethod(weaponObject, stateContext, scriptInterface);
    };
    return pullReset;
  };
  return wrappedMethod(weaponObject, stateContext, scriptInterface);
}

@wrapMethod(ShootEvents)
protected final func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  let weapon = this.GetWeaponObject(scriptInterface);
  if this.VRDirectSemiAuto(scriptInterface) && IsDefined(weapon) {
    // Start the interval on the native shot, never on a blocked trigger pull.
    weapon.vrTriggerLastShotReal = VRMeleeRealTime();
    weapon.vrTriggerHasShot = true;
  };
  wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(WeaponTransition)
protected final const func IsSemiAutoAction(weaponObject: ref<WeaponObject>, const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let lastPress: StateResultInt;
  if this.VRDirectSemiAuto(scriptInterface) {
    lastPress = stateContext.GetPermanentIntParameter(this.GetLastChargePressCount());
    // ShootEvents records this native count on every actual shot (2.5: per hand, Right/LeftLastChargePressCount). The count
    // survives a one-frame Shoot/CycleRound transition, unlike JustPressed,
    // while holding at .84 after the break cannot create another shot.
    return scriptInterface.GetActionValue(n"RangedAttack") > 0.00 &&
      (!lastPress.valid || lastPress.value != Cast<Int32>(scriptInterface.GetActionPressCount(n"RangedAttack")));
  };
  return wrappedMethod(weaponObject, stateContext, scriptInterface);
}

@wrapMethod(WeaponTransition)
protected final func StartShootingSequence(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>, fireDelay: Float, burstCycleTime: Float, numShotsBurst: Int32, isFullChargeFullAuto: Bool) -> Void {
  wrappedMethod(stateContext, scriptInterface, this.VRDirectSemiAuto(scriptInterface) ? 0.00 : fireDelay, burstCycleTime, numShotsBurst, isFullChargeFullAuto);
}
