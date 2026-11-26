//! modules/holo.lua: the lobby's menu as a fan of glass panels in the air before the player -- each page a spawned UI
//! quad (vr_lobby_holo.ent) on an arc at the spec's bearing, distance and height, turned to the eyes; a pushed page
//! slides in on the right, the one before steps left and older ones fade; a window page (vr_lobby_window.ent) grows out
//! of the top panel into the room; the hovered row's picture shows on its own preview quad. The rows are drawn with the
//! ink kit -- headers, texts, pictures, cards and posters, rows with an image or icon, sliders, left/right choosers,
//! plain rows -- in a scroll view; the hands point at them (VRMenuPointer: a fingertip touching, or the beam with the
//! trigger), press, drag the list and slide the sliders; marks and ripples where they point, the beams drawn to the eye.
use super::lobby::Lobby;
use super::lobby_ink::{self as ink, C3};
use super::lobby_scroll::{Style, View};
use super::lobby_spawn::{self as sp, cross, norm, quad_pose, Surface, V3};
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const ENT: &str = "vrport\\ui\\vr_lobby_holo.ent";
const TAG: &str = "VRLobbyHolo";
const UI: &str = "vrp_lobby_ui";
const CW: f64 = 2580.0;
const CH: f64 = 1980.0;
const HAPTIC_CLICK: i32 = 20;
const HAPTIC_HOVER: i32 = 21;
const EYE: f64 = 1.7;
pub(super) const RED: C3 = [1.10, 0.34, 0.30];
pub(super) const RED_DIM: C3 = [0.34, 0.08, 0.07];
pub(super) const GLASS: C3 = [0.16, 0.035, 0.03];
pub(super) const TEXT: C3 = [1.00, 0.52, 0.46];
pub(super) const CYAN: C3 = [0.34, 0.92, 1.00];
pub(super) const WHITE: C3 = [1.0, 0.96, 0.92];
pub(super) const DIM: C3 = [0.38, 0.17, 0.16];
const BLACK: C3 = [0.0, 0.0, 0.0];
const MARK: C3 = RED;
const MARK_HOT: C3 = TEXT;
const MARK_DOT: C3 = TEXT;
const MARK_M: f64 = 0.04;
const RIPPLE: f64 = 0.24;
const PHONE_ATLAS: &str = "base\\gameplay\\gui\\widgets\\phone\\new_phone_assets.inkatlas";
const LOGO_ATLAS: &str = "base\\gameplay\\gui\\fullscreen\\main_menu\\menu_background.inkatlas";
const LOGO_W: f64 = 940.0;
const LOGO_H: f64 = 230.0;
const PAD: f64 = 70.0;
const HEAD_Y: f64 = 60.0;
const HEAD_H: f64 = 250.0;
const ROW_H: f64 = 148.0;
const ROW_GAP: f64 = 18.0;
const FOOT_H: f64 = 120.0;
const FOOT_BUTTON_W: f64 = 190.0;
const FOOT_ICON_H: f64 = 100.0;
const CUT: f64 = 64.0;
const TOUCH: f64 = 0.006;
const HOVER_TIP: f64 = 0.06;
const PULL: f64 = 0.75;
const LET_GO: f64 = 0.35;
const SLIDE: f64 = 7.0;
const FADE: f64 = 6.0;
const IMAGE_ROW_H: f64 = 240.0;
const ICON_ROW_H: f64 = 190.0;
const CARDS_ROW_H: f64 = 900.0;
const POSTER_ROW_H: f64 = 1200.0;
const PICTURE_ROW_H: f64 = 560.0;
const DIMMED: C3 = [0.62, 0.62, 0.62];
const PHOTO: C3 = [0.37, 0.39, 0.43];
const CLEAR_GLASS: f64 = 0.12;
const DRAG: f64 = 28.0;
const PREVIEW_ENT: &str = "vrport\\ui\\vr_lobby_preview.ent";
const PV_X: f64 = 130.0;
const PV_Y: f64 = 130.0;
const PV_W: f64 = CW - 2.0 * PV_X;
const PV_SETTLE: f64 = 0.25;
const WINDOW_ENT: &str = "vrport\\ui\\vr_lobby_window.ent";
const GROW: f64 = 0.8;
const WINDOW_SETTLE: f64 = 0.25;
fn pv_h() -> f64 {
    (PV_W * 9.0 / 16.0).floor()
}

// ---------------------------------------------------------------- what the pages are made of
pub(super) type Act = Rc<dyn Fn(&mut Lobby, usize)>;
pub(super) struct Slider {
    pub min: f64,
    pub max: f64,
    pub step: Option<f64>,
    pub value: f64,
    pub format: Option<Rc<dyn Fn(f64) -> String>>,
    pub set: Rc<dyn Fn(&mut Lobby, f64)>,
}
/// What a row that draws itself (Row::draw) is given to draw in: the panels' colours.
pub(super) struct Palette {
    pub glyph: C3,
    pub text: C3,
    pub dim: C3,
    pub red: C3,
    pub line: C3,
    pub white: C3,
}
const PALETTE: Palette = Palette { glyph: WHITE, text: TEXT, dim: DIM, red: RED, line: RED_DIM, white: WHITE };
/// A row's own drawing: (canvas, width, height, palette).
pub(super) type Draw = Rc<dyn Fn(&Handle, f64, f64, &Palette)>;
pub(super) struct Card {
    pub label: String,
    pub picture: (String, String),
    pub on: bool,
    pub go: Option<Act>,
}
/// One key of a row of keys (Row::keys): its text, its width in key units (1 = a letter), what pressing it does.
pub(super) struct Key {
    pub label: String,
    pub units: f64,
    pub go: Act,
}
#[derive(Default)]
pub(super) struct Row {
    pub key: String,
    pub label: String,
    /// A row of keys side by side (a keyboard's line), each pressed on its own.
    pub keys: Option<Vec<Key>>,
    pub detail: Option<String>,
    pub disabled: bool,
    pub header: bool,
    pub text: bool,
    pub warn: bool,
    pub picture: Option<(String, String)>,
    pub cards: Option<Vec<Card>>,
    pub poster: Option<f64>,
    pub image: Option<Handle>,
    pub icon: Option<(String, String)>,
    pub sign: Option<String>,
    pub indent: f64,
    pub slider: Option<Slider>,
    pub left: Option<Act>,
    pub right: Option<Act>,
    pub go: Option<Act>,
    pub value: Option<String>,
    pub on: bool,
    pub preview: Option<Handle>,
    pub preview_text: Option<String>,
    pub preview_detail: Option<String>,
    /// The creation's figure zoom while this row is hovered (body, face, hands).
    pub zoom: Option<&'static str>,
    /// A row's own height (px), for one that draws itself.
    pub height: Option<f64>,
    /// A row that draws itself (`height` px tall): nothing on it to press.
    pub draw: Option<Draw>,
}
impl Row {
    pub(super) fn new(key: &str, label: &str) -> Row {
        Row { key: key.into(), label: label.into(), ..Default::default() }
    }
    pub(super) fn go(mut self, f: impl Fn(&mut Lobby, usize) + 'static) -> Row {
        self.go = Some(Rc::new(f));
        self
    }
}
pub(super) type Glyph = Rc<dyn Fn(&Handle, f64, f64, f64, C3) -> Vec<Handle>>;
pub(super) struct FootButton {
    pub key: String,
    pub glyph: Glyph,
    pub go: Option<Act>,
}
pub(super) type Rows = Rc<dyn Fn(&mut Lobby) -> Vec<Row>>;
/// A page's heading read at each build (the texts in the game's language): title, subtitle, code and its back button.
#[derive(Default)]
pub(super) struct Heading {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub code: Option<String>,
    pub back: Option<String>,
}
pub(super) struct Page {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub code: Option<String>,
    pub logo: bool,
    pub rows: Rows,
    pub footer: Option<Rc<dyn Fn(&mut Lobby) -> Vec<FootButton>>>,
    pub on_back: Option<Rc<dyn Fn(&mut Lobby)>>,
    pub update: Option<Rc<dyn Fn(&mut Lobby, f64) -> bool>>,
    pub scroll: std::cell::Cell<f64>,
    /// The heading in place of title / subtitle / code (and the back button's own text), read at each build.
    pub heading: Option<Rc<dyn Fn() -> Heading>>,
}
impl Page {
    pub(super) fn new(title: Option<String>, code: &str, rows: impl Fn(&mut Lobby) -> Vec<Row> + 'static) -> Page {
        Page {
            title,
            subtitle: None,
            code: Some(code.into()),
            logo: false,
            rows: Rc::new(rows),
            footer: None,
            on_back: None,
            update: None,
            scroll: std::cell::Cell::new(0.0),
            heading: None,
        }
    }
    /// The heading shown: the page's own (read now) or its fixed texts.
    fn heading_now(&self) -> Heading {
        match &self.heading {
            Some(h) => h(),
            None => Heading { title: self.title.clone(), subtitle: self.subtitle.clone(), code: self.code.clone(), back: None },
        }
    }
}
pub(super) type PageRef = Rc<Page>;

