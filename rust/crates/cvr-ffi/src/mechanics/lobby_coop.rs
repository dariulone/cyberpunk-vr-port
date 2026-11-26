//! The lobby menu's co-op page, shown when a multiplayer mod is in the game (NightCoop): the nickname, hosting and
//! joining -- over the internet by a code, on the local network by an address --, the session's state and leaving it.
//! These are NightCoop's own items in the game's main menu, which the lobby hides; here they call NightCoop's natives
//! (NightCoopName, NightCoopHostOnline / NightCoopJoinCode, NightCoopHost / NightCoopJoin, NightCoopStop,
//! NightCoopStatus) and show what each answered.
use super::lobby::Lobby;
use super::lobby_holo::{self as holo, Page, PageRef, Row};
use super::lobby_keyboard::{self as kb, Keys};
use super::lobby_menu::lang;
use super::lobby_spawn::{self as sp, Str};
use crate::script_call::Value;
use red4ext_abi::cname::CName;
use red4ext_abi::rtti::RttiSystem;
use std::cell::RefCell;
use std::rc::Rc;

/// NightCoop's port on the local network (its default).
const PORT: i32 = 47800;
const STATUS_EVERY: f64 = 1.0;
const LINE: usize = 64;
const NICK_MAX: usize = 24;

struct Texts {
    title: &'static str,
    sub: &'static str,
    nick: &'static str,
    nick_none: &'static str,
    online: &'static str,
    host_code: &'static str,
    join_code: &'static str,
    code: &'static str,
    lan: &'static str,
    host_lan: &'static str,
    join_addr: &'static str,
    addr: &'static str,
    leave: &'static str,
    idle: &'static str,
    hint: &'static str,
}
const EN: Texts = Texts {
    title: "Co-op",
    sub: "NightCoop",
    nick: "Nickname",
    nick_none: "not set",
    online: "Over the internet",
    host_code: "Host a session (get a code)",
    join_code: "Join by code",
    code: "Session code",
    lan: "Local network",
    host_lan: "Host on the local network",
    join_addr: "Join by address",
    addr: "Host's address",
    leave: "Leave the session",
    idle: "No session",
    hint: "The host then loads a save (Continue / Load game): the partners join its world.",
};
const RU: Texts = Texts {
    title: "Кооператив",
    sub: "NightCoop",
    nick: "Ник",
    nick_none: "не задан",
    online: "Через интернет",
    host_code: "Создать сессию (получить код)",
    join_code: "Войти по коду",
    code: "Код сессии",
    lan: "Локальная сеть",
    host_lan: "Создать сессию в локальной сети",
    join_addr: "Войти по адресу",
    addr: "Адрес хоста",
    leave: "Выйти из сессии",
    idle: "Нет сессии",
    hint: "Затем хост загружает сохранение (Продолжить / Загрузить игру): партнёры попадают в его мир.",
};
fn texts() -> &'static Texts {
    if lang() == "ru" {
        &RU
    } else {
        &EN
    }
}

/// A multiplayer mod the page drives is in the game.
pub(super) fn available() -> bool {
    cvr_rig::vrik::nightcoop::loaded()
}

/// NightCoop's global native `name`, its result as text (a String's, a Bool's, an Int32's); None when it is missing.
fn call(name: &str, args: &[Value]) -> Option<String> {
    let f = RttiSystem::get()?.get_function(CName::hashed(name))?;
    // SAFETY: the function's return property and its type (types live as long as the game).
    let ret = unsafe { f.base.return_type.as_ref().map(|r| (*r.ty).name().text().to_string()) }.unwrap_or_default();
    let out = match ret.as_str() {
        "String" => sp::native_string(name, args),
        "Bool" => sp::native::<bool>(name, args).map(|b| b.to_string()),
        "Int32" => sp::native::<i32>(name, args).map(|v| v.to_string()),
        _ => sp::native::<()>(name, args).map(|_| String::new()),
    };
    cvr_host::log!("[VRLobby] co-op: {name} -> {:?}\n", out.as_deref().map(|s| s.chars().take(160).collect::<String>()));
    out
}

fn nickname() -> Option<String> {
    let text = std::fs::read_to_string(cvr_rig::vrik::nightcoop::folder()?.join("name.txt")).ok()?;
    Some(text.trim().to_string()).filter(|s| !s.is_empty())
}

#[derive(Default)]
struct Ui {
    status: String,
    status_at: f64,
    /// What the last action answered, shown under the buttons.
    answer: String,
    address: String,
}

