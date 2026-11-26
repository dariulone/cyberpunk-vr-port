//! journal_view.lua, wait_view.lua and summary_view.lua: the quests (filtered active / main / side / completed, the
//! tracked one marked, the distance to the nearest objective updated in place as V walks), a quest's details (its
//! active objectives -- a press tracks one --, the message threads and shards they link to, the description, the codex
//! entries it links to, opened over the panels), the shards by group (encrypted chips cracked through their own
//! action); the wait's clock dial (dragged around, or - / +) and its blink; the person group's summary of V.
use super::lobby_ink as ink;
use super::menu_core::*;
use super::menu_gameplay::{self as gp, t};
use super::menu_gpdata as gd;
use super::menu_ink as k;
use super::menu_locale as loc;
use super::lobby_rtti::A;
use red4ext_abi::handle::Handle;
use std::rc::Rc;

fn mark(p: &Handle, x: f64, y: f64, size: f64, filled: bool, col: [f64; 3]) {
    k::image(p, x, y, size, size, k::ATLAS_PHONE, "circle", Some(col), 1.0);
    if !filled {
        k::image(p, x + 3.0, y + 3.0, size - 6.0, size - 6.0, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
    }
    k::text(p, x, y - 1.0, size, size, "!", (size * 0.75 + 0.5).floor(), if filled { k::BLACK } else { col }, "center", false);
}
/// How wide a label draws, estimated per glyph from the font the language uses (menu_ink's foreign fonts): Rajdhani caps
/// (English, Russian) .62 of the size, Blender caps (the other Latin languages, Ukrainian) .72, a CJK glyph a full em,
/// Thai and Arabic narrower, Thai's marks above and below the line nothing. An ink text sizes itself only when it is
/// laid out, after the chip under it is drawn, so the chip is sized from this.
fn label_w(s: &str, size: f64) -> f64 {
    let latin = if k::foreign(&super::menu_locale::code()).is_some() { 0.72 } else { 0.62 };
    let mut w = 0.0;
    for ch in s.chars() {
        let cp = ch as u32;
        if cp == 32 {
            w += 0.3;
        } else if cp >= 0x2E80 {
            w += 1.0;
        } else if (0x0E00..0x0E80).contains(&cp) {
            if !(cp == 0x0E31 || (0x0E34..=0x0E3A).contains(&cp) || (0x0E47..=0x0E4E).contains(&cp)) {
                w += 0.55;
            }
        } else if (0x0600..0x0700).contains(&cp) {
            if !(0x064B..=0x065F).contains(&cp) {
                w += 0.5;
            }
        } else {
            w += latin;
        }
    }
    w * size
}
fn crosshair(p: &Handle, x: f64, y: f64, size: f64, col: [f64; 3]) {
    k::image(p, x, y, size, size, k::ATLAS_PHONE, "circle", Some(col), 1.0);
    k::image(p, x + 2.0, y + 2.0, size - 4.0, size - 4.0, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
    let m = size / 2.0;
    gp::line(p, x + m, y - 4.0, x + m, y + 5.0, col, 1.0, 2.0);
    gp::line(p, x + m, y + size - 5.0, x + m, y + size + 4.0, col, 1.0, 2.0);
    gp::line(p, x - 4.0, y + m, x + 5.0, y + m, col, 1.0, 2.0);
    gp::line(p, x + size - 5.0, y + m, x + size + 4.0, y + m, col, 1.0, 2.0);
}
const CHIP_H: f64 = 46.0;
/// TRACKING SAYS SO: an active objective's state at its row's right end -- TRACK (an outlined chip with a crosshair, lit
/// with the row's hover) or TRACKING (filled yellow, the tracked quest's colour). Drawing only: the row stays the button.
/// The chip's fill for the hover to light (none for the tracked one, which stays as it is).
fn track_chip(p: &Handle, x: f64, y: f64, chip_w: f64, tracked: bool) -> Option<Handle> {
    if tracked {
        k::round_fill(p, x, y, chip_w, CHIP_H, k::YELLOW, 0.9, Some(6.0));
        k::text(p, x, y, chip_w, CHIP_H, &super::menu_locale::tr("ОТСЛЕЖИВАЕТСЯ", "TRACKING"), 23.0, k::BLACK, "center", false);
        return None;
    }
    let bg = k::round_fill(p, x, y, chip_w, CHIP_H, k::CYAN, 0.08, Some(6.0));
    k::round_edge(p, x, y, chip_w, CHIP_H, k::CYAN, 0.8, Some(6.0));
    crosshair(p, x + 17.0, y + 14.0, 18.0, k::CYAN);
    k::text(p, x + 46.0, y, chip_w - 54.0, CHIP_H, &super::menu_locale::tr("ОТСЛЕЖИВАТЬ", "TRACK"), 23.0, k::CYAN, "center", false);
    Some(bg)
}
fn book(p: &Handle, x: f64, y: f64, size: f64, col: [f64; 3]) {
    k::round_edge(p, x, y, size * 0.73, size, col, 0.9, Some(2.0));
    k::rect(p, x + size * 0.18, y + 3.0, 2.0, size - 6.0, col, 1.0);
    k::text(p, x + size * 0.2, y, size * 0.5, size, "!", (size * 0.66 + 0.5).floor(), col, "center", false);
}
fn filter_icon(p: &Handle, x: f64, y: f64, kind: &str, col: [f64; 3]) {
    match kind {
        "main" => mark(p, x, y, 30.0, true, col),
        "active" => {
            for i in 0..3 {
                let i = f64::from(i);
                k::rect(p, x, y + 3.0 + i * 10.0, 4.0, 4.0, col, 1.0);
                k::rect(p, x + 9.0, y + 3.0 + i * 10.0, 25.0, 3.0, col, 1.0);
            }
        }
        _ => {
            k::image(p, x, y, 32.0, 32.0, k::ATLAS_PHONE, "circle", Some(col), 1.0);
            k::image(p, x + 3.0, y + 3.0, 26.0, 26.0, k::ATLAS_PHONE, "circle", Some(k::BLACK), 1.0);
            if kind == "activities" {
                gp::line(p, x + 9.0, y + 23.0, x + 23.0, y + 9.0, col, 1.0, 4.0);
                gp::line(p, x + 16.0, y + 11.0, x + 23.0, y + 9.0, col, 1.0, 3.0);
            } else {
                gp::line(p, x + 6.0, y + 16.0, x + 13.0, y + 22.0, col, 1.0, 3.0);
                gp::line(p, x + 13.0, y + 22.0, x + 26.0, y + 9.0, col, 1.0, 3.0);
            }
        }
    }
}
fn distance_text(d: f64) -> String {
    if d >= 1000.0 {
        format!("{:.1}{}", d / 1000.0, loc::tr("км", "km"))
    } else {
        format!("{}{}", (d + 0.5).floor() as i64, loc::tr("м", "m"))
    }
}
fn hkey(h: f64) -> i64 {
    h as i64
}

pub(super) fn journal(m: &mut Menu, p: &Handle, p2: &Handle) {
    let inspect = m.gp.journal_inspect;
    let (list_w, detail_x, detail_w) = (820.0, P2X, 900.0);
    ink::move_to(p2, detail_x, P1Y);
    m.gp.distance_widgets.clear();
    let changed: gp::Changed = Rc::new(|m: &mut Menu, _first| {
        let Some(list) = m.gp.jobs.get::<gd::QuestList>("quests") else { return true };
        if list.structure != m.gp.quest_structure {
            return true;
        }
        for q in &list.quests {
            if let Some(d) = q.distance {
                m.gp.distances.insert(hkey(q.hash), d);
                if let Some(w) = m.gp.distance_widgets.get(&hkey(q.hash)) {
                    ink::set_text(w, &distance_text(d));
                }
            }
        }
        false
    });
    let quests = gp::fetch::<gd::QuestList>(m, "quests", gd::quest_list(), 2.0, 0.012, false, Some(changed)).unwrap_or_default();
    m.gp.distances.clear();
    m.gp.quest_structure = quests.structure.clone();
    let active_state = super::menu_rt::ev("gameJournalEntryState", "Active");
    let mut counts = [("active", 0usize), ("main", 0), ("activities", 0), ("completed", 0)];
    let flags: Vec<(bool, bool)> = quests
        .quests
        .iter()
        .map(|q| {
            let active = q.state == active_state;
            let main = q.story || q.kind.contains("SideQuest") || q.kind.contains("MinorQuest");
            (active, main)
        })
        .collect();
    for (active, main) in &flags {
        if *active {
            counts[0].1 += 1;
            counts[if *main { 1 } else { 2 }].1 += 1;
        } else {
            counts[3].1 += 1;
        }
    }
    if !counts.iter().any(|c| c.0 == m.gp.quest_filter) {
        m.gp.quest_filter = "active".into();
    }
    glass(m, p, list_w, 1130.0, Some((P1X, P1Y)));
    gp::title(p, &t("journal"), list_w);
    gp::head_action(m, p, P1X, list_w - 72.0, &loc::tr("ЧИПЫ", "SHARDS"), Rc::new(|m: &mut Menu, _| gp::open_section(m, "shards")), true, None);
    let tw = (list_w - 40.0 - 24.0) / 4.0;
    for (i, (key, count)) in counts.iter().enumerate() {
        let x = 20.0 + i as f64 * (tw + 8.0);
        let on = m.gp.quest_filter == *key;
        let own = match *key {
            "main" => k::YELLOW,
            "activities" => [0.2, 0.85, 0.65],
            _ => k::CYAN,
        };
        let col = if on { k::CYAN } else { own };
        let bg = k::round_fill(p, x, 100.0, tw, 60.0, col, if on { 0.16 } else { 0.035 }, Some(8.0));
        filter_icon(p, x + 13.0, 115.0, key, col);
        k::text(p, x + 46.0, 100.0, tw - 50.0, 60.0, &count.to_string(), 30.0, col, "center", false);
        let key = key.to_string();
        button(
            m,
            "p1",
            &at(P1X, P1Y),
            x,
            100.0,
            tw,
            60.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                let g = &mut m.gp;
                (g.quest_filter, g.quest, g.journal_inspect, g.reset_list) = (key.clone(), None, None, true);
                gp::redraw(m);
            })),
            Some(Rc::new(move |h, _| ink::opacity(&bg, if h { 0.25 } else if on { 0.16 } else { 0.035 }))),
        );
    }
    // the chip and the list's TRACKED label as wide as the language's words need
    let tr = super::menu_locale::tr;
    let chip_w = 176f64.max((label_w(&tr("ОТСЛЕЖИВАЕТСЯ", "TRACKING"), 23.0)).ceil() + 64.0).max((label_w(&tr("ОТСЛЕЖИВАТЬ", "TRACK"), 23.0)).ceil() + 66.0);
    let tag_w = 140f64.max((label_w(&tr("ОТСЛЕЖИВАЕТСЯ", "TRACKED"), 22.0)).ceil() + 28.0);
    let mut rows = Vec::new();
    let filter = m.gp.quest_filter.clone();
    for (q, (active, main)) in quests.quests.iter().zip(&flags) {
        let show = match filter.as_str() {
            "active" => *active,
            "main" => *active && *main,
            "activities" => *active && !*main,
            _ => !*active,
        };
        if !show {
            continue;
        }
        let (q, active, sel) = (q.clone(), *active, m.gp.quest == Some(q.hash));
        rows.push(Row {
            h: 106.0,
            build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                let bg = k::round_fill(c, 0.0, 0.0, it.cw, 96.0, if q.tracked { k::YELLOW } else { k::ICE }, if q.tracked { 0.12 } else { 0.035 }, Some(7.0));
                let edge = k::round_edge(c, 0.0, 0.0, it.cw, 96.0, if sel { k::YELLOW } else { k::CYAN }, if sel { 1.0 } else { 0.35 }, Some(7.0));
                mark(c, 20.0, 27.0, 34.0, q.story, if active { k::YELLOW } else { k::GREY });
                if q.tracked {
                    k::round_fill(c, -53.0, 16.0, 38.0, 64.0, k::YELLOW, 0.14, Some(5.0));
                    book(c, -48.0, 28.0, 30.0, k::YELLOW);
                }
                let reserved = if q.distance.is_some() && active { 110.0 } else { 8.0 };
                let available = it.cw - 78.0 - reserved;
                k::text(c, 70.0, 5.0, available, 42.0, &k::clip(&q.name, (available / 16.0).floor() as usize), 29.0, k::CYAN, "left", false);
                // the tracked quest says so in words too, not only in yellow
                let tag = if q.tracked && active { tag_w } else { 0.0 };
                k::text(c, 70.0, 49.0, it.cw - 90.0 - tag, 37.0, &k::clip(&q.objective, ((it.cw - 90.0 - tag) / 13.0).floor() as usize), 25.0, k::GREY, "left", false);
                if tag > 0.0 {
                    k::text(c, it.cw - tag - 14.0, 49.0, tag, 37.0, &super::menu_locale::tr("ОТСЛЕЖИВАЕТСЯ", "TRACKED"), 22.0, k::YELLOW, "right", false);
                }
                if let (Some(d), true) = (q.distance, active) {
                    if q.tracked {
                        k::round_fill(c, it.cw - 112.0, 3.0, 108.0, 43.0, k::YELLOW, 0.9, Some(5.0));
                    }
                    let d = m.gp.distances.get(&hkey(q.hash)).copied().unwrap_or(d);
                    let w = k::text(c, it.cw - 110.0, 3.0, 101.0, 43.0, &distance_text(d), 27.0, if q.tracked { k::BLACK } else { k::CYAN }, "center", false);
                    m.gp.distance_widgets.insert(hkey(q.hash), w);
                }
                let (hash, tracked) = (q.hash, q.tracked);
                button(
                    m,
                    "p1",
                    &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                    0.0,
                    0.0,
                    it.cw,
                    96.0,
                    Some(Rc::new(move |m: &mut Menu, _| {
                        (m.gp.quest, m.gp.journal_inspect) = (Some(hash), None);
                        gd::visit(hash);
                        gp::redraw(m);
                    })),
                    Some(Rc::new(move |h, _| {
                        ink::opacity(&bg, if h { 0.22 } else if tracked { 0.12 } else { 0.035 });
                        k::tint(&edge, if h || sel { k::YELLOW } else { k::CYAN }, Some(if h { 1.0 } else { 0.5 }));
                    })),
                );
            }),
        });
    }
    let keep = m.keep_scroll;
    gp::glist(m, p, "p1", "p1", P1X, 178.0, list_w, rows, 855.0, keep);
    glass(m, p2, detail_w, 1130.0, Some((detail_x, P1Y)));
    let mut details = Vec::new();
    let detail = m.gp.quest.and_then(|hash| gp::fetch::<Option<gd::QuestDetail>>(m, &format!("quest:{}", gd::fmt_g(hash)), gd::quest_detail(hash), 2.0, 0.03, false, None));
    match detail.as_deref().and_then(|d| d.as_ref()) {
        Some(det) => {
            let q = &det.quest;
            gp::text_rows(&mut details, &k::upper(&q.name), detail_w, Some(k::WHITE), Some(34.0));
            details.push(Row { h: 28.0, build: Rc::new(|_, _, _| {}) });
            // ONLY OBJECTIVES GET A CHIP: the native details panel makes a trackable row of each JournalQuestObjective only;
            // sub-objectives are drawn indented under their objective, without a chip, and a tap on one tracks the
            // objective it belongs to (the quest lists each objective's subs right after it). WHICH ONE IS TRACKED comes
            // from JournalManager.GetTrackedEntry, not from the rows' flag (IsTrackedInHierarchy is true only when one of
            // an objective's SUB-objectives is the tracked entry, never for the objective itself).
            let jm = super::menu_rt::jm();
            let entry = super::menu_rt::o(&jm, "GetTrackedEntry", &[]);
            let tracked_hash = if entry.is_null() { 0.0 } else { super::menu_rt::n(&jm, "GetEntryHash", &[A::H(&entry)]) };
            let mut tracked_rows = std::collections::HashSet::new();
            let mut parent: Option<f64> = None;
            for o in &q.objectives {
                if !o.sub {
                    parent = Some(o.hash);
                }
                if o.hash == tracked_hash || o.tracked {
                    tracked_rows.insert(gd::fmt_g(if o.sub { parent.unwrap_or(o.hash) } else { o.hash }));
                }
            }
            parent = None;
            for o in &q.objectives {
                if !o.sub {
                    parent = Some(o.hash);
                }
                if !o.active {
                    continue;
                }
                let sub = o.sub && parent.is_some();
                let target = if sub { parent.unwrap_or(o.hash) } else { o.hash };
                let tracked = !sub && tracked_rows.contains(&gd::fmt_g(o.hash));
                let indent = if sub { 36.0 } else { 0.0 };
                let chip_space = if sub { 0.0 } else { chip_w + 16.0 };
                let lines = gp::wrap(&k::upper(&o.name), ((detail_w - 100.0 - indent - chip_space) / 17.0).floor() as usize);
                let h = lines.len() as f64 * 41.0 + 26.0;
                let text = lines.join("\n");
                let n = lines.len() as f64;
                details.push(Row {
                    h: h + 12.0,
                    build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                        let bg = k::round_fill(c, indent, 0.0, it.cw - indent, h, k::CYAN, 0.05, Some(6.0));
                        if sub {
                            k::rect(c, indent + 22.0, 25.0, 10.0, 10.0, k::CYAN, 0.8);
                        } else {
                            book(c, 16.0, 15.0, 27.0, if tracked { k::YELLOW } else { k::CYAN });
                        }
                        k::text(c, indent + 66.0, 11.0, it.cw - 90.0 - indent - chip_space, n * 41.0, &text, if sub { 27.0 } else { 30.0 }, k::CYAN, "left", false);
                        let chip = if sub { None } else { track_chip(c, it.cw - chip_w - 14.0, ((h - CHIP_H) / 2.0).floor(), chip_w, tracked) };
                        button(
                            m,
                            "p2",
                            &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                            indent,
                            0.0,
                            it.cw - indent,
                            h,
                            Some(Rc::new(move |m: &mut Menu, _| gp::perform(m, |_| gd::track(target)))),
                            Some(Rc::new(move |hv, _| {
                                ink::opacity(&bg, if hv { 0.2 } else { 0.05 });
                                if let Some(chip) = &chip {
                                    ink::opacity(chip, if hv { 0.3 } else { 0.08 });
                                }
                            })),
                        );
                    }),
                });
            }
            for entry in det.actions.iter().cloned() {
                details.push(Row {
                    h: 76.0,
                    build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                        let bg = k::round_fill(c, 0.0, 0.0, it.cw, 64.0, k::CYAN, 0.055, Some(8.0));
                        k::round_edge(c, 18.0, 16.0, 29.0, 25.0, k::CYAN, 1.0, Some(3.0));
                        k::rect(c, 22.0, 42.0, 9.0, 3.0, k::CYAN, 1.0);
                        k::text(c, 64.0, 0.0, it.cw - 85.0, 64.0, &k::clip(&k::upper(&entry.name), ((it.cw - 85.0) / 17.0).floor() as usize), 29.0, k::CYAN, "left", false);
                        let e = entry.clone();
                        button(
                            m,
                            "p2",
                            &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                            0.0,
                            0.0,
                            it.cw,
                            64.0,
                            Some(Rc::new(move |m: &mut Menu, _| {
                                if e.kind == "message" {
                                    super::menu_messages::show_thread(m, e.thread.clone(), e.contact, &e.name);
                                    gp::open_section(m, "contacts");
                                } else if gd::read_shard(&mut m.gp.jobs, e.hash).is_some() {
                                    (m.gp.shard, m.gp.shard_group) = (Some(e.hash), gp::Open::Unset);
                                    gp::open_section(m, "shards");
                                }
                            })),
                            Some(Rc::new(move |h, _| ink::opacity(&bg, if h { 0.2 } else { 0.055 }))),
                        );
                    }),
                });
            }
            gp::text_rows(&mut details, &format!("\n{}\n", q.description), detail_w, Some(k::TEXT), Some(31.0));
            let columns: usize = if detail_w >= 800.0 { 3 } else { 2 };
            let cell = (detail_w - 58.0) / columns as f64;
            for group in det.links.chunks(columns) {
                let group = group.to_vec();
                details.push(Row {
                    h: 190.0,
                    build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                        for (i, link) in group.iter().enumerate() {
                            let x = 14.0 + i as f64 * cell;
                            gp::picture(m, c, x, 6.0, cell - 16.0, 120.0, link.image.as_ref(), Some((it.view.clone(), it.y)));
                            k::text(c, x, 130.0, cell - 16.0, 44.0, &k::clip(&k::upper(&link.name), ((cell - 16.0) / 13.0).floor() as usize), 24.0, k::CYAN, "left", false);
                            let edge = k::round_edge(c, x, 5.0, cell - 16.0, 122.0, k::CYAN, 0.0, Some(3.0));
                            let hash = link.hash;
                            button(
                                m,
                                "p2",
                                &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                                x,
                                5.0,
                                cell - 16.0,
                                171.0,
                                Some(Rc::new(move |m: &mut Menu, _| {
                                    m.gp.journal_inspect = Some(hash);
                                    gd::visit(hash);
                                    gp::redraw(m);
                                })),
                                Some(Rc::new(move |h, _| k::tint(&edge, k::CYAN, Some(if h { 1.0 } else { 0.0 })))),
                            );
                        }
                    }),
                });
            }
            for o in q.objectives.iter().filter(|o| !o.active) {
                gp::text_rows(&mut details, &o.name, detail_w, Some(k::GREY), Some(25.0));
            }
        }
        None => {
            if !(m.gp.quest.is_some() && m.gp.reading) {
                gp::text_rows(&mut details, &t("choose"), detail_w, Some(k::GREY), None);
            }
        }
    }
    gp::glist(m, p2, "p2", "p2", detail_x, 20.0, detail_w, details, 1010.0, None);
    if let Some(inspect) = inspect {
        let (x, w) = (P1X, 1780.0);
        let pane = ink::canvas(&m.layer_panel, x, P1Y, w, 1160.0);
        k::round_fill(&pane, 0.0, 0.0, w, 1130.0, k::BLACK, 1.0, Some(18.0));
        glass(m, &pane, w, 1130.0, Some((x, P1Y)));
        let blocker = button(m, "p2", &at(x, P1Y), 0.0, 0.0, w, 1130.0, None, None);
        blocker.blocker.set(true);
        let entry = gd::codex_entry(&mut m.gp.jobs, inspect);
        let mut rows3 = Vec::new();
        let close_bg = k::round_fill(&pane, w - 144.0, 22.0, 72.0, 48.0, k::CYAN, 0.12, Some(6.0));
        gp::line(&pane, w - 120.0, 34.0, w - 96.0, 58.0, k::CYAN, 1.0, 3.0);
        gp::line(&pane, w - 96.0, 34.0, w - 120.0, 58.0, k::CYAN, 1.0, 3.0);
        let close = button(
            m,
            "p2",
            &at(x, P1Y),
            w - 144.0,
            22.0,
            72.0,
            48.0,
            Some(Rc::new(|m: &mut Menu, _| {
                m.gp.journal_inspect = None;
                gp::redraw(m);
            })),
            Some(Rc::new(move |h, _| ink::opacity(&close_bg, if h { 0.3 } else { 0.12 }))),
        );
        *close.key.borrow_mut() = Some("journalClose".into());
        match entry {
            Some(e) => {
                let img = gd::image(e.image);
                rows3.push(Row {
                    h: 360.0,
                    build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| gp::picture(m, c, 20.0, 10.0, it.cw - 40.0, 330.0, img.as_ref(), Some((it.view.clone(), it.y)))),
                });
                gp::text_rows(&mut rows3, &k::upper(&e.name), w, Some(k::CYAN), Some(40.0));
                gp::text_rows(&mut rows3, &e.description, w, Some(k::TEXT), Some(34.0));
            }
            None => gp::text_rows(&mut rows3, &t("empty"), w, Some(k::GREY), None),
        }
        let v = gp::glist(m, &pane, "p2", "p3", x, 86.0, w, rows3, 950.0, None);
        v.borrow_mut().enable_mask();
        if m.gp.inspect_shown != Some(inspect) {
            slide_in(m, "p2", &pane, x, P1Y, 0.02, 36.0, 0.0, None);
        }
    }
    m.gp.inspect_shown = inspect;
}

