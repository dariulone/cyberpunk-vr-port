//! modules/creation.lua and figure.lua: a new game made in the lobby. The game's own character creation is driven
//! behind the lobby (its screens stepped through by their menu events: the expansion's start, the difficulty, the life
//! path, the body) while the holo shows the choices -- the life path's posters, then a window with the mode, the
//! difficulty, the body and voice, every appearance option of the game's screen (the figure turning to the face or the
//! hands for them), the attributes -- and a photo-mode figure of the player stands in the room wearing them
//! (VRCreationTarget). "Create and continue" finalizes the state and starts the game. The same steps put an engram's
//! saved look on the player (M.wear). A step that does not get its screen in 10 s fails the job.
use super::lobby::Lobby;
use super::lobby_hands::yaw_quat;
use super::lobby_holo::{self as holo, Card, Page, PageRef, Row};
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, A};
use super::lobby_spawn::{self as sp, native};
use crate::script_call::Value;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

struct Texts {
    title: &'static str,
    sub: &'static str,
    pick_sub: &'static str,
    code: &'static str,
    mode: &'static str,
    base: &'static str,
    ep1: &'static str,
    difficulty: &'static str,
    life: &'static str,
    body: &'static str,
    voice: &'static str,
    male: &'static str,
    female: &'static str,
    looks: &'static str,
    stats: &'static str,
    start: &'static str,
    busy: &'static str,
    presets: &'static str,
    random: &'static str,
    points: &'static str,
    reset: &'static str,
    looks_sub: &'static str,
    stats_sub: &'static str,
    fixed: &'static str,
    diff: [&'static str; 4],
    attrs: [&'static str; 5],
}
const EN: Texts = Texts {
    title: "New game",
    sub: "A body for the construct",
    pick_sub: "Where the story begins",
    code: "CC//NEW",
    mode: "Start",
    base: "Night City",
    ep1: "Phantom Liberty",
    difficulty: "Difficulty",
    life: "Life path",
    body: "Body",
    voice: "Voice",
    male: "Masculine",
    female: "Feminine",
    looks: "Appearance",
    stats: "Attributes",
    start: "Create and continue",
    busy: "Working...",
    presets: "Clothes",
    random: "Randomize",
    points: "Points left",
    reset: "Reset",
    looks_sub: "Every option of the game's screen",
    stats_sub: "First attribute points",
    fixed: "Set by Phantom Liberty",
    diff: ["Story", "Easy", "Hard", "Very hard"],
    attrs: ["Body", "Reflexes", "Technical Ability", "Intelligence", "Cool"],
};
const RU: Texts = Texts {
    title: "Новая игра",
    sub: "Тело для конструкта",
    pick_sub: "С чего начнётся история",
    code: "CC//NEW",
    mode: "Начало",
    base: "Найт-Сити",
    ep1: "Призрачная свобода",
    difficulty: "Сложность",
    life: "Предыстория",
    body: "Тело",
    voice: "Голос",
    male: "Мужское",
    female: "Женское",
    looks: "Внешность",
    stats: "Характеристики",
    start: "Создать и продолжить",
    busy: "Подождите...",
    presets: "Одежда",
    random: "Случайно",
    points: "Осталось очков",
    reset: "Сбросить",
    looks_sub: "Все параметры экрана игры",
    stats_sub: "Первые очки характеристик",
    fixed: "Задано «Призрачной свободой»",
    diff: ["Сюжет", "Лёгкий", "Сложный", "Очень сложный"],
    attrs: ["Тело", "Рефлексы", "Техника", "Интеллект", "Хладнокровие"],
};
fn t() -> &'static Texts {
    if super::lobby_menu::lang() == "ru" {
        &RU
    } else {
        &EN
    }
}
struct Life {
    id: &'static str,
    key: &'static str,
    preset: &'static str,
    attrs: &'static str,
    en: &'static str,
    ru: &'static str,
    poster: &'static str,
}
const LIFE: [Life; 3] = [
    Life { id: "LifePaths.Nomad", key: "LocKey#1799", preset: "nomad", attrs: "UIAttributePresets.Preset1", en: "Nomad", ru: "Кочевник", poster: "lifepath_nomad" },
    Life { id: "LifePaths.StreetKid", key: "LocKey#1801", preset: "street", attrs: "UIAttributePresets.Preset2", en: "Street Kid", ru: "Дитя улиц", poster: "lifepath_street" },
    Life { id: "LifePaths.Corporate", key: "LocKey#1800", preset: "corpo", attrs: "UIAttributePresets.Preset3", en: "Corpo", ru: "Корпорат", poster: "lifepath_corpo" },
];
const LIFE_ATLAS: &str = "base\\gameplay\\gui\\fullscreen\\main_menu\\character_creation_step_5.inkatlas";
const LIFE_POSTER: f64 = 747.0 / 1088.0;
const FIGURE_BEHIND: f64 = 0.25;
const FIGURE_HALF: f64 = 0.3;
const ZOOM_CLEAR: f64 = 3.0;
const ZOOM_DWELL: f64 = 0.3;
const DIFFICULTY: [&str; 4] = ["Story", "Easy", "Hard", "VeryHard"];
const ATTRS: [&str; 5] = ["Strength", "Reflexes", "TechnicalAbility", "Intelligence", "Cool"];
const STEP_TIMEOUT: f64 = 10.0;
const SHUT_TIME: f64 = 0.15;
const OPEN_TIME: f64 = 0.3;
const BARE_EVERY: f64 = 0.25;
const BARE_FOR: f64 = 8.0;
const UNDERWEAR: &str = "AttachmentSlots.UnderwearBottom";
const CHOSEN_FOR: f64 = 2.0;
fn zoom_dist(z: &str) -> Option<f64> {
    match z {
        "face" => Some(0.95),
        "hands" => Some(1.2),
        _ => None,
    }
}

type Cb = Rc<dyn Fn(&mut Lobby)>;
type Wait = Rc<dyn Fn(&mut Lobby, f64) -> bool>;
struct Step {
    name: String,
    act: Option<Cb>,
    wait: Option<Wait>,
    min: f64,
    timeout: f64,
}
fn step(name: &str) -> Step {
    Step { name: name.into(), act: None, wait: None, min: 0.0, timeout: STEP_TIMEOUT }
}
impl Step {
    fn act(mut self, f: impl Fn(&mut Lobby) + 'static) -> Step {
        self.act = Some(Rc::new(f));
        self
    }
    fn wait(mut self, f: impl Fn(&mut Lobby, f64) -> bool + 'static) -> Step {
        self.wait = Some(Rc::new(f));
        self
    }
    fn min(mut self, m: f64) -> Step {
        self.min = m;
        self
    }
}
struct Job {
    steps: Vec<Step>,
    i: usize,
    t: f64,
    started: bool,
    done: Option<Cb>,
    fail: Option<Cb>,
    kind: Option<&'static str>,
}
/// What an engram's wear hands back: when the look is staged, ready to show, and done (true: it went on).
pub(super) struct WearCb {
    pub stage: Cb,
    pub ready: Cb,
    pub done: Rc<dyn Fn(&mut Lobby, bool)>,
}
#[derive(Default)]
pub(super) struct Figure {
    id: Option<u64>,
    male: Option<bool>,
    entity: Handle,
    anchor: Option<(super::lobby_spawn::V3, f64)>,
    home: (f64, f64),
    at: (f64, f64),
    from: (f64, f64),
    to: (f64, f64),
    t: f64,
    pub last_error: Option<String>,
}
pub(super) struct Creation {
    open: bool,
    held: bool,
    ep1: bool,
    standalone: bool,
    difficulty: usize,
    life: usize,
    male: bool,
    job: Option<Job>,
    pub error: Option<String>,
    time: f64,
    window_open: bool,
    dressed: bool,
    dim: f64,
    dim_to: f64,
    stalled: bool,
    zoom: &'static str,
    zoom_want: Option<&'static str>,
    zoom_since: f64,
    zoom_page: Option<PageRef>,
    bare_at: Option<f64>,
    bare_until: f64,
    chosen: HashMap<String, (i64, f64)>,
    ready: bool,
    look_applied: u32,
    pub figure: Figure,
}
impl Default for Creation {
    fn default() -> Creation {
        Creation {
            open: false,
            held: false,
            ep1: false,
            standalone: false,
            difficulty: 3,
            life: 1,
            male: true,
            job: None,
            error: None,
            time: 0.0,
            window_open: false,
            dressed: false,
            dim: 0.0,
            dim_to: 0.0,
            stalled: false,
            zoom: "body",
            zoom_want: None,
            zoom_since: 0.0,
            zoom_page: None,
            bare_at: None,
            bare_until: 0.0,
            chosen: HashMap::new(),
            ready: false,
            look_applied: 0,
            figure: Figure::default(),
        }
    }
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Creation {}

fn system() -> Handle {
    super::game_system("GetCharacterCustomizationSystem")
}
fn state() -> Handle {
    rt::obj(&system(), "GetState", &[])
}
fn screen(l: &Lobby) -> Option<String> {
    let c = &l.shared.creation;
    if c.is_null() {
        return None;
    }
    let n = super::belt::class_name(c);
    (!n.is_empty()).then_some(n)
}
fn event(l: &Lobby, name: &str) -> bool {
    let d = &l.shared.creation_events;
    if d.is_null() {
        return false;
    }
    ink::call(d, "SpawnEvent", &[sp::cn(name)]);
    true
}
fn hold(l: &mut Lobby, on: bool) {
    if l.creation.held == on {
        return;
    }
    l.creation.held = on;
    native::<()>(if on { "VREngineStereoMenuShown" } else { "VREngineStereoMenuClosed" }, &[]);
    l.shared.creating = on;
}
fn target(e: &Handle) {
    native::<bool>("VRCreationTarget", &[Value::Handle(e)]);
}
fn run(l: &mut Lobby, steps: Vec<Step>, done: Option<Cb>, kind: Option<&'static str>) {
    l.creation.job = Some(Job { steps, i: 0, t: 0.0, started: false, done, fail: None, kind });
    l.creation.error = None;
    l.creation.dim_to = 0.0;
}
fn set_fail(l: &mut Lobby, f: impl Fn(&mut Lobby) + 'static) {
    if let Some(j) = l.creation.job.as_mut() {
        j.fail = Some(Rc::new(f));
    }
}
fn step_job(l: &mut Lobby, dt: f64) {
    let Some(mut j) = l.creation.job.take() else { return };
    if j.i >= j.steps.len() {
        if let Some(d) = j.done.clone() {
            d(l);
        }
        holo::refresh(l);
        return;
    }
    if !j.started {
        (j.started, j.t) = (true, 0.0);
        if let Some(a) = j.steps[j.i].act.clone() {
            a(l);
        }
    }
    j.t += dt;
    let st = &j.steps[j.i];
    let ready = st.wait.clone().is_none_or(|w| w(l, j.t));
    let st = &j.steps[j.i];
    if ready && j.t >= st.min {
        (j.i, j.started) = (j.i + 1, false);
    } else if j.t > st.timeout {
        l.creation.error = Some(format!("{} timed out on {}", st.name, screen(l).unwrap_or_else(|| "nil".into())));
        if let Some(f) = j.fail.clone() {
            f(l);
        }
        holo::refresh(l);
        return;
    }
    if l.creation.job.is_none() {
        l.creation.job = Some(j);
    }
}
fn shut_step() -> Step {
    step("shut").act(|l| l.creation.dim_to = 1.0).wait(|l, _| l.creation.dim >= 1.0).min(SHUT_TIME + 0.05)
}
fn open_step() -> Step {
    step("open").wait(|l, t| {
        if !(l.creation.stalled || t > 1.0) {
            return false;
        }
        l.creation.dim_to = 0.0;
        true
    })
}
fn stat_type(name: &str) -> f64 {
    rt::enum_value("gamedataStatType", name)
}
fn preset_attributes(l: &Lobby) {
    let st = state();
    if l.creation.standalone {
        let build = rt::record("ProgressionBuilds.EP1_Standalone_StartingBuild");
        for a in rt::objs(&build, "StartingAttributes", &[]) {
            let stat = rt::num(&rt::obj(&a, "Attribute", &[]), "StatType", &[]).unwrap_or(0.0);
            let lv = rt::num(&a, "Level", &[]).unwrap_or(0.0);
            rt::call(&st, "SetAttribute", &[A::N(stat), A::N(lv)]);
        }
    } else {
        rt::call(&st, "SetAttributePointsAvailable", &[A::N(7.0)]);
        let preset = rt::record(LIFE[l.creation.life - 1].attrs);
        for a in rt::objs(&preset, "Attributes", &[]) {
            let stat = rt::num(&rt::obj(&a, "Attribute", &[]), "StatType", &[]).unwrap_or(0.0);
            let v = rt::num(&a, "Value", &[]).unwrap_or(0.0);
            rt::call(&st, "SetAttribute", &[A::N(stat), A::N(v)]);
        }
    }
}
fn preset(l: &mut Lobby) {
    l.creation.chosen.clear();
    rt::call(&system(), "ApplyUIPreset", &[A::Name(CName::hashed(LIFE[l.creation.life - 1].preset).0)]);
    (l.creation.bare_at, l.creation.bare_until) = (Some(l.creation.time + BARE_EVERY), l.creation.time + BARE_FOR);
}
fn options() -> Vec<Handle> {
    rt::objs(&system(), "GetUnitedOptions", &[A::B(true), A::B(true), A::B(true)])
}
fn info(o: &Handle) -> Option<rt::Val> {
    rt::field(o, "info")
}
fn cur_index(o: &Handle) -> i64 {
    rt::field(o, "currIndex").and_then(|v| v.num()).unwrap_or(0.0) as i64
}
/// bare: the figure's genitals option set while nudity is allowed and nothing is worn under (true: done).
fn bare(l: &mut Lobby) -> bool {
    let sys = system();
    if rt::num(&sys, "IsNudityAllowed", &[]).is_none_or(|v| v == 0.0) {
        return true;
    }
    let fig = figure_entity(l);
    let dressed = !fig.is_null() && !super::belt::in_slot(&fig, UNDERWEAR).is_null();
    let want = if l.creation.male { "penis01" } else { "vagina" };
    for o in options() {
        let Some(inf) = info(&o) else { continue };
        if inf.f("name").map(|n| n.text()).as_deref() != Some("genitals") {
            continue;
        }
        let cur = cur_index(&o);
        let done = cur != 0 && !dressed;
        if cur != 0 || !dressed {
            return done;
        }
        if let Some(ops) = inf.f("options") {
            for i in 0..ops.len() {
                if ops.at(i).and_then(|op| op.f("localizedName")).is_some_and(|n| n.text().contains(want)) {
                    rt::call(&sys, "ApplyChangeToOption", &[A::H(&o), A::N(i as f64)]);
                    return done;
                }
            }
        }
        return done;
    }
    true
}
fn figure_spot(l: &Lobby, zoom: &str) -> (f64, f64) {
    let spot = holo::window_spot(l, FIGURE_BEHIND).unwrap_or((super::lobby_scene::FIGURE.0, super::lobby_scene::FIGURE.1));
    let Some(d) = zoom_dist(zoom) else { return spot };
    let mut bearing = spot.0;
    if let Some(edge) = holo::window_rows_edge(l) {
        bearing = bearing.max(edge + (FIGURE_HALF / d).atan().to_degrees() + ZOOM_CLEAR);
    }
    (bearing, d)
}
fn dress_steps() -> Vec<Step> {
    vec![
        step("figure")
            .act(|l| {
                (l.creation.zoom, l.creation.zoom_want, l.creation.zoom_page) = ("body", None, None);
                let spot = figure_spot(l, "body");
                let (anchor, male) = (l.anchor(), l.creation.male);
                figure_spawn(l, anchor, spot, male);
                figure_go_to(l, spot);
            })
            .wait(|l, _| !figure_entity(l).is_null())
            .min(0.5),
        step("target")
            .act(|l| {
                let e = figure_entity(l);
                target(&e);
            })
            .min(0.3),
        step("dress").act(preset).min(1.0),
    ]
}
fn on(class: &'static str) -> impl Fn(&mut Lobby, f64) -> bool {
    move |l, _| screen(l).as_deref() == Some(class)
}
fn body_ready(l: &mut Lobby, _t: f64) -> bool {
    screen(l).as_deref() == Some("characterCreationBodyMorphMenu") && !options().is_empty()
}
fn enter_steps(with_figure: bool) -> Vec<Step> {
    let mut steps = vec![
        step("new game")
            .act(|l| {
                hold(l, true);
                let d = rt::field(&l.shared.main_menu, "menuEventDispatcher").map(|v| v.handle()).unwrap_or_else(Handle::null);
                ink::call(&d, "SpawnEvent", &[sp::cn("OnNewGame")]);
            })
            .wait(|l, _| matches!(screen(l).as_deref(), Some("ExpansionNewGame") | Some("DifficultySelectionMenu"))),
        step("start")
            .act(|l| {
                if screen(l).as_deref() != Some("ExpansionNewGame") {
                    return;
                }
                rt::call(&state(), "SetIsExpansionStandalone", &[A::B(l.creation.standalone)]);
                event(l, "OnAccept");
            })
            .wait(on("DifficultySelectionMenu")),
        step("difficulty")
            .act(|l| {
                set_difficulty(l);
                event(l, "OnAccept");
            })
            .wait(on("CharacterCreationBackstorySelectionMenu")),
        shut_step(),
        step("life path")
            .act(|l| {
                set_life_path(l);
                event(l, "OnAccept");
            })
            .wait(on("CharacterCreationGenderSelectionMenu")),
        step("body")
            .act(|l| {
                l.creation.stalled = false;
                let st = state();
                rt::call(&st, "SetIsBodyGenderMale", &[A::B(l.creation.male)]);
                rt::call(&st, "SetIsBrainGenderMale", &[A::B(l.creation.male)]);
                event(l, "OnAccept");
            })
            .wait(body_ready)
            .min(0.5),
        open_step(),
    ];
    if with_figure {
        steps.extend(dress_steps());
    }
    steps
}
fn set_difficulty(l: &Lobby) {
    let v = rt::enum_value("gameDifficulty", DIFFICULTY[l.creation.difficulty - 1]);
    rt::call(&super::game_system("GetStatsDataSystem"), "SetDifficulty", &[A::N(v)]);
}
fn set_life_path(l: &Lobby) {
    let id = red4ext_abi::tweakdbid::from_name(LIFE[l.creation.life - 1].id);
    rt::call(&state(), "SetLifePath", &[A::Name(id)]);
}
fn leave_steps(keep_figure: bool, ready: Option<Cb>) -> Vec<Step> {
    let sent = Rc::new(Cell::new(0.0));
    vec![
        step("untarget").act(move |l| {
            target(&Handle::null());
            if !keep_figure {
                figure_clear(l);
            }
        }),
        shut_step(),
        step("ready").act(move |l| {
            if let Some(r) = ready.clone() {
                r(l);
            }
        }),
        Step {
            timeout: 15.0,
            ..step("back")
                .act(|l| {
                    if screen(l).is_some() {
                        event(l, "OnBack");
                    }
                })
                .wait(move |l, t| {
                    let s = screen(l);
                    if s.is_none() || matches!(s.as_deref(), Some("DifficultySelectionMenu") | Some("ExpansionNewGame")) {
                        l.creation.dim_to = 0.0;
                    }
                    if s.is_none() {
                        return !l.shared.main_menu.is_null();
                    }
                    if t - sent.get() > 0.5 {
                        sent.set(t);
                        event(l, "OnBack");
                    }
                    false
                })
        },
    ]
}
/// loc: a key or name in the language (LocKey, a name key, else UI-CharacterCreation-<name> made readable).
fn loc(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if let Some(t) = sp::global_text("GetLocalizedText", s).filter(|t| !t.is_empty() && t != s) {
        return t;
    }
    if !s.starts_with("LocKey#") {
        if let Some(t) = sp::global_text("GetLocalizedTextByKey", s).filter(|t| !t.is_empty()) {
            return t;
        }
    }
    if let Some(key) = s.strip_prefix("UI-CharacterCreation-") {
        let k = key.trim_start_matches(|c: char| c.is_ascii_digit());
        let k = if k.len() < key.len() && k.starts_with('_') { &k[1..] } else { key };
        return k.replace('_', " ");
    }
    s.to_string()
}
fn values(o: &Handle) -> (usize, Vec<String>) {
    let Some(inf) = info(o) else { return (0, Vec::new()) };
    let list = match inf.class_name().as_str() {
        "gameuiAppearanceInfo" => inf.f("definitions"),
        "gameuiMorphInfo" => inf.f("morphNames"),
        "gameuiSwitcherInfo" => inf.f("options"),
        _ => None,
    };
    let Some(list) = list else { return (0, Vec::new()) };
    let names = (0..list.len()).map(|i| list.at(i).and_then(|e| e.f("localizedName")).map(|n| loc(&n.text())).unwrap_or_default()).collect();
    (list.len(), names)
}
fn option_key(o: &Handle) -> String {
    info(o).and_then(|i| i.f("name")).map(|n| n.text()).unwrap_or_default()
}
fn change(l: &mut Lobby, o: &Handle, index: i64) {
    if l.creation.job.is_some() {
        return;
    }
    if rt::call(&system(), "ApplyChangeToOption", &[A::H(o), A::N(index as f64)]).is_none() {
        l.creation.error = Some("ApplyChangeToOption failed".into());
        return;
    }
    let time = l.creation.time;
    l.creation.chosen.insert(option_key(o), (index, time));
}
fn current_index(l: &mut Lobby, o: &Handle) -> i64 {
    let key = option_key(o);
    let cur = cur_index(o);
    if let Some(&(index, at)) = l.creation.chosen.get(&key) {
        if cur != index && l.creation.time - at < CHOSEN_FOR {
            return index;
        }
        l.creation.chosen.remove(&key);
    }
    cur
}
fn zoom_of(o: &Handle) -> &'static str {
    let part = rt::field(o, "bodyPart").and_then(|v| v.num());
    if part == Some(rt::enum_value("gameuiCharacterCustomizationPart", "Head")) {
        "face"
    } else if part == Some(rt::enum_value("gameuiCharacterCustomizationPart", "Arms")) {
        "hands"
    } else {
        "body"
    }
}
fn looks_rows(l: &mut Lobby) -> Vec<Row> {
    let tx = t();
    let life = &LIFE[l.creation.life - 1];
    let mut rows = vec![
        Row { value: Some(loc(life.key)), zoom: Some("body"), left: Some(Rc::new(|l, _| preset(l))), right: Some(Rc::new(|l, _| preset(l))), ..Row::new("preset", tx.presets) },
        Row::new("random", tx.random).go(|l, _| {
            l.creation.chosen.clear();
            let sys = system();
            let p = rt::call(&sys, "GetRandomizationParameters", &[]);
            if let Some(p) = p {
                let v = rt::Val::of(&p);
                rt::call(&sys, "RandomizeOptions", &[A::P(v.ptr.cast_const())]);
            }
        }),
    ];
    for (i, o) in options().into_iter().enumerate() {
        let flag = |n: &str| rt::field(&o, n).is_some_and(|v| v.bool());
        if !(flag("isEditable") && flag("isActive") && !flag("isCensored")) {
            continue;
        }
        let (n, names) = values(&o);
        if n <= 1 {
            continue;
        }
        let cur = current_index(l, &o);
        let label = info(&o).and_then(|i| i.f("localizedName")).map(|v| loc(&v.text())).unwrap_or_default();
        let (oa, ob) = (o.clone(), o.clone());
        let nn = n as i64;
        rows.push(Row {
            value: Some(format!("{}  {}/{}", names.get(cur as usize).cloned().unwrap_or_default(), cur + 1, n)),
            zoom: Some(zoom_of(&o)),
            left: Some(Rc::new(move |l, _| change(l, &oa, (cur - 1).rem_euclid(nn)))),
            right: Some(Rc::new(move |l, _| change(l, &ob, (cur + 1).rem_euclid(nn)))),
            ..Row::new(&format!("o{}{}", i + 1, option_key(&o)), &label)
        });
    }
    rows
}
fn kept(name: &str) -> bool {
    name.contains("genital") || name.contains("underpants") || name.contains("pubic")
}
/// applyLook: the saved look's appearances, morphs and switchers chosen on the game's options.
fn apply_look(l: &mut Lobby, look: &super::lobby_engrams::Look, only: Option<&str>) {
    let mut apps: HashMap<String, String> = HashMap::new();
    let mut morphs: HashMap<String, String> = HashMap::new();
    let mut names: HashMap<String, bool> = HashMap::new();
    let mut vals: HashMap<String, bool> = HashMap::new();
    for a in &look.apps {
        if !kept(&a.option) {
            apps.insert(a.option.clone(), a.appearance.clone());
            names.insert(a.option.clone(), true);
            vals.insert(a.appearance.clone(), true);
        }
    }
    for m in &look.morphs {
        morphs.insert(m.region.clone(), m.morph.clone());
        names.insert(m.region.clone(), true);
    }
    let sys = system();
    let mut applied = 0;
    for o in options() {
        let Some(inf) = info(&o) else { continue };
        let kind = inf.class_name();
        let iname = inf.f("name").map(|n| n.text()).unwrap_or_default();
        if only.is_some_and(|k| k != kind) || kept(&iname) {
            continue;
        }
        let mut index = None;
        match kind.as_str() {
            "gameuiAppearanceInfo" => {
                let want = apps.get(&iname);
                if let Some(defs) = inf.f("definitions") {
                    for i in 0..defs.len() {
                        let n = defs.at(i).and_then(|d| d.f("name")).map(|n| n.text()).unwrap_or_default();
                        match want {
                            Some(w) => {
                                if &n == w {
                                    index = Some(i as i64);
                                }
                            }
                            None => {
                                if vals.contains_key(&n) && index.is_none() {
                                    index = Some(i as i64);
                                }
                            }
                        }
                    }
                }
            }
            "gameuiMorphInfo" => {
                if let (Some(want), Some(ms)) = (morphs.get(&iname), inf.f("morphNames")) {
                    for i in 0..ms.len() {
                        if ms.at(i).and_then(|m| m.f("morphName")).map(|n| n.text()).as_ref() == Some(want) {
                            index = Some(i as i64);
                        }
                    }
                }
            }
            "gameuiSwitcherInfo" => {
                let mut best = 0;
                if let Some(opts) = inf.f("options") {
                    for i in 0..opts.len() {
                        let Some(sw) = opts.at(i) else { continue };
                        let mut n = 0;
                        if let Some(ns) = sw.f("names") {
                            for k in 0..ns.len() {
                                if ns.at(k).is_some_and(|nm| names.contains_key(&nm.text())) {
                                    n += 1;
                                }
                            }
                        }
                        if n > best {
                            (best, index) = (n, Some(i as i64));
                        }
                    }
                }
            }
            _ => {}
        }
        if let Some(ix) = index.filter(|ix| *ix != cur_index(&o)) {
            rt::call(&sys, "ApplyChangeToOption", &[A::H(&o), A::N(ix as f64)]);
            applied += 1;
        }
    }
    l.creation.look_applied += applied;
}
fn look_steps(look: Rc<super::lobby_engrams::Look>) -> Vec<Step> {
    let (a, b, c) = (look.clone(), look.clone(), look);
    vec![
        step("look: switchers")
            .act(move |l| {
                l.creation.look_applied = 0;
                apply_look(l, &a, Some("gameuiSwitcherInfo"));
            })
            .min(0.5),
        step("look").act(move |l| apply_look(l, &b, None)).min(0.9),
        step("look: again").act(move |l| apply_look(l, &c, None)).min(0.5),
    ]
}
fn life_index(id: &str) -> usize {
    LIFE.iter().position(|l| l.id == format!("LifePaths.{id}")).map_or(1, |i| i + 1)
}
fn limits() -> (f64, f64) {
    let rd = |n: &str, d: f64| super::weapon::flat_u64(n).map_or(d, |v| f64::from(v as u32 as i32));
    (rd("UICharacterCreationGeneral.BaseValues.minAttributeValue", 3.0), rd("UICharacterCreationGeneral.BaseValues.maxAttributeValue", 6.0))
}
fn stats_rows(l: &mut Lobby) -> Vec<Row> {
    let tx = t();
    let st = state();
    let (lo, hi) = limits();
    let points = rt::num(&st, "GetAttributePointsAvailable", &[]).unwrap_or(0.0);
    let standalone = l.creation.standalone;
    let mut rows = vec![Row { header: true, ..Row::new("points", &if standalone { tx.fixed.to_string() } else { format!("{}: {}", tx.points, holo::fmt_num(points)) }) }];
    for (i, name) in ATTRS.iter().enumerate() {
        let stat = stat_type(name);
        let v = rt::num(&st, "GetAttribute", &[A::N(stat)]).unwrap_or(0.0);
        let mut row = Row { value: Some(holo::fmt_num(v)), ..Row::new(name, tx.attrs[i]) };
        if !standalone {
            let (sa, sb) = (st.clone(), st.clone());
            row.left = Some(Rc::new(move |_, _| {
                if v > lo {
                    rt::call(&sa, "SetAttribute", &[A::N(stat), A::N(v - 1.0)]);
                    rt::call(&sa, "SetAttributePointsAvailable", &[A::N(points + 1.0)]);
                }
            }));
            row.right = Some(Rc::new(move |_, _| {
                if v < hi && points > 0.0 {
                    rt::call(&sb, "SetAttribute", &[A::N(stat), A::N(v + 1.0)]);
                    rt::call(&sb, "SetAttributePointsAvailable", &[A::N(points - 1.0)]);
                }
            }));
        }
        rows.push(row);
    }
    if !standalone {
        let sr = st.clone();
        rows.push(Row::new("reset", tx.reset).go(move |_, _| {
            for name in ATTRS {
                rt::call(&sr, "SetAttribute", &[A::N(stat_type(name)), A::N(lo)]);
            }
            rt::call(&sr, "SetAttributePointsAvailable", &[A::N(7.0)]);
        }));
    }
    rows
}
fn cycle(i: usize, n: usize, d: i64) -> usize {
    ((i as i64 - 1 + d).rem_euclid(n as i64) + 1) as usize
}
fn busy_row(l: &Lobby) -> Row {
    Row { detail: l.creation.error.clone(), disabled: true, ..Row::new("busy", t().busy) }
}
fn start_game(l: &mut Lobby) {
    if l.creation.job.is_some() {
        return;
    }
    let steps = vec![
        step("figure out")
            .act(|l| {
                target(&Handle::null());
                figure_clear(l);
                (l.creation.window_open, l.creation.dressed) = (false, false);
            })
            .min(0.6),
        step("finalize")
            .act(|_| {
                let sys = system();
                rt::call(&sys, "FinalizeState", &[]);
                let st = state();
                if let Ok(rh) = super::requests_handler() {
                    ink::call(&rh, "StartNewGame", &[Value::Handle(&st)]);
                }
            })
            .min(0.5),
    ];
    run(
        l,
        steps,
        Some(Rc::new(|l| {
            hold(l, false);
            l.creation.open = false;
            holo::close(l);
        })),
        None,
    );
}
fn rebody(l: &mut Lobby, male: bool) {
    if l.creation.job.is_some() || male == l.creation.male {
        return;
    }
    l.creation.male = male;
    let mut steps = vec![
        step("untarget").act(|_| target(&Handle::null())),
        shut_step(),
        step("back to body").act(|l| {
            event(l, "OnBack");
        })
        .wait(on("CharacterCreationGenderSelectionMenu")),
        step("body")
            .act(move |l| {
                l.creation.stalled = false;
                let st = state();
                rt::call(&st, "SetIsBodyGenderMale", &[A::B(male)]);
                rt::call(&st, "SetIsBrainGenderMale", &[A::B(male)]);
                event(l, "OnAccept");
            })
            .wait(body_ready)
            .min(0.5),
        open_step(),
    ];
    steps.extend(dress_steps());
    run(l, steps, None, None);
}
fn relife(l: &mut Lobby, d: i64) {
    l.creation.life = cycle(l.creation.life, LIFE.len(), d);
    set_life_path(l);
    preset_attributes(l);
    preset(l);
}
fn life_name(i: usize) -> String {
    let lf = &LIFE[i - 1];
    let fallback = if super::lobby_menu::lang() == "ru" { lf.ru } else { lf.en };
    super::lobby_menu::label(lf.key, fallback)
}
fn close_window(l: &mut Lobby) {
    (l.creation.window_open, l.creation.dressed, l.creation.bare_at) = (false, false, None);
    if l.creation.job.as_ref().is_some_and(|j| j.kind == Some("dress")) {
        l.creation.job = None;
    }
    target(&Handle::null());
    figure_clear(l);
}
fn flip_mode(l: &mut Lobby) {
    l.creation.standalone = !l.creation.standalone;
    rt::call(&state(), "SetIsExpansionStandalone", &[A::B(l.creation.standalone)]);
    preset_attributes(l);
}
fn diff_by(l: &mut Lobby, d: i64) {
    l.creation.difficulty = cycle(l.creation.difficulty, 4, d);
    set_difficulty(l);
}
fn voice(_l: &mut Lobby) {
    let st = state();
    let brain = rt::num(&st, "IsBrainGenderMale", &[]).is_none_or(|v| v != 0.0);
    rt::call(&st, "SetIsBrainGenderMale", &[A::B(!brain)]);
    rt::call(&system(), "TriggerVoiceToneSample", &[]);
}
fn new_page() -> PageRef {
    let tx = t();
    let mut p = Page::new(Some(tx.title.into()), tx.code, |l| {
        let tx = t();
        if l.creation.job.is_some() || !l.creation.ready {
            return vec![busy_row(l)];
        }
        let c = &l.creation;
        let mut rows = Vec::new();
        if c.ep1 {
            rows.push(Row { value: Some(if c.standalone { tx.ep1 } else { tx.base }.into()), left: Some(Rc::new(|l, _| flip_mode(l))), right: Some(Rc::new(|l, _| flip_mode(l))), ..Row::new("mode", tx.mode) });
        }
        rows.push(Row { value: Some(tx.diff[c.difficulty - 1].into()), left: Some(Rc::new(|l, _| diff_by(l, -1))), right: Some(Rc::new(|l, _| diff_by(l, 1))), ..Row::new("difficulty", tx.difficulty) });
        rows.push(Row { value: Some(life_name(c.life)), left: Some(Rc::new(|l, _| relife(l, -1))), right: Some(Rc::new(|l, _| relife(l, 1))), ..Row::new("life", tx.life) });
        rows.push(Row {
            value: Some(if c.male { tx.male } else { tx.female }.into()),
            left: Some(Rc::new(|l, _| {
                let m = !l.creation.male;
                rebody(l, m);
            })),
            right: Some(Rc::new(|l, _| {
                let m = !l.creation.male;
                rebody(l, m);
            })),
            ..Row::new("body", tx.body)
        });
        let brain = rt::num(&state(), "IsBrainGenderMale", &[]).is_none_or(|v| v != 0.0);
        rows.push(Row { value: Some(if brain { tx.male } else { tx.female }.into()), left: Some(Rc::new(|l, _| voice(l))), right: Some(Rc::new(|l, _| voice(l))), ..Row::new("voice", tx.voice) });
        rows.push(Row::new("looks", tx.looks).go(|l, _| {
            let tx = t();
            let mut pg = Page::new(Some(tx.looks.into()), "CC//LOOKS", |l| if l.creation.job.is_some() { vec![busy_row(l)] } else { looks_rows(l) });
            pg.subtitle = Some(tx.looks_sub.into());
            holo::push(l, Rc::new(pg));
        }));
        rows.push(Row::new("stats", tx.stats).go(|l, _| {
            let tx = t();
            let mut pg = Page::new(Some(tx.stats.into()), "CC//ATTR", stats_rows);
            pg.subtitle = Some(tx.stats_sub.into());
            holo::push(l, Rc::new(pg));
        }));
        rows.push(Row { on: true, ..Row::new("start", tx.start) }.go(|l, _| start_game(l)));
        rows
    });
    p.subtitle = Some(tx.sub.into());
    p.on_back = Some(Rc::new(close_window));
    Rc::new(p)
}
fn choose(l: &mut Lobby, i: usize) {
    l.creation.life = i;
    set_life_path(l);
    preset_attributes(l);
    if holo::window(l, new_page()) {
        l.creation.window_open = true;
    }
}
fn life_page() -> PageRef {
    let tx = t();
    let mut p = Page::new(Some(tx.title.into()), "CC//PATH", |l| {
        let cards = (1..=LIFE.len())
            .map(|i| Card { label: life_name(i), picture: (LIFE_ATLAS.into(), LIFE[i - 1].poster.into()), on: i == l.creation.life, go: Some(Rc::new(move |l, _| choose(l, i))) })
            .collect();
        vec![Row { cards: Some(cards), poster: Some(LIFE_POSTER), ..Row::new("paths", "") }]
    });
    p.subtitle = Some(tx.pick_sub.into());
    p.on_back = Some(Rc::new(leave));
    Rc::new(p)
}
/// M.open: the new game's first page, the game's creation entered behind it.
pub(super) fn open(l: &mut Lobby) {
    if l.creation.open || l.creation.job.is_some() || l.shared.main_menu.is_null() {
        return;
    }
    (l.creation.open, l.creation.ready, l.creation.window_open, l.creation.dressed) = (true, false, false, false);
    l.creation.ep1 = super::requests_handler().ok().is_some_and(|rh| ink::get::<bool>(&rh, "IsAdditionalContentEnabled", &[sp::cn("EP1")]) == Some(true));
    if !l.creation.ep1 {
        l.creation.standalone = false;
    }
    holo::push(l, life_page());
    run(l, enter_steps(false), Some(Rc::new(|l| l.creation.ready = true)), None);
    set_fail(l, holo::home);
}
pub(super) fn leave(l: &mut Lobby) {
    if !l.creation.open {
        return;
    }
    (l.creation.open, l.creation.ready) = (false, false);
    l.creation.job = None;
    run(l, leave_steps(false, None), Some(Rc::new(|l| hold(l, false))), None);
    set_fail(l, |l| hold(l, false));
}
/// M.wear: the engram's look put on the player through the creation's steps (false: busy).
pub(super) fn wear(l: &mut Lobby, look: Rc<super::lobby_engrams::Look>, on_: WearCb) -> bool {
    if l.creation.job.is_some() || l.creation.open || l.shared.main_menu.is_null() {
        return false;
    }
    (l.creation.male, l.creation.life, l.creation.standalone) = (look.body != "Female", life_index(&look.life), false);
    let mut steps = Vec::new();
    for st in enter_steps(false) {
        let shut = st.name == "shut";
        steps.push(st);
        if shut {
            let s = on_.stage.clone();
            steps.push(step("stage").act(move |l| s(l)));
        }
    }
    steps.push(step("the player").act(|_| target(&super::belt::player())).min(0.4));
    steps.extend(look_steps(look));
    steps.extend(leave_steps(true, Some(on_.ready.clone())));
    let (d1, d2) = (on_.done.clone(), on_.done);
    run(
        l,
        steps,
        Some(Rc::new(move |l| {
            hold(l, false);
            d1(l, true);
        })),
        None,
    );
    set_fail(l, move |l| {
        target(&Handle::null());
        run(l, leave_steps(true, None), Some(Rc::new(|l| hold(l, false))), None);
        d2(l, false);
    });
    true
}
pub(super) fn busy(l: &Lobby) -> bool {
    l.creation.job.is_some()
}
pub(super) fn closure(l: &Lobby) -> f64 {
    l.creation.dim
}
pub(super) fn is_open(l: &Lobby) -> bool {
    l.creation.open
}
pub(super) fn update(l: &mut Lobby, dt: f64) {
    l.creation.time += dt;
    step_job(l, dt);
    if l.creation.job.is_none() {
        l.creation.dim_to = 0.0;
    }
    if dt > 0.2 {
        l.creation.stalled = true;
    }
    let stp = dt.min(0.05);
    let c = &mut l.creation;
    if c.dim < c.dim_to {
        c.dim = c.dim_to.min(c.dim + stp / SHUT_TIME);
    } else if c.dim > c.dim_to {
        c.dim = c.dim_to.max(c.dim - stp / OPEN_TIME);
    }
    if c.open && c.window_open && c.ready && !c.dressed && c.job.is_none() && holo::window_open(l) {
        run(l, dress_steps(), Some(Rc::new(|l| l.creation.dressed = true)), Some("dress"));
        set_fail(l, |l| l.creation.dressed = true);
    }
    if l.creation.open && l.creation.dressed && l.creation.figure.id.is_some() {
        let page = holo::top(l);
        let same = match (&page, &l.creation.zoom_page) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            (l.creation.zoom_page, l.creation.zoom_want, l.creation.zoom) = (page, None, "body");
        }
        let row = if l.creation.job.is_none() { holo::hovered(l) } else { None };
        let want = row.and_then(|r| r.zoom);
        if want != l.creation.zoom_want {
            (l.creation.zoom_want, l.creation.zoom_since) = (want, l.creation.time);
        }
        if let Some(w) = want {
            if l.creation.time - l.creation.zoom_since >= ZOOM_DWELL {
                l.creation.zoom = w;
            }
        }
        let spot = figure_spot(l, l.creation.zoom);
        figure_go_to(l, spot);
        if let Some(at) = l.creation.bare_at {
            if l.creation.time >= at && l.creation.job.is_none() {
                let done = bare(l);
                l.creation.bare_at = (!done && l.creation.time < l.creation.bare_until).then(|| l.creation.time + BARE_EVERY);
            }
        }
    }
    figure_update(l, dt);
}
pub(super) fn close(l: &mut Lobby) {
    if l.creation.held {
        target(&Handle::null());
    }
    figure_clear(l);
    l.creation.job = None;
    (l.creation.open, l.creation.ready, l.creation.window_open, l.creation.dressed) = (false, false, false, false);
    hold(l, false);
}
pub(super) fn state_line(l: &Lobby) -> String {
    let c = &l.creation;
    format!(
        "creation open {} ready {} job {} screen {} error {} held {} window {} dressed {}",
        c.open,
        c.ready,
        c.job.as_ref().and_then(|j| j.steps.get(j.i)).map_or("-".to_string(), |s| s.name.clone()),
        screen(l).unwrap_or_else(|| "-".into()),
        c.error.clone().unwrap_or_else(|| "-".into()),
        c.held,
        c.window_open,
        c.dressed
    )
}

