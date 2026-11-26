//! THE WORKER: a thread of its own turns each step's result into the drawn hand's offset, solves the fingers against
//! the lists, decides when the hands are on (vehicles, menus, ladders, water), reloads config/physhands.ini when it
//! changes, and keeps the statistics.
use super::body::{scene_owns_arms, shared_hand, AFTER_MOST_US, AFTER_US, BEFORE_MOST_US, BEFORE_US, LIVE, STEPS_SEEN, TOUCHED_STEPS, WORLD_STEPS};
use super::census::{menu_mode, player_valid};
use super::fingers::{self, BACK_PRESSED, FINGERS_OPENED, FINGERS_OUT, FINGER_CALLS, FINGER_CONTACTS, FINGER_SOLVES, FINGER_US, PALM_SHIFTED, PALM_WRAPS, SHOWN_BENDS};
use super::loose::{APART_KEPT, DROPS_MADE, FINGER_FAULTS, LOOSE_CAPPED, LOOSE_LET_THROUGH, LOOSE_LISTED, LOOSE_STACKED, PALM_REFUSED};
use super::math::{qconj, qmul, qnorm, qrot};
use super::near::{NEAR_CYCLES, NEAR_MOST_SHAPES};
use super::pose::{REACH_HELD, TARGETS};
use super::seq::Seq;
use super::settings::{self, Settings, SETTINGS};
use super::stats::*;
use super::tick_ms;
use super::types::{Offset, PoseFrame, Step, Wanted};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::*};
use cvr_host::log;
use cvr_physhands::handspace::{self, Motion, Rotation, Vector};
use cvr_settings::lc;
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};

pub static STEPS: Seq<Step> = Seq::new(unsafe { core::mem::zeroed() });
pub static OFFSETS: [Seq<Offset>; 2] = [Seq::new(unsafe { core::mem::zeroed() }), Seq::new(unsafe { core::mem::zeroed() })];
/// worker -> physics thread
pub static WANTED: Seq<Wanted> = Seq::new(Wanted { on: [false; 2], at: 0 });
/// the auto-reset event the physics thread sets after each fetch
static STEPPED: AtomicUsize = AtomicUsize::new(0);
static WORKER_UP: AtomicBool = AtomicBool::new(false);
/// The physics hands in a vehicle too (item 6 of 2026-11-23's list: the car-hands self-test, CVR_CAR_HANDS_TEST, turns it
/// on to measure them against the car's own body).
pub static IN_VEHICLE: AtomicBool = AtomicBool::new(false);
pub fn worker_up() -> bool {
    WORKER_UP.load(Acquire)
}
pub fn signal_stepped() {
    let e = STEPPED.load(Acquire);
    if e != 0 {
        // SAFETY: the worker's event, made once and never closed.
        unsafe { SetEvent(e as _) };
    }
}

extern "C" {
    static g_isInVehicle: bool;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    fn cvr_cpp_lobby_world_shown() -> bool;
    fn cvr_ladder_active() -> bool;
    fn cvr_swimming_active() -> bool;
}
const K_WEAPON_FLAG: usize = 144;

/// The drawn hand's offset through a filter whose cutoff rises with how fast the offset moves (a "1 euro" filter): still
/// against a wall it is calm, coming off it is quick.
#[derive(Clone, Copy, Default)]
struct Smoothed {
    p: [f32; 3],
    r: [f32; 3],
    on: bool,
}
fn smooth(m: &mut Smoothed, p: &[f32; 3], r: &[f32; 3], dt_s: f64, s: &Settings) {
    if !m.on {
        m.p = *p;
        m.r = *r;
        m.on = true;
        return;
    }
    let dt = dt_s.clamp(1e-4, 0.1) as f32;
    let (mut dp, mut dr) = (0.0f32, 0.0f32);
    for k in 0..3 {
        dp += (p[k] - m.p[k]) * (p[k] - m.p[k]);
        dr += (r[k] - m.r[k]) * (r[k] - m.r[k]);
    }
    let speed = dp.sqrt() / dt + 0.1 * dr.sqrt() / dt; // a radian counted as 10 cm
    let cutoff = s.smooth_hz + s.smooth_beta * speed;
    let alpha = 1.0 - (-2.0 * core::f32::consts::PI * cutoff * dt).exp();
    for k in 0..3 {
        m.p[k] += (p[k] - m.p[k]) * alpha;
        m.r[k] += (r[k] - m.r[k]) * alpha;
    }
}
fn v3(p: &[f32; 3]) -> Vector {
    Vector::new(p[0], p[1], p[2])
}
fn r4(q: &[f32; 4]) -> Rotation {
    Rotation::new(q[0], q[1], q[2], q[3])
}

