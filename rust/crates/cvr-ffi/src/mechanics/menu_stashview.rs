//! stash_data.lua and stash_view.lua: a storage container's visit -- V's items and the storage's, the same visibility
//! rules as the fullscreen stash, each side's filters and order, the game's tooltips, STORE / TAKE with a confirmation
//! for an equipped item, VendorDataManager's native transfer; the panel read again when either side's items change.
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
use super::menu_trade::{self as td, hover, hover_key, HOVER, HOVER_AT, NOW};
use red4ext_abi::handle::Handle;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const W: f64 = 1780.0;
const H: f64 = 1305.0;
const FILTERS: [&str; 11] = ["AllItems", "RangedWeapons", "MeleeWeapons", "Clothes", "Consumables", "Attachments", "SoftwareMods", "Programs", "Cyberware", "Junk", "Quest"];
fn tr(ru: &str, en: &str) -> String {
    loc::tr(ru, en)
}
#[derive(Clone)]
struct Thing {
    item: Item,
    side: usize,
    slots: Vec<idt::Slot>,
}
struct Snapshot {
    sides: [Vec<Thing>; 2],
    by_key: HashMap<String, Thing>,
}
fn transferable(raw: &Handle) -> bool {
    let kind = sn("RPGManager", "GetItemType", &[id16(raw, "GetID", &[]).a()]);
    ![ev("gamedataItemType", "Con_Inhaler"), ev("gamedataItemType", "Con_Injector"), ev("gamedataItemType", "Gad_Grenade")].contains(&kind)
}
fn visible(raw: &Handle, side: usize) -> bool {
    if raw.is_null() {
        return false;
    }
    let id = id16(raw, "GetID", &[]);
    if sb("ItemID", "HasFlag", &[id.a(), A::N(ev("gameEItemIDFlag", "Preview"))]) {
        return false;
    }
    if side == 0 {
        let tags = rt::static_call("UIInventoryItemsManager", "GetStashBlacklistedTags", &[]);
        let blocked = tags.is_some_and(|t| b(raw, "HasAnyOfTags", &[A::P(t.as_ptr().cast())]));
        !super::menu_invdata::ui_item(&idt::ui_inventory(), &id).is_null() && !blocked
    } else {
        !sb("InventoryDataManagerV2", "IsItemBlacklisted", &[A::H(raw)])
    }
}
fn thing(ctx: &idt::Ctx, object: &Handle, raw: &Handle, side: usize) -> Option<Thing> {
    if !visible(raw, side) {
        return None;
    }
    let inv = idt::ui_inventory();
    let id = id16(raw, "GetID", &[]);
    let ui = if side == 0 { super::menu_invdata::ui_item(&inv, &id) } else { so("UIInventoryItem", "Make", &[A::H(object), A::H(raw), A::H(&o(&inv, "GetInventoryItemsManager", &[]))]) };
    let data = rt::call(&ctx.manager, "GetInventoryItemData", &[A::H(raw)])?;
    let mut it = idt::item(ctx, data)?;
    it.key = format!("{}:{}", if side == 0 { "deposit" } else { "withdraw" }, idt::key_of(&it.id));
    it.quantity = n(raw, "GetQuantity", &[]);
    it.equipped = side == 0 && b(&ui, "IsEquipped", &[]);
    it.category = None;
    it.raw = raw.clone();
    let resolver = so("InventoryItemPreferredComparisonResolver", "Make", &[A::H(&inv)]);
    it.comparison = Some(if b(&resolver, "IsComparable", &[A::H(&ui)]) { n(&resolver, "GetItemComparisonState", &[A::H(&ui)]) } else { ev("ItemComparisonState", "Default") });
    let cat = n(&ui, "GetFilterCategory", &[]);
    it.price = Some(cat);
    it.ui = ui;
    let slots = idt::slots(ctx, Some(&it));
    Some(Thing { item: it, side, slots })
}
fn category(t: &Thing) -> f64 {
    t.item.price.unwrap_or(-1.0)
}
fn item_list(owner: &Handle) -> Vec<Handle> {
    match rt::call_outs(&super::game_system("GetTransactionSystem"), "GetItemList", &[A::H(owner)], 1) {
        Some((_, outs)) => outs.first().map(|a| items(&Val::of(a)).iter().map(|v| v.handle()).collect()).unwrap_or_default(),
        None => Vec::new(),
    }
}
fn snapshot(ctx: &idt::Ctx, object: &Handle, manager: &Handle) -> Snapshot {
    let mut out = Snapshot { sides: [Vec::new(), Vec::new()], by_key: HashMap::new() };
    rt::call(&idt::ui_inventory(), "FlushTempData", &[]);
    let pl = player();
    let eid = id16(&pl, "GetEntityID", &[]);
    for side in 0..2 {
        let list = if side == 0 { item_list(&pl) } else { rt::objs(manager, "GetStorageItems", &[]) };
        for raw in list {
            if side == 1 && visible(&raw, 1) {
                rt::call(&raw, "ReinitializePlayerStats", &[eid.a()]);
            }
            if let Some(t) = thing(ctx, object, &raw, side) {
                out.by_key.insert(t.item.key.clone(), t.clone());
                out.sides[side].push(t);
            }
        }
    }
    out
}
struct Quote {
    allowed: bool,
    max: f64,
    confirm: bool,
    raw: Handle,
    source: Handle,
    target: Handle,
}
fn quote(object: &Handle, t: &Thing, quantity: f64) -> Quote {
    let p = player();
    let ts = super::game_system("GetTransactionSystem");
    let source = if t.side == 0 { p.clone() } else { object.clone() };
    let raw = o(&ts, "GetItemData", &[A::H(&source), t.item.id.a()]);
    let none = Quote { allowed: false, max: 0.0, confirm: false, raw: Handle::null(), source: Handle::null(), target: Handle::null() };
    if !visible(&raw, t.side) || !transferable(&raw) {
        return none;
    }
    let count = n(&raw, "GetQuantity", &[]).min(n(&ts, "GetItemQuantity", &[A::H(&source), t.item.id.a()]));
    let amount = quantity.floor().max(1.0);
    let es = so("EquipmentSystem", "GetInstance", &[A::H(&p)]);
    let equipped = t.side == 0 && b(&es, "IsEquipped", &[A::H(&p), t.item.id.a()]);
    if equipped && b(&raw, "HasTag", &[A::Name(cname("UnequipBlocked"))]) {
        return Quote { max: count, ..none };
    }
    Quote { allowed: count >= amount, max: count, confirm: equipped, raw, target: if t.side == 0 { object.clone() } else { p }, source }
}
fn signature(object: &Handle) -> String {
    let mut values = Vec::new();
    for (i, owner) in [player(), object.clone()].iter().enumerate() {
        let mut side: Vec<String> = item_list(owner).iter().filter(|r| visible(r, i)).map(|r| format!("{}:{}", idt::key_of(&id16(r, "GetID", &[])), gd::fmt_g(n(r, "GetQuantity", &[])))).collect();
        side.sort();
        values.push(side.join("|"));
    }
    values.join("\n")
}

