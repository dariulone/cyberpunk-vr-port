//! reference_view.lua's character pages: the attributes around the level's hex (pointing shows one, a press opens its
//! perk tree), the skill progression with its rewards, an attribute's perk tree at the game's own slot positions (the
//! wires from RequiresPerks, OR branches among them; the tiers and their gates), a perk's or the attribute's details
//! with LEARN / REFUND / UPGRADE behind a confirmation; and the tarot collection.
use super::lobby_ink as ink;
use super::lobby_rtti::A;
use super::menu_core::*;
use super::menu_gameplay::{self as gp, t};
use super::menu_gpdata as gd;
use super::menu_ink as k;
use super::menu_locale as loc;
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use std::cell::Cell;
use std::rc::Rc;

const W: f64 = 1780.0;
thread_local! {
    /// The attribute pointed at last (s.attributeHover), set from its hex's paint.
    static HOVER: Cell<Option<f64>> = const { Cell::new(None) };
    /// The +1 confirmations opened so far: each card's number.
    static PROMPTS: Cell<u64> = const { Cell::new(0) };
}

fn hex(p: &Handle, x: f64, y: f64, w: f64, h: f64, col: [f64; 3], op: f64) -> Vec<Handle> {
    let cut = (w * 0.18).min(h * 0.5);
    let pts = [[x + cut, y], [x + w - cut, y], [x + w, y + h / 2.0], [x + w - cut, y + h], [x + cut, y + h], [x, y + h / 2.0]];
    (0..6).map(|i| gp::line(p, pts[i][0], pts[i][1], pts[(i + 1) % 6][0], pts[(i + 1) % 6][1], col, op, 3.0)).collect()
}
/// How far a ray from the centre of a hex (w x h) runs before it meets the outline.
fn hex_reach(w: f64, h: f64, dx: f64, dy: f64) -> f64 {
    let cut = (w * 0.18).min(h * 0.5);
    let pts = [[-w / 2.0 + cut, -h / 2.0], [w / 2.0 - cut, -h / 2.0], [w / 2.0, 0.0], [w / 2.0 - cut, h / 2.0], [-w / 2.0 + cut, h / 2.0], [-w / 2.0, 0.0]];
    let mut best = f64::INFINITY;
    for i in 0..6 {
        let (a, b) = (pts[i], pts[(i + 1) % 6]);
        let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
        let den = dx * ey - dy * ex;
        if den.abs() > 1e-9 {
            let (tt, u) = ((a[0] * ey - a[1] * ex) / den, (a[0] * dy - a[1] * dx) / den);
            if tt > 0.0 && (0.0..=1.0).contains(&u) {
                best = best.min(tt);
            }
        }
    }
    best
}
fn back(m: &mut Menu) {
    gp::redraw(m);
}

