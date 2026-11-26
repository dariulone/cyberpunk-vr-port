//! CyberpunkVRPort_Menu/modules/messages.lua: the phone section of the VR menu -- the game's messenger on the menu's
//! glass, the way the phone shows it (newHudPhoneGameController): two tabs, MESSAGES and CONTACTS.
//!
//! MESSAGES: the threads (quest ones first with the "!", unread and waiting-for-a-reply ones lit, the rest dim; a reply
//! envelope or the time on the right; the last row shows or hides the read ones). A thread opens on the right: its
//! messages as the phone's bubbles, the reply options under them, CALL when the contact can be called. The answer that
//! comes back arrives as in the game -- "<name> is typing" from half its delay, then it lands -- heard through the
//! redscript watcher (PlayerPuppet.VRMenuJournalWatch).
//! CONTACTS: the callable and non-empty contacts; one opens on the right with CALL and MESSAGES.
use super::lobby_ink as ink;
use super::menu_core::*;
use super::menu_data::{self as d, Contact, Thread};
use super::menu_ink as k;
use red4ext_abi::handle::Handle;
use std::rc::Rc;

const GREEN: [f64; 3] = [0.36, 1.0, 0.72];
const SOFT: [f64; 3] = [1.0, 0.76, 0.72];
const ROW2: f64 = 124.0;
const BUBBLE_W: f64 = 640.0;
const FS: f64 = 34.0;
const LINE: f64 = 42.0;
const POLL: f64 = 0.25;
pub(super) const THREAD_W: f64 = 900.0;

#[derive(Clone, Copy)]
struct Typing {
    contact: i64,
    conversation: i64,
    player: bool,
    from: f64,
    till: f64,
}
/// The phone's state, kept across redraws (S.phone).
#[derive(Default)]
pub(super) struct Phone {
    contacts_tab: bool,
    show_read: bool,
    thread: Option<Thread>,
    contact: Option<Contact>,
    filter: Option<(i64, String)>,
    contacts: Option<Vec<Contact>>,
    typing: Option<Typing>,
    typing_shown: bool,
    thread_count: Option<usize>,
    thread_last_player: bool,
    dots: Vec<Handle>,
    poll_in: f64,
    seen: Option<(i64, i64)>,
    sent_at: Option<f64>,
}
fn same(a: Option<&Thread>, b: &Thread) -> bool {
    a.is_some_and(|a| a.hash == b.hash && a.conversation_hash == b.conversation_hash)
}

