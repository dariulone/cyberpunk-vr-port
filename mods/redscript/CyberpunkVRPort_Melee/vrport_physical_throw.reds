// Physical release supplies only the launch. Native PSM, projectile attack,
// collision, effects, recovery pool and pickup still own the weapon lifecycle.
public native func VRMeleeThrowFrame() -> array<Vector4>;
public native func VRMeleeThrowEnabled() -> Bool;

public class VrMeleeThrow extends IScriptable {
  public let weapon: wref<WeaponObject>;
  public let origin: Vector4;
  public let direction: Vector4;
  // The stroke's own heading: assistance also takes a target the head is not looking at.
  public let stroke: Vector4;
  // Where the head looked right before the throw, and from where. Assistance prefers this.
  public let gaze: Vector4;
  public let eye: Vector4;
  public let speed: Float;
  public let gravity: Float;
  public let stamp: Float;
  public let launched: Bool;
  public let claimed: Bool;
  public let assisted: Bool;
  // The enemy part the assist picked: the flight curves on to it, as the flat game's throw does.
  public let target: wref<TargetingComponent>;
}
@addField(PlayerPuppet) public let vrMeleeThrow: ref<VrMeleeThrow>;
@addField(PlayerPuppet) public let vrMeleeThrowShots: Int32;
@addField(PlayerPuppet) public let vrMeleeThrowHits: Int32;
@addField(PlayerPuppet) public let vrMeleeThrowLastOrigin: Vector4;
@addField(PlayerPuppet) public let vrMeleeThrowLastDirection: Vector4;
@addField(WeaponObject) public let vrMeleeThrow: ref<VrMeleeThrow>;
@addField(WeaponObject) public let vrPhysicalThrowHidden: Bool;
@addField(MeleeProjectile) private let vrMeleeThrow: ref<VrMeleeThrow>;

@addMethod(PlayerPuppet)
public func VRMeleeThrowReady(weapon: ref<WeaponObject>) -> Bool {
  let bb: ref<IBlackboard>;
  if !VRMeleeThrowEnabled() || !IsDefined(weapon) || !weapon.WeaponHasTag(n"Throwable") || this.IsDead() || this.VRHolsterIntentActive(1.00) { return false; };
  if GameInstance.GetTransactionSystem(this.GetGame()).GetItemInSlot(this, VRLeftHandedWeapon.HeldSlot(this)) != weapon || !MeleeTransition.CanThrowWeaponObject(this, weapon) { return false; };
  bb = GameInstance.GetBlackboardSystem(this.GetGame()).GetLocalInstanced(this.GetEntityID(), GetAllBlackboardDefs().PlayerStateMachine);
  return IsDefined(bb) && bb.GetInt(GetAllBlackboardDefs().PlayerStateMachine.MeleeWeapon) == 9;
}

@addMethod(PlayerPuppet)
public func VRMeleeThrowPending(weapon: ref<WeaponObject>) -> Bool {
  let request = this.vrMeleeThrow;
  return IsDefined(request) && !request.launched && request.weapon == weapon && VRMeleeRealTime() - request.stamp < 0.30;
}

@addMethod(PlayerPuppet)
public func VRMeleeThrowCancel() -> Void {
  if IsDefined(this.vrMeleeThrow) && !this.vrMeleeThrow.launched { this.vrMeleeThrow = null; };
}

@addMethod(PlayerPuppet)
public func VRMeleeThrowRecovering(weapon: ref<WeaponObject>) -> Bool {
  return IsDefined(weapon) && weapon.vrPhysicalThrowHidden;
}

