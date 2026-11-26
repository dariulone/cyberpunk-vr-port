//! CyberpunkVRPort_Menu/modules/gameplay.lua and the shared parts of reference_view.lua: the menu's gameplay sections
//! (character, inventory, cyberware, crafting, trade, stash, load, journal, codex, shards, tarot, wait) -- their state,
//! the background reads they draw from (menu_jobs), the widgets they share (rows, text rows, headings, the hub-style
//! action, the title-line action), the redraw when a watched read changes, the fitted pictures, the waiting's blink.
use super::lobby_ink as ink;
use super::lobby_rtti as rt;
use super::menu_bits::ViewRef;
use super::menu_core::*;
use super::menu_data::Icon;
use super::menu_gpdata as gd;
use super::menu_ink as k;
use super::menu_jobs::{Factory, Jobs};
use super::menu_locale as loc;
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use std::collections::HashMap;
use std::rc::Rc;

pub(super) const WIDTH: f64 = 820.0;
pub(super) const DETAIL: f64 = 900.0;

const LABELS: &[(&str, &str, &str)] = &[
    ("character", "CHARACTER", "ПЕРСОНАЖ"),
    ("journal", "JOURNAL", "ЖУРНАЛ"),
    ("codex", "DATABASE", "БАЗА ДАННЫХ"),
    ("timeskip", "WAIT", "ОЖИДАНИЕ"),
    ("attributes", "Attributes", "Характеристики"),
    ("perks", "Perks", "Способности"),
    ("skills", "Skills", "Навыки"),
    ("level", "Level", "Уровень"),
    ("points", "Attribute points", "Очки характеристик"),
    ("perkPoints", "Perk points", "Очки способностей"),
    ("relic", "Relic", "Биочип"),
    ("upgrade", "UPGRADE +1", "ПОВЫСИТЬ +1"),
    ("buy", "LEARN", "ИЗУЧИТЬ"),
    ("refund", "REFUND", "ВЕРНУТЬ ОЧКИ"),
    ("confirm", "CONFIRM", "ПОДТВЕРДИТЬ"),
    ("cancel", "CANCEL", "ОТМЕНА"),
    ("yes", "YES", "ДА"),
    ("back", "BACK", "НАЗАД"),
    ("requires", "Requires", "Требуется"),
    ("cost", "Cost", "Стоимость"),
    ("upgradeQuestion", "Spend 1 attribute point?", "Потратить 1 очко характеристики?"),
    ("upgradeHint", "Future +1 clicks spend points immediately.", "В дальнейшем нажатие +1 сразу потратит очко."),
    ("active", "Active", "Активные"),
    ("completed", "Completed", "Завершённые"),
    ("failed", "Failed", "Проваленные"),
    ("all", "All", "Все"),
    ("track", "TRACK OBJECTIVE", "ОТСЛЕЖИВАТЬ ЦЕЛЬ"),
    ("tracked", "Tracked", "Отслеживается"),
    ("optional", "Optional", "Необязательно"),
    ("choose", "Select an entry", "Выберите запись"),
    ("empty", "No entries", "Нет записей"),
    ("readError", "Unable to read game data", "Не удалось прочитать данные игры"),
    ("retry", "RETRY", "ПОВТОРИТЬ"),
    ("skip", "SKIP", "ПОДОЖДАТЬ"),
    ("hours", "hours", "ч."),
    ("now", "Now", "Сейчас"),
    ("target", "After skipping", "После ожидания"),
    ("tomorrow", "Next day", "Следующий день"),
    ("blocked", "Unavailable right now", "Сейчас недоступно"),
    ("done", "Done", "Готово"),
    ("characters", "Characters", "Персонажи"),
    ("locations", "Locations", "Места"),
    ("lore", "World", "Мир"),
    ("reset", "RESET FILTER", "СБРОСИТЬ ФИЛЬТР"),
    ("xp", "XP", "Опыт"),
    ("spend", "Spend points?", "Потратить очки?"),
    ("pending", "Applying…", "Применение…"),
    ("closeDetails", "ATTRIBUTES", "ХАРАКТЕРИСТИКИ"),
    ("ability", "Abilities", "Способности"),
];
/// The gameplay sections' words (LABELS through L.labels).
pub(super) fn t(key: &str) -> String {
    LABELS.iter().find(|l| l.0 == key).map(|l| loc::tr(l.2, l.1)).unwrap_or_else(|| key.to_string())
}

