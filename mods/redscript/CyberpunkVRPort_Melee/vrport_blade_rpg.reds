// A probe reserves a native shape; only the authoritative Health ledger publishes an event.
@addField(VrBladeImpact) public let stealthIntent: Bool;
@addField(VrBladeImpact) public let stealthApplied: Bool;
@addField(VrBladeImpact) public let portAtStart: Vector4;
@addField(VrBladeImpact) public let rearAtStart: Bool;
@addField(VrBladeImpact) public let portMatched: Bool;
@addField(VrBladeImpact) public let stealthStage: Int32;
@addField(VrBladeImpact) public let parentSerial: Uint32;
@addField(VrBladeImpact) public let overlapPoint: Vector4;
@addField(VrBladeImpact) public let published: Bool;
@addField(VrBladeImpact) public let availableHealth: Float;
@addField(VrBladeImpact) public let strengthReaction: Bool;
@addField(VrBladeImpact) public let acceptedShape: CName;
@addField(VrBladeImpact) public let partNotified: Bool;
public native func VRBladeEdgePath(side: Int32, serial: Uint32) -> array<Vector4>;
public native func VRBladeEdgeSample(side: Int32, serial: Uint32) -> array<Vector4>;
public native func VRBladeEdgeStrokeLive(side: Int32, serial: Uint32, stroke: Uint32) -> Bool;
public native func VRBladeEdgeShapes(component: ref<HitRepresentationComponent>, from: Vector4, to: Vector4) -> array<CName>;
public native func VRBladeReadSlotCapsule(component: ref<HitRepresentationComponent>, name: CName, data: ref<HitShapeUserData>, binding: CName) -> array<Vector4>;
public native func VRBladeAnimatedAttachment(side: Int32, key: Uint32, animated: Vector4, rotation: Quaternion, station: Vector4, bind: Bool) -> Vector4;

@addField(WeaponObject) public let vrBladeSlideTicks: Uint32;
public class VrBladeSlotAttachment extends IScriptable {
  public let slots: wref<HitRepresentationComponent>;
  public let shape: CName;
  public let slot: CName;
  public let capsule: array<Vector4>;
  public let localPoint: Vector4;
  public let fraction: Float;
  public let depth: Float;
  public let edgeFrames: array<ref<VrBladeVisiblePose>>;
  public let slideTracking: Bool;
  private let slideStation: Vector4;
  private let slideGrip: Vector4;
  private let slideLastGrip: Vector4;
  private let slideMovingGrip: Vector4;
  private let slideMovingStation: Vector4;
  private let slideStart: Float;
  private let slideSample: Float;
  private let slideLastMotion: Float;
  private let slideLastSlip: Float;

  // Keep a material point on the virtual blade and observe it in the target
  // slot's frame. Rigid motion of the joint pair leaves that point stationary.
  public func SlideFeedback(station: Vector4, grip: Vector4, now: Float) -> Bool {
    let gap=now-this.slideSample;
    let elapsed=now-this.slideStart;
    let reset=!this.slideTracking || gap<0.0 || gap>0.10
      || grip.W!=this.slideLastGrip.W || Vector4.Distance(grip,this.slideLastGrip)>0.20;
    if reset || Vector4.Distance(grip,this.slideMovingGrip)>0.002 {
      this.slideMovingGrip=grip;
      this.slideLastMotion=now;
    };
    if reset || Vector4.Distance(station,this.slideMovingStation)>0.002 {
      this.slideMovingStation=station;
      this.slideLastSlip=now;
    };
    let moving=now-this.slideLastMotion<=0.08 && now-this.slideLastSlip<=0.08;
    let pulse=!reset && moving && elapsed>=0.075
      && Vector4.Distance(station,this.slideStation)>=0.024
      && Vector4.Distance(grip,this.slideGrip)>=0.018;
    if reset || pulse || !moving {
      this.slideStation=station;this.slideGrip=grip;this.slideStart=now;
    };
    this.slideTracking=true;this.slideLastGrip=grip;this.slideSample=now;
    return pulse;
  }
}

public class VrBladeCutShape extends IScriptable {
  public let observation: ref<VrBladeObservedShape>;
  public let name: CName;
  public let slot: CName;
  public let capsule: array<Vector4>;
  public let reverseQuery: Bool;
  public let eitherEnd: Bool;
  public let window: Vector4;
  public let observedFrame: WorldTransform;
  public let shaftFrom: Vector4;
  public let shaftTo: Vector4;
  public let from: Vector4;
  public let to: Vector4;
  public let oldA: Vector4;
  public let oldB: Vector4;
  public let endA: Vector4;
  public let endB: Vector4;
}
@addField(VrBladeImpact) public let edgeStroke: Uint32;
@addField(VrBladeImpact) public let cutSlot: CName;
@addField(VrBladeImpact) public let cutPoint: Vector4;
@addField(VrBladeImpact) public let cutShape: ref<VrBladeCutShape>;
@addField(VrBladeImpact) public let cutCandidates: array<ref<VrBladeCutShape>>;

public class VrBladeReceipt extends IScriptable {
  public let impact: ref<VrBladeImpact>;
  public let entry: ref<VrBladeEntry>; // original sweep claim, including on continuation receipts
  public let released: Bool;
  public let lodged: Bool;
  public let awaitingSeat: Bool;
  public let lodgedAt: Float;
  public let serial: Uint32;
  public let jointSerial: Uint32; // The initial contact owns the solver joint across accepted continuations.
  public let lastEdgeStroke: Uint32;
  public let seatSlot: CName;
  public let seatPartPoint: Vector4;
  public let seatPoint: Vector4;
  public let seatAxis: Vector4;
  public let seatEdge: Vector4;
  public let attachment: ref<VrBladeSlotAttachment>;
  public let owner: wref<PlayerPuppet>;
  public let target: wref<NPCPuppet>;
  public let point: Vector4; // target entity local, not a skin or bone transform
  public let slots: wref<SlotComponent>;
  public let slot: CName;
  public let shapeName: CName;
  public let shapeData: ref<HitShapeUserData>;
  public let partPoint: Vector4;
  public let meshStart: Vector4;
  public let meshEnd: Vector4;
  public let zone: EHitReactionZone;
  public let time: Float;
  public let follow: Bool;
  public let puncture: Bool;
  public let stealth: Bool;
  public let spent: Bool;
  public let capsule: array<Vector4>;
  public let material: Int32;
  public let requestedBodies: Bool;
  public let jointTraceKind: Int32;
  public let jointProbeIndex: Int32;
  public let jointProbeUntil: Float;
  public let pending: ref<VrBladeImpact>;
  public let healthLoss: Float;
  public let loadTime: Float; // sim time of the last reaction on an already active ragdoll
}
@addField(WeaponObject) public let vrBladeReceipt: ref<VrBladeReceipt>;
@addField(WeaponObject) private let vrBladeFollowGate: CName;
// Distance to rendered shaft, solver direction cosine, XR direction cosine,
// number of intersected shape/slot pairs from the captured solver interval.
@addField(WeaponObject) public let vrBladeFollowGeometry: Vector4;
// Per-window signed solver travel, visible travel in the attachment and
// candidate slot frames, and length actually clipped to the native shape.
@addField(WeaponObject) public let vrBladeFollowWindow: Vector4;
// Raw occupied containers, resolved observations, retained intervals, unresolved slots.
@addField(WeaponObject) public let vrBladeFollowCounts: Vector4;
@addField(WeaponObject) public let vrBladeFollowSerial: Uint32;
@addField(WeaponObject) public let vrBladeFollowAttempt: Uint32;
@addMethod(WeaponObject)
public func VRBladeLastFollowGate() -> String { return NameToString(this.vrBladeFollowGate); }
@addMethod(WeaponObject)
public func VRBladeFollowResult(gate: CName) -> Bool {
  this.vrBladeFollowGate=gate; return Equals(gate,n"Dispatched");
}
@addField(PlayerPuppet) private let vrBladeNextJointSerial: Uint32;

