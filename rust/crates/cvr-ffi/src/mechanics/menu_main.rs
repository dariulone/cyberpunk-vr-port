//! CyberpunkVRPort_Menu/modules/menu.lua, part 2: the column of icons, the sections' panels (vehicles, radio, game; the
//! others are their modules'), opening and closing, the pointers -- a fingertip pressing into the screen, the beam with
//! the trigger, dragging lists and sliders, the right stick scrolling -- and the menu's tick (opened by the wrist
//! gesture when unarmed, closed by it, by walking away, arming, a scene, or a vehicle change).
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, Val, A};
use super::lobby_spawn::{self as sp, add, cross, dot, mul, norm, sub, V3};
use super::menu_bits::{camera, Cam, ViewRef};
use super::menu_core::*;
use super::menu_data as d;
use super::menu_ink as k;
use crate::script_call::Value;
use red4ext_abi::handle::{Handle, WeakHandle};
use std::rc::Rc;
use std::sync::Mutex;

static MENU: Mutex<Option<Menu>> = Mutex::new(None);
pub(super) fn with<R>(f: impl FnOnce(&mut Menu) -> R) -> R {
    let mut g = MENU.lock().unwrap_or_else(|p| p.into_inner());
    let m = g.get_or_insert_with(|| Menu { glass_on: true, column_offset: COL_HOME, column_target: COL_HOME, ..Default::default() });
    f(m)
}

// ---------------------------------------------------------------- the game's state the menu watches
fn armed() -> bool {
    let pl = super::belt::player();
    !pl.is_null() && !super::belt::in_slot(&pl, "AttachmentSlots.WeaponRight").is_null()
}
fn scene_tier() -> i32 {
    let pl = super::belt::player();
    let bb = super::belt::h(&pl, "GetPlayerStateMachineBlackboard", &[]);
    if bb.is_null() {
        return 0;
    }
    (|| -> Result<i32, String> {
        let defs = super::boards::defs()?;
        let dd = super::boards::definition(&defs, "PlayerStateMachine")?;
        let f = super::boards::field(dd, "SceneTier")?;
        super::boards::get::<i32>(&bb, "GetInt", f)
    })()
    .unwrap_or(0)
}
/// A handle into an object's handle (or weak handle) property, by the property's own type.
pub(super) fn set_handle_prop(o: &Handle, name: &str, v: &Handle) {
    let Some(f) = rt::field(o, name) else { return };
    // SAFETY: the property's own type assigns the value of its kind.
    unsafe {
        match f.ty.kind() {
            9 => f.ty.assign(f.ptr, (v as *const Handle).cast()),
            10 => {
                let w = WeakHandle::from_handle(v);
                f.ty.assign(f.ptr, (&w as *const WeakHandle).cast());
            }
            _ => {}
        }
    }
}
pub(super) fn set_enum_prop(o: &Handle, name: &str, en: &str, member: &str) {
    let (Some(f), Some(v)) = (rt::field(o, name), crate::script_call::enum_value(en, member)) else { return };
    // SAFETY: the enum's storage, by its size.
    unsafe {
        match f.ty.size() {
            1 => *(f.ptr as *mut i8) = v as i8,
            2 => *(f.ptr as *mut i16) = v as i16,
            8 => *(f.ptr as *mut i64) = v,
            _ => *(f.ptr as *mut i32) = v as i32,
        }
    }
}
fn holster_all() {
    let pl = super::belt::player();
    if pl.is_null() {
        return;
    }
    let r = ink::new("EquipmentSystemWeaponManipulationRequest");
    set_handle_prop(&r, "owner", &pl);
    set_enum_prop(&r, "requestType", "EquipmentManipulationAction", "UnequipAll");
    set_enum_prop(&r, "equipAnimType", "gameEquipAnimationType", "Default");
    let c = super::game_system("GetScriptableSystemsContainer");
    let es = ink::child(&c, "Get", &[sp::cn("EquipmentSystem")]);
    rt::call(&es, "QueueRequest", &[A::H(&r)]);
}
fn rh_bool(name: &str) -> bool {
    super::requests_handler().ok().and_then(|rh| rt::num(&rh, name, &[])).is_some_and(|v| v != 0.0)
}
pub(super) fn paused() -> bool {
    rh_bool("IsGamePaused")
}
fn pre_game() -> bool {
    rh_bool("IsPreGame")
}

// ---------------------------------------------------------------- the anchor
fn frame(m: &Menu) -> Option<(V3, V3, V3)> {
    let a = m.anchor?;
    if !a.vehicle {
        return Some((a.c, a.r, a.u));
    }
    let v = d::mounted();
    if v.is_null() {
        return None;
    }
    let p = super::belt::m::<[f32; 4]>(&v, "GetWorldPosition", &[]).ok()?;
    let q = super::belt::m::<[f32; 4]>(&v, "GetWorldOrientation", &[]).ok()?.map(f64::from);
    let p = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
    Some((add(p, super::belt::qrot(q, a.c)), super::belt::qrot(q, a.r), super::belt::qrot(q, a.u)))
}
const REACH: f64 = 0.50;
const DROP: f64 = -0.18;
fn anchor_for(cam: &Cam) -> Anchor {
    let up = [0.0, 0.0, 1.0];
    let lean = (-DROP).atan2(REACH).clamp(0.0, 25f64.to_radians());
    let u = add(mul(up, lean.cos()), mul(cam.f, lean.sin()));
    let c = add(add(cam.p, mul(cam.f, REACH)), mul(up, DROP));
    let a = Anchor { vehicle: false, c, r: cam.r, u };
    let v = d::mounted();
    if !v.is_null() {
        if let (Ok(wp), Ok(wq)) = (super::belt::m::<[f32; 4]>(&v, "GetWorldPosition", &[]), super::belt::m::<[f32; 4]>(&v, "GetWorldOrientation", &[])) {
            let qi = super::belt::conj(wq.map(f64::from));
            let wp = [f64::from(wp[0]), f64::from(wp[1]), f64::from(wp[2])];
            return Anchor { vehicle: true, c: super::belt::qrot(qi, sub(c, wp)), r: super::belt::qrot(qi, cam.r), u: super::belt::qrot(qi, u) };
        }
    }
    a
}