/// The worker's own state (C++ Work()'s locals).
struct Work {
    s: Settings,
    seen_write: Option<std::time::SystemTime>,
    read_at: u64,
    logged_at: u64,
    off_most: [f32; 2],
    off_sum: [f32; 2],
    off_n: u64,
    was_on: [bool; 2],
    last_step_at: u64,
    last_scanner_epoch: u64,
    last_smooth_at: f64,
    smoothed: [Smoothed; 2],
    body_smoothed: [Smoothed; 2],
    held: [f32; 2],
    latch: [[f32; 3]; 2],
    held_goal: [f32; 2],
    stuck: [[f32; 3]; 2],
    stuck_turn: [[f32; 4]; 2],
    stuck_on: [bool; 2],
    stuck_turn_on: [bool; 2],
    /// the pressed hand's filter: the body's place and turn a step before, its speed filtered; the press as it was
    stuck_prev: [[f32; 3]; 2],
    stuck_prev_turn: [[f32; 4]; 2],
    stuck_vel: [[f32; 3]; 2],
    stuck_spin: [f32; 2],
    pressed_was: [bool; 2],
    /// what the drawn hand is off its body as it lets go of the world (eased out), and how much a gun's turn is the body's
    residual: [[f32; 3]; 2],
    residual_turn: [[f32; 4]; 2],
    contact_w: [f32; 2],
    latch_on: [bool; 2],
    last_shown: [[f32; 3]; 2],
    jitter_sum: [f32; 2],
    jitter_most: [f32; 2],
    last_body: [[f32; 3]; 2],
    body_jitter_sum: [f32; 2],
    last_sent: [[f32; 3]; 2],
    sent_jitter_sum: [f32; 2],
    last_shown_on: [bool; 2],
    previous_filter_frame: [PoseFrame; 2],
    previous_filter_riding: [bool; 2],
    jitter_n: [u32; 2],
}

fn ini_modified() -> Option<std::time::SystemTime> {
    std::fs::metadata(cvr_host::paths::file(cvr_host::paths::Folder::Config, "physhands.ini")).and_then(|m| m.modified()).ok()
}

impl Work {
    fn reset_side(&mut self, side: usize) {
        self.smoothed[side].on = false;
        self.body_smoothed[side].on = false;
        self.held[side] = 0.0;
        self.held_goal[side] = 0.0;
        self.contact_w[side] = 0.0;
        self.latch_on[side] = false;
        self.stuck_on[side] = false;
        self.stuck_turn_on[side] = false;
        self.pressed_was[side] = false;
        self.last_shown_on[side] = false;
        self.residual[side] = [0.0; 3];
        self.residual_turn[side] = [0.0, 0.0, 0.0, 1.0];
    }