@addMethod(WeaponObject)
public func VRBladeConfirmedSerial() -> Uint32 { return IsDefined(this.vrBladeReceipt) ? this.vrBladeReceipt.serial : 0u; }
@addMethod(WeaponObject)
public func VRBladeConfirmedTarget() -> wref<NPCPuppet> { return IsDefined(this.vrBladeReceipt) ? this.vrBladeReceipt.target : null; }
@addMethod(WeaponObject)
public func VRBladeConfirmedLocalPoint() -> Vector4 { return IsDefined(this.vrBladeReceipt) ? this.vrBladeReceipt.point : Vector4(0.00, 0.00, 0.00, 0.00); }
@addMethod(WeaponObject)
public func VRBladeAnchorWorld(opt allowRoot: Bool) -> Vector4 {
  let frame: WorldTransform;
  let r = this.vrBladeReceipt;
  let point: Vector4;
  if !IsDefined(r) || !IsDefined(r.target) || !r.target.IsAttached() { return Vector4(0,0,0,0); };
  if IsNameValid(r.seatSlot) && IsDefined(r.slots) && r.slots.GetSlotTransform(r.seatSlot,frame) {
    point = WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,r.seatPartPoint));
  } else {
    if IsNameValid(r.seatSlot) && !allowRoot { return Vector4(0,0,0,0); };
    point = WorldPosition.ToVector4(WorldTransform.TransformPoint(r.target.GetWorldTransform(),r.seatPoint));
  };
  point.W=1.00; return point;
}
@addMethod(WeaponObject)
public func VRBladeConfirmedZone() -> EHitReactionZone { return IsDefined(this.vrBladeReceipt) ? this.vrBladeReceipt.zone : EHitReactionZone.Special; }
@addMethod(WeaponObject)
public func VRBladeConfirmedFollowThrough() -> Bool { return IsDefined(this.vrBladeReceipt) && this.vrBladeReceipt.follow; }
@addMethod(WeaponObject)
public func VRBladeConfirmedStealth() -> Bool { return IsDefined(this.vrBladeReceipt) && this.vrBladeReceipt.stealth; }
@addMethod(WeaponObject)
public func VRBladeConfirmedAge() -> Float {
  let r = this.vrBladeReceipt;
  let age: Float;
  if !IsDefined(r) || !IsDefined(r.owner) { return -1.00; };
  age = EngineTime.ToFloat(GameInstance.GetSimTime(r.owner.GetGame())) - r.time;
  return age >= 0.00 ? age : -1.00;
}

