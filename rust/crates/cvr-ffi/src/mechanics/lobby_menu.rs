//! modules/mainmenu.lua, gamesettings.lua and vrsettings.lua: the lobby's own main menu on the holo panels -- Continue
//! (the newest save, or the engram's), New game (the creation in the lobby), Load game (playthroughs, newest first,
//! opened to their saves, a mod-verification page for a modded save), Settings (the plugin's VR pages and the game's
//! own groups: booleans and lists as choosers, numbers as sliders, Apply while changes wait), Quit (confirmed). The
//! game's main menu controller and its list of saves come from the Lua host's observers (VRLobbyMenu, VRLobbySaves).
use super::lobby::Lobby;
use super::lobby_holo::{self as holo, FootButton, Page, PageRef, Row, Slider};
use super::lobby_ink as ink;
use super::lobby_spawn::native_string;
use super::menu_settings as vrs;
use crate::script_call::{method_owned, Value};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub(super) struct Texts {
    pub vr: &'static str,
    pub cont: &'static str,
    pub new: &'static str,
    pub load: &'static str,
    pub settings: &'static str,
    pub quit: &'static str,
    pub back: &'static str,
    pub quit_title: &'static str,
    pub quit_sub: &'static str,
    pub yes: &'static str,
    pub no: &'static str,
    pub saves: &'static str,
    pub saves_sub: &'static str,
    pub level: &'static str,
    pub none: &'static str,
    pub mod_title: &'static str,
    pub mod1: &'static str,
    pub mod2: &'static str,
    pub mod3: &'static str,
    pub confirm: &'static str,
    pub cancel: &'static str,
    pub on: &'static str,
    pub off: &'static str,
    pub apply: &'static str,
    pub game: &'static str,
    pub pending: &'static str,
    pub restart: &'static str,
    pub reading: &'static str,
    pub vr_missing: &'static str,
    pub life: [(&'static str, &'static str); 4],
}
const EN: Texts = Texts {
    vr: "VR settings",
    cont: "Continue",
    new: "New game",
    load: "Load game",
    settings: "Settings",
    quit: "Quit game",
    back: "Back",
    quit_title: "Quit",
    quit_sub: "Close the game?",
    yes: "Quit",
    no: "Stay",
    saves: "Saves",
    saves_sub: "Playthroughs, newest first",
    level: "LVL",
    none: "No saves",
    mod_title: "Mod Verification",
    mod1: "This save file was made on a version of the game with mods.",
    mod2: "Loading it on this system may cause unexpected behavior in the game.",
    mod3: "Are you sure you want to continue?",
    confirm: "Confirm",
    cancel: "Cancel",
    on: "On",
    off: "Off",
    apply: "Apply",
    game: "Game",
    pending: "Changes waiting to be applied",
    restart: "Some changes take effect after the game restarts",
    reading: "Reading the settings...",
    vr_missing: "The VR menu's mod is not loaded",
    life: [("Nomad", "Nomad"), ("StreetKid", "Street Kid"), ("Corporate", "Corpo"), ("Count", "Phantom Liberty")],
};
const RU: Texts = Texts {
    vr: "Настройки VR",
    cont: "Продолжить",
    new: "Новая игра",
    load: "Загрузить игру",
    settings: "Настройки",
    quit: "Выход",
    back: "Назад",
    quit_title: "Выход",
    quit_sub: "Закрыть игру?",
    yes: "Выйти",
    no: "Остаться",
    saves: "Сохранения",
    saves_sub: "Прохождения, новые первыми",
    level: "УР.",
    none: "Нет сохранений",
    mod_title: "Проверка модификаций",
    mod1: "Этот файл сохранения создан в модифицированной версии игры.",
    mod2: "Его загрузка может привести к ошибкам в этой системе.",
    mod3: "Продолжить?",
    confirm: "Подтвердить",
    cancel: "Отменить",
    on: "Вкл.",
    off: "Выкл.",
    apply: "Применить",
    game: "Игра",
    pending: "Изменения ждут применения",
    restart: "Часть изменений вступит в силу после перезапуска игры",
    reading: "Чтение настроек...",
    vr_missing: "Мод VR-меню не загружен",
    life: [("Nomad", "Кочевник"), ("StreetKid", "Дитя улиц"), ("Corporate", "Корпорат"), ("Count", "Призрачная свобода")],
};
impl Texts {
    pub(super) fn life_name(&self, life: &str) -> String {
        self.life.iter().find(|(k, _)| *k == life).map_or_else(|| life.to_string(), |(_, v)| (*v).to_string())
    }
}
pub(super) fn lang() -> &'static str {
    if ink::language().starts_with("ru") {
        "ru"
    } else {
        "en"
    }
}
pub(super) fn texts() -> &'static Texts {
    if lang() == "ru" {
        &RU
    } else {
        &EN
    }
}
/// label: the game's localized text for the key, or the fallback.
pub(super) fn label(key: &str, fallback: &str) -> String {
    match super::lobby_spawn::global_text("GetLocalizedText", key) {
        Some(t) if !t.is_empty() && t != key => t,
        _ => fallback.to_string(),
    }
}
/// GetLocalizedTextByKey of a name by its hash, as the Lua handed the CName over (a name made from a TweakDB string
/// has no text in the name pool: by its text it was "").
fn text_by_key(key: u64) -> String {
    super::menu_data::loc_name(key)
}
/// activate: the game's main menu controller handles a menu action (PauseMenuAction, an event name).
pub(super) fn activate(l: &Lobby, action: &str, event: Option<&str>) -> bool {
    let ctrl = &l.shared.main_menu;
    if ctrl.is_null() {
        return false;
    }
    let data = ink::new("PauseMenuListItemData");
    if data.is_null() {
        return false;
    }
    // SAFETY: the new object's enum and CName properties.
    unsafe {
        if let (Some(p), Some(v)) = (crate::script_call::property(data.instance, "action"), crate::script_call::enum_value("PauseMenuAction", action)) {
            *(p as *mut i32) = v as i32;
        }
        if let (Some(p), Some(e)) = (crate::script_call::property(data.instance, "eventName"), event) {
            *(p as *mut u64) = CName::hashed(e).0;
        }
    }
    ink::call(ctrl, "HandleMenuItemActivate", &[Value::Handle(&data)]);
    true
}

