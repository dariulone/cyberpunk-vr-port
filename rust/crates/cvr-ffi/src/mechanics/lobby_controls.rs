//! modules/controls.lua (main's fb8f5156a / df0f5ebdf / 503885d7b): the controls guide in the lobby -- the README's
//! Controls (General, On Foot, Driving) as the controllers' glyphs beside what they do: a Controls page in the lobby's
//! menu with all of it, and a board standing in the hall with the major ones, in view while the player walks about and
//! tries them.
//!
//! THE GLYPHS ARE THE README'S ICONS (docs/controls/icons: Kenney's Meta Quest glyphs and the VR gestures drawn to
//! match), without their tiles, white on clear in one atlas -- vrport\ui\vr_controls_icons.inkatlas, a part per icon
//! named as the icon -- drawn tinted in the panels' own light. The texts in every game language
//! (lobby_controls_texts.json, made from main's controls_text.lua): a binding's English action is its key, the
//! translated label first and the optional hint second; English lives with the glyphs here.
//!
//! A binding is a list of tokens: an icon's name, or "+" / "/" between two (drawn as text).
use super::lobby_holo::{Heading, Page, PageRef, Palette, Row, Spot};
use super::lobby_ink::{self as ink, C3};
use red4ext_abi::handle::Handle;
use serde_json::Value as J;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

// ---------------------------------------------------------------- the glyphs
const ATLAS: &str = "vrport\\ui\\vr_controls_icons.inkatlas";
const ICONS: &[&str] = &[
    "a", "b", "x", "y", "menu", "lt", "rt", "lg", "rg", "ls", "ls_up", "ls_down", "ls_vertical", "ls_press", "rs", "rs_up", "rs_down",
    "rs_left", "rs_right", "rs_horizontal", "rs_vertical", "rs_press", "ear_left", "hold", "squat", "swing", "holster", "throw",
    "wheel_grab", "steer", "horn",
];
fn is_icon(name: &str) -> bool {
    ICONS.contains(&name)
}
/// M.glyph: one icon in the box (x, y, size square), tinted.
pub(super) fn glyph(parent: &Handle, name: &str, x: f64, y: f64, size: f64, col: C3, op: f64) -> Option<Handle> {
    is_icon(name).then(|| ink::image(parent, x, y, size, size, ATLAS, name, col, op))
}
/// M.strip: a binding's tokens left to right from (x, y), each glyph `size` square and a "+" or "/" between them
/// narrower; the width they take.
#[allow(clippy::too_many_arguments)]
pub(super) fn strip(parent: &Handle, tokens: &[&str], x: f64, y: f64, size: f64, col: C3, sep: C3) -> f64 {
    let mut cx = x;
    for t in tokens {
        if is_icon(t) {
            glyph(parent, t, cx, y, size, col, 1.0);
            cx += size + size * 0.08;
        } else {
            ink::text(parent, cx, y, size * 0.42, size, t, (size * 0.5 + 0.5).floor(), sep, "centre", true, None);
            cx += size * 0.5;
        }
    }
    cx - x
}

// ---------------------------------------------------------------- the guide
/// A binding: its glyphs, what it does (the texts' key), a word on it; `board`: shown on the standing board.
struct Entry {
    tokens: &'static [&'static str],
    action: &'static str,
    hint: Option<&'static str>,
    board: bool,
}
struct Section {
    title: &'static str,
    entries: &'static [Entry],
}
const fn e(tokens: &'static [&'static str], action: &'static str, hint: Option<&'static str>, board: bool) -> Entry {
    Entry { tokens, action, hint, board }
}
/// The README's Controls, in the same order.
static SECTIONS: &[Section] = &[
    Section {
        title: "General",
        entries: &[
            e(&["ls_press", "+", "rs_press", "hold"], "VR menu", Some("hold both sticks in for half a second"), true),
            e(&["ls_press", "+", "rs_vertical"], "Dialogue choices", Some("hold L3, right stick up or down to the stop; X picks (A in a car)"), true),
            e(&["menu"], "Pause menu", Some("left on Touch; right on Steam Frame"), false),
            e(&["ls_press", "+", "rs_vertical", "rs_horizontal"], "D-pad", Some("hold L3, right stick to the stop; let go alone for a plain L3"), false),
        ],
    },
    Section {
        title: "On Foot",
        entries: &[
            e(&["ls"], "Move", Some("one speed, the direction from the stick"), false),
            e(&["ls_up"], "Sprint", Some("to the stop, held 0.2 s"), true),
            e(&["rs_up"], "Dash", Some("to the stop, once per push"), true),
            e(&["rs_horizontal"], "Turn", Some("snap or smooth"), true),
            e(&["rs_down"], "Crouch", Some("to the stop"), false),
            e(&["squat"], "Crouch in the room", Some("squat; the stick still works"), true),
            e(&["ear_left", "+", "ls_press"], "Scanner", Some("left controller to your left ear, click; again to close"), true),
            e(&["ls_vertical"], "Quickhack list", Some("to the stop, scanner open"), false),
            e(&["rt", "/", "lt"], "Fire / Aim", Some("the left trigger also blocks"), true),
            e(&["holster", "rg"], "Draw / holster", Some("the right grip at a holster"), true),
            e(&["b"], "Drop the magazine", Some("With a gun drawn, on foot or in a vehicle (driver or passenger)"), true),
            e(&["lg"], "Grab the magazine", Some("during a reload"), true),
            e(&["rs_press"], "Rack the slide", Some("physical reload"), true),
            e(&["x"], "Reload / interact", None, true),
            e(&["a"], "Jump", None, true),
            e(&["y"], "Next weapon", Some("tap"), false),
            e(&["y", "hold"], "Weapon wheel", Some("point and let go of Y"), true),
            e(&["ls_press", "+", "lg", "hold"], "Cyberware", Some("Sandevistan, Berserk, overclock"), false),
            e(&["swing"], "Melee", Some("swing the weapon"), true),
            e(&["lt", "+", "rg", "throw"], "Throw a melee weapon", Some("hold LT, grip it, throw and open the hand"), false),
        ],
    },
    Section {
        title: "Driving",
        entries: &[
            e(&["wheel_grab", "lg", "/", "rg"], "Grab the wheel", Some("a grip with the hand at the wheel or the bars"), true),
            e(&["steer"], "Steer", Some("tilt the line through both hands"), true),
            e(&["horn"], "Horn", Some("a hand on the middle of the wheel"), false),
            e(&["rt", "/", "lt"], "Throttle / Brake", Some("with a weapon drawn RT fires"), true),
            e(&["x", "hold"], "Get out", Some("a hold, never B"), true),
            e(&["a"], "Confirm a dialogue line", Some("X is the exit here"), false),
        ],
    },
];