public abstract class VrBladeRPG {
  public static func MaterialId(material: CName) -> Int32 {
    let name = NameToString(material);
    if StrContains(name, "armor") || StrContains(name, "armour") { return 3; };
    if StrContains(name, "metal") || StrContains(name, "steel") { return 2; };
    if StrContains(name, "cyber") { return 1; };
    if StrContains(name, "flesh") || StrContains(name, "skin") { return 0; };
    return 4;
  }
  public static func ValidZone(shape: HitShapeData) -> Bool {
    return VrBladeRPG.ValidZoneData(shape.userData as HitShapeUserDataBase);
  }
  public static func ValidZoneData(data: ref<HitShapeUserDataBase>) -> Bool {
    return IsDefined(data) && !HitShapeUserDataBase.IsProtectionLayer(data) && NotEquals(data.GetShapeType(), EHitShapeType.None)
      && (HitShapeUserDataBase.IsHitReactionZoneHead(data) || HitShapeUserDataBase.IsHitReactionZoneTorso(data) || HitShapeUserDataBase.IsHitReactionZoneLimb(data));
  }
  // Prefer an authored hit region. On rigs without one, intersect the selected
  // native contact interval and the current weapon segment with the same port.
  public static func StealthRegion(name: CName) -> Bool { return Equals(name, n"Neck"); }
  public static func PortContact(impact: ref<VrBladeImpact>, shape: HitShapeData) -> Bool {
    let port: Vector4;
    let entry = shape.result.hitPositionEnter; entry.W = 1.00;
    let exit = shape.result.hitPositionExit; exit.W = 1.00;
    let a: Vector4;
    let b: Vector4;
    let along: Vector4;
    if VrBladeRPG.StealthRegion(shape.hitShapeName) { return true; };
    if impact.hasNeckSlot { return false; };
    port = impact.portAtStart;
    if port.W < 0.50 || !(Equals(shape.hitShapeName,n"Head") || Equals(shape.hitShapeName,n"Chest") || Equals(shape.hitShapeName,n"LeftTorso")) { return false; };
    along=VrCyberwareContactMath.Closest(port,impact.a,impact.b);
    if Vector4.Distance(port,along) > 0.06 { return false; };
    // The authored marker is a separate trigger. Stock upper-body proxies may end on
    // either side of it. Require the same bounded neighbourhood ALONG the actual blade,
    // not two coincident collider surfaces. A generic Head hit cannot activate the port.
    a=VrCyberwareContactMath.Closest(entry,impact.a,impact.b); b=a;
    if Vector4.Distance(entry,exit) <= 1.25 { b=VrCyberwareContactMath.Closest(exit,impact.a,impact.b); };
    // Compare the surface of the marker, not its centre, to the local interval.
    return MaxF(0.00, Vector4.Distance(along,VrCyberwareContactMath.Closest(along,a,b)) - 0.06) <= 0.06;
  }
  public static func Behind(player: ref<PlayerPuppet>, npc: ref<NPCPuppet>) -> Bool {
    let relative = player.GetWorldPosition() - npc.GetWorldPosition();
    let facing = npc.GetWorldForward();
    return relative.X * facing.X + relative.Y * facing.Y < 0.00;
  }
  public static func WoundTarget(player: ref<PlayerPuppet>, npc: ref<NPCPuppet>) -> Bool {
    let id = npc.GetEntityID();
    let stats = GameInstance.GetStatsSystem(player.GetGame());
    let god = GameInstance.GetGodModeSystem(player.GetGame());
    return IsDefined(stats) && IsDefined(god) && npc.IsActive() && !npc.IsDead() && !npc.IsIncapacitated() && !npc.IsAboutToDieOrDefeated()
      && !ScriptedPuppet.IsDefeated(npc) && !npc.IsQuest() && !npc.IsVendor() && !npc.IsCrowd() && !npc.IsCharacterCivilian()
      && NotEquals(GameObject.GetAttitudeTowards(npc, player), EAIAttitude.AIA_Friendly) && Equals(npc.GetNPCType(),gamedataNPCType.Human)
      && !god.HasGodMode(id, gameGodModeType.Invulnerable) && !god.HasGodMode(id, gameGodModeType.Immortal)
      && !stats.GetStatBoolValue(Cast<StatsObjectID>(id), gamedataStatType.IsInvulnerable);
  }
  public static func SpecialTarget(player: ref<PlayerPuppet>, npc: ref<NPCPuppet>) -> Bool {
    let id=npc.GetEntityID();
    let rarity = npc.GetNPCRarity();
    let bb = player.GetPlayerStateMachineBlackboard();
    return VrBladeRPG.WoundTarget(player,npc) && IsDefined(bb) && bb.GetInt(GetAllBlackboardDefs().PlayerStateMachine.Takedown) == EnumInt(gamePSMTakedown.Default)
      && !player.GetIsInFastFinisher() && !player.GetIsInWorkspotFinisher() && !GameInstance.GetWorkspotSystem(player.GetGame()).IsActorInWorkspot(player)
      && !GameInstance.GetWorkspotSystem(player.GetGame()).IsActorInWorkspot(npc)
      && !npc.IsRagdolling() && !npc.IsBoss() && !npc.IsMassive() && Equals(GameObject.GetAttitudeTowards(npc, player), EAIAttitude.AIA_Hostile)
      && (Equals(rarity, gamedataNPCRarity.Trash) || Equals(rarity, gamedataNPCRarity.Weak)
        || Equals(rarity, gamedataNPCRarity.Normal) || Equals(rarity, gamedataNPCRarity.Elite))
      && !npc.IsResistantToTakedown() && !NPCManager.HasTag(npc.GetRecordID(), n"ImmuneToTakedown") && !NPCManager.HasTag(npc.GetRecordID(), n"MiniBoss")
      && !GameInstance.GetStatusEffectSystem(player.GetGame()).HasStatusEffect(id, t"BaseStatusEffect.BlockGrapple")
      && NotEquals(RPGManager.CalculatePowerDifferential(npc), gameEPowerDifferential.IMPOSSIBLE);
  }
  public static func FollowMatches(hit: ref<gameHitEvent>, impact: ref<VrBladeImpact>, shape: HitShapeData) -> Bool {
    let weapon = hit.attackData.GetWeapon();
    let player = hit.attackData.GetInstigator() as PlayerPuppet;
    let npc = hit.target as NPCPuppet;
    let age: Float;
    if !IsDefined(weapon) || !IsDefined(player) || !IsDefined(npc) || !IsDefined(weapon.vrBladeReceipt) || !player.VRBladeAllowed(weapon)
      || weapon.VRBladeConfirmedSerial() != impact.parentSerial || weapon.vrBladeReceipt.owner != player || weapon.vrBladeReceipt.target != npc
      || weapon.vrBladeReceipt.pending != impact || impact.cancelled || weapon.vrBladeReceipt.released || weapon.vrBladeReceipt.spent || !VrBladeRPG.SpecialTarget(player, npc) { return false; };
    age = weapon.VRBladeConfirmedAge();
    if age<0.0 || EngineTime.ToFloat(GameInstance.GetSimTime(player.GetGame()))-impact.created>0.20
      || impact.contactMetrics.X<=0.0 || impact.contactMetrics.Z<0.96 { return false; };
    if impact.edgeStroke!=0u {
      return weapon.vrBladeReceipt.lodged && impact.edgeStroke!=weapon.vrBladeReceipt.lastEdgeStroke
        && weapon.vrBladeReceipt.slots==npc.FindComponentByName(n"HitRepresentation") as SlotComponent
        && VRBladeEdgeStrokeLive(VRLeftHandedWeapon.InHand(player) ? 0 : 1,weapon.vrBladeReceipt.jointSerial,impact.edgeStroke);
    };
    return Equals(weapon.vrBladeReceipt.zone, HitShapeUserDataBase.GetHitReactionZone(shape.userData as HitShapeUserDataBase))
      && IsNameValid(weapon.vrBladeReceipt.slot) && Equals(weapon.vrBladeReceipt.shapeName,shape.hitShapeName)
      && VrBladeRPG.SameShapeData(weapon.vrBladeReceipt,shape)
      && impact.overlapPoint.W > 0.50;
  }
  public static func SameShapeData(r: ref<VrBladeReceipt>, shape: HitShapeData) -> Bool {
    if !IsDefined(r.shapeData) || !IsDefined(shape.userData) { return false; };
    if r.shapeData==shape.userData { return true; };
    if VrBladeRPG.MaterialId(shape.physicsMaterial)!=r.material || ArraySize(r.capsule)!=2 { return false; };
    let geometry=VRBladeReadCapsule(r.slots as HitRepresentationComponent,shape.hitShapeName,shape.userData);
    return ArraySize(geometry)==2 && Vector4.Distance(geometry[0],r.capsule[0])<0.0001
      && Vector4.Distance(geometry[1],r.capsule[1])<0.0001 && AbsF(geometry[0].W-r.capsule[0].W)<0.0001
      && AbsF(geometry[1].W-r.capsule[1].W)<0.0001;
  }
  public static func Budget(current: Float, maximum: Float, floor: Float, elite: Bool) -> Float {
    let available = MaxF(0.00, current - MaxF(0.00, floor));
    return elite ? MinF(available, 0.80 * maximum) : available;
  }
  public static func FollowFactor(strength: Float) -> Float {
    return 1.75 + 0.25 * ClampF((strength - 3.00) / 17.00, 0.00, 1.00);
  }
  public static func Rejected(data: ref<AttackData>) -> Bool {
    return data.WasBlocked() || data.WasDeflectedAny() || data.HasFlag(hitFlag.WasDodged) || data.HasFlag(hitFlag.WasEvaded)
      || data.HasFlag(hitFlag.DealNoDamage) || data.HasFlag(hitFlag.DamageNullified) || data.HasFlag(hitFlag.FriendlyFireIgnored)
      || data.HasFlag(hitFlag.ImmortalTarget) || data.HasFlag(hitFlag.TargetWasAlreadyDeadNoStatPool);
  }
  public static func ApplyRule(hit: ref<gameHitEvent>, impact: ref<VrBladeImpact>, player: ref<PlayerPuppet>, npc: ref<NPCPuppet>, weapon: ref<WeaponObject>) -> Void {
    let data = hit.attackData;
    let stats = GameInstance.GetStatsSystem(player.GetGame());
    let pools = GameInstance.GetStatPoolsSystem(player.GetGame());
    let id = Cast<StatsObjectID>(npc.GetEntityID());
    let shape = hit.hitRepresentationResult.hitShapes[0].userData as HitShapeUserDataBase;
    let physical = hit.attackComputed.GetAttackValue(gamedataDamageType.Physical);
    let hp: Float;
    let maxHP: Float;
    let floor: Float;
    let budget: Float;
    impact.physicalBefore=physical;
    impact.physicalAfter=physical;
    // Native DrainStatPool may impose a floor after computing its Health ledger.
    // Bound the amount we can confirm, without suppressing ordinary above-floor contacts.
    if !IsDefined(pools) { return; };
    hp = pools.GetStatPoolValue(id, gamedataStatPoolType.Health, false);
    maxHP = pools.GetStatPoolMaxPointValue(id, gamedataStatPoolType.Health);
    floor = pools.GetStatPoolValueCustomLimit(id, gamedataStatPoolType.Health, false);
    impact.availableHealth = hp - MaxF(floor, maxHP * data.GetMinimumHealthPercent() * 0.01);
    if (!impact.thrust && !impact.stealthIntent && impact.parentSerial == 0u) || !IsDefined(stats) || !VrBladeRPG.WoundTarget(player, npc)
      || !player.VRBladeAllowed(weapon) || VrBladeRPG.Rejected(data) || data.HasFlag(hitFlag.Nonlethal) || data.HasFlag(hitFlag.ReduceDamage)
      || data.HasFlag(hitFlag.WasMitigated) || data.HasFlag(hitFlag.CannotModifyDamage) || data.HasFlag(hitFlag.DeterministicDamage)
      || data.HasFlag(hitFlag.DoNotTriggerFinisher) || data.HasFlag(hitFlag.Kill) || data.HasFlag(hitFlag.FinisherTriggered)
      || data.HasFlag(hitFlag.IgnoreImmortalityModes) || data.HasFlag(hitFlag.IgnoreStatPoolCustomLimit) || data.GetMinimumHealthPercent() > 0.00
      || Equals(data.GetHitType(), gameuiHitType.Glance) || !IsDefined(shape) || NotEquals(shape.GetShapeType(), EHitShapeType.Flesh)
      || (shape.m_hitShapeDamageMod > 0.00 && shape.m_hitShapeDamageMod < 1.00) || stats.GetStatValue(id, gamedataStatType.Armor) > 0.00
      || stats.GetStatValue(id, gamedataStatType.PhysicalResistance) > 0.00 || stats.GetStatValue(id, gamedataStatType.MeleeResistance) > 0.00
      || stats.GetStatValue(id, gamedataStatType.MaxPercentDamageTakenPerHit) > 0.00
      || pools.GetStatPoolValue(id, gamedataStatPoolType.Overshield, false) > 0.00 || pools.GetStatPoolValue(id, gamedataStatPoolType.CPO_Armor, false) > 0.00
      || !(physical > 0.00) || physical != hit.attackComputed.GetTotalAttackValue(gamedataStatPoolType.Health) { return; };
    if impact.parentSerial != 0u {
      if !VrBladeRPG.SpecialTarget(player,npc) { return; };
      let strength = stats.GetStatValue(Cast<StatsObjectID>(player.GetEntityID()), gamedataStatType.Strength);
      impact.strengthReaction = !impact.noDismember && strength >= 12.00;
      impact.physicalAfter=physical * VrBladeRPG.FollowFactor(strength);
      hit.attackComputed.SetAttackValue(impact.physicalAfter, gamedataDamageType.Physical);
      return;
    };
    if impact.thrust {
      impact.physicalAfter=physical * 1.50;
      hit.attackComputed.SetAttackValue(impact.physicalAfter, gamedataDamageType.Physical);
      impact.bleedOnHit = true;
    };
    if !impact.stealthIntent || !VrBladeRPG.SpecialTarget(player,npc) { return; };
    impact.stealthStage=1;
    impact.portMatched=VrBladeRPG.PortContact(impact,hit.hitRepresentationResult.hitShapes[0]);
    let rearThrust = impact.thrust && (HitShapeUserDataBase.IsHitReactionZoneTorso(shape) || HitShapeUserDataBase.IsHitReactionZoneHead(shape));
    if weapon.VRBladeKind() <= 0 || !impact.rearAtStart || !(rearThrust || impact.portMatched) { return; };
    impact.stealthStage=2;
    if !data.HasFlag(hitFlag.StealthHit) || AttackData.IsPlayerInCombat(data)
      || !IsDefined(npc.GetPuppetStateBlackboard()) || NotEquals(npc.GetHighLevelStateFromBlackboard(), gamedataNPCHighLevelState.Relaxed) || npc.GetDetectionPercentage() != 0.00 { return; };
    budget = VrBladeRPG.Budget(hp, maxHP, floor, npc.IsElite());
    // Sub-point channels round up to 1 in ApplyDamage. Do not promise a smaller cap in that case.
    if !(hp > 0.00 && maxHP >= hp && budget >= 1.00) { return; };
    hit.attackComputed.SetAttackValue(budget, gamedataDamageType.Physical);
    impact.physicalAfter=budget;
    impact.stealthApplied = true;
    impact.bleedOnHit = false;
  }
}

