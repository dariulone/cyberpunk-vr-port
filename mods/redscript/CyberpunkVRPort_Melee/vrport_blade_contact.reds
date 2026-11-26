// Handheld blade contact only. No aim target, named body regions or simulated wounds.
// Values on a freshly created attack identify this one native effect, even after an equip change.
public native func VRBladeReadCapsule(component: ref<HitRepresentationComponent>, name: CName, data: ref<HitShapeUserData>) -> array<Vector4>;
public native func VRBladeSweptContact(component: ref<HitRepresentationComponent>, oldA: Vector4, oldB: Vector4, a: Vector4, b: Vector4) -> Vector4;
// W=1 for a crossing, W=2 when the same slot observations also prove a clear end pose.
public native func VRBladeEntrySweep(component: ref<HitRepresentationComponent>, oldA: Vector4, oldB: Vector4, a: Vector4, b: Vector4, endGrip: Vector4) -> Vector4;
public native func VRBladeCapsuleMetrics(start: Vector4, end: Vector4, motion: Vector4, edge: Vector4, center: Vector4, axis: Vector4, material: Int32) -> Vector4;
public native func VRBladeCapsuleInterval(start: Vector4, end: Vector4, motion: Vector4, edge: Vector4, center: Vector4, axis: Vector4, material: Int32) -> Vector4;
public native func VRBladeSubmitJoint(side: Int32, serial: Uint32, trace: TraceResult, point: Vector4, edge: Vector4) -> Bool;
public native func VRBladeClearJoint(side: Int32) -> Void;
public native func VRBladeJointPoint(side: Int32, serial: Uint32) -> Vector4;
public native func VRBladeTraceKind(trace: TraceResult) -> Int32;
public native func VRBladeFindPuppetJoint(target: ref<Entity>, side: Int32, serial: Uint32, point: Vector4, edge: Vector4) -> Bool;
public class VrBladeEntry extends IScriptable {
  public let claimed: Bool;
  public let expired: Bool;
  public let endedClear: Bool;
  public let priorSerial: Uint32;
}
public class VrBladeImpact extends IScriptable {
  public let entry: ref<VrBladeEntry>;
  public let thrust: Bool;
  public let attackEdge: Vector4;
  public let bleedOnHit: Bool;
  public let physicalBefore: Float;
  public let physicalAfter: Float;
  public let previousB: Vector4;
  public let previousA: Vector4;
  public let sweepFrom: Vector4;
  public let sweepTo: Vector4;
  public let endA: Vector4;
  public let endB: Vector4;
  public let target: EntityID;
  public let a: Vector4;
  public let b: Vector4;
  public let claimed: Bool;
  public let cancelled: Bool;
  public let created: Float;
  public let events: Int32;
  public let lastShapes: Int32;
  public let lastDistance: Float;
  public let cutProof: Vector4;
  public let noDismember: Bool;
  public let hasNeckSlot: Bool;
  public let contactMetrics: Vector4;
  public let swing: wref<VrCyberwareSwing>;
}
@addField(Attack_GameEffect) public let vrMeleeContactTarget: EntityID;
@addField(Attack_GameEffect) private let vrMeleeContactClaimed: Bool;
@addField(Attack_GameEffect) public let vrBladeImpact: ref<VrBladeImpact>;
@addField(WeaponObject) private let vrBladeEnd: Vector4;
@addField(WeaponObject) private let vrBladeMeasured: Bool;
@addField(WeaponObject) private let vrBladeProbeAttempts: Int32;
@addField(WeaponObject) private let vrBladeRetryAt: Float;
@addField(WeaponObject) private let vrBladeFamily: Int32;
@addField(WeaponObject) private let vrBladeTouch: wref<NPCPuppet>;
@addField(WeaponObject) private let vrLastBladeProbe: ref<VrBladeImpact>;
@addField(WeaponObject) private let vrBladeAttempt: ref<VrBladeImpact>;
// Last active query: time along sweep, point along blade, capsule depth, stage
// (0 clear, 1 NPC proxy, 2 measured overlap, 3 effect dispatched).
@addField(WeaponObject) private let vrBladeSweepSample: Vector4;
@addMethod(WeaponObject)
public func VRBladeLastSweep() -> Vector4 { return this.vrBladeSweepSample; }

