// CyberpunkVRPort -- the haptics' native and the guard-hands helper, shared: the Cyberware module calls them, and the
// Haptics module's DamageSystem wrapper is Rust's in the Rust build (rust/crates/cvr-ffi/src/mechanics/reds_hooks.rs).

native func VRHapticEvent(kind: Int32, hands: Int32) -> Void;

func VRPortGuardHands(player: ref<PlayerPuppet>) -> Int32 {
  let ts: ref<TransactionSystem> = GameInstance.GetTransactionSystem(player.GetGame());
  let weapon: ref<WeaponObject> = ts.GetItemInSlot(player, t"AttachmentSlots.WeaponRight") as WeaponObject;
  let left: ref<WeaponObject>;
  if IsDefined(weapon) {
    let id: ItemID = weapon.GetItemID();
    if WeaponObject.IsOfType(id, gamedataItemType.Wea_Fists) || WeaponObject.IsOfType(id, gamedataItemType.Cyb_StrongArms)
      || WeaponObject.IsOfType(id, gamedataItemType.Cyb_MantisBlades) {
      return 3;
    };
    return 2;
  };
  left = ts.GetItemInSlot(player, t"AttachmentSlots.WeaponLeft") as WeaponObject;
  if IsDefined(left) && !WeaponObject.IsCyberwareWeapon(left.GetItemID()) {
    return 1;
  };
  return 2;
}