@addMethod(PlayerPuppet)
private func VRMeleeThrowAssist(request: ref<VrMeleeThrow>) -> Void {
  let query: TargetSearchQuery;
  let parts: array<TS_TargetPartInfo>;
  let component: wref<TargetingComponent>;
  let target: wref<ScriptedPuppet>;
  let point: Vector4;
  let delta: Vector4;
  let direction: Vector4;
  let bestDirection: Vector4;
  let score: Float;
  let look: Float;
  let best: Float = 0.965926; // fifteen-degree cone around the aimed throw or the stroke
  let bestTarget: wref<TargetingComponent>;
  let bestLook: Float = 0.939693; // twenty degrees around the gaze, from the eye: wins over the rest
  let lookDirection: Vector4;
  let lookTarget: wref<TargetingComponent>;
  let ray: Vector4;
  let trace: TraceResult;
  let queries = GameInstance.GetSpatialQueriesSystem(this.GetGame());
  let i: Int32;
  query.testedSet = TargetingSet.Complete;
  query.searchFilter = TSF_NPC();
  query.maxDistance = 30.00;
  query.filterObjectByDistance = true;
  query.ignoreInstigator = true;
  GameInstance.GetTargetingSystem(this.GetGame()).GetTargetParts(this, query, parts);
  while i < ArraySize(parts) {
    component = TS_TargetPartInfo.GetComponent(parts[i]);
    if IsDefined(component) {
      target = component.GetEntity() as ScriptedPuppet;
      if IsDefined(target) && target.IsActive() && !target.IsDead() && !target.IsCrowd() && !GameObject.IsFriendlyTowardsPlayer(target) {
        point = ProjectileTargetingHelper.GetTargetingComponentsWorldPosition(component);
        delta = point - request.origin; delta.W = 0.00;
        if Vector4.Length(delta) >= 1.00 && Vector4.Length(delta) <= 30.00 {
          direction = Vector4.Normalize(delta);
          score = MaxF(Vector4.Dot(request.direction, direction), Vector4.Dot(request.stroke, direction));
          ray = point - request.eye; ray.W = 0.00;
          look = Vector4.Dot(request.gaze, Vector4.Normalize(ray));
          if look > bestLook || (!IsDefined(lookTarget) && score > best) {
            // Only geometry blocks this sight query. The projectile keeps its
            // actual swept collision and cannot receive damage through a wall.
            if !queries.SyncRaycastByQueryPreset(request.origin, point, n"Sight Blocker", trace, true, false) || Vector4.Distance(Vector4(trace.position.X, trace.position.Y, trace.position.Z, 1.00), point) < 0.35 {
              if look > bestLook { bestLook = look; lookDirection = direction; lookTarget = component; };
              if score > best { best = score; bestDirection = direction; bestTarget = component; };
            };
          };
        };
      };
    };
    i += 1;
  };
  if IsDefined(lookTarget) {
    // Leave straight at the part; the launch then curves on to it while it moves.
    request.direction = lookDirection;
    request.target = lookTarget;
    request.assisted = true;
  } else {
    if IsDefined(bestTarget) {
      request.direction = bestDirection;
      request.target = bestTarget;
      request.assisted = true;
    };
  };
}

@addMethod(PlayerPuppet)
public func VRMeleeThrowRelease(weapon: ref<WeaponObject>, direction: Vector4, speed: Float, handSpeed: Float, stroke: Vector4, gaze: Vector4, eyeOffset: Vector4) -> Bool {
  let request: ref<VrMeleeThrow>;
  if !this.VRMeleeThrowReady(weapon) || this.VRMeleeThrowPending(weapon) || Vector4.Length(direction) < 0.50 { return false; };
  request = new VrMeleeThrow();
  request.weapon = weapon;
  request.origin = weapon.GetWorldPosition();
  direction.W = 0.00;
  request.direction = Vector4.Normalize(direction);
  stroke.W = 0.00;
  request.stroke = Vector4.Length(stroke) > 0.50 ? Vector4.Normalize(stroke) : request.direction;
  gaze.W = 0.00;
  request.gaze = Vector4.Length(gaze) > 0.50 ? Vector4.Normalize(gaze) : request.direction;
  eyeOffset.W = 0.00;
  request.eye = request.origin + eyeOffset;
  request.speed = ClampF(speed, 4.00, 45.00);
  request.gravity = -1.50; // close to a straight line: 0.2 m down over 15 m at 30 m/s
  request.stamp = VRMeleeRealTime();
  if handSpeed >= 1.00 { this.VRMeleeThrowAssist(request); };
  this.vrMeleeThrow = request;
  return true;
}

