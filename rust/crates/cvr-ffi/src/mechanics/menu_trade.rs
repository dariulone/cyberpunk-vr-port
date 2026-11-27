//! trade_data.lua and trade_view.lua: a shop visit's trade -- the player's sellable items and the vendor's stock (its
//! buyback apart), the game's own prices, quantity limits and protected items, the categories, each side's sort order,
//! the game's tooltips with the preferred comparison, SELL / BUY / BUY BACK with a confirmation for an iconic or
//! equipped item, several of your items sold at once from a selection; quest items first with the native tile's marker;
//! the panel read again when the money, the weight or the vendor's stock moves.
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_core::*;
use super::menu_gameplay as gp;
use super::menu_gpdata as gd;
use super::menu_ink as k;
use super::menu_invdata::{self as idt, Item};
use super::menu_invview::{self as iv, SortPopup, Tip};
use super::menu_locale as loc;
use super::menu_rt::*;
use red4ext_abi::handle::Handle;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const W: f64 = 1780.0;
const H: f64 = 1305.0;
fn tr(ru: &str, en: &str) -> String {
    loc::tr(ru, en)
}
pub(super) fn col_of(q: &str) -> [f64; 3] {
    match q {
        "Common" => k::GREY,
        "Uncommon" => [0.2, 0.85, 0.5],
        "Rare" => [0.25, 0.6, 1.0],
        "Epic" => [0.75, 0.3, 1.0],
        "Legendary" => [1.0, 0.62, 0.16],
        _ => k::CYAN,
    }
}
thread_local! {
    pub(super) static HOVER: RefCell<Option<String>> = const { RefCell::new(None) };
    pub(super) static HOVER_AT: Cell<f64> = const { Cell::new(0.0) };
    pub(super) static NOW: Cell<f64> = const { Cell::new(0.0) };
}
pub(super) fn hover(key: &str, on: bool) {
    let now = NOW.with(|c| c.get());
    HOVER.with(|h| {
        let mut h = h.borrow_mut();
        if on {
            *h = Some(key.to_string());
            HOVER_AT.with(|c| c.set(now));
        } else if h.as_deref() == Some(key) {
            *h = None;
            HOVER_AT.with(|c| c.set(now));
        }
    });
}
pub(super) fn hover_key() -> Option<String> {
    HOVER.with(|h| h.borrow().clone())
}