@addMethod(WeaponObject)
public func VRBladeKind() -> Int32 {
  let record: ref<WeaponItem_Record>;
  if this.vrBladeFamily == 0 {
    record = this.GetWeaponRecord();
    if !IsDefined(record) || !IsDefined(record.ItemType()) { return -1; };
    switch record.ItemType().Type() {
      case gamedataItemType.Wea_Knife:
      case gamedataItemType.Wea_ShortBlade:
        this.vrBladeFamily = 1; break;
      case gamedataItemType.Wea_Katana:
      case gamedataItemType.Wea_Sword:
      case gamedataItemType.Wea_LongBlade:
      case gamedataItemType.Wea_Machete:
      case gamedataItemType.Wea_Chainsword:
      case gamedataItemType.Wea_Axe:
        this.vrBladeFamily = 2; break;
      default: this.vrBladeFamily = -1;
    };
  };
  return this.vrBladeFamily;
}

@addMethod(WeaponObject)
public func VRBladeLocalEnd() -> Vector4 {
  let slots: ref<SlotComponent>;
  let marker: WorldTransform;
  let p: Vector4;
  let size: Float;
  let now: Float;
  if !this.vrBladeMeasured && this.vrBladeProbeAttempts < 16 {
    now = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
    if this.vrBladeProbeAttempts > 0 && now < this.vrBladeRetryAt { return this.vrBladeEnd; };
    // A freshly drawn entity can have its slot component before the attachment pose is ready.
    // Retry for the first few seconds, not for the entire life of an unmarked item.
    this.vrBladeProbeAttempts += 1; this.vrBladeRetryAt = now + 0.20;
    slots = this.FindComponentByName(n"item_slot_attachments") as SlotComponent;
    if !IsDefined(slots) { slots = this.FindComponentByName(n"item_slots_attachments") as SlotComponent; };
    if IsDefined(slots) && slots.GetSlotTransform(n"Impact", marker) {
      p = WorldTransform.TransformInvWorldPosition(this.GetWorldTransform(), WorldTransform.GetWorldPosition(marker));
      p.W = 0.00;
      size = Vector4.Length(p);
      if p.Z > 0.04 && size <= 1.25 { this.vrBladeEnd = p; this.vrBladeMeasured = true; };
    };
    // An unmarked asset still has a small local contact segment, never the PSM lunge range.
    if !this.vrBladeMeasured { this.vrBladeEnd = Vector4(0.00, 0.00, this.VRBladeKind() == 1 ? 0.16 : 0.35, 0.00); };
  };
  return this.vrBladeEnd;
}

@addMethod(WeaponObject)
public func VRBladeNoDismember() -> Bool {
  let span = Vector4.Length(this.VRBladeLocalEnd());
  return this.VRBladeKind() == 1 || !this.vrBladeMeasured || span < 0.45;
}

@addMethod(PlayerPuppet)
public func VRBladeAllowed(weapon: wref<WeaponObject>) -> Bool {
  let bb = this.GetPlayerStateMachineBlackboard();
  let def = GetAllBlackboardDefs().PlayerStateMachine;
  let state: Int32;
  if !IsDefined(bb) || this.IsDead() || !IsDefined(weapon) || weapon.vrPhysicalThrowHidden
    || GameInstance.GetTransactionSystem(this.GetGame()).GetItemInSlot(this, VRLeftHandedWeapon.HeldSlot(this)) != weapon { return false; };
  state = bb.GetInt(def.MeleeWeapon);
  return !bb.GetBool(def.SceneSafeForced) && !EntityID.IsDefined(bb.GetEntityID(def.FinisherTarget))
    && !StatusEffectSystem.ObjectHasStatusEffectWithTag(this, n"NoCombat")
    && !StatusEffectSystem.ObjectHasStatusEffectWithTag(this, n"NoWeapons")
    && !StatusEffectSystem.ObjectHasStatusEffectWithTag(this, n"VehicleScene")
    && !StatusEffectSystem.ObjectHasStatusEffectOfType(this, gamedataStatusEffectType.Stunned)
    && (state == EnumInt(gamePSMMeleeWeapon.Idle) || state == EnumInt(gamePSMMeleeWeapon.PublicSafe)
      || state == EnumInt(gamePSMMeleeWeapon.Hold) || state == EnumInt(gamePSMMeleeWeapon.ChargedHold)
      || state == EnumInt(gamePSMMeleeWeapon.Block) || state == EnumInt(gamePSMMeleeWeapon.Deflect)
      || state == EnumInt(gamePSMMeleeWeapon.Targeting));
}

