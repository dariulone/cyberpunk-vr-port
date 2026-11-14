// The launcher occupies the native fourth, cyberware slot in the weapon cycle.
// It remains a LEFT-hand launcher: no record/category mutation or fake gun.
@addField(PlayerPuppet)
public let vrArmBLauncherPending: Bool;

@addMethod(PlayerPuppet)
public final const func VRArmBLauncherAllowed() -> Bool {
  let tag: CName;
  let restrictions: array<CName> = [n"NoWeapons", n"NoArmsCW", n"NoCombat", n"Melee", n"Fists", n"OneHandedFirearms", n"DriverCombatFirearms", n"DriverCombatBikeWeapons"];
  for tag in restrictions { if StatusEffectSystem.ObjectHasStatusEffectWithTag(this, tag) { return false; }; };
  return !PlayerGameplayRestrictions.IsHotkeyRestricted(this.GetGame(), EHotkey.RB);
}
@addMethod(EquipmentSystemPlayerData)
private final const func VrInstalledLauncher() -> ItemID {
  let item = this.GetActiveItem(gamedataEquipmentArea.ArmsCW);
  if ItemID.IsValid(item) && WeaponObject.IsOfType(item, gamedataItemType.Cyb_Launcher) { return item; };
  return ItemID.None();
}

@addMethod(EquipmentSystemPlayerData)
private final const func VrCanCycleLauncher() -> Bool {
  let tag: CName;
  let restrictions: array<CName> = [n"NoWeapons", n"NoArmsCW", n"NoCombat", n"Melee", n"Fists", n"OneHandedFirearms", n"DriverCombatFirearms", n"DriverCombatBikeWeapons"];
  for tag in restrictions { if StatusEffectSystem.ObjectHasStatusEffectWithTag(this.m_owner, tag) { return false; }; };
  return !PlayerGameplayRestrictions.IsHotkeyRestricted(this.m_owner.GetGame(), EHotkey.RB);
}

@wrapMethod(EquipmentSystemPlayerData)
public final func CycleWeapon(cycleNext: Bool, onlyCheck: Bool) -> ItemID {
  let launcher = this.VrInstalledLauncher();
  let area: Int32;
  let slots: Int32;
  let current: Int32;
  let candidate: ItemID;
  let active: ItemID;
  let i: Int32;
  let direction: Int32 = cycleNext ? 1 : -1;
  if !ItemID.IsValid(launcher) || !(this.m_owner as PlayerPuppet).ShouldAllowCycleToFistCyberware() {
    return wrappedMethod(cycleNext, onlyCheck);
  };
  area = this.GetEquipAreaIndex(gamedataEquipmentArea.WeaponWheel);
  if area < 0 { return wrappedMethod(cycleNext, onlyCheck); };
  slots = ArraySize(this.m_equipment.equipAreas[area].equipSlots);
  if slots < 2 || slots > 16 { return wrappedMethod(cycleNext, onlyCheck); };
  active = this.GetLastUsedWeaponItemID();
  current = this.m_equipment.equipAreas[area].activeIndex;
  if this.GetSlotActiveItem(EquipmentManipulationRequestSlot.Left) == launcher {
    active = launcher;
    current = slots - 1;
  };
  while i < slots {
    current = (current + direction + slots) % slots;
    candidate = current == slots - 1 ? launcher : this.m_equipment.equipAreas[area].equipSlots[current].itemID;
    // Native weapon-cycle validation rejects items without RangedWeapon or
    // MeleeWeapon tags. The launcher is a gadget; use its own restrictions.
    if ItemID.IsValid(candidate) && candidate != active && (candidate == launcher ? this.VrCanCycleLauncher() : this.CheckWeaponAgainstGameplayRestrictions(candidate, true)) {
      if !onlyCheck { this.m_equipment.equipAreas[area].activeIndex = current; };
      return candidate;
    };
    i += 1;
  };
  return ItemID.None();
}

// 2.5 returns what each hand should hold (dual wielding). Our cycle's pick goes through the game's own
// request for it: the launcher as RequestLeftHandCyberware, a weapon alone in the right hand.
@wrapMethod(EquipmentSystemPlayerData)
public final func GetItemIDfromEquipmentManipulationAction(action: EquipmentManipulationAction, currentState: SSlotActiveItems) -> SSlotActiveItems {
  let state: SSlotActiveItems;
  let item: ItemID;
  let player = this.m_owner as PlayerPuppet;
  let launcher = this.VrInstalledLauncher();
  if Equals(action, EquipmentManipulationAction.CycleWeaponWheelItem) && ItemID.IsValid(launcher) {
    item = this.CycleWeapon(true, false);
    if item == launcher {
      state = wrappedMethod(EquipmentManipulationAction.RequestLeftHandCyberware, currentState);
    } else {
      if !ItemID.IsValid(item) { return currentState; };
      state.rightHandItem = item;
    };
  } else {
    state = wrappedMethod(action, currentState);
    item = Equals(action, EquipmentManipulationAction.RequestLeftHandCyberware) || !ItemID.IsValid(state.rightHandItem)
      ? state.leftHandItem : state.rightHandItem;
  };
  if this.vrCyberwareHandlingRequest && IsDefined(player) && ItemID.IsValid(item) && !this.IsEquipmentManipulationAnUnequipRequest(action) {
    if WeaponObject.IsOfType(item, gamedataItemType.Cyb_Launcher) {
      player.vrLauncherSelectedAsWeapon = NotEquals(action, EquipmentManipulationAction.RequestLeftHandCyberware) || player.vrArmBLauncherPending;
      player.vrArmBLauncherPending = false;
      if player.vrLauncherSelectedAsWeapon { player.vrLauncherFromWeaponCycle = true; };
    } else {
      if !this.CheckCyberwareItemForActivatedAction(item) {
        player.vrLauncherKeepOpen = false;
        player.vrLauncherSelectedAsWeapon = false;
      };
    };
  };
  return state;
}

@addMethod(LeftHandCyberwareEquipDecisions)
protected final const func ToVrLauncherReady(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let player = scriptInterface.executionOwner as PlayerPuppet;
  if player.vrLauncherSelectedAsWeapon && LeftHandCyberwareHelper.IsProjectileLauncherEquipped(scriptInterface) {
    player.vrLauncherKeepOpen = true;
  };
  return (scriptInterface.executionOwner as PlayerPuppet).vrLauncherSelectedAsWeapon
    && this.VrLauncherCanStay(stateContext, scriptInterface)
    && this.GetInStateTime() >= this.GetEquipDuration(scriptInterface);
}

@wrapMethod(LeftHandCyberwareEquipDecisions)
protected final const func ToLeftHandCyberwareUnequip(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let player = scriptInterface.executionOwner as PlayerPuppet;
  if player.vrLauncherFromWeaponCycle && !player.vrLauncherSelectedAsWeapon { return true; };
  return wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(LeftHandCyberwareQuickActionDecisions)
protected final const func EnterCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  // Selecting an arm with the weapon cycle is not a gadget-button release.
  if (scriptInterface.executionOwner as PlayerPuppet).vrLauncherFromWeaponCycle
    && stateContext.IsStateActive(n"LeftHandCyberware", n"leftHandCyberwareEquip") { return false; };
  return wrappedMethod(stateContext, scriptInterface);
}
