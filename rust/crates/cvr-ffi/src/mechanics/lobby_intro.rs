//! modules/intro.lua: the lobby's intro, once per launch -- a screen spawned before the eyes (vr_lobby.ent) takes the
//! boot's notice over (VRIntroTakeBootNotice), then plays the game's logo video in the language's cut and flies away
//! and up into the dark while the lobby opens behind it (the curtain's closure), the hum, the whoosh and the glitch on
//! cue (VRLobbyIntro draws it into the frame at its distance, height and opacity).
use super::belt::qrot;
use super::lobby::Lobby;
use super::lobby_ink as ink;
use super::lobby_spawn::{self as sp, add, native, Surface, V3, Q};
use crate::script_call::Value;
use red4ext_abi::handle::Handle;

const ENT: &str = "vrport\\ui\\vr_lobby.ent";
const UI: &str = "vrp_lobby_ui";
const TAG: &str = "VRLobbyIntro";
const CW: f64 = 2580.0;
const CH: f64 = 1980.0;
const AHEAD: f64 = 1.5;
const NOTICE_OUT: f64 = 0.8;
const FROM: f64 = 3.4;
const TO: f64 = 13.0;
const HEIGHT_FROM: f64 = 2.3;
const HEIGHT_TO: f64 = 5.0;
const TIME: f64 = 6.5;
const OPEN_AFTER: f64 = 0.8;
const OPEN: f64 = 3.0;
const FADE_OUT: f64 = 1.4;
const BLACK_POINT: f64 = 0.06;
const GLITCH_AT: f64 = 0.15;
const GLITCH_END: f64 = 1.15;