#[derive(Clone)]
pub(super) struct Ware {
    pub item: Item,
    pub side: &'static str,
    pub buyback: bool,
    pub available: bool,
    pub recipe: bool,
}
pub(super) struct Snapshot {
    sell: Vec<Ware>,
    buy: Vec<Ware>,
    by_key: HashMap<String, Ware>,
    vendor_money: f64,
    name: String,
}
fn ware(visit: &(Handle, Handle), raw: &Handle, side: &'static str, quantity: Option<f64>, buyback: bool, available: bool) -> Option<Ware> {
    if raw.is_null() {
        return None;
    }
    let id = id16(raw, "GetID", &[]);
    if sb("ItemID", "HasFlag", &[id.a(), A::N(ev("gameEItemIDFlag", "Preview"))]) {
        return None;
    }
    let inv = idt::ui_inventory();
    let ui = if side == "sell" { o(&inv, "GetPlayerItemFromAnySource", &[A::H(raw)]) } else { so("UIInventoryItem", "Make", &[A::H(&visit.0), A::H(raw), A::H(&o(&inv, "GetInventoryItemsManager", &[]))]) };
    if ui.is_null() {
        return None;
    }
    let mgr = &visit.1;
    let icon = s(&ui, "GetIconPath", &[]);
    let mut it = Item {
        key: format!("{side}:{}{}", if buyback { "back:" } else { "" }, idt::key_of(&id)),
        name: s(&ui, "GetName", &[]),
        quality: s(&ui, "GetQualityName", &[]),
        quality_int: n(&ui, "GetQualityInt", &[]),
        ty: n(&ui, "GetItemType", &[]),
        equipped: side == "sell" && b(&ui, "IsEquipped", &[]),
        new: b(&ui, "IsNew", &[]),
        quantity: quantity.unwrap_or_else(|| n(raw, "GetQuantity", &[])),
        iconic: b(&ui, "IsIconic", &[]),
        favorite: b(&ui, "IsPlayerFavourite", &[]),
        image: if icon.is_empty() || icon == "None" { None } else { gd::image(tdb(&icon)) },
        price: Some(if side == "sell" || buyback { n(mgr, "GetSellingPrice", &[id.a()]) } else { n(mgr, "GetBuyingPrice", &[id.a()]) }),
        raw: raw.clone(),
        allowed: true,
        id,
        ..Default::default()
    };
    // the native tile's quest marker (issue #258): UIInventoryItem.IsQuestItem, the Quest or UnequipBlocked tag
    it.quest = b(&ui, "IsQuestItem", &[]);
    // ranged and melee weapons in tabs of their own (issue #221)
    let weapon = b(&ui, "IsWeapon", &[]);
    it.melee = weapon && b(raw, "HasTag", &[A::Name(cname("MeleeWeapon"))]);
    it.category = Some(if weapon {
        if it.melee {
            "melee"
        } else {
            "ranged"
        }
    } else if b(&ui, "IsClothing", &[]) {
        "clothing"
    } else if b(&ui, "IsPart", &[]) {
        "mods"
    } else if sn("RPGManager", "GetItemCategory", &[id.a()]) == ev("gamedataItemCategory", "Consumable") {
        "consumable"
    } else {
        "other"
    });
    let recipe = b(&ui, "IsRecipe", &[]);
    it.ui = ui;
    Some(Ware { item: it, side, buyback, available, recipe })
}
fn stock_available(st: &Val) -> bool {
    st.f("itemStack").and_then(|s0| rt::static_call("VendorItemAdditionalData", "Make", &[A::P(s0.ptr.cast_const())])).is_some_and(|a| fbool(&Val::of(&a), "IsAvailable"))
}
fn sold_quantity(sold: &Handle, id: &Id16) -> f64 {
    o(sold, "GetItem", &[id.a()]).is_null().then_some(0.0).unwrap_or_else(|| pnum(&o(sold, "GetItem", &[id.a()]), "quantity"))
}
fn snapshot(visit: &(Handle, Handle)) -> Snapshot {
    let mgr = &visit.1;
    let mut out = Snapshot { sell: Vec::new(), buy: Vec::new(), by_key: HashMap::new(), vendor_money: sn("MarketSystem", "GetVendorMoney", &[A::H(&visit.0)]), name: loc(&s(mgr, "GetVendorName", &[])) };
    for raw in rt::objs(mgr, "GetItemsPlayerCanSellFast", &[A::H(&player()), A::B(false)]) {
        if let Some(w) = ware(visit, &raw, "sell", None, false, true) {
            out.by_key.insert(w.item.key.clone(), w.clone());
            out.sell.push(w);
        }
    }
    let sold = o(mgr, "GetVendorSoldItems", &[]);
    if let Some(arr) = rt::call(mgr, "GetVendorInventoryItems", &[]) {
        for st in items(&Val::of(&arr)) {
            let raw = st.f("itemData").map(|v| v.handle()).unwrap_or_else(Handle::null);
            let id = id16(&raw, "GetID", &[]);
            let qty = n(&raw, "GetQuantity", &[]);
            let back = sold_quantity(&sold, &id).min(qty);
            let available = stock_available(&st);
            for (q, bb) in [(qty - back, false), (back, true)] {
                if q > 0.0 {
                    if let Some(w) = ware(visit, &raw, "buy", Some(q), bb, available) {
                        out.by_key.insert(w.item.key.clone(), w.clone());
                        out.buy.push(w);
                    }
                }
            }
        }
    }
    // what the page shows of a visit, once per vendor: its stock and the prices read (an empty stock or a price of 0
    // is a manager without its vendor)
    static SAID: std::sync::Mutex<usize> = std::sync::Mutex::new(0);
    let vendor = visit.0.instance as usize;
    let mut said = SAID.lock().unwrap_or_else(|p| p.into_inner());
    if *said != vendor {
        *said = vendor;
        let first = |l: &Vec<Ware>| l.first().and_then(|w| w.item.price).map_or("-".to_string(), |p| format!("{p}"));
        cvr_host::log!(
            "[VRMenu] trade: '{}' sells {} (first price {}), buys {} of the player's (first price {}), vendor money {} (from Rust)\n",
            out.name,
            out.buy.len(),
            first(&out.buy),
            out.sell.len(),
            first(&out.sell),
            out.vendor_money
        );
    }
    out
}
/// A development check (the "trade" command of CVR_VENDOR_WALK's command file): with a visit's trade page open, one
/// piece of the first thing the page would sell is sold through the sell button's own quote and transact, and 2 s later
/// bought back through the buyback's -- the player's and the vendor's money and the player's count written before, after
/// the sale and after the buyback.
pub(super) fn dev_trade(step: u32, state: &mut Option<(Id16, String, f64)>) -> bool {
    let Some(visit) = super::menu_vendor::visit_parts() else {
        cvr_host::log!("[VRMenu] trade test: no visit open (from Rust)\n");
        return false;
    };
    let mgr = visit.1.clone();
    let ts = super::game_system("GetTransactionSystem");
    let money = || (n(&mgr, "GetLocalPlayerCurrencyAmount", &[]), sn("MarketSystem", "GetVendorMoney", &[A::H(&visit.0)]));
    let count = |id: &Id16| n(&ts, "GetItemQuantity", &[A::H(&player()), id.a()]);
    let snap = snapshot(&visit);
    match step {
        0 => {
            let pick = snap.sell.iter().find(|w| {
                let q = quote(&visit, w, 1.0);
                q.allowed && !q.confirm && q.total > 0.0
            });
            let Some(w) = pick else {
                cvr_host::log!("[VRMenu] trade test: nothing the page would sell (from Rust)\n");
                return false;
            };
            let (pm, vm) = money();
            let q = quote(&visit, w, 1.0);
            cvr_host::log!(
                "[VRMenu] trade test: before -- player money {pm}, vendor money {vm}; selling 1 of '{}' (player has {}) for {} (from Rust)\n",
                w.item.name,
                count(&w.item.id),
                q.total
            );
            let done = matches!(transact(&visit, w, 1.0, true), Done::Ok);
            *state = Some((w.item.id, w.item.name.clone(), q.total));
            cvr_host::log!("[VRMenu] trade test: sale sent: {done} (from Rust)\n");
            done
        }
        1 => {
            let Some((id, name, _)) = state.clone() else { return false };
            let (pm, vm) = money();
            cvr_host::log!("[VRMenu] trade test: after the sale -- player money {pm}, vendor money {vm}, player has {} of '{name}' (from Rust)\n", count(&id));
            let back = snap.buy.iter().find(|w| w.buyback && w.item.id == id);
            let Some(w) = back else {
                cvr_host::log!("[VRMenu] trade test: '{name}' is not offered back (from Rust)\n");
                return false;
            };
            let q = quote(&visit, w, 1.0);
            cvr_host::log!("[VRMenu] trade test: buying back 1 of '{name}' for {} (allowed {}) (from Rust)\n", q.total, q.allowed);
            let done = matches!(transact(&visit, w, 1.0, true), Done::Ok);
            cvr_host::log!("[VRMenu] trade test: buyback sent: {done} (from Rust)\n");
            done
        }
        _ => {
            let Some((id, name, _)) = state.clone() else { return false };
            let (pm, vm) = money();
            cvr_host::log!("[VRMenu] trade test: after the buyback -- player money {pm}, vendor money {vm}, player has {} of '{name}' (from Rust)\n", count(&id));
            false
        }
    }
}
pub(super) struct Quote {
    pub allowed: bool,
    pub max: f64,
    pub total: f64,
    pub reason: &'static str,
    pub confirm: bool,
    raw: Handle,
}
fn quote(visit: &(Handle, Handle), w: &Ware, quantity: f64) -> Quote {
    let refused = |reason| Quote { allowed: false, max: 0.0, total: 0.0, reason, confirm: false, raw: Handle::null() };
    let p = player();
    let ts = super::game_system("GetTransactionSystem");
    let owner = if w.side == "sell" { p.clone() } else { visit.0.clone() };
    let raw = o(&ts, "GetItemData", &[A::H(&owner), w.item.id.a()]);
    if raw.is_null() {
        return refused("missing");
    }
    let mgr = &visit.1;
    let mut count = n(&raw, "GetQuantity", &[]);
    let price;
    if w.side == "sell" {
        price = n(mgr, "GetSellingPrice", &[w.item.id.a()]);
        let kind = n(&raw, "GetItemType", &[]);
        let ui_sys = o(&super::game_system("GetScriptableSystemsContainer"), "Get", &[A::Name(cname("UIScriptableSystem"))]);
        if !b(mgr, "CanPlayerSellItem", &[w.item.id.a()]) || b(&ui_sys, "IsItemPlayerFavourite", &[w.item.id.a()]) || [ev("gamedataItemType", "Con_Inhaler"), ev("gamedataItemType", "Con_Injector"), ev("gamedataItemType", "Gad_Grenade")].contains(&kind) {
            return refused("protected");
        }
        if price > 0.0 {
            count = count.min((sn("MarketSystem", "GetVendorMoney", &[A::H(&visit.0)]) / price).floor());
        }
    } else {
        price = if w.buyback { n(mgr, "GetSellingPrice", &[w.item.id.a()]) } else { n(mgr, "GetBuyingPrice", &[w.item.id.a()]) };
        let back = sold_quantity(&o(mgr, "GetVendorSoldItems", &[]), &w.item.id);
        count = if w.buyback { count.min(back) } else { (count - back).max(0.0) };
        let found = rt::call(mgr, "GetVendorInventoryItems", &[]).and_then(|arr| items(&Val::of(&arr)).into_iter().find(|st| st.f("itemData").is_some_and(|d| id16(&d.handle(), "GetID", &[]) == w.item.id)).map(|st| stock_available(&st))).unwrap_or(false);
        if !found {
            return refused("requirements");
        }
        if b(&raw, "HasTag", &[A::Name(cname("MustBeWearableToPurchase"))]) && !b(&o(&w.item.ui, "GetRequirementsManager", &[A::H(&p)]), "IsEquippable", &[]) {
            return refused("requirements");
        }
        if b(&raw, "HasTag", &[A::Name(cname("Ammo"))]) {
            count = count.min((n(&raw, "GetStatValueByType", &[A::N(ev("gamedataStatType", "Quantity"))]) - n(&ts, "GetItemQuantity", &[A::H(&p), w.item.id.a()])).max(0.0));
        }
        if ["Grenade", "Injector", "Inhaler"].iter().any(|t| b(&raw, "HasTag", &[A::Name(cname(t))])) {
            count = count.min(1.0);
        }
        if price > 0.0 {
            count = count.min((n(mgr, "GetLocalPlayerCurrencyAmount", &[]) / price).floor());
        }
    }
    let count = count.floor().max(0.0);
    let quantity = quantity.floor();
    let es = so("EquipmentSystem", "GetInstance", &[A::H(&p)]);
    Quote {
        allowed: quantity > 0.0 && quantity <= count,
        max: count,
        total: price * quantity,
        reason: if count == 0.0 { "funds" } else { "" },
        confirm: w.side == "sell" && (b(&w.item.ui, "IsIconic", &[]) || b(&es, "IsEquipped", &[A::H(&p), w.item.id.a()])),
        raw,
    }
}
enum Done {
    Ok,
    Confirm,
    No,
}
fn transact(visit: &(Handle, Handle), w: &Ware, quantity: f64, confirmed: bool) -> Done {
    let q = quote(visit, w, quantity);
    if !q.allowed {
        return Done::No;
    }
    if q.confirm && !confirmed {
        return Done::Confirm;
    }
    let mgr = &visit.1;
    let f = if w.side == "sell" {
        "SellItemToVendor"
    } else if w.buyback {
        "BuybackItemFromVendor"
    } else {
        "BuyItemFromVendor"
    };
    rt::call(mgr, f, &[A::H(&q.raw), A::N(quantity)]);
    Done::Ok
}
/// SEVERAL AT ONCE (the sell side's selection, issue #221): whole stacks, in the order given, as far as the vendor's money
/// goes. Equipped and protected items (favourites, inhalers, injectors, grenades) are left where they are. Each sale is a
/// queued MarketSystem request, so the vendor's money is counted down here rather than read back (the read lags a tick).
/// The stacks sold, the ones left, the eurodollars and the pieces.
fn sell_many(visit: &(Handle, Handle), list: &[Ware]) -> (u32, u32, f64, f64) {
    let p = player();
    let eq = so("EquipmentSystem", "GetInstance", &[A::H(&p)]);
    let mut money = sn("MarketSystem", "GetVendorMoney", &[A::H(&visit.0)]);
    let (mut items, mut skipped, mut earned, mut pieces) = (0, 0, 0.0, 0.0);
    for w in list {
        let q = (w.side == "sell").then(|| quote(visit, w, 1.0));
        let Some(q) = q.filter(|q| q.allowed && !q.raw.is_null() && !b(&eq, "IsEquipped", &[A::H(&p), w.item.id.a()])) else {
            skipped += 1;
            continue;
        };
        let price = q.total;
        let mut count = n(&q.raw, "GetQuantity", &[]);
        if price > 0.0 {
            count = count.min((money / price).floor());
        }
        if count < 1.0 {
            skipped += 1;
            continue;
        }
        rt::call(&visit.1, "SellItemToVendor", &[A::H(&q.raw), A::N(count)]);
        money -= price * count;
        earned += price * count;
        items += 1;
        pieces += count;
    }
    (items, skipped, earned, pieces)
}
/// THE VENDOR'S STOCK IS PART OF IT (issue #258): a quest can put an item in a vendor's stock while the shop is open
/// (Wilson's Dying Night in the prologue arrives through MarketSystem's AddItemToVendorRequest), and an item can become
/// buyable when a fact changes. Neither moves the money or the weight, so the stock the native grid reads is counted.
fn stock(visit: &(Handle, Handle)) -> String {
    let mut parts: Vec<String> = rt::static_call("MarketSystem", "GetVendorItemsForSale", &[A::H(&visit.0), A::B(true)])
        .map(|arr| {
            items(&Val::of(&arr))
                .iter()
                .map(|st| {
                    let id = st.f("itemID").map(|v| Id16::of(&v)).unwrap_or_default();
                    format!("{}x{}{}", idt::key_of(&id), gd::fmt_g(fnum(st, "quantity")), if fbool(st, "isAvailable") { "" } else { "!" })
                })
                .collect()
        })
        .unwrap_or_default();
    // a change in enumeration order alone leaves the displayed stock unchanged
    parts.sort();
    parts.join(",")
}
fn signature(visit: &(Handle, Handle)) -> String {
    format!(
        "{}:{}:{}:{}",
        gd::fmt_g(n(&visit.1, "GetLocalPlayerCurrencyAmount", &[])),
        gd::fmt_g(sn("MarketSystem", "GetVendorMoney", &[A::H(&visit.0)])),
        gd::fmt_g(pnum(&player(), "m_curInventoryWeight")),
        stock(visit)
    )
}
/// QUEST ITEMS FIRST (issue #258): a quest's item (Wilson's Dying Night) heads the list whatever the sort, keeping the
/// chosen order within each part.
fn quest_first(list: Vec<Ware>) -> Vec<Ware> {
    if !list.iter().any(|w| w.item.quest) {
        return list;
    }
    let (mut out, rest): (Vec<Ware>, Vec<Ware>) = list.into_iter().partition(|w| w.item.quest);
    out.extend(rest);
    out
}

