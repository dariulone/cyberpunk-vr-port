//! inventory_view.lua with wardrobe_view.lua and backpack_view.lua: the equipment page (the three weapon slots -- the
//! first one's left-hand partner with the dual-wield perk --, the character preview, the clothing slots, the holster
//! badge of each weapon slot and its row of places, the model and holster switches), a slot's picker (the equipped
//! item, a weapon's attachment slots, the filters, the game's sort order, the grid of items, EQUIP / INSTALL and the
//! hold actions), the game's own tooltip of the item pointed at; EquipmentEx's wardrobe; the backpack (the crafting
//! components, the junk, the game's filters and order, the selection mode's batches).
use super::lobby_ink as ink;
use super::menu_core::*;
use super::menu_gameplay::{self as gp};
use super::menu_gpdata as gd;
use super::menu_ink as k;
use super::menu_invdata::{self as idt, Item};
use super::menu_invview::{self as iv, SortPopup, Tip};
use super::menu_locale as loc;
use super::menu_rt::{cname, ev, tdb};
use red4ext_abi::handle::Handle;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const W: f64 = 1780.0;
const HOLSTER_ATLAS: &str = "vrport\\ui\\vr_holster_icons.inkatlas";
const HOLSTER_PARTS: &[&str] = &[
    "body", "body_w", "none", "chest_l_knife", "chest_r_knife", "hip_l_blade", "hip_l_gun", "hip_r_blade", "hip_r_gun", "lower_back_blade", "lower_back_knife", "lower_back_long", "lower_back_long_w", "shoulder_l_blade", "shoulder_l_blade_w", "shoulder_l_long", "shoulder_l_long_w", "shoulder_r_blade", "shoulder_r_blade_w", "shoulder_r_long", "shoulder_r_long_w",
    "thigh_l_gun", "thigh_l_knife", "thigh_r_gun", "thigh_r_knife", "underarm_l_gun", "underarm_r_gun",
];
fn place_name(p: &str) -> (&'static str, &'static str) {
    match p {
        "auto" => ("АВТО", "AUTO"),
        "none" => ("НЕ\nНОСИТЬ", "NOT\nCARRIED"),
        "hip_r" => ("ПРАВОЕ\nБЕДРО", "RIGHT\nHIP"),
        "hip_l" => ("ЛЕВОЕ\nБЕДРО", "LEFT\nHIP"),
        "thigh_r" => ("ПРАВАЯ\nНОГА", "RIGHT\nTHIGH"),
        "thigh_l" => ("ЛЕВАЯ\nНОГА", "LEFT\nTHIGH"),
        "underarm_l" => ("ПОД ЛЕВОЙ\nРУКОЙ", "UNDER\nLEFT ARM"),
        "underarm_r" => ("ПОД ПРАВОЙ\nРУКОЙ", "UNDER\nRIGHT ARM"),
        "chest_l" => ("ГРУДЬ\nСЛЕВА", "LEFT\nCHEST"),
        "chest_r" => ("ГРУДЬ\nСПРАВА", "RIGHT\nCHEST"),
        "shoulder_r" => ("ЗА ПРАВЫМ\nПЛЕЧОМ", "RIGHT\nSHOULDER"),
        "shoulder_l" => ("ЗА ЛЕВЫМ\nПЛЕЧОМ", "LEFT\nSHOULDER"),
        "lower_back" => ("ПОЯСНИЦА", "LOWER\nBACK"),
        _ => ("", ""),
    }
}
fn has_part(p: &str) -> bool {
    HOLSTER_PARTS.contains(&p)
}
pub(super) fn quality_col(q: &str) -> [f64; 3] {
    k::quality(q)
}
fn tr(ru: &str, en: &str) -> String {
    loc::tr(ru, en)
}
fn label(area: &str) -> String {
    let (ru, en) = match area {
        "Weapon" => ("ОРУЖИЕ", "WEAPONS"),
        "Head" => ("ГОЛОВА", "HEAD"),
        "Face" => ("ЛИЦО", "FACE"),
        "OuterChest" => ("ВЕРХНЯЯ ОДЕЖДА", "OUTER TORSO"),
        "InnerChest" => ("НИЖНЯЯ ОДЕЖДА", "INNER TORSO"),
        "Legs" => ("НОГИ", "LEGS"),
        "Feet" => ("ОБУВЬ", "FEET"),
        "Outfit" => ("КОМПЛЕКТ ОДЕЖДЫ", "OUTFIT"),
        _ => return area.to_string(),
    };
    tr(ru, en)
}
/// The game's own word where it has one.
fn gloc(key: &str, ru: &str, en: &str) -> String {
    let t = super::menu_rt::loc(key);
    if !t.is_empty() && t != key {
        t
    } else {
        tr(ru, en)
    }
}
thread_local! {
    /// The item pointed at and when (a paint has no menu): the tooltip shows after a short rest.
    static HOVER: RefCell<Option<String>> = const { RefCell::new(None) };
    static HOVER_AT: Cell<f64> = const { Cell::new(0.0) };
    static NOW: Cell<f64> = const { Cell::new(0.0) };
}
fn hover_cb(key: String) -> Rc<dyn Fn(bool)> {
    Rc::new(move |on| {
        let now = NOW.with(|c| c.get());
        HOVER.with(|h| {
            let mut h = h.borrow_mut();
            if on {
                *h = Some(key.clone());
                HOVER_AT.with(|c| c.set(now));
            } else if h.as_deref() == Some(key.as_str()) {
                *h = None;
                HOVER_AT.with(|c| c.set(now));
            }
        });
    })
}
fn hover_key() -> Option<String> {
    HOVER.with(|h| h.borrow().clone())
}