// ---------------------------------------------------------------- sections
pub(super) fn group_of(key: Option<&str>) -> Option<&'static str> {
    Some(match key? {
        "inventory" | "character" | "cyberware" | "crafting" | "trade" | "stash" => "person",
        "journal" | "codex" | "shards" | "tarot" => "quests",
        "load" => "game",
        "contacts" => "contacts",
        "vehicles" => "vehicles",
        "radio" => "radio",
        "timeskip" => "timeskip",
        "game" => "game",
        "settings" => "settings",
        _ => return None,
    })
}
fn icon_selected(section: Option<&str>, key: &str) -> bool {
    section == Some(key) || group_of(section) == Some(key) && key != "game" || (key == "game" && section == Some("load"))
}
fn mark_icons(m: &Menu) {
    for b in &m.icons {
        if let Some(k0) = b.key() {
            b.selected.set(icon_selected(m.section.as_deref(), &k0));
        }
    }
}
/// openSection: a section opened (or, the same again, closed) -- the column moves aside, the panels rebuild.
/// The mirror's appearance editor and the Glamdoc stand alone: none of the column's sections can be reached from them.
fn standalone(m: &Menu) -> bool {
    matches!(m.section.as_deref(), Some("appearance" | "glamdoc"))
}
pub(super) fn open_section(m: &mut Menu, key: Option<&str>, keep_open: bool) {
    if standalone(m) || !m.open || !m.menu.as_ref().is_some_and(|s| s.built) {
        return;
    }
    if m.section.as_deref() == Some("contacts") {
        super::menu_messages::leave(m);
    }
    super::menu_gameplay::leave(m);
    if m.section.as_deref() == key && !keep_open {
        m.section = None;
    } else {
        m.section = key.map(str::to_string);
    }
    (m.vehicles, m.stations, m.vehicle) = (None, None, None);
    let target = if m.section.is_some() { 0.0 } else { COL_HOME };
    move_column(m, target);
    m.panel_start = Some(m.time + if m.section.is_some() && m.column_move.is_some() { COLUMN_SHIFT } else { 0.0 });
    build_panels(m, true, false);
    mark_icons(m);
    for b in m.buttons.clone() {
        if b.group == "col" {
            b.repaint(false);
        }
    }
}
fn open_group(m: &mut Menu, group: &str) {
    if group_of(m.section.as_deref()) == Some(group) {
        open_section(m, None, true);
        return;
    }
    let key = if group == "person" {
        if super::menu_stash::current(m) {
            "stash"
        } else {
            super::menu_vendor::current_section(m).unwrap_or("inventory")
        }
    } else {
        "journal"
    };
    open_section(m, Some(key), true);
}
type GlyphFn = Rc<dyn Fn(&Handle, f64, f64, f64, [f64; 3]) -> Vec<Handle>>;
enum IconGlyph {
    Close,
    Glyph(Glyph),
    Draw(GlyphFn),
}
/// iconButton: a round icon of the column; lit and pointed at while its section (or its group) is open.
fn icon_button(m: &mut Menu, key: &str, g: IconGlyph, y: f64, click: Act, size: Option<f64>, left: bool) -> BtnRef {
    let size = size.unwrap_or(ICON);
    let (x0, y0) = (COLX - size / 2.0, y - size / 2.0);
    let c = ink::canvas(&m.layer_col, x0, y0, size, size);
    let ring_host = ink::canvas(&c, 0.0, 0.0, size, size);
    ink::v2(&ring_host, "SetRenderTransformPivot", 0.5, 0.5);
    let bare = key == "close";
    let ring = if bare { Vec::new() } else { k::ring(&ring_host, size / 2.0, size / 2.0, size * 0.39, 1.5, k::CYAN, 0.68, 32, Some(4)) };
    let outer = if bare { Vec::new() } else { k::ring(&c, size / 2.0, size / 2.0, size / 2.0 - 3.0, 1.5, k::CYAN, 0.38, 48, None) };
    let visual = Rc::new(std::cell::Cell::new((k::CYAN, 0.68, 0.38)));
    let marks: Vec<Handle> = match g {
        IconGlyph::Draw(f) => f(&c, size / 2.0, size / 2.0, size * 0.78, k::WHITE),
        IconGlyph::Glyph(g) => vec![glyph(&c, size / 2.0, size / 2.0, size, g, k::WHITE, 1.0)],
        IconGlyph::Close => {
            let a = k::slash(&c, size / 2.0, size / 2.0, size * 0.40, 7.0, k::CYAN, 1.0);
            let b = k::slash(&c, size / 2.0, size / 2.0, size * 0.40, 7.0, k::CYAN, 1.0);
            ink::rotation(&b, 45.0);
            vec![a, b]
        }
    };
    let pointer = if left { chevron(&c, -42.0, size / 2.0, 15.0, 7.0, "left", k::YELLOW, 0.0) } else { chevron(&c, size + 42.0, size / 2.0, 15.0, 7.0, "right", k::YELLOW, 0.0) };
    let summary_pointer = if key == "person" { chevron(&c, -42.0, size / 2.0, 15.0, 7.0, "left", k::YELLOW, 0.0) } else { Vec::new() };
    let (r2, o2, v2) = (ring.clone(), outer.clone(), visual.clone());
    let paint: Paint = Rc::new(move |hover, sel| {
        let lit = if hover || sel { Some(k::YELLOW) } else { None };
        let col = lit.unwrap_or(k::CYAN);
        v2.set((col, if lit.is_some() { 1.0 } else { 0.68 }, if lit.is_some() { 0.9 } else { 0.38 }));
        k::tint(&r2, col, Some(if lit.is_some() { 1.0 } else { 0.68 }));
        k::tint(&o2, col, Some(if lit.is_some() { 0.9 } else { 0.38 }));
        k::tint(&marks, if lit.is_some() { k::YELLOW } else { k::WHITE }, Some(1.0));
        k::tint(&pointer, k::YELLOW, Some(if sel { 1.0 } else { 0.0 }));
        k::tint(&summary_pointer, k::YELLOW, Some(if sel { 1.0 } else { 0.0 }));
    });
    let b = button(m, "col", &ORIGIN, x0, y0, size, size, Some(click), Some(paint));
    *b.key.borrow_mut() = Some(key.into());
    *b.icon.borrow_mut() = Some(IconBits { x0, y0, canvas: c, ring_host, ring, outer, visual });
    b.x.set(x0 + m.column_offset);
    b.circle.set(true);
    b.selected.set(icon_selected(m.section.as_deref(), key));
    b.repaint(false);
    b
}
fn build_column(m: &mut Menu) {
    clear_buttons(m, "col");
    drop_anims(m, "col");
    let act = |f: fn(&mut Menu)| -> Act { Rc::new(move |m, _| f(m)) };
    let quests: GlyphFn = Rc::new(|p, cx, cy, size, col| vec![k::round_fill(p, cx - 4.0, cy - size * 0.26, 8.0, size * 0.34, col, 1.0, Some(4.0)), k::round_fill(p, cx - 4.0, cy + size * 0.16, 8.0, 8.0, col, 1.0, Some(4.0))]);
    let clock: GlyphFn = Rc::new(|parent, cx, cy, size, col| {
        let r = size * 0.48;
        let c = ink::canvas(parent, cx - r / 2.0, cy - r / 2.0, r, r);
        let ring = k::image(&c, 0.0, 0.0, r, r, k::ATLAS_PHONE, "circle", Some(col), 1.0);
        k::image(&c, 5.0, 5.0, r - 10.0, r - 10.0, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
        let hour = k::rect(&c, r / 2.0 - 2.0, r * 0.2, 4.0, r * 0.32, col, 1.0);
        let minute = k::rect(&c, r / 2.0, r / 2.0 - 2.0, r * 0.25, 4.0, col, 1.0);
        vec![ring, hour, minute]
    });
    let pad: GlyphFn = Rc::new(|p, cx, cy, size, col| gamepad_glyph(p, cx, cy, size, col));
    let g = |key: &str| IconGlyph::Glyph(glyph_of(key).unwrap_or_else(|| glyph_of("car").expect("car glyph")));
    let icons = vec![
        icon_button(m, "close", IconGlyph::Close, col_y("close"), act(close), Some(CLOSE_ICON), false),
        icon_button(m, "person", g("person"), col_y("person"), Rc::new(|m, _| open_group(m, "person")), None, false),
        icon_button(m, "map", g("map"), col_y("map"), act(|m| {
            open_map(m);
        }), None, false),
        icon_button(m, "quests", IconGlyph::Draw(quests), col_y("quests"), Rc::new(|m, _| open_group(m, "quests")), None, false),
        icon_button(m, "contacts", g("call"), col_y("contacts"), Rc::new(|m, _| open_section(m, Some("contacts"), false)), None, false),
        icon_button(m, "vehicles", g("car"), col_y("vehicles"), Rc::new(|m, _| open_section(m, Some("vehicles"), false)), None, false),
        icon_button(m, "radio", g("radio"), col_y("radio"), Rc::new(|m, _| open_section(m, Some("radio"), false)), None, false),
        icon_button(m, "timeskip", IconGlyph::Draw(clock), col_y("timeskip"), Rc::new(|m, _| open_section(m, Some("timeskip"), false)), None, false),
        icon_button(m, "game", IconGlyph::Draw(pad), col_y("game"), Rc::new(|m, _| open_section(m, Some("game"), false)), None, false),
        icon_button(m, "settings", g("cog"), col_y("settings"), Rc::new(|m, _| open_section(m, Some("settings"), false)), None, false),
    ];
    for (i, b) in icons.iter().enumerate() {
        let (c, x0, y0) = {
            let ib = b.icon.borrow();
            let ib = ib.as_ref().expect("icon bits");
            (ib.canvas.clone(), ib.x0, ib.y0)
        };
        slide_in(m, "col", &c, x0, y0, i as f64 * 0.05, 0.0, 70.0, Some(b.clone()));
    }
    m.icons = icons;
    let top = col_y("close") - RES_H / 2.0;
    let status = ink::canvas(&m.layer_col, COLX + 90.0, top, 650.0, RES_H);
    m.resource_bg = k::round_fill(&status, -16.0, 0.0, 650.0, RES_H, k::NAVY, 0.14, Some(8.0));
    m.resource_edge = k::round_edge(&status, -16.0, 0.0, 650.0, RES_H, k::CYAN, 0.4, Some(8.0));
    m.resource_rect = Some(GRect { x: COLX + m.column_offset + 74.0 + RES_INSET, y: top + RES_INSET, w: 650.0 - 2.0 * RES_INSET, h: RES_H - 2.0 * RES_INSET, depth: Some(0.0006), icon: false, opaque: false, visible: true });
    m.money_widget = k::text(&status, 0.0, 0.0, 410.0, RES_H, "", 34.0, k::YELLOW, "left", false);
    ink::opacity(&m.money_widget, 0.86);
    m.weight_icon = ink::canvas(&status, 250.0, 0.0, 30.0, RES_H);
    k::round_edge(&m.weight_icon, 0.0, 20.0, 24.0, 24.0, k::CYAN, 0.85, Some(3.0));
    k::round_edge(&m.weight_icon, 6.0, 11.0, 12.0, 12.0, k::CYAN, 0.85, Some(3.0));
    m.weight_widget = k::text(&status, 291.0, 0.0, 300.0, RES_H, "", 32.0, k::CYAN, "left", false);
    ink::opacity(&m.weight_widget, 0.86);
    (m.resource_at, m.resource_text, m.resource_layout, m.resource_target) = (None, None, None, None);
    slide_in(m, "col", &status, COLX + 90.0, top, 0.04, 0.0, 24.0, None);
}
fn grouped_number(n: f64) -> String {
    let s = (n.floor() as i64).to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 && ch != '-' {
            out.push(' ');
        }
        out.push(ch);
    }
    out.chars().rev().collect::<String>().trim_start().to_string()
}
fn desired_x(w: &Handle) -> f64 {
    rt::call(w, "GetDesiredSize", &[]).and_then(|o| Val::of(&o).f("X").and_then(|x| x.num())).unwrap_or(0.0)
}
fn update_resources(m: &mut Menu, dt: f64) {
    if m.money_widget.is_null() {
        return;
    }
    if m.resource_at.is_none_or(|a| m.time - a >= 1.0) {
        m.resource_at = Some(m.time);
        if let Some(r) = super::menu_invdata::resources() {
            let money = format!("€$ {}", grouped_number(r.money));
            let weight = format!("{}/{}", super::lobby_holo::fmt_num(r.weight), super::lobby_holo::fmt_num(r.capacity));
            let key = format!("{money}:{weight}");
            if m.resource_text.as_deref() != Some(key.as_str()) {
                m.resource_text = Some(key);
                m.resource_measure = Some(2);
                ink::set_text(&m.money_widget, &money);
                ink::set_text(&m.weight_widget, &weight);
                ink::tint(&m.weight_widget, if r.weight > r.capacity { k::RED } else { k::CYAN });
            }
        }
    }
    if let Some(n) = m.resource_measure {
        let n = n - 1;
        m.resource_measure = Some(n);
        if n <= 0 {
            let (mw, ww) = (desired_x(&m.money_widget), desired_x(&m.weight_widget));
            if mw > 1.0 && ww > 1.0 {
                m.resource_target = Some((mw + 18.0, mw + 55.0, mw + ww + 86.0));
                m.resource_measure = None;
            } else {
                m.resource_measure = Some(1);
            }
        }
    }
    if let Some(t) = m.resource_target {
        let mut a = m.resource_layout.unwrap_or(t);
        let k1 = (dt * 12.0).min(1.0);
        let step = |a: f64, t: f64| {
            let v = a + (t - a) * k1;
            if (t - v).abs() < 0.5 {
                t
            } else {
                v
            }
        };
        a = (step(a.0, t.0), step(a.1, t.1), step(a.2, t.2));
        m.resource_layout = Some(a);
        ink::move_to(&m.weight_icon, a.0, 0.0);
        ink::move_to(&m.weight_widget, a.1, RES_H / 2.0);
        k::round_size(&m.resource_bg, a.2, RES_H, Some(8.0));
        if let Some(e) = m.resource_edge.first() {
            k::round_size(e, a.2, RES_H, Some(8.0));
        }
        if let Some(r) = m.resource_rect.as_mut() {
            r.w = a.2 - 2.0 * RES_INSET;
        }
    }
}
/// listPanel: a panel with its header and the rows in a list (a count when it scrolls, "nothing here" when empty).
pub(super) fn list_panel(m: &mut Menu, p: &Handle, title: &str, rows: Vec<Row>) -> Option<ViewRef> {
    let (h, scrolls) = list_size(&rows, LIST_H);
    let empty = rows.is_empty();
    glass(m, p, P1W, HEAD + h.max(if empty { ROW } else { 0.0 }) + 14.0, Some((P1X, P1Y)));
    let hc = header(p, P1W, title);
    if m.anim1 {
        slide_in(m, "p1", &hc, 0.0, 0.0, 0.0, 0.0, 20.0, None);
    }
    if scrolls {
        count_pill(&hc, P1W - 72.0, rows.len());
    }
    if empty {
        let t = m.tx("empty");
        k::text(p, 34.0, HEAD, P1W - 68.0, ROW, &t, 44.0, k::GREY, "left", false);
        return None;
    }
    let keep = m.keep_scroll;
    Some(list(m, p, "p1", "p1", P1X, P1Y, HEAD, P1W - 2.0 * PAD, rows, LIST_H, keep))
}
fn build_vehicles(m: &mut Menu, p: &Handle) {
    if m.vehicles.is_none() {
        m.vehicles = Some(d::vehicles());
    }
    let items = m.vehicles.clone().unwrap_or_default();
    let mut rows = Vec::new();
    for v in items {
        let g = glyph_of(v.kind).or_else(|| glyph_of("car")).expect("car glyph");
        let mut o = RowOpts { icon_w: Some(84.0), tint_icon: true, icon: Some(Rc::new(move |rc, x, _y, w, h| glyph(rc, x + w / 2.0, h / 2.0, h * 1.1, g, k::CYAN, 1.0))), ..Default::default() };
        if v.repair > 0.0 {
            o.right = Some(format!("{}:{:02}", (v.repair / 60.0).floor() as i64, (v.repair % 60.0).floor() as i64));
            (o.right_w, o.right_col, o.disabled) = (Some(140.0), Some(k::GREY), true);
        }
        let selected = m.vehicle.as_ref().is_some_and(|s| s.name == v.name && s.picture.as_ref().map(|p| p.part) == v.picture.as_ref().map(|p| p.part));
        o.selected = selected;
        let name = v.name.clone();
        rows.push(list_row(
            "p1",
            name,
            Rc::new(move |m, _| {
                m.vehicle = if selected { None } else { Some(v.clone()) };
                let has = m.vehicle.is_some();
                build_panels(m, false, has);
            }),
            o,
        ));
    }
    let t = m.tx("vehicles");
    list_panel(m, p, &t, rows);
}
fn build_vehicle_detail(m: &mut Menu, p: &Handle) {
    let Some(v) = m.vehicle.clone() else { return };
    let h2 = at(P2X, P1Y);
    const PIC_H: f64 = 240.0;
    glass(m, p, P2W, HEAD + PIC_H + 30.0 + 70.0 + ROW + 30.0, Some((P2X, P1Y)));
    header(p, P2W, &k::clip(&k::upper(&v.name), 18));
    let mut y = HEAD + 10.0;
    let repairing = v.repair > 0.0;
    match &v.picture {
        Some(pic) => {
            let sz = super::menu_pictures::size(&pic.name).unwrap_or((470.0, 156.0));
            fit(p, P2W / 2.0, y + PIC_H / 2.0, P2W - 2.0 * PAD - 40.0, PIC_H, pic.atlas, pic.part, sz.0, sz.1, k::WHITE, if repairing { 0.25 } else { 1.0 });
        }
        None => {
            glyph(p, P2W / 2.0, y + PIC_H / 2.0, PIC_H, glyph_of(v.kind).or_else(|| glyph_of("car")).expect("car glyph"), k::CYAN, 1.0);
        }
    }
    y += PIC_H + 20.0;
    let status = if repairing {
        format!("{}  {}:{:02}", m.tx("repair"), (v.repair / 60.0).floor() as i64, (v.repair % 60.0).floor() as i64)
    } else if !d::mounted().is_null() {
        m.tx("mounted")
    } else {
        m.tx("ready")
    };
    k::text(p, 0.0, y, P2W, 60.0, &status, 38.0, if repairing { k::GREY } else { k::CYAN }, "center", false);
    y += 80.0;
    let label = m.tx("summon");
    let enabled = !repairing && d::mounted().is_null();
    let g = glyph_of(v.kind).or_else(|| glyph_of("car"));
    action(
        m,
        p,
        "p2",
        &h2,
        PAD,
        y,
        P2W - 2.0 * PAD,
        ROW + 10.0,
        &label,
        enabled,
        Rc::new(move |m, _| {
            let (ok, why) = d::summon(&v);
            if ok {
                let t = m.tx("summoned");
                toast(m, &t);
                m.close_at = Some(m.time + 0.9);
            } else {
                let t = m.t.get(why).cloned().unwrap_or_else(|| m.tx("blocked"));
                toast(m, &t);
            }
        }),
        Some(k::CYAN),
        g,
    );
}
fn build_radio(m: &mut Menu, p: &Handle) {
    if m.stations.is_none() {
        m.stations = Some(d::stations());
    }
    let items = m.stations.clone().unwrap_or_default();
    let mut st = d::radio();
    if let Some((active, index, at_t)) = m.radio_want {
        if (st.active == active && (!active || index < 0 || st.index == index)) || m.time - at_t > 2.0 {
            m.radio_want = None;
        } else {
            st.active = active;
            if active && index >= 0 {
                st.index = index;
                if let Some(s) = items.iter().find(|s| s.index == index) {
                    st.station = s.name.clone();
                }
                st.track = String::new();
            }
            m.refresh_at = Some(m.time + 0.15);
        }
    }
    m.radio_state = Some(st.clone());
    let mut rows = Vec::new();
    for s in &items {
        let logo = s.logo.clone();
        let mut o = RowOpts {
            icon_w: Some(150.0),
            icon: Some(Rc::new(move |rc, x, _y, w, h| match &logo {
                None => Handle::null(),
                Some(l) => {
                    let sz = logo_size(&l.name).unwrap_or((300.0, 180.0));
                    fit(rc, x + w / 2.0, h / 2.0, w, h - 16.0, l.atlas, l.part, sz.0, sz.1, k::WHITE, 1.0)
                }
            })),
            ..Default::default()
        };
        if st.active && s.index == st.index {
            (o.selected, o.eq, o.right_w) = (true, true, Some(90.0));
        }
        let idx = s.index;
        rows.push(list_row(
            "p1",
            s.name.clone(),
            Rc::new(move |m, _| {
                d::tune(idx);
                m.radio_want = Some((true, idx, m.time));
                build_panels(m, false, false);
            }),
            o,
        ));
    }
    glass(m, p, SW, 1120.0, Some((P1X, P1Y)));
    let t = m.tx("radio");
    header(p, SW, &t);
    let keep = m.keep_scroll;
    let v = list(m, p, "p1", "p1", P1X, P1Y, HEAD + 20.0, P1W - 2.0 * PAD, rows, 860.0, keep);
    v.borrow_mut().enable_mask();
    k::rect(p, P2X - P1X - 30.0, HEAD + 20.0, 2.0, 980.0, k::CYAN, 0.2);
    let (rx, rw) = (P2X - P1X - 14.0, 628.0);
    let p2 = ink::canvas(p, rx, 0.0, rw, 1120.0);
    let h2 = at(P1X + rx, P1Y);
    let mut y = HEAD + 20.0;
    let title = if st.active && !st.station.is_empty() { st.station.clone() } else { m.tx("radio") };
    k::text(&p2, PAD, y, rw - 2.0 * PAD, 60.0, &k::clip(&k::upper(&title), 23), 36.0, k::YELLOW, "center", false);
    y += 76.0;
    let on = st.active;
    let label = format!("{}: {}", m.tx("radio"), if on { m.tx("on") } else { m.tx("off") });
    action(
        m,
        &p2,
        "p2",
        &h2,
        PAD,
        y,
        rw - 2.0 * PAD,
        ROW + 10.0,
        &label,
        true,
        Rc::new(move |m, _| {
            if on {
                d::radio_off();
                m.radio_want = Some((false, -1, m.time));
            } else {
                let list = d::stations();
                let index = m.last_station.or(list.first().map(|s| s.index)).unwrap_or(0);
                d::tune(index);
                m.radio_want = Some((true, index, m.time));
            }
            build_panels(m, false, false);
        }),
        Some(k::CYAN),
        glyph_of("radio"),
    );
    if on && st.index >= 0 {
        m.last_station = Some(st.index);
    }
    y += ROW + 30.0;
    let vol = d::volume();
    let vtext = format!("{}   {}", m.tx("volume"), vol.map_or("--".to_string(), |v| super::lobby_holo::fmt_num(v.0)));
    let vol_label = k::text(&p2, PAD, y, rw - 2.0 * PAD, 60.0, &vtext, 40.0, k::TEXT, "center", false);
    y += 64.0;
    let slider_ref: Rc<std::cell::RefCell<Option<BtnRef>>> = Rc::new(std::cell::RefCell::new(None));
    let vol_word = m.tx("volume");
    let (sr, vl) = (slider_ref.clone(), vol_label.clone());
    let apply: Rc<dyn Fn(&mut Menu, f64) -> Option<f64>> = Rc::new(move |_m, n| {
        let got = d::set_volume(n)?;
        ink::set_text(&vl, &format!("{vol_word}   {}", super::lobby_holo::fmt_num(got)));
        if let Some(s) = sr.borrow().as_ref() {
            if let Some(show) = s.show.borrow().as_ref() {
                show(got);
            }
        }
        Some(got)
    });
    const SB: f64 = 104.0;
    let cur = vol.map(|v| v.0);
    let (a1, s1) = (apply.clone(), slider_ref.clone());
    step_button(
        m,
        &p2,
        "p2",
        &h2,
        PAD,
        y,
        SB,
        ROW - 14.0,
        Err(false),
        vol.is_some(),
        Rc::new(move |m, _| {
            let v = s1.borrow().as_ref().and_then(|s| s.value.get()).or(cur).unwrap_or(0.0);
            a1(m, v - 1.0);
        }),
    );
    let (a2, s2) = (apply.clone(), slider_ref.clone());
    step_button(
        m,
        &p2,
        "p2",
        &h2,
        rw - PAD - SB,
        y,
        SB,
        ROW - 14.0,
        Err(true),
        vol.is_some(),
        Rc::new(move |m, _| {
            let v = s2.borrow().as_ref().and_then(|s| s.value.get()).or(cur).unwrap_or(0.0);
            a2(m, v + 1.0);
        }),
    );
    if let Some((v, lo, hi)) = vol {
        let sx = PAD + SB + 44.0;
        let s = slider(m, &p2, "p2", &h2, sx, y, rw - 2.0 * sx, ROW - 14.0, lo, hi, Some(v), apply, None, None);
        *slider_ref.borrow_mut() = Some(s);
    }
    y += ROW + 16.0;
    if on && !st.track.is_empty() {
        let pl = m.tx("playing");
        k::text(&p2, 0.0, y, rw, 56.0, &pl, 36.0, k::CYAN, "center", false);
        k::text(&p2, 0.0, y + 56.0, rw, 70.0, &k::clip(&st.track, 28), 44.0, k::CYAN, "center", true);
    }
    if on {
        if let Some(l) = items.iter().find(|s| s.index == st.index).and_then(|s| s.logo.clone()) {
            let sz = logo_size(&l.name).unwrap_or((300.0, 180.0));
            fit(&p2, rw / 2.0, 820.0, rw - 120.0, 230.0, l.atlas, l.part, sz.0, sz.1, k::WHITE, 1.0);
        }
    }
}
fn pause_page(m: &mut Menu, event: Option<&str>) {
    let ctrl = m.shared.menu_controller.clone();
    if ctrl.is_null() {
        let t = m.tx("noMenu");
        toast(m, &t);
        return;
    }
    close(m);
    if d::menu_event(&ctrl, "OnOpenPauseMenu") {
        if let Some(e) = event {
            m.pending = Some((m.time + 0.3, e.to_string()));
        }
    }
}
fn build_game(m: &mut Menu, p: &Handle) {
    struct It {
        key: &'static str,
        label: String,
        event: Option<&'static str>,
        section: Option<&'static str>,
        confirm: Option<String>,
        go: Option<fn() -> bool>,
    }
    let mut items = vec![It { key: "save", label: d::label("UI-ResourceExports-SaveGame", &m.tx("save")), event: Some("OnSwitchToSaveGame"), section: None, confirm: None, go: None }];
    if d::saves_count() > 0 {
        items.push(It { key: "load", label: d::label("UI-ScriptExports-LoadGame0", &m.tx("load")), event: None, section: Some("load"), confirm: None, go: None });
    }
    items.push(It { key: "cog", label: d::label("UI-Labels-Settings", &m.tx("options")), event: Some("OnSwitchToSettings"), section: None, confirm: None, go: None });
    items.push(It { key: "credits", label: d::label("UI-Labels-Credits", &m.tx("credits")), event: Some("OnCreditsPicker"), section: None, confirm: None, go: None });
    items.push(It { key: "exit", label: d::label("UI-Labels-ExitToMenu", &m.tx("exit")), event: None, section: None, confirm: Some(m.tx("confirmExit")), go: Some(d::exit_to_main_menu) });
    items.push(It { key: "quit", label: d::label("UI-Labels-CloseGame", &m.tx("quit")), event: None, section: None, confirm: Some(m.tx("confirmQuit")), go: Some(d::exit_game) });
    let armed_key = m.armed_until.as_ref().filter(|(_, u)| m.time < *u).map(|(k0, _)| k0.clone());
    let mut rows = Vec::new();
    for it in items {
        let g = glyph_of(it.key).expect("menu glyph");
        let is_armed = armed_key.as_deref() == Some(it.key);
        let o = RowOpts { icon_w: Some(84.0), tint_icon: true, icon: Some(Rc::new(move |rc, x, _y, w, h| glyph(rc, x + w / 2.0, h / 2.0, h * 1.1, g, k::CYAN, 1.0))), selected: is_armed, ..Default::default() };
        let label = if is_armed { it.confirm.clone().unwrap_or(it.label.clone()) } else { it.label.clone() };
        let (key, event, section, go) = (it.key, it.event, it.section, it.go);
        rows.push(list_row(
            "p1",
            label,
            Rc::new(move |m, _| {
                if let Some(s) = section {
                    open_section(m, Some(s), true);
                } else if go.is_none() {
                    pause_page(m, event);
                } else if m.armed_until.as_ref().is_some_and(|(k0, u)| k0 == key && m.time < *u) {
                    m.armed_until = None;
                    close(m);
                    if let Some(f) = go {
                        f();
                    }
                } else {
                    m.armed_until = Some((key.into(), m.time + 3.0));
                    m.refresh_at = Some(m.time + 3.05);
                    build_panels(m, false, false);
                    play(m, "notify");
                }
            }),
            o,
        ));
    }
    let t = m.tx("game");
    list_panel(m, p, &t, rows);
}
fn drop_gestures(m: &mut Menu) {
    for p in m.hands.iter_mut() {
        p.stick.cancel();
        (p.drag, p.drag_by, p.pend, p.scroll, p.touch) = (None, None, None, None, None);
    }
}
fn build_tabs(m: &mut Menu) {
    let group = group_of(m.section.as_deref());
    let mut tabs: Vec<(&str, &str, &str, bool)> = match group {
        Some("person") => vec![("inventory", "СНАРЯЖЕНИЕ", "EQUIPMENT", false), ("character", "ПЕРСОНАЖ", "CHARACTER", false), ("cyberware", "КИБЕРИМПЛАНТЫ", "CYBERWARE", false), ("crafting", "СОЗДАНИЕ", "CRAFTING", false)],
        Some("quests") => vec![("journal", "КВЕСТЫ", "QUESTS", false), ("codex", "БАЗА ДАННЫХ", "DATABASE", true), ("tarot", "КАРТЫ ТАРО", "TAROT CARDS", false)],
        _ => return,
    };
    if group == Some("person") {
        if super::menu_vendor::current_ripperdoc(m).is_some() {
            tabs.push(("trade", "ТОРГОВЛЯ", "TRADE", false));
        }
        if super::menu_stash::current(m) {
            tabs.push(("stash", "ТАЙНИК", "STASH", false));
        }
    }
    let (width, y) = (1780.0, 174.0);
    let host = ink::canvas(&m.layer_panel, P1X, y, width, 64.0);
    let gap = 20.0;
    let tw = (width - 24.0 - gap * (tabs.len() as f64 - 1.0)) / tabs.len() as f64;
    for (i, (key, ru, en, shards)) in tabs.into_iter().enumerate() {
        let x = 12.0 + i as f64 * (tw + gap);
        let selected = m.section.as_deref() == Some(key) || (shards && m.section.as_deref() == Some("shards"));
        let tab = ink::canvas(&host, x, 0.0, tw, 64.0);
        glass(m, &tab, tw, 64.0, Some((P1X + x, y)));
        let bg = k::round_fill(&tab, 6.0, 5.0, tw - 12.0, 54.0, if selected { k::YELLOW } else { k::NAVY }, 0.12, Some(3.0));
        let mut text = if key == "crafting" { super::lobby_menu::label("UI-PanelNames-CRAFTING", "") } else { String::new() };
        if text.is_empty() {
            text = super::menu_locale::tr(ru, en);
        }
        let label = k::text(&tab, 8.0, 4.0, tw - 16.0, 56.0, &k::upper(&text), 30.0, if selected { k::YELLOW } else { k::TEXT }, "center", false);
        if selected {
            k::rect(&tab, 14.0, 59.0, tw - 28.0, 2.0, k::YELLOW, 1.0);
        }
        let k2 = key;
        let b = button(
            m,
            "tabs",
            &at(P1X, y),
            x,
            4.0,
            tw,
            56.0,
            Some(Rc::new(move |m, _| {
                if k2 == "trade" {
                    open_trade(m);
                } else if m.section.as_deref() != Some(k2) {
                    open_section(m, Some(k2), true);
                }
            })),
            Some(Rc::new(move |h, _| {
                ink::tint(&bg, if h || selected { k::YELLOW } else { k::NAVY });
                ink::opacity(&bg, if h { 0.18 } else { 0.12 });
                ink::tint(&label, if h || selected { k::YELLOW } else { k::TEXT });
            })),
        );
        *b.key.borrow_mut() = Some(format!("tab:{key}"));
    }
}
fn build_summary(m: &mut Menu) {
    let show = group_of(m.section.as_deref()) == Some("person");
    if show == m.summary_visible {
        return;
    }
    m.summary_visible = show;
    drop_anims(m, "summary");
    let layer = m.layer_summary.clone();
    if show {
        ink::visible(&layer, true);
        super::menu_gameplay::summary(m, &layer);
        slide_in(m, "summary", &layer, 0.0, 0.0, 0.0, -28.0, 0.0, None);
    } else {
        clear_buttons(m, "summary");
        ink::remove_all(&layer);
        ink::visible(&layer, false);
        m.summary_rect = None;
    }
}
/// build.panels: the open section's panels drawn again (the scroll kept when it is the same section).
pub(super) fn build_panels(m: &mut Menu, anim1: bool, anim2: bool) {
    super::menu_gameplay::before_build(m);
    let l = m.layer_panel.clone();
    let revealing = m.section.is_some() && m.time < m.panel_reveal_end;
    m.keep_scroll = m.view("p1").filter(|v| v.section == m.section).map(|v| v.v.borrow().offset);
    ink::remove_all(&l);
    for g in ["p1", "p2", "set", "sethead", "tabs", "sort", "attributePrompt"] {
        clear_buttons(m, g);
    }
    m.modal_group = None;
    drop_anims(m, "p1");
    drop_anims(m, "p2");
    drop_gestures(m);
    super::menu_setview::leave(m);
    m.views.clear();
    m.glass_rects.clear();
    m.hovered.clear();
    m.anim1 = anim1;
    build_summary(m);
    m.pool.begin();
    let cm = m.column_move.map_or(0.0, |(_, _, t0)| t0 + COLUMN_SHIFT);
    m.panel_interactive_at = m.time.max(cm).max(if revealing { m.panel_reveal_end } else { 0.0 });
    if anim1 && m.section.is_some() {
        drop_anims(m, "panelRoot");
        m.panel_reveal_end = m.time.max(m.panel_start.unwrap_or(m.time)) + ANIM;
        slide_in(m, "panelRoot", &l, 0.0, 0.0, 0.0, 30.0, 0.0, None);
    } else if !revealing {
        drop_anims(m, "panelRoot");
        m.panel_reveal_end = 0.0;
        ink::opacity(&l, 1.0);
        ink::move_to(&l, 0.0, 0.0);
    }
    let section = m.section.clone();
    if section.as_deref() == Some("settings") {
        let ps = ink::canvas(&l, P1X, P1Y, SW, CH - P1Y);
        super::menu_setview::build(m, &ps, P1X, P1Y, SW);
        if anim1 {
            slide_in(m, "p1", &ps, P1X, P1Y, 0.0, 0.0, 30.0, None);
        }
    } else {
        let p1 = ink::canvas(&l, P1X, P1Y, P1W, CH - P1Y);
        let p2 = ink::canvas(&l, P2X, P1Y, super::menu_messages::THREAD_W, CH - P1Y);
        match section.as_deref() {
            Some("glamdoc") => {
                let host = ink::canvas(&l, super::menu_glamdoc::X, P1Y, 1780.0, CH - P1Y);
                super::menu_glamdoc::draw(m, &host);
            }
            Some("appearance") => {
                // beside the mirror the menu opened centred on, on the side of the dominant hand
                let ax = if super::belt::left_handed() { CW - P2X - P1W } else { P2X };
                let host = ink::canvas(&l, ax, P1Y, P1W, CH - P1Y);
                super::menu_appearance::draw(m, &host, ax);
            }
            Some("load" | "stash" | "trade" | "cyberware" | "crafting" | "inventory" | "character" | "journal" | "codex" | "shards" | "tarot" | "timeskip") => super::menu_gameplay::build(m, &p1, &p2),
            Some("contacts") => super::menu_messages::build(m, &p1, &p2),
            Some("vehicles") => {
                build_vehicles(m, &p1);
                build_vehicle_detail(m, &p2);
            }
            Some("radio") => build_radio(m, &p1),
            Some("game") => build_game(m, &p1),
            _ => {}
        }
        if anim2 || (anim1 && section.as_deref() == Some("radio")) {
            slide_in(m, "p2", &p2, P2X, P1Y, if anim1 { 0.12 } else { 0.0 }, 50.0, 0.0, None);
        }
    }
    build_tabs(m);
    m.toast_bg = k::round_fill(&l, P1X, TOAST_Y, P1W, 70.0, k::ICE, 0.08, Some(35.0));
    m.toast_widget = k::text(&l, P1X + 30.0, TOAST_Y, P1W - 60.0, 70.0, "", 44.0, k::YELLOW, "left", false);
    let live = m.toast.as_ref().filter(|(_, u)| m.time < *u).map(|(t, _)| t.clone());
    ink::visible(&m.toast_widget, live.is_some());
    ink::visible(&m.toast_bg, live.is_some());
    if let Some(t) = live {
        ink::set_text(&m.toast_widget, &t);
    }
    m.pool.end();
}
fn build_all(m: &mut Menu) {
    let Some(root) = m.menu.as_ref().map(|s| s.root.clone()) else { return };
    ink::remove_all(&root);
    m.pool.flush();
    (m.buttons, m.hovered, m.anims, m.views) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    m.surface_warmup = k::rect(&root, 0.0, 0.0, 1.0, 1.0, k::BLACK, 1.0);
    m.layer_col = ink::canvas(&root, m.column_offset, 0.0, CW, CH);
    m.layer_summary = ink::canvas(&root, 0.0, 0.0, 1000.0, CH);
    m.summary_visible = false;
    ink::visible(&m.layer_summary, false);
    m.layer_panel = ink::canvas(&root, 0.0, 0.0, CW, CH);
    let top = ink::canvas(&root, 0.0, 0.0, CW, CH);
    m.cursors.clear();
    m.press_fx.clear();
    for _ in 0..2 {
        let host = ink::canvas(&top, 0.0, 0.0, 64.0, 64.0);
        ink::v2(&host, "SetRenderTransformPivot", 0.0, 0.0);
        ink::opacity(&host, 0.0);
        let inner = k::ring(&host, 32.0, 32.0, 17.0, 2.0, k::CYAN, 0.9, 28, None);
        let outer = k::ring(&host, 32.0, 32.0, 26.0, 1.5, k::CYAN, 0.4, 32, Some(4));
        let dot = k::image(&host, 29.0, 29.0, 6.0, 6.0, k::ATLAS_PHONE, "circle", Some(k::WHITE), 1.0);
        m.cursors.push(Cursor { host, inner, outer, dot, color: None, x: 0.0, y: 0.0 });
        let fx = ink::canvas(&top, 0.0, 0.0, 96.0, 96.0);
        ink::v2(&fx, "SetRenderTransformPivot", 0.5, 0.5);
        ink::opacity(&fx, 0.0);
        k::ring(&fx, 48.0, 48.0, 22.0, 3.0, k::YELLOW, 0.95, 32, None);
        k::ring(&fx, 48.0, 48.0, 32.0, 2.0, k::YELLOW, 0.65, 36, None);
        k::ring(&fx, 48.0, 48.0, 42.0, 1.0, k::YELLOW, 0.3, 40, None);
        m.press_fx.push(PressFx { host: fx, x: 0.0, y: 0.0, at: None });
    }
    if standalone(m) {
        clear_buttons(m, "col");
        m.icons = Vec::new();
        (m.money_widget, m.weight_widget, m.resource_rect) = (Handle::null(), Handle::null(), None);
    } else {
        build_column(m);
    }
    let anim = m.section.is_some();
    build_panels(m, anim, false);
    m.open_anim_at = Some(m.time);
    if let Some(s) = m.menu.as_mut() {
        s.built = true;
    }
}

