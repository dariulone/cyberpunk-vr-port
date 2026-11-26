// Collision follows the final visible arm/line geometry. Native weapon attack
// records own damage, elemental effects, perks, reactions and stamina costs.
public class VrCyberwareSwing extends IScriptable {
  public let weapon: wref<WeaponObject>;
  public let record: wref<Attack_Melee_Record>;
  public let targets: array<EntityID>;
  public let strong: Bool;
  public let number: Int32;
  public let peakSpeed: Float;
  public let chain: Bool;    // a light stroke of the combo chain (VRMeleeChainAttack)
  public let landed: Bool;   // its attack started on a body: the chain moved on once
}
public class VrCyberwareAttackTicket extends IScriptable {
  public let attack: ref<IAttack>;
  public let time: Float;
  public let targets: array<EntityID>;
  public let intendedTarget: EntityID;
  public let weapon: ItemID;
}
@addField(PlayerPuppet) private let vrCyberwareSwings: array<ref<VrCyberwareSwing>>;
@addField(PlayerPuppet) private let vrCyberwareTickets: array<ref<VrCyberwareAttackTicket>>;
@addField(PlayerPuppet) public let vrCyberwareSwingCount: Int32;
@addField(PlayerPuppet) public let vrCyberwareAttackCount: Int32;
@addField(PlayerPuppet) public let vrCyberwareHitCount: Int32;
@addField(PlayerPuppet) public let vrCyberwareLastNativeDamage: Float;
@addField(PlayerPuppet) public let vrCyberwareLastHitDelay: Float;
@addField(PlayerPuppet) public let vrCyberwareDamageEvents: Int32;
@addField(PlayerPuppet) public let vrCyberwareDamageTrace: String;
// The last started attack's geometry, read by live tests: contact on the line, body point, sweep start/direction/range.
@addField(PlayerPuppet) public let vrCyberwareLastAttackGeometry: array<Vector4>;

@addMethod(PlayerPuppet)
public func VRCyberwareKind(weapon: wref<WeaponObject>) -> Int32 {
  if !IsDefined(weapon) { return 0; };
  if WeaponObject.IsOfType(weapon.GetItemID(), gamedataItemType.Cyb_StrongArms) { return 1; };
  if WeaponObject.IsOfType(weapon.GetItemID(), gamedataItemType.Cyb_MantisBlades) { return 2; };
  if WeaponObject.IsOfType(weapon.GetItemID(), gamedataItemType.Cyb_NanoWires) { return 3; };
  if WeaponObject.IsOfType(weapon.GetItemID(), gamedataItemType.Cyb_Launcher) { return 4; };
  return 0;
}

// THE LIGHT COMBO. The flat game numbers light attacks MeleeComboAttack0, 1, ... and
// makes the last one of AttacksNumber (a weapon stat) MeleeFinalAttack0, the
// FinalAttack subtype that hit reactions and the VR training count as a combo
// finisher (meleeTransitions CheckIfFinalAttack / IncrementAttackNumber).
// There every button press is an attack. A tracked hand also strokes on the way
// back, and the reversal re-arms the swing, so the chain moves on only for a
// stroke that reached a body, and holds through whiffs and retractions. A hit
// within VRMeleeChainWindow() of the previous one continues the chain, as
// pressing attack before the recovery ends does there.
@addField(PlayerPuppet) public let vrMeleeChainNumber: Int32;   // the attack number of the next light stroke
@addField(PlayerPuppet) public let vrMeleeChainTime: Float;     // when the last chained stroke landed
@addField(PlayerPuppet) public let vrMeleeChainWeapon: ItemID;
@addField(PlayerPuppet) public let vrMeleeChainFinals: Int32;   // finishers landed, read by live tests

public func VRMeleeChainWindow() -> Float { return 1.50; }

@addMethod(PlayerPuppet)
public func VRMeleeChainReset() -> Void {
  this.vrMeleeChainNumber = 0;
  this.vrMeleeChainTime = -10.00;
}

