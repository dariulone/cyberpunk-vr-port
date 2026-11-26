//! A keyboard on the holo for the text the lobby asks for (a co-op nickname, a join code, an address): the text typed
//! so far with its caret, the keys a row each, and the line's own keys -- case, space, erase, done. Done goes back to
//! the page that asked and hands it the text.
use super::lobby::Lobby;
use super::lobby_holo::{self as holo, Act, Key, Page, PageRef, Row};
use super::lobby_menu::lang;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Keys {
    /// Letters, digits and - _ . (a nickname, a code).
    Text,
    /// Digits, the dot and the colon (an address).
    Address,
}
pub(super) type Done = Rc<dyn Fn(&mut Lobby, &str)>;

struct Entry {
    text: String,
    upper: bool,
    max: usize,
}

/// Space, erase, done, and the case key's two states (the holo's key font draws every letter as a capital, so the
/// case key names the case the letters type in).
fn words() -> [&'static str; 5] {
    if lang() == "ru" {
        ["Пробел", "Стереть", "Готово", "строчные", "ЗАГЛАВНЫЕ"]
    } else {
        ["Space", "Erase", "Done", "lower case", "UPPER CASE"]
    }
}

fn typing(e: &Rc<RefCell<Entry>>, ch: char) -> Act {
    let e = e.clone();
    Rc::new(move |_l, _| {
        let mut s = e.borrow_mut();
        if s.text.chars().count() < s.max {
            let ch = if s.upper { ch.to_ascii_uppercase() } else { ch };
            s.text.push(ch);
        }
    })
}

fn line(e: &Rc<RefCell<Entry>>, key: &str, chars: &str) -> Row {
    let upper = e.borrow().upper;
    let keys = chars
        .chars()
        .map(|ch| Key { label: if upper { ch.to_ascii_uppercase() } else { ch }.to_string(), units: 1.0, go: typing(e, ch) })
        .collect();
    Row { keys: Some(keys), ..Row::new(key, "") }
}

/// The keyboard's page: `title`, the text it starts with, at most `max` characters, `done` given the text (trimmed).
pub(super) fn page(title: &str, initial: &str, max: usize, keys: Keys, done: Done) -> PageRef {
    let e = Rc::new(RefCell::new(Entry { text: initial.chars().take(max).collect(), upper: false, max }));
    let p = Page::new(Some(title.to_string()), "SYS//INPUT", move |_l| {
        let [space, erase, ok, lower, upper] = words();
        let case = if e.borrow().upper { upper } else { lower };
        // a text row: its font keeps the case the holo's row labels drop
        let mut rows = vec![Row { text: true, ..Row::new("entry", &format!("> {}_", e.borrow().text)) }];
        let lines: &[(&str, &str)] = match keys {
            Keys::Text => &[("k1", "1234567890"), ("k2", "qwertyuiop"), ("k3", "asdfghjkl-"), ("k4", "zxcvbnm_.")],
            Keys::Address => &[("k1", "1234567890"), ("k2", ".:")],
        };
        for (key, chars) in lines {
            rows.push(line(&e, key, chars));
        }
        let mut last = Vec::new();
        if keys == Keys::Text {
            let e2 = e.clone();
            last.push(Key { label: case.into(), units: 3.0, go: Rc::new(move |_l, _| {
                let mut s = e2.borrow_mut();
                s.upper = !s.upper;
            }) });
            last.push(Key { label: space.into(), units: 3.0, go: typing(&e, ' ') });
        }
        let e3 = e.clone();
        last.push(Key { label: erase.into(), units: 2.0, go: Rc::new(move |_l, _| {
            e3.borrow_mut().text.pop();
        }) });
        let (e4, done) = (e.clone(), done.clone());
        last.push(Key { label: ok.into(), units: 2.5, go: Rc::new(move |l, _| {
            let text = e4.borrow().text.trim().to_string();
            holo::back(l);
            done(l, &text);
        }) });
        rows.push(Row { keys: Some(last), ..Row::new("k9", "") });
        rows
    });
    Rc::new(p)
}
