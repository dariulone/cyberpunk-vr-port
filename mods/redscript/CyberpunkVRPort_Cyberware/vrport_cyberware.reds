@addField(PlayerPuppet)
public let vrLauncherConsumeSwitchRelease: Bool;

// A selected projectile launcher remains ready after its native shot/cancel
// completes. The native action states still own projectiles, charges and costs.
// No synthetic held input and no repeated equip requests are used.

@addField(PlayerPuppet)
public let vrLauncherKeepOpen: Bool;
@addField(PlayerPuppet)
public let vrLauncherSelectedAsWeapon: Bool;
@addField(PlayerPuppet)
public let vrLauncherFromWeaponCycle: Bool;
@addField(EquipmentSystemPlayerData)
private let vrCyberwareHandlingRequest: Bool;

@wrapMethod(LeftHandCyberwareEquipEvents)
public func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  (scriptInterface.executionOwner as PlayerPuppet).vrLauncherShotRejected = false;
  (scriptInterface.executionOwner as PlayerPuppet).vrLauncherShotQueued = false;
  (scriptInterface.executionOwner as PlayerPuppet).vrLauncherKeepOpen = LeftHandCyberwareHelper.IsProjectileLauncherEquipped(scriptInterface);
}

@wrapMethod(LeftHandCyberwareEquipEvents)
public func OnExit(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  (scriptInterface.executionOwner as PlayerPuppet).vrLauncherKeepOpen = LeftHandCyberwareHelper.IsProjectileLauncherEquipped(scriptInterface);
}

@wrapMethod(LeftHandCyberwareEventsTransition)
protected func CleanUpLeftHandCyberwareState(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  (scriptInterface.executionOwner as PlayerPuppet).vrLauncherKeepOpen = false;
  (scriptInterface.executionOwner as PlayerPuppet).vrLauncherSelectedAsWeapon = false;
  (scriptInterface.executionOwner as PlayerPuppet).vrLauncherFromWeaponCycle = false;
  wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(EquipmentSystemPlayerData)
public final func OnEquipmentSystemWeaponManipulationRequest(request: ref<EquipmentSystemWeaponManipulationRequest>) -> Void {
  let player = this.m_owner as PlayerPuppet;
  let wasHandling = this.vrCyberwareHandlingRequest;
  if IsDefined(player) && Equals(request.requestType, EquipmentManipulationAction.UnequipAll) {
    player.vrLauncherKeepOpen = false;
    player.vrLauncherSelectedAsWeapon = false;
  };
  // GetItemID/CycleWeapon is stateful. Let the original request resolve it once.
  this.vrCyberwareHandlingRequest = true;
  wrappedMethod(request);
  this.vrCyberwareHandlingRequest = wasHandling;
}

@addMethod(LeftHandCyberwareTransition)
protected final const func VrLauncherCanStay(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let player = scriptInterface.executionOwner as PlayerPuppet;
  let weapon = this.GetLeftHandWeaponObject(scriptInterface);
  let selected: ItemID;
  if !IsDefined(player) { return false; };
  if !player.vrLauncherKeepOpen || !IsDefined(weapon) { return false; };
  if !WeaponObject.IsOfType(weapon.GetItemID(), gamedataItemType.Cyb_Launcher) { return false; };
  if this.ShouldInstantlyUnequipCyberware(scriptInterface, stateContext) { return false; };
  if stateContext.GetBoolParameter(n"InterruptLeftHandAction") { return false; };
  if this.IsInVisionModeActiveState(stateContext, scriptInterface) { return false; };
  if this.GetCancelChargeButtonInput(scriptInterface) { return false; };
  if scriptInterface.IsActionJustPressed(n"SwitchItem") || scriptInterface.IsActionJustTapped(n"HolsterWeapon") || scriptInterface.IsActionJustReleased(n"UseConsumable") { return false; };
  selected = EquipmentSystem.GetData(player).GetSlotActiveItem(EquipmentManipulationRequestSlot.Left);
  if !ItemID.IsValid(selected) || selected != weapon.GetItemID() { return false; };
  return true;
}

// Preserve each native action's completion time. This earlier transition wins
// over automatic unequip only for the launcher that is still selected.
@addMethod(LeftHandCyberwareQuickActionDecisions)
protected final const func ToVrLauncherReady(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return this.VrLauncherCanStay(stateContext, scriptInterface) && this.ToLeftHandCyberwareUnequip(stateContext, scriptInterface);
}
@addMethod(LeftHandCyberwareChargeActionDecisions)
protected final const func ToVrLauncherReady(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return this.VrLauncherCanStay(stateContext, scriptInterface) && this.ToLeftHandCyberwareWaitForUnequip(stateContext, scriptInterface);
}
@addMethod(LeftHandCyberwareChargeRepeatActionDecisions)
protected final const func ToVrLauncherReady(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return this.VrLauncherCanStay(stateContext, scriptInterface) && this.ToLeftHandCyberwareWaitForUnequip(stateContext, scriptInterface);
}
@addMethod(LeftHandCyberwareWaitForUnequipDecisions)
protected final const func ToVrLauncherReady(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return this.VrLauncherCanStay(stateContext, scriptInterface) && this.ToLeftHandCyberwareUnequip(stateContext, scriptInterface);
}

public class VrLauncherReadyDecisions extends LeftHandCyberwareTransition {
  protected final const func EnterCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
    return this.VrLauncherCanStay(stateContext, scriptInterface);
  }
  protected final const func ToLeftHandCyberwareUnequip(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
    return !this.VrLauncherCanStay(stateContext, scriptInterface);
  }
  protected final const func ToLeftHandCyberwareCharge(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
    return stateContext.GetBoolParameter(n"vrLauncherInputArmed", true) && LeftHandCyberwareHelper.ProjectileLauncherHasCharge(scriptInterface) && this.LeftHandCyberwareHasTag(scriptInterface, n"ChargeAction") && scriptInterface.IsActionHeld(n"UseCombatGadget");
  }
  protected final const func ToLeftHandCyberwareQuickAction(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
    if !stateContext.GetBoolParameter(n"vrLauncherInputArmed", true) || !LeftHandCyberwareHelper.ProjectileLauncherHasCharge(scriptInterface) { return false; };
    if (scriptInterface.executionOwner as PlayerPuppet).IsAimingAtFriendly() { return false; };
    return scriptInterface.IsActionJustPressed(n"RangedAttack") || scriptInterface.IsActionJustReleased(n"UseCombatGadget");
  }
}

