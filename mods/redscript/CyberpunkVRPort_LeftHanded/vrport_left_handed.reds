// CyberpunkVRPort -- LEFT-HANDED, THE WEAPON IS THE LEFT HAND'S (xr_left_handed).
//
// The game knows two hands. A weapon drawn on its own goes to the right one: EquipmentSystemPlayerData decides
// which hand holds what (GetItemIDfromEquipmentManipulationAction -> rightHandItem / leftHandItem), and each hand's
// equipment state machine (RightHand / LeftHand) attaches its item to its own slot -- AttachmentSlots.WeaponRight or
// WeaponLeft -- and runs that slot's weapon state machine (WeaponRight / WeaponLeft). The left hand's chain is the
// one dual wielding uses, complete: its own shoot state, reload, recoil, ammo, safe state and animation.
//
// With the left-handed controls on, a weapon drawn on its own is given to the LEFT hand at that one decision, and
// everything downstream is the game's own left-hand path. The right hand stays empty; two guns stay as the game
// pairs them (the partner in slot 4 is the left one), and an implanted arm, fists or a heavy weapon are anatomy or
// both-handed and keep their hands.
//
// The game never draws a weapon in the left hand alone, so some of its rules assume a right-hand weapon beside it:
//   * the left hand may not equip while the right one is unequipped (CanEquipRequestedWeapon);
//   * the weapon to re-request or put away is the right hand's (GetSlotActiveWeapon / GetActiveWeaponToUnequip);
//   * a firearm in the left hand is the second of a pair (IsDualWielding, the dual_wield animation wrapper);
//   * the weapon in hand is the one on WeaponRight -- to the upper body (singleWield / emptyHands) and to the HUD.
// Each answers for the left hand here while it holds the only weapon. Raising and lowering it is
// CyberpunkVRPort_NoAnims/ControlLowerAndHolsterWeapons.reds (VRPortFirearm).
//
// THE WEAPON ALONE IN THE LEFT HAND ANIMATES AS A SINGLE WIELD. The ranged graph's single-wield branch is the right
// slot's; the left slot drives the branch of a pair's second gun. While the weapon is alone in the left hand the
// plugin hands its animation names to the single-wield branch (src/Hooks/LeftHandedAnimation.cpp), and the
// single-wield animations that branch plays are the right hand's own mirrored onto the left (tools/lefthanded_anims),
// on the player's root component under the anim wrapper vr_left_handed -- played in the graph the right hand's weapon
// state machine attaches as it starts, which the plugin attaches for the left hand's draw. All of it is set here, at
// each draw, before the draw looks its animation up.
//
// This file intentionally stays in the global namespace: the plugin registers plain global natives.

native func VRLeftHanded() -> Bool;
native func VRLeftHandedAnimations(controller: ref<IScriptable>, on: Bool, graph: CName, slot: CName) -> Void;

public abstract class VRLeftHandedWeapon {
  // A weapon that goes to the left hand when it is drawn on its own: a hand-held weapon that may hang on that slot.
  public static func Movable(item: ItemID) -> Bool {
    let record: ref<Item_Record>;
    let slots: array<wref<AttachmentSlot_Record>>;
    let i: Int32;
    if !ItemID.IsValid(item) || WeaponObject.IsCyberwareWeapon(item) || WeaponObject.IsFists(item) {
      return false;
    };
    record = TweakDBInterface.GetItemRecord(ItemID.GetTDBID(item));
    if !IsDefined(record) || NotEquals(record.ItemCategory().Type(), gamedataItemCategory.Weapon) {
      return false;
    };
    if Equals(EquipmentSystem.GetEquipAreaType(item), gamedataEquipmentArea.WeaponHeavy) {
      return false;
    };
    record.PlacementSlots(slots);
    i = 0;
    while i < ArraySize(slots) {
      if slots[i].GetID() == t"AttachmentSlots.WeaponLeft" {
        return true;
      };
      i += 1;
    };
    return false;
  }

  // The left hand holds the only weapon, as the equipment system decided each hand's item.
  public static func Alone(owner: ref<GameObject>) -> Bool {
    return VRLeftHandedWeapon.Movable(EquipmentSystem.GetSlotActiveItem(owner, EquipmentManipulationRequestSlot.Left))
      && !VRLeftHandedWeapon.IsWeapon(EquipmentSystem.GetSlotActiveItem(owner, EquipmentManipulationRequestSlot.Right));
  }

