//! CyberpunkVRPort_Menu/modules/settings.lua: the port's settings in the VR menu -- the pages of the ImGui panel, read
//! out by the plugin (menu_settings, settings_data.lua) and drawn as the menu's own rows; no setting is known by name.
//!
//! The page is polled while it is on the screen. A recording with the same rows as the one drawn only refreshes values
//! and texts in place; rows coming or going (a section folded, a mode that shows other controls) draw the list again
//! where it was scrolled to, once no hand is in it.
use super::lobby_ink as ink;
use super::lobby_scroll::{Style, View};
use super::menu_bits::ViewRef;
use super::menu_core::*;
use super::menu_ink as k;
use super::menu_settings::{self as sd, has, Item, PageRec};
use red4ext_abi::handle::Handle;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

const TAB_Y: f64 = 104.0;
const TAB_H: f64 = 80.0;
const TAB_GAP: f64 = 8.0;
const ARROW_H: f64 = 52.0;
const VIEW_Y: f64 = 196.0;
const VIEW_H: f64 = 800.0;
const DOWN_Y: f64 = VIEW_Y + VIEW_H + 8.0;
const INFO_Y: f64 = DOWN_Y + ARROW_H + 12.0;
const INFO_H: f64 = 136.0;
const PANEL_H: f64 = INFO_Y + INFO_H + 14.0;
const RH: f64 = 92.0;
const GAP: f64 = 10.0;
const INDENT: f64 = 40.0;
const FS_LABEL: f64 = 38.0;
const FS_VALUE: f64 = 36.0;
const FS_TEXT: f64 = 32.0;
const FS_INFO: f64 = 30.0;
const LINE: f64 = 40.0;
const SOFT: [f64; 3] = [1.0, 0.76, 0.72];
const HEAD_BH: f64 = 48.0;
const HEAD_FS: f64 = 28.0;

type Refresh = Rc<dyn Fn(&Item, &[String])>;
/// A row of the list: the item it shows (its wrapped lines for a text), held while its slider is dragged.
struct Slot {
    f: Item,
    lines: Vec<String>,
    held: Rc<Cell<bool>>,
    refresh: Option<Refresh>,
}
type SlotRef = Rc<RefCell<Slot>>;

/// The settings panel's state (settings.lua's ST).
#[derive(Default)]
pub(super) struct SetState {
    panel: bool,
    host: Handle,
    head: Handle,
    info: Handle,
    sx: f64,
    sy: f64,
    w: f64,
    view: Option<ViewRef>,
    current: Rc<RefCell<Option<ViewRef>>>,
    view_page: Option<i32>,
    page: i32,
    str_: Option<String>,
    cur: Option<PageRec>,
    sig: Option<String>,
    slots: Vec<SlotRef>,
    open: HashMap<String, bool>,
    offsets: HashMap<i32, f64>,
    poll_at: f64,
    fast_until: f64,
    sent: HashMap<String, f64>,
    queued: HashMap<String, (Item, f64)>,
    tip: Option<String>,
    rebuild_now: bool,
    tabs: Vec<BtnRef>,
    arrows: Vec<BtnRef>,
}

fn header_open(st: &SetState, f: &Item) -> bool {
    st.open.get(&f.id).copied().unwrap_or_else(|| has(f.flags, 8))
}
/// The push buttons a page opens with (Recenter, Save): in the panel's header instead of the list.
fn leading(items: &[Item]) -> (Vec<Item>, Vec<Item>) {
    let (mut lead, mut rest) = (Vec::new(), Vec::new());
    for f in items {
        if rest.is_empty() && f.kind == "B" && f.indent == 0.0 {
            lead.push(f.clone());
        } else {
            rest.push(f.clone());
        }
    }
    (lead, rest)
}
fn lua_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
/// What makes two recordings the same list of rows; values and texts may differ.
fn signature(st: &SetState, vis: &[(Item, Vec<String>)]) -> String {
    let parts: Vec<String> = vis
        .iter()
        .map(|(f, lines)| {
            let extra = match f.kind.as_str() {
                "T" => lines.len().to_string(),
                "O" => f.text.clone(),
                "P" => format!("{}={}", f.text, lua_num(f.value)),
                "S" | "I" => format!("{}..{}", lua_num(f.min), lua_num(f.max)),
                "H" => (if header_open(st, f) { "1" } else { "0" }).to_string(),
                _ => String::new(),
            };
            [f.kind.clone(), f.id.clone(), f.label.clone(), f.flags.to_string(), lua_num(f.indent), extra].join("\u{1}")
        })
        .collect();
    parts.join("\u{2}")
}
fn differs(a: &Item, b: &Item) -> bool {
    a.value != b.value || a.text != b.text || a.tip != b.tip
}
fn chars_for(width: f64, size: f64) -> usize {
    10f64.max((width / (size * 0.52)).floor()) as usize
}
fn text_color(f: &Item, dim: bool) -> [f64; 3] {
    if has(f.flags, 4) {
        let v = f.value.floor() as i64;
        return [((v / 65536) % 256) as f64 / 255.0, ((v / 256) % 256) as f64 / 255.0, (v % 256) as f64 / 255.0];
    }
    if dim || has(f.flags, 1) {
        k::GREY
    } else {
        SOFT
    }
}