@addMethod(PlayerPuppet)
public func VRBladeSweep(weapon: wref<WeaponObject>, oldA: Vector4, oldB: Vector4, a: Vector4, b: Vector4, passive: Bool, opt beginOnContact: Bool, opt trackedMotion: Vector4, opt strong: Bool, opt speed: Float) -> Int32 {
  let npc: ref<NPCPuppet>;
  let last = weapon.vrBladeTouch;
  let previous = weapon.vrBladeAttempt;
  let now = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  let p: Vector4;
  let q: Vector4;
  let point: Vector4;
  let direction: Vector4;
  let impact: ref<VrBladeImpact>;
  let trace: TraceResult;
  let t: Float;
  let sq = GameInstance.GetSpatialQueriesSystem(this.GetGame());
  if weapon.VRBladeEntryBlocked() { return 0; };
  let curStart=a+(b-a)*0.10;
  if !passive { weapon.vrBladeSweepSample=Vector4(0,0,0,0); };
  // A retained crossing has a weak touch. Despawn can clear it between
  // samples, or leave a detached puppet until the last handle is dropped.
  if IsDefined(previous) && IsDefined(previous.entry) && !previous.entry.endedClear
    && (!IsDefined(last) || !last.IsAttached()) {
    previous.entry.expired=true; weapon.vrBladeAttempt=null; weapon.vrBladeTouch=null;
    previous=null; last=null;
  };
  if IsDefined(previous) && IsDefined(previous.entry) && !previous.entry.claimed {
    // A delayed effect must not eat the intervening physical frames. Probes
    // from one crossing share a single claim in the damage callback.
    last=null;
    if now<previous.created || now-previous.created>0.12 {
      previous.entry.expired=true;weapon.vrBladeAttempt=null;weapon.vrBladeTouch=null;previous=null;
    };
  };
  if IsDefined(last) {
    // A reversal is not a new impact while the blade remains at the same moving contact.
    let present=VRBladeSweptContact(last.FindComponentByName(n"HitRepresentation") as HitRepresentationComponent,a,b,a,b);
    if present.W<0.5 { weapon.vrBladeTouch=null;weapon.vrBladeAttempt=null; };
    return 0;
  };
  if passive { return 0; };
  if !this.VRBladeAllowed(weapon) { weapon.vrBladeTouch = null; return 0; };
  if Vector4.Distance(a,oldA)<0.001 && Vector4.Distance(b,oldB)<0.001 { return 0; };
  let prevStart=oldA+(oldB-oldA)*0.10;
  if IsDefined(sq) && VrCyberwareContactMath.SweptTrace(sq,prevStart,oldB,curStart,b,passive,trace,t) {
    npc = TraceResult.GetHitEntity(trace) as NPCPuppet;
    if IsDefined(npc) && npc.IsActive() && !npc.IsDead() && !ScriptedPuppet.IsDefeated(npc)
      && !GameObject.IsFriendlyTowardsPlayer(npc) {
      weapon.vrBladeSweepSample=Vector4(t,0,0,1);
      let overlap=VRBladeEntrySweep(npc.FindComponentByName(n"HitRepresentation") as HitRepresentationComponent,prevStart,oldB,curStart,b,a);
      if overlap.W<0.5 { return 0; };
      weapon.vrBladeSweepSample=Vector4(overlap.X,overlap.Y,overlap.Z,2);
      p=prevStart+(curStart-prevStart)*overlap.X; q=oldB+(b-oldB)*overlap.X;
      point=p+(q-p)*overlap.Y;
      let separated=IsDefined(previous) && IsDefined(previous.entry) && previous.entry.endedClear;
      // The newest probe may lose to a sibling. Only their shared entry can
      // still reserve this crossing or permit another probe to retry it.
      let pending=IsDefined(previous) && IsDefined(previous.entry) && !previous.entry.claimed && !previous.entry.expired;
      let retry=pending && previous.target==npc.GetEntityID() && !separated;
      if pending && !retry && !separated { return 0; };
      if beginOnContact && !retry && !this.VRCyberwareBeginSwing(weapon,1,strong,speed) { return 0; };
      direction = Vector4(-trace.normal.X, -trace.normal.Y, -trace.normal.Z, 0.00);
      if Vector4.Length(direction) < 0.50 { direction = Vector4.Normalize(q - p); };
      impact = new VrBladeImpact();
      if retry { impact.entry=previous.entry; } else {
        impact.entry=new VrBladeEntry(); impact.entry.priorSerial=weapon.VRBladeConfirmedSerial();
      };
      impact.target = npc.GetEntityID(); impact.a = p; impact.b = q;
      impact.attackEdge=weapon.GetWorldForward();impact.attackEdge.W=0.0;
      impact.previousA=prevStart;impact.previousB=oldB;
      impact.endA=curStart;impact.endB=b;
      impact.sweepFrom=prevStart+(oldB-prevStart)*overlap.Y;
      impact.sweepTo=curStart+(b-curStart)*overlap.Y;
      let axis=Vector4.Normalize(q-p);
      let tipMotion=b-oldB; tipMotion.W=0.0;
      let handMotion=a-oldA; handMotion.W=0.0;
      let forward=Vector4.Dot(tipMotion,axis);
      impact.thrust=forward>0.005 && forward>=0.75*Vector4.Length(tipMotion) && Vector4.Dot(handMotion,axis)>0.0;
      // The on-screen prop can stop or deflect on a solver frame. Use the
      // sampled controller translation to classify a confirmed contact.
      // The actual prop and native hit shape still decide where it landed.
      trackedMotion.W=0.0;
      let trackedLength=Vector4.Length(trackedMotion);
      if trackedLength>=0.018 && trackedLength<=1.5 {
        impact.thrust=Vector4.Dot(trackedMotion,axis)>=0.75*trackedLength;
      };
      impact.created = now; impact.noDismember = weapon.VRBladeNoDismember();
      impact.stealthIntent = impact.thrust || GetVRSharedSlot(VRLeftHandedWeapon.InHand(this) ? 154 : 160) > 0.50;
      if impact.stealthIntent {
        let hitSlots = npc.FindComponentByName(n"HitRepresentation") as SlotComponent;
        let neckFrame: WorldTransform;
        impact.hasNeckSlot = npc.vrBladeNeckConfigured || (IsDefined(hitSlots) && hitSlots.GetSlotTransform(n"Neck",neckFrame));
        impact.portAtStart=npc.VRBladeServicePort(); impact.rearAtStart=VrBladeRPG.Behind(this,npc);
      };
      let priorTouch=weapon.vrBladeTouch;
      impact.entry.endedClear=overlap.W>1.5;
      weapon.vrBladeAttempt=impact;weapon.vrBladeTouch=impact.entry.endedClear ? null : npc;
      if this.VRCyberwareOrdinaryContact(weapon, npc, point, point + direction * 0.12, false, impact) > 0 {
        weapon.vrLastBladeProbe = impact;
        weapon.vrBladeSweepSample=Vector4(overlap.X,overlap.Y,overlap.Z,3);
        return 1;
      };
      impact.cancelled=true; weapon.vrBladeAttempt=previous;
      weapon.vrBladeTouch=retry && impact.entry.endedClear ? null : priorTouch;
    };
  };
  if !IsDefined(previous) { weapon.vrBladeTouch = null; };
  return 0;
}

