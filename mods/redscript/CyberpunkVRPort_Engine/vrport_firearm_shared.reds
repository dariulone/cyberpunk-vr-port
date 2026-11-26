// CyberpunkVRPort -- the firearm in hand (moved from CyberpunkVRPort_NoAnims/ControlLowerAndHolsterWeapons.reds):
// the right hand's, or with the left-handed controls a lone one in the left. CyberpunkVRPort_LeftHanded calls it;
// NoAnims is Rust's in the Rust build (rust/crates/cvr-ffi/src/mechanics/no_anims.rs).

public func VRPortFirearmIsLeft(scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let ts: ref<TransactionSystem> = scriptInterface.GetTransactionSystem();
  return !IsDefined(ts.GetItemInSlot(scriptInterface.executionOwner, t"AttachmentSlots.WeaponRight") as WeaponObject)
    && IsDefined(ts.GetItemInSlot(scriptInterface.executionOwner, t"AttachmentSlots.WeaponLeft") as WeaponObject);
}

public func VRPortFirearm(scriptInterface: ref<StateGameScriptInterface>) -> ref<WeaponObject> {
  let slot: TweakDBID = VRPortFirearmIsLeft(scriptInterface) ? t"AttachmentSlots.WeaponLeft" : t"AttachmentSlots.WeaponRight";
  return scriptInterface.GetTransactionSystem().GetItemInSlot(scriptInterface.executionOwner, slot) as WeaponObject;
}
