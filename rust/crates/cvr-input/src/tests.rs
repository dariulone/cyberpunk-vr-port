//! The groups of tools/analog_input_tests/main.cpp, case for case.
extern crate std;

use super::keypad::{self, Cursor, Point};
use super::stick::*;
use super::trigger::WeaponTrigger;

const NAN: f32 = f32::NAN;
const INF: f32 = f32::INFINITY;

fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

#[test]
fn curve() {
    for sign in [-1.0f32, 1.0] {
        assert_eq!(analog_axis(sign * 0.14, 0.15, 0.9), 0.0, "centre drift escapes deadzone");
        assert_eq!(analog_axis(sign * 0.15, 0.15, 0.9), 0.0, "deadzone boundary moves");
        assert!((analog_axis(sign * 0.525, 0.15, 0.9) - sign * 0.5).abs() < 1e-6, "half usable travel");
        assert_eq!(analog_axis(sign * 0.9, 0.15, 0.9), sign, "outer threshold does not give full input");
        assert_eq!(analog_axis(sign * 1.1, 0.15, 0.9), sign, "overshoot exceeds full input");
    }
}

#[test]
fn ranges() {
    for dead in [0.0f32, 0.05, 0.15, 0.30] {
        for full in [0.8f32, 0.9, 1.0] {
            let mut previous = -1.0f32;
            for i in -1000..=1000 {
                let raw = i as f32 * 0.001;
                let out = analog_axis(raw, dead, full);
                assert!(out.is_finite() && out.abs() <= 1.0, "analog output outside range");
                assert!(out >= previous, "stick speed is not monotonic");
                previous = out;
                assert!((out + analog_axis(-raw, dead, full)).abs() < 1e-6, "asymmetric response");
            }
        }
    }
}

#[test]
fn gestures() {
    for dead in [0.0f32, 0.15, 0.30] {
        for full in [0.8f32, 0.9, 1.0] {
            assert_eq!(analog_axis(full, dead, full), 1.0, "full travel mapping lost");
            assert!(!at_full_travel(full - 0.001, true, full, false), "partial travel activates sprint/dash");
            assert!(at_full_travel(full, true, full, false), "full raw threshold missed");
            assert!(at_full_travel(-full, true, full, true), "crouch threshold differs from forward");
            assert!(!at_full_travel(0.0, true, full, false), "consumed scanner axis activates sprint");
        }
    }
    assert!(!at_full_travel(0.9, false, 0.8, false) && at_full_travel(0.901, false, 1.0, false));
}

#[test]
fn invalid() {
    for value in [NAN, INF, -INF] {
        assert_eq!(analog_axis(value, 0.15, 0.9), 0.0, "invalid axis is not neutral");
        assert!(!at_full_travel(value, true, 0.9, false), "invalid axis activates a gesture");
        assert!(deadzone(value) == 0.15 && full_input(value) == 0.9, "invalid settings lack safe defaults");
    }
    assert!(deadzone(-0.1) == 0.15 && deadzone(0.31) == 0.15, "deadzone bounds not validated");
    assert!(full_input(0.79) == 0.9 && full_input(1.01) == 0.9, "full threshold bounds not validated");
}

#[test]
fn default_mode() {
    assert!(!use_analog_movement(0, false), "fixed default replaced with analog");
    assert!(use_analog_movement(1, false), "analog switch has no effect on foot");
    assert!(!use_analog_movement(1, true), "analog tuning changes driving");
    assert!(!use_analog_movement(-1, false) && !use_analog_movement(2, false), "unknown movement mode accepted");
}

#[test]
fn keypad_curve() {
    let mut previous = 0.0f32;
    for i in 0..=1000 {
        let raw = i as f32 / 1000.0;
        let value = keypad::cursor_axis(raw, 0.15);
        assert!(value >= previous && value <= 0.5, "keypad cursor is not bounded and proportional");
        assert_eq!(keypad::cursor_axis(-raw, 0.15), -value, "keypad cursor direction is asymmetric");
        if raw <= 0.15 {
            assert_eq!(value, 0.0, "keypad centre drifts");
        }
        previous = value;
    }
    let partial = keypad::cursor_axis(0.575, 0.15);
    assert!(partial > 0.249 && partial < 0.251, "partial keypad push became full movement");
    assert_eq!(keypad::cursor_axis(NAN, 0.15), 0.0, "invalid keypad input moves cursor");
    assert!(!keypad::input_fresh(0, 1000) && keypad::input_fresh(1000, 1350) && !keypad::input_fresh(1000, 1351));
    assert!(!keypad::input_fresh(1000, 999), "clock rollback keeps keypad input claimed");
}

