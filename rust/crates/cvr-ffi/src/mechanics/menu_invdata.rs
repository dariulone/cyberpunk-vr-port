//! inventory_data.lua, backpack_data.lua, wardrobe_data.lua, item_sort.lua's data half and preferences.lua: the
//! game's equipment adapter (InventoryDataManagerV2, full ItemIDs kept -- two copies of a weapon record can differ),
//! the equipped items, the picker's items, the attachments, equip / unequip / install / remove, the loadout's signature
//! read a slice a frame, the dual-wield partner slot, the backpack's snapshot and its hold actions (favourite, drop,
//! disassemble, the junk), EquipmentEx's outfits, the game's own sort order, the character-model preference.
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_data::Icon;
use super::menu_gpdata as gd;
use super::menu_jobs::{Factory, Step};
use super::menu_rt::*;
use crate::script_call::Owned;
use red4ext_abi::handle::Handle;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub(super) const CLOTHES: [&str; 7] = ["Head", "Face", "OuterChest", "InnerChest", "Legs", "Feet", "Outfit"];
pub(super) const DUAL_SLOT: i32 = 0;

fn container_get(name: &str) -> Handle {
    let c = super::game_system("GetScriptableSystemsContainer");
    o(&c, "Get", &[A::Name(cname(name))])
}
pub(super) fn ui_inventory() -> Handle {
    container_get("UIInventoryScriptableSystem")
}
fn ui_system() -> Handle {
    container_get("UIScriptableSystem")
}
fn ts() -> Handle {
    super::game_system("GetTransactionSystem")
}
/// UIInventoryScriptableSystem.GetPlayerItem by ItemID (an overload takes the combined hash).
pub(super) fn ui_item(inv: &Handle, id: &Id16) -> Handle {
    rt::call_typed(inv, "GetPlayerItem", &["gameItemID"], &[id.a()]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null)
}
pub(super) fn area(name: &str) -> f64 {
    ev("gamedataEquipmentArea", name)
}
/// A struct value copied into a value of its own (an array element kept past the array).
pub(super) fn own(v: &Val) -> Option<Owned> {
    let mut o = Owned::of_type(v.ty)?;
    // SAFETY: the same type assigns its value.
    unsafe { v.ty.assign(o.as_mut_ptr().cast(), v.ptr) };
    Some(o)
}
/// A 16-byte id written into an object's property (an ItemID field).
pub(super) fn hset_id(h: &Handle, field: &str, id: &Id16) {
    if let Some(f) = rt::field(h, field) {
        let n = (f.ty.size() as usize).min(16);
        // SAFETY: the field's own bytes, at most its size.
        unsafe { core::ptr::copy_nonoverlapping(id.0.as_ptr().cast::<u8>(), f.ptr.cast::<u8>(), n) };
    }
}
pub(super) fn hset_handle(h: &Handle, field: &str, v: &Handle) {
    super::menu_main::set_handle_prop(h, field, v);
}
/// I.key: an item's combined hash as text.
pub(super) fn key_of(id: &Id16) -> String {
    rt::static_call("ItemID", "GetCombinedHash", &[id.a()]).map_or_else(String::new, |o| {
        // SAFETY: a Uint64 result.
        unsafe { *(o.as_ptr() as *const u64) }.to_string()
    })
}
fn iid(data: &Owned, f: &str) -> Id16 {
    rt::static_call("InventoryItemData", f, &[A::P(ptr(data))]).map(|o| Id16::of(&Val::of(&o))).unwrap_or_default()
}
fn idn(data: &Owned, f: &str) -> f64 {
    sn("InventoryItemData", f, &[A::P(ptr(data))])
}
fn ids(data: &Owned, f: &str) -> String {
    ss("InventoryItemData", f, &[A::P(ptr(data))])
}
fn idh(data: &Owned, f: &str) -> Handle {
    so("InventoryItemData", f, &[A::P(ptr(data))])
}
pub(super) fn image_name(name: &str) -> Option<Icon> {
    if name.is_empty() || name == "None" {
        return None;
    }
    gd::image(tdb(&format!("UIIcon.{name}")))
}
pub(super) fn weapon_type_icon(kind: f64) -> Option<Icon> {
    let n = rt::static_call("UIItemsHelper", "GetWeaponTypeIcon", &[A::N(kind)]).map(|o| Val::of(&o).text()).unwrap_or_default();
    if n.is_empty() {
        None
    } else {
        gd::image(tdb(&n))
    }
}

