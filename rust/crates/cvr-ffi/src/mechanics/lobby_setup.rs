//! modules/setup.lua: the first launch's setup on the holo before the main menu -- the weapon hand, the left hand's
//! bindings, the full body or the hands only, the T-pose calibration (a countdown, then the plugin samples the arms:
//! VRCalibrationStart / State / Progress) or the height by hand, confirmed into the plugin (VRSetPlayerHeight). The
//! texts in the game's language (lobby_setup_texts.json, made from the Lua table by the scratch generator).
use super::lobby::Lobby;
use super::lobby_holo::{self as holo, Card, Page, PageRef, Row};
use super::lobby_spawn::native;
use crate::script_call::Value;
use serde_json::Value as J;
use std::rc::Rc;
use std::sync::OnceLock;

const READY_SECONDS: f64 = 3.0;
const SAMPLE_SECONDS: f64 = 4.0;
const ART: &str = "vrport\\ui\\vr_lobby_setup.inkatlas";

static TEXTS: OnceLock<J> = OnceLock::new();
fn texts() -> &'static J {
    let all = TEXTS.get_or_init(|| serde_json::from_str(include_str!("lobby_setup_texts.json")).unwrap_or(J::Null));
    let lang = super::lobby_ink::language();
    all.get(&lang).or_else(|| all.get("en-us")).unwrap_or(&J::Null)
}
fn tx(k: &str) -> String {
    texts().get(k).and_then(J::as_str).unwrap_or("").to_string()
}
pub(super) struct Setup {
    phase: &'static str,
    clock: f64,
    height: f64,
    shown: Option<String>,
    finish: Option<Rc<dyn Fn(&mut Lobby)>>,
    active: bool,
}
impl Default for Setup {
    fn default() -> Setup {
        Setup { phase: "idle", clock: 0.0, height: 1.75, shown: None, finish: None, active: false }
    }
}
fn height_text(h: f64) -> String {
    let inches = (h / 0.0254 + 0.5).floor() as i64;
    format!("{h:.2} m · {}' {}\"", inches / 12, inches % 12)
}
/// M.needed: the plugin has not been set up yet.
pub(super) fn needed() -> bool {
    native::<bool>("VRFirstSetupNeeded", &[]) == Some(true)
}
fn suggested() -> f64 {
    native::<f32>("VRSuggestedHeight", &[]).map_or(1.75, f64::from)
}
fn card(key: &str, part: &str, label: String, on: bool, go: impl Fn(&mut Lobby, usize) + 'static) -> Card {
    let _ = key;
    Card { label, picture: (ART.into(), part.into()), on, go: Some(Rc::new(go)) }
}
fn page(title: &str, sub: &str, code: &str, rows: impl Fn(&mut Lobby) -> Vec<Row> + 'static) -> Page {
    let mut p = Page::new(Some(tx(title)), code, rows);
    p.subtitle = Some(tx(sub));
    p
}
fn height_page() -> PageRef {
    Rc::new(page("heightTitle", "heightSub", "SETUP//BODY", |l| {
        let step = |l: &mut Lobby, d: f64| {
            l.setup.height = 1.40f64.max(2.20f64.min(((l.setup.height + d) * 100.0 + 0.5).floor() / 100.0));
            holo::refresh(l);
        };
        vec![
            Row { text: true, ..Row::new("h1", &tx("height1")) },
            Row { value: Some(height_text(l.setup.height)), left: Some(Rc::new(move |l, _| step(l, -0.01))), right: Some(Rc::new(move |l, _| step(l, 0.01))), ..Row::new("height", &tx("heightRow")) },
            Row::new("confirm", &tx("confirm")).go(|l, _| {
                native::<bool>("VRSetPlayerHeight", &[Value::Float(l.setup.height as f32)]);
                if let Some(f) = l.setup.finish.clone() {
                    f(l);
                }
            }),
        ]
    }))
}
fn calibration_page() -> PageRef {
    let mut p = page("calTitle", "calSub", "SETUP//T-POSE", |l| {
        let mut rows = vec![
            Row { text: true, ..Row::new("c1", &tx("cal1")) },
            Row { text: true, ..Row::new("c2", &tx("cal2")) },
            Row { picture: Some((ART.into(), "tpose".into())), ..Row::new("pose", "") },
        ];
        match l.setup.phase {
            "ready" => rows.push(Row { detail: Some(format!("{}", (READY_SECONDS - l.setup.clock).ceil() as i64)), disabled: true, ..Row::new("state", &tx("ready")) }),
            "sampling" => {
                let p = native::<f32>("VRCalibrationProgress", &[]).map_or(0.0, f64::from);
                rows.push(Row { detail: Some(format!("{}%", (p * 100.0).floor() as i64)), disabled: true, ..Row::new("state", &tx("hold")) });
            }
            _ => {
                if l.setup.phase == "failed" {
                    rows.push(Row { text: true, warn: true, ..Row::new("failed", &tx("failed")) });
                }
                rows.push(Row::new("start", &tx("start")).go(|l, _| {
                    (l.setup.phase, l.setup.clock) = ("ready", 0.0);
                    holo::refresh(l);
                }));
                rows.push(Row::new("skip", &tx("skip")).go(|l, _| {
                    l.setup.phase = "idle";
                    l.setup.height = suggested();
                    holo::push(l, height_page());
                }));
            }
        }
        rows
    });
    p.on_back = Some(Rc::new(|l| {
        if l.setup.phase == "ready" {
            l.setup.phase = "idle";
        }
    }));
    Rc::new(p)
}
fn body_page() -> PageRef {
    Rc::new(page("bodyTitle", "bodySub", "SETUP//AVATAR", |_l| {
        let hands_only = native::<bool>("VRBodyHandsOnly", &[]) == Some(true);
        let choose = |l: &mut Lobby, h: bool| {
            native::<bool>("VRSetBodyHandsOnly", &[Value::Bool(h)]);
            l.setup.phase = "idle";
            holo::push(l, calibration_page());
        };
        vec![
            Row {
                cards: Some(vec![card("full", "body_full", tx("bodyFull"), !hands_only, move |l, _| choose(l, false)), card("hands", "body_hands", tx("bodyHands"), hands_only, move |l, _| choose(l, true))]),
                ..Row::new("body", "")
            },
            Row { text: true, ..Row::new("later", &tx("later")) },
        ]
    }))
}
fn bindings_page() -> PageRef {
    Rc::new(page("bindTitle", "bindSub", "SETUP//BINDINGS", |_l| {
        let mode = native::<i32>("VRLeftHandedBindings", &[]).unwrap_or(2);
        let choose = |l: &mut Lobby, m: i32| {
            native::<bool>("VRSetLeftHandedBindings", &[Value::Int32(m)]);
            holo::push(l, body_page());
        };
        vec![
            Row {
                cards: Some(vec![
                    card("same", "bind_same", tx("bindSame"), mode == 0, move |l, _| choose(l, 0)),
                    card("walk", "bind_walk", tx("bindWalk"), mode == 1, move |l, _| choose(l, 1)),
                    card("turn", "bind_turn", tx("bindTurn"), mode == 2, move |l, _| choose(l, 2)),
                ]),
                ..Row::new("bindings", "")
            },
            Row { text: true, ..Row::new("note", &tx("bindNote")) },
            Row { text: true, ..Row::new("later", &tx("later")) },
        ]
    }))
}
fn hand_page() -> PageRef {
    Rc::new(page("handTitle", "handSub", "SETUP//HAND", |_l| {
        let left = native::<bool>("VRLeftHanded", &[]) == Some(true);
        let choose = |l: &mut Lobby, lft: bool| {
            native::<bool>("VRSetLeftHanded", &[Value::Bool(lft)]);
            holo::push(l, if lft { bindings_page() } else { body_page() });
        };
        vec![
            Row { cards: Some(vec![card("left", "hand_left", tx("left"), left, move |l, _| choose(l, true)), card("right", "hand_right", tx("right"), !left, move |l, _| choose(l, false))]), ..Row::new("hands", "") },
            Row { text: true, ..Row::new("later", &tx("later")) },
        ]
    }))
}
/// M.root: the setup's first page; `finish` runs once the height is confirmed.
pub(super) fn root(l: &mut Lobby, finish: Rc<dyn Fn(&mut Lobby)>) -> PageRef {
    (l.setup.finish, l.setup.phase, l.setup.active) = (Some(finish), "idle", true);
    hand_page()
}
pub(super) fn update(l: &mut Lobby, dt: f64) {
    if !l.setup.active || !holo::is_open(l) {
        return;
    }
    let shown = match l.setup.phase {
        "ready" => {
            l.setup.clock += dt;
            if l.setup.clock >= READY_SECONDS {
                native::<bool>("VRCalibrationStart", &[Value::Float(SAMPLE_SECONDS as f32)]);
                l.setup.phase = "sampling";
            }
            format!("ready{}", (READY_SECONDS - l.setup.clock).ceil() as i64)
        }
        "sampling" => {
            let st = native::<i32>("VRCalibrationState", &[]).unwrap_or(0);
            if st == 2 {
                l.setup.phase = "idle";
                l.setup.height = suggested();
                holo::push(l, height_page());
            } else if st == 3 || st == 0 {
                l.setup.phase = "failed";
            }
            format!("{}{}", l.setup.phase, (native::<f32>("VRCalibrationProgress", &[]).map_or(0.0, f64::from) * 20.0).floor() as i64)
        }
        p => p.to_string(),
    };
    if l.setup.shown.as_deref() != Some(shown.as_str()) {
        l.setup.shown = Some(shown);
        holo::refresh(l);
    }
}