pub(super) type Changed = Rc<dyn Fn(&mut Menu, bool) -> bool>;
pub(super) struct Watch {
    producer: Factory,
    period: f64,
    seq: Option<u64>,
    changed: Option<Changed>,
}
/// A picture measured once it has loaded, then fitted into its box.
pub(super) struct Img {
    widget: Handle,
    view: Option<ViewRef>,
    item_y: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    no_hit: bool,
    fitted: bool,
}
#[derive(Clone, PartialEq)]
pub(super) enum Open {
    Unset,
    Closed,
    Key(String),
}
/// The gameplay sections' state (kit.S.gameplay).
pub(super) struct Gp {
    pub jobs: Jobs,
    pub hours: f64,
    pub quest_filter: String,
    pub codex_filter: f64,
    pub codex: Option<f64>,
    pub reset_list: bool,
    pub images: Vec<Img>,
    pub watch: Vec<(String, Watch)>,
    pub reading: bool,
    pub view_key: String,
    pub last_poll: Option<f64>,
    pub signature: Option<String>,
    pub confirm: Option<String>,
    pub pending_until: f64,
    pub quest: Option<f64>,
    pub journal_inspect: Option<f64>,
    pub inspect_shown: Option<f64>,
    pub shard: Option<f64>,
    pub shard_group: Open,
    pub attribute: Option<f64>,
    pub attribute_hover: Option<f64>,
    /// The open +1 confirmation (s.attributePrompt): its number (a stale card's buttons do nothing) and the attribute.
    pub attribute_prompt: Option<(u64, gd::Attr)>,
    pub perk: Option<String>,
    pub skills: bool,
    pub tarot: Option<i64>,
    pub tarot_offset: Option<f64>,
    pub summary_expanded: bool,
    pub summary_hint_owner: Option<f64>,
    pub summary_at: f64,
    pub summary: Option<gd::Summary>,
    pub tree_keys: HashMap<String, String>,
    pub distances: HashMap<i64, f64>,
    pub distance_widgets: HashMap<i64, Handle>,
    pub quest_structure: String,
    pub clock: Option<gd::Clock>,
    pub videos: Vec<Handle>,
    pub wait: Option<Wait>,
    pub inv: super::menu_inventory::InvState,
    pub cyber: super::menu_cyberware::CyberState,
    pub trade: super::menu_trade::TradeState,
    pub stash: super::menu_stashview::StashState,
    pub load: super::menu_load::LoadState,
}
impl Default for Gp {
    fn default() -> Gp {
        Gp {
            jobs: Jobs::default(),
            hours: 1.0,
            quest_filter: "active".into(),
            codex_filter: 0.0,
            codex: None,
            reset_list: false,
            images: Vec::new(),
            watch: Vec::new(),
            reading: false,
            view_key: String::new(),
            last_poll: None,
            signature: None,
            confirm: None,
            pending_until: 0.0,
            quest: None,
            journal_inspect: None,
            inspect_shown: None,
            shard: None,
            shard_group: Open::Unset,
            attribute: None,
            attribute_hover: None,
            attribute_prompt: None,
            perk: None,
            skills: false,
            tarot: None,
            tarot_offset: None,
            summary_expanded: false,
            summary_hint_owner: None,
            summary_at: 0.0,
            summary: None,
            tree_keys: HashMap::new(),
            distances: HashMap::new(),
            distance_widgets: HashMap::new(),
            quest_structure: String::new(),
            clock: None,
            videos: Vec::new(),
            wait: None,
            inv: Default::default(),
            cyber: Default::default(),
            trade: Default::default(),
            stash: Default::default(),
            load: Default::default(),
        }
    }
}
/// The waiting's blink: the eyes closed, the time skipped, held until it took, opened again.
#[derive(Clone)]
pub(super) struct Wait {
    pub started: f64,
    pub hours: f64,
    pub phase: u8,
    pub at: f64,
    pub target: f64,
    pub applied: bool,
}

// ---------------------------------------------------------------- text
/// clean: the game's markup and escaped breaks out of a text.
pub(super) fn clean(text: &str) -> String {
    let s = text.replace("\\n", "\n").replace('\r', "");
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    if in_tag {
        // an unclosed '<' is text, as the Lua pattern leaves it
        return s;
    }
    out
}
/// wrap: lines of at most `count` characters, long words split at a character boundary.
pub(super) fn wrap(text: &str, count: usize) -> Vec<String> {
    let count = count.max(1);
    let mut lines = Vec::new();
    for para in clean(text).split('\n') {
        let mut line = String::new();
        for token in para.split_whitespace() {
            let mut word = token.to_string();
            if !line.is_empty() && line.chars().count() + word.chars().count() + 1 > count {
                lines.push(std::mem::take(&mut line));
            }
            while word.chars().count() > count {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                let head: String = word.chars().take(count).collect();
                word = word.chars().skip(count).collect();
                lines.push(head);
            }
            line = if line.is_empty() { word } else { format!("{line} {word}") };
        }
        lines.push(line);
    }
    lines
}