/// An item of the equipment adapter (I.item), or of the backpack's snapshot (B.item: `data` None).
#[derive(Clone, Default)]
pub(super) struct Item {
    pub id: Id16,
    pub key: String,
    pub data: Option<Rc<Owned>>,
    pub name: String,
    pub description: String,
    pub quality: String,
    pub quality_int: f64,
    pub image: Option<Icon>,
    pub ty: f64,
    pub area: f64,
    pub equipped: bool,
    pub type_name: String,
    pub type_icon: Option<Icon>,
    pub new: bool,
    pub allowed: bool,
    pub iconic: bool,
    pub melee: bool,
    pub blocked: bool,
    pub dps: f64,
    pub ammo: f64,
    pub quantity: f64,
    pub is_mod: bool,
    pub ui: Handle,
    pub raw: Handle,
    pub favorite: bool,
    pub junk: bool,
    pub quest: bool,
    pub weapon: bool,
    pub category: Option<&'static str>,
    pub price: Option<f64>,
    pub comparison: Option<f64>,
}
pub(super) struct Ctx {
    pub manager: Handle,
    cache: HashMap<String, Vec<Item>>,
    equipped_cache: HashMap<String, Option<Item>>,
    comparison: Handle,
    ui: Handle,
}
pub(super) fn open() -> Ctx {
    let m = super::lobby_ink::new("InventoryDataManagerV2");
    rt::call(&m, "Initialize", &[A::H(&player())]);
    let comparison = so("ItemPreferredComparisonResolver", "Make", &[A::H(&m)]);
    Ctx { manager: m, cache: HashMap::new(), equipped_cache: HashMap::new(), comparison, ui: ui_system() }
}
pub(super) fn close(ctx: Ctx) {
    rt::call(&ctx.manager, "UnInitialize", &[]);
}
pub(super) fn invalidate(ctx: &mut Ctx) {
    ctx.cache.clear();
    ctx.equipped_cache.clear();
    ctx.comparison = so("ItemPreferredComparisonResolver", "Make", &[A::H(&ctx.manager)]);
    rt::call(&ctx.manager, "MarkToRebuild", &[]);
    rt::call(&ctx.manager, "ClearInventoryItemDataCache", &[]);
}
pub(super) fn item(ctx: &Ctx, data: Owned) -> Option<Item> {
    if sb("InventoryItemData", "IsEmpty", &[A::P(ptr(&data))]) {
        return None;
    }
    let id = iid(&data, "GetID");
    let mut icon = ids(&data, "GetIconPath");
    if icon.is_empty() {
        let resolver = so("IconsNameResolver", "GetIconsNameResolver", &[]);
        let male = idn(&data, "GetIconGender") == ev("ItemIconGender", "Male");
        icon = s(&resolver, "TranslateItemToIconName", &[A::Name(id.tdbid()), A::B(male)]);
    }
    let raw = idh(&data, "GetGameItemData");
    let kind = idn(&data, "GetItemType");
    let tag = |t: &str| !raw.is_null() && b(&raw, "HasTag", &[A::Name(cname(t))]);
    let pl = player();
    Some(Item {
        key: key_of(&id),
        name: loc(&ids(&data, "GetName")),
        description: loc(&ids(&data, "GetDescription")),
        quality: ids(&data, "GetQuality"),
        image: image_name(&icon),
        ty: kind,
        area: idn(&data, "GetEquipmentArea"),
        equipped: idn(&data, "IsEquipped") != 0.0,
        type_name: loc(&ids(&data, "GetLocalizedItemType")),
        type_icon: weapon_type_icon(kind),
        new: b(&ctx.ui, "IsInventoryItemNew", &[id.a()]),
        allowed: sb("InventoryGPRestrictionHelper", "CanEquip", &[A::P(ptr(&data)), A::H(&pl)]),
        iconic: tag("IconicWeapon"),
        melee: tag("MeleeWeapon"),
        blocked: tag("UnequipBlocked"),
        dps: idn(&data, "GetDPS"),
        ammo: idn(&data, "GetAmmo"),
        quantity: idn(&data, "GetQuantity"),
        is_mod: sb("RPGManager", "IsWeaponMod", &[A::N(kind)]),
        raw,
        id,
        data: Some(Rc::new(data)),
        ..Default::default()
    })
}
pub(super) fn inspect(ctx: &Ctx, item: &mut Item) {
    let r = super::lobby_ink::new("UIScriptableSystemInventoryInspectItem");
    hset_id(&r, "itemID", &item.id);
    rt::call(&ctx.ui, "QueueRequest", &[A::H(&r)]);
    item.new = false;
}
pub(super) fn compare(ctx: &Ctx, item: &Item, baseline: Option<&Item>) -> f64 {
    match baseline.and_then(|b| b.data.as_ref()) {
        Some(d) => rt::call(&ctx.comparison, "ForceComparedItem", &[A::P(ptr(d))]),
        None => rt::call(&ctx.comparison, "DisableForceComparedItem", &[]),
    };
    item.data.as_ref().map_or(-1.0, |d| n(&ctx.comparison, "GetItemComparisonState", &[A::P(ptr(d))]))
}
/// I.refreshWeight: the encumbrance counted again from zero (the incremental counter can be stale).
pub(super) fn refresh_weight() {
    let pl = player();
    if pl.is_null() {
        return;
    }
    // the counter adds to what it holds: only recounted from zero (the script's field is m_curInventoryWeight)
    let Some(f) = rt::field(&pl, "m_curInventoryWeight") else { return };
    super::menu_rt::write(&f, 0.0);
    rt::call(&pl, "CalculateEncumbrance", &[]);
    rt::call(&pl, "EvaluateEncumbrance", &[]);
}
pub(super) struct Res {
    pub money: f64,
    pub weight: f64,
    pub capacity: f64,
}
pub(super) fn resources() -> Option<Res> {
    let pl = player();
    if pl.is_null() {
        return None;
    }
    let money_id = rt::static_call("ItemID", "FromTDBID", &[A::Name(tdb("Items.money"))]).map(|o| Id16::of(&Val::of(&o)))?;
    let money = n(&ts(), "GetItemQuantity", &[A::H(&pl), money_id.a()]);
    let defs = super::boards::defs().ok()?;
    let dd = super::boards::definition(&defs, "UI_PlayerStats").ok()?;
    let bb = super::boards::system_board(dd).ok()?;
    let f = super::boards::field(dd, "currentInventoryWeight").ok()?;
    let weight = super::boards::get::<f32>(&bb, "GetFloat", f).map_or(0.0, f64::from).floor();
    let stats = super::game_system("GetStatsSystem");
    let eid = id16(&pl, "GetEntityID", &[]);
    let capacity = n(&stats, "GetStatValue", &[eid.a(), A::N(ev("gamedataStatType", "CarryCapacity"))]).floor();
    Some(Res { money, weight, capacity })
}
pub(super) fn equipped(ctx: &mut Ctx, area_name: &str, slot: i32) -> Option<Item> {
    let key = format!("{area_name}:{slot}");
    if let Some(hit) = ctx.equipped_cache.get(&key) {
        return hit.clone();
    }
    let data = rt::call(&ctx.manager, "GetItemDataEquippedInArea", &[A::N(area(area_name)), A::N(f64::from(slot))]);
    let it = data.and_then(|d| item(ctx, d));
    ctx.equipped_cache.insert(key, it.clone());
    it
}
#[derive(Clone)]
pub(super) struct Slot {
    pub id: u64,
    pub key: String,
    pub name: String,
    pub item: Option<Item>,
    pub removable: bool,
}
pub(super) fn slots(ctx: &Ctx, it: Option<&Item>) -> Vec<Slot> {
    let mut out = Vec::new();
    let Some(data) = it.and_then(|i| i.data.clone()) else { return out };
    let mut seen = HashSet::new();
    let Some(arr) = rt::static_call("InventoryItemData", "GetAttachments", &[A::P(ptr(&data))]) else { return out };
    for part in items(&Val::of(&arr)) {
        let sid = fid(&part, "SlotID");
        if !seen.insert(sid) {
            continue;
        }
        let pdata = part.f("ItemData").and_then(|v| own(&v));
        let removable_data = pdata.as_ref().map(|d| sb("RPGManager", "CanPartBeUnequipped", &[A::P(ptr(d)), A::Name(sid)])).unwrap_or(false);
        let installed = pdata.and_then(|d| item(ctx, d));
        out.push(Slot { id: sid, key: sid.to_string(), name: loc(&ftext(&part, "SlotName")), removable: installed.is_some() && removable_data, item: installed });
    }
    out
}
pub(super) fn dual_wield() -> bool {
    let data = so("PlayerDevelopmentSystem", "GetData", &[A::H(&player())]);
    b(&data, "IsNewPerkBoughtAnyLevel", &[A::N(ev("gamedataNewPerkType", "Cool_Central_Milestone_2"))])
}
/// Left-hand guns beside slots 1 and 2 (an earlier menu offered them; the game never does): taken off.
pub(super) fn clear_stray_left(ctx: &mut Ctx) -> bool {
    let mut removed = false;
    for slot in DUAL_SLOT + 4..=5 {
        if equipped(ctx, "Weapon", slot).is_some() {
            rt::call(&ctx.manager, "UnequipItem", &[A::N(area("Weapon")), A::N(f64::from(slot)), A::B(false)]);
            removed = true;
        }
    }
    if removed {
        invalidate(ctx);
    }
    removed
}
pub(super) fn can_dual(it: Option<&Item>) -> bool {
    let Some(d) = it.and_then(|i| i.data.as_ref()) else { return false };
    let raw = idh(d, "GetGameItemData");
    b(&raw, "CanDualWield", &[A::H(&player())])
}
fn rank(q: &str) -> i32 {
    match q {
        "Legendary" => 5,
        "Epic" => 4,
        "Rare" => 3,
        "Uncommon" => 2,
        "Common" => 1,
        _ => 0,
    }
}
fn sort(mut v: Vec<Item>) -> Vec<Item> {
    v.sort_by(|a, b| b.equipped.cmp(&a.equipped).then(rank(&b.quality).cmp(&rank(&a.quality))).then_with(|| a.name.cmp(&b.name)).then_with(|| a.key.cmp(&b.key)));
    v
}
pub(super) fn area_items(ctx: &mut Ctx, area_name: &str) -> Vec<Item> {
    if let Some(c) = ctx.cache.get(area_name) {
        return c.clone();
    }
    let mut out = Vec::new();
    if let Some(arr) = rt::call_typed(&ctx.manager, "GetPlayerInventoryData", &["gamedataEquipmentArea"], &[A::N(area(area_name)), A::B(false), A::D]) {
        for v in items(&Val::of(&arr)) {
            if let Some(it) = own(&v).and_then(|d| item(ctx, d)) {
                out.push(it);
            }
        }
    }
    let out = sort(out);
    ctx.cache.insert(area_name.to_string(), out.clone());
    out
}
pub(super) fn parts(ctx: &Ctx, it: Option<&Item>, slot: Option<&Slot>) -> Vec<Item> {
    let (Some(it), Some(slot)) = (it, slot) else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(arr) = rt::call_typed(&ctx.manager, "GetPlayerInventoryPartsForItem", &["gameItemID", "TweakDBID"], &[it.id.a(), A::Name(slot.id)]) {
        for v in items(&Val::of(&arr)) {
            let Some(d) = own(&v) else { continue };
            let allowed = b(&ctx.manager, "CanInstallPart", &[A::P(ptr(&d))]);
            if let Some(mut p) = item(ctx, d) {
                p.allowed = allowed && !(p.is_mod && slot.item.is_some() && !it.iconic);
                out.push(p);
            }
        }
    }
    sort(out)
}
fn owned_item(id: &Id16) -> bool {
    !o(&ts(), "GetItemData", &[A::H(&player()), id.a()]).is_null()
}
pub(super) fn can_change(ctx: &mut Ctx, area_name: &str, slot: i32) -> bool {
    if let Some(it) = equipped(ctx, area_name, slot) {
        if it.blocked || !it.allowed {
            return false;
        }
    }
    !(area_name != "Weapon" && area_name != "Outfit" && equipped(ctx, "Outfit", 0).is_some())
}
fn loadout_item(ctx: &Ctx, id: &Id16) -> Option<Item> {
    rt::call(&ctx.manager, "GetItemDataFromIDInLoadout", &[id.a()]).and_then(|d| item(ctx, d))
}
pub(super) fn equip(ctx: &mut Ctx, area_name: &str, slot: i32, id: &Id16) -> bool {
    if !owned_item(id) || !can_change(ctx, area_name, slot) {
        return false;
    }
    let Some(it) = loadout_item(ctx, id) else { return false };
    if it.area != area(area_name) || !it.allowed {
        return false;
    }
    let r = super::lobby_ink::new("GameplayEquipRequest");
    hset_handle(&r, "owner", &player());
    hset_id(&r, "itemID", id);
    hset(&r, "slotIndex", f64::from(slot));
    hset(&r, "forceEquipWeapon", 0.0);
    hset(&r, "blockUpdateWeaponActiveSlots", 1.0);
    rt::call(&container_get("EquipmentSystem"), "QueueRequest", &[A::H(&r)]).is_some()
}
pub(super) fn unequip(ctx: &mut Ctx, area_name: &str, slot: i32) -> bool {
    if equipped(ctx, area_name, slot).is_none() || !can_change(ctx, area_name, slot) {
        return false;
    }
    rt::call(&ctx.manager, "UnequipItem", &[A::N(area(area_name)), A::N(f64::from(slot)), A::B(false)]);
    true
}
pub(super) enum Install {
    Done,
    Confirm,
    Refused,
}
pub(super) fn install(ctx: &mut Ctx, weapon: &Id16, slot_id: u64, part: &Id16, confirmed: bool) -> Install {
    if !owned_item(weapon) || !owned_item(part) {
        return Install::Refused;
    }
    invalidate(ctx);
    let Some(it) = loadout_item(ctx, weapon).filter(|i| i.allowed && !i.blocked) else { return Install::Refused };
    for sl in slots(ctx, Some(&it)).iter().filter(|s| s.id == slot_id) {
        for p in parts(ctx, Some(&it), Some(sl)).iter().filter(|p| p.id == *part && p.allowed) {
            if p.is_mod && sl.item.is_some() && !confirmed {
                return Install::Confirm;
            }
            rt::call_typed(&ctx.manager, "InstallPart", &["gameItemID", "gameItemID", "TweakDBID"], &[weapon.a(), part.a(), A::Name(slot_id)]);
            return Install::Done;
        }
    }
    Install::Refused
}
pub(super) fn remove(ctx: &mut Ctx, weapon: &Id16, slot_id: u64) -> bool {
    if !owned_item(weapon) {
        return false;
    }
    invalidate(ctx);
    let Some(it) = loadout_item(ctx, weapon).filter(|i| i.allowed && !i.blocked) else { return false };
    if !slots(ctx, Some(&it)).iter().any(|s| s.id == slot_id && s.removable) {
        return false;
    }
    let r = super::lobby_ink::new("RemoveItemPart");
    hset_handle(&r, "obj", &player());
    hset_id(&r, "baseItem", weapon);
    hset(&r, "slotToEmpty", 0.0);
    if let Some(f) = rt::field(&r, "slotToEmpty") {
        // SAFETY: a TweakDBID field.
        unsafe { *(f.ptr as *mut u64) = slot_id };
    }
    rt::call(&container_get("ItemModificationSystem"), "QueueRequest", &[A::H(&r)]).is_some()
}
fn equip_key(pd: &Handle, area_name: &str, slot: i32) -> String {
    let a = area(area_name) as u64;
    super::holster::equip_slot_item(pd, a, slot).map_or_else(String::new, |id| key_of(&Id16(id)))
}
fn item_list() -> Vec<Handle> {
    match rt::call_outs(&ts(), "GetItemList", &[A::H(&player())], 1) {
        Some((_, outs)) => outs.first().map(|a| items(&Val::of(a)).iter().map(|v| v.handle()).collect()).unwrap_or_default(),
        None => Vec::new(),
    }
}
fn preview_flag(id: &Id16) -> bool {
    sb("ItemID", "HasFlag", &[id.a(), A::N(ev("gameEItemIDFlag", "Preview"))])
}
/// I.signatureJob: the loadout and the visible backpack (eighty items a step); a changed stamp redraws.
pub(super) fn signature_job() -> Factory {
    Rc::new(|| {
        let mut st: Option<(Vec<String>, Vec<Handle>, Vec<String>, usize, Handle)> = None;
        Box::new(move || {
            let Some((values, list, visible, i, inv)) = st.as_mut() else {
                let es = container_get("EquipmentSystem");
                let pd = o(&es, "GetPlayerData", &[A::H(&player())]);
                let mut values = Vec::new();
                for slot in 0..=5 {
                    values.push(equip_key(&pd, "Weapon", slot));
                }
                for a in CLOTHES {
                    values.push(equip_key(&pd, a, 0));
                }
                st = Some((values, item_list(), Vec::new(), 0, ui_inventory()));
                return Step::More;
            };
            let last = list.len().min(*i + 80);
            while *i < last {
                let raw = &list[*i];
                *i += 1;
                let id = id16(raw, "GetID", &[]);
                if !preview_flag(&id) && !super::menu_invdata::ui_item(&inv, &id).is_null() {
                    visible.push(format!("{}:{}", key_of(&id), gd::fmt_g(n(raw, "GetQuantity", &[]))));
                }
            }
            if *i < list.len() {
                return Step::More;
            }
            visible.sort();
            let mut v = std::mem::take(values);
            v.push(visible.join(","));
            Step::Done(Rc::new(true), Some(v.join("|")))
        })
    })
}
pub(super) fn wardrobe() -> bool {
    let set = so("EquipmentSystem", "GetActiveWardrobeSet", &[A::H(&player())]);
    !set.is_null() && !sb("ClothingSet", "IsEmpty", &[A::H(&set)])
}
pub(super) fn clear_wardrobe(ctx: &Ctx) -> bool {
    if !wardrobe() || !b(&ctx.manager, "IsWardrobeEnabled", &[]) {
        return false;
    }
    let r = super::lobby_ink::new("UnequipWardrobeSetRequest");
    hset_handle(&r, "owner", &player());
    rt::call(&container_get("EquipmentSystem"), "QueueRequest", &[A::H(&r)]).is_some()
}
pub(super) fn wardrobe_enabled(ctx: &Ctx) -> bool {
    b(&ctx.manager, "IsWardrobeEnabled", &[])
}