// ---------------------------------------------------------------- the saves
#[derive(Clone)]
pub(super) struct Save {
    pub folder: String,
    pub playthrough: String,
    pub life: String,
    pub gender: String,
    pub level: f64,
    pub time: f64,
    pub location: String,
    pub quest: String,
    pub played: f64,
    pub modded: bool,
}
pub(super) struct Playthrough {
    pub id: String,
    pub saves: Vec<Rc<Save>>,
    pub newest: Rc<Save>,
}
#[derive(Default)]
pub(super) struct MenuState {
    cache: Option<(u64, Rc<Vec<Rc<Save>>>, Rc<Vec<Rc<Playthrough>>>)>,
    thumbs: HashMap<String, Handle>,
    pub last_error: Option<String>,
}
/// readSaves: the saves (VRLobbySaveLooks, newest first) and their playthroughs, cached until the menu's list changes.
pub(super) fn read_saves(l: &mut Lobby) -> (Rc<Vec<Rc<Save>>>, Rc<Vec<Rc<Playthrough>>>) {
    let gen = l.shared.names_gen;
    if let Some((g, s, o)) = &l.menu.cache {
        if *g == gen {
            return (s.clone(), o.clone());
        }
    }
    let mut saves = Vec::new();
    let mut order: Vec<(String, Vec<Rc<Save>>)> = Vec::new();
    let text = native_string("VRLobbySaveLooks", &[]).unwrap_or_default();
    for line in text.split('\n').filter(|s| !s.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.first() != Some(&"save") {
            continue;
        }
        let g = |i: usize| f.get(i).copied().unwrap_or("").to_string();
        let n = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let s = Rc::new(Save {
            folder: g(1),
            playthrough: g(2),
            life: g(3),
            gender: g(4),
            level: n(5),
            time: n(6),
            location: g(7),
            quest: g(8),
            played: n(9),
            modded: f.get(10).copied().unwrap_or("") != "0",
        });
        saves.push(s.clone());
        match order.iter_mut().find(|(id, _)| *id == s.playthrough) {
            Some((_, list)) => list.push(s),
            None => order.push((s.playthrough.clone(), vec![s])),
        }
    }
    let order: Vec<Rc<Playthrough>> = order.into_iter().map(|(id, list)| Rc::new(Playthrough { id, newest: list[0].clone(), saves: list })).collect();
    let (s, o) = (Rc::new(saves), Rc::new(order));
    l.menu.cache = Some((gen, s.clone(), o.clone()));
    (s, o)
}
#[repr(C)]
#[derive(Default)]
struct Tm {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
    isdst: i32,
}
extern "C" {
    fn _localtime64_s(tm: *mut Tm, t: *const i64) -> i32;
}
/// when: the save's time as dd.mm.yyyy hh:mm, local.
pub(super) fn when(t: f64) -> String {
    if t <= 0.0 {
        return String::new();
    }
    let mut tm = Tm::default();
    let secs = t as i64;
    // SAFETY: the CRT's conversion into our struct.
    if unsafe { _localtime64_s(&mut tm, &secs) } != 0 {
        return String::new();
    }
    format!("{:02}.{:02}.{:04} {:02}:{:02}", tm.mday, tm.mon + 1, tm.year + 1900, tm.hour, tm.min)
}
fn menu_index(l: &mut Lobby, folder: &str) -> Option<usize> {
    match l.shared.save_names.clone() {
        None => read_saves(l).0.iter().position(|s| s.folder == folder),
        Some(names) => names.iter().position(|n| n == folder),
    }
}
/// thumb: the save's screenshot into an image widget (asked of the game once per save and use).
pub(super) fn thumb(l: &mut Lobby, folder: &str, use_: &str) -> Option<Handle> {
    let key = format!("{folder}/{use_}");
    if let Some(img) = l.menu.thumbs.get(&key) {
        return Some(img.clone());
    }
    l.shared.save_names.as_ref()?;
    let index = menu_index(l, folder)?;
    let img = ink::new("inkImageWidget");
    if let Ok(rh) = super::requests_handler() {
        ink::call(&rh, "RequestSavedGameScreenshot", &[Value::Int32(index as i32), Value::Handle(&img)]);
    }
    l.menu.thumbs.insert(key, img.clone());
    Some(img)
}
pub(super) fn load_save(l: &mut Lobby, folder: &str) -> bool {
    let index = menu_index(l, folder);
    let ctrl = l.shared.main_menu.clone();
    let (false, Some(index)) = (ctrl.is_null(), index) else {
        l.menu.last_error = Some(format!("save {folder} is not in the menu's list"));
        return false;
    };
    ink::call(&ctrl, "LoadSaveInGame", &[Value::Int32(index as i32)]);
    true
}
fn verifies_mods() -> bool {
    let v = ink::setting_var("/gameplay/misc", "EnableModVerification");
    v.is_null() || ink::get::<bool>(&v, "GetValue", &[]) == Some(true)
}
pub(super) fn place_name(s: &Save) -> String {
    if s.location.is_empty() {
        return s.folder.clone();
    }
    let l = label(&s.location, "");
    if l.is_empty() {
        s.folder.clone()
    } else {
        l
    }
}
fn mod_check_page(s: Rc<Save>) -> PageRef {
    let t = texts();
    let sub = format!("{}  {}", place_name(&s), when(s.time));
    let mut p = Page::new(Some(label("LocKey#98280", t.mod_title)), "SYS//MODS", move |_l| {
        let t = texts();
        let s2 = s.clone();
        vec![
            Row { text: true, ..Row::new("m1", &label("LocKey#98259", t.mod1)) },
            Row { text: true, warn: true, ..Row::new("m2", &label("LocKey#98312", t.mod2)) },
            Row { text: true, ..Row::new("m3", &label("LocKey#98261", t.mod3)) },
            Row::new("cancel", &label("LocKey#22175", t.cancel)).go(|l, _| holo::back(l)),
            Row::new("confirm", &label("LocKey#23123", t.confirm)).go(move |l, _| {
                load_save(l, &s2.folder);
            }),
        ]
    });
    p.subtitle = Some(sub);
    Rc::new(p)
}
/// openSave: a modded save through the verification page when the game asks for it, else loaded.
pub(super) fn open_save(l: &mut Lobby, s: Rc<Save>) -> bool {
    if s.modded && verifies_mods() {
        holo::push(l, mod_check_page(s));
        return true;
    }
    load_save(l, &s.folder)
}
const LIFE_ATLAS: &str = "base\\gameplay\\gui\\fullscreen\\load_game\\save_game.inkatlas";
fn life_icon(life: &str) -> Option<&'static str> {
    match life {
        "Nomad" => Some("LifepathNomad1"),
        "Corporate" => Some("LifepathCorpo1"),
        "StreetKid" => Some("LifepathStreetKid1"),
        _ => None,
    }
}
fn load_page() -> PageRef {
    let t = texts();
    let open: Rc<RefCell<Option<HashMap<String, bool>>>> = Rc::new(RefCell::new(None));
    let mut p = Page::new(Some(t.saves.into()), "SAVE//INDEX", move |l| {
        let t = texts();
        let (_, order) = read_saves(l);
        if open.borrow().is_none() {
            if let Some(first) = order.first() {
                *open.borrow_mut() = Some(HashMap::from([(first.id.clone(), true)]));
            }
        }
        let mut rows = Vec::new();
        for (i, p) in order.iter().enumerate() {
            let i = i + 1;
            let s = p.newest.clone();
            let opened = open.borrow().as_ref().is_some_and(|m| m.get(&p.id) == Some(&true));
            let icon = life_icon(&s.life);
            let image = if icon.is_none() { thumb(l, &s.folder, "playthrough") } else { None };
            let (o2, pid) = (open.clone(), p.id.clone());
            rows.push(Row {
                detail: Some(format!("{} {}  {}  ·  {}", t.level, holo::fmt_num(s.level), when(s.time), p.saves.len())),
                sign: Some(if opened { "-" } else { "+" }.into()),
                icon: icon.map(|ic| (LIFE_ATLAS.to_string(), ic.to_string())),
                image,
                ..Row::new(&format!("p{i}"), &format!("{} · {}", t.life_name(&s.life), place_name(&s)))
            }
            .go(move |_, _| {
                let mut m = o2.borrow_mut();
                let map = m.get_or_insert_with(HashMap::new);
                map.insert(pid.clone(), !opened);
            }));
            if opened {
                for (j, save) in p.saves.iter().enumerate() {
                    let sv = save.clone();
                    let img = thumb(l, &save.folder, "");
                    rows.push(
                        Row { indent: 90.0, detail: Some(format!("{} {}  {}", t.level, holo::fmt_num(save.level), when(save.time))), image: img, ..Row::new(&format!("p{i}s{}", j + 1), &place_name(save)) }.go(move |l, _| {
                            open_save(l, sv.clone());
                        }),
                    );
                }
            }
        }
        if rows.is_empty() {
            rows.push(Row { disabled: true, ..Row::new("none", t.none) });
        }
        rows
    });
    p.subtitle = Some(t.saves_sub.into());
    Rc::new(p)
}

