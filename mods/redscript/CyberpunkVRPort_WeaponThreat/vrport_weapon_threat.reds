module CyberpunkVRPort.WeaponThreat

public class VRWeaponAimExpiryEvent extends Event {
  public let token: Uint32;
}

// PlayerPuppet sets these handles on equip and clears them on unequip.
// A lingering attachment can still have a valid item ID and muzzle transform.
@addMethod(PlayerPuppet)
public final const func VRPortHasEquippedAimWeapon(weapon: ref<WeaponObject>) -> Bool {
  return IsDefined(weapon) && (weapon == this.m_equippedLeftHandWeapon || weapon == this.m_equippedRightHandWeapon);
}

@addField(NPCPuppet) private let m_vrWeaponAimActive: Bool;
@addField(NPCPuppet) private let m_vrWeaponAimUntil: Float;
@addField(NPCPuppet) private let m_vrWeaponAimToken: Uint32;

@addMethod(NPCPuppet)
public final const func VRPortIsWeaponAimedAt() -> Bool {
  return this.m_vrWeaponAimActive
    && EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame())) <= this.m_vrWeaponAimUntil
    && this.IsAttached() && this.IsActive();
}

@addMethod(NPCPuppet)
private final func VRPortScheduleAimExpiry(delay: Float) -> Void {
  let evt = new VRWeaponAimExpiryEvent();
  evt.token = this.m_vrWeaponAimToken;
  GameInstance.GetDelaySystem(this.GetGame()).DelayEvent(this, evt, delay);
}

// The caller refreshes a short lease while a barrel remains on this NPC. Only
// transitions send stimuli: repeatedly sending AimingAt postpones the native
// aggressive NPC's delayed reaction instead of letting it execute.
@addMethod(NPCPuppet)
public final func VRPortSetWeaponAim(active: Bool) -> Void {
  let player = GetPlayer(this.GetGame());
  let broadcaster: ref<StimBroadcasterComponent>;
  if active {
    if !this.IsAttached() || !this.IsActive() || !IsDefined(player) { return; };
    this.m_vrWeaponAimUntil = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame())) + 0.60;
    if this.m_vrWeaponAimActive { return; };
    this.m_vrWeaponAimActive = true;
    this.m_vrWeaponAimToken += 1u;
    this.VRPortScheduleAimExpiry(0.60);
  } else {
    if !this.m_vrWeaponAimActive { return; };
    this.m_vrWeaponAimActive = false;
    this.m_vrWeaponAimUntil = 0.00;
    this.m_vrWeaponAimToken += 1u;
  };
  if !IsDefined(player) { return; };
  broadcaster = player.GetStimBroadcasterComponent();
  if !IsDefined(broadcaster) { return; };
  if active {
    broadcaster.SendDrirectStimuliToTarget(this, gamedataStimType.AimingAt, this);
    if this.IsCharacterCivilian() { NPCPuppet.TutorialAddIllegalActionFact(this); };
  } else if !this.m_wasAimedAtLast && !this.m_wasCWChargedAtLast
      && !this.m_wasMeleeChargedAtLast && !this.m_wasChargingGadgetAtLast {
    broadcaster.SendDrirectStimuliToTarget(this, gamedataStimType.StopedAiming, this);
  };
}

@addMethod(NPCPuppet)
protected cb func OnVRWeaponAimExpiryEvent(evt: ref<VRWeaponAimExpiryEvent>) -> Bool {
  let remaining: Float;
  if evt.token != this.m_vrWeaponAimToken || !this.m_vrWeaponAimActive { return false; };
  remaining = this.m_vrWeaponAimUntil - EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  if remaining > 0.00 {
    this.VRPortScheduleAimExpiry(remaining);
  } else {
    this.VRPortSetWeaponAim(false);
  };
}

// Native reactions otherwise require ADS and the camera's CurrentVisibleTarget.
// Keep those rules for normal aim; only this leased NPC accepts the VR barrel.
@wrapMethod(ReactionManagerComponent)
private final const func IsPlayerAiming() -> Bool {
  let npc = this.GetOwner() as NPCPuppet;
  return (IsDefined(npc) && npc.VRPortIsWeaponAimedAt()) || wrappedMethod();
}