// ---------------------------------------------------------------- the skills
fn skills(m: &mut Menu, p: &Handle, d: &gd::Character) {
    glass(m, p, W, 1140.0, Some((P1X, P1Y)));
    gp::title(p, &loc::tr("РАЗВИТИЕ НАВЫКОВ", "SKILL PROGRESSION"), W);
    gp::head_action(
        m,
        p,
        P1X,
        W - 72.0,
        &loc::tr("НАЗАД", "BACK"),
        Rc::new(|m: &mut Menu, _| {
            m.gp.skills = false;
            back(m);
        }),
        true,
        None,
    );
    let info = k::text(p, 30.0, 1010.0, W - 60.0, 88.0, &loc::tr("Наведите на награду, чтобы прочитать её эффект", "Point at a reward to read its effect"), 31.0, k::CYAN, "left", false);
    for (i, skill) in d.skills.iter().enumerate() {
        let y = 130.0 + i as f64 * 165.0;
        let ratio = if skill.level >= skill.max { 1.0 } else { skill.xp / (skill.xp + skill.remaining).max(1.0) };
        k::text(p, 30.0, y, 420.0, 48.0, &format!("{}  {}", gd::fmt_g(skill.level), skill.name), 36.0, k::WHITE, "left", false);
        k::rect(p, 30.0, y + 62.0, 400.0, 4.0, k::ICE, 0.2);
        k::rect(p, 30.0, y + 62.0, 400.0 * ratio, 4.0, k::CYAN, 1.0);
        k::text(p, 30.0, y + 76.0, 400.0, 35.0, &format!("{} / {}   {}%", gd::fmt_g(skill.xp), gd::fmt_g(skill.xp + skill.remaining), (ratio * 100.0).floor() as i64), 26.0, k::GREY, "right", false);
        for (j, reward) in skill.rewards.iter().enumerate() {
            let x = 470.0 + j as f64 * 105.0;
            let col = if reward.unlocked { k::YELLOW } else { k::RED };
            let bg = k::round_fill(p, x, y + 8.0, 92.0, 94.0, col, if reward.unlocked { 0.12 } else { 0.035 }, Some(8.0));
            k::round_edge(p, x, y + 8.0, 92.0, 94.0, col, 0.55, Some(8.0));
            k::text(p, x, y + 12.0, 92.0, 40.0, &gd::fmt_g(reward.level), 31.0, col, "center", false);
            k::text(p, x, y + 54.0, 92.0, 40.0, "+", 33.0, col, "center", false);
            let text = gp::wrap(&format!("{} / {}: {}", skill.name, gd::fmt_g(reward.level), reward.text), 95).join("\n");
            let (i1, t1, i2, t2) = (info.clone(), text.clone(), info.clone(), text);
            button(
                m,
                "p1",
                &at(P1X, P1Y),
                x,
                y + 8.0,
                92.0,
                94.0,
                Some(Rc::new(move |_m: &mut Menu, _| ink::set_text(&i1, &t1))),
                Some(Rc::new(move |h, _| {
                    ink::opacity(&bg, if h { 0.25 } else { 0.1 });
                    if h {
                        ink::set_text(&i2, &t2);
                    }
                })),
            );
        }
    }
}