  public static func IsWeapon(item: ItemID) -> Bool {
    let record: ref<Item_Record>;
    if !ItemID.IsValid(item) {
      return false;
    };
    record = TweakDBInterface.GetItemRecord(ItemID.GetTDBID(item));
    return IsDefined(record) && Equals(record.ItemCategory().Type(), gamedataItemCategory.Weapon);
  }

  // The weapon in hand is the left one: the slots hold a weapon on WeaponLeft and none on WeaponRight. Where the game
  // asks "the weapon in hand" of the right slot, this is the left hand's answer.
  public static func InHand(owner: ref<GameObject>) -> Bool {
    let ts: ref<TransactionSystem>;
    let left: ref<WeaponObject>;
    if !VRLeftHanded() || !IsDefined(owner) {
      return false;
    };
    ts = GameInstance.GetTransactionSystem(owner.GetGame());
    left = ts.GetItemInSlot(owner, t"AttachmentSlots.WeaponLeft") as WeaponObject;
    return IsDefined(left) && !IsDefined(ts.GetItemInSlot(owner, t"AttachmentSlots.WeaponRight") as WeaponObject)
      && VRLeftHandedWeapon.Movable(left.GetItemID());
  }

  // The slot of the weapon in hand: the left one while it holds the only weapon (InHand), the right one otherwise.
  public static func HeldSlot(owner: ref<GameObject>) -> TweakDBID {
    return VRLeftHandedWeapon.InHand(owner) ? t"AttachmentSlots.WeaponLeft" : t"AttachmentSlots.WeaponRight";
  }

  // The single wield on the left hand -- its animations, the names that drive them and the graph they play in --
  // while the weapon is alone there, as the equipment system decided each hand's item; off for anything else, a pair
  // or a right-hand weapon. The graph is the one the right hand's weapon state machine attaches as it starts.
  public static func SetAnimations(owner: ref<GameObject>) -> Void {
    let on: Bool = VRLeftHanded() && VRLeftHandedWeapon.Alone(owner);
    let puppet: ref<ScriptedPuppet> = owner as ScriptedPuppet;
    AnimationControllerComponent.SetAnimWrapperWeight(owner, n"vr_left_handed", on ? 1.00 : 0.00);
    if IsDefined(puppet) {
      VRLeftHandedAnimations(puppet.GetAnimationControllerComponent(), on,
        TweakDBInterface.GetCName(t"playerStateMachineDefinitions.WeaponRight.animGraphName", n"None"),
        TweakDBInterface.GetCName(t"playerStateMachineDefinitions.WeaponRight.animGraphSlot", n"None"));
    };
  }

  // The equipment UI data as its readers take it: the weapon in hand under rightWeapon.
  public static func InHandData(value: Variant) -> Variant {
    let data: ref<SlotDataHolder> = FromVariant<ref<SlotDataHolder>>(value);
    let held: ref<SlotDataHolder>;
    if !VRLeftHanded() || !IsDefined(data) || ItemID.IsValid(data.rightWeapon.weaponID)
      || !ItemID.IsValid(data.leftWeapon.weaponID) {
      return value;
    };
    held = new SlotDataHolder();
    held.ammoData = data.ammoData;
    held.rightWeapon = data.leftWeapon;
    return ToVariant(held);
  }
}

// THE ONE DECISION: which hand holds what after this request. A weapon alone in the right hand goes to the left.
@wrapMethod(EquipmentSystemPlayerData)
public final func GetItemIDfromEquipmentManipulationAction(eqManipulationAction: EquipmentManipulationAction, currentState: SSlotActiveItems) -> SSlotActiveItems {
  let state: SSlotActiveItems = wrappedMethod(eqManipulationAction, currentState);
  if VRLeftHanded() && VRLeftHandedWeapon.Movable(state.rightHandItem) && !VRLeftHandedWeapon.IsWeapon(state.leftHandItem) {
    state.leftHandItem = state.rightHandItem;
    state.rightHandItem = ItemID.None();
  };
  return state;
}