// ---------------------------------------------------------------- the beams
const BEAM_LEN: f64 = 1.5;
const BCW: f64 = 16.0;
const BCH: f64 = 2048.0;
fn build_beam(h: &mut Surf) {
    ink::remove_all(&h.root);
    h.strip.clear();
    for kk in 1..=7usize {
        let f = if kk <= 6 { kk as f64 / 7.0 } else { 1.0 };
        h.strip.push((kk, 0.0, BCW, k::rect(&h.root, 0.0, 0.0, BCW, 0.0, k::CYAN, 0.25 * f)));
        h.strip.push((kk, (BCW - 6.0) / 2.0, 6.0, k::rect(&h.root, (BCW - 6.0) / 2.0, 0.0, 6.0, 0.0, k::WHITE, 0.9 * f)));
    }
    h.strip_px = -1.0;
    h.built = true;
}
fn beam_length(h: &mut Surf, length: f64) {
    let per_m = BCH / BEAM_LEN;
    let px = length.clamp(0.0, BEAM_LEN) * per_m;
    if (px - h.strip_px).abs() < 1.0 {
        return;
    }
    h.strip_px = px;
    let (hide, step) = (0.12 * per_m, 0.12 * per_m / 6.0);
    for (kk, x, w, r) in &h.strip {
        let top = hide + (*kk as f64 - 1.0) * step;
        let bottom = if *kk <= 6 { top + step } else { px };
        ink::move_to(r, *x, top);
        ink::size(r, *w, (bottom.min(px) - top).max(0.0));
    }
}
fn beam_show(h: &Surf, on: bool) {
    for (_, _, _, r) in &h.strip {
        ink::visible(r, on);
    }
}
fn beam(m: &mut Menu, i: usize, show: bool, origin: V3, hit: V3, length: f64, cam: &Cam) {
    let mut h = std::mem::take(&mut m.beams[i]);
    let ok = resolve(m, &mut h);
    if ok {
        if !h.built {
            build_beam(&mut h);
        }
        if ready(&mut h) {
            unglitch(&h);
            if !show {
                if h.shown != Some(false) {
                    beam_show(&h, false);
                    h.shown = Some(false);
                }
            } else {
                let dir = norm(sub(hit, origin));
                let to_cam = sub(cam.p, origin);
                let n0 = sub(to_cam, mul(dir, dot(to_cam, dir)));
                if dot(n0, n0) > 1e-12 {
                    let n = norm(n0);
                    let r = cross(n, dir);
                    let (p, q) = pose(add(origin, mul(dir, BEAM_LEN / 2.0)), r, mul(dir, -1.0));
                    place(&h, p, q);
                    beam_length(&mut h, length);
                    if h.shown != Some(true) {
                        beam_show(&h, true);
                        h.shown = Some(true);
                    }
                }
            }
        }
    }
    m.beams[i] = h;
}
fn capture(on: bool) {
    sp::native::<bool>("VRMenuCapture", &[Value::Bool(on)]);
}