#[test]
fn keypad_cursor() {
    let size = p(600.0, 1066.0);
    let centre = p(300.0, 533.0);
    for fps in [30, 45, 60, 90, 120, 144] {
        let mut cursor = Cursor::default();
        assert!(!cursor.update(false, p(0.0, 0.0), p(0.0, 0.0), size, 0.02), "panel without a hit was claimed");
        assert!(cursor.update(true, centre, p(0.0, 0.0), size, 0.0), "initial head hit failed");
        for _ in 0..fps {
            cursor.update(false, p(0.0, 0.0), p(0.1, 0.0), size, 1.0 / fps as f32);
        }
        assert!((cursor.position.x - 390.0).abs() < 0.02, "stick speed depends on FPS or gaze");
        let held = cursor.position;
        cursor.update(false, p(0.0, 0.0), p(0.0, 0.0), size, 0.03);
        assert_eq!(cursor.position.x, held.x, "cursor moves after releasing the stick");
        cursor.update(true, p(50.0, 50.0), p(0.0, 0.0), size, 0.02);
        assert_eq!(cursor.position.x, held.x, "returning gaze teleports the held cursor");
        cursor.update(true, p(55.0, 57.0), p(0.0, 0.0), size, 0.02);
        assert!((cursor.position.x - held.x - 5.0).abs() < 0.001 && (cursor.position.y - held.y - 7.0).abs() < 0.001);
    }
    for axis in [-0.5f32, 0.5] {
        let mut cursor = Cursor::default();
        cursor.update(true, centre, p(0.0, 0.0), size, 0.0);
        for _ in 0..1000 {
            cursor.update(false, p(0.0, 0.0), p(axis, axis), size, 0.02);
        }
        let pos = cursor.position;
        assert!(pos.x >= 1.0 && pos.x <= 599.0 && pos.y >= 1.0 && pos.y <= 1065.0, "cursor escaped panel");
        let edge = cursor.position;
        cursor.update(false, p(0.0, 0.0), p(-axis, -axis), size, 0.02);
        assert!((cursor.position.x - edge.x).abs() > 8.9, "edge overtravel delays reversal");
    }
    let mut cursor = Cursor::default();
    cursor.update(true, centre, p(0.0, 0.0), size, 0.0);
    cursor.update(false, p(0.0, 0.0), p(0.5, 0.0), size, 10.0);
    assert!((cursor.position.x - 322.5).abs() < 0.001, "hitch jumped across keypad");
    let before = cursor.position;
    cursor.update(true, p(NAN, NAN), p(NAN, NAN), size, NAN);
    assert_eq!(cursor.position, before, "invalid pose/input moved cursor");
    assert!(!cursor.update(true, centre, p(0.0, 0.0), p(0.0, 1066.0), 0.01) && !cursor.initialized);
    cursor.update(true, p(75.0, 80.0), p(0.0, 0.0), size, 0.0);
    assert_eq!(cursor.position, p(75.0, 80.0), "next device inherits old cursor");
}

#[test]
fn weapon_trigger() {
    let mut t = WeaponTrigger::default();
    assert!(!t.update(true, 0.0), "rest fired");
    for v in [0.2f32, 0.5, 0.849] {
        assert!(!t.update(true, v), "early break");
    }
    assert!(t.update(true, 0.85), "break missed");
    for _ in 0..100 {
        assert!(t.update(true, 1.0), "automatic hold interrupted");
    }
    for v in [0.84f32, 0.5, 0.201, 0.9] {
        assert!(t.update(true, v), "hold reset early");
    }
    assert!(!t.update(true, 0.20), "reset missed");
    for _ in 0..100 {
        assert!(t.update(true, 0.85), "rapid re-press missed");
        assert!(!t.update(true, 0.2), "rapid reset missed");
    }
    assert!(!t.update(false, 1.0), "inactive fired");
    assert!(!t.update(true, 1.0) && !t.update(true, 0.3), "reentry with held trigger fired");
    assert!(!t.update(true, 0.2) && t.update(true, 0.85), "reentry did not rearm");
    assert!(!t.update(true, NAN), "invalid input fired");
    assert!(!t.update(true, 1.0), "tracking recovery auto-fired");
}

