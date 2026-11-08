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
private func VRMeleeChainLanded(swing: ref<VrCyberwareSwing>) -> Void {
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
  item = GameInstance.GetTransactionSystem(this.GetGame()).GetItemInSlot(this, VRLeftHandedWeapon.HeldSlot(this));
  if !IsDefined(item) || item.GetItemID() != weapon.GetItemID() { return false; };
  // Hand-held weapons and fists walk the game's light combo; arm cyberware keeps
  // its per-arm records, which drive the arm animation.
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
  weapon.SetAttack(record.GetID());
  while ArraySize(this.vrCyberwareSwings) < 2 { ArrayPush(this.vrCyberwareSwings, null); };
  if this.VRCyberwareKind(weapon) != 0 && IsDefined(this.vrCyberwareSwings[side]) && this.vrCyberwareSwings[side].weapon == weapon {
    swing.targets = this.vrCyberwareSwings[side].targets;
  };
  this.vrCyberwareSwings[side] = swing;
  record.StaminaCost(costs);
  cost = RPGManager.CalculateStatModifiers(costs, this.GetGame(), weapon, Cast<StatsObjectID>(weapon.GetEntityID()));
  if cost > 0.00 { PlayerStaminaHelpers.ModifyStamina(this, -cost); };
  this.vrCyberwareSwingCount += 1;
  if WeaponObject.IsOfType(weapon.GetItemID(), gamedataItemType.Cyb_MantisBlades) || WeaponObject.IsOfType(weapon.GetItemID(), gamedataItemType.Cyb_StrongArms) {
    this.VRCyberwareAnimateSwing(weapon, record, side, strong);
  };
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
public func VRCyberwareOrdinaryContact(weapon: wref<WeaponObject>, target: wref<GameObject>, contact: Vector4, hit: Vector4, passive: Bool) -> Int32 {
  let swing: ref<VrCyberwareSwing>;
  let touched: Bool;
  if ArraySize(this.vrCyberwareSwings) < 2 || !IsDefined(weapon) || this.VRHolsterIntentActive(1.00) { return 0; };
  swing = this.vrCyberwareSwings[1];
  if !IsDefined(swing) || swing.weapon != weapon { return 0; };
  if !IsDefined(target) { return 0; };
  touched = ArrayContains(swing.targets, target.GetEntityID());
  // Ordinary weapons hit a target once per deliberate stroke. A new physical
  // reversal can cut again without first leaving an oversized body capsule.
  if passive || touched { return 0; };
  if this.VRCyberwareAttack(swing, contact, hit, 1, target.GetEntityID(), 0.175) {
    ArrayPush(swing.targets, target.GetEntityID());
    VRHapticEvent(14, VRPortGuardHands(this));   // the blow landed: felt in the hand holding the weapon
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
  public static func ClosestBody(a: Vector4, b: Vector4, base: Vector4) -> Vector4 {
    let u = b - a;
    let wx = a.X - base.X;
    let wy = a.Y - base.Y;
    let wz = a.Z - base.Z - 0.25;
    let aa = u.X*u.X + u.Y*u.Y + u.Z*u.Z;
    let bb = u.Z * 1.40;
    let dd = u.X*wx + u.Y*wy + u.Z*wz;
    let ee = wz * 1.40;
    let determinant = aa * 1.96 - bb*bb;
    let s: Float;
    let t: Float;
    let point: Vector4;
    let dx: Float;
    let dy: Float;
    let dz: Float;
    if determinant > 0.0000001 { s = ClampF((bb*ee - 1.96*dd) / determinant, 0.00, 1.00); };
    t = ClampF((bb*s + ee) / 1.96, 0.00, 1.00);
    if aa > 0.0000001 { s = ClampF((bb*t - dd) / aa, 0.00, 1.00); };
    t = ClampF((bb*s + ee) / 1.96, 0.00, 1.00);
    point = a + u*s;
    dx = point.X - base.X; dy = point.Y - base.Y; dz = point.Z - base.Z - 0.25 - 1.40*t;
    point.W = SqrtF(dx*dx + dy*dy + dz*dz);
    return point;
  }
}

@addMethod(PlayerPuppet)
public func VRCyberwareContacts(weapon: wref<WeaponObject>, side: Int32, points: array<Vector4>, previous: array<Vector4>, radius: Float, opt passive: Bool) -> Int32 {
  let swing: ref<VrCyberwareSwing>;
  let query: TargetSearchQuery;
  let parts: array<TS_TargetPartInfo>;
  let checked: array<EntityID>;
  let touching: array<EntityID>;
  let retained: array<EntityID>;
  let id: EntityID;
  let target: wref<GameObject>;
  let component: wref<TargetingComponent>;
  let origin = this.GetWorldPosition();
  let a: Vector4;
  let b: Vector4;
  let p: Vector4;
  let closest: Vector4;
  let contact: Vector4;
  let hit: Vector4;
  let base: Vector4;
  let d: Float;
  let best: Float;
  let reach: Float = 1.00;
  let i: Int32;
  let j: Int32;
  let height: Int32;
  let step: Int32;
  let steps: Int32;
  let count: Int32;
  let alpha: Float;
  let item: wref<ItemObject>;
  if side < 0 || side >= ArraySize(this.vrCyberwareSwings) || ArraySize(points) < 2 || ArraySize(points) > 46 { return 0; };
  swing = this.vrCyberwareSwings[side];
  if !IsDefined(swing) || !IsDefined(weapon) || swing.weapon != weapon || this.VRHolsterIntentActive(1.00) { return 0; };
  item = GameInstance.GetTransactionSystem(this.GetGame()).GetItemInSlot(this, VRLeftHandedWeapon.HeldSlot(this));
  if !IsDefined(item) || item.GetItemID() != weapon.GetItemID() { return 0; };
  radius = ClampF(radius, 0.02, 0.10);
  while i < ArraySize(points) { reach = MaxF(reach, Vector4.Distance(points[i], origin)); i += 1; };
  query.testedSet = TargetingSet.Complete;
  query.searchFilter = TSF_NPC();
  query.maxDistance = reach + 1.80;
  query.filterObjectByDistance = true;
  query.ignoreInstigator = true;
  GameInstance.GetTargetingSystem(this.GetGame()).GetTargetParts(this, query, parts);
  i = 0;
  while i < ArraySize(parts) {
    component = TS_TargetPartInfo.GetComponent(parts[i]);
    if IsDefined(component) { target = component.GetEntity() as GameObject; } else { target = null; };
    if IsDefined(target) && !target.IsDead() && !ArrayContains(checked, target.GetEntityID()) {
      ArrayPush(checked, target.GetEntityID());
      base = target.GetWorldPosition();
      best = 9999.00;
      j = 0;
      while j + 1 < ArraySize(points) {
        // Sweep the visible segment between samples, bounded for a tracking
        // discontinuity. The caller rejects teleports and equipment changes.
        steps = 1;
        if ArraySize(previous) == ArraySize(points) {
          steps = Clamp(CeilF(MaxF(Vector4.Distance(points[j], previous[j]), Vector4.Distance(points[j + 1], previous[j + 1])) / 0.05), 1, 12);
        };
        step = 0;
        while step <= steps {
          alpha = Cast<Float>(step) / Cast<Float>(steps);
          a = points[j]; b = points[j + 1];
          if ArraySize(previous) == ArraySize(points) { a = previous[j] + (a - previous[j]) * alpha; b = previous[j + 1] + (b - previous[j + 1]) * alpha; };
          closest = VrCyberwareContactMath.ClosestBody(a, b, base);
          d = closest.W;
          if d < best {
            best = d; contact = closest; contact.W = 1.00;
            hit = Vector4(base.X, base.Y, ClampF(closest.Z, base.Z + 0.25, base.Z + 1.65), 1.00);
          };
          step += 1;
        };
        j += 1;
      };
      // Approximate body capsule for candidate discovery; the native effect
      // below performs the actual physics hit and all damage processing.
      if best <= radius + 0.20 {
        ArrayPush(touching, target.GetEntityID());
        if !passive && !ArrayContains(swing.targets, target.GetEntityID()) && this.VRCyberwareAttack(swing, contact, hit, side, target.GetEntityID(), radius) {
          ArrayPush(swing.targets, target.GetEntityID());
          count += 1;
        };
      };
    };
    i += 1;
  };
  // Pulling a fist/blade back while it is still in the target is the same
  // contact, even when its speed direction starts a new swing episode.
  // All cyberware must separate before another contact can damage the target.
  for id in swing.targets { if ArrayContains(touching, id) { ArrayPush(retained, id); }; };
  swing.targets = retained;
  return count;
}

@addMethod(PlayerPuppet)
private func VRCyberwareAttack(swing: ref<VrCyberwareSwing>, contact: Vector4, target: Vector4, side: Int32, targetID: EntityID, radius: Float) -> Bool {
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
  start = contact - direction * 0.35;
  EffectData.SetVector(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.box, Vector4(radius * 2.00, radius * 2.00, radius * 2.00, 0.00));
  EffectData.SetFloat(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.duration, 0.05);
  EffectData.SetVector(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.position, start);
  EffectData.SetQuat(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.rotation, Quaternion.BuildFromDirectionVector(direction));
  EffectData.SetVector(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.forward, direction);
  EffectData.SetFloat(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.range, Vector4.Distance(start, target) + 0.08);
  EffectData.SetInt(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.attackNumber, swing.number);
  EffectData.SetName(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.impactOrientationSlot, swing.record.ImpactFxSlot());
  EffectData.SetFloat(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.charge, WeaponObject.GetWeaponChargeNormalized(swing.weapon));
  EffectData.SetBool(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.meleeCleave, swing.strong);
  EffectData.SetVariant(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.fxPackage, ToVariant(swing.weapon.GetFxPackage()));
  EffectData.SetBool(effect.GetSharedData(), GetAllBlackboardDefs().EffectSharedData.playerOwnedWeapon, true);
  this.vrCyberwareLastAttackGeometry = [contact, target, start, Vector4(direction.X, direction.Y, direction.Z, Vector4.Distance(start, target) + 0.08)];
  ticket = new VrCyberwareAttackTicket(); ticket.attack = attack; ticket.time = now;
  ticket.intendedTarget = targetID; ticket.weapon = swing.weapon.GetItemID();
  ArrayPush(this.vrCyberwareTickets, ticket);
  enhanced = this.VRMeleeRegisterAttack(attack, swing.weapon, targetID, swing.peakSpeed);
  if !attack.StartAttack() { ArrayRemove(this.vrCyberwareTickets, ticket); this.VRMeleeCancelAttack(enhanced); return false; };
  if swing.chain && !swing.landed { swing.landed = true; this.VRMeleeChainLanded(swing); };
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
      && definition.GetRecord().GetID() == ticket.attack.GetRecord().GetID()
      && now >= ticket.time && now - ticket.time < 0.35 && !ArrayContains(ticket.targets, receiver.GetEntityID()) {
      ArrayPush(ticket.targets, receiver.GetEntityID());
      this.vrCyberwareHitCount += 1;
      this.vrCyberwareLastNativeDamage = damage;
      this.vrCyberwareLastHitDelay = now - ticket.time;
      GameObject.PlaySoundEvent(receiver, n"w_feedback_hit_npc");
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
