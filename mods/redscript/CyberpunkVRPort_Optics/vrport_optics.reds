// The sight's display is independent of the flat-screen ADS camera state.
// Weapons, hands, accuracy, charge and input stay under their original owners.
public native func VRPhysicalOpticsEnabled() -> Bool;

public class VRPhysicalScopeController extends inkGameController {
  protected cb func OnInitialize() -> Bool {
    let root: ref<inkWidget> = this.GetRootWidget();
    root.SetVisible(true);
    root.SetOpacity(1.00);
  }
}

@wrapMethod(AimDownSightController)
public final func OnPlayerAttach(playerPuppet: ref<GameObject>) -> Void {
  wrappedMethod(playerPuppet);
  if VRPhysicalOpticsEnabled() {
    this.OnAim(true);
  };
}

@wrapMethod(AimDownSightController)
public final func OnAim(isAiming: Bool) -> Void {
  wrappedMethod(isAiming || VRPhysicalOpticsEnabled());
}

@wrapMethod(IronsightGameController)
protected cb func OnPlayerAttach(playerPuppet: ref<GameObject>) -> Bool {
  wrappedMethod(playerPuppet);
  if VRPhysicalOpticsEnabled() {
    this.EnableUpdate(true);
  };
}

@wrapMethod(IronsightGameController)
protected cb func OnUpperBodyChanged(state: Int32) -> Bool {
  wrappedMethod(state);
  if VRPhysicalOpticsEnabled() {
    this.EnableUpdate(true);
  };
}

// A sight's RELOADING loop ends when the magazine is full again, not only when the crosshair leaves its Reload
// state. The loop starts the moment the count reads 0 and the game stops it on that state change alone, and a
// reload done by hand (the port's physical one) never enters it: the Pozhar's sight said "reloading" over a
// full magazine. Ended the game's own way -- the loop runs out its current cycle (OnReloadEndLoop).
@wrapMethod(IronsightGameController)
protected final func OnAmmoCountChanged(value: Int32) -> Void {
  wrappedMethod(value);
  if value > 0 && IsDefined(this.m_animReload) && !Equals(this.m_crosshairState, gamePSMCrosshairStates.Reload) {
    this.m_animReload.RegisterToCallback(inkanimEventType.OnEndLoop, this, n"OnReloadEndLoop");
    this.m_animReload = null;
  };
}

// A smart sniper rifle (Ashura) aims through its smart HUD, not through an
// optic: it keeps the game's own ADS zoom and HUD on the aim trigger.
public func VRIsSmartSniper(weapon: ref<WeaponObject>) -> Bool {
  let record: ref<WeaponItem_Record>;
  if !IsDefined(weapon) {
    return false;
  };
  record = weapon.GetWeaponRecord();
  if !IsDefined(record) || !IsDefined(record.Evolution()) {
    return false;
  };
  return Equals(record.Evolution().Type(), gamedataWeaponEvolution.Smart) &&
    Equals(WeaponObject.GetWeaponType(weapon.GetItemID()), gamedataItemType.Wea_SniperRifle);
}

// Keep weapon ADS gameplay but leave the world at its normal angular scale.
// Scanner, device cameras, unarmed zoom and scripted temporary unequip retain
// their native zoom feature. Optical magnification belongs to the lens view.
// The weapon is the right hand's, or the left's when it holds the only one (the left-handed hold): read off the
// right slot alone, a left-handed player's aim trigger got the game's zoom.
@wrapMethod(ZoomTransition)
protected final func SendZoomAnimFeatureData(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  let weapon: ref<WeaponObject> = ScriptedPuppet.GetActiveWeapon(scriptInterface.executionOwner);
  let feature: ref<AnimFeature_Zoom>;
  let worldFOV: Float;
  if !VRPhysicalOpticsEnabled() || !IsDefined(scriptInterface.executionOwner as PlayerPuppet) ||
      !IsDefined(weapon) || !weapon.IsRanged() || VRIsSmartSniper(weapon) ||
      this.IsInVisionModeActiveState(stateContext, scriptInterface) ||
      stateContext.IsStateActive(n"UpperBody", n"temporaryUnequip") ||
      this.IsControllingDevice(stateContext, scriptInterface) {
    wrappedMethod(stateContext, scriptInterface);
    return;
  };
  worldFOV = GameInstance.GetCameraSystem(scriptInterface.owner.GetGame()).GetActiveCameraFOV();
  feature = new AnimFeature_Zoom();
  feature.finalZoomLevel = 1.00;
  feature.weaponZoomLevel = 1.00;
  feature.weaponScopeFov = worldFOV;
  feature.weaponAimFOV = worldFOV;
  feature.worldFOV = worldFOV;
  feature.zoomLevelNum = this.GetZoomLevelNumber(stateContext);
  feature.noWeaponAimInTime = this.GetStaticFloatParameterDefault("noWeaponAimInTime", 0.20);
  feature.noWeaponAimOutTime = this.GetStaticFloatParameterDefault("noWeaponAimOutTime", 0.20);
  feature.shouldUseWeaponZoomStats = this.GetShouldUseWeaponZoomData(stateContext);
  feature.focusModeActive = false;
  scriptInterface.SetAnimationParameterFeature(n"ZoomAnimData", feature);
}