// ---------------------------------------------------------------- widgets
/// textRows: a text as rows of its lines (drag-scrollable, no action).
pub(super) fn text_rows(rows: &mut Vec<Row>, text: &str, width: f64, color: Option<[f64; 3]>, size: Option<f64>) {
    let size = size.unwrap_or(34.0);
    let col = color.unwrap_or(k::TEXT);
    for line in wrap(text, ((width - 64.0) / (size * 0.56)).floor() as usize) {
        rows.push(Row {
            h: size + 12.0,
            build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                k::text(c, 22.0, 0.0, it.cw - 44.0, size + 12.0, &line, size, col, "left", false);
                if !line.is_empty() {
                    button(m, "p2", &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) }, 0.0, 0.0, it.cw, size + 12.0, None, None);
                }
            }),
        });
    }
}
/// row: a list row with its label and a second line.
pub(super) fn row(group: &str, label: &str, sub: &str, click: Option<Act>, selected: bool, color: Option<[f64; 3]>) -> Row {
    let (group, label, sub) = (group.to_string(), clean(label), clean(sub));
    Row {
        h: 110.0,
        build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
            let bg = k::round_fill(c, 0.0, 0.0, it.cw, 96.0, k::ICE, 0.07, Some(20.0));
            k::text(c, 24.0, 2.0, it.cw - 48.0, 53.0, &k::clip(&label, 31), 38.0, color.unwrap_or(k::TEXT), "left", false);
            k::text(c, 24.0, 52.0, it.cw - 48.0, 35.0, &k::clip(&sub, 42), 28.0, k::GREY, "left", false);
            let b2 = bg.clone();
            button(
                m,
                &group,
                &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                0.0,
                0.0,
                it.cw,
                96.0,
                click.clone(),
                Some(Rc::new(move |hover, _| {
                    ink::tint(&b2, if hover { k::CYAN } else if selected { k::RED } else { k::ICE });
                    ink::opacity(&b2, if hover { 0.25 } else if selected { 0.20 } else { 0.07 });
                })),
            );
        }),
    }
}
/// button: the menu's wide action, 76 high.
#[allow(clippy::too_many_arguments)]
pub(super) fn wide(m: &mut Menu, p: &Handle, x: f64, y: f64, w: f64, label: &str, enabled: bool, f: Act, group: Option<&str>, sx: Option<f64>) -> BtnRef {
    action(m, p, group.unwrap_or("p1"), &at(sx.unwrap_or(P1X), P1Y), x, y, w, 76.0, label, enabled, f, Some(k::CYAN), None)
}
pub(super) fn heading(m: &mut Menu, p: &Handle, w: f64, title: &str, sx: f64) {
    glass(m, p, w, 1160.0, Some((sx, P1Y)));
    k::text(p, 32.0, 8.0, w - 64.0, 76.0, &k::clip(title, 32), 42.0, k::WHITE, "left", false);
    k::rect(p, 24.0, 90.0, w - 48.0, 2.0, k::ICE, 0.2);
}
/// list: the menu's list (an "empty" row when there is nothing and nothing is being read).
#[allow(clippy::too_many_arguments)]
pub(super) fn glist(m: &mut Menu, p: &Handle, group: &str, key: &str, sx: f64, top: f64, w: f64, mut rows: Vec<Row>, height: f64, keep: Option<f64>) -> ViewRef {
    if rows.is_empty() && !m.gp.reading {
        rows.push(row(group, &t("empty"), "", None, false, None));
    }
    list(m, p, group, key, sx, P1Y, top, w - 28.0, rows, height, keep)
}
pub(super) fn redraw(m: &mut Menu) {
    super::menu_main::build_panels(m, false, false);
}
/// fetch: the last result of `key` at once, a refresh queued behind it once `period` old; the first time, read now
/// (s.reading while it has not landed). Watched: a newer result redraws (or goes to `changed`).
pub(super) fn fetch<T: 'static>(m: &mut Menu, key: &str, producer: Factory, period: f64, budget: f64, background: bool, changed: Option<Changed>) -> Option<Rc<T>> {
    let jobs = &mut m.gp.jobs;
    let mut value = jobs.get::<T>(key);
    if value.is_none() && background {
        jobs.run(key, producer.clone(), false);
    } else if value.is_none() {
        jobs.run(key, producer.clone(), true);
        if jobs.finish_now(key, budget) {
            value = jobs.get::<T>(key);
        } else {
            m.gp.reading = true;
        }
    } else if jobs.age(key) > period {
        jobs.run(key, producer.clone(), true);
    }
    let seq = m.gp.jobs.seq(key);
    m.gp.watch.retain(|(k0, _)| k0 != key);
    m.gp.watch.push((key.to_string(), Watch { producer, period, seq, changed }));
    value
}
pub(super) fn open_section(m: &mut Menu, key: &str) {
    super::menu_main::open_section(m, Some(key), false);
}
/// perform: a request made; the toast says it applies, the panel is read again behind it.
pub(super) fn perform(m: &mut Menu, f: impl FnOnce(&mut Menu) -> bool) {
    let ok = f(m);
    m.gp.confirm = None;
    match m.section.as_deref() {
        Some("journal") => {
            if let Some(h) = m.gp.quest {
                m.gp.jobs.run(&format!("quest:{}", gd::fmt_g(h)), gd::quest_detail(h), true);
            }
            m.gp.jobs.run("quests", gd::quest_list(), true);
        }
        Some("character") => {
            m.gp.jobs.drop_key("character");
            m.gp.jobs.drop_prefix("perks:");
        }
        _ => {}
    }
    let tx = if ok { t("pending") } else { t("blocked") };
    toast(m, &tx);
    m.gp.pending_until = m.time + if m.section.as_deref() == Some("timeskip") { 3.0 } else { 0.35 };
    m.refresh_at = Some(m.gp.pending_until);
    redraw(m);
}
pub(super) fn filters(m: &mut Menu, p: &Handle, items: &[(f64, String)], current: f64, change: Rc<dyn Fn(&mut Menu, f64)>) {
    let w = (WIDTH - 40.0 - (items.len() as f64 - 1.0) * 12.0) / items.len() as f64;
    for (i, (key, label)) in items.iter().enumerate() {
        let x = 20.0 + i as f64 * (w + 12.0);
        let on = *key == current;
        let bg = k::round_fill(p, x, 108.0, w, 60.0, if on { k::CYAN } else { k::ICE }, 0.14, Some(20.0));
        k::text(p, x, 108.0, w, 60.0, label, 26.0, if on { k::CYAN } else { k::TEXT }, "center", false);
        let (c, key) = (change.clone(), *key);
        button(m, "p1", &at(P1X, P1Y), x, 108.0, w, 60.0, Some(Rc::new(move |m: &mut Menu, _| c(m, key))), Some(Rc::new(move |h, _| ink::opacity(&bg, if h { 0.3 } else { 0.14 }))));
    }
}
// reference_view's helpers
pub(super) fn line(p: &Handle, x1: f64, y1: f64, x2: f64, y2: f64, col: [f64; 3], op: f64, width: f64) -> Handle {
    k::line(p, x1, y1, x2, y2, col, op, width)
}
pub(super) fn icon(p: &Handle, x: f64, y: f64, w: f64, h: f64, image: Option<&Icon>, col: Option<[f64; 3]>) -> Handle {
    match image {
        Some(i) => k::image_raw(p, x, y, w, h, i.atlas, i.part, Some(col.unwrap_or(k::CYAN)), 1.0),
        None => Handle::null(),
    }
}
fn set_align(w: &Handle, setter: &str, en: &str, member: &str) {
    if let Some(v) = crate::script_call::enum_value(en, member) {
        ink::call(w, setter, &[Value::U64(v as u64)]);
    }
}
/// picture: an image shown at its own proportions, fitted into its box once it has loaded (G.update).
#[allow(clippy::too_many_arguments)]
pub(super) fn picture(m: &mut Menu, p: &Handle, x: f64, y: f64, w: f64, h: f64, image: Option<&Icon>, view: Option<(ViewRef, f64)>) {
    let Some(image) = image else { return };
    let pic = k::image_raw(p, x, y, 1.0, 1.0, image.atlas, image.part, Some(k::WHITE), 0.001);
    ink::call(&pic, "SetFitToContent", &[Value::Bool(true)]);
    set_align(&pic, "SetContentHAlign", "inkEHorizontalAlign", "Center");
    set_align(&pic, "SetContentVAlign", "inkEVerticalAlign", "Center");
    ink::v2(&pic, "SetRenderTransformPivot", 0.0, 0.0);
    ink::v2(&pic, "SetScale", 0.001, 0.001);
    let no_hit = view.is_none();
    let (view, item_y) = view.map_or((None, 0.0), |(v, y)| (Some(v), y));
    m.gp.images.push(Img { widget: pic, view, item_y, x, y, w, h, no_hit, fitted: false });
}
/// action: the hub's own button (a cut frame, the label), disabled it says it is not available.
#[allow(clippy::too_many_arguments)]
pub(super) fn hub_action(m: &mut Menu, p: &Handle, sx: f64, x: f64, y: f64, w: f64, label: &str, f: Act, enabled: bool, h: Option<f64>, fs: Option<f64>) -> BtnRef {
    let h = h.unwrap_or(68.0);
    let bg = k::round_fill(p, x, y, w, h, k::NAVY, if enabled { 0.18 } else { 0.035 }, Some(4.0));
    let edge = k::cut_frame(&mut m.pool, p, x, y, w, h, k::CYAN, if enabled { 0.45 } else { 0.18 }, Some(9.0), Some(1.5));
    let text = k::text(p, x, y, w, h, label, fs.unwrap_or(32.0), if enabled { k::TEXT } else { k::GREY }, "center", false);
    button(
        m,
        "p1",
        &at(sx, P1Y),
        x,
        y,
        w,
        h,
        Some(Rc::new(move |m: &mut Menu, hand| {
            if enabled {
                f(m, hand);
            } else {
                let b = m.tx("blocked");
                toast(m, &b);
            }
        })),
        Some(Rc::new(move |hv, _| {
            if !enabled {
                return;
            }
            ink::tint(&bg, if hv { k::YELLOW } else { k::NAVY });
            ink::opacity(&bg, if hv { 0.16 } else { 0.18 });
            k::tint(&edge, if hv { k::YELLOW } else { k::CYAN }, Some(if hv { 1.0 } else { 0.45 }));
            ink::tint(&text, if hv { k::YELLOW } else { k::TEXT });
        })),
    )
}
const HEAD_Y: f64 = 22.0;
const HEAD_BH: f64 = 48.0;
const HEAD_FS: f64 = 28.0;
/// headAction: a small button on a panel's title line, its right edge at x1.
#[allow(clippy::too_many_arguments)]
pub(super) fn head_action(m: &mut Menu, p: &Handle, sx: f64, x1: f64, label: &str, f: Act, enabled: bool, w: Option<f64>) -> BtnRef {
    let w = w.unwrap_or_else(|| (label.chars().count() as f64 * HEAD_FS * 0.56).floor() + 48.0);
    hub_action(m, p, sx, x1 - w, HEAD_Y, w, label, f, enabled, Some(HEAD_BH), Some(HEAD_FS))
}
pub(super) fn title(p: &Handle, text: &str, w: f64) {
    k::text(p, 28.0, 12.0, w - 56.0, 60.0, &k::clip(text, 49), 42.0, k::WHITE, "left", false);
    k::rect(p, 28.0, 82.0, w - 56.0, 1.0, k::CYAN, 0.32);
}
/// comparison: the inventory's COMPARE switch.
pub(super) fn comparison(m: &mut Menu, p: &Handle, height: f64, enabled: bool, toggle: Act, key: &str) -> BtnRef {
    let label = if enabled { loc::tr("СРАВНЕНИЕ: ВКЛ", "COMPARE: ON") } else { loc::tr("СРАВНЕНИЕ: ВЫКЛ", "COMPARE: OFF") };
    let b = hub_action(m, p, P1X, 24.0, height - 95.0, 430.0, &label, toggle, true, None, None);
    *b.key.borrow_mut() = Some(key.to_string());
    b
}
pub(super) fn stop_videos(m: &mut Menu) {
    for v in std::mem::take(&mut m.gp.videos) {
        ink::call(&v, "Stop", &[]);
    }
}
/// A widget's margin (where it was placed in its parent).
pub(super) fn margin(w: &Handle) -> (f64, f64) {
    rt::call(w, "GetMargin", &[]).map_or((0.0, 0.0), |o| {
        let v = rt::Val::of(&o);
        (v.f("left").and_then(|x| x.num()).unwrap_or(0.0), v.f("top").and_then(|x| x.num()).unwrap_or(0.0))
    })
}