#[test]
fn mirrored_part_follows_the_bindings() {
    use crate::handedness::mirrored_part;
    assert_eq!(mirrored_part("left_trigger", 0), "right_trigger");
    assert_eq!(mirrored_part("left_thumbstick", 1), "left_thumbstick");
    assert_eq!(mirrored_part("left_thumbstick", 2), "right_thumbstick");
    assert_eq!(mirrored_part("left_shoulder", 2), "left_shoulder");
    assert_eq!(mirrored_part("ls_click", 2), "rs_click");
    assert_eq!(mirrored_part("a", 0), "a");
    assert_eq!(mirrored_part("a", 1), "x");
    assert_eq!(mirrored_part("y_hold", 1), "b_hold");
    assert_eq!(mirrored_part("b_disabled", 2), "y_disabled");
    assert_eq!(mirrored_part("y_wide", 2), "y_wide");
    assert_eq!(mirrored_part("menu", 2), "menu");
}

// ---- main's tools/vrik_tests/weapon_controls_test.cpp, analog_input_tests/dual_wield_rapid_test.cpp and
// scanner_input_tests/main.cpp (the input state machines), case for case.

#[test]
fn gameplay_secondary_follows_the_bindings() {
    use crate::handedness::{mirrored_buttons, trades_buttons, trades_sticks};
    use crate::secondary::GameplaySecondary;
    for bindings in 0..3 {
        let mut secondary = GameplaySecondary::default();
        let mirror = trades_buttons(bindings);
        let source = if mirror { 0 } else { 1 };
        let mut press = |secondary: &mut GameplaySecondary, eligible: bool, buttons: u16| {
            let logical = mirrored_buttons(buttons, mirror, trades_sticks(bindings));
            secondary.update(eligible, logical & 0x2000 != 0, source)
        };
        let (b, other) = if mirror { (0x8000u16, 0x2000u16) } else { (0x2000, 0x8000) };
        assert!(!press(&mut secondary, true, b), "equip while held");
        assert!(!press(&mut secondary, true, other) && press(&mut secondary, true, b), "mirrored magazine button");
        assert!(!press(&mut secondary, false, b) && !press(&mut secondary, true, b), "held overlay exit leaked to gameplay");
        assert!(!press(&mut secondary, true, 0) && press(&mut secondary, true, b), "new press after close");
        assert!(!secondary.update(true, true, 1 - source), "held button survived binding swap");
    }
}

#[test]
fn finger_pulls_press_once_per_pull() {
    use crate::trigger::{FingerAction, FingerButton, FingerPull};
    let mut finger = FingerPull::default();
    let mut button = FingerButton::default();
    let mut poll = |finger: &mut FingerPull, button: &mut FingerButton, value: f32, weapon: u64| {
        let pull = finger.sample(true, value);
        button.poll(true, weapon, pull)
    };
    assert!(!poll(&mut finger, &mut button, 0.0, 1), "initial release");
    for _ in 0..12 {
        assert!(poll(&mut finger, &mut button, 0.85, 1) && poll(&mut finger, &mut button, 0.3, 1), "break or hold lost");
        assert!(!poll(&mut finger, &mut button, 0.2, 1), "reset lost");
    }
    assert!(poll(&mut finger, &mut button, 1.0, 1), "press before asynchronous reset");
    finger.sample(true, 0.2);
    finger.sample(true, 0.85);
    assert!(!poll(&mut finger, &mut button, 0.5, 1) && poll(&mut finger, &mut button, 0.5, 1), "XR reset and re-press coalesced by game polls");
    finger.sample(true, 0.0);
    finger.sample(true, 1.0);
    assert!(!poll(&mut finger, &mut button, 1.0, 1) && !poll(&mut finger, &mut button, 0.0, 1) && !poll(&mut finger, &mut button, 0.0, 1),
        "replayed a pull already released");
    assert!(poll(&mut finger, &mut button, 1.0, 1) && !poll(&mut finger, &mut button, 1.0, 2) && !poll(&mut finger, &mut button, 1.0, 2),
        "equip inherited a held trigger");
    assert!(!poll(&mut finger, &mut button, 0.0, 2) && poll(&mut finger, &mut button, 1.0, 2), "new pull after equip");
    let paused = finger.sample(false, 1.0);
    assert!(!button.poll(false, 2, paused), "pause leaked to gameplay");
    assert!(!poll(&mut finger, &mut button, 1.0, 2) && !poll(&mut finger, &mut button, 1.0, 2), "resume while held");
    assert!(!poll(&mut finger, &mut button, 0.0, 2) && poll(&mut finger, &mut button, 1.0, 2), "new pull after resume");
    for _ in 0..100 {
        assert!(poll(&mut finger, &mut button, 0.3, 2), "continuous hold interrupted");
    }

    let mut action = FingerAction::default();
    assert!(action.sample(4, false, 10) == 0 && action.sample(4, true, 11) == 0, "blocked pull inherited on resume");
    assert!(action.sample(0, false, 12) == 0 && action.sample(5, true, 13) == 5, "first fresh action after resume");
    assert!(action.sample(5, true, 113) == 5 && action.sample(5, true, 114) == 0, "expired action became fresh");
    assert!(action.sample(6, true, 115) == 6 && action.sample(6, false, 116) == 0 && action.sample(6, true, 117) == 0,
        "reload gate repeated an interrupted action");
    assert!(action.sample(0, true, 118) == 0, "released action queued");
}