// No new hit for stealth: adjust only the exact contact about to enter ApplyDamage.
@wrapMethod(DamageSystem)
private final func DealDamages(hitEvent: ref<gameHitEvent>) -> Void {
  let ticket = hitEvent.vrMeleeEnhancedTicket;
  let blade: ref<Attack_GameEffect>;
  let player: ref<PlayerPuppet>;
  let npc: ref<NPCPuppet>;
  let weapon: ref<WeaponObject>;
  if IsDefined(ticket) && !ticket.confirmed && !hitEvent.projectionPipeline && IsDefined(hitEvent.attackData) && IsDefined(hitEvent.attackComputed) {
    blade = hitEvent.attackData.GetAttackDefinition() as Attack_GameEffect;
    if IsDefined(blade) && blade == ticket.attack && IsDefined(blade.vrBladeImpact) && blade.vrBladeImpact.claimed && !blade.vrBladeImpact.cancelled {
      player = hitEvent.attackData.GetInstigator() as PlayerPuppet; npc = hitEvent.target as NPCPuppet; weapon = hitEvent.attackData.GetWeapon();
      if IsDefined(player) && IsDefined(npc) && IsDefined(weapon) && weapon.GetItemID() == ticket.weapon && npc.GetEntityID() == ticket.target && ticket.target == blade.vrBladeImpact.target
        && !GameInstance.GetRuntimeInfo(player.GetGame()).IsClient() && ArraySize(hitEvent.hitRepresentationResult.hitShapes) == 1 && VrBladeRPG.ValidZone(hitEvent.hitRepresentationResult.hitShapes[0]) {
        VrBladeRPG.ApplyRule(hitEvent, blade.vrBladeImpact, player, npc, weapon);
      };
    };
  };
  wrappedMethod(hitEvent);
}

