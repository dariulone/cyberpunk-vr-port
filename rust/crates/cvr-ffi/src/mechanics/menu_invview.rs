//! inventory_preview.lua, item_tooltip.lua and item_sort.lua's control: the character preview (the inventory scene's
//! own puppet sampled from its live atlas, else a static figure), the game's own item tooltips on the menu's canvas,
//! and the sort order's dropdown with its popup.
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, A};
use super::menu_core::*;
use super::menu_invdata as idt;
use super::menu_ink as k;
use super::menu_rt::*;
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use std::rc::Rc;

fn set_enum(w: &Handle, setter: &str, en: &str, member: &str) {
    if let Some(v) = crate::script_call::enum_value(en, member) {
        ink::call(w, setter, &[Value::U64(v as u64)]);
    }
}
fn resref(path: &str) -> u64 {
    red4ext_abi::resource::resource_path(path)
}
/// SpawnFromExternal on the menu controller (the game's own widget library item), null without it.
pub(super) fn spawn_external(m: &Menu, parent: &Handle, path: &str, library: &str) -> Handle {
    let ctrl = m.shared.menu_controller.clone();
    if ctrl.is_null() {
        return Handle::null();
    }
    let rr = resref(path);
    rt::obj(&ctrl, "SpawnFromExternal", &[A::H(parent), A::P((&rr as *const u64).cast()), A::Name(cname(library))])
}

