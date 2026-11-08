// VR TRAINING FACTS that the flat game writes from player state-machine states a
// VR stroke or guard never enters.
//
// melee_deflect_tutorial: the flat game adds it on entering MeleeDeflectAttack,
// the counter that follows a successful deflect (meleeTransitions
// MeleeDeflectAttackEvents, through TutorialAddFact). The VR guard parries
// through the IsDeflecting stat without the Deflect state, so a parried melee hit
// on the player adds it here. The native Deflect state (PSM Melee = 2) keeps its
// own path, so a hit is never counted twice.
@wrapMethod(DamageSystem)
private func ModifyHitData(hitEvent: ref<gameHitEvent>) -> Void {
  let player: ref<PlayerPuppet>;
  let quests: ref<QuestsSystem>;
  let psm: ref<IBlackboard>;
  wrappedMethod(hitEvent);
  if hitEvent.projectionPipeline || !IsDefined(hitEvent.attackData) || !hitEvent.attackData.WasDeflected() { return; };
  if !AttackData.IsMelee(hitEvent.attackData.GetAttackType()) { return; };
  player = hitEvent.target as PlayerPuppet;
  if !IsDefined(player) { return; };
  psm = GameInstance.GetBlackboardSystem(player.GetGame()).GetLocalInstanced(player.GetEntityID(), GetAllBlackboardDefs().PlayerStateMachine);
  if IsDefined(psm) && psm.GetInt(GetAllBlackboardDefs().PlayerStateMachine.Melee) == 2 { return; };
  quests = GameInstance.GetQuestsSystem(player.GetGame());
  if quests.GetFact(n"disable_tutorials") == 0 {
    quests.SetFact(n"melee_deflect_tutorial", quests.GetFact(n"melee_deflect_tutorial") + 1);
  };
}