// ---------------------------------------------------------------- the panels
#[derive(Clone, Copy)]
pub(super) struct Layout {
    cw: f64,
    ch: f64,
    row_x: f64,
    row_w: f64,
    row_top: f64,
    foot_y: f64,
    list_w: f64,
    list_h: f64,
    slider_w: f64,
    slider_x: f64,
    clear_x: Option<f64>,
}
fn layout(cw: f64, ch: f64, row_w: f64) -> Layout {
    let (row_x, row_top, foot_y) = (130.0, 360.0, ch - 230.0);
    let slider_w = 820f64.min(((row_w - 360.0) / 2.0).floor());
    Layout { cw, ch, row_x, row_w, row_top, foot_y, list_w: row_w + 40.0, list_h: foot_y - 40.0 - row_top, slider_w, slider_x: row_w - 360.0 - slider_w, clear_x: None }
}
fn panel_layout() -> Layout {
    layout(CW, CH, CW - 260.0)
}
fn window_layout() -> Layout {
    let mut w = layout(3190.0, 2900.0, 1650.0);
    w.clear_x = Some(w.row_x + w.list_w + 60.0);
    w
}
#[derive(Clone, Copy, Default)]
pub(super) struct Frame {
    pub c: V3,
    pub r: V3,
    pub u: V3,
    pub w: f64,
    pub h: f64,
}
#[derive(Clone)]
pub(super) struct Hit {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    pub row: String,
    pub part: String,
    list: bool,
}
struct CardLook {
    on: bool,
    poster: bool,
    field: Handle,
    frame: Vec<Handle>,
    picture: Handle,
    label: Handle,
}
#[derive(Default)]
struct Look {
    key: String,
    has_go: bool,
    header: bool,
    button: bool,
    cards: Option<Vec<CardLook>>,
    /// A row of keys: each key's field and text.
    keys: Vec<(Handle, Handle)>,
    live: bool,
    on: bool,
    field: Handle,
    frame: Vec<Handle>,
    line: Handle,
    label: Handle,
    detail: Handle,
    glyph: Vec<Handle>,
    sign: Handle,
    arrows: Vec<(String, Handle, Handle)>,
    knob: Handle,
    fill: Handle,
    value: Handle,
    slider: bool,
}
struct Mark {
    host: Handle,
    inner: Vec<Handle>,
    outer: Vec<Handle>,
    color: C3,
    shown: bool,
}
struct Ripple {
    host: Handle,
    x: f64,
    y: f64,
    at: Option<f64>,
}
#[derive(Default)]
pub(super) struct Panel {
    pub page: Option<PageRef>,
    pub slot: f64,
    target: f64,
    pub alpha: f64,
    alpha_target: f64,
    hits: Vec<Hit>,
    pub dirty: bool,
    l: Option<Layout>,
    pub frame: Frame,
    pub s: Surface,
    pub ready: bool,
    index: usize,
    placed: bool,
    shown_alpha: Option<f64>,
    layer: Handle,
    glass: Handle,
    top: Handle,
    marks: Vec<Mark>,
    ripple: Option<Ripple>,
    pub builds: u32,
    looks: HashMap<String, Look>,
    pub view: Option<View>,
    built_page: Option<PageRef>,
    footer: HashMap<String, FootButton>,
    rows_cache: Vec<Rc<Row>>,
    hover: Option<Hit>,
    pub window: bool,
    pub pages: Vec<PageRef>,
    pub grow: f64,
    opening: bool,
    panel_w: f64,
    panel_h: f64,
    from: Frame,
    to: Frame,
    painted_at: Option<f64>,
    placed_at: Option<f64>,
    scaled_at: Option<f64>,
    shown_at: Option<f64>,
}
impl Panel {
    fn lay(&self) -> Layout {
        self.l.unwrap_or_else(panel_layout)
    }
    fn page(&self) -> PageRef {
        self.page.clone().unwrap_or_else(|| Rc::new(Page::new(None, "VR//LOBBY", |_| Vec::new())))
    }
}
pub(super) type PanelRef = Rc<RefCell<Panel>>;

#[derive(Clone, Copy)]
pub(super) struct Anchor {
    pub pos: V3,
    pub yaw: f64,
}
#[derive(Clone, Copy)]
pub(super) struct WindowSpec {
    pub bearing: f64,
    pub dist: f64,
    pub height: f64,
    pub size: [f64; 2],
}
#[derive(Clone, Copy)]
pub(super) struct Spec {
    pub bearing: f64,
    pub dist: f64,
    pub height: f64,
    pub panel: [f64; 2],
    pub gap: f64,
    pub window: Option<WindowSpec>,
}
struct Touch {
    x: f64,
    y: f64,
    tip: bool,
    depth: f64,
    trigger: f64,
    len: f64,
    origin: Option<V3>,
    hit: Option<V3>,
}
struct Down {
    p: PanelRef,
    h: Option<Hit>,
    x: f64,
    y: f64,
    x0: f64,
    y0: f64,
    slider: Option<String>,
    sliding: bool,
    dragging: bool,
    done: bool,
    sent: Option<f64>,
    sent_at: Option<f64>,
}
pub(super) struct At {
    p: PanelRef,
    x: f64,
    y: f64,
    pub len: f64,
    hot: bool,
}
#[derive(Default)]
pub(super) struct HandS {
    down: Option<Down>,
    over: Option<String>,
    pub at: Option<At>,
}
struct Preview {
    s: Surface,
    alpha: f64,
    alpha_target: f64,
    frame: Frame,
    built: bool,
    body: Handle,
    host: Handle,
    title: Handle,
    detail: Handle,
    painted_at: Option<f64>,
    ready: bool,
    shown_alpha: Option<f64>,
    img: Option<Handle>,
    row: Option<Rc<Row>>,
}
pub(super) struct Texts {
    pub back: String,
}
/// Where a board stands: bearing in degrees from the menu's heading, dist and height in metres.
#[derive(Clone, Copy)]
pub(super) struct Spot {
    pub bearing: f64,
    pub dist: f64,
    pub height: f64,
}
/// A board's face: drawn on its layer (canvas, cw, ch, palette).
pub(super) type BoardDraw = fn(&Handle, f64, f64, &Palette);
/// A screen of its own away from the arc, standing while the menu is open (`board`): nothing on it is pressed.
struct Board {
    s: Surface,
    frame: Frame,
    draw: BoardDraw,
    alpha: f64,
    shown_alpha: Option<f64>,
    built: bool,
    ready: bool,
    layer: Handle,
    language: Option<String>,
}
pub(super) struct Holo {
    time: f64,
    anchor: Option<Anchor>,
    spec: Option<Spec>,
    stack: Vec<PanelRef>,
    leaving: Vec<PanelRef>,
    pub hands: [HandS; 2],
    captured: bool,
    texts: Texts,
    preview: Option<Preview>,
    closing: Option<PanelRef>,
    pub last_error: Option<String>,
    beams: sp::Beams,
    board: Option<Board>,
}
impl Default for Holo {
    fn default() -> Holo {
        Holo {
            time: 0.0,
            anchor: None,
            spec: None,
            stack: Vec::new(),
            leaving: Vec::new(),
            hands: Default::default(),
            captured: false,
            texts: Texts { back: "BACK".into() },
            preview: None,
            closing: None,
            last_error: None,
            beams: Default::default(),
            board: None,
        }
    }
}

