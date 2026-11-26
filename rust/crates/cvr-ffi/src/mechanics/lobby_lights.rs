//! modules/lights.lua: the lobby scene's lights at the player's level (light.txt beside the mod, 0..3, 1 by default)
//! -- every light component of the scene's entities at its own intensity times the level -- and, when the scene has a
//! dial, the dial turned by a gripping wrist or stepped through its stops by a press.
use super::belt::qrot;
use super::lobby::Lobby;
use super::lobby_hands::{self as hands, len, Face};
use super::lobby_spawn::{self as sp, dot, mul, native, sub, V3};
use crate::script_call::Value;
use std::collections::HashMap;

const SCENE_TAG: &str = "VRLobbyScene";
const LIGHT: &str = "entLightComponent";
const LEVEL_MAX: f64 = 3.0;
const LEVEL_DEFAULT: f64 = 1.0;
const STOPS: [f64; 5] = [0.0, 0.5, 1.0, 2.0, 3.0];
const WRIST_GAIN: f64 = 1.5;
const REACH: f64 = 0.10;
const HAPTIC_TICK: i32 = 21;
const HAPTIC_CLICK: i32 = 20;
const SOUND: &str = "CyberpunkVRPort_Menu/sounds/click.wav";
const WHO: &str = "lights";
fn travel() -> f64 {
    270f64.to_radians()
}
pub(super) struct DialSpec {
    pub centre: V3,
    pub right: V3,
    pub up: V3,
    pub front: V3,
    pub radius: f64,
}
struct Dial {
    face: Face,
    n: V3,
    radius: f64,
}
struct HeldDial {
    hand: usize,
    axis: V3,
    last: f64,
}
#[derive(Default)]
pub(super) struct Lights {
    level: Option<f64>,
    dial: Option<Dial>,
    base: HashMap<String, f32>,
    applied: Option<f64>,
    held: Option<HeldDial>,
}
fn file() -> std::path::PathBuf {
    cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_Lobby\\light.txt")
}
fn load() -> f64 {
    std::fs::read_to_string(file()).ok().and_then(|s| s.lines().next().and_then(|l| l.trim().parse::<f64>().ok())).map_or(LEVEL_DEFAULT, |v| v.clamp(0.0, LEVEL_MAX))
}
fn save(s: &Lights) {
    let _ = std::fs::write(file(), format!("{:.3}\n", s.level.unwrap_or(LEVEL_DEFAULT)));
}
fn apply(s: &mut Lights) -> usize {
    let level = s.level.unwrap_or(LEVEL_DEFAULT);
    let mut n = 0;
    let ents = sp::native_owned("VRSpawnTagged", &[sp::cn(SCENE_TAG)]).map(|o| super::belt::handles(o.as_ptr())).unwrap_or_default();
    for e in ents {
        let id = super::belt::entity_hash(&e).unwrap_or(0);
        for c in super::belt::components(&e) {
            if super::belt::class_name(&c) != LIGHT {
                continue;
            }
            let key = format!("{id}/{}", super::belt::comp_name(&c));
            if !s.base.contains_key(&key) {
                // SAFETY: the light component's Float property.
                let v = unsafe { crate::script_call::property(c.instance, "intensity") }.map_or(1.0, |p| unsafe { *(p as *const f32) });
                s.base.insert(key.clone(), v);
            }
            let b = f64::from(s.base[&key]);
            super::lobby_ink::call(&c, "SetIntensity", &[Value::Float((b * level) as f32)]);
            n += 1;
        }
    }
    s.applied = Some(level);
    n
}
fn set(s: &mut Lights, level: f64, hand: Option<usize>) {
    let level = level.clamp(0.0, LEVEL_MAX);
    let cur = s.level.unwrap_or(LEVEL_DEFAULT);
    let before = (cur / LEVEL_MAX * 10.0 + 1e-6).floor();
    s.level = Some(level);
    if (level / LEVEL_MAX * 10.0 + 1e-6).floor() != before {
        if let Some(h) = hand {
            sp::haptic(h, HAPTIC_TICK);
        }
    }
}
fn wrist_angle(d: &Dial, rot: [f64; 4], axis: V3) -> f64 {
    let a = qrot(rot, axis);
    dot(a, d.face.u).atan2(dot(a, d.face.r))
}
fn take(l: &mut Lobby, hand: usize) {
    let Some(d) = l.lights.dial.as_ref() else { return };
    let h = l.hands.h(hand);
    let Some(pos) = h.pos else { return };
    let dv = sub(pos, d.face.c);
    let along = dot(dv, d.n);
    let across = len(sub(dv, mul(d.n, along)));
    if across > d.radius + REACH || !(-0.03..=REACH + 0.05).contains(&along) {
        return;
    }
    let (mut best, mut flat) = ([1.0, 0.0, 0.0], -1.0);
    for axis in [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]] {
        let f = 1.0 - dot(qrot(h.rot, axis), d.n).abs();
        if f > flat {
            (best, flat) = (axis, f);
        }
    }
    let last = wrist_angle(d, h.rot, best);
    l.lights.held = Some(HeldDial { hand, axis: best, last });
    l.hands.claim(hand, WHO);
    sp::haptic(hand, HAPTIC_CLICK);
}
fn turn(l: &mut Lobby) {
    let Some(p) = l.lights.held.as_ref() else { return };
    let hand = p.hand;
    let h = l.hands.h(hand).clone();
    if !h.grip || h.pos.is_none() {
        l.hands.release(hand, WHO);
        l.lights.held = None;
        save(&l.lights);
        return;
    }
    let Some(d) = l.lights.dial.as_ref() else { return };
    let a = wrist_angle(d, h.rot, p.axis);
    let mut dd = a - p.last;
    if dd > std::f64::consts::PI {
        dd -= 2.0 * std::f64::consts::PI;
    } else if dd < -std::f64::consts::PI {
        dd += 2.0 * std::f64::consts::PI;
    }
    if let Some(p) = l.lights.held.as_mut() {
        p.last = a;
    }
    let level = l.lights.level.unwrap_or(LEVEL_DEFAULT) - dd * WRIST_GAIN / travel() * LEVEL_MAX;
    set(&mut l.lights, level, Some(hand));
}
fn step(s: &mut Lights, hand: usize) {
    let cur = s.level.unwrap_or(LEVEL_DEFAULT);
    let next = STOPS.iter().copied().find(|v| *v > cur + 1e-3).unwrap_or(STOPS[0]);
    set(s, next, None);
    sp::haptic(hand, HAPTIC_CLICK);
    let f = sp::Str::new(SOUND);
    native::<bool>("VRMenuPlayWav", &[f.v(), Value::Float(0.8)]);
    save(s);
}
pub(super) fn open(l: &mut Lobby, anchor: Option<(V3, f64)>, dial: Option<&DialSpec>) {
    close(l);
    if l.lights.level.is_none() {
        l.lights.level = Some(load());
    }
    l.lights.base.clear();
    if let (Some((pos, yaw)), Some(d)) = (anchor, dial) {
        let r = 2.0 * d.radius;
        l.lights.dial = Some(Dial { face: hands::face(pos, yaw, d.centre, d.right, d.up, r, r), n: qrot(hands::yaw_quat(yaw), d.front), radius: d.radius });
    }
    apply(&mut l.lights);
}
pub(super) fn close(l: &mut Lobby) {
    if let Some(h) = l.lights.held.take() {
        l.hands.release(h.hand, WHO);
    }
    (l.lights.dial, l.lights.applied) = (None, None);
}
pub(super) fn update(l: &mut Lobby, _dt: f64) {
    if l.lights.dial.is_none() {
        return;
    }
    if l.lights.held.is_some() {
        turn(l);
    } else {
        for hand in 1..=2 {
            if l.lights.held.is_none() && l.hands.taking(hand) {
                take(l, hand);
            }
        }
        let pressed = l.lights.dial.as_mut().and_then(|d| {
            let hits = hands::pointer(&d.face);
            hands::pressed(&mut d.face, hits.as_deref())
        });
        if let Some(h) = pressed {
            step(&mut l.lights, h);
        }
    }
    if l.lights.applied != l.lights.level {
        apply(&mut l.lights);
    }
}