// ...AND ITS SECOND PLACE: a weapon equipped straight into the hand -- the game's inventory equipping a weapon, a quest
// handing one over (EquipRequest / GameplayEquipRequest with forceEquipWeapon) -- is given to a hand by
// HandleEquipWeapon, which sends each hand's request itself: the right one for a weapon on its own. Left-handed it is
// the left hand's, as above; the right hand's request is turned back into putting away, and the left one, empty,
// takes the weapon. A pair, a consumable or gadget in the left hand, a weapon that cannot hang there: as the game
// decided.
//
// THE WEAPON IN THE LEFT HAND EQUIPPED AGAIN WHERE IT IS CHANGES NOTHING. The game first unequips an equipped weapon
// asked for and then equips it again, and the transaction system, told the item left its equipment slot
// (OnItemRemovedFromEquipmentSlot, Cyberpunk2077.exe 2.5 RVA 0x8A180C), only marks it when it hangs on WeaponRight;
// on any other slot it takes it off. The weapon alone in the left hand stayed in the hand, detached from the world:
// its shots went through the state machine and spent nothing (measured 2026-10-29: IsAttached false after one
// EquipRequest of the Lexington in hand, 20 -> 20 rounds). For the right hand's weapon the same request ends where it
// began, so for the left hand's it is answered as done: still the active item, still the last used.
@wrapMethod(EquipmentSystemPlayerData)
private final func HandleEquipWeapon(itemID: ItemID, out slotIndex: Int32, opt blockActiveSlotsUpdate: Bool, opt forceEquipWeapon: Bool, out isLeftWeapon: Bool) -> Bool {
  let handled: Bool;
  if !blockActiveSlotsUpdate && VRLeftHanded() && VRLeftHandedWeapon.Movable(itemID)
    && this.GetSlotActiveItem(EquipmentManipulationRequestSlot.Left) == itemID
    && !ItemID.IsValid(this.GetSlotActiveItem(EquipmentManipulationRequestSlot.Right))
    && this.GetItemInEquipSlot(EquipmentSystem.GetEquipAreaType(itemID), slotIndex) == itemID {
    isLeftWeapon = false;
    this.UpdateEquipAreaActiveIndex(itemID);
    this.SetLastUsedItem(itemID);
    return true;
  };
  handled = wrappedMethod(itemID, slotIndex, blockActiveSlotsUpdate, forceEquipWeapon, isLeftWeapon);
  if handled && VRLeftHanded() && this.GetSlotActiveItem(EquipmentManipulationRequestSlot.Right) == itemID
    && VRLeftHandedWeapon.Movable(itemID) && !ItemID.IsValid(this.GetSlotActiveItem(EquipmentManipulationRequestSlot.Left)) {
    this.SetSlotActiveItem(EquipmentManipulationRequestSlot.Right, ItemID.None());
    this.SendPSMWeaponManipulationRequest(EquipmentManipulationRequestType.Unequip, EquipmentManipulationRequestSlot.Right, gameEquipAnimationType.Default);
    this.SetSlotActiveItem(EquipmentManipulationRequestSlot.Left, itemID);
    this.SendPSMWeaponManipulationRequest(EquipmentManipulationRequestType.Equip, EquipmentManipulationRequestSlot.Left, gameEquipAnimationType.Default);
  };
  return handled;
}

// A WEAPON ASKED FOR OVER THE LEFT HAND'S ONLY WEAPON IS NO TEMPORARY EQUIP. Whenever a request finds a weapon in
// the left hand, the game sends that hand's request as a temporary equip over it (tempEquipOverWeapon) -- made for a
// consumable or a gadget taken up over a pair's second gun and put back after: the left hand's state machine then
// marks its item a dual wield and the animations treat it as laid over a weapon. Left-handed, the weapon there is
// the only one, and a weapon asked for -- the same one again on the wheel, or another -- takes its place.
@wrapMethod(EquipmentSystemPlayerData)
private final func WeaponManipulation(request: ref<EquipmentSystemWeaponManipulationRequest>, item: ItemID, slot: EquipmentManipulationRequestSlot, opt isDual: Bool, opt tempEquipOverWeapon: Bool, opt dualWieldLoadoutEdit: Bool) -> Void {
  if tempEquipOverWeapon && !isDual && Equals(slot, EquipmentManipulationRequestSlot.Left) && VRLeftHanded() && VRLeftHandedWeapon.Movable(item) {
    tempEquipOverWeapon = false;
  };
  wrappedMethod(request, item, slot, isDual, tempEquipOverWeapon, dualWieldLoadoutEdit);
}

// The weapon to re-request: the left hand's while it holds the only one.
@wrapMethod(EquipmentSystemPlayerData)
public final func GetSlotActiveWeapon() -> ItemID {
  let item: ItemID = wrappedMethod();
  if !ItemID.IsValid(item) && VRLeftHanded() {
    let left: ItemID = this.GetSlotActiveItem(EquipmentManipulationRequestSlot.Left);
    if VRLeftHandedWeapon.Movable(left) {
      return left;
    };
  };
  return item;
}