@addMethod(PlayerPuppet)
private func VRMeleeChainLength(weapon: wref<WeaponObject>, out infinite: Bool) -> Int32 {
  let stats = GameInstance.GetStatsSystem(this.GetGame());
  let weaponStats = Cast<StatsObjectID>(weapon.GetEntityID());
  infinite = stats.GetStatValue(weaponStats, gamedataStatType.CanWeaponInfinitlyCombo) > 0.00
    && stats.GetStatValue(Cast<StatsObjectID>(this.GetEntityID()), gamedataStatType.CanMeleeInfinitelyCombo) > 0.00;
  return Cast<Int32>(stats.GetStatValue(weaponStats, gamedataStatType.AttacksNumber));
}

@addMethod(PlayerPuppet)
private func VRMeleeChainAttack(weapon: wref<WeaponObject>, out number: Int32) -> ref<Attack_GameEffect> {
  let now = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  let infinite: Bool;
  let attacks = this.VRMeleeChainLength(weapon, infinite);
  let attack: ref<Attack_GameEffect>;
  if this.vrMeleeChainWeapon != weapon.GetItemID() || now < this.vrMeleeChainTime || now - this.vrMeleeChainTime > VRMeleeChainWindow() {
    this.vrMeleeChainNumber = 0;
  };
  number = this.vrMeleeChainNumber;
  if !infinite && attacks > 1 && number + 1 >= attacks {
    attack = weapon.GetAttack(n"MeleeFinalAttack0") as Attack_GameEffect;
  };
  if !IsDefined(attack) { attack = weapon.GetAttack(StringToName("MeleeComboAttack" + IntToString(number))) as Attack_GameEffect; };
  if !IsDefined(attack) { number = 0; attack = weapon.GetAttack(n"MeleeComboAttack0") as Attack_GameEffect; };
  return attack;
}

// A chained stroke reached a body: the next light stroke is the next attack.
@addMethod(PlayerPuppet)
public func VRMeleeChainLanded(swing: ref<VrCyberwareSwing>) -> Void {
  let infinite: Bool;
  let attacks = this.VRMeleeChainLength(swing.weapon, infinite);
  let subtype = swing.record.AttackSubtype();
  let next = swing.number + 1;
  if IsDefined(subtype) && Equals(subtype.Type(), gamedataAttackSubtype.FinalAttack) {
    this.vrMeleeChainFinals += 1;
    next = attacks;
  };
  if next >= attacks { next = infinite ? 1 : 0; };
  this.vrMeleeChainNumber = next;
  this.vrMeleeChainTime = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  this.vrMeleeChainWeapon = swing.weapon.GetItemID();
}

@addMethod(PlayerPuppet)
public func VRCyberwareBeginSwing(weapon: wref<WeaponObject>, side: Int32, strong: Bool, opt speed: Float) -> Bool {
  let attack: ref<Attack_GameEffect>;
  let record: wref<Attack_Melee_Record>;
  let swing: ref<VrCyberwareSwing>;
  let costs: array<wref<StatModifier_Record>>;
  let cost: Float;
  let item: wref<ItemObject>;
  let number: Int32 = side == 0 ? 1 : 0;
  if side < 0 || side > 1 || !IsDefined(weapon) || this.VRHolsterIntentActive(1.00) { return false; };
  if weapon.VRBladeKind() > 0 && !this.VRBladeAllowed(weapon) { return false; };
  item = GameInstance.GetTransactionSystem(this.GetGame()).GetItemInSlot(this, VRLeftHandedWeapon.HeldSlot(this));
  if !IsDefined(item) || item.GetItemID() != weapon.GetItemID() { return false; };
  // Hand-held weapons and fists walk the game's light combo; arm cyberware keeps
  // its per-arm damage records.
  let chained = this.VRCyberwareKind(weapon) == 0 && !strong;
  if this.VRCyberwareKind(weapon) == 0 {
    if strong { this.VRMeleeChainReset(); } else { attack = this.VRMeleeChainAttack(weapon, number); };
  };
  if strong { attack = weapon.GetAttack(StringToName("MeleeStrongAttack" + IntToString(number))) as Attack_GameEffect; };
  if strong && !IsDefined(attack) { attack = weapon.GetAttack(n"MeleeStrongAttack0") as Attack_GameEffect; };
  if !IsDefined(attack) { attack = weapon.GetAttack(StringToName("MeleeComboAttack" + IntToString(number))) as Attack_GameEffect; };
  if !IsDefined(attack) { attack = weapon.GetAttack(n"MeleeComboAttack0") as Attack_GameEffect; };
  if !IsDefined(attack) { attack = weapon.VRGetMeleeAttack(strong); };
  if !IsDefined(attack) { return false; };
  record = attack.GetRecord() as Attack_Melee_Record;
  if !IsDefined(record) { return false; };
  swing = new VrCyberwareSwing();
  swing.weapon = weapon;
  swing.record = record;
  swing.strong = strong;
  swing.number = number;
  swing.chain = chained;
  swing.peakSpeed = ClampF(speed, 0.00, 30.00);
  while ArraySize(this.vrCyberwareSwings) < 2 { ArrayPush(this.vrCyberwareSwings, null); };
  if IsDefined(this.vrCyberwareSwings[side]) && this.vrCyberwareSwings[side].weapon == weapon {
    swing.targets = this.vrCyberwareSwings[side].targets;
  };
  this.vrCyberwareSwings[side] = swing;
  record.StaminaCost(costs);
  cost = RPGManager.CalculateStatModifiers(costs, this.GetGame(), weapon, Cast<StatsObjectID>(weapon.GetEntityID()));
  if cost > 0.00 { PlayerStaminaHelpers.ModifyStamina(this, -cost); };
  this.vrCyberwareSwingCount += 1;
  return true;
}

