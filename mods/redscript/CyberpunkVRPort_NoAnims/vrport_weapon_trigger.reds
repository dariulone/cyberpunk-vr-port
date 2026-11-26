public native func VRWeaponTriggerEnabled() -> Bool;
public native func VRCurrentFingerAction() -> Uint64;

// Existing CET profiles publish this field on draw.
@addField(WeaponObject) public let vrTapCycleScale: Float;
@addField(WeaponObject) private let vrLastFingerAction: Uint64;
@addField(WeaponObject) public let vrAwaitFingerPressCount: Bool;
@addField(WeaponObject) private let vrHasRejectedPress: Bool;
@addField(WeaponObject) private let vrRejectedPressCount: Uint32;
@addField(WeaponObject) private let vrRejectFollowingEcho: Bool;

// The two player weapon state machines can enter Shoot in the same frame.
// Keep time on the player so changing items or modes cannot reset a hand.
@addField(PlayerPuppet) public let vrNextLeftShotTime: Double;
@addField(PlayerPuppet) public let vrNextRightShotTime: Double;

@addMethod(WeaponTransition)
protected final const func VRCadencePlayer(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> ref<PlayerPuppet> {
  let forced: StateResultBool;
  if !VRWeaponTriggerEnabled() { return null; };
  forced = stateContext.GetTemporaryBoolParameter(this.GetQuestForceShootName());
  if forced.value || scriptInterface.localBlackboard.GetBool(GetAllBlackboardDefs().PlayerStateMachine.QuestForceShoot) {
    return null;
  };
  return scriptInterface.executionOwner as PlayerPuppet;
}

@addMethod(WeaponTransition)
protected final const func VRShotIntervalReady(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let player = this.VRCadencePlayer(stateContext, scriptInterface);
  if !IsDefined(player) { return true; };
  let deadline = this.IsLeftWeaponLogic() ? player.vrNextLeftShotTime : player.vrNextRightShotTime;
  return EngineTime.ToDouble(GameInstance.GetEngineTime(scriptInterface.GetGame())) >= deadline;
}

// The merged attack action can stay high when just one of two fingers lifts.
// Read the current hand at admission, including after a long native cycle.
@addMethod(WeaponTransition)
protected final const func VRDualHandReleased(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if !VRDualWieldTriggers.Active() || !IsDefined(this.VRCadencePlayer(stateContext, scriptInterface))
    || this.IsInValentinosFrenzy(scriptInterface)
    || StatusEffectSystem.ObjectHasStatusEffect(scriptInterface.executionOwner, t"BaseStatusEffect.ValentinosFrenzySE") {
    return false;
  };
  return this.IsLeftWeaponLogic() ? !VRDualWieldTriggers.Left() : !VRDualWieldTriggers.Right();
}

@addMethod(WeaponObject)
public final func VRHasFreshFingerAction() -> Bool {
  let action = VRCurrentFingerAction();
  return action != 0ul && action != this.vrLastFingerAction;
}

@addMethod(WeaponObject)
public final func VRConsumeFingerAction(opt awaitingNativePress: Bool) -> Void {
  this.vrLastFingerAction = VRCurrentFingerAction();
  this.vrAwaitFingerPressCount = this.vrLastFingerAction != 0ul && awaitingNativePress;
  this.vrHasRejectedPress = false;
}

@addMethod(WeaponObject)
public final func VRDiscardEarlyPress(const scriptInterface: ref<StateGameScriptInterface>, lastShotPress: StateResultInt) -> Void {
  let held = scriptInterface.GetActionValue(n"RangedAttack") > 0.00;
  let count = scriptInterface.GetActionPressCount(n"RangedAttack");
  let action = VRCurrentFingerAction();
  let fresh = action != 0ul && action != this.vrLastFingerAction;
  let known = this.VRRejectedPress(count, false);
  if !fresh && (!held || known) { return; };
  // The merged action may still show the previous pull as held. Remember
  // the following count too if this XR pull has not reached the game yet.
  this.vrRejectFollowingEcho = fresh && (!held || known || (lastShotPress.valid && lastShotPress.value == Cast<Int32>(count)));
  if fresh { this.vrLastFingerAction = action; };
  this.vrHasRejectedPress = true;
  this.vrRejectedPressCount = count;
}

@addMethod(WeaponObject)
public final func VRRejectedPress(count: Uint32, keyboard: Bool) -> Bool {
  return this.vrHasRejectedPress && (this.vrRejectedPressCount == count
    || (!keyboard && this.vrRejectFollowingEcho && this.vrRejectedPressCount + 1u == count));
}

@addMethod(WeaponTransition)
protected final const func VRFingerSemiAuto(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let questForce: StateResultBool;
  if !VRWeaponTriggerEnabled() || !IsDefined(scriptInterface.executionOwner as PlayerPuppet)
    || !scriptInterface.IsTriggerModeActive(gamedataTriggerMode.SemiAuto) {
    return false;
  };
  questForce = stateContext.GetTemporaryBoolParameter(this.GetQuestForceShootName());
  return !questForce.value && !scriptInterface.localBlackboard.GetBool(GetAllBlackboardDefs().PlayerStateMachine.QuestForceShoot)
    && !scriptInterface.localBlackboard.GetBool(GetAllBlackboardDefs().PlayerStateMachine.MountedToVehicle)
    && !this.IsInVisionModeActiveState(stateContext, scriptInterface) && !this.IsInValentinosFrenzy(scriptInterface)
    && !StatusEffectSystem.ObjectHasStatusEffect(scriptInterface.executionOwner, t"BaseStatusEffect.ValentinosFrenzySE");
}

@wrapMethod(WeaponTransition)
protected final const func CanPerformNextShotInSequence(const weaponObject: ref<WeaponObject>, const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  // CycleRound exit and semi-auto entry both ask this question. The finger
  // route replaces the mouse/pad CycleTime with its accepted-shot interval.
  let fingerSemi = this.VRFingerSemiAuto(stateContext, scriptInterface);
  let ready = this.VRShotIntervalReady(stateContext, scriptInterface);
  if !ready && fingerSemi && this.ShouldShoot(scriptInterface) {
    weaponObject.VRDiscardEarlyPress(scriptInterface, stateContext.GetPermanentIntParameter(this.GetLastChargePressCount()));
  };
  return ready && (fingerSemi || wrappedMethod(weaponObject, stateContext, scriptInterface));
}

@wrapMethod(WeaponTransition)
protected final const func IsSemiAutoAction(weaponObject: ref<WeaponObject>, const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let lastPress: StateResultInt;
  let count: Int32;
  if this.VRDualHandReleased(stateContext, scriptInterface) { return false; };
  if !this.VRFingerSemiAuto(stateContext, scriptInterface) {
    return wrappedMethod(weaponObject, stateContext, scriptInterface);
  };
  if VRCurrentFingerAction() != 0ul {
    return this.ShouldShoot(scriptInterface) && weaponObject.VRHasFreshFingerAction();
  };
  if !this.ShouldShoot(scriptInterface) || scriptInterface.GetActionValue(n"RangedAttack") <= 0.00 { return false; };
  lastPress = stateContext.GetPermanentIntParameter(this.GetLastChargePressCount());
  count = Cast<Int32>(scriptInterface.GetActionPressCount(n"RangedAttack"));
  if weaponObject.VRRejectedPress(Cast<Uint32>(count), scriptInterface.executionOwner.PlayerLastUsedKBM()) { return false; };
  if lastPress.valid && lastPress.value == count { return false; };
  // ShootEvents sampled the previous engine count if XR won the race. The
  // following gamepad count acknowledges that same round. A new physical
  // pull has its own identity and was handled above, even if the echo is lost.
  if weaponObject.vrAwaitFingerPressCount && lastPress.valid && lastPress.value + 1 == count
    && !scriptInterface.executionOwner.PlayerLastUsedKBM()
    && !scriptInterface.localBlackboard.GetBool(GetAllBlackboardDefs().PlayerStateMachine.IsDualWielding) {
    return false;
  };
  // ShootEvents consumes the per-hand press count. Allow a still-held edge
  // across a state transition, but never retain a released or old request.
  return scriptInterface.IsActionJustPressed(n"RangedAttack") || scriptInterface.GetActionStateTime(n"RangedAttack") <= 0.10;
}

@wrapMethod(WeaponTransition)
protected final const func VRReleasedFullAuto(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return wrappedMethod(stateContext, scriptInterface) || (scriptInterface.IsTriggerModeActive(gamedataTriggerMode.FullAuto)
    && !stateContext.GetBoolParameter(this.GetIsChargedFullAutoName(), true)
    && this.VRDualHandReleased(stateContext, scriptInterface));
}

// The native input listener can remain silent for the first fresh press after
// closing the pause menu. Keep the player's decision eligible to observe the
// routed XR press, even while the listener reports zero. The PSM still owns
// entry, ammo, mechanical transitions and shooting.
@addField(SemiAutoDecisions) private let vrPlayerListener: Bool;

@wrapMethod(SemiAutoDecisions)
protected final func OnAttach(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Void {
  this.vrPlayerListener = IsDefined(scriptInterface.executionOwner as PlayerPuppet);
  wrappedMethod(stateContext, scriptInterface);
  if this.vrPlayerListener {
    this.GetWeaponObject(scriptInterface).VRConsumeFingerAction();
    this.EnableOnEnterCondition(true);
  };
}

@wrapMethod(SemiAutoDecisions)
private final func RefreshEnterCondition() -> Void {
  wrappedMethod();
  if this.vrPlayerListener { this.EnableOnEnterCondition(true); };
}

@wrapMethod(SemiAutoDecisions)
protected final func OnQuestForcedShoot(value: Bool) -> Void {
  wrappedMethod(value);
  if this.vrPlayerListener { this.EnableOnEnterCondition(true); };
}

@wrapMethod(SemiAutoDecisions)
protected final const func EnterCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let weapon = this.GetWeaponObject(scriptInterface);
  if this.vrPlayerListener && this.m_inputValue <= 0.00 && VRCurrentFingerAction() != 0ul
    && this.VRFingerSemiAuto(stateContext, scriptInterface) {
    return this.ShouldShoot(scriptInterface) && weapon.VRHasFreshFingerAction()
      && this.CanPerformNextSemiAutoShot(weapon, stateContext, scriptInterface);
  };
  return wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(ShootEvents)
protected final func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  let deadline: Double;
  let player = this.VRCadencePlayer(stateContext, scriptInterface);
  if IsDefined(player) {
    deadline = EngineTime.ToDouble(GameInstance.GetEngineTime(scriptInterface.GetGame())) + 0.10d;
    if this.IsLeftWeaponLogic() { player.vrNextLeftShotTime = deadline; }
    else { player.vrNextRightShotTime = deadline; };
  };
  if this.VRFingerSemiAuto(stateContext, scriptInterface) {
    this.GetWeaponObject(scriptInterface).VRConsumeFingerAction(scriptInterface.GetActionValue(n"RangedAttack") <= 0.00);
  };
  wrappedMethod(stateContext, scriptInterface);
}

// The first round of a burst uses SemiAuto. Each committed follow-up has
// its own native burst timer and goes directly from Burst to Shoot.
@wrapMethod(BurstDecisions)
protected final const func ToShoot(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return wrappedMethod(stateContext, scriptInterface) && this.VRShotIntervalReady(stateContext, scriptInterface);
}

// Gate entry to charge, including a change to the secondary trigger mode.
// Once admitted, release, perfect charge and fire-when-ready stay native.
@addMethod(WeaponTransition)
protected final const func VRCanBeginCharge(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let weapon = this.GetWeaponObject(scriptInterface);
  if !IsDefined(this.VRCadencePlayer(stateContext, scriptInterface)) { return true; };
  if this.VRDualHandReleased(stateContext, scriptInterface) { return false; };
  if !this.VRShotIntervalReady(stateContext, scriptInterface) {
    weapon.VRDiscardEarlyPress(scriptInterface, stateContext.GetPermanentIntParameter(this.GetLastChargePressCount()));
    return false;
  };
  if VRCurrentFingerAction() != 0ul { return weapon.VRHasFreshFingerAction(); };
  return scriptInterface.GetActionValue(n"RangedAttack") > 0.00
    && !weapon.VRRejectedPress(scriptInterface.GetActionPressCount(n"RangedAttack"), scriptInterface.executionOwner.PlayerLastUsedKBM());
}

@wrapMethod(ChargeDecisions)
protected final const func EnterCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return wrappedMethod(stateContext, scriptInterface) && this.VRCanBeginCharge(stateContext, scriptInterface);
}

@wrapMethod(CycleTriggerModeDecisions)
protected final const func ToCharge(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return wrappedMethod(stateContext, scriptInterface) && this.VRCanBeginCharge(stateContext, scriptInterface);
}

@wrapMethod(WeaponTransition)
protected final func StartShootingSequence(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>, fireDelay: Float, burstCycleTime: Float, numShotsBurst: Int32, isFullChargeFullAuto: Bool) -> Void {
  wrappedMethod(stateContext, scriptInterface, this.VRFingerSemiAuto(stateContext, scriptInterface) ? 0.00 : fireDelay,
    burstCycleTime, numShotsBurst, isFullChargeFullAuto);
}