#[test]
fn dual_wield_rapid_cadence() {
    use crate::dual_wield::DualWieldRapidFire;
    let mut g = DualWieldRapidFire::default();
    assert!(!g.update(true, true, false, 100), "one trigger fired hurricane");
    g.update(true, false, false, 120);
    assert!(!g.update(true, false, true, 250), "one pair fired hurricane");
    g.update(true, false, false, 270);
    assert!(!g.update(true, true, false, 400), "missing second right press accepted");
    g.update(true, false, false, 420);
    assert!(g.update(true, false, true, 550), "rapid alternating pulls missed");
    assert!(g.update(true, false, true, 749), "request disappeared before state machine can consume it");
    assert!(!g.update(true, true, true, 750), "held request never expired");
    for t in 751..1800 {
        assert!(!g.update(true, true, true, t), "held automatic triggers repeated hurricane");
    }
    g = DualWieldRapidFire::default();
    let mut t = 0;
    while t < 1000 {
        assert!(!g.update(true, true, false, t), "one fast hand triggered hurricane");
        g.update(true, false, false, t + 30);
        t += 100;
    }
    g = DualWieldRapidFire::default();
    assert!(!g.update(true, true, true, 100), "simultaneous first shot triggered hurricane");
    g.update(true, false, false, 150);
    assert!(g.update(true, true, true, 400), "rapid paired pulls missed");
    assert!(!g.update(false, true, true, 410), "menu did not cancel request");
    assert!(!g.update(true, true, true, 420), "menu exit replayed gesture");
    g = DualWieldRapidFire::default();
    g.update(true, true, true, 100);
    g.update(true, false, false, 150);
    assert!(!g.update(true, true, true, 701), "slow ordinary fire triggered hurricane");
    g.update(true, false, false, 720);
    assert!(g.update(true, true, true, 900), "fresh rapid pair after stale input missed");
}