// ---------------------------------------------------------------- frames
fn place(a: Anchor, c: V3, r: V3, u: V3, w: f64, h: f64) -> Frame {
    let yaw = a.yaw.to_radians();
    let (cs, sn) = (yaw.cos(), yaw.sin());
    let dir = |v: V3| [cs * v[0] - sn * v[1], sn * v[0] + cs * v[1], v[2]];
    let p = dir(c);
    Frame { c: [a.pos[0] + p[0], a.pos[1] + p[1], a.pos[2] + p[2]], r: dir(r), u: dir(u), w, h }
}
/// A screen w x h m on the arc at bearing b (radians, from the player's heading), its centre at `height`, the arc's
/// own distance away.
fn frame_on(a: Anchor, spec: &Spec, b: f64, height: f64, w: f64, h: f64) -> Frame {
    frame_on_at(a, b, height, w, h, spec.dist)
}
/// The same `dist` m away.
fn frame_on_at(a: Anchor, b: f64, height: f64, w: f64, h: f64, dist: f64) -> Frame {
    let c = [dist * b.sin(), dist * b.cos(), height];
    let n = norm([-c[0], -c[1], EYE - c[2]]);
    let r = norm(cross([0.0, 0.0, 1.0], n));
    place(a, c, r, cross(n, r), w, h)
}
fn frame_at(a: Anchor, spec: &Spec, slot: f64) -> Frame {
    let step = ((spec.panel[0] + spec.gap) / spec.dist).to_degrees();
    frame_on(a, spec, (spec.bearing + slot * step).to_radians(), spec.height, spec.panel[0], spec.panel[1])
}
fn frame_preview(a: Anchor, spec: &Spec) -> Frame {
    let (w, h) = (0.52, 0.40);
    frame_on(a, spec, spec.bearing.to_radians() + (spec.panel[0] / 2.0 + spec.gap + w / 2.0) / spec.dist, spec.height + (spec.panel[1] - h) / 2.0, w, h)
}
fn frame_window(a: Anchor, w: &WindowSpec) -> Frame {
    let b = w.bearing.to_radians();
    place(a, [w.dist * b.sin(), w.dist * b.cos(), w.height], [b.cos(), -b.sin(), 0.0], [0.0, 0.0, 1.0], w.size[0], w.size[1])
}
fn lerp3(a: V3, b: V3, k: f64) -> V3 {
    [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
}
fn between(f0: &Frame, f1: &Frame, k: f64) -> Frame {
    let r = norm(lerp3(f0.r, f1.r, k));
    let u = lerp3(f0.u, f1.u, k);
    let d = sp::dot(u, r);
    let u = norm([u[0] - r[0] * d, u[1] - r[1] * d, u[2] - r[2] * d]);
    Frame { c: lerp3(f0.c, f1.c, k), r, u, w: f1.w, h: f1.h }
}
fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
fn approach(v: f64, target: f64, rate: f64, dt: f64) -> f64 {
    v + (target - v) * (1.0 - (-rate * dt).exp())
}
fn row_height(row: &Row) -> f64 {
    if let Some(h) = row.height {
        h
    } else if row.image.is_some() {
        IMAGE_ROW_H
    } else if row.icon.is_some() {
        ICON_ROW_H
    } else if row.cards.is_some() {
        if row.poster.is_some() {
            POSTER_ROW_H
        } else {
            CARDS_ROW_H
        }
    } else if row.picture.is_some() {
        PICTURE_ROW_H
    } else {
        ROW_H
    }
}

// ---------------------------------------------------------------- drawing a row
fn restyle(look: Option<&Look>, part: Option<&str>) {
    let Some(look) = look else { return };
    if let Some(cards) = &look.cards {
        for (i, cl) in cards.iter().enumerate() {
            let hot = part == Some(format!("card{}", i + 1).as_str());
            ink::tint(&cl.field, if hot { RED_DIM } else { GLASS });
            ink::opacity(&cl.field, if hot { 1.0 } else { 0.6 });
            for w in &cl.frame {
                ink::tint(w, if hot { RED } else if cl.on { CYAN } else { RED_DIM });
            }
            if cl.poster {
                ink::tint(&cl.picture, if hot || cl.on { WHITE } else { DIMMED });
            } else {
                ink::tint(&cl.picture, if hot { WHITE } else if cl.on { CYAN } else { TEXT });
            }
            ink::tint(&cl.label, if hot || cl.on { WHITE } else { TEXT });
        }
        return;
    }
    if !look.keys.is_empty() {
        for (i, (field, label)) in look.keys.iter().enumerate() {
            let hot = part == Some(format!("key{}", i + 1).as_str());
            ink::tint(field, if hot { RED } else { RED_DIM });
            ink::opacity(field, if hot { 1.0 } else { 0.7 });
            ink::tint(label, if hot { BLACK } else { WHITE });
        }
        return;
    }
    if look.header {
        let hot = part.is_some() && look.has_go;
        ink::tint(&look.label, if hot { WHITE } else { RED });
        if !look.detail.is_null() {
            ink::tint(&look.detail, if hot { WHITE } else { RED });
        }
        ink::tint(&look.line, if hot { RED } else { RED_DIM });
        return;
    }
    if look.button {
        let hot = part.is_some();
        ink::tint(&look.field, if hot { RED } else { RED_DIM });
        ink::opacity(&look.field, if hot { 1.0 } else { 0.8 });
        ink::tint(&look.label, if hot { BLACK } else { WHITE });
        return;
    }
    let hot = part.is_some() && look.live;
    ink::tint(&look.field, if hot { RED_DIM } else { GLASS });
    ink::opacity(&look.field, if hot { 1.0 } else { 0.6 });
    for w in &look.frame {
        ink::visible(w, hot || look.on);
    }
    ink::visible(&look.line, !(hot || look.on));
    let col = if !look.live {
        DIM
    } else if part.is_some() || look.on {
        WHITE
    } else {
        TEXT
    };
    if !look.label.is_null() {
        ink::tint(&look.label, col);
    }
    for w in &look.glyph {
        ink::tint(w, col);
    }
    if !look.sign.is_null() {
        ink::tint(&look.sign, if hot { WHITE } else { RED });
    }
    for (side, bx, sign) in &look.arrows {
        let on = part == Some(side.as_str());
        ink::tint(bx, if on { RED } else { RED_DIM });
        ink::opacity(bx, if on { 1.0 } else { 0.7 });
        ink::tint(sign, if on { BLACK } else { WHITE });
    }
    if !look.knob.is_null() {
        ink::tint(&look.knob, if part == Some("slider") && look.live { WHITE } else { TEXT });
    }
}
fn slider_value(sl: &Slider, frac: f64) -> f64 {
    let (lo, hi) = (sl.min.min(sl.max), sl.min.max(sl.max));
    let mut v = lo + frac.clamp(0.0, 1.0) * (hi - lo);
    if let Some(step) = sl.step.filter(|s| *s > 0.0) {
        v = lo + ((v - lo) / step + 0.5).floor() * step;
    }
    v.clamp(lo, hi)
}
fn show_slider(look: &Look, l: &Layout, sl: &Slider, v: f64) {
    let (lo, hi) = (sl.min.min(sl.max), sl.min.max(sl.max));
    let frac = if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
    ink::size(&look.fill, l.slider_w * frac, 14.0);
    ink::move_to(&look.knob, l.slider_x + l.slider_w * frac - 14.0, ROW_H / 2.0 - 32.0);
    ink::set_text(&look.value, &sl.format.as_ref().map_or_else(|| fmt_num(v), |f| f(v)));
}
/// Lua's tostring of a number: integers without the fraction.
pub(super) fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.14}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
fn hit(p: &mut Panel, x: f64, y: f64, w: f64, h: f64, row: &str, part: &str, list: bool) {
    p.hits.push(Hit { x, y, w, h, row: row.into(), part: part.into(), list });
}
fn draw_row(p: &mut Panel, c: &Handle, row: &Row, hit_y: f64) {
    let l = p.lay();
    let (row_x, row_w, slider_x, slider_w) = (l.row_x, l.row_w, l.slider_x, l.slider_w);
    let id = row.key.clone();
    let mut look = Look { key: id.clone(), has_go: row.go.is_some(), ..Default::default() };
    if row.header {
        look.header = true;
        look.label = ink::text(c, 20.0, ROW_H * 0.35, row_w - 160.0, ROW_H * 0.65, &row.label, 62.0, RED, "left", false, None);
        if let Some(d) = &row.detail {
            look.detail = ink::text(c, row_w - 140.0, ROW_H * 0.35, 120.0, ROW_H * 0.65, d, 72.0, RED, "right", true, None);
        }
        look.line = ink::rect(c, 20.0, ROW_H - 10.0, row_w - 40.0, 3.0, RED_DIM, 1.0);
        if row.go.is_some() {
            hit(p, row_x, hit_y, row_w, ROW_H, &id, "main", true);
        }
    } else if row.text {
        ink::text(c, 20.0, 0.0, row_w - 40.0, ROW_H, &row.label, 60.0, if row.warn { RED } else { TEXT }, "left", true, None);
        return;
    } else if let Some(draw) = &row.draw {
        // a row that draws itself (row.draw(canvas, width, height, PALETTE), row.height px tall): nothing to press
        let h = row_height(row);
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| draw(c, row_w, h, &PALETTE))).is_err() {
            cvr_host::log!("[VRLobby] row {} failed to draw\n", row.key);
        }
        return;
    } else if let Some((atlas, part)) = &row.picture {
        let s = PICTURE_ROW_H - 20.0;
        ink::image(c, (row_w - s) / 2.0, 10.0, s, s, atlas, part, TEXT, 1.0);
        return;
    } else if let Some(keys) = &row.keys {
        let (rh, gap) = (row_height(row), 16.0);
        let units: f64 = keys.iter().map(|k| k.units).sum::<f64>().max(1.0);
        let unit = (row_w - gap * (keys.len() as f64 - 1.0).max(0.0)) / units;
        let mut x = 0.0;
        for (i, k) in keys.iter().enumerate() {
            let w = (unit * k.units).floor();
            let field = ink::rect(c, x, 8.0, w, rh - 16.0, RED_DIM, 0.7);
            let label = ink::text(c, x, 0.0, w, rh, &k.label, ink::fit(&k.label, w - 24.0, 84.0, 40.0), WHITE, "centre", false, None);
            look.keys.push((field, label));
            hit(p, row_x + x, hit_y, w, rh, &id, &format!("key{}", i + 1), true);
            x += w + gap;
        }
    } else if let Some(cards) = &row.cards {
        let mut looks = Vec::new();
        let gap = 60.0;
        let rh = row_height(row);
        let n = cards.len() as f64;
        let cw = ((row_w - gap * (n - 1.0)) / n).floor();
        let pw = match row.poster {
            Some(pr) => (cw - 60.0).min(((rh - 220.0) * pr).floor()),
            None => (rh - 220.0).min(cw - 60.0),
        };
        let ph = row.poster.map_or(pw, |pr| (pw / pr).floor());
        for (i, card) in cards.iter().enumerate() {
            let x = i as f64 * (cw + gap);
            let tint = if row.poster.is_some() {
                if card.on {
                    WHITE
                } else {
                    DIMMED
                }
            } else {
                TEXT
            };
            looks.push(CardLook {
                on: card.on,
                poster: row.poster.is_some(),
                field: ink::rect(c, x, 0.0, cw, rh, GLASS, 0.6),
                frame: ink::cut_frame(c, x, 0.0, cw, rh, 40.0, RED_DIM, 1.0, 3.0),
                picture: ink::image(c, x + (cw - pw) / 2.0, 30.0, pw, ph, &card.picture.0, &card.picture.1, tint, 1.0),
                label: ink::text(c, x, rh - 180.0, cw, 150.0, &card.label, ink::fit(&card.label, cw - 60.0, 84.0, 50.0), TEXT, "centre", false, None),
            });
            hit(p, row_x + x, hit_y, cw, rh, &id, &format!("card{}", i + 1), true);
        }
        look.cards = Some(looks);
    } else {
        look.live = !row.disabled && (row.go.is_some() || row.left.is_some() || row.right.is_some() || row.slider.is_some());
        look.on = row.on;
        let rh = row_height(row);
        let ix = row.indent;
        let rw = row_w - ix;
        look.field = ink::rect(c, ix, 0.0, rw, rh, GLASS, 0.6);
        look.frame = ink::cut_frame(c, ix, 0.0, rw, rh, 28.0, if look.on { CYAN } else { RED }, 1.0, 3.0);
        look.line = ink::rect(c, ix, rh - 3.0, rw, 3.0, RED_DIM, 1.0);
        if row.image.is_some() || row.icon.is_some() {
            let (x, y, h) = (ix + 16.0, 14.0, rh - 28.0);
            let w = if row.image.is_some() { (h * 16.0 / 9.0).floor() } else { h };
            ink::rect(c, x, y, w, h, BLACK, 0.8);
            if let Some((atlas, part)) = &row.icon {
                ink::image(c, x + 10.0, y + 10.0, w - 20.0, h - 20.0, atlas, part, RED, 0.95);
            }
            if let Some(img) = &row.image {
                ink::adopt(img, c, x, y, w, h, PHOTO);
            }
            let tx = x + w + 40.0;
            let tw = row_w - tx - if row.sign.is_some() { 160.0 } else { 40.0 };
            look.label = ink::text(c, tx, 20.0, tw, rh * 0.45, &row.label, ink::fit(&row.label, tw, 76.0, 50.0), TEXT, "left", false, None);
            if let Some(d) = &row.detail {
                ink::text(c, tx, rh * 0.5, tw, rh * 0.4, d, 54.0, DIM, "left", true, None);
            }
            if let Some(sg) = &row.sign {
                look.sign = ink::text(c, row_w - 140.0, 0.0, 110.0, rh, sg, 96.0, RED, "centre", true, None);
            }
            if look.live {
                hit(p, row_x + ix, hit_y, rw, rh, &id, "main", true);
            }
        } else if let Some(sl) = &row.slider {
            look.slider = true;
            look.label = ink::text(c, 60.0, 0.0, slider_x - 100.0, ROW_H, &row.label, ink::fit(&row.label, slider_x - 100.0, 66.0, 46.0), TEXT, "left", false, None);
            ink::rect(c, slider_x, ROW_H / 2.0 - 7.0, slider_w, 14.0, RED_DIM, 1.0);
            look.fill = ink::rect(c, slider_x, ROW_H / 2.0 - 7.0, 0.0, 14.0, if look.live { RED } else { DIM }, 1.0);
            look.knob = ink::rect(c, slider_x - 14.0, ROW_H / 2.0 - 32.0, 28.0, 64.0, TEXT, if look.live { 1.0 } else { 0.4 });
            let vx = slider_x + slider_w + 40.0;
            look.value = ink::text(c, vx, 0.0, row_w - vx - 30.0, ROW_H, "", 64.0, WHITE, "centre", false, None);
            show_slider(&look, &l, sl, sl.value);
            hit(p, row_x + slider_x - 40.0, hit_y, slider_w + 80.0, ROW_H, &id, "slider", true);
        } else if row.left.is_some() || row.right.is_some() {
            let (aw, vw) = (150.0, 640.0);
            let vx = row_w - aw * 2.0 - vw;
            look.label = ink::text(c, 60.0, 0.0, vx - 80.0, ROW_H, &row.label, ink::fit(&row.label, vx - 80.0, 66.0, 46.0), TEXT, "left", false, None);
            for (i, side) in ["left", "right"].iter().enumerate() {
                let ax = if i == 0 { vx } else { vx + aw + vw };
                let bx = ink::rect(c, ax + 20.0, 22.0, aw - 40.0, ROW_H - 44.0, RED_DIM, 0.7);
                let sg = ink::text(c, ax, 0.0, aw, ROW_H, if i == 0 { "<" } else { ">" }, 96.0, WHITE, "centre", false, None);
                look.arrows.push(((*side).into(), bx, sg));
                hit(p, row_x + ax, hit_y, aw, ROW_H, &id, side, true);
            }
            let value = row.value.clone().unwrap_or_default();
            ink::text(c, vx + aw, 0.0, vw, ROW_H, &value, ink::fit(&value, vw - 30.0, 64.0, 40.0), if look.on { CYAN } else { WHITE }, "centre", false, None);
            hit(p, row_x, hit_y, vx, ROW_H, &id, "right", true);
        } else {
            look.label = ink::text(c, 60.0, 0.0, rw - 160.0, ROW_H, &row.label, 84.0, TEXT, "left", false, None);
            if let Some(d) = &row.detail {
                ink::text(c, 60.0, 0.0, rw - 120.0, ROW_H, d, 54.0, DIM, "right", true, None);
            }
            if look.live {
                hit(p, row_x, hit_y, rw, ROW_H, &id, "main", true);
            }
        }
    }
    let part = p.hover.as_ref().filter(|h| h.row == id).map(|h| h.part.clone());
    restyle(Some(&look), part.as_deref());
    p.looks.insert(id, look);
}
fn px_per_metre(p: &Panel) -> f64 {
    p.lay().cw / p.frame.w
}
fn build_mark(p: &mut Panel) {
    p.marks.clear();
    for _ in 0..2 {
        let host = ink::canvas(&p.top, 0.0, 0.0, 64.0, 64.0);
        ink::v2(&host, "SetRenderTransformPivot", 0.0, 0.0);
        ink::opacity(&host, 0.0);
        let inner = ink::ring(&host, 32.0, 32.0, 17.0, 2.0, MARK, 0.9, 28, None);
        let outer = ink::ring(&host, 32.0, 32.0, 26.0, 1.5, MARK, 0.4, 32, Some(4));
        ink::image(&host, 29.0, 29.0, 6.0, 6.0, PHONE_ATLAS, "circle", MARK_DOT, 1.0);
        p.marks.push(Mark { host, inner, outer, color: MARK, shown: false });
    }
    let fx = ink::canvas(&p.top, 0.0, 0.0, 96.0, 96.0);
    ink::v2(&fx, "SetRenderTransformPivot", 0.5, 0.5);
    ink::opacity(&fx, 0.0);
    ink::ring(&fx, 48.0, 48.0, 22.0, 3.0, MARK, 0.95, 32, None);
    ink::ring(&fx, 48.0, 48.0, 32.0, 2.0, MARK, 0.65, 36, None);
    ink::ring(&fx, 48.0, 48.0, 42.0, 1.0, MARK, 0.3, 40, None);
    p.ripple = Some(Ripple { host: fx, x: 0.0, y: 0.0, at: None });
}
fn show_mark(p: &mut Panel, hand: usize, at: Option<(f64, f64, bool)>) {
    let span = MARK_M * px_per_metre(p);
    let Some(m) = p.marks.get_mut(hand) else { return };
    let Some((x, y, hot)) = at else {
        if m.shown {
            ink::opacity(&m.host, 0.0);
            m.shown = false;
        }
        return;
    };
    ink::move_to(&m.host, x - span / 2.0, y - span / 2.0);
    ink::v2(&m.host, "SetScale", span / 64.0, span / 64.0);
    let col = if hot { MARK_HOT } else { MARK };
    if m.color != col {
        for w in &m.inner {
            ink::tint(w, col);
            ink::opacity(w, 0.95);
        }
        for w in &m.outer {
            ink::tint(w, col);
            ink::opacity(w, 0.45);
        }
        m.color = col;
    }
    if !m.shown {
        ink::opacity(&m.host, 1.0);
        m.shown = true;
    }
}
fn update_ripple(p: &mut Panel, time: f64) {
    let scale0 = MARK_M * px_per_metre(p) / 52.0;
    let Some(r) = p.ripple.as_mut() else { return };
    let Some(at) = r.at else { return };
    let t = (time - at) / RIPPLE;
    if t >= 1.0 {
        r.at = None;
        ink::opacity(&r.host, 0.0);
        return;
    }
    let scale = (0.65 + 0.6 * t) * scale0;
    ink::move_to(&r.host, r.x - 48.0, r.y - 48.0);
    ink::v2(&r.host, "SetScale", scale, scale);
    ink::opacity(&r.host, (1.0 - t).powi(2));
}
fn build_glass(layer: &Handle, l: &Layout) {
    let (x0, y0, w0, h0) = (PAD, PAD, l.cw - 2.0 * PAD, l.ch - 2.0 * PAD);
    let step = 2.0;
    let field = |x: f64, y: f64, w: f64, h: f64| {
        let cx = l.clear_x.unwrap_or(x + w);
        if cx > x {
            ink::rect(layer, x, y, w.min(cx - x), h, GLASS, 0.55);
        }
        if cx < x + w {
            let from = x.max(cx);
            ink::rect(layer, from, y, x + w - from, h, GLASS, CLEAR_GLASS);
        }
    };
    field(x0 + CUT, y0, w0 - CUT, CUT);
    field(x0, y0 + CUT, w0, h0 - 2.0 * CUT);
    field(x0, y0 + h0 - CUT, w0 - CUT, CUT);
    let mut d = 0.0;
    while d <= CUT - step {
        field(x0 + CUT - d - step, y0 + d, d + step, step);
        field(x0 + w0 - CUT, y0 + h0 - CUT + d, CUT - d, step);
        d += step;
    }
    ink::cut_frame(layer, x0, y0, w0, h0, CUT, RED, 1.0, 4.0);
    ink::cut_frame(layer, x0 + 16.0, y0 + 16.0, w0 - 32.0, h0 - 32.0, CUT - 7.0, RED_DIM, 1.0, 2.0);
    ink::rect(layer, x0 + CUT + 40.0, y0 - 5.0, 460.0, 12.0, RED, 1.0);
    ink::rect(layer, x0 + w0 - CUT - 500.0, y0 + h0 - 7.0, 460.0, 12.0, RED, 1.0);
}
/// build: the page drawn into the panel -- the head (the logo or the title), the rows in the scroll view, the back
/// button below a pushed page, the footer's glyph buttons.
fn build(l: &mut Lobby, pr: &PanelRef) {
    let page = pr.borrow().page();
    let rows: Vec<Rc<Row>> = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (page.rows)(l))) {
        Ok(r) => r.into_iter().map(Rc::new).collect(),
        Err(_) => {
            l.holo.last_error = Some("rows failed".into());
            Vec::new()
        }
    };
    let footer = page.footer.as_ref().map(|f| f(l)).unwrap_or_default();
    let heading = page.heading_now();
    let back = heading.back.clone().unwrap_or_else(|| l.holo.texts.back.clone());
    let mut p = pr.borrow_mut();
    let lay = p.lay();
    if p.layer.is_null() {
        p.glass = ink::canvas(&p.s.root, 0.0, 0.0, lay.cw, lay.ch);
        build_glass(&p.glass, &lay);
        p.layer = ink::canvas(&p.s.root, 0.0, 0.0, lay.cw, lay.ch);
        p.top = ink::canvas(&p.s.root, 0.0, 0.0, lay.cw, lay.ch);
        build_mark(&mut p);
    }
    let layer = p.layer.clone();
    p.builds += 1;
    ink::remove_all(&layer);
    p.hits.clear();
    p.looks.clear();
    if page.logo {
        let part = match ink::language().as_str() {
            "kr-kr" => "cp_logo_kr",
            "zh-cn" => "cp_logo_zhs",
            "zh-tw" => "cp_logo_zht",
            _ => "cp_logo",
        };
        ink::image(&layer, lay.row_x - 10.0, HEAD_Y + 30.0, LOGO_W, LOGO_H, LOGO_ATLAS, part, RED, 1.0);
    } else {
        ink::text(&layer, lay.row_x, HEAD_Y + 30.0, lay.row_w, 150.0, heading.title.as_deref().unwrap_or(""), 118.0, WHITE, "left", false, None);
        if let Some(st) = &heading.subtitle {
            ink::text(&layer, lay.row_x, HEAD_Y + 150.0, lay.row_w, 90.0, st, 58.0, TEXT, "left", true, None);
        }
    }
    ink::text(&layer, lay.row_x, HEAD_Y + 30.0, lay.row_w, 70.0, heading.code.as_deref().unwrap_or("VR//LOBBY"), 40.0, DIM, "right", false, None);
    ink::rect(&layer, lay.row_x, HEAD_Y + HEAD_H + 10.0, lay.row_w, 4.0, RED, 0.8);
    let same = p.built_page.as_ref().is_some_and(|b| Rc::ptr_eq(b, &page));
    let keep = match (&p.view, same) {
        (Some(v), true) => v.offset,
        _ => page.scroll.get(),
    };
    p.built_page = Some(page.clone());
    let mut v = View::new(&layer, lay.row_x, lay.row_top, lay.row_x, lay.row_top, lay.list_w, lay.list_h, Style { track: RED_DIM, thumb: RED, held: WHITE });
    for row in &rows {
        v.add(row_height(row) + ROW_GAP);
    }
    v.scroll_to(keep);
    {
        let pp: &mut Panel = &mut p;
        v.update(0.0, rows.len(), &mut |i, c, y| draw_row(pp, c, &rows[i], y));
    }
    p.view = Some(v);
    if p.index > 1 {
        let mut look = Look { key: "__back".into(), button: true, ..Default::default() };
        look.field = ink::rect(&layer, lay.row_x, lay.foot_y, 620.0, FOOT_H, RED_DIM, 0.8);
        look.label = ink::text(&layer, lay.row_x, lay.foot_y, 620.0, FOOT_H, &format!("< {back}"), 72.0, WHITE, "centre", false, None);
        hit(&mut p, lay.row_x, lay.foot_y, 620.0, FOOT_H, "__back", "main", false);
        let part = p.hover.as_ref().filter(|h| h.row == "__back").map(|_| "main");
        restyle(Some(&look), part);
        p.looks.insert("__back".into(), look);
    }
    p.footer.clear();
    let mut fx = lay.row_x + lay.row_w;
    for b in footer {
        let key = format!("__foot:{}", b.key);
        fx -= FOOT_BUTTON_W;
        let mut look = Look { key: key.clone(), live: true, ..Default::default() };
        look.field = ink::rect(&layer, fx, lay.foot_y, FOOT_BUTTON_W, FOOT_H, GLASS, 0.6);
        look.frame = ink::cut_frame(&layer, fx, lay.foot_y, FOOT_BUTTON_W, FOOT_H, 28.0, RED, 1.0, 3.0);
        look.line = ink::rect(&layer, fx, lay.foot_y + FOOT_H - 3.0, FOOT_BUTTON_W, 3.0, RED_DIM, 1.0);
        look.glyph = (b.glyph)(&layer, fx + (FOOT_BUTTON_W - FOOT_ICON_H) / 2.0, lay.foot_y + (FOOT_H - FOOT_ICON_H) / 2.0, FOOT_ICON_H, TEXT);
        hit(&mut p, fx, lay.foot_y, FOOT_BUTTON_W, FOOT_H, &key, "main", false);
        let part = p.hover.as_ref().filter(|h| h.row == key).map(|_| "main");
        restyle(Some(&look), part);
        p.looks.insert(key.clone(), look);
        p.footer.insert(key, b);
        fx -= 30.0;
    }
    if p.builds == 1 {
        static REPORTED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if REPORTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 3 {
            let labels: Vec<&str> = rows.iter().take(6).map(|r| r.label.as_str()).collect();
            cvr_host::log!("[VRLobby] page {} built from Rust: {} rows {:?}\n", heading.code.as_deref().unwrap_or("-"), rows.len(), labels);
        }
    }
    p.rows_cache = rows;
    p.dirty = false;
}
fn hit_at(p: &Panel, x: f64, y: f64) -> Option<Hit> {
    let cy = match &p.view {
        Some(v) => v.visible_at(x, y).then(|| y - v.y + v.offset),
        None => {
            let l = p.lay();
            (y >= l.row_top && y <= l.row_top + l.list_h).then(|| y - l.row_top)
        }
    };
    for h in p.hits.iter().rev() {
        let hy = if h.list { cy } else { Some(y) };
        if let Some(hy) = hy {
            if x >= h.x && x <= h.x + h.w && hy >= h.y && hy <= h.y + h.h {
                return Some(h.clone());
            }
        }
    }
    None
}
fn row_by_key(p: &Panel, key: &str) -> Option<Rc<Row>> {
    p.rows_cache.iter().find(|r| r.key == key).cloned()
}