@addMethod(WeaponObject)
private func VRBladeCanClaimEntry(impact: ref<VrBladeImpact>, player: wref<PlayerPuppet>) -> Bool {
  let current=this.vrBladeAttempt;
  if !IsDefined(current) || !IsDefined(current.entry) || !IsDefined(impact.entry) || this.VRBladeEntryBlocked() { return false; };
  if current.entry==impact.entry { return true; };
  // A second crossing may be queued before the native callback of a completed
  // pass-through. Its predecessor can still land in order during the probe window.
  if !impact.entry.endedClear || current.entry.claimed || impact.entry.priorSerial!=this.VRBladeConfirmedSerial()
    || !IsDefined(player) || !player.VRBladeAllowed(this) { return false; };
  let age=EngineTime.ToFloat(GameInstance.GetSimTime(player.GetGame()))-impact.created;
  return age>=0.0 && age<=0.12;
}

// Even if two NPCs overlap or an effect ticks again, only its traced NPC may enter the
// real damage pipeline once. All vanilla block, parry, dodge and immunity logic runs inside it.
@wrapMethod(DamageSystem)
private final func ProcessPipeline(hitEvent: ref<gameHitEvent>, cache: ref<CacheData>) -> Void {
  let blade: ref<Attack_GameEffect>;
  let impact: ref<VrBladeImpact>;
  let point: Vector4;
  let shapes: array<HitShapeData>;
  let selected: HitShapeData;
  let nearest: Float;
  let index: Int32;
  let i: Int32;
  let gap: Float;
  let followGap: Float;
  let chosenCut: ref<VrBladeCutShape>;
  let overlap: Vector4;
  if IsDefined(hitEvent.attackData) && AttackData.IsMelee(hitEvent.attackData.GetAttackType()) {
    blade = hitEvent.attackData.GetAttackDefinition() as Attack_GameEffect;
    if IsDefined(blade) { impact = blade.vrBladeImpact; };
  };
  if IsDefined(blade) && EntityID.IsDefined(blade.vrMeleeContactTarget) {
    if hitEvent.projectionPipeline || !IsDefined(hitEvent.target) || hitEvent.target.GetEntityID()!=blade.vrMeleeContactTarget { return; };
    if !IsDefined(impact) {
      if blade.vrMeleeContactClaimed || ArraySize(hitEvent.hitRepresentationResult.hitShapes)==0 { return; };
      blade.vrMeleeContactClaimed=true;
    };
  };
  if IsDefined(impact) {
    impact.events += 1;
    impact.lastShapes = ArraySize(hitEvent.hitRepresentationResult.hitShapes);
    if impact.cancelled || impact.claimed || (IsDefined(impact.entry) && (impact.entry.claimed || impact.entry.expired)) || !IsDefined(hitEvent.target) || hitEvent.target.GetEntityID() != impact.target
      || ArraySize(hitEvent.hitRepresentationResult.hitShapes) == 0 { return; };
    if impact.parentSerial==0u {
      let weapon=hitEvent.attackData.GetWeapon();
      if !IsDefined(weapon) || !weapon.VRBladeCanClaimEntry(impact,hitEvent.attackData.GetInstigator() as PlayerPuppet) { return; };
    };
    // The physics trace may only have touched a broad NPC proxy. Require the actual native
    // hit shape, at the blade, before accepting damage; never retarget it to a proxy or a bone.
    // The native sweep returns several surfaces in sweep order, not distance to the
    // blade. Choose among the reported surfaces, keeping all native zone and material data.
    shapes = hitEvent.hitRepresentationResult.hitShapes;
    if impact.parentSerial != 0u && impact.edgeStroke==0u {
      let weapon=hitEvent.attackData.GetWeapon();
      if !IsDefined(weapon) || weapon.VRBladeConfirmedSerial()!=impact.parentSerial { return; };
      overlap=weapon.VRBladeOverlapPoint(impact.a,impact.b,weapon.GetWorldForward());
      if overlap.W<0.50 { return; };
      // An embedded prop can be entirely inside the selected collider. The
      // sweep entry lies outside the prop in that case. Compare its overlap at
      // request and callback, then require the exact native shape and owner.
      followGap=Vector4.Distance(overlap,impact.overlapPoint);
    };
    nearest = 100000.00;
    if impact.edgeStroke!=0u { impact.cutProof=Vector4(Cast<Float>(ArraySize(impact.cutCandidates)),0,0,0); };
    while i < ArraySize(shapes) {
      let rowCut: ref<VrBladeCutShape>;
      point = shapes[i].result.hitPositionEnter; point.W = 1.00;
      gap = Vector4.Distance(point, VrCyberwareContactMath.Closest(point, impact.a, impact.b));
      if impact.sweepFrom.W>0.5 {
        gap=MinF(gap,Vector4.Distance(point,VrCyberwareContactMath.Closest(point,impact.sweepFrom,impact.sweepTo)));
      };
      if impact.parentSerial != 0u && impact.edgeStroke==0u { gap = followGap; };
      if impact.edgeStroke!=0u {
        let weapon=hitEvent.attackData.GetWeapon();
        if !IsDefined(weapon) { return; };
        gap=100000.0;
        for candidate in impact.cutCandidates {
          if (candidate.eitherEnd || (impact.cutShape.reverseQuery ? candidate.reverseQuery : !candidate.reverseQuery))
            && Equals(candidate.name,shapes[i].hitShapeName) {
            impact.cutProof.Y+=1.0;
            let distance=weapon.VRBladeCutGap(candidate,shapes[i]);
            if distance<gap { gap=distance;rowCut=candidate; };
          };
        };
        if gap<100000.0 { impact.cutProof.Z+=1.0; };
      };
      // A sustained contact belongs to its existing animated slot. An adjacent
      // shape nearer to the segment must not mask a valid hit on the original slot.
      if gap < nearest && (impact.parentSerial == 0u || (VrBladeRPG.ValidZone(shapes[i]) && VrBladeRPG.FollowMatches(hitEvent, impact, shapes[i]))) {
        nearest = gap; index = i;
        if impact.edgeStroke!=0u && gap<=0.06 {
          // The first proven surface in stock order retains all preceding protection layers.
          chosenCut=rowCut;impact.cutProof.W=1.0;break;
        };
      };
      i += 1;
    };
    impact.lastDistance = nearest;
    if nearest > 0.06 { return; };
    selected = shapes[index];
    if !IsDefined(hitEvent.target as NPCPuppet) || hitEvent.projectionPipeline { return; };
    // Do not discard an intervening native protection layer when reducing the sweep to
    // a single local hit. The untouched shape array is ordered along the effect sweep.
    i = 0;
    while i <= index {
      if DamageSystemHelper.IsProtectionLayer(shapes[i]) || IsDefined(shapes[i].userData as HitData_Base)
        && Equals((shapes[i].userData as HitData_Base).m_hitShapeType, HitShape_Type.ProtectionLayer) { return; };
      i += 1;
    };
    if !VrBladeRPG.ValidZone(selected) { return; };
    if impact.parentSerial != 0u {
      if !VrBladeRPG.FollowMatches(hitEvent, impact, selected) { return; };
      if impact.edgeStroke!=0u {
        let frame: WorldTransform;
        let r=hitEvent.attackData.GetWeapon().vrBladeReceipt;
        if !IsDefined(chosenCut) || !r.slots.GetSlotTransform(chosenCut.slot,frame) { return; };
        VrBladeVisibleCut.UseCut(impact,chosenCut);
        impact.cutSlot=impact.cutShape.slot;
        impact.cutPoint=impact.cutShape.to;impact.cutPoint.W=1.0;
        impact.overlapPoint=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,impact.cutPoint));
        impact.overlapPoint.W=1.0;
      };
      hitEvent.attackData.GetWeapon().vrBladeReceipt.spent = true;
    };
    ArrayClear(hitEvent.hitRepresentationResult.hitShapes);
    ArrayPush(hitEvent.hitRepresentationResult.hitShapes, selected);
    impact.acceptedShape = selected.hitShapeName;
    impact.claimed = true; // shape reserved, NOT a successful hit or combo step
    if IsDefined(impact.entry) { impact.entry.claimed=true; };
  };
  wrappedMethod(hitEvent, cache);
}