// ---------------------------------------------------------------- the preview
#[derive(Default)]
pub(super) struct Preview {
    puppet: Handle,
    at: Option<f64>,
    pub dirty: bool,
    source: Handle,
    source_parent: Handle,
}
fn crop(parent: &Handle, x: f64, y: f64, w: f64, h: f64, atlas: &str, part: &str, aspect: f64, uv: [f64; 4], col: [f64; 3]) -> Handle {
    let ratio = aspect * (uv[2] - uv[0]) / (uv[3] - uv[1]);
    let width = w.min(h * ratio);
    let height = width / ratio;
    let viewport = ink::canvas(parent, x, y, w, h);
    let mask = ink::place(ink::new("inkScrollAreaWidget"), &viewport, 0.0, 0.0, w, h);
    ink::call(&mask, "SetFitToContent", &[Value::Bool(false)]);
    ink::call(&mask, "SetUseInternalMask", &[Value::Bool(true)]);
    hset(&mask, "constrainContentPosition", 0.0);
    hset(&mask, "fitToContentDirection", ev("inkFitToContentDirection", "None"));
    let content = ink::canvas(&mask, 0.0, 0.0, w, h);
    let (full_w, full_h) = (width / (uv[2] - uv[0]), height / (uv[3] - uv[1]));
    k::image(&content, (w - width) / 2.0 - uv[0] * full_w, (h - height) / 2.0 - uv[1] * full_h, full_w, full_h, atlas, part, Some(col), 1.0)
}
fn equip_slot(area_name: &str, slot: i32) -> Option<Id16> {
    let c = super::game_system("GetScriptableSystemsContainer");
    let es = o(&c, "Get", &[A::Name(cname("EquipmentSystem"))]);
    let pd = o(&es, "GetPlayerData", &[A::H(&player())]);
    super::holster::equip_slot_item(&pd, idt::area(area_name) as u64, slot).map(Id16)
}
fn valid(id: &Id16) -> bool {
    sb("ItemID", "IsValid", &[id.a()])
}
fn weapon(puppet: &Handle, id: Option<Id16>, refresh: bool) {
    let ts = super::game_system("GetTransactionSystem");
    let slot = tdb("AttachmentSlots.WeaponRight");
    let object = o(&ts, "GetItemInSlot", &[A::H(puppet), A::Name(slot)]);
    let previous = (!object.is_null()).then(|| id16(&object, "GetItemID", &[]));
    let id = id.filter(valid);
    let preview = id.map(|i| id16(&ts, "CreatePreviewItemID", &[i.a()]));
    if previous.is_some() && previous == preview && !refresh {
        return;
    }
    if let Some(prev) = previous {
        rt::static_call("gameuiInGameMenuGameController", "SetAnimWrapperBasedOnItemFriendlyName", &[A::H(puppet), prev.a(), A::N(0.0)]);
        rt::call(&ts, "RemoveItemFromSlot", &[A::H(puppet), A::Name(slot), A::B(true)]);
        rt::call(&ts, "RemoveItem", &[A::H(puppet), prev.a(), A::N(1.0)]);
    }
    let (Some(id), Some(preview)) = (id, preview) else { return };
    let source = o(&ts, "GetItemData", &[A::H(&player()), id.a()]);
    if source.is_null() {
        return;
    }
    if !b(&ts, "HasItem", &[A::H(puppet), preview.a()]) {
        rt::call(&ts, "GivePreviewItemByItemData", &[A::H(puppet), A::H(&source)]);
    }
    rt::call(&ts, "AddItemToSlot", &[A::H(puppet), A::Name(slot), preview.a()]);
    rt::static_call("gameuiInGameMenuGameController", "SetAnimWrapperBasedOnItemFriendlyName", &[A::H(puppet), preview.a(), A::N(1.0)]);
}
pub(super) fn preview_update(m: &mut Menu) {
    if !m.gp.inv.model || m.gp.inv.preview.puppet.is_null() {
        return;
    }
    let pv = &mut m.gp.inv.preview;
    if m.time - pv.at.unwrap_or(0.0) < 0.2 && !pv.dirty {
        return;
    }
    pv.at = Some(m.time);
    let slot = m.gp.inv.s.weapon_slot;
    weapon(&pv.puppet.clone(), equip_slot("Weapon", slot), pv.dirty);
    m.gp.inv.preview.dirty = false;
}
fn female() -> bool {
    id(&player(), "GetResolvedGenderName", &[]) == cname("Female")
}
fn static_figure(parent: &Handle, x: f64, y: f64, w: f64, h: f64, area: Option<&str>) {
    let fem = female();
    let uv = match area {
        Some("Head" | "Face") => [0.18, 0.0, 0.82, 0.42],
        Some("OuterChest" | "InnerChest") => [0.05, 0.12, 0.95, 0.68],
        Some("Legs") => [0.15, 0.46, 0.85, 1.0],
        Some("Feet") => [0.2, 0.76, 0.8, 1.0],
        _ => [0.0, 0.0, 1.0, 1.0],
    };
    let (atlas, part, aspect) = if fem {
        ("base\\gameplay\\gui\\fullscreen\\ripperdoc\\woman_body.inkatlas", "wo_intergumentary", 735.0 / 1781.0)
    } else {
        ("base\\gameplay\\gui\\fullscreen\\ripperdoc\\man_body.inkatlas", "ma_intergumentary", 1089.0 / 1861.0)
    };
    crop(parent, x, y, w, h, atlas, part, aspect, uv, k::CYAN);
}
/// P.show: the live puppet's view when the model is on and the scene's puppet is there, else the static figure.
pub(super) fn preview_show(m: &mut Menu, parent: &Handle, x: f64, y: f64, w: f64, h: f64, area: Option<&str>) {
    super::menu_main::trace("preview_show");
    if !m.gp.inv.model || std::env::var_os("CVR_MENU_NOPREVIEW").is_some() {
        preview_release(m);
        return static_figure(parent, x, y, w, h, area);
    }
    let ctrl = m.shared.menu_controller.clone();
    let puppet = if ctrl.is_null() { Handle::null() } else { o(&ctrl, "GetPuppet", &[A::Name(cname("inventory"))]) };
    if puppet.is_null() {
        return static_figure(parent, x, y, w, h, area);
    }
    if m.gp.inv.preview.puppet.instance != puppet.instance {
        preview_release(m);
        m.gp.inv.preview.puppet = puppet.clone();
        let anim = super::game_system("GetAnimationSystem");
        let eid = id16(&puppet, "GetEntityID", &[]);
        rt::call(&anim, "SetForcedVisible", &[eid.a(), A::B(true)]);
        let feature = ink::new("AnimFeature_Paperdoll");
        hset(&feature, "inventoryScreen", 1.0);
        rt::static_call("AnimationControllerComponent", "ApplyFeature", &[A::H(&puppet), A::Name(cname("Paperdoll")), A::H(&feature)]);
    }
    if m.gp.inv.preview.source.is_null() {
        let col = m.layer_col.clone();
        let source = spawn_external(m, &col, "base\\gameplay\\gui\\fullscreen\\inventory\\inventory_screen.inkwidget", "previewPanel");
        if source.is_null() {
            return static_figure(parent, x, y, w, h, area);
        }
        ink::opacity(&source, 0.0);
        ink::v2(&source, "SetScale", 0.001, 0.001);
        (m.gp.inv.preview.source, m.gp.inv.preview.source_parent) = (source, col);
    }
    preview_update(m);
    let uv = match area {
        Some("Head" | "Face") => [0.34, 0.08, 0.64, 0.525],
        Some("OuterChest" | "InnerChest") => [0.27, 0.10, 0.80, 0.72],
        Some("Legs") => [0.30, 0.55, 0.68, 1.0],
        Some("Feet") => [0.35, 0.78, 0.66, 1.0],
        _ => [0.20, 0.04, 0.85, 1.0],
    };
    crop(parent, x, y, w, h, "base\\gameplay\\gui\\fullscreen\\inventory\\preview.inkatlas", "preview", 1900.0 / 1800.0, uv, k::WHITE);
}
pub(super) fn preview_release(m: &mut Menu) {
    let pv = std::mem::take(&mut m.gp.inv.preview);
    if !pv.puppet.is_null() {
        let c = super::game_system("GetScriptableSystemsContainer");
        let es = o(&c, "Get", &[A::Name(cname("EquipmentSystem"))]);
        let pd = o(&es, "GetPlayerData", &[A::H(&player())]);
        let visual = rt::call(&pd, "GetVisualItemInSlot", &[A::N(idt::area("Weapon"))]).map(|o| Id16::of(&rt::Val::of(&o)));
        weapon(&pv.puppet, visual, false);
        let anim = super::game_system("GetAnimationSystem");
        let eid = id16(&pv.puppet, "GetEntityID", &[]);
        rt::call(&anim, "SetForcedVisible", &[eid.a(), A::B(false)]);
    }
    if !pv.source.is_null() && !pv.source_parent.is_null() {
        ink::call(&pv.source_parent, "RemoveChild", &[Value::Handle(&pv.source)]);
    }
}