// ---------------------------------------------------------------- opening and closing
/// M.open: the menu before the eyes (an initial section open); false when it cannot open now.
pub(super) fn open(m: &mut Menu, cam: Option<Cam>, initial: Option<&str>) -> bool {
    if !super::menu_vendor::interface_enabled() {
        return false;
    }
    if m.wait_transition || pre_game() || m.fbt_calibrating {
        return false;
    }
    if super::menu_appearance::current() && initial != Some("appearance") {
        return false;
    }
    if super::menu_glamdoc::open_session() && initial != Some("glamdoc") {
        return false;
    }
    if initial == Some("glamdoc") && super::menu_glamdoc::closed_pending() {
        return false;
    }
    let Some(cam) = cam.or_else(camera) else { return false };
    if m.open {
        close(m);
    }
    super::menu_invdata::refresh_weight();
    if !m.cap_on && sp::native::<()>("SetVRDiagCapture", &[Value::Int32(1)]).is_some() {
        m.cap_on = true;
    }
    m.anchor = Some(anchor_for(&cam));
    m.anchor_gen += 1;
    (m.open, m.section, m.toast, m.close_at) = (true, initial.map(str::to_string), None, None);
    (m.vehicles, m.stations, m.vehicle) = (None, None, None);
    sp::native::<()>("VRMenuModOpen", &[Value::Bool(true)]);
    let column = if initial.is_some() { 0.0 } else { COL_HOME };
    (m.column_offset, m.column_target, m.column_move) = (column, column, None);
    (m.panel_start, m.panel_interactive_at, m.open_anim_at) = (Some(m.time), m.time, None);
    m.panel_reveal_end = 0.0;
    super::menu_messages::leave(m);
    super::menu_gameplay::leave(m);
    m.lang = m.lang_override.clone().unwrap_or_else(d::language);
    super::menu_locale::set(&m.lang);
    m.volume = d::effects_volume();
    m.t = texts();
    let Some((c, r, u)) = frame(m) else { return false };
    if m.menu.as_ref().is_none_or(|s| s.id.is_none()) {
        let (p, q) = pose(c, r, u);
        m.menu = Some(spawn(m, ENT, p, q));
    }
    for i in 0..2 {
        if m.beams[i].id.is_none() {
            let (p, q) = pose(c, r, u);
            m.beams[i] = spawn(m, BEAM_ENT, p, q);
        }
    }
    capture(true);
    play(m, "open");
    super::menu_gameplay::prefetch(m);
    true
}
pub(super) fn close(m: &mut Menu) {
    m.column_action = None;
    if !m.open {
        return;
    }
    match m.section.as_deref() {
        Some("appearance") => {
            super::menu_appearance::cancel();
        }
        Some("glamdoc") => {
            super::menu_glamdoc::finish(false);
        }
        _ => {}
    }
    super::menu_vendor::close_sessions(m);
    m.open = false;
    sp::native::<()>("VRMenuModOpen", &[Value::Bool(false)]);
    m.summary_rect = None;
    m.summary_visible = false;
    m.resource_rect = None;
    (m.section, m.vehicles, m.stations, m.vehicle) = (None, None, None, None);
    super::menu_messages::leave(m);
    super::menu_gameplay::leave(m);
    super::menu_setview::leave(m);
    if let Some(mut s) = m.menu.take() {
        despawn(m, &mut s);
    }
    for i in 0..2 {
        let mut b = std::mem::take(&mut m.beams[i]);
        despawn(m, &mut b);
    }
    (m.buttons, m.hovered, m.anims, m.views) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    (m.icons, m.column_move, m.open_anim_at) = (Vec::new(), None, None);
    (m.money_widget, m.weight_widget) = (Handle::null(), Handle::null());
    drop_gestures(m);
    despawn_glass(m);
    capture(false);
    play(m, "close");
}
pub(super) fn open_trade(m: &mut Menu) -> bool {
    if super::menu_vendor::current_ripperdoc(m).is_none() {
        let t = m.tx("blocked");
        toast(m, &t);
        return false;
    }
    if super::menu_vendor::purchasing(m) {
        let t = super::menu_locale::tr("Покупка ещё выполняется", "Purchase is still in progress");
        toast(m, &t);
        return false;
    }
    open_section(m, Some("trade"), true);
    true
}
pub(super) fn open_map(m: &mut Menu) -> bool {
    if standalone(m) {
        return false;
    }
    let ctrl = m.shared.menu_controller.clone();
    if !d::open_map(&ctrl) {
        let t = m.tx("blocked");
        toast(m, &t);
        return false;
    }
    close(m);
    true
}

