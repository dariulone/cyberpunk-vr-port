//! load_data.lua and load_view.lua: LOAD GAME -- the engine's own save enumeration, metadata and thumbnails (a request
//! handler with a listener of our own, tagged so the game's LoadGameMenuGameController hooks in the Lua host pass its
//! callbacks here: VRMenuLoadNames, VRMenuHook 16), the saves grouped by playthrough as the game groups them, a
//! confirmation (and the modded-save warning), the slot validated again just before the engine loads it.
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, A};
use super::menu_core::*;
use super::menu_gameplay as gp;
use super::menu_gpdata as gd;
use super::menu_ink as k;
use super::menu_locale as loc;
use super::menu_rt::*;
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const MARKER: f64 = -7392000.0;
const W: f64 = 1780.0;
const H: f64 = 1305.0;
const ATLAS: &str = "base\\gameplay\\gui\\fullscreen\\load_game\\save_game.inkatlas";

#[derive(Clone)]
struct Info {
    h: Handle,
    index: i64,
    save_id: f64,
    stamp: u64,
    valid: bool,
    ep1: bool,
    playthrough: String,
    life: f64,
    level: f64,
    seconds: f64,
    quest: String,
    internal: String,
    modded: bool,
    version: String,
    location: String,
}
fn info_of(h: &Handle) -> Info {
    Info {
        index: pnum(h, "saveIndex") as i64,
        save_id: pnum(h, "saveID"),
        stamp: pf(h, "timestamp").map_or(0, |v| unsafe { *(v.ptr as *const u64) }),
        valid: pf(h, "isValid").is_some_and(|v| v.bool()),
        ep1: b(h, "IsEp1Save", &[]),
        playthrough: ptext(h, "playthroughID"),
        life: pnum(h, "lifePath"),
        level: pnum(h, "level"),
        seconds: pnum(h, "playTime").max(pnum(h, "playthroughTime")),
        quest: ptext(h, "trackedQuest"),
        internal: ptext(h, "internalName"),
        modded: pf(h, "isModded").is_some_and(|v| v.bool()),
        version: ptext(h, "gameVersion"),
        location: ptext(h, "locationName"),
        h: h.clone(),
    }
}
fn ep1() -> bool {
    rt::global_call("IsEP1", &[]).is_some_and(|o| rt::Val::of(&o).bool())
}
fn available(i: &Info) -> bool {
    i.valid && (ep1() || !i.ep1)
}
struct Pending {
    file: String,
    id: f64,
    stamp: u64,
    index: Option<i64>,
}
struct Ctx {
    handler: Handle,
    listener: Handle,
    marker: f64,
    records: HashMap<i64, Info>,
    names: Vec<String>,
    images: HashMap<i64, Handle>,
    revision: u64,
    received: usize,
    total: usize,
    loading: bool,
    timed_out: bool,
    error: bool,
    pending: Option<Pending>,
    ready_load: Option<i64>,
    closed: bool,
    _validation: Handle,
}
thread_local! {
    static LOAD: RefCell<Option<Ctx>> = const { RefCell::new(None) };
    static EPOCH: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}