// ---------------------------------------------------------------- talking to the plugin
fn send(m: &mut Menu, f: &Item, value: f64) {
    sd::edit(f, value);
    let now = m.time;
    let st = &mut m.setv;
    st.sent.insert(f.id.clone(), now);
    st.fast_until = now + 1.0;
    st.poll_at = st.poll_at.min(now + 0.05);
}
/// A slider moving sends at most ten values a second, and the one it is let go at.
fn queue(m: &mut Menu, f: &Item, v: f64, last: bool) {
    if last {
        m.setv.queued.remove(&f.id);
        send(m, f, v);
    } else {
        m.setv.queued.insert(f.id.clone(), (f.clone(), v));
    }
}
fn flush(m: &mut Menu, id: &str) {
    if let Some((f, v)) = m.setv.queued.remove(id) {
        send(m, &f, v);
    }
}
fn flush_due(m: &mut Menu, now: f64) {
    let due: Vec<String> = m.setv.queued.keys().filter(|id| now - m.setv.sent.get(*id).copied().unwrap_or(-1.0) >= 0.1).cloned().collect();
    for id in due {
        flush(m, &id);
    }
}
fn flush_all(m: &mut Menu) {
    let ids: Vec<String> = m.setv.queued.keys().cloned().collect();
    for id in ids {
        flush(m, &id);
    }
}