// ---------------------------------------------------------------- the game's tooltips
// A GAME TOOLTIP THAT DOES NOT SHOW IS NEVER A BLACK PLATE (main's item_tooltip.lua, issue #188: "just a black space
// where the item information should be", at a ripperdoc). The game's tooltip is spawned invisible and shown once it has
// laid out (tip_fit); a spawn that fails or a tooltip that never lays out leaves the menu's own card -- the item's
// name, quality, price and description -- in its place (card), and the reason goes to the log once per item (report).
/// How long a game tooltip may take to lay out before the card stands in for it (seconds).
const LAYOUT_WAIT: f64 = 0.75;
/// A tooltip spawn: the tip, the library still loading, or why it failed.
pub(super) enum Spawned {
    Tip(Tip),
    Loading,
    Failed(String),
}
pub(super) type SpawnTip = Rc<dyn Fn() -> Spawned>;
pub(super) struct Tip {
    widget: Handle,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    back: Handle,
    back_size: (f64, f64),
    ui: Handle,
    parent: Handle,
    price: Option<f64>,
    born: f64,
    /// A spawn retried while its library loads.
    pending: Option<SpawnTip>,
    /// The card stands in; the widget may still lay out (then it replaces the card).
    fell: bool,
    card: Handle,
    laid: bool,
    no_card: bool,
    /// The black plate shown behind it (the panel's coordinates): the tooltip's own content, or the card.
    plate: Option<[f64; 4]>,
}
fn clock() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}
impl Tip {
    fn blank(ui: &Handle, parent: &Handle, x: f64, y: f64, w: f64, h: f64, price: Option<f64>) -> Tip {
        Tip { widget: Handle::null(), x, y, w, h, back: Handle::null(), back_size: (0.0, 0.0), ui: ui.clone(), parent: parent.clone(), price, born: clock(), pending: None, fell: false, card: Handle::null(), laid: false, no_card: false, plate: None }
    }
    /// Its plate drawn on `back` (a canvas made behind the widget, tip_back).
    pub(super) fn backed(self, back: Handle) -> Tip {
        Tip { back, ..self }
    }
    /// A spawned widget in its box (the item's ui, the parent and the price for the card).
    pub(super) fn spawned(widget: Handle, ui: &Handle, parent: &Handle, x: f64, y: f64, w: f64, h: f64, price: Option<f64>) -> Tip {
        Tip { widget, ..Tip::blank(ui, parent, x, y, w, h, price) }
    }
}
fn item_name(ui: &Handle) -> String {
    let n0 = s(ui, "GetName", &[]);
    if n0.is_empty() {
        "?".into()
    } else {
        n0
    }
}
/// T.report: why the game tooltip of an item did not show, once per item and reason.
fn report(ui: &Handle, why: &str) {
    use std::sync::Mutex;
    static REPORTED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let name = item_name(ui);
    let key = format!("{name}|{why}");
    let mut seen = REPORTED.lock().unwrap_or_else(|p| p.into_inner());
    if seen.contains(&key) {
        return;
    }
    seen.push(key);
    cvr_host::log!("[VRMenu] the game tooltip for {name} did not show ({why}); the menu's card stands in (from Rust)\n");
}
/// Lines of at most `chars` characters (not bytes), paragraphs kept, trailing empty lines dropped.
fn wrap(text: &str, chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.replace('\r', "").split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            if line.is_empty() {
                line = word.to_string();
            } else if line.chars().count() + 1 + word.chars().count() <= chars {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            }
        }
        lines.push(line);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}