// ---------------------------------------------------------------- text
fn ulen(s: &str) -> usize {
    s.chars().count()
}
pub(super) fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.split('\n') {
        let (mut line, mut len) = (String::new(), 0usize);
        for word in para.split_whitespace() {
            let wl = ulen(word);
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
    while lines.len() > 1 && lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}
pub(super) fn chars_for(w: f64, fs: f64) -> usize {
    8f64.max((w / (fs * 0.50)).floor()) as usize
}

// ---------------------------------------------------------------- the tabs
fn tabs(m: &mut Menu, p: &Handle, w: f64) {
    let items = [(false, m.tx("messagesTab")), (true, m.tx("contactsTab"))];
    let (gap, y, h, x0) = (24.0, 26.0, HEAD - 40.0, PAD + 12.0);
    let tw = (w - x0 - 72.0 - gap) / 2.0;
    for (i, (contacts, label)) in items.into_iter().enumerate() {
        let x = x0 + i as f64 * (tw + gap);
        let on = m.phone.contacts_tab == contacts;
        let bg = k::round_fill(p, x, y, tw, h, if on { k::CYAN } else { k::ICE }, if on { 0.14 } else { 0.04 }, Some(h / 2.0));
        let edge = k::round_edge(p, x, y, tw, h, k::CYAN, if on { 0.9 } else { 0.0 }, Some(h / 2.0));
        let txt = k::text(p, x, y, tw, h, &label, 36.0, if on { k::WHITE } else { k::TEXT }, "center", false);
        button(
            m,
            "p1",
            &at(P1X, P1Y),
            x,
            y,
            tw,
            h,
            Some(Rc::new(move |m: &mut Menu, _| {
                if m.phone.contacts_tab == contacts {
                    return;
                }
                let ph = &mut m.phone;
                (ph.contacts_tab, ph.thread, ph.contact, ph.filter) = (contacts, None, None, None);
                super::menu_main::build_panels(m, true, false);
            })),
            Some(Rc::new(move |hover, _| {
                if on {
                    return;
                }
                ink::tint(&bg, if hover { k::YELLOW } else { k::ICE });
                ink::opacity(&bg, if hover { 0.22 } else { 0.04 });
                k::tint(&edge, k::CYAN, Some(if hover { 1.0 } else { 0.0 }));
                ink::tint(&txt, if hover { k::WHITE } else { k::TEXT });
            })),
        );
    }
}

// ---------------------------------------------------------------- the threads
fn thread_row(t: Thread, selected: bool, click: Act) -> Row {
    Row {
        h: ROW2,
        build: Rc::new(move |m: &mut Menu, itc: &Handle, it: RowCtx| {
            let (w, h) = (it.cw, ROW2 - 14.0);
            let c = ink::canvas(itc, 0.0, 0.0, w, h);
            let bg = k::round_fill(&c, 0.0, 0.0, w, h, k::ICE, 0.06, Some(R_CELL));
            let edge = k::round_edge(&c, 0.0, 0.0, w, h, k::ICE, 0.0, Some(R_CELL));
            let mut x = 26.0;
            if let Some(a) = &t.avatar {
                k::image_raw(&c, x, (h - 76.0) / 2.0, 76.0, 76.0, a.atlas, a.part, Some(k::WHITE), 1.0);
                x += 76.0 + 22.0;
            }
            let live = t.unread > 0 || t.can_reply;
            let name_col = if t.quest { k::YELLOW } else if live { k::CYAN } else { k::GREY };
            let mut nx = x;
            if t.quest {
                k::image(&c, x, 14.0, 36.0, 36.0, k::ATLAS_COMMON, "ico_exclamation_circle", Some(k::YELLOW), 1.0);
                nx = x + 46.0;
            }
            const RIGHT: f64 = 130.0;
            let name = k::text(&c, nx, 4.0, w - nx - RIGHT, 54.0, &k::clip(&k::upper(&t.name), 24), 40.0, name_col, "left", false);
            k::text(&c, x, 54.0, w - x - RIGHT, 48.0, &k::clip(&t.preview, 36), 32.0, if live { SOFT } else { k::GREY }, "left", true);
            if t.can_reply {
                k::image(&c, w - 40.0 - 56.0, (h - 40.0) / 2.0, 56.0, 40.0, k::ATLAS_COMMON, "ico_envelelope_reply1", Some(if t.quest { k::YELLOW } else { k::CYAN }), 1.0);
            } else {
                k::text(&c, w - RIGHT - 30.0, 0.0, RIGHT, h, &t.time, 32.0, k::GREY, "right", true);
            }
            if t.unread > 0 {
                k::image(&c, w - RIGHT - 48.0, (h - 18.0) / 2.0, 18.0, 18.0, k::ATLAS_PHONE, "circle", Some(if t.quest { k::YELLOW } else { k::CYAN }), 1.0);
            }
            let paint: Paint = Rc::new(move |hover, sel| {
                let lit = if hover || sel { Some(k::YELLOW) } else { None };
                ink::tint(&bg, lit.unwrap_or(k::ICE));
                ink::opacity(&bg, if hover { 0.24 } else if sel { 0.16 } else { 0.06 });
                k::tint(&edge, lit.unwrap_or(k::ICE), Some(if lit.is_some() { 1.0 } else { 0.0 }));
                ink::tint(&name, if hover { k::WHITE } else if sel { k::YELLOW } else { name_col });
            });
            let at = At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) };
            let b = button(m, "p1", &at, 0.0, 0.0, w, h, Some(click.clone()), Some(paint));
            b.selected.set(selected);
            b.repaint(false);
            if m.anim1 && it.first {
                slide_in(m, "p1", &c, 0.0, 0.0, 0.03 + it.index as f64 * 0.035, 0.0, 36.0, None);
            }
        }),
    }
}
fn messages_list(m: &mut Menu, p1: &Handle) {
    let ph = &m.phone;
    let threads = d::threads(ph.show_read || ph.filter.is_some(), ph.filter.as_ref().map(|f| f.0));
    let mut rows = Vec::new();
    if m.phone.filter.is_some() {
        rows.push(list_row(
            "p1",
            format!("‹  {}", m.tx("allMessages")),
            Rc::new(|m: &mut Menu, _| {
                (m.phone.filter, m.phone.thread) = (None, None);
                super::menu_main::build_panels(m, false, false);
            }),
            RowOpts { clip: Some(30), ..Default::default() },
        ));
    }
    for t in threads {
        let sel = same(m.phone.thread.as_ref(), &t);
        let t2 = t.clone();
        rows.push(thread_row(
            t,
            sel,
            Rc::new(move |m: &mut Menu, _| {
                if same(m.phone.thread.as_ref(), &t2) {
                    m.phone.thread = None;
                } else {
                    m.phone.thread = Some(t2.clone());
                }
                let open = m.phone.thread.is_some();
                super::menu_main::build_panels(m, false, open);
            }),
        ));
    }
    if m.phone.filter.is_none() {
        let label = if m.phone.show_read { m.tx("hideRead") } else { m.tx("showRead") };
        rows.push(list_row(
            "p1",
            label,
            Rc::new(|m: &mut Menu, _| {
                m.phone.show_read = !m.phone.show_read;
                super::menu_main::build_panels(m, false, false);
            }),
            RowOpts { clip: Some(32), ..Default::default() },
        ));
    }
    let (h, _) = list_size(&rows, LIST_H);
    glass(m, p1, P1W, HEAD + h.max(ROW) + 14.0, Some((P1X, P1Y)));
    tabs(m, p1, P1W);
    let keep = m.keep_scroll;
    list(m, p1, "p1", "p1", P1X, P1Y, HEAD, P1W - 2.0 * PAD, rows, LIST_H, keep);
}