@addMethod(PlayerPuppet)
public func VRCyberwareUpdateSwing(weapon: wref<WeaponObject>, side: Int32, speed: Float) -> Void {
  let swing: ref<VrCyberwareSwing>;
  if side < 0 || side >= ArraySize(this.vrCyberwareSwings) { return; };
  swing = this.vrCyberwareSwings[side];
  if IsDefined(swing) && swing.weapon == weapon { swing.peakSpeed = MaxF(swing.peakSpeed, ClampF(speed, 0.00, 30.00)); };
}

@addMethod(PlayerPuppet)
public func VRCyberwareOrdinaryContact(weapon: wref<WeaponObject>, target: wref<GameObject>, contact: Vector4, hit: Vector4, passive: Bool, opt impact: ref<VrBladeImpact>) -> Int32 {
  let swing: ref<VrCyberwareSwing>;
  let touched: Bool;
  if ArraySize(this.vrCyberwareSwings) < 2 || !IsDefined(weapon) || this.VRHolsterIntentActive(1.00) { return 0; };
  swing = this.vrCyberwareSwings[1];
  if !IsDefined(swing) || swing.weapon != weapon { return 0; };
  if !IsDefined(target) { return 0; };
  touched = ArrayContains(swing.targets, target.GetEntityID());
  // Ordinary weapons hit a target once per deliberate stroke. A new physical
  // reversal can cut again without first leaving an oversized body capsule.
  if passive || (touched && !IsDefined(impact)) { return 0; };
  if this.VRCyberwareAttack(swing, contact, hit, 1, target.GetEntityID(), 0.175, impact) {
    if !IsDefined(impact) {
      ArrayPush(swing.targets, target.GetEntityID());
      VRHapticEvent(14, VRPortGuardHands(this));
    }; // local blades latch on the native hit and pulse only on a damage event
    return 1;
  };
  return 0;
}