@addMethod(PlayerPuppet)
public func VRBladePublishHit(hit: ref<gameHitEvent>, healthLoss: Float) -> Bool {
  let ticket = hit.vrMeleeEnhancedTicket;
  let weapon = hit.attackData.GetWeapon();
  let blade = hit.attackData.GetAttackDefinition() as Attack_GameEffect;
  let npc = hit.target as NPCPuppet;
  let impact: ref<VrBladeImpact>;
  let r: ref<VrBladeReceipt>;
  let shape: HitShapeData;
  let entry: Vector4;
  let frame: WorldTransform;
  if !(healthLoss > 0.00) || hit.projectionPipeline || !IsDefined(ticket) || !IsDefined(npc) || !IsDefined(weapon) || !IsDefined(blade)
    || blade != ticket.attack || weapon.GetItemID() != ticket.weapon || ticket.target != npc.GetEntityID() || hit.attackData.GetInstigator() != this
    || GameInstance.GetRuntimeInfo(this.GetGame()).IsClient() || GameInstance.GetTransactionSystem(this.GetGame()).GetItemInSlot(this, VRLeftHandedWeapon.HeldSlot(this)) != weapon { return false; };
  impact = blade.vrBladeImpact;
  if !IsDefined(impact) || !impact.claimed || impact.cancelled || impact.published || impact.target != ticket.target || ArraySize(hit.hitRepresentationResult.hitShapes) != 1
    || VrBladeRPG.Rejected(hit.attackData) || !(impact.availableHealth > 0.00) { return false; };
  shape = hit.hitRepresentationResult.hitShapes[0];
  if !VrBladeRPG.ValidZone(shape) || (impact.parentSerial != 0u && impact.parentSerial != weapon.VRBladeConfirmedSerial()) { return false; };
  impact.published = true;
  r = new VrBladeReceipt(); r.serial = weapon.VRBladeConfirmedSerial() + 1u;
  r.impact = impact;
  if r.serial == 0u { r.serial = 1u; };
  r.time = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame())); r.owner = this; r.target = npc;
  entry = impact.parentSerial != 0u ? impact.overlapPoint : shape.result.hitPositionEnter; entry.W = 1.00;
  r.point = WorldTransform.TransformInvPoint(npc.GetWorldTransform(), entry); r.point.W = 1.00;
  r.slots = npc.FindComponentByName(n"HitRepresentation") as SlotComponent;
  r.shapeName = shape.hitShapeName; r.shapeData = shape.userData;
  let binding = impact.edgeStroke!=0u ? impact.cutSlot : (Equals(r.shapeName,n"Neck") ? n"Head" : r.shapeName);
  if IsDefined(r.slots) && IsNameValid(binding) && r.slots.GetSlotTransform(binding,frame) {
    r.slot = binding; r.partPoint = WorldTransform.TransformInvPoint(frame,entry); r.partPoint.W=1.00;
    if impact.edgeStroke!=0u {
      r.partPoint=impact.cutPoint;
      r.capsule=impact.cutShape.capsule;
    } else {
      r.capsule=VRBladeReadCapsule(r.slots as HitRepresentationComponent,r.shapeName,shape.userData);
    };
    r.meshStart = WorldTransform.TransformInvPoint(frame, impact.a);
    r.meshEnd = WorldTransform.TransformInvPoint(frame, impact.b);
  };
  r.zone = HitShapeUserDataBase.GetHitReactionZone(shape.userData as HitShapeUserDataBase);
  r.material = VrBladeRPG.MaterialId(shape.physicsMaterial);
  r.follow = impact.parentSerial != 0u; r.stealth = impact.stealthApplied; r.healthLoss = MinF(healthLoss, impact.availableHealth);
  r.entry = r.follow ? weapon.vrBladeReceipt.entry : impact.entry;
  r.puncture = impact.thrust || impact.stealthApplied;
  if r.follow {
    r.jointSerial = weapon.vrBladeReceipt.jointSerial;
    r.seatPoint=weapon.vrBladeReceipt.seatPoint; r.seatSlot=weapon.vrBladeReceipt.seatSlot;
    r.seatPartPoint=weapon.vrBladeReceipt.seatPartPoint;
    r.seatAxis=weapon.vrBladeReceipt.seatAxis;r.seatEdge=weapon.vrBladeReceipt.seatEdge;
    r.attachment=weapon.vrBladeReceipt.attachment;
    r.lastEdgeStroke=impact.edgeStroke;
    r.lodged = weapon.vrBladeReceipt.lodged;
    r.lodgedAt = weapon.vrBladeReceipt.lodgedAt;
    r.requestedBodies = weapon.vrBladeReceipt.requestedBodies;
    if impact.edgeStroke==0u && !hit.attackData.HasFlag(hitFlag.DisableVFX) { VRBladeContinueTrail(r,weapon.vrBladeReceipt); };
  } else {
    r.seatPoint=r.point; r.seatSlot=r.slot; r.seatPartPoint=r.partPoint;
    r.released=IsDefined(impact.entry) && impact.entry.endedClear;
    if r.puncture && !r.released {
      r.seatAxis=Vector4.Normalize(impact.b-impact.a);
      r.seatEdge=Vector4.Normalize(impact.attackEdge-r.seatAxis*Vector4.Dot(impact.attackEdge,r.seatAxis));
      r.awaitingSeat=true;r.lodgedAt=VRMeleeRealTime();
    };
    this.vrBladeNextJointSerial += 1u;
    if this.vrBladeNextJointSerial == 0u { this.vrBladeNextJointSerial = 1u; };
    r.jointSerial = this.vrBladeNextJointSerial;
  };
  if !r.follow && IsDefined(weapon.vrBladeReceipt) && weapon.vrBladeReceipt.lodged && IsDefined(weapon.vrBladeReceipt.target) {
    weapon.VREndBladeMovementHold(weapon.vrBladeReceipt);
  };
  hit.vrSeatedBladeReaction = r.awaitingSeat || (r.follow && r.lodged);
  weapon.vrBladeReceipt = r;
  if impact.bleedOnHit && !npc.IsDead() && !StatusEffectSystem.ObjectHasStatusEffect(npc,t"BaseStatusEffect.Bleeding") {
    StatusEffectHelper.ApplyStatusEffect(npc,t"BaseStatusEffect.Bleeding",this.GetEntityID());
  };
  if !r.follow && IsDefined(impact.swing) && impact.swing.chain && !impact.swing.landed {
    impact.swing.landed = true; this.VRMeleeChainLanded(impact.swing);
  };
  if (!r.follow || impact.edgeStroke!=0u) && !hit.attackData.HasFlag(hitFlag.DisableVFX) {
    let motion=impact.edgeStroke!=0u ? impact.sweepTo-impact.sweepFrom : impact.endB-impact.previousB;
    if IsDefined(impact.cutShape) && IsDefined(r.slots) && r.slots.GetSlotTransform(r.slot,frame) {
      impact.previousA=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,impact.cutShape.oldA));
      impact.previousB=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,impact.cutShape.oldB));
      impact.endA=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,impact.cutShape.endA));
      impact.endB=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,impact.cutShape.endB));
      motion=Quaternion.Transform(WorldTransform.GetOrientation(frame),impact.cutShape.to-impact.cutShape.from);
    };
    VRBladeAddSurfaceMark(npc,shape,motion,r,impact);
  };
  return true;
}