fn with<R>(f: impl FnOnce(&mut Ctx) -> R) -> Option<R> {
    LOAD.with(|l| l.borrow_mut().as_mut().map(f))
}
fn open() {
    let epoch = EPOCH.with(|e| {
        e.set(e.get() + 1.0);
        e.get()
    });
    let handler = super::requests_handler().unwrap_or_else(|_| Handle::null());
    let listener = ink::new("LoadGameMenuGameController");
    let marker = MARKER - epoch;
    hset(&listener, "sourceIndex", marker);
    let mut registered = 0;
    for cb in ["OnSavesForLoadReady", "OnSaveMetadataReady"] {
        registered += rt::call(&handler, "RegisterToCallback", &[A::Name(cname(cb)), A::H(&listener), A::Name(cname(cb))]).is_some() as i32;
    }
    cvr_host::log!(
        "[VRMenu] load: the saves asked for, marker {}, {registered} callback(s) registered (from Rust)\n",
        rt::field(&listener, "sourceIndex").and_then(|v| v.num()).map_or("none".to_string(), |v| v.to_string())
    );
    LOAD.with(|l| {
        *l.borrow_mut() = Some(Ctx { handler: handler.clone(), listener, marker, records: HashMap::new(), names: Vec::new(), images: HashMap::new(), revision: 0, received: 0, total: 0, loading: true, timed_out: false, error: false, pending: None, ready_load: None, closed: false, _validation: Handle::null() })
    });
    rt::call(&handler, "RequestSavesForLoad", &[]);
}
fn refresh() {
    let h = with(|c| {
        if c.loading || c.pending.is_some() {
            return None;
        }
        (c.loading, c.error, c.timed_out) = (true, false, false);
        Some(c.handler.clone())
    })
    .flatten();
    if let Some(h) = h {
        rt::call(&h, "RequestSavesForLoad", &[]);
    }
}
/// The listener's OnSavesForLoadReady (the Lua host's hook, its names joined by newlines).
pub(crate) fn names_ready(marker: f64, joined: &str) -> bool {
    let names: Vec<String> = if joined.is_empty() { Vec::new() } else { joined.split('\n').map(str::to_string).collect() };
    cvr_host::log!("[VRMenu] load: {} save name(s) from the game (from Rust)\n", names.len());
    let Some((handler, pending)) = with(|c| {
        if c.marker != marker {
            return None;
        }
        c.names = names.clone();
        c.revision += 1;
        Some((c.handler.clone(), c.pending.as_ref().map(|p| p.file.clone())))
    })
    .flatten() else { return false };
    if let Some(file) = pending {
        match names.iter().position(|n| *n == file) {
            None => {
                with(|c| {
                    c.error = true;
                    c.pending = None;
                });
            }
            Some(i) => {
                let img = ink::new("inkImageWidget");
                with(|c| {
                    if let Some(p) = c.pending.as_mut() {
                        p.index = Some(i as i64);
                    }
                    c._validation = img.clone();
                });
                rt::call(&handler, "RequestSavedGameScreenshot", &[A::N(i as f64), A::H(&img)]);
            }
        }
        return true;
    }
    let mut images = HashMap::new();
    for i in 0..names.len() {
        let img = ink::new("inkImageWidget");
        rt::call(&handler, "RequestSavedGameScreenshot", &[A::N(i as f64), A::H(&img)]);
        images.insert(i as i64, img);
    }
    with(|c| {
        (c.records, c.images, c.received, c.total, c.loading) = (HashMap::new(), images, 0, names.len(), !names.is_empty());
    });
    true
}
/// The listener's OnSaveMetadataReady.
pub(crate) fn metadata_ready(listener: &Handle, info: &Handle) {
    let marker = pnum(listener, "sourceIndex");
    let i = info_of(info);
    with(|c| {
        if c.marker != marker {
            return;
        }
        if let Some(p) = c.pending.as_ref() {
            if Some(i.index) != p.index {
                return;
            }
            if i.save_id != p.id || i.stamp != p.stamp || !available(&i) {
                c.error = true;
                c.pending = None;
            } else {
                c.ready_load = p.index;
            }
            c.revision += 1;
            return;
        }
        if c.names.get(i.index as usize).is_none() {
            return;
        }
        if !c.records.contains_key(&i.index) {
            c.received += 1;
        }
        c.records.insert(i.index, i);
        c.loading = c.received < c.total;
        c.revision += 1;
    });
}
struct Group {
    id: String,
    life: f64,
    level: f64,
    seconds: f64,
    stamp: u64,
    saves: Vec<Info>,
}
fn groups() -> Vec<Group> {
    let records: Vec<Info> = with(|c| (0..c.total as i64).filter_map(|i| c.records.get(&i).cloned()).collect()).unwrap_or_default();
    let mut out: Vec<Group> = Vec::new();
    for i in records {
        let id = if i.playthrough.is_empty() { format!("unknown:{}", gd::fmt_g(i.save_id)) } else { i.playthrough.clone() };
        match out.iter_mut().find(|g| g.id == id) {
            Some(g) => {
                g.stamp = g.stamp.max(i.stamp);
                g.saves.push(i);
            }
            None => out.push(Group { id, life: i.life, level: i.level, seconds: i.seconds, stamp: i.stamp, saves: vec![i] }),
        }
    }
    for g in out.iter_mut() {
        g.saves.sort_by(|a, b| b.stamp.cmp(&a.stamp).then(a.index.cmp(&b.index)));
        let latest = &g.saves[0];
        (g.seconds, g.level, g.life) = (latest.seconds, latest.level, latest.life);
    }
    out.sort_by(|a, b| b.stamp.cmp(&a.stamp).then_with(|| a.id.cmp(&b.id)));
    out
}
fn begin_load(i: &Info) -> bool {
    let h = with(|c| {
        if c.pending.is_some() || c.loading || !available(i) {
            return None;
        }
        let file = c.names.get(i.index as usize).cloned().unwrap_or_default();
        c.pending = Some(Pending { file, id: i.save_id, stamp: i.stamp, index: None });
        c.error = false;
        Some(c.handler.clone())
    })
    .flatten();
    match h {
        Some(h) => {
            rt::call(&h, "RequestSavesForLoad", &[]);
            true
        }
        None => false,
    }
}
fn close() {
    let c = LOAD.with(|l| l.borrow_mut().take());
    if let Some(c) = c.filter(|c| !c.closed) {
        for cb in ["OnSavesForLoadReady", "OnSaveMetadataReady"] {
            rt::call(&c.handler, "UnregisterFromCallback", &[A::Name(cname(cb)), A::H(&c.listener), A::Name(cname(cb))]);
        }
        rt::call(&c.handler, "CancelSavesRequest", &[]);
        rt::call(&c.handler, "CancelSavedGameScreenshotRequests", &[]);
    }
}

