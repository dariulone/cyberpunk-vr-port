// Both ready checks in mq011_00_gun_range (392 and 1366) wait for UpperBody.Aim.
// Feed the physical pistol pose into the normal aim decision while that objective is active.
public native func VRWilsonRangeTarget() -> NodeRef;

public class VRWilsonRangePoll extends DelayCallback {
  public let decisions: wref<AimingStateDecisions>;

  public func Call() -> Void {
    if IsDefined(this.decisions) { this.decisions.VRUpdateRangeAim(this); };
  }
}

@addField(AimingStateDecisions)
private let vrRangeReady: wref<JournalEntry>;
@addField(AimingStateDecisions)
private let vrRangePoll: ref<VRWilsonRangePoll>;
@addField(AimingStateDecisions)
private let vrRangeDelay: DelayID;
@addField(AimToggleListener)
public let vrRangeAim: Bool;

@wrapMethod(AimToggleListener)
public final const func GetADS() -> Bool {
  return this.vrRangeAim || wrappedMethod();
}

@wrapMethod(AimingStateDecisions)
protected final func OnAttach(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  let journal = GameInstance.GetJournalManager(this.m_executionOwner.GetGame());
  this.vrRangeReady = journal.GetEntryByString("quests/minor_quest/mq011_wilson/01_competition/00b_ready", "gameJournalQuestObjective");
  if IsDefined(this.vrRangeReady) {
    journal.RegisterScriptCallback(this, n"VRWilsonObjectiveChanged", gameJournalListenerType.State);
    this.VRWatchRangeAim(Equals(journal.GetEntryState(this.vrRangeReady), gameJournalEntryState.Active));
  };
}

@wrapMethod(AimingStateDecisions)
protected final func OnDetach(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Void {
  GameInstance.GetJournalManager(this.m_executionOwner.GetGame()).UnregisterScriptCallback(this, n"VRWilsonObjectiveChanged");
  this.VRWatchRangeAim(false);
  this.vrRangeReady = null;
  wrappedMethod(stateContext, scriptInterface);
}

@addMethod(AimingStateDecisions)
protected cb func VRWilsonObjectiveChanged(hash: Uint32, className: CName, notify: JournalNotifyOption, change: JournalChangeType) -> Bool {
  if Equals(className, n"gameJournalQuestObjective") && IsDefined(this.vrRangeReady) {
    let journal = GameInstance.GetJournalManager(this.m_executionOwner.GetGame());
    if hash == Cast<Uint32>(journal.GetEntryHash(this.vrRangeReady)) {
      this.VRWatchRangeAim(Equals(journal.GetEntryState(this.vrRangeReady), gameJournalEntryState.Active));
    };
  };
}

@addMethod(AimingStateDecisions)
private final func VRWatchRangeAim(active: Bool) -> Void {
  if Equals(active, IsDefined(this.vrRangePoll)) { return; };
  if active {
    this.vrRangePoll = new VRWilsonRangePoll();
    this.vrRangePoll.decisions = this;
    this.VRUpdateRangeAim(this.vrRangePoll);
  } else {
    GameInstance.GetDelaySystem(this.m_executionOwner.GetGame()).CancelCallback(this.vrRangeDelay);
    this.vrRangePoll = null;
    this.VRSetRangeAim(false);
  };
}

@addMethod(AimingStateDecisions)
private final func VRSetRangeAim(aim: Bool) -> Void {
  if NotEquals(this.m_controlsListener.vrRangeAim, aim) {
    this.m_controlsListener.vrRangeAim = aim;
    this.UpdateEnterConditionEnabled();
  };
}

@addMethod(AimingStateDecisions)
public final func VRUpdateRangeAim(poll: ref<VRWilsonRangePoll>) -> Void {
  if NotEquals(poll, this.vrRangePoll) { return; };
  this.vrRangeDelay = GameInstance.GetDelaySystem(this.m_executionOwner.GetGame()).DelayCallback(poll, 0.10);
  this.VRSetRangeAim(VRWilsonAimedDownrange(this.m_executionOwner));
}

public func VRWilsonAimedDownrange(player: ref<GameObject>) -> Bool {
  let hands = VRMeleeInputFrame(); // support hand, then primary hand, with W > 0 for fresh tracking
  let transaction = GameInstance.GetTransactionSystem(player.GetGame());
  let gun = transaction.GetItemInSlot(player, VRLeftHandedWeapon.HeldSlot(player)) as WeaponObject;
  let targetTransform: Transform;
  if ArraySize(hands) < 3 || hands[2].W <= 0.0 || !IsDefined(gun) ||
      !(transaction.HasTag(player, n"Handgun", gun.GetItemID()) || transaction.HasTag(player, n"Revolver", gun.GetItemID())) {
    return false;
  };
  let refID = ResolveNodeRefWithEntityID(VRWilsonRangeTarget(), player.GetEntityID());
  if !GameInstance.GetNodeTransform(player.GetGame(), refID, targetTransform) { return false; };
  let muzzle = gun.GetMuzzleSlotWorldTransform();
  let toTarget = Transform.GetPosition(targetTransform) - Transform.GetPosition(muzzle);
  // The scene uses a 15-degree cone around this same target for its focus check.
  return Vector4.Dot(Transform.GetForward(muzzle), Vector4.Normalize(toTarget)) >= CosF(Deg2Rad(15.0));
}