// ---------------------------------------------------------------- the pointers
fn set_cursor(m: &mut Menu, i: usize, x: f64, y: f64, size: f64, col: [f64; 3], op: f64) {
    let Some(c) = m.cursors.get_mut(i) else { return };
    if op <= 0.0 {
        ink::opacity(&c.host, 0.0);
        return;
    }
    let span = 52f64.max(size * 1.5);
    (c.x, c.y) = (x, y);
    ink::move_to(&c.host, x - span / 2.0, y - span / 2.0);
    ink::v2(&c.host, "SetScale", span / 64.0, span / 64.0);
    if c.color != Some(col) {
        k::tint(&c.inner, col, Some(0.95));
        k::tint(&c.outer, col, Some(0.45));
        ink::tint(&c.dot, col);
        c.color = Some(col);
    }
    ink::opacity(&c.host, op);
}
fn invoke_click(m: &mut Menu, b: &BtnRef, hand: usize) {
    (m.in_click, m.sound_in_click) = (true, false);
    if let Some(c) = b.click.clone() {
        c(m, hand);
    }
    m.in_click = false;
    if !b.mute.get() && !m.sound_in_click {
        play(m, "click");
    }
}
fn click(m: &mut Menu, b: Option<BtnRef>, hand: usize, x: Option<f64>, y: Option<f64>) {
    let Some(b) = b.filter(|b| b.click.is_some()) else { return };
    if m.column_action.is_some() || m.time - m.last_click < 0.15 {
        return;
    }
    m.last_click = m.time;
    haptic(m, hand, HAPTIC_CLICK);
    if b.group == "col" {
        b.pulse_at.set(Some(m.time));
    }
    let cur = m.cursors.get(hand - 1).map(|c| (c.x, c.y));
    if let Some(fx) = m.press_fx.get_mut(hand - 1) {
        if let Some((cx, cy)) = x.zip(y).or(cur) {
            (fx.x, fx.y, fx.at) = (cx, cy, Some(m.time));
        }
    }
    let key = b.key();
    if b.group == "col" && matches!(key.as_deref(), Some("close") | Some("map")) {
        m.column_action = Some((b, m.time + 0.10));
    } else {
        invoke_click(m, &b, hand);
    }
}
fn update_press_fx(m: &mut Menu) {
    for b in m.icons.clone() {
        let Some(at0) = b.pulse_at.get() else { continue };
        let ib = b.icon.borrow();
        let Some(ib) = ib.as_ref() else { continue };
        let t = (m.time - at0) / 0.26;
        if t >= 1.0 {
            b.pulse_at.set(None);
            ink::rotation(&ib.ring_host, 0.0);
            ink::v2(&ib.ring_host, "SetScale", 1.0, 1.0);
            let (col, o, i) = ib.visual.get();
            k::tint(&ib.ring, col, Some(o));
            k::tint(&ib.outer, col, Some(i));
        } else {
            let scale = 1.0 + 0.07 * (std::f64::consts::PI * t).sin();
            ink::v2(&ib.ring_host, "SetScale", scale, scale);
            ink::rotation(&ib.ring_host, 45.0 * (1.0 - (1.0 - t).powi(3)));
            let n = ib.ring.len() as f64;
            for (i, seg) in ib.ring.iter().enumerate() {
                let distance = (i as f64 / n - t * 1.5).rem_euclid(1.0);
                ink::tint(seg, k::YELLOW);
                ink::opacity(seg, 0.35 + 0.65 * (1.0 - distance / 0.3).max(0.0));
            }
            k::tint(&ib.outer, k::YELLOW, Some(0.95));
        }
    }
    let time = m.time;
    for i in 0..m.press_fx.len() {
        let Some(at0) = m.press_fx[i].at else { continue };
        let t = (time - at0) / 0.24;
        let (x, y) = (m.press_fx[i].x, m.press_fx[i].y);
        if t >= 1.0 || button_at(m, x, y).is_none() {
            m.press_fx[i].at = None;
            ink::opacity(&m.press_fx[i].host, 0.0);
        } else {
            let fx = &m.press_fx[i];
            ink::move_to(&fx.host, x - 48.0, y - 48.0);
            let scale = 0.65 + 0.6 * t;
            ink::v2(&fx.host, "SetScale", scale, scale);
            ink::opacity(&fx.host, (1.0 - t).powi(2));
        }
    }
}
fn drag_call(m: &mut Menu, b: &BtnRef, px: Option<f64>, phase: &str, hand: usize, py: Option<f64>) {
    let f = b.drag.borrow().clone();
    if let Some(f) = f {
        f(m, px, phase, hand, py);
    }
}
fn take_slider(m: &mut Menu, hand: usize, b: BtnRef, by: &'static str, x: f64, y: f64) {
    (m.hands[hand - 1].drag, m.hands[hand - 1].drag_by) = (Some(b.clone()), Some(by));
    haptic(m, hand, HAPTIC_CLICK);
    drag_call(m, &b, Some(x), "begin", hand, Some(y));
}
fn push_slope(t: &Touch) -> (f64, f64) {
    let dd = t.dd - t.d0;
    if dd.abs() < 0.004 {
        return (0.0, 0.0);
    }
    let (mut kx, mut ky) = ((t.xd - t.x0) / dd, (t.yd - t.y0) / dd);
    let (mg, cap) = ((kx * kx + ky * ky).sqrt(), SLOPE_MAX * PX);
    if mg > cap {
        (kx, ky) = (kx * cap / mg, ky * cap / mg);
    }
    (kx, ky)
}
fn press(m: &mut Menu, b: Option<BtnRef>, hand: usize, x: f64, y: f64, by: &'static str, d: Option<f64>) {
    m.hands[hand - 1].touch = d.map(|d| Touch { by, x0: x, y0: y, d0: d, dd: d, xd: x, yd: y, kx: 0.0, ky: 0.0, settled: false });
    if let Some(view) = b.as_ref().and_then(|b| b.view.clone()) {
        m.hands[hand - 1].pend = Some(Pend { b, view, x0: x, y0: y, by, t0: m.time });
    } else if let Some(bb) = b.clone().filter(|b| b.drag.borrow().is_some()) {
        take_slider(m, hand, bb, by, x, y);
    } else {
        click(m, b, hand, Some(x), Some(y));
    }
}
fn move_(m: &mut Menu, hand: usize, x: f64, y: f64, by: &'static str, d: Option<f64>) {
    let (mut x, mut y) = (x, y);
    let mut tk = None;
    if let (Some(t), Some(d)) = (m.hands[hand - 1].touch.as_mut().filter(|t| t.by == by), d) {
        if !t.settled && d < t.dd {
            let rx = x - t.kx * (d - t.d0) - t.x0;
            let ry = y - t.ky * (d - t.d0) - t.y0;
            if t.d0 - d < LINE_START || rx.abs().max(ry.abs()) < tap(by) * 0.75 {
                (t.dd, t.xd, t.yd) = (d, x, y);
                let (kx, ky) = push_slope(t);
                (t.kx, t.ky) = (kx, ky);
            }
        }
        (x, y) = (x - t.kx * (d - t.d0), y - t.ky * (d - t.d0));
        if !t.settled && (x - t.x0).abs().max((y - t.y0).abs()) > tap(by) {
            t.settled = true;
        }
        tk = Some((t.kx, t.ky));
    }
    let _ = tk;
    if m.hands[hand - 1].drag_by == Some(by) {
        if let Some(b) = m.hands[hand - 1].drag.clone() {
            drag_call(m, &b, Some(x), "move", hand, Some(y));
        }
    }
    let time = m.time;
    if let Some(g) = m.hands[hand - 1].pend.as_mut().filter(|g| g.by == by) {
        if by == "trigger" && time - g.t0 < SETTLE {
            (g.x0, g.y0) = (x, y);
        }
    }
    let pend = m.hands[hand - 1].pend.as_ref().filter(|g| g.by == by).map(|g| (g.b.clone(), g.view.clone(), g.x0, g.y0));
    if let Some((gb, gv, x0, y0)) = pend {
        let (dx, dy) = (x - x0, y - y0);
        let along = dx.abs().max(dy.abs());
        if along > tap(by) {
            m.hands[hand - 1].pend = None;
            match gb.filter(|b| b.drag.borrow().is_some() && dx.abs() > dy.abs()) {
                Some(b) => {
                    take_slider(m, hand, b.clone(), by, x0, y0);
                    drag_call(m, &b, Some(x), "move", hand, Some(y));
                }
                None => {
                    if gv.borrow().scrollable() {
                        gv.borrow_mut().begin_drag(y0);
                        m.hands[hand - 1].scroll = Some((gv, by));
                    }
                }
            }
        }
    }
    if let Some((v, _)) = m.hands[hand - 1].scroll.clone().filter(|s| s.1 == by) {
        v.borrow_mut().drag_to(y, time);
    }
}
fn finish(m: &mut Menu, hand: usize, by: &'static str, cancel: bool) {
    if m.hands[hand - 1].touch.as_ref().is_some_and(|t| t.by == by) {
        m.hands[hand - 1].touch = None;
    }
    if m.hands[hand - 1].drag_by == Some(by) {
        if let Some(b) = m.hands[hand - 1].drag.take() {
            m.hands[hand - 1].drag_by = None;
            drag_call(m, &b, None, "end", hand, None);
        }
    }
    if let Some(g) = m.hands[hand - 1].pend.take() {
        if g.by == by {
            if !cancel {
                if let Some(b) = g.b {
                    if b.drag.borrow().is_some() {
                        take_slider(m, hand, b.clone(), by, g.x0, g.y0);
                        (m.hands[hand - 1].drag, m.hands[hand - 1].drag_by) = (None, None);
                        drag_call(m, &b, None, "end", hand, None);
                    } else {
                        click(m, Some(b), hand, Some(g.x0), Some(g.y0));
                    }
                }
            }
        } else {
            m.hands[hand - 1].pend = Some(g);
        }
    }
    if let Some((v, sby)) = m.hands[hand - 1].scroll.clone() {
        if sby == by {
            m.hands[hand - 1].scroll = None;
            v.borrow_mut().end_drag();
        }
    }
}
fn busy_with(p: &HandP, by: &str) -> bool {
    p.drag_by == Some(by) || p.pend.as_ref().is_some_and(|g| g.by == by) || p.scroll.as_ref().is_some_and(|s| s.1 == by)
}
fn pointers(m: &mut Menu, cam: &Cam, c: V3, r: V3, u: V3) {
    let vs = [[c[0] as f32, c[1] as f32, c[2] as f32, 1.0], [r[0] as f32, r[1] as f32, r[2] as f32, 0.0], [u[0] as f32, u[1] as f32, u[2] as f32, 0.0], [W as f32, H as f32, 0.0, 0.0]];
    let out = sp::native_v4s("VRMenuPointer", &[Value::Ptr(vs[0].as_ptr().cast()), Value::Ptr(vs[1].as_ptr().cast()), Value::Ptr(vs[2].as_ptr().cast()), Value::Ptr(vs[3].as_ptr().cast())]);
    if out.len() < 8 {
        for hand in 1..=2 {
            m.hands[hand - 1].stick.cancel();
            finish(m, hand, "finger", true);
            finish(m, hand, "trigger", true);
            set_cursor(m, hand - 1, 0.0, 0.0, 40.0, k::CYAN, 0.0);
            beam(m, hand - 1, false, [0.0; 3], [0.0; 3], 0.0, cam);
        }
        return;
    }
    let mut hover: Vec<BtnRef> = Vec::new();
    let add_hover = |hover: &mut Vec<BtnRef>, b: &BtnRef| {
        if !hover.iter().any(|h| Rc::ptr_eq(h, b)) {
            hover.push(b.clone());
        }
    };
    let right_stick = f64::from(super::belt::shared(159));
    for hand in 1..=2usize {
        let base = (hand - 1) * 4;
        let (tip, ray, origin, hit) = (out[base], out[base + 1], out[base + 2], out[base + 3]);
        let f = |v: f32| f64::from(v);
        let mut poking = false;
        let mut cursor = false;
        m.hands[hand - 1].poke = None;
        let (mut tip_x, mut tip_y) = (None, None);
        let stick = if hand == 2 { right_stick } else { 0.0 };
        let mut stick_held = m.hands[hand - 1].stick.held(stick);
        if stick_held || m.hands[hand - 1].stick.scroll.is_some() {
            finish(m, hand, "finger", true);
            finish(m, hand, "trigger", true);
            m.hands[hand - 1].state = "idle";
            m.hands[hand - 1].last_d = None;
            m.hands[hand - 1].trig = f(origin[3]) > TRIGGER_OFF;
        }
        let (mut stick_button, mut stick_mode) = (None, None);
        if tip[3] > 0.5 && f(tip[2]) < HOVER && f(tip[2]) > THROUGH {
            stick_button = button_at(m, f(tip[0]) * CW, f(tip[1]) * CH);
            stick_mode = Some("tip");
        }
        if stick_button.is_none() && ray[3] > 0.5 {
            stick_button = button_at(m, f(ray[0]) * CW, f(ray[1]) * CH);
            stick_mode = Some("ray");
        }
        let now = m.time;
        let sv = stick_button.as_ref().and_then(|b| b.view.clone());
        m.hands[hand - 1].stick.update(stick, tip[3] > 0.5 || ray[3] > 0.5, sv, stick_mode, (ray[3] > 0.5).then(|| f(ray[1]) * CH), (tip[3] > 0.5).then(|| f(tip[1]) * CH), now);
        stick_held = stick_held || m.hands[hand - 1].stick.down;
        if tip[3] > 0.5 {
            let dd = f(tip[2]);
            let inside = tip[0] >= 0.0 && tip[0] <= 1.0 && tip[1] >= 0.0 && tip[1] <= 1.0;
            let (x, y) = (f(tip[0]) * CW, f(tip[1]) * CH);
            (tip_x, tip_y) = (Some(x), Some(y));
            let b = if inside { button_at(m, x, y) } else { None };
            if m.hands[hand - 1].state.is_empty() {
                m.hands[hand - 1].state = "idle";
            }
            let state = m.hands[hand - 1].state;
            let last_d = m.hands[hand - 1].last_d;
            if !stick_held && state == "idle" {
                if b.is_some() && dd <= PRESS && dd > THROUGH && last_d.is_some_and(|l| l > PRESS) {
                    press(m, b.clone(), hand, x, y, "finger", Some(dd));
                    m.hands[hand - 1].state = "pressed";
                } else if dd <= THROUGH {
                    m.hands[hand - 1].state = "through";
                }
            } else if !stick_held && state == "pressed" && dd > RELEASE {
                m.hands[hand - 1].state = "idle";
                finish(m, hand, "finger", false);
            } else if !stick_held && state == "through" && dd > RELEASE {
                m.hands[hand - 1].state = "idle";
            }
            if !stick_held && m.hands[hand - 1].state == "pressed" {
                move_(m, hand, x, y, "finger", Some(dd));
            }
            m.hands[hand - 1].last_d = if stick_held { None } else { Some(dd) };
            m.hands[hand - 1].poke = None;
            if let Some(bb) = b.filter(|_| dd < HOVER && (dd > THROUGH || busy_with(&m.hands[hand - 1], "finger"))) {
                poking = true;
                add_hover(&mut hover, &bb);
                m.hands[hand - 1].poke = Some(bb);
                let ring = 20.0 + dd.max(0.0) * 380.0;
                let op = if m.hands[hand - 1].state == "pressed" { 1.0 } else { 0.9 };
                set_cursor(m, hand - 1, x, y, ring, k::YELLOW, op);
                cursor = true;
            }
        } else {
            m.hands[hand - 1].last_d = None;
            if m.hands[hand - 1].state == "pressed" {
                m.hands[hand - 1].state = "idle";
                finish(m, hand, "finger", true);
            }
        }
        let trigger = f(origin[3]);
        let ray_inside = !poking && !busy_with(&m.hands[hand - 1], "finger") && ray[3] > 0.5 && ray[0] >= 0.0 && ray[0] <= 1.0 && ray[1] >= 0.0 && ray[1] <= 1.0;
        let (mut rb, mut ray_x, mut ray_y) = (None, None, None);
        if ray_inside {
            let (x, y) = (f(ray[0]) * CW, f(ray[1]) * CH);
            (ray_x, ray_y) = (Some(x), Some(y));
            rb = button_at(m, x, y);
        }
        let ray_on = rb.is_some();
        if let (Some(b), Some(x), Some(y)) = (rb.clone(), ray_x, ray_y) {
            add_hover(&mut hover, &b);
            let size = if m.hands[hand - 1].trig || m.hands[hand - 1].stick.scroll.is_some() { 16.0 } else { 22.0 };
            set_cursor(m, hand - 1, x, y, size, k::YELLOW, 1.0);
            cursor = true;
        }
        if !stick_held && trigger > TRIGGER_ON && !m.hands[hand - 1].trig {
            m.hands[hand - 1].trig = true;
            if let (true, Some(x), Some(y)) = (ray_on, ray_x, ray_y) {
                press(m, rb.clone(), hand, x, y, "trigger", None);
            } else if poking {
                let pb = m.hands[hand - 1].poke.clone();
                press(m, pb, hand, tip_x.unwrap_or(0.0), tip_y.unwrap_or(0.0), "trigger", None);
            }
        } else if !stick_held && trigger < TRIGGER_OFF && m.hands[hand - 1].trig {
            m.hands[hand - 1].trig = false;
            finish(m, hand, "trigger", false);
        }
        if !stick_held && m.hands[hand - 1].trig {
            let (mut x, mut y) = (ray_x, ray_y);
            if x.is_none() && poking {
                (x, y) = (tip_x, tip_y);
            }
            if let (Some(x), Some(y)) = (x, y) {
                move_(m, hand, x, y, "trigger", None);
            }
        }
        if let Some(b) = m.hands[hand - 1].drag.clone() {
            add_hover(&mut hover, &b);
        }
        if !cursor {
            set_cursor(m, hand - 1, 0.0, 0.0, 40.0, k::CYAN, 0.0);
        }
        beam(m, hand - 1, ray_on, [f(origin[0]), f(origin[1]), f(origin[2])], [f(hit[0]), f(hit[1]), f(hit[2])], f(ray[2]), cam);
    }
    // the tour's hovered button stays hovered while its step lasts (the pointers above would let it go next frame)
    let held = TOUR_HOVER.with(|t| t.borrow().clone());
    if let Some(key) = held {
        if let Some(b) = m.buttons.iter().find(|b| b.visible.get() && b.key().as_deref() == Some(key.as_str())) {
            add_hover(&mut hover, b);
        }
    }
    let was = std::mem::take(&mut m.hovered);
    for b in &was {
        if !hover.iter().any(|h| Rc::ptr_eq(h, b)) {
            b.repaint(false);
        }
    }
    for b in &hover {
        if !was.iter().any(|h| Rc::ptr_eq(h, b)) {
            b.repaint(true);
        }
    }
    m.hovered = hover;
    if m.section.as_deref() == Some("settings") {
        let h = m.hovered.clone();
        super::menu_setview::hover(m, &h);
    }
}