#[derive(Default)]
pub(super) struct InvS {
    pub slot: i32,
    pub weapon_slot: i32,
    pub filter: String,
    pub area: Option<String>,
    pub part: Option<String>,
    pub selection: Option<String>,
    pub confirm: Option<String>,
    pub holster_pick: Option<i32>,
    pub sort: Option<String>,
    pub sort_popup: Option<SortPopup>,
    pub wardrobe: bool,
    pub wardrobe_delete: Option<String>,
    pub backpack: bool,
    pub disarm: Option<String>,
}
#[derive(Default)]
pub(super) struct Bp {
    filter: String,
    sort: String,
    quantity: f64,
    selected: Option<String>,
    confirm: Option<String>,
    multi: bool,
    marked: HashSet<String>,
    batch_armed: Option<(String, f64)>,
    junk_armed: Option<f64>,
    refresh_at: Option<f64>,
    sorted: Option<Vec<Item>>,
    sorted_key: String,
    sorted_seq: Option<u64>,
    snap_seq: Option<u64>,
    sig_seq: Option<u64>,
    sort_popup: Option<SortPopup>,
    col_of: HashMap<String, usize>,
    tips: Vec<Tip>,
    shown: Option<Option<String>>,
    tip_host: Handle,
    block: Option<BtnRef>,
    plates: Option<iv::Plates>,
}
/// The inventory's state (gameplay.lua's s.inventory, s.inventoryContext and the tooltip / preview keys).
#[derive(Default)]
pub(super) struct InvState {
    pub s: InvS,
    pub ctx: Option<idt::Ctx>,
    pub comparison: Option<Item>,
    pub refresh_at: Option<f64>,
    pub seq: Option<u64>,
    pub model: bool,
    pub preview: iv::Preview,
    tip_host: Handle,
    tips: Vec<Tip>,
    tip_shown: Option<Option<String>>,
    tip_rect: Option<[f64; 4]>,
    tip_block: Option<BtnRef>,
    tip_plates: Option<iv::Plates>,
    by_key: HashMap<String, Item>,
    equipped: Option<Item>,
    bp: Bp,
    started: bool,
}
fn ensure(m: &mut Menu) {
    let inv = &mut m.gp.inv;
    if !inv.started {
        inv.started = true;
        inv.s.filter = "all".into();
        inv.model = idt::character_model();
    }
    if inv.ctx.is_none() {
        inv.ctx = Some(idt::open());
    }
}
fn ctx(m: &mut Menu) -> &mut idt::Ctx {
    ensure(m);
    m.gp.inv.ctx.as_mut().expect("inventory context")
}
fn redraw_reset(m: &mut Menu) {
    m.gp.reset_list = true;
    gp::redraw(m);
}
fn pending(m: &mut Menu, ok: bool) {
    if ok {
        m.gp.inv.refresh_at = Some(m.time + 0.3);
        m.gp.inv.preview.dirty = true;
    } else {
        let t = tr("Сейчас недоступно", "Not available right now");
        toast(m, &t);
    }
}
#[allow(clippy::too_many_arguments)]
fn action(m: &mut Menu, p: &Handle, x: f64, y: f64, w: f64, ru: &str, en: &str, f: Act, enabled: bool, key: &str) -> BtnRef {
    let b = gp::hub_action(m, p, P1X, x, y, w, &tr(ru, en), f, enabled, None, None);
    *b.key.borrow_mut() = Some(key.to_string());
    b
}
fn tick(p: &Handle, x: f64, y: f64) {
    gp::line(p, x, y, x + 8.0, y + 8.0, k::CYAN, 1.0, 3.0);
    gp::line(p, x + 8.0, y + 8.0, x + 20.0, y - 6.0, k::CYAN, 1.0, 3.0);
}
/// tile: an item's tile (its picture, name, quality, new / equipped marks; a weapon's attachment dots, comparison,
/// ammo, type), or an empty slot's caption.
#[allow(clippy::too_many_arguments)]
pub(super) fn tile(m: &mut Menu, p: &Handle, hh: &At, x: f64, y: f64, w: f64, h: f64, item: Option<&Item>, caption: Option<&str>, f: Act, selected: bool, key: &str, on_hover: Option<Rc<dyn Fn(bool)>>) -> BtnRef {
    let col = item.map_or(k::CYAN, |i| quality_col(&i.quality));
    let idle = 0.045;
    let bg = k::round_fill(p, x, y, w, h, if selected { k::YELLOW } else { col }, if selected { 0.12 } else { idle }, Some(4.0));
    let edge = k::cut_frame(&mut m.pool, p, x, y, w, h, if selected { k::YELLOW } else { k::CYAN }, if selected { 1.0 } else { 0.35 }, Some(10.0), Some(1.0));
    k::rect(p, x + 2.0, y + 5.0, 6.0, h - 10.0, col, 1.0);
    match item {
        Some(it) => {
            let weapon = it.area == idt::area("Weapon");
            gp::picture(m, p, x + 14.0, y + if weapon { 28.0 } else { 8.0 }, w - 28.0, h - if weapon { 96.0 } else { 61.0 }, it.image.as_ref(), None);
            k::text(p, x + 14.0, y + h - if weapon { 61.0 } else { 50.0 }, w - 28.0, if weapon { 31.0 } else { 44.0 }, &k::clip(&it.name, 9usize.max(((w - 28.0) / 13.0).floor() as usize)), if weapon { 23.0 } else { 25.0 }, col, "center", false);
            if it.new {
                k::image(p, x + w - 22.0, y + 10.0, 10.0, 10.0, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
            }
            if it.equipped {
                gp::line(p, x + w - 63.0, y + 15.0, x + w - 55.0, y + 23.0, k::CYAN, 1.0, 3.0);
                gp::line(p, x + w - 55.0, y + 23.0, x + w - 43.0, y + 9.0, k::CYAN, 1.0, 3.0);
            }
            if weapon {
                let comparison = m.gp.inv.comparison.clone();
                let c = ctx(m);
                let parts = idt::slots(c, Some(it));
                let ox = if key.starts_with("weaponSlot") { 48.0 } else { 16.0 };
                let mut attachments = Vec::new();
                let (scope, power) = (tdb("AttachmentSlots.Scope"), tdb("AttachmentSlots.PowerModule"));
                for (i, part) in parts.iter().enumerate() {
                    let px = x + ox + i as f64 * 19.0;
                    k::image(p, px, y + 12.0, 12.0, 12.0, k::ATLAS_PHONE, "circle", Some(k::RED), 1.0);
                    if part.item.is_none() {
                        k::image(p, px + 3.0, y + 15.0, 6.0, 6.0, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
                    }
                    if part.id == scope || part.id == power {
                        attachments.push(part.clone());
                    }
                }
                let cmp = idt::compare(c, it, comparison.as_ref());
                if cmp == ev("ItemComparisonState", "Better") || cmp == ev("ItemComparisonState", "Worse") {
                    let up = cmp == ev("ItemComparisonState", "Better");
                    let cc = if up { [0.2, 0.95, 0.5] } else { k::RED };
                    let (cy, sign) = (y + 40.0, if up { -1.0 } else { 1.0 });
                    gp::line(p, x + w - 30.0, cy - sign * 5.0, x + w - 23.0, cy + sign * 3.0, cc, 1.0, 3.0);
                    gp::line(p, x + w - 23.0, cy + sign * 3.0, x + w - 16.0, cy - sign * 5.0, cc, 1.0, 3.0);
                }
                let mut tx = x + 15.0;
                if !it.melee {
                    k::rect(p, tx, y + h - 25.0, 3.0, 14.0, k::CYAN, 1.0);
                    k::rect(p, tx + 6.0, y + h - 25.0, 3.0, 14.0, k::CYAN, 1.0);
                    k::text(p, tx + 16.0, y + h - 31.0, 80.0, 28.0, &gd::fmt_g(it.ammo), 23.0, k::CYAN, "left", false);
                    tx += 94.0;
                }
                let tcol = if it.iconic { k::YELLOW } else { k::CYAN };
                if let Some(ti) = &it.type_icon {
                    k::image_raw(p, tx, y + h - 29.0, 28.0, 25.0, ti.atlas, ti.part, Some(tcol), 1.0);
                }
                if w < 200.0 {
                    attachments.clear();
                }
                let na = attachments.len() as f64;
                let available = x + w - 14.0 - na * 34.0 - (tx + 32.0);
                if available >= 40.0 {
                    k::text(p, tx + 32.0, y + h - 31.0, available, 28.0, &k::clip(&k::upper(&it.type_name), 8usize.max((available / 11.0).floor() as usize)), 21.0, tcol, "left", false);
                }
                for (i, part) in attachments.iter().enumerate() {
                    let n = super::lobby_rtti::static_call("UIItemsHelper", "GetSlotShadowIcon", &[super::lobby_rtti::A::Name(part.id), super::lobby_rtti::A::N(ev("gamedataItemType", "Invalid")), super::lobby_rtti::A::N(ev("gamedataEquipmentArea", "Invalid"))]).map(|o| super::lobby_rtti::Val::of(&o).text()).unwrap_or_default();
                    if let Some(icon) = gd::image(tdb(&n)) {
                        let on = part.item.is_some();
                        k::image_raw(p, x + w - 14.0 - na * 34.0 + i as f64 * 34.0, y + h - 31.0, 30.0, 28.0, icon.atlas, icon.part, Some(if on { k::CYAN } else { k::RED }), if on { 1.0 } else { 0.5 });
                    }
                }
            } else if it.iconic {
                k::text(p, x + 17.0, y + 5.0, 32.0, 34.0, "◆", 25.0, k::YELLOW, "center", false);
            }
        }
        None => {
            k::text(p, x + 10.0, y + 15.0, w - 20.0, h - 30.0, caption.unwrap_or("+"), 30.0, k::GREY, "center", false);
        }
    }
    let b = button(
        m,
        "p1",
        hh,
        x,
        y,
        w,
        h,
        Some(f),
        Some(Rc::new(move |hover, _| {
            ink::tint(&bg, if hover || selected { k::YELLOW } else { col });
            ink::opacity(&bg, if hover { 0.16 } else if selected { 0.12 } else { idle });
            k::tint(&edge, if hover || selected { k::YELLOW } else { k::CYAN }, Some(if hover { 1.0 } else if selected { 0.85 } else { 0.35 }));
            if let Some(cb) = &on_hover {
                cb(hover);
            }
        })),
    );
    *b.key.borrow_mut() = Some(key.to_string());
    b
}

// ---------------------------------------------------------------- the holster badges
fn figure_suffix() -> &'static str {
    if super::menu_rt::id(&super::menu_rt::player(), "GetResolvedGenderName", &[]) == cname("Female") {
        "_w"
    } else {
        ""
    }
}
fn place_icon(p: &Handle, x: f64, y: f64, size: f64, place: Option<&str>, family: &str, col: [f64; 3], op: f64) {
    let w = figure_suffix();
    let body = if has_part(&format!("body{w}")) { format!("body{w}") } else { "body".into() };
    k::image(p, x, y, size, size, HOLSTER_ATLAS, &body, Some(k::CYAN), op * 0.6);
    match place {
        Some("none") => {
            k::image(p, x, y, size, size, HOLSTER_ATLAS, "none", Some(k::RED), op);
        }
        Some(pl) => {
            let mut part = format!("{pl}_{family}");
            if has_part(&format!("{part}{w}")) {
                part = format!("{part}{w}");
            }
            if has_part(&part) {
                k::image(p, x, y, size, size, HOLSTER_ATLAS, &part, Some(col), op);
            }
        }
        None => {}
    }
}
fn holster_badge(m: &mut Menu, p: &Handle, origin: &At, x: f64, y: f64, slot: i32) {
    let Some(info) = super::holster::slot_info(slot) else { return };
    let place = if info.choice == "none" { Some("none".to_string()) } else { info.place.clone() };
    let bg = k::round_fill(p, x, y, 84.0, 84.0, k::BLACK, 0.62, Some(6.0));
    let edge = k::cut_frame(&mut m.pool, p, x, y, 84.0, 84.0, k::CYAN, 0.4, Some(7.0), Some(1.0));
    if info.pair {
        place_icon(p, x + 4.0, y + 4.0, 76.0, info.place.as_deref(), info.family, k::YELLOW, 1.0);
        let mut other = format!("{}_{}", if info.place.as_deref() == Some("hip_r") { "hip_l" } else { "hip_r" }, info.family);
        if has_part(&format!("{other}{}", figure_suffix())) {
            other = format!("{other}{}", figure_suffix());
        }
        if has_part(&other) {
            k::image(p, x + 4.0, y + 4.0, 76.0, 76.0, HOLSTER_ATLAS, &other, Some(k::YELLOW), 1.0);
        }
        k::text(p, x + 4.0, y + 60.0, 74.0, 22.0, "x2", 18.0, k::CYAN, "right", false);
    } else {
        place_icon(p, x + 4.0, y + 4.0, 76.0, place.as_deref(), info.family, k::YELLOW, 1.0);
        if info.choice == "auto" {
            k::text(p, x + 4.0, y + 60.0, 74.0, 22.0, "A", 18.0, k::CYAN, "right", false);
        }
    }
    let pair = info.pair;
    let b = button(
        m,
        "p1",
        origin,
        x,
        y,
        84.0,
        84.0,
        Some(Rc::new(move |m: &mut Menu, _| {
            if pair {
                let t = tr("Две пушки висят на бёдрах", "Dual wielding: both guns hang on the hips");
                toast(m, &t);
                return;
            }
            m.gp.inv.s.holster_pick = Some(slot);
            gp::redraw(m);
        })),
        Some(Rc::new(move |h, _| {
            k::tint(&edge, if h { k::YELLOW } else { k::CYAN }, Some(if h { 1.0 } else { 0.4 }));
            ink::opacity(&bg, if h { 0.8 } else { 0.62 });
        })),
    );
    *b.key.borrow_mut() = Some(format!("holsterSlot{slot}"));
}
fn holster_picker(m: &mut Menu, p: &Handle, origin: &At, x: f64, y: f64, w: f64, h: f64, slot: i32) {
    let Some(info) = super::holster::slot_info(slot) else {
        m.gp.inv.s.holster_pick = None;
        return;
    };
    let mut opts: Vec<(String, Option<String>, String)> = vec![("auto".into(), if info.choice == "auto" { info.place.clone() } else { info.auto.clone() }, "auto".into())];
    for (name, shown) in &info.choices {
        opts.push((name.to_string(), Some(shown.clone()), shown.clone()));
    }
    opts.push(("none".into(), Some("none".into()), "none".into()));
    k::round_fill(p, x, y, w, h, k::BLACK, 0.94, Some(6.0));
    k::cut_frame(&mut m.pool, p, x, y, w, h, k::YELLOW, 0.85, Some(10.0), Some(1.5));
    let b = button(
        m,
        "p1",
        origin,
        x,
        y,
        w,
        h,
        Some(Rc::new(|m: &mut Menu, _| {
            m.gp.inv.s.holster_pick = None;
            gp::redraw(m);
        })),
        None,
    );
    *b.key.borrow_mut() = Some("holsterPickClose".into());
    let cw = ((w - 16.0) / opts.len() as f64).floor();
    let size = 88f64.min(cw - 14.0);
    for (i, (name, shown, lab)) in opts.into_iter().enumerate() {
        let cx = x + 8.0 + i as f64 * cw;
        let selected = info.choice == name;
        let cell = k::round_fill(p, cx + 2.0, y + 10.0, cw - 4.0, h - 20.0, k::CYAN, 0.05, Some(4.0));
        if selected {
            k::cut_frame(&mut m.pool, p, cx + 2.0, y + 10.0, cw - 4.0, h - 20.0, k::YELLOW, 0.95, Some(8.0), Some(2.0));
        }
        place_icon(p, cx + ((cw - size) / 2.0).floor(), y + 16.0, size, shown.as_deref(), info.family, if selected { k::YELLOW } else { k::CYAN }, 1.0);
        let (ru, en) = place_name(&lab);
        let txt = if ru.is_empty() { lab.clone() } else { tr(ru, en) };
        k::text(p, cx + 2.0, y + 20.0 + size, cw - 4.0, h - 30.0 - size, &txt, 18.0, if selected { k::YELLOW } else { k::TEXT }, "center", false);
        let nm = name.clone();
        let b = button(
            m,
            "p1",
            origin,
            cx + 2.0,
            y + 10.0,
            cw - 4.0,
            h - 20.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                if super::holster::set_place(slot, &nm) {
                    m.gp.inv.s.holster_pick = None;
                    gp::redraw(m);
                } else {
                    let t = tr("Не удалось сохранить настройку", "Unable to save preference");
                    toast(m, &t);
                }
            })),
            Some(Rc::new(move |hv, _| {
                ink::tint(&cell, if hv { k::YELLOW } else { k::CYAN });
                ink::opacity(&cell, if hv { 0.16 } else { 0.05 });
            })),
        );
        *b.key.borrow_mut() = Some(format!("holsterPick:{name}"));
    }
}
fn choose(m: &mut Menu, area: &str, slot: i32) {
    let s = &mut m.gp.inv.s;
    (s.area, s.slot, s.part, s.selection, s.confirm, s.holster_pick) = (Some(area.into()), slot, None, None, None, None);
    s.filter = "all".into();
    if area == "Weapon" {
        s.weapon_slot = slot;
    }
    redraw_reset(m);
}
fn hover_tip(m: &mut Menu, item: Option<&Item>) -> Option<Rc<dyn Fn(bool)>> {
    let it = item?;
    m.gp.inv.by_key.insert(it.key.clone(), it.clone());
    Some(hover_cb(it.key.clone()))
}

