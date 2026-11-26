//! CyberpunkVRPort_Menu/modules/menu.lua, part 1: the VR menu's state and its widgets. The menu is a UI quad (vr_menu.ent,
//! 1 x 0.5 m at 3200 px per metre) held before the player at the height of the hands, its column of round icons and
//! one or two panels of a section beside it, glass planes behind them, the two pointer beams. Everything drawn is
//! ink in canvas pixels; what can be pressed is a button rectangle (in a scroll view's content when it lies in a list).
use super::lobby_ink::{self as ink, C3};
use super::lobby_scroll::View;
use super::lobby_spawn::{self as sp, add, cross, mul, Q, V3};
use super::menu_bits::{Stick, ViewRef};
use super::menu_ink::{self as k, Pool};
use red4ext_abi::handle::Handle;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

pub(super) const ENT: &str = "vrport\\ui\\vr_menu.ent";
pub(super) const BEAM_ENT: &str = "vrport\\ui\\vr_menu_beam.ent";
pub(super) const GLASS_ENT: &str = "vrport\\ui\\vr_menu_glass.ent";
pub(super) const OPAQUE_ENT: &str = "vrport\\ui\\vr_menu_opaque.ent";
pub(super) const ICON_ENT: &str = "vrport\\ui\\vr_menu_icon.ent";
pub(super) const UI: &str = "vrp_menu_ui";
pub(super) const CW: f64 = 3200.0;
pub(super) const CH: f64 = 1600.0;
pub(super) const PX: f64 = 3200.0;
pub(super) const W: f64 = CW / PX;
pub(super) const H: f64 = CH / PX;
pub(super) const COLX: f64 = 1100.0;
pub(super) const COL_HOME: f64 = CW / 2.0 - COLX;
pub(super) const ICON: f64 = 108.0;
pub(super) const CLOSE_ICON: f64 = 100.0;
pub(super) const RES_H: f64 = 60.0;
pub(super) const RES_INSET: f64 = 5.0;
pub(super) fn col_y(key: &str) -> f64 {
    match key {
        "close" => 120.0,
        "person" => 270.0,
        "map" => 390.0,
        "quests" => 510.0,
        "contacts" => 630.0,
        "vehicles" => 750.0,
        "radio" => 870.0,
        "timeskip" => 990.0,
        "game" => 1110.0,
        _ => 1230.0,
    }
}
pub(super) const COLY: f64 = (270.0 + 1230.0) / 2.0;
pub(super) const P1X: f64 = 1280.0;
pub(super) const P1Y: f64 = 250.0;
pub(super) const P1W: f64 = 820.0;
pub(super) const HEAD: f64 = 96.0;
pub(super) const ROW: f64 = 112.0;
pub(super) const VIEW_ROWS: f64 = 8.0;
pub(super) const LIST_H: f64 = VIEW_ROWS * ROW;
pub(super) const ARROW_H: f64 = 52.0;
pub(super) const ARROW_GAP: f64 = 8.0;
pub(super) const P2X: f64 = 2160.0;
pub(super) const P2W: f64 = 640.0;
pub(super) const SW: f64 = P2X + P2W - P1X;
pub(super) const PAD: f64 = 14.0;
pub(super) const TOAST_Y: f64 = 1480.0;
pub(super) const HOVER: f64 = 0.10;
pub(super) const PRESS: f64 = 0.010;
pub(super) const RELEASE: f64 = 0.025;
pub(super) const THROUGH: f64 = -0.05;
pub(super) const TRIGGER_ON: f64 = 0.70;
pub(super) const TRIGGER_OFF: f64 = 0.40;
pub(super) const WALK_AWAY: f64 = 1.1;
pub(super) fn tap(by: &str) -> f64 {
    if by == "finger" {
        32.0
    } else {
        40.0
    }
}
pub(super) const SLOPE_MAX: f64 = 2.5;
pub(super) const SETTLE: f64 = 0.12;
pub(super) const LINE_START: f64 = 0.008;
pub(super) const HAPTIC_CLICK: i32 = 20;
pub(super) const ANIM: f64 = 0.16;
pub(super) const COLUMN_SHIFT: f64 = 0.26;
pub(super) const R_PANEL: f64 = 30.0;
pub(super) const R_CELL: f64 = 22.0;
pub(super) const ARROW_FOOT: f64 = 26.0;
pub(super) const ARROW_LIFT: f64 = 36.0;
pub(super) const GLASS_BACK: f64 = 0.006;

// ---------------------------------------------------------------- the texts
const TEXT_KEYS: &[(&str, &str, &str)] = &[
    ("contacts", "CONTACTS", "КОНТАКТЫ"),
    ("vehicles", "VEHICLES", "ТРАНСПОРТ"),
    ("radio", "RADIO", "РАДИО"),
    ("settings", "VR SETTINGS", "НАСТРОЙКИ VR"),
    ("call", "CALL", "ПОЗВОНИТЬ"),
    ("messages", "MESSAGES", "СООБЩЕНИЯ"),
    ("on", "ON", "ВКЛ"),
    ("off", "OFF", "ВЫКЛ"),
    ("volume", "VOLUME", "ГРОМКОСТЬ"),
    ("playing", "NOW PLAYING", "СЕЙЧАС ИГРАЕТ"),
    ("empty", "Nothing here yet", "Пока пусто"),
    ("blocked", "Not available right now", "Сейчас недоступно"),
    ("calling", "Calling…", "Вызов…"),
    ("summoned", "On its way", "Уже едет"),
    ("mounted", "You are driving already", "Вы уже за рулём"),
    ("repair", "Being repaired", "В ремонте"),
    ("noMessages", "No messages", "Нет сообщений"),
    ("game", "GAME", "ИГРА"),
    ("save", "Save Game", "Сохранить игру"),
    ("load", "Load Game", "Загрузить игру"),
    ("options", "Settings", "Настройки"),
    ("credits", "Credits", "Титры"),
    ("character", "CHARACTER", "ПЕРСОНАЖ"),
    ("messagesTab", "MESSAGES", "СООБЩЕНИЯ"),
    ("contactsTab", "CONTACTS", "КОНТАКТЫ"),
    ("allMessages", "All messages", "Все сообщения"),
    ("showRead", "Show read", "Показать прочитанные"),
    ("hideRead", "Hide read", "Скрыть прочитанные"),
    ("summon", "CALL VEHICLE", "ВЫЗВАТЬ"),
    ("ready", "Ready", "Готов"),
    ("exit", "Exit to Main Menu", "Выйти в главное меню"),
    ("confirmExit", "Press again: main menu", "Ещё раз — в главное меню"),
    ("quit", "Exit Game", "Выйти из игры"),
    ("confirmQuit", "Press again: quit the game", "Ещё раз — выход из игры"),
    ("noMenu", "The game menu is not ready yet", "Меню игры ещё не готово"),
    ("hint", "Point at a setting to read what it does", "Наведите на параметр — здесь будет подсказка"),
    ("loading", "Reading the settings…", "Чтение настроек…"),
];
/// L.labels(TEXT.en, TEXT.ru): the menu's words in its language.
pub(super) fn texts() -> HashMap<&'static str, String> {
    let ru = super::menu_locale::ru();
    TEXT_KEYS.iter().map(|(k, en, r)| (*k, if ru { (*r).to_string() } else { super::menu_locale::t(en) })).collect()
}