/// Where the board stands: to the right of the menu and its preview window, turned to the eyes (lobby frame, degrees
/// from the heading the menu is on, metres).
pub(super) const BOARD: Spot = Spot { bearing: 82.0, dist: 1.25, height: 1.50 };

const GLYPH: f64 = 124.0; // a glyph's square on a page's row (px)
const ROW_H: f64 = 168.0;

static TEXTS: OnceLock<J> = OnceLock::new();
/// The game language's texts (en-us's for a language without its own) and the language they are.
fn texts() -> (&'static J, String) {
    let all = TEXTS.get_or_init(|| serde_json::from_str(include_str!("lobby_controls_texts.json")).unwrap_or(J::Null));
    let lang = ink::language();
    match all.get(&lang) {
        Some(t) => (t, lang),
        None => (all.get("en-us").unwrap_or(&J::Null), "en-us".into()),
    }
}
fn tx(t: &J, key: &str) -> String {
    t.get(key).and_then(J::as_str).unwrap_or("").to_string()
}
/// M.title: the guide's name in the game's language (the main menu's row).
pub(super) fn title() -> String {
    tx(texts().0, "title")
}
fn section_title(t: &J, sec: &Section) -> String {
    t.get(sec.title).and_then(J::as_str).unwrap_or(sec.title).to_string()
}
/// A binding's label and hint: the language's, else the English beside the glyphs.
fn binding_text(t: &J, entry: &Entry) -> (String, Option<String>) {
    match t.get(entry.action).and_then(J::as_array) {
        Some(tr) => (
            tr.first().and_then(J::as_str).unwrap_or("").to_string(),
            tr.get(1).and_then(J::as_str).map(str::to_string),
        ),
        None => (entry.action.to_string(), entry.hint.map(str::to_string)),
    }
}

/// The Controls page: every section, a header and its rows -- the glyphs on the left, what it does and a word on it.
fn page_rows(t: &J) -> Vec<Row> {
    let full_width = t.get("fullWidth").and_then(J::as_bool) == Some(true);
    let mut rows = Vec::new();
    for sec in SECTIONS {
        rows.push(Row { header: true, ..Row::new(&format!("h_{}", sec.title), &section_title(t, sec)) });
        for (i, entry) in sec.entries.iter().enumerate() {
            let (label, hint) = binding_text(t, entry);
            let tokens = entry.tokens;
            let draw = move |c: &Handle, w: f64, h: f64, p: &Palette| {
                let sw = 640.0;
                strip(c, tokens, 20.0, (h - GLYPH) / 2.0, GLYPH, p.glyph, p.dim);
                let tw = w - sw - 60.0;
                ink::text(c, sw + 40.0, 12.0, tw, h * 0.5, &label, ink::fit(&label, tw, 72.0, 48.0), p.text, "left", false, None);
                if let Some(hint) = &hint {
                    // ink::fit estimates Latin characters at 0.55 em: a full em for CJK hints.
                    let fit_width = if full_width { tw * 0.55 } else { tw };
                    ink::text(c, sw + 40.0, h * 0.5, tw, h * 0.42, hint, ink::fit(hint, fit_width, 50.0, 26.0), p.dim, "left", true, None);
                }
                ink::rect(c, 0.0, h - 3.0, w, 3.0, p.line, 1.0);
            };
            rows.push(Row { height: Some(ROW_H), draw: Some(Rc::new(draw)), ..Row::new(&format!("{}{}", sec.title, i + 1), "") });
        }
    }
    rows
}
/// M.page: the Controls page, its rows and heading made again when the game's language changes.
pub(super) fn page() -> PageRef {
    let shown = RefCell::new(texts().1);
    let mut p = Page::new(None, "SYS//CONTROLS", |_l| page_rows(texts().0));
    p.heading = Some(Rc::new(|| {
        let t = texts().0;
        let title = tx(t, "title");
        Heading { code: Some(format!("{}//{}", tx(t, "system"), title)), title: Some(title), subtitle: Some(tx(t, "subtitle")), back: Some(tx(t, "back")) }
    }));
    p.update = Some(Rc::new(move |_l, _time| {
        let now = texts().1;
        if *shown.borrow() == now {
            return false;
        }
        *shown.borrow_mut() = now;
        true
    }));
    Rc::new(p)
}