// Equal-response side of the soft hand constraint. Do not change the animator's
// ownership, turn on ragdoll, or queue damage. Sleeping/no-contact frames cost no events.
@addMethod(WeaponObject)
public func VRBladeLoadRagdoll(serial: Uint32, wristReaction: Vector4) -> Bool {
  let r = this.vrBladeReceipt;
  let anchor: Vector4;
  let magnitude: Float;
  let now: Float;
  let dt: Float;
  if serial == 0u || !IsDefined(r) || r.serial != serial || !IsDefined(r.owner) || !IsDefined(r.target)
    || r.follow || r.spent || !IsNameValid(r.slot) || !r.target.IsRagdolling() || !r.owner.VRBladeAllowed(this) { return false; };
  wristReaction.W = 0.00; magnitude = Vector4.Length(wristReaction);
  if !(magnitude > 0.006 && magnitude <= 0.081) { return false; };
  now = EngineTime.ToFloat(GameInstance.GetSimTime(r.owner.GetGame()));
  dt = now - MaxF(r.time, r.loadTime);
  if dt < 0.05 { return false; };
  anchor = this.VRBladeAnchorWorld(); if anchor.W < 0.50 { return false; };
  r.loadTime = now;
  r.target.QueueEvent(CreateRagdollApplyImpulseEvent(anchor, wristReaction * (-160.00 * MinF(dt, 0.08)), 0.12));
  return true;
}

@addMethod(PlayerPuppet)
public func VRBladeTryFollowThrough(weapon: wref<WeaponObject>, serial: Uint32, a: Vector4, b: Vector4, motion: Vector4, opt oldA: Vector4, opt oldB: Vector4) -> Bool {
  let r: ref<VrBladeReceipt>;
  let impact: ref<VrBladeImpact>;
  let anchor: Vector4;
  let age: Float;
  let len: Float;
  if !IsDefined(weapon) { return false; };
  weapon.vrBladeFollowSerial=serial;
  weapon.vrBladeFollowAttempt+=1u;
  weapon.vrBladeFollowCounts=Vector4(0,0,0,-1.0);
  weapon.vrBladeFollowGeometry=Vector4(0,0,0,0);
  weapon.vrBladeFollowWindow=Vector4(0,0,0,0);
  if serial == 0u || weapon.VRBladeConfirmedSerial() != serial { return weapon.VRBladeFollowResult(n"ReceiptChanged"); };
  if !this.VRBladeAllowed(weapon) || this.VRHolsterIntentActive(1.00) { return weapon.VRBladeFollowResult(n"WeaponUnavailable"); };
  r = weapon.vrBladeReceipt; age = weapon.VRBladeConfirmedAge();
  if r.owner != this || age < 0.00 || !IsDefined(r.target) { return weapon.VRBladeFollowResult(n"ReceiptInvalid"); };
  if r.spent || r.released { return weapon.VRBladeFollowResult(n"SpentOrReleased"); };
  if !VrBladeRPG.SpecialTarget(this,r.target) {
    return weapon.VRBladeFollowResult(r.target.IsRagdolling() ? n"TargetRagdoll" : n"TargetRules");
  };
  if IsDefined(r.pending) && EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame())) - r.pending.created < 0.085 { return weapon.VRBladeFollowResult(n"Pending"); };
  len = Vector4.Distance(a, b);
  if !(len >= 0.04 && len <= weapon.VRMeleeReach() + 0.02 && Vector4.Distance(a, weapon.GetWorldPosition()) <= 0.15) { return weapon.VRBladeFollowResult(n"WeaponPose"); };
  motion.W = 0.0;
  if !(Vector4.Length(motion) >= 0.045 && Vector4.Length(motion) <= 0.35) { return weapon.VRBladeFollowResult(n"ControllerDistance"); };
  impact = new VrBladeImpact(); impact.parentSerial = serial; impact.target = r.target.GetEntityID(); impact.a = a; impact.b = b;
  if r.lodged || r.puncture {
    if !r.lodged { return weapon.VRBladeFollowResult(n"NotLodged"); };
    if !weapon.VRBladeCaptureCut(impact,motion) { return false; };
  } else {
    let measured = weapon.VRBladeMeasure(serial, a, b, motion);
    if !(measured.X > 0.0 && measured.Z >= 0.96) { return weapon.VRBladeFollowResult(n"EntryShapeMotion"); };
    anchor = weapon.VRBladeOverlapPoint(a,b,weapon.GetWorldForward());
    if anchor.W < 0.50 { return weapon.VRBladeFollowResult(n"EntryShapeOverlap"); };
    impact.overlapPoint = anchor; impact.contactMetrics = measured;
  };
  impact.created = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame())); impact.noDismember = weapon.VRBladeNoDismember();
  if IsDefined(r.pending) { r.pending.cancelled = true; };
  r.pending = impact; // Reserve before dispatch, including synchronous callbacks.
  weapon.VRBladeNoteProbe(impact);
  if this.VRCyberwareOrdinaryContact(weapon, r.target, a, b, false, impact) > 0 { return weapon.VRBladeFollowResult(n"Dispatched"); };
  impact.cancelled = true; r.pending = null; return weapon.VRBladeFollowResult(n"EffectStart");
}