// ---------------------------------------------------------------- an attribute's perk tree
fn slot_position(perk: &gd::Perk, width: f64) -> (f64, f64, f64) {
    let pos = super::menu_perk_layout::slot(&perk.slot).unwrap_or((1920.0, 1200.0, 133.0, 142.0));
    let scale = ((width - 140.0) / 2808.0).min(850.0 / 1532.0);
    let left = (width - 2808.0 * scale) / 2.0;
    (left + (pos.0 - 516.0) * scale, 240.0 + (pos.1 - 421.0) * scale, 45f64.max(pos.2.min(pos.3) * scale))
}
fn node_frame(p: &Handle, x: f64, y: f64, size: f64, col: [f64; 3], op: f64, inset: f64) -> Vec<Handle> {
    let (r, cut) = (size / 2.0, size * 0.21);
    let pts = [[x - r + cut, y - r], [x + r - cut, y - r], [x + r, y - r + cut], [x + r, y + r - cut], [x + r - cut, y + r], [x - r + cut, y + r], [x - r, y + r - cut], [x - r, y - r + cut]];
    (0..8).map(|i| gp::line(p, pts[i][0], pts[i][1], pts[(i + 1) % 8][0], pts[(i + 1) % 8][1], col, op, if inset > 0.0 { inset } else { 3.0 })).collect()
}
fn wire(p: &Handle, from: (f64, f64), to: (f64, f64), col: [f64; 3], op: f64) {
    let points: Vec<[f64; 2]> = if (from.0 - to.0).abs() < 3.0 || (from.1 - to.1).abs() < 12.0 {
        vec![[from.0, from.1], [to.0, to.1]]
    } else {
        let middle = (from.1 + to.1) / 2.0;
        let dir = if to.0 > from.0 { 1.0 } else { -1.0 };
        let vert = if to.1 > from.1 { 1.0 } else { -1.0 };
        let bend = 15f64.min((to.0 - from.0).abs() / 4.0);
        vec![[from.0, from.1], [from.0, middle - vert * bend], [from.0 + dir * bend, middle], [to.0 - dir * bend, middle], [to.0, middle + vert * bend], [to.0, to.1]]
    };
    for offset in [-2.0, 0.0, 2.0] {
        for i in 0..points.len() - 1 {
            let (a, b) = (points[i], points[i + 1]);
            gp::line(p, a[0] + offset, a[1], b[0] + offset, b[1], col, op, 1.0);
        }
    }
}
fn node_col(perk: &gd::Perk, relic: bool) -> [f64; 3] {
    if perk.level > 0.0 {
        if relic {
            [0.35, 0.55, 1.0]
        } else {
            k::YELLOW
        }
    } else if perk.locked {
        k::RED
    } else {
        k::CYAN
    }
}
/// The +1 spends the point; the first one confirmed makes the next ones immediate (main's reference_view.lua buyAttribute).
fn buy_attribute(m: &mut Menu, kind: f64) {
    gp::perform(m, move |_| {
        if !gd::buy_attribute(kind) {
            return false;
        }
        if !super::menu_invdata::attribute_upgrade_confirmed() {
            super::menu_invdata::confirm_attribute_upgrade();
        }
        true
    });
}
/// The first +1's question, a card in the middle of the panel layer, modal over everything else.
fn confirm_attribute(m: &mut Menu, id: u64, a: &gd::Attr) {
    let (w, h) = (1160.0, 380.0);
    let (x, y) = ((CW - w) / 2.0, (CH - h) / 2.0);
    let card = ink::canvas(&m.layer_panel, x, y, w, h);
    let origin = at(x, y);
    m.modal_group = Some("attributePrompt".into());
    k::round_fill(&card, 0.0, 0.0, w, h, k::NAVY, 0.94, Some(12.0));
    glass(m, &card, w, h, Some((x, y)));
    k::text(&card, 40.0, 26.0, w - 80.0, 64.0, &t("upgradeQuestion"), 46.0, k::WHITE, "center", false);
    k::text(&card, 40.0, 96.0, w - 80.0, 64.0, &format!("{}   {} → {}", a.name, gd::fmt_g(a.value), gd::fmt_g(a.value + 1.0)), 42.0, k::CYAN, "center", false);
    k::text(&card, 40.0, 166.0, w - 80.0, 90.0, &gp::wrap(&t("upgradeHint"), 54).join("\n"), 32.0, k::TEXT, "center", false);
    let kind = a.ty;
    let resolve = move |accept: bool| -> Act {
        Rc::new(move |m: &mut Menu, _| {
            if m.gp.attribute_prompt.as_ref().map(|p| p.0) != Some(id) {
                return;
            }
            m.gp.attribute_prompt = None;
            if accept {
                buy_attribute(m, kind);
            } else {
                back(m);
            }
        })
    };
    action(m, &card, "attributePrompt", &origin, 68.0, 272.0, 490.0, 76.0, &t("yes"), true, resolve(true), None, None);
    action(m, &card, "attributePrompt", &origin, w - 558.0, 272.0, 490.0, 76.0, &t("cancel"), true, resolve(false), None, None);
}
fn tree(m: &mut Menu, p: &Handle, p2: &Handle, a: &gd::Attr, d: &gd::Character) {
    if let Some((id, pa)) = m.gp.attribute_prompt.clone() {
        return confirm_attribute(m, id, &pa);
    }
    let has_details = m.gp.perk.is_some();
    let (tree_w, detail_x, detail_w) = (if has_details { 1050.0 } else { W }, P1X + 1090.0, 690.0);
    ink::move_to(p2, detail_x, P1Y);
    glass(m, p, tree_w, 1190.0, Some((P1X, P1Y)));
    gp::title(p, &format!("{}  {}", a.name, gd::fmt_g(if a.relic { d.relic } else { a.value })), tree_w);
    k::text(p, 375.0, 16.0, 650.0, 48.0, &format!("{}: {}  /  {}: {}", t("perkPoints"), gd::fmt_g(d.perks), t("relic"), gd::fmt_g(d.relic)), 26.0, k::YELLOW, "right", false);
    let label = if has_details { loc::tr("< ВЕТКА", "< TREE") } else { loc::tr("ХАРАКТЕРИСТИКИ", "ATTRIBUTES") };
    gp::hub_action(
        m,
        p,
        P1X,
        20.0,
        100.0,
        480.0,
        &label,
        Rc::new(move |m: &mut Menu, _| {
            if has_details {
                m.gp.perk = None;
            } else {
                m.gp.attribute = None;
            }
            m.gp.confirm = None;
            back(m);
        }),
        true,
        None,
        None,
    );
    if a.ty != gd::stat("Espionage") {
        let a2 = a.clone();
        gp::hub_action(
            m,
            p,
            P1X,
            520.0,
            100.0,
            132.0,
            "+1",
            Rc::new(move |m: &mut Menu, _| {
                if super::menu_invdata::attribute_upgrade_confirmed() {
                    buy_attribute(m, a2.ty);
                } else {
                    let id = PROMPTS.with(|c| {
                        c.set(c.get() + 1);
                        c.get()
                    });
                    (m.gp.confirm, m.gp.attribute_prompt) = (None, Some((id, a2.clone())));
                    back(m);
                }
            }),
            a.can_buy,
            Some(68.0),
            Some(44.0),
        );
        k::text(p, 672.0, 100.0, tree_w - 690.0, 68.0, &format!("{}: {}", t("points"), gd::fmt_g(d.points)), 28.0, k::CYAN, "left", false);
    }
    let aty = a.ty;
    let perks = gp::fetch::<Vec<gd::Perk>>(m, &format!("perks:{}", gd::fmt_g(aty)), super::menu_jobs::once(move || (gd::perks(aty), None)), f64::INFINITY, 0.05, false, None).unwrap_or_default();
    let positions: Vec<(f64, f64, f64)> = perks.iter().map(|pk| slot_position(pk, tree_w)).collect();
    let pos_of = |key: &str| perks.iter().position(|pk| pk.key == key).map(|i| positions[i]);
    let selected = perks.iter().find(|pk| Some(&pk.key) == m.gp.perk.as_ref()).cloned();
    let mut sig = vec![a.name.clone(), gd::fmt_g(tree_w), a.relic.to_string()];
    for pk in perks.iter() {
        sig.push(format!("{}:{}/{}{}", pk.key, gd::fmt_g(pk.level), gd::fmt_g(pk.max), if pk.locked { "L" } else { "" }));
    }
    let key = format!("tree:{}", sig.join("|"));
    let slot = format!("{}:{}", gd::fmt_g(aty), gd::fmt_g(tree_w));
    if let Some(old) = m.gp.tree_keys.get(&slot).filter(|o| **o != key).cloned() {
        m.pool.drop_key(&old);
    }
    m.gp.tree_keys.insert(slot, key.clone());
    let relic = a.relic;
    let perks2 = perks.clone();
    let positions2 = positions.clone();
    let (nodes, reused) = m.pool.kept(&key, p, 0.0, 0.0, tree_w, 1190.0, &move |c: &Handle| {
        for (i, pk) in perks2.iter().enumerate() {
            let to = positions2[i];
            for parent in &pk.parents {
                if let Some(j) = perks2.iter().position(|x| &x.key == parent) {
                    let from = positions2[j];
                    let fp = &perks2[j];
                    let col = if fp.level >= fp.max { k::YELLOW } else { k::RED };
                    wire(c, (from.0, from.1), (to.0, to.1), col, if pk.level > 0.0 { 0.9 } else { 0.25 });
                }
            }
        }
        let mut out = Vec::new();
        for (i, pk) in perks2.iter().enumerate() {
            let (x, y, size) = positions2[i];
            let col = node_col(pk, relic);
            let bg = k::round_fill(c, x - size / 2.0, y - size / 2.0, size, size, k::BLACK, 0.82, Some(12.0));
            if pk.level > 0.0 {
                node_frame(c, x, y, size + 14.0, col, 0.12, 10.0);
            }
            let edges = node_frame(c, x, y, size + 4.0, col, if pk.locked { 0.25 } else { 1.0 }, 0.0);
            if size > 70.0 {
                node_frame(c, x, y, size + 16.0, col, 0.3, 0.0);
            }
            gp::icon(c, x - size * 0.36, y - size * 0.36, size * 0.72, size * 0.72, pk.icon.as_ref(), Some(col));
            if pk.slot.contains("Milestone") || pk.max > 1.0 {
                k::round_fill(c, x - 26.0, y + size / 2.0 - 11.0, 52.0, 27.0, col, if pk.level > 0.0 { 0.8 } else { 0.12 }, Some(3.0));
                k::text(c, x - 26.0, y + size / 2.0 - 11.0, 52.0, 27.0, &format!("{}/{}", gd::fmt_g(pk.level), gd::fmt_g(pk.max)), 21.0, if pk.level > 0.0 { k::BLACK } else { col }, "center", false);
            }
            out.push(bg);
            out.extend(edges);
        }
        out
    });
    for (i, pk) in perks.iter().enumerate() {
        let (x, y, size) = positions[i];
        let Some(n) = nodes.get(i * 9..i * 9 + 9) else { continue };
        let (bg, edges) = (n[0].clone(), n[1..].to_vec());
        let col = node_col(pk, relic);
        let rest = if pk.locked { 0.25 } else { 1.0 };
        if reused {
            k::tint(&edges, col, Some(rest));
            ink::tint(&bg, k::BLACK);
            ink::opacity(&bg, 0.82);
        }
        let pkey = pk.key.clone();
        let (e2, b2) = (edges.clone(), bg.clone());
        let b = button(
            m,
            "p1",
            &at(P1X, P1Y),
            x - size / 2.0 - 6.0,
            y - size / 2.0 - 6.0,
            size + 12.0,
            size + 12.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                (m.gp.perk, m.gp.confirm) = (Some(pkey.clone()), None);
                back(m);
            })),
            Some(Rc::new(move |h, _| {
                k::tint(&e2, if h { k::WHITE } else { col }, Some(if h { 1.0 } else { rest }));
                ink::tint(&b2, if h { k::CYAN } else { k::BLACK });
                ink::opacity(&b2, if h { 0.22 } else { 0.82 });
            })),
        );
        let w = size + 12.0;
        *b.contains.borrow_mut() = Some(Rc::new(move |px, py| {
            let cut = w * 0.21;
            px + py >= cut && px + w - py >= cut && w - px + py >= cut && 2.0 * w - px - py >= cut
        }));
        *b.key.borrow_mut() = Some(pk.key.clone());
        if m.gp.perk.as_ref() == Some(&pk.key) {
            k::tint(&edges, k::WHITE, Some(1.0));
        }
    }
    let _ = pos_of;
    if !a.relic {
        let scale = ((tree_w - 140.0) / 2808.0).min(850.0 / 1532.0);
        for (need, ty, ru, en) in [(20.0, 580.5, "МАСТЕР", "MASTER"), (15.0, 1011.0, "ЭКСПЕРТ", "EXPERT"), (9.0, 1457.0, "ПРОФИ", "PRO"), (4.0, 1841.0, "НОВИЧОК", "ROOKIE")] {
            let y = 240.0 + (ty - 421.0) * scale;
            let locked = a.value < need;
            k::text(p, tree_w - 135.0, y - 24.0, 115.0, 32.0, &loc::tr(ru, en), 18.0, if locked { k::RED } else { k::CYAN }, "right", false);
            if locked {
                k::text(p, tree_w - 135.0, y + 10.0, 115.0, 28.0, &format!("{} / {}", gd::fmt_g(a.value), gd::fmt_g(need)), 18.0, k::RED, "right", false);
            }
        }
        for (need, gy) in [(9.0, 1650.0), (15.0, 1234.0), (20.0, 795.0)] {
            if a.value < need {
                let y = 240.0 + (gy - 421.0) * scale;
                gp::line(p, 20.0, y, tree_w - 20.0, y, k::RED, 0.7, 2.0);
                k::round_edge(p, tree_w / 2.0 - 9.0, y - 12.0, 18.0, 23.0, k::RED, 1.0, Some(3.0));
                break;
            }
        }
    }
    let Some(sel) = selected else { return };
    glass(m, p2, detail_w, 1190.0, Some((detail_x, P1Y)));
    gp::title(p2, &sel.name, detail_w);
    let mut rows = Vec::new();
    let mut top = 100.0;
    if sel.video != 0 {
        let video = ink::place(ink::new("inkVideoWidget"), p2, 20.0, 100.0, detail_w - 40.0, (detail_w - 40.0) * 9.0 / 16.0);
        super::lobby_rtti::call(&video, "SetVideoPath", &[A::P((&sel.video as *const u64).cast())]);
        ink::call(&video, "SetLoop", &[Value::Bool(true)]);
        ink::call(&video, "Play", &[]);
        m.gp.videos.push(video);
        top = 480.0;
    }
    gp::text_rows(&mut rows, &sel.name, detail_w, Some(k::CYAN), Some(34.0));
    gp::text_rows(&mut rows, &format!("{}: {} {}  /  {}: {}", t("requires"), a.name, gd::fmt_g(sel.required), t("cost"), gd::fmt_g(sel.cost)), detail_w, Some(k::YELLOW), Some(28.0));
    for lv in gd::perk_description(&sel) {
        gp::text_rows(&mut rows, &format!("{} {}\n{}\n", t("level"), lv.level, lv.text), detail_w, Some(if lv.level as f64 <= sel.level { k::YELLOW } else { k::TEXT }), Some(30.0));
    }
    gp::glist(m, p2, "p2", "p2", detail_x, top, detail_w, rows, 1040.0 - top, None);
    let kind = sel.ty;
    if let Some(conf) = m.gp.confirm.clone() {
        gp::hub_action(
            m,
            p2,
            detail_x,
            18.0,
            1110.0,
            360.0,
            &t("confirm"),
            Rc::new(move |m: &mut Menu, _| {
                let refund = conf == "refund";
                gp::perform(m, |_| gd::change_perk(aty, kind, refund));
            }),
            true,
            None,
            None,
        );
        gp::hub_action(
            m,
            p2,
            detail_x,
            392.0,
            1110.0,
            280.0,
            &t("cancel"),
            Rc::new(|m: &mut Menu, _| {
                m.gp.confirm = None;
                back(m);
            }),
            true,
            None,
            None,
        );
    } else {
        gp::hub_action(
            m,
            p2,
            detail_x,
            18.0,
            1110.0,
            300.0,
            &t("buy"),
            Rc::new(|m: &mut Menu, _| {
                m.gp.confirm = Some("buy".into());
                back(m);
            }),
            sel.can_buy,
            None,
            None,
        );
        gp::hub_action(
            m,
            p2,
            detail_x,
            336.0,
            1110.0,
            336.0,
            &t("refund"),
            Rc::new(|m: &mut Menu, _| {
                m.gp.confirm = Some("refund".into());
                back(m);
            }),
            sel.can_sell,
            None,
            None,
        );
    }
}