/// M.drawBoard: the board's face -- the major ones in two columns, under the section they belong to, on a canvas
/// cw x ch.
pub(super) fn draw_board(layer: &Handle, cw: f64, ch: f64, c: &Palette) {
    let t = texts().0;
    let (x0, top, bottom, gap) = (120.0, 300.0, ch - 110.0, 70.0);
    let title = tx(t, "title");
    ink::text(layer, x0, 70.0, cw - 2.0 * x0, 150.0, &title, 118.0, c.white, "left", false, None);
    ink::text(layer, x0, 70.0, cw - 2.0 * x0, 70.0, &format!("{}//{}", tx(t, "vr"), title), 40.0, c.dim, "right", false, None);
    ink::rect(layer, x0, 245.0, cw - 2.0 * x0, 4.0, c.red, 0.8);
    let col_w = ((cw - 2.0 * x0 - gap) / 2.0).floor();
    enum Item {
        Header(String),
        Entry(&'static Entry),
    }
    let mut items = Vec::new();
    for sec in SECTIONS {
        let mine: Vec<&'static Entry> = sec.entries.iter().filter(|e| e.board).collect();
        if !mine.is_empty() {
            items.push(Item::Header(section_title(t, sec)));
            items.extend(mine.into_iter().map(Item::Entry));
        }
    }
    let (head, entry_h, g) = (76.0, 124.0, 104.0);
    let (mut col, mut y) = (0.0, top);
    for it in &items {
        let h = if matches!(it, Item::Header(_)) { head } else { entry_h };
        if y + h > bottom && col == 0.0 {
            (col, y) = (1.0, top);
        }
        let x = x0 + col * (col_w + gap);
        match it {
            Item::Header(text) => {
                ink::text(layer, x, y + 10.0, col_w, 56.0, text, 50.0, c.red, "left", false, None);
                ink::rect(layer, x, y + h - 6.0, col_w, 3.0, c.line, 1.0);
            }
            Item::Entry(entry) => {
                let sw = strip(layer, entry.tokens, x, y + (h - g) / 2.0, g, c.glyph, c.dim);
                let lx = x + sw.max(4.0 * g) + 30.0;
                let lw = x + col_w - lx;
                let (label, _) = binding_text(t, entry);
                ink::text(layer, lx, y, lw, h, &label, ink::fit(&label, lw, 58.0, 36.0), c.text, "left", false, None);
            }
        }
        y += h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn all() -> J {
        serde_json::from_str(include_str!("lobby_controls_texts.json")).expect("the texts parse")
    }
    #[test]
    fn every_language_names_every_binding_and_section() {
        let all = all();
        let langs = all.as_object().expect("languages");
        assert_eq!(langs.len(), 19);
        for (lang, t) in langs {
            for key in ["title", "subtitle", "back", "system", "vr"] {
                assert!(t.get(key).and_then(J::as_str).is_some_and(|s| !s.is_empty()), "{lang}: {key}");
            }
            if lang == "en-us" {
                continue;
            }
            for sec in SECTIONS {
                assert!(t.get(sec.title).and_then(J::as_str).is_some(), "{lang}: {}", sec.title);
                for entry in sec.entries {
                    let (label, hint) = binding_text(t, entry);
                    assert!(!label.is_empty(), "{lang}: {}", entry.action);
                    assert_eq!(hint.is_some(), entry.hint.is_some(), "{lang}: {}", entry.action);
                }
            }
        }
    }
    #[test]
    fn english_comes_from_the_glyphs_and_the_glyphs_are_icons_or_separators() {
        let all = all();
        let en = &all["en-us"];
        assert_eq!(binding_text(en, &SECTIONS[1].entries[13]), ("Reload / interact".to_string(), None));
        assert_eq!(binding_text(en, &SECTIONS[0].entries[0]).1.as_deref(), Some("hold both sticks in for half a second"));
        for sec in SECTIONS {
            for entry in sec.entries {
                assert!(entry.tokens.iter().all(|t| is_icon(t) || *t == "+" || *t == "/"), "{}", entry.action);
            }
        }
        let on_board: usize = SECTIONS.iter().map(|s| s.entries.iter().filter(|e| e.board).count()).sum();
        assert_eq!(on_board, 20);
    }
}