// ---------------------------------------------------------------- the codex
fn codex(m: &mut Menu, p: &Handle, p2: &Handle) {
    heading(m, p, WIDTH, &t("codex"), P1X);
    let cur = m.gp.codex_filter;
    let items = [(0.0, t("all")), (1.0, t("lore")), (2.0, t("characters")), (3.0, t("locations"))];
    filters(
        m,
        p,
        &items,
        cur,
        Rc::new(|m: &mut Menu, f| {
            (m.gp.codex_filter, m.gp.codex, m.gp.reset_list) = (f, None, true);
            redraw(m);
        }),
    );
    let watch: Changed = Rc::new(|m: &mut Menu, first| {
        if !first {
            m.gp.jobs.run("codex", gd::codex_list(), false);
        }
        false
    });
    fetch::<bool>(m, "codexWatch", gd::codex_watch(), 4.0, 0.0, true, Some(watch));
    let entries = fetch::<Vec<gd::CodexEntry>>(m, "codex", gd::codex_list(), f64::INFINITY, 0.012, false, None).unwrap_or_default();
    let mut rows = Vec::new();
    let mut selected = None;
    for e in entries.iter() {
        if Some(e.hash) == m.gp.codex {
            selected = Some(e.clone());
        }
        if m.gp.codex_filter == 0.0 || m.gp.codex_filter == e.category {
            let sub = match e.category as i64 {
                1 => t("lore"),
                2 => t("characters"),
                3 => t("locations"),
                _ => String::new(),
            };
            let hash = e.hash;
            rows.push(row(
                "p1",
                &e.name,
                &sub,
                Some(Rc::new(move |m: &mut Menu, _| {
                    m.gp.codex = Some(hash);
                    gd::visit(hash);
                    if let Some(mut list) = m.gp.jobs.get::<Vec<gd::CodexEntry>>("codex").map(|l| (*l).clone()) {
                        for x in list.iter_mut().filter(|x| x.hash == hash) {
                            x.fresh = false;
                        }
                        m.gp.jobs.put("codex", list, None);
                    }
                    redraw(m);
                })),
                m.gp.codex == Some(e.hash),
                e.fresh.then_some(k::YELLOW),
            ));
        }
    }
    let keep = m.keep_scroll;
    glist(m, p, "p1", "p1", P1X, 188.0, WIDTH, rows, 800.0, keep);
    heading(m, p2, DETAIL, &t("codex"), P2X);
    let mut details = Vec::new();
    match selected {
        Some(sel) => {
            text_rows(&mut details, &sel.name, DETAIL, Some(k::WHITE), Some(40.0));
            if let Some(img) = gd::image(sel.image) {
                details.push(Row {
                    h: 320.0,
                    build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                        picture(m, c, 22.0, 10.0, it.cw - 44.0, 290.0, Some(&img), Some((it.view.clone(), it.y)));
                    }),
                });
            }
            text_rows(&mut details, &sel.description, DETAIL, None, None);
        }
        None => text_rows(&mut details, &t("choose"), DETAIL, Some(k::GREY), None),
    }
    glist(m, p2, "p2", "p2", P2X, 110.0, DETAIL, details, 880.0, None);
    head_action(m, p, P1X, WIDTH - 72.0, &loc::tr("ЧИПЫ", "SHARDS"), Rc::new(|m: &mut Menu, _| open_section(m, "shards")), true, None);
}