// ---------------------------------------------------------------- the game's settings (gamesettings.lua)
fn settings_system() -> Handle {
    super::game_system("GetSettingsSystem")
}
fn arr_handles(o: &Handle, name: &str, values: &[Value]) -> Vec<Handle> {
    if o.is_null() {
        return Vec::new();
    }
    // SAFETY: the object, held; an array of handles kept while read.
    unsafe { method_owned(o.instance, name, values) }.map(|ow| super::belt::handles(ow.as_ptr())).unwrap_or_default()
}
fn cname_text(n: Option<u64>) -> String {
    n.map(|n| CName(n).text().to_string()).unwrap_or_default()
}
fn var_kind(v: &Handle) -> String {
    ink::get::<i32>(v, "GetType", &[]).and_then(|t| crate::script_call::enum_name("ConfigVarType", i64::from(t))).unwrap_or_default()
}
fn list_values(v: &Handle, _kind: &str) -> Vec<String> {
    let Some(o) = super::lobby_rtti::call(v, "GetValues", &[]) else { return Vec::new() };
    let arr = super::lobby_rtti::Val::of(&o);
    (0..arr.len())
        .filter_map(|i| arr.at(i))
        .map(|e| match e.num() {
            Some(n) => holo::fmt_num(n),
            None => e.text(),
        })
        .collect()
}
fn list_value(v: &Handle, kind: &str, i: i32) -> String {
    if ink::get::<bool>(v, "ListHasDisplayValues", &[]) == Some(true) {
        return text_by_key(ink::get::<u64>(v, "GetDisplayValue", &[Value::Int32(i)]).unwrap_or(0));
    }
    let vals = list_values(v, kind);
    let Some(raw) = vals.get(i as usize) else { return String::new() };
    let s = raw.clone();
    let loc = label(&s, "");
    if loc.is_empty() {
        s
    } else {
        loc
    }
}
fn number(v: &Handle, value: f64) -> String {
    let step = ink::get::<f32>(v, "GetStepValue", &[]).map(f64::from).or_else(|| ink::get::<i32>(v, "GetStepValue", &[]).map(f64::from)).unwrap_or(1.0);
    let mut d = 0;
    while d < 3 && (step * 10f64.powi(d) - (step * 10f64.powi(d) + 0.5).floor()).abs() > 1e-4 {
        d += 1;
    }
    format!("{value:.*}", d as usize)
}
fn num_of(v: &Handle, name: &str, int: bool) -> f64 {
    if int {
        ink::get::<i32>(v, name, &[]).map_or(0.0, f64::from)
    } else {
        ink::get::<f32>(v, name, &[]).map_or(0.0, f64::from)
    }
}
fn var_row(v: Handle, key: String) -> Option<Row> {
    let t = texts();
    if ink::get::<bool>(&v, "IsVisible", &[]) == Some(false) {
        return None;
    }
    let disabled = ink::get::<bool>(&v, "IsDisabled", &[]) == Some(true);
    let kind = var_kind(&v);
    let mut row = Row { disabled, ..Row::new(&key, &text_by_key(ink::get::<u64>(&v, "GetDisplayName", &[]).unwrap_or(0))) };
    if kind.contains("Bool") {
        let on = ink::get::<bool>(&v, "GetValue", &[]) == Some(true);
        row.value = Some(if on { t.on } else { t.off }.into());
        let v2 = v.clone();
        let flip: holo::Act = Rc::new(move |_, _| {
            let cur = ink::get::<bool>(&v2, "GetValue", &[]) == Some(true);
            ink::call(&v2, "SetValue", &[Value::Bool(!cur)]);
        });
        (row.left, row.right) = (Some(flip.clone()), Some(flip));
    } else if kind.contains("List") {
        let n = list_values(&v, &kind).len() as i32;
        let i = ink::get::<i32>(&v, "GetIndex", &[]).unwrap_or(0);
        row.value = Some(list_value(&v, &kind, i));
        let (va, vb) = (v.clone(), v.clone());
        row.left = Some(Rc::new(move |_, _| {
            let i = ink::get::<i32>(&va, "GetIndex", &[]).unwrap_or(0);
            if i > 0 {
                ink::call(&va, "SetIndex", &[Value::Int32(i - 1)]);
            }
        }));
        row.right = Some(Rc::new(move |_, _| {
            let i = ink::get::<i32>(&vb, "GetIndex", &[]).unwrap_or(0);
            if i < n - 1 {
                ink::call(&vb, "SetIndex", &[Value::Int32(i + 1)]);
            }
        }));
    } else if kind.contains("Int") || kind.contains("Float") {
        let is_int = kind.contains("Int");
        let (vf, vs) = (v.clone(), v.clone());
        row.slider = Some(Slider {
            min: num_of(&v, "GetMinValue", is_int),
            max: num_of(&v, "GetMaxValue", is_int),
            step: Some(num_of(&v, "GetStepValue", is_int)),
            value: num_of(&v, "GetValue", is_int),
            format: Some(Rc::new(move |x| number(&vf, x))),
            set: Rc::new(move |_, x| {
                if is_int {
                    ink::call(&vs, "SetValue", &[Value::Int32((x + 0.5).floor() as i32)]);
                } else {
                    ink::call(&vs, "SetValue", &[Value::Float(x as f32)]);
                }
            }),
        });
    } else {
        return None;
    }
    Some(row)
}
fn pending_rows() -> Vec<Row> {
    let t = texts();
    let ss = settings_system();
    let mut rows = Vec::new();
    if ink::get::<bool>(&ss, "NeedsConfirmation", &[]) == Some(true) {
        let apply = label("UI-ResourceExports-Apply", t.apply);
        rows.push(Row { detail: Some(t.pending.into()), ..Row::new("__apply", &apply) }.go(|l, _| {
            let ss = settings_system();
            ink::call(&ss, "ConfirmChanges", &[]);
            if let Ok(rh) = super::requests_handler() {
                ink::call(&rh, "RequestSaveUserSettings", &[]);
            }
            holo::refresh(l);
        }));
    }
    if ink::get::<bool>(&ss, "NeedsRestartToApply", &[]) == Some(true) {
        rows.push(Row { text: true, warn: true, ..Row::new("__restart", t.restart) });
    }
    rows
}
fn leave() {
    let ss = settings_system();
    if ink::get::<bool>(&ss, "NeedsConfirmation", &[]) == Some(true) {
        ink::call(&ss, "RejectChanges", &[]);
    }
    if ink::get::<bool>(&ss, "WasModifiedSinceLastSave", &[]) == Some(true) {
        if let Ok(rh) = super::requests_handler() {
            ink::call(&rh, "RequestSaveUserSettings", &[]);
        }
    }
}
fn category_page(group: Handle) -> PageRef {
    let title = text_by_key(ink::get::<u64>(&group, "GetDisplayName", &[]).unwrap_or(0));
    let path = cname_text(ink::get::<u64>(&group, "GetPath", &[]));
    let code = format!("SYS//{}", path.get(1..).unwrap_or("").to_uppercase());
    let p = Page::new(Some(title), &code, move |_l| {
        let mut rows = pending_rows();
        let mut n = 0;
        let mut add = |v: Handle, rows: &mut Vec<Row>| {
            n += 1;
            if let Some(r) = var_row(v, format!("v{n}")) {
                rows.push(r);
            }
        };
        if ink::get::<bool>(&group, "HasVars", &[Value::Bool(true)]) == Some(true) {
            for v in arr_handles(&group, "GetVars", &[Value::Bool(true)]) {
                add(v, &mut rows);
            }
        }
        for (i, sub) in arr_handles(&group, "GetGroups", &[Value::Bool(true)]).into_iter().enumerate() {
            if ink::get::<bool>(&sub, "IsEmpty", &[Value::Bool(true)]) != Some(true) && ink::get::<bool>(&sub, "HasVars", &[Value::Bool(true)]) == Some(true) {
                rows.push(Row { header: true, ..Row::new(&format!("h{}", i + 1), &text_by_key(ink::get::<u64>(&sub, "GetDisplayName", &[]).unwrap_or(0))) });
                for v in arr_handles(&sub, "GetVars", &[Value::Bool(true)]) {
                    add(v, &mut rows);
                }
            }
        }
        rows
    });
    Rc::new(p)
}
fn game_rows() -> Vec<Row> {
    let ss = settings_system();
    let root = ink::child(&ss, "GetRootGroup", &[]);
    let mut rows = Vec::new();
    for (i, g) in arr_handles(&root, "GetGroups", &[Value::Bool(true)]).into_iter().enumerate() {
        let path = cname_text(ink::get::<u64>(&g, "GetPath", &[]));
        let mut shown = false;
        if path != "/key_bindings" {
            if ink::get::<bool>(&g, "HasVars", &[Value::Bool(true)]) == Some(true) {
                shown = true;
            }
            for sub in arr_handles(&g, "GetGroups", &[Value::Bool(true)]) {
                if ink::get::<bool>(&sub, "IsEmpty", &[Value::Bool(true)]) != Some(true) && ink::get::<bool>(&sub, "HasVars", &[Value::Bool(true)]) == Some(true) {
                    shown = true;
                }
            }
        }
        if shown {
            let label = text_by_key(ink::get::<u64>(&g, "GetDisplayName", &[]).unwrap_or(0));
            rows.push(Row::new(&format!("g{}", i + 1), &label).go(move |l, _| holo::push(l, category_page(g.clone()))));
        }
    }
    rows
}