// ...and the weapon to put away.
@wrapMethod(EquipmentSystemPlayerData)
public final func GetActiveWeaponToUnequip() -> ItemID {
  let left: ItemID;
  if !ItemID.IsValid(this.GetSlotActiveItem(EquipmentManipulationRequestSlot.Right)) && VRLeftHanded() {
    left = this.GetSlotActiveItem(EquipmentManipulationRequestSlot.Left);
    if VRLeftHandedWeapon.Movable(left) {
      return this.GetActiveItem(this.GetEquipAreaFromItemID(left).areaType);
    };
  };
  return wrappedMethod();
}

// THE CONTROLS TURNED OVER WITH A WEAPON OUT: it changes hands in two steps, put away and drawn again, and the draw
// goes through the decision above into the hand it now belongs in. Not in one step: a re-equip hands the same item
// from one slot to the other while both hands' state machines work on it, and with a draw request landing in that
// window the game crashed in its mesh proxy job (a component whose entity was gone, 2026-10-29). Only
// a weapon alone in the hand it no longer belongs in; two guns stay paired. Called by the weapon mod
// (CyberpunkVRPort_Weapon) when VRLeftHanded() changes; true when the weapon was put away -- the mod then draws it
// with VRLeftHandedRedraw once both hands are empty.
@addMethod(PlayerPuppet)
public func VRLeftHandedRehand() -> Bool {
  let toLeft: Bool = VRLeftHanded();
  let right: ref<WeaponObject> = GameObject.GetRightActiveWeapon(this);
  let left: ref<WeaponObject> = GameObject.GetLeftActiveWeapon(this);
  let held: ref<WeaponObject> = toLeft ? right : left;
  let other: ref<WeaponObject> = toLeft ? left : right;
  VRLeftHandedWeapon.SetAnimations(this);
  if !IsDefined(held) || IsDefined(other) || !VRLeftHandedWeapon.Movable(held.GetItemID()) {
    return false;
  };
  return this.VRLeftHandedRequest(EquipmentManipulationAction.UnequipWeapon);
}

@addMethod(PlayerPuppet)
public func VRLeftHandedHandsFree() -> Bool {
  return !IsDefined(GameObject.GetRightActiveWeapon(this)) && !IsDefined(GameObject.GetLeftActiveWeapon(this));
}

@addMethod(PlayerPuppet)
public func VRLeftHandedRedraw() -> Bool {
  return this.VRLeftHandedRequest(EquipmentManipulationAction.RequestLastUsedWeapon);
}

@addMethod(PlayerPuppet)
private func VRLeftHandedRequest(action: EquipmentManipulationAction) -> Bool {
  let request: ref<EquipmentSystemWeaponManipulationRequest>;
  let equipment: ref<EquipmentSystem> = GameInstance.GetScriptableSystemsContainer(this.GetGame()).Get(n"EquipmentSystem") as EquipmentSystem;
  if !IsDefined(equipment) {
    return false;
  };
  request = new EquipmentSystemWeaponManipulationRequest();
  request.requestType = action;
  request.owner = this;
  equipment.QueueRequest(request);
  return true;
}

// A SAVE KEEPS WHICH HAND HELD THE WEAPON (m_slotActiveItemsInHands, persistent), and a load draws only what the
// RIGHT hand had: the right hand's equipment state machine reads its item as the player starts, while the left
// hand's runs only once something asks it to. So a save made right-handed put the weapon in the right hand of a
// left-handed player, and one made left-handed loaded with no weapon out at all (both seen 2026-11-01). The
// restored hands follow the controls: a weapon alone in the hand they no longer use moves to the other one, before
// either state machine has read it. Right-handed, the game's own draw then takes it into the right hand;
// left-handed, VRLeftHandedDrawRestored draws it into the left once the world is up.
@wrapMethod(EquipmentSystemPlayerData)
public final func OnRestored() -> Void {
  let right: ItemID;
  let left: ItemID;
  wrappedMethod();
  if !IsDefined(this.m_owner as PlayerPuppet) {
    return;
  };
  right = this.m_slotActiveItemsInHands.rightHandItem;
  left = this.m_slotActiveItemsInHands.leftHandItem;
  if VRLeftHanded() {
    if VRLeftHandedWeapon.Movable(right) && !VRLeftHandedWeapon.IsWeapon(left) {
      this.m_slotActiveItemsInHands.leftHandItem = right;
      this.m_slotActiveItemsInHands.rightHandItem = ItemID.None();
    };
  } else {
    if VRLeftHandedWeapon.Movable(left) && !VRLeftHandedWeapon.IsWeapon(right) {
      this.m_slotActiveItemsInHands.rightHandItem = left;
      this.m_slotActiveItemsInHands.leftHandItem = ItemID.None();
    };
  };
}