// ---------------------------------------------------------------- buttons, views
pub(super) type Act = Rc<dyn Fn(&mut Menu, usize)>;
pub(super) type Paint = Rc<dyn Fn(bool, bool)>;
pub(super) type DragFn = Rc<dyn Fn(&mut Menu, Option<f64>, &str, usize, Option<f64>)>;
pub(super) struct IconBits {
    pub x0: f64,
    pub y0: f64,
    pub canvas: Handle,
    pub ring_host: Handle,
    pub ring: Vec<Handle>,
    pub outer: Vec<Handle>,
    pub visual: Rc<Cell<(C3, f64, f64)>>,
}
pub(super) struct Btn {
    pub group: String,
    pub x: Cell<f64>,
    pub y: Cell<f64>,
    pub w: f64,
    pub h: f64,
    pub click: Option<Act>,
    pub paint: Option<Paint>,
    pub view: Option<ViewRef>,
    pub visible: Cell<bool>,
    pub blocker: Cell<bool>,
    pub circle: Cell<bool>,
    pub contains: RefCell<Option<Rc<dyn Fn(f64, f64) -> bool>>>,
    pub selected: Cell<bool>,
    pub key: RefCell<Option<String>>,
    pub mute: Cell<bool>,
    pub drag: RefCell<Option<DragFn>>,
    pub value: Rc<Cell<Option<f64>>>,
    pub show: RefCell<Option<Rc<dyn Fn(f64)>>>,
    pub icon: RefCell<Option<IconBits>>,
    pub pulse_at: Cell<Option<f64>>,
    pub tip: RefCell<Option<String>>,
    pub passive: Cell<bool>,
    pub also: RefCell<Option<Rc<dyn Fn(bool)>>>,
}
pub(super) type BtnRef = Rc<Btn>;
impl Btn {
    pub(super) fn repaint(&self, hover: bool) {
        if let Some(p) = &self.paint {
            p(hover, self.selected.get());
        }
        let also = self.also.borrow().clone();
        if let Some(a) = also {
            a(hover);
        }
    }
    pub(super) fn key(&self) -> Option<String> {
        self.key.borrow().clone()
    }
}
/// at(): where a group's buttons are measured from (a panel's corner), and the view they scroll in.
#[derive(Clone)]
pub(super) struct At {
    pub ox: f64,
    pub oy: f64,
    pub view: Option<ViewRef>,
}
pub(super) fn at(ox: f64, oy: f64) -> At {
    At { ox, oy, view: None }
}
pub(super) const ORIGIN: At = At { ox: 0.0, oy: 0.0, view: None };
/// A list row's builder: draws into the row's canvas (its y in the list, its index from 1).
pub(super) type RowBuild = Rc<dyn Fn(&mut Menu, &Handle, RowCtx)>;
#[derive(Clone)]
pub(super) struct RowCtx {
    pub y: f64,
    pub h: f64,
    pub index: usize,
    pub vx: f64,
    pub vy: f64,
    pub cw: f64,
    pub first: bool,
    pub view: ViewRef,
}
pub(super) struct Row {
    pub h: f64,
    pub build: RowBuild,
}
pub(super) struct MView {
    pub v: ViewRef,
    pub builds: Vec<RowBuild>,
    pub section: Option<String>,
    pub arrows: Vec<BtnRef>,
    pub first: bool,
    pub last_offset: f64,
}
pub(super) struct Anim {
    group: String,
    w: Handle,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    t0: f64,
    button: Option<BtnRef>,
}
impl Anim {
    /// The opening drop held back until the screen shows.
    pub(super) fn shift(&mut self, wait: f64) {
        self.t0 += wait;
    }
}
#[derive(Default, Clone)]
pub(super) struct Surf {
    pub id: Option<u64>,
    pub entity: Handle,
    pub comp: Handle,
    pub root: Handle,
    pub at: f64,
    pub ready_in: Option<f64>,
    pub ready: bool,
    pub built: bool,
    pub placed: bool,
    pub shown: Option<bool>,
    pub asset: &'static str,
    pub key: Option<String>,
    pub anchor_gen: u64,
    pub strip: Vec<(usize, f64, f64, Handle)>,
    pub strip_px: f64,
}
#[derive(Clone, Copy)]
pub(super) struct GRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub depth: Option<f64>,
    pub icon: bool,
    pub opaque: bool,
    pub visible: bool,
}
#[derive(Clone, Copy)]
pub(super) struct Anchor {
    pub vehicle: bool,
    pub c: V3,
    pub r: V3,
    pub u: V3,
}
#[derive(Clone)]
pub(super) struct Touch {
    pub by: &'static str,
    pub x0: f64,
    pub y0: f64,
    pub d0: f64,
    pub dd: f64,
    pub xd: f64,
    pub yd: f64,
    pub kx: f64,
    pub ky: f64,
    pub settled: bool,
}
pub(super) struct Pend {
    pub b: Option<BtnRef>,
    pub view: ViewRef,
    pub x0: f64,
    pub y0: f64,
    pub by: &'static str,
    pub t0: f64,
}
#[derive(Default)]
pub(super) struct HandP {
    pub state: &'static str,
    pub last_d: Option<f64>,
    pub trig: bool,
    pub touch: Option<Touch>,
    pub pend: Option<Pend>,
    pub scroll: Option<(ViewRef, &'static str)>,
    pub drag: Option<BtnRef>,
    pub drag_by: Option<&'static str>,
    pub poke: Option<BtnRef>,
    pub stick: Stick,
}
pub(super) struct Cursor {
    pub host: Handle,
    pub inner: Vec<Handle>,
    pub outer: Vec<Handle>,
    pub dot: Handle,
    pub color: Option<C3>,
    pub x: f64,
    pub y: f64,
}
pub(super) struct PressFx {
    pub host: Handle,
    pub x: f64,
    pub y: f64,
    pub at: Option<f64>,
}

/// What the Lua host's observers and the visits share with the menu.
#[derive(Default)]
pub(super) struct Shared {
    pub menu_controller: Handle,
    pub vendor_clock: f64,
}

/// The menu's state (menu.lua's S).
#[derive(Default)]
pub(super) struct Menu {
    pub time: f64,
    pub open: bool,
    pub anchor: Option<Anchor>,
    pub anchor_gen: u64,
    pub menu: Option<Surf>,
    pub beams: [Surf; 2],
    pub section: Option<String>,
    pub hands: [HandP; 2],
    pub buttons: Vec<BtnRef>,
    pub views: Vec<(String, MView)>,
    pub hovered: Vec<BtnRef>,
    pub anims: Vec<Anim>,
    pub toast: Option<(String, f64)>,
    pub close_at: Option<f64>,
    pub refresh_at: Option<f64>,
    pub cap_on: bool,
    pub glass: Vec<Option<Surf>>,
    pub glass_rects: Vec<GRect>,
    pub glass_on: bool,
    pub glass_after: f64,
    pub slide_at: Option<f64>,
    pub haptics: HashMap<i32, u32>,
    pub column_offset: f64,
    pub column_target: f64,
    pub column_move: Option<(f64, f64, f64)>,
    pub panel_interactive_at: f64,
    pub panel_start: Option<f64>,
    pub panel_reveal_end: f64,
    pub open_anim_at: Option<f64>,
    pub in_click: bool,
    pub sound_in_click: bool,
    pub volume: f64,
    pub icons: Vec<BtnRef>,
    pub summary_rect: Option<GRect>,
    pub resource_rect: Option<GRect>,
    pub layer_col: Handle,
    pub layer_summary: Handle,
    pub layer_panel: Handle,
    pub summary_visible: bool,
    pub toast_widget: Handle,
    pub toast_bg: Handle,
    pub modal_group: Option<String>,
    pub anim1: bool,
    pub keep_scroll: Option<f64>,
    pub lang: String,
    pub lang_override: Option<String>,
    pub t: HashMap<&'static str, String>,
    pub retiring: Vec<u64>,
    pub cursors: Vec<Cursor>,
    pub press_fx: Vec<PressFx>,
    pub surface_warmup: Handle,
    pub column_action: Option<(BtnRef, f64)>,
    pub last_click: f64,
    pub pending: Option<(f64, String)>,
    pub slow: f64,
    pub armed_until: Option<(String, f64)>,
    pub open_when_unarmed: Option<f64>,
    pub fbt_calibrating: bool,
    /// The VR interface was switched off (xr_vr_interface=0): everything the menu held was let go once.
    pub interface_disabled: bool,
    pub wait_transition: bool,
    pub last_error: Option<String>,
    // the column's resource readout
    pub money_widget: Handle,
    pub weight_widget: Handle,
    pub weight_icon: Handle,
    pub resource_bg: Handle,
    pub resource_edge: Vec<Handle>,
    pub resource_at: Option<f64>,
    pub resource_text: Option<String>,
    pub resource_measure: Option<i32>,
    pub resource_target: Option<(f64, f64, f64)>,
    pub resource_layout: Option<(f64, f64, f64)>,
    // the sections' own state
    pub vehicle: Option<super::menu_data::Vehicle>,
    pub vehicles: Option<Vec<super::menu_data::Vehicle>>,
    pub stations: Option<Vec<super::menu_data::Station>>,
    pub radio_want: Option<(bool, i64, f64)>,
    pub radio_state: Option<super::menu_data::Radio>,
    pub last_station: Option<i64>,
    pub pool: Pool,
    pub shared: Shared,
    pub hud: super::menu_bits::HudFade,
    pub phone: super::menu_messages::Phone,
    pub setv: super::menu_setview::SetState,
    pub gp: super::menu_gameplay::Gp,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Menu {}

impl Menu {
    pub(super) fn tx(&self, k: &str) -> String {
        self.t.get(k).cloned().unwrap_or_else(|| k.to_string())
    }
    pub(super) fn view(&self, key: &str) -> Option<&MView> {
        self.views.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

// ---------------------------------------------------------------- sounds, haptics, entities
const SOUND_DIR: &str = "CyberpunkVRPort_Menu/sounds/";
pub(super) fn game_sound(name: &str) {
    let a = super::game_system("GetAudioSystem");
    super::lobby_rtti::call(&a, "Play", &[super::lobby_rtti::A::Name(k::cname(name))]);
}
/// play: the menu's own sound (the game's as a fallback), at the effects volume.
pub(super) fn play(m: &mut Menu, kind: &str) {
    let (file, fallback) = match kind {
        "open" => ("open.wav", "ui_messenger_recieved"),
        "close" => ("close.wav", "ui_gui_close"),
        "click" => ("click.wav", "ui_terminal_positive"),
        "notify" => ("notify.wav", "ui_messenger_select"),
        _ => return,
    };
    if m.in_click {
        m.sound_in_click = true;
    }
    let f = sp::Str::new(&format!("{SOUND_DIR}{file}"));
    let vol = if m.volume > 0.0 { m.volume } else { 1.0 };
    if sp::native::<bool>("VRMenuPlayWav", &[f.v(), crate::script_call::Value::Float(vol as f32)]) != Some(true) {
        game_sound(fallback);
    }
}
pub(super) fn haptic(m: &mut Menu, hand: usize, kind: i32) {
    *m.haptics.entry(kind).or_insert(0) += 1;
    sp::native::<()>("VRHapticEvent", &[crate::script_call::Value::Int32(kind), crate::script_call::Value::Int32(if hand == 1 { 1 } else { 2 })]);
}
pub(super) fn spawn(m: &Menu, path: &'static str, p: V3, q: Q) -> Surf {
    Surf { id: super::reload_core::spawn_at(path, p, q), at: m.time, asset: path, ..Default::default() }
}
fn retire_entity(id: u64) -> bool {
    super::reload_core::despawn(id)
}
pub(super) fn retire_pending(m: &mut Menu) {
    m.retiring.retain(|id| !retire_entity(*id));
}
pub(super) fn despawn(m: &mut Menu, h: &mut Surf) {
    if let Some(id) = h.id.take() {
        if !retire_entity(id) {
            m.retiring.push(id);
        }
    }
    (h.entity, h.comp, h.root) = (Handle::null(), Handle::null(), Handle::null());
}
pub(super) fn resolve(m: &Menu, h: &mut Surf) -> bool {
    if h.id.is_none() {
        return false;
    }
    if !h.root.is_null() {
        return true;
    }
    h.entity = sp::find_entity(h.id);
    if !h.entity.is_null() {
        h.comp = sp::find_comp(&h.entity, UI);
    }
    if !h.comp.is_null() {
        h.root = ink::child(&h.comp, "GetWidget", &[]);
    }
    if !h.root.is_null() {
        h.ready_in = Some(m.time - h.at);
        return true;
    }
    false
}
pub(super) fn unglitch(h: &Surf) {
    if h.comp.is_null() {
        return;
    }
    // SAFETY: the UI component's Float property.
    if let Some(p) = unsafe { crate::script_call::property(h.comp.instance, "glitchValue") } {
        unsafe {
            if *(p as *const f32) != 0.0 {
                *(p as *mut f32) = 0.0;
            }
        }
    }
}
pub(super) fn place(h: &Surf, p: V3, q: Q) {
    if !h.entity.is_null() {
        sp::set_world(&h.entity, p, q);
    }
}
pub(super) fn ready(h: &mut Surf) -> bool {
    if h.ready {
        return true;
    }
    if h.comp.is_null() || h.root.is_null() {
        return false;
    }
    if sp::native::<bool>("VRMenuSurfaceReady", &[crate::script_call::Value::Handle(&h.comp), crate::script_call::Value::Handle(&h.root)]) != Some(true) {
        return false;
    }
    let display = sp::find_comp(&h.entity, "display");
    if display.is_null() {
        return false;
    }
    h.ready = super::belt::toggle(&display, true);
    h.ready
}
/// transformFor: the quad's pose for its centre, right and up.
pub(super) fn pose(c: V3, r: V3, u: V3) -> (V3, Q) {
    (c, sp::quad_pose(r, u))
}

// ---------------------------------------------------------------- the glass behind the panels
fn glass_mesh(h: &mut Surf) -> Handle {
    if !h.comp.is_null() {
        return h.comp.clone();
    }
    h.entity = sp::find_entity(h.id);
    if !h.entity.is_null() {
        h.comp = sp::find_comp(&h.entity, "display");
    }
    h.comp.clone()
}
pub(super) fn despawn_glass(m: &mut Menu) {
    let gl = std::mem::take(&mut m.glass);
    for mut g in gl.into_iter().flatten() {
        despawn(m, &mut g);
    }
}
/// syncGlass: a glass (or icon, or opaque) plane behind each panel, icon and readout, placed and shown with it.
pub(super) fn sync_glass(m: &mut Menu, c: V3, r: V3, u: V3, moving: bool) {
    let mut want: Vec<GRect> = Vec::new();
    for b in &m.icons {
        let ib = b.icon.borrow();
        let Some(ib) = ib.as_ref() else { continue };
        let alpha = ink::get::<f32>(&ib.canvas, "GetOpacity", &[]).map_or(0.0, f64::from);
        let size = b.w - 8.0;
        want.push(GRect { x: ib.x0 + m.column_offset + (b.w - size) / 2.0, y: ib.y0 + (b.h - size) / 2.0, w: size, h: size, depth: None, icon: true, opaque: false, visible: alpha > 0.05 });
    }
    if let Some(mut s) = m.summary_rect {
        s.visible = !m.layer_summary.is_null() && ink::get::<f32>(&m.layer_summary, "GetOpacity", &[]).is_some_and(|o| o > 0.2);
        m.summary_rect = Some(s);
        want.push(s);
    }
    if let Some(r0) = m.resource_rect {
        want.push(r0);
    }
    if m.glass_on {
        let vis = !m.layer_panel.is_null() && ink::get::<f32>(&m.layer_panel, "GetOpacity", &[]).is_some_and(|o| o > 0.2);
        for g in m.glass_rects.iter_mut() {
            g.visible = vis;
            want.push(*g);
        }
    }
    let n = cross(r, u);
    let count = want.len().max(m.glass.len());
    m.glass.resize_with(count, || None);
    for i in 0..count {
        let rr = want.get(i).copied();
        let asset = rr.map(|r0| if r0.icon { ICON_ENT } else if r0.opaque { OPAQUE_ENT } else { GLASS_ENT });
        let mut h = m.glass[i].take();
        if let (Some(hh), Some(a)) = (h.as_mut(), asset) {
            if hh.asset != a {
                let mut old = h.take().unwrap_or_default();
                despawn(m, &mut old);
            }
        }
        if let (Some(_), None, Some(a)) = (rr, &h, asset) {
            let (p, q) = pose(c, r, u);
            h = Some(spawn(m, a, p, q));
        }
        if let Some(hh) = h.as_mut() {
            let comp = glass_mesh(hh);
            if !comp.is_null() {
                match rr {
                    Some(r0) if r0.visible => {
                        let key = format!("{},{},{},{}", r0.x as i64, r0.y as i64, r0.w as i64, r0.h as i64);
                        if hh.key.as_deref() != Some(key.as_str()) {
                            let sc = [(r0.w / PX) as f32, (r0.h / PX) as f32, 1.0f32];
                            // SAFETY: the mesh component's Vector3 property.
                            if let Some(p) = unsafe { crate::script_call::property(comp.instance, "visualScale") } {
                                unsafe { *(p as *mut [f32; 3]) = sc };
                            }
                            super::belt::toggle(&comp, false);
                            hh.shown = Some(false);
                        }
                        if hh.key.as_deref() != Some(key.as_str()) || moving || hh.anchor_gen != m.anchor_gen {
                            let cx = (r0.x + r0.w / 2.0 - CW / 2.0) / PX;
                            let cy = (CH / 2.0 - (r0.y + r0.h / 2.0)) / PX;
                            let depth = r0.depth.unwrap_or(if r0.icon { 0.0006 } else { GLASS_BACK });
                            let (p, q) = pose(add(add(add(c, mul(r, cx)), mul(u, cy)), mul(n, -depth)), r, u);
                            place(hh, p, q);
                            (hh.key, hh.anchor_gen) = (Some(key), m.anchor_gen);
                        }
                        let placed = m.menu.as_ref().is_some_and(|s| s.placed);
                        if hh.shown != Some(true) && placed && (r0.icon || m.time >= m.glass_after) {
                            super::belt::toggle(&comp, true);
                            hh.shown = Some(true);
                        }
                    }
                    _ => {
                        if hh.shown != Some(false) {
                            super::belt::toggle(&comp, false);
                            hh.shown = Some(false);
                        }
                    }
                }
            }
        }
        m.glass[i] = h;
    }
}

// ---------------------------------------------------------------- buttons
pub(super) fn clear_buttons(m: &mut Menu, group: &str) {
    m.buttons.retain(|b| b.group != group);
}
pub(super) fn drop_view(m: &mut Menu, view: &ViewRef) {
    m.buttons.retain(|b| !b.view.as_ref().is_some_and(|v| Rc::ptr_eq(v, view)));
}
/// button: a pressable rectangle of a group at (x, y) from the place `hh` measures from; painted unhovered at once.
#[allow(clippy::too_many_arguments)]
pub(super) fn button(m: &mut Menu, group: &str, hh: &At, x: f64, y: f64, w: f64, h: f64, click: Option<Act>, paint: Option<Paint>) -> BtnRef {
    let b = Rc::new(Btn {
        group: group.into(),
        x: Cell::new(hh.ox + x),
        y: Cell::new(hh.oy + y),
        w,
        h,
        click,
        paint,
        view: hh.view.clone(),
        visible: Cell::new(true),
        blocker: Cell::new(false),
        circle: Cell::new(false),
        contains: RefCell::new(None),
        selected: Cell::new(false),
        key: RefCell::new(None),
        mute: Cell::new(false),
        drag: RefCell::new(None),
        value: Rc::new(Cell::new(None)),
        show: RefCell::new(None),
        icon: RefCell::new(None),
        pulse_at: Cell::new(None),
        tip: RefCell::new(None),
        passive: Cell::new(false),
        also: RefCell::new(None),
    });
    m.buttons.push(b.clone());
    b.repaint(false);
    b
}
/// buttonAt: the topmost visible button under the point (a list's only where its row shows).
pub(super) fn button_at(m: &Menu, x: f64, y: f64) -> Option<BtnRef> {
    for b in m.buttons.iter().rev() {
        let mut by = b.y.get();
        if let Some(v) = &b.view {
            let v = v.borrow();
            by = v.y + b.y.get() - v.offset;
        }
        let visible = b.visible.get() && (b.group == "col" || m.time >= m.panel_interactive_at) && (m.modal_group.is_none() || m.modal_group.as_deref() == Some(b.group.as_str()) || (m.modal_group.as_deref() == Some("sort") && b.group == "col"));
        let (bx, bw, bh) = (b.x.get(), b.w, b.h);
        if visible && x >= bx && x <= bx + bw && y >= by && y <= by + bh && b.view.as_ref().is_none_or(|v| v.borrow().visible_at(x, y)) {
            let (dx, dy) = ((x - bx) / bw - 0.5, (y - by) / bh - 0.5);
            let in_circle = !b.circle.get() || dx * dx + dy * dy <= 0.25;
            let contained = b.contains.borrow().as_ref().is_none_or(|f| f(x - bx, y - by));
            // a blocker blocks within its shape (a tooltip's: its plate only)
            if b.blocker.get() {
                if contained {
                    return None;
                }
                continue;
            }
            if in_circle && contained {
                return Some(b.clone());
            }
        }
    }
    None
}
pub(super) fn toast(m: &mut Menu, msg: &str) {
    m.toast = Some((msg.to_string(), m.time + 2.5));
    play(m, "notify");
    if !m.toast_widget.is_null() {
        ink::set_text(&m.toast_widget, msg);
        ink::visible(&m.toast_widget, true);
        ink::visible(&m.toast_bg, true);
    }
}

// ---------------------------------------------------------------- the column and the slide-ins
pub(super) fn move_column(m: &mut Menu, target: f64) {
    if m.column_target == target {
        return;
    }
    m.column_target = target;
    m.column_move = Some((m.column_offset, target, m.time));
}
pub(super) fn step_column(m: &mut Menu) {
    if let Some((from, to, t0)) = m.column_move {
        let t = ((m.time - t0) / COLUMN_SHIFT).clamp(0.0, 1.0);
        let e = t * t * (3.0 - 2.0 * t);
        m.column_offset = from + (to - from) * e;
        if t >= 1.0 {
            (m.column_offset, m.column_move) = (to, None);
        }
    }
    if !m.layer_col.is_null() {
        ink::move_to(&m.layer_col, m.column_offset, 0.0);
    }
    if let Some(r) = m.resource_rect.as_mut() {
        r.x = COLX + m.column_offset + 74.0 + RES_INSET;
    }
    for b in &m.icons {
        if let Some(ib) = b.icon.borrow().as_ref() {
            b.x.set(ib.x0 + m.column_offset);
        }
    }
}
/// slideIn: a widget faded and slid into place after `delay` (a panel's not before its column has moved).
#[allow(clippy::too_many_arguments)]
pub(super) fn slide_in(m: &mut Menu, group: &str, w: &Handle, x: f64, y: f64, delay: f64, dx: f64, dy: f64, hit: Option<BtnRef>) {
    let mut delay = delay;
    if group != "col" {
        delay += (m.panel_start.unwrap_or(m.time) - m.time).max(0.0);
    }
    ink::opacity(w, 0.0);
    ink::move_to(w, x - dx, y - dy);
    if let Some(b) = &hit {
        b.visible.set(false);
    }
    m.anims.push(Anim { group: group.into(), w: w.clone(), x, y, dx, dy, t0: m.time + delay, button: hit });
    if group != "col" {
        m.panel_interactive_at = m.panel_interactive_at.max(m.time + delay + ANIM);
    }
    let lit = m.time + delay + ANIM / 2.0;
    if m.slide_at != Some(m.time) {
        (m.slide_at, m.glass_after) = (Some(m.time), lit);
    } else {
        m.glass_after = m.glass_after.min(lit);
    }
}
pub(super) fn drop_anims(m: &mut Menu, group: &str) {
    m.anims.retain(|a| a.group != group);
}
pub(super) fn step_anims(m: &mut Menu) {
    let time = m.time;
    let off = m.column_offset;
    m.anims.retain(|a| {
        let t = ((time - a.t0) / ANIM).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t) * (1.0 - t);
        ink::opacity(&a.w, e);
        ink::move_to(&a.w, a.x - a.dx * (1.0 - e), a.y - a.dy * (1.0 - e));
        if let Some(b) = &a.button {
            b.x.set(a.x - a.dx * (1.0 - e) + off);
            b.y.set(a.y - a.dy * (1.0 - e));
            b.visible.set(e > 0.05);
        }
        t < 1.0
    });
}

// ---------------------------------------------------------------- glyphs
/// A glyph of an atlas: (atlas, part, width, height, box x0, y0, x1, y1, scale).
pub(super) type Glyph = (&'static str, &'static str, f64, f64, f64, f64, f64, f64, f64);
pub(super) fn glyph_of(key: &str) -> Option<Glyph> {
    Some(match key {
        "call" => (k::ATLAS_PHONE, "phone_icon", 47.0, 44.0, 0.0, 0.0, 47.0, 44.0, 1.0),
        "message" => (k::ATLAS_MAPPIN, "talk", 64.0, 64.0, 5.0, 7.0, 59.0, 58.0, 1.0),
        "car" => (k::ATLAS_MAPPIN, "car", 64.0, 64.0, 1.0, 22.0, 63.0, 42.0, 1.0),
        "motorcycle" => (k::ATLAS_MAPPIN, "motorcycle", 64.0, 64.0, 1.0, 17.0, 63.0, 45.0, 1.0),
        "weaponized" => (k::ATLAS_MAPPIN, "vehicle_weaponized", 64.0, 64.0, 0.0, 11.0, 64.0, 54.0, 1.0),
        "cog" => (k::ATLAS_MAPPIN, "cog", 64.0, 64.0, 21.0, 18.0, 42.0, 39.0, 1.12),
        "radio" => (k::ATLAS_PHONE, "radio_icon", 57.0, 51.0, 1.0, 0.0, 57.0, 51.0, 1.0),
        "save" => (k::ATLAS_MAPPIN, "download", 64.0, 64.0, 12.0, 19.0, 52.0, 42.0, 1.0),
        "load" => (k::ATLAS_MAPPIN, "journal_all", 64.0, 64.0, 8.0, 10.0, 56.0, 52.0, 1.0),
        "credits" => (k::ATLAS_MAPPIN, "codex_glossary", 64.0, 64.0, 13.0, 7.0, 53.0, 56.0, 1.0),
        "exit" => (k::ATLAS_MAPPIN, "apartment", 64.0, 64.0, 1.0, 3.0, 63.0, 61.0, 0.85),
        "quit" => (k::ATLAS_MAPPIN, "Off", 64.0, 64.0, 11.0, 9.0, 54.0, 56.0, 1.0),
        "person" => (k::ATLAS_MAPPIN, "codex_character", 64.0, 64.0, 13.0, 10.0, 51.0, 54.0, 1.0),
        "map" => ("base\\gameplay\\gui\\fullscreen\\hub_menu\\hub_atlas.inkatlas", "ico_map", 57.0, 57.0, 0.0, 0.0, 57.0, 57.0, 1.12),
        _ => return None,
    })
}
pub(super) fn logo_size(name: &str) -> Option<(f64, f64)> {
    Some(match name {
        "30_principales" => (292.0, 260.0),
        "body_heat" => (352.0, 192.0),
        "dark_star" => (350.0, 153.0),
        "growl_fm" => (244.0, 323.0),
        "impulse_999" => (343.0, 308.0),
        "morro_rock" => (400.0, 192.0),
        "night_fm" => (344.0, 146.0),
        "no_station" => (230.0, 214.0),
        "pacific_dreams" => (267.0, 253.0),
        "radio_pebkac" => (355.0, 211.0),
        "radio_samizdat" => (239.0, 292.0),
        "ritual" => (344.0, 328.0),
        "royal_blue" => (362.0, 199.0),
        "the_dirge" => (335.0, 178.0),
        "vexElsTrom" => (382.0, 129.0),
        _ => return None,
    })
}
pub(super) fn gamepad_glyph(c: &Handle, cx: f64, cy: f64, s: f64, col: C3) -> Vec<Handle> {
    let parts = vec![
        k::rect(c, cx - 0.25 * s, cy - 0.12 * s, 0.50 * s, 0.20 * s, col, 1.0),
        k::image(c, cx - 0.33 * s, cy - 0.10 * s, 0.24 * s, 0.24 * s, k::ATLAS_PHONE, "circle", Some(col), 1.0),
        k::image(c, cx + 0.09 * s, cy - 0.10 * s, 0.24 * s, 0.24 * s, k::ATLAS_PHONE, "circle", Some(col), 1.0),
    ];
    let (kk, dx, dy) = (0.035 * s, cx - 0.20 * s, cy - 0.02 * s);
    k::rect(c, dx - 0.06 * s, dy - kk / 2.0, 0.12 * s, kk, k::BLACK, 1.0);
    k::rect(c, dx - kk / 2.0, dy - 0.06 * s, kk, 0.12 * s, k::BLACK, 1.0);
    let b = 0.06 * s;
    k::image(c, cx + 0.14 * s, cy - 0.09 * s, b, b, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
    k::image(c, cx + 0.21 * s, cy - 0.02 * s, b, b, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
    parts
}
/// fit: an atlas picture fitted into a box about its centre.
#[allow(clippy::too_many_arguments)]
pub(super) fn fit(parent: &Handle, cx: f64, cy: f64, bw: f64, bh: f64, atlas: u64, part: u64, pw: f64, ph: f64, col: C3, op: f64) -> Handle {
    let s = (bw / pw).min(bh / ph);
    let (w, h) = (pw * s, ph * s);
    k::image_raw(parent, cx - w / 2.0, cy - h / 2.0, w, h, atlas, part, Some(col), op)
}
pub(super) fn glyph(parent: &Handle, cx: f64, cy: f64, size: f64, g: Glyph, col: C3, op: f64) -> Handle {
    let (bw, bh) = (g.6 - g.4, g.7 - g.5);
    let s = (size * 0.42 / (bw * bh).sqrt()).min(size * 0.62 / bw.max(bh)) * g.8;
    let (w, h) = (g.2 * s, g.3 * s);
    let ox = ((g.4 + g.6) / 2.0 - g.2 / 2.0) * s;
    let oy = ((g.5 + g.7) / 2.0 - g.3 / 2.0) * s;
    k::image(parent, cx - w / 2.0 - ox, cy - h / 2.0 - oy, w, h, g.0, g.1, Some(col), op)
}
pub(super) fn chevron(parent: &Handle, cx: f64, cy: f64, a: f64, t: f64, dir: &str, col: C3, op: f64) -> Vec<Handle> {
    let (l, h) = (a * 2f64.sqrt() + t * 0.5, a / 2.0);
    let arms = match dir {
        "up" => [(cx - h, cy, -45.0), (cx + h, cy, 45.0)],
        "down" => [(cx - h, cy, 45.0), (cx + h, cy, -45.0)],
        "right" => [(cx, cy - h, 45.0), (cx, cy + h, -45.0)],
        _ => [(cx, cy - h, -45.0), (cx, cy + h, 45.0)],
    };
    arms.iter()
        .map(|(x, y, rot)| {
            let r = k::rect(parent, x - l / 2.0, y - t / 2.0, l, t, col, op);
            ink::rotation(&r, *rot);
            r
        })
        .collect()
}
pub(super) fn glass_rect(sx: f64, sy: f64, w: f64, h: f64, r: f64) -> GRect {
    let compact = h < 100.0;
    let i = if compact { 2.0 } else { (r * 0.5).floor() };
    GRect { x: sx + i, y: sy + i, w: w - 2.0 * i, h: h - 2.0 * i, depth: compact.then_some(0.0006), icon: false, opaque: false, visible: true }
}
/// glass: a panel's holo look, its glass plane behind it when its place on the screen is given.
pub(super) fn glass(m: &mut Menu, parent: &Handle, w: f64, h: f64, at: Option<(f64, f64)>) {
    if let Some((sx, sy)) = at {
        m.glass_rects.push(glass_rect(sx, sy, w, h, R_PANEL));
    }
    k::holo_panel(&mut m.pool, parent, w, h);
}
pub(super) fn header(parent: &Handle, w: f64, title: &str) -> Handle {
    let c = ink::canvas(parent, 0.0, 0.0, w, HEAD);
    k::text(&c, PAD + 18.0, 12.0, w - 340.0, HEAD - 26.0, title, 52.0, k::WHITE, "left", false);
    k::rect(&c, PAD + 16.0, HEAD - 8.0, w - 2.0 * PAD - 32.0, 2.0, k::ICE, 0.16);
    c
}
pub(super) fn count_pill(c: &Handle, x1: f64, n: usize) {
    let s = n.to_string();
    let w = 52.0 + 22.0 * s.len() as f64;
    k::round_fill(c, x1 - w, 26.0, w, HEAD - 52.0, k::YELLOW, 0.10, Some(22.0));
    k::round_edge(c, x1 - w, 26.0, w, HEAD - 52.0, k::YELLOW, 0.85, Some(22.0));
    k::text(c, x1 - w, 26.0, w, HEAD - 52.0, &s, 36.0, k::YELLOW, "center", false);
}

// ---------------------------------------------------------------- widgets
#[derive(Default)]
pub(super) struct RowOpts {
    pub icon: Option<Rc<dyn Fn(&Handle, f64, f64, f64, f64) -> Handle>>,
    pub icon_w: Option<f64>,
    pub clip: Option<usize>,
    pub disabled: bool,
    pub eq: bool,
    pub right: Option<String>,
    pub right_w: Option<f64>,
    pub right_col: Option<C3>,
    pub tint_icon: bool,
    pub selected: bool,
}
/// listRow: a list's row -- an optional icon, the label, the equaliser, a right-hand value -- pressable, highlighted.
pub(super) fn list_row(group: &str, label: String, click: Act, opts: RowOpts) -> Row {
    let group = group.to_string();
    let opts = Rc::new(opts);
    Row {
        h: ROW,
        build: Rc::new(move |m: &mut Menu, itc: &Handle, it: RowCtx| {
            let (w, h) = (it.cw, ROW - 14.0);
            let c = ink::canvas(itc, 0.0, 0.0, w, h);
            let bg = k::round_fill(&c, 0.0, 0.0, w, h, k::NAVY, 0.18, Some(4.0));
            let edge = k::cut_frame(&mut m.pool, &c, 1.0, 1.0, w - 2.0, h - 2.0, k::CYAN, 0.22, Some(10.0), Some(1.0));
            let mut tx = 36.0;
            let mut icon = Handle::null();
            if let Some(f) = &opts.icon {
                let iw = opts.icon_w.unwrap_or(84.0);
                icon = f(&c, 30.0, 0.0, iw, h);
                tx = 30.0 + iw + 24.0;
            }
            let txt = k::text(&c, tx, 0.0, w - tx - 34.0 - opts.right_w.unwrap_or(0.0), h, &k::clip(&label, opts.clip.unwrap_or(24)), 50.0, if opts.disabled { k::GREY } else { k::TEXT }, "left", false);
            if opts.eq {
                for (i, hh) in [34.0, 56.0, 42.0].iter().enumerate() {
                    k::rect(&c, w - 110.0 + i as f64 * 22.0, (h - hh) / 2.0, 12.0, *hh, k::YELLOW, 1.0);
                }
            }
            if let Some(r) = &opts.right {
                let rw = opts.right_w.unwrap_or(160.0);
                k::text(&c, w - 30.0 - rw, 0.0, rw, h, r, 40.0, opts.right_col.unwrap_or(k::YELLOW), "right", false);
            }
            let o = opts.clone();
            let paint: Paint = Rc::new(move |hover, sel| {
                if o.disabled {
                    ink::opacity(&bg, 0.02);
                    k::tint(&edge, k::GREY, Some(0.0));
                    return;
                }
                let lit = if hover || sel { Some(k::YELLOW) } else { None };
                ink::tint(&bg, lit.unwrap_or(k::NAVY));
                ink::opacity(&bg, if hover { 0.16 } else if sel { 0.12 } else { 0.18 });
                k::tint(&edge, lit.unwrap_or(k::CYAN), Some(if lit.is_some() { 1.0 } else { 0.22 }));
                ink::tint(&txt, if hover { k::WHITE } else if sel { k::YELLOW } else { k::TEXT });
                if !icon.is_null() && o.tint_icon {
                    ink::tint(&icon, if hover { k::WHITE } else if sel { k::YELLOW } else { k::CYAN });
                }
            });
            let at = At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) };
            let b = button(m, &group, &at, 0.0, 0.0, w, h, Some(click.clone()), Some(paint));
            b.selected.set(opts.selected);
            b.repaint(false);
            if m.anim1 && it.first {
                slide_in(m, &group, &c, 0.0, 0.0, 0.03 + it.index as f64 * 0.035, 0.0, 36.0, None);
            }
        }),
    }
}
/// action: a wide button with an optional glyph; disabled it says it is not available.
#[allow(clippy::too_many_arguments)]
pub(super) fn action(m: &mut Menu, parent: &Handle, group: &str, hh: &At, x: f64, y: f64, w: f64, h: f64, label: &str, enabled: bool, f: Act, col_on: Option<C3>, g: Option<Glyph>) -> BtnRef {
    let base = col_on.unwrap_or(k::CYAN);
    let bg = k::round_fill(parent, x, y, w, h, k::NAVY, if enabled { 0.18 } else { 0.04 }, Some(4.0));
    let edge = k::cut_frame(&mut m.pool, parent, x, y, w, h, if enabled { base } else { k::GREY }, if enabled { 0.5 } else { 0.2 }, Some(10.0), Some(1.5));
    let icon = g.map(|g| glyph(parent, x + h * 0.62, y + h / 2.0, h * 1.05, g, if enabled { base } else { k::GREY }, 1.0)).unwrap_or_else(Handle::null);
    let tx = if g.is_some() { h * 0.9 } else { 0.0 };
    let txt = k::text(parent, x + tx, y, w - tx, h, label, 54.0, if enabled { k::TEXT } else { k::GREY }, "center", false);
    let click: Act = Rc::new(move |m: &mut Menu, hand| {
        if enabled {
            f(m, hand);
        } else {
            let t = m.tx("blocked");
            toast(m, &t);
        }
    });
    let paint: Paint = Rc::new(move |hover, _| {
        if !enabled {
            return;
        }
        ink::tint(&bg, if hover { k::YELLOW } else { k::NAVY });
        ink::opacity(&bg, if hover { 0.16 } else { 0.18 });
        k::tint(&edge, if hover { k::YELLOW } else { base }, Some(if hover { 1.0 } else { 0.5 }));
        ink::tint(&txt, if hover { k::YELLOW } else { k::TEXT });
        if !icon.is_null() {
            ink::tint(&icon, if hover { k::WHITE } else { base });
        }
    });
    button(m, group, hh, x, y, w, h, Some(click), Some(paint))
}
/// slider: a track with a knob, dragged (its `set` gets the stepped value and answers what took), shown from outside.
#[allow(clippy::too_many_arguments)]
pub(super) fn slider(m: &mut Menu, parent: &Handle, group: &str, hh: &At, x: f64, y: f64, w: f64, h: f64, lo: f64, hi: f64, value: Option<f64>, set: Rc<dyn Fn(&mut Menu, f64) -> Option<f64>>, step: Option<f64>, on_phase: Option<Rc<dyn Fn(&mut Menu, &str)>>) -> BtnRef {
    let step = step.unwrap_or(1.0);
    const KNOB: f64 = 64.0;
    let ty = y + h / 2.0;
    let track = k::round_fill(parent, x, ty - 6.0, w, 12.0, k::CYAN, 0.35, Some(6.0));
    let fill = k::round_fill(parent, x, ty - 6.0, 0.0, 12.0, k::CYAN, 1.0, Some(6.0));
    let knob = ink::canvas(parent, x, ty - KNOB / 2.0, KNOB, KNOB);
    let ring = k::image(&knob, 0.0, 0.0, KNOB, KNOB, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
    k::image(&knob, 7.0, 7.0, KNOB - 14.0, KNOB - 14.0, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
    let dot = k::image(&knob, KNOB / 2.0 - 10.0, KNOB / 2.0 - 10.0, 20.0, 20.0, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
    let state = Rc::new(Cell::new((false, false)));
    let (f2, k2) = (fill.clone(), knob.clone());
    let show: Rc<dyn Fn(f64)> = Rc::new(move |v| {
        let t = if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
        k::round_size(&f2, w * t, 12.0, Some(6.0));
        ink::move_to(&k2, x + w * t - KNOB / 2.0, ty - KNOB / 2.0);
    });
    let (st2, fill2, ring2, dot2, track2) = (state.clone(), fill.clone(), ring.clone(), dot.clone(), track.clone());
    let paint_all: Rc<dyn Fn()> = Rc::new(move || {
        let (held, hovered) = st2.get();
        let col = if held || hovered { k::YELLOW } else { k::CYAN };
        ink::tint(&fill2, col);
        ink::tint(&ring2, col);
        ink::tint(&dot2, col);
        ink::opacity(&track2, if held || hovered { 0.5 } else { 0.35 });
    });
    show(value.unwrap_or(lo));
    let (st3, pa3) = (state.clone(), paint_all.clone());
    let paint: Paint = Rc::new(move |hover, _| {
        let (held, _) = st3.get();
        st3.set((held, hover));
        pa3();
    });
    let b = button(m, group, hh, x - KNOB / 2.0, y, w + KNOB, h, None, Some(paint));
    let x0 = b.x.get() + KNOB / 2.0;
    let grab = Rc::new(Cell::new(0.0));
    let value_cell = b.value.clone();
    value_cell.set(value);
    let (st4, pa4, show4) = (state, paint_all, show.clone());
    let drag: DragFn = Rc::new(move |m: &mut Menu, px, phase, _hand, _py| {
        if phase == "begin" {
            let (_, hov) = st4.get();
            st4.set((true, hov));
            let cur = value_cell.get().unwrap_or(lo);
            let t = if hi > lo { ((cur - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
            let knob_x = x0 + w * t;
            grab.set(match px {
                Some(p) if (p - knob_x).abs() <= KNOB / 2.0 => knob_x - p,
                _ => 0.0,
            });
            if let Some(op) = &on_phase {
                op(m, "begin");
            }
        } else if phase == "end" {
            let (_, hov) = st4.get();
            st4.set((false, hov));
        }
        if let (true, Some(p)) = (phase != "end", px) {
            let t = ((p + grab.get() - x0) / w).clamp(0.0, 1.0);
            let v = (lo + (t * (hi - lo) / step + 0.5).floor() * step).clamp(lo.min(hi), lo.max(hi));
            if value_cell.get().is_none_or(|cur| (v - cur).abs() > step * 0.001) {
                if let Some(got) = set(m, v) {
                    value_cell.set(Some(got));
                    show4(got);
                }
            }
        }
        pa4();
        if phase == "end" {
            if let Some(op) = &on_phase {
                op(m, "end");
            }
        }
    });
    *b.drag.borrow_mut() = Some(drag);
    let vc = b.value.clone();
    *b.show.borrow_mut() = Some(Rc::new(move |v| {
        vc.set(Some(v));
        show(v);
    }));
    b
}
/// stepButton: a square step button (a chevron direction, or minus / plus).
#[allow(clippy::too_many_arguments)]
pub(super) fn step_button(m: &mut Menu, parent: &Handle, group: &str, hh: &At, x: f64, y: f64, w: f64, h: f64, mark: Result<&str, bool>, enabled: bool, f: Act) -> BtnRef {
    let bg = k::round_fill(parent, x, y, w, h, k::CYAN, if enabled { 0.14 } else { 0.04 }, Some(R_CELL));
    let edge = k::round_edge(parent, x, y, w, h, if enabled { k::CYAN } else { k::GREY }, if enabled { 0.9 } else { 0.3 }, Some(R_CELL));
    let col = if enabled { k::CYAN } else { k::GREY };
    let bars = match mark {
        Ok(dir) => chevron(parent, x + w / 2.0, y + h / 2.0, w.min(h) * 0.20, 7.0, dir, col, 1.0),
        Err(plus) => {
            let (l, t) = (w.min(h) * 0.42, 8.0);
            let mut b = vec![k::rect(parent, x + (w - l) / 2.0, y + (h - t) / 2.0, l, t, col, 1.0)];
            if plus {
                b.push(k::rect(parent, x + (w - t) / 2.0, y + (h - l) / 2.0, t, l, col, 1.0));
            }
            b
        }
    };
    let click: Act = Rc::new(move |m, hand| {
        if enabled {
            f(m, hand);
        }
    });
    let paint: Paint = Rc::new(move |hover, _| {
        if !enabled {
            return;
        }
        ink::tint(&bg, if hover { k::YELLOW } else { k::CYAN });
        ink::opacity(&bg, if hover { 0.30 } else { 0.14 });
        k::tint(&edge, if hover { k::YELLOW } else { k::CYAN }, Some(if hover { 1.0 } else { 0.9 }));
        k::tint(&bars, if hover { k::WHITE } else { k::CYAN }, None);
    });
    button(m, group, hh, x, y, w, h, Some(click), Some(paint))
}
/// arrowBar: the list's arrow above or below its window, live while the list can scroll that way.
#[allow(clippy::too_many_arguments)]
pub(super) fn arrow_bar(m: &mut Menu, parent: &Handle, group: &str, hh: &At, x: f64, y: f64, w: f64, h: f64, up: bool, view: ViewRef) -> BtnRef {
    arrow_bar_of(m, parent, group, hh, x, y, w, h, up, Rc::new(move || Some(view.clone())))
}
/// arrowBar over whatever view `current` names now (the settings' list is drawn anew).
#[allow(clippy::too_many_arguments)]
pub(super) fn arrow_bar_of(m: &mut Menu, parent: &Handle, group: &str, hh: &At, x: f64, y: f64, w: f64, h: f64, up: bool, current: Rc<dyn Fn() -> Option<ViewRef>>) -> BtnRef {
    let bg = k::round_fill(parent, x, y, w, h, k::ICE, 0.04, Some(h / 2.0));
    let edge = k::round_edge(parent, x, y, w, h, k::ICE, 0.0, Some(h / 2.0));
    let chev = chevron(parent, x + w / 2.0, y + h / 2.0, 19.0, 7.0, if up { "up" } else { "down" }, k::CYAN, 1.0);
    let live = move |v: &ViewRef| {
        let v = v.borrow();
        v.scrollable() && if up { !v.at_top() } else { !v.at_bottom() }
    };
    let c1 = current.clone();
    let click: Act = Rc::new(move |_m, _| {
        if let Some(v1) = c1().filter(|v| live(v)) {
            let hh = v1.borrow().h;
            v1.borrow_mut().scroll_by(if up { -1.0 } else { 1.0 } * hh * 0.8);
        }
    });
    let paint: Paint = Rc::new(move |hover, _| {
        let on = current().is_some_and(|v| live(&v));
        let lit = if hover && on { Some(k::YELLOW) } else { None };
        ink::tint(&bg, lit.unwrap_or(k::ICE));
        ink::opacity(&bg, if on { if hover { 0.24 } else { 0.04 } } else { 0.02 });
        k::tint(&edge, lit.unwrap_or(k::ICE), Some(if on && hover { 1.0 } else { 0.0 }));
        k::tint(&chev, if lit.is_some() { k::WHITE } else { k::CYAN }, Some(if on { 1.0 } else { 0.25 }));
    });
    button(m, group, hh, x, y, w, h, Some(click), Some(paint))
}
pub(super) fn list_size(rows: &[Row], max_h: f64) -> (f64, bool) {
    let total: f64 = rows.iter().map(|r| r.h).sum();
    if total <= max_h {
        (total, false)
    } else {
        (max_h + ARROW_GAP + ARROW_H + ARROW_FOOT, true)
    }
}
/// list: the rows in a scroll view under `key` (arrows when longer than its window), scrolled to `keep`.
#[allow(clippy::too_many_arguments)]
pub(super) fn list(m: &mut Menu, p: &Handle, group: &str, key: &str, sx: f64, sy: f64, top: f64, w: f64, rows: Vec<Row>, max_h: f64, keep: Option<f64>) -> ViewRef {
    let total: f64 = rows.iter().map(|r| r.h).sum();
    let scrolls = total > max_h;
    let view_h = if scrolls { max_h - ARROW_LIFT } else { total.max(1.0) };
    let mut v = View::new(p, PAD, top, sx + PAD, sy + top, w, view_h, super::lobby_scroll::Style { track: k::CYAN, thumb: k::CYAN, held: k::YELLOW });
    let mut builds = Vec::new();
    for r in rows {
        v.add(r.h);
        builds.push(r.build);
    }
    let vr: ViewRef = Rc::new(RefCell::new(v));
    let mut arrows = Vec::new();
    if scrolls {
        let aw = (w - 14.0) / 2.0;
        let hh = at(sx, sy);
        arrows.push(arrow_bar(m, p, group, &hh, PAD, top + view_h + ARROW_GAP, aw, ARROW_H, true, vr.clone()));
        arrows.push(arrow_bar(m, p, group, &hh, PAD + aw + 14.0, top + view_h + ARROW_GAP, aw, ARROW_H, false, vr.clone()));
    }
    if let Some(kp) = keep {
        vr.borrow_mut().scroll_to(kp);
    }
    m.views.retain(|(k0, _)| k0 != key);
    m.views.push((key.to_string(), MView { v: vr.clone(), builds, section: m.section.clone(), arrows, first: true, last_offset: f64::NAN }));
    update_view(m, key, 0.0, 0);
    vr
}
/// A view's update with its rows built (after the view's own update, outside its borrow) and its arrows repainted on a
/// scroll (V.onScroll).
pub(super) fn update_view(m: &mut Menu, key: &str, dt: f64, budget: usize) {
    let Some((vr, builds, first)) = m.views.iter().find(|(k0, _)| k0 == key).map(|(_, mv)| (mv.v.clone(), mv.builds.clone(), mv.first)) else { return };
    let mut pending: Vec<(usize, Handle, f64)> = Vec::new();
    vr.borrow_mut().update(dt, budget, &mut |i, c, y| pending.push((i, c.clone(), y)));
    let (vx, vy, cw) = {
        let v = vr.borrow();
        (v.x, v.y, v.cw_pub)
    };
    for (i, c, y) in pending {
        let h = vr.borrow().items.get(i).map_or(0.0, |it| it.h);
        if let Some(b) = builds.get(i).cloned() {
            b(m, &c, RowCtx { y, h, index: i + 1, vx, vy, cw, first, view: vr.clone() });
        }
    }
    let off = vr.borrow().offset;
    let mut repaint = Vec::new();
    if let Some((_, mv)) = m.views.iter_mut().find(|(k0, _)| k0 == key) {
        mv.first = false;
        if mv.last_offset != off {
            mv.last_offset = off;
            repaint = mv.arrows.clone();
        }
    }
    for b in repaint {
        let hovered = m.hovered.iter().any(|h| Rc::ptr_eq(h, &b));
        b.repaint(hovered);
    }
}