// ---------------------------------------------------------------- the backpack (backpack_data.lua)
pub(super) const FILTERS: [&str; 12] = ["AllItems", "RangedWeapons", "MeleeWeapons", "Clothes", "Consumables", "Grenades", "Attachments", "SoftwareMods", "Programs", "Cyberware", "Junk", "Quest"];
pub(super) fn is_favorite(id: &Id16) -> bool {
    b(&ui_system(), "IsItemPlayerFavourite", &[id.a()])
}
fn category_of(v: f64) -> Option<&'static str> {
    FILTERS.iter().find(|n| ev("ItemFilterCategory", n) == v).copied()
}
fn bp_item(ui: Handle, raw: Handle) -> Item {
    let id = id16(&ui, "GetID", &[]);
    let mut v = Item {
        key: key_of(&id),
        name: s(&ui, "GetName", &[]),
        quality: s(&ui, "GetQualityName", &[]),
        quality_int: n(&ui, "GetQualityInt", &[]),
        ty: n(&ui, "GetItemType", &[]),
        equipped: b(&ui, "IsEquipped", &[]),
        new: b(&ui, "IsNew", &[]),
        iconic: b(&ui, "IsIconic", &[]),
        favorite: is_favorite(&id),
        quantity: n(&ui, "GetQuantity", &[A::B(true)]),
        junk: b(&ui, "IsJunk", &[]),
        quest: b(&ui, "IsQuestItem", &[]),
        weapon: b(&ui, "IsWeapon", &[]),
        allowed: true,
        id,
        ..Default::default()
    };
    let icon = s(&ui, "GetIconPath", &[]);
    if !icon.is_empty() && icon != "None" {
        v.image = gd::image(tdb(&icon));
    }
    v.category = if v.quest { Some("Quest") } else { category_of(n(&ui, "GetFilterCategory", &[])) };
    if v.weapon {
        v.melee = b(&raw, "HasTag", &[A::Name(cname("MeleeWeapon"))]);
        v.ammo = if v.melee { 0.0 } else { n(&ui, "GetAmmo", &[]) };
        let key = rt::static_call("UIItemsHelper", "GetItemTypeKey", &[A::N(v.ty)]).map(|o| Val::of(&o).text()).unwrap_or_default();
        v.type_name = loc(&key);
        v.type_icon = weapon_type_icon(v.ty);
    }
    v.ui = ui;
    v.raw = raw;
    v
}
#[derive(Default)]
pub(super) struct Snapshot {
    pub list: Vec<Item>,
    pub by_key: HashMap<String, usize>,
    pub junk: Vec<usize>,
    pub categories: HashSet<&'static str>,
}
impl Snapshot {
    pub(super) fn get(&self, key: &str) -> Option<&Item> {
        self.by_key.get(key).map(|&i| &self.list[i])
    }
}
/// B.snapshotJob: the backpack as the game's own screen lists it, sixty items a step.
pub(super) fn snapshot_job() -> Factory {
    Rc::new(|| {
        let mut st: Option<(Vec<Handle>, usize, Snapshot, Handle)> = None;
        Box::new(move || {
            let Some((list, i, out, inv)) = st.as_mut() else {
                let inv = ui_inventory();
                rt::call(&inv, "FlushTempData", &[]);
                st = Some((item_list(), 0, Snapshot::default(), inv));
                return Step::More;
            };
            let hidden = [cname("HideInBackpackUI"), cname("SoftwareShard")];
            let last = list.len().min(*i + 60);
            while *i < last {
                let raw = list[*i].clone();
                *i += 1;
                let id = id16(&raw, "GetID", &[]);
                if preview_flag(&id) || hidden.iter().any(|t| b(&raw, "HasTag", &[A::Name(*t)])) {
                    continue;
                }
                let ui = super::menu_invdata::ui_item(&inv, &id);
                if ui.is_null() {
                    continue;
                }
                let v = bp_item(ui, raw);
                let idx = out.list.len();
                out.by_key.insert(v.key.clone(), idx);
                if v.junk {
                    out.junk.push(idx);
                }
                if let Some(c) = v.category {
                    out.categories.insert(c);
                }
                out.list.push(v);
            }
            if *i < list.len() {
                return Step::More;
            }
            let snap = std::mem::take(out);
            let count = snap.list.len();
            Step::Done(Rc::new(snap), Some(count.to_string()))
        })
    })
}
pub(super) fn filtered(snap: &Snapshot, filter: &str) -> Vec<Item> {
    snap.list.iter().filter(|v| filter == "AllItems" || v.category == Some(filter)).cloned().collect()
}
pub(super) struct Material {
    pub name: String,
    pub image: Option<Icon>,
    pub quantity: f64,
}
pub(super) fn materials() -> (Vec<Material>, Vec<Material>) {
    let pl = player();
    let read = |f: &str| -> Vec<Material> {
        let Some(arr) = rt::static_call("UIInventoryHelper", f, &[]) else { return Vec::new() };
        items(&Val::of(&arr))
            .iter()
            .map(|v| {
                // SAFETY: a TweakDBID element.
                let t = unsafe { *(v.ptr as *const u64) };
                let rec = so("TweakDBInterface", "GetItemRecord", &[A::Name(t)]);
                let icon = ss("UIInventoryItemsManager", "ResolveItemIconName", &[A::Name(t), A::H(&rec), A::B(false)]);
                let q = rt::static_call("ItemID", "CreateQuery", &[A::Name(t)]).map(|o| Id16::of(&Val::of(&o))).unwrap_or_default();
                let dn = id(&rec, "DisplayName", &[]);
                Material {
                    name: rt::global_call("GetLocalizedItemNameByCName", &[A::Name(dn)]).map(|o| Val::of(&o).text()).unwrap_or_default(),
                    image: if icon.is_empty() { None } else { gd::image(tdb(&icon)) },
                    quantity: n(&ts(), "GetItemQuantity", &[A::H(&pl), q.a()]),
                }
            })
            .collect()
    };
    (read("GetCommonCraftingMaterials"), read("GetHackingCraftingMaterials"))
}
#[derive(Default, Clone, Copy)]
pub(super) struct Actions {
    pub favorite: bool,
    pub can_favorite: bool,
    pub max: f64,
    pub iconic: bool,
    pub drop: bool,
    pub disassemble: bool,
}
/// B.actions: what the item allows, read again at click time from the live item.
pub(super) fn actions(id: &Id16, ui_hint: &Handle) -> Actions {
    let pl = player();
    let raw = o(&ts(), "GetItemData", &[A::H(&pl), id.a()]);
    if raw.is_null() {
        return Actions::default();
    }
    let mut ui = super::menu_invdata::ui_item(&ui_inventory(), &id);
    if ui.is_null() {
        ui = ui_hint.clone();
    }
    let blocked = b(&raw, "HasTag", &[A::Name(cname("UnequipBlocked"))]);
    let es = so("EquipmentSystem", "GetInstance", &[A::H(&pl)]);
    let equipped = b(&es, "IsEquipped", &[A::H(&pl), id.a()]);
    let favorite = is_favorite(id);
    let bb = o(&pl, "GetPlayerStateMachineBlackboard", &[]);
    let in_car = (|| -> Result<i32, String> {
        let defs = super::boards::defs()?;
        let dd = super::boards::definition(&defs, "PlayerStateMachine")?;
        let f = super::boards::field(dd, "Vehicle")?;
        super::boards::get::<i32>(&bb, "GetInt", f)
    })()
    .unwrap_or(0)
        != 0;
    let count = n(&ts(), "GetItemQuantity", &[A::H(&pl), id.a()]);
    let iconic = b(&ui, "IsIconic", &[]);
    let drop = !equipped
        && !blocked
        && !favorite
        && !in_car
        && !b(&ui, "IsQuestItem", &[])
        && !iconic
        && !b(&raw, "HasTag", &[A::Name(cname("Quest"))])
        && sb("RPGManager", "CanItemBeDropped", &[A::H(&pl), A::H(&raw)])
        && sb("InventoryGPRestrictionHelper", "CanDrop", &[A::H(&ui), A::H(&pl)])
        && !so("ItemActionsHelper", "GetDropAction", &[id.a()]).is_null();
    let disassemble = !equipped && !blocked && !favorite && !so("ItemActionsHelper", "GetDisassembleAction", &[id.a()]).is_null();
    Actions { favorite, can_favorite: b(&ui, "IsWeapon", &[]), max: count.max(1.0), iconic, drop, disassemble }
}
pub(super) fn set_favorite(id: &Id16, on: bool) -> bool {
    let r = super::lobby_ink::new("UIScriptableSystemSetItemPlayerFavourite");
    hset_id(&r, "itemID", id);
    hset(&r, "favourite", if on { 1.0 } else { 0.0 });
    rt::call(&ui_system(), "QueueRequest", &[A::H(&r)]);
    true
}
fn drop_params(list: &[(Id16, f64)]) {
    // an array<ItemModParams> built by the parameter's own type (the function's array parameter)
    let Some(f) = crate::script_call::static_function("RPGManager", "DropManyItems") else { return };
    // SAFETY: the function's own parameters and their types.
    let params: Vec<&'static red4ext_abi::rtti::IType> = f.params.as_slice().iter().filter_map(|p| unsafe { p.as_ref().and_then(|p| p.ty.as_ref()) }).collect();
    let Some(aty) = params.iter().find(|t| t.kind() == 3).copied() else { return };
    let with_game = params.first().is_some_and(|t| t.name().text() == "ScriptGameInstance" || t.name().text() == "GameInstance");
    let Some(mut arr) = Owned::of_type(aty) else { return };
    let ty = arr.ty();
    // SAFETY: an array of this type, resized, then its structs filled.
    unsafe {
        ty.array_resize(arr.as_mut_ptr().cast(), list.len() as u32);
    }
    let v = Val::of(&arr);
    for (i, (id, q)) in list.iter().enumerate() {
        if let Some(e) = v.at(i) {
            if let Some(f) = e.f("itemID") {
                let n = (f.ty.size() as usize).min(16);
                // SAFETY: the element's ItemID field.
                unsafe { core::ptr::copy_nonoverlapping(id.0.as_ptr().cast::<u8>(), f.ptr.cast::<u8>(), n) };
            }
            set(&e, "quantity", *q);
        }
    }
    let g = rt::game();
    let pl = player();
    if with_game {
        rt::static_call("RPGManager", "DropManyItems", &[A::P((&g as *const crate::script_call::GameInstance).cast()), A::H(&pl), A::P(arr.as_ptr().cast())]);
    } else {
        rt::static_call("RPGManager", "DropManyItems", &[A::H(&pl), A::P(arr.as_ptr().cast())]);
    }
}
pub(super) fn drop(id: &Id16, ui: &Handle, quantity: f64) -> bool {
    let a = actions(id, ui);
    if !a.drop {
        return false;
    }
    let q = quantity.floor().clamp(1.0, a.max);
    if q == 1.0 {
        rt::static_call("ItemActionsHelper", "DropItem", &[A::H(&player()), id.a()]);
    } else {
        drop_params(&[(*id, q)]);
    }
    true
}
pub(super) fn disassemble(id: &Id16, ui: &Handle, quantity: f64) -> bool {
    let a = actions(id, ui);
    if !a.disassemble {
        return false;
    }
    let q = quantity.floor().clamp(1.0, a.max);
    rt::static_call("ItemActionsHelper", "DisassembleItem", &[A::H(&player()), id.a(), A::N(q)]);
    true
}
pub(super) fn drop_many(list: &[Item]) -> (f64, usize) {
    let (mut params, mut count, mut skipped) = (Vec::new(), 0.0, 0);
    for v in list {
        let a = actions(&v.id, &v.ui);
        if a.drop {
            params.push((v.id, a.max));
            count += a.max;
        } else {
            skipped += 1;
        }
    }
    if !params.is_empty() {
        drop_params(&params);
    }
    (count, skipped)
}
pub(super) fn disassemble_many(list: &[Item]) -> (f64, usize) {
    let (mut count, mut skipped) = (0.0, 0);
    for v in list {
        let a = actions(&v.id, &v.ui);
        if a.disassemble && !a.iconic {
            rt::static_call("ItemActionsHelper", "DisassembleItem", &[A::H(&player()), v.id.a(), A::N(a.max)]);
            count += a.max;
        } else {
            skipped += 1;
        }
    }
    (count, skipped)
}
pub(super) fn disassemble_junk(snap: &Snapshot) -> f64 {
    let pl = player();
    let mut n_total = 0.0;
    for &i in &snap.junk {
        let v = &snap.list[i];
        let count = n(&ts(), "GetItemQuantity", &[A::H(&pl), v.id.a()]);
        if count > 0.0 {
            rt::static_call("ItemActionsHelper", "DisassembleItem", &[A::H(&pl), v.id.a(), A::N(count)]);
            n_total += count;
        }
    }
    n_total
}
pub(super) fn junk_count(snap: Option<&Snapshot>) -> f64 {
    snap.map_or(0.0, |s| s.junk.iter().map(|&i| s.list[i].quantity).sum())
}
pub(super) fn filter_label(name: &str) -> String {
    let f = ev("ItemFilterCategory", name);
    loc(&ss("ItemFilterCategories", "GetLabelKey", &[A::N(f)]))
}
pub(super) fn filter_icon(name: &str) -> String {
    let f = ev("ItemFilterCategory", name);
    ss("ItemFilterCategories", "GetIcon", &[A::N(f)])
}