// ---------------------------------------------------------------- spawning and placing the quads
fn spawn_panel(h: &Holo, page: PageRef, slot: f64) -> PanelRef {
    let (a, spec) = (h.anchor.unwrap_or(Anchor { pos: [0.0; 3], yaw: 0.0 }), h.spec.unwrap_or_else(default_spec));
    let frame = frame_at(a, &spec, slot);
    let id = sp::spawn(ENT, frame.c, quad_pose(frame.r, frame.u), TAG);
    Rc::new(RefCell::new(Panel { page: Some(page), slot, alpha: 0.0, dirty: true, l: Some(panel_layout()), frame, s: Surface { id, ..Default::default() }, ..Default::default() }))
}
fn default_spec() -> Spec {
    Spec { bearing: 0.0, dist: 1.1, height: 1.55, panel: [0.98, 0.75], gap: 0.08, window: None }
}
fn ready(p: &mut Panel) -> bool {
    if p.ready {
        return true;
    }
    if !p.s.painted() {
        return false;
    }
    p.ready = p.s.display_on();
    p.ready
}
fn retarget(h: &mut Holo) {
    let n = h.stack.len();
    let window = h.stack.last().is_some_and(|p| p.borrow().window);
    let panels = if window { n - 1 } else { n };
    let hidden = window && h.stack.last().is_some_and(|p| p.borrow().ready);
    for (i, pr) in h.stack.iter().enumerate() {
        let i = i + 1;
        let mut p = pr.borrow_mut();
        p.index = i;
        if p.window {
            (p.target, p.alpha_target) = (0.0, 1.0);
        } else {
            p.target = i as f64 - panels as f64;
            p.alpha_target = if !hidden && (i == panels || i + 1 == panels) { 1.0 } else { 0.0 };
        }
        p.dirty = true;
    }
}
fn animate(h: &Holo, p: &mut Panel, dt: f64) {
    let mut moved = false;
    if (p.slot - p.target).abs() > 0.001 {
        p.slot = approach(p.slot, p.target, SLIDE, dt);
        if (p.slot - p.target).abs() < 0.002 {
            p.slot = p.target;
        }
        moved = true;
    }
    p.alpha = approach(p.alpha, p.alpha_target, FADE, dt);
    if moved || !p.placed {
        if let (Some(a), Some(spec)) = (h.anchor, h.spec) {
            p.frame = frame_at(a, &spec, p.slot);
        }
        if !p.s.entity.is_null() {
            sp::set_world(&p.s.entity, p.frame.c, quad_pose(p.frame.r, p.frame.u));
            p.placed = true;
        }
    }
    if !p.s.root.is_null() && p.shown_alpha != Some(p.alpha) {
        ink::opacity(&p.s.root, p.alpha);
        p.shown_alpha = Some(p.alpha);
    }
}
// ---------------------------------------------------------------- a board standing in the hall
/// spawnBoard: a panel's screen at `spot`, turned to the eyes, its face drawn by `draw` over the panels' glass; it
/// fades in once drawn.
fn spawn_board(h: &Holo, spot: Spot, draw: BoardDraw) -> Board {
    let (a, spec) = (h.anchor.unwrap_or(Anchor { pos: [0.0; 3], yaw: 0.0 }), h.spec.unwrap_or_else(default_spec));
    let frame = frame_on_at(a, spot.bearing.to_radians(), spot.height, spec.panel[0], spec.panel[1], spot.dist);
    let id = sp::spawn(ENT, frame.c, quad_pose(frame.r, frame.u), TAG);
    Board { s: Surface { id, ..Default::default() }, frame, draw, alpha: 0.0, shown_alpha: None, built: false, ready: false, layer: Handle::null(), language: None }
}
fn board_ready(b: &mut Board) -> bool {
    if b.ready {
        return true;
    }
    if !b.s.painted() {
        return false;
    }
    b.ready = b.s.display_on();
    b.ready
}
/// updateBoard: placed and its glass built once resolved, its face drawn again in a new language, faded in once ready.
fn update_board(l: &mut Lobby, dt: f64) {
    let Some(b) = l.holo.board.as_mut() else { return };
    if !b.s.resolve(UI) {
        return;
    }
    let lay = panel_layout();
    if !b.built {
        sp::set_world(&b.s.entity, b.frame.c, quad_pose(b.frame.r, b.frame.u));
        // The display mesh is hidden until board_ready sees a completed ink draw: the canvas stays paintable while
        // that first draw is pending.
        ink::opacity(&b.s.root, 1.0);
        let glass = ink::canvas(&b.s.root, 0.0, 0.0, lay.cw, lay.ch);
        build_glass(&glass, &lay);
        b.layer = ink::canvas(&b.s.root, 0.0, 0.0, lay.cw, lay.ch);
        b.built = true;
    }
    let language = ink::language();
    let mut refresh = false;
    if b.language.as_deref() != Some(language.as_str()) {
        if b.language.is_some() {
            ink::remove_all(&b.layer);
            // the Controls entry can still be visible behind the language settings
            refresh = true;
        }
        b.language = Some(language);
        let (draw, layer) = (b.draw, b.layer.clone());
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| draw(&layer, lay.cw, lay.ch, &PALETTE))).is_err() {
            l.holo.last_error = Some("the board failed to draw".into());
        }
    }
    if let Some(b) = l.holo.board.as_mut() {
        let was_ready = b.ready;
        if board_ready(b) {
            if !was_ready {
                cvr_host::log!("[VRLobby] the controls board drawn from Rust ({})\n", b.language.as_deref().unwrap_or("-"));
            }
            b.alpha = approach(b.alpha, 1.0, FADE, dt);
            if b.shown_alpha != Some(b.alpha) {
                ink::opacity(&b.s.root, b.alpha);
                b.shown_alpha = Some(b.alpha);
            }
        }
    }
    if refresh {
        dirty_all(&l.holo);
    }
}
/// The board stands drawn and shown.
pub(super) fn board_shown(l: &Lobby) -> bool {
    l.holo.board.as_ref().is_some_and(|b| b.ready)
}
/// M.board: a board standing in the hall while the menu is open, after `open`; a board already up is taken down, and
/// it closes with the menu.
pub(super) fn board(l: &mut Lobby, spot: Spot, draw: BoardDraw) {
    if l.holo.anchor.is_none() {
        return;
    }
    if let Some(old) = l.holo.board.take() {
        sp::delete(old.s.id);
    }
    l.holo.board = Some(spawn_board(&l.holo, spot, draw));
}
fn spawn_preview(h: &Holo) -> Preview {
    let (a, spec) = (h.anchor.unwrap_or(Anchor { pos: [0.0; 3], yaw: 0.0 }), h.spec.unwrap_or_else(default_spec));
    let frame = frame_preview(a, &spec);
    let id = sp::spawn(PREVIEW_ENT, frame.c, quad_pose(frame.r, frame.u), TAG);
    Preview {
        s: Surface { id, ..Default::default() },
        alpha: 0.0,
        alpha_target: 0.0,
        frame,
        built: false,
        body: Handle::null(),
        host: Handle::null(),
        title: Handle::null(),
        detail: Handle::null(),
        painted_at: None,
        ready: false,
        shown_alpha: None,
        img: None,
        row: None,
    }
}
fn build_preview(pv: &mut Preview) {
    let body = ink::canvas(&pv.s.root, 0.0, 0.0, CW, CH);
    build_glass(&ink::canvas(&body, 0.0, 0.0, CW, CH), &panel_layout());
    let ph = pv_h();
    ink::rect(&body, PV_X, PV_Y, PV_W, ph, BLACK, 0.8);
    pv.host = ink::canvas(&body, PV_X + 4.0, PV_Y + 4.0, PV_W - 8.0, ph - 8.0);
    ink::cut_frame(&body, PV_X, PV_Y, PV_W, ph, 48.0, RED, 1.0, 4.0);
    pv.title = ink::text(&body, PV_X, PV_Y + ph + 50.0, PV_W, 190.0, "", 140.0, WHITE, "left", false, None);
    pv.detail = ink::text(&body, PV_X, PV_Y + ph + 250.0, PV_W, 140.0, "", 100.0, TEXT, "left", true, None);
    pv.body = body;
    pv.built = true;
}
fn preview_ready(pv: &mut Preview, time: f64) -> bool {
    if pv.ready {
        return true;
    }
    let Some(at) = pv.painted_at else {
        if !pv.s.painted() {
            return false;
        }
        pv.painted_at = Some(time);
        ink::opacity(&pv.body, 0.0);
        pv.shown_alpha = Some(0.0);
        return false;
    };
    if time - at < PV_SETTLE {
        return false;
    }
    pv.ready = pv.s.display_on();
    pv.ready
}
fn fill_preview(pv: &mut Preview, row: &Rc<Row>) {
    if let Some(img) = &row.preview {
        if pv.img.as_ref().is_none_or(|i| i.instance != img.instance) {
            ink::remove_all(&pv.host);
            ink::adopt(img, &pv.host, 0.0, 0.0, PV_W - 8.0, pv_h() - 8.0, PHOTO);
            pv.img = Some(img.clone());
        }
    }
    ink::set_text(&pv.title, row.preview_text.as_deref().unwrap_or(""));
    ink::set_text(&pv.detail, row.preview_detail.as_deref().unwrap_or(""));
    pv.row = Some(row.clone());
}
fn update_preview(h: &mut Holo, dt: f64) {
    let time = h.time;
    let row = h.stack.last().and_then(|p| {
        let p = p.borrow();
        p.hover.as_ref().and_then(|hv| row_by_key(&p, &hv.row))
    });
    let row = row.filter(|r| r.preview.is_some());
    let Some(pv) = h.preview.as_mut() else { return };
    if !pv.s.resolve(UI) {
        return;
    }
    if !pv.built {
        build_preview(pv);
    }
    if !pv.built || !preview_ready(pv, time) {
        return;
    }
    if let Some(r) = &row {
        if pv.row.as_ref().is_none_or(|old| !Rc::ptr_eq(old, r)) {
            fill_preview(pv, r);
        }
    }
    pv.alpha_target = if row.is_some() { 1.0 } else { 0.0 };
    pv.alpha = approach(pv.alpha, pv.alpha_target, FADE, dt);
    if pv.alpha < 0.01 && pv.alpha_target == 0.0 {
        pv.alpha = 0.0;
    }
    if pv.shown_alpha != Some(pv.alpha) {
        ink::opacity(&pv.body, pv.alpha);
        pv.shown_alpha = Some(pv.alpha);
    }
}
fn glass_scale(w: &Panel, k: f64) -> (f64, f64) {
    let l = w.lay();
    let pl = panel_layout();
    let sx = (w.panel_w * (pl.cw - 2.0 * PAD) / pl.cw) / (w.frame.w * (l.cw - 2.0 * PAD) / l.cw);
    let sy = (w.panel_h * (pl.ch - 2.0 * PAD) / pl.ch) / (w.frame.h * (l.ch - 2.0 * PAD) / l.ch);
    (sx + (1.0 - sx) * k, sy + (1.0 - sy) * k)
}
fn spawn_window(h: &Holo, page: PageRef, from: Frame) -> PanelRef {
    let a = h.anchor.unwrap_or(Anchor { pos: [0.0; 3], yaw: 0.0 });
    let ws = h.spec.and_then(|s| s.window).unwrap_or(WindowSpec { bearing: -10.0, dist: 1.5, height: 1.1, size: [2.2, 2.0] });
    let to = frame_window(a, &ws);
    let fr = Frame { c: from.c, r: from.r, u: from.u, w: to.w, h: to.h };
    let id = sp::spawn(WINDOW_ENT, fr.c, quad_pose(fr.r, fr.u), TAG);
    Rc::new(RefCell::new(Panel {
        window: true,
        l: Some(window_layout()),
        pages: vec![page.clone()],
        page: Some(page),
        alpha: 1.0,
        alpha_target: 1.0,
        dirty: true,
        opening: true,
        panel_w: from.w,
        panel_h: from.h,
        to,
        from: fr,
        frame: fr,
        s: Surface { id, ..Default::default() },
        ..Default::default()
    }))
}
fn window_ready(h: &mut Holo, pr: &PanelRef) -> bool {
    let time = h.time;
    let ready_now = {
        let mut w = pr.borrow_mut();
        if w.ready {
            return true;
        }
        let Some(at) = w.painted_at else {
            if !w.s.painted() {
                return false;
            }
            w.painted_at = Some(time);
            ink::opacity(&w.s.root, 0.0);
            return false;
        };
        if time - at < WINDOW_SETTLE {
            return false;
        }
        w.ready = w.s.display_on();
        w.ready
    };
    if ready_now && h.stack.last().is_some_and(|t| Rc::ptr_eq(t, pr)) {
        retarget(h);
    }
    ready_now
}
fn animate_window(w: &mut Panel, dt: f64) {
    if w.ready {
        w.grow = (w.grow + (if w.opening { dt } else { -dt }) / GROW).clamp(0.0, 1.0);
    }
    let k = smooth(w.grow);
    if w.placed_at != Some(w.grow) && !w.s.entity.is_null() {
        w.frame = between(&w.from, &w.to, k);
        sp::set_world(&w.s.entity, w.frame.c, quad_pose(w.frame.r, w.frame.u));
        w.placed_at = Some(w.grow);
    }
    if !w.glass.is_null() && w.scaled_at != Some(w.grow) {
        let (sx, sy) = glass_scale(w, k);
        ink::v2(&w.glass, "SetRenderTransformPivot", 0.5, 0.5);
        ink::v2(&w.glass, "SetScale", sx, sy);
        w.scaled_at = Some(w.grow);
    }
    if w.ready && !w.s.root.is_null() && w.shown_at != Some(w.grow) {
        ink::opacity(&w.s.root, smooth(w.grow / 0.25));
        ink::opacity(&w.layer, smooth((w.grow - 0.7) / 0.3));
        w.shown_at = Some(w.grow);
    }
}