// ---------------------------------------------------------------- the attributes
fn show_detail(host: &Handle, title: &Handle, a: &gd::Attr) {
    ink::set_text(title, &k::clip(&a.name, 40));
    ink::remove_all(host);
    gp::icon(host, 28.0, 18.0, 100.0, 100.0, a.icon.as_ref(), Some(k::CYAN));
    let lv = if a.relic { t("relic") } else { format!("{} {} / {}", t("level"), gd::fmt_g(a.value), gd::fmt_g(a.max)) };
    k::text(host, 152.0, 35.0, 692.0, 65.0, &lv, 36.0, k::CYAN, "left", false);
    let lines = gp::wrap(&a.description, 47);
    k::text(host, 28.0, 146.0, 844.0, 750f64.min(lines.len() as f64 * 43.0), &lines.join("\n"), 32.0, k::TEXT, "left", false);
    k::text(host, 28.0, 935.0, 844.0, 48.0, &loc::tr("НАЖМИТЕ, ЧТОБЫ ОТКРЫТЬ ВЕТКУ", "CLICK TO OPEN THE PERK TREE"), 30.0, k::CYAN, "left", false);
}
pub(super) fn character(m: &mut Menu, p: &Handle, p2: &Handle) {
    let Some(d) = gp::fetch::<gd::Character>(m, "character", super::menu_jobs::once(|| (gd::character().unwrap_or_default(), None)), f64::INFINITY, 0.05, false, None) else { return };
    if m.gp.skills {
        return skills(m, p, &d);
    }
    if let Some(a) = d.attributes.iter().find(|a| Some(a.ty) == m.gp.attribute).cloned() {
        return tree(m, p, p2, &a, &d);
    }
    glass(m, p, 820.0, 1140.0, Some((P1X, P1Y)));
    gp::title(p, &t("character"), 820.0);
    for (i, (label, value, col)) in [(t("attributes"), d.points, k::CYAN), (t("perks"), d.perks, k::YELLOW), (t("relic"), d.relic, k::CYAN)].iter().enumerate() {
        let x = 22.0 + i as f64 * 266.0;
        k::text(p, x, 96.0, 244.0, 30.0, label, 22.0, *col, "center", false);
        k::text(p, x, 126.0, 244.0, 42.0, &gd::fmt_g(*value), 34.0, *col, "center", false);
    }
    let place = |name: &str| -> Option<(f64, f64)> {
        Some(match name {
            "Strength" => (190.0, 380.0),
            "Reflexes" => (410.0, 255.0),
            "TechnicalAbility" => (630.0, 380.0),
            "Intelligence" => (190.0, 770.0),
            "Cool" => (630.0, 770.0),
            "Espionage" => (410.0, 915.0),
            _ => return None,
        })
    };
    let hover = HOVER.with(|h| h.get());
    let Some(detail) = d.attributes.iter().find(|a| Some(a.ty) == hover).or(d.attributes.first()).cloned() else { return };
    glass(m, p2, 900.0, 1140.0, Some((P2X, P1Y)));
    let detail_title = k::text(p2, 28.0, 12.0, 844.0, 60.0, &k::clip(&detail.name, 40), 42.0, k::CYAN, "left", false);
    k::rect(p2, 28.0, 82.0, 844.0, 1.0, k::CYAN, 0.32);
    let detail_host = ink::canvas(p2, 0.0, 96.0, 900.0, 1000.0);
    show_detail(&detail_host, &detail_title, &detail);
    let center = (410.0, 575.0);
    for a in &d.attributes {
        if let Some(xy) = place(&a.type_name()) {
            let (dx, dy) = (xy.0 - center.0, xy.1 - center.1);
            let len = (dx * dx + dy * dy).sqrt();
            let (dx, dy) = (dx / len, dy / len);
            let (s0, s1) = (hex_reach(160.0, 160.0, dx, dy) + 10.0, len - hex_reach(290.0, 164.0, -dx, -dy) - 10.0);
            if s1 > s0 {
                gp::line(p, center.0 + dx * s0, center.1 + dy * s0, center.0 + dx * s1, center.1 + dy * s1, k::CYAN, 0.23, 3.0);
            }
        }
    }
    hex(p, 330.0, 495.0, 160.0, 160.0, k::CYAN, 0.8);
    k::text(p, 330.0, 520.0, 160.0, 70.0, &gd::fmt_g(d.level), 54.0, k::CYAN, "center", false);
    k::text(p, 330.0, 587.0, 160.0, 40.0, &t("level"), 28.0, k::CYAN, "center", false);
    for (i, a) in d.attributes.iter().enumerate() {
        let Some(xy) = place(&a.type_name()) else { continue };
        let (x, y) = (xy.0 - 145.0, xy.1 - 82.0);
        let c = ink::canvas(p, x, y, 290.0, 164.0);
        let fill = k::round_fill(&c, 24.0, 0.0, 242.0, 164.0, k::NAVY, 0.18, Some(8.0));
        let edge = hex(&c, 0.0, 0.0, 290.0, 164.0, k::CYAN, 0.55);
        k::text(&c, 20.0, 12.0, 250.0, 44.0, &a.name, 28.0, k::CYAN, "center", false);
        gp::icon(&c, 92.0, 70.0, 56.0, 56.0, a.icon.as_ref(), Some(k::CYAN));
        k::text(&c, 155.0, 72.0, 100.0, 60.0, &if a.relic { "R".to_string() } else { gd::fmt_g(a.value) }, 40.0, k::CYAN, "left", false);
        let ty = a.ty;
        let (a2, host, title) = (a.clone(), detail_host.clone(), detail_title.clone());
        let b = button(
            m,
            "p1",
            &at(P1X, P1Y),
            x,
            y,
            290.0,
            164.0,
            Some(Rc::new(move |m: &mut Menu, _| {
                let g = &mut m.gp;
                (g.attribute, g.perk, g.confirm, g.skills) = (Some(ty), None, None, false);
                back(m);
            })),
            Some(Rc::new(move |h, _| {
                k::tint(&edge, if h { k::YELLOW } else { k::CYAN }, Some(if h { 1.0 } else { 0.55 }));
                ink::opacity(&fill, if h { 0.18 } else { 0.06 });
                if h && HOVER.with(|c| c.get()) != Some(a2.ty) {
                    HOVER.with(|c| c.set(Some(a2.ty)));
                    show_detail(&host, &title, &a2);
                }
            })),
        );
        *b.contains.borrow_mut() = Some(Rc::new(|px, py| px >= (py - 82.0).abs() * 0.35 && px <= 290.0 - (py - 82.0).abs() * 0.35));
        if m.anim1 {
            slide_in(m, "p1", &c, x, y, i as f64 * 0.025, 0.0, 26.0, None);
        }
    }
    gp::hub_action(
        m,
        p,
        P1X,
        25.0,
        1050.0,
        770.0,
        &loc::tr("РАЗВИТИЕ НАВЫКОВ", "SKILL PROGRESSION"),
        Rc::new(|m: &mut Menu, _| {
            m.gp.skills = true;
            back(m);
        }),
        true,
        None,
        None,
    );
}