// ---------------------------------------------------------------- a thread
fn bubble_row(msg: &d::Message) -> Row {
    let lines = wrap(&msg.text, chars_for(BUBBLE_W - 76.0, FS));
    let widest = lines.iter().map(|l| ulen(l)).max().unwrap_or(0) as f64;
    let text_h = if msg.text.is_empty() { 0.0 } else { lines.len() as f64 * LINE };
    let img_h = if msg.image.is_some() { 300.0 } else { 0.0 };
    let bh = 84f64.max(text_h + img_h + 44.0);
    let bw = if msg.image.is_some() { BUBBLE_W } else { BUBBLE_W.min(200f64.max((widest * FS * 0.50).floor() + 80.0)) };
    let (player, quest, image) = (msg.player, msg.quest, msg.image.clone());
    let text = lines.join("\n");
    Row {
        h: bh + 18.0,
        build: Rc::new(move |_m: &mut Menu, itc: &Handle, it: RowCtx| {
            let x = if player { it.cw - bw - 6.0 } else { 6.0 };
            let col = if quest { k::YELLOW } else if player { GREEN } else { k::CYAN };
            let part = if player { "msgBuble_reply" } else { "msgBuble" };
            k::slice(itc, x, 0.0, bw, bh, k::ATLAS_PHONE, &format!("{part}_bg"), Some(col), 0.12);
            k::slice(itc, x, 0.0, bw, bh, k::ATLAS_PHONE, &format!("{part}_fg"), Some(col), 0.85);
            if text_h > 0.0 {
                k::text(itc, x + 36.0, 20.0, bw - 72.0, text_h, &text, FS, if player { k::WHITE } else { SOFT }, "left", true);
            }
            if let Some(img) = &image {
                k::image_raw(itc, x + 30.0, 22.0 + text_h, bw - 60.0, img_h - 12.0, img.atlas, img.part, Some(k::WHITE), 1.0);
            }
        }),
    }
}
fn typing_row(name: String, player: bool) -> Row {
    Row {
        h: 130.0,
        build: Rc::new(move |m: &mut Menu, itc: &Handle, it: RowCtx| {
            let (bw, bh) = (170.0, 84.0);
            let x = if player { it.cw - bw - 6.0 } else { 6.0 };
            let col = if player { GREEN } else { k::CYAN };
            let part = if player { "msgBuble_reply" } else { "msgBuble" };
            if !player {
                let label = d::loc_key("UI-Phone-isTyping", "{contact_name}…").replace("{contact_name}", &name);
                k::text(itc, x + 10.0, 0.0, it.cw - 20.0, 40.0, &label, 28.0, k::CYAN, "left", true);
            }
            let y = 42.0;
            k::slice(itc, x, y, bw, bh, k::ATLAS_PHONE, &format!("{part}_bg"), Some(col), 0.10);
            k::slice(itc, x, y, bw, bh, k::ATLAS_PHONE, &format!("{part}_fg"), Some(col), 0.7);
            m.phone.dots = (0..3).map(|i| k::image(itc, x + 44.0 + f64::from(i) * 32.0, y + 32.0, 20.0, 20.0, k::ATLAS_PHONE, "circle", Some(col), 0.35)).collect();
        }),
    }
}
/// Whether the watcher's last scheduled message belongs to the open thread, and its typing window.
fn typing_for(m: &Menu, t: Option<&Thread>) -> Option<Typing> {
    let ty = m.phone.typing?;
    let t = t?;
    if ty.contact != t.hash || (t.multi && ty.conversation != 0 && ty.conversation != t.conversation_hash) {
        return None;
    }
    (m.time >= ty.from && m.time <= ty.till).then_some(ty)
}
fn reply_button(m: &mut Menu, p: &Handle, hh: &At, x: f64, y: f64, w: f64, r: &d::Message, send: Act) -> f64 {
    let lines = wrap(&r.text, chars_for(w - 80.0, FS));
    let h = 84f64.max(lines.len() as f64 * LINE + 40.0);
    let col = if r.quest { k::YELLOW } else { GREEN };
    let bg = k::slice(p, x, y, w, h, k::ATLAS_PHONE, "msgBuble_reply_bg", Some(col), 0.22);
    let fg = k::slice(p, x, y, w, h, k::ATLAS_PHONE, "msgBuble_reply_fg", Some(col), 1.0);
    let txt = k::text(p, x + 36.0, y + 20.0, w - 76.0, h - 40.0, &lines.join("\n"), FS, k::WHITE, "left", true);
    button(
        m,
        "p2",
        hh,
        x,
        y,
        w,
        h,
        Some(send),
        Some(Rc::new(move |hover, _| {
            ink::tint(&bg, if hover { k::YELLOW } else { col });
            ink::opacity(&bg, if hover { 0.34 } else { 0.22 });
            ink::tint(&fg, if hover { k::YELLOW } else { col });
            ink::tint(&txt, k::WHITE);
        })),
    );
    h
}
fn call_result(m: &mut Menu, ok: bool, why: &str) {
    let t = if ok { m.tx("calling") } else { m.t.get(why).cloned().unwrap_or_else(|| m.tx("blocked")) };
    toast(m, &t);
    if ok {
        m.close_at = Some(m.time + 0.9);
    }
}
fn thread_panel(m: &mut Menu, p2: &Handle, t: &Thread) {
    let w = THREAD_W;
    let data = Rc::new(d::thread(t));
    d::mark_read(&data.messages);
    m.phone.thread_count = Some(data.messages.len());
    m.phone.thread_last_player = data.messages.last().is_some_and(|x| x.player);
    m.phone.dots.clear();
    let h2 = at(P2X, P1Y);
    let mut rows: Vec<Row> = data.messages.iter().map(bubble_row).collect();
    let ty = typing_for(m, Some(t));
    m.phone.typing_shown = ty.is_some();
    if let Some(ty) = ty {
        rows.push(typing_row(t.name.clone(), ty.player));
    }
    let inner = w - 2.0 * PAD;
    let reply_w = (inner - 40.0).min(BUBBLE_W + 120.0);
    let reply_h: f64 = data.replies.iter().map(|r| 84f64.max(wrap(&r.text, chars_for(reply_w - 80.0, FS)).len() as f64 * LINE + 40.0) + 14.0).sum();
    let call_h = if t.callable { ROW + 14.0 } else { 0.0 };
    let avail = 300f64.max(LIST_H - reply_h - call_h);
    let list_h = list_size(&rows, avail).0.max(120.0);
    let total = HEAD + list_h + if reply_h > 0.0 { reply_h + 16.0 } else { 0.0 } + call_h + 20.0;
    glass(m, p2, w, total, Some((P2X, P1Y)));
    header(p2, w, &k::clip(&k::upper(&t.name), 26));
    let empty = rows.is_empty();
    list(m, p2, "p2", "p2", P2X, P1Y, HEAD, inner, rows, avail, Some(1e9));
    if empty {
        let nm = m.tx("noMessages");
        k::text(p2, PAD + 30.0, HEAD, inner - 60.0, ROW, &nm, 40.0, k::GREY, "left", false);
    }
    let mut y = HEAD + list_h + 16.0;
    for i in 0..data.replies.len() {
        let dd = data.clone();
        let hh = reply_button(
            m,
            p2,
            &h2,
            PAD + inner - reply_w,
            y,
            reply_w,
            &data.replies[i],
            Rc::new(move |m: &mut Menu, _| {
                if d::reply(&dd.replies, &dd.replies[i]) {
                    play(m, "notify");
                    m.phone.sent_at = Some(m.time);
                }
                super::menu_main::build_panels(m, false, false);
            }),
        );
        y += hh + 14.0;
    }
    if t.callable {
        let contact = Contact { name: t.name.clone(), contact_id: t.contact_id.clone(), callable: true, messages: true, unread: 0, hash: t.hash, conversation_hash: t.conversation_hash, avatar: None };
        let label = m.tx("call");
        action(
            m,
            p2,
            "p2",
            &h2,
            PAD,
            y,
            inner,
            ROW,
            &label,
            d::calling_enabled(),
            Rc::new(move |m: &mut Menu, _| {
                let (ok, why) = d::call(&contact);
                call_result(m, ok, why);
            }),
            Some(k::CYAN),
            glyph_of("call"),
        );
    }
}

