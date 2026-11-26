// CyberpunkVRPort -- the left-handed weapon's questions (moved from CyberpunkVRPort_LeftHanded): the natives and
// VRLeftHandedWeapon, which the Melee and Cyberware modules call; the module itself is Rust's in the Rust build
// (rust/crates/cvr-ffi/src/mechanics/left_handed.rs). Global namespace: the plugin registers plain global natives.

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