// ---------------------------------------------------------------- pointing, pressing, dragging
fn touches(p: &Panel) -> [Option<Touch>; 2] {
    let f = &p.frame;
    let l = p.lay();
    let vs = [[f.c[0] as f32, f.c[1] as f32, f.c[2] as f32, 1.0], [f.r[0] as f32, f.r[1] as f32, f.r[2] as f32, 0.0], [f.u[0] as f32, f.u[1] as f32, f.u[2] as f32, 0.0], [f.w as f32, f.h as f32, 0.0, 0.0]];
    let hits = sp::native_v4s("VRMenuPointer", &[Value::Ptr(vs[0].as_ptr().cast()), Value::Ptr(vs[1].as_ptr().cast()), Value::Ptr(vs[2].as_ptr().cast()), Value::Ptr(vs[3].as_ptr().cast())]);
    let mut out: [Option<Touch>; 2] = [None, None];
    if hits.len() < 8 {
        return out;
    }
    for hand in 0..2 {
        let (tip, ray, origin, at) = (hits[4 * hand], hits[4 * hand + 1], hits[4 * hand + 2], hits[4 * hand + 3]);
        let on = |p: [f32; 4]| p[0] >= 0.0 && p[0] <= 1.0 && p[1] >= 0.0 && p[1] <= 1.0;
        let d = |v: [f32; 4]| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])];
        if tip[3] > 0.5 && on(tip) && f64::from(tip[2]) > -0.03 && f64::from(tip[2]) < HOVER_TIP {
            out[hand] = Some(Touch { x: f64::from(tip[0]) * l.cw, y: f64::from(tip[1]) * l.ch, tip: true, depth: f64::from(tip[2]), trigger: 0.0, len: 0.0, origin: None, hit: None });
        } else if ray[3] > 0.5 && on(ray) {
            out[hand] = Some(Touch {
                x: f64::from(ray[0]) * l.cw,
                y: f64::from(ray[1]) * l.ch,
                tip: false,
                depth: 0.0,
                trigger: f64::from(origin[3]),
                len: f64::from(ray[2]),
                origin: Some(d(origin)),
                hit: Some(d(at)),
            });
        }
    }
    out
}
fn dirty_all(h: &Holo) {
    for q in &h.stack {
        q.borrow_mut().dirty = true;
    }
}
fn press(l: &mut Lobby, pr: &PanelRef, hh: Option<Hit>, hand: usize) {
    let Some(hh) = hh else { return };
    sp::haptic(hand + 1, HAPTIC_CLICK);
    l.sound.menu("click");
    let index = pr.borrow().index;
    if index != l.holo.stack.len() {
        back_to(l, index);
        return;
    }
    if hh.row == "__back" {
        back(l);
        return;
    }
    let button = pr.borrow().footer.get(&hh.row).and_then(|b| b.go.clone());
    if pr.borrow().footer.contains_key(&hh.row) {
        if let Some(go) = button {
            go(l, hand + 1);
        }
        dirty_all(&l.holo);
        return;
    }
    let Some(row) = row_by_key(&pr.borrow(), &hh.row) else { return };
    if row.disabled {
        return;
    }
    let card = hh.part.strip_prefix("card").and_then(|n| n.parse::<usize>().ok()).and_then(|n| row.cards.as_ref().and_then(|c| c.get(n.wrapping_sub(1))));
    let key = hh.part.strip_prefix("key").and_then(|n| n.parse::<usize>().ok()).and_then(|n| row.keys.as_ref().and_then(|k| k.get(n.wrapping_sub(1))));
    let f = card
        .and_then(|c| c.go.clone())
        .or_else(|| key.map(|k| k.go.clone()))
        .or_else(|| if hh.part == "left" { row.left.clone() } else { None })
        .or_else(|| if hh.part == "right" { row.right.clone().or_else(|| row.go.clone()) } else { None })
        .or_else(|| row.go.clone());
    if let Some(f) = f {
        f(l, hand + 1);
    }
    dirty_all(&l.holo);
}
fn slide(l: &mut Lobby, hand: usize, d: &mut Down, x: f64, fin: bool) {
    let Some(key) = d.slider.clone() else { return };
    let row = row_by_key(&d.p.borrow(), &key);
    let Some(row) = row else { return };
    let Some(sl) = row.slider.as_ref() else { return };
    let lay = d.p.borrow().lay();
    let v = slider_value(sl, (x - lay.row_x - lay.slider_x) / lay.slider_w);
    if let Some(look) = d.p.borrow().looks.get(&key) {
        show_slider(look, &lay, sl, v);
    }
    let time = l.holo.time;
    if (Some(v) != d.sent && time - d.sent_at.unwrap_or(-1.0) >= 0.1) || (fin && Some(v) != d.sent) {
        (d.sent, d.sent_at) = (Some(v), Some(time));
        let set = sl.set.clone();
        set(l, v);
    }
    if fin {
        sp::haptic(hand + 1, HAPTIC_CLICK);
        dirty_all(&l.holo);
    }
}
fn ripple(p: &mut Panel, x: f64, y: f64, time: f64) {
    if let Some(r) = p.ripple.as_mut() {
        (r.x, r.y, r.at) = (x, y, Some(time));
    }
}
fn release(l: &mut Lobby, hand: usize, pressed: bool) {
    let Some(mut d) = l.holo.hands[hand].down.take() else { return };
    if d.done {
        return;
    }
    if d.dragging {
        if let Some(v) = d.p.borrow_mut().view.as_mut() {
            v.end_drag();
        }
    } else if d.slider.is_some() {
        if d.sliding || pressed {
            let x = if d.sliding { d.x } else { d.x0 };
            slide(l, hand, &mut d, x, true);
        }
    } else if pressed && d.h.is_some() {
        let time = l.holo.time;
        ripple(&mut d.p.borrow_mut(), d.x, d.y, time);
        press(l, &d.p, d.h.clone(), hand);
    }
}
fn held(h: &Holo, pr: &PanelRef) -> bool {
    h.hands.iter().any(|hs| hs.down.as_ref().is_some_and(|d| Rc::ptr_eq(&d.p, pr) && (d.dragging || d.sliding)))
}
fn gesture(l: &mut Lobby, hand: usize, pr: &PanelRef, t: &Touch, hh: Option<Hit>) {
    let has_down = l.holo.hands[hand].down.is_some();
    let engaged = if t.tip { t.depth <= TOUCH } else { (has_down && t.trigger > LET_GO) || t.trigger >= PULL };
    if !engaged {
        if has_down {
            release(l, hand, true);
        }
        return;
    }
    let time = l.holo.time;
    let Some(mut d) = l.holo.hands[hand].down.take() else {
        let (in_list, look) = {
            let p = pr.borrow();
            let in_list = p.view.as_ref().is_some_and(|v| v.scrollable() && v.contains(t.x, t.y));
            let look = hh.as_ref().filter(|h| h.part == "slider").and_then(|h| p.looks.get(&h.row).filter(|lk| lk.live).map(|_| h.row.clone()));
            (in_list, look)
        };
        let mk = |slider: Option<String>, sliding: bool| Down { p: pr.clone(), h: hh.clone(), x: t.x, y: t.y, x0: t.x, y0: t.y, slider, sliding, dragging: false, done: false, sent: None, sent_at: None };
        if let Some(key) = look {
            let mut d = mk(Some(key), !in_list);
            if !in_list {
                slide(l, hand, &mut d, t.x, false);
            }
            l.holo.hands[hand].down = Some(d);
        } else if in_list {
            l.holo.hands[hand].down = Some(mk(None, false));
        } else {
            if hh.is_some() {
                ripple(&mut pr.borrow_mut(), t.x, t.y, time);
                press(l, pr, hh.clone(), hand);
            }
            let mut d = mk(None, false);
            d.done = true;
            l.holo.hands[hand].down = Some(d);
        }
        return;
    };
    if d.done || !Rc::ptr_eq(&d.p, pr) {
        l.holo.hands[hand].down = Some(d);
        return;
    }
    (d.x, d.y) = (t.x, t.y);
    let (dx, dy) = ((t.x - d.x0).abs(), (t.y - d.y0).abs());
    if d.sliding {
        slide(l, hand, &mut d, t.x, false);
    } else if d.dragging {
        if let Some(v) = pr.borrow_mut().view.as_mut() {
            v.drag_to(t.y, time);
        }
    } else if d.slider.is_some() && dx > DRAG && dx >= dy {
        d.sliding = true;
        slide(l, hand, &mut d, t.x, false);
    } else if dy > DRAG && pr.borrow().view.is_some() {
        d.dragging = true;
        if let Some(v) = pr.borrow_mut().view.as_mut() {
            v.begin_drag(d.y0);
            v.drag_to(t.y, time);
        }
    }
    l.holo.hands[hand].down = Some(d);
}
fn pointers(l: &mut Lobby) {
    let mut seen: [Option<(PanelRef, Touch)>; 2] = [None, None];
    let stack = l.holo.stack.clone();
    for pr in &stack {
        let (ok, ts, slot) = {
            let p = pr.borrow();
            (p.ready && p.alpha > 0.4 && (!p.window || p.grow >= 1.0), if p.ready && p.alpha > 0.4 && (!p.window || p.grow >= 1.0) { touches(&p) } else { [None, None] }, p.slot)
        };
        if !ok {
            continue;
        }
        for (hand, t) in ts.into_iter().enumerate() {
            let Some(t) = t else { continue };
            let better = match &seen[hand] {
                None => true,
                Some((bp, bt)) => (t.tip && !bt.tip) || (t.tip == bt.tip && slot.abs() < bp.borrow().slot.abs()),
            };
            if better {
                seen[hand] = Some((pr.clone(), t));
            }
        }
    }
    let mut hovers: Vec<(PanelRef, Option<Hit>)> = Vec::new();
    for hand in 0..2 {
        if let Some((pr, t)) = &seen[hand] {
            let mut hh = hit_at(&pr.borrow(), t.x, t.y);
            gesture(l, hand, pr, t, hh.clone());
            if l.holo.hands[hand].down.as_ref().is_some_and(|d| d.dragging) {
                hh = None;
            }
            match hovers.iter_mut().find(|(p, _)| Rc::ptr_eq(p, pr)) {
                Some((_, v)) => {
                    if v.is_none() {
                        *v = hh.clone();
                    }
                }
                None => hovers.push((pr.clone(), hh.clone())),
            }
            let over = hh.as_ref().map(|h| format!("{}/{}", h.row, h.part));
            if over.is_some() && l.holo.hands[hand].over != over {
                sp::haptic(hand + 1, HAPTIC_HOVER);
                l.sound.menu("hover");
            }
            l.holo.hands[hand].over = over;
            l.holo.hands[hand].at = Some(At { p: pr.clone(), x: t.x, y: t.y, len: t.len, hot: hh.is_some() });
        } else {
            if l.holo.hands[hand].down.is_some() {
                release(l, hand, false);
            }
            (l.holo.hands[hand].over, l.holo.hands[hand].at) = (None, None);
        }
    }
    capture(&mut l.holo, seen[0].is_some() || seen[1].is_some());
    let eye = sp::native::<[f32; 4]>("VRViewWorldPos", &[]).map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]);
    for hand in 0..2 {
        let t = seen[hand].as_ref().map(|s| &s.1);
        let show = eye.is_some() && t.is_some_and(|t| !t.tip);
        l.holo.beams.set(hand + 1, show, t.and_then(|t| t.origin), t.and_then(|t| t.hit), eye);
    }
    let stack = l.holo.stack.clone();
    for pr in &stack {
        let hh = hovers.iter().find(|(p, _)| Rc::ptr_eq(p, pr)).and_then(|(_, h)| h.clone());
        let mut p = pr.borrow_mut();
        let was = p.hover.clone();
        let changed = match (&hh, &was) {
            (None, None) => false,
            (Some(a), Some(b)) => a.row != b.row || a.part != b.part,
            _ => true,
        };
        if changed {
            p.hover = hh.clone();
            if let Some(w) = &was {
                restyle(p.looks.get(&w.row), None);
            }
            if let Some(n) = &hh {
                restyle(p.looks.get(&n.row), Some(&n.part));
            }
        }
    }
    let time = l.holo.time;
    for pr in &stack {
        for hand in 0..2 {
            let at = l.holo.hands[hand].at.as_ref().filter(|a| Rc::ptr_eq(&a.p, pr)).map(|a| (a.x, a.y, a.hot));
            show_mark(&mut pr.borrow_mut(), hand, at);
        }
        update_ripple(&mut pr.borrow_mut(), time);
    }
}
fn capture(h: &mut Holo, on: bool) {
    if h.captured == on {
        return;
    }
    if sp::native::<bool>("VRMenuCapture", &[Value::Bool(on)]).is_some() {
        h.captured = on;
    }
}