// Closest point on a segment, used for contact location and the swept geometry.
public class VrCyberwareContactMath extends IScriptable {
  public static func Closest(p: Vector4, a: Vector4, b: Vector4) -> Vector4 {
    let ab = b - a;
    let d = Vector4.Dot(ab, ab);
    let t: Float;
    if d > 0.0000001 { t = ClampF(Vector4.Dot(p - a, ab) / d, 0.00, 1.00); };
    return a + ab * t;
  }
  // Each ray follows a material point from its old position to its new
  // position. Work depends on item length even for a fast translation.
  // t locates the virtual prop at the earliest sampled physics contact.
  public static func SweptTrace(sq: ref<SpatialQueriesSystem>, oldA: Vector4, oldB: Vector4, a: Vector4, b: Vector4, passive: Bool, out trace: TraceResult, out t: Float) -> Bool {
    let count = Clamp(CeilF(MaxF(Vector4.Distance(oldA,oldB),Vector4.Distance(a,b)) / 0.04),1,32);
    let i: Int32;
    let hit: TraceResult;
    t=2.0;
    if !passive {
      while i<=count {
        let f=Cast<Float>(i)/Cast<Float>(count);
        let start=oldA+(oldB-oldA)*f;
        let end=a+(b-a)*f;
        let delta=end-start;
        let squared=Vector4.Dot(delta,delta);
        if squared>0.000001 && sq.SyncRaycastByQueryPreset(start,end,n"Melee",hit,false,false) {
          let point=Vector4(hit.position.X,hit.position.Y,hit.position.Z,1.0);
          let when=ClampF(Vector4.Dot(point-start,delta)/squared,0.0,1.0);
          if when<t { t=when;trace=hit; };
        };
        i+=1;
      };
    };
    if t<=1.0 { return true; };
    t=1.0;
    return sq.SyncRaycastByQueryPreset(a,b,n"Melee",trace,false,false);
  }
}

@addMethod(PlayerPuppet)
public func VRCyberwareContacts(weapon: wref<WeaponObject>, side: Int32, points: array<Vector4>, previous: array<Vector4>, radius: Float, opt passive: Bool, opt beginOnContact: Bool, opt strong: Bool, opt speed: Float) -> Int32 {
  let swing: ref<VrCyberwareSwing>;
  let touching: array<EntityID>;
  let retained: array<EntityID>;
  let trace: TraceResult;
  let t: Float;
  let i: Int32;
  let count: Int32;
  let sq=GameInstance.GetSpatialQueriesSystem(this.GetGame());
  if !IsDefined(weapon) || side<0 || side>1 || ArraySize(points)<2 || ArraySize(points)>46 || ArraySize(previous)!=ArraySize(points)
    || this.VRHolsterIntentActive(1.0) || !IsDefined(sq) { return 0; };
  if side<ArraySize(this.vrCyberwareSwings) { swing=this.vrCyberwareSwings[side]; };
  if IsDefined(swing) && swing.weapon!=weapon { swing=null; };
  if passive && (!IsDefined(swing) || ArraySize(swing.targets)==0) { return 0; };
  if GameInstance.GetTransactionSystem(this.GetGame()).GetItemInSlot(this, VRLeftHandedWeapon.HeldSlot(this))!=weapon { return 0; };
  while i+1<ArraySize(points) {
    let oldA=previous[i];let oldB=previous[i+1];
    let a=points[i];let b=points[i+1];
    let moving=Vector4.Distance(oldA,a)>0.001 || Vector4.Distance(oldB,b)>0.001;
    if VrCyberwareContactMath.SweptTrace(sq,oldA,oldB,a,b,passive || !moving,trace,t) {
      let npc=TraceResult.GetHitEntity(trace) as NPCPuppet;
      if IsDefined(npc) && npc.IsActive() && !npc.IsDead() && !GameObject.IsFriendlyTowardsPlayer(npc) {
        let id=npc.GetEntityID();
        let shapes=npc.FindComponentByName(n"HitRepresentation") as HitRepresentationComponent;
        let overlap=VRBladeSweptContact(shapes,oldA,oldB,a,b);
        if overlap.W>0.5 { ArrayPush(touching,id); };
        if overlap.W>0.5 && moving && !passive && (!IsDefined(swing) || !ArrayContains(swing.targets,id)) {
          let from=oldA+(oldB-oldA)*overlap.Y;
          let to=a+(b-a)*overlap.Y;
          if Vector4.Distance(from,to)<0.001 { from=oldA+(a-oldA)*overlap.X; to=oldB+(b-oldB)*overlap.X; };
          if beginOnContact {
            if !this.VRCyberwareBeginSwing(weapon,side,strong,speed) { return count; };
            swing=this.vrCyberwareSwings[side];beginOnContact=false;
          };
          if IsDefined(swing) {
            if this.VRCyberwareAttack(swing,from,to,side,id,ClampF(radius,0.02,0.10)) {
              ArrayPush(swing.targets,id);count+=1;
            };
          };
        };
      };
    };
    i+=1;
  };
  if IsDefined(swing) {
    for id in swing.targets { if ArrayContains(touching,id) { ArrayPush(retained,id); }; };
    swing.targets=retained;
  };
  return count;
}

