//! cyberware_data.lua, cyberware_anatomy.lua (+ its layout) and cyberware_view.lua: the implants by equipment area
//! (every one the player owns, installed or not, and the ripperdoc's stock during a visit), the capacity and armor
//! meters with their tooltips, an area's slots and items over the anatomy figure focused on it (the organ layers
//! aligned as ripperdoc.inkwidget aligns them), the game's tooltips, INSTALL / BUY AND INSTALL (the purchase followed
//! until the item arrives, then installed), the seeded upgrade with its choices.
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_core::*;
use super::menu_gameplay as gp;
use super::menu_gpdata as gd;
use super::menu_ink as k;
use super::menu_invdata::{self as idt, Item};
use super::menu_invview as iv;
use super::menu_locale as loc;
use super::menu_rt::*;
use crate::script_call::Owned;
use red4ext_abi::handle::Handle;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

const W: f64 = 1780.0;
const AREAS: [&str; 10] = ["FrontalCortexCW", "SystemReplacementCW", "ArmsCW", "EyesCW", "MusculoskeletalSystemCW", "HandsCW", "NervousSystemCW", "CardiovascularSystemCW", "IntegumentarySystemCW", "LegsCW"];
fn tr(ru: &str, en: &str) -> String {
    loc::tr(ru, en)
}
fn col_of(q: &str) -> [f64; 3] {
    match q {
        "Common" => k::GREY,
        "Uncommon" => [0.2, 0.85, 0.5],
        "Rare" => [0.2, 0.6, 1.0],
        "Epic" => [0.75, 0.3, 1.0],
        "Legendary" => k::YELLOW,
        _ => k::CYAN,
    }
}

