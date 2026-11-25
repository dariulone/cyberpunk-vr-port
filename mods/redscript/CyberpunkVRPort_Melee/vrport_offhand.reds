// These two slots belong to the port. Removing their attachment returns the
// item to inventory and leaves the game's active weapon and quick slots alone.
@addMethod(PlayerPuppet)
public final func VROffhandSlot(side: Int32) -> TweakDBID {
  return side == 0 ? t"AttachmentSlots.VRPortOffhandLeft" : t"AttachmentSlots.VRPortOffhandRight";
}

@addMethod(PlayerPuppet)
public final func VROffhandItem(side: Int32) -> ref<WeaponObject> {
  return GameInstance.GetTransactionSystem(this.GetGame()).GetItemInSlot(this, this.VROffhandSlot(side)) as WeaponObject;
}

@addMethod(PlayerPuppet)
public final func VRClearOffhandItem(side: Int32) -> Void {
  let ts = GameInstance.GetTransactionSystem(this.GetGame());
  let slot = this.VROffhandSlot(side);
  if IsDefined(ts.GetItemInSlot(this,slot)) { ts.RemoveItemFromSlot(this,slot,false); };
}

@addMethod(PlayerPuppet)
public final func VRMountOffhandItem(primary: ItemID, item: ItemID, side: Int32) -> Bool {
  let ts = GameInstance.GetTransactionSystem(this.GetGame());
  let opposite = side == 0 ? t"AttachmentSlots.WeaponRight" : t"AttachmentSlots.WeaponLeft";
  let free = side == 0 ? t"AttachmentSlots.WeaponLeft" : t"AttachmentSlots.WeaponRight";
  let main = ts.GetItemInSlot(this,opposite);
  let slot = this.VROffhandSlot(side);
  let allowed: Bool;
  if !IsDefined(main) || main.GetItemID() != primary || !WeaponObject.IsOfType(primary,gamedataItemType.Wea_Katana)
    || !ItemID.IsValid(item) || item == primary || !ts.HasItem(this,item)
    || IsDefined(ts.GetItemInSlot(this,free)) || IsDefined(ts.GetItemInSlot(this,slot)) { return false; };
  if WeaponObject.IsFists(item) {
    allowed = ArrayContains(EquipmentSystem.GetItemsInArea(this,gamedataEquipmentArea.BaseFists),item);
  } else {
    allowed = WeaponObject.IsOfType(item,gamedataItemType.Wea_Katana)
      && ArrayContains(EquipmentSystem.GetItemsInArea(this,gamedataEquipmentArea.Weapon),item);
  };
  return allowed && ts.AddItemToSlot(this,slot,item,true,null,ERenderingPlane.RPl_Weapon,false,true);
}

@addMethod(PlayerPuppet)
public final func VRIsOffhandMelee(weapon: wref<WeaponObject>) -> Bool {
  let side = VRLeftHandedWeapon.InHand(this) ? 1 : 0;
  let main = GameInstance.GetTransactionSystem(this.GetGame()).GetItemInSlot(this,VRLeftHandedWeapon.HeldSlot(this));
  return IsDefined(weapon) && IsDefined(main) && WeaponObject.IsOfType(main.GetItemID(),gamedataItemType.Wea_Katana)
    && this.VROffhandItem(side) == weapon;
}