// ---------------------------------------------------------------- contacts
fn initial(c: &Handle, x: f64, w: f64, h: f64, name: &str) -> Handle {
    let ch = name.chars().next().map_or("?".to_string(), |c| c.to_string());
    let s = w.min(h - 12.0);
    k::image(c, x + (w - s) / 2.0, (h - s) / 2.0, s, s, k::ATLAS_PHONE, "circle", Some(k::CYAN), 0.25);
    k::text(c, x, 0.0, w, h, &k::upper(&ch), 46.0, k::CYAN, "center", false)
}
fn contacts_list(m: &mut Menu, p1: &Handle) {
    if m.phone.contacts.is_none() {
        m.phone.contacts = Some(d::contacts());
    }
    let list_c = m.phone.contacts.clone().unwrap_or_default();
    let mut rows = Vec::new();
    for c in list_c {
        let (avatar, name) = (c.avatar.clone(), c.name.clone());
        let mut o = RowOpts {
            icon_w: Some(84.0),
            icon: Some(Rc::new(move |rc, x, _y, w, h| match &avatar {
                Some(a) => {
                    let s = w.min(h - 12.0);
                    k::round_fill(rc, x + (w - s) / 2.0 - 3.0, (h - s) / 2.0 - 3.0, s + 6.0, s + 6.0, k::CYAN, 0.35, Some(8.0));
                    k::image_raw(rc, x + (w - s) / 2.0, (h - s) / 2.0, s, s, a.atlas, a.part, Some(k::WHITE), 1.0)
                }
                None => initial(rc, x, w, h, &name),
            })),
            tint_icon: c.avatar.is_none(),
            selected: m.phone.contact.as_ref().is_some_and(|s| s.hash == c.hash),
            ..Default::default()
        };
        if c.unread > 0 {
            (o.right, o.right_w) = (Some(c.unread.to_string()), Some(90.0));
        }
        let label = c.name.clone();
        rows.push(list_row(
            "p1",
            label,
            Rc::new(move |m: &mut Menu, _| {
                let same = m.phone.contact.as_ref().is_some_and(|s| s.hash == c.hash);
                m.phone.contact = if same { None } else { Some(c.clone()) };
                let open = m.phone.contact.is_some();
                super::menu_main::build_panels(m, false, open);
            }),
            o,
        ));
    }
    let (h, _) = list_size(&rows, LIST_H);
    glass(m, p1, P1W, HEAD + h.max(ROW) + 14.0, Some((P1X, P1Y)));
    tabs(m, p1, P1W);
    if rows.is_empty() {
        let e = m.tx("empty");
        k::text(p1, 34.0, HEAD, P1W - 68.0, ROW, &e, 44.0, k::GREY, "left", false);
        return;
    }
    let keep = m.keep_scroll;
    list(m, p1, "p1", "p1", P1X, P1Y, HEAD, P1W - 2.0 * PAD, rows, LIST_H, keep);
}
fn contact_panel(m: &mut Menu, p2: &Handle, c: &Contact) {
    let w = P2W;
    let h2 = at(P2X, P1Y);
    glass(m, p2, w, HEAD + if c.avatar.is_some() { 216.0 } else { 0.0 } + 2.0 * (ROW + 30.0) + 10.0, Some((P2X, P1Y)));
    header(p2, w, &k::clip(&k::upper(&c.name), 18));
    let mut y = HEAD;
    if let Some(a) = &c.avatar {
        k::round_fill(p2, PAD, y, 196.0, 196.0, k::YELLOW, 0.8, Some(10.0));
        k::image_raw(p2, PAD + 3.0, y + 3.0, 190.0, 190.0, a.atlas, a.part, Some(k::WHITE), 1.0);
        y += 216.0;
    }
    let call_label = m.tx("call");
    let cc = c.clone();
    action(
        m,
        p2,
        "p2",
        &h2,
        PAD,
        y,
        w - 2.0 * PAD,
        ROW + 10.0,
        &call_label,
        c.callable && d::calling_enabled(),
        Rc::new(move |m: &mut Menu, _| {
            let (ok, why) = d::call(&cc);
            call_result(m, ok, why);
        }),
        None,
        glyph_of("call"),
    );
    y += ROW + 30.0;
    let msg_label = m.tx("messages");
    let (hash, name) = (c.hash, c.name.clone());
    action(
        m,
        p2,
        "p2",
        &h2,
        PAD,
        y,
        w - 2.0 * PAD,
        ROW + 10.0,
        &msg_label,
        c.messages,
        Rc::new(move |m: &mut Menu, _| {
            (m.phone.contacts_tab, m.phone.filter, m.phone.contact) = (false, Some((hash, name.clone())), None);
            let mut list = d::threads(true, Some(hash));
            m.phone.thread = if list.len() == 1 { list.pop() } else { None };
            let open = m.phone.thread.is_some();
            super::menu_main::build_panels(m, true, open);
        }),
        None,
        glyph_of("message"),
    );
}