// The left hand's weapon from a save, which the load did not draw: the equipment system has it as the left hand's
// while both hands are empty. A draw in flight looks the same for a moment (the request names the hand's item
// before the state machine attaches it), so the weapon mod asks this until it has held for a while.
@addMethod(PlayerPuppet)
public func VRLeftHandedRestorePending() -> Bool {
  let data: ref<EquipmentSystemPlayerData> = EquipmentSystem.GetData(this);
  if !VRLeftHanded() || !IsDefined(data) || !this.VRLeftHandedHandsFree() {
    return false;
  };
  return VRLeftHandedWeapon.Movable(data.GetSlotActiveItem(EquipmentManipulationRequestSlot.Left))
    && !VRLeftHandedWeapon.IsWeapon(data.GetSlotActiveItem(EquipmentManipulationRequestSlot.Right));
}

// ...and drawn the way a redraw is, so it goes through the hand decision above. True when the draw was asked for.
// Called by the weapon mod (CyberpunkVRPort_Weapon) after a load.
@addMethod(PlayerPuppet)
public func VRLeftHandedDrawRestored() -> Bool {
  return this.VRLeftHandedRestorePending() && this.VRLeftHandedRequest(EquipmentManipulationAction.RequestLastUsedWeapon);
}

// A WEAPON ALONE IN THE LEFT HAND IS NO PAIR. The game draws a firearm into the left hand only as the second of two,
// so this state marks every one a dual wield -- IsDualWielding and the owner's dual_wield animation wrapper -- and
// everything that serves a pair (the shoot routing, both triggers firing, the second muzzle) would take one gun for
// two. A lone one is marked single, the way a weapon in the right hand is. Each draw also turns the single wield on
// the left hand on or off (SetAnimations), before the draw looks its animation up.
@wrapMethod(EquipCycleInitEvents)
protected func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  VRLeftHandedWeapon.SetAnimations(scriptInterface.executionOwner);
  wrappedMethod(stateContext, scriptInterface);
  if this.IsLeftHandLogic(this.stateMachineInstanceData) && VRLeftHandedWeapon.Alone(scriptInterface.executionOwner) {
    scriptInterface.localBlackboard.SetBool(GetAllBlackboardDefs().PlayerStateMachine.IsDualWielding, false);
    AnimationControllerComponent.SetAnimWrapperWeight(scriptInterface.executionOwner, n"dual_wield", 0.00);
  };
}

// The left hand equips its weapon with the right hand empty. Everything else the left hand checks still applies --
// a menu open, the upper body busy, the right hand holding fists or an arm implant.
@wrapMethod(EquipmentBaseTransition)
protected final const func CanEquipRequestedWeapon(scriptInterface: ref<StateGameScriptInterface>, stateContext: ref<StateContext>, stateMachineInstanceData: StateMachineInstanceData) -> Bool {
  let item: ItemID;
  if wrappedMethod(scriptInterface, stateContext, stateMachineInstanceData) {
    return true;
  };
  if !VRLeftHanded() || !this.IsLeftHandLogic(stateMachineInstanceData) {
    return false;
  };
  item = this.GetSlotActiveItem(scriptInterface, stateMachineInstanceData);
  if !VRLeftHandedWeapon.Movable(item) || IsDefined(GameObject.GetRightActiveWeapon(scriptInterface.executionOwner)) {
    return false;
  };
  return this.IsRightHandInUnequippedState(stateContext, true) && this.CanProcessEquip(stateContext, scriptInterface);
}

// THE UPPER BODY TAKES THE RIGHT SLOT FOR THE WEAPON IN HAND. One weapon out is singleWield only with it on
// WeaponRight (HasSingleWeaponEquipped), and no weapon there is emptyHands -- so with the weapon in the left hand the
// upper body sat in emptyHands, and every time it entered that state again (the first aim after the draw did it) its
// entry zeroed both weapon states: LeftWeapon 5 -> 0, the shot routing back to Idle, the trigger dead until the next
// draw. Measured 2026-10-29. Each of the upper body's own questions answers here for the weapon in the left hand as
// it does for one in the right.
@wrapMethod(DefaultTransition)
public final static func HasSingleWeaponEquipped(const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return wrappedMethod(scriptInterface) || VRLeftHandedWeapon.InHand(scriptInterface.executionOwner);
}

@wrapMethod(EmptyHandsDecisions)
protected const final func EnterCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) {
    return false;
  };
  return wrappedMethod(stateContext, scriptInterface);
}

