// Controller melee samples the finite, world-space weapon segment.
module CyberpunkVRPort.Melee

// Use a native melee record, never a ranged/quickhack effect from GetAttacks().
@addMethod(WeaponObject)
public func VRGetMeleeAttack(opt strong: Bool) -> ref<Attack_GameEffect> {
  let a: ref<Attack_GameEffect>;
  if strong { a = this.GetAttack(n"MeleeStrongAttack0") as Attack_GameEffect; };
  if !IsDefined(a) { a = this.GetAttack(n"MeleeComboAttack0") as Attack_GameEffect; };
  if IsDefined(a) && IsDefined(a.GetRecord() as Attack_Melee_Record) { return a; };
  a = this.GetCurrentAttack() as Attack_GameEffect;
  if IsDefined(a) && IsDefined(a.GetRecord() as Attack_Melee_Record) { return a; };
  let attacks = this.GetAttacks();
  for attack in attacks {
    a = attack as Attack_GameEffect;
    if IsDefined(a) && IsDefined(a.GetRecord() as Attack_Melee_Record) { return a; };
  };
  return null;
}

// The Impact marker gives clubs and blades their physical reach. Fists
// use the knuckle segment even when their attack record includes a lunge.
@addMethod(WeaponObject)
public func VRMeleeReach() -> Float {
  if WeaponObject.IsOfType(this.GetItemID(), gamedataItemType.Wea_Fists) { return 0.10; };
  return Vector4.Length(this.VRBladeLocalEnd());
}

@addMethod(WeaponObject)
public func VRMeleeDirection() -> Vector4 {
  if WeaponObject.IsOfType(this.GetItemID(), gamedataItemType.Wea_Fists) {
    return this.GetWorldForward();
  };
  return Vector4.Normalize(Quaternion.Transform(this.GetWorldOrientation(), this.VRBladeLocalEnd()));
}

@addMethod(PlayerPuppet)
public func VRMeleeSweepHit(weapon: wref<WeaponObject>, bladeStart: Vector4, bladeEnd: Vector4, previousStart: Vector4, previousEnd: Vector4, hitRadius: Float, passive: Bool, opt trackedMotion: Vector4, opt beginOnContact: Bool, opt strong: Bool, opt speed: Float) -> Int32 {
  if !IsDefined(weapon) || this.VRHolsterIntentActive(1.00) { return 0; };
  if weapon.VRBladeKind() > 0 {
    return this.VRBladeSweep(weapon, previousStart, previousEnd, bladeStart, bladeEnd, passive,beginOnContact,trackedMotion,strong,speed);
  };
  return this.VRCyberwareContacts(weapon,1,[bladeStart,bladeEnd],[previousStart,previousEnd],hitRadius,passive,beginOnContact,strong,speed);
}

// SWING WHOOSH (VR). In the flat game the whoosh rides on the attack ANIMATION's audio events,
// resolved through the weapon's cooked audio metadata (audioMeleeSettings: normalWhoosh /
// fastWhoosh / slowWhoosh event lists per weapon family) — a VR swing plays no attack anim, so
// swings are dead silent. Scripts can't read cooked audio metadata, so this replicates the SAME
// selection with the exact event names from the game's audio configs (dumped from
// base\sound\metadata: audio_melee_metadata_* for every melee family + cyberware banks):
// family = weapon ItemType, variant = fast/normal by real swing speed, heavy on strong swings.
// Played positionally on the WEAPON object — same audio language as the vanilla whoosh
// (weapon emitter), and the same "main whoosh + detail layer" pairing the configs use.
@addMethod(PlayerPuppet)
public func VRMeleeWhoosh(weapon: wref<WeaponObject>, fast: Bool, strong: Bool) -> Void {
  if !IsDefined(weapon) { return; };
  let rec = weapon.GetWeaponRecord();
  if !IsDefined(rec) { return; };
  let t = rec.ItemType().Type();
  let a: CName = n"";  // main whoosh
  let b: CName = n"";  // detail layer (knife ring / tomahawk head / sledge ring), as in the configs
  switch t {
    case gamedataItemType.Wea_Katana:
    case gamedataItemType.Wea_Sword:
    case gamedataItemType.Wea_LongBlade:
      a = n"w_melee_katana_whoosh";                                        // katana config: normalWhoosh only
      break;
    case gamedataItemType.Wea_ShortBlade:
    case gamedataItemType.Wea_Knife:
    case gamedataItemType.Wea_Machete:
      a = fast ? n"w_melee_blade_whoosh_fast" : n"w_melee_blade_whoosh_normal";
      b = n"w_melee_detail_knife_ring";
      break;
    case gamedataItemType.Wea_Axe:
      a = fast ? n"w_melee_blade_whoosh_fast" : n"w_melee_blade_whoosh_normal";
      b = n"w_melee_detail_tomahawk";                                      // tomahawk/vb_axe configs
      break;
    case gamedataItemType.Wea_Chainsword:
      a = fast ? n"w_melee_blade_whoosh_fast" : n"w_melee_blade_whoosh_normal";
      b = fast ? n"w_melee_cut_o_matic_whoosh_fast" : n"w_melee_cut_o_matic_whoosh";
      break;
    case gamedataItemType.Wea_Hammer:
      a = fast ? n"w_melee_sledgehammer_whoosh_fast" : n"w_melee_sledgehammer_whoosh";
      b = fast ? n"w_melee_sledgehammer_detail_ring" : n"w_melee_sledgehammer_detail_ring_sharp";
      break;
    case gamedataItemType.Wea_OneHandedClub:
    case gamedataItemType.Wea_TwoHandedClub:
    case gamedataItemType.Wea_Melee:
      if strong { a = n"w_melee_blunt_whoosh_heavy"; }
      else { a = fast ? n"w_melee_blunt_whoosh_fast" : n"w_melee_blunt_whoosh_low"; };
      break;
    case gamedataItemType.Cyb_MantisBlades:
      if strong { a = n"w_cyb_mantis_whoosh_heavy"; }
      else { a = fast ? n"w_cyb_mantis_whoosh_fast" : n"w_cyb_mantis_whoosh"; };
      b = n"w_cyb_mantis_whoosh_ring";
      break;
    case gamedataItemType.Cyb_NanoWires:
      a = fast ? n"w_cyb_nano_wire_whoosh_swirl" : n"w_cyb_nano_wire_whoosh";
      break;
    case gamedataItemType.Cyb_StrongArms:
    case gamedataItemType.Wea_Fists:
      a = fast ? n"w_melee_fist_whoosh_fast" : n"w_melee_fist_whoosh_heavy";  // fists config naming
      break;
    default:
      a = fast ? n"w_melee_fist_whoosh_fast" : n"w_melee_fist_whoosh_heavy";  // neutral air whoosh
      break;
  }
  if NotEquals(a, n"") { GameObject.PlaySoundEvent(weapon, a); };
  if NotEquals(b, n"") { GameObject.PlaySoundEvent(weapon, b); };
}