fn act(ui: &Rc<RefCell<Ui>>, name: &'static str, args: Vec<Arg>) -> impl Fn(&mut Lobby, usize) + 'static {
    let ui = ui.clone();
    move |_l, _| answer(&ui, name, &args)
}
/// A native's argument kept until the press.
#[derive(Clone)]
enum Arg {
    Text(String),
    Int(i32),
}
fn answer(ui: &Rc<RefCell<Ui>>, name: &str, args: &[Arg]) {
    let strs: Vec<Str> = args.iter().filter_map(|a| if let Arg::Text(s) = a { Some(Str::new(s)) } else { None }).collect();
    let mut texts = strs.iter();
    let values: Vec<Value> = args
        .iter()
        .map(|a| match a {
            Arg::Text(_) => texts.next().map_or(Value::Int32(0), |s| s.v()),
            Arg::Int(i) => Value::Int32(*i),
        })
        .collect();
    let out = call(name, &values).unwrap_or_else(|| format!("{name}: NightCoop does not have it"));
    let mut u = ui.borrow_mut();
    u.answer = out;
    u.status_at = 0.0;
}

/// NightCoopStatus ("role=guest connected=yes peerId=1 peers=1 rtt=0.5ms offset=...") said plainly; the raw line when
/// it does not read so.
fn status_line(raw: &str) -> String {
    let field = |k: &str| raw.split_whitespace().find_map(|w| w.strip_prefix(k).map(str::to_string));
    let ru = lang() == "ru";
    let Some(role) = field("role=") else { return raw.to_string() };
    let connected = field("connected=").as_deref() == Some("yes");
    let peers = field("peers=").unwrap_or_else(|| "0".into());
    let rtt = field("rtt=").unwrap_or_default();
    match (role.as_str(), connected) {
        ("offline", _) => texts().idle.into(),
        ("host", _) if ru => format!("Вы хост · партнёров: {peers}"),
        ("host", _) => format!("Hosting · partners: {peers}"),
        (_, true) if ru => format!("Подключено к хосту · пинг {rtt}"),
        (_, true) => format!("Connected to the host · ping {rtt}"),
        (_, false) if ru => "Подключение...".into(),
        (_, false) => "Connecting...".into(),
    }
}

fn text_rows(key: &str, text: &str, warn: bool) -> Vec<Row> {
    super::menu_settings::wrap(text, LINE)
        .into_iter()
        .enumerate()
        .map(|(i, line)| Row { text: true, warn, ..Row::new(&format!("{key}{i}"), &line) })
        .collect()
}

/// The co-op page.
pub(super) fn page() -> PageRef {
    let ui = Rc::new(RefCell::new(Ui::default()));
    let (u_rows, u_up) = (ui.clone(), ui);
    let t = texts();
    let mut p = Page::new(Some(t.title.into()), "NET//COOP", move |_l| {
        let t = texts();
        let ui = &u_rows;
        let status = ui.borrow().status.clone();
        let in_session = status.contains("connected=yes") || status.contains("role=host");
        let mut rows = text_rows("st", &if status.is_empty() { t.idle.to_string() } else { status_line(&status) }, false);
        let nick = nickname();
        let u_nick = ui.clone();
        rows.push(Row { detail: Some(nick.clone().unwrap_or_else(|| t.nick_none.into())), ..Row::new("nick", t.nick) }.go(move |l, _| {
            holo::push(l, nick_keyboard(Some(u_nick.clone())));
        }));
        rows.push(Row { header: true, ..Row::new("h_online", t.online) });
        rows.push(Row::new("host_code", t.host_code).go(act(ui, "NightCoopHostOnline", vec![])));
        let u_code = ui.clone();
        rows.push(Row::new("join_code", t.join_code).go(move |l, _| {
            let u = u_code.clone();
            let done: kb::Done = Rc::new(move |_l, text| {
                if !text.is_empty() {
                    answer(&u, "NightCoopJoinCode", &[Arg::Text(text.to_string())]);
                }
            });
            holo::push(l, kb::page(texts().code, "", 32, Keys::Text, done));
        }));
        rows.push(Row { header: true, ..Row::new("h_lan", t.lan) });
        rows.push(Row::new("host_lan", t.host_lan).go(act(ui, "NightCoopHost", vec![Arg::Int(PORT)])));
        let u_addr = ui.clone();
        rows.push(Row::new("join_addr", t.join_addr).go(move |l, _| {
            let u = u_addr.clone();
            let start = u.borrow().address.clone();
            let done: kb::Done = Rc::new(move |_l, text| {
                if text.is_empty() {
                    return;
                }
                // an address may carry its port ("192.168.0.11:47800")
                let (host, port) = match text.rsplit_once(':') {
                    Some((h, p)) if p.parse::<i32>().is_ok() => (h.to_string(), p.parse::<i32>().unwrap_or(PORT)),
                    _ => (text.to_string(), PORT),
                };
                u.borrow_mut().address = text.to_string();
                answer(&u, "NightCoopJoin", &[Arg::Text(host), Arg::Int(port)]);
            });
            holo::push(l, kb::page(texts().addr, &start, 40, Keys::Address, done));
        }));
        if in_session {
            rows.push(Row::new("leave", t.leave).go(act(ui, "NightCoopStop", vec![])));
        }
        let answer_text = ui.borrow().answer.clone();
        if !answer_text.is_empty() {
            rows.extend(text_rows("ans", &answer_text, answer_text.contains("NightCoopStop") || answer_text.contains("false")));
        }
        rows.extend(text_rows("hint", t.hint, false));
        rows
    });
    p.subtitle = Some(t.sub.into());
    p.update = Some(Rc::new(move |_l, now| {
        let mut u = u_up.borrow_mut();
        if now < u.status_at {
            return false;
        }
        u.status_at = now + STATUS_EVERY;
        drop(u);
        let status = call_quiet("NightCoopStatus").unwrap_or_default();
        let mut u = u_up.borrow_mut();
        if status == u.status {
            return false;
        }
        u.status = status;
        true
    }));
    Rc::new(p)
}

/// The nickname's keyboard: Done gives NightCoop the name (NightCoopName), its answer shown on the co-op page.
fn nick_keyboard(ui: Option<Rc<RefCell<Ui>>>) -> PageRef {
    let ui = ui.unwrap_or_default();
    let done: kb::Done = Rc::new(move |_l, text| {
        if !text.is_empty() {
            answer(&ui, "NightCoopName", &[Arg::Text(text.to_string())]);
        }
    });
    kb::page(texts().nick, &nickname().unwrap_or_default(), NICK_MAX, Keys::Text, done)
}
/// The nickname's keyboard on its own (the lobby's co-op test).
pub(super) fn nickname_keyboard() -> PageRef {
    nick_keyboard(None)
}

/// [`call`] without the log line (the status is asked every second).
fn call_quiet(name: &str) -> Option<String> {
    RttiSystem::get()?.get_function(CName::hashed(name))?;
    sp::native_string(name, &[])
}