// Put away as the weapon in the right hand is: on the unequip request, or the empty-hands condition held.
@wrapMethod(SingleWieldDecisions)
protected const final func ToEmptyHands(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let broadcaster: ref<StimBroadcasterComponent>;
  if !VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) {
    return wrappedMethod(stateContext, scriptInterface);
  };
  if this.GetInStateTime() >= 0.45 && this.EmptyHandsCondition(stateContext, scriptInterface)
    || stateContext.GetBoolParameter(n"requestWeaponUnequip", false) {
    broadcaster = scriptInterface.executionOwner.GetStimBroadcasterComponent();
    if IsDefined(broadcaster) {
      broadcaster.TriggerSingleBroadcast(scriptInterface.executionOwner, gamedataStimType.WeaponHolstered);
    };
    return true;
  };
  return false;
}

@wrapMethod(UpperBodyTransition)
protected const final func EmptyHandsCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let blackboard: ref<IBlackboard>;
  let empty: Bool = wrappedMethod(stateContext, scriptInterface);
  if empty || !VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) {
    return empty;
  };
  blackboard = scriptInterface.localBlackboard;
  if this.IsInSafeSceneTier(scriptInterface)
    && (blackboard.GetBool(GetAllBlackboardDefs().PlayerStateMachine.SceneAimForced)
      || blackboard.GetBool(GetAllBlackboardDefs().PlayerStateMachine.SceneSafeForced)) {
    return false;
  };
  return stateContext.GetBoolParameter(n"requestWeaponUnequip", false);
}

@wrapMethod(ForceSafeDecisions)
protected const final func ToEmptyHands(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let autoUnequip: Float;
  if !VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) {
    return wrappedMethod(stateContext, scriptInterface);
  };
  if !this.IsSafeStateForced(stateContext, scriptInterface)
    || this.GetInStateTime() >= 0.45 && this.EmptyHandsCondition(stateContext, scriptInterface) {
    return true;
  };
  autoUnequip = this.GetStaticFloatParameterDefault("timeToAutoUnequipWeapon", -1.0);
  return !IsMultiplayer() && autoUnequip > 0.0
    && stateContext.GetConditionFloat(n"ForceSafeCurrentTimeToAutoUnequip")
      >= autoUnequip + stateContext.GetConditionFloat(n"ForceSafeTimeStampToAutoUnequip");
}

@wrapMethod(AdHocAnimationDecisions)
protected const func ToEmptyHands(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return !VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) && wrappedMethod(stateContext, scriptInterface);
}

// ...and the input hints: no exploration hints with a weapon in hand.
@wrapMethod(BaseContextEvents)
private final func IsStateValidForExploration(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  return !VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) && wrappedMethod(stateContext, scriptInterface);
}

// ...and the weapon's own hints, the reload, quick melee, dodge and crouch over the ammo counter: UpdateWeaponInputHints
// takes them from the RightHand equipment state machine and the right hand's weapon, so with the weapon alone in the
// left hand the HUD had none of them (2026-11-01). While it is, the same decisions are taken for the left hand. The
// charge hint stays the right hand's: ShowRangedInputHints reads the right weapon for it (IsChargeRangedWeapon), and
// asking for it here would ask again every frame.
@wrapMethod(InputContextTransitionEvents)
protected final func UpdateWeaponInputHints(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> ActiveBaseContext {
  if !VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) {
    return wrappedMethod(stateContext, scriptInterface);
  };
  return this.VRLeftHandedWeaponInputHints(stateContext, scriptInterface);
}