// ---------------------------------------------------------------- the rows
struct Ctx {
    hh: At,
    x: f64,
    w: f64,
    h: f64,
    dim: bool,
    held: Rc<Cell<bool>>,
    lit: Rc<dyn Fn(bool)>,
}
/// Every control of a row lights the row's cell along with itself.
fn join_row(ctx: &Ctx, b: &BtnRef, tip: &str) {
    let lit = ctx.lit.clone();
    *b.also.borrow_mut() = Some(Rc::new(move |h| lit(h)));
    *b.tip.borrow_mut() = Some(tip.to_string());
}
/// A row's cell, and under its controls a button of its own that is only pointed at: it lights the row and shows the
/// tooltip.
fn backing(m: &mut Menu, c: &Handle, ctx: &mut Ctx, f: &Item) {
    let (x, w, h) = (ctx.x, ctx.w, ctx.h);
    let col = if ctx.dim { k::GREY } else { k::NAVY };
    let (rest, rest_edge) = (if ctx.dim { 0.02 } else { 0.18 }, 0.15);
    let bg = k::round_fill(c, x, 0.0, w, h, col, rest, Some(4.0));
    let edge = k::cut_frame(&mut m.pool, c, x, 0.0, w, h, k::CYAN, rest_edge, Some(9.0), Some(1.0));
    let lit: Rc<dyn Fn(bool)> = Rc::new(move |hover| {
        ink::tint(&bg, if hover { k::YELLOW } else { col });
        ink::opacity(&bg, if hover { 0.16 } else { rest });
        k::tint(&edge, if hover { k::YELLOW } else { k::CYAN }, Some(if hover { 0.8 } else { rest_edge }));
    });
    ctx.lit = lit.clone();
    let b = button(m, "set", &ctx.hh, x, 0.0, w, h, None, Some(Rc::new(move |h, _| lit(h))));
    b.passive.set(true);
    *b.tip.borrow_mut() = Some(f.tip.clone());
}
fn label(c: &Handle, ctx: &Ctx, f: &Item, x: f64, w: f64) -> Handle {
    let n = 6f64.max((w / (FS_LABEL * 0.47)).floor()) as usize;
    k::text(c, x, 0.0, w, ctx.h, &k::clip(&f.label, n), FS_LABEL, if ctx.dim { k::GREY } else { k::TEXT }, "left", false)
}
fn tip_of(b: &BtnRef, f: &Item) {
    *b.tip.borrow_mut() = Some(f.tip.clone());
}
/// A section of the page: a lit band with its title and a chevron; it folds what follows.
fn kind_h(m: &mut Menu, c: &Handle, f: &Item, ctx: &Ctx) -> Option<Refresh> {
    let (x, w) = (ctx.x, ctx.w);
    let open = header_open(&m.setv, f);
    let rest = if open { 0.26 } else { 0.14 };
    let bg = k::round_fill(c, x, 14.0, w, 68.0, k::CYAN, rest, Some(R_CELL));
    k::text(c, x + 28.0, 14.0, w - 140.0, 68.0, &k::upper(&f.label), 42.0, k::WHITE, "left", false);
    let chev = chevron(c, x + w - 50.0, 48.0, 13.0, 6.0, if open { "down" } else { "right" }, k::CYAN, 1.0);
    let id = f.id.clone();
    let b = button(
        m,
        "set",
        &ctx.hh,
        x,
        14.0,
        w,
        68.0,
        Some(Rc::new(move |m: &mut Menu, _| {
            m.setv.open.insert(id.clone(), !open);
            m.setv.rebuild_now = true;
        })),
        Some(Rc::new(move |hover, _| {
            ink::tint(&bg, if hover { k::YELLOW } else { k::CYAN });
            ink::opacity(&bg, if hover { 0.34 } else { rest });
            k::tint(&chev, if hover { k::WHITE } else { k::CYAN }, None);
        })),
    );
    tip_of(&b, f);
    None
}
/// Text: lines of the panel's own, in its colour.
fn kind_t(m: &mut Menu, c: &Handle, f: &Item, lines: &[String], ctx: &Ctx) -> Option<Refresh> {
    let t = k::text(c, ctx.x + 28.0, 2.0, ctx.w - 56.0, lines.len() as f64 * LINE, &lines.join("\n"), FS_TEXT, text_color(f, ctx.dim), "left", false);
    if !f.tip.is_empty() {
        let b = button(m, "set", &ctx.hh, ctx.x, 0.0, ctx.w, ctx.h, None, None);
        b.passive.set(true);
        tip_of(&b, f);
    }
    let dim = ctx.dim;
    Some(Rc::new(move |nf, lines| {
        ink::set_text(&t, &lines.join("\n"));
        ink::tint(&t, text_color(nf, dim));
    }))
}
/// A checkbox: the whole row toggles; the switch on its right says ON / OFF.
fn kind_c(m: &mut Menu, c: &Handle, f: &Item, ctx: &mut Ctx) -> Option<Refresh> {
    let (x, w, h) = (ctx.x, ctx.w, ctx.h);
    backing(m, c, ctx, f);
    const TW: f64 = 150.0;
    let tx = x + w - TW - 16.0;
    label(c, ctx, f, x + 28.0, tx - x - 48.0);
    let tbg = k::round_fill(c, tx, 12.0, TW, h - 24.0, k::CYAN, 0.1, Some((h - 24.0) / 2.0));
    let tedge = k::round_edge(c, tx, 12.0, TW, h - 24.0, k::CYAN, 0.6, Some((h - 24.0) / 2.0));
    let ttxt = k::text(c, tx, 12.0, TW, h - 24.0, "", FS_VALUE, k::CYAN, "center", false);
    let st = Rc::new(Cell::new((f.value != 0.0, false)));
    let (on_w, off_w) = (m.tx("on"), m.tx("off"));
    let dim = ctx.dim;
    let s2 = st.clone();
    let paint: Rc<dyn Fn()> = Rc::new(move || {
        let (on, hover) = s2.get();
        let base = if dim { k::GREY } else if on { k::YELLOW } else { k::CYAN };
        let col = if hover { k::YELLOW } else { base };
        ink::tint(&tbg, col);
        ink::opacity(&tbg, if on { 0.40 } else if hover { 0.20 } else { 0.06 });
        k::tint(&tedge, col, Some(if on || hover { 1.0 } else { 0.55 }));
        ink::set_text(&ttxt, if on { &on_w } else { &off_w });
        ink::tint(&ttxt, if on || hover { k::WHITE } else { base });
    });
    paint();
    if !ctx.dim {
        let (s3, p3, f3) = (st.clone(), paint.clone(), f.clone());
        let (s4, p4, lit) = (st.clone(), paint.clone(), ctx.lit.clone());
        let b = button(
            m,
            "set",
            &ctx.hh,
            x,
            0.0,
            w,
            h,
            Some(Rc::new(move |m: &mut Menu, _| {
                let (on, hv) = s3.get();
                s3.set((!on, hv));
                p3();
                send(m, &f3, if on { 0.0 } else { 1.0 });
            })),
            Some(Rc::new(move |hv, _| {
                s4.set((s4.get().0, hv));
                p4();
                lit(hv);
            })),
        );
        tip_of(&b, f);
    }
    Some(Rc::new(move |nf, _| {
        st.set((nf.value != 0.0, st.get().1));
        paint();
    }))
}
/// A slider: the label, the value as the panel prints it, then - [track] +.
fn kind_s(m: &mut Menu, c: &Handle, f: &Item, ctx: &mut Ctx) -> Option<Refresh> {
    let (x, w, h) = (ctx.x, ctx.w, ctx.h);
    backing(m, c, ctx, f);
    let (lw, vw, sb) = ((w * 0.36).floor(), 200.0, 80.0);
    label(c, ctx, f, x + 28.0, lw - 28.0);
    let val = k::text(c, x + lw, 0.0, vw, h, &f.text, FS_VALUE, if ctx.dim { k::GREY } else { k::YELLOW }, "right", false);
    let mx = x + lw + vw + 28.0;
    let px = x + w - sb - 12.0;
    let sx = mx + sb + 46.0;
    let sw = px - 46.0 - sx;
    let (lo, hi) = (f.min.min(f.max), f.min.max(f.max));
    let step = sd::step_of(f);
    let cur = Rc::new(Cell::new(f.value));
    let sl: Rc<RefCell<Option<BtnRef>>> = Rc::new(RefCell::new(None));
    let item = Rc::new(RefCell::new(f.clone()));
    let (c2, sl2, val2, it2) = (cur.clone(), sl.clone(), val.clone(), item.clone());
    let set: Rc<dyn Fn(&mut Menu, f64, bool) -> f64> = Rc::new(move |m, v, last| {
        let fi = it2.borrow().clone();
        let v = sd::snap(&fi, v, lo, step).clamp(lo, hi);
        c2.set(v);
        ink::set_text(&val2, &sd::fmt(&fi, v));
        let show = sl2.borrow().as_ref().and_then(|s| s.show.borrow().clone());
        if let Some(show) = show {
            show(v);
        }
        queue(m, &fi, v, last);
        v
    });
    if ctx.dim {
        let t = if hi > lo { ((cur.get() - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
        k::rect(c, sx, h / 2.0 - 5.0, sw, 10.0, k::GREY, 0.25);
        k::rect(c, sx, h / 2.0 - 5.0, sw * t, 10.0, k::GREY, 0.6);
    } else {
        let (s1, c1) = (set.clone(), cur.clone());
        let b = step_button(m, c, "set", &ctx.hh, mx, 10.0, sb, h - 20.0, Err(false), true, Rc::new(move |m: &mut Menu, _| {
            s1(m, c1.get() - step, true);
        }));
        join_row(ctx, &b, &f.tip);
        let (s1, c1) = (set.clone(), cur.clone());
        let b = step_button(m, c, "set", &ctx.hh, px, 10.0, sb, h - 20.0, Err(true), true, Rc::new(move |m: &mut Menu, _| {
            s1(m, c1.get() + step, true);
        }));
        join_row(ctx, &b, &f.tip);
        let s1 = set.clone();
        let (held, id) = (ctx.held.clone(), f.id.clone());
        let b = slider(
            m,
            c,
            "set",
            &ctx.hh,
            sx,
            0.0,
            sw,
            h,
            lo,
            hi,
            Some(cur.get()),
            Rc::new(move |m: &mut Menu, v| Some(s1(m, v, false))),
            Some(step),
            Some(Rc::new(move |m: &mut Menu, phase: &str| {
                held.set(phase == "begin");
                if phase == "end" {
                    flush(m, &id);
                }
            })),
        );
        join_row(ctx, &b, &f.tip);
        *sl.borrow_mut() = Some(b);
    }
    let held = ctx.held.clone();
    Some(Rc::new(move |nf, _| {
        if held.get() {
            return;
        }
        *item.borrow_mut() = nf.clone();
        cur.set(nf.value);
        ink::set_text(&val, &nf.text);
        let show = sl.borrow().as_ref().and_then(|s| s.show.borrow().clone());
        if let Some(show) = show {
            show(nf.value);
        }
    }))
}
/// A number field: - value +, by the field's own step.
fn kind_n(m: &mut Menu, c: &Handle, f: &Item, ctx: &mut Ctx) -> Option<Refresh> {
    let (x, w, h) = (ctx.x, ctx.w, ctx.h);
    backing(m, c, ctx, f);
    let (sb, vw) = (80.0, 200.0);
    let px = x + w - sb - 12.0;
    let vx = px - 12.0 - vw;
    let mx = vx - 12.0 - sb;
    label(c, ctx, f, x + 28.0, mx - x - 48.0);
    let step = if f.min != 0.0 { f.min } else { 1.0 };
    let cur = Rc::new(Cell::new(f.value));
    let val = k::text(c, vx, 0.0, vw, h, &f.text, FS_VALUE, if ctx.dim { k::GREY } else { k::YELLOW }, "center", false);
    let (c2, v2, f2) = (cur.clone(), val.clone(), f.clone());
    let set: Rc<dyn Fn(&mut Menu, f64)> = Rc::new(move |m, v| {
        c2.set(v);
        ink::set_text(&v2, &((v + 0.5).floor() as i64).to_string());
        send(m, &f2, v);
    });
    if !ctx.dim {
        for (bx, sign, plus) in [(mx, -1.0, false), (px, 1.0, true)] {
            let (s1, c1) = (set.clone(), cur.clone());
            let b = step_button(m, c, "set", &ctx.hh, bx, 10.0, sb, h - 20.0, Err(plus), true, Rc::new(move |m: &mut Menu, _| s1(m, c1.get() + sign * step)));
            join_row(ctx, &b, &f.tip);
        }
    }
    Some(Rc::new(move |nf, _| {
        cur.set(nf.value);
        ink::set_text(&val, &nf.text);
    }))
}
/// A combo: < option >, the option itself steps forward too.
fn kind_o(m: &mut Menu, c: &Handle, f: &Item, ctx: &mut Ctx) -> Option<Refresh> {
    let (x, w, h) = (ctx.x, ctx.w, ctx.h);
    backing(m, c, ctx, f);
    let (sb, ow) = (80.0, 460f64.min((w * 0.34).floor()));
    let rx = x + w - sb - 12.0;
    let ox = rx - 10.0 - ow;
    let lx = ox - 10.0 - sb;
    label(c, ctx, f, x + 28.0, lx - x - 48.0);
    let state = Rc::new(RefCell::new((sd::options(f), (f.value + 0.5).floor() as i64)));
    let col = if ctx.dim { k::GREY } else { k::CYAN };
    let obg = k::round_fill(c, ox, 10.0, ow, h - 20.0, col, 0.10, Some(R_CELL));
    let oedge = k::round_edge(c, ox, 10.0, ow, h - 20.0, col, 0.5, Some(R_CELL));
    let otxt = k::text(c, ox, 10.0, ow, h - 20.0, "", FS_VALUE, if ctx.dim { k::GREY } else { k::YELLOW }, "center", false);
    let (s2, t2) = (state.clone(), otxt.clone());
    let show: Rc<dyn Fn()> = Rc::new(move || {
        let s = s2.borrow();
        let o = usize::try_from(s.1).ok().and_then(|i| s.0.get(i)).map_or("?", String::as_str);
        ink::set_text(&t2, &k::clip(o, 4f64.max((ow / (FS_VALUE * 0.5)).floor()) as usize));
    });
    show();
    let (s3, sh3, f3) = (state.clone(), show.clone(), f.clone());
    let pick: Rc<dyn Fn(&mut Menu, i64)> = Rc::new(move |m, dd| {
        let idx = {
            let mut s = s3.borrow_mut();
            let n = s.0.len() as i64;
            if n == 0 {
                return;
            }
            s.1 = (s.1 + dd).rem_euclid(n);
            s.1
        };
        sh3();
        send(m, &f3, idx as f64);
    });
    if !ctx.dim {
        let p1 = pick.clone();
        let b = step_button(m, c, "set", &ctx.hh, lx, 10.0, sb, h - 20.0, Ok("left"), true, Rc::new(move |m: &mut Menu, _| p1(m, -1)));
        join_row(ctx, &b, &f.tip);
        let p2 = pick.clone();
        let b = button(
            m,
            "set",
            &ctx.hh,
            ox,
            10.0,
            ow,
            h - 20.0,
            Some(Rc::new(move |m: &mut Menu, _| p2(m, 1))),
            Some(Rc::new(move |hv, _| {
                ink::tint(&obg, if hv { k::YELLOW } else { k::CYAN });
                ink::opacity(&obg, if hv { 0.28 } else { 0.10 });
                k::tint(&oedge, if hv { k::YELLOW } else { k::CYAN }, Some(if hv { 1.0 } else { 0.5 }));
                ink::tint(&otxt, if hv { k::WHITE } else { k::YELLOW });
            })),
        );
        join_row(ctx, &b, &f.tip);
        let p3 = pick.clone();
        let b = step_button(m, c, "set", &ctx.hh, rx, 10.0, sb, h - 20.0, Ok("right"), true, Rc::new(move |m: &mut Menu, _| p3(m, 1)));
        join_row(ctx, &b, &f.tip);
    }
    Some(Rc::new(move |nf, _| {
        *state.borrow_mut() = (sd::options(nf), (nf.value + 0.5).floor() as i64);
        show();
    }))
}
/// Tabs inside a page: one segment each, the chosen one yellow.
fn kind_p(m: &mut Menu, c: &Handle, f: &Item, ctx: &Ctx) -> Option<Refresh> {
    let (x, w, h) = (ctx.x, ctx.w, ctx.h);
    let opts = sd::options(f);
    let (n, gap) = (opts.len().max(1) as f64, 8.0);
    let tw = (w - (n - 1.0) * gap) / n;
    let sel = (f.value + 0.5).floor() as i64;
    for (i, name) in opts.iter().enumerate() {
        let tx = x + i as f64 * (tw + gap);
        let on = sel == i as i64;
        let base = if ctx.dim { k::GREY } else if on { k::YELLOW } else { k::CYAN };
        let bg = k::round_fill(c, tx, 0.0, tw, h, base, if on { 0.30 } else { 0.08 }, Some(R_CELL));
        let edge = k::round_edge(c, tx, 0.0, tw, h, base, if on { 1.0 } else { 0.45 }, Some(R_CELL));
        let txt = k::text(c, tx, 0.0, tw, h, &k::clip(name, 4f64.max((tw / (34.0 * 0.5)).floor()) as usize), 34.0, if on { k::WHITE } else if ctx.dim { k::GREY } else { k::TEXT }, "center", false);
        if !ctx.dim && !on {
            let f2 = f.clone();
            let b = button(
                m,
                "set",
                &ctx.hh,
                tx,
                0.0,
                tw,
                h,
                Some(Rc::new(move |m: &mut Menu, _| send(m, &f2, i as f64))),
                Some(Rc::new(move |hv, _| {
                    ink::tint(&bg, if hv { k::YELLOW } else { k::CYAN });
                    ink::opacity(&bg, if hv { 0.28 } else { 0.08 });
                    k::tint(&edge, if hv { k::YELLOW } else { k::CYAN }, Some(if hv { 1.0 } else { 0.45 }));
                    ink::tint(&txt, if hv { k::WHITE } else { k::TEXT });
                })),
            );
            tip_of(&b, f);
        }
    }
    None
}
/// A push button.
fn kind_b(m: &mut Menu, c: &Handle, f: &Item, ctx: &Ctx) -> Option<Refresh> {
    let (x, w, h) = (ctx.x, ctx.w, ctx.h);
    let bw = w.min(760.0);
    let col = if ctx.dim { k::GREY } else { k::CYAN };
    let bg = k::round_fill(c, x, 0.0, bw, h, col, if ctx.dim { 0.04 } else { 0.14 }, Some(R_CELL));
    let edge = k::round_edge(c, x, 0.0, bw, h, col, if ctx.dim { 0.3 } else { 0.9 }, Some(R_CELL));
    let txt = k::text(c, x, 0.0, bw, h, &k::clip(&k::upper(&f.label), (bw / (FS_LABEL * 0.55)).floor() as usize), FS_LABEL, col, "center", false);
    if !ctx.dim {
        let f2 = f.clone();
        let b = button(m, "set", &ctx.hh, x, 0.0, bw, h, Some(Rc::new(move |m: &mut Menu, _| send(m, &f2, 1.0))), Some(push_paint(bg, edge, txt)));
        tip_of(&b, f);
    }
    None
}
fn push_paint(bg: Handle, edge: Vec<Handle>, txt: Handle) -> Paint {
    Rc::new(move |hv, _| {
        ink::tint(&bg, if hv { k::YELLOW } else { k::CYAN });
        ink::opacity(&bg, if hv { 0.30 } else { 0.14 });
        k::tint(&edge, if hv { k::YELLOW } else { k::CYAN }, Some(if hv { 1.0 } else { 0.9 }));
        ink::tint(&txt, if hv { k::WHITE } else { k::CYAN });
    })
}
/// A progress bar.
fn kind_g(c: &Handle, f: &Item, ctx: &Ctx) -> Option<Refresh> {
    let (x, w) = (ctx.x + 28.0, ctx.w - 56.0);
    k::round_fill(c, x, 10.0, w, 16.0, k::CYAN, 0.25, Some(8.0));
    let fill = k::round_fill(c, x, 10.0, w * f.value.clamp(0.0, 1.0), 16.0, k::YELLOW, 0.9, Some(8.0));
    Some(Rc::new(move |nf, _| k::round_size(&fill, w * nf.value.clamp(0.0, 1.0), 16.0, Some(8.0))))
}
/// A binding: the input in yellow, what it does after it.
fn kind_k(c: &Handle, f: &Item, ctx: &Ctx) -> Option<Refresh> {
    let (x, w, h) = (ctx.x, ctx.w, ctx.h);
    let iw = (w * 0.40).floor();
    k::text(c, x + 28.0, 0.0, iw - 28.0, h, &k::clip(&f.label, ((iw - 28.0) / (34.0 * 0.5)).floor() as usize), 34.0, k::YELLOW, "left", false);
    k::text(c, x + iw, 0.0, w - iw - 28.0, h, &k::clip(&f.text, ((w - iw - 28.0) / (34.0 * 0.5)).floor() as usize), 34.0, SOFT, "left", false);
    k::rect(c, x + 28.0, h - 2.0, w - 56.0, 2.0, k::CYAN, 0.18);
    None
}
fn height_of(f: &Item, lines: &[String]) -> f64 {
    match f.kind.as_str() {
        "H" => 96.0,
        "T" => lines.len() as f64 * LINE + 14.0,
        "X" => 26.0,
        "G" => 46.0,
        "K" => 56.0,
        _ => RH,
    }
}
fn build_item(m: &mut Menu, c: &Handle, it: &RowCtx, slot: &SlotRef) {
    let (f, lines, held) = {
        let s = slot.borrow();
        (s.f.clone(), s.lines.clone(), s.held.clone())
    };
    let x = f.indent * INDENT;
    let mut ctx = Ctx { hh: At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) }, x, w: it.cw - x, h: it.h - if f.kind == "H" { 0.0 } else { GAP }, dim: has(f.flags, 16), held, lit: Rc::new(|_| {}) };
    let refresh = match f.kind.as_str() {
        "H" => kind_h(m, c, &f, &ctx),
        "T" => kind_t(m, c, &f, &lines, &ctx),
        "C" => kind_c(m, c, &f, &mut ctx),
        "S" | "I" => kind_s(m, c, &f, &mut ctx),
        "N" => kind_n(m, c, &f, &mut ctx),
        "O" => kind_o(m, c, &f, &mut ctx),
        "P" => kind_p(m, c, &f, &ctx),
        "B" => kind_b(m, c, &f, &ctx),
        "G" => kind_g(c, &f, &ctx),
        "K" => kind_k(c, &f, &ctx),
        "X" => {
            k::rect(c, ctx.x + 20.0, 6.0, ctx.w - 40.0, 2.0, k::CYAN, 0.35);
            None
        }
        _ => None,
    };
    slot.borrow_mut().refresh = refresh;
}
/// The page's leading push buttons, small, right-aligned on the header's line.
fn draw_head(m: &mut Menu, lead: &[Item]) {
    let head = m.setv.head.clone();
    if head.is_null() {
        return;
    }
    ink::remove_all(&head);
    clear_buttons(m, "sethead");
    let mut x1 = m.setv.w - 76.0;
    let hh = at(m.setv.sx, m.setv.sy);
    for f in lead.iter().rev() {
        let label = k::upper(&f.label);
        let bw = (label.chars().count() as f64 * HEAD_FS * 0.55).floor() + 44.0;
        let x = x1 - bw;
        let y = ((HEAD - HEAD_BH) / 2.0).floor() + 2.0;
        let dim = has(f.flags, 16);
        let col = if dim { k::GREY } else { k::CYAN };
        let bg = k::round_fill(&head, x, y, bw, HEAD_BH, col, if dim { 0.04 } else { 0.14 }, Some(HEAD_BH / 2.0));
        let edge = k::round_edge(&head, x, y, bw, HEAD_BH, col, if dim { 0.3 } else { 0.9 }, Some(HEAD_BH / 2.0));
        let txt = k::text(&head, x, y, bw, HEAD_BH, &label, HEAD_FS, col, "center", false);
        if !dim {
            let f2 = f.clone();
            let b = button(m, "sethead", &hh, x, y, bw, HEAD_BH, Some(Rc::new(move |m: &mut Menu, _| send(m, &f2, 1.0))), Some(push_paint(bg, edge, txt)));
            tip_of(&b, f);
        }
        x1 = x - 14.0;
    }
}

// ---------------------------------------------------------------- the list
fn repaint(m: &Menu, list: &[BtnRef]) {
    for b in list {
        let hov = m.hovered.iter().any(|h| Rc::ptr_eq(h, b));
        b.repaint(hov);
    }
}
/// A hand pressing, dragging or scrolling in the list: it holds the list's widgets.
fn busy(m: &Menu) -> bool {
    let Some(v) = m.setv.view.as_ref() else { return false };
    m.hands.iter().any(|p| {
        p.pend.as_ref().is_some_and(|g| Rc::ptr_eq(&g.view, v)) || p.scroll.as_ref().is_some_and(|s| Rc::ptr_eq(&s.0, v)) || p.drag.as_ref().and_then(|b| b.view.as_ref()).is_some_and(|bv| Rc::ptr_eq(bv, v))
    })
}
fn rebuild(m: &mut Menu, vis: Vec<(Item, Vec<String>)>) {
    if m.setv.host.is_null() {
        return;
    }
    let st = &m.setv;
    let offset = match (&st.view, st.view_page == Some(st.page)) {
        (Some(old), true) => old.borrow().offset,
        _ => st.offsets.get(&st.page).copied().unwrap_or(0.0),
    };
    if let Some(old) = m.setv.view.take() {
        drop_view(m, &old);
    }
    let host = m.setv.host.clone();
    ink::remove_all(&host);
    let (sx, sy, w) = (m.setv.sx, m.setv.sy, m.setv.w);
    let mut v = View::new(&host, PAD, VIEW_Y, sx + PAD, sy + VIEW_Y, w - 2.0 * PAD, VIEW_H, Style { track: k::CYAN, thumb: k::CYAN, held: k::YELLOW });
    let mut slots = Vec::new();
    let mut builds: Vec<RowBuild> = Vec::new();
    let empty = vis.is_empty();
    for (f, lines) in vis {
        v.add(height_of(&f, &lines));
        let slot: SlotRef = Rc::new(RefCell::new(Slot { f, lines, held: Rc::new(Cell::new(false)), refresh: None }));
        let s2 = slot.clone();
        builds.push(Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| build_item(m, c, &it, &s2)));
        slots.push(slot);
    }
    if empty && m.setv.cur.is_some() {
        let e = m.tx("empty");
        k::text(&host, PAD + 34.0, VIEW_Y, w - 2.0 * PAD - 68.0, 100.0, &e, 40.0, k::GREY, "left", false);
    }
    v.scroll_to(offset);
    let vr: ViewRef = Rc::new(RefCell::new(v));
    m.setv.slots = slots;
    m.setv.view = Some(vr.clone());
    *m.setv.current.borrow_mut() = Some(vr.clone());
    m.setv.view_page = Some(m.setv.page);
    let arrows = m.setv.arrows.clone();
    m.views.retain(|(k0, _)| k0 != "set");
    m.views.push(("set".into(), MView { v: vr, builds, section: m.section.clone(), arrows: arrows.clone(), first: true, last_offset: f64::NAN }));
    update_view(m, "set", 0.0, 0);
    repaint(m, &arrows);
}
/// Draws a recording: in place when it has the rows already drawn, else as a new list. False when the list could not
/// be drawn again yet (a hand is in it).
fn show(m: &mut Menu, pg: PageRec, force: bool, anyway: bool) -> bool {
    let cw = m.setv.w - 2.0 * PAD - 18.0;
    let (lead, rest) = leading(&pg.items);
    let st = &m.setv;
    let vis: Vec<(Item, Vec<String>)> = sd::visible(&rest, &|f| header_open(st, f))
        .into_iter()
        .map(|f| {
            let lines = if f.kind == "T" { sd::wrap(&f.text, chars_for(cw - f.indent * INDENT - 56.0, FS_TEXT)) } else { Vec::new() };
            (f.clone(), lines)
        })
        .collect();
    let lead_vis: Vec<(Item, Vec<String>)> = lead.iter().map(|f| (f.clone(), Vec::new())).collect();
    let sig = format!("{}\u{3}{}", signature(st, &vis), signature(st, &lead_vis));
    if force || m.setv.view.is_none() || m.setv.sig.as_deref() != Some(sig.as_str()) {
        if busy(m) && !anyway {
            return false;
        }
        (m.setv.cur, m.setv.sig) = (Some(pg), Some(sig));
        draw_head(m, &lead);
        rebuild(m, vis);
        return true;
    }
    m.setv.cur = Some(pg);
    let now = m.time;
    for (i, (f, lines)) in vis.into_iter().enumerate() {
        let Some(slot) = m.setv.slots.get(i).cloned() else { continue };
        let (changed, held) = {
            let s = slot.borrow();
            (differs(&s.f, &f), s.held.get())
        };
        if !changed {
            continue;
        }
        let quiet = held || m.setv.queued.contains_key(&f.id) || m.setv.sent.get(&f.id).is_some_and(|t| now - t < 0.4);
        if !quiet {
            let refresh = {
                let mut s = slot.borrow_mut();
                (s.f, s.lines) = (f.clone(), lines.clone());
                s.refresh.clone()
            };
            if let Some(r) = refresh {
                r(&f, &lines);
            }
        }
    }
    true
}
fn load_page(m: &mut Menu) {
    let page = m.setv.page;
    let s = sd::fetch(page);
    match s.as_deref().and_then(sd::parse).filter(|pg| pg.page == page) {
        Some(pg) => {
            m.setv.str_ = s;
            show(m, pg, true, true);
        }
        None => rebuild(m, Vec::new()),
    }
}
fn set_page(m: &mut Menu, i: i32) {
    if i == m.setv.page || i < 0 || i >= sd::PAGES.len() as i32 {
        return;
    }
    flush_all(m);
    if let Some(v) = &m.setv.view {
        let off = v.borrow().offset;
        m.setv.offsets.insert(m.setv.page, off);
    }
    m.setv.page = i;
    (m.setv.str_, m.setv.sig, m.setv.cur) = (None, None, None);
    PAGE.with(|pc| pc.set(i));
    let tabs = m.setv.tabs.clone();
    repaint(m, &tabs);
    load_page(m);
    m.setv.poll_at = 0.0;
}

