// CyberpunkVRPort -- the holster's intent and its equip request, shared: the Melee and Cyberware modules ask
// VRHolsterIntentActive, the plugin calls VRHolsterMelee; the module's equip helpers are Rust's in the Rust build
// (rust/crates/cvr-ffi/src/mechanics/holster.rs).

module CyberpunkVRPort.Holster

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
