// A fresh attack and its confirmed health ledger identify one controller hit.
// The queued HitReactionRequest retains that hit even if another receipt lands.
@addField(Attack_GameEffect) public let vrPhysicalLightChain: Bool;
@addField(gameHitEvent) public let vrSeatedBladeReaction: Bool;

public abstract class VrPhysicalMeleeReaction {
  public static func SeverityCap(hit: ref<gameHitEvent>, npc: wref<NPCPuppet>) -> Int32 {
    if !IsDefined(hit) || !IsDefined(npc) || hit.target != npc || hit.projectionPipeline { return -1; };
    let ticket = hit.vrMeleeEnhancedTicket;
    let data = hit.attackData;
    if !IsDefined(ticket) || !ticket.confirmed || !IsDefined(data) || !AttackData.IsMelee(data.GetAttackType()) { return -1; };
    let attack = data.GetAttackDefinition() as Attack_GameEffect;
    let weapon = data.GetWeapon();
    if !IsDefined(attack) || attack != ticket.attack || ticket.target != npc.GetEntityID()
      || attack.vrMeleeContactTarget != ticket.target || !IsDefined(weapon) || weapon.GetItemID() != ticket.weapon
      || !IsDefined(data.GetInstigator() as PlayerPuppet) || VrBladeRPG.Rejected(data)
      || data.HasFlag(hitFlag.ForceKnockdown) || data.HasFlag(hitFlag.HighSpeedMelee) || data.HasFlag(hitFlag.StunApplied)
      || !npc.IsActive() || npc.IsAboutToBeDefeated() || npc.IsIncapacitated() || npc.IsRagdolling() { return -1; };
    // A retained entry or its lodged follow-through uses the graph's brief twitch.
    // Detached pass-throughs still get the reaction for their own attack record.
    if hit.vrSeatedBladeReaction { return 0; };
    if !attack.vrPhysicalLightChain || !AttackData.IsLightMelee(data.GetAttackType()) { return -1; };
    let record = attack.GetRecord() as Attack_Melee_Record;
    if !IsDefined(record) || !IsDefined(record.AttackSubtype())
      || NotEquals(record.AttackSubtype().Type(), gamedataAttackSubtype.ComboAttack) { return -1; };
    return 1; // ordinary combo: stock impact threshold, immunity and cooldown
  }
}

@wrapMethod(HitReactionComponent)
protected final func GetReactionType(guardBreakImpulse: Float, newHitEvent: ref<gameHitEvent>) -> animHitReactionType {
  let cap = VrPhysicalMeleeReaction.SeverityCap(newHitEvent, this.m_ownerNPC);
  if cap < 0 || !this.m_isAlive { return wrappedMethod(guardBreakImpulse, newHitEvent); };
  let data = newHitEvent.attackData;
  let maximum = data.GetHitReactionSeverityMax();
  // Limit selection before the stock selector records the winning reaction's
  // timeout. Rewriting its result afterwards would leave a phantom stagger.
  data.SetHitReactionSeverityMax(maximum < 0 ? cap : Min(maximum, cap));
  let reaction = wrappedMethod(guardBreakImpulse, newHitEvent);
  data.SetHitReactionSeverityMax(maximum);
  if Equals(reaction, animHitReactionType.Impact) || Equals(reaction, animHitReactionType.Twitch) {
    this.m_hitIntensity = EAIHitIntensity.Light;
  };
  return reaction;
}

// Only the seated weapon owns this AI command. Follow-through receipts keep
// the joint serial. The delayed check recovers a lost CET release on reload.
public class VrBladeMovementHold extends DelayCallback {
  public let weapon: wref<WeaponObject>;
  public let target: wref<NPCPuppet>;
  public let jointSerial: Uint32;
  public let command: ref<AIHoldPositionCommand>;
  public let checkID: DelayID;

