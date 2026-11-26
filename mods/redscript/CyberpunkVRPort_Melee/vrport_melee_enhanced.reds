public native func VRMeleeInputFrame() -> array<Vector4>;
public native func VRMeleeRealTime() -> Float;
public native func VRMeleeTwoHandedActive() -> Bool;

public class VrMeleeEnhancedTicket extends IScriptable {
  public let attack: ref<IAttack>;
  public let weapon: ItemID;
  public let target: EntityID;
  public let created: Float;
  public let createdSim: Float;
  public let speed: Float;
  public let twoHanded: Bool;
  public let claimed: Bool;
  public let confirmed: Bool;
  public let combo: Int32;
  public let multiplier: Float;
  public let nativeDamage: Float;
}

public class VrMeleeEnhancedMath extends IScriptable {
  public static func ComboFactor(hit: Int32) -> Float {
    return 1.00 + Cast<Float>(Clamp(hit, 1, 10) - 1) / 9.00;
  }
  public static func Multiplier(hit: Int32, speed: Float, fastThreshold: Float, twoHanded: Bool) -> Float {
    return VrMeleeEnhancedMath.ComboFactor(hit) * (twoHanded ? 2.00 : 1.00) * (speed >= fastThreshold ? 1.50 : 1.00);
  }
  public static func TwoHandEligible(weapon: wref<WeaponObject>) -> Bool {
    let id: ItemID;
    if !IsDefined(weapon) { return false; };
    id = weapon.GetItemID();
    return WeaponObject.IsOfType(id, gamedataItemType.Wea_Katana)
      || WeaponObject.IsOfType(id, gamedataItemType.Wea_OneHandedClub)
      || WeaponObject.IsOfType(id, gamedataItemType.Wea_TwoHandedClub);
  }
}

@addField(gameHitEvent) public let vrMeleeEnhancedTicket: ref<VrMeleeEnhancedTicket>;
@addField(PlayerPuppet) private let vrMeleeEnhancedTickets: array<ref<VrMeleeEnhancedTicket>>;
@addField(PlayerPuppet) public let vrMeleeEnhancedDisabled: Bool;
@addField(PlayerPuppet) public let vrMeleeComboTarget: EntityID;
@addField(PlayerPuppet) public let vrMeleeComboCount: Int32;
@addField(PlayerPuppet) public let vrMeleeComboLastTime: Float;
@addField(PlayerPuppet) public let vrMeleeConfirmedHits: Int32;
@addField(PlayerPuppet) public let vrMeleeLastBaseDamage: Float;
@addField(PlayerPuppet) public let vrMeleeLastMultiplier: Float;
@addField(PlayerPuppet) public let vrMeleeLastDealt: Float;
@addField(PlayerPuppet) public let vrMeleeLastSpeed: Float;
@addField(PlayerPuppet) public let vrMeleeLastTwoHanded: Bool;
// Scoped live-test audit: native and VR attacks are captured before the game's
// random 0.9..1.1 variant. Disabled in ordinary play; no damage is overridden.
@addField(PlayerPuppet) public let vrMeleeAuditEnabled: Bool;
@addField(PlayerPuppet) public let vrMeleeAuditValues: array<Float>;
@addField(PlayerPuppet) public let vrMeleeAuditEnhancedValues: array<Float>;
@addField(PlayerPuppet) public let vrMeleeAuditRecord: TweakDBID;
@addField(PlayerPuppet) public let vrMeleeAuditCount: Int32;

@addMethod(PlayerPuppet)
public func VRMeleeResetCombo() -> Void {
  let empty: EntityID;
  this.vrMeleeComboTarget = empty;
  this.vrMeleeComboCount = 0;
  this.vrMeleeComboLastTime = -10.00;
}

@addMethod(PlayerPuppet)
public func VRMeleeRegisterAttack(attack: ref<IAttack>, weapon: wref<WeaponObject>, target: EntityID, speed: Float) -> ref<VrMeleeEnhancedTicket> {
  let ticket: ref<VrMeleeEnhancedTicket>;
  let now = VRMeleeRealTime();
  let sim = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  let i = ArraySize(this.vrMeleeEnhancedTickets) - 1;
  while i >= 0 {
    if now - this.vrMeleeEnhancedTickets[i].created > 10.00 || sim - this.vrMeleeEnhancedTickets[i].createdSim > 0.50 || this.vrMeleeEnhancedTickets[i].confirmed {
      ArrayErase(this.vrMeleeEnhancedTickets, i);
    };
    i -= 1;
  };
  if !IsDefined(attack) || !IsDefined(weapon) { return null; };
  ticket = new VrMeleeEnhancedTicket();
  ticket.attack = attack; ticket.weapon = weapon.GetItemID(); ticket.target = target;
  ticket.created = now; ticket.createdSim = sim; ticket.speed = ClampF(speed, 0.00, 30.00);
  ticket.twoHanded = VrMeleeEnhancedMath.TwoHandEligible(weapon) && VRMeleeTwoHandedActive();
  ArrayPush(this.vrMeleeEnhancedTickets, ticket);
  return ticket;
}

@addMethod(PlayerPuppet)
public func VRMeleeCancelAttack(ticket: ref<VrMeleeEnhancedTicket>) -> Void {
  ArrayRemove(this.vrMeleeEnhancedTickets, ticket);
}