// ---------------------------------------------------------------- the page
pub(super) fn draw(m: &mut Menu, p: &Handle, _p2: &Handle) {
    ensure(m);
    NOW.with(|c| c.set(m.time));
    glass(m, p, W, 1305.0, Some((P1X, P1Y)));
    if m.gp.inv.s.wardrobe && idt::wd_available() {
        m.gp.inv.comparison = None;
        return wardrobe(m, p);
    }
    m.gp.inv.s.wardrobe = false;
    {
        let inv = &mut m.gp.inv;
        inv.tip_host = Handle::null();
        inv.tips.clear();
        HOVER.with(|h| *h.borrow_mut() = None);
        inv.tip_shown = None;
        inv.tip_rect = None;
        (inv.tip_block, inv.tip_plates) = (None, None);
        inv.by_key.clear();
    }
    if m.gp.inv.s.backpack {
        m.gp.inv.comparison = None;
        iv::preview_release(m);
        return backpack(m, p);
    }
    gp::title(p, &tr("СНАРЯЖЕНИЕ", "INVENTORY"), W);
    let hol = m.gp.inv.s.area.is_none() && super::holster::menu_active();
    let model = m.gp.inv.model;
    action(
        m,
        p,
        24.0,
        1210.0,
        if hol { 300.0 } else { 404.0 },
        if model { "МОДЕЛЬ: ВКЛ" } else { "МОДЕЛЬ: ВЫКЛ" },
        if model { "MODEL: ON" } else { "MODEL: OFF" },
        Rc::new(move |m: &mut Menu, _| {
            if idt::set_character_model(!model) {
                m.gp.inv.model = !model;
                iv::preview_release(m);
                gp::redraw(m);
            } else {
                let t = tr("Не удалось сохранить настройку", "Unable to save preference");
                toast(m, &t);
            }
        }),
        true,
        "inventoryModelToggle",
    );
    if hol {
        let hidden = super::holster::hidden();
        let b = gp::hub_action(
            m,
            p,
            P1X,
            334.0,
            1210.0,
            300.0,
            &tr(if hidden { "ХОЛСТЕРЫ: СКРЫТЫ" } else { "ХОЛСТЕРЫ: ВИДНЫ" }, if hidden { "HOLSTERS: HIDDEN" } else { "HOLSTERS: SHOWN" }),
            Rc::new(move |m: &mut Menu, _| {
                if super::holster::set_hidden(!hidden) {
                    gp::redraw(m);
                } else {
                    let t = tr("Не удалось сохранить настройку", "Unable to save preference");
                    toast(m, &t);
                }
            }),
            true,
            Some(68.0),
            Some(26.0),
        );
        *b.key.borrow_mut() = Some("inventoryHolsterToggle".into());
    }
    m.gp.inv.comparison = None;
    let origin = at(P1X, P1Y);
    let Some(area) = m.gp.inv.s.area.clone() else {
        return main_page(m, p, &origin, hol);
    };
    picker(m, p, &origin, &area);
}
fn main_page(m: &mut Menu, p: &Handle, origin: &At, hol: bool) {
    k::text(p, 24.0, 107.0, 450.0, 45.0, &label("Weapon"), 32.0, k::WHITE, "left", false);
    let dual = idt::dual_wield();
    idt::clear_stray_left(ctx(m));
    for slot in 0..3 {
        let item = idt::equipped(ctx(m), "Weapon", slot);
        let paired = dual && slot == idt::DUAL_SLOT;
        let yy = 163.0 + f64::from(slot) * 222.0;
        let hv = hover_tip(m, item.as_ref());
        tile(m, p, origin, 24.0, yy, if paired { 300.0 } else { 454.0 }, 205.0, item.as_ref(), Some(&format!("{}{}", tr("СЛОТ ", "SLOT "), slot + 1)), Rc::new(move |m: &mut Menu, _| choose(m, "Weapon", slot)), false, &format!("weaponSlot{slot}"), hv);
        k::text(p, 36.0, 170.0 + f64::from(slot) * 222.0, 50.0, 34.0, &(slot + 1).to_string(), 27.0, k::CYAN, "left", false);
        if paired {
            let left = idt::equipped(ctx(m), "Weapon", slot + 3);
            let hv = hover_tip(m, left.as_ref());
            tile(m, p, origin, 334.0, yy, 144.0, 205.0, left.as_ref(), Some(&tr("ЛЕВАЯ", "LEFT")), Rc::new(move |m: &mut Menu, _| choose(m, "Weapon", slot + 3)), false, &format!("weaponSlotLeft{slot}"), hv);
            k::text(p, 344.0, 170.0 + f64::from(slot) * 222.0, 40.0, 34.0, &tr("Л", "L"), 27.0, k::CYAN, "left", false);
        }
    }
    iv::preview_show(m, p, 486.0, 100.0, 760.0, 1080.0, None);
    if hol {
        for slot in 0..3 {
            if idt::equipped(ctx(m), "Weapon", slot).is_some() {
                holster_badge(m, p, origin, 488.0, 163.0 + f64::from(slot) * 222.0 + 60.0, slot);
            }
        }
        if let Some(pick) = m.gp.inv.s.holster_pick {
            holster_picker(m, p, origin, 24.0, 163.0 + f64::from(pick) * 222.0, 760.0, 205.0, pick);
        }
    }
    for (i, area) in idt::CLOTHES.iter().enumerate() {
        let (x, y) = (1260.0 + (i % 2) as f64 * 250.0, 155.0 + (i / 2) as f64 * 258.0);
        k::text(p, x, y - 47.0, 234.0, 43.0, &label(area), 25.0, k::WHITE, "left", false);
        let worn = idt::equipped(ctx(m), area, 0);
        let hv = hover_tip(m, worn.as_ref());
        let a = *area;
        tile(m, p, origin, x, y, 234.0, 194.0, worn.as_ref(), Some("+"), Rc::new(move |m: &mut Menu, _| choose(m, a, 0)), false, &format!("clothingSlot{area}"), hv);
    }
    if idt::wd_available() {
        let b = gp::hub_action(
            m,
            p,
            P1X,
            1260.0,
            1210.0,
            484.0,
            &k::upper(&idt::wd_text("Gameplay-Devices-DisplayNames-Wardrobe")),
            Rc::new(|m: &mut Menu, _| {
                (m.gp.inv.s.wardrobe, m.gp.inv.s.wardrobe_delete) = (true, None);
                redraw_reset(m);
            }),
            true,
            None,
            None,
        );
        *b.key.borrow_mut() = Some("inventoryWardrobe".into());
    }
    let b = gp::hub_action(
        m,
        p,
        P1X,
        646.0,
        1210.0,
        440.0,
        &k::upper(&gloc("UI-PanelNames-BACKPACK", "Рюкзак", "Backpack")),
        Rc::new(|m: &mut Menu, _| {
            (m.gp.inv.s.backpack, m.gp.inv.s.area) = (true, None);
            redraw_reset(m);
        }),
        true,
        None,
        None,
    );
    *b.key.borrow_mut() = Some("inventoryBackpack".into());
    m.gp.inv.tip_rect = Some([490.0, 110.0, 752.0, 1070.0]);
    m.gp.inv.tip_host = ink::canvas(p, 0.0, 0.0, W, 1200.0);
    if idt::wardrobe() {
        k::text(p, 24.0, 922.0, 450.0, 110.0, &gp::wrap(&tr("Внешний вид задан комплектом гардероба", "Appearance is set by your wardrobe outfit"), 28).join("\n"), 28.0, k::YELLOW, "left", false);
        let enabled = idt::wardrobe_enabled(ctx(m));
        action(
            m,
            p,
            24.0,
            1060.0,
            454.0,
            "СНЯТЬ КОМПЛЕКТ",
            "REMOVE OUTFIT",
            Rc::new(|m: &mut Menu, _| {
                let ok = idt::clear_wardrobe(ctx(m));
                pending(m, ok);
            }),
            enabled,
            "wardrobeRemove",
        );
    }
}
fn picker(m: &mut Menu, p: &Handle, origin: &At, area: &str) {
    action(
        m,
        p,
        24.0,
        105.0,
        320.0,
        "К СНАРЯЖЕНИЮ",
        "EQUIPMENT",
        Rc::new(|m: &mut Menu, _| {
            let s = &mut m.gp.inv.s;
            (s.area, s.part, s.selection) = (None, None, None);
            redraw_reset(m);
        }),
        true,
        "inventoryBack",
    );
    let slot = m.gp.inv.s.slot;
    let left_slot = area == "Weapon" && slot >= 3;
    // The weapon slot being filled, said in words: "ОРУЖИЕ  /  1" read as a broken fraction.
    let slot_name = if left_slot {
        format!("{} {}, {}", tr("СЛОТ", "SLOT"), slot - 2, tr("ЛЕВАЯ РУКА", "LEFT HAND"))
    } else {
        format!("{} {}", tr("СЛОТ", "SLOT"), slot + 1)
    };
    k::text(p, 680.0, 110.0, 700.0, 58.0, &format!("{}{}", label(area), if area == "Weapon" { format!("  —  {slot_name}") } else { String::new() }), 34.0, k::CYAN, "left", false);
    let mut popup = m.gp.inv.s.sort_popup.take();
    let sort_value = m.gp.inv.s.sort.clone();
    iv::sort_control(
        m,
        p,
        &mut popup,
        "inventorySort",
        1448.0,
        112.0,
        280.0,
        sort_value.as_deref(),
        Rc::new(|m: &mut Menu, v: String| {
            let s = &mut m.gp.inv.s;
            (s.sort, s.selection, s.confirm) = (Some(v), None, None);
            m.gp.reset_list = true;
        }),
        Rc::new(|m: &mut Menu| &mut m.gp.inv.s.sort_popup),
    );
    m.gp.inv.s.sort_popup = popup;
    let equipped = idt::equipped(ctx(m), area, slot);
    m.gp.inv.comparison = equipped.clone();
    let parts = idt::slots(ctx(m), equipped.as_ref());
    let mut selected_slot: Option<idt::Slot> = None;
    if area == "Weapon" {
        iv::preview_release(m);
        let none_part = m.gp.inv.s.part.is_none();
        tile(
            m,
            p,
            origin,
            24.0,
            200.0,
            554.0,
            255.0,
            equipped.as_ref(),
            Some("+"),
            Rc::new(|m: &mut Menu, _| {
                (m.gp.inv.s.part, m.gp.inv.s.selection) = (None, None);
                redraw_reset(m);
            }),
            none_part,
            "equippedWeapon",
            None,
        );
        k::text(p, 24.0, 484.0, 554.0, 48.0, &tr("НАСАДКИ И МОДИФИКАЦИИ", "ATTACHMENTS AND MODS"), 29.0, k::WHITE, "left", false);
        if parts.is_empty() {
            k::text(p, 24.0, 553.0, 554.0, 110.0, &tr("У этого оружия нет доступных слотов", "This weapon has no available attachment slots"), 29.0, k::GREY, "left", true);
        }
        let (scope, power) = (tdb("AttachmentSlots.Scope"), tdb("AttachmentSlots.PowerModule"));
        for (i, part) in parts.iter().enumerate() {
            let (x, y) = (24.0 + (i % 3) as f64 * 188.0, 570.0 + (i / 3) as f64 * 205.0);
            let sel = m.gp.inv.s.part.as_deref() == Some(part.key.as_str());
            if sel {
                selected_slot = Some(part.clone());
            }
            let pk = part.key.clone();
            tile(
                m,
                p,
                origin,
                x,
                y,
                178.0,
                155.0,
                part.item.as_ref(),
                Some("+"),
                Rc::new(move |m: &mut Menu, _| {
                    let s = &mut m.gp.inv.s;
                    (s.part, s.selection, s.confirm) = (Some(pk.clone()), None, None);
                    redraw_reset(m);
                }),
                sel,
                &format!("attachmentSlot{}", i + 1),
                None,
            );
            let sl = if part.id == scope || part.id == power { part.name.clone() } else { tr("Модификация", "Mod") };
            k::text(p, x, y - 38.0, 178.0, 34.0, &k::clip(&sl, 15), 24.0, k::CYAN, "center", false);
        }
        if m.gp.inv.s.part.is_some() && selected_slot.is_none() {
            m.gp.inv.s.part = None;
        }
    } else {
        iv::preview_show(m, p, 0.0, 215.0, 640.0, 900.0, Some(area));
        if let Some(e) = &equipped {
            k::text(p, 25.0, 180.0, 550.0, 45.0, &k::clip(&e.name, 36), 29.0, quality_col(&e.quality), "center", false);
        }
        if idt::wardrobe() {
            k::text(p, 24.0, 1018.0, 554.0, 70.0, &tr("Внешний вид: комплект гардероба", "Appearance: wardrobe outfit"), 26.0, k::YELLOW, "left", false);
        }
    }
    let area_s = area.to_string();
    if equipped.is_some() && selected_slot.is_none() {
        let can = idt::can_change(ctx(m), area, slot);
        let a2 = area_s.clone();
        action(
            m,
            p,
            24.0,
            1110.0,
            554.0,
            "СНЯТЬ",
            "UNEQUIP",
            Rc::new(move |m: &mut Menu, _| {
                let ok = idt::unequip(ctx(m), &a2, slot);
                pending(m, ok);
            }),
            can,
            "inventoryUnequip",
        );
    } else if let Some(ss) = selected_slot.as_ref().filter(|s| s.item.is_some()) {
        let (wid, sid) = (equipped.as_ref().map(|e| e.id).unwrap_or_default(), ss.id);
        action(
            m,
            p,
            24.0,
            1110.0,
            554.0,
            "СНЯТЬ НАСАДКУ",
            "REMOVE ATTACHMENT",
            Rc::new(move |m: &mut Menu, _| {
                let ok = idt::remove(ctx(m), &wid, sid);
                pending(m, ok);
            }),
            ss.removable,
            "attachmentRemove",
        );
        if !ss.removable {
            k::text(p, 24.0, 1005.0, 554.0, 78.0, &tr("Установленный мод нельзя снять", "This installed mod cannot be removed"), 28.0, k::GREY, "left", true);
        }
    }
    let items = match &selected_slot {
        Some(ss) => idt::parts(ctx(m), equipped.as_ref(), Some(ss)),
        None => idt::area_items(ctx(m), area),
    };
    if area == "Weapon" && selected_slot.is_none() {
        let filters = [("all", "ВСЁ", "ALL"), ("ranged", "ОГНЕСТРЕЛ", "RANGED"), ("melee", "БЛИЖНИЙ БОЙ", "MELEE")];
        for (i, (key, ru, en)) in filters.iter().enumerate() {
            let x = 680.0 + i as f64 * 352.0;
            let key2 = key.to_string();
            action(
                m,
                p,
                x,
                180.0,
                336.0,
                ru,
                en,
                Rc::new(move |m: &mut Menu, _| {
                    (m.gp.inv.s.filter, m.gp.inv.s.selection) = (key2.clone(), None);
                    redraw_reset(m);
                }),
                true,
                &format!("inventoryFilter{key}"),
            );
            if i > 0 {
                let kind = ev("gamedataItemType", if i == 1 { "Wea_Handgun" } else { "Wea_Katana" });
                if let Some(icon) = idt::weapon_type_icon(kind) {
                    k::image_raw(p, x + 12.0, 199.0, 34.0, 30.0, icon.atlas, icon.part, Some(k::CYAN), 1.0);
                }
            }
            if m.gp.inv.s.filter == *key {
                k::rect(p, x + 15.0, 244.0, 306.0, 3.0, k::CYAN, 1.0);
            }
        }
    } else if let Some(ss) = &selected_slot {
        k::text(p, 680.0, 178.0, 1048.0, 60.0, &ss.name, 30.0, k::YELLOW, "left", false);
    }
    let partner_ok = !left_slot || {
        let partner = idt::equipped(ctx(m), "Weapon", slot - 3);
        idt::can_dual(partner.as_ref())
    };
    let filter = m.gp.inv.s.filter.clone();
    let mut shown = Vec::new();
    for it in items {
        if (!left_slot || selected_slot.is_some() || idt::can_dual(Some(&it))) && (filter == "all" || selected_slot.is_some() || area != "Weapon" || (filter == "melee" && it.melee) || (filter == "ranged" && !it.melee)) {
            shown.push(it);
        }
    }
    let shown = idt::sort_apply(shown, m.gp.inv.s.sort.as_deref());
    let selected = shown.iter().find(|i| Some(&i.key) == m.gp.inv.s.selection.as_ref()).cloned();
    let (grid_w, cols) = (1080.0, if area == "Weapon" && selected_slot.is_none() { 2 } else { 5 });
    let cell_w = ((grid_w - 46.0 - (cols as f64 - 1.0) * 12.0) / cols as f64).floor();
    let row_h = if cols == 2 { 210.0 } else { 205.0 };
    let mut rows = Vec::new();
    for group in shown.chunks(cols) {
        let group = group.to_vec();
        rows.push(Row {
            h: row_h,
            build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                for (i, item) in group.iter().enumerate() {
                    let key = item.key.clone();
                    let sel = m.gp.inv.s.selection.as_deref() == Some(key.as_str());
                    let k2 = key.clone();
                    tile(
                        m,
                        c,
                        &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                        i as f64 * (cell_w + 12.0),
                        4.0,
                        cell_w,
                        row_h - 12.0,
                        Some(item),
                        None,
                        Rc::new(move |m: &mut Menu, _| {
                            (m.gp.inv.s.selection, m.gp.inv.s.confirm) = (Some(k2.clone()), None);
                            if let Some(mut it) = m.gp.inv.by_key.get(&k2).cloned() {
                                idt::inspect(ctx(m), &mut it);
                            }
                            gp::redraw(m);
                        }),
                        sel,
                        &format!("inventoryItem:{key}"),
                        Some(hover_cb(key)),
                    );
                }
            }),
        });
    }
    let host = ink::canvas(p, 660.0, 0.0, grid_w, 1200.0);
    let keep = m.keep_scroll;
    let v = gp::glist(m, &host, "p1", "p1", P1X + 660.0, 270.0, grid_w, rows, 660.0, keep);
    v.borrow_mut().enable_mask();
    if let Some(sel) = selected.clone() {
        k::text(p, 680.0, 1005.0, 1048.0, 44.0, &k::clip(&sel.name, 60), 33.0, quality_col(&sel.quality), "left", false);
        let mut detail = if sel.is_mod {
            tr("Модификация оружия", "Weapon mod")
        } else if sel.ty == ev("gamedataItemType", "Wea_Katana") {
            tr("Катана", "Katana")
        } else {
            sel.description.clone()
        };
        if area == "Weapon" && selected_slot.is_none() {
            detail = format!("{}{}    {}{}", tr("Урон в секунду: ", "DPS: "), gd::fmt_g(sel.dps), tr("Боеприпасы: ", "Ammo: "), gd::fmt_g(sel.ammo));
        }
        k::text(p, 680.0, 1055.0, 1048.0, 47.0, &k::clip(&gp::clean(&detail), 83), 27.0, k::GREY, "left", false);
        let can = idt::can_change(ctx(m), area, slot);
        let enabled = sel.allowed && (selected_slot.is_some() || (can && partner_ok));
        let (mut ru, mut en) = if selected_slot.is_some() { ("УСТАНОВИТЬ", "INSTALL") } else { ("ЭКИПИРОВАТЬ", "EQUIP") };
        if m.gp.inv.s.confirm.is_some() {
            (ru, en) = ("ЗАМЕНИТЬ МОД", "REPLACE MOD");
            k::text(p, 680.0, 1050.0, 1048.0, 55.0, &tr("Предыдущий мод будет уничтожен", "The previous mod will be destroyed"), 28.0, k::YELLOW, "left", false);
        }
        let (wid, ssid, sid, skey, a2) = (equipped.as_ref().map(|e| e.id).unwrap_or_default(), selected_slot.as_ref().map(|s| s.id), sel.id, sel.key.clone(), area_s.clone());
        action(
            m,
            p,
            680.0,
            1110.0,
            500.0,
            ru,
            en,
            Rc::new(move |m: &mut Menu, _| {
                let confirmed = m.gp.inv.s.confirm.as_deref() == Some(skey.as_str());
                let r = match ssid {
                    Some(slot_id) => idt::install(ctx(m), &wid, slot_id, &sid, confirmed),
                    None => {
                        if idt::equip(ctx(m), &a2, slot, &sid) {
                            idt::Install::Done
                        } else {
                            idt::Install::Refused
                        }
                    }
                };
                match r {
                    idt::Install::Confirm => {
                        m.gp.inv.s.confirm = Some(skey.clone());
                        gp::redraw(m);
                    }
                    idt::Install::Done => {
                        m.gp.inv.s.confirm = None;
                        pending(m, true);
                    }
                    idt::Install::Refused => {
                        m.gp.inv.s.confirm = None;
                        pending(m, false);
                    }
                }
            }),
            enabled,
            "inventoryApply",
        );
        let a = idt::actions(&sel.id, &sel.ui);
        if a.can_favorite {
            let fav = a.favorite;
            let sid = sel.id;
            let b = gp::hub_action(
                m,
                p,
                P1X,
                1192.0,
                1110.0,
                90.0,
                "",
                Rc::new(move |m: &mut Menu, _| {
                    idt::set_favorite(&sid, !fav);
                    pending(m, true);
                    gp::redraw(m);
                }),
                true,
                None,
                None,
            );
            *b.key.borrow_mut() = Some("inventoryFavorite".into());
            star(p, 1237.0, 1144.0, 22.0, if fav { k::YELLOW } else { k::CYAN }, 1.0, if fav { 4.0 } else { 2.0 });
        }
        let (sid, sui) = (sel.id, sel.ui.clone());
        let b = gp::hub_action(
            m,
            p,
            P1X,
            1294.0,
            1110.0,
            226.0,
            &k::upper(&gloc("UI-UserActions-Drop", "Бросить", "Drop")),
            Rc::new(move |m: &mut Menu, _| {
                let ok = idt::drop(&sid, &sui, 1.0);
                done(m, ok, "Предмет выброшен", "Item dropped");
            }),
            a.drop,
            Some(68.0),
            Some(27.0),
        );
        *b.key.borrow_mut() = Some("inventoryDrop".into());
        let arm = m.gp.inv.s.disarm.as_deref() == Some(sel.key.as_str());
        let dis_label = if arm { tr("ПОДТВЕРДИТЬ", "CONFIRM") } else { k::upper(&gloc("UI-UserActions-Disassemble", "Разобрать", "Disassemble")) };
        let (sid, sui, skey, iconic) = (sel.id, sel.ui.clone(), sel.key.clone(), a.iconic);
        let b = gp::hub_action(
            m,
            p,
            P1X,
            1532.0,
            1110.0,
            224.0,
            &dis_label,
            Rc::new(move |m: &mut Menu, _| {
                if iconic && m.gp.inv.s.disarm.as_deref() != Some(skey.as_str()) {
                    m.gp.inv.s.disarm = Some(skey.clone());
                    gp::redraw(m);
                    return;
                }
                let ok = idt::disassemble(&sid, &sui, 1.0);
                done(m, ok, "Предмет разобран", "Item disassembled");
            }),
            a.disassemble,
            Some(68.0),
            Some(27.0),
        );
        *b.key.borrow_mut() = Some("inventoryDisassemble".into());
    } else if !partner_ok && selected_slot.is_none() {
        k::text(p, 680.0, 1035.0, 1048.0, 90.0, &format!("{}{}{}", tr("Сначала возьмите в слот ", "First equip slot "), slot - 2, tr(" оружие для двух рук", " with a dual-wield gun")), 31.0, k::YELLOW, "center", false);
    } else {
        let t = if shown.is_empty() { tr("Нет подходящих предметов", "No compatible items") } else { tr("Выберите предмет", "Select an item") };
        k::text(p, 680.0, 1035.0, 1048.0, 90.0, &t, 31.0, k::GREY, "center", false);
    }
    m.gp.inv.by_key = shown.iter().map(|i| (i.key.clone(), i.clone())).collect();
    m.gp.inv.equipped = equipped;
    m.gp.inv.tip_host = ink::canvas(p, 0.0, 0.0, 660.0, 1200.0);
    m.gp.inv.tips.clear();
    m.gp.inv.tip_shown = None;
    let block = button(m, "p2", &at(P1X, P1Y), 12.0, 186.0, 640.0, 924.0, None, None);
    block.blocker.set(true);
    block.visible.set(false);
    m.gp.inv.tip_plates = Some(iv::plate_blocker(&block));
    m.gp.inv.tip_block = Some(block);
    let popup = m.gp.inv.s.sort_popup.clone();
    iv::sort_popup(m, p, &popup, Rc::new(|m: &mut Menu| &mut m.gp.inv.s.sort_popup));
}
fn done(m: &mut Menu, ok: bool, ru: &str, en: &str) {
    if ok {
        let t = tr(ru, en);
        toast(m, &t);
        (m.gp.inv.s.selection, m.gp.inv.s.disarm) = (None, None);
    }
    pending(m, ok);
}
/// A five-pointed star outline (the favourite mark).
pub(super) fn star(p: &Handle, cx: f64, cy: f64, r: f64, col: [f64; 3], alpha: f64, width: f64) -> Vec<Handle> {
    let pts: Vec<(f64, f64)> = (0..10)
        .map(|i| {
            let a = (-90.0 + f64::from(i) * 36.0).to_radians();
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            (cx + rr * a.cos(), cy + rr * a.sin())
        })
        .collect();
    (0..10).map(|i| gp::line(p, pts[i].0, pts[i].1, pts[(i + 1) % 10].0, pts[(i + 1) % 10].1, col, alpha, width)).collect()
}
/// The tooltip of the item pointed at (after a short rest) or selected, compared with the equipped one.
fn tooltip(m: &mut Menu) {
    if m.gp.inv.tip_host.is_null() {
        return;
    }
    iv::tip_fit(&mut m.gp.inv.tips);
    if let Some(plates) = &m.gp.inv.tip_plates {
        iv::set_plates(plates, &m.gp.inv.tips);
    }
    let key = hover_key().or_else(|| m.gp.inv.s.selection.clone());
    if Some(&key) == m.gp.inv.tip_shown.as_ref() || m.time - HOVER_AT.with(|c| c.get()) < 0.15 {
        return;
    }
    m.gp.inv.tip_shown = Some(key.clone());
    let host = m.gp.inv.tip_host.clone();
    ink::remove_all(&host);
    m.gp.inv.tips.clear();
    let item = key.as_ref().and_then(|k0| m.gp.inv.by_key.get(k0)).cloned();
    let inv = idt::ui_inventory();
    let ui = item.as_ref().map(|i| super::menu_invdata::ui_item(&inv, &i.id)).filter(|u| !u.is_null());
    if let Some(b) = &m.gp.inv.tip_block {
        b.visible.set(ui.is_some());
    }
    let (Some(ui), Some(item)) = (ui, item) else { return };
    let r = m.gp.inv.tip_rect.unwrap_or([12.0, 186.0, 640.0, 924.0]);
    let compare = match (&m.gp.inv.tip_rect, &m.gp.inv.equipped) {
        (None, Some(e)) if e.id != item.id => Some(super::menu_invdata::ui_item(&inv, &e.id)).filter(|h| !h.is_null()),
        _ => None,
    };
    if let Some(t) = iv::tip_spawn(m, &host, &ui, "Backpack", r[0] + 12.0, r[1] + 8.0, r[2] - 40.0, r[3] - 24.0, compare.as_ref(), None) {
        m.gp.inv.tips.push(t);
    }
}
pub(super) fn update(m: &mut Menu) {
    NOW.with(|c| c.set(m.time));
    if m.gp.inv.ctx.is_none() {
        return;
    }
    iv::preview_update(m);
    if m.gp.inv.s.backpack {
        backpack_update(m);
    } else if !m.gp.inv.tip_host.is_null() {
        tooltip(m);
    }
    if m.gp.inv.refresh_at.is_some_and(|t| m.time >= t) {
        m.gp.inv.refresh_at = None;
        idt::invalidate(ctx(m));
        m.gp.jobs.run("inventorySig", idt::signature_job(), true);
        gp::redraw(m);
        return;
    }
    let seq = m.gp.jobs.seq("inventorySig");
    if seq != m.gp.inv.seq {
        let first = m.gp.inv.seq.is_none();
        m.gp.inv.seq = seq;
        if !first {
            idt::invalidate(ctx(m));
            gp::redraw(m);
            return;
        }
    }
    if !m.gp.jobs.busy("inventorySig") && m.gp.jobs.age("inventorySig") > 1.0 {
        m.gp.jobs.run("inventorySig", idt::signature_job(), false);
    }
}
pub(super) fn leave(m: &mut Menu) {
    iv::preview_release(m);
    if let Some(c) = m.gp.inv.ctx.take() {
        idt::close(c);
    }
    (m.gp.inv.refresh_at, m.gp.inv.seq) = (None, None);
    let s = &mut m.gp.inv.s;
    (s.wardrobe, s.wardrobe_delete, s.holster_pick) = (false, None, None);
    m.gp.inv.bp.tips.clear();
    m.gp.inv.bp.tip_host = Handle::null();
}
pub(super) fn before_build(_m: &mut Menu) {}