#[test]
fn scanner_click_owns_its_press_and_release() {
    use crate::scanner::{DeferredLeftStickClick, ScannerClick, ScannerEarZone, ScannerRequest};
    let zone = ScannerEarZone::default();
    assert!(zone.contains(-0.25, -0.10, 0.15, false));
    assert!(zone.contains(-0.20, -0.18, 0.12, false));
    assert!(zone.contains(-0.20, 0.0, -0.05, false));
    assert!(!zone.contains(-0.20, -0.30, -0.40, false)); // ordinary hand rest / weapon position
    assert!(!zone.contains(0.10, -0.03, 0.06, false)); // other side of the head
    let mut click = ScannerClick::default();
    let mut deferred = DeferredLeftStickClick::default();
    let mut sprint_clicks = 0u32;
    let mut tick = |click: &mut ScannerClick, deferred: &mut DeferredLeftStickClick, sprint: &mut u32, down: bool, at_ear: bool, enabled: bool,
                    valid: bool, dt: f64, dpad: bool| {
        let claim = click.update(enabled, down, valid, if at_ear { -0.18 } else { 0.0 }, -0.08, 0.08, dt, &zone, 80, 250);
        if deferred.update(down, claim || dpad) {
            *sprint += 1;
        }
        claim
    };
    macro_rules! t {
        ($down:expr) => { tick(&mut click, &mut deferred, &mut sprint_clicks, $down, true, true, true, 20.0, false) };
        ($down:expr, $ear:expr) => { tick(&mut click, &mut deferred, &mut sprint_clicks, $down, $ear, true, true, 20.0, false) };
        ($down:expr, $ear:expr, $en:expr) => { tick(&mut click, &mut deferred, &mut sprint_clicks, $down, $ear, $en, true, 20.0, false) };
        ($down:expr, $ear:expr, $en:expr, $valid:expr) => { tick(&mut click, &mut deferred, &mut sprint_clicks, $down, $ear, $en, $valid, 20.0, false) };
        ($down:expr, $ear:expr, $en:expr, $valid:expr, $dt:expr, $dpad:expr) => {
            tick(&mut click, &mut deferred, &mut sprint_clicks, $down, $ear, $en, $valid, $dt, $dpad)
        };
    }
    for _ in 0..6 {
        assert!(!t!(true)); // held from startup, even once the zone settles
    }
    assert!(click.presses() == 0 && !click.held());
    assert!(!t!(false)); // release the unclaimed ordinary L3
    assert_eq!(sprint_clicks, 1);
    assert!(t!(true) && click.presses() == 1 && click.held()); // fresh at the ear, immediate
    for _ in 0..20 {
        assert!(t!(true));
    }
    assert_eq!(click.presses(), 1); // holding never toggles twice
    assert!(t!(false));
    assert!(!click.held());
    assert_eq!(sprint_clicks, 1); // the release consumed too
    assert!(t!(true) && click.presses() == 2); // release and re-arm permit closing
    for _ in 0..20 {
        assert!(t!(true, false)); // a consumed hold carried out of the zone
    }
    assert!(!click.held());
    assert!(t!(false, false));
    assert_eq!(sprint_clicks, 1);
    assert!(!t!(false, false));
    assert!(!t!(true, false));
    for _ in 0..10 {
        assert!(!t!(true, true)); // an ordinary L3 carried into the ear never scans
    }
    assert_eq!(click.presses(), 2);
    assert!(!t!(false));
    assert_eq!(sprint_clicks, 2);
    assert!(t!(true) && click.presses() == 3);
    assert!(t!(true, true, false));
    assert!(t!(false, true, false));
    assert_eq!(sprint_clicks, 2);
    // Pressing during a menu or a disabled state cannot turn into a scanner click after resuming.
    assert!(!t!(true, true, false));
    for _ in 0..10 {
        assert!(!t!(true));
    }
    assert_eq!(click.presses(), 3);
    assert!(!t!(false));
    let ordinary = sprint_clicks;
    assert!(t!(true));
    for _ in 0..20 {
        assert!(t!(true, true, true, false)); // invalid tracking cannot repeat
    }
    assert_eq!(click.presses(), 4);
    assert!(t!(false));
    assert_eq!(sprint_clicks, ordinary);
    assert!(!t!(false));
    // No press during the exit grace: the current pose, not just the armed latch, must be inside.
    assert!(!t!(true, false));
    assert_eq!(click.presses(), 4);
    assert!(!t!(false, false));
    // Ordinary D-pad use outside the ear still suppresses its deferred sprint.
    let before_dpad = sprint_clicks;
    assert!(!t!(true, false, true, true, 20.0, true));
    assert!(!t!(false, false));
    assert_eq!(sprint_clicks, before_dpad);
    // Settling after an early press must not manufacture a fresh edge.
    let mut early = ScannerClick::default();
    let mut e = |early: &mut ScannerClick, down: bool, dt: f64| early.update(true, down, true, -0.18, -0.08, 0.08, dt, &zone, 80, 250);
    assert!(!e(&mut early, false, 20.0));
    assert!(!e(&mut early, true, 20.0));
    assert!(!e(&mut early, true, 100.0));
    assert_eq!(early.presses(), 0);
    e(&mut early, false, 20.0);
    assert!(e(&mut early, true, 20.0));
    assert!(e(&mut early, false, 20.0));
    assert!(!e(&mut early, true, 1000.0)); // resume while held: not a fresh edge
    assert!(!e(&mut early, true, 100.0));
    assert_eq!(early.presses(), 1);
    e(&mut early, false, 20.0);
    assert!(e(&mut early, true, 20.0));
    assert_eq!(early.presses(), 2);
    assert!(!zone.contains(f32::NAN, 0.0, 0.0, false));
    let mut request = ScannerRequest::default();
    let mut held = true;
    request.update(&mut held, 0, 1000);
    assert!(held);
    request.update(&mut held, 0, 1749);
    assert!(held);
    request.update(&mut held, 0, 1750);
    assert!(!held); // a rejected opening releases LB
    held = true;
    request.update(&mut held, 0, 2000);
    request.update(&mut held, 1, 2040);
    request.update(&mut held, 1, 10000);
    assert!(held); // an accepted hold can last indefinitely
    request.update(&mut held, 0, 10020);
    assert!(!held); // a game-driven exit releases the latch
}

