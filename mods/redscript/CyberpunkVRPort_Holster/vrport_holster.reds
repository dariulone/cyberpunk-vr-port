// CyberpunkVRPort — hand-to-visual-holster equip (redscript helpers).
//
// CET (CyberpunkVRPort_Holster/init.lua) does the per-frame detection (grip rising edge + wrist
// proximity to body zone + presence of a VisualHolster-tagged clothing item). These helpers expose
// the bone world-positions the CET side cannot read directly, and queue the equip request.

module CyberpunkVRPort.Holster

// World-space position of a bone / attachment slot on the player puppet's SlotComponent (e.g.
// 'RightHand', 'LeftHand', 'RightUpLeg', 'LeftUpLeg'). Returns (0,0,0,0) if the slot does not
// resolve, so the caller can gracefully skip it.
@addMethod(PlayerPuppet)
public func VRGetSlotWorldPos(slotName: CName) -> Vector4 {
  let slotComp = this.GetSlotComponent();
  if !IsDefined(slotComp) { return new Vector4(0.0, 0.0, 0.0, 0.0); }
  let xform: WorldTransform;
  if !slotComp.GetSlotTransform(slotName, xform) { return new Vector4(0.0, 0.0, 0.0, 0.0); }
  return WorldPosition.ToVector4(WorldTransform.GetWorldPosition(xform));
}

// Send any weapon-equip request through the game's own EquipmentSystem. This is the same path
// QuickSlotsManager.HideWeapon and the vanilla holster button take: build an
// EquipmentSystemWeaponManipulationRequest, set the action and owner, queue it on the system.
@addMethod(PlayerPuppet)
private func VRSendEquipRequest(action: EquipmentManipulationAction) -> Void {
  let req = new EquipmentSystemWeaponManipulationRequest();
  req.requestType = action;
  req.owner = this;
  let eqSystem = GameInstance.GetScriptableSystemsContainer(this.GetGame()).Get(n"EquipmentSystem") as EquipmentSystem;
  if IsDefined(eqSystem) { eqSystem.QueueRequest(req); }
}

// Equip the FIRST melee WEAPON (the actual blade, e.g. the katana). NOT RequestLastUsedOrFirstAvailable
// — that pulls whatever melee was used last, which is the cyberware "power" arms (Gorilla Arms / Mantis
// Blades) if the player just fought with them. The left visual holster IS the katana, so reaching to it
// must always draw the katana, never the cyberware arms.
@addMethod(PlayerPuppet)
public func VREquipMeleeWeapon() -> Void {
  this.VRSendEquipRequest(EquipmentManipulationAction.RequestFirstMeleeWeapon);
}

// Equip the LAST-USED-or-FIRST-AVAILABLE ranged weapon (any kind: pistol, rifle, sniper).
@addMethod(PlayerPuppet)
public func VREquipRangedWeapon() -> Void {
  this.VRSendEquipRequest(EquipmentManipulationAction.RequestLastUsedOrFirstAvailableRangedWeapon);
}

// Equip the LAST-USED-or-FIRST-AVAILABLE ONE-HANDED ranged weapon — pistol / revolver only,
// excluding rifles / SMGs / snipers. Used for the hip-pistol visual holster zone so reaching to
// the hip doesn't accidentally pull out the sniper rifle (which is the "last used ranged" because
// the user fired it most recently).
@addMethod(PlayerPuppet)
public func VREquipOneHandedRangedWeapon() -> Void {
  this.VRSendEquipRequest(EquipmentManipulationAction.RequestLastUsedOrFirstAvailableOneHandedRangedWeapon);
}

// Equip weapon wheel slot 1 — the player's PRIMARY weapon (typically the rifle / sniper they keep
// slung over the shoulder). Used for the reach-over-right-shoulder gesture so it always gives the
// long gun, not whichever ranged weapon was last used.
@addMethod(PlayerPuppet)
public func VREquipPrimaryWeapon() -> Void {
  this.VRSendEquipRequest(EquipmentManipulationAction.RequestWeaponSlot1);
}

// Weapon-wheel slots 2 and 3, used by the NON-immersive holster mode (visual holsters
// ignored). Over-shoulder -> slot 1 (VREquipPrimaryWeapon above), right hip -> slot 2,
// left hip -> slot 3. RequestWeaponSlotN both equips that slot and, if it is already the
// active weapon, the CET side issues UnequipWeapon instead (same toggle as immersive).
@addMethod(PlayerPuppet)
public func VREquipWeaponSlot2() -> Void {
  this.VRSendEquipRequest(EquipmentManipulationAction.RequestWeaponSlot2);
}

@addMethod(PlayerPuppet)
public func VREquipWeaponSlot3() -> Void {
  this.VRSendEquipRequest(EquipmentManipulationAction.RequestWeaponSlot3);
}

// Unequip a RANGED weapon — same as the vanilla holster button. UnequipWeapon politely tells the
// active weapon's state machine to holster; that works for guns.
@addMethod(PlayerPuppet)
public func VRUnequipWeapon() -> Void {
  this.VRSendEquipRequest(EquipmentManipulationAction.UnequipWeapon);
}

// Timestamp (sim time) of the last VR melee-holster intent. Read by the VR motion-melee module
// (VRMeleeBladeHit) to SUPPRESS swing-attacks for a short window, and optionally by WeaponUp.
@addField(PlayerPuppet) let m_vrHolsterIntentTime: Float;

// Put a MELEE weapon (katana etc.) away. The native holster button (B / double-tap Y) sends the SAME
// EquipmentManipulationAction.UnequipWeapon we do (QuickSlotsManager.HideWeapon / upperBodyTransitions
// both build owner=player + UnequipWeapon + QueueRequest identically). The reason OUR request was being
// dropped for melee but not for guns: reaching the hand to the holster zone is hand MOTION, which our
// VR motion-melee picks up as a swing and fires a melee attack — that puts melee into an attack/busy
// state, and a same-frame UnequipWeapon is rejected. A gun never swings, so its unequip goes through.
// Fix: stamp a holster-intent time so VRMeleeBladeHit suppresses the swing for ~1s, THEN unequip.
@addMethod(PlayerPuppet)
public func VRHolsterMelee() -> Void {
  this.m_vrHolsterIntentTime = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  this.VRSendEquipRequest(EquipmentManipulationAction.UnequipWeapon);
}

// True if a melee holster was requested within the last `window` seconds (used to suppress VR swings
// so reaching for the holster doesn't fire an attack that blocks the unequip).
@addMethod(PlayerPuppet)
public func VRHolsterIntentActive(window: Float) -> Bool {
  let now = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  return (now - this.m_vrHolsterIntentTime) >= 0.0 && (now - this.m_vrHolsterIntentTime) < window;
}
