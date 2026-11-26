// The game can miss the release callback while its physical XR trigger slot
// already reads zero. Remember which native press belongs to the VR finger.
@addField(WeaponObject) private let vrAutoFingerSlot: Int32;
@addField(WeaponObject) private let vrAutoPressCount: Uint32;
@addField(WeaponObject) private let vrAutoReleased: Bool;

@addMethod(WeaponObject)
public final func VRAutoReleaseBlocks(slot: Int32, count: Uint32, travel: Float, fresh: Bool) -> Bool {
  if this.vrAutoFingerSlot != slot || this.vrAutoPressCount != count {
    this.vrAutoFingerSlot = 0;
    this.vrAutoReleased = false;
  };
  // A delayed first FullAuto transition can outlast the fresh-press token.
  // Full travel still identifies the source of that native press.
  if fresh || (this.vrAutoFingerSlot == 0 && travel >= 0.85) {
    this.vrAutoFingerSlot = slot;
    this.vrAutoPressCount = count;
    this.vrAutoReleased = false;
  };
  if this.vrAutoFingerSlot == slot && travel <= 0.20 {
    this.vrAutoReleased = true;
  };
  return this.vrAutoReleased;
}

@addMethod(WeaponTransition)
protected final const func VRReleasedFullAuto(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let forced: StateResultBool;
  let slot: Int32;
  let weapon: ref<WeaponObject>;
  if !scriptInterface.IsTriggerModeActive(gamedataTriggerMode.FullAuto) || !VRWeaponTriggerEnabled()
    || stateContext.GetBoolParameter(this.GetIsChargedFullAutoName(), true)
    || !IsDefined(scriptInterface.executionOwner as PlayerPuppet) || VRDualWieldTriggers.Active()
    || this.IsInVisionModeActiveState(stateContext, scriptInterface) || this.IsInValentinosFrenzy(scriptInterface)
    || scriptInterface.localBlackboard.GetBool(GetAllBlackboardDefs().PlayerStateMachine.MountedToVehicle)
    || scriptInterface.localBlackboard.GetBool(GetAllBlackboardDefs().PlayerStateMachine.QuestForceShoot)
    || StatusEffectSystem.ObjectHasStatusEffect(scriptInterface.executionOwner, t"BaseStatusEffect.ValentinosFrenzySE") {
    return false;
  };
  forced = stateContext.GetTemporaryBoolParameter(this.GetQuestForceShootName());
  weapon = this.GetWeaponObject(scriptInterface);
  if forced.value || !IsDefined(weapon) { return false; };
  // kLeftTriggerAnalog / kRightTriggerAnalog in SharedSlots.hpp are physical.
  slot = VRLeftHanded() ? 154 : 160;
  return weapon.VRAutoReleaseBlocks(slot, scriptInterface.GetActionPressCount(n"RangedAttack"),
    GetVRSharedSlot(slot), VRCurrentFingerAction() != 0ul);
}

@wrapMethod(WeaponTransition)
protected final const func IsFullAutoAction(weaponObject: ref<WeaponObject>, const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return wrappedMethod(weaponObject, stateContext, scriptInterface) && !this.VRReleasedFullAuto(stateContext, scriptInterface);
}

@wrapMethod(FullAutoDecisions)
protected final const func ToShoot(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return wrappedMethod(stateContext, scriptInterface) && !this.VRReleasedFullAuto(stateContext, scriptInterface);
}

@wrapMethod(FullAutoDecisions)
protected final const func ToReady(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return wrappedMethod(stateContext, scriptInterface) || this.VRReleasedFullAuto(stateContext, scriptInterface);
}