// ---------------------------------------------------------------- data (cyberware_data.lua)
#[derive(Clone)]
pub(super) struct Cw {
    pub item: Item,
    pub vendor: bool,
    pub available: bool,
    pub price: f64,
    pub capacity: f64,
    pub slot: i32,
    pub upgrade: Option<Rc<Upgrade>>,
}
pub(super) struct Upgrade {
    pub allowed: bool,
    pub record: Handle,
    pub cost: Option<Owned>,
    pub have: f64,
}
#[derive(Clone, Copy, Default)]
pub(super) struct Stats {
    used: f64,
    max: f64,
    base: f64,
    extra: f64,
    over: f64,
    health_loss: f64,
    armor: f64,
    armor_max: f64,
    reduction: f64,
}
fn stat(name: &str) -> f64 {
    let p = player();
    let eid = id16(&p, "GetEntityID", &[]);
    n(&super::game_system("GetStatsSystem"), "GetStatValue", &[eid.a(), A::N(ev("gamedataStatType", name))])
}
fn stats() -> Stats {
    let sd = super::game_system("GetStatsDataSystem");
    let (total, over) = (stat("HumanityTotalMaxValue"), stat("HumanityOverallocationPossible"));
    let armor_max = n(&sd, "GetValueFromCurve", &[A::Name(cname("cyberware_curves")), A::N(stat("Level")), A::Name(cname("max_armor_for_level"))]);
    let factor = stat("ArmorEffectivenessMultiplier") * n(&sd, "GetArmorEffectivenessValue", &[A::B(true)]);
    let (health, penalty) = (stat("Health"), stat("EdgerunnerHealthReduction"));
    Stats {
        used: stat("HumanityAllocated"),
        max: total,
        base: total - over,
        extra: over,
        over: stat("HumanityOverallocated"),
        health_loss: if penalty < 1.0 { health / (1.0 - penalty) * penalty } else { 0.0 },
        armor: stat("Armor"),
        armor_max,
        reduction: (1.0 - 1.0 / (1.0 + armor_max * factor)) * 100.0,
    }
}
fn upgrade_of(it: &Item) -> Option<Rc<Upgrade>> {
    if !it.equipped {
        return None;
    }
    let (ok, outs) = rt::static_outs("RPGManager", "CanUpgradeCyberware", &[A::H(&player()), it.id.a(), A::B(true), A::N(ev("gamedataQuality", "Invalid")), A::B(true)])?;
    let allowed = ok.as_ref().is_some_and(|o| Val::of(o).bool());
    let record = outs.iter().map(Val::of).find(|v| v.ty.kind() == 9 || v.ty.kind() == 10).map(|v| v.handle()).unwrap_or_else(Handle::null);
    if record.is_null() {
        return None;
    }
    let cost = outs.into_iter().find(|o| Val::of(o).f("materialRecordID").is_some());
    let have = cost.as_ref().map_or(0.0, |c| {
        let mat = fid(&Val::of(c), "materialRecordID");
        let q = rt::static_call("ItemID", "CreateQuery", &[A::Name(mat)]).map(|o| Id16::of(&Val::of(&o))).unwrap_or_default();
        n(&super::game_system("GetTransactionSystem"), "GetItemQuantityWithDuplicates", &[A::H(&player()), q.a()])
    });
    Some(Rc::new(Upgrade { allowed, record, cost, have }))
}
fn requirements(raw: &Handle) -> Vec<(f64, f64)> {
    let Some(arr) = rt::static_call("RPGManager", "GetEquipRequirements", &[A::H(&player()), A::H(raw)]) else { return Vec::new() };
    items(&Val::of(&arr)).iter().map(|r| (fnum(r, "statType"), fnum(r, "requiredValue"))).collect()
}
fn cw_item(ctx: &idt::Ctx, raw: &Handle, vendor: bool, ui: Option<Handle>) -> Option<Cw> {
    if raw.is_null() {
        return None;
    }
    let data = rt::call(&ctx.manager, "GetInventoryItemData", &[A::H(raw)])?;
    let mut it = idt::item(ctx, data)?;
    it.raw = raw.clone();
    it.ui = ui.unwrap_or_else(|| super::menu_invdata::ui_item(&idt::ui_inventory(), &it.id));
    let humanity = ev("gamedataStatType", "HumanityAvailable");
    let capacity = requirements(raw).iter().find(|r| r.0 == humanity).map_or(0.0, |r| r.1);
    let upgrade = if vendor { None } else { upgrade_of(&it) };
    Some(Cw { item: it, vendor, available: true, price: 0.0, capacity, slot: -1, upgrade })
}
pub(super) struct Group {
    area: &'static str,
    name: String,
    slots: Vec<(i32, Option<Cw>)>,
    items: Vec<Cw>,
}
pub(super) struct Snapshot {
    groups: Vec<Group>,
    by_key: HashMap<String, Cw>,
    stats: Stats,
    vendor: bool,
}
fn player_data() -> Handle {
    let c = super::game_system("GetScriptableSystemsContainer");
    let es = o(&c, "Get", &[A::Name(cname("EquipmentSystem"))]);
    o(&es, "GetPlayerData", &[A::H(&player())])
}
fn equip_id(pd: &Handle, area: &str, slot: i32) -> Option<Id16> {
    super::holster::equip_slot_item(pd, idt::area(area) as u64, slot).map(Id16).filter(|i| sb("ItemID", "IsValid", &[i.a()]))
}
fn slots_count(pd: &Handle, area: &str) -> i32 {
    n(pd, "GetNumberOfSlots", &[A::N(idt::area(area))]) as i32
}
fn item_list() -> Vec<Handle> {
    match rt::call_outs(&super::game_system("GetTransactionSystem"), "GetItemList", &[A::H(&player())], 1) {
        Some((_, outs)) => outs.first().map(|a| items(&Val::of(a)).iter().map(|v| v.handle()).collect()).unwrap_or_default(),
        None => Vec::new(),
    }
}
fn snapshot(ctx: &idt::Ctx) -> Snapshot {
    let p = player();
    let pd = player_data();
    let ts = super::game_system("GetTransactionSystem");
    let vendor = super::menu_vendor::ripper_session();
    let mut out = Snapshot { groups: Vec::new(), by_key: HashMap::new(), stats: stats(), vendor };
    let mut owned: HashMap<i64, Vec<Handle>> = HashMap::new();
    for data in item_list() {
        let iid = id16(&data, "GetID", &[]);
        if sb("ItemID", "HasFlag", &[iid.a(), A::N(ev("gameEItemIDFlag", "Preview"))]) {
            continue;
        }
        let area = sn("EquipmentSystem", "GetEquipAreaType", &[iid.a()]) as i64;
        owned.entry(area).or_default().push(data);
    }
    let inv = idt::ui_inventory();
    for area in AREAS {
        let count = slots_count(&pd, area);
        let rec = so("TweakDBInterface", "GetEquipmentAreaRecord", &[A::Name(tdb(&format!("EquipmentArea.{area}")))]);
        let mut g = Group { area, name: loc(&s(&rec, "LocalizedName", &[])), slots: Vec::new(), items: Vec::new() };
        for slot in 0..count {
            let it = equip_id(&pd, area, slot).and_then(|iid| cw_item(ctx, &o(&ts, "GetItemData", &[A::H(&p), iid.a()]), false, None)).map(|mut c| {
                c.slot = slot;
                c
            });
            if let Some(c) = &it {
                out.by_key.insert(c.item.key.clone(), c.clone());
            }
            g.slots.push((slot, it));
        }
        for data in owned.get(&(idt::area(area) as i64)).cloned().unwrap_or_default() {
            let ui = super::menu_invdata::ui_item(&inv, &id16(&data, "GetID", &[]));
            if let Some(c) = cw_item(ctx, &data, false, Some(ui)) {
                out.by_key.insert(c.item.key.clone(), c.clone());
                g.items.push(c);
            }
        }
        g.items.sort_by(|a, b| b.item.equipped.cmp(&a.item.equipped).then_with(|| a.item.name.cmp(&b.item.name)));
        out.groups.push(g);
    }
    if vendor {
        if let Some((vend, manager)) = super::menu_vendor::visit_parts() {
            let mgr_items = o(&inv, "GetInventoryItemsManager", &[]);
            if let Some(arr) = rt::call(&manager, "GetRipperDocItems", &[]) {
                for v in items(&Val::of(&arr)) {
                    let raw = v.f("itemData").map(|x| x.handle()).unwrap_or_else(Handle::null);
                    let ui = so("UIInventoryItem", "Make", &[A::H(&vend), A::H(&raw), A::H(&mgr_items)]);
                    let Some(mut c) = cw_item(ctx, &raw, true, Some(ui)) else { continue };
                    c.item.key = format!("vendor:{}", c.item.key);
                    c.available = v.f("itemStack").and_then(|st| rt::static_call("VendorItemAdditionalData", "Make", &[A::P(st.ptr.cast_const())])).is_some_and(|a| fbool(&Val::of(&a), "IsAvailable"));
                    c.price = n(&manager, "GetBuyingPrice", &[c.item.id.a()]);
                    if let Some(g) = out.groups.iter_mut().find(|g| idt::area(g.area) == c.item.area) {
                        out.by_key.insert(c.item.key.clone(), c.clone());
                        g.items.push(c);
                    }
                }
            }
        }
    }
    out
}
fn cyber_type(iid: &Id16) -> u64 {
    // TweakDBID.new(id, ".cyberwareType"): the record's flat by its name appended to the id
    let flat = red4ext_abi::tweakdbid::derive(iid.tdbid(), ".cyberwareType");
    rt::static_call("TweakDBInterface", "GetCName", &[A::Name(flat), A::Name(cname("None"))]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
/// D.check: whether the item fits the slot (the same cyberware type's slot first), and the capacity it lacks.
fn check(c: &Cw, area: &str, slot: i32) -> (bool, f64, i32) {
    let pd = player_data();
    let count = slots_count(&pd, area);
    if slot < 0 || slot >= count {
        return (false, 0.0, slot);
    }
    let mut slot = slot;
    let mut replaced = equip_id(&pd, area, slot);
    let kind = cyber_type(&c.item.id);
    for i in 0..count {
        if let Some(iid) = equip_id(&pd, area, i) {
            if cyber_type(&iid) == kind {
                (slot, replaced) = (i, Some(iid));
                break;
            }
        }
    }
    let pl = player();
    let ts = super::game_system("GetTransactionSystem");
    let old = replaced.map(|r| o(&ts, "GetItemData", &[A::H(&pl), r.a()])).filter(|h| !h.is_null());
    let old_req = old.as_ref().map(requirements).unwrap_or_default();
    let (mut excess, mut ok) = (0.0, true);
    let humanity = ev("gamedataStatType", "HumanityAvailable");
    let eid = id16(&pl, "GetEntityID", &[]);
    let ss = super::game_system("GetStatsSystem");
    for (st, req) in requirements(&c.item.raw) {
        let required = req - old_req.iter().find(|r| r.0 == st).map_or(0.0, |r| r.1);
        let available = n(&ss, "GetStatValue", &[eid.a(), A::N(st)]);
        if required > available + 0.001 {
            ok = false;
            if st == humanity {
                excess = required - available;
            }
        }
    }
    if let Some(o0) = &old {
        if b(o0, "HasTag", &[A::Name(cname("UnequipBlocked"))]) {
            ok = false;
        }
        if b(o0, "HasTag", &[A::Name(cname("CapacityBooster"))]) && n(&ss, "GetStatValue", &[eid.a(), A::N(humanity)]) < n(&ss, "GetStatValue", &[eid.a(), A::N(ev("gamedataStatType", "CapacityBoosterHumanity"))]) {
            ok = false;
        }
    }
    if c.vendor && !c.available {
        ok = false;
    }
    (ok, excess, slot)
}
fn queue_equipment(r: &Handle) {
    let c = super::game_system("GetScriptableSystemsContainer");
    rt::call(&o(&c, "Get", &[A::Name(cname("EquipmentSystem"))]), "QueueRequest", &[A::H(r)]);
}
fn area_items_keys(area: f64) -> std::collections::HashSet<String> {
    rt::objs(&idt::ui_inventory(), "GetPlayerAreaItems", &[A::N(area)]).iter().map(|u| idt::key_of(&id16(u, "GetID", &[]))).collect()
}
fn equip(m: &Menu, c: &Cw, area: &str, slot: i32) -> bool {
    if !super::menu_vendor::ripper_session() || super::menu_vendor::purchasing(m) {
        return false;
    }
    let (allowed, _, target) = check(c, area, slot);
    if !allowed {
        return false;
    }
    let pl = player();
    if c.vendor {
        let Some((_, manager)) = super::menu_vendor::visit_parts() else { return false };
        let available = rt::call(&manager, "GetRipperDocItems", &[]).is_some_and(|arr| {
            items(&Val::of(&arr)).iter().any(|v| {
                let raw = v.f("itemData").map(|x| x.handle()).unwrap_or_else(Handle::null);
                id16(&raw, "GetID", &[]) == c.item.id && v.f("itemStack").and_then(|st| rt::static_call("VendorItemAdditionalData", "Make", &[A::P(st.ptr.cast_const())])).is_some_and(|a| fbool(&Val::of(&a), "IsAvailable"))
            })
        });
        if !available || n(&manager, "GetBuyingPrice", &[c.item.id.a()]) > n(&manager, "GetLocalPlayerCurrencyAmount", &[]) {
            return false;
        }
        let before = area_items_keys(c.item.area);
        let purchase = super::menu_vendor::Purchase { record: c.item.id.tdbid(), slot: target, area: area.to_string(), before, at: m.time, polled: None };
        super::menu_vendor::with_visit(|v| {
            if let Some(v) = v {
                v.purchase = Some(purchase);
            }
        });
        rt::call(&manager, "BuyItemFromVendor", &[A::H(&c.item.raw), A::N(1.0)]);
        return true;
    }
    let ts = super::game_system("GetTransactionSystem");
    if !b(&ts, "HasItem", &[A::H(&pl), c.item.id.a()]) {
        return false;
    }
    let pd = player_data();
    let previous = equip_id(&pd, area, target);
    if previous == Some(c.item.id) {
        return false;
    }
    if previous.is_some() {
        let r = ink::new("UnequipRequest");
        idt::hset_handle(&r, "owner", &pl);
        hset(&r, "areaType", idt::area(area));
        hset(&r, "slotIndex", f64::from(target));
        queue_equipment(&r);
    }
    let r = ink::new("GameplayEquipRequest");
    idt::hset_handle(&r, "owner", &pl);
    idt::hset_id(&r, "itemID", &c.item.id);
    hset(&r, "slotIndex", f64::from(target));
    queue_equipment(&r);
    true
}
/// D.updatePurchase: the bought implant, once it is in the player's items, installed; true when it ended.
fn update_purchase(m: &mut Menu) -> bool {
    let Some((record, slot, area, before, at, polled)) = super::menu_vendor::with_visit(|v| v.and_then(|v| v.purchase.as_ref().map(|p| (p.record, p.slot, p.area.clone(), p.before.clone(), p.at, p.polled)))) else { return false };
    if polled.is_some_and(|t| m.time - t < 0.1) {
        return false;
    }
    super::menu_vendor::with_visit(|v| {
        if let Some(p) = v.and_then(|v| v.purchase.as_mut()) {
            p.polled = Some(m.time);
        }
    });
    for raw in item_list() {
        let iid = id16(&raw, "GetID", &[]);
        if iid.tdbid() == record && !before.contains(&idt::key_of(&iid)) {
            super::menu_vendor::with_visit(|v| {
                if let Some(v) = v {
                    v.purchase = None;
                }
            });
            rt::call(&o(&idt::ui_inventory(), "GetInventoryItemsManager", &[]), "FlushEquippedItems", &[]);
            if let Some(c) = m.gp.cyber.ctx.as_ref().and_then(|ctx| cw_item(ctx, &raw, false, None)) {
                equip(m, &c, &area, slot);
            }
            return true;
        }
    }
    if m.time - at > 5.0 {
        super::menu_vendor::with_visit(|v| {
            if let Some(v) = v {
                v.purchase = None;
            }
        });
        let t = m.tx("blocked");
        toast(m, &t);
        return true;
    }
    false
}
pub(super) struct Prepared {
    base: Cw,
    up: Rc<Upgrade>,
    data: Owned,
    seed: f64,
    choice: i32,
}
fn inventory_manager() -> Handle {
    super::game_system("GetInventoryManager")
}
fn prepare_upgrade(c: &Cw) -> Option<Prepared> {
    if !super::menu_vendor::ripper_session() {
        return None;
    }
    let up = upgrade_of(&c.item)?;
    let cost = up.cost.as_ref()?;
    let seed = n(&inventory_manager(), "GetCyberwareUpgradeSeed", &[]);
    let pl = player();
    let show = sb("RPGManager", "ShouldShowCWChoice", &[A::H(&pl), A::H(&c.item.ui)]);
    let data = rt::static_call("RPGManager", "GetCyberwareUpgradeData", &[A::H(&pl), A::H(&c.item.ui), A::H(&up.record), A::P(ptr(cost)), A::N(seed), A::H(&idt::ui_inventory()), A::B(show)])?;
    Some(Prepared { base: c.clone(), up, data, seed, choice: 1 })
}
fn option_field(p: &Prepared, i: i32, f: &str) -> Handle {
    Val::of(&p.data).f(&format!("option{i}{f}")).map(|v| v.handle()).unwrap_or_else(Handle::null)
}
fn apply_upgrade(m: &Menu, p: &Prepared) -> bool {
    if !super::menu_vendor::ripper_session() {
        return false;
    }
    let pd = player_data();
    let slot = n(&pd, "GetSlotIndex", &[p.base.item.id.a()]) as i32;
    let Some(area) = AREAS.iter().find(|a| idt::area(a) == p.base.item.area) else { return false };
    if slot < 0 || equip_id(&pd, area, slot) != Some(p.base.item.id) {
        return false;
    }
    let Some(up) = upgrade_of(&p.base.item) else { return false };
    if !up.allowed || id(&up.record, "GetID", &[]) != id(&p.up.record, "GetID", &[]) || n(&inventory_manager(), "GetCyberwareUpgradeSeed", &[]) != p.seed {
        return false;
    }
    let chosen = option_field(p, p.choice, "GameItemData");
    if chosen.is_null() {
        return false;
    }
    let Some(ctx) = m.gp.cyber.ctx.as_ref() else { return false };
    let Some(candidate) = cw_item(ctx, &chosen, false, Some(option_field(p, p.choice, "InventoryItem"))) else { return false };
    let Some(carea) = AREAS.iter().find(|a| idt::area(a) == candidate.item.area) else { return false };
    if !check(&candidate, carea, slot).0 {
        return false;
    }
    let r = ink::new("RipperdocTokenPopupCloseData");
    hset(&r, "confirm", 1.0);
    hset(&r, "chosenOptionIndex", f64::from(p.choice));
    idt::hset_handle(&r, "chosenOptionData", &chosen);
    if let (Some(f), Some(cost)) = (rt::field(&r, "costData"), up.cost.as_ref()) {
        // SAFETY: the same struct type assigns its value.
        unsafe { f.ty.assign(f.ptr, cost.as_ptr().cast()) };
    }
    rt::static_call("RPGManager", "HandleBuyShardPopupClosed", &[A::H(&player()), p.base.item.id.a(), A::H(&r)]);
    true
}
fn signature() -> String {
    let pd = player_data();
    let mut a = Vec::new();
    for area in AREAS {
        for sl in 0..slots_count(&pd, area) {
            a.push(super::holster::equip_slot_item(&pd, idt::area(area) as u64, sl).map_or_else(String::new, |i| idt::key_of(&Id16(i))));
        }
    }
    let st = stats();
    a.extend([gd::fmt_g(st.used), gd::fmt_g(st.max), gd::fmt_g(st.armor)]);
    let ts = super::game_system("GetTransactionSystem");
    for tier in ["Common", "Uncommon", "Rare", "Epic", "Legendary"] {
        let q = rt::static_call("ItemID", "CreateQuery", &[A::Name(tdb(&format!("Items.{tier}Material1")))]).map(|o| Id16::of(&Val::of(&o))).unwrap_or_default();
        a.push(gd::fmt_g(n(&ts, "GetItemQuantityWithDuplicates", &[A::H(&player()), q.a()])));
    }
    a.join("|")
}

// ---------------------------------------------------------------- the anatomy (cyberware_anatomy.lua)
const MAN: [(&str, f64, f64, f64); 11] = [("ma_arms", 51.0, -315.0, 0.9), ("ma_circular", 50.0, -85.0, 0.9), ("ma_full_skeleton", 50.0, -85.0, 0.9), ("ma_hands", 50.0, -152.0, 0.9), ("ma_immune", 50.0, -85.0, 0.9), ("ma_intergumentary", 50.0, -85.0, 0.9), ("ma_legs", 39.0, 261.0, 0.9), ("ma_nervous", 50.0, -85.0, 0.9), ("ma_ocular", 50.0, -824.0, 0.9), ("ma_operating_system", 62.0, -580.0, 0.9), ("ma_pfrontal_cortex", 56.0, -765.0, 0.9)];
const WOMAN: [(&str, f64, f64, f64); 11] = [("wo_arms", 46.0, -274.0, 0.9), ("wo_circular", 42.0, -110.0, 0.9), ("wo_frontal_cortex", 53.0, -760.0, 0.9), ("wo_hands", 42.0, -157.0, 0.9), ("wo_immune", 42.0, -110.0, 0.9), ("wo_intergumentary", 42.0, -110.0, 0.9), ("wo_legs", 49.0, 210.0, 0.9), ("wo_nervous", 42.0, -110.0, 0.9), ("wo_ocular", 52.0, -815.0, 0.9), ("wo_operating_system", 42.0, -611.0, 0.9), ("wo_skeleton", 42.0, -110.0, 0.9)];
fn organ(area: &str, female: bool) -> &'static str {
    let (m, w) = match area {
        "FrontalCortexCW" => ("ma_pfrontal_cortex", "wo_frontal_cortex"),
        "SystemReplacementCW" => ("ma_operating_system", "wo_operating_system"),
        "ArmsCW" => ("ma_arms", "wo_arms"),
        "EyesCW" => ("ma_ocular", "wo_ocular"),
        "MusculoskeletalSystemCW" => ("ma_full_skeleton", "wo_skeleton"),
        "HandsCW" => ("ma_hands", "wo_hands"),
        "NervousSystemCW" => ("ma_nervous", "wo_nervous"),
        "CardiovascularSystemCW" => ("ma_circular", "wo_circular"),
        "IntegumentarySystemCW" => ("ma_intergumentary", "wo_intergumentary"),
        _ => ("ma_legs", "wo_legs"),
    };
    if female {
        w
    } else {
        m
    }
}
fn focus_of(area: &str) -> [f64; 3] {
    match area {
        "FrontalCortexCW" => [0.5, 0.13, 3.2],
        "SystemReplacementCW" => [0.5, 0.21, 2.0],
        "ArmsCW" => [0.5, 0.36, 1.4],
        "EyesCW" => [0.5, 0.14, 3.2],
        "MusculoskeletalSystemCW" => [0.5, 0.43, 1.25],
        "HandsCW" => [0.5, 0.48, 1.4],
        "NervousSystemCW" => [0.5, 0.35, 1.65],
        "CardiovascularSystemCW" => [0.5, 0.30, 1.65],
        "IntegumentarySystemCW" => [0.5, 0.40, 1.3],
        _ => [0.5, 0.76, 1.5],
    }
}
struct Part {
    widget: Handle,
    part: &'static str,
    fitted: bool,
    rect: (f64, f64, f64, f64),
}
pub(super) struct Body {
    w: f64,
    h: f64,
    female: bool,
    base: &'static str,
    bw: f64,
    bh: f64,
    body: Handle,
    parts: Vec<(&'static str, Part)>,
    scale: f64,
    zoom: [f64; 3],
    target: [f64; 3],
    area: Option<&'static str>,
    placed: bool,
}
fn layout(female: bool, part: &str) -> (f64, f64, f64) {
    let t: &[(&str, f64, f64, f64)] = if female { &WOMAN } else { &MAN };
    t.iter().find(|e| e.0 == part).map_or((0.0, 0.0, 0.9), |e| (e.1, e.2, e.3))
}
fn body_new(parent: &Handle, x: f64, y: f64, w: f64, h: f64) -> Body {
    let female = id(&player(), "GetResolvedGenderName", &[]) == cname("Female");
    let base = if female { "wo_intergumentary" } else { "ma_intergumentary" };
    let (bw, bh) = if female { (735.0, 1781.0) } else { (1089.0, 1861.0) };
    let mask = ink::place(ink::new("inkScrollAreaWidget"), parent, x, y, w, h);
    ink::call(&mask, "SetFitToContent", &[crate::script_call::Value::Bool(false)]);
    ink::call(&mask, "SetUseInternalMask", &[crate::script_call::Value::Bool(true)]);
    hset(&mask, "constrainContentPosition", 0.0);
    hset(&mask, "fitToContentDirection", ev("inkFitToContentDirection", "None"));
    let content = ink::canvas(&mask, 0.0, 0.0, w, h);
    let atlas = format!("base\\gameplay\\gui\\fullscreen\\ripperdoc\\{}.inkatlas", if female { "woman_body" } else { "man_body" });
    let body = k::image(&content, 0.0, 0.0, bw, bh, &atlas, base, Some(k::WHITE), 1.0);
    let mut parts = Vec::new();
    for area in AREAS {
        let part = organ(area, female);
        if part != base {
            let img = k::image(&content, 0.0, 0.0, 1.0, 1.0, &atlas, part, Some(k::CYAN), 0.0);
            ink::call(&img, "SetFitToContent", &[crate::script_call::Value::Bool(true)]);
            parts.push((area, Part { widget: img, part, fitted: false, rect: (0.0, 0.0, 0.0, 0.0) }));
        }
    }
    Body { w, h, female, base, bw, bh, body, parts, scale: (w / bw).min(h / bh), zoom: [0.5, 0.5, 1.0], target: [0.5, 0.5, 1.0], area: None, placed: false }
}
fn body_focus(b: &mut Body, area: Option<&'static str>, zoom: bool) {
    b.area = area;
    b.target = match (area, zoom) {
        (Some(a), true) => focus_of(a),
        _ => [0.5, 0.5, 1.0],
    };
}
fn body_update(b: &mut Body, dt: f64) {
    let base = layout(b.female, b.base);
    for (area, p) in b.parts.iter_mut() {
        if !p.fitted {
            if let Some(o) = rt::call(&p.widget, "GetDesiredSize", &[]) {
                let v = Val::of(&o);
                let (sx, sy) = (v.f("X").and_then(|x| x.num()).unwrap_or(0.0), v.f("Y").and_then(|x| x.num()).unwrap_or(0.0));
                if sx > 1.0 && sy > 1.0 {
                    let src = layout(b.female, p.part);
                    let scale = src.2 / base.2;
                    ink::call(&p.widget, "SetFitToContent", &[crate::script_call::Value::Bool(false)]);
                    let (pw, ph) = (sx * scale, sy * scale);
                    p.rect = (b.bw / 2.0 + (src.0 - base.0) / base.2 - pw / 2.0, b.bh / 2.0 + (src.1 - base.1) / base.2 - ph / 2.0, pw, ph);
                    b.placed = false;
                    p.fitted = true;
                }
            }
        }
        ink::opacity(&p.widget, if b.area == Some(*area) { 1.0 } else { 0.0 });
    }
    ink::opacity(&b.body, if b.area.is_some_and(|a| a != "IntegumentarySystemCW") { 0.35 } else { 0.85 });
    let mut moved = false;
    for i in 0..3 {
        let d = b.target[i] - b.zoom[i];
        if d.abs() > 0.0005 {
            b.zoom[i] += d * (dt * 12.0).min(1.0);
            moved = true;
        } else {
            b.zoom[i] = b.target[i];
        }
    }
    if moved || !b.placed {
        let scale = b.scale * b.zoom[2];
        let (x, y) = (b.w / 2.0 - b.zoom[0] * b.bw * scale, b.h / 2.0 - b.zoom[1] * b.bh * scale);
        ink::move_to(&b.body, x, y);
        ink::size(&b.body, b.bw * scale, b.bh * scale);
        for (_, p) in &b.parts {
            if p.fitted {
                ink::move_to(&p.widget, x + p.rect.0 * scale, y + p.rect.1 * scale);
                ink::size(&p.widget, p.rect.2 * scale, p.rect.3 * scale);
            }
        }
        b.placed = true;
    }
}

// ---------------------------------------------------------------- the view (cyberware_view.lua)
#[derive(Default)]
pub(super) struct CyberState {
    pub ctx: Option<idt::Ctx>,
    slot: i32,
    area: Option<&'static str>,
    selected: Option<String>,
    shown: Option<Option<String>>,
    pulses: Vec<Handle>,
    tips: Vec<iv::Tip>,
    snapshot: Option<Rc<Snapshot>>,
    stamp: String,
    poll: f64,
    body: Option<Body>,
    upgrade: Option<Rc<RefCell<Prepared>>>,
    refresh_at: Option<f64>,
    last_frame: Option<f64>,
    tip_host: Handle,
    tip_block: Option<BtnRef>,
    tip_block_bar: Option<BtnRef>,
    tip_plates: Option<iv::Plates>,
}
thread_local! {
    static HOVER: RefCell<(Option<String>, Option<&'static str>)> = const { RefCell::new((None, None)) };
    static HOVER_AT: Cell<f64> = const { Cell::new(0.0) };
    static NOW: Cell<f64> = const { Cell::new(0.0) };
}
fn hover(key: String, area: Option<&'static str>, on: bool) {
    let now = NOW.with(|c| c.get());
    HOVER.with(|h| {
        let mut h = h.borrow_mut();
        if on {
            *h = (Some(key), area);
            HOVER_AT.with(|c| c.set(now));
        } else if h.0.as_deref() == Some(key.as_str()) {
            *h = (None, None);
            HOVER_AT.with(|c| c.set(now));
        }
    });
}
fn ensure(m: &mut Menu) {
    if m.gp.cyber.ctx.is_none() {
        m.gp.cyber.ctx = Some(idt::open());
    }
}
fn redraw_reset(m: &mut Menu) {
    m.gp.reset_list = true;
    gp::redraw(m);
}
#[allow(clippy::too_many_arguments)]
fn action(m: &mut Menu, p: &Handle, x: f64, y: f64, w: f64, ru: &str, en: &str, f: Act, enabled: bool, key: &str) -> BtnRef {
    let b = gp::hub_action(m, p, P1X, x, y, w, &tr(ru, en), f, enabled, None, None);
    *b.key.borrow_mut() = Some(key.to_string());
    b
}
#[allow(clippy::too_many_arguments)]
fn card(m: &mut Menu, p: &Handle, hh: &At, x: f64, y: f64, size: f64, item: Option<&Cw>, area: &'static str, click: Act, key: &str, unavailable: bool) -> BtnRef {
    let mut col = item.map_or(k::CYAN, |c| col_of(&c.item.quality));
    if unavailable {
        col = k::RED;
    }
    let bg = k::round_fill(p, x, y, size, size, col, if unavailable { 0.23 } else { 0.045 }, Some(4.0));
    let edge = k::cut_frame(&mut m.pool, p, x, y, size, size, if unavailable { k::RED } else { k::CYAN }, if unavailable { 0.75 } else { 0.35 }, Some(9.0), Some(1.0));
    match item {
        Some(c) => {
            gp::picture(m, p, x + 9.0, y + 12.0, size - 18.0, size - 25.0, c.item.image.as_ref(), None);
            k::rect(p, x + 2.0, y + 5.0, 5.0, size - 10.0, col, 1.0);
            if c.upgrade.as_ref().is_some_and(|u| u.allowed) {
                let a = ink::canvas(p, x + size - 32.0, y + 7.0, 26.0, 26.0);
                k::round_edge(&a, 0.0, 0.0, 26.0, 26.0, k::YELLOW, 1.0, Some(3.0));
                gp::line(&a, 6.0, 18.0, 13.0, 10.0, k::YELLOW, 1.0, 3.0);
                gp::line(&a, 13.0, 10.0, 20.0, 18.0, k::YELLOW, 1.0, 3.0);
                m.gp.cyber.pulses.push(a);
            }
            if c.item.new {
                k::image(p, x + 12.0, y + 7.0, 9.0, 9.0, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
            }
            if c.item.equipped {
                gp::line(p, x + size - 28.0, y + size - 18.0, x + size - 20.0, y + size - 10.0, k::CYAN, 1.0, 3.0);
                gp::line(p, x + size - 20.0, y + size - 10.0, x + size - 9.0, y + size - 25.0, k::CYAN, 1.0, 3.0);
            }
        }
        None => {
            k::text(p, x, y, size, size, "+", 34.0, k::GREY, "center", false);
        }
    }
    let hkey = item.map_or(format!("area:{area}"), |c| c.item.key.clone());
    let b = button(
        m,
        "p1",
        hh,
        x,
        y,
        size,
        size,
        Some(click),
        Some(Rc::new(move |on, _| {
            ink::tint(&bg, if unavailable { k::RED } else if on { k::YELLOW } else { col });
            ink::opacity(&bg, if unavailable { 0.23 } else if on { 0.14 } else { 0.045 });
            k::tint(&edge, if unavailable { k::RED } else if on { k::YELLOW } else { k::CYAN }, Some(if on { 1.0 } else if unavailable { 0.75 } else { 0.35 }));
            hover(hkey.clone(), Some(area), on);
        })),
    );
    *b.key.borrow_mut() = Some(key.to_string());
    b
}
fn tooltip(m: &mut Menu, parent: &Handle, c: &Cw, x: f64, y: f64, w: f64, h: f64, upgrade: bool, base: Option<&Cw>) {
    if c.item.ui.is_null() {
        return;
    }
    let ctx_data = so("ItemDisplayContextData", "Make", &[A::H(&player()), A::N(ev("gameItemDisplayContext", if c.vendor { "Vendor" } else { "GearPanel" })), A::B(false)]);
    if super::menu_vendor::ripper_session() {
        rt::call(&ctx_data, "AddTag", &[A::Name(cname("Ripperdoc"))]);
    }
    if upgrade {
        rt::call(&ctx_data, "AddTag", &[A::Name(cname("CyberwareUpgrade"))]);
    }
    let data = so("UIInventoryItemTooltipWrapper", "Make", &[A::H(&c.item.ui), A::H(&ctx_data)]);
    if let Some(bs) = base {
        let cmp = so("UIInventoryItemComparisonManager", "Make", &[A::H(&bs.item.ui), A::H(&c.item.ui)]);
        idt::hset_handle(&data, "comparisonData", &cmp);
    }
    let deck = b(&c.item.raw, "HasTag", &[A::Name(cname("Cyberdeck"))]);
    let (path, library) = if deck { ("base\\gameplay\\gui\\common\\tooltip\\cyberdecktooltip.inkwidget", "cyberdeckTooltip") } else { ("base\\gameplay\\gui\\common\\tooltip\\itemtooltip.inkwidget", "itemTooltip") };
    // a spawn or SetData that fails, or a tooltip that never lays out, shows the menu's own card instead of the empty
    // black plate (item_tooltip.lua, issue #188)
    let (controller, parent2, ui, price) = (m.shared.menu_controller.clone(), parent.clone(), c.item.ui.clone(), c.item.price);
    let spawn: iv::SpawnTip = Rc::new(move || {
        // its plate behind it, sized to what it shows once it has laid out (tip_fit)
        let back = iv::tip_back(&parent2);
        let result = match iv::tip_widget(&controller, &parent2, path, library) {
            Ok(widget) => match iv::tip_feed(&widget, x, y, &data, library) {
                Ok(()) => return iv::Spawned::Tip(iv::Tip::spawned(widget, &ui, &parent2, x, y, w, h, price).backed(back)),
                Err(why) => iv::Spawned::Failed(why),
            },
            Err(why) if why == "loading" => iv::Spawned::Loading,
            Err(why) => iv::Spawned::Failed(why),
        };
        rt::call(&parent2, "RemoveChild", &[A::H(&back)]);
        result
    });
    if let Some(t) = iv::tip_create(spawn, &c.item.ui, parent, x, y, w, h, c.item.price) {
        m.gp.cyber.tips.push(t);
    }
}
fn bar(m: &mut Menu, p: &Handle, x: f64, kind: &'static str, d: &Stats) {
    let capacity = kind == "capacity";
    let col = if capacity { k::YELLOW } else { k::CYAN };
    let max = if capacity { d.max.max(d.used) } else { d.armor_max.max(d.armor) };
    let value = if capacity { d.used } else { d.armor };
    let baseline = if capacity { d.base } else { d.armor_max };
    let (sy, sh) = (220.0, 750.0);
    k::text(p, x - 55.0, 120.0, 170.0, 70.0, &if capacity { tr("ЛИМИТ", "CAPACITY") } else { tr("БРОНЯ", "ARMOR") }, 25.0, col, "center", false);
    for i in 0..50 {
        let v = f64::from(i + 1) / 50.0 * max;
        k::rect(p, x, sy + sh - f64::from(i + 1) * 15.0, 55.0, 7.0, if capacity && v > baseline { k::RED } else { col }, if v <= value { 0.9 } else { 0.13 });
    }
    let y = sy + sh - value / max.max(1.0) * sh;
    k::rect(p, x - 10.0, y, 83.0, 3.0, col, 1.0);
    k::text(p, x - 55.0, sy + sh + 16.0, 170.0, 66.0, &format!("{} / {}", (value + 0.5).floor() as i64, ((if capacity { d.max } else { d.armor_max }) + 0.5).floor() as i64), 28.0, col, "center", false);
    if capacity && d.extra > 0.0 {
        let by = sy + sh - d.base / max.max(1.0) * sh;
        k::rect(p, x - 20.0, by, 95.0, 3.0, k::RED, 1.0);
    }
    let b = button(
        m,
        "p1",
        &at(P1X, P1Y),
        x - 35.0,
        120.0,
        125.0,
        940.0,
        Some(Rc::new(|m: &mut Menu, _| {
            (m.gp.skills, m.gp.attribute, m.gp.perk) = (false, Some(ev("gamedataStatType", "TechnicalAbility")), None);
            gp::open_section(m, "character");
        })),
        Some(Rc::new(move |on, _| hover(format!("bar:{kind}"), None, on))),
    );
    *b.key.borrow_mut() = Some(format!("cyberBar:{kind}"));
}
fn bar_tip(p: &Handle, kind: &str) {
    let d = stats();
    let col = if kind == "capacity" { k::YELLOW } else { k::CYAN };
    let title = if kind == "capacity" { tr("ЛИМИТ ИМПЛАНТОВ", "CYBERWARE CAPACITY") } else { tr("БРОНЯ", "ARMOR") };
    k::round_fill(p, 0.0, 0.0, 660.0, 830.0, k::BLACK, 1.0, Some(12.0));
    k::round_edge(p, 0.0, 0.0, 660.0, 830.0, col, 0.9, Some(12.0));
    k::text(p, 25.0, 20.0, 610.0, 60.0, &title, 36.0, col, "left", false);
    let r = |v: f64| (v + 0.5).floor() as i64;
    let text = if kind == "capacity" {
        format!(
            "{}{} / {}\n\n{}{}\n{}{}\n\n{}{}\n{}{}\n{}{:.1}%",
            tr("Установлено: ", "Installed: "),
            r(d.used),
            r(d.max),
            tr("Базовый лимит: ", "Base capacity: "),
            r(d.base),
            tr("Настоящий киберпанк: +", "Edgerunner: +"),
            r(d.extra),
            tr("Превышение базового лимита: ", "Capacity above the base limit: "),
            r(d.over),
            tr("Уменьшение запаса здоровья: -", "Health reduction: -"),
            r(d.health_loss),
            tr("Вероятность ярости: ", "Fury chance: "),
            d.over * 0.1
        )
    } else {
        format!(
            "{} / {}\n\n{}\n\n{}{:.0}%\n\n{}",
            r(d.armor),
            r(d.armor_max),
            tr("Броня определяет вашу устойчивость к урону. Её можно повысить имплантами категорий «Скелет» и «Кожа».", "Armor determines your resistance to damage. Increase it with Skeleton and Integumentary System cyberware."),
            tr("Снижение урона при максимальной броне для вашего уровня: ", "Damage reduction at the armor cap for your level: "),
            d.reduction,
            tr("Влияющая способность: Лицензия на хром, уровень 2.", "Related perk: License to Chrome, rank 2.")
        )
    };
    let lines = gp::wrap(&text, 39);
    k::text(p, 25.0, 104.0, 610.0, lines.len() as f64 * 39.0, &lines.join("\n"), 30.0, k::TEXT, "left", false);
    let perk = so("TweakDBInterface", "GetNewPerkRecord", &[A::Name(tdb(if kind == "capacity" { "NewPerks.Tech_Master_Perk_3" } else { "NewPerks.Tech_Central_Milestone_3" }))]);
    if let Some(icon) = gd::image(id(&o(&perk, "PerkIcon", &[]), "GetID", &[])) {
        k::image_raw(p, 25.0, 640.0, 70.0, 70.0, icon.atlas, icon.part, Some(k::YELLOW), 1.0);
    }
    k::text(p, 110.0, 640.0, 520.0, 70.0, &if kind == "capacity" { tr("НАСТОЯЩИЙ КИБЕРПАНК", "EDGERUNNER") } else { tr("ЛИЦЕНЗИЯ НА ХРОМ — 2", "LICENSE TO CHROME — 2") }, 27.0, k::YELLOW, "left", false);
    k::text(p, 25.0, 735.0, 610.0, 70.0, &tr("Нажмите на шкалу: дерево способностей", "Click the meter to open the perk tree"), 27.0, k::CYAN, "left", true);
}
pub(super) fn draw(m: &mut Menu, p: &Handle, _p2: &Handle) {
    ensure(m);
    NOW.with(|c| c.set(m.time));
    {
        let s = &mut m.gp.cyber;
        s.pulses.clear();
        s.tips.clear();
        s.shown = None;
        s.poll = m.time;
    }
    HOVER.with(|h| *h.borrow_mut() = (None, None));
    let data = Rc::new(snapshot(m.gp.cyber.ctx.as_ref().expect("ctx")));
    m.gp.cyber.snapshot = Some(data.clone());
    m.gp.cyber.stamp = signature();
    glass(m, p, W, 1210.0, Some((P1X, P1Y)));
    gp::title(p, &tr("КИБЕРИМПЛАНТЫ", "CYBERWARE"), W);
    let origin = at(P1X, P1Y);
    if let Some(up) = m.gp.cyber.upgrade.clone() {
        return draw_upgrade(m, p, &origin, up);
    }
    bar(m, p, 65.0, "capacity", &data.stats);
    bar(m, p, W - 110.0, "armor", &data.stats);
    let group = data.groups.iter().find(|g| Some(g.area) == m.gp.cyber.area);
    if let Some(group) = group {
        action(
            m,
            p,
            180.0,
            105.0,
            270.0,
            "К ИМПЛАНТАМ",
            "CYBERWARE",
            Rc::new(|m: &mut Menu, _| {
                (m.gp.cyber.area, m.gp.cyber.selected) = (None, None);
                redraw_reset(m);
            }),
            true,
            "cyberBack",
        );
        k::text(p, 475.0, 107.0, 600.0, 60.0, &group.name, 32.0, k::CYAN, "left", false);
        let areas: Vec<&'static str> = data.groups.iter().map(|g| g.area).collect();
        for (x, delta, key) in [(1110.0, -1i32, "cyberPrevious"), (1220.0, 1, "cyberNext")] {
            let areas = areas.clone();
            action(
                m,
                p,
                x,
                105.0,
                90.0,
                "",
                "",
                Rc::new(move |m: &mut Menu, _| {
                    let i = areas.iter().position(|a| Some(*a) == m.gp.cyber.area).unwrap_or(0) as i32;
                    let next = (i + delta).rem_euclid(areas.len() as i32) as usize;
                    (m.gp.cyber.area, m.gp.cyber.slot, m.gp.cyber.selected) = (Some(areas[next]), 0, None);
                    redraw_reset(m);
                }),
                true,
                key,
            );
        }
        gp::line(p, 1164.0, 124.0, 1146.0, 139.0, k::CYAN, 1.0, 3.0);
        gp::line(p, 1146.0, 139.0, 1164.0, 154.0, k::CYAN, 1.0, 3.0);
        gp::line(p, 1256.0, 124.0, 1274.0, 139.0, k::CYAN, 1.0, 3.0);
        gp::line(p, 1274.0, 139.0, 1256.0, 154.0, k::CYAN, 1.0, 3.0);
        for (i, (index, it)) in group.slots.iter().enumerate() {
            let (index, key) = (*index, it.as_ref().map(|c| c.item.key.clone()));
            card(
                m,
                p,
                &origin,
                185.0 + i as f64 * 151.0,
                195.0,
                138.0,
                it.as_ref(),
                group.area,
                Rc::new(move |m: &mut Menu, _| {
                    (m.gp.cyber.slot, m.gp.cyber.selected) = (index, key.clone());
                    gp::redraw(m);
                }),
                &format!("cyberSlot{index}"),
                false,
            );
        }
        let mut body = body_new(p, 965.0, 170.0, 650.0, 865.0);
        body_focus(&mut body, Some(group.area), true);
        m.gp.cyber.body = Some(body);
        let (columns, size) = (4usize, 170.0);
        let slot = m.gp.cyber.slot;
        let area = group.area;
        let mut rows = Vec::new();
        for chunk in group.items.chunks(columns) {
            let chunk = chunk.to_vec();
            rows.push(Row {
                h: 192.0,
                build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                    for (i, item) in chunk.iter().enumerate() {
                        let (allowed, excess, _) = check(item, area, slot);
                        let unavailable = !allowed && !item.item.equipped;
                        let key = item.item.key.clone();
                        card(
                            m,
                            c,
                            &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                            i as f64 * 184.0,
                            6.0,
                            size,
                            Some(item),
                            area,
                            Rc::new(move |m: &mut Menu, _| {
                                m.gp.cyber.selected = Some(key.clone());
                                if let (Some(ctx), Some(mut it)) = (m.gp.cyber.ctx.as_ref(), m.gp.cyber.snapshot.as_ref().and_then(|s| s.by_key.get(&key)).map(|c| c.item.clone())) {
                                    idt::inspect(ctx, &mut it);
                                }
                                gp::redraw(m);
                            }),
                            &format!("cyberItem:{}", item.item.key),
                            unavailable,
                        );
                        if unavailable {
                            let x = i as f64 * 184.0;
                            k::round_fill(c, x + 5.0, 130.0, size - 10.0, 41.0, k::BLACK, 0.85, Some(5.0));
                            k::image(c, x + 9.0, 133.0, 34.0, 34.0, k::ATLAS_COMMON, "ico_lock", Some(k::RED), 1.0);
                            let t = if excess > 0.0 { format!("+{} {}", excess.ceil() as i64, tr("ЛИМИТ", "CAPACITY")) } else { tr("ТРЕБУЕТСЯ", "REQUIRES") };
                            k::text(c, x + 44.0, 133.0, size - 50.0, 36.0, &t, 21.0, k::RED, "center", false);
                        }
                    }
                }),
            });
        }
        let host = ink::canvas(p, 166.0, 0.0, 778.0, 1080.0);
        let keep = m.keep_scroll;
        let v = gp::glist(m, &host, "p1", "p1", P1X + 166.0, 370.0, 778.0, rows, 620.0, keep);
        v.borrow_mut().enable_mask();
        if let Some(sel) = m.gp.cyber.selected.clone().and_then(|k0| data.by_key.get(&k0).cloned()) {
            let (ok, excess, _) = check(&sel, area, slot);
            if excess > 0.0 {
                k::text(p, 180.0, 1015.0, 740.0, 65.0, &format!("{}{}", tr("Не хватает лимита: ", "Capacity shortfall: "), excess.ceil() as i64), 29.0, k::RED, "left", false);
            }
            if data.vendor {
                if sel.item.equipped {
                    let s2 = sel.clone();
                    action(
                        m,
                        p,
                        190.0,
                        1110.0,
                        730.0,
                        "УЛУЧШИТЬ",
                        "UPGRADE",
                        Rc::new(move |m: &mut Menu, _| {
                            if let Some(pp) = prepare_upgrade(&s2) {
                                m.gp.cyber.upgrade = Some(Rc::new(RefCell::new(pp)));
                                gp::redraw(m);
                            }
                        }),
                        sel.upgrade.is_some(),
                        "cyberUpgrade",
                    );
                } else {
                    let (ru, en) = if sel.vendor { (format!("КУПИТЬ И УСТАНОВИТЬ  €$ {}", gd::fmt_g(sel.price)), format!("{}  €$ {}", loc::t("BUY AND INSTALL"), gd::fmt_g(sel.price))) } else { ("УСТАНОВИТЬ".into(), "INSTALL".into()) };
                    let s2 = sel.clone();
                    action(
                        m,
                        p,
                        190.0,
                        1110.0,
                        730.0,
                        &ru,
                        &en,
                        Rc::new(move |m: &mut Menu, _| {
                            if equip(m, &s2, area, slot) {
                                m.gp.cyber.refresh_at = Some(m.time + 0.6);
                            } else {
                                let t = m.tx("blocked");
                                toast(m, &t);
                            }
                        }),
                        ok,
                        "cyberEquip",
                    );
                }
            }
        }
    } else {
        m.gp.cyber.body = Some(body_new(p, 620.0, 115.0, 530.0, 1000.0));
        for (i, g) in data.groups.iter().enumerate() {
            let left = i % 2 == 0;
            let x = if left { 180.0 } else { 1165.0 };
            let y = 155.0 + (i / 2) as f64 * 188.0;
            k::text(p, x, y - 40.0, 430.0, 40.0, &k::clip(&g.name, 30), 26.0, k::RED, if left { "right" } else { "left" }, false);
            let bx = if left { x + 430.0 - g.slots.len() as f64 * 138.0 } else { x };
            for (j, (index, it)) in g.slots.iter().enumerate() {
                let (area, index) = (g.area, *index);
                card(
                    m,
                    p,
                    &origin,
                    bx + j as f64 * 138.0,
                    y,
                    128.0,
                    it.as_ref(),
                    g.area,
                    Rc::new(move |m: &mut Menu, _| {
                        (m.gp.cyber.area, m.gp.cyber.slot, m.gp.cyber.selected) = (Some(area), index, None);
                        redraw_reset(m);
                    }),
                    &format!("cyberArea:{}:{index}", g.area),
                    false,
                );
            }
        }
    }
    if !data.vendor {
        k::text(p, 180.0, 1110.0, 1430.0, 70.0, &tr("Посетите рипера, чтобы установить или улучшить импланты.", "Visit a ripperdoc to install or upgrade cyberware."), 29.0, k::CYAN, "center", false);
    }
    m.gp.cyber.tip_host = ink::canvas(p, 0.0, 0.0, W, 1210.0);
    let block = button(m, "p2", &origin, 980.0, 165.0, 690.0, 925.0, None, None);
    block.blocker.set(true);
    block.visible.set(false);
    let block_bar = button(m, "p2", &origin, 520.0, 230.0, 660.0, 830.0, None, None);
    block_bar.blocker.set(true);
    block_bar.visible.set(false);
    m.gp.cyber.tip_plates = Some(iv::plate_blocker(&block));
    (m.gp.cyber.tip_block, m.gp.cyber.tip_block_bar) = (Some(block), Some(block_bar));
}
fn hide_tip(m: &Menu) {
    let s = &m.gp.cyber;
    for b in [&s.tip_block, &s.tip_block_bar].into_iter().flatten() {
        b.visible.set(false);
    }
}
fn draw_upgrade(m: &mut Menu, p: &Handle, origin: &At, up: Rc<RefCell<Prepared>>) {
    k::text(p, 25.0, 106.0, 1550.0, 50.0, &tr("УЛУЧШЕНИЕ — ВЫБЕРИТЕ БОНУСЫ", "UPGRADE — SELECT BONUSES"), 34.0, k::YELLOW, "left", false);
    let close = gp::head_action(
        m,
        p,
        P1X,
        W - 72.0,
        "",
        Rc::new(|m: &mut Menu, _| {
            m.gp.cyber.upgrade = None;
            redraw_reset(m);
        }),
        true,
        Some(72.0),
    );
    *close.key.borrow_mut() = Some("cyberUpgradeClose".into());
    gp::line(p, W - 120.0, 34.0, W - 96.0, 58.0, k::CYAN, 1.0, 3.0);
    gp::line(p, W - 96.0, 34.0, W - 120.0, 58.0, k::CYAN, 1.0, 3.0);
    let (base, mut options) = {
        let u = up.borrow();
        (u.base.clone(), Vec::new())
    };
    for i in 1..=3 {
        let ui = option_field(&up.borrow(), i, "InventoryItem");
        if !ui.is_null() {
            let raw = o(&ui, "GetItemData", &[]);
            if let Some(c) = m.gp.cyber.ctx.as_ref().and_then(|ctx| cw_item(ctx, &raw, false, Some(ui.clone()))) {
                options.push((i, c));
            }
        }
    }
    let width = (W - 50.0) / (options.len() + 1) as f64;
    tooltip(m, p, &base, 20.0, 175.0, width - 12.0, 870.0, true, None);
    let choice = up.borrow().choice;
    for (n_i, (index, c)) in options.iter().enumerate() {
        let x = 20.0 + (n_i + 1) as f64 * width;
        tooltip(m, p, c, x, 175.0, width - 12.0, 870.0, true, Some(&base));
        let sel = choice == *index;
        let edge = k::round_edge(p, x, 165.0, width - 12.0, 890.0, k::YELLOW, if sel { 1.0 } else { 0.15 }, Some(8.0));
        let (u2, index) = (up.clone(), *index);
        let b = button(
            m,
            "p1",
            origin,
            x,
            165.0,
            width - 12.0,
            890.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                u2.borrow_mut().choice = index;
                gp::redraw(m);
            })),
            Some(Rc::new(move |h, _| k::tint(&edge, k::YELLOW, Some(if h || sel { 1.0 } else { 0.15 })))),
        );
        *b.key.borrow_mut() = Some(format!("cyberUpgradeOption{index}"));
    }
    let current = upgrade_of(&base.item);
    let enough = current.as_ref().is_some_and(|c| c.allowed);
    let count = up.borrow().up.cost.as_ref().map_or(0.0, |c| fnum(&Val::of(c), "materialCount"));
    let have = up.borrow().up.have;
    k::text(p, 25.0, 1080.0, 730.0, 90.0, &format!("{}{} / {}", tr("Компоненты: ", "Components: "), gd::fmt_g(have), gd::fmt_g(count)), 30.0, if enough { k::CYAN } else { k::RED }, "left", false);
    let u3 = up.clone();
    action(
        m,
        p,
        810.0,
        1100.0,
        930.0,
        "УЛУЧШИТЬ",
        "UPGRADE",
        Rc::new(move |m: &mut Menu, _| {
            if apply_upgrade(m, &u3.borrow()) {
                m.gp.cyber.upgrade = None;
                m.gp.cyber.refresh_at = Some(m.time + 0.6);
            } else {
                let t = tr("Недостаточно ресурсов или изменились условия", "Insufficient resources or requirements changed");
                toast(m, &t);
            }
        }),
        enough && super::menu_vendor::ripper_session(),
        "cyberUpgradeConfirm",
    );
}
pub(super) fn update(m: &mut Menu) {
    NOW.with(|c| c.set(m.time));
    let now = m.time;
    let dt = (now - m.gp.cyber.last_frame.unwrap_or(now)).min(0.1);
    m.gp.cyber.last_frame = Some(now);
    for p in &m.gp.cyber.pulses {
        ink::opacity(p, 0.45 + 0.55 * (0.5 + 0.5 * (now * 5.0).sin()));
    }
    if m.gp.cyber.upgrade.is_none() {
        if let Some(b) = m.gp.cyber.body.as_mut() {
            body_update(b, dt);
        }
    }
    iv::tip_fit(&mut m.gp.cyber.tips);
    if let Some(plates) = &m.gp.cyber.tip_plates {
        iv::set_plates(plates, &m.gp.cyber.tips);
    }
    if m.gp.cyber.refresh_at.is_some_and(|t| now >= t) {
        m.gp.cyber.refresh_at = None;
        if let Some(c) = m.gp.cyber.ctx.as_mut() {
            idt::invalidate(c);
        }
        gp::redraw(m);
        return;
    }
    if update_purchase(m) {
        m.gp.cyber.refresh_at = Some(now + 0.4);
    }
    if m.gp.cyber.upgrade.is_none() && !m.gp.cyber.tip_host.is_null() {
        let (hk, harea) = HOVER.with(|h| h.borrow().clone());
        let key = hk.or_else(|| m.gp.cyber.selected.clone());
        if Some(&key) != m.gp.cyber.shown.as_ref() && now - HOVER_AT.with(|c| c.get()) > 0.12 {
            let host = m.gp.cyber.tip_host.clone();
            ink::remove_all(&host);
            m.gp.cyber.tips.clear();
            m.gp.cyber.shown = Some(key.clone());
            hide_tip(m);
            let item = key.as_ref().and_then(|k0| m.gp.cyber.snapshot.as_ref().and_then(|s| s.by_key.get(k0).cloned()));
            if let Some(c) = item {
                let mut x = if m.gp.cyber.area.is_some() { 980.0 } else { 1020.0 };
                // the overview's cards alternate left / right: the tooltip opposite its source, like the native
                // ripperdoc (its blocker on a right-hand card caused hover-out, hide, hover-in, repeat)
                if m.gp.cyber.area.is_none() {
                    let groups: Vec<&'static str> = m.gp.cyber.snapshot.as_ref().map(|s| s.groups.iter().map(|g| g.area).collect()).unwrap_or_default();
                    if let Some(i) = groups.iter().position(|g| Some(*g) == harea || idt::area(g) == c.item.area) {
                        if (i + 1) % 2 == 0 {
                            x = 180.0;
                        }
                    }
                }
                tooltip(m, &host, &c, x + 10.0, 180.0, 630.0, 890.0, false, None);
                if let Some(b) = &m.gp.cyber.tip_block {
                    b.x.set(P1X + x);
                    b.visible.set(true);
                }
            } else if let Some(kind) = key.as_deref().and_then(|k0| k0.strip_prefix("bar:")) {
                let pane = ink::canvas(&host, 520.0, 230.0, 660.0, 830.0);
                bar_tip(&pane, kind);
                if let Some(b) = &m.gp.cyber.tip_block_bar {
                    b.visible.set(true);
                }
            }
            let area = m.gp.cyber.area.or(harea);
            let zoom = m.gp.cyber.area.is_some();
            if let Some(b) = m.gp.cyber.body.as_mut() {
                body_focus(b, area, zoom);
            }
        }
    }
    if now - m.gp.cyber.poll > 1.0 {
        m.gp.cyber.poll = now;
        let vendor = super::menu_vendor::ripper_session();
        if signature() != m.gp.cyber.stamp || m.gp.cyber.snapshot.as_ref().is_some_and(|s| s.vendor != vendor) {
            if let Some(c) = m.gp.cyber.ctx.as_mut() {
                idt::invalidate(c);
            }
            gp::redraw(m);
        }
    }
}
pub(super) fn before_build(m: &mut Menu) {
    let s = &mut m.gp.cyber;
    s.body = None;
    s.tip_host = Handle::null();
    s.tips.clear();
    s.pulses.clear();
}
pub(super) fn leave(m: &mut Menu) {
    if let Some(c) = m.gp.cyber.ctx.take() {
        idt::close(c);
    }
    m.gp.cyber = CyberState::default();
}