@addMethod(PlayerPuppet)
private func VRCyberwareAttack(swing: ref<VrCyberwareSwing>, contact: Vector4, target: Vector4, side: Int32, targetID: EntityID, radius: Float, opt impact: ref<VrBladeImpact>) -> Bool {
  let context: AttackInitContext;
  let attack: ref<Attack_GameEffect>;
  let effect: ref<EffectInstance>;
  let ticket: ref<VrCyberwareAttackTicket>;
  let enhanced: ref<VrMeleeEnhancedTicket>;
  let direction = Vector4.Normalize(target - contact);
  let start: Vector4;
  let now = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  let i: Int32 = ArraySize(this.vrCyberwareTickets) - 1;
  while i >= 0 {
    if now - this.vrCyberwareTickets[i].time > 2.00 { ArrayErase(this.vrCyberwareTickets, i); };
    i -= 1;
  };
  context.record = swing.record;
  context.instigator = this;
  // The source is the attacking puppet, as in the flat game (meleeTransitions): hit reactions read the
  // attacker's weapon type through ScriptedPuppet.GetWeaponRight(GetSource()). With the weapon object as the
  // source that type came out empty, "not fists", so every hit on a blocking fist fighter broke the guard
  // and a block never counted (VR training course 4).
  context.source = this;
  context.weapon = swing.weapon;
  attack = IAttack.Create(context) as Attack_GameEffect;
  if !IsDefined(attack) { return false; };
  effect = attack.PrepareAttack(this);
  if !IsDefined(effect) { return false; };
  if Vector4.Distance(contact, target) < 0.01 {
    direction = this.VRCyberwareKind(swing.weapon) == 0 ? swing.weapon.VRMeleeDirection() : Vector4.Normalize(VRCyberwarePoint(side, 1) - VRCyberwarePoint(side, 0));
  };
  // A sweep must begin outside the touched body, including when the visible
  // segment has already crossed its centre between tracking samples.
  start = contact;
  if IsDefined(impact) {
    // Trace the real segment in its own direction. A second box aimed down the
    // broad proxy normal can return eight valid hits, all away from the blade.
    start = impact.a;
    target = impact.b;
    if impact.edgeStroke!=0u {
      // Keep the clipped material chord for damage reporting. Acquire the
      // game's hit representation along the same visible shaft from outside.
      if !IsDefined(impact.cutShape) { return false; };
      if impact.cutShape.reverseQuery { start=impact.b;target=impact.a; };
    } else {
      if impact.sweepFrom.W>0.5 && Vector4.Distance(impact.sweepFrom,impact.sweepTo)>=0.02 {
        start=impact.sweepFrom;target=impact.sweepTo;
      };
    };
    direction=Vector4.Normalize(target-start);
    radius = VrBladeVisibleCut.BoxHalfExtent();
    impact.swing = swing;
    attack.vrBladeImpact = impact;
  };
  attack.vrMeleeContactTarget=targetID;
  attack.vrPhysicalLightChain = swing.chain;
  EffectData.SetVariant(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.attack, ToVariant(attack));
  EffectData.SetVector(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.box, Vector4(radius * 2.00, radius * 2.00, radius * 2.00, 0.00));
  EffectData.SetFloat(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.duration, 0.05);
  EffectData.SetVector(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.position, start);
  EffectData.SetQuat(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.rotation, Quaternion.BuildFromDirectionVector(direction));
  EffectData.SetVector(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.forward, direction);
  EffectData.SetFloat(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.range, Vector4.Distance(start,target));
  EffectData.SetInt(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.attackNumber, swing.number);
  EffectData.SetName(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.impactOrientationSlot, swing.record.ImpactFxSlot());
  EffectData.SetFloat(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.charge, WeaponObject.GetWeaponChargeNormalized(swing.weapon));
  EffectData.SetBool(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.meleeCleave, false);
  EffectData.SetVariant(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.fxPackage, ToVariant(swing.weapon.GetFxPackage()));
  EffectData.SetBool(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.playerOwnedWeapon, true);
  this.vrCyberwareLastAttackGeometry = [contact, target, start, Vector4(direction.X, direction.Y, direction.Z, Vector4.Distance(start,target))];
  ticket = new VrCyberwareAttackTicket(); ticket.attack = attack; ticket.time = now;
  ticket.intendedTarget = targetID; ticket.weapon = swing.weapon.GetItemID();
  ArrayPush(this.vrCyberwareTickets, ticket);
  enhanced = this.VRMeleeRegisterAttack(attack, swing.weapon, targetID, swing.peakSpeed);
  if !attack.StartAttack() { ArrayRemove(this.vrCyberwareTickets, ticket); this.VRMeleeCancelAttack(enhanced); return false; };
  if !IsDefined(impact) && swing.chain && !swing.landed { swing.landed = true; this.VRMeleeChainLanded(swing); };
  this.vrCyberwareAttackCount += 1;
  return true;
}