pub(super) struct Intro {
    played: bool,
    phase: Option<&'static str>,
    clock: f64,
    closure: f64,
    s: Surface,
    video: Handle,
    logo_path: String,
    glitched: bool,
}
impl Default for Intro {
    fn default() -> Intro {
        Intro { played: false, phase: None, clock: 0.0, closure: 1.0, s: Surface::default(), video: Handle::null(), logo_path: String::new(), glitched: false }
    }
}
fn video_h() -> f64 {
    (CW * 9.0 / 16.0).floor()
}
fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
fn play(i: &Intro, path: &str) {
    let v = &i.video;
    ink::call(v, "Stop", &[]);
    ink::call(v, "SetVideoPath", &[Value::U64(red4ext_abi::resource::resource_path(path))]);
    ink::call(v, "Play", &[]);
    ink::visible(v, false);
    ink::visible(v, true);
}
fn build(i: &mut Intro) {
    let layer = ink::canvas(&i.s.root, 0.0, 0.0, CW, CH);
    ink::rect(&layer, 0.0, 0.0, 4.0, 4.0, [0.0; 3], 1.0);
    let vh = video_h();
    i.video = ink::place(ink::new("inkVideoWidget"), &layer, 0.0, ((CH - vh) / 2.0).floor(), CW, vh);
    ink::call(&i.video, "SetLoop", &[Value::Bool(false)]);
    let code = match ink::language().as_str() {
        "kr-kr" => "kr",
        "zh-cn" => "zhs",
        "zh-tw" => "zht",
        _ => "en",
    };
    i.logo_path = format!("base\\movies\\fullscreen\\logo_splashscreen\\cp_logo_intro_{code}.bk2");
}
fn show(i: &Intro, distance: f64, height: f64, opacity: f64, black: f64) -> bool {
    if i.s.comp.is_null() {
        return false;
    }
    native::<bool>("VRLobbyIntro", &[Value::Handle(&i.s.comp), Value::Float(distance as f32), Value::Float(height as f32), Value::Float(opacity as f32), Value::Float(black as f32)]) == Some(true)
}
fn finish(l: &mut Lobby) {
    l.sound.hum(false);
    l.sound.glitch(false);
    let null = Handle::null();
    native::<bool>("VRLobbyIntro", &[Value::Handle(&null), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0)]);
    sp::delete(l.intro.s.id);
    l.intro.s = Surface::default();
    (l.intro.phase, l.intro.closure) = (Some("done"), 0.0);
}
fn enter(i: &mut Intro, phase: &'static str) {
    (i.phase, i.clock) = (Some(phase), 0.0);
}
/// view: the eyes in the world (the plugin's view, else the active camera).
fn view() -> Option<(V3, Q)> {
    if let Some(v) = sp::view_world() {
        return Some(v);
    }
    let cs = super::game_system("GetCameraSystem");
    let mut t = [[0f32; 4]; 2];
    ink::call(&cs, "GetActiveCameraWorldTransform", &[Value::Out(t.as_mut_ptr().cast())]);
    Some(([f64::from(t[0][0]), f64::from(t[0][1]), f64::from(t[0][2])], t[1].map(f64::from)))
}
fn before_eyes() -> Option<(V3, Q)> {
    let (p, q) = view()?;
    Some((add(p, qrot(q, [0.0, AHEAD, 0.0])), q))
}
/// M.begin: the intro, the first time the lobby is ready.
pub(super) fn begin(l: &mut Lobby) {
    if l.intro.played {
        return;
    }
    l.intro.played = true;
    let id = before_eyes().and_then(|(p, q)| sp::spawn(ENT, p, q, TAG));
    if id.is_none() {
        finish(l);
        return;
    }
    l.intro.s = Surface { id, ..Default::default() };
    l.intro.closure = 1.0;
    enter(&mut l.intro, "spawn");
}
pub(super) fn update(l: &mut Lobby, dt: f64) {
    if matches!(l.intro.phase, None | Some("done")) {
        return;
    }
    l.intro.clock += dt;
    if !l.intro.s.entity.is_null() {
        if let Some((p, q)) = before_eyes() {
            sp::set_world(&l.intro.s.entity, p, q);
        }
    }
    match l.intro.phase {
        Some("spawn") => {
            if l.intro.s.resolve(UI) {
                build(&mut l.intro);
                enter(&mut l.intro, "ready");
            } else if l.intro.clock > 10.0 {
                finish(l);
            }
        }
        Some("ready") => {
            if l.intro.s.painted() && show(&l.intro, FROM, HEIGHT_FROM, 0.0, 0.0) {
                let took = native::<bool>("VRIntroTakeBootNotice", &[Value::Float(NOTICE_OUT as f32)]) == Some(true);
                l.sound.hum(true);
                enter(&mut l.intro, if took { "notice" } else { "logo" });
            } else if l.intro.clock > 10.0 {
                finish(l);
            }
        }
        Some("notice") => {
            if l.intro.clock >= NOTICE_OUT {
                enter(&mut l.intro, "logo");
            }
        }
        Some("logo") => {
            let path = l.intro.logo_path.clone();
            play(&l.intro, &path);
            l.sound.logo();
            l.intro.glitched = false;
            enter(&mut l.intro, "flight");
        }
        Some("flight") => {
            let c = l.intro.clock;
            let k = smooth(c / TIME);
            let distance = FROM + (TO - FROM) * k;
            let height = HEIGHT_FROM + (HEIGHT_TO - HEIGHT_FROM) * k;
            l.intro.closure = 1.0 - smooth((c - OPEN_AFTER) / OPEN);
            let a = 1.0 - smooth((c - TIME) / FADE_OUT);
            show(&l.intro, distance, height, a, BLACK_POINT);
            if !l.intro.glitched && c >= GLITCH_AT {
                l.intro.glitched = true;
                l.sound.glitch(true);
            } else if l.intro.glitched && c >= GLITCH_END {
                l.sound.glitch(false);
            }
            if c >= TIME + FADE_OUT {
                finish(l);
            }
        }
        _ => {}
    }
}
pub(super) fn running(l: &Lobby) -> bool {
    !matches!(l.intro.phase, None | Some("done"))
}
pub(super) fn closure(l: &Lobby) -> f64 {
    l.intro.closure
}
pub(super) fn stop(l: &mut Lobby) {
    if running(l) {
        finish(l);
    }
}