// ---------------------------------------------------------------- driven by the menu
pub(super) fn build(m: &mut Menu, p1: &Handle, p2: &Handle) {
    if m.phone.contacts_tab {
        contacts_list(m, p1);
        if let Some(c) = m.phone.contact.clone() {
            contact_panel(m, p2, &c);
        }
    } else {
        messages_list(m, p1);
        if let Some(t) = m.phone.thread.clone() {
            thread_panel(m, p2, &t);
        }
    }
}
/// A quest's message link: the messages tab on that contact's thread.
pub(super) fn show_thread(m: &mut Menu, thread: Option<Thread>, contact: i64, name: &str) {
    m.phone = Phone { show_read: true, thread, filter: Some((contact, name.to_string())), ..Default::default() };
}
/// The section closed or another opened: the next visit starts from the lists, fresh.
pub(super) fn leave(m: &mut Menu) {
    let ph = &mut m.phone;
    (ph.thread, ph.contact, ph.filter, ph.contacts, ph.typing) = (None, None, None, None, None);
}
/// Every frame the section is open: the journal's counters; a change redraws the panels, a message scheduled for the
/// open thread shows the typing dots, a new one from the other side sounds as the phone's does.
pub(super) fn update(m: &mut Menu, dt: f64) -> bool {
    if !m.phone.dots.is_empty() {
        let lit = ((m.time * 3.0) % 3.0).floor() as usize;
        for (i, dd) in m.phone.dots.iter().enumerate() {
            ink::opacity(dd, if i == lit { 1.0 } else { 0.35 });
        }
    }
    m.phone.poll_in -= dt;
    if m.phone.poll_in > 0.0 {
        return false;
    }
    m.phone.poll_in = POLL;
    let mut redraw = false;
    if let Some(w) = d::journal_watch() {
        let (updates, scheduled) = (w.updates, w.scheduled);
        if let Some((seen_u, seen_s)) = m.phone.seen {
            if scheduled != seen_s {
                let delay = w.last_delay;
                let now = m.time;
                m.phone.typing = w.last_typing.then(|| Typing {
                    contact: w.last_contact,
                    conversation: w.last_conversation,
                    player: w.last_from_player,
                    from: now + delay * 0.5,
                    till: now + delay + 0.5,
                });
            }
            if updates != seen_u {
                redraw = true;
                if m.phone.typing.is_some_and(|t| m.time >= t.from) {
                    m.phone.typing = None;
                }
                m.phone.contacts = None;
            }
        }
        m.phone.seen = Some((updates, scheduled));
    }
    let now_typing = typing_for(m, m.phone.thread.as_ref()).is_some();
    if now_typing != m.phone.typing_shown {
        redraw = true;
    }
    if !redraw {
        return false;
    }
    let before = m.phone.thread.as_ref().and(m.phone.thread_count);
    super::menu_main::build_panels(m, false, false);
    if let Some(b) = before {
        if m.phone.thread.is_some() && m.phone.thread_count.unwrap_or(0) > b && !m.phone.thread_last_player {
            game_sound("ui_messenger_recieved");
        }
    }
    true
}