public class VrLauncherReadyEvents extends LeftHandCyberwareEventsTransition {
  public func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
    stateContext.SetPermanentBoolParameter(n"vrLauncherInputArmed", false, true);
    (scriptInterface.executionOwner as PlayerPuppet).vrLauncherShotRejected = false;
    (scriptInterface.executionOwner as PlayerPuppet).vrLauncherShotQueued = false;
    this.ResetAnimFeatureParameters(stateContext);
    // Use the native deployed visual without starting its charge pool,
    // projectile preview, aiming camera or firing action.
    this.SetAnimFeatureState(stateContext, 2);
    this.SetIsCharging(stateContext, true);
    this.SetActionDuration(stateContext, 2.00);
    this.SetLeftHandItemTypeAndState(scriptInterface, 2, 2);
    this.SetAnimEquipState(scriptInterface, true);
    this.SetBlackboardIntVariable(scriptInterface, GetAllBlackboardDefs().PlayerStateMachine.LeftHandCyberware, 1);
    scriptInterface.PushAnimationEvent(n"LeftHandCyberwareReady");
    super.OnEnter(stateContext, scriptInterface);
    this.UpdateAndSendChargeAnimData(1.00, false, stateContext, scriptInterface);
  }
  protected func OnUpdate(timeDelta: Float, stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
    // Returning from a shot cannot recycle its release edge into another shot.
    if scriptInterface.GetActionValue(n"UseCombatGadget") == 0.00 && scriptInterface.GetActionValue(n"RangedAttack") == 0.00 {
      stateContext.SetPermanentBoolParameter(n"vrLauncherInputArmed", true, true);
    };
  }
  public func OnExit(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
    if (scriptInterface.executionOwner as PlayerPuppet).vrLauncherKeepOpen && scriptInterface.IsActionJustPressed(n"SwitchItem") {
      (scriptInterface.executionOwner as PlayerPuppet).vrLauncherConsumeSwitchRelease = true;
      this.SendEquipmentSystemWeaponManipulationRequest(scriptInterface, EquipmentManipulationAction.CycleNextWeaponWheelItem, gameEquipAnimationType.Default);
    };
    stateContext.RemovePermanentBoolParameter(n"vrLauncherInputArmed");
    this.SetIsCharging(stateContext, false);
    this.SetAnimFeatureState(stateContext, 1);
    super.OnExit(stateContext, scriptInterface);
  }
}
