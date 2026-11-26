// Workspots can be evaluated by crowd jobs while a saved world is being
// restored. The condition only needs the configured item's tags. Read those
// flats by ID. The crash dump shows the Item_Record cast failing inside Item().
public abstract class VRWorkspotEquipment {
  public static func HasTag(equipment: TweakDBID, tag: CName) -> Bool {
    let item=TDB.GetForeignKey(equipment+t".item");
    if !TDBID.IsValid(item) { return false; };
    let tags=TDB.GetCNameArray(item+t".tags");
    return ArrayContains(tags,tag);
  }
}

@replaceMethod(PrimaryWeaponTypeCondition)
protected const func CheckCondition(ent: ref<Entity>) -> Bool {
  let obj = ent as GameObject;
  let npc: ref<NPCPuppet>;
  let tag: CName;
  let items: array<wref<NPCEquipmentItem_Record>>;
  let entry: ref<NPCEquipmentItem_Record>;
  let i: Int32;
  if !IsDefined(obj) { return true; };
  if !obj.IsNPC() { return false; };
  switch this.weaponType {
    case WorkspotWeaponConditionEnum.Ranged: tag=n"RangedWeapon"; break;
    case WorkspotWeaponConditionEnum.OneHandedRanged: tag=n"OneHandedRangedWeapon"; break;
    case WorkspotWeaponConditionEnum.Melee: tag=n"MeleeWeapon"; break;
    case WorkspotWeaponConditionEnum.MeleeCyberware: tag=n"Meleeware"; break;
    case WorkspotWeaponConditionEnum.LMG: tag=n"LMG"; break;
    case WorkspotWeaponConditionEnum.HMG: tag=n"HMG"; break;
    default: return true;
  };
  npc = obj as NPCPuppet;
  if !IsDefined(npc) { return false; };
  AIActionTransactionSystem.CalculateEquipmentItems(npc,n"PrimaryEquipment",items);
  while i<ArraySize(items) {
    entry=items[i];
    if IsDefined(entry) && VRWorkspotEquipment.HasTag(entry.GetID(),tag) { return true; };
    i+=1;
  };
  return false;
}