@addMethod(WeaponObject)
private func VRBladeCaptureCut(impact: ref<VrBladeImpact>, motion: Vector4) -> Bool {
  let r=this.vrBladeReceipt;
  let side=VRLeftHandedWeapon.InHand(r.owner) ? 0 : 1;
  let path=VRBladeEdgeSample(side,r.jointSerial);
  if ArraySize(path)!=3 || path[2].W!=5.0 { return this.VRBladeFollowResult(n"NoSolverPath"); };
  if !IsDefined(r.slots) { return this.VRBladeFollowResult(n"NoHitSlots"); };
  let stroke=Cast<Uint32>(path[0].W);
  if stroke==0u || stroke==r.lastEdgeStroke { return this.VRBladeFollowResult(n"UsedStroke"); };
  let start=path[0]; start.W=1.0;
  let end=path[1]; end.W=1.0;
  let travel=end-start; travel.W=0.0;
  let distance=Vector4.Length(travel);
  let railEdge=path[2]; railEdge.W=0.0;
  this.vrBladeFollowGeometry.X=Vector4.Distance(end,VrCyberwareContactMath.Closest(end,impact.a,impact.b));
  this.vrBladeFollowGeometry.Y=Vector4.Dot(travel,railEdge)/MaxF(distance,0.000001);
  this.vrBladeFollowGeometry.Z=Vector4.Dot(motion,r.seatEdge)/MaxF(Vector4.Length(motion),0.000001);
  // Distance since rail opening may exceed one accepted sample interval.
  // ClipStations checks the signed increment over the same visible window.
  if !(distance>=0.008) { return this.VRBladeFollowResult(n"SolverDistance"); };
  if !(this.vrBladeFollowGeometry.Y>=0.96) { return this.VRBladeFollowResult(n"SolverDirection"); };
  if !(this.vrBladeFollowGeometry.Z>=0.96) { return this.VRBladeFollowResult(n"ControllerDirection"); };
  return this.VRBladeClipStations(impact,stroke,Vector4.Dot(motion,r.seatEdge));
}

// A native entry on the occupied shaft confirms the stock target/shape query.
// It may arrive in request-time world coordinates after the slot has moved.
@addMethod(WeaponObject)
public func VRBladeCutGap(shape: ref<VrBladeCutShape>, hit: HitShapeData) -> Float {
  let r=this.vrBladeReceipt;
  let frame: WorldTransform;
  if !IsDefined(r) || !IsDefined(shape) || !IsDefined(r.slots) || !Equals(shape.name,hit.hitShapeName)
    || !r.slots.GetSlotTransform(shape.slot,frame) { return 100000.0; };
  let obs=shape.observation;
  if !IsDefined(obs) || !VRBladeMatchesOccupied(r.slots as HitRepresentationComponent,hit,shape.slot,obs.geometry,obs.data,
    obs.material,shape.capsule[0],shape.capsule[1]) { return 100000.0; };
  let entry=hit.result.hitPositionEnter;entry.W=1.0;
  let here=WorldTransform.TransformInvPoint(frame,entry);
  let then=WorldTransform.TransformInvPoint(shape.observedFrame,entry);
  return MinF(Vector4.Distance(here,VrCyberwareContactMath.Closest(here,shape.shaftFrom,shape.shaftTo)),
    Vector4.Distance(then,VrCyberwareContactMath.Closest(then,shape.shaftFrom,shape.shaftTo)));
}

@addMethod(WeaponObject)
public func VRBladeMeasure(serial: Uint32, a: Vector4, b: Vector4, motion: Vector4) -> Vector4 {
  let r = this.vrBladeReceipt;
  let frame: WorldTransform;
  if !IsDefined(r) || serial == 0u || r.serial != serial || !IsDefined(r.slots) || !IsDefined(r.target) || !r.target.IsAttached()
    || !IsDefined(r.owner) || !r.owner.VRBladeAllowed(this) || ArraySize(r.capsule) != 2 || !r.slots.GetSlotTransform(r.slot, frame)
    || Vector4.Distance(a,b) > this.VRMeleeReach() + 0.02 { return Vector4(0,0,0,0); };
  let inverse = Quaternion.Conjugate(WorldTransform.GetOrientation(frame));
  return VRBladeCapsuleMetrics(WorldTransform.TransformInvPoint(frame,a), WorldTransform.TransformInvPoint(frame,b),
    Quaternion.Transform(inverse,motion), Quaternion.Transform(inverse,this.GetWorldForward()), r.capsule[0], r.capsule[1], r.material);
}

@addMethod(WeaponObject)
private func VRBladeReceiptInterval(a: Vector4, b: Vector4, edge: Vector4) -> Vector4 {
  let r=this.vrBladeReceipt;
  let frame: WorldTransform;
  if !IsDefined(r) || ArraySize(r.capsule)!=2 || !IsDefined(r.slots) || !r.slots.GetSlotTransform(r.slot,frame) { return Vector4(0,0,0,0); };
  return VRBladeCapsuleInterval(WorldTransform.TransformInvPoint(frame,a),WorldTransform.TransformInvPoint(frame,b),
    Vector4(0,0,0,0),Quaternion.Transform(Quaternion.Conjugate(WorldTransform.GetOrientation(frame)),edge),
    r.capsule[0],r.capsule[1],r.material);
}

@addMethod(WeaponObject)
public func VRBladeOverlapPoint(a: Vector4, b: Vector4, edge: Vector4) -> Vector4 {
  let interval=this.VRBladeReceiptInterval(a,b,edge);
  if interval.W<0.5 || interval.Y<=0.0 { return Vector4(0,0,0,0); };
  let point=a+(b-a)*interval.X; point.W=1.0; return point;
}

// Attachment follows the occupied shaft on this puppet. Damage, follow-through
// and marks continue to use the shape selected by the original hit event.
@addMethod(WeaponObject)
public func VRBladeSeatInterval(serial: Uint32, a: Vector4, b: Vector4, edge: Vector4) -> Vector4 {
  let r=this.vrBladeReceipt;
  if !IsDefined(r) || r.serial!=serial || r.released || !IsDefined(r.target) || !r.target.IsAttached()
    || !IsDefined(r.owner) || !r.owner.VRBladeAllowed(this) || Vector4.Distance(a,b)>this.VRMeleeReach()+0.02 { return Vector4(0,0,0,0); };
  let interval=this.VRBladeReceiptInterval(a,b,edge);
  if (r.puncture || r.lodged) && interval.W<0.5 {
    let shapes=r.target.FindComponentByName(n"HitRepresentation") as HitRepresentationComponent;
    let body=VRBladeSweptContact(shapes,a,b,a,b);
    if body.W>0.5 { return Vector4(body.Y,body.Z,0,1); };
  };
  return interval;
}

@addMethod(WeaponObject)
private func VRBladeBindSlot(r: ref<VrBladeReceipt>, component: ref<HitRepresentationComponent>, a: Vector4, b: Vector4, edge: Vector4) -> ref<VrBladeSlotAttachment> {
  let frame: WorldTransform;
  let hit=new VrBladeSlotAttachment();
  hit.slots=component;
  let interval=Vector4(0,0,0,0);
  if component==r.slots && ArraySize(r.capsule)==2 && component.GetSlotTransform(r.slot,frame) {
    interval=VRBladeCapsuleInterval(WorldTransform.TransformInvPoint(frame,a),WorldTransform.TransformInvPoint(frame,b),
      Vector4(0,0,0,0),Quaternion.Transform(Quaternion.Conjugate(WorldTransform.GetOrientation(frame)),edge),
      r.capsule[0],r.capsule[1],r.material);
  };
  if interval.W>0.5 {
    hit.shape=r.shapeName;hit.slot=r.slot;hit.capsule=r.capsule;hit.fraction=interval.X;hit.depth=interval.Y;
  } else {
    let pose=new VrBladeVisiblePose();
    VRBladeCollectOccupied(component,a,b,edge,n"None",frame,pose);
    for obs in pose.shapes {
      if obs.interval.Y>hit.depth {
        hit.shape=obs.name;hit.slot=obs.slot;hit.capsule=obs.capsule;
        hit.fraction=obs.interval.X;hit.depth=obs.interval.Y;frame=obs.frame;
      };
    };
  };
  if hit.depth<=0.0 { return null; };
  hit.localPoint=WorldTransform.TransformInvPoint(frame,a+(b-a)*hit.fraction);hit.localPoint.W=1.0;
  return hit;
}