// ---------------------------------------------------------------- the tick
fn tick(m: &mut Menu, dt: f64) {
    m.pool.end();
    retire_pending(m);
    if let Some((b, at0)) = m.column_action.clone() {
        if m.time >= at0 {
            m.column_action = None;
            invoke_click(m, &b, 2);
        }
    }
    trace("tick");
    if super::menu_gameplay::transition(m, dt) {
        return;
    }
    let g = sp::native_v4s("VRMenuGesture", &[]);
    if let Some(g0) = g.first() {
        if g0[0] > 1.5 || g.len() >= 3 {
            if m.open {
                if g0[0] > 1.5 || !m.hands[1].stick.blocks_swipe(m.time) {
                    close(m);
                }
            } else if scene_tier() < 3 && !paused() {
                if !armed() {
                    open(m, camera(), None);
                } else if g0[0] > 1.5 {
                    holster_all();
                    m.open_when_unarmed = Some(m.time + 2.0);
                }
            }
        }
    }
    if let (Some(until), false) = (m.open_when_unarmed, m.open) {
        if !armed() {
            m.open_when_unarmed = None;
            if scene_tier() < 3 && !paused() {
                open(m, camera(), None);
            }
        } else if m.time > until {
            m.open_when_unarmed = None;
        }
    }
    if !m.open {
        return;
    }
    trace("step");
    super::menu_gameplay::step(m);
    trace("summary");
    super::menu_gameplay::update_summary(m);
    if m.close_at.is_some_and(|t| m.time >= t) {
        close(m);
        return;
    }
    m.slow += dt;
    if m.slow > 0.25 {
        m.slow = 0.0;
        let vehicle = m.anchor.is_some_and(|a| a.vehicle);
        if (!standalone(m) && (armed() || scene_tier() >= 3)) || vehicle != !d::mounted().is_null() {
            close(m);
            return;
        }
    }
    let (Some((c, r, u)), Some(cam)) = (frame(m), camera()) else {
        close(m);
        return;
    };
    if !m.anchor.is_some_and(|a| a.vehicle) {
        let col = add(add(c, mul(r, (COLX + m.column_offset - CW / 2.0) / PX)), mul(u, (CH / 2.0 - COLY) / PX));
        let dv = sub(col, cam.p);
        if (dv[0] * dv[0] + dv[1] * dv[1]).sqrt() > WALK_AWAY || dot(sub(cam.p, c), cross(r, u)) < 0.0 {
            close(m);
            return;
        }
    }
    trace("resolve");
    let mut s = m.menu.take().unwrap_or_default();
    let resolved = resolve(m, &mut s);
    m.menu = Some(s);
    trace(if resolved { "resolved" } else { "not resolved" });
    if !resolved {
        return;
    }
    if let Some(s) = m.menu.as_ref() {
        unglitch(s);
    }
    if !m.menu.as_ref().is_some_and(|s| s.built) {
        trace("build_all");
        build_all(m);
        trace("build_all done");
        cvr_host::log!("[VRMenu] screen ready {:.2} s after the spawn (from Rust)\n", m.menu.as_ref().and_then(|s| s.ready_in).unwrap_or(-1.0));
    }
    trace("resources");
    update_resources(m, dt);
    trace("place");
    let vehicle = m.anchor.is_some_and(|a| a.vehicle);
    let mut s = m.menu.take().unwrap_or_default();
    if ready(&mut s) && (vehicle || !s.placed) {
        let (p, q) = pose(c, r, u);
        place(&s, p, q);
        s.placed = true;
    }
    let placed = s.placed;
    m.menu = Some(s);
    if !placed {
        return;
    }
    if !m.surface_warmup.is_null() {
        if let Some(root) = m.menu.as_ref().map(|s| s.root.clone()) {
            ink::call(&root, "RemoveChild", &[Value::Handle(&m.surface_warmup)]);
        }
        m.surface_warmup = Handle::null();
    }
    if let Some(oa) = m.open_anim_at.take() {
        let wait = m.time - oa;
        for a in m.anims.iter_mut() {
            a.shift(wait);
        }
        m.glass_after += wait;
        m.panel_start = m.panel_start.map(|p| p + wait);
        m.panel_interactive_at += wait;
        m.panel_reveal_end += wait;
    }
    trace("anims");
    step_column(m);
    step_anims(m);
    trace("glass");
    sync_glass(m, c, r, u, vehicle);
    trace("sections");
    if m.refresh_at.is_some_and(|t| m.time >= t) {
        m.refresh_at = None;
        build_panels(m, false, false);
    }
    if m.toast.as_ref().is_some_and(|(_, u)| m.time >= *u) && !m.toast_widget.is_null() {
        m.toast = None;
        ink::visible(&m.toast_widget, false);
        ink::visible(&m.toast_bg, false);
    }
    if m.section.as_deref() == Some("appearance") {
        super::menu_appearance::present(m);
    }
    if m.section.as_deref() == Some("glamdoc") {
        super::menu_glamdoc::update(m);
        if !m.open {
            return;
        }
    }
    match m.section.as_deref() {
        Some("settings") => super::menu_setview::update(m, dt),
        Some("contacts") => {
            super::menu_messages::update(m, dt);
        }
        Some("load" | "stash" | "trade" | "cyberware" | "crafting" | "inventory" | "character" | "journal" | "codex" | "shards" | "tarot" | "timeskip") => super::menu_gameplay::update(m),
        _ => {}
    }
    trace("pointers");
    pointers(m, &cam, c, r, u);
    update_press_fx(m);
    trace("views");
    let keys: Vec<String> = m.views.iter().map(|(k0, _)| k0.clone()).collect();
    for key in keys {
        update_view(m, &key, dt, 3);
    }
}
fn fbt(m: &mut Menu) {
    let on = sp::native::<i32>("VRFbtCalibrating", &[]) == Some(1);
    if on && m.open {
        close(m);
    }
    if on && !m.fbt_calibrating {
        let steps = [
            "Step 1: look ahead at the horizon line, then press both triggers (press to cancel)",
            "Step 2: arms down, look down, then press both triggers (press to cancel)",
            "Step 3: face the cross: head on its top, arms straight out along its arms, until the countdown ends (press to keep V's arms)",
            "",
            "Not taken: look straight at the horizon line until it turns green, then press again",
            "Not taken: stretch both arms straight out along the cross, then hold still again",
            "Not taken: stand straight with the headset, trackers and controllers tracked, then press again",
        ];
        for (i, en) in steps.iter().enumerate() {
            let mut t = super::menu_locale::setting(en);
            t = t.trim_start_matches('*').trim_start().to_string();
            if let Some(p) = t.rfind(" (").filter(|_| t.ends_with(')')) {
                t.truncate(p);
            } else if let Some(p) = t.rfind('（').filter(|_| t.ends_with('）')) {
                t = t[..p].trim_end().to_string();
            } else if let Some(p) = t.rfind('(').filter(|_| t.ends_with(')')) {
                t = t[..p].trim_end().to_string();
            }
            let s = sp::Str::new(&t);
            sp::native::<bool>("VRFbtSetCalibrationText", &[Value::Int32(i as i32 + 1), s.v()]);
        }
    }
    m.fbt_calibrating = on;
}
/// M.update: the menu's frame (the calibration's guard, the HUD's fade, the visits, the tick).
pub(super) fn update(m: &mut Menu, dt: f64) {
    if !super::menu_vendor::interface_enabled() {
        if !m.interface_disabled {
            release(m);
            super::menu_vendor::cancel_all(m);
            m.pending = None;
            m.interface_disabled = true;
        }
        return;
    }
    m.interface_disabled = false;
    m.time += dt;
    super::menu_glamdoc::poll(dt);
    if super::menu_appearance::poll(dt) && !m.open {
        open_centred(m, Some("appearance"));
    }
    if m.open && m.section.as_deref() == Some("appearance") && !super::menu_appearance::current() {
        close(m);
    }
    fbt(m);
    let placed = m.open && m.menu.as_ref().is_some_and(|s| s.placed);
    m.hud.update(placed, dt);
    m.shared.vendor_clock = m.time;
    if super::menu_vendor::poll(m) {
        return;
    }
    if let Some((at0, ev)) = m.pending.clone() {
        if m.time >= at0 {
            m.pending = None;
            let ctrl = m.shared.menu_controller.clone();
            d::menu_event(&ctrl, &ev);
        }
    }
    if super::belt::player().is_null() {
        m.hud.restore();
        super::menu_vendor::cancel_all(m);
        super::menu_gameplay::cancel_transition(m);
        if m.open {
            super::menu_gameplay::leave(m);
            m.open = false;
            sp::native::<()>("VRMenuModOpen", &[Value::Bool(false)]);
            capture(false);
        }
        (m.menu, m.beams, m.anchor, m.glass) = (None, Default::default(), None, Vec::new());
        return;
    }
    tick(m, dt);
    tour(m);
}
pub(super) fn release(m: &mut Menu) {
    super::menu_appearance::release();
    super::menu_glamdoc::finish(false);
    sp::native::<()>("VRMenuVendorPoll", &[Value::Bool(false)]);
    m.hud.restore();
    super::menu_vendor::restore_presentation(m);
    close(m);
    super::menu_gameplay::cancel_transition(m);
    capture(false);
}
pub(super) fn open_centred(m: &mut Menu, initial: Option<&str>) -> bool {
    open(m, camera(), initial)
}
/// The menu's and the wheel's frame (init.lua's onUpdate: Menu.update, then Wheel.update with the menu's state).
pub(in crate::mechanics) fn tick_frame(dt: f32) {
    let open = with(|m| {
        update(m, f64::from(dt));
        m.open
    });
    trace(if open { "menu end (open)" } else { "menu end" });
    super::menu_wheel::update(f64::from(dt), open);
    trace(if open { "wheel end (open)" } else { "wheel end" });
}
/// init.lua's onShutdown: Wheel.release, Menu.release.
pub(in crate::mechanics) fn shutdown() {
    super::menu_wheel::release_all();
    with(release);
}