@wrapMethod(MeleeThrowAttackDecisions)
protected const func EnterCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let player = scriptInterface.executionOwner as PlayerPuppet;
  let weapon = MeleeTransition.GetWeaponObject(scriptInterface);
  if IsDefined(player) && VRMeleeThrowEnabled() && IsDefined(weapon) && weapon.WeaponHasTag(n"Throwable") {
    return player.VRMeleeThrowPending(weapon) && this.HasAttackRecord(scriptInterface) && MeleeTransition.CanThrowWeaponObject(player, weapon);
  };
  return wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(MeleeTargetingDecisions)
protected final const func ExitCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let player = scriptInterface.executionOwner as PlayerPuppet;
  if IsDefined(player) && VRMeleeThrowEnabled() && player.VRMeleeThrowPending(MeleeTransition.GetWeaponObject(scriptInterface)) { return true; };
  return wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(MeleeTransition)
public final static func GetAimAssistMeleeRecord(const scriptInterface: ref<StateGameScriptInterface>) -> ref<AimAssistMelee_Record> {
  let weapon = MeleeTransition.GetWeaponObject(scriptInterface);
  if VRMeleeThrowEnabled() && IsDefined(scriptInterface.executionOwner as PlayerPuppet) && IsDefined(weapon) && weapon.WeaponHasTag(n"Throwable") {
    // Native camera snaps are inappropriate for tracked throwing. Only our
    // release direction receives assistance; neither eye/view is rotated.
    return null;
  };
  return wrappedMethod(scriptInterface);
}

@wrapMethod(MeleeThrowAttackEvents)
public func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  let player = scriptInterface.executionOwner as PlayerPuppet;
  let weapon = MeleeTransition.GetWeaponObject(scriptInterface);
  let physical = IsDefined(player) && VRMeleeThrowEnabled() && player.VRMeleeThrowPending(weapon);
  wrappedMethod(stateContext, scriptInterface);
  if physical {
    // The player's release replaces the flat animation's delayed release frame.
    // Execute the same native launch/recovery/visibility operations exactly once.
    this.m_projectileThrown = true;
    this.SetIsThrowReloading(stateContext, true);
    this.SendAnimFeatureData(stateContext, scriptInterface);
    this.SpawnMeleeWeaponProjectile(scriptInterface);
    this.ApplyThrowableCooldown(stateContext, scriptInterface);
    weapon.vrPhysicalThrowHidden = true;
    weapon.SetVisible(false);
  };
}

@wrapMethod(MeleeThrowReloadEvents)
public func OnExit(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  MeleeTransition.GetWeaponObject(scriptInterface).vrPhysicalThrowHidden = false;
}
@wrapMethod(MeleeThrowReloadEvents)
public func OnForcedExit(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  MeleeTransition.GetWeaponObject(scriptInterface).vrPhysicalThrowHidden = false;
}

@wrapMethod(ProjectileLaunchHelper)
private final static func SpawnProjectile(ownerObject: ref<GameObject>, projectileTemplateName: CName, appearance: CName, itemObj: ref<ItemObject>, logicalPositionProvider: ref<IPositionProvider>, logicalOrientationProvider: ref<IOrientationProvider>) -> Bool {
  let player = ownerObject as PlayerPuppet;
  let weapon = itemObj as WeaponObject;
  let request: ref<VrMeleeThrow>;
  let launch: ref<gameprojectileSpawnerLaunchEvent>;
  let position: WorldPosition;
  if IsDefined(player) && IsDefined(weapon) && VRMeleeThrowEnabled() && player.VRMeleeThrowPending(weapon) {
    request = player.vrMeleeThrow;
    WorldPosition.SetVector4(position, request.origin);
    launch = new gameprojectileSpawnerLaunchEvent();
    launch.launchParams.logicalPositionProvider = IPositionProvider.CreateStaticPositionProvider(position);
    launch.launchParams.logicalOrientationProvider = IOrientationProvider.CreateStaticOrientationProvider(Quaternion.BuildFromDirectionVector(request.direction));
    launch.launchParams.visualPositionProvider = launch.launchParams.logicalPositionProvider;
    launch.launchParams.visualOrientationProvider = launch.launchParams.logicalOrientationProvider;
    launch.templateName = projectileTemplateName;
    launch.appearance = appearance;
    launch.owner = ownerObject;
    launch.projectileParams.ignoreMountedVehicleCollision = true;
    if IsDefined(request.target) {
      launch.projectileParams.trackedTargetComponent = request.target;
      launch.projectileParams.targetPosition = ProjectileTargetingHelper.GetTargetingComponentsWorldPosition(request.target);
    };
    weapon.vrMeleeThrow = request;
    request.launched = true;
    itemObj.QueueEvent(launch);
    return true;
  };
  return wrappedMethod(ownerObject, projectileTemplateName, appearance, itemObj, logicalPositionProvider, logicalOrientationProvider);
}