@addMethod(PlayerPuppet)
public func VRCyberwareIsAttack(attack: ref<IAttack>) -> Bool {
  let ticket: ref<VrCyberwareAttackTicket>;
  for ticket in this.vrCyberwareTickets { if ticket.attack == attack { return true; }; };
  return false;
}

@addMethod(PlayerPuppet)
public func VRCyberwareConfirmHit(hit: ref<gameHitEvent>, receiver: ref<GameObject>, damage: Float) -> Void {
  let ticket: ref<VrCyberwareAttackTicket>;
  let weapon = hit.attackData.GetWeapon();
  let definition = hit.attackData.GetAttackDefinition();
  let now = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  let i: Int32 = ArraySize(this.vrCyberwareTickets) - 1;
  if !IsDefined(weapon) || !IsDefined(definition) || !AttackData.IsMelee(hit.attackData.GetAttackType()) { return; };
  while i >= 0 {
    ticket = this.vrCyberwareTickets[i];
    if ticket.intendedTarget == receiver.GetEntityID() && ItemID.GetTDBID(ticket.weapon) == ItemID.GetTDBID(weapon.GetItemID())
      && definition == ticket.attack
      && definition.GetRecord().GetID() == ticket.attack.GetRecord().GetID()
      && now >= ticket.time && now - ticket.time < 0.35 && !ArrayContains(ticket.targets, receiver.GetEntityID()) {
      ArrayPush(ticket.targets, receiver.GetEntityID());
      this.vrCyberwareHitCount += 1;
      this.vrCyberwareLastNativeDamage = damage;
      this.vrCyberwareLastHitDelay = now - ticket.time;
      if IsDefined((ticket.attack as Attack_GameEffect).vrBladeImpact) { VRHapticEvent(14, VRPortGuardHands(this)); }
      else { GameObject.PlaySoundEvent(receiver, n"w_feedback_hit_npc"); };
      return;
    };
    i -= 1;
  };
}

@wrapMethod(ScriptedPuppet)
protected cb func OnDamageReceived(event: ref<gameDamageReceivedEvent>) -> Bool {
  let result = wrappedMethod(event);
  let hit = event.hitEvent;
  let player: ref<PlayerPuppet>;
  let instigator: ref<GameObject>;
  let weapon: ref<WeaponObject>;
  if !this.IsPlayer() && event.totalDamageReceived > 0.00 && IsDefined(hit) && IsDefined(hit.attackData) {
    player = GetPlayer(this.GetGame());
    instigator = hit.attackData.GetInstigator(); weapon = hit.attackData.GetWeapon();
    if IsDefined(player) {
      player.vrCyberwareDamageEvents += 1;
      player.vrCyberwareDamageTrace = "type=" + ToString(hit.attackData.GetAttackType());
      if IsDefined(instigator) { player.vrCyberwareDamageTrace += " instigator=" + EntityID.ToDebugStringDecimal(instigator.GetEntityID()); };
      if IsDefined(weapon) { player.vrCyberwareDamageTrace += " weapon=" + ToString(weapon.GetItemID()); };
      if IsDefined(instigator) && instigator.GetEntityID() == player.GetEntityID() { player.VRCyberwareConfirmHit(hit, this, event.totalDamageReceived); };
    };
  };
  return result;
}