#[derive(Default)]
pub(super) struct StashState {
    ctx: Option<idt::Ctx>,
    started: bool,
    quantity: f64,
    comparison: bool,
    filters: [String; 2],
    sorts: [String; 2],
    selected: Option<String>,
    confirm: Option<String>,
    snapshot: Option<Rc<Snapshot>>,
    stamp: String,
    poll: f64,
    pending: Option<(f64, Id16, usize, f64, f64)>,
    offsets: Option<[Option<f64>; 2]>,
    reset_side: Option<usize>,
    sort_popup: Option<SortPopup>,
    tips: Vec<Tip>,
    shown: Option<Option<String>>,
    tip_host: Handle,
    block: Option<BtnRef>,
    plates: Option<iv::Plates>,
    apply: Option<BtnRef>,
}
fn ensure(m: &mut Menu) {
    let s = &mut m.gp.stash;
    if !s.started {
        s.started = true;
        s.quantity = 1.0;
        s.comparison = true;
        s.filters = ["all".into(), "all".into()];
        s.sorts = ["QualityAsc".into(), "QualityAsc".into()];
    }
    if s.ctx.is_none() {
        s.ctx = Some(idt::open());
    }
}
fn card(m: &mut Menu, c: &Handle, it: &RowCtx, t: &Thing, x: f64) {
    let (w, h) = (392.0, 178.0);
    let v = &t.item;
    let col = td::col_of(&v.quality);
    let weapon = b(&v.ui, "IsWeapon", &[]);
    let sel = m.gp.stash.selected.as_deref() == Some(v.key.as_str());
    let bg = k::round_fill(c, x, 4.0, w, h, col, 0.045, Some(4.0));
    let edge = k::cut_frame(&mut m.pool, c, x, 4.0, w, h, if sel { k::YELLOW } else { k::CYAN }, 0.35, Some(10.0), Some(1.0));
    k::rect(c, x + 2.0, 8.0, 5.0, h - 8.0, col, 1.0);
    gp::picture(m, c, x + 14.0, 27.0, w - 28.0, 92.0, v.image.as_ref(), None);
    k::text(c, x + 12.0, 119.0, w - 24.0, 28.0, &k::clip(&v.name, 29), 23.0, col, "left", false);
    if v.new && t.side == 0 {
        k::image(c, x + w - 20.0, 13.0, 9.0, 9.0, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
    }
    if v.equipped {
        gp::line(c, x + w - 50.0, 15.0, x + w - 43.0, 22.0, k::CYAN, 1.0, 3.0);
        gp::line(c, x + w - 43.0, 22.0, x + w - 31.0, 8.0, k::CYAN, 1.0, 3.0);
    }
    if let Some(cmp) = v.comparison.filter(|c0| *c0 == ev("ItemComparisonState", "Better") || *c0 == ev("ItemComparisonState", "Worse")) {
        let up = cmp == ev("ItemComparisonState", "Better");
        let (sign, cc) = (if up { -1.0 } else { 1.0 }, if up { [0.2, 0.95, 0.5] } else { k::RED });
        gp::line(c, x + w - 29.0, 40.0 - sign * 5.0, x + w - 22.0, 40.0 + sign * 3.0, cc, 1.0, 3.0);
        gp::line(c, x + w - 22.0, 40.0 + sign * 3.0, x + w - 15.0, 40.0 - sign * 5.0, cc, 1.0, 3.0);
    }
    let mut tx = x + 14.0;
    if weapon {
        let (scope, power) = (tdb("AttachmentSlots.Scope"), tdb("AttachmentSlots.PowerModule"));
        let mut att = Vec::new();
        for (i, part) in t.slots.iter().enumerate() {
            k::image(c, x + 14.0 + i as f64 * 18.0, 13.0, 11.0, 11.0, k::ATLAS_PHONE, "circle", Some(k::RED), 1.0);
            if part.item.is_none() {
                k::image(c, x + 17.0 + i as f64 * 18.0, 16.0, 5.0, 5.0, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
            }
            if part.id == scope || part.id == power {
                att.push(part.clone());
            }
        }
        if !v.melee {
            k::rect(c, tx, 155.0, 3.0, 15.0, k::CYAN, 1.0);
            k::rect(c, tx + 6.0, 155.0, 3.0, 15.0, k::CYAN, 1.0);
            k::text(c, tx + 15.0, 149.0, 65.0, 28.0, &if v.ammo > 999.0 { "999+".into() } else { gd::fmt_g(v.ammo) }, 22.0, k::CYAN, "left", false);
            tx += 78.0;
        }
        let tcol = if v.iconic { k::YELLOW } else { k::CYAN };
        if let Some(ti) = &v.type_icon {
            k::image_raw(c, tx, 150.0, 25.0, 25.0, ti.atlas, ti.part, Some(tcol), 1.0);
        }
        let na = att.len() as f64;
        let avail = x + w - 12.0 - na * 29.0 - (tx + 28.0);
        k::text(c, tx + 28.0, 149.0, avail, 28.0, &k::clip(&k::upper(&v.type_name), 7usize.max((avail / 10.0).floor() as usize)), 20.0, if v.iconic { k::YELLOW } else { k::GREY }, "left", false);
        for (i, part) in att.iter().enumerate() {
            let nm = rt::static_call("UIItemsHelper", "GetSlotShadowIcon", &[A::Name(part.id), A::N(ev("gamedataItemType", "Invalid")), A::N(ev("gamedataEquipmentArea", "Invalid"))]).map(|o| Val::of(&o).text()).unwrap_or_default();
            if let Some(icon) = gd::image(tdb(&nm)) {
                let on = part.item.is_some();
                k::image_raw(c, x + w - 12.0 - na * 29.0 + i as f64 * 29.0, 149.0, 25.0, 25.0, icon.atlas, icon.part, Some(if on { k::CYAN } else { k::GREY }), if on { 1.0 } else { 0.4 });
            }
        }
    } else {
        k::text(c, tx, 149.0, 275.0, 28.0, &k::clip(&v.type_name, 25), 21.0, k::GREY, "left", false);
        k::text(c, x + w - 95.0, 149.0, 80.0, 28.0, &if v.quantity > 1.0 { format!("x{}", gd::fmt_g(v.quantity)) } else { String::new() }, 22.0, k::CYAN, "right", false);
    }
    let (key, key2, side) = (v.key.clone(), v.key.clone(), t.side);
    let b = button(
        m,
        "p1",
        &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
        x,
        4.0,
        w,
        h,
        Some(Rc::new(move |m: &mut Menu, _| {
            let s = &mut m.gp.stash;
            (s.selected, s.quantity, s.confirm) = (Some(key.clone()), 1.0, None);
            if side == 0 {
                if let (Some(ctx), Some(mut it)) = (s.ctx.as_ref(), s.snapshot.as_ref().and_then(|sn| sn.by_key.get(&key)).map(|t| t.item.clone())) {
                    idt::inspect(ctx, &mut it);
                }
            }
            gp::redraw(m);
        })),
        Some(Rc::new(move |on, _| {
            ink::tint(&bg, if on { k::YELLOW } else { col });
            ink::opacity(&bg, if on { 0.13 } else { 0.045 });
            k::tint(&edge, if on || sel { k::YELLOW } else { k::CYAN }, Some(if on { 1.0 } else { 0.35 }));
            hover(&key2, on);
        })),
    );
    *b.key.borrow_mut() = Some(format!("stashItem:{}", v.key));
}
fn sorted(list: &[Thing], filter: &str, order: &str) -> Vec<Thing> {
    let filt: Vec<Thing> = list.iter().filter(|t| filter == "all" || gd::fmt_g(category(t)) == filter).cloned().collect();
    let mut by: HashMap<String, Thing> = filt.iter().map(|t| (t.item.key.clone(), t.clone())).collect();
    let items: Vec<Item> = filt.into_iter().map(|mut t| {
        t.item.price = None;
        t.item
    }).collect();
    idt::sort_apply(items, Some(order)).into_iter().filter_map(|it| by.remove(&it.key)).collect()
}
pub(super) fn draw(m: &mut Menu, p: &Handle, _p2: &Handle) {
    ensure(m);
    NOW.with(|c| c.set(m.time));
    m.gp.stash.tips.clear();
    m.gp.stash.shown = None;
    HOVER.with(|h| *h.borrow_mut() = None);
    glass(m, p, W, H, Some((P1X, P1Y)));
    gp::title(p, &tr("ТАЙНИК ВИ", "V'S STASH"), W);
    let Some((object, manager)) = super::menu_vendor::stash_parts().filter(|_| super::menu_vendor::stash_current_m(m)) else {
        k::text(p, 30.0, 210.0, W - 60.0, 160.0, &tr("Откройте тайник, чтобы перемещать предметы.", "Open your stash to transfer items."), 34.0, k::CYAN, "center", false);
        return;
    };
    if m.gp.stash.snapshot.is_none() {
        let snap = snapshot(m.gp.stash.ctx.as_ref().expect("ctx"), &object, &manager);
        m.gp.stash.snapshot = Some(Rc::new(snap));
    }
    m.gp.stash.stamp = signature(&object);
    m.gp.stash.poll = m.time;
    let cmp = m.gp.stash.comparison;
    gp::comparison(m, p, H, cmp, Rc::new(|m: &mut Menu, _| {
        m.gp.stash.comparison = !m.gp.stash.comparison;
        gp::redraw(m);
    }), "stashComparison");
    k::rect(p, 882.0, 110.0, 1.0, 910.0, k::CYAN, 0.22);
    let snap = m.gp.stash.snapshot.clone().expect("snapshot");
    for side in 0..2 {
        let x = if side == 0 { 12.0 } else { 908.0 };
        let list = &snap.sides[side];
        let cats: HashSet<String> = list.iter().map(|t| gd::fmt_g(category(t))).collect();
        if m.gp.stash.filters[side] != "all" && !cats.contains(&m.gp.stash.filters[side]) {
            m.gp.stash.filters[side] = "all".into();
        }
        let shown = sorted(list, &m.gp.stash.filters[side].clone(), &m.gp.stash.sorts[side].clone());
        let title = if side == 0 { tr("СНАРЯЖЕНИЕ ВИ", "V'S EQUIPMENT") } else { tr("ХРАНИЛИЩЕ", "STORAGE") };
        k::text(p, x + 12.0, 105.0, 810.0, 54.0, &format!("{title}  /  {}", list.len()), 32.0, k::CYAN, "left", false);
        let caption = k::text(p, x + 12.0, 235.0, 450.0, 32.0, &tr("ВСЕ ПРЕДМЕТЫ", "ALL ITEMS"), 23.0, k::GREY, "left", false);
        let mut nn = 0.0;
        for name in FILTERS {
            let fval = ev("ItemFilterCategory", name);
            let key = if name == "AllItems" { "all".to_string() } else { gd::fmt_g(fval) };
            if key != "all" && !cats.contains(&key) {
                continue;
            }
            let xx = x + 10.0 + nn * 73.0;
            nn += 1.0;
            let on = m.gp.stash.filters[side] == key;
            let lbl = idt::filter_label(name);
            if on {
                ink::set_text(&caption, &lbl);
            }
            let bg = k::round_fill(p, xx, 169.0, 66.0, 56.0, if on { k::YELLOW } else { k::NAVY }, if on { 0.12 } else { 0.18 }, Some(3.0));
            if let Some(icon) = gd::image(tdb(&idt::filter_icon(name))) {
                k::image_raw(p, xx + 9.0, 174.0, 48.0, 46.0, icon.atlas, icon.part, Some(if on { k::YELLOW } else { k::CYAN }), 1.0);
            }
            let cap = caption.clone();
            let b = button(
                m,
                "p1",
                &at(P1X, P1Y),
                xx,
                169.0,
                66.0,
                56.0,
                Some(Rc::new(move |m: &mut Menu, _| {
                    let s = &mut m.gp.stash;
                    (s.selected, s.confirm, s.reset_side) = (None, None, Some(side));
                    s.filters[side] = key.clone();
                    gp::redraw(m);
                })),
                Some(Rc::new(move |h, _| {
                    ink::tint(&bg, if h || on { k::YELLOW } else { k::NAVY });
                    ink::opacity(&bg, if h { 0.18 } else if on { 0.12 } else { 0.18 });
                    if h {
                        ink::set_text(&cap, &lbl);
                    }
                })),
            );
            *b.key.borrow_mut() = Some(format!("stashFilter:{}:{name}", if side == 0 { "deposit" } else { "withdraw" }));
        }
        let mut popup = m.gp.stash.sort_popup.take();
        let value = m.gp.stash.sorts[side].clone();
        iv::sort_control(
            m,
            p,
            &mut popup,
            &format!("stashSort:{}", if side == 0 { "deposit" } else { "withdraw" }),
            x + 528.0,
            232.0,
            244.0,
            Some(&value),
            Rc::new(move |m: &mut Menu, v: String| {
                let s = &mut m.gp.stash;
                (s.selected, s.confirm, s.reset_side) = (None, None, Some(side));
                s.sorts[side] = v;
            }),
            Rc::new(|m: &mut Menu| &mut m.gp.stash.sort_popup),
        );
        m.gp.stash.sort_popup = popup;
        let mut rows = Vec::new();
        for pair in shown.chunks(2) {
            let pair = pair.to_vec();
            rows.push(Row {
                h: 190.0,
                build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                    for (i, t) in pair.iter().enumerate() {
                        card(m, c, &it, t, i as f64 * 404.0);
                    }
                }),
            });
        }
        if rows.is_empty() {
            rows.push(Row { h: 130.0, build: Rc::new(|_m: &mut Menu, c: &Handle, _| {
                k::text(c, 10.0, 20.0, 790.0, 90.0, &tr("Нет предметов в этой категории", "No items in this category"), 29.0, k::GREY, "center", false);
            }) });
        }
        let host = ink::canvas(p, x, 0.0, 850.0, 1100.0);
        let keep = m.gp.stash.offsets.as_ref().and_then(|o| o[side]);
        let v = gp::glist(m, &host, "p1", if side == 0 { "p1" } else { "p2" }, P1X + x, 320.0, 850.0, rows, 645.0, keep);
        v.borrow_mut().enable_mask();
    }
    let selected = m.gp.stash.selected.clone().and_then(|k0| snap.by_key.get(&k0).cloned());
    match selected {
        Some(sel) => {
            let q = quote(&object, &sel, m.gp.stash.quantity);
            k::text(p, 26.0, 1037.0, 1710.0, 47.0, &k::clip(&sel.item.name, 78), 32.0, td::col_of(&sel.item.quality), "left", false);
            let qty = m.gp.stash.quantity;
            if q.max > 1.0 {
                let max = q.max;
                td::action(m, p, 24.0, 1110.0, 90.0, "−", "−", Rc::new(|m: &mut Menu, _| {
                    let s = &mut m.gp.stash;
                    (s.quantity, s.confirm) = ((s.quantity - 1.0).max(1.0), None);
                    gp::redraw(m);
                }), qty > 1.0, "stashQuantityMinus");
                k::text(p, 125.0, 1110.0, 150.0, 68.0, &gd::fmt_g(qty), 32.0, k::CYAN, "center", false);
                td::action(m, p, 286.0, 1110.0, 90.0, "+", "+", Rc::new(move |m: &mut Menu, _| {
                    let s = &mut m.gp.stash;
                    (s.quantity, s.confirm) = ((s.quantity + 1.0).min(max), None);
                    gp::redraw(m);
                }), qty < max, "stashQuantityPlus");
                td::action(m, p, 392.0, 1110.0, 180.0, "МАКС.", "MAX", Rc::new(move |m: &mut Menu, _| {
                    let s = &mut m.gp.stash;
                    (s.quantity, s.confirm) = (max, None);
                    gp::redraw(m);
                }), true, "stashQuantityMax");
            }
            let confirming = m.gp.stash.confirm.is_some();
            let (ru, en) = if confirming { ("ПОДТВЕРДИТЬ ПЕРЕНОС", "CONFIRM TRANSFER") } else if sel.side == 0 { ("ПОЛОЖИТЬ В ТАЙНИК →", "STORE →") } else { ("← ЗАБРАТЬ", "← TAKE") };
            let (s2, obj) = (sel.clone(), object.clone());
            let qmax = q.max;
            let b = td::action(
                m,
                p,
                900.0,
                1110.0,
                850.0,
                ru,
                en,
                Rc::new(move |m: &mut Menu, _| {
                    if m.gp.stash.pending.is_some() {
                        return;
                    }
                    let confirmed = m.gp.stash.confirm.as_deref() == Some(s2.item.key.as_str());
                    let qty = m.gp.stash.quantity;
                    let q = quote(&obj, &s2, qty);
                    if !q.allowed {
                        let t = m.tx("blocked");
                        toast(m, &t);
                    } else if q.confirm && !confirmed {
                        m.gp.stash.confirm = Some(s2.item.key.clone());
                        gp::redraw(m);
                    } else {
                        if let Some((_, mgr)) = super::menu_vendor::stash_parts() {
                            rt::call(&mgr, "TransferItem", &[A::H(&q.source), A::H(&q.target), A::H(&q.raw), A::N(qty.floor().max(1.0))]);
                        }
                        m.gp.stash.confirm = None;
                        m.gp.stash.pending = Some((m.time, s2.item.id, s2.side, qmax, qty));
                        if let Some(a) = &m.gp.stash.apply {
                            a.visible.set(false);
                        }
                        let t = tr("Перемещение…", "Transferring…");
                        toast(m, &t);
                    }
                }),
                q.allowed && m.gp.stash.pending.is_none(),
                "stashApply",
            );
            m.gp.stash.apply = Some(b);
            if confirming {
                k::text(p, 585.0, 1100.0, 300.0, 100.0, &tr("Экипированный предмет будет снят", "The equipped item will be unequipped"), 24.0, k::YELLOW, "left", true);
            } else if !q.allowed {
                k::text(p, 24.0, 1100.0, 820.0, 100.0, &tr("Этот предмет сейчас нельзя переместить", "This item cannot be transferred right now"), 26.0, k::RED, "left", true);
            }
        }
        None => {
            k::text(p, 30.0, 1080.0, W - 60.0, 100.0, &tr("Выберите предмет, чтобы положить его в тайник или забрать", "Select an item to store or take"), 31.0, k::GREY, "center", false);
        }
    }
    let (host, block, plates) = td::tip_layer(m, p, 900.0, 302.0, 850.0, 730.0);
    let s = &mut m.gp.stash;
    (s.tip_host, s.block, s.plates) = (host, Some(block), Some(plates));
    let popup = m.gp.stash.sort_popup.clone();
    iv::sort_popup(m, p, &popup, Rc::new(|m: &mut Menu| &mut m.gp.stash.sort_popup));
}
pub(super) fn update(m: &mut Menu) {
    NOW.with(|c| c.set(m.time));
    if !m.gp.stash.started {
        return;
    }
    let Some((object, manager)) = super::menu_vendor::stash_parts().filter(|_| super::menu_vendor::stash_current_m(m)) else {
        super::menu_main::open_section(m, Some("inventory"), false);
        return;
    };
    iv::tip_fit(&mut m.gp.stash.tips);
    if let Some(plates) = &m.gp.stash.plates {
        iv::set_plates(plates, &m.gp.stash.tips);
    }
    let key = hover_key().or_else(|| m.gp.stash.selected.clone());
    if !m.gp.stash.tip_host.is_null() && Some(&key) != m.gp.stash.shown.as_ref() && m.time - HOVER_AT.with(|c| c.get()) > 0.12 {
        let host = m.gp.stash.tip_host.clone();
        ink::remove_all(&host);
        m.gp.stash.tips.clear();
        m.gp.stash.shown = Some(key.clone());
        td::hide_layer(&m.gp.stash.block);
        let t = key.as_ref().and_then(|k0| m.gp.stash.snapshot.as_ref().and_then(|s| s.by_key.get(k0).cloned()));
        if let Some(t) = t {
            let x = if t.side == 0 { 912.0 } else { 24.0 };
            let resolver = so("InventoryItemPreferredComparisonResolver", "Make", &[A::H(&idt::ui_inventory())]);
            let comparison = if m.gp.stash.comparison { o(&resolver, "GetPreferredComparisonItem", &[A::H(&t.item.ui)]) } else { Handle::null() };
            if !comparison.is_null() && id16(&comparison, "GetID", &[]) != t.item.id {
                if let Some(tp) = iv::tip_spawn(m, &host, &t.item.ui, "VendorPlayer", x + 8.0, 310.0, 400.0, 700.0, Some(&comparison), None) {
                    m.gp.stash.tips.push(tp);
                }
                if let Some(tp) = iv::tip_spawn(m, &host, &comparison, "VendorPlayer", x + 425.0, 310.0, 400.0, 700.0, Some(&t.item.ui), None) {
                    m.gp.stash.tips.push(tp);
                }
            } else if let Some(tp) = iv::tip_spawn(m, &host, &t.item.ui, "VendorPlayer", x + 12.0, 310.0, 810.0, 700.0, None, None) {
                m.gp.stash.tips.push(tp);
            }
            td::show_layer(&m.gp.stash.block, x);
        }
    }
    if m.time - m.gp.stash.poll < 0.5 {
        return;
    }
    m.gp.stash.poll = m.time;
    let stamp = signature(&object);
    if stamp != m.gp.stash.stamp || m.gp.stash.pending.is_some_and(|p0| m.time - p0.0 > 3.0) {
        if let Some((_, id, side, before, amount)) = m.gp.stash.pending {
            let owner = if side == 0 { player() } else { object.clone() };
            let count = n(&super::game_system("GetTransactionSystem"), "GetItemQuantity", &[A::H(&owner), id.a()]);
            let t = if count == before - amount {
                m.gp.stash.selected = None;
                tr("Предмет перемещён", "Item transferred")
            } else {
                tr("Перенос не выполнен", "Transfer did not complete")
            };
            toast(m, &t);
        }
        let s = &mut m.gp.stash;
        (s.pending, s.confirm) = (None, None);
        if let Some(c) = s.ctx.as_mut() {
            idt::invalidate(c);
        }
        let snap = snapshot(s.ctx.as_ref().expect("ctx"), &object, &manager);
        match s.selected.as_ref().and_then(|k0| snap.by_key.get(k0)) {
            Some(sel) => s.quantity = s.quantity.min(sel.item.quantity),
            None => (s.selected, s.quantity) = (None, 1.0),
        }
        s.snapshot = Some(Rc::new(snap));
        gp::redraw(m);
    }
}
pub(super) fn before_build(m: &mut Menu) {
    if !m.gp.stash.started {
        return;
    }
    let offs = [m.view("p1").map(|v| v.v.borrow().offset), m.view("p2").map(|v| v.v.borrow().offset)];
    let s = &mut m.gp.stash;
    let mut o = offs;
    if let Some(side) = s.reset_side.take() {
        o[side] = Some(0.0);
    }
    s.offsets = Some(o);
    s.tip_host = Handle::null();
    s.tips.clear();
}
pub(super) fn leave(m: &mut Menu) {
    if let Some(c) = m.gp.stash.ctx.take() {
        idt::close(c);
    }
    m.gp.stash = StashState::default();
}