// A small blade may still produce an ordinary native wound. Do not suppress the
// entire wound/reaction routine just to veto its part-removing branches.
@addMethod(HitReactionComponent)
private final func VRLocalBladeImpact() -> ref<VrBladeImpact> {
  let attack: ref<Attack_GameEffect>;
  if IsDefined(this.m_attackData) { attack = this.m_attackData.GetAttackDefinition() as Attack_GameEffect; };
  return IsDefined(attack) ? attack.vrBladeImpact : null;
}

@wrapMethod(HitReactionComponent)
protected final func DismembermentConditions() -> Bool {
  let impact = this.VRLocalBladeImpact();
  if IsDefined(impact) {
    if impact.noDismember { return false; };
    // Let the stock reaction select its pre-authored part and VFX. Override
    // its damage threshold only on the paid, matching local continuation.
    if impact.parentSerial != 0u {
      if !impact.strengthReaction || !impact.published || impact.target != this.GetOwner().GetEntityID()
        || !IsNameValid(impact.acceptedShape) || NotEquals(this.m_hitShapeData.hitShapeName,impact.acceptedShape) { return false; };
      return true;
    };
  };
  return wrappedMethod();
}

@wrapMethod(HitReactionComponent)
protected final func WoundedCyberConditions(dismembermentCheck: Bool, woundedBaseConditions: Bool) -> Bool {
  // Despite its name this native branch detaches the cybernetic part.
  let impact = this.VRLocalBladeImpact();
  if IsDefined(impact) && (impact.noDismember || (impact.parentSerial != 0u && !dismembermentCheck)) { return false; };
  return wrappedMethod(dismembermentCheck, woundedBaseConditions);
}