/// A stage of the frame logged while the tour runs (each the first few times): where a crash came from.
pub(super) fn trace(stage: &str) {
    if !tour_on() {
        return;
    }
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::HashMap<String, u32>> = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let n = SEEN.with(|s| {
        let mut s = s.borrow_mut();
        let e = s.entry(stage.to_string()).or_insert(0);
        *e += 1;
        *e
    });
    if n <= 3 {
        cvr_host::log!("[VRMenu] trace {stage} #{n}\n");
    }
}

// ---------------------------------------------------------------- the test tour (CVR_MENU_TOUR=1)
/// With CVR_MENU_TOUR=1 in the game's environment the menu opens itself in the world once it has settled and visits
/// every section (the inventory's picker and backpack among them) a few seconds each, logging what each one built --
/// what the simulator's harness cannot reach with the wrist gesture.
const TOUR: [&str; 18] = ["inventory", "inventory/weapon", "inventory/backpack", "character", "cyberware", "crafting", "journal", "codex", "shards", "tarot", "contacts", "vehicles", "radio", "timeskip", "game", "load", "settings", "close"];
thread_local! {
    static TOUR_AT: std::cell::Cell<(f64, usize, f64)> = const { std::cell::Cell::new((-1.0, 0, 0.0)) };
    /// The key the tour's hover step holds hovered until the next step.
    static TOUR_HOVER: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}