#[test]
fn scanner_hand_physics_generations() {
    use crate::scanner::ScannerHandPhysics;
    let p = ScannerHandPhysics::new();
    let idle = p.epoch();
    assert!(!p.active() && p.accepts(idle));
    p.publish(1);
    assert!(p.active() && p.scanner_active() && !p.accepts(p.epoch()));
    p.time_scale(0.5);
    p.publish(0);
    assert!(p.active() && !p.scanner_active(), "the time scale still holds the hands");
    p.time_scale(1.0);
    let resumed = p.epoch();
    assert!(!p.active() && p.accepts(resumed) && !p.accepts(idle), "work from before the slowdown is stale");
    p.publish(-1);
    assert_eq!(p.epoch(), resumed, "no sample changes nothing");
}

#[test]
fn stick_pulse_taps_once_per_push() {
    let mut pulse = StickPulse::default();
    assert!(pulse.update(true, 1.0, 1000, 100, true));
    assert!(pulse.update(true, 1.0, 1099, 100, true));
    assert!(!pulse.update(true, 1.0, 1100, 100, true), "a held stick is one tap");
    assert!(!pulse.update(false, 0.7, 1200, 100, true) && !pulse.update(true, 1.0, 1210, 100, true), "re-armed above half travel");
    assert!(!pulse.update(false, 0.4, 1300, 100, true) && pulse.update(true, 1.0, 1310, 100, true), "re-armed under half travel");
    let mut blocked = StickPulse::default();
    assert!(!blocked.update(true, 1.0, 0, 100, false), "a blocked context presses");
    assert!(!blocked.update(true, 1.0, 50, 100, true), "leaving the context pressed by itself");
    assert!(!blocked.update(false, 0.0, 60, 100, true) && blocked.update(true, 1.0, 70, 100, true));
}

#[test]
fn grenade_stick_claims_its_hand() {
    use crate::grenade_stick::GrenadeStick;
    let mut g = GrenadeStick::default();
    assert_eq!(g.update(0, true, true, false), 0);
    assert_eq!(g.update(0, true, true, false), 0, "held stays claimed");
    assert_eq!(g.update(0, true, false, false), -1, "released");
    assert_eq!(g.update(1, true, false, true), 1);
    assert_eq!(g.update(1, true, false, false), -1);
    assert_eq!(g.update(1, true, true, true), -1, "both clicks are the overlay's");
    assert_eq!(g.update(1, true, false, false), -1);
    assert_eq!(g.update(0, false, true, false), -1, "not eligible");
    assert_eq!(g.update(0, true, true, false), -1, "a held click never becomes the grenade's");
}

// ---- the D-pad's and the menu swipe's modifier (2026-11-24): the thumbrest held past 0.2 s on a Touch controller, the
// left stick held in elsewhere.
#[test]
fn modifier_by_thumbrest_or_stick() {
    use crate::modifier::*;
    // a controller that never reported a thumbrest: the left stick's click is the modifier
    publish([false, false], true, 1_000);
    assert!(!thumbrest_controller());
    assert!(held(1_000));
    publish([false, false], false, 1_010);
    assert!(!held(1_010));
    // a thumbrest touched: the thumbrest scheme from now on, held only past 0.2 s
    publish([true, false], false, 2_000);
    assert!(thumbrest_controller());
    assert!(!held(2_100));
    publish([true, false], false, 2_150);
    assert!(!held(2_199));
    assert!(held(2_200));
    // the click no longer counts, the release ends the hold at once
    publish([false, false], true, 2_300);
    assert!(!held(2_300));
    // the right thumbrest alone is no modifier
    publish([false, true], false, 3_000);
    assert!(!held(3_500));
    // a left-hander's traded sticks: the right thumbrest is, the left one is not
    publish_side([false, true], 1, false, 4_000);
    assert!(held(4_200));
    publish_side([true, false], 1, false, 4_300);
    assert!(!held(4_600));
    // back to the left: a touch held on the right before does not count
    publish_side([true, true], 0, false, 5_000);
    assert!(!held(5_100));
    assert!(held(5_200));
    publish([false, false], false, 5_300);
}