// ---------------------------------------------------------------- EquipmentEx's outfits (wardrobe_data.lua)
fn system(name: &str) -> Handle {
    container_get(name)
}
fn outfits() -> Handle {
    system("EquipmentEx.OutfitSystem")
}
pub(super) fn wd_text(key: &str) -> String {
    rt::global_call("GetLocalizedTextByKey", &[A::Name(cname(key))]).map(|o| Val::of(&o).text()).unwrap_or_default()
}
pub(super) fn wd_available() -> bool {
    !outfits().is_null()
}
pub(super) fn wd_blocked() -> bool {
    let s = outfits();
    s.is_null() || b(&s, "IsBlocked", &[])
}
pub(super) struct Outfit {
    pub name: u64,
    pub label: String,
    pub equipped: bool,
}
pub(super) fn wd_outfits() -> (bool, Vec<Outfit>) {
    let s = outfits();
    if s.is_null() {
        return (false, Vec::new());
    }
    let mut list = Vec::new();
    if let Some(arr) = rt::call(&s, "GetOutfits", &[]) {
        for v in items(&Val::of(&arr)) {
            // SAFETY: a CName element.
            let name = unsafe { *(v.ptr as *const u64) };
            let eq = rt::call_typed(&s, "IsEquipped", &["CName"], &[A::Name(name)]).is_some_and(|o| Val::of(&o).bool());
            list.push(Outfit { name, label: v.text(), equipped: eq });
        }
    }
    (b(&s, "IsActive", &[]), list)
}
pub(super) fn wd_load(name: u64) -> bool {
    let s = outfits();
    !s.is_null() && b(&s, "LoadOutfit", &[A::Name(name)])
}
pub(super) fn wd_deactivate() -> bool {
    let s = outfits();
    if !s.is_null() && b(&s, "IsActive", &[]) {
        rt::call(&s, "Deactivate", &[]);
    }
    !s.is_null()
}
pub(super) fn wd_delete(name: u64) -> bool {
    let s = outfits();
    !s.is_null() && b(&s, "DeleteOutfit", &[A::Name(name)])
}
fn string_to_name(t: &str) -> u64 {
    let a = Arg::new(t);
    rt::global_call("StringToName", &[a.a()]).map_or(cname(t), |o| unsafe { *(o.as_ptr() as *const u64) })
}
pub(super) fn wd_save() -> bool {
    let s = outfits();
    if s.is_null() || !b(&s, "IsActive", &[]) {
        return false;
    }
    let mut base = wd_text("UI-Inventory-Labels-Outfit");
    if base.is_empty() {
        base = "Outfit".into();
    }
    let mut i = 1;
    while b(&s, "HasOutfit", &[A::Name(string_to_name(&format!("{base} {i}")))]) {
        i += 1;
    }
    b(&s, "SaveOutfit", &[A::Name(string_to_name(&format!("{base} {i}"))), A::B(false)])
}
const SOURCES: [(f64, &str); 3] = [(1.0, "UI-EquipmentEx-WardrobeItemSource-InventoryAndStash"), (2.0, "UI-EquipmentEx-WardrobeItemSource-InventoryOnly"), (0.0, "UI-EquipmentEx-WardrobeItemSource-WardrobeStore")];
fn source_index(v: &Handle) -> usize {
    let value = n(v, "GetItemSource", &[]);
    SOURCES.iter().position(|s| s.0 == value).unwrap_or(0)
}
pub(super) fn wd_source() -> String {
    let v = system("EquipmentEx.ViewManager");
    if v.is_null() {
        return String::new();
    }
    wd_text(SOURCES[source_index(&v)].1)
}
pub(super) fn wd_next_source() -> bool {
    let v = system("EquipmentEx.ViewManager");
    if v.is_null() {
        return false;
    }
    let next = SOURCES[(source_index(&v) + 1) % SOURCES.len()].0;
    rt::call(&v, "SetItemSource", &[A::N(next)]);
    true
}
pub(super) struct Group {
    pub slot: Rc<Owned>,
    pub key: String,
    pub name: String,
    pub collapsed: bool,
    pub equipped: Option<Item>,
    pub items: Vec<Item>,
}
fn slot_key(v: &Val) -> String {
    format!("{}:{}", gd::fmt_g(fnum(v, "hash")), gd::fmt_g(fnum(v, "length")))
}
/// W.groups: the clothing EquipmentEx offers, by its outfit slots in its order (`equipped` meaning worn in the outfit).
pub(super) fn wd_groups(ctx: &Ctx) -> Vec<Group> {
    let (s, h, v) = (outfits(), system("EquipmentEx.InventoryHelper"), system("EquipmentEx.ViewManager"));
    if s.is_null() || h.is_null() || v.is_null() {
        return Vec::new();
    }
    let mut order: Vec<Group> = Vec::new();
    if let Some(arr) = rt::call(&s, "GetOutfitSlots", &[]) {
        for slot in items(&Val::of(&arr)) {
            let Some(own_slot) = own(&slot) else { continue };
            let name = rt::call(&s, "GetSlotName", &[A::P(ptr(&own_slot))]).map(|o| Val::of(&o).text()).unwrap_or_default();
            let collapsed = b(&v, "IsCollapsed", &[A::P(ptr(&own_slot))]);
            order.push(Group { key: slot_key(&slot), slot: Rc::new(own_slot), name, collapsed, equipped: None, items: Vec::new() });
        }
    }
    if let Some(arr) = rt::call(&h, "GetAvailableItems", &[A::D]) {
        for d in items(&Val::of(&arr)) {
            let Some(it) = own(&d).and_then(|d| item(ctx, d)) else { continue };
            let slot_k = rt::call(&s, "GetItemSlot", &[it.id.a()]).map(|o| slot_key(&Val::of(&o))).unwrap_or_default();
            if let Some(g) = order.iter_mut().find(|g| g.key == slot_k) {
                let mut it = it;
                it.equipped = rt::call_typed(&s, "IsEquipped", &["gameItemID"], &[it.id.a()]).is_some_and(|o| Val::of(&o).bool());
                it.key = format!("{}:{}", it.key, g.key);
                if it.equipped {
                    g.equipped = Some(it.clone());
                }
                g.items.push(it);
            }
        }
    }
    order
        .into_iter()
        .filter(|g| !g.items.is_empty())
        .map(|mut g| {
            g.items.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.key.cmp(&b.key)));
            g
        })
        .collect()
}
pub(super) fn wd_toggle_slot(g: &Group) {
    let v = system("EquipmentEx.ViewManager");
    if !v.is_null() {
        rt::call(&v, "ToggleCollapsed", &[A::P(ptr(&g.slot))]);
    }
}
pub(super) fn wd_toggle(it: &Item) -> bool {
    let s = outfits();
    if s.is_null() {
        return false;
    }
    let worn = rt::call_typed(&s, "IsEquipped", &["gameItemID"], &[it.id.a()]).is_some_and(|o| Val::of(&o).bool());
    b(&s, if worn { "UnequipItem" } else { "EquipItem" }, &[it.id.a()])
}