// ---------------------------------------------------------------- driven by the menu
fn draw(m: &mut Menu, p: &Handle, p2: &Handle) {
    match m.section.clone().as_deref() {
        Some("codex") => codex(m, p, p2),
        Some("character") => super::menu_character::character(m, p, p2),
        Some("tarot") => super::menu_character::tarot(m, p, p2),
        Some("journal") => super::menu_journal::journal(m, p, p2),
        Some("shards") => super::menu_journal::shards(m, p, p2),
        Some("timeskip") => super::menu_journal::wait(m, p),
        Some("inventory") => super::menu_inventory::draw(m, p, p2),
        Some("cyberware") => super::menu_cyberware::draw(m, p, p2),
        Some("crafting") => super::menu_crafting::draw(m, p, p2),
        Some("trade") => super::menu_trade::draw(m, p, p2),
        Some("stash") => super::menu_stashview::draw(m, p, p2),
        Some("load") => super::menu_load::draw(m, p, p2),
        _ => {}
    }
}
pub(super) fn build(m: &mut Menu, p: &Handle, p2: &Handle) {
    m.gp.images.clear();
    m.gp.watch.clear();
    m.gp.reading = false;
    if m.gp.reset_list {
        m.keep_scroll = None;
        m.gp.reset_list = false;
    }
    if m.section.as_deref() == Some("character") {
        refresh_character(m);
    }
    draw(m, p, p2);
    let section = m.section.clone().unwrap_or_default();
    let g = &m.gp;
    let view_key = format!("{section}:{:?}:{}:{:?}:{}:{}", g.attribute, g.skills, g.tarot, g.perk.is_some(), g.journal_inspect.is_some());
    if m.gp.view_key != view_key && (section == "character" || section == "tarot") {
        let (m1, m2) = (margin(p), margin(p2));
        slide_in(m, "p1", p, m1.0, m1.1, 0.0, 0.0, 22.0, None);
        slide_in(m, "p2", p2, m2.0, m2.1, 0.03, 28.0, 0.0, None);
        m.panel_interactive_at = m.panel_interactive_at.max(m.time + 0.22);
    }
    m.gp.view_key = view_key;
    m.gp.last_poll = Some(m.time);
}
/// A queued purchase may land between the click's redraw and its scheduled refresh: the snapshot is dropped before the
/// new point totals are drawn or recorded (main's gameplay.lua refreshCharacter). True when the points moved.
fn refresh_character(m: &mut Menu) -> bool {
    let sig = gd::character_signature();
    if Some(&sig) == m.gp.signature.as_ref() {
        return false;
    }
    m.gp.jobs.drop_key("character");
    m.gp.jobs.drop_prefix("perks:");
    (m.gp.confirm, m.gp.attribute_prompt, m.gp.signature) = (None, None, Some(sig));
    true
}
fn clear_state(m: &mut Menu) {
    let g = &mut m.gp;
    (g.confirm, g.attribute_prompt, g.last_poll, g.signature, g.reading) = (None, None, None, None, false);
    g.images.clear();
    g.watch.clear();
}
pub(super) fn leave(m: &mut Menu) {
    stop_videos(m);
    super::menu_inventory::leave(m);
    super::menu_cyberware::leave(m);
    super::menu_crafting::leave(m);
    super::menu_trade::leave(m);
    super::menu_stashview::leave(m);
    super::menu_load::leave(m);
    clear_state(m);
}
pub(super) fn before_build(m: &mut Menu) {
    stop_videos(m);
    super::menu_cyberware::before_build(m);
    super::menu_trade::before_build(m);
    super::menu_crafting::before_build(m);
    super::menu_stashview::before_build(m);
    super::menu_load::before_build(m);
}
fn hands_busy(m: &Menu) -> bool {
    m.hands.iter().any(|h| h.drag.is_some() || h.pend.is_some() || h.scroll.is_some()) || m.views.iter().any(|(_, v)| v.v.borrow().moving())
}
/// Redraw keeping the detail list where it was scrolled.
pub(super) fn redraw_keep_detail(m: &mut Menu) {
    let offset = m.view("p2").map(|v| v.v.borrow().offset);
    redraw(m);
    if let (Some(o), Some(v)) = (offset, m.view("p2")) {
        v.v.borrow_mut().scroll_to(o);
    }
}
/// The keys the panel on screen drew from: a newer result redraws it (or goes to its own handler), a result older than
/// its period is read again behind the panel.
fn watch(m: &mut Menu) {
    let busy = hands_busy(m);
    let mut stale = false;
    let keys: Vec<String> = m.gp.watch.iter().map(|(k0, _)| k0.clone()).collect();
    for key in keys {
        let seq = m.gp.jobs.seq(&key);
        let Some((_, w)) = m.gp.watch.iter_mut().find(|(k0, _)| *k0 == key) else { continue };
        let mut call: Option<(Changed, bool)> = None;
        if seq != w.seq {
            let first = w.seq.is_none();
            w.seq = seq;
            match &w.changed {
                Some(c) => call = Some((c.clone(), first)),
                None => stale = true,
            }
        }
        let (producer, period) = (w.producer.clone(), w.period);
        if let Some((c, first)) = call {
            if c(m, first) {
                stale = true;
            }
        }
        if !m.gp.jobs.busy(&key) && m.gp.jobs.age(&key) > period {
            m.gp.jobs.run(&key, producer, false);
        }
    }
    if stale && !busy {
        redraw_keep_detail(m);
    }
}
pub(super) fn update(m: &mut Menu) {
    watch(m);
    let mut adds = Vec::new();
    for img in m.gp.images.iter_mut().filter(|i| !i.fitted) {
        let size = rt::call(&img.widget, "GetDesiredSize", &[]).map(|o| {
            let v = rt::Val::of(&o);
            (v.f("X").and_then(|x| x.num()).unwrap_or(0.0), v.f("Y").and_then(|x| x.num()).unwrap_or(0.0))
        });
        let Some((sx, sy)) = size.filter(|s| s.0 > 1.0 && s.1 > 1.0) else { continue };
        let scale = (img.w / sx).min(img.h / sy);
        let (x, y) = (img.x + (img.w - sx * scale) / 2.0, img.y + (img.h - sy * scale) / 2.0);
        ink::call(&img.widget, "SetFitToContent", &[Value::Bool(false)]);
        set_align(&img.widget, "SetContentHAlign", "inkEHorizontalAlign", "Fill");
        set_align(&img.widget, "SetContentVAlign", "inkEVerticalAlign", "Fill");
        ink::size(&img.widget, sx * scale, sy * scale);
        ink::v2(&img.widget, "SetScale", 1.0, 1.0);
        ink::move_to(&img.widget, x, y);
        ink::opacity(&img.widget, 1.0);
        if !img.no_hit {
            if let Some(v) = &img.view {
                let vx = v.borrow().x;
                adds.push((At { ox: vx, oy: img.item_y, view: Some(v.clone()) }, x, y, sx * scale, sy * scale));
            }
        }
        img.fitted = true;
    }
    for (a, x, y, w, h) in adds {
        button(m, "p2", &a, x, y, w, h, None, None);
    }
    match m.section.clone().as_deref() {
        Some("inventory") => return super::menu_inventory::update(m),
        Some("cyberware") => return super::menu_cyberware::update(m),
        Some("crafting") => return super::menu_crafting::update(m),
        Some("trade") => return super::menu_trade::update(m),
        Some("stash") => return super::menu_stashview::update(m),
        Some("load") => return super::menu_load::update(m),
        _ => {}
    }
    if m.time - m.gp.last_poll.unwrap_or(0.0) < 1.0 {
        return;
    }
    m.gp.last_poll = Some(m.time);
    if hands_busy(m) {
        return;
    }
    match m.section.clone().as_deref() {
        Some("timeskip") => {
            let c = gd::clock();
            if m.gp.clock.is_some_and(|o| c.minute != o.minute || c.hour != o.hour || c.allowed != o.allowed || c.hidden != o.hidden) {
                redraw(m);
            }
        }
        Some("character") => {
            if refresh_character(m) {
                redraw_keep_detail(m);
            }
        }
        _ => {}
    }
}
pub(super) fn summary(m: &mut Menu, layer: &Handle) {
    super::menu_journal::summary(m, layer);
}
pub(super) fn update_summary(m: &mut Menu) {
    super::menu_journal::summary_rect(m);
    if !m.summary_visible || m.time - m.gp.summary_at < 2.0 || m.layer_summary.is_null() {
        return;
    }
    m.gp.summary_at = m.time;
    let Some(data) = gd::summary() else { return };
    let stamp = |d: &gd::Summary| {
        let mut v = vec![(d.seconds / 60.0).floor(), d.health.floor(), d.armor.floor(), d.dps.floor()].iter().map(|x| gd::fmt_g(*x)).collect::<Vec<_>>();
        for p in &d.progress {
            v.push(gd::fmt_g(p.level));
            v.push(gd::fmt_g(p.xp));
        }
        for (_, value) in &d.other {
            v.push(value.clone());
        }
        v.join("|")
    };
    if m.gp.summary.as_ref().is_none_or(|old| stamp(old) != stamp(&data)) {
        let layer = m.layer_summary.clone();
        super::menu_journal::summary(m, &layer);
    }
}
/// On a menu open: what the quest group draws, read behind the opening animation.
pub(super) fn prefetch(m: &mut Menu) {
    if super::menu_main::tour_no_jobs() {
        return;
    }
    let j = &mut m.gp.jobs;
    if !j.has("quests") || j.age("quests") > 2.0 {
        j.run("quests", gd::quest_list(), false);
    }
    if !j.has("codex") {
        j.run("codex", gd::codex_list(), false);
    }
    if !j.has("shards") || j.age("shards") > 5.0 {
        j.run("shards", gd::shard_list(), false);
    }
}
pub(super) fn step(m: &mut Menu) {
    m.gp.jobs.step(0.0015);
}
fn blink(v: f64) {
    super::lobby_spawn::native::<()>("VRMenuBlink", &[Value::Float(v as f32)]);
}
fn capture(on: bool) {
    super::lobby_spawn::native::<bool>("VRMenuCapture", &[Value::Bool(on)]);
}
pub(super) fn cancel_transition(m: &mut Menu) {
    m.gp.wait = None;
    m.wait_transition = false;
    blink(0.0);
    capture(false);
}
/// Start the waiting's blink (the menu closed first).
pub(super) fn start_wait(m: &mut Menu, hours: f64) {
    m.gp.wait = Some(Wait { started: m.time, hours, phase: 0, at: 0.0, target: 0.0, applied: false });
    m.wait_transition = true;
    capture(true);
    blink(0.0);
}
pub(super) fn transition(m: &mut Menu, _dt: f64) -> bool {
    let Some(mut tr) = m.gp.wait.clone() else { return false };
    let elapsed = m.time - tr.started;
    let ease = |t: f64| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    match tr.phase {
        0 => {
            blink(ease(elapsed));
            if elapsed >= 1.0 {
                tr.target = gd::game_seconds_now() + tr.hours * 3600.0;
                tr.applied = gd::skip(tr.hours);
                (tr.phase, tr.at) = (1, m.time);
            }
        }
        1 => {
            blink(1.0);
            let now = gd::game_seconds_now();
            if m.time - tr.at >= 0.4 && (!tr.applied || now >= tr.target || m.time - tr.at >= 8.0) {
                (tr.phase, tr.at) = (2, m.time);
            }
        }
        _ => {
            blink(1.0 - ease(m.time - tr.at));
            if m.time - tr.at >= 1.0 {
                cancel_transition(m);
                return true;
            }
        }
    }
    m.gp.wait = Some(tr);
    true
}