    unsafe fn tick(&mut self) {
        let now = tick_ms();
        // the settings, when the file has changed
        if now - self.read_at > 1000 {
            self.read_at = now;
            let m = ini_modified();
            if m.is_some() && m != self.seen_write {
                self.seen_write = m;
                let mut fresh = Settings::default();
                settings::read_settings(&mut fresh);
                SETTINGS.write(&fresh);
                super::super::xinput::PHYSHANDS_TRACE.store(fresh.trace != 0, Relaxed);
                self.s = fresh;
                log!(
                    "[physhands] settings read: on {}, {:.2} kg, {:.0} N, {:.0} rad/s^2, deadband {:.3} m\n",
                    self.s.enabled,
                    self.s.mass,
                    self.s.force,
                    self.s.spin_accel,
                    self.s.deadband
                );
            }
        }
        let s = self.s;
        // when the hands are wanted: in the world (in a vehicle with the hands' collision there on), no menu, no ladder,
        // no water (and no weapon out, if so set)
        let weapon = shared_hand(K_WEAPON_FLAG).is_some_and(|v| v > 0.5);
        let scene = scene_owns_arms();
        let scanner = cvr_input::scanner::SCANNER_HAND_PHYSICS.active();
        let world = !scanner
            && s.enabled != 0
            && lc!(xrHandPhysics) != 0
            && lc!(xrHandCollision) != 0
            && menu_mode() == 0
            && (!core::ptr::read_volatile(&raw const g_isInVehicle) || lc!(xrHandCollisionVehicle) != 0 || IN_VEHICLE.load(Relaxed))
            && player_valid()
            && !cvr_cpp_lobby_world_shown()
            && !cvr_ladder_active()
            && !cvr_swimming_active()
            && (s.with_weapon != 0 || !weapon)
            && !scene;
        let mut w = Wanted::default();
        for side in 0..2 {
            w.on[side] = world && TARGETS[side].read().is_some_and(|t| now.wrapping_sub(t.at) < 150);
            if w.on[side] != self.was_on[side] {
                self.was_on[side] = w.on[side];
                log!(
                    "[physhands] {} hand {}\n",
                    if side == 1 { "right" } else { "left" },
                    if w.on[side] {
                        "wanted"
                    } else if scanner {
                        "not wanted: scanner or world slow motion (tracking stays live)"
                    } else if scene {
                        "not wanted: a scene plays the body (the cutscene tier, a grapple, a carried body, a finisher, a camera)"
                    } else {
                        "not wanted (vehicle, menu, ladder, water, no tracking, or a weapon with with_weapon=0)"
                    }
                );
            }
        }
        w.at = now;
        WANTED.write(&w);
        // the drawn hands: each body's place against where it was sent, in the player's model space, through the filter
        if let Some(st) = STEPS.read().filter(|st| now.wrapping_sub(st.at) < 200 && st.serial != self.last_step_at) {
            self.last_step_at = st.serial;
            if st.scanner_epoch != self.last_scanner_epoch {
                // A worker can miss the parked step. Do not blend a newly placed hand with a pre-scan contact.
                for side in 0..2 {
                    self.reset_side(side);
                }
                self.last_scanner_epoch = st.scanner_epoch;
            }
            let t = now_s();
            let dt_s = if self.last_smooth_at > 0.0 { t - self.last_smooth_at } else { 1.0 / 60.0 };
            self.last_smooth_at = t;
            let inv = qconj(&st.entity_rot);
            for side in 0..2 {
                let mut o = Offset { at: now, scanner_epoch: st.scanner_epoch, frame: st.frame, riding: st.riding[side], ..Default::default() };
                let before = self.previous_filter_frame[side];
                if before.entity.valid && st.frame.entity.valid && (before.entity.owner != st.frame.entity.owner || before.entity.origin != st.frame.entity.origin) {
                    self.smoothed[side].on = false;
                    self.body_smoothed[side].on = false;
                    self.latch_on[side] = false;
                    self.stuck_on[side] = false;
                    self.stuck_turn_on[side] = false;
                    self.pressed_was[side] = false;
                    self.last_shown_on[side] = false;
                    self.residual[side] = [0.0; 3];
                    self.residual_turn[side] = [0.0, 0.0, 0.0, 1.0];
                    self.contact_w[side] = 0.0;
                }
                let motion = if self.previous_filter_riding[side] && st.riding[side] { super::adjust::ride_motion(&before, &st.frame, 4.0) } else { Motion::default() };
                if motion.valid {
                    if self.body_smoothed[side].on {
                        self.body_smoothed[side].p = motion.point(v3(&self.body_smoothed[side].p)).to_array();
                    }
                    if self.latch_on[side] {
                        self.latch[side] = motion.point(v3(&self.latch[side])).to_array();
                    }
                    if self.stuck_on[side] {
                        self.stuck[side] = motion.point(v3(&self.stuck[side])).to_array();
                        self.stuck_prev[side] = motion.point(v3(&self.stuck_prev[side])).to_array();
                        self.stuck_vel[side] = motion.direction(v3(&self.stuck_vel[side])).to_array();
                        self.stuck_prev_turn[side] = motion.orientation(r4(&self.stuck_prev_turn[side])).to_array();
                    }
                    if self.stuck_turn_on[side] {
                        self.stuck_turn[side] = motion.orientation(r4(&self.stuck_turn[side])).to_array();
                    }
                    self.residual[side] = motion.direction(v3(&self.residual[side])).to_array();
                    self.residual_turn[side] = handspace::multiply(handspace::multiply(motion.turn, r4(&self.residual_turn[side])), handspace::conjugate(motion.turn)).to_array();
                    if self.last_shown_on[side] {
                        self.last_shown[side] = motion.point(v3(&self.last_shown[side])).to_array();
                        self.last_body[side] = motion.point(v3(&self.last_body[side])).to_array();
                        self.last_sent[side] = motion.point(v3(&self.last_sent[side])).to_array();
                    }
                }
                if self.smoothed[side].on && before.entity.valid && st.frame.entity.valid {
                    for which in 0..2 {
                        let v = if which == 0 { &mut self.smoothed[side].p } else { &mut self.smoothed[side].r };
                        let mut world = handspace::rotate(before.entity.rotation, v3(v));
                        if motion.valid {
                            world = motion.direction(world);
                        }
                        *v = handspace::rotate(handspace::conjugate(st.frame.entity.rotation), world).to_array();
                    }
                }
                self.previous_filter_frame[side] = st.frame;
                self.previous_filter_riding[side] = st.riding[side];
                o.on = st.valid[side] && LIVE[side].load(Acquire) && cvr_input::scanner::SCANNER_HAND_PHYSICS.accepts(st.scanner_epoch);
                if o.on {
                    self.offset_on(side, &st, &inv, &s, dt_s, &mut o);
                } else {
                    o.rot[3] = 1.0;
                    self.reset_side(side);
                }
                OFFSETS[side].write(&o);
            }
            self.off_n += 1;
            fingers::solve_fingers(&st, &s, dt_s);
        }
        super::trace::flush_trace(s.trace != 0);
        super::trace::flush_grip_trace(s.trace != 0);
        // THE STATISTICS EVERY log_every SECONDS: a tuning aid, written with trace=1 or the launcher's DEBUG box only.
        // The block runs either way: its counters are taken (and reset) in the calls' arguments.
        if now - self.logged_at > s.log_every.max(1) as u64 * 1000 {
            self.statistics(&s, s.trace != 0 || core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0);
            self.logged_at = now;
        }
    }