@addMethod(InputContextTransitionEvents)
private final func VRLeftHandedWeaponInputHints(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> ActiveBaseContext {
  let equipCycle: StateMachineIdentifier;
  equipCycle.definitionName = n"Equipment";
  equipCycle.referenceName = n"LeftHand";
  let context: ActiveBaseContext;
  let isExploring: Bool = this.IsInHighLevelState(stateContext, n"exploration");
  let isExaminingDevice: Bool = this.IsExaminingDevice(scriptInterface) || DefaultTransition.IsInteractingWithTerminal(scriptInterface);
  let isDeviceControlled: Bool = scriptInterface.executionOwner.GetTakeOverControlSystem().IsDeviceControlled();
  let inEquipState: Bool = DefaultTransition.IsLeftHandInEquippedState(stateContext) || stateContext.IsStateActiveWithIdentifier(equipCycle, n"equipCycle");
  let weapon: wref<WeaponObject> = GameObject.GetLeftActiveWeapon(scriptInterface.executionOwner);
  let isRangedWeaponEquipped: Bool = inEquipState && IsDefined(weapon) && weapon.IsRanged();
  let isMeleeWeaponEquipped: Bool = inEquipState && IsDefined(weapon) && !isRangedWeaponEquipped && weapon.IsMelee();
  let canDisplayChargeInputHint: Bool = isRangedWeaponEquipped && DefaultTransition.IsChargeRangedWeapon(scriptInterface);
  let canDisplayThrowInputHint: Bool = isMeleeWeaponEquipped && weapon.IsThrowable() && stateContext.IsStateActive(n"UpperBody", n"aimingState");
  let canDisplayRangedDodgeInputHint: Bool = !isRangedWeaponEquipped || !stateContext.IsStateActive(n"UpperBody", n"aimingState") || scriptInterface.HasStatFlag(gamedataStatType.CanAimWhileDodging);
  let canDisplayCrouchInputHint: Bool = NotEquals(IntEnum<gamePSMCombat>(scriptInterface.localBlackboard.GetInt(GetAllBlackboardDefs().PlayerStateMachine.Combat)), gamePSMCombat.InCombat);
  let canDisplayThrowCarriedNPCInputHint: Bool = scriptInterface.localBlackboard.GetBool(GetAllBlackboardDefs().PlayerStateMachine.CanThrowCarriedNPC);
  let isMeleeInputHintDisplayed: Bool = stateContext.GetBoolParameter(n"isMeleeInputHintDisplayed", true);
  let isThrowInputHintDisplayed: Bool = stateContext.GetBoolParameter(n"isThrowInputHintDisplayed", true);
  let isRangedInputHintDisplayed: Bool = stateContext.GetBoolParameter(n"isRangedInputHintDisplayed", true);
  let isChargeInputHintDisplayed: Bool = stateContext.GetBoolParameter(n"isChargeInputHintDisplayed", true);
  let isRangedDodgeInputHintDisplayed: Bool = stateContext.GetBoolParameter(n"isRangedDodgeInputHintDisplayed", true);
  let isCrouchInputHintDisplayed: Bool = stateContext.GetBoolParameter(n"isCrouchInputHintDisplayed", true);
  let isThrowCarriedNPCInputHintDisplayed: Bool = stateContext.GetBoolParameter(n"isThrowCarriedNPCInputHintDisplayed", true);
  if isMeleeWeaponEquipped && isExploring && !isExaminingDevice && !isDeviceControlled {
    if isCrouchInputHintDisplayed && !canDisplayCrouchInputHint {
      this.RemoveInputHint(scriptInterface, n"ToggleCrouch", n"Melee");
      stateContext.RemovePermanentBoolParameter(n"isCrouchInputHintDisplayed");
    };
    if !isMeleeInputHintDisplayed || this.m_hasControllerChanged || this.m_hasControllerSchemeChanged || NotEquals(isThrowInputHintDisplayed, canDisplayThrowInputHint) || !isCrouchInputHintDisplayed && canDisplayCrouchInputHint {
      return ActiveBaseContext.MeleeWeapon;
    };
  } else {
    if isMeleeInputHintDisplayed && (!isMeleeWeaponEquipped || !isExploring || isExaminingDevice || isDeviceControlled) {
      this.RemoveMeleeInputHints(stateContext, scriptInterface);
    };
  };
  if isRangedWeaponEquipped && !weapon.IsHeavyWeapon() && isExploring && !isExaminingDevice && !isDeviceControlled {
    if isChargeInputHintDisplayed && !canDisplayChargeInputHint {
      this.RemoveInputHint(scriptInterface, n"RangedAttack", n"Ranged");
      stateContext.RemovePermanentBoolParameter(n"isChargeInputHintDisplayed");
    };
    if isRangedDodgeInputHintDisplayed && !canDisplayRangedDodgeInputHint {
      this.RemoveInputHint(scriptInterface, n"Dodge", n"Ranged");
      stateContext.RemovePermanentBoolParameter(n"isRangedInputHintDisplayed");
    };
    if isCrouchInputHintDisplayed && !canDisplayCrouchInputHint {
      this.RemoveInputHint(scriptInterface, n"ToggleCrouch", n"Ranged");
      stateContext.RemovePermanentBoolParameter(n"isCrouchInputHintDisplayed");
    };
    if !isRangedInputHintDisplayed || this.m_hasControllerChanged || this.m_hasControllerSchemeChanged || !isChargeInputHintDisplayed && canDisplayChargeInputHint || !isRangedDodgeInputHintDisplayed && canDisplayRangedDodgeInputHint || !isCrouchInputHintDisplayed && canDisplayCrouchInputHint || !isThrowCarriedNPCInputHintDisplayed && canDisplayThrowCarriedNPCInputHint {
      if stateContext.GetBoolParameter(n"isBodyCarryInputHintDisplayed", true) {
        return ActiveBaseContext.BodyCarringWithRangedWeapon;
      };
      return ActiveBaseContext.RangedWeapon;
    };
  } else {
    if isRangedInputHintDisplayed && (!isRangedWeaponEquipped || !isExploring || isExaminingDevice || isDeviceControlled) {
      this.RemoveRangedInputHints(stateContext, scriptInterface);
    };
  };
  return context;
}

// THE HUD READS THE WEAPON IN HAND UNDER rightWeapon. The equipment UI data names the weapons by slot, and its readers
// take the right one for the weapon in hand: the roster stayed folded with the weapon in the left hand (no name, no
// ammo), the iron sight's counter and the inventory's active weapon stayed empty. They read it as the weapon in hand.
@wrapMethod(WeaponRosterGameController)
protected cb func OnWeaponDataChanged(value: Variant) -> Bool {
  return wrappedMethod(VRLeftHandedWeapon.InHandData(value));
}

@wrapMethod(IronsightGameController)
protected cb func OnWeaponDataChanged(value: Variant) -> Bool {
  return wrappedMethod(VRLeftHandedWeapon.InHandData(value));
}

@wrapMethod(InventoryDataManagerV2)
protected cb func OnWeaponDataChanged(value: Variant) -> Bool {
  return wrappedMethod(VRLeftHandedWeapon.InHandData(value));
}

// A MELEE WEAPON IN THE LEFT HAND. Its melee state machine is the left slot's (Melee / WeaponLeft) and runs as the right
// slot's does, but four of its questions name the right hand: equipping ends when the RIGHT hand's equipment state is
// equipped, readiness ends while the RIGHT hand unequips, the charged hold is read off the WeaponRight instance, and a
// throw takes its projectile from the item on WeaponRight. With the knife alone in the left hand the machine stayed in
// equipping for good (MeleeWeapon 1): no attack, no throw aim, only a block could leave it -- measured 2026-10-30. Each
// answers here for the left hand while it holds the weapon.
@wrapMethod(MeleeEquippingDecisions)
protected final const func ToMeleeIdle(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) {
    return DefaultTransition.IsLeftHandInEquippedState(stateContext);
  };
  return wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(MeleeTransition)
protected final const func IsWeaponReady(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) && DefaultTransition.IsLeftHandInUnequippingState(stateContext) {
    return false;
  };
  return wrappedMethod(stateContext, scriptInterface);
}