@addMethod(WeaponObject)
public func VRBladeFollowSlot(side: Int32, serial: Uint32, a: Vector4, b: Vector4, edge: Vector4, opt trackedGrip: Vector4) -> Vector4 {
  let r=this.vrBladeReceipt;
  let frame: WorldTransform;
  if !IsDefined(r) || serial==0u || serial!=r.serial || !r.lodged || r.released || !IsDefined(r.target) || !r.target.IsAttached()
    || !IsDefined(r.owner) || !r.owner.VRBladeAllowed(this)
    || Vector4.Distance(a,this.GetWorldPosition())>0.05 || Vector4.Distance(a,b)>this.VRMeleeReach()+0.02 { return Vector4(0,0,0,0); };
  let component=r.target.FindComponentByName(n"HitRepresentation") as HitRepresentationComponent;
  if IsDefined(r.attachment) && (!IsDefined(component) || r.attachment.slots!=component) { r.attachment=null; };
  if !IsDefined(component) { return Vector4(0,0,0,0); };
  let pin=r.attachment;
  if !IsDefined(pin) { pin=this.VRBladeBindSlot(r,component,a,b,edge); };
  if !IsDefined(pin) { return Vector4(0,0,0,0); };
  pin.depth=0.0;
  if !component.GetSlotTransform(pin.slot,frame) { return Vector4(0,0,0,0); };
  let interval=VRBladeCapsuleInterval(WorldTransform.TransformInvPoint(frame,a),WorldTransform.TransformInvPoint(frame,b),
    Vector4(0,0,0,0),Quaternion.Transform(Quaternion.Conjugate(WorldTransform.GetOrientation(frame)),edge),
    pin.capsule[0],pin.capsule[1],r.material);
  if interval.W>0.5 { pin.depth=interval.Y; };
  let station=a+(b-a)*pin.fraction;
  let animated=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,pin.localPoint));
  let bind=!IsDefined(r.attachment);
  if bind && pin.depth<=0.0 { return Vector4(0,0,0,0); };
  let goal=VRBladeAnimatedAttachment(side,r.jointSerial,animated,WorldTransform.GetOrientation(frame),station,bind);
  if goal.W>0.5 { r.attachment=pin; };
  if goal.W>2.5 && pin.depth>0.001 && trackedGrip.W>0.0 {
    if pin.SlideFeedback(WorldTransform.TransformInvPoint(frame,station),trackedGrip,VRMeleeRealTime()) {
      this.vrBladeSlideTicks+=1u;
      VRHapticEvent(16,side==0 ? 1 : 2);
    };
  } else { pin.slideTracking=false; };
  if goal.W>2.5 {
    this.VRBladeRecordEdgePose(r,pin,frame,a,b,edge,goal,side);
  } else { ArrayClear(pin.edgeFrames); };
  return goal;
}

@addMethod(WeaponObject)
public func VRBladeUpdateJoint(side: Int32, serial: Uint32, a: Vector4, b: Vector4, edge: Vector4) -> Bool {
  let r = this.vrBladeReceipt;
  let trace: TraceResult;
  let anchor: Vector4;
  let entry: Vector4;
  if !IsDefined(r) || r.serial != serial || r.released || !IsDefined(r.target) || !r.target.IsAttached() || !IsDefined(r.owner)
    || !r.owner.VRBladeAllowed(this) || Vector4.Distance(a,this.GetWorldPosition()) > 0.05
    || Vector4.Distance(a,b)>this.VRMeleeReach()+0.02 { VRBladeClearJoint(side); return false; };
  let interval=this.VRBladeSeatInterval(serial,a,b,edge);
  if interval.W<0.5 { return false; };
  let query = GameInstance.GetSpatialQueriesSystem(this.GetGame());
  let axis=Vector4.Normalize(b-a);
  let middle=a+(b-a)*interval.X;
  let now=VRMeleeRealTime();
  // An authored damage capsule and the puppet's PhysX shapes can have
  // different extents. Search inside the measured shaft interval.
  if r.jointProbeUntil==0.0 { r.jointProbeUntil=now+0.15; };
  if now>=r.jointProbeUntil {
    r.jointProbeIndex=(r.jointProbeIndex+1)%6;
    r.jointProbeUntil=now+0.15;
  };
  let section=r.jointProbeIndex%3;
  let offset=section==1 ? -0.4 : (section==2 ? 0.4 : 0.0);
  anchor=middle+axis*(offset*interval.Y); anchor.W=1.0;
  let start=anchor-axis*0.09;
  let end=anchor+axis*0.09;
  let preset: CName;
  let index=0;
  if !r.requestedBodies && !r.spent && !r.target.IsRagdollEnabled() && r.target.CanRagdoll() && VrBladeRPG.SpecialTarget(r.owner,r.target)
    && !StatusEffectSystem.ObjectHasStatusEffectWithTag(r.target,n"DisableRagdoll") {
    r.requestedBodies=true;
    r.target.QueueEvent(CreateRagdollActivationRequestEvent(entragdollActivationRequestType.Animated,n"None",false,false,n"VRBladeContact"));
  };
  // Give an asynchronous trace request a solver step to bind. On alternate
  // windows a proxy that never bound cannot starve the owned-body lookup.
  while index<3 && r.jointProbeIndex%2==0 {
    preset=index==0 ? n"Melee" : (index==1 ? n"Sight Blocker" : n"Bullet");
    if query.SyncRaycastByQueryPreset(start,end,preset,trace,false,true) && TraceResult.GetHitEntity(trace)==r.target {
      r.jointTraceKind=VRBladeTraceKind(trace);
      if r.jointTraceKind==12 || (r.jointTraceKind==1 && (r.puncture || r.lodged || r.target.IsRagdolling())) {
        entry=Vector4(trace.position.X,trace.position.Y,trace.position.Z,1.0);
        let onBlade=middle+axis*ClampF(Vector4.Dot(entry-middle,axis),-0.5*interval.Y,0.5*interval.Y);
        if Vector4.Distance(entry,onBlade)<=0.035 { return VRBladeSubmitJoint(side,r.jointSerial,trace,onBlade,axis); };
      };
    };
    index+=1;
  };
  // During a reaction or death transition the actor may already exist
  // before IsRagdollEnabled updates. The native lookup verifies its owner.
  return VRBladeFindPuppetJoint(r.target,side,r.jointSerial,anchor,axis);
}