fn tour_mode() -> &'static str {
    static MODE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    MODE.get_or_init(|| std::env::var("CVR_MENU_TOUR").unwrap_or_default())
}
fn tour_on() -> bool {
    !tour_mode().is_empty()
}
/// The tour without the background reads (CVR_MENU_TOUR=nojobs).
pub(super) fn tour_no_jobs() -> bool {
    tour_mode() == "nojobs"
}
/// The tour's sections: CVR_MENU_TOUR_LIST (comma separated) or all of them.
fn tour_list() -> &'static [String] {
    static LIST: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| match std::env::var("CVR_MENU_TOUR_LIST") {
        Ok(v) if !v.trim().is_empty() => v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        _ => TOUR.iter().map(|s| s.to_string()).collect(),
    })
}
fn tour(m: &mut Menu) {
    if !tour_on() || super::belt::player().is_null() {
        return;
    }
    // the main menu's puppet is a player too (the first one seen), and the save's exists under its loading screen: the
    // tour counts from the loaded world
    thread_local! {
        static FIRST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    let p = super::belt::player().instance as usize;
    let first = FIRST.with(|f| {
        if f.get() == 0 {
            f.set(p);
        }
        f.get()
    });
    if first == p || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) || pre_game() || super::vrcam_select::loading_screen() {
        TOUR_AT.with(|c| c.set((-1.0, 0, 0.0)));
        return;
    }
    let (start, step, at0) = TOUR_AT.with(|c| c.get());
    if start < 0.0 {
        super::menu_crash_probe::install();
        TOUR_AT.with(|c| c.set((m.time, 0, m.time)));
        return;
    }
    if step == 0 {
        if m.time - start < 25.0 {
            return;
        }
        if tour_mode() == "spawn" {
            // only the screen's entity, a metre ahead: its own spawn and attach, nothing of the menu
            if let Some(cam) = camera() {
                let c = add(cam.p, cam.f);
                let id = super::reload_core::spawn_at(ENT, c, sp::quad_pose(cam.r, [0.0, 0.0, 1.0]));
                cvr_host::log!("[VRMenu] tour: spawned the screen alone: {id:?} (from Rust)\n");
            }
            TOUR_AT.with(|c| c.set((start, 999, m.time)));
            return;
        }
        if !m.open {
            if armed() {
                holster_all();
                return;
            }
            if std::env::var("CVR_MENU_TOUR_CALLS").is_ok_and(|v| v == "1") {
                super::lobby_rtti::ALL_CALLS.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            let ok = open(m, camera(), None);
            cvr_host::log!("[VRMenu] tour: open {ok} (from Rust)\n");
            if !ok {
                TOUR_AT.with(|c| c.set((m.time - 20.0, 0, m.time)));
            }
            return;
        }
        if m.menu.as_ref().is_some_and(|s| s.built && s.placed) {
            cvr_host::log!("[VRMenu] tour: built, {} buttons on the column (from Rust)\n", m.buttons.len());
            TOUR_AT.with(|c| c.set((start, 1, m.time)));
        }
        return;
    }
    let list = tour_list();
    // the save list's metadata comes in over seconds: the load page gets longer; CVR_MENU_TOUR_WAIT for shots
    let each = std::env::var("CVR_MENU_TOUR_WAIT").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(3.0);
    let wait = if step >= 2 && list.get(step - 2).is_some_and(|s| s == "load") { each.max(10.0) } else { each };
    if step > list.len() || m.time - at0 < wait {
        return;
    }
    if step > 1 {
        let prev = list[step - 2].as_str();
        let rows: usize = m.views.iter().map(|(_, v)| v.v.borrow().items.len()).sum();
        cvr_host::log!("[VRMenu] tour {prev}: {} buttons, {} views ({} rows), section {:?}, open {} (from Rust)\n", m.buttons.len(), m.views.len(), rows, m.section, m.open);
    }
    let name = list[step - 1].as_str();
    cvr_host::log!("[VRMenu] tour -> {name} (from Rust)\n");
    cvr_host::log::flush();
    if !name.starts_with("hover:") {
        TOUR_HOVER.with(|t| *t.borrow_mut() = None);
    }
    match name {
        "close" => {
            close(m);
            cvr_host::log!("[VRMenu] tour done (from Rust)\n");
        }
        // click:<prefix>: the first shown button whose key starts with it, pressed as a pointer would
        _ if name.starts_with("click:") => {
            let prefix = &name[6..];
            let hit = m.buttons.iter().find(|b| b.visible.get() && b.key().is_some_and(|k| k.starts_with(prefix))).cloned();
            cvr_host::log!("[VRMenu] tour click {prefix:?}: {} (from Rust)
", hit.as_ref().and_then(|b| b.key()).unwrap_or_default());
            if let Some(f) = hit.and_then(|b| b.click.clone()) {
                f(m, 0);
            }
        }
        // hover:<prefix>: the first shown button whose key starts with it, hovered as a pointer would (its tooltip)
        _ if name.starts_with("hover:") => {
            let prefix = &name[6..];
            let keys: Vec<String> = m.buttons.iter().filter(|b| b.visible.get()).filter_map(|b| b.key()).take(24).collect();
            let hit = m.buttons.iter().find(|b| b.visible.get() && b.key().is_some_and(|k| k.starts_with(prefix))).cloned();
            cvr_host::log!("[VRMenu] tour hover {prefix:?}: {} (keys {keys:?}) (from Rust)\n", hit.as_ref().and_then(|b| b.key()).unwrap_or_default());
            for b in std::mem::take(&mut m.hovered) {
                b.repaint(false);
            }
            if let Some(b) = hit {
                TOUR_HOVER.with(|t| *t.borrow_mut() = b.key());
                b.repaint(true);
                m.hovered = vec![b];
            }
        }
        "inventory/weapon" => {
            let s = &mut m.gp.inv.s;
            (s.area, s.slot, s.weapon_slot) = (Some("Weapon".into()), 0, 0);
            build_panels(m, false, false);
        }
        "inventory/backpack" => {
            m.gp.inv.s.area = None;
            m.gp.inv.s.backpack = true;
            build_panels(m, false, false);
        }
        _ => {
            if name == "character" {
                m.gp.inv.s.backpack = false;
            }
            if !m.open {
                open(m, camera(), Some(name));
            } else {
                open_section(m, Some(name), true);
            }
        }
    }
    TOUR_AT.with(|c| c.set((start, step + 1, m.time)));
}
