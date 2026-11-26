//! CyberpunkVRPort_Menu/modules/locale.lua: the VR menu's words in the game's language -- the menu's own (lang/<code>.lua,
//! keyed by the English; Russian is written inline by the callers) and the VR settings pages' (lang/settings/<code>.lua,
//! with printf formats matched against the plugin's formatted text and filled into the translation). The tables are in
//! menu_locale.json, made from the Lua files by the scratch generator (gen_menu_locale.py); regenerate when they change.
use serde_json::Value as J;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static DATA: OnceLock<J> = OnceLock::new();
fn data() -> &'static J {
    DATA.get_or_init(|| serde_json::from_str(include_str!("menu_locale.json")).unwrap_or(J::Null))
}
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Int,
    Uint,
    Hex,
    Char,
    Str,
    Num,
}
enum Seg {
    Lit(Vec<char>),
    Cap(Kind),
}
struct Format {
    segs: Vec<Seg>,
    kinds: Vec<Kind>,
    tr: String,
}
struct Set {
    exact: HashMap<String, String>,
    formats: Vec<Format>,
    memo: HashMap<String, String>,
}
struct L {
    code: String,
    sets: HashMap<String, Option<Set>>,
}
static STATE: Mutex<Option<L>> = Mutex::new(None);

/// A printf conversion at chars[i..]: its length and kind ("%[-+ #0]*\d*\.?\d*[hlLzjt]*[diouxXeEfgGcsp]").
fn conversion(c: &[char], i: usize) -> Option<(usize, char)> {
    if c.get(i) != Some(&'%') {
        return None;
    }
    let mut j = i + 1;
    while j < c.len() && "-+ #0".contains(c[j]) {
        j += 1;
    }
    while j < c.len() && c[j].is_ascii_digit() {
        j += 1;
    }
    if c.get(j) == Some(&'.') {
        j += 1;
    }
    while j < c.len() && c[j].is_ascii_digit() {
        j += 1;
    }
    while j < c.len() && "hlLzjt".contains(c[j]) {
        j += 1;
    }
    let k = *c.get(j)?;
    "diouxXeEfgGcsp".contains(k).then_some((j + 1 - i, k))
}
fn compile(en: &str) -> (Vec<Seg>, Vec<Kind>) {
    let c: Vec<char> = en.chars().collect();
    let (mut segs, mut kinds, mut lit) = (Vec::new(), Vec::new(), Vec::new());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '%' && c.get(i + 1) == Some(&'%') {
            lit.push('%');
            i += 2;
            continue;
        }
        if let Some((n, k)) = conversion(&c, i) {
            if !lit.is_empty() {
                segs.push(Seg::Lit(std::mem::take(&mut lit)));
            }
            let kind = match k {
                'd' | 'i' => Kind::Int,
                'u' | 'o' => Kind::Uint,
                'x' | 'X' | 'p' => Kind::Hex,
                'c' => Kind::Char,
                's' => Kind::Str,
                _ => Kind::Num,
            };
            kinds.push(kind);
            segs.push(Seg::Cap(kind));
            i += n;
            continue;
        }
        lit.push(c[i]);
        i += 1;
    }
    if !lit.is_empty() {
        segs.push(Seg::Lit(lit));
    }
    (segs, kinds)
}
/// fill: the format's conversions replaced by the values in turn.
fn fill(format: &str, values: &[String]) -> String {
    let c: Vec<char> = format.chars().collect();
    let (mut out, mut n, mut i) = (String::new(), 0, 0);
    while i < c.len() {
        if c[i] == '%' && c.get(i + 1) == Some(&'%') {
            out.push('%');
            i += 2;
        } else if let Some((len, _)) = conversion(&c, i) {
            out.push_str(values.get(n).map_or("", String::as_str));
            n += 1;
            i += len;
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}
/// The lengths a capture of this kind can take at s[i..], longest first (shortest first for %s, Lua's `.-`).
fn spans(kind: Kind, s: &[char], i: usize) -> Vec<usize> {
    let rest = &s[i..];
    let mut j = 0;
    while j < rest.len() && rest[j] == ' ' && kind != Kind::Char && kind != Kind::Str {
        j += 1;
    }
    let digits = |from: usize, hex: bool| {
        let mut k = from;
        while k < rest.len() && (if hex { rest[k].is_ascii_hexdigit() } else { rest[k].is_ascii_digit() }) {
            k += 1;
        }
        k
    };
    match kind {
        Kind::Char => (!rest.is_empty()).then_some(vec![1]).unwrap_or_default(),
        Kind::Str => (0..=rest.len()).collect(),
        Kind::Int | Kind::Num => {
            let mut k = j;
            if k < rest.len() && (rest[k] == '+' || rest[k] == '-') {
                k += 1;
            }
            let start = k;
            let end = if kind == Kind::Int {
                digits(k, false)
            } else {
                let mut e = k;
                while e < rest.len() && (rest[e].is_ascii_digit() || rest[e] == '.') {
                    e += 1;
                }
                e
            };
            (start + 1..=end).rev().collect()
        }
        Kind::Uint | Kind::Hex => {
            let end = digits(j, kind == Kind::Hex);
            (j + 1..=end).rev().collect()
        }
    }
}
fn matches(segs: &[Seg], s: &[char], i: usize, caps: &mut Vec<String>) -> bool {
    let Some(seg) = segs.first() else { return i == s.len() };
    match seg {
        Seg::Lit(l) => s.len() >= i + l.len() && s[i..i + l.len()] == l[..] && matches(&segs[1..], s, i + l.len(), caps),
        Seg::Cap(k) => {
            for n in spans(*k, s, i) {
                caps.push(s[i..i + n].iter().collect());
                if matches(&segs[1..], s, i + n, caps) {
                    return true;
                }
                caps.pop();
            }
            false
        }
    }
}
fn setting_set(code: &str) -> Option<Set> {
    let t = data().get(code)?.get("s")?.as_object()?;
    let mut set = Set { exact: HashMap::new(), formats: Vec::new(), memo: HashMap::new() };
    for (en, tr) in t {
        let tr = tr.as_str().unwrap_or("");
        if en.contains('%') {
            let (segs, kinds) = compile(en);
            if kinds.is_empty() {
                set.exact.insert(fill(en, &[]), fill(tr, &[]));
            } else {
                set.formats.push(Format { segs, kinds, tr: tr.to_string() });
            }
        } else {
            set.exact.insert(en.clone(), tr.to_string());
        }
    }
    Some(set)
}
fn with<R>(f: impl FnOnce(&mut L) -> R) -> R {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let l = g.get_or_insert_with(|| L { code: "en-us".into(), sets: HashMap::new() });
    f(l)
}
/// L.set: the language the words are shown in.
pub(super) fn set(code: &str) {
    with(|l| l.code = if code.is_empty() { "en-us".into() } else { code.to_string() });
}
pub(super) fn code() -> String {
    with(|l| l.code.clone())
}
pub(super) fn ru() -> bool {
    with(|l| l.code == "ru-ru")
}
/// L.t: the menu's own word for the English.
pub(super) fn t(en: &str) -> String {
    let code = code();
    if code == "en-us" || code == "ru-ru" {
        return en.to_string();
    }
    data().get(&code).and_then(|c| c.get("t")).and_then(|m| m.get(en)).and_then(J::as_str).unwrap_or(en).to_string()
}
/// L.tr: Russian given inline, the rest through the table.
pub(super) fn tr(ru_text: &str, en: &str) -> String {
    if ru() {
        ru_text.to_string()
    } else {
        t(en)
    }
}
/// L.setting: a VR settings text in the language (an exact entry, or a format whose conversions it matches).
pub(super) fn setting(en: &str) -> String {
    if en.is_empty() {
        return en.to_string();
    }
    with(|l| {
        if l.code == "en-us" {
            return en.to_string();
        }
        let code = l.code.clone();
        let set = l.sets.entry(code.clone()).or_insert_with(|| setting_set(&code));
        let Some(set) = set.as_mut() else { return en.to_string() };
        if let Some(s) = set.exact.get(en).or_else(|| set.memo.get(en)) {
            return s.clone();
        }
        let chars: Vec<char> = en.chars().collect();
        let mut out = None;
        for f in &set.formats {
            let mut caps = Vec::new();
            if matches(&f.segs, &chars, 0, &mut caps) && caps.len() == f.kinds.len() {
                for (k, kind) in f.kinds.iter().enumerate() {
                    if *kind == Kind::Str {
                        if let Some(x) = set.exact.get(&caps[k]) {
                            caps[k] = x.clone();
                        }
                    }
                }
                out = Some(fill(&f.tr, &caps));
                break;
            }
        }
        let s = out.unwrap_or_else(|| en.to_string());
        if set.memo.len() > 4000 {
            set.memo.clear();
        }
        set.memo.insert(en.to_string(), s.clone());
        s
    })
}
/// L.count: n and its noun -- Russian's three written forms, another language's forms of the English singular
/// (`#<one>` in its table, chosen by the language's rule; {n} marks where the number goes), else English.
pub(super) fn count(n: i64, ru_one: &str, ru_few: &str, ru_many: &str, en_one: &str, en_other: &str) -> String {
    let code = code();
    if code == "ru-ru" {
        let (last, teen) = (n % 10, n % 100);
        let form = if (11..=14).contains(&teen) {
            ru_many
        } else if last == 1 {
            ru_one
        } else if (2..=4).contains(&last) {
            ru_few
        } else {
            ru_many
        };
        return format!("{n} {form}");
    }
    let forms: Option<Vec<String>> = data().get(&code).and_then(|c| c.get("t")).and_then(|m| m.get(format!("#{en_one}").as_str())).and_then(J::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());
    if let Some(forms) = forms.filter(|f| !f.is_empty()) {
        let i = match code.as_str() {
            "pl-pl" => {
                let (a, b) = (n % 10, n % 100);
                if n == 1 {
                    1
                } else if (2..=4).contains(&a) && !(12..=14).contains(&b) {
                    2
                } else {
                    3
                }
            }
            "cz-cz" => {
                if n == 1 {
                    1
                } else if (2..=4).contains(&n) {
                    2
                } else {
                    3
                }
            }
            "ua-ua" => {
                let (a, b) = (n % 10, n % 100);
                if a == 1 && b != 11 {
                    1
                } else if (2..=4).contains(&a) && !(12..=14).contains(&b) {
                    2
                } else {
                    3
                }
            }
            "ar-ar" => {
                let b = n % 100;
                if n == 0 {
                    6
                } else if n == 1 {
                    1
                } else if n == 2 {
                    2
                } else if (3..=10).contains(&b) {
                    3
                } else if b >= 11 {
                    4
                } else {
                    5
                }
            }
            "fr-fr" | "pt-br" => {
                if n == 0 || n == 1 {
                    1
                } else {
                    2
                }
            }
            _ => {
                if n == 1 {
                    1
                } else {
                    2
                }
            }
        };
        let form = forms.get(i.min(forms.len()) - 1).or(forms.last()).cloned().unwrap_or_default();
        if form.contains("{n}") {
            return form.replace("{n}", &n.to_string());
        }
        return format!("{n} {form}");
    }
    format!("{n} {}", t(if n == 1 { en_one } else { en_other }))
}