// The one melee machine there is, whichever slot runs it: the game never has a melee weapon in each hand.
@wrapMethod(DefaultTransition)
public final const func IsInMeleeState(const stateContext: ref<StateContext>, meleeStateName: CName) -> Bool {
  let identifier: StateMachineIdentifier;
  if wrappedMethod(stateContext, meleeStateName) {
    return true;
  };
  identifier.definitionName = n"Melee";
  identifier.referenceName = n"WeaponLeft";
  return Equals(stateContext.GetStateMachineCurrentStateWithIdentifier(identifier), meleeStateName);
}

// The thrown weapon is the machine's own (the item in the left slot), launched as the game launches one from the right.
@wrapMethod(MeleeTransition)
public final func SpawnMeleeWeaponProjectile(scriptInterface: ref<StateGameScriptInterface>) -> Void {
  let weapon: ref<WeaponObject>;
  let template: CName;
  let appearance: CName;
  if !VRLeftHandedWeapon.InHand(scriptInterface.executionOwner) {
    wrappedMethod(scriptInterface);
    return;
  };
  weapon = MeleeTransition.GetWeaponObject(scriptInterface);
  if !IsDefined(weapon) {
    return;
  };
  if weapon.WeaponHasTag(n"Throwable") && weapon.GetBoolPropertyFromWeaponDefinition(t".useProjectileAppearance") {
    appearance = weapon.GetAppearanceNameFromComponent(n"Grip");
  };
  template = weapon.GetNamePropertyFromWeaponDefinition(t".projectileTemplateName");
  if !IsNameValid(template) {
    return;
  };
  if this.m_driverCombatListener.IsMountedInTPP() {
    ProjectileLaunchHelper.SpawnProjectileFromRightHand(scriptInterface.executionOwner, template, appearance, weapon);
  } else {
    ProjectileLaunchHelper.SpawnProjectileFromScreenCenter(scriptInterface.executionOwner, template, appearance, weapon);
  };
}