// ---------------------------------------------------------------- the module's API
const BEAM: sp::BeamLook = sp::BeamLook { glow: RED, glow_op: 0.16, core: TEXT, core_op: 0.5 };
pub(super) fn open(l: &mut Lobby, anchor: Option<Anchor>, spec: Option<Spec>, page: PageRef, back_text: Option<String>) {
    close(l);
    let (Some(a), Some(sp_)) = (anchor, spec) else { return };
    (l.holo.anchor, l.holo.spec) = (Some(a), Some(sp_));
    if let Some(b) = back_text {
        l.holo.texts.back = b;
    }
    let first = spawn_panel(&l.holo, page, 0.0);
    let c = first.borrow().frame.c;
    l.holo.stack = vec![first];
    l.holo.preview = Some(spawn_preview(&l.holo));
    l.holo.beams.open(c, Some(BEAM));
    retarget(&mut l.holo);
    l.sound.menu("appear");
}
pub(super) fn is_open(l: &Lobby) -> bool {
    l.holo.anchor.is_some()
}
/// M.pointing: the beams' lengths of the two hands where they point at a panel.
pub(super) fn pointing(l: &Lobby) -> [Option<f64>; 2] {
    [l.holo.hands[0].at.as_ref().map(|a| a.len), l.holo.hands[1].at.as_ref().map(|a| a.len)]
}
pub(super) fn push(l: &mut Lobby, page: PageRef) {
    if l.holo.anchor.is_none() {
        return;
    }
    page.scroll.set(0.0);
    if let Some(top) = l.holo.stack.last().cloned() {
        if top.borrow().window {
            let mut t = top.borrow_mut();
            let off = t.view.as_ref().map_or(0.0, |v| v.offset);
            t.page().scroll.set(off);
            t.pages.push(page.clone());
            (t.page, t.dirty) = (Some(page), true);
            drop(t);
            l.sound.menu("open");
            return;
        }
    }
    let p = spawn_panel(&l.holo, page, 1.0);
    l.holo.stack.push(p);
    retarget(&mut l.holo);
    l.sound.menu("open");
}
pub(super) fn back(l: &mut Lobby) {
    if let Some(top) = l.holo.stack.last().cloned() {
        let popped = {
            let mut t = top.borrow_mut();
            if t.window && t.pages.len() > 1 {
                let page = t.pages.pop();
                let next = t.pages.last().cloned();
                (t.page, t.dirty) = (next, true);
                page
            } else {
                None
            }
        };
        if let Some(page) = popped {
            if let Some(ob) = page.on_back.clone() {
                ob(l);
            }
            l.sound.menu("close");
            return;
        }
    }
    if l.holo.stack.len() <= 1 {
        return;
    }
    let n = l.holo.stack.len() - 1;
    back_to(l, n);
}
pub(super) fn back_to(l: &mut Lobby, index: usize) {
    while l.holo.stack.len() > index {
        let Some(pr) = l.holo.stack.pop() else { break };
        for hand in 0..2 {
            show_mark(&mut pr.borrow_mut(), hand, None);
        }
        let (window, pages, page) = {
            let p = pr.borrow();
            (p.window, p.pages.clone(), p.page())
        };
        if window {
            for pg in pages.iter().rev() {
                if let Some(ob) = pg.on_back.clone() {
                    ob(l);
                }
            }
            pr.borrow_mut().opening = false;
            if let Some(old) = l.holo.closing.take() {
                sp::delete(old.borrow().s.id);
            }
            l.holo.closing = Some(pr);
        } else {
            {
                let mut p = pr.borrow_mut();
                (p.target, p.alpha_target) = (1.0, 0.0);
            }
            if let Some(ob) = page.on_back.clone() {
                ob(l);
            }
            l.holo.leaving.push(pr);
        }
    }
    retarget(&mut l.holo);
    l.sound.menu("close");
}
/// M.window: a page grown out of the top panel into the room (one at a time).
pub(super) fn window(l: &mut Lobby, page: PageRef) -> bool {
    let Some(top) = l.holo.stack.last().cloned() else { return false };
    if l.holo.anchor.is_none() || l.holo.spec.and_then(|s| s.window).is_none() || top.borrow().window {
        return false;
    }
    page.scroll.set(0.0);
    let from = top.borrow().frame;
    let w = spawn_window(&l.holo, page, from);
    l.holo.stack.push(w);
    retarget(&mut l.holo);
    l.sound.menu("open");
    true
}
pub(super) fn window_open(l: &Lobby) -> bool {
    l.holo.stack.last().is_some_and(|t| {
        let t = t.borrow();
        t.window && t.grow >= 1.0
    })
}
fn window_column(l: &Lobby, x: f64, behind: f64) -> Option<(f64, f64)> {
    let h = l.holo.spec?.window?;
    let wl = window_layout();
    let off = (x - wl.cw / 2.0) * h.size[0] / wl.cw;
    let b = h.bearing.to_radians();
    let k = (h.dist + behind) / h.dist;
    let px = (h.dist * b.sin() + b.cos() * off) * k;
    let py = (h.dist * b.cos() - b.sin() * off) * k;
    Some((px.atan2(py).to_degrees(), (px * px + py * py).sqrt()))
}
/// M.windowSpot: the bearing and distance of the window's clear part, `behind` metres beyond it.
pub(super) fn window_spot(l: &Lobby, behind: f64) -> Option<(f64, f64)> {
    let wl = window_layout();
    window_column(l, (wl.clear_x.unwrap_or(0.0) + wl.cw - PAD) / 2.0, behind)
}
pub(super) fn window_rows_edge(l: &Lobby) -> Option<f64> {
    let wl = window_layout();
    window_column(l, wl.row_x + wl.list_w, 0.0).map(|c| c.0)
}
/// M.hovered: the top panel's row under a hand.
pub(super) fn hovered(l: &Lobby) -> Option<Rc<Row>> {
    let p = l.holo.stack.last()?.borrow();
    let hv = p.hover.as_ref()?;
    row_by_key(&p, &hv.row)
}
pub(super) fn home(l: &mut Lobby) {
    back_to(l, 1);
}
pub(super) fn top(l: &Lobby) -> Option<PageRef> {
    l.holo.stack.last().map(|p| p.borrow().page())
}
pub(super) fn depth(l: &Lobby) -> usize {
    l.holo.stack.len()
}
pub(super) fn refresh(l: &mut Lobby) {
    dirty_all(&l.holo);
}
pub(super) fn close(l: &mut Lobby) {
    capture(&mut l.holo, false);
    sp::clear(TAG);
    l.holo.beams.close();
    l.holo.stack.clear();
    l.holo.leaving.clear();
    (l.holo.anchor, l.holo.preview, l.holo.closing, l.holo.board) = (None, None, None, None);
    l.holo.hands = Default::default();
}
pub(super) fn update(l: &mut Lobby, dt: f64) {
    l.holo.time += dt;
    if l.holo.anchor.is_none() {
        return;
    }
    let leaving = l.holo.leaving.clone();
    let mut keep = Vec::new();
    for pr in leaving {
        let gone = {
            let mut p = pr.borrow_mut();
            p.s.resolve(UI);
            animate(&l.holo, &mut p, dt);
            p.alpha < 0.02
        };
        if gone {
            sp::delete(pr.borrow().s.id);
        } else {
            keep.push(pr);
        }
    }
    l.holo.leaving = keep;
    if let Some(w) = l.holo.closing.clone() {
        let gone = {
            let mut p = w.borrow_mut();
            p.s.resolve(UI);
            animate_window(&mut p, dt);
            p.grow <= 0.0
        };
        if gone {
            sp::delete(w.borrow().s.id);
            l.holo.closing = None;
        }
    }
    if let Some(top) = l.holo.stack.last().cloned() {
        let up = top.borrow().page().update.clone();
        if let Some(up) = up {
            let time = l.holo.time;
            if up(l, time) {
                top.borrow_mut().dirty = true;
            }
        }
    }
    let stack = l.holo.stack.clone();
    for pr in &stack {
        let resolved = pr.borrow_mut().s.resolve(UI);
        if resolved {
            let (dirty, window) = {
                let p = pr.borrow();
                (p.dirty, p.window)
            };
            if dirty && !held(&l.holo, pr) {
                build(l, pr);
            }
            {
                let mut p = pr.borrow_mut();
                let rows = p.rows_cache.clone();
                if let Some(mut v) = p.view.take() {
                    let pp: &mut Panel = &mut p;
                    v.update(dt, 4, &mut |i, c, y| {
                        if let Some(r) = rows.get(i) {
                            draw_row(pp, c, r, y);
                        }
                    });
                    p.view = Some(v);
                }
            }
            if window {
                window_ready(&mut l.holo, pr);
            } else {
                ready(&mut pr.borrow_mut());
            }
        }
        let window = pr.borrow().window;
        if window {
            animate_window(&mut pr.borrow_mut(), dt);
        } else {
            animate(&l.holo, &mut pr.borrow_mut(), dt);
        }
    }
    pointers(l);
    update_preview(&mut l.holo, dt);
    update_board(l, dt);
}
/// M.frame: a panel's frame in the world (the top one by default).
pub(super) fn frame(l: &Lobby, index: Option<usize>) -> Option<Frame> {
    let i = index.unwrap_or(l.holo.stack.len());
    l.holo.stack.get(i.checked_sub(1)?).map(|p| p.borrow().frame)
}
/// M.state's summary for the log.
pub(super) fn state(l: &Lobby) -> String {
    let pages: Vec<String> = l
        .holo
        .stack
        .iter()
        .map(|p| {
            let p = p.borrow();
            format!("{}@{:.2}{}{}", p.page().heading_now().title.unwrap_or_else(|| "-".into()), p.slot, if p.ready { "" } else { "(not ready)" }, if p.window { " window" } else { "" })
        })
        .collect();
    let board = match &l.holo.board {
        Some(b) => format!("ready {} alpha {:.2} built {}", b.ready, b.alpha, b.built),
        None => "-".into(),
    };
    format!("open {} pages [{}] leaving {} captured {} board {}", l.holo.anchor.is_some(), pages.join(", "), l.holo.leaving.len(), l.holo.captured, board)
}