// ---- the combat gadget's chord (2026-11-24): the grip held and the stick pressed in, half a second, RB until released.
#[test]
fn gadget_chord_holds_rb() {
    use crate::gadget_hold::*;
    let mut g = GadgetHold::new();
    // the click with the grip held: the chord's at once, RB after half a second, until either lets go
    let held = |rb: bool| GadgetResult { claimed: true, rb, tap: false };
    assert_eq!(g.step(true, 1.0, true, 1_000), held(false));
    assert_eq!(g.step(true, 1.0, true, 1_499), held(false));
    assert_eq!(g.step(true, 1.0, true, 1_500), held(true));
    assert_eq!(g.step(true, 0.6, true, 2_000), held(true), "a grip easing a little holds");
    assert_eq!(g.step(false, 1.0, true, 2_100), GadgetResult::default(), "the click's release throws, and is no tap");
    // a short press with the grip held is the click it was: R3 for a moment after the release
    let mut t = GadgetHold::new();
    assert_eq!(t.step(true, 1.0, true, 10_000), held(false));
    assert_eq!(t.step(false, 1.0, true, 10_100), GadgetResult { claimed: false, rb: false, tap: true });
    assert_eq!(t.step(false, 1.0, true, 10_150), GadgetResult { claimed: false, rb: false, tap: true });
    assert_eq!(t.step(false, 1.0, true, 10_300), GadgetResult::default(), "the tap is over");
    // a click without the grip is the game's, and the grip coming later does not take it
    assert_eq!(g.step(true, 0.0, true, 3_000), GadgetResult::default());
    assert_eq!(g.step(true, 1.0, true, 3_600), GadgetResult::default());
    assert_eq!(g.step(false, 1.0, true, 3_700), GadgetResult::default());
    // the grip let go mid-hold: RB released, the click still the chord's (no stray quick melee) until released
    g.step(true, 1.0, true, 4_000);
    assert!(g.step(true, 1.0, true, 4_600).rb);
    assert_eq!(g.step(true, 0.0, true, 4_700), held(false));
    assert_eq!(g.step(true, 1.0, true, 5_400), held(false), "no new hold without a new click");
    assert_eq!(g.step(false, 0.0, true, 5_500), GadgetResult::default(), "the gadget was used: no tap");
    // not allowed (a menu): never claimed
    g.step(false, 0.0, true, 6_000);
    assert_eq!(g.step(true, 1.0, false, 6_100), GadgetResult::default());
}