    unsafe fn offset_on(&mut self, side: usize, st: &Step, inv: &[f32; 4], s: &Settings, dt_s: f64, o: &mut Offset) {
        let dw = [st.pose[side][4] - st.sent[side][4], st.pose[side][5] - st.sent[side][5], st.pose[side][6] - st.sent[side][6]];
        let mut raw = qrot(inv, &dw);
        let len = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
        let keep = if len > s.deadband { (len - s.deadband) / len } else { 0.0 };
        for c in &mut raw {
            *c *= keep;
        }
        // the turn the body could not make, the same way round, as a rotation vector
        let sent_q = [st.sent[side][0], st.sent[side][1], st.sent[side][2], st.sent[side][3]];
        let pose_q = [st.pose[side][0], st.pose[side][1], st.pose[side][2], st.pose[side][3]];
        let qw = qmul(&pose_q, &qconj(&sent_q));
        let tmp = qmul(inv, &qw);
        let mut q = qmul(&tmp, &st.entity_rot);
        let mut turn = [0.0f32; 3];
        if qnorm(&mut q) {
            if q[3] < 0.0 {
                for c in &mut q {
                    *c = -*c;
                }
            }
            let sn = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
            let angle = 2.0 * sn.atan2(q[3]);
            let kept = (if angle > s.turn_deadband { angle - s.turn_deadband } else { 0.0 }).min(s.max_turn);
            if sn > 1e-7 {
                for k in 0..3 {
                    turn[k] = q[k] / sn * kept;
                }
            }
        }
        smooth(&mut self.smoothed[side], &raw, &turn, dt_s, s);
        o.pos = self.smoothed[side].p;
        // HELD OFF, THE DRAWN HAND IS THE BODY. Tracking plus an offset carries the controller's own tremor through
        // whatever the offset's filter does; the body against a wall has none. So past a centimetre or two of offset the
        // drawn hand is the body's place (filtered alone), tracking only where nothing stops it.
        let bw = [st.pose[side][4] + st.origin[0], st.pose[side][5] + st.origin[1], st.pose[side][6] + st.origin[2]];
        {
            smooth(&mut self.body_smoothed[side], &bw, &[0.0; 3], dt_s, s);
            // only a hand really on something is drawn at its body: in the air the body trails tracking by a step; on
            // while it touches or is held off more than a centimetre, off once it is back within a few millimetres
            if st.touching[side] || len > 0.012 {
                self.held_goal[side] = 1.0;
            } else if len < 0.006 {
                self.held_goal[side] = 0.0;
            }
            let goal = self.held_goal[side];
            self.held[side] += (goal - self.held[side]) * (1.0 - (-dt_s / 0.04).exp()) as f32;
            if self.held[side] < 0.01 {
                self.held[side] = 0.0;
            }
            o.held = self.held[side];
            // STILL AT THE CONTACT: the drawn place stays put while the body stirs within a few millimetres, and is
            // dragged along (by the excess) when the hand slides on the surface
            if o.held > 0.5 {
                if !self.latch_on[side] {
                    self.latch[side] = self.body_smoothed[side].p;
                    self.latch_on[side] = true;
                }
                let d = [bw[0] - self.latch[side][0], bw[1] - self.latch[side][1], bw[2] - self.latch[side][2]];
                let dn = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                if dn > s.stick_hold {
                    for k in 0..3 {
                        self.latch[side][k] += d[k] * (1.0 - s.stick_hold / dn);
                    }
                }
                o.body = self.latch[side];
            } else {
                self.latch_on[side] = false;
                o.body = self.body_smoothed[side].p;
            }
        }
        // THE HAND IS ITS BODY (drawn=1): where PhysX has it, moved on by tracking on the animation thread, held still
        // while it presses the world and stirs within a few millimetres
        o.body_mode = s.drawn != 0;
        if o.body_mode {
            o.body_rot = [st.pose[side][0], st.pose[side][1], st.pose[side][2], st.pose[side][3]];
            o.spin = st.spin[side];
            o.when = st.when;
            o.step_dt = st.dt;
            // PRESSED: on the world and refused more than a few millimetres of where it was sent, and pressed until
            // refused under 3 mm again
            let refused2: f32 = (0..3).map(|k| (st.pose[side][4 + k] - st.sent[side][4 + k]) * (st.pose[side][4 + k] - st.sent[side][4 + k])).sum();
            const PRESSED_FROM: f32 = 0.006;
            const PRESSED_TO: f32 = 0.003;
            let pressed = st.on_world[side] && (refused2 > PRESSED_FROM * PRESSED_FROM || (self.pressed_was[side] && refused2 > PRESSED_TO * PRESSED_TO));
            self.pressed_was[side] = pressed;
            o.pressed = pressed;
            if pressed {
                // from where it is drawn now, what a contact just let go of had left included
                if !self.stuck_on[side] {
                    for k in 0..3 {
                        self.stuck[side][k] = bw[k] + self.residual[side][k];
                    }
                    self.stuck_prev[side] = bw;
                    self.stuck_prev_turn[side] = pose_q;
                    self.stuck_vel[side] = [0.0; 3];
                    self.stuck_spin[side] = 0.0;
                    self.stuck_on[side] = true;
                }
                if !self.stuck_turn_on[side] {
                    self.stuck_turn[side] = qmul(&self.residual_turn[side], &pose_q);
                    if !qnorm(&mut self.stuck_turn[side]) {
                        self.stuck_turn[side] = pose_q;
                    }
                    self.stuck_turn_on[side] = true;
                }
                // FOLLOWS ITS BODY THROUGH A FILTER (one-euro): still, the stir of a box pressed on the world is filtered
                // away at STUCK_HZ; sliding, the cut-off rises with the body's speed and the drawn hand keeps up
                const STUCK_HZ: f32 = 1.5;
                const STUCK_BETA: f32 = 40.0;
                const STUCK_SPEED_HZ: f32 = 2.0;
                let pi = core::f32::consts::PI;
                let sdt = dt_s.clamp(1e-3, 0.1) as f32;
                let mut sp = 0.0f32;
                {
                    let a = 1.0 - (-2.0 * pi * STUCK_SPEED_HZ * sdt).exp();
                    for k in 0..3 {
                        self.stuck_vel[side][k] += ((bw[k] - self.stuck_prev[side][k]) / sdt - self.stuck_vel[side][k]) * a;
                        sp += self.stuck_vel[side][k] * self.stuck_vel[side][k];
                    }
                    sp = sp.sqrt();
                    let mut dq = qmul(&pose_q, &qconj(&self.stuck_prev_turn[side]));
                    if dq[3] < 0.0 {
                        for c in &mut dq {
                            *c = -*c;
                        }
                    }
                    let an = 2.0 * (dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]).sqrt().atan2(dq[3]);
                    self.stuck_spin[side] += (an / sdt - self.stuck_spin[side]) * a;
                    self.stuck_prev[side] = bw;
                    self.stuck_prev_turn[side] = pose_q;
                }
                let cutoff = STUCK_HZ + STUCK_BETA * (sp + 0.1 * self.stuck_spin[side]); // a radian as 10 cm
                let alpha = 1.0 - (-2.0 * pi * cutoff * sdt).exp();
                for k in 0..3 {
                    self.stuck[side][k] += (bw[k] - self.stuck[side][k]) * alpha;
                }
                let d = [bw[0] - self.stuck[side][0], bw[1] - self.stuck[side][1], bw[2] - self.stuck[side][2]];
                let dn = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                // and never farther off its body than stick_hold
                if dn > s.stick_hold {
                    for k in 0..3 {
                        self.stuck[side][k] += d[k] * (1.0 - s.stick_hold / dn);
                    }
                }
                o.body = self.stuck[side];
                o.vel = [0.0; 3];
                // the turn the same way: filtered toward the body's, never more than stick_turn off it
                if !self.stuck_turn_on[side] {
                    self.stuck_turn[side] = pose_q;
                    self.stuck_turn_on[side] = true;
                }
                let mut dq = qmul(&pose_q, &qconj(&self.stuck_turn[side]));
                if dq[3] < 0.0 {
                    for c in &mut dq {
                        *c = -*c;
                    }
                }
                let sn = (dq[0] * dq[0] + dq[1] * dq[1] + dq[2] * dq[2]).sqrt();
                let angle = 2.0 * sn.atan2(dq[3]);
                if sn > 1e-7 {
                    let left = (angle * (1.0 - alpha)).min(s.stick_turn);
                    let part = angle - left;
                    let h = (0.5 * part).sin() / sn;
                    let step = [dq[0] * h, dq[1] * h, dq[2] * h, (0.5 * part).cos()];
                    let mut q = qmul(&step, &self.stuck_turn[side]);
                    if qnorm(&mut q) {
                        self.stuck_turn[side] = q;
                    }
                }
                o.body_rot = self.stuck_turn[side];
                o.spin = [0.0; 3];
                for k in 0..3 {
                    self.residual[side][k] = self.stuck[side][k] - bw[k];
                }
                self.residual_turn[side] = qmul(&self.stuck_turn[side], &qconj(&pose_q));
                qnorm(&mut self.residual_turn[side]);
            } else {
                self.stuck_on[side] = false;
                self.stuck_turn_on[side] = false;
                // LETTING GO EASES OUT: what the drawn hand was held off its body (up to stick_hold, stick_turn) goes over
                // 60 ms. Dropped at once, the hand jumped by it as it left a table.
                let keep = (-dt_s / 0.06).exp() as f32;
                for k in 0..3 {
                    self.residual[side][k] *= keep;
                    o.body[k] = bw[k] + self.residual[side][k];
                }
                let rt = &mut self.residual_turn[side];
                if rt[3] < 0.0 {
                    for c in rt.iter_mut() {
                        *c = -*c;
                    }
                }
                let rs = (rt[0] * rt[0] + rt[1] * rt[1] + rt[2] * rt[2]).sqrt();
                if rs > 1e-6 {
                    let angle = 2.0 * rs.atan2(rt[3]) * keep;
                    let h = (0.5 * angle).sin() / rs;
                    rt[0] *= h;
                    rt[1] *= h;
                    rt[2] *= h;
                    rt[3] = (0.5 * angle).cos();
                } else {
                    *rt = [0.0, 0.0, 0.0, 1.0];
                }
                o.body_rot = qmul(rt, &pose_q);
                qnorm(&mut o.body_rot);
                o.vel = st.vel[side];
            }
            // a gun's hand turns as its body does while it touches something, as tracking has it otherwise
            let goal = if st.touching[side] { 1.0 } else { 0.0 };
            self.contact_w[side] += (goal - self.contact_w[side]) * (1.0 - (-dt_s / if goal > self.contact_w[side] { 0.03 } else { 0.15 }).exp()) as f32;
            o.contact = self.contact_w[side];
            o.held = 1.0;
            o.raw = bw;
            o.touching = u8::from(st.touching[side]);
            o.on_world = u8::from(st.on_world[side]);
            o.trace = u8::from(s.trace != 0);
            o.origin = st.origin;
            o.raw_rot = pose_q;
            o.margin = s.skin_margin;
            for k in 0..3 {
                o.sent[k] = st.sent[side][4 + k] + st.origin[k];
                o.sent_track[k] = st.sent_track[side][k] + st.origin[k];
                o.sent_entity[k] = st.sent_entity[side][k] + st.origin[k];
            }
            o.sent_rot = sent_q;
        }
        let r = self.smoothed[side].r;
        let a = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
        if a > 1e-6 {
            let sa = (0.5 * a).sin() / a;
            o.rot = [r[0] * sa, r[1] * sa, r[2] * sa, (0.5 * a).cos()];
        } else {
            o.rot[3] = 1.0;
        }
        self.off_sum[side] += len;
        if len > self.off_most[side] {
            self.off_most[side] = len;
        }
        // how much the drawn hand moves in the world a step while it is held off: the shaking (tracking's own tremor
        // cancels in it, as it does on screen), in the game's world, as PhysX's origin can move under the player
        let ow = qrot(&st.entity_rot, &o.pos);
        let mut shown_world = [0.0f32; 3];
        let mut body_world = [0.0f32; 3];
        let mut sent_world = [0.0f32; 3];
        for k in 0..3 {
            body_world[k] = st.pose[side][4 + k] + st.origin[k];
            shown_world[k] = (st.sent[side][4 + k] + st.origin[k] + ow[k]) * (1.0 - o.held) + o.body[k] * o.held;
            sent_world[k] = st.sent[side][4 + k] + st.origin[k];
        }
        let dist = |a: &[f32; 3], b: &[f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let jump = dist(&shown_world, &self.last_shown[side]);
        let body_jump = dist(&body_world, &self.last_body[side]);
        let sent_jump = dist(&sent_world, &self.last_sent[side]);
        self.last_sent[side] = sent_world;
        if len > 0.02 && self.last_shown_on[side] {
            self.jitter_sum[side] += jump;
            self.body_jitter_sum[side] += body_jump;
            self.sent_jitter_sum[side] += sent_jump;
            self.jitter_n[side] += 1;
            if jump > self.jitter_most[side] {
                self.jitter_most[side] = jump;
            }
        }
        self.last_shown[side] = shown_world;
        self.last_body[side] = body_world;
        self.last_shown_on[side] = true;
    }

