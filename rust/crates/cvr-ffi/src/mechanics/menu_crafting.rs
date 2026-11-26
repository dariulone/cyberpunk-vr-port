//! crafting_data.lua and crafting_view.lua (the C++ build's CyberpunkVRPort_Menu): the game's CRAFTING and UPGRADING
//! screens as two tabs. Recipes or upgradable iconic weapons on the left with the game's filters; on the right the
//! game's tooltip of what the pick makes (for an upgrade, the weapon now and one upgrade further), its components --
//! had / needed -- and the button. The game's own way (UI/fullscreen/crafting: craftingPanelController.swift,
//! upgradePanelController.swift, craftingItemTemplateClassifier.swift): the craft book's recipes, the iconic weapons
//! that can still be upgraded, what each costs, and the requests that do it.
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_core::*;
use super::menu_data::Icon;
use super::menu_gameplay as gp;
use super::menu_gpdata as gd;
use super::menu_ink as k;
use super::menu_invdata::{self as idt, Item};
use super::menu_invview::{self as iv, Tip};
use super::menu_locale as loc;
use super::menu_rt::*;
use super::menu_trade as td;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::rc::Rc;

const W: f64 = 1780.0;
const H: f64 = 1305.0;
const LIST_X: f64 = 24.0;
const LIST_W: f64 = 876.0;
const LIST_Y: f64 = 262.0;
const LIST_H: f64 = 820.0;
const CARD_W: f64 = 426.0;
const CARD_H: f64 = 170.0;
const ROW_H: f64 = 182.0;
const DX: f64 = 920.0;
const DW: f64 = 836.0;
// the bottom of the detail column, centred as the game's screens have it: the components, the quantity (crafting),
// the button, and under it why it is blocked
const ING_Y: f64 = 850.0;
const QTY_Y: f64 = 1044.0;
const BTN_Y: f64 = 1114.0;
const WHY_Y: f64 = 1186.0;
const GOOD: [f64; 3] = [0.25, 0.95, 0.55];
const CRAFT_FILTERS: [&str; 7] = ["AllItems", "RangedWeapons", "MeleeWeapons", "Consumables", "Grenades", "Attachments", "Programs"];
const UPGRADE_FILTERS: [&str; 3] = ["AllItems", "RangedWeapons", "MeleeWeapons"];