// ---------------------------------------------------------------- figure.lua
const FIG_TAG: &str = "VRLobbyFigure";
const GLIDE: f64 = 0.6;
fn template(male: bool) -> &'static str {
    let ep1 = super::requests_handler().ok().is_some_and(|rh| ink::get::<bool>(&rh, "IsAdditionalContentEnabled", &[sp::cn("EP1")]) == Some(true));
    match (ep1, male) {
        (true, true) => "ep1\\characters\\entities\\player\\photo_mode\\player_ma_photomode_ep1.ent",
        (true, false) => "ep1\\characters\\entities\\player\\photo_mode\\player_wa_photomode_ep1.ent",
        (false, true) => "base\\characters\\entities\\player\\photo_mode\\player_ma_photomode.ent",
        (false, false) => "base\\characters\\entities\\player\\photo_mode\\player_wa_photomode.ent",
    }
}
/// record: the figure's record (Character.VRPort_Figure{Male,Female}, a copy of the photo mode's puppet made by the
/// tweak data) pointed at the template of the game's content.
fn record(male: bool) -> &'static str {
    let name = if male { "Character.VRPort_FigureMale" } else { "Character.VRPort_FigureFemale" };
    crate::tweak_records::set_cname_flat(name, "entityTemplatePath", red4ext_abi::resource::resource_path(template(male)));
    name
}
fn place_of(anchor: (super::lobby_spawn::V3, f64), spec: (f64, f64)) -> (super::lobby_spawn::V3, f64) {
    let (b, yaw) = (spec.0.to_radians(), anchor.1.to_radians());
    let (x, y) = (spec.1 * b.sin(), spec.1 * b.cos());
    let (cs, sn) = (yaw.cos(), yaw.sin());
    let (wx, wy) = (cs * x - sn * y, sn * x + cs * y);
    ([anchor.0[0] + wx, anchor.0[1] + wy, anchor.0[2]], wx.atan2(-wy).to_degrees())
}
fn figure_spawn(l: &mut Lobby, anchor: Option<(super::lobby_spawn::V3, f64)>, spec: (f64, f64), male: bool) -> bool {
    let Some(a) = anchor else { return false };
    if l.creation.figure.id.is_some() && l.creation.figure.male == Some(male) {
        return true;
    }
    figure_clear(l);
    let (pos, heading) = place_of(a, spec);
    let id = sp::spawn_record(record(male), pos, yaw_quat(heading), FIG_TAG);
    let f = &mut l.creation.figure;
    if id.is_none() {
        f.last_error = Some("the figure did not spawn".into());
    }
    (f.id, f.male, f.entity) = (id, Some(male), Handle::null());
    (f.anchor, f.home) = (Some(a), spec);
    (f.at, f.from, f.to, f.t) = (spec, spec, spec, GLIDE);
    id.is_some()
}
fn figure_go_to(l: &mut Lobby, spec: (f64, f64)) {
    let f = &mut l.creation.figure;
    if f.id.is_none() || f.to == spec {
        return;
    }
    (f.from, f.to, f.t) = (f.at, spec, 0.0);
}
fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
fn figure_update(l: &mut Lobby, dt: f64) {
    if l.creation.figure.id.is_none() || l.creation.figure.t >= GLIDE {
        return;
    }
    let e = figure_entity(l);
    if e.is_null() {
        return;
    }
    let f = &mut l.creation.figure;
    let Some(a) = f.anchor else { return };
    f.t = GLIDE.min(f.t + dt);
    let k = smooth(f.t / GLIDE);
    f.at = (f.from.0 + (f.to.0 - f.from.0) * k, f.from.1 + (f.to.1 - f.from.1) * k);
    let (home, h0) = place_of(a, f.home);
    let (pos, h) = place_of(a, f.at);
    let r = h0.to_radians();
    let (dx, dy) = (pos[0] - home[0], pos[1] - home[1]);
    let vo = sp::find_comp(&e, "visualOffset");
    super::reload_core::set_local(&vo, [dx * r.cos() + dy * r.sin(), -dx * r.sin() + dy * r.cos(), 0.0], yaw_quat(h - h0));
}
pub(super) fn figure_entity(l: &mut Lobby) -> Handle {
    let f = &mut l.creation.figure;
    if f.id.is_none() {
        return Handle::null();
    }
    if f.entity.is_null() {
        f.entity = sp::find_entity(f.id);
    }
    f.entity.clone()
}
fn figure_clear(l: &mut Lobby) {
    sp::clear(FIG_TAG);
    let err = l.creation.figure.last_error.take();
    l.creation.figure = Figure { last_error: err, ..Figure::default() };
}