// ---------------------------------------------------------------- the game's sort order (item_sort.lua)
#[derive(Clone)]
pub(super) struct SortOption {
    pub key: String,
    pub mode: f64,
    pub label: String,
    pub direction: f64,
}
thread_local! {
    static OPTIONS: std::cell::RefCell<Option<Vec<SortOption>>> = const { std::cell::RefCell::new(None) };
}
/// FromVariant for an enum held inline (or behind its pointer) in a Variant.
fn variant_num(v: &Val) -> f64 {
    // SAFETY: a Variant: its type pointer (low bit: held inline), then its data.
    unsafe {
        let p = v.ptr as *const usize;
        let tyw = *p;
        let ty = (tyw & !1usize) as *const red4ext_abi::rtti::IType;
        if ty.is_null() {
            return 0.0;
        }
        let data = if tyw & 1 != 0 { p.add(1) as *const u8 } else { *(p.add(1)) as *const u8 };
        match (*ty).size() {
            1 => f64::from(*(data as *const i8)),
            2 => f64::from(*(data as *const i16)),
            8 => *(data as *const i64) as f64,
            _ => f64::from(*(data as *const i32)),
        }
    }
}
pub(super) fn sort_options() -> Vec<SortOption> {
    if let Some(o) = OPTIONS.with(|c| c.borrow().clone()) {
        return o;
    }
    let mut out = Vec::new();
    if let Some(arr) = rt::static_call("SortingDropdownData", "GetDefaultDropdownOptions", &[]) {
        for e in items(&Val::of(&arr)) {
            let h = e.handle();
            let Some(idv) = pf(&h, "identifier") else { continue };
            let mode = variant_num(&idv);
            out.push(SortOption { key: enum_name("ItemSortMode", mode), mode, label: super::menu_ink::upper(&loc(&ptext(&h, "labelKey"))), direction: pnum(&h, "direction") });
        }
    }
    if !out.is_empty() {
        OPTIONS.with(|c| *c.borrow_mut() = Some(out.clone()));
    }
    out
}
pub(super) fn sort_option(key: Option<&str>) -> SortOption {
    let key = match key {
        Some("quality") | None => "QualityAsc",
        Some("name") => "NameAsc",
        Some("weight") => "WeightDesc",
        Some(k) => k,
    };
    let opts = sort_options();
    opts.iter().find(|o| o.key == key).or_else(|| opts.iter().find(|o| o.key == "QualityAsc")).or(opts.first()).cloned().unwrap_or(SortOption { key: "QualityAsc".into(), mode: 0.0, label: String::new(), direction: 0.0 })
}
/// S.apply: the items in the game's own backpack order (its comparator), a trade's quoted price for a price order.
pub(super) fn sort_apply(mut list: Vec<Item>, key: Option<&str>) -> Vec<Item> {
    let mode = sort_option(key);
    let view = super::lobby_ink::new("BackpackDataView");
    hset(&view, "itemSortMode", mode.mode);
    let inv = ui_inventory();
    let mut wraps: Vec<Handle> = Vec::with_capacity(list.len());
    for it in list.iter_mut() {
        if it.ui.is_null() {
            let raw = if it.raw.is_null() { it.data.as_ref().map(|d| idh(d, "GetGameItemData")).unwrap_or_else(Handle::null) } else { it.raw.clone() };
            it.ui = o(&inv, "GetPlayerItemFromAnySource", &[A::H(&raw)]);
        }
        let w = super::lobby_ink::new("WrappedInventoryItemData");
        hset_handle(&w, "Item", &it.ui);
        wraps.push(w);
    }
    let price_mode = mode.key == "PriceAsc" || mode.key == "PriceDesc";
    let quality_mode = mode.key == "QualityAsc" || mode.key == "QualityDesc";
    let mut idx: Vec<usize> = (0..list.len()).collect();
    let quick: Vec<(bool, f64, String)> = if quality_mode {
        list.iter().map(|it| (b(&it.ui, "IsPlayerFavourite", &[]), if it.quality_int != 0.0 { it.quality_int } else { n(&it.ui, "GetQualityInt", &[]) }, if it.name.is_empty() { s(&it.ui, "GetName", &[]) } else { it.name.clone() })).collect()
    } else {
        Vec::new()
    };
    idx.sort_by(|&a, &b2| {
        use std::cmp::Ordering;
        let (x, y) = (&list[a], &list[b2]);
        if quality_mode {
            let (qa, qb) = (&quick[a], &quick[b2]);
            if qa.0 != qb.0 {
                return if qa.0 { Ordering::Less } else { Ordering::Greater };
            }
            if qa.1 != qb.1 {
                let ord = qb.1.partial_cmp(&qa.1).unwrap_or(Ordering::Equal);
                return if mode.key == "QualityAsc" { ord } else { ord.reverse() };
            }
            return qa.2.to_lowercase().cmp(&qb.2.to_lowercase()).then_with(|| x.key.cmp(&y.key));
        }
        if price_mode {
            if let (Some(pa), Some(pb)) = (x.price, y.price) {
                if pa != pb {
                    let ord = pa.partial_cmp(&pb).unwrap_or(Ordering::Equal);
                    return if mode.key == "PriceAsc" { ord } else { ord.reverse() };
                }
            }
        }
        if b(&view, "SortItemNew", &[A::H(&wraps[a]), A::H(&wraps[b2])]) {
            return Ordering::Less;
        }
        if b(&view, "SortItemNew", &[A::H(&wraps[b2]), A::H(&wraps[a])]) {
            return Ordering::Greater;
        }
        x.key.cmp(&y.key)
    });
    let mut out: Vec<Option<Item>> = list.into_iter().map(Some).collect();
    idx.into_iter().filter_map(|i| out[i].take()).collect()
}