pub(super) fn shards(m: &mut Menu, p: &Handle, p2: &Handle) {
    let groups = gp::fetch::<Vec<gd::ShardGroup>>(m, "shards", gd::shard_list(), 5.0, 0.012, false, None).unwrap_or_default();
    glass(m, p, 820.0, 1140.0, Some((P1X, P1Y)));
    gp::title(p, &loc::tr("ЧИПЫ", "SHARDS"), 820.0);
    gp::head_action(m, p, P1X, 820.0 - 72.0, &loc::tr("БАЗА ДАННЫХ", "DATABASE"), Rc::new(|m: &mut Menu, _| gp::open_section(m, "codex")), true, None);
    let mut selected: Option<gd::Shard> = None;
    let mut selected_group = None;
    if let Some(h) = m.gp.shard {
        for g in groups.iter() {
            for e in &g.entries {
                if e.hash == h {
                    selected = Some(e.clone());
                    selected_group = Some(g.key.clone());
                }
            }
        }
    }
    if m.gp.shard_group == gp::Open::Unset {
        if let Some(g) = selected_group {
            m.gp.shard_group = gp::Open::Key(g);
        }
    }
    let mut rows = Vec::new();
    for g in groups.iter() {
        let fresh = g.entries.iter().filter(|e| e.fresh).count();
        let open = m.gp.shard_group == gp::Open::Key(g.key.clone());
        let key = g.key.clone();
        rows.push(gp::row(
            "p1",
            &format!("{}{}  ({})", if open { "- " } else { "+ " }, k::upper(&g.name), g.entries.len()),
            &if fresh > 0 { format!("{}{}", loc::tr("Новые: ", "New: "), fresh) } else { String::new() },
            Some(Rc::new(move |m: &mut Menu, _| {
                m.gp.shard_group = if m.gp.shard_group == gp::Open::Key(key.clone()) { gp::Open::Closed } else { gp::Open::Key(key.clone()) };
                m.gp.reset_list = true;
                gp::redraw(m);
            })),
            open,
            Some(if fresh > 0 { k::CYAN } else { k::GREY }),
        ));
        if open {
            for e in &g.entries {
                let hash = e.hash;
                rows.push(gp::row(
                    "p1",
                    &format!("  {}", e.name),
                    &if e.crypted { loc::tr("Зашифровано", "Encrypted") } else { String::new() },
                    Some(Rc::new(move |m: &mut Menu, _| {
                        if gd::read_shard(&mut m.gp.jobs, hash).is_some() {
                            m.gp.shard = Some(hash);
                            gp::redraw(m);
                        }
                    })),
                    m.gp.shard == Some(e.hash),
                    Some(if e.fresh { k::YELLOW } else { k::TEXT }),
                ));
            }
        }
    }
    let keep = m.keep_scroll;
    gp::glist(m, p, "p1", "p1", P1X, 100.0, 820.0, rows, 935.0, keep);
    glass(m, p2, 900.0, 1140.0, Some((P2X, P1Y)));
    let mut details = Vec::new();
    let crypted = selected.as_ref().is_some_and(|s| s.crypted);
    match &selected {
        Some(sel) => {
            gp::text_rows(&mut details, &k::upper(&sel.name), 900.0, Some(k::CYAN), Some(38.0));
            gp::text_rows(&mut details, &format!("\n{}", sel.text), 900.0, Some(k::TEXT), Some(32.0));
            if sel.crypted {
                let hash = sel.hash;
                gp::hub_action(
                    m,
                    p2,
                    P2X,
                    20.0,
                    1050.0,
                    860.0,
                    &loc::tr("ВЗЛОМАТЬ ЧИП", "CRACK SHARD"),
                    Rc::new(move |m: &mut Menu, _| {
                        super::menu_main::close(m);
                        gd::crack(&mut m.gp.jobs, hash);
                    }),
                    true,
                    None,
                    None,
                );
            }
        }
        None => gp::text_rows(&mut details, &loc::tr("[ Запись не выбрана ]", "[ No entry selected ]"), 900.0, Some(k::CYAN), Some(38.0)),
    }
    gp::glist(m, p2, "p2", "p2", P2X, 30.0, 900.0, details, if crypted { 930.0 } else { 1020.0 }, None);
}