// ---------------------------------------------------------------- the view
#[derive(Default)]
pub(super) struct TradeState {
    ctx: Option<idt::Ctx>,
    started: bool,
    filter: String,
    quantity: f64,
    comparison: bool,
    sorts: [String; 2],
    selected: Option<String>,
    confirm: Option<String>,
    snapshot: Option<Rc<Snapshot>>,
    sorted: Option<[Vec<Ware>; 2]>,
    sorted_key: String,
    loading: bool,
    stamp: String,
    /// The signature taken when the snapshot's read began: a change during the read shows as a new stamp.
    snapshot_stamp: Option<String>,
    /// The sell side's selection (issue #221): on, the marked keys, the sale armed for its confirming press.
    multi: bool,
    marked: HashSet<String>,
    batch_armed: Option<f64>,
    poll: f64,
    pending: Option<f64>,
    offsets: Option<[Option<f64>; 2]>,
    reset_side: Option<usize>,
    sort_popup: Option<SortPopup>,
    tips: Vec<Tip>,
    shown: Option<Option<String>>,
    tip_host: Handle,
    block: Option<BtnRef>,
    plates: Option<iv::Plates>,
}
fn ensure(m: &mut Menu) {
    let s = &mut m.gp.trade;
    if !s.started {
        s.started = true;
        s.filter = "all".into();
        s.quantity = 1.0;
        s.comparison = true;
        s.sorts = ["QualityAsc".into(), "QualityAsc".into()];
    }
    if s.ctx.is_none() {
        s.ctx = Some(idt::open());
    }
}
fn sort_key(s: &TradeState) -> String {
    format!("{}:{}:{}", s.filter, s.sorts[0], s.sorts[1])
}
fn blueprint(p: &Handle, x: f64, y: f64, col: [f64; 3]) {
    let bp = ink::canvas(p, x, y, 146.0, 30.0);
    k::round_fill(&bp, 0.0, 0.0, 146.0, 30.0, k::BLACK, 0.85, Some(3.0));
    let g = ink::canvas(&bp, 116.0, 0.0, 30.0, 30.0);
    let pts = [(8.0, 4.0), (20.0, 4.0), (26.0, 10.0), (26.0, 25.0), (8.0, 25.0), (8.0, 4.0)];
    for i in 0..pts.len() - 1 {
        gp::line(&g, pts[i].0, pts[i].1, pts[i + 1].0, pts[i + 1].1, col, 1.0, 1.5);
    }
    gp::line(&g, 20.0, 4.0, 20.0, 10.0, col, 1.0, 1.5);
    gp::line(&g, 20.0, 10.0, 26.0, 10.0, col, 1.0, 1.5);
    gp::line(&g, 12.0, 14.0, 22.0, 14.0, col, 1.0, 1.5);
    gp::line(&g, 12.0, 19.0, 20.0, 19.0, col, 1.0, 1.5);
    k::text(&bp, 5.0, 0.0, 109.0, 30.0, &tr("ЧЕРТЁЖ", "BLUEPRINT"), 20.0, col, "right", false);
}
#[allow(clippy::too_many_arguments)]
pub(super) fn action(m: &mut Menu, p: &Handle, x: f64, y: f64, w: f64, ru: &str, en: &str, f: Act, enabled: bool, key: &str) -> BtnRef {
    let b = gp::hub_action(m, p, P1X, x, y, w, &tr(ru, en), f, enabled, None, None);
    *b.key.borrow_mut() = Some(key.to_string());
    b
}
/// The tooltip layer's blocker, its close box (shared by trade and stash).
/// The tooltips' layer: its host and its blocker (blocking only where a tooltip's plate is shown, iv::plate_blocker).
pub(super) fn tip_layer(m: &mut Menu, p: &Handle, bx: f64, by: f64, bw: f64, bh: f64) -> (Handle, BtnRef, iv::Plates) {
    let host = ink::canvas(p, 0.0, 0.0, W, 1030.0);
    let block = button(m, "p2", &at(P1X, P1Y), bx, by, bw, bh, None, None);
    block.blocker.set(true);
    block.visible.set(false);
    let plates = iv::plate_blocker(&block);
    (host, block, plates)
}
fn grid(m: &mut Menu, p: &Handle, side: usize, x: f64) {
    let shown = m.gp.trade.sorted.as_ref().map(|s| s[side].clone()).unwrap_or_default();
    let mut popup = m.gp.trade.sort_popup.take();
    let value = m.gp.trade.sorts[side].clone();
    iv::sort_control(
        m,
        p,
        &mut popup,
        &format!("tradeSort:{}", if side == 0 { "sell" } else { "buy" }),
        x + 528.0,
        256.0,
        244.0,
        Some(&value),
        Rc::new(move |m: &mut Menu, v: String| {
            let s = &mut m.gp.trade;
            (s.sorts[side], s.selected, s.confirm, s.reset_side) = (v, None, None, Some(side));
        }),
        Rc::new(|m: &mut Menu| &mut m.gp.trade.sort_popup),
    );
    m.gp.trade.sort_popup = popup;
    let mut rows = Vec::new();
    if shown.is_empty() {
        let t = if side == 1 { tr("Нет товаров в этой категории", "No items in this category") } else { tr("Нет подходящих предметов", "No matching items") };
        rows.push(Row { h: 100.0, build: Rc::new(move |_m: &mut Menu, c: &Handle, _| {
            k::text(c, 10.0, 10.0, 790.0, 80.0, &t, 29.0, k::GREY, "center", false);
        }) });
    }
    for pair in shown.chunks(2) {
        let pair = pair.to_vec();
        rows.push(Row {
            h: 200.0,
            build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                for (i, w) in pair.iter().enumerate() {
                    let (xx, width) = (i as f64 * 404.0, 392.0);
                    let col = col_of(&w.item.quality);
                    let sel = m.gp.trade.selected.as_deref() == Some(w.item.key.as_str());
                    let marking = m.gp.trade.multi && side == 0;
                    let marked = marking && m.gp.trade.marked.contains(&w.item.key);
                    let bg = k::round_fill(c, xx, 4.0, width, 185.0, col, if marked { 0.13 } else { 0.045 }, Some(4.0));
                    let edge = k::cut_frame(&mut m.pool, c, xx, 4.0, width, 185.0, if marked || sel { k::YELLOW } else { k::CYAN }, if marked { 1.0 } else { 0.35 }, Some(10.0), Some(if marked { 3.0 } else { 1.0 }));
                    k::rect(c, xx + 2.0, 8.0, 5.0, 177.0, col, 1.0);
                    gp::picture(m, c, xx + 14.0, if w.recipe { 44.0 } else { 20.0 }, width - 28.0, if w.recipe { 80.0 } else { 104.0 }, w.item.image.as_ref(), None);
                    if w.recipe {
                        blueprint(c, xx + width - 150.0, 10.0, col);
                    }
                    k::text(c, xx + 12.0, 126.0, width - 24.0, 30.0, &k::clip(&w.item.name, 27), 24.0, col, "left", false);
                    k::text(c, xx + 14.0, 158.0, 200.0, 28.0, &format!("€$ {}", gd::fmt_g(w.item.price.unwrap_or(0.0))), 24.0, k::YELLOW, "left", false);
                    let right = if w.item.quantity > 1.0 { format!("x{}", gd::fmt_g(w.item.quantity)) } else if w.buyback { tr("ВЫКУП", "BUYBACK") } else { String::new() };
                    k::text(c, xx + 230.0, 158.0, 147.0, 28.0, &right, 23.0, k::GREY, "right", false);
                    if w.item.equipped {
                        gp::line(c, xx + width - 34.0, 15.0, xx + width - 26.0, 23.0, k::CYAN, 1.0, 3.0);
                        gp::line(c, xx + width - 26.0, 23.0, xx + width - 12.0, 8.0, k::CYAN, 1.0, 3.0);
                    }
                    if w.item.new {
                        k::image(c, xx + 12.0, 10.0, 9.0, 9.0, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
                    }
                    // favourite, iconic (issue #221): the backpack's star, and the word in yellow
                    if w.item.favorite {
                        super::menu_inventory::star(c, xx + 36.0, 24.0, 10.0, k::YELLOW, 1.0, 2.0);
                    }
                    if w.item.iconic {
                        k::text(c, xx + width - 196.0, 4.0, 150.0, 30.0, &tr("КУЛЬТОВОЕ", "ICONIC"), 19.0, k::YELLOW, "right", false);
                    }
                    // the top-right slot left to the equipped check or the blueprint label when present
                    if w.item.quest {
                        let badge_x = xx + if w.item.equipped || w.recipe { 52.0 } else { width - 40.0 };
                        k::image(c, badge_x, 10.0, 28.0, 28.0, k::ATLAS_PHONE, "circle", Some(k::YELLOW), 1.0);
                        k::text(c, badge_x, 9.0, 28.0, 30.0, "!", 23.0, k::BLACK, "center", false);
                    }
                    if marking {
                        // the selection box, ticked when marked
                        k::cut_frame(&mut m.pool, c, xx + width - 52.0, 44.0, 34.0, 34.0, if marked { k::YELLOW } else { k::CYAN }, if marked { 1.0 } else { 0.6 }, Some(5.0), Some(2.0));
                        if marked {
                            gp::line(c, xx + width - 45.0, 61.0, xx + width - 38.0, 69.0, k::YELLOW, 1.0, 4.0);
                            gp::line(c, xx + width - 38.0, 69.0, xx + width - 25.0, 51.0, k::YELLOW, 1.0, 4.0);
                        }
                    }
                    let (key, key2) = (w.item.key.clone(), w.item.key.clone());
                    let b = button(
                        m,
                        "p1",
                        &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                        xx,
                        4.0,
                        width,
                        185.0,
                        Some(Rc::new(move |m: &mut Menu, _| {
                            let s = &mut m.gp.trade;
                            if marking {
                                if !s.marked.remove(&key) {
                                    s.marked.insert(key.clone());
                                }
                                s.batch_armed = None;
                                gp::redraw(m);
                                return;
                            }
                            if s.multi {
                                // the vendor's side waits while your items are being marked
                                return;
                            }
                            (s.selected, s.quantity, s.confirm) = (Some(key.clone()), 1.0, None);
                            if let (Some(ctx), Some(mut it)) = (s.ctx.as_ref(), s.snapshot.as_ref().and_then(|sn| sn.by_key.get(&key)).map(|w| w.item.clone())) {
                                idt::inspect(ctx, &mut it);
                            }
                            gp::redraw(m);
                        })),
                        Some(Rc::new(move |on, _| {
                            ink::tint(&bg, if on { k::YELLOW } else { col });
                            ink::opacity(&bg, if on || marked { 0.13 } else { 0.045 });
                            k::tint(&edge, if on || marked || sel { k::YELLOW } else { k::CYAN }, Some(if on || marked { 1.0 } else { 0.35 }));
                            hover(&key2, on);
                        })),
                    );
                    *b.key.borrow_mut() = Some(format!("tradeItem:{}", w.item.key));
                }
            }),
        });
    }
    let host = ink::canvas(p, x, 0.0, 850.0, 1100.0);
    let keep = m.gp.trade.offsets.as_ref().and_then(|o| o[side]);
    let v = gp::glist(m, &host, "p1", if side == 0 { "p1" } else { "p2" }, P1X + x, 320.0, 850.0, rows, 645.0, keep);
    v.borrow_mut().enable_mask();
}
pub(super) fn draw(m: &mut Menu, p: &Handle, _p2: &Handle) {
    ensure(m);
    NOW.with(|c| c.set(m.time));
    m.gp.trade.tips.clear();
    m.gp.trade.shown = None;
    HOVER.with(|h| *h.borrow_mut() = None);
    glass(m, p, W, H, Some((P1X, P1Y)));
    gp::title(p, &tr("ТОРГОВЛЯ", "TRADE"), W);
    let Some(visit) = super::menu_vendor::current_ripperdoc(m).and_then(|_| super::menu_vendor::visit_parts()) else {
        k::text(p, 30.0, 210.0, W - 60.0, 160.0, &tr("Откройте магазин у продавца, чтобы торговать.", "Open a shop with a vendor to trade."), 34.0, k::CYAN, "center", false);
        return;
    };
    let sk = sort_key(&m.gp.trade);
    if m.gp.trade.snapshot.is_none() || m.gp.trade.sorted.is_none() || m.gp.trade.sorted_key != sk {
        m.gp.trade.loading = true;
        return;
    }
    m.gp.trade.stamp = m.gp.trade.snapshot_stamp.clone().unwrap_or_else(|| signature(&visit));
    m.gp.trade.poll = m.time;
    let cmp = m.gp.trade.comparison;
    gp::comparison(
        m,
        p,
        H,
        cmp,
        Rc::new(|m: &mut Menu, _| {
            m.gp.trade.comparison = !m.gp.trade.comparison;
            gp::redraw(m);
        }),
        "tradeComparison",
    );
    // ranged and melee weapons each in a tab (issue #221)
    let filters = [("all", "ВСЁ", "ALL"), ("ranged", "ОГНЕСТРЕЛЬНОЕ", "RANGED"), ("melee", "ХОЛОДНОЕ", "MELEE"), ("clothing", "ОДЕЖДА", "CLOTHING"), ("mods", "МОДИФИКАЦИИ", "MODS"), ("consumable", "РАСХОДНИКИ", "CONSUMABLES"), ("other", "ДРУГОЕ", "OTHER")];
    if m.gp.trade.filter == "weapon" {
        m.gp.trade.filter = "all".into();
    }
    for (i, (key, ru, en)) in filters.iter().enumerate() {
        let x = 24.0 + i as f64 * 248.0;
        let on = m.gp.trade.filter == *key;
        let bg = k::round_fill(p, x, 105.0, 238.0, 60.0, if on { k::YELLOW } else { k::NAVY }, if on { 0.12 } else { 0.18 }, Some(4.0));
        let label = k::text(p, x, 105.0, 238.0, 60.0, &tr(ru, en), 23.0, if on { k::YELLOW } else { k::TEXT }, "center", false);
        let key2 = key.to_string();
        let b = button(
            m,
            "p1",
            &at(P1X, P1Y),
            x,
            105.0,
            238.0,
            60.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                let s = &mut m.gp.trade;
                (s.filter, s.selected, s.confirm) = (key2.clone(), None, None);
                m.gp.reset_list = true;
                gp::redraw(m);
            })),
            Some(Rc::new(move |h, _| {
                ink::tint(&bg, if h || on { k::YELLOW } else { k::NAVY });
                ink::tint(&label, if h || on { k::YELLOW } else { k::TEXT });
                ink::opacity(&bg, if h { 0.18 } else if on { 0.12 } else { 0.18 });
            })),
        );
        *b.key.borrow_mut() = Some(format!("tradeFilter:{key}"));
    }
    let snap = m.gp.trade.snapshot.clone().expect("snapshot");
    k::text(p, 24.0, 180.0, 820.0, 60.0, &format!("{}  /  {}", tr("ВАШИ ПРЕДМЕТЫ", "YOUR ITEMS"), snap.sell.len()), 31.0, k::CYAN, "left", false);
    let vname = if snap.name.is_empty() { tr("ПРОДАВЕЦ", "VENDOR") } else { snap.name.clone() };
    k::text(p, 920.0, 180.0, 820.0, 60.0, &format!("{}  /  €$ {}", k::clip(&vname, 25), gd::fmt_g(snap.vendor_money)), 31.0, k::YELLOW, "left", false);
    k::rect(p, 882.0, 180.0, 1.0, 840.0, k::CYAN, 0.22);
    // SELECTION on the sell side, as in the backpack: mark any number of your items and sell them at once (issue #221)
    let multi_label = if m.gp.trade.multi { tr("ГОТОВО", "DONE") } else { tr("ВЫДЕЛЕНИЕ", "SELECT") };
    let sb = gp::hub_action(
        m,
        p,
        P1X,
        24.0,
        252.0,
        240.0,
        &multi_label,
        Rc::new(|m: &mut Menu, _| {
            let s = &mut m.gp.trade;
            s.multi = !s.multi;
            s.marked.clear();
            (s.batch_armed, s.selected, s.confirm) = (None, None, None);
            gp::redraw(m);
        }),
        true,
        Some(56.0),
        Some(24.0),
    );
    *sb.key.borrow_mut() = Some("tradeMulti".into());
    grid(m, p, 0, 10.0);
    grid(m, p, 1, 906.0);
    let selected = if m.gp.trade.multi { None } else { m.gp.trade.selected.clone().and_then(|k0| snap.by_key.get(&k0).cloned()) };
    match selected {
        Some(sel) => {
            let q = quote(&visit, &sel, m.gp.trade.quantity);
            let name = format!("{}{}", if sel.recipe { tr("ЧЕРТЁЖ · ", "BLUEPRINT · ") } else { String::new() }, sel.item.name);
            k::text(p, 26.0, 1037.0, 1710.0, 47.0, &k::clip(&name, 75), 32.0, col_of(&sel.item.quality), "left", false);
            let qty = m.gp.trade.quantity;
            if q.max > 1.0 {
                let max = q.max;
                action(m, p, 24.0, 1110.0, 90.0, "", "", Rc::new(|m: &mut Menu, _| {
                    let s = &mut m.gp.trade;
                    (s.quantity, s.confirm) = ((s.quantity - 1.0).max(1.0), None);
                    gp::redraw(m);
                }), qty > 1.0, "tradeQuantityMinus");
                k::rect(p, 51.0, 1142.0, 36.0, 4.0, k::CYAN, if qty > 1.0 { 1.0 } else { 0.25 });
                k::text(p, 125.0, 1110.0, 150.0, 68.0, &gd::fmt_g(qty), 32.0, k::CYAN, "center", false);
                action(m, p, 286.0, 1110.0, 90.0, "+", "+", Rc::new(move |m: &mut Menu, _| {
                    let s = &mut m.gp.trade;
                    (s.quantity, s.confirm) = ((s.quantity + 1.0).min(max), None);
                    gp::redraw(m);
                }), qty < max, "tradeQuantityPlus");
                action(m, p, 392.0, 1110.0, 180.0, "МАКС.", "MAX", Rc::new(move |m: &mut Menu, _| {
                    let s = &mut m.gp.trade;
                    (s.quantity, s.confirm) = (max, None);
                    gp::redraw(m);
                }), true, "tradeQuantityMax");
            }
            let (mut ru, mut en) = if sel.side == "sell" { ("ПРОДАТЬ", "SELL") } else if sel.buyback { ("ВЫКУПИТЬ", "BUY BACK") } else { ("КУПИТЬ", "BUY") };
            let confirming = m.gp.trade.confirm.is_some();
            if confirming {
                (ru, en) = ("ПОДТВЕРДИТЬ ПРОДАЖУ", "CONFIRM SALE");
            }
            let total = gd::fmt_g(q.total);
            let label = if loc::ru() { format!("{ru}  €$ {total}") } else { format!("{}  €$ {total}", loc::t(en)) };
            let (s2, v2) = (sel.clone(), visit.clone());
            let b = gp::hub_action(
                m,
                p,
                P1X,
                900.0,
                1110.0,
                850.0,
                &label,
                Rc::new(move |m: &mut Menu, _| {
                    if m.gp.trade.pending.is_some() {
                        return;
                    }
                    let confirmed = m.gp.trade.confirm.as_deref() == Some(s2.item.key.as_str());
                    match transact(&v2, &s2, m.gp.trade.quantity, confirmed) {
                        Done::Confirm => {
                            m.gp.trade.confirm = Some(s2.item.key.clone());
                            gp::redraw(m);
                        }
                        Done::Ok => {
                            m.gp.trade.confirm = None;
                            m.gp.trade.pending = Some(m.time);
                            let t = tr("Применение…", "Applying…");
                            toast(m, &t);
                        }
                        Done::No => {
                            let t = m.tx("blocked");
                            toast(m, &t);
                        }
                    }
                }),
                q.allowed && m.gp.trade.pending.is_none(),
                None,
                None,
            );
            *b.key.borrow_mut() = Some("tradeApply".into());
            if confirming {
                k::text(p, 585.0, 1100.0, 300.0, 100.0, &tr("Будет продан культовый или экипированный предмет", "Selling an iconic or equipped item"), 24.0, k::YELLOW, "left", true);
            } else if !q.allowed {
                let t = if q.reason == "protected" { tr("Этот предмет нельзя продать", "This item cannot be sold") } else { tr("Недостаточно денег, запаса или не выполнены требования", "Insufficient funds, stock or requirements") };
                k::text(p, 24.0, 1100.0, 820.0, 100.0, &t, 26.0, k::RED, "left", true);
            }
        }
        None if m.gp.trade.multi => multi_sale(m, p, &snap, &visit),
        None => {
            k::text(p, 30.0, 1080.0, W - 60.0, 100.0, &tr("Выберите предмет для покупки или продажи", "Select an item to buy or sell"), 31.0, k::GREY, "center", false);
        }
    }
    let (host, block, plates) = tip_layer(m, p, 900.0, 240.0, 850.0, 790.0);
    let s = &mut m.gp.trade;
    (s.tip_host, s.block, s.plates) = (host, Some(block), Some(plates));
    let popup = m.gp.trade.sort_popup.clone();
    iv::sort_popup(m, p, &popup, Rc::new(|m: &mut Menu| &mut m.gp.trade.sort_popup));
}
/// The marked items: how many, what they fetch, and one sale for all of them confirmed by a second press.
fn multi_sale(m: &mut Menu, p: &Handle, snap: &Rc<Snapshot>, visit: &(Handle, Handle)) {
    let (mut list, mut pieces, mut total, mut iconic, mut equipped) = (Vec::new(), 0.0, 0.0, 0, 0);
    // every tab's marks, not only the one shown
    for w in snap.sell.iter().filter(|w| m.gp.trade.marked.contains(&w.item.key)) {
        pieces += w.item.quantity;
        total += w.item.price.unwrap_or(0.0) * w.item.quantity;
        iconic += w.item.iconic as u32;
        equipped += w.item.equipped as u32;
        list.push(w.clone());
    }
    m.gp.trade.marked.retain(|key| snap.by_key.contains_key(key));
    let armed = m.gp.trade.batch_armed.is_some_and(|t| m.time - t < 3.0);
    let count = list.len();
    let summary = if count > 0 {
        let extra = if pieces > count as f64 { format!("  ({}{})", tr("шт. ", "pcs "), gd::fmt_g(pieces)) } else { String::new() };
        format!("{}{count}{extra}  ·  €$ {}", tr("Выбрано: ", "Selected: "), gd::fmt_g(total))
    } else {
        tr("Отметьте предметы для продажи", "Mark the items to sell")
    };
    k::text(p, 26.0, 1037.0, 1710.0, 47.0, &summary, 32.0, if count > 0 { k::YELLOW } else { k::GREY }, "left", false);
    let any = m.gp.trade.sorted.as_ref().is_some_and(|s| !s[0].is_empty());
    action(
        m,
        p,
        24.0,
        1110.0,
        200.0,
        "ВСЁ",
        "ALL",
        Rc::new(|m: &mut Menu, _| {
            let s = &mut m.gp.trade;
            let keys: Vec<String> = s.sorted.as_ref().map(|l| l[0].iter().filter(|w| !w.item.equipped && !w.item.favorite).map(|w| w.item.key.clone()).collect()).unwrap_or_default();
            s.marked.extend(keys);
            s.batch_armed = None;
            gp::redraw(m);
        }),
        any,
        "tradeMarkAll",
    );
    action(
        m,
        p,
        236.0,
        1110.0,
        200.0,
        "СНЯТЬ",
        "NONE",
        Rc::new(|m: &mut Menu, _| {
            m.gp.trade.marked.clear();
            m.gp.trade.batch_armed = None;
            gp::redraw(m);
        }),
        count > 0,
        "tradeMarkNone",
    );
    let total_text = gd::fmt_g(total);
    let (ru, en) = if armed { (format!("ПОДТВЕРДИТЬ ПРОДАЖУ ({count})"), format!("CONFIRM SALE ({count})")) } else { (format!("ПРОДАТЬ ВЫБРАННОЕ  €$ {total_text}"), format!("SELL SELECTED  €$ {total_text}")) };
    let v2 = visit.clone();
    action(
        m,
        p,
        900.0,
        1110.0,
        850.0,
        &ru,
        &en,
        Rc::new(move |m: &mut Menu, _| {
            if m.gp.trade.pending.is_some() {
                return;
            }
            if !m.gp.trade.batch_armed.is_some_and(|t| m.time - t < 3.0) {
                m.gp.trade.batch_armed = Some(m.time);
                gp::redraw(m);
                return;
            }
            m.gp.trade.batch_armed = None;
            let (items, skipped, earned, _) = sell_many(&v2, &list);
            let skip = if skipped > 0 { format!("{}{skipped}", tr(", пропущено: ", ", skipped: ")) } else { String::new() };
            let t = format!("{}{items}  €$ {}{skip}", tr("Продано: ", "Sold: "), gd::fmt_g(earned));
            toast(m, &t);
            m.gp.trade.marked.clear();
            if items > 0 {
                m.gp.trade.pending = Some(m.time);
            }
            gp::redraw(m);
        }),
        count > 0 && m.gp.trade.pending.is_none(),
        "tradeSellMarked",
    );
    if armed && (iconic > 0 || equipped > 0) {
        let mut t = String::new();
        if iconic > 0 {
            t = format!("{}{iconic}", tr("Культовых: ", "Iconic: "));
        }
        if iconic > 0 && equipped > 0 {
            t.push_str("; ");
        }
        if equipped > 0 {
            t.push_str(&format!("{}{equipped}", tr("экипированные останутся: ", "equipped stay: ")));
        }
        k::text(p, 450.0, 1100.0, 440.0, 100.0, &t, 24.0, k::YELLOW, "left", true);
    }
}
fn filtered(list: &[Ware], filter: &str) -> Vec<Ware> {
    list.iter().filter(|w| filter == "all" || w.item.category == Some(filter)).cloned().collect()
}
/// Sorted by the game's order, the wares keep their own data (the order is computed on their items).
pub(super) fn sort_wares(list: Vec<Ware>, key: &str) -> Vec<Ware> {
    let mut by: HashMap<String, Ware> = list.iter().map(|w| (w.item.key.clone(), w.clone())).collect();
    let sorted = idt::sort_apply(list.into_iter().map(|w| w.item).collect(), Some(key));
    sorted.into_iter().filter_map(|it| by.remove(&it.key)).collect()
}
pub(super) fn update(m: &mut Menu) {
    NOW.with(|c| c.set(m.time));
    if !m.gp.trade.started {
        return;
    }
    let Some(visit) = super::menu_vendor::current_ripperdoc(m).and_then(|_| super::menu_vendor::visit_parts()) else {
        super::menu_main::open_section(m, Some("inventory"), false);
        return;
    };
    if m.gp.trade.loading {
        let snap = match m.gp.trade.snapshot.clone() {
            Some(s) => s,
            None => {
                // the stamp is taken when the read begins: a change during it shows as a new stamp
                m.gp.trade.snapshot_stamp = Some(signature(&visit));
                Rc::new(snapshot(&visit))
            }
        };
        let s = &m.gp.trade;
        let sorted = [quest_first(sort_wares(filtered(&snap.sell, &s.filter), &s.sorts[0])), quest_first(sort_wares(filtered(&snap.buy, &s.filter), &s.sorts[1]))];
        let key = sort_key(s);
        let s = &mut m.gp.trade;
        (s.snapshot, s.sorted, s.sorted_key, s.loading) = (Some(snap), Some(sorted), key, false);
        gp::redraw(m);
        return;
    }
    iv::tip_fit(&mut m.gp.trade.tips);
    if let Some(plates) = &m.gp.trade.plates {
        iv::set_plates(plates, &m.gp.trade.tips);
    }
    let key = hover_key().or_else(|| m.gp.trade.selected.clone());
    if !m.gp.trade.tip_host.is_null() && Some(&key) != m.gp.trade.shown.as_ref() && m.time - HOVER_AT.with(|c| c.get()) > 0.12 {
        let host = m.gp.trade.tip_host.clone();
        ink::remove_all(&host);
        m.gp.trade.tips.clear();
        m.gp.trade.shown = Some(key.clone());
        hide_layer(&m.gp.trade.block);
        let w = key.as_ref().and_then(|k0| m.gp.trade.snapshot.as_ref().and_then(|s| s.by_key.get(k0).cloned()));
        if let Some(w) = w {
            let x = if w.side == "sell" { 912.0 } else { 24.0 };
            let resolver = so("InventoryItemPreferredComparisonResolver", "Make", &[A::H(&idt::ui_inventory())]);
            let comparison = if m.gp.trade.comparison { o(&resolver, "GetPreferredComparisonItem", &[A::H(&w.item.ui)]) } else { Handle::null() };
            let ctx_of = |side: &str| if side == "sell" { "VendorPlayer" } else { "Vendor" };
            if !comparison.is_null() && id16(&comparison, "GetID", &[]) != w.item.id {
                if let Some(t) = iv::tip_spawn(m, &host, &w.item.ui, ctx_of(w.side), x + 8.0, 250.0, 400.0, 755.0, Some(&comparison), w.item.price) {
                    m.gp.trade.tips.push(t);
                }
                if let Some(t) = iv::tip_spawn(m, &host, &comparison, "VendorPlayer", x + 425.0, 250.0, 400.0, 755.0, Some(&w.item.ui), None) {
                    m.gp.trade.tips.push(t);
                }
            } else if let Some(t) = iv::tip_spawn(m, &host, &w.item.ui, ctx_of(w.side), x + 12.0, 250.0, 810.0, 755.0, None, w.item.price) {
                m.gp.trade.tips.push(t);
            }
            show_layer(&m.gp.trade.block, x);
        }
    }
    if m.time - m.gp.trade.poll < 0.5 {
        return;
    }
    m.gp.trade.poll = m.time;
    let stamp = signature(&visit);
    if stamp != m.gp.trade.stamp || m.gp.trade.pending.is_some_and(|t| m.time - t > 3.0) {
        let offs = [m.view("p1").map(|v| v.v.borrow().offset), m.view("p2").map(|v| v.v.borrow().offset)];
        let s = &mut m.gp.trade;
        s.pending = None;
        s.offsets = Some(offs);
        if let Some(c) = s.ctx.as_mut() {
            idt::invalidate(c);
        }
        (s.snapshot, s.sorted) = (None, None);
        gp::redraw(m);
    }
}
pub(super) fn hide_layer(block: &Option<BtnRef>) {
    if let Some(b) = block {
        b.visible.set(false);
    }
}
pub(super) fn show_layer(block: &Option<BtnRef>, x: f64) {
    if let Some(b) = block {
        b.x.set(P1X + x);
        b.visible.set(true);
    }
}
pub(super) fn before_build(m: &mut Menu) {
    if !m.gp.trade.started {
        return;
    }
    let reset = m.gp.reset_list;
    let any_view = m.view("p1").is_some() || m.view("p2").is_some();
    let offs = [m.view("p1").map(|v| v.v.borrow().offset), m.view("p2").map(|v| v.v.borrow().offset)];
    let s = &mut m.gp.trade;
    // THE SCROLL SURVIVES A SALE (issue #221): a transaction's refresh first builds the panel without its lists (loading),
    // and the views read then are empty; the offsets taken before it stand until there are lists again
    if reset {
        s.offsets = None;
    } else if any_view {
        s.offsets = Some(offs);
    }
    if let Some(side) = s.reset_side.take() {
        let mut o = s.offsets.unwrap_or([None, None]);
        o[side] = Some(0.0);
        s.offsets = Some(o);
    }
    s.tip_host = Handle::null();
    s.tips.clear();
}
pub(super) fn leave(m: &mut Menu) {
    if let Some(c) = m.gp.trade.ctx.take() {
        idt::close(c);
    }
    m.gp.trade = TradeState::default();
}