// ---------------------------------------------------------------- the tarot
pub(super) fn tarot(m: &mut Menu, p: &Handle, _p2: &Handle) {
    let (cards, count, total) = gd::tarot();
    let selected = cards.iter().find(|c| Some(c.index) == m.gp.tarot).cloned();
    let collection_w = 1280.0;
    let open = selected.as_ref().is_some_and(|c| c.found);
    let panel_w = if open { W } else { collection_w };
    glass(m, p, panel_w, 1305.0, Some((P1X, P1Y)));
    gp::title(p, &format!("{}  {} / {}", loc::tr("КАРТЫ ТАРО", "TAROT CARDS"), count, total), panel_w);
    if let Some(sel) = selected.filter(|c| c.found) {
        gp::hub_action(
            m,
            p,
            P1X,
            20.0,
            100.0,
            280.0,
            &loc::tr("К КОЛЛЕКЦИИ", "COLLECTION"),
            Rc::new(|m: &mut Menu, _| {
                m.gp.tarot = None;
                back(m);
            }),
            true,
            None,
            None,
        );
        gp::picture(m, p, 80.0, 200.0, 530.0, 900.0, sel.image.as_ref(), None);
        k::text(p, 700.0, 125.0, 1000.0, 66.0, &sel.name, 42.0, k::CYAN, "left", false);
        let mut rows = Vec::new();
        gp::text_rows(&mut rows, &sel.description, 1000.0, Some(k::TEXT), Some(34.0));
        let host = ink::canvas(p, 680.0, 0.0, 1040.0, 1100.0);
        gp::glist(m, &host, "p2", "p2", P1X + 680.0, 220.0, 1040.0, rows, 820.0, None);
        return;
    }
    let mut rows = Vec::new();
    for group in cards.chunks(5) {
        let group = group.to_vec();
        rows.push(Row {
            h: 450.0,
            build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                for (i, card) in group.iter().enumerate() {
                    let x = 7.0 + i as f64 * 248.0;
                    let col = if card.found { k::CYAN } else { k::GREY };
                    let edge = k::round_edge(c, x, 5.0, 228.0, 440.0, col, 0.6, Some(8.0));
                    gp::picture(m, c, x + 14.0, 11.0, 200.0, 400.0, card.image.as_ref(), Some((it.view.clone(), it.y)));
                    k::text(c, x + 8.0, 411.0, 212.0, 30.0, &if card.found { k::clip(&card.name, 19) } else { "?".into() }, 24.0, col, "center", false);
                    let (found, index, view) = (card.found, card.index, it.view.clone());
                    button(
                        m,
                        "p1",
                        &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                        x,
                        5.0,
                        228.0,
                        440.0,
                        Some(Rc::new(move |m: &mut Menu, _| {
                            if found {
                                m.gp.tarot_offset = Some(view.borrow().offset);
                                m.gp.tarot = Some(index);
                                back(m);
                            }
                        })),
                        Some(Rc::new(move |h, _| k::tint(&edge, col, Some(if h { 1.0 } else { 0.6 })))),
                    );
                }
            }),
        });
    }
    let keep = m.gp.tarot_offset.or(m.keep_scroll);
    let view = gp::glist(m, p, "p1", "p1", P1X, 110.0, collection_w, rows, 1125.0, keep);
    view.borrow_mut().enable_mask();
}