// ---------------------------------------------------------------- the wait
const TIME_ATLAS: &str = "base\\gameplay\\gui\\widgets\\time_skip\\atlas_time_skip.inkatlas";
struct Dial {
    clock: gd::Clock,
    day: Handle,
    night: Handle,
    hours: Handle,
    phase: Handle,
    next_day: Handle,
    target: Handle,
    knob: Handle,
    segments: Vec<Handle>,
}
fn angle(h: f64) -> f64 {
    h / 24.0 * std::f64::consts::PI * 2.0 + std::f64::consts::PI / 2.0
}
const CX: f64 = 450.0;
const CY: f64 = 430.0;
const R: f64 = 220.0;
fn dial_time(c: &gd::Clock, h: f64) -> String {
    if c.hidden {
        "--:--".into()
    } else {
        format!("{:02}:{:02}", (h as i64).rem_euclid(24), c.minute as i64)
    }
}
fn dial_update(d: &Dial, hours: f64) {
    let c = &d.clock;
    let hour = (c.hour + hours) % 24.0;
    let daylight = (6.0..20.0).contains(&hour);
    ink::visible(&d.day, daylight);
    ink::visible(&d.night, !daylight);
    ink::set_text(&d.hours, &format!("{}{}", gd::fmt_g(hours), loc::tr(" ч.", " h")));
    let label = if (6.0..12.0).contains(&hour) {
        loc::tr("Утро", "Morning")
    } else if (12.0..18.0).contains(&hour) {
        loc::tr("День", "Day")
    } else if (18.0..22.0).contains(&hour) {
        loc::tr("Вечер", "Evening")
    } else {
        loc::tr("Ночь", "Night")
    };
    ink::set_text(&d.phase, &label);
    ink::set_text(&d.next_day, &if c.hour + hours >= 24.0 { t("tomorrow") } else { String::new() });
    ink::set_text(&d.target, &dial_time(c, c.hour + hours));
    let a = angle(c.hour + c.minute / 60.0 + hours);
    ink::move_to(&d.knob, CX + a.cos() * R - 17.0, CY + a.sin() * R - 17.0);
    for (i, seg) in d.segments.iter().enumerate() {
        let lit = (i as f64 / 4.0 - c.hour - c.minute / 60.0).rem_euclid(24.0) <= hours;
        ink::opacity(seg, if lit { 1.0 } else { 0.36 });
    }
}
pub(super) fn wait(m: &mut Menu, p: &Handle) {
    let clock = gd::clock();
    m.gp.clock = Some(clock);
    let w = 900.0;
    glass(m, p, w, 1130.0, Some((P1X, P1Y)));
    gp::title(p, &loc::tr("ОЖИДАНИЕ", "WAIT"), w);
    let img = |x: f64, y: f64, s: f64, part: &str, col: [f64; 3], op: f64| k::image(p, x, y, s, s, TIME_ATLAS, part, Some(col), op);
    img(CX - R - 16.0, CY - R - 16.0, (R + 16.0) * 2.0, "circle-fg", k::CYAN, 0.45);
    img(CX - R + 22.0, CY - R + 22.0, (R - 22.0) * 2.0, "inner-circle", k::CYAN, 0.4);
    let segments: Vec<Handle> = (0..96)
        .map(|i| {
            let (a, b) = (angle(f64::from(i) / 4.0), angle((f64::from(i) + 0.94) / 4.0));
            gp::line(p, CX + a.cos() * R, CY + a.sin() * R, CX + b.cos() * R, CY + b.sin() * R, k::CYAN, 0.65, 10.0)
        })
        .collect();
    for h in 0..24 {
        let a = angle(f64::from(h));
        let outer = R + 18.0;
        let inner = outer + if h % 3 == 0 { 15.0 } else { 7.0 };
        gp::line(p, CX + a.cos() * outer, CY + a.sin() * outer, CX + a.cos() * inner, CY + a.sin() * inner, k::CYAN, 0.75, 2.0);
    }
    img(CX - 24.0, CY - R - 73.0 - 24.0, 48.0, "ico-day", k::YELLOW, 1.0);
    img(CX - 23.0, CY + R + 73.0 - 23.0, 46.0, "ico-night", k::CYAN, 1.0);
    for h in [6.0, 18.0] {
        let x = CX + angle(h).cos() * (R + 73.0);
        img(x - 26.0, CY - 26.0, 52.0, if h == 6.0 { "ico-morning" } else { "ico-evening" }, if h == 6.0 { k::CYAN } else { k::YELLOW }, 1.0);
    }
    let day = img(CX - 27.0, CY - 112.0 - 27.0, 54.0, "ico-day", k::YELLOW, 1.0);
    let night = img(CX - 26.0, CY - 112.0 - 26.0, 52.0, "ico-night", k::CYAN, 1.0);
    let hours = k::text(p, CX - 160.0, CY - 54.0, 320.0, 94.0, "", 72.0, k::CYAN, "center", false);
    let phase = k::text(p, CX - 170.0, CY + 52.0, 340.0, 42.0, "", 28.0, k::GREY, "center", false);
    let next_day = k::text(p, CX - 170.0, CY + 97.0, 340.0, 34.0, "", 23.0, k::GREY, "center", false);
    k::text(p, 70.0, 787.0, 310.0, 34.0, &loc::tr("СЕЙЧАС", "NOW"), 25.0, k::GREY, "center", false);
    k::text(p, 520.0, 787.0, 310.0, 34.0, &loc::tr("ПОСЛЕ ОЖИДАНИЯ", "AFTER WAITING"), 25.0, k::GREY, "center", false);
    k::text(p, 70.0, 833.0, 310.0, 66.0, &dial_time(&clock, clock.hour), 52.0, k::TEXT, "center", false);
    let target = k::text(p, 520.0, 833.0, 310.0, 66.0, "", 52.0, k::CYAN, "center", false);
    gp::line(p, 416.0, 866.0, 477.0, 866.0, k::CYAN, 0.8, 3.0);
    gp::line(p, 466.0, 854.0, 478.0, 866.0, k::CYAN, 0.8, 3.0);
    gp::line(p, 478.0, 866.0, 466.0, 878.0, k::CYAN, 0.8, 3.0);
    let start = angle(clock.hour + clock.minute / 60.0);
    k::image(p, CX + start.cos() * R - 8.0, CY + start.sin() * R - 8.0, 16.0, 16.0, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
    let knob = k::image(p, 0.0, 0.0, 34.0, 34.0, k::ATLAS_PHONE, "circle", Some(k::CYAN), 1.0);
    let dial = Rc::new(Dial { clock, day, night, hours, phase, next_day, target, knob, segments });
    dial_update(&dial, m.gp.hours);
    let b = button(m, "p1", &at(P1X, P1Y), CX - R - 29.0, CY - R - 29.0, (R + 29.0) * 2.0, (R + 29.0) * 2.0, None, None);
    *b.contains.borrow_mut() = Some(Rc::new(|x, y| {
        let radius = ((x - R - 29.0).powi(2) + (y - R - 29.0).powi(2)).sqrt();
        radius >= R - 26.0 && radius <= R + 26.0
    }));
    let d1 = dial.clone();
    *b.drag.borrow_mut() = Some(Rc::new(move |m: &mut Menu, x, event, _hand, y| {
        let (Some(x), Some(y)) = (x, y) else { return };
        if event == "end" {
            return;
        }
        let theta = (y - P1Y - CY).atan2(x - P1X - CX) - std::f64::consts::PI / 2.0;
        let c = &d1.clock;
        let delta = ((theta / (2.0 * std::f64::consts::PI) * 24.0 - c.hour - c.minute / 60.0).rem_euclid(24.0) + 0.5).floor();
        m.gp.hours = if delta == 0.0 { 24.0 } else { delta };
        dial_update(&d1, m.gp.hours);
    }));
    let d2 = dial.clone();
    gp::hub_action(
        m,
        p,
        P1X,
        60.0,
        960.0,
        90.0,
        "-",
        Rc::new(move |m: &mut Menu, _| {
            m.gp.hours = (m.gp.hours - 1.0).max(1.0);
            dial_update(&d2, m.gp.hours);
        }),
        true,
        None,
        None,
    );
    let d3 = dial.clone();
    gp::hub_action(
        m,
        p,
        P1X,
        750.0,
        960.0,
        90.0,
        "+",
        Rc::new(move |m: &mut Menu, _| {
            m.gp.hours = (m.gp.hours + 1.0).min(24.0);
            dial_update(&d3, m.gp.hours);
        }),
        true,
        None,
        None,
    );
    let label = if clock.allowed { t("confirm") } else { t("blocked") };
    gp::hub_action(
        m,
        p,
        P1X,
        175.0,
        960.0,
        550.0,
        &label,
        Rc::new(|m: &mut Menu, _| {
            if !gd::clock().allowed {
                let b = t("blocked");
                toast(m, &b);
                return;
            }
            let hours = m.gp.hours;
            super::menu_main::close(m);
            gp::start_wait(m, hours);
        }),
        clock.allowed,
        None,
        None,
    );
}

// ---------------------------------------------------------------- the summary
pub(super) fn summary(m: &mut Menu, p: &Handle) {
    let Some(data) = gd::summary() else { return };
    m.gp.summary = Some(data.clone());
    m.gp.summary_at = m.time;
    clear_buttons(m, "summary");
    ink::remove_all(p);
    let (x, y, w) = (270.0, 250.0, 640.0);
    let expanded = m.gp.summary_expanded;
    let mut extra = 0.0;
    if expanded {
        for (name, _) in &data.other {
            extra += 30f64.max(gp::wrap(name, 32).len() as f64 * 27.0);
        }
        extra += 12.0;
    }
    let h = 590.0 + extra;
    m.summary_rect = Some(glass_rect(x, y, w, h, R_PANEL));
    let gc = ink::canvas(p, x, y, w, h);
    glass(m, &gc, w, h, None);
    k::text(p, x + 24.0, y + 8.0, w - 48.0, 40.0, "V", 34.0, k::WHITE, "left", false);
    let rows = [("gun", loc::tr("УРОН В СЕКУНДУ", "DAMAGE PER SECOND"), data.dps), ("armor", loc::tr("БРОНЯ", "ARMOR"), data.armor), ("health", loc::tr("ЗДОРОВЬЕ", "HEALTH"), data.health)];
    for (i, (part, label, v)) in rows.iter().enumerate() {
        let yy = y + 58.0 + i as f64 * 50.0;
        k::image(p, x + 20.0, yy + 5.0, 32.0, 32.0, k::ATLAS_MAPPIN, part, Some(k::CYAN), 1.0);
        k::text(p, x + 66.0, yy, w - 190.0, 42.0, label, 26.0, k::TEXT, "left", false);
        k::text(p, x + w - 120.0, yy, 96.0, 42.0, &((v + 0.5).floor() as i64).to_string(), 29.0, k::CYAN, "right", false);
        k::rect(p, x + 20.0, yy + 45.0, w - 40.0, 1.0, k::CYAN, 0.14);
    }
    let more = k::round_fill(p, x + 18.0, y + 216.0, w - 36.0, 38.0, k::NAVY, 0.18, Some(3.0));
    let more_text = k::text(p, x + 30.0, y + 216.0, w - 60.0, 38.0, &format!("{}{}", if expanded { "- " } else { "+ " }, loc::tr("ДРУГОЕ", "OTHER")), 24.0, k::TEXT, "left", false);
    let p1 = p.clone();
    button(
        m,
        "summary",
        &at(0.0, 0.0),
        x + 18.0,
        y + 216.0,
        w - 36.0,
        38.0,
        Some(Rc::new(move |m: &mut Menu, _| {
            m.gp.summary_expanded = !expanded;
            summary(m, &p1);
        })),
        Some(Rc::new(move |hv, _| {
            ink::tint(&more, if hv { k::YELLOW } else { k::NAVY });
            ink::opacity(&more, if hv { 0.14 } else { 0.18 });
            ink::tint(&more_text, if hv { k::YELLOW } else { k::TEXT });
        })),
    );
    if expanded {
        let mut yy = y + 264.0;
        for (name, value) in &data.other {
            let names = gp::wrap(name, 32);
            let rh = 30f64.max(names.len() as f64 * 27.0);
            k::text(p, x + 26.0, yy, 110.0, 30.0, value, 24.0, k::CYAN, "left", false);
            k::text(p, x + 145.0, yy, w - 169.0, rh, &names.join("\n"), 23.0, k::TEXT, "left", false);
            yy += rh;
        }
    }
    let life = match data.life.as_str() {
        "Nomad" => loc::tr("КОЧЕВНИК", "NOMAD"),
        "StreetKid" => loc::tr("ДИТЯ УЛИЦ", "STREETKID"),
        "Corporate" => loc::tr("КОРПОРАТ", "CORPO"),
        other => other.to_string(),
    };
    let played = format!("{:02}:{:02}", (data.seconds / 3600.0).floor() as i64, ((data.seconds / 60.0).floor() as i64) % 60);
    k::text(p, x + 22.0, y + 270.0 + extra, w - 44.0, 36.0, &format!("{played}  /  {life}"), 24.0, k::TEXT, "left", false);
    for (i, pr) in data.progress.iter().enumerate() {
        let yy = y + 324.0 + extra + i as f64 * 97.0;
        let label = if i == 0 { loc::tr("УРОВЕНЬ", "LEVEL") } else { loc::tr("РЕПУТАЦИЯ", "STREET CRED") };
        let ratio = if pr.level >= pr.max { 1.0 } else { pr.xp / pr.total.max(1.0) };
        k::text(p, x + 22.0, yy, 315.0, 38.0, &format!("{}  {}", gd::fmt_g(pr.level), label), 28.0, k::CYAN, "left", false);
        let right = if pr.level >= pr.max { loc::tr("МАКС.", "MAX") } else { format!("{}/{}", gd::fmt_g(pr.xp), gd::fmt_g(pr.total)) };
        k::text(p, x + 340.0, yy + 3.0, w - 362.0, 32.0, &right, 21.0, k::GREY, "right", false);
        k::rect(p, x + 22.0, yy + 48.0, w - 44.0, 4.0, k::CYAN, 0.18);
        k::rect(p, x + 22.0, yy + 48.0, (w - 44.0) * ratio, 4.0, k::CYAN, 1.0);
        k::text(p, x + 22.0, yy + 56.0, w - 44.0, 25.0, &format!("{}%", (ratio * 100.0).floor() as i64), 20.0, k::CYAN, "right", false);
    }
    let hint = k::text(p, x + 22.0, y + h + 12.0, w - 44.0, 82.0, "", 24.0, k::CYAN, "left", false);
    let street = data.progress.get(1).map_or(0.0, |p| p.level);
    for (i, (level, text)) in data.rewards.iter().enumerate() {
        let xx = x + 22.0 + i as f64 * 66.0;
        let col = if street >= *level { k::CYAN } else { k::GREY };
        let bg = k::round_fill(p, xx, y + 529.0 + extra, 58.0, 42.0, col, 0.08, Some(6.0));
        k::text(p, xx, y + 529.0 + extra, 58.0, 42.0, &gd::fmt_g(*level), 22.0, col, "center", false);
        let lines = gp::wrap(text, 42).join("\n");
        let level = *level;
        let (h1, l1) = (hint.clone(), lines.clone());
        let show: Rc<dyn Fn(&mut Menu)> = Rc::new(move |m: &mut Menu| {
            m.gp.summary_hint_owner = Some(level);
            ink::set_text(&h1, &l1);
            m.summary_rect = Some(glass_rect(x, y, w, h + 105.0, R_PANEL));
        });
        let s2 = show.clone();
        // the hover's half needs the menu: it is applied in the summary's update through the hint owner
        let (h2, l2) = (hint.clone(), lines);
        button(
            m,
            "summary",
            &at(0.0, 0.0),
            xx,
            y + 529.0 + extra,
            58.0,
            42.0,
            Some(Rc::new(move |m: &mut Menu, _| s2(m))),
            Some(Rc::new(move |hv, _| {
                ink::opacity(&bg, if hv { 0.24 } else { 0.08 });
                if hv {
                    ink::set_text(&h2, &l2);
                    SUMMARY_HINT.with(|c| c.set(Some(level)));
                } else if SUMMARY_HINT.with(|c| c.get()) == Some(level) {
                    SUMMARY_HINT.with(|c| c.set(None));
                    ink::set_text(&h2, "");
                }
            })),
        );
    }
    SUMMARY_GEOM.with(|c| c.set(Some((x, y, w, h))));
}
thread_local! {
    static SUMMARY_HINT: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
    static SUMMARY_GEOM: std::cell::Cell<Option<(f64, f64, f64, f64)>> = const { std::cell::Cell::new(None) };
}
/// The summary's glass follows its hint (shown while a reward is pointed at).
pub(super) fn summary_rect(m: &mut Menu) {
    if !m.summary_visible {
        return;
    }
    let (Some((x, y, w, h)), hint) = (SUMMARY_GEOM.with(|c| c.get()), SUMMARY_HINT.with(|c| c.get())) else { return };
    let want_h = if hint.is_some() || m.gp.summary_hint_owner.is_some() { h + 105.0 } else { h };
    if m.summary_rect.as_ref().is_some_and(|r| (r.h - (want_h - 2.0 * (R_PANEL * 0.5).floor())).abs() > 0.5) {
        m.summary_rect = Some(glass_rect(x, y, w, want_h, R_PANEL));
    }
    if hint.is_none() {
        m.gp.summary_hint_owner = None;
    }
}