// ---------------------------------------------------------------- the VR settings (vrsettings.lua)
const POLL: f64 = 0.25;
const LINE: usize = 64;
struct Section {
    open: HashMap<String, bool>,
    s: Option<String>,
    rec: Option<vrs::PageRec>,
    poll_at: f64,
}
fn is_open(sec: &Section, f: &vrs::Item) -> bool {
    sec.open.get(&f.id).copied().unwrap_or_else(|| vrs::has(f.flags, 8))
}
fn poll(sec: &mut Section, i: usize) -> bool {
    let Some(s) = vrs::fetch(i as i32 - 1) else { return false };
    if sec.s.as_deref() == Some(s.as_str()) {
        return false;
    }
    let Some(pg) = vrs::parse(&s).filter(|pg| pg.page == i as i32 - 1) else { return false };
    (sec.s, sec.rec) = (Some(s), Some(pg));
    true
}
fn item_rows(sec: &Rc<RefCell<Section>>, f: &vrs::Item, n: usize, rows: &mut Vec<Row>) {
    let t = texts();
    let key = format!("i{n}");
    let dim = vrs::has(f.flags, 16);
    let mk_set = |f: &vrs::Item| {
        let (f2, sec2) = (f.clone(), sec.clone());
        move |v: f64| {
            let mut s = sec2.borrow_mut();
            if let Some(rec) = s.rec.as_mut() {
                if let Some(it) = rec.items.iter_mut().find(|it| it.id == f2.id) {
                    it.value = v;
                }
            }
            vrs::edit(&f2, v);
        }
    };
    match f.kind.as_str() {
        "H" => {
            let open = is_open(&sec.borrow(), f);
            let (sec2, id) = (sec.clone(), f.id.clone());
            rows.push(Row { header: true, detail: Some(if open { "-" } else { "+" }.into()), ..Row::new(&key, &f.label) }.go(move |_, _| {
                sec2.borrow_mut().open.insert(id.clone(), !open);
            }));
        }
        "C" => {
            let set = Rc::new(mk_set(f));
            let (s1, s2, val) = (set.clone(), set, f.value);
            rows.push(Row {
                disabled: dim,
                value: Some(if f.value != 0.0 { t.on } else { t.off }.into()),
                left: Some(Rc::new(move |_, _| s1(if val != 0.0 { 0.0 } else { 1.0 }))),
                right: Some(Rc::new(move |_, _| s2(if val != 0.0 { 0.0 } else { 1.0 }))),
                ..Row::new(&key, &f.label)
            });
        }
        "S" | "I" => {
            let step = vrs::step_of(f);
            let lo = f.min.min(f.max);
            let set = mk_set(f);
            let (ff, fs) = (f.clone(), f.clone());
            rows.push(Row {
                disabled: dim,
                slider: Some(Slider {
                    min: f.min,
                    max: f.max,
                    step: Some(step),
                    value: f.value,
                    format: Some(Rc::new(move |v| vrs::fmt(&ff, v))),
                    set: Rc::new(move |_, v| set(vrs::snap(&fs, v, lo, step))),
                }),
                ..Row::new(&key, &f.label)
            });
        }
        "N" => {
            let step = if f.min != 0.0 { f.min } else { 1.0 };
            let set = Rc::new(mk_set(f));
            let (s1, s2, val) = (set.clone(), set, f.value);
            rows.push(Row {
                disabled: dim,
                value: Some(((f.value + 0.5).floor() as i64).to_string()),
                left: Some(Rc::new(move |_, _| s1(val - step))),
                right: Some(Rc::new(move |_, _| s2(val + step))),
                ..Row::new(&key, &f.label)
            });
        }
        "O" | "P" => {
            let opts = vrs::options(f);
            let idx = (f.value + 0.5).floor() as i64;
            let n_opts = opts.len() as i64;
            let set = Rc::new(mk_set(f));
            let (s1, s2) = (set.clone(), set);
            let pick = move |d: i64, s: &dyn Fn(f64)| {
                if n_opts > 0 {
                    s((idx + d).rem_euclid(n_opts) as f64);
                }
            };
            let pick2 = pick;
            rows.push(Row {
                disabled: dim,
                value: Some(opts.get(idx as usize).cloned().unwrap_or_else(|| "?".into())),
                left: Some(Rc::new(move |_, _| pick(-1, &*s1))),
                right: Some(Rc::new(move |_, _| pick2(1, &*s2))),
                ..Row::new(&key, &f.label)
            });
        }
        "B" => {
            let f2 = f.clone();
            rows.push(Row { disabled: dim, ..Row::new(&key, &f.label) }.go(move |_, _| vrs::edit(&f2, 1.0)));
        }
        "T" => {
            for (j, line) in vrs::wrap(&f.text, LINE).into_iter().enumerate() {
                rows.push(Row { text: true, ..Row::new(&format!("{key}.{}", j + 1), &line) });
            }
        }
        "G" | "K" => rows.push(Row { text: true, ..Row::new(&key, &format!("{}  {}", f.label, f.text)) }),
        _ => {}
    }
}
fn section_page(i: usize) -> PageRef {
    let sec = Rc::new(RefCell::new(Section { open: HashMap::new(), s: None, rec: None, poll_at: 0.0 }));
    let (s_rows, s_up) = (sec.clone(), sec);
    let mut p = Page::new(Some(vrs::page_name(i)), &format!("VR//{}", vrs::PAGES[i - 1]), move |_l| {
        let t = texts();
        if s_rows.borrow().rec.is_none() {
            poll(&mut s_rows.borrow_mut(), i);
        }
        let Some(rec) = s_rows.borrow().rec.clone() else { return vec![Row { text: true, ..Row::new("wait", t.reading) }] };
        let mut rows = Vec::new();
        let vis: Vec<vrs::Item> = {
            let s = s_rows.borrow();
            vrs::visible(&rec.items, &|f| is_open(&s, f)).into_iter().cloned().collect()
        };
        for (n, f) in vis.iter().enumerate() {
            item_rows(&s_rows, f, n + 1, &mut rows);
        }
        rows
    });
    p.update = Some(Rc::new(move |_l, now| {
        let mut s = s_up.borrow_mut();
        if now < s.poll_at {
            return false;
        }
        s.poll_at = now + POLL;
        poll(&mut s, i)
    }));
    Rc::new(p)
}
fn vr_rows() -> Vec<Row> {
    vrs::use_game_language();
    (1..=vrs::PAGES.len()).map(|i| Row::new(&format!("vr{i}"), &vrs::page_name(i)).go(move |l, _| holo::push(l, section_page(i)))).collect()
}
fn settings_page() -> PageRef {
    let t = texts();
    let mut p = Page::new(Some(label("UI-Labels-Settings", t.settings)), "SYS//SETTINGS", |_l| {
        let t = texts();
        let mut rows = pending_rows();
        rows.push(Row { header: true, ..Row::new("hvr", t.vr) });
        rows.extend(vr_rows());
        rows.push(Row { header: true, ..Row::new("hgame", t.game) });
        rows.extend(game_rows());
        rows
    });
    p.on_back = Some(Rc::new(|_| leave()));
    Rc::new(p)
}
fn quit_page() -> PageRef {
    let t = texts();
    let mut p = Page::new(Some(t.quit_title.into()), "SYS//EXIT", |_l| {
        let t = texts();
        vec![
            Row::new("no", t.no).go(|l, _| holo::back(l)),
            Row::new("yes", t.yes).go(|l, _| {
                activate(l, "ExitGame", None);
            }),
        ]
    });
    p.subtitle = Some(t.quit_sub.into());
    Rc::new(p)
}
/// M.root: the menu's first page.
pub(super) fn root() -> PageRef {
    let mut p = Page::new(None, "VR//LOBBY", |l| {
        let t = texts();
        let (saves, _) = read_saves(l);
        let mut newest = saves.first().cloned();
        if let Some(folder) = super::lobby_engrams::inserted_folder(l) {
            if let Some(s) = saves.iter().find(|s| s.folder == folder) {
                newest = Some(s.clone());
            }
        }
        let preview = newest.as_ref().and_then(|n| thumb(l, &n.folder.clone(), "continue"));
        let n2 = newest.clone();
        let mut rows = vec![
            Row {
                disabled: newest.is_none(),
                preview,
                preview_text: newest.as_ref().map(|n| format!("{} · {}", t.life_name(&n.life), place_name(n))),
                preview_detail: newest.as_ref().map(|n| format!("{} {}  {}", t.level, holo::fmt_num(n.level), when(n.time))),
                ..Row::new("continue", &label("UI-ScriptExports-Continue0", t.cont))
            }
            .go(move |l, _| {
                if let Some(n) = n2.clone() {
                    open_save(l, n);
                }
            }),
            Row::new("new", &label("UI-ScriptExports-NewGame0", t.new)).go(|l, _| super::lobby_creation::open(l)),
            Row { disabled: newest.is_none(), ..Row::new("load", &label("UI-ScriptExports-LoadGame0", t.load)) }.go(|l, _| holo::push(l, load_page())),
            Row::new("settings", &label("UI-Labels-Settings", t.settings)).go(|l, _| holo::push(l, settings_page())),
            Row::new("controls", &super::lobby_controls::title()).go(|l, _| holo::push(l, super::lobby_controls::page())),
            Row::new("quit", &label("UI-Labels-CloseGame", t.quit)).go(|l, _| holo::push(l, quit_page())),
        ];
        // a multiplayer mod in the game: its nickname, hosting and joining (the game's own menu items are hidden here)
        if super::lobby_coop::available() {
            let coop = Row::new("coop", if lang() == "ru" { "Кооператив" } else { "Co-op" }).go(|l, _| holo::push(l, super::lobby_coop::page()));
            rows.insert(3, coop);
        }
        rows
    });
    p.logo = true;
    p.footer = Some(Rc::new(|_l| {
        vec![FootButton {
            key: "ambient".into(),
            glyph: Rc::new(|parent, x, y, sz, col| ink::speaker(parent, x, y, sz, if super::lobby_spawn::Sound::ambient_on() { 3 } else { 0 }, col, 1.0)),
            go: Some(Rc::new(|_, _| super::lobby_spawn::Sound::set_ambient_on(!super::lobby_spawn::Sound::ambient_on()))),
        }]
    }));
    Rc::new(p)
}