// ---------------------------------------------------------------- the menu's preferences (preferences.lua)
fn prefs_path() -> std::path::PathBuf {
    cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_Menu\\menu_preferences.json")
}
fn pref(key: &str) -> bool {
    std::fs::read_to_string(prefs_path()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v.get(key).and_then(|x| x.as_bool())) == Some(true)
}
/// The attribute +1's first confirmation, kept for the session when the file cannot be written (P.attributeUpgradeConfirmed).
static ATTRIBUTE_UPGRADE_CONFIRMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn save_prefs(character_model: bool, attribute_upgrade_confirmed: bool) -> bool {
    std::fs::write(prefs_path(), format!("{{\"characterModel\":{character_model},\"attributeUpgradeConfirmed\":{attribute_upgrade_confirmed}}}\n")).is_ok()
}
pub(super) fn character_model() -> bool {
    pref("characterModel")
}
pub(super) fn set_character_model(on: bool) -> bool {
    save_prefs(on, attribute_upgrade_confirmed())
}
/// After the first confirmed +1 the next ones spend the point at once (main's reference_view.lua).
pub(super) fn attribute_upgrade_confirmed() -> bool {
    ATTRIBUTE_UPGRADE_CONFIRMED.load(std::sync::atomic::Ordering::Relaxed) || pref("attributeUpgradeConfirmed")
}
pub(super) fn confirm_attribute_upgrade() {
    ATTRIBUTE_UPGRADE_CONFIRMED.store(true, std::sync::atomic::Ordering::Relaxed);
    if !save_prefs(character_model(), true) {
        cvr_host::log!("[VRMenu] Could not save attribute upgrade preference (from Rust)\n");
    }
}