// ---------------------------------------------------------------- the view
#[derive(Default)]
pub(super) struct LoadState {
    open: bool,
    expanded: HashMap<String, bool>,
    seen_revision: Option<u64>,
    started: f64,
    initial_group: bool,
    selection: Option<i64>,
    confirm: Option<&'static str>,
    load_at: Option<f64>,
    draw_at: f64,
    offset: Option<f64>,
}
fn tr(ru: &str, en: &str) -> String {
    loc::tr(ru, en)
}
fn save_count(n: usize) -> String {
    loc::count(n as i64, "сохранение", "сохранения", "сохранений", "save", "saves")
}
fn life(v: f64) -> (String, Option<&'static str>) {
    if v == ev("inkLifePath", "Nomad") {
        (tr("КОЧЕВНИК", "NOMAD"), Some("LifepathNomad1"))
    } else if v == ev("inkLifePath", "Corporate") {
        (tr("КОРПОРАТ", "CORPO"), Some("LifepathCorpo1"))
    } else if v == ev("inkLifePath", "StreetKid") {
        (tr("ДИТЯ УЛИЦ", "STREETKID"), Some("LifepathStreetKid1"))
    } else {
        (tr("ПРОХОЖДЕНИЕ", "PLAYTHROUGH"), None)
    }
}
fn duration(seconds: f64) -> String {
    let minutes = (seconds.max(0.0) / 60.0).floor() as i64;
    let hours = minutes / 60;
    if hours > 0 {
        format!("{hours}{}{}{}", tr(" ч. ", " h "), minutes % 60, tr(" мин.", " min"))
    } else {
        format!("{minutes}{}", tr(" мин.", " min"))
    }
}
fn date(p: &Handle, x: f64, y: f64, w: f64, h: f64, stamp: u64, size: f64) -> Handle {
    let label = k::text(p, x, y, w, h, "", size, k::CYAN, "right", false);
    ink::call(&label, "SetDateTimeByTimestamp", &[Value::U64(stamp)]);
    label
}
fn preview(p: &Handle, index: i64, x: f64, y: f64, w: f64, h: f64) {
    k::round_fill(p, x, y, w, h, k::BLACK, 0.8, Some(5.0));
    let Some(img) = with(|c| c.images.get(&index).cloned()).flatten() else { return };
    if let Some(v) = crate::script_call::enum_value("inkEAnchor", "TopLeft") {
        ink::call(&img, "SetAnchor", &[Value::U64(v as u64)]);
    }
    ink::v2(&img, "SetAnchorPoint", 0.0, 0.0);
    ink::v2(&img, "SetRenderTransformPivot", 0.0, 0.0);
    ink::v2(&img, "SetScale", 1.0, 1.0);
    ink::reparent(&img, p);
    ink::move_to(&img, x, y);
    ink::size(&img, w, h);
    ink::opacity(&img, 1.0);
    ink::visible(&img, true);
}
#[allow(clippy::too_many_arguments)]
fn action(m: &mut Menu, p: &Handle, x: f64, y: f64, w: f64, ru: &str, en: &str, f: Act, enabled: bool, key: &str) -> BtnRef {
    let b = gp::hub_action(m, p, P1X, x, y, w, &tr(ru, en), f, enabled, None, None);
    *b.key.borrow_mut() = Some(key.to_string());
    b
}
fn confirm(m: &mut Menu, p: &Handle, info: &Info) {
    preview(p, info.index, 80.0, 135.0, 400.0, 225.0);
    k::text(p, 520.0, 155.0, 1150.0, 60.0, &k::clip(&loc_of(&info.quest), 62), 38.0, k::WHITE, "left", false);
    k::text(p, 520.0, 230.0, 1100.0, 50.0, &format!("{}  /  {}", info.internal, duration(info.seconds)), 29.0, k::CYAN, "left", false);
    date(p, 520.0, 295.0, 1100.0, 44.0, info.stamp, 27.0);
    let mods = m.gp.load.confirm == Some("mods");
    let heading = if mods { tr("ПРОВЕРКА МОДИФИКАЦИЙ", "MOD VERIFICATION") } else { tr("ЗАГРУЗИТЬ ЭТО СОХРАНЕНИЕ?", "LOAD THIS SAVE?") };
    k::round_fill(p, 80.0, 425.0, W - 160.0, 520.0, k::BLACK, 1.0, Some(16.0));
    k::cut_frame(&mut m.pool, p, 80.0, 425.0, W - 160.0, 520.0, k::YELLOW, 0.8, Some(20.0), Some(2.0));
    k::text(p, 125.0, 460.0, W - 250.0, 65.0, &heading, 40.0, k::YELLOW, "left", false);
    k::rect(p, 125.0, 545.0, W - 250.0, 2.0, k::CYAN, 0.5);
    let message = if mods {
        tr("Этот файл сохранения создан в модифицированной версии игры.\nЕго загрузка может привести к ошибкам в этой системе.\nПродолжить?", "This save was created in a modified version of the game.\nLoading it may cause errors on this system.\nContinue?")
    } else {
        tr("Несохранённый прогресс будет потерян.", "Unsaved progress will be lost.")
    };
    k::text(p, 125.0, 590.0, W - 250.0, 215.0, &message, 35.0, k::WHITE, "left", true);
    if with(|c| c.pending.is_some()).unwrap_or(false) {
        k::text(p, 125.0, 840.0, W - 250.0, 65.0, &tr("Проверка сохранения…", "Checking save…"), 32.0, k::CYAN, "center", false);
    } else {
        let i2 = info.clone();
        let loading = with(|c| c.loading).unwrap_or(true);
        action(
            m,
            p,
            740.0,
            840.0,
            440.0,
            "ПОДТВЕРДИТЬ",
            "CONFIRM",
            Rc::new(move |m: &mut Menu, _| {
                if !mods && i2.modded {
                    m.gp.load.confirm = Some("mods");
                    gp::redraw(m);
                } else if begin_load(&i2) {
                    m.gp.load.load_at = Some(m.time);
                    gp::redraw(m);
                } else {
                    let t = m.tx("blocked");
                    toast(m, &t);
                }
            }),
            available(info) && !loading,
            "loadConfirm",
        );
        action(
            m,
            p,
            1210.0,
            840.0,
            440.0,
            "ОТМЕНА",
            "CANCEL",
            Rc::new(|m: &mut Menu, _| {
                (m.gp.load.confirm, m.gp.load.selection) = (None, None);
                gp::redraw(m);
            }),
            true,
            "loadCancel",
        );
    }
}
fn loc_of(s: &str) -> String {
    super::menu_rt::loc(s)
}
pub(super) fn draw(m: &mut Menu, p: &Handle, _p2: &Handle) {
    if !m.gp.load.open || LOAD.with(|l| l.borrow().is_none()) {
        m.gp.load = LoadState { open: true, started: m.time, ..Default::default() };
        open();
    }
    glass(m, p, W, H, Some((P1X, P1Y)));
    gp::title(p, &tr("ЗАГРУЗИТЬ ИГРУ", "LOAD GAME"), W);
    let (loading, pending) = with(|c| (c.loading, c.pending.is_some())).unwrap_or((false, false));
    let refresh_b = gp::head_action(
        m,
        p,
        P1X,
        W - 72.0,
        &tr("ОБНОВИТЬ", "REFRESH"),
        Rc::new(|m: &mut Menu, _| {
            (m.gp.load.confirm, m.gp.load.selection, m.gp.load.started) = (None, None, m.time);
            refresh();
            gp::redraw(m);
        }),
        !loading && !pending,
        None,
    );
    *refresh_b.key.borrow_mut() = Some("loadRefresh".into());
    let b = gp::head_action(m, p, P1X, refresh_b.x.get() - P1X - 12.0, &tr("МЕНЮ", "MENU"), Rc::new(|m: &mut Menu, _| gp::open_section(m, "game")), !pending, None);
    *b.key.borrow_mut() = Some("loadBack".into());
    let info = m.gp.load.selection.and_then(|i| with(|c| c.records.get(&i).cloned()).flatten());
    if let (Some(_), Some(info)) = (m.gp.load.confirm, info) {
        return confirm(m, p, &info);
    }
    let groups = groups();
    if !m.gp.load.initial_group && !loading && !groups.is_empty() {
        m.gp.load.expanded.insert(groups[0].id.clone(), true);
        m.gp.load.initial_group = true;
    }
    let mut rows = Vec::new();
    for g in &groups {
        let opened = m.gp.load.expanded.get(&g.id) == Some(&true);
        let (gid, life_v, n_saves, seconds, level, stamp) = (g.id.clone(), g.life, g.saves.len(), g.seconds, g.level, g.stamp);
        rows.push(Row {
            h: 130.0,
            build: Rc::new(move |m: &mut Menu, row: &Handle, it: RowCtx| {
                let w = it.cw - 8.0;
                let (name, part) = life(life_v);
                let bg = k::round_fill(row, 4.0, 6.0, w, 112.0, k::ICE, 0.08, Some(9.0));
                let edge = k::round_edge(row, 4.0, 6.0, w, 112.0, k::CYAN, 0.45, Some(9.0));
                if let Some(part) = part {
                    k::image(row, 20.0, 18.0, 84.0, 84.0, ATLAS, part, Some(k::CYAN), 0.8);
                }
                k::text(row, 120.0, 18.0, 385.0, 48.0, &name, 33.0, k::WHITE, "left", false);
                k::text(row, 120.0, 69.0, 350.0, 35.0, &save_count(n_saves), 23.0, k::GREY, "left", false);
                k::text(row, 525.0, 20.0, w - 620.0, 42.0, &format!("{}{}    {}{}", tr("Время в игре ", "Play time "), duration(seconds), tr("Макс. уровень ", "Max level "), level.floor() as i64), 28.0, k::CYAN, "left", false);
                k::text(row, 525.0, 69.0, 410.0, 35.0, &tr("Последнее сохранение", "Last saved"), 24.0, k::GREY, "left", false);
                date(row, w - 605.0, 69.0, 485.0, 35.0, stamp, 24.0);
                chevron(row, w - 45.0, 60.0, 14.0, 3.0, if opened { "down" } else { "up" }, k::CYAN, 1.0);
                let g2 = gid.clone();
                let b = button(
                    m,
                    "p1",
                    &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                    4.0,
                    6.0,
                    w,
                    112.0,
                    Some(Rc::new(move |m: &mut Menu, _| {
                        m.gp.load.expanded.insert(g2.clone(), !opened);
                        gp::redraw(m);
                    })),
                    Some(Rc::new(move |h, _| {
                        ink::opacity(&bg, if h { 0.17 } else { 0.08 });
                        k::tint(&edge, k::CYAN, Some(if h { 1.0 } else { 0.45 }));
                    })),
                );
                *b.key.borrow_mut() = Some(format!("loadGroup:{gid}"));
            }),
        });
        if opened {
            for save in g.saves.clone() {
                rows.push(Row {
                    h: 190.0,
                    build: Rc::new(move |m: &mut Menu, row: &Handle, it: RowCtx| {
                        let w = it.cw - 28.0;
                        let avail = available(&save);
                        let bg = k::round_fill(row, 20.0, 5.0, w, 177.0, k::ICE, 0.035, Some(8.0));
                        let edge = k::round_edge(row, 20.0, 5.0, w, 177.0, if avail { k::CYAN } else { k::GREY }, 0.45, Some(8.0));
                        preview(row, save.index, 28.0, 14.0, 260.0, 146.0);
                        let mut title = if save.valid { loc_of(&save.quest) } else { tr("Повреждённое сохранение", "Corrupted save") };
                        if save.valid && !avail {
                            title = tr("Требуется Phantom Liberty", "Phantom Liberty required");
                        }
                        k::text(row, 310.0, 16.0, w - 675.0, 48.0, &k::clip(&title, 58), 34.0, if avail { k::WHITE } else { k::GREY }, "left", false);
                        if save.modded {
                            if let Some(g) = glyph_of("cog") {
                                glyph(row, w - 320.0, 40.0, 54.0, g, k::CYAN, 1.0);
                            }
                        }
                        k::text(row, w - 285.0, 20.0, 275.0, 38.0, &format!("{}{}", tr("Версия ", "Version "), save.version), 23.0, k::GREY, "right", false);
                        let (path, _) = life(save.life);
                        k::text(row, 310.0, 79.0, w - 330.0, 40.0, &format!("{}   /   {}   /   {}{}   /   {}", duration(save.seconds), path, tr("Уровень ", "Level "), save.level.floor() as i64, k::clip(&loc_of(&save.location), 40)), 25.0, k::CYAN, "left", false);
                        k::text(row, 310.0, 130.0, 760.0, 32.0, &save.internal, 24.0, k::GREY, "left", false);
                        date(row, w - 490.0, 130.0, 480.0, 32.0, save.stamp, 24.0);
                        let idx = save.index;
                        let b = button(
                            m,
                            "p1",
                            &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                            20.0,
                            5.0,
                            w,
                            177.0,
                            Some(Rc::new(move |m: &mut Menu, _| {
                                if !avail || with(|c| c.loading).unwrap_or(true) {
                                    let t = m.tx("blocked");
                                    toast(m, &t);
                                    return;
                                }
                                (m.gp.load.selection, m.gp.load.confirm) = (Some(idx), Some("load"));
                                gp::redraw(m);
                            })),
                            Some(Rc::new(move |h, _| {
                                ink::opacity(&bg, if h { 0.14 } else { 0.035 });
                                k::tint(&edge, if h || avail { k::CYAN } else { k::GREY }, Some(if h { 1.0 } else { 0.45 }));
                            })),
                        );
                        *b.key.borrow_mut() = Some(format!("loadSave:{}", gd::fmt_g(save.save_id)));
                    }),
                });
            }
        }
    }
    let (timed_out, received) = with(|c| (c.timed_out, c.received)).unwrap_or((false, 0));
    if !rows.is_empty() {
        let keep = m.gp.load.offset;
        let v = gp::glist(m, p, "p1", "p1", P1X, 115.0, W, rows, 1050.0, keep);
        v.borrow_mut().enable_mask();
    } else if !loading {
        let t = if timed_out { tr("Не удалось прочитать сохранения. Нажмите «Обновить».", "Could not read saves. Select Refresh.") } else { tr("Нет сохранений", "No saves found") };
        k::text(p, 30.0, 450.0, W - 60.0, 120.0, &t, 36.0, k::CYAN, "center", false);
    }
    if !(loading && groups.is_empty()) {
        let t = if timed_out && !loading {
            tr("Не все данные получены. Нажмите «Обновить».", "Some data is missing. Select Refresh.")
        } else {
            format!("{}  /  {}", loc::count(groups.len() as i64, "прохождение", "прохождения", "прохождений", "playthrough", "playthroughs"), save_count(received))
        };
        k::text(p, 24.0, 1232.0, W - 48.0, 45.0, &t, 26.0, k::GREY, "left", false);
    }
}
pub(super) fn update(m: &mut Menu) {
    if !m.gp.load.open {
        return;
    }
    let ready = with(|c| c.ready_load.take().map(|i| (i, c.handler.clone()))).flatten();
    if let Some((index, handler)) = ready {
        super::menu_main::close(m);
        rt::call(&handler, "LoadSavedGame", &[A::N(index as f64)]);
        return;
    }
    if with(|c| std::mem::replace(&mut c.error, false)).unwrap_or(false) {
        with(|c| c.pending = None);
        (m.gp.load.confirm, m.gp.load.selection, m.gp.load.started) = (None, None, m.time);
        let t = tr("Список сохранений изменился. Выберите сохранение снова.", "Saves changed. Select the save again.");
        toast(m, &t);
        refresh();
    }
    if with(|c| c.pending.is_some()).unwrap_or(false) && m.time - m.gp.load.load_at.unwrap_or(m.time) > 12.0 {
        with(|c| c.pending = None);
        (m.gp.load.confirm, m.gp.load.selection, m.gp.load.started) = (None, None, m.time);
        let t = tr("Не удалось проверить сохранение", "Could not verify the save");
        toast(m, &t);
        refresh();
    }
    let started = m.gp.load.started;
    with(|c| {
        if c.loading && m.time - started > 15.0 {
            (c.loading, c.timed_out) = (false, true);
            c.revision += 1;
        }
    });
    let rev = with(|c| c.revision);
    if rev != m.gp.load.seen_revision && m.time - m.gp.load.draw_at > 0.2 {
        m.gp.load.seen_revision = rev;
        m.gp.load.draw_at = m.time;
        gp::redraw(m);
    }
}
pub(super) fn before_build(m: &mut Menu) {
    if m.gp.load.open {
        let off = m.view("p1").map(|v| v.v.borrow().offset);
        if off.is_some() {
            m.gp.load.offset = off;
        }
    }
}
pub(super) fn leave(m: &mut Menu) {
    if m.gp.load.open {
        close();
        m.gp.load = LoadState::default();
    }
}