@wrapMethod(HitReactionComponent)
protected final func IsValidBodyPerkDismemberAttack(healthMissing: Float) -> Bool {
  // This perk can pick additional random parts. A traced blade hit must
  // leave only the native, local hit-shape route eligible.
  if IsDefined(this.VRLocalBladeImpact()) { return false; };
  return wrappedMethod(healthMissing);
}

@wrapMethod(HitReactionComponent)
private final func NotifyAboutDismembermentInstigated(instigator: wref<GameObject>, const bodyPart: EHitReactionZone, const targetPosition: Vector4) -> Void {
  let impact = this.VRLocalBladeImpact();
  if IsDefined(impact) && impact.parentSerial != 0u && impact.published && impact.target == this.GetOwner().GetEntityID()
    && Equals(this.m_hitShapeData.hitShapeName,impact.acceptedShape) { impact.partNotified = true; };
  wrappedMethod(instigator,bodyPart,targetPosition);
}

@addMethod(WeaponObject)
public func VRBladeLastPortCheck() -> Vector4 {
  let p=this.vrLastBladeProbe;
  if !IsDefined(p) { return Vector4(0,0,0,0); };
  return Vector4(p.stealthIntent ? 1.00 : 0.00,p.rearAtStart ? 1.00 : 0.00,p.portMatched ? 1.00 : 0.00,Cast<Float>(p.stealthStage));
}

@addMethod(WeaponObject)
public func VRBladePartNotified() -> Bool { return IsDefined(this.vrBladeReceipt) && IsDefined(this.vrBladeReceipt.impact) && this.vrBladeReceipt.impact.partNotified; }

@addMethod(WeaponObject)
public func VRBladeNoteProbe(p: ref<VrBladeImpact>) -> Void { this.vrLastBladeProbe = p; }

// Clipped candidates, compatible name comparisons, identity-matched callback
// rows and whether a row passed the distance and receipt gates before protection.
@addMethod(WeaponObject)
public func VRBladeLastCutProof() -> Vector4 {
  return IsDefined(this.vrLastBladeProbe) ? this.vrLastBladeProbe.cutProof : Vector4(0,0,0,0);
}

@addMethod(WeaponObject)
public func VRBladeLastProbe() -> Vector4 {
  let p = this.vrLastBladeProbe;
  if !IsDefined(p) { return Vector4(-1.00, 0.00, 0.00, 0.00); };
  return Vector4(Cast<Float>(p.events),Cast<Float>(p.lastShapes),p.lastDistance,p.claimed ? 1.00 : 0.00);
}
