// CyberpunkVRPort — neutralise game animations that fight VR.
//
// We keep gameplay systems intact (ADS, weapon stats, recoil math, reload logic) and only kill
// the visual anim features that drag the camera/hands. The user controls hand and head position
// with the controllers; the game must not animate them on top.

module CyberpunkVRPort.NoAnims

// EXPERIMENT (post-sprint pistol twirl): the itemType = -1 sentinel below is NON-VANILLA.
// Vanilla always publishes a VALID weapon-type index (ItemType().AnimFeatureIndex()); -1 means
// "no weapon type" and was used to force a neutral/empty-hands upper-body pose so the weapon
// hold pose wouldn't shift the shoulder against VRIK. But on the run->idle weapon RETURN blend
// the animgraph mishandles the invalid -1 and plays a flourish = the pistol "twirl" after sprint
// (confirmed: with NoAnims off the twirl is gone). So this neutral-pose publish is now a NO-OP:
// the game uses the real weapon itemType (valid) -> correct hold pose, no twirl. The shoulder/
// torso hold-pose shift is handled by VRIK + the native shoulder-anchor stabilization instead.
func VRPortPublishNeutralWeaponPose(scriptInterface: ref<StateGameScriptInterface>) -> Void {
  return;
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

// (Camera-shake channels inventory, for reference. KEPT vanilla BY USER ORDER (only bobbing,
// post-SHOT shake and melee/sprint shake are killed): on-hit/explosion/turret shake
// (PlayerPuppet.PushHitDataToGraph -> SendCameraShakeDataToGraph), knockdown/stagger shake
// (StatusEffectEvents.SendCameraShakeDataToGraph), heavy-footstep proximity shake, Breathing
// status-effect camera sway (BreathingLow/Heavy after stamina drain, gate:
// PlayerPuppet.CanApplyBreathingEffect). Native gameCameraShakeEvent receiver:
// PlayerPuppet.OnCameraShakeEvent -> same funnel — wrappable there if ever needed.)

// ============================================================================================
// 2) (REMOVED) inAirState arms-stable hack. Forcing AnimFeature_PlayerLocomotionStateMachine
//    .inAirState = true did NOT visibly stop the run/sprint arm motion (redscript can only swap
//    which upper-body POSE plays, it cannot zero the per-bone locomotion translation). The
//    stable-arms-during-run fix belongs in the native VRIK pose pass (shoulder anchor), not here.
// ============================================================================================

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
// EXPERIMENT: both equip wraps previously re-published itemType = -1 (the twirl-causing sentinel).
// Removed those publishes so the vanilla weapon itemType (set by wrappedMethod) stands. We keep the
// wraps as pass-throughs (no behavior change beyond not forcing -1) to stay minimal/reversible.
@wrapMethod(EquipmentBaseTransition)
protected const func HandleWeaponEquip(scriptInterface: ref<StateGameScriptInterface>,
                                        stateContext: ref<StateContext>,
                                        stateMachineInstanceData: StateMachineInstanceData,
                                        item: ItemID) -> Void {
  wrappedMethod(scriptInterface, stateContext, stateMachineInstanceData, item);
}

@wrapMethod(SingleWieldEvents)
public func InstantEquipHACK(stateContext: ref<StateContext>,
                             scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
}

// ============================================================================================
// 4) Sprint / jog weapon SPIN fix (PRIMARY VR fix for controller hands, g_VRBind==4).
//
//    Root cause: when sprinting (jog uses the SAME SprintEvents state — see
//    highLevelTransitions mapping sprint -> slow/normal/fastJog) the game plays the native
//    "sprint weapon-lower" pose (flat game can't sprint+shoot, so the weapon is tucked down
//    and rotated). With VRIK driving the hand bones from the controller, that lower pose
//    fights VRIK and the pistol visibly SPINS.
//
//    The game already has the exact lever we need: AnimFeature_WeaponSprintBlock.active.
//    Engine ref (locomotionTransitions.swift:1696-1700, 1702-1724): SprintEvents publishes
//    n"WeaponSprintBlock" with active = m_sprintAnimBlocked. The vanilla game only sets that
//    true for ~2s after shooting *if* CanWeaponShootWhileSprinting — i.e. active=true BLOCKS
//    the sprint weapon-lower animation and keeps the weapon UP. We want it UP the entire time.
//
//    Critically, SprintEvents.OnUpdate RE-APPLIES its own feature every frame whenever
//    m_sprintAnimBlocked changes, which is why one-shot neutralisation elsewhere loses the
//    per-frame fight. So we force-publish active=true from BOTH OnEnter (avoid a one-frame
//    pop on entry) and OnUpdate (win the per-frame fight). We call wrappedMethod first so the
//    vanilla logic runs, then overwrite the feature with our value last so ours is the one
//    that lands for the frame.
//
//    These reds mods are VR-only installs, so no separate VR gate is needed (matches how the
//    rest of this module unconditionally neutralises poses).
// ============================================================================================

func VRPortPublishWeaponSprintBlock(scriptInterface: ref<StateGameScriptInterface>) -> Void {
  if !IsDefined(scriptInterface) { return; }
  let f = new AnimFeature_WeaponSprintBlock();
  f.active = true;   // true => block the sprint weapon-lower anim => weapon stays UP for VRIK
  AnimationControllerComponent.ApplyFeatureToReplicate(scriptInterface.executionOwner, n"WeaponSprintBlock", f);
}

@wrapMethod(SprintEvents)
public func OnEnter(stateContext: ref<StateContext>,
                    scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  VRPortPublishWeaponSprintBlock(scriptInterface);
}

@wrapMethod(SprintEvents)
protected func OnUpdate(timeDelta: Float,
                        stateContext: ref<StateContext>,
                        scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(timeDelta, stateContext, scriptInterface);
  // Re-publish AFTER vanilla OnUpdate so our active=true wins every frame.
  VRPortPublishWeaponSprintBlock(scriptInterface);
}

// THE post-sprint twirl fix. Vanilla SprintEvents.OnExit / OnForcedExit force
// m_sprintAnimBlocked = false and re-apply it (locomotionTransitions.swift:1818-1819, 1849-1850),
// which UN-BLOCKS the sprint weapon-lower. The locomotion animgraph then blends the weapon back
// from the lowered/sprint pose to ready and plays the pistol raise/settle FLOURISH -- the 1-2
// rotation "twirl" that lasts ~2s and is visible even with VRIK OFF (pure vanilla animgraph clip).
// Our OnEnter/OnUpdate wraps above kept active=true only DURING sprint, so the exit un-block still
// fired the twirl. Here we re-assert active=true AFTER the vanilla exit runs: nothing outside
// SprintEvents ever sets the feature false, so it stays true through the exit blend window and the
// weapon is never seen lowering/raising -> no twirl. OnExit also covers OnExitToJump /
// OnExitToChargeJump (both call this.OnExit). OnForcedExit is the interrupt path (e.g. ADS / jump).
@wrapMethod(SprintEvents)
public func OnExit(stateContext: ref<StateContext>,
                   scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  VRPortPublishWeaponSprintBlock(scriptInterface);
}

@wrapMethod(SprintEvents)
public func OnForcedExit(stateContext: ref<StateContext>,
                         scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  VRPortPublishWeaponSprintBlock(scriptInterface);
}

// ============================================================================================
// 5) Disable camera / head BOBBING -> VR hands stay 1:1 with the controllers during run.
//
//    WHY: VRIK anchors the hand target to the FPP camera world pose (CET getCameraWorldPose ->
//    GetFPPCameraComponent:GetLocalToWorld). CP2077 adds procedural CAMERA BOB during locomotion;
//    dxgi locks the rendered VIEW to the HMD, but VRIK reads the RAW bobbing camera, so the HANDS
//    bob during run while the view does not -- exactly the "arms move when running" the user sees.
//    Native VR games have no camera bob, so hands are rock-stable.
//
//    The game already exposes the lever: the animgraph input n"disable_camera_bobbing", set from
//    the TweakDB flag player.camera.disableHeadBobbing (PlayerPuppet.UpdatePlayerSettings) and
//    toggled by state transitions (DefaultTransition/PlayerPuppet.DisableCameraBobbing). Those are
//    the ONLY three sites that write that input. We force ALL of them to TRUE so the FPP camera
//    never bobs -> camModelPos is stable -> hands stay on the controllers. Purely visual; no
//    gameplay/aim/recoil impact.
// ============================================================================================

@wrapMethod(DefaultTransition)
protected final func DisableCameraBobbing(stateContext: ref<StateContext>,
                                          scriptInterface: ref<StateGameScriptInterface>,
                                          b: Bool) -> Void {
  wrappedMethod(stateContext, scriptInterface, true);   // force bob OFF regardless of state
}

@wrapMethod(PlayerPuppet)
private final func DisableCameraBobbing(b: Bool) -> Void {
  wrappedMethod(true);
}

@wrapMethod(PlayerPuppet)
private final func UpdatePlayerSettings() -> Void {
  wrappedMethod();
  // Re-assert after the vanilla settings read so the menu's head-bob setting can't turn it back on.
  AnimationControllerComponent.SetInputBool(this, n"disable_camera_bobbing", true);
  // MELEE CAMERA SHAKE -> 0. The animgraph input 'melee_camera_shake_weight' (vanilla: TweakDB
  // player.camera.meleeCameraShakeWeight, default 1.0; this wrap is its ONLY script write site)
  // weights the melee-context camera additive: swing sway, the post-sprint settle with a blade,
  // etc. In VR that additive leaks into the view two ways: dxgi derives the view YAW base from
  // the game camera quat (the wobble = motion sickness the user reported after sprinting with
  // the katana), and VRIK anchors hands to camModelPos (hands would sway with it). Same
  // kill-at-the-animgraph-source pattern as the bobbing input above.
  AnimationControllerComponent.SetInputFloat(this, n"melee_camera_shake_weight", 0.0);
}

// ============================================================================================
// 6) POST-SHOT recoil camera kick -> 0. (User-ordered scope: ONLY bobbing, post-shot shake and
//    the melee/sprint shake are killed — on-hit / knockdown / footstep / breathing camera
//    motion stays vanilla, see the inventory note in section 1.)
//
//    ReadyEvents.OnTick (weaponTransitions.script:974..1010) republishes
//    AnimFeature_WeaponHandlingStats into the 'WeaponHandlingData' animgraph slot every tick
//    the weapon is ready: weaponRecoil = RecoilAnimation stat, weaponSpread = SpreadAnimation
//    stat. weaponRecoil weights the per-shot recoil ADDITIVE the graph plays on each shot
//    (FPP camera kick + weapon-mesh kick) — pure presentation; the ballistic recoil/spread
//    STATS live elsewhere and are untouched. In VR the kick leaks into the view as yaw wobble
//    (dxgi derives the view yaw base from the game camera quat) and fights VRIK's
//    controller-locked weapon pose => "shake after every shot". Re-publish AFTER the vanilla
//    tick with weaponRecoil = 0 so ours lands last (the same per-frame-win pattern as
//    WeaponSprintBlock above); weaponSpread is recomputed exactly like vanilla and kept.
// ============================================================================================

@wrapMethod(ReadyEvents)
protected func OnTick(timeDelta: Float, stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(timeDelta, stateContext, scriptInterface);
  let f = new AnimFeature_WeaponHandlingStats();
  f.weaponRecoil = 0.0;
  f.weaponSpread = GameInstance.GetStatsSystem(scriptInterface.GetGame())
    .GetStatValue(Cast<StatsObjectID>(scriptInterface.ownerEntityID), gamedataStatType.SpreadAnimation);
  scriptInterface.SetAnimationParameterFeature(n"WeaponHandlingData", f, scriptInterface.executionOwner);
}
