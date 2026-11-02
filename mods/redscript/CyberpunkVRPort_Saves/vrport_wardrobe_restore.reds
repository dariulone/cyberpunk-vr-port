// CyberpunkVRPort -- A SAVE WHOSE WARDROBE NAMES CLOTHES THE GAME NO LONGER HAS.
//
// A wardrobe set made with a clothing mod keeps that mod's items as the visual overrides of the areas it
// covers, and the save keeps them after the mod is gone. On load, EquipmentSystemPlayerData.OnRestored puts
// every equipped item in its slot, and an item whose area is overridden goes in with the override as its
// look: TransactionSystem.AddItemToSlot(owner, slot, item, visualItem). With the visual item missing from the
// TweakDB, measured on a save (ManualSave-476: pants worn under a modded Legs override, shoes under a modded
// Feet one), nothing of the clothing reached the body: pants and shoes never entered their slots, the
// jacket had its look and no mesh, and taking the set off after the load brought none of it back. A save
// with the same set but no pants under the modded override (ManualSave-483) recovered when the set came off.
//
// So the set comes off BEFORE the items are put on: when any overridden area names an item the TweakDB does
// not have, every area loses its override and its hide, as UnequipWardrobeSet would leave them, and the
// wardrobe has no active set. The set itself stays in the wardrobe for when the mod is back. Underwear is
// not touched: its hide flags are the game's own (pants hide the underwear), not a set's.

@wrapMethod(EquipmentSystemPlayerData)
public final func OnRestored() -> Void {
  this.VRPortDropLostWardrobeSet();
  wrappedMethod();
}

@addMethod(EquipmentSystemPlayerData)
private final func VRPortDropLostWardrobeSet() -> Void {
  let lost: Bool = false;
  let i: Int32 = 0;
  let item: ItemID;
  let area: gamedataEquipmentArea;
  while i < ArraySize(this.m_clothingVisualsInfo) {
    item = this.m_clothingVisualsInfo[i].visualItem;
    if ItemID.IsValid(item) && !IsDefined(TweakDBInterface.GetItemRecord(ItemID.GetTDBID(item))) {
      lost = true;
    };
    i += 1;
  };
  if !lost {
    return;
  };
  i = 0;
  while i < ArraySize(this.m_clothingVisualsInfo) {
    area = this.m_clothingVisualsInfo[i].areaType;
    if NotEquals(area, gamedataEquipmentArea.UnderwearTop) && NotEquals(area, gamedataEquipmentArea.UnderwearBottom) {
      this.m_clothingVisualsInfo[i].visualItem = ItemID.None();
      this.m_clothingVisualsInfo[i].isHidden = false;
    };
    i += 1;
  };
  GameInstance.GetWardrobeSystem(this.m_owner.GetGame()).SetActiveClothingSetIndex(gameWardrobeClothingSetIndex.INVALID);
  FTLog("[VRPort] wardrobe set taken off on load: it names clothes the game does not have");
}