fn tr(ru: &str, en: &str) -> String {
    loc::tr(ru, en)
}
/// The game's text for a key, else the menu's own words.
fn loc_or(key: &str, ru: &str, en: &str) -> String {
    let t = super::menu_data::loc(key);
    if !t.is_empty() && t != key {
        t
    } else {
        tr(ru, en)
    }
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
/// "LegendaryPlus" draws as "Legendary".
fn base_quality(name: &str) -> String {
    name.replace("Plus", "")
}

// ---------------------------------------------------------------- crafting_data.lua
fn system() -> Handle {
    let c = super::game_system("GetScriptableSystemsContainer");
    o(&c, "Get", &[A::Name(cname("CraftingSystem"))])
}
fn tag(rec: &Handle, name: &str) -> bool {
    b(rec, "TagsContains", &[A::Name(cname(name))])
}
fn image(tdbid: u64, rec: &Handle) -> Option<Icon> {
    // the record's id as the name gives it ("UIIcon.<name>"), not prefixed again (crafting_data.lua's image())
    let icon = rt::static_call("UIInventoryItemsManager", "ResolveItemIconName", &[A::Name(tdbid), A::H(rec), A::B(false)]).map(|o| Val::of(&o).text()).unwrap_or_default();
    if icon.is_empty() || icon == "None" {
        return None;
    }
    gd::image(tdb(&icon))
}
#[derive(Clone)]
struct Ingredient {
    name: String,
    image: Option<Icon>,
    need: f64,
    have: f64,
}
/// D.ingredients: an IngredientData array's names, pictures, needed and had.
fn ingredients(list: Option<Val>) -> Vec<Ingredient> {
    let Some(list) = list else { return Vec::new() };
    let mut out = Vec::new();
    for g in items(&list) {
        let rec = fh(&g, "id");
        if rec.is_null() {
            continue;
        }
        let dn = id(&rec, "DisplayName", &[]);
        let name = rt::global_call("GetLocalizedItemNameByCName", &[A::Name(dn)]).map(|o| Val::of(&o).text()).unwrap_or_default();
        out.push(Ingredient { name, image: image(id(&rec, "GetID", &[]), &rec), need: fnum(&g, "quantity"), have: fnum(&g, "inventoryQuantity") });
    }
    out
}
/// A recipe of the craft book, or an upgradable iconic weapon (its item and game data).
#[derive(Clone)]
struct Entry {
    rec: Handle,
    tdbid: u64,
    key: String,
    name: String,
    amount: f64,
    quality: String,
    craftable: bool,
    new: bool,
    image: Option<Icon>,
    weapon: bool,
    ammo: bool,
    type_icon: Option<Icon>,
    item: Option<Item>,
    game: Handle,
}
/// D.matches: craftingItemTemplateClassifier.swift FilterItem.
fn matches(v: &Entry, filter: &str) -> bool {
    let r = &v.rec;
    match filter {
        "AllItems" => true,
        "RangedWeapons" => tag(r, "RangedWeapon"),
        "MeleeWeapons" => tag(r, "MeleeWeapon"),
        "Consumables" => tag(r, "Consumable") || tag(r, "Ammo"),
        "Grenades" => tag(r, "Grenade"),
        "Attachments" => tag(r, "itemPart") && !tag(r, "Fragment") && !tag(r, "SoftwareShard"),
        "Programs" => tag(r, "SoftwareShard") || tag(r, "QuickhackCraftingPart"),
        _ => true,
    }
}
fn order(list: &mut [Entry]) {
    list.sort_by(|a, b| rank(&b.quality).cmp(&rank(&a.quality)).then_with(|| a.name.cmp(&b.name)).then_with(|| a.key.cmp(&b.key)));
}
fn quality_of(rec: &Handle) -> String {
    let q = o(rec, "Quality", &[]);
    let t = n(&q, "Type", &[]);
    base_quality(&ss("UIItemsHelper", "QualityEnumToName", &[A::N(t)]))
}
/// D.recipes: every recipe the craft book offers (craftingPanelController.swift GetRecipesList).
fn recipes() -> Vec<Entry> {
    let cs = system();
    let book = o(&cs, "GetPlayerCraftBook", &[]);
    if book.is_null() {
        return Vec::new();
    }
    rt::call(&book, "HideRecipesForOwnedItems", &[]);
    let mut out = Vec::new();
    for rec in os(&book, "GetCraftableItems", &[]) {
        if rec.is_null() {
            continue;
        }
        let rd = rt::call_typed(&cs, "GetRecipeData", &["handle:gamedataItem_Record"], &[A::H(&rec)]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
        if rd.is_null() {
            continue;
        }
        let label = ptext(&rd, "label");
        if label.is_empty() {
            continue;
        }
        let tdbid = id(&rec, "GetID", &[]);
        let craftable = rt::call_typed(&cs, "CanItemBeCrafted", &["whandle:gamedataItem_Record"], &[A::H(&rec)]).is_some_and(|o| Val::of(&o).bool());
        out.push(Entry {
            key: format!("{tdbid:016X}"),
            name: label,
            amount: pnum(&rd, "amount"),
            quality: quality_of(&rec),
            craftable,
            new: b(&book, "IsRecipeNew", &[A::Name(tdbid)]),
            image: image(tdbid, &rec),
            weapon: tag(&rec, "Weapon") || tag(&rec, "RangedWeapon") || tag(&rec, "MeleeWeapon"),
            ammo: tag(&rec, "Ammo"),
            type_icon: None,
            item: None,
            game: Handle::null(),
            rec,
            tdbid,
        });
    }
    order(&mut out);
    out
}
/// The item a recipe makes, made for show only (craftingPanelController.swift DryMakeItem): what its tooltip describes
/// and what the crafting system prices.
struct Dry {
    key: String,
    data: Handle,
    ui: Handle,
}
fn dry(v: &Entry) -> Option<Dry> {
    let p = player();
    let params = new_struct("ItemModParams")?;
    let pv = val(&params);
    let item_id = rt::static_call("ItemID", "FromTDBID", &[A::Name(v.tdbid)])?;
    if let Some(f) = pv.f("itemID") {
        let n = (f.ty.size() as usize).min(item_id.ty().size() as usize);
        // SAFETY: the ItemID's bytes into the struct's ItemID field, at most its size.
        unsafe { core::ptr::copy_nonoverlapping(item_id.as_ptr(), f.ptr.cast::<u8>(), n) };
    }
    set(&pv, "quantity", 1.0);
    let data = so("Inventory", "CreateItemData", &[A::P(ptr(&params)), A::H(&p)]);
    if data.is_null() {
        return None;
    }
    rt::static_call("CraftingSystem", "SetItemLevel", &[A::H(&p), A::H(&data)]);
    rt::static_call("CraftingSystem", "MarkItemAsCrafted", &[A::H(&p), A::H(&data)]);
    rt::call(&data, "SetDynamicTag", &[A::Name(cname("SkipActivityLog"))]);
    let manager = o(&idt::ui_inventory(), "GetInventoryItemsManager", &[]);
    let ui = so("UIInventoryItem", "Make", &[A::H(&p), A::H(&data), A::H(&manager)]);
    Some(Dry { key: v.key.clone(), data, ui })
}
struct Quote {
    can: bool,
    max: f64,
    ingredients: Vec<Ingredient>,
    reason: Option<String>,
}
/// D.craftQuote: whether it can be crafted now, how many at most, what it costs, and the game's reason when it cannot
/// (craftingPanelController.swift SetNotification).
fn craft_quote(d: &Dry) -> Quote {
    let cs = system();
    let data = &d.data;
    let mut q = Quote {
        can: rt::call_typed(&cs, "CanItemBeCrafted", &["whandle:gameItemData"], &[A::H(data)]).is_some_and(|o| Val::of(&o).bool()),
        max: n(&cs, "GetMaxCraftingAmount", &[A::H(data)]).max(0.0),
        ingredients: Vec::new(),
        reason: None,
    };
    // the cost array kept while it is read
    let cost = rt::call_typed(&cs, "GetItemCraftingCost", &["whandle:gameItemData"], &[A::H(data)]);
    q.ingredients = ingredients(cost.as_ref().map(Val::of));
    if n(&d.ui, "GetItemType", &[]) == ev("gamedataItemType", "Prt_Program") {
        q.reason = Some("LocKey#78498".into());
    } else if !b(&cs, "EnoughIngredientsForCrafting", &[A::H(data)]) {
        q.reason = Some("LocKey#42797".into());
    } else if q.max == 0.0 {
        q.reason = Some("LocKey#81496".into());
    }
    if q.reason.is_some() {
        q.can = false;
    }
    q
}
/// D.craft: CraftItemRequest queued (the ammo's bullet amount), the recipe marked inspected.
fn craft(v: &Entry, amount: f64) {
    let cs = system();
    let request = ink::new("CraftItemRequest");
    if request.is_null() {
        return;
    }
    idt::hset_handle(&request, "target", &player());
    idt::hset_handle(&request, "itemRecord", &v.rec);
    hset(&request, "amount", amount.floor().max(1.0));
    if v.ammo {
        hset(&request, "bulletAmount", v.amount);
    }
    rt::call(&cs, "QueueRequest", &[A::H(&request)]);
    inspect(v);
}
/// D.inspect: the recipe seen.
fn inspect(v: &Entry) {
    let book = o(&system(), "GetPlayerCraftBook", &[]);
    rt::call(&book, "SetRecipeInspected", &[A::Name(v.tdbid)]);
}
/// D.upgradable: the iconic weapons that can still be upgraded (upgradePanelController.swift GetUpgradableList).
fn upgradable(ctx: &mut idt::Ctx) -> Vec<Entry> {
    let cs = system();
    rt::call(&ctx.manager, "MarkToRebuild", &[]);
    let mut out = Vec::new();
    let Some(kinds) = rt::static_call("InventoryDataManagerV2", "GetInventoryWeaponTypes", &[]) else { return out };
    for kind in items(&Val::of(&kinds)) {
        let Some(kind) = kind.num() else { continue };
        let Some(arr) = rt::call(&ctx.manager, "GetPlayerIconicWeaponsByType", &[A::N(kind)]) else { continue };
        for d in items(&Val::of(&arr)) {
            let Some(data) = idt::own(&d) else { continue };
            let game = so("InventoryItemData", "GetGameItemData", &[A::P(ptr(&data))]);
            if game.is_null() || sb("RPGManager", "IsItemMaxTier", &[A::H(&game)]) {
                continue;
            }
            let Some(it) = idt::item(ctx, data) else { continue };
            let rec = so("TweakDBInterface", "GetItemRecord", &[A::Name(it.id.tdbid())]);
            out.push(Entry {
                rec,
                tdbid: it.id.tdbid(),
                key: it.key.clone(),
                name: it.name.clone(),
                amount: 1.0,
                quality: base_quality(&it.quality),
                craftable: b(&cs, "CanItemBeUpgraded", &[A::H(&game)]),
                new: false,
                image: it.image.clone(),
                weapon: true,
                ammo: false,
                type_icon: it.type_icon.clone(),
                item: Some(it),
                game,
            });
        }
    }
    order(&mut out);
    out
}
/// D.upgradeQuote: whether it can be upgraded now and what it costs ("blocked": the menu's own words, the game has no
/// key it can resolve for it).
fn upgrade_quote(v: &Entry) -> Quote {
    let cs = system();
    let Some(it) = &v.item else { return Quote { can: false, max: 0.0, ingredients: Vec::new(), reason: Some("blocked".into()) } };
    let recipe = o(&cs, "GetUpgradeRecipeData", &[it.id.a()]);
    let can = b(&cs, "CanItemBeUpgraded", &[A::H(&v.game)]);
    let reason = (!can).then(|| if b(&cs, "EnoughIngredientsForUpgrading", &[A::H(&v.game)]) { "blocked".to_string() } else { "LocKey#42797".to_string() });
    Quote { can, max: 1.0, ingredients: ingredients(pf(&recipe, "ingredients")), reason }
}
/// D.upgrade: UpgradeItemRequest queued.
fn upgrade(v: &Entry) -> bool {
    let cs = system();
    let Some(it) = &v.item else { return false };
    if !b(&cs, "CanItemBeUpgraded", &[A::H(&v.game)]) {
        return false;
    }
    let request = ink::new("UpgradeItemRequest");
    if request.is_null() {
        return false;
    }
    idt::hset_handle(&request, "owner", &player());
    idt::hset_id(&request, "itemID", &it.id);
    rt::call(&cs, "QueueRequest", &[A::H(&request)]);
    true
}
/// D.withUpgrade: `f` run with the item one upgrade further, as the upgrade screen previews it (ApplyQualityModifier):
/// the WasItemUpgraded stat raised by one for the call, then put back.
fn with_upgrade<R>(v: &Entry, f: impl FnOnce() -> Option<R>) -> Option<R> {
    let stats = super::game_system("GetStatsSystem");
    let stat_id = rt::call(&v.game, "GetStatsObjectID", &[])?;
    let t = ev("gamedataStatType", "WasItemUpgraded");
    let old = n(&v.game, "GetStatValueByType", &[A::N(t)]);
    let set_to = |value: f64| {
        rt::call(&stats, "RemoveAllModifiers", &[A::P(ptr(&stat_id)), A::N(t), A::B(true)]);
        let modifier = so("RPGManager", "CreateStatModifier", &[A::N(t), A::N(ev("gameStatModifierType", "Additive")), A::N(value)]);
        rt::call(&stats, "AddSavedModifier", &[A::P(ptr(&stat_id)), A::H(&modifier)]);
    };
    set_to(old + 1.0);
    let result = f();
    set_to(old);
    result
}
/// D.signature: how many of each crafting component the player has -- what changes when anything is crafted or
/// upgraded.
fn signature() -> String {
    let p = player();
    let ts = super::game_system("GetTransactionSystem");
    let mut out = Vec::new();
    for set_name in ["GetCommonCraftingMaterials", "GetHackingCraftingMaterials"] {
        let Some(arr) = rt::static_call("UIInventoryHelper", set_name, &[]) else { continue };
        for t in items(&Val::of(&arr)) {
            // SAFETY: a TweakDBID element.
            let tdbid = unsafe { *(t.ptr as *const u64) };
            let Some(query) = rt::static_call("ItemID", "CreateQuery", &[A::Name(tdbid)]) else { continue };
            out.push(gd::fmt_g(n(&ts, "GetItemQuantity", &[A::H(&p), A::P(ptr(&query))])));
        }
    }
    out.join(",")
}

// ---------------------------------------------------------------- crafting_view.lua
#[derive(Default)]
struct State {
    ctx: Option<idt::Ctx>,
    tab: String,
    filters: [String; 2],
    selected: [Option<String>; 2],
    quantity: f64,
    recipes: Option<Rc<Vec<Entry>>>,
    upgrades: Option<Rc<Vec<Entry>>>,
    sig: Option<String>,
    dry: Option<Rc<Dry>>,
    tips: Vec<Tip>,
    poll: f64,
    refresh_at: Option<f64>,
    offset: Option<f64>,
    reset_list: bool,
}
thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let st = s.get_or_insert_with(|| State {
            tab: "craft".into(),
            filters: ["AllItems".into(), "AllItems".into()],
            quantity: 1.0,
            ctx: Some(idt::open()),
            ..Default::default()
        });
        f(st)
    })
}
fn tab_index(tab: &str) -> usize {
    usize::from(tab != "craft")
}
fn card(m: &mut Menu, c: &Handle, it: &RowCtx, v: &Entry, x: f64, tab: &str, selected: bool) {
    let col = td::col_of(&v.quality);
    let bg = k::round_fill(c, x, 4.0, CARD_W, CARD_H, col, if v.craftable { 0.06 } else { 0.02 }, Some(4.0));
    let edge = k::cut_frame(&mut m.pool, c, x, 4.0, CARD_W, CARD_H, if selected { k::YELLOW } else { k::CYAN }, if selected { 0.85 } else { 0.35 }, Some(10.0), Some(1.0));
    k::rect(c, x + 2.0, 8.0, 5.0, CARD_H - 8.0, col, if v.craftable { 1.0 } else { 0.4 });
    gp::picture(m, c, x + 14.0, 22.0, CARD_W - 28.0, 100.0, v.image.as_ref(), None);
    k::text(c, x + 14.0, 126.0, CARD_W - 28.0, 32.0, &k::clip(&v.name, 32), 24.0, if v.craftable { col } else { k::GREY }, "left", false);
    if v.new {
        k::image(c, x + CARD_W - 20.0, 13.0, 9.0, 9.0, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
    }
    if let Some(ti) = &v.type_icon {
        k::image_raw(c, x + CARD_W - 44.0, 128.0, 26.0, 26.0, ti.atlas, ti.part, Some(k::YELLOW), 1.0);
    }
    let (key, tab_s, entry) = (v.key.clone(), tab.to_string(), v.clone());
    let craftable = v.craftable;
    let b = button(
        m,
        "p1",
        &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
        x,
        4.0,
        CARD_W,
        CARD_H,
        Some(Rc::new(move |m: &mut Menu, _| {
            with(|s| {
                s.selected[tab_index(&tab_s)] = Some(key.clone());
                s.quantity = 1.0;
            });
            if tab_s == "craft" {
                inspect(&entry);
            }
            gp::redraw(m);
        })),
        Some(Rc::new(move |on, _| {
            ink::tint(&bg, if on { k::YELLOW } else { col });
            ink::opacity(&bg, if on { 0.14 } else if craftable { 0.06 } else { 0.02 });
            k::tint(&edge, if on || selected { k::YELLOW } else { k::CYAN }, Some(if on { 1.0 } else if selected { 0.85 } else { 0.35 }));
        })),
    );
    *b.key.borrow_mut() = Some(format!("crafting:{tab}:{}", v.key));
}
fn tabs(m: &mut Menu, p: &Handle, tab: &str) {
    for (i, (id, key, ru, en)) in [("craft", "UI-PanelNames-CRAFTING", "Создание", "Crafting"), ("upgrade", "UI-PanelNames-UPGRADING", "Улучшение", "Upgrading")].into_iter().enumerate() {
        let x = LIST_X + i as f64 * (LIST_W / 2.0 + 6.0);
        let w = LIST_W / 2.0 - 6.0;
        let on = tab == id;
        let bg = k::round_fill(p, x, 100.0, w, 60.0, if on { k::YELLOW } else { k::NAVY }, if on { 0.14 } else { 0.18 }, Some(4.0));
        let label = k::text(p, x, 100.0, w, 60.0, &k::upper(&loc_or(key, ru, en)), 29.0, if on { k::YELLOW } else { k::TEXT }, "center", false);
        if on {
            k::rect(p, x + 14.0, 155.0, w - 28.0, 2.0, k::YELLOW, 1.0);
        }
        let b = button(
            m,
            "p1",
            &at(P1X, P1Y),
            x,
            100.0,
            w,
            60.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                let changed = with(|s| {
                    if s.tab == id {
                        return false;
                    }
                    s.tab = id.to_string();
                    s.quantity = 1.0;
                    s.reset_list = true;
                    true
                });
                if changed {
                    gp::redraw(m);
                }
            })),
            Some(Rc::new(move |h, _| {
                ink::tint(&bg, if h || on { k::YELLOW } else { k::NAVY });
                ink::tint(&label, if h || on { k::YELLOW } else { k::TEXT });
            })),
        );
        *b.key.borrow_mut() = Some(format!("craftingTab:{id}"));
    }
}
fn filters(m: &mut Menu, p: &Handle, tab: &str, current: &str) {
    let names: &[&str] = if tab == "craft" { &CRAFT_FILTERS } else { &UPGRADE_FILTERS };
    let count = names.len() as f64;
    let caption = k::text(p, LIST_X + count * 73.0 + 8.0, 178.0, LIST_W - count * 73.0 - 8.0, 56.0, "", 23.0, k::GREY, "left", false);
    for (i, &name) in names.iter().enumerate() {
        let x = LIST_X + i as f64 * 73.0;
        let on = current == name;
        let label = idt::filter_label(name);
        if on {
            ink::set_text(&caption, &label);
        }
        let bg = k::round_fill(p, x, 178.0, 66.0, 56.0, if on { k::YELLOW } else { k::NAVY }, if on { 0.12 } else { 0.18 }, Some(3.0));
        if let Some(icon) = gd::image(tdb(&idt::filter_icon(name))) {
            k::image_raw(p, x + 9.0, 183.0, 48.0, 46.0, icon.atlas, icon.part, Some(if on { k::YELLOW } else { k::CYAN }), 1.0);
        }
        let (cap, tab_s) = (caption.clone(), tab.to_string());
        let b = button(
            m,
            "p1",
            &at(P1X, P1Y),
            x,
            178.0,
            66.0,
            56.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                with(|s| {
                    s.filters[tab_index(&tab_s)] = name.to_string();
                    s.reset_list = true;
                });
                gp::redraw(m);
            })),
            Some(Rc::new(move |h, _| {
                ink::tint(&bg, if h || on { k::YELLOW } else { k::NAVY });
                ink::opacity(&bg, if h { 0.18 } else if on { 0.12 } else { 0.18 });
                if h {
                    ink::set_text(&cap, &label);
                }
            })),
        );
        *b.key.borrow_mut() = Some(format!("craftingFilter:{name}"));
    }
}
fn ingredient_row(p: &Handle, list: &[Ingredient], y: f64) {
    k::text(p, DX, y, DW, 40.0, &k::upper(&loc_or("UI-ResourceExports-CraftingMaterials", "Компоненты", "Components")), 24.0, k::GREY, "center", false);
    let x0 = DX + (DW - (list.len() as f64 * 168.0 - 12.0)) / 2.0;
    for (i, g) in list.iter().enumerate() {
        let x = x0 + i as f64 * 168.0;
        let enough = g.have >= g.need;
        k::round_fill(p, x, y + 44.0, 156.0, 136.0, k::NAVY, 0.18, Some(4.0));
        k::rect(p, x, y + 44.0, 156.0, 2.0, if enough { k::CYAN } else { k::RED }, 0.45);
        if let Some(img) = &g.image {
            k::image_raw(p, x + 23.0, y + 52.0, 110.0, 72.0, img.atlas, img.part, Some(k::WHITE), 1.0);
        }
        let have = if g.have > 9999.0 { "9999+".to_string() } else { gd::fmt_g(g.have) };
        k::text(p, x + 4.0, y + 130.0, 148.0, 40.0, &format!("{have} / {}", gd::fmt_g(g.need)), 26.0, if enough { GOOD } else { k::RED }, "center", false);
        let _ = &g.name;
    }
}
fn craft_detail(m: &mut Menu, p: &Handle, v: &Entry) {
    let current = with(|s| s.dry.clone().filter(|d| d.key == v.key));
    let d = match current {
        Some(d) => d,
        None => {
            let Some(d) = dry(v).map(Rc::new) else {
                k::text(p, DX, 500.0, DW, 120.0, &tr("Не удалось подготовить предмет", "The item could not be prepared"), 30.0, k::RED, "center", false);
                return;
            };
            with(|s| s.dry = Some(d.clone()));
            d
        }
    };
    let q = craft_quote(&d);
    let qty = with(|s| {
        s.quantity = s.quantity.min(q.max.max(1.0)).max(1.0);
        s.quantity
    });
    k::text(p, DX, 100.0, DW, 56.0, &k::clip(&v.name, 40), 36.0, td::col_of(&v.quality), "left", false);
    let host = ink::canvas(p, 0.0, 0.0, W, 1200.0);
    if let Some(t) = iv::tip_spawn(m, &host, &d.ui, "Backpack", DX + 12.0, 172.0, DW - 24.0, 680.0, None, None) {
        with(|s| s.tips.push(t));
    }
    ingredient_row(p, &q.ingredients, ING_Y);
    if q.max > 1.0 && !v.weapon {
        let x = DX + (DW - 430.0) / 2.0;
        let max = q.max;
        td::action(m, p, x, QTY_Y, 80.0, "−", "−", Rc::new(|m: &mut Menu, _| {
            with(|s| s.quantity = (s.quantity - 1.0).max(1.0));
            gp::redraw(m);
        }), qty > 1.0, "craftQuantityMinus");
        k::text(p, x + 84.0, QTY_Y, 110.0, 60.0, &gd::fmt_g(qty), 32.0, k::CYAN, "center", false);
        td::action(m, p, x + 198.0, QTY_Y, 80.0, "+", "+", Rc::new(move |m: &mut Menu, _| {
            with(|s| s.quantity = (s.quantity + 1.0).min(max));
            gp::redraw(m);
        }), qty < max, "craftQuantityPlus");
        td::action(m, p, x + 290.0, QTY_Y, 140.0, "МАКС.", "MAX", Rc::new(move |m: &mut Menu, _| {
            with(|s| s.quantity = max);
            gp::redraw(m);
        }), true, "craftQuantityMax");
    }
    if let Some(r) = &q.reason {
        k::text(p, DX, WHY_Y, DW, 40.0, &super::menu_data::loc(r), 26.0, k::RED, "center", false);
    }
    let entry = v.clone();
    let label = k::upper(&loc_or("UI-Crafting-CraftItem", "Изготовить", "Craft"));
    let b = gp::hub_action(m, p, P1X, DX + (DW - 396.0) / 2.0, BTN_Y, 396.0, &label, Rc::new(move |m: &mut Menu, _| {
        let amount = with(|s| s.quantity);
        craft(&entry, amount);
        let msg = format!("{}{}{}", tr("Изготовлено: ", "Crafted: "), entry.name, if amount > 1.0 { format!(" x{}", gd::fmt_g(amount)) } else { String::new() });
        toast(m, &msg);
        let at = m.time + 0.4;
        with(|s| {
            s.quantity = 1.0;
            s.refresh_at = Some(at);
        });
        gp::redraw(m);
    }), q.can, None, None);
    *b.key.borrow_mut() = Some("craftApply".into());
}
fn upgrade_detail(m: &mut Menu, p: &Handle, v: &Entry) {
    let q = upgrade_quote(v);
    k::text(p, DX, 100.0, DW, 56.0, &k::clip(&v.name, 40), 36.0, td::col_of(&v.quality), "left", false);
    let host = ink::canvas(p, 0.0, 0.0, W, 1200.0);
    let half = (DW - 8.0) / 2.0;
    // the weapon now, and one upgrade further: tooltip data snapshots, the second one made while the item carries the
    // extra upgrade (upgradePanelController.swift UpdateTooltipRightAndShow), each from item data made at that moment
    // (an InventoryItemData keeps the stats it was made with)
    let manager = with(|s| s.ctx.as_ref().map(|c| c.manager.clone())).unwrap_or_else(Handle::null);
    let snapshot = |equipped: bool| -> Option<Handle> {
        let data = rt::call(&manager, "GetInventoryItemData", &[A::H(&v.game)])?;
        let d = rt::call(&manager, "GetMinimalTooltipDataForInventoryItem", &[A::P(ptr(&data)), A::B(false), A::H(&Handle::null()), A::B(false)])
            .map(|o| Val::of(&o).handle())
            .filter(|h| !h.is_null())?;
        hset(&d, "displayContext", ev("InventoryTooltipDisplayContext", "Upgrading"));
        if !equipped {
            hset(&d, "isEquipped", 0.0);
        }
        // a stats manager reads the item's stats when first asked: each snapshot's are read at its own level
        let stats = o(&d, "GetStatsManager", &[]);
        rt::call(&stats, "SizeTooltipStats", &[]);
        rt::call(&stats, "GetWeaponBars", &[]);
        Some(d)
    };
    if let Some(now) = snapshot(true) {
        if let Some(t) = iv::tip_spawn_data(m, &host, &now, DX + 8.0, 172.0, half - 12.0, 680.0) {
            with(|s| s.tips.push(t));
        }
        if let Some(next) = with_upgrade(v, || snapshot(false)) {
            if let Some(t) = iv::tip_spawn_data(m, &host, &next, DX + half + 12.0, 172.0, half - 12.0, 680.0) {
                with(|s| s.tips.push(t));
            }
        }
    }
    ingredient_row(p, &q.ingredients, ING_Y);
    if let Some(r) = &q.reason {
        let text = if r == "blocked" { tr("Сейчас недоступно", "Not available right now") } else { super::menu_data::loc(r) };
        k::text(p, DX, WHY_Y, DW, 40.0, &text, 26.0, k::RED, "center", false);
    }
    let entry = v.clone();
    let label = k::upper(&loc_or("UI-Crafting-Upgrade", "Улучшить", "Upgrade"));
    let b = gp::hub_action(m, p, P1X, DX + (DW - 396.0) / 2.0, BTN_Y, 396.0, &label, Rc::new(move |m: &mut Menu, _| {
        let msg = if upgrade(&entry) { format!("{}{}", tr("Улучшено: ", "Upgraded: "), entry.name) } else { m.tx("blocked") };
        toast(m, &msg);
        let at = m.time + 0.4;
        with(|s| s.refresh_at = Some(at));
        gp::redraw(m);
    }), q.can, None, None);
    *b.key.borrow_mut() = Some("upgradeApply".into());
}
/// V.draw.
pub(super) fn draw(m: &mut Menu, p: &Handle, _p2: &Handle) {
    with(|s| s.tips.clear());
    glass(m, p, W, H, Some((P1X, P1Y)));
    let tab = with(|s| s.tab.clone());
    let list: Rc<Vec<Entry>> = if tab == "craft" {
        match with(|s| s.recipes.clone()) {
            Some(l) => l,
            None => {
                let l = Rc::new(recipes());
                with(|s| s.recipes = Some(l.clone()));
                l
            }
        }
    } else {
        match with(|s| s.upgrades.clone()) {
            Some(l) => l,
            None => {
                let mut ctx = with(|s| s.ctx.take()).unwrap_or_else(idt::open);
                let l = Rc::new(upgradable(&mut ctx));
                with(|s| {
                    s.ctx = Some(ctx);
                    s.upgrades = Some(l.clone());
                });
                l
            }
        }
    };
    if with(|s| s.sig.is_none()) {
        let sig = signature();
        with(|s| s.sig = Some(sig));
    }
    let (rus, ens) = if tab == "craft" { ("Создание", "Crafting") } else { ("Улучшение", "Upgrading") };
    gp::title(p, &k::upper(&loc_or(if tab == "craft" { "UI-PanelNames-CRAFTING" } else { "UI-PanelNames-UPGRADING" }, rus, ens)), W);
    let ti = tab_index(&tab);
    let filter = with(|s| s.filters[ti].clone());
    tabs(m, p, &tab);
    filters(m, p, &tab, &filter);
    k::rect(p, DX - 10.0, 100.0, 1.0, 1080.0, k::CYAN, 0.22);
    let shown: Vec<Entry> = list.iter().filter(|v| matches(v, &filter)).cloned().collect();
    let selected_key = with(|s| s.selected[ti].clone());
    let mut rows = Vec::new();
    for pair in shown.chunks(2) {
        let pair = pair.to_vec();
        let (tab_s, sel) = (tab.clone(), selected_key.clone());
        rows.push(Row {
            h: ROW_H,
            build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                for (i, v) in pair.iter().enumerate() {
                    card(m, c, &it, v, i as f64 * (CARD_W + 12.0), &tab_s, sel.as_deref() == Some(v.key.as_str()));
                }
            }),
        });
    }
    if rows.is_empty() {
        rows.push(Row {
            h: 130.0,
            build: Rc::new(|_m: &mut Menu, c: &Handle, _| {
                k::text(c, 10.0, 20.0, LIST_W - 20.0, 90.0, &tr("Здесь пока ничего нет", "Nothing here yet"), 29.0, k::GREY, "center", false);
            }),
        });
    }
    let host = ink::canvas(p, LIST_X, 0.0, LIST_W, 1200.0);
    let keep = with(|s| if std::mem::take(&mut s.reset_list) { Some(0.0) } else { s.offset });
    let view = gp::glist(m, &host, "p1", "p1", P1X + LIST_X, LIST_Y, LIST_W, rows, LIST_H, keep);
    view.borrow_mut().enable_mask();
    let selected = shown.iter().find(|v| selected_key.as_deref() == Some(v.key.as_str())).cloned();
    let Some(selected) = selected else {
        with(|s| s.selected[ti] = None);
        let text = if tab == "craft" { tr("Выберите чертёж", "Select a recipe") } else { tr("Выберите оружие для улучшения", "Select a weapon to upgrade") };
        k::text(p, DX, 500.0, DW, 120.0, &text, 32.0, k::GREY, "center", false);
        return;
    };
    if tab == "craft" {
        craft_detail(m, p, &selected);
    } else {
        upgrade_detail(m, p, &selected);
    }
}
/// V.update: the tooltips laid out; after a craft or an upgrade, and whenever the components change, the lists read
/// again.
pub(super) fn update(m: &mut Menu) {
    let started = STATE.with(|s| s.borrow().is_some());
    if !started {
        return;
    }
    with(|s| iv::tip_fit(&mut s.tips));
    let (poll, refresh_at) = with(|s| (s.poll, s.refresh_at));
    let due = refresh_at.is_some_and(|t| m.time >= t);
    if m.time - poll < 0.5 && !due {
        return;
    }
    with(|s| s.poll = m.time);
    let sig = signature();
    let changed = with(|s| s.sig.as_deref() != Some(sig.as_str()));
    if due || changed {
        with(|s| {
            s.refresh_at = None;
            s.sig = Some(sig);
            s.recipes = None;
            s.upgrades = None;
            s.dry = None;
            if let Some(c) = s.ctx.as_mut() {
                idt::invalidate(c);
            }
        });
        gp::redraw(m);
    }
}
/// The list's scroll kept through a redraw.
pub(super) fn before_build(m: &mut Menu) {
    if STATE.with(|s| s.borrow().is_none()) {
        return;
    }
    let offset = m.view("p1").map(|v| v.v.borrow().offset);
    with(|s| {
        s.offset = offset;
        s.tips.clear();
    });
}
/// V.leave.
pub(super) fn leave(_m: &mut Menu) {
    if let Some(s) = STATE.with(|s| s.borrow_mut().take()) {
        if let Some(c) = s.ctx {
            idt::close(c);
        }
    }
}