mod prompts_tests {
    use crate::prompts::*;
    const FOOT: Place = Place { menu: false, mounted: false, click_sprint: false };
    const MENU: Place = Place { menu: true, mounted: false, click_sprint: false };
    const CAR: Place = Place { menu: false, mounted: true, click_sprint: false };
    #[test]
    fn the_dpad_is_a_sticks_push_and_rb_the_sticks_click() {
        assert_eq!(chord_part("dpad_up", "UseConsumable", false, FOOT), Some(("right_axis_up".into(), CHORD_DPAD)));
        assert_eq!(chord_part("dpad_down_hold", "", false, FOOT), Some(("right_axis_down".into(), CHORD_DPAD)));
        assert_eq!(chord_part("dpad_left_right", "", false, MENU), Some(("right_axis_leftright".into(), CHORD_DPAD)));
        assert_eq!(chord_part("dpad_up_down_hold", "", true, FOOT), Some(("left_axis_updown".into(), CHORD_DPAD)));
        assert_eq!(chord_part("right_shoulder_hold", "UseCombatGadget", false, FOOT), Some(("right_thumb".into(), CHORD_GADGET)));
        assert_eq!(chord_part("right_shoulder_hold", "UseCombatGadget", false, MENU), Some(("right_thumb".into(), CHORD_GADGET)));
        assert_eq!(chord_part("left_shoulder", "VisionHold", false, FOOT), None);
        assert_eq!(chord_part("a", "", false, FOOT), None);
    }
    #[test]
    fn the_shoulders_are_the_grips_in_a_menu() {
        assert_eq!(chord_part("right_shoulder", "next_menu", false, MENU), Some(("right_shoulder".into(), GRIP_RIGHT)));
        assert_eq!(chord_part("left_shoulder", "prior_menu", false, MENU), Some(("left_shoulder".into(), GRIP_LEFT)));
    }
    #[test]
    fn the_scanner_has_its_own_controls() {
        assert_eq!(chord_part("dpad_left", "QH_MoveLeft", false, FOOT), Some(("right_axis_left".into(), CHORD_DPAD)));
        assert_eq!(chord_part("dpad_up", "UseConsumable", false, FOOT), Some(("right_axis_up".into(), CHORD_DPAD)));
        assert_eq!(chord_part("dpad_up", "ZoomIn", false, FOOT), Some(("right_axis_up".into(), CHORD_ZOOM)));
        assert_eq!(chord_part("right_shoulder", "DescriptionChange", false, FOOT), Some(("right_thumb".into(), 0)));
        assert_eq!(chord_part("right_thumb", "TagButton", false, FOOT), Some(("right_trigger".into(), 0)));
    }
    #[test]
    fn the_moves_are_the_sticks() {
        assert_eq!(chord_part("b", "UI_FakeDodge", false, FOOT), Some(("right_axis_up".into(), 0)));
        assert_eq!(chord_part("right_thumb", "ToggleCrouch", false, FOOT), Some(("right_axis_down".into(), 0)));
        assert_eq!(chord_part("b_hold", "ToggleCrouch", false, FOOT), Some(("right_axis_down".into(), 0)));
        assert_eq!(chord_part("left_thumb", "ToggleSprint", false, FOOT), Some(("left_axis_up".into(), 0)));
        let click = Place { click_sprint: true, ..FOOT };
        assert_eq!(chord_part("left_thumb", "ToggleSprint", false, click), None);
        assert_eq!(chord_part("b", "Exit", false, CAR), None);
        assert_eq!(chord_part("b", "", false, FOOT), None);
    }
    #[test]
    fn a_car_has_its_own_controls() {
        assert_eq!(chord_part("right_shoulder_hold", "VehicleInsideWheel", false, CAR), Some(("right_axis_left".into(), CHORD_DPAD)));
        assert_eq!(chord_part("right_shoulder", "ShootPrimary", false, CAR), Some(("right_trigger".into(), 0)));
        // the phone's tabs: the D-pad's chord, in a car too (both of its D-pad's sides switch the one other tab)
        assert_eq!(chord_part("right_shoulder", "popup_next", false, CAR), Some(("right_axis_right".into(), CHORD_DPAD)));
        assert_eq!(chord_part("left_shoulder", "popup_prior", false, FOOT), Some(("right_axis_left".into(), CHORD_DPAD)));
        assert_eq!(chord_part("left_shoulder", "popup_prior", true, FOOT), Some(("left_axis_left".into(), CHORD_DPAD)));
        assert_eq!(chord_part("dpad_left_hold", "VehicleVisualCustomization", false, CAR), Some(("dpad_left_hold".into(), UNREACHABLE)));
        assert_eq!(chord_part("dpad_left_hold", "PocketRadio", false, FOOT), Some(("right_axis_left".into(), CHORD_DPAD)));
        assert_eq!(chord_part("dpad_up", "UseConsumable", false, CAR), Some(("right_axis_up".into(), CHORD_DPAD)));
    }
}

#[test]
fn the_left_click_held_in_a_car_is_the_games() {
    use crate::scanner::DeferredLeftStickClick;
    let mut d = DeferredLeftStickClick::default();
    let mut step = |down: bool, used: bool, car: bool, now: u64| (d.update(down, used), d.hold(car, now));
    // a short click is the deferred one
    assert_eq!(step(true, false, true, 1_000), (false, false));
    assert_eq!(step(true, false, true, 1_300), (false, false));
    assert_eq!(step(false, false, true, 1_350), (true, false));
    // held: L3 from 0.4 s, and no click on its release
    assert_eq!(step(true, false, true, 2_000), (false, false));
    assert_eq!(step(true, false, true, 2_400), (false, true));
    assert_eq!(step(true, false, true, 3_000), (false, true));
    assert_eq!(step(false, false, true, 3_100), (false, false));
    // the D-pad's chord takes it: never held
    assert_eq!(step(true, false, true, 4_000), (false, false));
    assert_eq!(step(true, true, true, 4_200), (false, false));
    assert_eq!(step(true, false, true, 4_800), (false, false));
    assert_eq!(step(false, false, true, 4_900), (false, false));
    // on foot it stays the deferred click
    assert_eq!(step(true, false, false, 5_000), (false, false));
    assert_eq!(step(true, false, false, 6_000), (false, false));
    assert_eq!(step(false, false, false, 6_100), (true, false));
}