  public func Call() -> Void {
    if !IsDefined(this.target) || this.target.vrBladeMovementHold != this { return; };
    let receipt: ref<VrBladeReceipt>;
    if IsDefined(this.weapon) { receipt = this.weapon.vrBladeReceipt; };
    if IsDefined(receipt) && receipt.jointSerial == this.jointSerial && receipt.target == this.target
      && receipt.lodged && !receipt.released && IsDefined(receipt.owner) && receipt.owner.VRBladeAllowed(this.weapon)
      && this.target.IsActive() && !this.target.IsRagdolling() && !this.target.IsIncapacitated() {
      let elapsed = VRMeleeRealTime() - receipt.lodgedAt;
      if elapsed >= 0.0 && elapsed < 0.50 {
        this.checkID = GameInstance.GetDelaySystem(this.target.GetGame()).DelayCallback(this, 0.50);
        return;
      };
      // CET can remeasure this same seat after a long frame. Let its next
      // valid lodge sample reacquire once, including after receipt rollover.
      this.weapon.vrResumeBladeHoldJoint = this.jointSerial;
    };
    this.target.VRClearBladeMovementHold();
  }
}

@addField(NPCPuppet) public let vrBladeMovementHold: ref<VrBladeMovementHold>;
@addField(WeaponObject) private let vrResumeBladeHoldJoint: Uint32;

@addMethod(WeaponObject)
public func VRBeginBladeMovementHold(r: ref<VrBladeReceipt>) -> Void {
  if r.jointSerial == 0u || !IsDefined(r.target) || !IsDefined(r.owner) || !r.target.IsAttached()
    || !VrBladeRPG.SpecialTarget(r.owner, r.target) { return; };
  let ai = r.target.GetAIControllerComponent();
  if !IsDefined(ai) { return; };
  r.target.VRClearBladeMovementHold();
  // Leave a scripted movement order in control of its actor.
  if IsDefined(ai.GetEnqueuedOrExecutingCommand(n"AIMoveCommand", true)) { return; };
  let hold = new VrBladeMovementHold();
  hold.command = new AIHoldPositionCommand();
  hold.command.duration = -1.0;
  if !ai.SendCommand(hold.command) { return; };
  hold.target = r.target;
  hold.weapon = this;
  hold.jointSerial = r.jointSerial;
  r.target.vrBladeMovementHold = hold;
  hold.checkID = GameInstance.GetDelaySystem(r.target.GetGame()).DelayCallback(hold, 0.50);
}

@addMethod(WeaponObject)
public func VREndBladeMovementHold(r: ref<VrBladeReceipt>) -> Void {
  if !IsDefined(r) || !IsDefined(r.target) { return; };
  let hold = r.target.vrBladeMovementHold;
  if IsDefined(hold) && hold.weapon == this && hold.jointSerial == r.jointSerial {
    r.target.VRClearBladeMovementHold();
  };
}

@addMethod(NPCPuppet)
public func VRClearBladeMovementHold() -> Void {
  let hold = this.vrBladeMovementHold;
  if !IsDefined(hold) { return; };
  this.vrBladeMovementHold = null;
  GameInstance.GetDelaySystem(this.GetGame()).CancelCallback(hold.checkID);
  let ai = this.GetAIControllerComponent();
  if IsDefined(ai) {
    if Equals(hold.command.state, AICommandState.Executing) {
      ai.StopExecutingCommand(hold.command, true);
    } else {
      if Equals(hold.command.state, AICommandState.Enqueued) {
        ai.CancelCommand(hold.command);
      };
    };
  };
}

@wrapMethod(NPCPuppet)
protected func OnIncapacitated() -> Void {
  this.VRClearBladeMovementHold();
  wrappedMethod();
}

@wrapMethod(NPCPuppet)
protected cb func OnRagdollEnabledEvent(evt: ref<RagdollNotifyEnabledEvent>) -> Bool {
  this.VRClearBladeMovementHold();
  return wrappedMethod(evt);
}

@wrapMethod(NPCPuppet)
protected cb func OnDetach() -> Bool {
  let hold = this.vrBladeMovementHold;
  if IsDefined(hold) && IsDefined(hold.weapon) {
    let r = hold.weapon.vrBladeReceipt;
    if IsDefined(r) && r.jointSerial == hold.jointSerial && r.target == this {
      hold.weapon.VRBladeReleaseContact(r.serial);
    };
  };
  this.VRClearBladeMovementHold();
  return wrappedMethod();
}

@wrapMethod(WeaponObject)
protected cb func OnDetach() -> Bool {
  if IsDefined(this.vrBladeReceipt) { this.VRBladeReleaseContact(this.vrBladeReceipt.serial); };
  return wrappedMethod();
}