// ---------------------------------------------------------------- driven by the menu
/// The panel, in canvas p whose top-left is (sx, sy) on the screen, w wide.
pub(super) fn build(m: &mut Menu, p: &Handle, sx: f64, sy: f64, w: f64) {
    {
        let st = &mut m.setv;
        (st.panel, st.host, st.sx, st.sy, st.w) = (false, Handle::null(), sx, sy, w);
        (st.view, st.view_page, st.sig, st.str_, st.cur, st.tip) = (None, None, None, None, None, None);
        st.slots.clear();
        st.head = Handle::null();
        *st.current.borrow_mut() = None;
    }
    glass(m, p, w, PANEL_H, Some((sx, sy)));
    let title = m.tx("settings");
    header(p, w, &title);
    m.setv.head = ink::canvas(p, 0.0, 0.0, w, HEAD);
    let n = sd::PAGES.len();
    let tw = (w - 2.0 * PAD - (n as f64 - 1.0) * TAB_GAP) / n as f64;
    PAGE.with(|pc| pc.set(m.setv.page));
    let mut tabs = Vec::new();
    for i in 1..=n {
        let x = PAD + (i as f64 - 1.0) * (tw + TAB_GAP);
        let bg = k::round_fill(p, x, TAB_Y, tw, TAB_H, k::ICE, 0.04, Some(TAB_H / 2.0));
        let edge = k::round_edge(p, x, TAB_Y, tw, TAB_H, k::ICE, 0.0, Some(TAB_H / 2.0));
        let under = k::rect(p, x + 12.0, TAB_Y + TAB_H - 10.0, tw - 24.0, 4.0, k::YELLOW, 0.0);
        let txt = k::text(p, x, TAB_Y, tw, TAB_H, &sd::page_name(i), 34.0, k::TEXT, "center", false);
        let page = i as i32 - 1;
        // the selection is read from the page shown (PAGE), the tabs repainted when it changes
        let paint: Paint = Rc::new(move |hover, _| {
            let sel = PAGE.with(|pc| pc.get()) == page;
            let lit = if hover || sel { Some(k::YELLOW) } else { None };
            ink::tint(&bg, lit.unwrap_or(k::ICE));
            ink::opacity(&bg, if hover { 0.30 } else if sel { 0.22 } else { 0.04 });
            k::tint(&edge, lit.unwrap_or(k::ICE), Some(if lit.is_some() { 1.0 } else { 0.0 }));
            ink::opacity(&under, if sel { 1.0 } else { 0.0 });
            ink::tint(&txt, if hover { k::WHITE } else if sel { k::YELLOW } else { k::TEXT });
        });
        tabs.push(button(m, "set", &at(sx, sy), x, TAB_Y, tw, TAB_H, Some(Rc::new(move |m: &mut Menu, _| set_page(m, page))), Some(paint)));
    }
    m.setv.tabs = tabs;
    let tabs = m.setv.tabs.clone();
    repaint(m, &tabs);
    let cur = m.setv.current.clone();
    let current: Rc<dyn Fn() -> Option<ViewRef>> = Rc::new(move || cur.borrow().clone());
    let aw = (w - 2.0 * PAD - 14.0) / 2.0;
    m.setv.arrows = vec![
        arrow_bar_of(m, p, "set", &at(sx, sy), PAD, DOWN_Y, aw, ARROW_H, true, current.clone()),
        arrow_bar_of(m, p, "set", &at(sx, sy), (w + 14.0) / 2.0, DOWN_Y, aw, ARROW_H, false, current),
    ];
    m.setv.host = ink::canvas(p, 0.0, 0.0, w, PANEL_H);
    k::round_fill(p, PAD, INFO_Y, w - 2.0 * PAD, INFO_H, k::ICE, 0.04, Some(R_CELL));
    let hint = m.tx("hint");
    m.setv.info = k::text(p, PAD + 28.0, INFO_Y + 8.0, w - 2.0 * PAD - 56.0, INFO_H - 16.0, &hint, FS_INFO, k::GREY, "left", false);
    m.setv.panel = true;
    m.setv.poll_at = 0.0;
    load_page(m);
}
thread_local! {
    /// The page shown, for the tabs' paint (they are repainted when it changes).
    static PAGE: Cell<i32> = const { Cell::new(0) };
}
/// The panel is going (another section, a redraw, the menu closing): what a slider still holds goes out.
pub(super) fn leave(m: &mut Menu) {
    flush_all(m);
    if let Some(v) = &m.setv.view {
        let off = v.borrow().offset;
        m.setv.offsets.insert(m.setv.page, off);
    }
    let st = &mut m.setv;
    (st.panel, st.host, st.view, st.info, st.head) = (false, Handle::null(), None, Handle::null(), Handle::null());
    st.tabs.clear();
    st.arrows.clear();
    st.slots.clear();
    *st.current.borrow_mut() = None;
    st.rebuild_now = false;
}
pub(super) fn update(m: &mut Menu, _dt: f64) {
    if !m.setv.panel {
        return;
    }
    PAGE.with(|p| p.set(m.setv.page));
    let now = m.time;
    flush_due(m, now);
    if m.setv.rebuild_now {
        if let Some(cur) = m.setv.cur.clone() {
            if show(m, cur, true, false) {
                m.setv.rebuild_now = false;
            }
        }
    }
    if now < m.setv.poll_at {
        return;
    }
    m.setv.poll_at = now + if now < m.setv.fast_until || m.setv.cur.is_none() { 0.08 } else { 0.30 };
    let page = m.setv.page;
    let Some(s) = sd::fetch(page).filter(|s| !s.is_empty()) else { return };
    if m.setv.str_.as_deref() == Some(s.as_str()) {
        return;
    }
    let Some(pg) = sd::parse(&s).filter(|pg| pg.page == page) else { return };
    if show(m, pg, false, false) {
        m.setv.str_ = Some(s);
    }
}
/// The panel's tooltips flattened: a blank line is a paragraph, a single break a space.
fn flatten(tip: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let flush_run = |out: &mut String, run: &mut String| {
        let nl = run.matches('\n').count();
        out.push_str(match nl {
            0 => run.as_str(),
            1 => " ",
            _ => "\n",
        });
        run.clear();
    };
    for ch in tip.chars() {
        if ch.is_whitespace() {
            run.push(ch);
        } else {
            if !run.is_empty() {
                flush_run(&mut out, &mut run);
            }
            out.push(ch);
        }
    }
    if !run.is_empty() {
        flush_run(&mut out, &mut run);
    }
    out
}
/// The tooltip of what a pointer is on, in the box under the list.
pub(super) fn hover(m: &mut Menu, hovered: &[BtnRef]) {
    if m.setv.info.is_null() {
        return;
    }
    let mut tip: Option<String> = None;
    for b in hovered {
        if let Some(t) = b.tip.borrow().as_ref().filter(|t| !t.is_empty()) {
            tip = Some(t.clone());
            if !b.passive.get() {
                break;
            }
        }
    }
    if tip == m.setv.tip {
        return;
    }
    m.setv.tip = tip.clone();
    let mut lines = vec![m.tx("hint")];
    if let Some(t) = &tip {
        lines = sd::wrap(&flatten(t), chars_for(m.setv.w - 2.0 * PAD - 56.0, FS_INFO));
        if lines.len() > 3 {
            lines = vec![lines[0].clone(), lines[1].clone(), format!("{} …", lines[2])];
        }
    }
    ink::set_text(&m.setv.info, &lines.join("\n"));
    ink::tint(&m.setv.info, if tip.is_some() { SOFT } else { k::GREY });
}
