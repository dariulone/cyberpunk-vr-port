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

// The launcher-ready state (its fields, VrLauncherCanStay, the ToVrLauncherReady decisions and the two state classes the
// state machine makes by name): CyberpunkVRPort_Engine/vrport_cyberware_launcher_state.reds.