// ---------------------------------------------------------------- the wardrobe (wardrobe_view.lua)
const COLS: usize = 3;
const TILE_H: f64 = 206.0;
#[derive(Default)]
struct WRowOpts {
    disabled: bool,
    selected: bool,
    size: Option<f64>,
    clip: Option<usize>,
    right_w: f64,
    note_col: Option<[f64; 3]>,
    key: String,
}
type Extra = Rc<dyn Fn(&mut Menu, &Handle, f64, f64, &RowCtx)>;
fn wrow(label: String, note: Option<String>, click: Act, o: WRowOpts, extra: Option<Extra>) -> Row {
    let o = Rc::new(o);
    Row {
        h: 96.0,
        build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
            let (w, h) = (it.cw, 86.0);
            let col = if o.disabled { k::GREY } else { k::TEXT };
            let bg = k::round_fill(c, 0.0, 0.0, w, h, k::NAVY, if o.disabled { 0.04 } else { 0.18 }, Some(4.0));
            let edge = k::cut_frame(&mut m.pool, c, 1.0, 1.0, w - 2.0, h - 2.0, k::CYAN, if o.disabled { 0.1 } else { 0.22 }, Some(10.0), Some(1.0));
            let tw = w - 48.0 - o.right_w;
            let txt = k::text(c, 24.0, if note.is_some() { 6.0 } else { 0.0 }, tw, if note.is_some() { 44.0 } else { h }, &k::clip(&label, o.clip.unwrap_or(26)), o.size.unwrap_or(30.0), if o.selected { k::YELLOW } else { col }, "left", false);
            if let Some(n) = &note {
                k::text(c, 24.0, 48.0, tw, 32.0, &k::clip(n, 34), 22.0, o.note_col.unwrap_or(k::GREY), "left", false);
            }
            if !o.disabled {
                let sel = o.selected;
                let b = button(
                    m,
                    "p1",
                    &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                    0.0,
                    0.0,
                    w,
                    h,
                    Some(click.clone()),
                    Some(Rc::new(move |hover, _| {
                        let lit = if hover || sel { Some(k::YELLOW) } else { None };
                        ink::tint(&bg, lit.unwrap_or(k::NAVY));
                        ink::opacity(&bg, if hover { 0.16 } else if sel { 0.12 } else { 0.18 });
                        k::tint(&edge, lit.unwrap_or(k::CYAN), Some(if lit.is_some() { 1.0 } else { 0.22 }));
                        ink::tint(&txt, if hover { k::WHITE } else if sel { k::YELLOW } else { col });
                    })),
                );
                *b.key.borrow_mut() = Some(o.key.clone());
            }
            if let Some(e) = &extra {
                e(m, c, w, h, &it);
            }
        }),
    }
}
fn outfit_rows(m: &Menu) -> Vec<Row> {
    let (active, list) = idt::wd_outfits();
    let mut rows = Vec::new();
    rows.push(wrow(
        format!("+  {}", k::upper(&idt::wd_text("UI-Wardrobe-SaveSet"))),
        None,
        Rc::new(|m: &mut Menu, _| {
            m.gp.inv.s.wardrobe_delete = None;
            let ok = idt::wd_save();
            pending(m, ok);
        }),
        WRowOpts { disabled: !active || idt::wd_blocked(), size: Some(24.0), clip: Some(34), key: "wardrobeSave".into(), ..Default::default() },
        None,
    ));
    rows.push(wrow(
        idt::wd_text("UI-Wardrobe-NoOutfit"),
        None,
        Rc::new(|m: &mut Menu, _| {
            m.gp.inv.s.wardrobe_delete = None;
            let ok = idt::wd_deactivate();
            pending(m, ok);
        }),
        WRowOpts { selected: !active, key: "wardrobeNone".into(), ..Default::default() },
        None,
    ));
    for (i, e) in list.into_iter().enumerate() {
        let armed = m.gp.inv.s.wardrobe_delete.as_deref() == Some(e.label.as_str());
        let (name, lbl) = (e.name, e.label.clone());
        let extra: Extra = Rc::new(move |m: &mut Menu, c: &Handle, w: f64, h: f64, it: &RowCtx| {
            let x = w - 70.0;
            let col = if armed { k::RED } else { k::CYAN };
            let bx = k::round_fill(c, x, 14.0, 56.0, h - 28.0, col, if armed { 0.18 } else { 0.06 }, Some(4.0));
            let a = gp::line(c, x + 18.0, h / 2.0 - 10.0, x + 38.0, h / 2.0 + 10.0, col, 1.0, 3.0);
            let bb = gp::line(c, x + 38.0, h / 2.0 - 10.0, x + 18.0, h / 2.0 + 10.0, col, 1.0, 3.0);
            let lbl2 = lbl.clone();
            let b = button(
                m,
                "p1",
                &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                x,
                14.0,
                56.0,
                h - 28.0,
                Some(Rc::new(move |m: &mut Menu, _| {
                    if armed {
                        m.gp.inv.s.wardrobe_delete = None;
                        let ok = idt::wd_delete(name);
                        pending(m, ok);
                    } else {
                        m.gp.inv.s.wardrobe_delete = Some(lbl2.clone());
                        gp::redraw(m);
                    }
                })),
                Some(Rc::new(move |hover, _| {
                    let lit = if hover { if armed { k::RED } else { k::YELLOW } } else { col };
                    ink::tint(&bx, lit);
                    ink::opacity(&bx, if hover { 0.3 } else if armed { 0.18 } else { 0.06 });
                    ink::tint(&a, lit);
                    ink::tint(&bb, lit);
                })),
            );
            *b.key.borrow_mut() = Some(format!("wardrobeDelete{}", i + 1));
        });
        rows.push(wrow(
            e.label.clone(),
            armed.then(|| format!("{}?", idt::wd_text("UI-Wardrobe-Deleteset"))),
            Rc::new(move |m: &mut Menu, _| {
                m.gp.inv.s.wardrobe_delete = None;
                let ok = idt::wd_load(name);
                pending(m, ok);
            }),
            WRowOpts { selected: e.equipped, right_w: 64.0, note_col: Some(k::RED), key: format!("wardrobeOutfit{}", i + 1), ..Default::default() },
            Some(extra),
        ));
    }
    rows
}
fn clothing_rows(m: &mut Menu) -> Vec<Row> {
    let mut rows = Vec::new();
    let items_label = k::upper(&idt::wd_text("Story-base-journal-codex-tutorials-Itemization_groupName"));
    let groups = idt::wd_groups(ctx(m));
    for (gi, g) in groups.into_iter().enumerate() {
        let g = Rc::new(g);
        let (g1, il) = (g.clone(), items_label.clone());
        rows.push(Row {
            h: 104.0,
            build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                let (w, h) = (it.cw, 92.0);
                let g = &g1;
                let bg = k::round_fill(c, 0.0, 0.0, w, h, k::CYAN, if g.collapsed { 0.05 } else { 0.10 }, Some(4.0));
                let edge = k::cut_frame(&mut m.pool, c, 1.0, 1.0, w - 2.0, h - 2.0, k::CYAN, 0.3, Some(10.0), Some(1.0));
                let chev = chevron(c, 32.0, h / 2.0, 11.0, 5.0, if g.collapsed { "right" } else { "down" }, k::CYAN, 1.0);
                k::text(c, 62.0, 6.0, w - 280.0, 44.0, &k::clip(&k::upper(&g.name), 28), 28.0, k::CYAN, "left", false);
                let eq = g.equipped.as_ref().map_or_else(|| idt::wd_text("UI-Labels-EmptySlot"), |e| e.name.clone());
                k::text(c, 62.0, 48.0, w - 280.0, 34.0, &k::clip(&eq, 34), 24.0, if g.equipped.is_some() { k::TEXT } else { k::GREY }, "left", false);
                k::text(c, w - 232.0, 0.0, 208.0, h, &format!("{il}: {}", g.items.len()), 22.0, k::GREY, "right", false);
                let (g2, collapsed) = (g.clone(), g.collapsed);
                let b = button(
                    m,
                    "p1",
                    &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                    0.0,
                    0.0,
                    w,
                    h,
                    Some(Rc::new(move |m: &mut Menu, _| {
                        idt::wd_toggle_slot(&g2);
                        gp::redraw(m);
                    })),
                    Some(Rc::new(move |hover, _| {
                        ink::tint(&bg, if hover { k::YELLOW } else { k::CYAN });
                        ink::opacity(&bg, if hover { 0.16 } else if collapsed { 0.05 } else { 0.10 });
                        k::tint(&edge, if hover { k::YELLOW } else { k::CYAN }, Some(if hover { 1.0 } else { 0.3 }));
                        k::tint(&chev, if hover { k::WHITE } else { k::CYAN }, Some(1.0));
                    })),
                );
                *b.key.borrow_mut() = Some(format!("wardrobeSlot{}", gi + 1));
            }),
        });
        if !g.collapsed {
            for group in g.items.chunks(COLS) {
                let group = group.to_vec();
                rows.push(Row {
                    h: TILE_H + 12.0,
                    build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                        let cw = ((it.cw - (COLS as f64 - 1.0) * 10.0) / COLS as f64).floor();
                        for (i, item) in group.iter().enumerate() {
                            let it2 = item.clone();
                            tile(
                                m,
                                c,
                                &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                                i as f64 * (cw + 10.0),
                                0.0,
                                cw,
                                TILE_H,
                                Some(item),
                                None,
                                Rc::new(move |m: &mut Menu, _| {
                                    let ok = idt::wd_toggle(&it2);
                                    pending(m, ok);
                                }),
                                item.equipped,
                                &format!("wardrobeItem:{}", item.key),
                                None,
                            );
                        }
                    }),
                });
            }
        }
    }
    rows
}
fn wardrobe(m: &mut Menu, p: &Handle) {
    gp::title(p, &k::upper(&idt::wd_text("Gameplay-Devices-DisplayNames-Wardrobe")), W);
    let b = gp::head_action(
        m,
        p,
        P1X,
        W - 72.0,
        &tr("К СНАРЯЖЕНИЮ", "EQUIPMENT"),
        Rc::new(|m: &mut Menu, _| {
            (m.gp.inv.s.wardrobe, m.gp.inv.s.wardrobe_delete) = (false, None);
            redraw_reset(m);
        }),
        true,
        None,
    );
    *b.key.borrow_mut() = Some("wardrobeBack".into());
    let (left_x, left_w, right_x, right_w) = (10.0, 478.0, 1066.0, 704.0);
    k::text(p, 24.0, 96.0, 450.0, 48.0, &k::upper(&idt::wd_text("Story-base-gameplay-gui-fullscreen-inventory-inventory_screen-_localizationString15")), 26.0, k::WHITE, "left", false);
    let left = ink::canvas(p, left_x, 0.0, left_w, 1305.0);
    let rows = outfit_rows(m);
    gp::glist(m, &left, "p1", "wardrobeOutfits", P1X + left_x, 150.0, left_w, rows, 1060.0, None);
    iv::preview_show(m, p, 490.0, 100.0, 560.0, 1150.0, None);
    k::text(p, right_x + 14.0, 96.0, 330.0, 48.0, &k::upper(&idt::wd_text("Story-base-gameplay-gui-fullscreen-wardrobe-wardrobe_fullscreen-_localizationString6")), 26.0, k::WHITE, "left", false);
    let source = k::upper(&idt::wd_source());
    let sw = 340f64.min((source.chars().count() as f64 * 22.0 * 0.56).floor() + 44.0);
    let b = gp::hub_action(
        m,
        p,
        P1X,
        right_x + right_w - 14.0 - sw,
        98.0,
        sw,
        &source,
        Rc::new(|m: &mut Menu, _| {
            let ok = idt::wd_next_source();
            pending(m, ok);
            m.gp.reset_list = true;
        }),
        true,
        Some(46.0),
        Some(22.0),
    );
    *b.key.borrow_mut() = Some("wardrobeSource".into());
    let right = ink::canvas(p, right_x, 0.0, right_w, 1305.0);
    let rows = clothing_rows(m);
    let keep = m.keep_scroll;
    let v = gp::glist(m, &right, "p1", "p1", P1X + right_x, 160.0, right_w, rows, 1060.0, keep);
    v.borrow_mut().enable_mask();
}