// WEAPON DRAW SOUND (VR). Draw/equip sounds ride on the draw ANIMATION's audio events, which the
// VR rig never plays — so every draw is silent. Same replication approach as the whoosh:
//   melee — the equipSound lists from audio_melee_metadata_* per family (incl. detail layers);
//   guns  — audio family names live only in cooked entity data (scripts can't read it, TweakDB
//           has none of them), so the full set of player '*_equip' events from the sound banks
//           is embedded and matched by the gun's name token from its TweakDB id
//           ("Items.Preset_Lexington_Wilson" -> "lexington" -> w_gun_pistol_power_lexington_equip);
//           unmatched guns degrade to the game's own generic draws by weight class
//           (w_gun_npc_equip_light/medium/heavy — what NPC draws play).
// Played positionally on the weapon entity, like the whoosh and the hit feedback.
@addMethod(PlayerPuppet)
public func VREquipSound(weapon: wref<WeaponObject>) -> Void {
  if !IsDefined(weapon) { return; };
  let rec = weapon.GetWeaponRecord();
  if !IsDefined(rec) { return; };
  let t = rec.ItemType().Type();
  let a: CName = n"";
  let b: CName = n"";
  switch t {
    case gamedataItemType.Wea_Katana:
    case gamedataItemType.Wea_Sword:
    case gamedataItemType.Wea_LongBlade:
      a = n"w_melee_katana_equip";
      break;
    case gamedataItemType.Wea_ShortBlade:
    case gamedataItemType.Wea_Knife:
      a = n"w_melee_knife_additional_equip";
      b = n"w_melee_detail_knife_ring";
      break;
    case gamedataItemType.Wea_Machete:
      a = n"w_melee_knife_additional_equip";
      break;
    case gamedataItemType.Wea_Axe:
      a = n"w_melee_detail_tomahawk";
      b = n"w_melee_knife_additional_equip";
      break;
    case gamedataItemType.Wea_Chainsword:
      a = n"w_melee_cut_o_matic_equip";
      break;
    case gamedataItemType.Wea_Hammer:
      break;                                        // sledgehammer config: equipSound "None"
    case gamedataItemType.Wea_OneHandedClub:
    case gamedataItemType.Wea_TwoHandedClub:
    case gamedataItemType.Wea_Melee:
      a = n"w_melee_detail_bat_equip";
      break;
    case gamedataItemType.Wea_Fists:
      a = n"lcm_mvs_leather_jacket_fast_light";     // fists config: cloth swish
      break;
    case gamedataItemType.Cyb_MantisBlades:
      a = n"w_cyb_mantis_equip";
      break;
    case gamedataItemType.Cyb_StrongArms:
      a = n"w_cyb_strongarms_equip";
      break;
    case gamedataItemType.Cyb_NanoWires:
      a = n"w_cyb_nano_wire_out";                   // wire deploy
      break;
    default:
      break;
  }
  if NotEquals(a, n"") || NotEquals(b, n"") {
    if NotEquals(a, n"") { GameObject.PlaySoundEvent(weapon, a); };
    if NotEquals(b, n"") { GameObject.PlaySoundEvent(weapon, b); };
    return;
  };
  if Equals(t, gamedataItemType.Wea_Hammer) { return; };

  // guns: match the item-id name token against the sound banks' player equip events
  let idStr = StrLower(TDBID.ToStringDEBUG(ItemID.GetTDBID(weapon.GetItemID())));
  let parts = StrSplit(idStr, "_");
  if ArraySize(parts) >= 2 {
    let name = parts[1];
    let evts: array<String> = [
      "w_gun_pistol_power_lexington_equip", "w_gun_pistol_power_liberty_equip",
      "w_gun_pistol_power_unity_equip", "w_gun_pistol_power_nue_equip",
      "w_gun_pistol_power_slaughtomatic_equip", "w_gun_pistol_power_silverhand_equip",
      "w_gun_pistol_tech_omaha_equip", "w_gun_pistol_tech_kenshin_equip",
      "w_gun_grit_equip", "w_gun_ticon_equip", "w_gun_pistol_toygun_equip",
      "w_gun_pistol_smart_chao_equip", "w_gun_pistol_smart_kappa_equip",
      "w_gun_pistol_smart_yukimura_equip",
      "w_gun_revol_power_overture_equip", "w_gun_revol_power_crusher_equip",
      "w_gun_revol_power_nova_equip", "w_gun_metel_equip",
      "w_gun_revol_tech_burya_equip", "w_gun_revol_tech_quasar_equip",
      "w_gun_smg_power_saratoga_equip", "w_gun_smg_power_guillotine_equip",
      "w_gun_smg_power_pulsar_equip", "w_gun_borg4a_equip", "w_gun_warden_equip",
      "w_gun_smg_tech_senkoh_equip", "w_gun_smg_smart_shingen_equip",
      "w_gun_smg_smart_sidewinder_equip", "w_gun_smg_smart_dian_equip",
      "w_gun_rifle_power_ajax_equip", "w_gun_rifle_power_copperhead_equip",
      "w_gun_rifle_power_masamune_equip", "w_gun_rifle_power_nowaki_equip",
      "w_gun_rifle_power_pozhar_equip", "w_gun_rifle_power_sor22_equip",
      "w_gun_rifle_power_kolac_equip", "w_gun_rifle_tech_achilles_equip",
      "w_gun_shotgun_power_carnage_equip", "w_gun_shotgun_power_tactician_equip",
      "w_gun_shotgun_power_igla_equip", "w_gun_shotgun_power_testera_equip",
      "w_gun_shotgun_tech_satara_equip", "w_gun_shotgun_smart_zhuo_equip",
      "w_gun_shotgun_smart_palica_equip",
      "w_gun_sniper_power_grad_equip", "w_gun_osprey_equip",
      "w_gun_sniper_tech_rasetsu_equip", "w_gun_sniper_tech_nekomata_equip",
      "w_gun_sniper_smart_ashura_equip",
      "w_gun_lmg_power_ma70_equip", "w_gun_lmg_power_defender_equip",
      "w_gun_hmg_militech_equip", "w_gun_hercules3ax_equip"
    ];
    let i = 0;
    while i < ArraySize(evts) {
      if StrContains(evts[i], "_" + name) {
        GameObject.PlaySoundEvent(weapon, StringToName(evts[i]));
        return;
      };
      i += 1;
    };
  };

  // no bank event matched: the game's generic draw foley by weight class
  switch t {
    case gamedataItemType.Wea_Handgun:
    case gamedataItemType.Wea_Revolver:
    case gamedataItemType.Wea_SubmachineGun:
      a = n"w_gun_npc_equip_light";
      break;
    case gamedataItemType.Wea_SniperRifle:
    case gamedataItemType.Wea_LightMachineGun:
    case gamedataItemType.Wea_HeavyMachineGun:
      a = n"w_gun_npc_equip_heavy";
      break;
    default:
      a = n"w_gun_npc_equip_medium";
      break;
  }
  GameObject.PlaySoundEvent(weapon, a);
}

// NO IDLE AUTO-UNEQUIP FOR MELEE. The melee PSM's PublicSafe state arms a 15s timer
// (timeToAutoUnequipWeapon in public zones / timeToUnequipMeleeware for arm cyberware) and its
// OnTick sends UnequipWeapon when it expires. The timer resets only on ATTACK INPUT — which a VR
// swing never presses, so the blade kept holstering itself mid-use ("idle" by the game's book
// while the player is physically swinging it). Guns don't idle-unequip; give melee the same
// behavior by neutralizing the timer right before the vanilla check reads it.
@wrapMethod(MeleePublicSafeEvents)
protected func OnTick(timeDelta: Float, stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  this.m_unequipTime = -1.0;
  wrappedMethod(timeDelta, stateContext, scriptInterface);
}