    fn statistics(&mut self, _s: &Settings, say: bool) {
        let n = STEPS_SEEN.swap(0, Relaxed);
        let mean = |sum: u64| if n != 0 { sum as f64 / n as f64 } else { 0.0 };
        let (bu, bm, au, am) = (BEFORE_US.swap(0, Relaxed), BEFORE_MOST_US.swap(0, Relaxed), AFTER_US.swap(0, Relaxed), AFTER_MOST_US.swap(0, Relaxed));
        let off_n = self.off_n as f32;
        let per = |sum: f32, count: f32| if count != 0.0 { sum / count } else { 0.0 };
        let jn = [self.jitter_n[0] as f32, self.jitter_n[1] as f32];
        let lines1 = format!(
            "[physhands] {} steps: before the step {:.1} us mean (most {}), after {:.1} us (most {}); the drawn hand off tracking left {:.3} mean {:.3} most, right {:.3} mean {:.3} most m; pressed, its step to step move left {:.4} mean {:.4} most, right {:.4} mean {:.4} most m (the body itself {:.4} {:.4}, where it was sent {:.4} {:.4}); touching steps {} {} (the world {} {}); on {} {}\n",
            n,
            mean(bu),
            bm,
            mean(au),
            am,
            per(self.off_sum[0], off_n),
            self.off_most[0],
            per(self.off_sum[1], off_n),
            self.off_most[1],
            per(self.jitter_sum[0], jn[0]),
            self.jitter_most[0],
            per(self.jitter_sum[1], jn[1]),
            self.jitter_most[1],
            per(self.body_jitter_sum[0], jn[0]),
            per(self.body_jitter_sum[1], jn[1]),
            per(self.sent_jitter_sum[0], jn[0]),
            per(self.sent_jitter_sum[1], jn[1]),
            TOUCHED_STEPS[0].swap(0, Relaxed),
            TOUCHED_STEPS[1].swap(0, Relaxed),
            WORLD_STEPS[0].swap(0, Relaxed),
            WORLD_STEPS[1].swap(0, Relaxed),
            i32::from(LIVE[0].load(Relaxed)),
            i32::from(LIVE[1].load(Relaxed))
        );
        let mut mean_d = [0.0f64; 2];
        let mut most_d = [0.0f64; 2];
        let mut jumps = [0u64; 2];
        let mut tmean = [0.0f64; 2];
        let mut tmost = [0.0f64; 2];
        let mut smean = [0.0f64; 2];
        let mut smost = [0.0f64; 2];
        let mut amean = [0.0f64; 2];
        let mut sjumps = [0u64; 2];
        for side in 0..2 {
            let n = DRAWN_JERK_N[side].swap(0, Relaxed);
            mean_d[side] = if n != 0 { DRAWN_JERK_UM[side].swap(0, Relaxed) as f64 / n as f64 / 1000.0 } else { 0.0 };
            most_d[side] = DRAWN_JERK_MOST_UM[side].swap(0, Relaxed) as f64 / 1000.0;
            jumps[side] = DRAWN_JUMPS[side].swap(0, Relaxed);
            let n = TRACK_JERK_N[side].swap(0, Relaxed);
            tmean[side] = if n != 0 { TRACK_JERK_UM[side].swap(0, Relaxed) as f64 / n as f64 / 1000.0 } else { 0.0 };
            tmost[side] = TRACK_JERK_MOST_UM[side].swap(0, Relaxed) as f64 / 1000.0;
            let (n, an) = (SLIP_N[side].swap(0, Relaxed), AIR_SLIP_N[side].swap(0, Relaxed));
            smean[side] = if n != 0 { SLIP_UM[side].swap(0, Relaxed) as f64 / n as f64 / 1000.0 } else { 0.0 };
            smost[side] = SLIP_MOST_UM[side].swap(0, Relaxed) as f64 / 1000.0;
            sjumps[side] = SLIP_JUMPS[side].swap(0, Relaxed);
            amean[side] = if an != 0 { AIR_SLIP_UM[side].swap(0, Relaxed) as f64 / an as f64 / 1000.0 } else { 0.0 };
        }
        let lines2 = format!(
            "[physhands] drawn hand's offset from tracking, frame to frame: left {:.2} mm mean, {:.1} most, {} over 3 mm, in the air {:.2}; right {:.2} mean, {:.1} most, {} over 3 mm, in the air {:.2}\n",
            smean[0], smost[0], sjumps[0], amean[0], smean[1], smost[1], sjumps[1], amean[1]
        );
        let lines3 = format!(
            "[physhands] drawn hand off its own line in time, frame to frame: left {:.2} mm mean, {:.1} most, {} over 3 mm (tracking {:.2}, {:.1}); right {:.2} mean, {:.1} most, {} over 3 mm (tracking {:.2}, {:.1}). Loose things near {} (capped {}), the palm kept off them {} steps, let through them {} times, stacks let down {}, stacked things {}, let-down things kept in place {}; tracking past the arm's reach left {} right {} steps\n",
            mean_d[0],
            most_d[0],
            jumps[0],
            tmean[0],
            tmost[0],
            mean_d[1],
            most_d[1],
            jumps[1],
            tmean[1],
            tmost[1],
            LOOSE_LISTED.load(Relaxed),
            LOOSE_CAPPED.load(Relaxed),
            PALM_REFUSED.swap(0, Relaxed),
            LOOSE_LET_THROUGH.swap(0, Relaxed),
            DROPS_MADE.swap(0, Relaxed),
            LOOSE_STACKED.load(Relaxed),
            APART_KEPT.swap(0, Relaxed),
            REACH_HELD[0].swap(0, Relaxed),
            REACH_HELD[1].swap(0, Relaxed)
        );
        let solves = FINGER_SOLVES.swap(0, Relaxed);
        let lines4 = format!(
            "[physhands] fingers: {} solves with something near, {} penetration calls, {} in contact, {:.1} us a solve; world list: {} cycles, at most {} shapes; {} faults; posed down to the palm {}, palm on something {}; eased fingers put out {}, opened {}; the back of the hand pressed {}\n",
            solves,
            FINGER_CALLS.swap(0, Relaxed),
            FINGER_CONTACTS.swap(0, Relaxed),
            if solves != 0 { FINGER_US.swap(0, Relaxed) as f64 / solves as f64 } else { 0.0 },
            NEAR_CYCLES.swap(0, Relaxed),
            NEAR_MOST_SHAPES.swap(0, Relaxed),
            FINGER_FAULTS.load(Relaxed),
            PALM_SHIFTED.swap(0, Relaxed),
            PALM_WRAPS.swap(0, Relaxed),
            FINGERS_OUT.swap(0, Relaxed),
            FINGERS_OPENED.swap(0, Relaxed),
            BACK_PRESSED.swap(0, Relaxed)
        );
        if say {
            log!("{}", lines1);
            log!("{}", lines2);
            log!("{}", lines3);
            log!("{}", lines4);
            // the bends the solver gives each finger now (rad past the animation's, knuckle/middle/last): negative
            // straightens, positive curls toward the palm
            for sd in 0..2 {
                let b = |f: usize, j: usize| f32::from_bits(SHOWN_BENDS[sd][f][j].load(Relaxed));
                log!(
                    "[physhands] {} bends: thumb {:+.2} {:+.2} {:+.2}, index {:+.2} {:+.2} {:+.2}, middle {:+.2} {:+.2} {:+.2}, ring {:+.2} {:+.2} {:+.2}, little {:+.2} {:+.2} {:+.2}\n",
                    if sd == 1 { "right" } else { "left" },
                    b(4, 0), b(4, 1), b(4, 2), b(0, 0), b(0, 1), b(0, 2), b(1, 0), b(1, 1), b(1, 2), b(2, 0), b(2, 1), b(2, 2), b(3, 0), b(3, 1), b(3, 2)
                );
            }
        }
        self.off_sum = [0.0; 2];
        self.off_most = [0.0; 2];
        self.jitter_sum = [0.0; 2];
        self.jitter_most = [0.0; 2];
        self.body_jitter_sum = [0.0; 2];
        self.sent_jitter_sum = [0.0; 2];
        self.jitter_n = [0; 2];
        self.off_n = 0;
    }
}