// ---------------------------------------------------------------- the backpack (backpack_view.lua)
const GRID_X: f64 = 350.0;
const GRID_W: f64 = 1406.0;
const GRID_Y: f64 = 230.0;
const GRID_H: f64 = 800.0;
const CARD_W: f64 = 454.0;
const CARD_H: f64 = 178.0;
const ROW_H: f64 = 190.0;
fn bp_col(q: &str) -> [f64; 3] {
    match q {
        "Common" => k::GREY,
        "Uncommon" => [0.2, 0.85, 0.5],
        "Rare" => [0.25, 0.6, 1.0],
        "Epic" => [0.75, 0.3, 1.0],
        "Legendary" => [1.0, 0.62, 0.16],
        _ => k::CYAN,
    }
}
fn bp_ensure(m: &mut Menu) {
    let bp = &mut m.gp.inv.bp;
    if bp.filter.is_empty() {
        bp.filter = "AllItems".into();
        bp.sort = "QualityAsc".into();
        bp.quantity = 1.0;
    }
}
fn sort_key(bp: &Bp) -> String {
    format!("{}:{}", bp.filter, bp.sort)
}
fn card(m: &mut Menu, c: &Handle, it: &RowCtx, v: &Item, x: f64) {
    let col = bp_col(&v.quality);
    let bp = &m.gp.inv.bp;
    let marked = bp.multi && bp.marked.contains(&v.key);
    let selected = bp.selected.as_deref() == Some(v.key.as_str());
    let bg = k::round_fill(c, x, 4.0, CARD_W, CARD_H, col, if marked { 0.13 } else { 0.045 }, Some(4.0));
    let edge = k::cut_frame(&mut m.pool, c, x, 4.0, CARD_W, CARD_H, if marked || selected { k::YELLOW } else { k::CYAN }, if marked { 1.0 } else { 0.35 }, Some(10.0), Some(if marked { 3.0 } else { 1.0 }));
    if m.gp.inv.bp.multi {
        k::cut_frame(&mut m.pool, c, x + CARD_W - 52.0, 44.0, 34.0, 34.0, if marked { k::YELLOW } else { k::CYAN }, if marked { 1.0 } else { 0.6 }, Some(5.0), Some(2.0));
        if marked {
            gp::line(c, x + CARD_W - 45.0, 61.0, x + CARD_W - 38.0, 69.0, k::YELLOW, 1.0, 4.0);
            gp::line(c, x + CARD_W - 38.0, 69.0, x + CARD_W - 25.0, 51.0, k::YELLOW, 1.0, 4.0);
        }
    }
    k::rect(c, x + 2.0, 8.0, 5.0, CARD_H - 8.0, col, 1.0);
    gp::picture(m, c, x + 14.0, 27.0, CARD_W - 28.0, 92.0, v.image.as_ref(), None);
    k::text(c, x + 12.0, 119.0, CARD_W - 24.0, 28.0, &k::clip(&v.name, 33), 23.0, col, "left", false);
    if v.favorite {
        star(c, x + 24.0, 22.0, 10.0, k::YELLOW, 1.0, 2.0);
    }
    if v.new {
        k::image(c, x + CARD_W - 20.0, 13.0, 9.0, 9.0, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
    }
    if v.equipped {
        gp::line(c, x + CARD_W - 50.0, 15.0, x + CARD_W - 43.0, 22.0, k::CYAN, 1.0, 3.0);
        gp::line(c, x + CARD_W - 43.0, 22.0, x + CARD_W - 31.0, 8.0, k::CYAN, 1.0, 3.0);
    }
    if let Some(cmp) = v.comparison.filter(|c| *c == ev("ItemComparisonState", "Better") || *c == ev("ItemComparisonState", "Worse")) {
        let up = cmp == ev("ItemComparisonState", "Better");
        let (sign, cc) = (if up { -1.0 } else { 1.0 }, if up { [0.2, 0.95, 0.5] } else { k::RED });
        gp::line(c, x + CARD_W - 29.0, 40.0 - sign * 5.0, x + CARD_W - 22.0, 40.0 + sign * 3.0, cc, 1.0, 3.0);
        gp::line(c, x + CARD_W - 22.0, 40.0 + sign * 3.0, x + CARD_W - 15.0, 40.0 - sign * 5.0, cc, 1.0, 3.0);
    }
    let mut tx = x + 14.0;
    if v.weapon {
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
        let avail = x + CARD_W - 12.0 - (tx + 28.0);
        k::text(c, tx + 28.0, 149.0, avail, 28.0, &k::clip(&k::upper(&v.type_name), 7usize.max((avail / 10.0).floor() as usize)), 20.0, if v.iconic { k::YELLOW } else { k::GREY }, "left", false);
    } else {
        k::text(c, tx, 149.0, 320.0, 28.0, &if v.junk { gloc("UI-Filters-Junk", "Мусор", "Junk") } else { String::new() }, 21.0, k::GREY, "left", false);
        k::text(c, x + CARD_W - 110.0, 149.0, 95.0, 28.0, &if v.quantity > 1.0 { format!("x{}", gd::fmt_g(v.quantity)) } else { String::new() }, 22.0, k::CYAN, "right", false);
    }
    let (key, key2) = (v.key.clone(), v.key.clone());
    let hcb = hover_cb(v.key.clone());
    let id = v.id;
    let b = button(
        m,
        "p1",
        &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
        x,
        4.0,
        CARD_W,
        CARD_H,
        Some(Rc::new(move |m: &mut Menu, _| {
            let bp = &mut m.gp.inv.bp;
            if bp.multi {
                if !bp.marked.remove(&key) {
                    bp.marked.insert(key.clone());
                }
                bp.batch_armed = None;
                gp::redraw(m);
                return;
            }
            (bp.selected, bp.quantity, bp.confirm) = (Some(key.clone()), 1.0, None);
            let r = super::lobby_ink::new("UIScriptableSystemInventoryInspectItem");
            idt::hset_id(&r, "itemID", &id);
            let c = super::game_system("GetScriptableSystemsContainer");
            let ui = super::menu_rt::o(&c, "Get", &[super::lobby_rtti::A::Name(cname("UIScriptableSystem"))]);
            super::lobby_rtti::call(&ui, "QueueRequest", &[super::lobby_rtti::A::H(&r)]);
            gp::redraw(m);
        })),
        Some(Rc::new(move |on, _| {
            ink::tint(&bg, if on { k::YELLOW } else { col });
            ink::opacity(&bg, if on || marked { 0.13 } else { 0.045 });
            k::tint(&edge, if on || marked || selected { k::YELLOW } else { k::CYAN }, Some(if on || marked { 1.0 } else { 0.35 }));
            hcb(on);
        })),
    );
    *b.key.borrow_mut() = Some(format!("backpackItem:{key2}"));
}
fn materials(m: &mut Menu, p: &Handle) {
    let (common, hacking) = idt::materials();
    let grid = |m: &mut Menu, list: &[idt::Material], y: f64| -> f64 {
        for (i, mat) in list.iter().enumerate() {
            let (x, yy) = (24.0 + (i % 2) as f64 * 158.0, y + (i / 2) as f64 * 128.0);
            k::round_fill(p, x, yy, 148.0, 118.0, k::NAVY, 0.18, Some(4.0));
            k::cut_frame(&mut m.pool, p, x, yy, 148.0, 118.0, k::CYAN, 0.3, Some(8.0), Some(1.0));
            if let Some(img) = &mat.image {
                k::image_raw(p, x + 24.0, yy + 8.0, 100.0, 70.0, img.atlas, img.part, Some(k::WHITE), 1.0);
            }
            k::text(p, x + 8.0, yy + 80.0, 132.0, 34.0, &if mat.quantity > 999.0 { "999+".into() } else { gd::fmt_g(mat.quantity) }, 27.0, if mat.quantity > 0.0 { [0.25, 0.95, 0.55] } else { k::GREY }, "left", false);
            let b = button(m, "p1", &at(P1X, P1Y), x, yy, 148.0, 118.0, None, None);
            *b.key.borrow_mut() = Some(format!("backpackMaterial:{}", i + 1));
            let _ = &mat.name;
        }
        y + (list.len() as f64 / 2.0).ceil() * 128.0
    };
    k::text(p, 24.0, 170.0, 320.0, 44.0, &k::upper(&gloc("UI-ResourceExports-CraftingMaterials", "Компоненты", "Components")), 27.0, k::RED, "left", false);
    let y = grid(m, &common, 222.0);
    k::text(p, 24.0, y + 12.0, 320.0, 44.0, &k::upper(&gloc("UI-ResourceExports-DeamonComponents", "Компоненты скриптов", "Script components")), 27.0, k::RED, "left", false);
    grid(m, &hacking, y + 64.0);
}
fn bp_refresh(m: &mut Menu) {
    m.gp.inv.bp.refresh_at = Some(m.time + 0.3);
}
fn backpack(m: &mut Menu, p: &Handle) {
    bp_ensure(m);
    {
        let bp = &mut m.gp.inv.bp;
        bp.tips.clear();
        bp.shown = None;
    }
    HOVER.with(|h| *h.borrow_mut() = None);
    glass(m, p, W, 1305.0, Some((P1X, P1Y)));
    if !m.gp.jobs.has("backpack") {
        m.gp.jobs.run("backpack", idt::snapshot_job(), true);
        m.gp.jobs.finish_now("backpack", 0.03);
    }
    let snap = m.gp.jobs.get::<idt::Snapshot>("backpack");
    gp::title(p, &format!("{}{}", k::upper(&gloc("UI-PanelNames-BACKPACK", "Рюкзак", "Backpack")), snap.as_ref().map_or(String::new(), |s| format!("  /  {}", s.list.len()))), W);
    let b = gp::hub_action(
        m,
        p,
        P1X,
        24.0,
        105.0,
        306.0,
        &tr("К СНАРЯЖЕНИЮ", "EQUIPMENT"),
        Rc::new(|m: &mut Menu, _| {
            m.gp.inv.s.backpack = false;
            redraw_reset(m);
        }),
        true,
        Some(56.0),
        Some(27.0),
    );
    *b.key.borrow_mut() = Some("backpackBack".into());
    materials(m, p);
    let junk = idt::junk_count(snap.as_deref());
    let armed = m.gp.inv.bp.junk_armed.is_some_and(|t| m.time - t < 3.0);
    k::text(p, 24.0, 1030.0, 306.0, 60.0, &if junk > 0.0 { format!("{}{}", tr("Мусор: ", "Junk: "), gd::fmt_g(junk)) } else { tr("Мусора нет", "No junk") }, 25.0, k::GREY, "center", false);
    let label = if armed { tr("ПОДТВЕРДИТЬ", "CONFIRM") } else { k::upper(&gloc("UI-UserActions-DisassembleJunk", "Разобрать мусор", "Disassemble junk")) };
    let b = gp::hub_action(
        m,
        p,
        P1X,
        24.0,
        1100.0,
        306.0,
        &label,
        Rc::new(|m: &mut Menu, _| {
            if !m.gp.inv.bp.junk_armed.is_some_and(|t| m.time - t < 3.0) {
                m.gp.inv.bp.junk_armed = Some(m.time);
                gp::redraw(m);
                return;
            }
            m.gp.inv.bp.junk_armed = None;
            let n = m.gp.jobs.get::<idt::Snapshot>("backpack").map_or(0.0, |s| idt::disassemble_junk(&s));
            let t = format!("{}{}", tr("Разобрано предметов: ", "Items disassembled: "), gd::fmt_g(n));
            toast(m, &t);
            bp_refresh(m);
            gp::redraw(m);
        }),
        junk > 0.0,
        Some(68.0),
        Some(if armed { 30.0 } else { 26.0 }),
    );
    *b.key.borrow_mut() = Some("backpackJunk".into());
    let Some(snap) = snap else { return };
    if m.gp.inv.bp.filter != "AllItems" && !snap.categories.contains(m.gp.inv.bp.filter.as_str()) {
        m.gp.inv.bp.filter = "AllItems".into();
    }
    let caption = k::text(p, GRID_X + 10.0, 178.0, 600.0, 40.0, "", 23.0, k::GREY, "left", false);
    let mut n = 0.0;
    for name in idt::FILTERS {
        if name != "AllItems" && !snap.categories.contains(name) {
            continue;
        }
        let x = GRID_X + n * 73.0;
        n += 1.0;
        let on = m.gp.inv.bp.filter == name;
        let lbl = idt::filter_label(name);
        if on {
            ink::set_text(&caption, &lbl);
        }
        let bgf = k::round_fill(p, x, 105.0, 66.0, 56.0, if on { k::YELLOW } else { k::NAVY }, if on { 0.12 } else { 0.18 }, Some(3.0));
        if let Some(icon) = gd::image(tdb(&idt::filter_icon(name))) {
            k::image_raw(p, x + 9.0, 110.0, 48.0, 46.0, icon.atlas, icon.part, Some(if on { k::YELLOW } else { k::CYAN }), 1.0);
        }
        let cap = caption.clone();
        let fb = button(
            m,
            "p1",
            &at(P1X, P1Y),
            x,
            105.0,
            66.0,
            56.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                let bp = &mut m.gp.inv.bp;
                (bp.filter, bp.selected, bp.confirm) = (name.to_string(), None, None);
                redraw_reset(m);
            })),
            Some(Rc::new(move |h, _| {
                ink::tint(&bgf, if h || on { k::YELLOW } else { k::NAVY });
                ink::opacity(&bgf, if h { 0.18 } else if on { 0.12 } else { 0.18 });
                if h {
                    ink::set_text(&cap, &lbl);
                }
            })),
        );
        *fb.key.borrow_mut() = Some(format!("backpackFilter:{name}"));
    }
    let multi = m.gp.inv.bp.multi;
    let b = gp::hub_action(
        m,
        p,
        P1X,
        GRID_X + GRID_W - 300.0 - 12.0 - 200.0,
        105.0,
        200.0,
        &if multi { tr("ГОТОВО", "DONE") } else { tr("ВЫДЕЛЕНИЕ", "SELECT") },
        Rc::new(|m: &mut Menu, _| {
            let bp = &mut m.gp.inv.bp;
            bp.multi = !bp.multi;
            bp.marked.clear();
            (bp.batch_armed, bp.selected, bp.confirm) = (None, None, None);
            gp::redraw(m);
        }),
        true,
        Some(56.0),
        Some(24.0),
    );
    *b.key.borrow_mut() = Some("backpackMulti".into());
    let mut popup = m.gp.inv.bp.sort_popup.take();
    let sort = m.gp.inv.bp.sort.clone();
    iv::sort_control(
        m,
        p,
        &mut popup,
        "backpackSort",
        GRID_X + GRID_W - 300.0,
        112.0,
        300.0,
        Some(&sort),
        Rc::new(|m: &mut Menu, v: String| {
            let bp = &mut m.gp.inv.bp;
            (bp.sort, bp.selected, bp.confirm) = (v, None, None);
            m.gp.reset_list = true;
        }),
        Rc::new(|m: &mut Menu| &mut m.gp.inv.bp.sort_popup),
    );
    m.gp.inv.bp.sort_popup = popup;
    // the grid, once its sorted list is in (sorted in the update)
    let bpk = sort_key(&m.gp.inv.bp);
    let shown = m.gp.inv.bp.sorted.clone().filter(|_| m.gp.inv.bp.sorted_key == bpk && m.gp.inv.bp.sorted_seq == m.gp.inv.bp.snap_seq);
    m.gp.inv.bp.col_of.clear();
    if let Some(shown) = &shown {
        let mut rows = Vec::new();
        for group in shown.chunks(3) {
            for (i, v) in group.iter().enumerate() {
                m.gp.inv.bp.col_of.insert(v.key.clone(), i + 1);
            }
            let group = group.to_vec();
            rows.push(Row {
                h: ROW_H,
                build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                    for (i, v) in group.iter().enumerate() {
                        card(m, c, &it, v, i as f64 * (CARD_W + 12.0));
                    }
                }),
            });
        }
        if rows.is_empty() {
            rows.push(Row { h: 130.0, build: Rc::new(|_m: &mut Menu, c: &Handle, _| {
                k::text(c, 10.0, 20.0, GRID_W - 20.0, 90.0, &tr("Нет предметов в этой категории", "No items in this category"), 29.0, k::GREY, "center", false);
            }) });
        }
        let host = ink::canvas(p, GRID_X, 0.0, GRID_W, 1200.0);
        let keep = m.keep_scroll;
        let v = gp::glist(m, &host, "p1", "p1", P1X + GRID_X, GRID_Y, GRID_W, rows, GRID_H, keep);
        v.borrow_mut().enable_mask();
    }
    let sel = m.gp.inv.bp.selected.clone().and_then(|k0| snap.get(&k0).cloned());
    if sel.is_none() || multi {
        m.gp.inv.bp.selected = None;
    }
    if multi {
        let mut list = Vec::new();
        let mut count = 0.0;
        let marked: Vec<String> = m.gp.inv.bp.marked.iter().cloned().collect();
        for key in marked {
            match snap.get(&key) {
                Some(v) => {
                    count += v.quantity;
                    list.push(v.clone());
                }
                None => {
                    m.gp.inv.bp.marked.remove(&key);
                }
            }
        }
        let armed_for = m.gp.inv.bp.batch_armed.clone().filter(|(_, at0)| m.time - at0 < 3.0).map(|(k0, _)| k0);
        let txt = if list.is_empty() { tr("Выберите предметы", "Pick the items") } else { format!("{}{}{}", tr("Выбрано: ", "Selected: "), list.len(), if count > list.len() as f64 { format!("  ({}{})", tr("шт. ", "pcs "), gd::fmt_g(count)) } else { String::new() }) };
        k::text(p, GRID_X, 1052.0, GRID_W, 46.0, &txt, 32.0, if list.is_empty() { k::GREY } else { k::YELLOW }, "left", false);
        let keys: Vec<String> = shown.as_ref().map(|s| s.iter().map(|v| v.key.clone()).collect()).unwrap_or_default();
        let any = !keys.is_empty();
        let b = gp::hub_action(
            m,
            p,
            P1X,
            GRID_X,
            1110.0,
            200.0,
            &tr("ВСЁ", "ALL"),
            Rc::new(move |m: &mut Menu, _| {
                m.gp.inv.bp.marked.extend(keys.iter().cloned());
                m.gp.inv.bp.batch_armed = None;
                gp::redraw(m);
            }),
            any,
            Some(68.0),
            Some(28.0),
        );
        *b.key.borrow_mut() = Some("backpackMarkAll".into());
        let has = !list.is_empty();
        let b = gp::hub_action(
            m,
            p,
            P1X,
            GRID_X + 212.0,
            1110.0,
            200.0,
            &tr("СНЯТЬ", "NONE"),
            Rc::new(|m: &mut Menu, _| {
                m.gp.inv.bp.marked.clear();
                m.gp.inv.bp.batch_armed = None;
                gp::redraw(m);
            }),
            has,
            Some(68.0),
            Some(28.0),
        );
        *b.key.borrow_mut() = Some("backpackMarkNone".into());
        let list = Rc::new(list);
        for (kind, x, w, ru_done, en_done) in [("drop", GRID_X + GRID_W - 604.0, 290.0, "Выброшено: ", "Dropped: "), ("disassemble", GRID_X + GRID_W - 302.0, 302.0, "Разобрано: ", "Disassembled: ")] {
            let label = if armed_for.as_deref() == Some(kind) {
                tr("ПОДТВЕРДИТЬ", "CONFIRM")
            } else if kind == "drop" {
                k::upper(&gloc("UI-UserActions-Drop", "Бросить", "Drop"))
            } else {
                k::upper(&gloc("UI-UserActions-Disassemble", "Разобрать", "Disassemble"))
            };
            let (l2, af) = (list.clone(), armed_for.clone());
            let b = gp::hub_action(
                m,
                p,
                P1X,
                x,
                1110.0,
                w,
                &label,
                Rc::new(move |m: &mut Menu, _| {
                    if af.as_deref() != Some(kind) {
                        m.gp.inv.bp.batch_armed = Some((kind.into(), m.time));
                        gp::redraw(m);
                        return;
                    }
                    m.gp.inv.bp.batch_armed = None;
                    let (items, skipped) = if kind == "drop" { idt::drop_many(&l2) } else { idt::disassemble_many(&l2) };
                    let t = format!("{}{}{}", tr(ru_done, en_done), gd::fmt_g(items), if skipped > 0 { format!("{}{}", tr(", пропущено: ", ", skipped: "), skipped) } else { String::new() });
                    toast(m, &t);
                    m.gp.inv.bp.marked.clear();
                    bp_refresh(m);
                    gp::redraw(m);
                }),
                has,
                Some(68.0),
                Some(28.0),
            );
            *b.key.borrow_mut() = Some(if kind == "drop" { "backpackDropMarked".into() } else { "backpackDisassembleMarked".into() });
        }
    } else if let Some(v) = sel {
        let a = idt::actions(&v.id, &v.ui);
        m.gp.inv.bp.quantity = m.gp.inv.bp.quantity.min(a.max);
        k::text(p, GRID_X, 1052.0, GRID_W, 46.0, &k::clip(&v.name, 70), 32.0, bp_col(&v.quality), "left", false);
        let mut x = GRID_X;
        if a.can_favorite {
            let (fav, id) = (a.favorite, v.id);
            let fb = gp::hub_action(
                m,
                p,
                P1X,
                x,
                1110.0,
                90.0,
                "",
                Rc::new(move |m: &mut Menu, _| {
                    idt::set_favorite(&id, !fav);
                    bp_refresh(m);
                    gp::redraw(m);
                }),
                true,
                None,
                None,
            );
            *fb.key.borrow_mut() = Some("backpackFavorite".into());
            star(p, x + 45.0, 1144.0, 22.0, if fav { k::YELLOW } else { k::CYAN }, 1.0, if fav { 4.0 } else { 2.0 });
            x += 102.0;
        }
        let q = m.gp.inv.bp.quantity;
        if a.max > 1.0 {
            let max = a.max;
            let b = gp::hub_action(m, p, P1X, x, 1110.0, 80.0, "−", Rc::new(|m: &mut Menu, _| {
                let bp = &mut m.gp.inv.bp;
                (bp.quantity, bp.confirm) = ((bp.quantity - 1.0).max(1.0), None);
                gp::redraw(m);
            }), q > 1.0, None, None);
            *b.key.borrow_mut() = Some("backpackQuantityMinus".into());
            k::text(p, x + 84.0, 1110.0, 110.0, 68.0, &gd::fmt_g(q), 32.0, k::CYAN, "center", false);
            let b = gp::hub_action(m, p, P1X, x + 198.0, 1110.0, 80.0, "+", Rc::new(move |m: &mut Menu, _| {
                let bp = &mut m.gp.inv.bp;
                (bp.quantity, bp.confirm) = ((bp.quantity + 1.0).min(max), None);
                gp::redraw(m);
            }), q < max, None, None);
            *b.key.borrow_mut() = Some("backpackQuantityPlus".into());
            let b = gp::hub_action(m, p, P1X, x + 290.0, 1110.0, 150.0, &tr("МАКС.", "MAX"), Rc::new(move |m: &mut Menu, _| {
                let bp = &mut m.gp.inv.bp;
                (bp.quantity, bp.confirm) = (max, None);
                gp::redraw(m);
            }), true, Some(68.0), Some(28.0));
            *b.key.borrow_mut() = Some("backpackQuantityMax".into());
        }
        let (id, ui) = (v.id, v.ui.clone());
        let b = gp::hub_action(
            m,
            p,
            P1X,
            GRID_X + GRID_W - 604.0,
            1110.0,
            290.0,
            &k::upper(&gloc("UI-UserActions-Drop", "Бросить", "Drop")),
            Rc::new(move |m: &mut Menu, _| {
                let q = m.gp.inv.bp.quantity;
                let ok = idt::drop(&id, &ui, q);
                bp_done(m, ok, "Предмет выброшен", "Item dropped");
            }),
            a.drop,
            Some(68.0),
            Some(28.0),
        );
        *b.key.borrow_mut() = Some("backpackDrop".into());
        let confirm = m.gp.inv.bp.confirm.as_deref() == Some(v.key.as_str());
        let (id, ui, key, iconic) = (v.id, v.ui.clone(), v.key.clone(), a.iconic);
        let b = gp::hub_action(
            m,
            p,
            P1X,
            GRID_X + GRID_W - 302.0,
            1110.0,
            302.0,
            &if confirm { tr("ПОДТВЕРДИТЬ", "CONFIRM") } else { k::upper(&gloc("UI-UserActions-Disassemble", "Разобрать", "Disassemble")) },
            Rc::new(move |m: &mut Menu, _| {
                if iconic && m.gp.inv.bp.confirm.as_deref() != Some(key.as_str()) {
                    m.gp.inv.bp.confirm = Some(key.clone());
                    gp::redraw(m);
                    return;
                }
                let q = m.gp.inv.bp.quantity;
                let ok = idt::disassemble(&id, &ui, q);
                bp_done(m, ok, "Предмет разобран", "Item disassembled");
            }),
            a.disassemble,
            Some(68.0),
            Some(28.0),
        );
        *b.key.borrow_mut() = Some("backpackDisassemble".into());
        if confirm {
            k::text(p, GRID_X + GRID_W - 604.0, 1052.0, 604.0, 46.0, &tr("Культовое оружие будет разобрано", "The iconic weapon will be disassembled"), 24.0, k::YELLOW, "right", false);
        }
    } else {
        k::text(p, GRID_X, 1100.0, GRID_W, 80.0, &tr("Наведите на предмет, чтобы увидеть подробности; выберите его для действий", "Point at an item for its details; select it for actions"), 27.0, k::GREY, "center", false);
    }
    m.gp.inv.bp.tip_host = ink::canvas(p, 0.0, 0.0, W, 1100.0);
    let block = button(m, "p2", &at(P1X, P1Y), 0.0, 186.0, 640.0, 850.0, None, None);
    block.blocker.set(true);
    block.visible.set(false);
    m.gp.inv.bp.plates = Some(iv::plate_blocker(&block));
    m.gp.inv.bp.block = Some(block);
    let popup = m.gp.inv.bp.sort_popup.clone();
    iv::sort_popup(m, p, &popup, Rc::new(|m: &mut Menu| &mut m.gp.inv.bp.sort_popup));
}
fn bp_done(m: &mut Menu, ok: bool, ru: &str, en: &str) {
    if ok {
        let t = tr(ru, en);
        toast(m, &t);
        (m.gp.inv.bp.selected, m.gp.inv.bp.confirm) = (None, None);
        bp_refresh(m);
    } else {
        let t = m.tx("blocked");
        toast(m, &t);
    }
    gp::redraw(m);
}
fn backpack_update(m: &mut Menu) {
    bp_ensure(m);
    let sig = m.gp.jobs.seq("inventorySig");
    let bp = &mut m.gp.inv.bp;
    if bp.refresh_at.is_some_and(|t| m.time >= t) || (sig.is_some() && bp.sig_seq.is_some() && sig != bp.sig_seq) {
        bp.refresh_at = None;
        m.gp.jobs.run("backpack", idt::snapshot_job(), true);
    }
    m.gp.inv.bp.sig_seq = sig;
    let snap_seq = m.gp.jobs.seq("backpack");
    if snap_seq.is_some() && snap_seq != m.gp.inv.bp.snap_seq {
        m.gp.inv.bp.snap_seq = snap_seq;
        m.gp.inv.bp.sorted = None;
    }
    let key = sort_key(&m.gp.inv.bp);
    let Some(snap) = m.gp.jobs.get::<idt::Snapshot>("backpack") else { return };
    if m.gp.inv.bp.sorted.is_none() || m.gp.inv.bp.sorted_key != key {
        let list = idt::filtered(&snap, &m.gp.inv.bp.filter);
        let sorted = idt::sort_apply(list, Some(&m.gp.inv.bp.sort));
        let bp = &mut m.gp.inv.bp;
        (bp.sorted, bp.sorted_key, bp.sorted_seq) = (Some(sorted), key, bp.snap_seq);
        gp::redraw(m);
        return;
    }
    iv::tip_fit(&mut m.gp.inv.bp.tips);
    if let Some(plates) = &m.gp.inv.bp.plates {
        iv::set_plates(plates, &m.gp.inv.bp.tips);
    }
    let key = hover_key().or_else(|| m.gp.inv.bp.selected.clone());
    if m.gp.inv.bp.tip_host.is_null() || Some(&key) == m.gp.inv.bp.shown.as_ref() || m.time - HOVER_AT.with(|c| c.get()) <= 0.12 {
        return;
    }
    let host = m.gp.inv.bp.tip_host.clone();
    ink::remove_all(&host);
    m.gp.inv.bp.tips.clear();
    m.gp.inv.bp.shown = Some(key.clone());
    if let Some(b) = &m.gp.inv.bp.block {
        b.visible.set(false);
    }
    let Some(v) = key.as_ref().and_then(|k0| snap.get(k0)).cloned() else { return };
    let x = if m.gp.inv.bp.col_of.get(&v.key) == Some(&1) { W - 652.0 } else { 12.0 };
    let inv = idt::ui_inventory();
    let mut ui = super::menu_invdata::ui_item(&inv, &v.id);
    if ui.is_null() {
        ui = v.ui.clone();
    }
    let resolver = super::menu_rt::so("InventoryItemPreferredComparisonResolver", "Make", &[super::lobby_rtti::A::H(&inv)]);
    let mut compare = if super::menu_rt::b(&ui, "IsEquipped", &[]) { Handle::null() } else { super::menu_rt::o(&resolver, "GetPreferredComparisonItem", &[super::lobby_rtti::A::H(&ui)]) };
    if !compare.is_null() && super::menu_rt::id16(&compare, "GetID", &[]) == v.id {
        compare = Handle::null();
    }
    let cmp = (!compare.is_null()).then_some(&compare);
    if let Some(t) = iv::tip_spawn(m, &host, &ui, "Backpack", x + 12.0, 194.0, 616.0, 830.0, cmp, None) {
        m.gp.inv.bp.tips.push(t);
    }
    if let Some(b) = &m.gp.inv.bp.block {
        b.x.set(P1X + x);
        b.visible.set(true);
    }
}