@addMethod(PlayerPuppet)
public func VRMeleeApplyEnhancedHit(hit: ref<gameHitEvent>) -> Void {
  let ticket: ref<VrMeleeEnhancedTicket>;
  let weapon: wref<WeaponObject>;
  let definition: ref<IAttack>;
  let now = VRMeleeRealTime();
  let sim = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  let fastThreshold: Float;
  let i = ArraySize(this.vrMeleeEnhancedTickets) - 1;
  if hit.projectionPipeline || IsDefined(hit.vrMeleeEnhancedTicket) || !IsDefined(hit.target)
    || !IsDefined(hit.attackData) || !IsDefined(hit.attackComputed) { return; };
  if !AttackData.IsMelee(hit.attackData.GetAttackType()) { return; };
  weapon = hit.attackData.GetWeapon(); definition = hit.attackData.GetAttackDefinition();
  if !IsDefined(weapon) || !IsDefined(definition) { return; };
  while i >= 0 {
    ticket = this.vrMeleeEnhancedTickets[i];
    if !ticket.claimed && ticket.target == hit.target.GetEntityID() && ticket.weapon == weapon.GetItemID()
      && definition == ticket.attack
      && ticket.attack.GetRecord().GetID() == definition.GetRecord().GetID()
      && now >= ticket.created && now - ticket.created <= 10.00 && sim >= ticket.createdSim && sim - ticket.createdSim <= 0.35 {
      ticket.claimed = true; hit.vrMeleeEnhancedTicket = ticket;
      ticket.nativeDamage = hit.attackComputed.GetTotalAttackValue(gamedataStatPoolType.Health);
      ticket.combo = 1;
      if ticket.target == this.vrMeleeComboTarget && now >= this.vrMeleeComboLastTime && now - this.vrMeleeComboLastTime <= 5.00 {
        ticket.combo = Min(this.vrMeleeComboCount + 1, 10);
      };
      fastThreshold = this.VRCyberwareKind(weapon) == 1 ? 4.00 : 6.00;
      ticket.multiplier = this.vrMeleeEnhancedDisabled || (IsDefined((definition as Attack_GameEffect).vrBladeImpact) && (definition as Attack_GameEffect).vrBladeImpact.parentSerial != 0u)
        ? 1.00 : VrMeleeEnhancedMath.Multiplier(ticket.combo, ticket.speed, fastThreshold, ticket.twoHanded);
      // Multiply every native damage channel once, before the game's variant,
      // armor, crit/perk calculation and hit/status/poise reactions.
      hit.attackComputed.MultAttackValue(ticket.multiplier);
      this.vrMeleeLastBaseDamage = ticket.nativeDamage;
      this.vrMeleeLastMultiplier = ticket.multiplier;
      this.vrMeleeLastSpeed = ticket.speed;
      this.vrMeleeLastTwoHanded = ticket.twoHanded;
      return;
    };
    i -= 1;
  };
}

@addMethod(PlayerPuppet)
public func VRMeleeConfirmEnhancedHit(hit: ref<gameHitEvent>, damage: Float) -> Void {
  let ticket = hit.vrMeleeEnhancedTicket;
  if !(damage > 0.00) || hit.projectionPipeline || !IsDefined(ticket) || ticket.confirmed { return; };
  if IsDefined((ticket.attack as Attack_GameEffect).vrBladeImpact) && !this.VRBladePublishHit(hit, damage) { return; };
  ticket.confirmed = true;
  this.vrMeleeConfirmedHits += 1;
  this.vrMeleeLastDealt = damage;
  if !this.vrMeleeEnhancedDisabled && (!IsDefined((ticket.attack as Attack_GameEffect).vrBladeImpact) || (ticket.attack as Attack_GameEffect).vrBladeImpact.parentSerial == 0u) {
    this.vrMeleeComboTarget = ticket.target;
    this.vrMeleeComboCount = ticket.combo;
    this.vrMeleeComboLastTime = VRMeleeRealTime();
  };
}

@wrapMethod(DamageSystem)
private func CalculateDamageVariants(hitEvent: ref<gameHitEvent>) -> Void {
  let player: ref<PlayerPuppet>;
  if IsDefined(hitEvent.attackData) && IsDefined(hitEvent.attackComputed) && !hitEvent.projectionPipeline {
    player = hitEvent.attackData.GetInstigator() as PlayerPuppet;
    if IsDefined(player) && AttackData.IsMelee(hitEvent.attackData.GetAttackType()) {
      if player.vrMeleeAuditEnabled {
        player.vrMeleeAuditValues = hitEvent.attackComputed.GetAttackValues();
        player.vrMeleeAuditRecord = hitEvent.attackData.GetAttackDefinition().GetRecord().GetID();
        player.vrMeleeAuditCount += 1;
      };
      player.VRMeleeApplyEnhancedHit(hitEvent);
      if player.vrMeleeAuditEnabled { player.vrMeleeAuditEnhancedValues = hitEvent.attackComputed.GetAttackValues(); };
    };
  };
  wrappedMethod(hitEvent);
}

@wrapMethod(DamageSystem)
private func SendDamageEvents(hitEvent: ref<gameHitEvent>, const resourcesLost: script_ref<[SDamageDealt]>) -> Void {
  let player: ref<PlayerPuppet>;
  let damage: Float;
  let i: Int32;
  // Actual stat-pool loss is already known here. Commit before the next hit
  // pipeline, rather than waiting for an asynchronous NPC event; simultaneous
  // left/right strikes therefore receive consecutive combo steps.
  if IsDefined(hitEvent.vrMeleeEnhancedTicket) && IsDefined(hitEvent.attackData) {
    player = hitEvent.attackData.GetInstigator() as PlayerPuppet;
    if IsDefined(player) {
      while i < ArraySize(Deref(resourcesLost)) {
        if Equals(Deref(resourcesLost)[i].affectedStatPool, gamedataStatPoolType.Health) { damage += Deref(resourcesLost)[i].value; };
        i += 1;
      };
      player.VRMeleeConfirmEnhancedHit(hitEvent, damage);
    };
  };
  wrappedMethod(hitEvent, resourcesLost);
}
