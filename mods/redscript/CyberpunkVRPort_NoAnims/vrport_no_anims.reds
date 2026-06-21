// CyberpunkVRPort — neutralise game animations that fight VR.
//
// We keep gameplay systems intact (ADS, weapon stats, recoil math, reload logic) and only kill
// the visual anim features that drag the camera/hands. The user controls hand and head position
// with the controllers; the game must not animate them on top.

module CyberpunkVRPort.NoAnims

func VRPortPublishNeutralWeaponPose(scriptInterface: ref<StateGameScriptInterface>) -> Void {
  if !IsDefined(scriptInterface) { return; }

  let right = new AnimFeature_EquipUnequipItem();
  right.itemState = 2;
  right.itemType = -1;
  right.stateTransitionDuration = 0.0;

  let left = new AnimFeature_EquipUnequipItem();
  left.itemState = 2;
  left.itemType = -1;
  left.stateTransitionDuration = 0.0;

  scriptInterface.SetAnimationParameterFeature(n"rightHandItemHandling", right);
  scriptInterface.SetAnimationParameterFeature(n"leftHandItemHandling", left);
  scriptInterface.SetAnimationParameterFeature(n"rightHandItemHandling", right, scriptInterface.executionOwner);
  scriptInterface.SetAnimationParameterFeature(n"leftHandItemHandling", left, scriptInterface.executionOwner);
}

// ============================================================================================
// 1) Weapon equip / unequip: instant. Stops the camera pull-back & cinematic on every draw.
// ============================================================================================

@wrapMethod(EquipmentBaseTransition)
protected const func GetWeaponEquipDuration(const scriptInterface: ref<StateGameScriptInterface>,
                                            const stateContext: ref<StateContext>,
                                            stateMachineInstanceData: StateMachineInstanceData) -> Float {
  return 0.0;
}

@wrapMethod(EquipmentBaseTransition)
protected const func GetWeaponUnEquipDuration(const scriptInterface: ref<StateGameScriptInterface>,
                                              stateContext: ref<StateContext>,
                                              stateMachineInstanceData: StateMachineInstanceData) -> Float {
  return 0.0;
}

// No first-equip cinematic (long camera/hand animation the first time each weapon is drawn).
@wrapMethod(FirstEquipSystem)
public final func HasPlayedFirstEquip(itemID: TweakDBID) -> Bool {
  return true;
}

// (Camera-shake events are a native `Entity.QueueEvent` so they can't be wrapped at the event
// queue. Camera shake suppression — if still desired — would have to go through the rumble /
// effect system instead. Skipping for now; the equip / ADS / sprint fixes are the heavy hitters.)

// ============================================================================================
// 2) Sprint / jog hand swing animation (empty hands AND with weapons): the locomotion
//    state machine drives a procedural body-bob + camera offset cycle while running and
//    sprinting which makes arms swing left-right. VRIK writes hand BONE ROTATIONS but the
//    upper-arm and chest TRANSLATIONS still come from the locomotion clip — so the whole
//    arm chain visibly swings with the body. Forcing the LocomotionStateMachine into
//    `inAirState = true` makes the animgraph play the AIR-FALL upper-body pose (arms hang
//    motionless to the sides) instead of the ground-cycle, even while the lower body keeps
//    its sprint/run animation for locomotion. Result: legs still move, body still moves,
//    but arms stop swinging — exactly what VR wants.
// ============================================================================================

@wrapMethod(LocomotionGroundEvents)
public func OnEnter(stateContext: ref<StateContext>,
                    scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  let f = new AnimFeature_PlayerLocomotionStateMachine();
  f.inAirState = true;
  scriptInterface.SetAnimationParameterFeature(n"LocomotionStateMachine", f);
  VRPortPublishNeutralWeaponPose(scriptInterface);
}

// ============================================================================================
// 3) Weapon "ready" / holding pose: when a weapon is equipped the game tells the animgraph
//    which weapon-type-specific upper-body POSE to play (handgun grip, rifle grip, etc.) via
//    AnimFeature_EquipUnequipItem.itemType. This pose is what visibly "holds" the weapon in
//    front of the chest. In VR the user is holding the actual controller; the game's pose
//    fights with VRIK by shifting the shoulder/spine, and the whole arm chain shifts with it.
//
//    We override every right-hand item-handling feature publish so itemType = -1 (no weapon
//    type known to animgraph) -> animgraph plays the neutral idle/empty-hands pose for the
//    upper body, the weapon attachment slot keeps the mesh in the hand (so you still hold it),
//    and VRIK is free to put the hand wherever the controller is.
// ============================================================================================

// Two call sites that publish rightHandItemHandling with the weapon's type:
//   * EmptyHandsEvents (extended by SingleWieldEvents): HandleWeaponEquip in equipment.script
//   * SingleWieldEvents.InstantEquipHACK (in upperBodyTransitions.script) — fired when equip
//     duration is 0, which is what our own EquipDuration=0 override forces every time
// We wrap BOTH so whichever path runs, the published itemType becomes -1 (neutral pose).
@wrapMethod(EquipmentBaseTransition)
protected const func HandleWeaponEquip(scriptInterface: ref<StateGameScriptInterface>,
                                        stateContext: ref<StateContext>,
                                        stateMachineInstanceData: StateMachineInstanceData,
                                        item: ItemID) -> Void {
  wrappedMethod(scriptInterface, stateContext, stateMachineInstanceData, item);
  let neutral = new AnimFeature_EquipUnequipItem();
  neutral.itemState = 1;
  neutral.itemType  = -1;
  neutral.stateTransitionDuration = 0.0;
  scriptInterface.SetAnimationParameterFeature(n"rightHandItemHandling", neutral, scriptInterface.executionOwner);
  VRPortPublishNeutralWeaponPose(scriptInterface);
}

@wrapMethod(SingleWieldEvents)
public func InstantEquipHACK(stateContext: ref<StateContext>,
                             scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  let neutral = new AnimFeature_EquipUnequipItem();
  neutral.itemState = 2;     // "ready" state (matches the original)
  neutral.itemType  = -1;
  neutral.stateTransitionDuration = 0.0;
  scriptInterface.SetAnimationParameterFeature(n"rightHandItemHandling", neutral);
  VRPortPublishNeutralWeaponPose(scriptInterface);
}