/// T.card: the menu's own card for the tip -- what the game tooltip would have led with. A widget that is there stays
/// laid out while hidden, so a lazy module finishing later still brings the full tooltip back.
fn card(t: &mut Tip) {
    if t.fell {
        return;
    }
    t.fell = true;
    if !t.widget.is_null() {
        ink::call(&t.widget, "SetAffectsLayoutWhenHidden", &[Value::Bool(true)]);
        ink::visible(&t.widget, false);
    }
    let name = item_name(&t.ui);
    let quality = s(&t.ui, "GetQualityText", &[]);
    let desc = s(&t.ui, "GetDescription", &[]);
    let (x, y, w) = (t.x, t.y, t.w);
    let lines = wrap(&desc, 12usize.max(((w - 40.0) / (24.0 * 0.62)).floor() as usize));
    let mut h = 20.0 + 56.0 + if quality.is_empty() { 0.0 } else { 40.0 } + if t.price.is_some() { 40.0 } else { 0.0 } + lines.len() as f64 * 32.0 + 20.0;
    h = h.min(t.h);
    let host = ink::canvas(&t.parent, 0.0, 0.0, 10.0, 10.0);
    t.card = host.clone();
    k::round_fill(&host, x, y, w, h, k::BLACK, 1.0, Some(8.0));
    t.plate = Some([x, y, w, h]);
    let mut cy = y + 20.0;
    k::text(&host, x + 20.0, cy, w - 40.0, 46.0, &name, 34.0, k::YELLOW, "left", false);
    cy += 56.0;
    if !quality.is_empty() {
        k::text(&host, x + 20.0, cy, w - 40.0, 34.0, &quality, 24.0, k::GREY, "left", false);
        cy += 40.0;
    }
    if let Some(price) = t.price {
        k::text(&host, x + 20.0, cy, w - 40.0, 34.0, &format!("€$ {}", (price + 0.5).floor()), 26.0, k::CYAN, "left", false);
        cy += 40.0;
    }
    for l in lines {
        if cy + 32.0 > y + h {
            break;
        }
        k::text(&host, x + 20.0, cy, w - 40.0, 32.0, &l, 24.0, k::TEXT, "left", false);
        cy += 32.0;
    }
}
/// T.watch: past LAYOUT_WAIT without a layout the card stands in.
fn watch(t: &mut Tip) {
    if t.fell || t.laid || clock() - t.born < LAYOUT_WAIT {
        return;
    }
    report(&t.ui, &format!("no layout in {LAYOUT_WAIT} s"));
    card(t);
}
/// T.widget: the tooltip library's widget through the plugin's ready-library spawn (main's MenuTooltip.cpp, without
/// Codeware's loader): Err("loading") while the library loads.
pub(super) fn tip_widget(controller: &Handle, parent: &Handle, path: &str, library: &str) -> Result<Handle, String> {
    match super::menu_tooltip::prepare_path(path) {
        0 => return Err("loading".into()),
        1 => {}
        _ => return Err(format!("tooltip library unavailable: {path}")),
    }
    let widget = super::menu_tooltip::spawn(controller, parent, path, cname(library));
    if widget.is_null() {
        Err(format!("no widget from {library}"))
    } else {
        Ok(widget)
    }
}
/// T.create: a spawned tip, a pending one while its library loads (no callback spawns into a closed panel: the request
/// lives as long as its tip), or the card for an item whose tooltip cannot spawn.
pub(super) fn tip_create(spawn: SpawnTip, ui: &Handle, parent: &Handle, x: f64, y: f64, w: f64, h: f64, price: Option<f64>) -> Option<Tip> {
    match spawn() {
        Spawned::Tip(t) => Some(t),
        Spawned::Loading => Some(Tip { pending: Some(spawn), no_card: ui.is_null(), ..Tip::blank(ui, parent, x, y, w, h, price) }),
        Spawned::Failed(why) => {
            if ui.is_null() {
                cvr_host::log!("[VRMenu] the upgrade tooltip did not spawn: {why} (from Rust)\n");
                return None;
            }
            let mut t = Tip::blank(ui, parent, x, y, w, h, price);
            report(ui, &why);
            card(&mut t);
            Some(t)
        }
    }
}
fn place_tip(w: &Handle, x: f64, y: f64) {
    set_enum(w, "SetAnchor", "inkEAnchor", "TopLeft");
    ink::v2(w, "SetAnchorPoint", 0.0, 0.0);
    ink::v2(w, "SetRenderTransformPivot", 0.0, 0.0);
    ink::v2(w, "SetScale", 0.001, 0.001);
    ink::opacity(w, 0.001);
    ink::move_to(w, x, y);
}
/// A widget placed invisible and fed its data; Err when it has no controller.
pub(super) fn tip_feed(widget: &Handle, x: f64, y: f64, data: &Handle, library: &str) -> Result<(), String> {
    place_tip(widget, x, y);
    let ctrl = o(widget, "GetController", &[]);
    if ctrl.is_null() {
        return Err(format!("no controller on {library}"));
    }
    rt::call(&ctrl, "SetData", &[A::H(data)]);
    Ok(())
}
fn spawn_game(controller: &Handle, p: &Handle, ui: &Handle, context: &str, x: f64, y: f64, w: f64, h: f64, compare: Option<&Handle>, price: Option<f64>) -> Spawned {
    let ctx_data = so("ItemDisplayContextData", "Make", &[A::H(&player()), A::N(ev("gameItemDisplayContext", context)), A::B(false)]);
    let data = so("UIInventoryItemTooltipWrapper", "Make", &[A::H(ui), A::H(&ctx_data)]);
    if let Some(c) = compare {
        let cmp = so("UIInventoryItemComparisonManager", "Make", &[A::H(ui), A::H(c)]);
        super::menu_main::set_handle_prop(&data, "comparisonData", &cmp);
    }
    if let Some(pr) = price {
        hset(&data, "overridePrice", pr);
    }
    let (mut path, mut library) = ("base\\gameplay\\gui\\common\\tooltip\\itemtooltip.inkwidget", "itemTooltip");
    if b(ui, "IsWeapon", &[]) {
        path = "base\\gameplay\\gui\\common\\tooltip\\new_itemtooltip.inkwidget";
    } else if b(&o(ui, "GetItemData", &[]), "HasTag", &[A::Name(cname("Cyberdeck"))]) {
        (path, library) = ("base\\gameplay\\gui\\common\\tooltip\\cyberdecktooltip.inkwidget", "cyberdeckTooltip");
    } else if n(ui, "GetItemType", &[]) == ev("gamedataItemType", "Prt_Program") {
        (path, library) = ("base\\gameplay\\gui\\common\\tooltip\\programtooltip.inkwidget", "programTooltip");
    }
    let back = tip_back(p);
    let widget = match tip_widget(controller, p, path, library) {
        Ok(w) => w,
        Err(why) => {
            ink::call(p, "RemoveChild", &[Value::Handle(&back)]);
            return if why == "loading" { Spawned::Loading } else { Spawned::Failed(why) };
        }
    };
    if let Err(why) = tip_feed(&widget, x, y, &data, library) {
        return Spawned::Failed(why);
    }
    Spawned::Tip(Tip { back, ..Tip::spawned(widget, ui, p, x, y, w, h, price) })
}
/// T.spawn: the game's tooltip widget for a UI item (its kind's own library item), fed its data.
#[allow(clippy::too_many_arguments)]
pub(super) fn tip_spawn(m: &Menu, p: &Handle, ui: &Handle, context: &str, x: f64, y: f64, w: f64, h: f64, compare: Option<&Handle>, price: Option<f64>) -> Option<Tip> {
    let controller = m.shared.menu_controller.clone();
    let (p2, ui2, context2, compare2) = (p.clone(), ui.clone(), context.to_string(), compare.cloned());
    let spawn: SpawnTip = Rc::new(move || spawn_game(&controller, &p2, &ui2, &context2, x, y, w, h, compare2.as_ref(), price));
    tip_create(spawn, ui, p, x, y, w, h, price)
}
/// T.spawnData: the weapon tooltip given ready data (the upgrade screen's snapshot): what it shows is read when the data
/// is made, not when the widget draws. It has no card.
pub(super) fn tip_spawn_data(m: &Menu, p: &Handle, data: &Handle, x: f64, y: f64, w: f64, h: f64) -> Option<Tip> {
    let controller = m.shared.menu_controller.clone();
    let (p2, data2) = (p.clone(), data.clone());
    let spawn: SpawnTip = Rc::new(move || {
        let path = "base\\gameplay\\gui\\common\\tooltip\\new_itemtooltip.inkwidget";
        let back = tip_back(&p2);
        match tip_widget(&controller, &p2, path, "itemTooltip") {
            Ok(widget) => {
                place_tip(&widget, x, y);
                rt::call(&o(&widget, "GetController", &[]), "SetData", &[A::H(&data2)]);
                Spawned::Tip(Tip { no_card: true, back, ..Tip::spawned(widget, &Handle::null(), &p2, x, y, w, h, None) })
            }
            Err(why) => {
                ink::call(&p2, "RemoveChild", &[Value::Handle(&back)]);
                if why == "loading" {
                    Spawned::Loading
                } else {
                    Spawned::Failed(why)
                }
            }
        }
    });
    tip_create(spawn, &Handle::null(), p, x, y, w, h, None)
}
/// A canvas made behind a tooltip's widget (before it is spawned) for its plate.
pub(super) fn tip_back(parent: &Handle) -> Handle {
    ink::canvas(parent, 0.0, 0.0, 10.0, 10.0)
}
/// The size of what a tooltip shows: its root's contentWrapper (the item's panels), as it is laid out.
fn content_size(widget: &Handle) -> Option<(f64, f64)> {
    let n = rt::num(widget, "GetNumChildren", &[]).unwrap_or(0.0).clamp(0.0, 64.0) as i32;
    for i in 0..n {
        let c = o(widget, "GetWidgetByIndex", &[A::N(f64::from(i))]);
        let name = rt::call(&c, "GetName", &[]).map(|o| unsafe { *(o.as_ptr() as *const u64) });
        if name == Some(cname("contentWrapper")) {
            return rt::call(&c, "GetDesiredSize", &[]).map(|o| {
                let v = rt::Val::of(&o);
                (v.f("X").and_then(|x| x.num()).unwrap_or(0.0), v.f("Y").and_then(|x| x.num()).unwrap_or(0.0))
            });
        }
    }
    None
}
/// The plates the tooltips show now (the panel's coordinates).
pub(super) type Plates = Rc<std::cell::RefCell<Vec<[f64; 4]>>>;
/// A tooltip's blocker that blocks only where a plate is shown -- the pointer over the tooltip stays off what lies
/// under it, the menu round it stays usable. The plates are set by set_plates as the tooltips lay out.
pub(super) fn plate_blocker(block: &BtnRef) -> Plates {
    let plates: Plates = Rc::default();
    let (p2, weak) = (plates.clone(), Rc::downgrade(block));
    *block.contains.borrow_mut() = Some(Rc::new(move |lx, ly| {
        let Some(b) = weak.upgrade() else { return false };
        // the button's own coordinates are the panel's from (P1X, P1Y)
        let (x, y) = (lx + b.x.get() - P1X, ly + b.y.get() - P1Y);
        p2.borrow().iter().any(|r| x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3])
    }));
    plates
}
pub(super) fn set_plates(plates: &Plates, tips: &[Tip]) {
    let now: Vec<[f64; 4]> = tips.iter().filter_map(|t| t.plate).collect();
    if *plates.borrow() != now {
        *plates.borrow_mut() = now;
    }
}
/// CVR_MENU_TIP_TRACE=1: a laid-out tooltip's widget tree once (name, class, shown, desired size, margin), four levels.
fn trace_tree(w: &Handle, depth: usize, out: &mut Vec<String>) {
    if w.is_null() || depth > 4 || out.len() > 120 {
        return;
    }
    let name = rt::call(w, "GetName", &[]).map(|o| red4ext_abi::cname::CName(unsafe { *(o.as_ptr() as *const u64) }).text().to_string()).unwrap_or_default();
    let class = red4ext_abi::rtti::RttiSystem::get()
        .and(unsafe { red4ext_abi::rtti::object_class(w.instance) })
        .map(|c| c.class_name().text().to_string())
        .unwrap_or_default();
    let size = rt::call(w, "GetDesiredSize", &[]).map(|o| {
        let v = rt::Val::of(&o);
        (v.f("X").and_then(|x| x.num()).unwrap_or(0.0), v.f("Y").and_then(|x| x.num()).unwrap_or(0.0))
    });
    let shown = b(w, "IsVisible", &[]);
    out.push(format!("{}{name} [{class}] shown {shown} size {size:?}", "  ".repeat(depth)));
    let n = rt::num(w, "GetNumChildren", &[]).unwrap_or(0.0).clamp(0.0, 64.0) as i32;
    for i in 0..n {
        let c = o(w, "GetWidgetByIndex", &[A::N(f64::from(i))]);
        trace_tree(&c, depth + 1, out);
    }
}
/// T.fit: each pending tip spawned once its library is in; each tooltip scaled into its box once it has laid out (its
/// plate sized to it, the card taken away); the card for one that does not lay out in time.
pub(super) fn tip_fit(tips: &mut [Tip]) {
    for t in tips.iter_mut() {
        if let Some(spawn) = t.pending.clone() {
            match spawn() {
                Spawned::Tip(n0) => {
                    let card = std::mem::take(&mut t.card);
                    let parent = t.parent.clone();
                    *t = n0;
                    if !card.is_null() {
                        ink::call(&parent, "RemoveChild", &[Value::Handle(&card)]);
                    }
                }
                Spawned::Failed(why) => {
                    t.pending = None;
                    if !t.ui.is_null() {
                        report(&t.ui, &why);
                        card(t);
                    }
                }
                Spawned::Loading => {
                    if !t.ui.is_null() {
                        watch(t);
                    }
                }
            }
        }
        if t.widget.is_null() {
            continue;
        }
        let size = rt::call(&t.widget, "GetDesiredSize", &[]).map(|o| {
            let v = rt::Val::of(&o);
            (v.f("X").and_then(|x| x.num()).unwrap_or(0.0), v.f("Y").and_then(|x| x.num()).unwrap_or(0.0))
        });
        match size {
            Some((zx, zy)) if zx > 1.0 && zy > 1.0 => {
                if t.fell {
                    if !t.card.is_null() {
                        ink::visible(&t.card, false);
                        ink::call(&t.parent, "RemoveChild", &[Value::Handle(&t.card)]);
                        t.card = Handle::null();
                    }
                    ink::visible(&t.widget, true);
                    t.fell = false;
                }
                if !t.laid && std::env::var("CVR_MENU_TIP_TRACE").is_ok_and(|v| v == "1") {
                    let mut lines = Vec::new();
                    trace_tree(&t.widget, 0, &mut lines);
                    cvr_host::log!("[menu-tip-trace] laid out {zx:.0} x {zy:.0} in a box {:.0} x {:.0}:\n{}\n", t.w, t.h, lines.join("\n"));
                }
                t.laid = true;
                let scale = (t.w / zx).min(t.h / (zy + 65.0));
                ink::v2(&t.widget, "SetScale", scale, scale);
                ink::move_to(&t.widget, t.x, t.y + 35.0 * scale);
                ink::opacity(&t.widget, 1.0);
                // THE PLATE IS WHAT THE TOOLTIP SHOWS: its contentWrapper (710 wide), not its root -- the root also holds
                // the equipped badge's container out at its side (761 wide for a weapon), and a plate of the root's size
                // stood out to the right as a black band; the menu's fixed boxes (640 x 924) left a black field under a
                // short tooltip
                let (cx, cy) = content_size(&t.widget).filter(|c| c.0 > 1.0 && c.1 > 1.0).unwrap_or((zx, zy));
                let (bw, bh) = ((cx.min(zx) * scale + 0.5).floor(), (cy.min(zy) * scale + 0.5).floor());
                if !t.back.is_null() && (bw, bh) != t.back_size {
                    t.back_size = (bw, bh);
                    ink::remove_all(&t.back);
                    k::round_fill(&t.back, t.x - 8.0, t.y + 35.0 * scale - 8.0, bw + 16.0, bh + 16.0, k::BLACK, 1.0, Some(8.0));
                }
                t.plate = Some([t.x - 8.0, t.y + 35.0 * scale - 8.0, bw + 16.0, bh + 16.0]);
            }
            _ => {
                if !t.no_card && !t.ui.is_null() {
                    watch(t);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the sort order's dropdown
#[derive(Clone)]
pub(super) struct SortPopup {
    pub key: String,
    pub geom: Option<(f64, f64, f64)>,
    pub value: String,
    pub set: Option<Rc<dyn Fn(&mut Menu, String)>>,
}
fn arrow(p: &Handle, x: f64, y: f64, direction: f64, col: [f64; 3]) -> Vec<Handle> {
    if direction == ev("DropdownItemDirection", "None") {
        return Vec::new();
    }
    let sign = if direction == ev("DropdownItemDirection", "Up") { -1.0 } else { 1.0 };
    vec![k::line(p, x, y - 9.0, x, y + 9.0, col, 1.0, 2.0), k::line(p, x - 7.0, y + sign * 2.0, x, y + sign * 9.0, col, 1.0, 2.0), k::line(p, x, y + sign * 9.0, x + 7.0, y + sign * 2.0, col, 1.0, 2.0)]
}
/// S.control: the dropdown showing the order in use; a press opens (or closes) its popup.
#[allow(clippy::too_many_arguments)]
pub(super) fn sort_control(m: &mut Menu, p: &Handle, popup: &mut Option<SortPopup>, key: &str, x: f64, y: f64, w: f64, value: Option<&str>, set: Rc<dyn Fn(&mut Menu, String)>, which: Rc<dyn Fn(&mut Menu) -> &mut Option<SortPopup>>) {
    let option = idt::sort_option(value);
    let open = popup.as_ref().is_some_and(|pp| pp.key == key);
    let text = k::text(p, x + 28.0, y, w - 54.0, 42.0, &k::clip(&option.label, 22), 25.0, k::TEXT, "left", false);
    let marks = arrow(p, x + 12.0, y + 21.0, option.direction, k::CYAN);
    let edge = k::rect(p, x + 4.0, y + 41.0, w - 8.0, 1.0, k::CYAN, 0.25);
    chevron(p, x + w - 13.0, y + 21.0, 6.0, 2.0, if open { "up" } else { "down" }, k::CYAN, 0.8);
    let group = if popup.is_some() { "sort" } else { "p1" };
    let key2 = key.to_string();
    let b = button(
        m,
        group,
        &at(P1X, P1Y),
        x,
        y,
        w,
        44.0,
        Some(Rc::new(move |m: &mut Menu, _| {
            let slot = which(m);
            *slot = if open { None } else { Some(SortPopup { key: key2.clone(), geom: None, value: String::new(), set: None }) };
            super::menu_gameplay::redraw(m);
        })),
        Some(Rc::new(move |h, _| {
            let col = if h { k::YELLOW } else { k::CYAN };
            ink::tint(&text, if h { k::YELLOW } else { k::TEXT });
            k::tint(&marks, col, Some(1.0));
            ink::tint(&edge, col);
            ink::opacity(&edge, if h { 0.8 } else { 0.25 });
        })),
    );
    *b.key.borrow_mut() = Some(key.to_string());
    if open {
        *popup = Some(SortPopup { key: key.to_string(), geom: Some((x, y, w)), value: option.key, set: Some(set) });
    }
}
/// S.popup: the open dropdown's options, modal over the panel.
pub(super) fn sort_popup(m: &mut Menu, p: &Handle, popup: &Option<SortPopup>, which: Rc<dyn Fn(&mut Menu) -> &mut Option<SortPopup>>) {
    let Some(pp) = popup.clone() else { return };
    let (Some((px, py, pw)), Some(set)) = (pp.geom, pp.set.clone()) else { return };
    m.modal_group = Some("sort".into());
    let opts = idt::sort_options();
    let (w, row_h): (f64, f64) = (430.0, 48.0);
    let h = opts.len() as f64 * row_h + 12.0;
    let x = 12f64.max((1780.0 - w - 12.0).min(px + pw - w));
    let y = (py + 52.0).min(1195.0 - h);
    let host = ink::canvas(p, x, y, w, h);
    k::round_fill(&host, 0.0, 0.0, w, h, k::BLACK, 1.0, Some(4.0));
    k::cut_frame(&mut m.pool, &host, 0.0, 0.0, w, h, k::CYAN, 0.7, Some(10.0), Some(2.0));
    for (i, e) in opts.iter().enumerate() {
        let yy = 6.0 + i as f64 * row_h;
        let chosen = e.key == pp.value;
        let bg = k::round_fill(&host, 5.0, yy, w - 10.0, row_h - 3.0, if chosen { k::YELLOW } else { k::NAVY }, if chosen { 0.17 } else { 0.18 }, Some(3.0));
        let col = if chosen { k::YELLOW } else { k::TEXT };
        let marks = arrow(&host, 24.0, yy + row_h / 2.0, e.direction, col);
        let label = k::text(&host, 44.0, yy, w - 60.0, row_h - 3.0, &e.label, 27.0, col, "left", false);
        let (set2, which2, ek) = (set.clone(), which.clone(), e.key.clone());
        let b = button(
            m,
            "sort",
            &at(P1X + x, P1Y + y),
            5.0,
            yy,
            w - 10.0,
            row_h - 3.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                *which2(m) = None;
                set2(m, ek.clone());
                super::menu_gameplay::redraw(m);
            })),
            Some(Rc::new(move |hov, _| {
                ink::tint(&bg, if hov || chosen { k::YELLOW } else { k::NAVY });
                ink::opacity(&bg, if hov { 0.22 } else if chosen { 0.17 } else { 0.18 });
                let c = if hov { k::YELLOW } else { col };
                ink::tint(&label, c);
                k::tint(&marks, c, Some(1.0));
            })),
        );
        *b.key.borrow_mut() = Some(format!("{}:{}", pp.key, e.key));
    }
}