@wrapMethod(MeleeProjectile)
protected cb func OnProjectileInitialize(eventData: ref<gameprojectileSetUpEvent>) -> Bool {
  let weapon = eventData.weapon as WeaponObject;
  wrappedMethod(eventData);
  this.vrMeleeThrow = null;
  if IsDefined(eventData.owner as PlayerPuppet) && IsDefined(weapon) && IsDefined(weapon.vrMeleeThrow) {
    if weapon.vrMeleeThrow.launched && !weapon.vrMeleeThrow.claimed && VRMeleeRealTime()-weapon.vrMeleeThrow.stamp < 1.00 {
      this.vrMeleeThrow = weapon.vrMeleeThrow;
      this.vrMeleeThrow.claimed = true;
    };
    weapon.vrMeleeThrow = null;
  };
}

@addMethod(MeleeProjectile)
private func VRPhysicalLaunch(eventData: ref<gameprojectileShootEvent>) -> Bool {
  let player = this.m_user as PlayerPuppet;
  if !IsDefined(this.vrMeleeThrow) { return false; };
  let target: wref<TargetingComponent> = this.vrMeleeThrow.target;
  let puppet: wref<ScriptedPuppet>;
  if IsDefined(target) { puppet = target.GetEntity() as ScriptedPuppet; };
  this.GeneralLaunchSetup(eventData);
  this.m_initialLaunchVelocity = this.vrMeleeThrow.speed;
  if IsDefined(puppet) && puppet.IsActive() && !puppet.IsDead() {
    // The flat game's homing throw (MeleeProjectile.OnShootTarget), at the player's speed.
    this.CurvedLaunchToTarget(eventData, puppet, target);
  } else {
    this.ParabolicLaunch(eventData, this.vrMeleeThrow.gravity, this.vrMeleeThrow.speed, this.GetProjectileTweakDBFloatParameter("energyLossFactorAfterCollision"));
  };
  if IsDefined(player) {
    player.vrMeleeThrowShots += 1;
    player.vrMeleeThrowLastOrigin = eventData.startPoint;
    player.vrMeleeThrowLastDirection = Matrix.GetDirectionVector(eventData.localToWorld);
  };
  return true;
}
@wrapMethod(MeleeProjectile)
protected cb func OnShoot(eventData: ref<gameprojectileShootEvent>) -> Bool {
  if this.VRPhysicalLaunch(eventData) { return true; };
  return wrappedMethod(eventData);
}
@wrapMethod(MeleeProjectile)
protected cb func OnShootTarget(eventData: ref<gameprojectileShootTargetEvent>) -> Bool {
  if this.VRPhysicalLaunch(eventData) { return true; };
  return wrappedMethod(eventData);
}
@wrapMethod(MeleeProjectile)
protected cb func OnCollision(eventData: ref<gameprojectileHitEvent>) -> Bool {
  let player = this.m_user as PlayerPuppet;
  if IsDefined(this.vrMeleeThrow) && IsDefined(player) { player.vrMeleeThrowHits += 1; };
  return wrappedMethod(eventData);
}
