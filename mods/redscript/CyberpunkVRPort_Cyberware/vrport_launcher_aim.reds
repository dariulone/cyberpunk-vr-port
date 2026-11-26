public native func VRCyberwarePoint(side: Int32, index: Int32) -> Vector4;
public native func VRCyberwareFrame() -> array<Vector4>;
public native func VRLauncherControllerAim() -> Bool;

@wrapMethod(LeftHandCyberwareTransition)
protected final func AimSnap(scriptInterface: ref<StateGameScriptInterface>) -> Void {
  let player = scriptInterface.executionOwner as PlayerPuppet;
  let targeting: ref<TargetingSystem>;
  if IsDefined(player) && player.vrLauncherKeepOpen && VRLauncherControllerAim() {
    // Keep native aiming/perk notifications, without steering the view when
    // this projectile is aimed by the physical barrel.
    targeting = scriptInterface.GetTargetingSystem();
    if IsDefined(targeting) {
      targeting.OnAimStartBegin(player);
      targeting.OnAimStartEnd(player);
    };
    return;
  };
  wrappedMethod(scriptInterface);
}

@wrapMethod(LeftHandCyberwareTransition)
protected final func DrainLeftHandWeaponCharge(scriptInterface: ref<StateGameScriptInterface>, chargeValue: Float) -> Void {
  let player = scriptInterface.executionOwner as PlayerPuppet;
  // Preserve the native weapon-charge reset; spend a charge only when this
  // action could actually queue a shot. Never refund or force a stat pool.
  if IsDefined(player) && player.vrLauncherShotRejected && !player.vrLauncherShotQueued {
    wrappedMethod(scriptInterface, 0.00);
    return;
  };
  wrappedMethod(scriptInterface, chargeValue);
}

@wrapMethod(LeftHandCyberwareTransition)
public final func DetachProjectile(scriptInterface: ref<StateGameScriptInterface>, opt angleOffset: Float) -> Void {
  let player = scriptInterface.executionOwner as PlayerPuppet;
  let weapon = this.GetLeftHandWeaponObject(scriptInterface);
  let muzzle: Vector4;
  let end: Vector4;
  let direction: Vector4;
  let rotation: Quaternion;
  let right: Vector4;
  let position: WorldPosition;
  let installedProjectile: ItemID;
  let launch: ref<gameprojectileSpawnerLaunchEvent>;
  if !IsDefined(player) || !player.vrLauncherKeepOpen || !IsDefined(weapon) || !WeaponObject.IsOfType(weapon.GetItemID(), gamedataItemType.Cyb_Launcher) || !VRLauncherControllerAim() {
    wrappedMethod(scriptInterface, angleOffset);
    return;
  };
  muzzle = VRCyberwarePoint(0, 0);
  end = VRCyberwarePoint(0, 1);
  if muzzle.W == 0.00 || end.W == 0.00 || Vector4.Distance(muzzle, end) < 0.01 {
    player.vrLauncherShotRejected = true;
    return;
  };
  direction = Vector4.Normalize(end - muzzle);
  rotation = Quaternion.BuildFromDirectionVector(direction);
  if angleOffset != 0.00 {
    right = Quaternion.GetRight(rotation);
    direction = direction * CosF(angleOffset) + right * SinF(angleOffset);
    rotation = Quaternion.BuildFromDirectionVector(direction);
  };
  this.GetCurrentlyInstalledProjectile(scriptInterface, installedProjectile);
  WorldPosition.SetVector4(position, muzzle);
  launch = new gameprojectileSpawnerLaunchEvent();
  launch.owner = player;
  launch.templateName = TweakDBInterface.GetCName(ItemID.GetTDBID(installedProjectile) + t".projectileTemplateName", n"None");
  if Equals(launch.templateName, n"None") { player.vrLauncherShotRejected = true; return; };
  launch.appearance = n"None";
  launch.launchParams.logicalPositionProvider = IPositionProvider.CreateStaticPositionProvider(position);
  launch.launchParams.logicalOrientationProvider = IOrientationProvider.CreateStaticOrientationProvider(rotation);
  launch.projectileParams.ignoreMountedVehicleCollision = true;
  weapon.QueueEvent(launch);
  player.vrLauncherShotQueued = true;
  player.vrLauncherShotRejected = false;
  GameInstance.GetTelemetrySystem(player.GetGame()).LogActiveCyberwareUsed(player, weapon.GetItemID());
}