fn work() {
    let mut s = Settings::default();
    settings::read_settings(&mut s);
    SETTINGS.write(&s);
    let now = tick_ms();
    let mut w = Work {
        s,
        seen_write: None,
        read_at: now,
        logged_at: now,
        off_most: [0.0; 2],
        off_sum: [0.0; 2],
        off_n: 0,
        was_on: [false; 2],
        last_step_at: 0,
        last_scanner_epoch: 0,
        last_smooth_at: 0.0,
        smoothed: [Smoothed::default(); 2],
        body_smoothed: [Smoothed::default(); 2],
        held: [0.0; 2],
        latch: [[0.0; 3]; 2],
        held_goal: [0.0; 2],
        stuck: [[0.0; 3]; 2],
        stuck_turn: [[0.0; 4]; 2],
        stuck_on: [false; 2],
        stuck_turn_on: [false; 2],
        stuck_prev: [[0.0; 3]; 2],
        stuck_prev_turn: [[0.0, 0.0, 0.0, 1.0]; 2],
        stuck_vel: [[0.0; 3]; 2],
        stuck_spin: [0.0; 2],
        pressed_was: [false; 2],
        residual: [[0.0; 3]; 2],
        residual_turn: [[0.0, 0.0, 0.0, 1.0]; 2],
        contact_w: [0.0; 2],
        latch_on: [false; 2],
        last_shown: [[0.0; 3]; 2],
        jitter_sum: [0.0; 2],
        jitter_most: [0.0; 2],
        last_body: [[0.0; 3]; 2],
        body_jitter_sum: [0.0; 2],
        last_sent: [[0.0; 3]; 2],
        sent_jitter_sum: [0.0; 2],
        last_shown_on: [false; 2],
        previous_filter_frame: [PoseFrame::default(); 2],
        previous_filter_riding: [false; 2],
        jitter_n: [0; 2],
    };
    // the file's time as first read: the C++'s seenWrite starts empty, so the first comparison reads it again
    loop {
        let e = STEPPED.load(Acquire);
        // SAFETY: the worker's event.
        unsafe { WaitForSingleObject(e as _, 50) };
        // SAFETY: the worker's own state and the threads' published copies.
        unsafe { w.tick() };
    }
}

/// StartWorker: the worker thread, once.
pub fn start_worker() {
    if WORKER_UP.compare_exchange(false, true, AcqRel, Acquire).is_err() {
        return;
    }
    // SAFETY: an auto-reset event, unnamed.
    let e = unsafe { CreateEventW(core::ptr::null(), 0, 0, core::ptr::null()) };
    STEPPED.store(e as usize, Release);
    let _ = std::thread::Builder::new().name("CPVR physics hands".into()).spawn(work);
}
