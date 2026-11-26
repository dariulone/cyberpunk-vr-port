//! CyberpunkVRPort_Menu/modules/settings_data.lua: the plugin's VR settings pages as the menus show them -- a page is
//! fetched as one string (VRSettingsPage: records split by \x1e, fields by \x1f, options by \x1d), its labels and
//! texts put into the game's language (menu_locale), the folded headers' items skipped, the slider steps and the value
//! formats derived from the item's own text; an edit goes back through VRSettingsEdit.
use super::lobby_spawn::{native, native_string, Str};
use super::menu_locale as loc;
use crate::script_call::Value;

pub(super) const PAGES: [&str; 7] = ["GENERAL", "FRAMEGEN", "HUD", "CONTROLS", "STEREO", "AVATAR", "OVERLAY"];
const PAGES_RU: [&str; 7] = ["ОБЩИЕ", "ГЕН. КАДРОВ", "HUD", "УПРАВЛЕНИЕ", "СТЕРЕО", "АВАТАР", "ОВЕРЛЕЙ"];

#[derive(Clone, Debug)]
pub(super) struct Item {
    pub kind: String,
    pub id: String,
    pub label: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub text: String,
    pub flags: i64,
    pub tip: String,
    pub indent: f64,
    pub page: i32,
}
#[derive(Clone, Debug)]
pub(super) struct PageRec {
    pub name: String,
    pub page: i32,
    pub seq: i64,
    pub items: Vec<Item>,
}
/// D.useGameLanguage.
pub(super) fn use_game_language() {
    loc::set(&super::lobby_ink::language());
}
/// D.pageName (1-based).
pub(super) fn page_name(i: usize) -> String {
    loc::tr(PAGES_RU[i - 1], PAGES[i - 1])
}
pub(super) fn has(flags: i64, bit: i64) -> bool {
    (flags / bit) % 2 == 1
}
fn translate(text: &str, options: bool) -> String {
    if !options {
        return loc::setting(text);
    }
    text.split('\u{1d}').map(loc::setting).collect::<Vec<_>>().join("\u{1d}")
}
fn num(s: Option<&&str>) -> Option<f64> {
    s.and_then(|s| s.trim().parse::<f64>().ok())
}
/// D.parse: a page's string into its record.
pub(super) fn parse(s: &str) -> Option<PageRec> {
    if s.is_empty() {
        return None;
    }
    let recs: Vec<&str> = s.split('\u{1e}').collect();
    let head: Vec<&str> = recs[0].split('\u{1f}').collect();
    if head.first() != Some(&"#") {
        return None;
    }
    let mut pg = PageRec { name: head.get(2).unwrap_or(&"").to_string(), page: num(head.get(3)).map_or(-1, |v| v as i32), seq: num(head.get(4)).map_or(0, |v| v as i64), items: Vec::new() };
    for r in &recs[1..] {
        let f: Vec<&str> = r.split('\u{1f}').collect();
        let kind = f.first().unwrap_or(&"").to_string();
        let opts = kind == "O" || kind == "P";
        pg.items.push(Item {
            id: f.get(1).unwrap_or(&"").to_string(),
            label: translate(f.get(2).unwrap_or(&""), false),
            value: num(f.get(3)).unwrap_or(0.0),
            min: num(f.get(4)).unwrap_or(0.0),
            max: num(f.get(5)).unwrap_or(0.0),
            text: translate(f.get(6).unwrap_or(&""), opts),
            flags: num(f.get(7)).map_or(0, |v| v as i64),
            tip: translate(f.get(8).unwrap_or(&""), false),
            indent: num(f.get(9)).unwrap_or(0.0),
            page: pg.page,
            kind,
        });
    }
    Some(pg)
}
pub(super) fn options(f: &Item) -> Vec<String> {
    f.text.split('\u{1d}').map(str::to_string).collect()
}
/// D.visible: the items shown -- those under a folded header skipped.
pub(super) fn visible<'a>(items: &'a [Item], is_open: &dyn Fn(&Item) -> bool) -> Vec<&'a Item> {
    let (mut out, mut fold): (Vec<&Item>, Option<f64>) = (Vec::new(), None);
    for f in items {
        if fold.is_some_and(|fo| f.kind == "H" && f.indent <= fo) {
            fold = None;
        }
        if fold.is_none() {
            out.push(f);
            if f.kind == "H" && !is_open(f) {
                fold = Some(f.indent);
            }
        }
    }
    out
}
/// D.decimals: the digits after the point of the first number in the text.
pub(super) fn decimals(s: &str) -> i32 {
    let c: Vec<char> = s.chars().collect();
    for i in 1..c.len() {
        if c[i] == '.' && c[i - 1].is_ascii_digit() {
            let n = c[i + 1..].iter().take_while(|ch| ch.is_ascii_digit()).count();
            if n > 0 {
                return n as i32;
            }
        }
    }
    0
}
fn nice(x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    let p = 10f64.powf(x.log10().floor());
    let m = x / p;
    if m < 1.5 {
        p
    } else if m < 3.5 {
        2.0 * p
    } else if m < 7.5 {
        5.0 * p
    } else {
        10.0 * p
    }
}
pub(super) fn step_of(f: &Item) -> f64 {
    let span = (f.max - f.min).abs();
    let steps = 200.0;
    if f.kind == "I" {
        return 1f64.max((nice(span / steps) + 0.5).floor());
    }
    10f64.powi(-decimals(&f.text)).max(nice(span / steps))
}
pub(super) fn snap(f: &Item, v: f64, lo: f64, step: f64) -> f64 {
    let v = lo + ((v - lo) / step + 0.5).floor() * step;
    if f.kind == "I" {
        return (v + 0.5).floor();
    }
    format!("{v:.6}").parse().unwrap_or(v)
}
/// D.fmt: the value written as the item's text writes its number (its prefix, decimals and suffix).
pub(super) fn fmt(f: &Item, v: f64) -> String {
    let t: Vec<char> = f.text.chars().collect();
    // "^(.-)(%-?%d+%.?%d*)(.*)$": the first number, with its sign
    let mut found = None;
    for i in 0..t.len() {
        let mut j = i;
        if t[j] == '-' {
            j += 1;
        }
        let d0 = j;
        while j < t.len() && t[j].is_ascii_digit() {
            j += 1;
        }
        if j == d0 {
            continue;
        }
        if j < t.len() && t[j] == '.' {
            j += 1;
        }
        while j < t.len() && t[j].is_ascii_digit() {
            j += 1;
        }
        found = Some((i, j));
        break;
    }
    let numtxt: String = found.map(|(a, b)| t[a..b].iter().collect()).unwrap_or_else(|| "0.00".into());
    let s = if f.kind == "I" { format!("{}", (v + 0.5).floor() as i64) } else { format!("{v:.*}", decimals(&numtxt) as usize) };
    match found {
        None => s,
        Some((a, b)) => format!("{}{s}{}", t[..a].iter().collect::<String>(), t[b..].iter().collect::<String>()),
    }
}
/// D.wrap: the text in lines of at most `n` characters, by words.
pub(super) fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.split('\n') {
        let (mut line, mut len) = (String::new(), 0usize);
        for word in para.split_whitespace() {
            let wl = word.chars().count();
            if len > 0 && len + 1 + wl > n {
                lines.push(std::mem::take(&mut line));
                (line, len) = (word.to_string(), wl);
            } else if len > 0 {
                line.push(' ');
                line.push_str(word);
                len += 1 + wl;
            } else {
                (line, len) = (word.to_string(), wl);
            }
        }
        lines.push(line);
    }
    while lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}
pub(super) fn fetch(page: i32) -> Option<String> {
    native_string("VRSettingsPage", &[Value::Int32(page)])
}
pub(super) fn edit(f: &Item, value: f64) {
    let id = Str::new(&f.id);
    native::<bool>("VRSettingsEdit", &[Value::Int32(f.page), id.v(), Value::Float(value as f32)]);
}
