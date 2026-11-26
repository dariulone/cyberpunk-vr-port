//! reload.lua's M.frame and the helpers that read its slide / magazine / hatch / hammer / cylinder state: part A, the
//! frame's start (the rig signatures per drawn weapon, the holder debounced, the shotgun handed over, the bore axis,
//! the free hand) and the slide (its lock and catch, the press pose, the manual cycle, the grab by the recorded grips
//! turned round the part, the rack with its riders, the ejected round's fling and the next one's chase).
use super::belt::{f4, m, native, player, qmul, qrot, V3, Q};
use super::reload_core::*;
use super::reload_data::Lua;
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use serde_json::Value as J;

#[derive(Default)]
pub(super) struct SlideS {
    pub ammo: Option<i32>,
    pub ammo_last: Option<i32>,
    pub catch_off: bool,
    pub chambered: bool,
    pub chase_v: Option<f64>,
    pub click_prev: bool,
    pub cycle_held: bool,
    pub cycle_hid: Option<bool>,
    pub dist: f64,
    pub drain: f64,
    pub eject_t: Option<f64>,
    pub fired: bool,
    pub fired_at: Option<f64>,
    pub g_loc: Option<V3>,
    pub grabbed: bool,
    pub grip: Option<J>,
    pub hand: Option<usize>,
    pub hist: Vec<(f64, f64)>,
    pub hist_d: f64,
    pub hist_dir: i32,
    pub hist_t: f64,
    pub hr: V3,
    pub lock_base: Option<V3>,
    pub locked: bool,
    pub lt_near: bool,
    pub moved: bool,
    pub own_spend_at: Option<f64>,
    pub rack: f64,
    pub racked: bool,
    pub releasing: bool,
    pub rider_v: Vec<f64>,
    pub seated_shown: Option<bool>,
    pub snd_rider: bool,
    pub spent: Option<bool>,
    pub style: String,
    pub tagged: bool,
    pub trip: bool,
    pub user_closed: bool,
    pub was_active: bool,
    pub weapon_key: Option<u64>,
    pub why: String,
    pub case_snd: Option<String>,
    /// The stroke held until the hand and the spring finish it (main's slideStroke, 9528baf08).
    pub active_stroke: Option<&'static J>,
}
#[derive(Default)]
pub(super) struct HatchS {
    pub btn_prev: bool,
    pub lat: Option<f64>,
    pub open: bool,
    pub open_t: f64,
    pub p: f64,
    pub pw: Vec<bool>,
    pub read: Option<String>,
    pub rest: Option<[f64; 7]>,
    pub rest_candidate: Option<[f64; 7]>,
    pub rest_time: f64,
    pub flick_samples: i32,
    pub flick_travel: f64,
    pub want_prev: Option<f64>,
    pub wrote: bool,
    pub write_slot: Option<i32>,
}
#[derive(Default)]
pub(super) struct HamS {
    pub dbg: String,
    pub fall: Option<f64>,
    pub fin: bool,
    pub fired: bool,
    pub key: Option<String>,
    pub last: Option<i32>,
    pub p: f64,
    pub p0: f64,
    pub prev: bool,
    pub seq: Option<f64>,
    pub sounded: bool,
    pub want: f64,
    pub wrote: bool,
}
#[derive(Default)]
pub(super) struct SpinS {
    pub ang: f64,
    pub dbg: String,
    pub det: Option<f64>,
    pub faded: bool,
    pub hand: Option<usize>,
    pub on: bool,
    pub pp: Option<V3>,
    pub pc: V3,
    pub w: f64,
    pub wrote: bool,
    /// A speedloader / magazine just went home: the hand that pushed it turns nothing until its grip has opened.
    pub seat_hold: bool,
    /// The magazine's state last frame (its seat is the change from the hand to home).
    pub mag_prev: &'static str,
}
#[derive(Default)]
pub(super) struct Jit {
    pub n: i32,
    pub on: i32,
    pub lo: [Option<f64>; 4],
    pub hi: [Option<f64>; 4],
}
#[derive(Default)]
#[allow(dead_code)]
pub(super) struct Glide {
    pub t: f64,
    pub from: V3,
    pub from_q: Q,
    pub dur: f64,
}
#[derive(Default)]
#[allow(dead_code)]
pub(super) struct MagS {
    pub armed: bool,
    pub btn_was: bool,
    pub carried: Option<i32>,
    pub cases_out: bool,
    pub dbg2: String,
    pub depth: Option<f64>,
    pub diag: String,
    pub dist: f64,
    pub drop_c: Option<V3>,
    pub drop_c0: Option<V3>,
    pub drop_c1: Option<V3>,
    pub drop_hits: i32,
    pub drop_id: Option<u64>,
    pub drop_p: Option<V3>,
    pub drop_q: Option<Q>,
    pub drop_q0: Option<Q>,
    pub drop_q1: Option<Q>,
    pub drop_rest: Option<f64>,
    pub drop_t: f64,
    pub drop_touch: Option<f64>,
    pub drop_v: Option<V3>,
    pub drop_w: Option<V3>,
    pub dry: bool,
    pub ent_id: Option<u64>,
    pub fresh_load: bool,
    pub from_belt: bool,
    pub gap_v: Option<f64>,
    pub glide: Option<Glide>,
    pub grip_lock: bool,
    pub guide_out: bool,
    pub hand: Option<usize>,
    /// The held magazine's last pose and motion: position 0..3, orientation 3..7, velocity 7..10, spin 10..13.
    pub hold: Option<[f64; 13]>,
    pub in_well: bool,
    pub ins_d: Option<f64>,
    pub ins_w: Option<f64>,
    pub jit: Option<Jit>,
    pub loaded: Option<i32>,
    pub lt_near: bool,
    pub mag_side: Option<usize>,
    pub magnet: i32,
    pub palm_dot: Option<f64>,
    pub pending: Option<i32>,
    pub perp: Option<f64>,
    pub pose_ok: Option<bool>,
    pub roll: Option<Q>,
    pub route: Option<String>,
    pub seat_wt: Option<[f64; 7]>,
    pub seen_d: Option<f64>,
    pub skin_want: Option<String>,
    pub slide_has_pose: bool,
    pub slot_comp: Handle,
    pub snap_d: Option<f64>,
    pub snap_dpre: Option<f64>,
    pub state: &'static str,
    pub t: f64,
    pub tk: V3,
    pub turn_q: Option<Q>,
    pub variant: Option<i32>,
    pub well_pick: i32,
    pub zero_at: Option<f64>,
    pub off: V3,
    pub ent_ready: bool,
    pub magnet_w: f64,
    pub stage_key: Option<String>,
    pub stage_pose: Option<super::reload_data::Pose>,
    pub turn_on: bool,
}
pub(super) struct FrameState {
    pub slide: SlideS,
    pub rot_t: f64,
    pub hatch: HatchS,
    pub ham: HamS,
    pub spin: SpinS,
    pub mag: MagS,
}
impl FrameState {
    pub(super) fn new() -> FrameState {
        FrameState {
            slide: SlideS { dist: -1.0, style: "-".into(), ..Default::default() },
            rot_t: -1.0,
            hatch: HatchS::default(),
            ham: HamS::default(),
            spin: SpinS::default(),
            mag: MagS { state: "in", dist: -1.0, ..Default::default() },
        }
    }
}

// ---------------------------------------------------------------- the helpers that read the frame's state
/// magHome: the magazine home -- the held entity and any fall despawned, the carriers and items let go, the hatch shut,
/// the hands' magnets off, the belt's pouch full again.
pub(super) fn mag_home(s: &mut State) {
    s.flick.cancel();
    for id in [s.frame.mag.ent_id.take(), s.frame.mag.drop_id.take()].into_iter().flatten() {
        if !despawn(id) {
            s.litter.push(id);
        }
    }
    hold_hide(s, None);
    item_drop(s, 0, "AttachmentSlots.WeaponLeft");
    item_drop(s, 1, "AttachmentSlots.WeaponRight");
    lead_reset(s, "hand");
    lead_reset(s, "glide");
    let mg = &mut s.frame.mag;
    (mg.drop_p, mg.drop_v, mg.drop_q, mg.drop_t) = (None, None, None, 0.0);
    (mg.drop_c, mg.drop_w, mg.drop_rest, mg.drop_hits, mg.hold) = (None, None, None, 0, None);
    mg.drop_touch = None;
    let h = &mut s.frame.hatch;
    (h.p, h.wrote, h.pw, h.want_prev) = (0.0, false, Vec::new(), None);
    if s.frame.mag.state == "glide" {
        s.frame.mag.state = "gone";
    }
    s.frame.mag.glide = None;
    track_write(0, 0, 1.0, 0);
    track_write(0, 1, 0.0, 0);
    if s.frame.mag.magnet > 0 {
        for h in 0..2 {
            stop_model(h, false, [0.0; 3]);
            stop_rot(h, 0, [0.0, 0.0, 0.0, 1.0]);
        }
    }
    let mg = &mut s.frame.mag;
    mg.magnet = 0;
    (mg.state, mg.hand, mg.t, mg.armed) = ("in", None, 0.0, false);
    mg.from_belt = false;
    belt_mag_taken(false);
    mg.slot_comp = Handle::null();
    s.hold.comp = [Handle::null(), Handle::null()];
    s.hold.mesh = [None, None];
    s.hold.skin = [None, None];
    let h = &mut s.frame.hatch;
    (h.open, h.btn_prev, h.open_t) = (false, false, 0.0);
    (h.rest, h.rest_candidate, h.rest_time) = (None, None, 0.0);
    (h.flick_travel, h.flick_samples) = (0.0, 0);
}
/// The game's own reload is running (the PSM's weapon state of the support hand reads Reload).
pub(super) fn game_reloading() -> bool {
    let pl = player();
    let bb = super::belt::h(&pl, "GetPlayerStateMachineBlackboard", &[]);
    if bb.is_null() {
        return false;
    }
    (|| -> Result<i32, String> {
        let defs = super::boards::defs()?;
        let d = super::boards::definition(&defs, "PlayerStateMachine")?;
        let f = super::boards::field(d, if support_hand() == 1 { "LeftWeapon" } else { "RightWeapon" })?;
        super::boards::get::<i32>(&bb, "GetInt", f)
    })()
    .is_ok_and(|v| v == 2)
}
/// oursActive: anything of ours is holding the weapon.
pub(super) fn ours_active(s: &State) -> bool {
    let (sl, mg, f) = (&s.frame.slide, &s.frame.mag, &s.frame);
    sl.grabbed
        || sl.releasing
        || sl.user_closed
        || sl.tagged
        || f.rot_t >= 0.0
        || sl.eject_t.is_some()
        || sl.chase_v.is_some()
        || mg.state != "in"
        || mg.pending.is_some()
        || mg.cases_out
        || f.hatch.p > 0.0
        || f.ham.p > 0.0
        || f.spin.wrote
        || s.own_tagged
        || s.fade.hand.is_some()
        || s.press.on
        || sl.fired
        || sl.cycle_held
        || super::reload_shotgun::active(s)
}
/// yieldAll: everything of ours let go; the game's reload has the weapon.
pub(super) fn yield_all(s: &mut State, weapon: &Handle) {
    if let Some(c) = s.frame.mag.carried {
        give_reserve(weapon, c);
    }
    let mg = &mut s.frame.mag;
    (mg.carried, mg.pending, mg.loaded) = (None, None, None);
    (mg.fresh_load, mg.grip_lock) = (false, false);
    if mg.cases_out {
        super::reload_revolver::clear(s);
        s.frame.mag.cases_out = false;
    }
    mag_home(s);
    if let Some(cfg) = (!weapon.is_null()).then(|| cfg_for(s, weapon)).flatten() {
        let mc = cfg.g("mag");
        if mc.present() {
            let which = mc.f("which", 0.0) as i32;
            let show = mc.f("showTrack", 0.0) as i32;
            if which != 0 || show != 0 {
                track_write(which, show, 1.0, 0);
            }
            for bn in mc.g("hideScale").arr() {
                rig_write_scale(which, bn.as_f64().unwrap_or(0.0) as i32, 0.0);
            }
        }
    }
    let sl = &mut s.frame.slide;
    (sl.grabbed, sl.releasing, sl.hand, sl.grip) = (false, false, None, None);
    sl.active_stroke = None;
    (sl.user_closed, sl.lock_base, sl.tagged) = (false, None, false);
    (sl.eject_t, sl.chase_v, sl.rack, sl.drain) = (None, None, 0.0, 0.0);
    (sl.fired, sl.fired_at, sl.cycle_held, sl.ammo_last, sl.racked, sl.cycle_hid) = (false, None, false, None, false, None);
    (sl.lt_near, sl.locked) = (false, false);
    s.frame.mag.lt_near = false;
    s.frame.rot_t = -1.0;
    let hm = &mut s.frame.ham;
    (hm.p, hm.want, hm.wrote) = (0.0, 0.0, false);
    s.frame.spin = SpinS::default();
    s.own_tagged = false;
    rig_write_clear();
    fade_reset(s);
    if s.press.on {
        if let Some(h) = s.press.hand {
            clear_grip(h);
        }
    }
    s.press = Press { t: -1.0, hand: None, on: false };
    pull_in_reset(s);
    for h in 0..2 {
        stop_rot(h, 0, [0.0, 0.0, 0.0, 1.0]);
        stop_model(h, false, [0.0; 3]);
    }
    s.owned_hand = None;
    super::reload_shotgun::yield_(s, weapon);
    set_trg(s, 0);
}
/// M.release: the weapon let go (dual wield took it, or it went away).
pub(super) fn release(s: &mut State, weapon: &Handle) {
    s.flick.cancel();
    s.frame.mag.btn_was = true;
    yield_all(s, weapon);
    th_mask(s, 0);
    super::reload_shotgun::lost(s);
    super::reload_shotgun::preview_stop(s);
}
/// M.gameReload: true while the game's own reload runs (ours cancelled once at its start).
pub(super) fn game_reload(s: &mut State, weapon: &Handle) -> bool {
    if !game_reloading() {
        s.yield_on = false;
        return false;
    }
    s.flick.cancel();
    s.frame.mag.btn_was = super::belt::shared(157) > 0.5;
    super::reload_shotgun::cycle_free(s);
    if s.yield_on {
        return true;
    }
    if !ours_active(s) {
        // the animated crane left to the game even when its reload began while the VR controls were idle
        let cfg = (!weapon.is_null()).then(|| cfg_for(s, weapon)).flatten();
        if !cfg.is_some_and(|c| c.g("hatch").g("closedRotation").present()) {
            return false;
        }
        (s.frame.hatch.rest_candidate, s.frame.hatch.rest_time) = (None, 0.0);
        s.frame.hatch.btn_prev = super::belt::shared(157) > 0.5;
        return true;
    }
    yield_all(s, weapon);
    s.yield_on = true;
    cvr_host::log!("[Reload] game reload: our interaction cancelled, the reload is the game's (from Rust)\n");
    true
}
/// cylinderLateralFlick (main's 60ad61ac9): a crane shut by a lateral swing of the tracked hand -- turning the gun to
/// look into the chambers and bringing it upright is not a closing gesture. At least two samples in one direction and
/// 2.5 cm of travel at the swing's speed: a single tracking spike cannot close it.
pub(super) fn cylinder_lateral_flick(h: &mut HatchS, v: Option<[f32; 4]>, a: V3, dt: f64, threshold: f64) -> bool {
    let Some(v) = v.filter(|v| v[3] > 0.5 && dt.is_finite() && dt > 0.0 && dt <= 0.10) else {
        (h.flick_travel, h.flick_samples) = (0.0, 0);
        return false;
    };
    let rate = f64::from(v[0]) * a[0] + f64::from(v[1]) * a[1] + f64::from(v[2]) * a[2];
    let speed = rate.abs();
    if !speed.is_finite() || speed < threshold {
        (h.flick_travel, h.flick_samples) = (0.0, 0);
        return false;
    }
    h.lat = Some(h.lat.unwrap_or(0.0).max(speed));
    let mut travel = h.flick_travel;
    if travel * rate < 0.0 {
        travel = 0.0;
        h.flick_samples = 0;
    }
    h.flick_travel = travel + rate * dt;
    h.flick_samples += 1;
    h.flick_samples >= 2 && h.flick_travel.abs() >= 0.025
}
/// captureCraneRest: the crane bone's own rest pose, once it has been still for 0.15 s.
pub(super) fn capture_crane_rest(h: &mut HatchS, hc: &J, dt: f64) {
    if h.rest.is_some() || h.open || h.p > 0.0 {
        return;
    }
    let (which, bone) = (hc.f("which", 0.0) as i32, hc.f("bone", 0.0) as i32);
    let mut p = [0.0; 7];
    for (i, v) in p.iter_mut().enumerate() {
        match rig_bone(which, bone, i as i32) {
            Some(x) => *v = x,
            None => {
                (h.rest_candidate, h.rest_time) = (None, 0.0);
                return;
            }
        }
    }
    let norm = (p[3] * p[3] + p[4] * p[4] + p[5] * p[5] + p[6] * p[6]).sqrt();
    if !(0.95..=1.05).contains(&norm) || p[0] * p[0] + p[1] * p[1] + p[2] * p[2] > 0.36 {
        (h.rest_candidate, h.rest_time) = (None, 0.0);
        return;
    }
    for c in &mut p[3..7] {
        *c /= norm;
    }
    // a native reload can hold the OPEN pose perfectly still: stability alone cannot identify the rest frame, so it is
    // compared against the equipped rig's bind rotation (within 2 degrees)
    let closed = hc.v4("closedRotation");
    if let Some(cl) = closed {
        let dot = (p[3] * cl[0] + p[4] * cl[1] + p[5] * cl[2] + p[6] * cl[3]).abs();
        if dot < 0.99985 {
            (h.rest_candidate, h.rest_time) = (None, 0.0);
            return;
        }
    }
    let stable = h.rest_candidate.is_some_and(|q| {
        let d = (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
        let dot = (p[3] * q[3] + p[4] * q[4] + p[5] * q[5] + p[6] * q[6]).abs();
        d < 0.000001 && dot > 0.99999
    });
    h.rest_time = if stable { h.rest_time + dt.min(0.05) } else { 0.0 };
    if !stable {
        h.rest_candidate = Some(p);
    }
    if h.rest_time >= 0.15 {
        if let Some(cl) = closed {
            p[3..7].copy_from_slice(&cl);
        }
        h.rest = Some(p);
    }
}
/// slideStroke: one stroke kept until the hand and the spring have finished it (ejecting the case clears `fired`
/// mid-stroke and must not reverse a held bolt); after a shot a manual action's `postShot` stroke stands in.
pub(super) fn slide_stroke(sc: &'static J, sl: &mut SlideS, mg: &MagS) -> &'static J {
    if let Some(a) = sl.active_stroke {
        if sl.grabbed || sl.releasing || sl.drain > 0.0 {
            return a;
        }
    }
    sl.active_stroke = None;
    if sc.g("postShot").present() && sl.fired && !mg.pending.is_some_and(|p| p > 0) {
        return post_shot(sc);
    }
    sc
}
/// The slide's config with its `postShot` fields over it (Lua's setmetatable(sc.postShot, { __index = sc })), made once.
fn post_shot(sc: &'static J) -> &'static J {
    use std::sync::Mutex;
    static MADE: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
    let key = sc as *const J as usize;
    let mut made = MADE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(&(_, at)) = made.iter().find(|(k, _)| *k == key) {
        // SAFETY: a leaked config, alive for the process.
        return unsafe { &*(at as *const J) };
    }
    let mut merged = sc.clone();
    if let (Some(m), Some(over)) = (merged.as_object_mut(), sc.g("postShot").as_object()) {
        for (k, v) in over {
            m.insert(k.clone(), v.clone());
        }
    }
    let leaked: &'static J = Box::leak(Box::new(merged));
    made.push((key, leaked as *const J as usize));
    leaked
}
/// driveCraneRest: the crane written as its rest pose turned by `angle` about its axis.
pub(super) fn drive_crane_rest(h: &mut HatchS, hc: &J, angle: f64) -> bool {
    let Some(p) = h.rest else { return false };
    let Some(ax) = hc.v3("axis") else { return false };
    let n = len3(ax);
    if n < 0.001 {
        return false;
    }
    let hh = (angle * hc.f("sign", 1.0)).to_radians() * 0.5;
    let sn = hh.sin() / n;
    let q = qmul([p[3], p[4], p[5], p[6]], [ax[0] * sn, ax[1] * sn, ax[2] * sn, hh.cos()]);
    let (which, bone) = (hc.f("which", 0.0) as i32, hc.f("bone", 0.0) as i32);
    rig_write_rot(which, bone, 0.0, ax);
    h.write_slot = rig_write_abs(which, bone, [p[0], p[1], p[2]], q, 1);
    true
}

// ---------------------------------------------------------------- M.frame, part A
/// The frame's locals shared by its parts.
pub(super) struct Ctx {
    pub weapon: Handle,
    pub slot_comp: Handle,
    pub holder: usize,
    pub free: usize,
    pub dt: f64,
    pub cfg: &'static J,
    pub snd: &'static J,
    pub sc: &'static J,
    pub la: V3,
    pub b: V3,
    pub c: Q,
    pub a: V3,
    pub h: V3,
    pub empty: bool,
    pub sp: Option<V3>,
    pub db: Option<V3>,
    pub sb: Option<V3>,
    pub g_weapon: bool,
    pub gf: V3,
    pub gd: V3,
    pub gs: V3,
}
static EMPTY: J = J::Null;
fn snd_of<'a>(c: &'a Ctx, k: &str) -> Option<&'a str> {
    c.snd.s(k)
}
fn slide_read(c: &Ctx) -> Option<V3> {
    let (which, bone) = (c.sc.f("which", 0.0) as i32, c.sc.f("bone", 0.0) as i32);
    Some([rig_bone(which, bone, 0)?, rig_bone(which, bone, 1)?, rig_bone(which, bone, 2)?])
}
fn slide_locked_now(c: &Ctx) -> bool {
    let Some(lk) = c.sc.v3("lockLocal") else { return false };
    let Some(x) = slide_read(c) else { return false };
    let rl = c.sc.v3("restLocal").unwrap_or([0.0; 3]);
    len3([x[0] - lk[0], x[1] - lk[1], x[2] - lk[2]]) < len3([x[0] - rl[0], x[1] - rl[1], x[2] - rl[2]])
}
/// driveSlide: the slide written `r` metres back along its axis (from its lock base), its riders after it.
pub(super) fn drive_slide(s: &mut State, c: &Ctx, r: f64) {
    let sc = c.sc;
    let la = c.la;
    let (which, bone) = (sc.f("which", 0.0) as i32, sc.f("bone", 0.0) as i32);
    let sign = sc.f("sign", 1.0);
    let d = r * sign;
    let rl = sc.v3("restLocal");
    let rq = sc.v4("restRot");
    let sl = &mut s.frame.slide;
    if let Some(rl) = rl {
        let base = sl.lock_base.unwrap_or(rl);
        rig_write_abs(which, bone, [base[0] + la[0] * d, base[1] + la[1] * d, base[2] + la[2] * d], rq.unwrap_or([0.0, 0.0, 0.0, 1.0]), 1);
    } else {
        rig_write(which, bone, la.map(|v| v * d));
    }
    let riders = sc.g("riders").arr();
    if riders.is_empty() {
        return;
    }
    sl.hist_t += c.dt;
    sl.hist.push((sl.hist_t, d));
    while sl.hist.len() > 1 && sl.hist[0].0 < sl.hist_t - 0.30 {
        sl.hist.remove(0);
    }
    let dd = d - sl.hist_d;
    if dd > 1e-6 {
        sl.hist_dir = 1;
    } else if dd < -1e-6 {
        sl.hist_dir = -1;
    }
    let outward = sl.hist_dir >= 0;
    sl.hist_d = d;
    let delayed = |sl: &SlideS, del: Option<f64>| -> f64 {
        let Some(del) = del.filter(|v| *v > 0.0) else { return d };
        if sl.hist.len() < 2 {
            return d;
        }
        let want = sl.hist_t - del;
        if sl.hist[0].0 >= want {
            return sl.hist[0].1;
        }
        for k in (1..sl.hist.len()).rev() {
            let (a, b) = (sl.hist[k - 1], sl.hist[k]);
            if a.0 <= want && want <= b.0 {
                let span = b.0 - a.0;
                let t2 = if span > 1e-6 { (want - a.0) / span } else { 0.0 };
                return a.1 + (b.1 - a.1) * t2;
            }
        }
        d
    };
    let mut lock_d = 0.0;
    if let (Some(lb), Some(rl)) = (sl.lock_base, rl) {
        lock_d = ((lb[0] - rl[0]) * la[0] + (lb[1] - rl[1]) * la[1] + (lb[2] - rl[2]) * la[2]) * sign;
    }
    if sl.rider_v.len() < riders.len() {
        sl.rider_v.resize(riders.len(), 0.0);
    }
    for (i, rd) in riders.iter().enumerate() {
        let mut v = d;
        if rd.b("abs") {
            v = d + lock_d;
        }
        if rd.g("delayOut").present() || rd.g("delayHome").present() || rd.g("delay").present() {
            let del = if outward { rd.fo("delayOut").or(rd.fo("delay")) } else { rd.fo("delayHome").or(rd.fo("delay")) };
            v = delayed(sl, del);
        } else if let Some(lag) = rd.fo("lag").filter(|l| *l > 0.0) {
            let prev = sl.rider_v[i];
            v = prev + (v - prev) * (1.0 - (-c.dt / lag).exp());
        }
        sl.rider_v[i] = v;
        let rbone = rd.f("bone", -1.0) as i32;
        let travel = sc.f("travel", 1.0);
        if rbone == bone {
            if let (Some(turn), Some(rl)) = (rd.fo("turn"), rl) {
                let ft = (v / rd.f("turnAt", travel)).clamp(0.0, 1.0);
                let ax = rd.v3("axis").unwrap_or([0.0, 0.0, 1.0]);
                let hh = (turn * ft).to_radians() * 0.5;
                let sn = hh.sin();
                let q = qmul(rq.unwrap_or([0.0, 0.0, 0.0, 1.0]), [ax[0] * sn, ax[1] * sn, ax[2] * sn, hh.cos()]);
                let base = sl.lock_base.unwrap_or(rl);
                rig_write_abs(which, bone, [base[0] + la[0] * d, base[1] + la[1] * d, base[2] + la[2] * d], q, 1);
            }
        } else if let Some(keys) = rd.g("turnKeys").as_array().filter(|k| !k.is_empty()) {
            // AN AUTHORED HINGE (main's 05481c080, the Metel): its parent-local rest and the keys pairing metres from
            // home with degrees from rest; written absolute, any relative turn cleared first
            let key = |k: &J| (k.get(0).and_then(J::as_f64).unwrap_or(0.0), k.get(1).and_then(J::as_f64).unwrap_or(0.0));
            let mut angle = key(&keys[keys.len() - 1]).1;
            for j in 1..keys.len() {
                let (ka, kb) = (key(&keys[j - 1]), key(&keys[j]));
                if v <= kb.0 {
                    let f = ((v - ka.0) / (kb.0 - ka.0)).max(0.0);
                    angle = ka.1 + (kb.1 - ka.1) * f;
                    break;
                }
            }
            let offset = v * rd.f("ratio", 1.0);
            let x = la.map(|l| l * offset);
            let ax = rd.v3("axis").unwrap_or([0.0, 0.0, 1.0]);
            let p = rd.v3("restLocal").unwrap_or([0.0; 3]);
            let q = rd.v4("restRot").unwrap_or([0.0, 0.0, 0.0, 1.0]);
            let half = angle.to_radians() * 0.5;
            let sn = half.sin();
            let qq = qmul(q, [ax[0] * sn, ax[1] * sn, ax[2] * sn, half.cos()]);
            rig_write_rot(which, rbone, 0.0, ax);
            rig_write_abs(which, rbone, [p[0] + x[0], p[1] + x[1], p[2] + x[2]], qq, 1);
        } else if let Some(vec) = rd.v3("vec") {
            rig_write(which, rbone, vec.map(|x| x * v));
        } else {
            let mut rr2 = v * rd.f("ratio", 1.0);
            if let Some(cap) = rd.fo("cap") {
                rr2 = rr2.min(cap);
            }
            rig_write(which, rbone, la.map(|x| x * rr2));
        }
        if let (Some(turn), true) = (rd.fo("turn"), rbone != bone) {
            let ft = (v / rd.f("turnAt", travel)).clamp(0.0, 1.0);
            let ax = rd.v3("axis").unwrap_or([0.0, 0.0, 1.0]);
            rig_write_rot(which, rbone, turn * ft, ax);
        }
    }
}
/// The chambered round's bones handed back to the animation (main's releaseRoundPose, 16c8d26fa): a write of zero is
/// an override still; a hidden spent case stays hidden but no longer owns a pose.
fn release_round_pose(s: &State, which: i32, bones: &[i32]) {
    for &bone in bones {
        rig_write_off(which, bone);
        if s.frame.slide.seated_shown == Some(false) {
            rig_write_scale(which, bone, 0.001);
        }
    }
}
fn show_seated(s: &mut State, c: &Ctx, v: f64) {
    let show = v > 0.5;
    s.frame.slide.seated_shown = Some(show);
    let scale = if show { 0.0 } else { 0.001 };
    let which = c.sc.f("which", 0.0) as i32;
    for bn in c.sc.g("chamberRound").g("bones").arr() {
        rig_write_scale(which, bn.as_f64().unwrap_or(0.0) as i32, scale);
    }
}
fn release_slide(s: &mut State, c: &Ctx) -> bool {
    let (which, bone) = (c.sc.f("which", 0.0) as i32, c.sc.f("bone", 0.0) as i32);
    rig_write_off(which, bone);
    for (i, rd) in c.sc.g("riders").arr().iter().enumerate() {
        if let Some(b) = rd.fo("bone") {
            rig_write_off(which, b as i32);
        }
        if let Some(v) = s.frame.slide.rider_v.get_mut(i) {
            *v = 0.0;
        }
    }
    (s.frame.slide.rack, s.frame.slide.lock_base) = (0.0, None);
    true
}
fn grip_target(c: &Ctx, g: &J, preview: bool) -> V3 {
    let o = g.v3("off").unwrap_or([0.0; 3]);
    let (mut x, mut y, mut z) = (o[0], o[1], o[2]);
    if preview {
        if let Some(po) = g.v3("previewOff") {
            (x, y, z) = (x + po[0], y + po[1], z + po[2]);
        }
    }
    let sp = c.sp.unwrap_or([0.0; 3]);
    [
        sp[0] + c.gf[0] * x + c.gd[0] * y + c.gs[0] * z,
        sp[1] + c.gf[1] * x + c.gd[1] * y + c.gs[1] * z,
        sp[2] + c.gf[2] * x + c.gd[2] * y + c.gs[2] * z,
    ]
}
fn reset_spin(s: &mut State) {
    let sp = &mut s.frame.spin;
    (sp.ang, sp.pp, sp.hand, sp.w, sp.det, sp.on) = (0.0, None, None, 0.0, None, false);
    if sp.faded {
        sp.faded = false;
        fade_out(s);
    }
}
/// M.tickLooseRounds: what is already loose -- the litter, the racked-out cases, the falling shells and rounds -- goes on
/// every frame, whatever is in the hands and whoever has the gun (main's d495625fb).
pub(super) fn tick_loose_rounds(s: &mut State, dt: f64) {
    mag_litter_sweep(s);
    case_tick(s);
    super::reload_shotgun::tick_bodies(s, dt);
}
/// M.frame: one frame of the hand-driven reload.
pub(super) fn frame(s: &mut State, weapon: &Handle, slot_comp: &Handle, holder: Option<usize>, dt: f64) {
    super::reload_shotgun::tick_preview(s, dt);
    s.owned_hand = None;
    if !s.frame.slide.grabbed {
        pull_in_reset(s);
    }
    let Some(holder) = holder.filter(|_| !weapon.is_null() && !slot_comp.is_null()) else {
        s.flick.cancel();
        s.frame.mag.btn_was = true;
        set_trg(s, 0);
        reset_spin(s);
        if s.frame.mag.state != "in" || s.frame.mag.ent_id.is_some() {
            mag_home(s);
        }
        if s.frame.slide.grabbed || s.fade.hand.is_some() {
            rig_write_clear();
            (s.frame.slide.grabbed, s.frame.slide.hand) = (false, None);
            fade_reset(s);
            if s.press.on {
                if let Some(h) = s.press.hand {
                    clear_grip(h);
                }
            }
            s.press = Press { t: -1.0, hand: None, on: false };
            for h in 0..2 {
                stop_rot(h, 0, [0.0, 0.0, 0.0, 1.0]);
            }
        }
        super::reload_shotgun::lost(s);
        s.exit_at = "noholder";
        return;
    };
    let cfg = cfg_for(s, weapon);
    if cfg.is_none_or(|c| !c.g("shotgun").present()) {
        super::reload_shotgun::lost(s);
    }
    let alt = cfg.and_then(|c| c.g("mag").fo("holdAlt")).filter(|_| s.frame.mag.state != "in");
    th_mask(s, alt.map_or(0, |a| 2f64.powf(a + 1.0).floor() as i32));
    let Some(cfg) = cfg else {
        s.exit_at = "nocfg";
        return;
    };
    let wkey = super::belt::entity_hash(weapon);
    if wkey != s.frame.slide.weapon_key {
        new_weapon(s, cfg, wkey);
    }
    if Some(holder) != s.holder_seen {
        (s.holder_seen, s.holder_for) = (Some(holder), 0.0);
    } else {
        s.holder_for += dt;
    }
    if s.holder_ok.is_none() || (s.holder_for >= 0.25 && Some(holder) != s.holder_ok) {
        s.holder_ok = Some(holder);
    }
    let holder = s.holder_ok.unwrap_or(holder);
    if cfg.g("shotgun").present() {
        let (r, own) = super::reload_shotgun::frame(s, weapon, slot_comp, holder, dt, cfg);
        s.exit_at = r;
        s.owned_hand = own;
        return;
    }
    let free = if holder == 1 { 0 } else { 1 };
    let Some((b, cq)) = player_frame() else {
        s.exit_at = "noframe";
        return;
    };
    let barrel = cfg.g("barrel");
    let (Some(fr), Some(bk)) = (slot_model(slot_comp, barrel.s("front"), b, cq), slot_model(slot_comp, barrel.s("back"), b, cq)) else {
        s.exit_at = "nobarrel";
        return;
    };
    let a0 = [fr[0] - bk[0], fr[1] - bk[1], fr[2] - bk[2]];
    let al = len3(a0);
    if al < 1e-4 {
        return;
    }
    let a = a0.map(|v| v / al);
    let Some(h) = hand_model(free) else {
        s.exit_at = "nohand";
        return;
    };
    s.exit_at = "ok";
    let base: &'static J = if cfg.g("slide").present() { cfg.g("slide") } else { &EMPTY };
    let sc = slide_stroke(base, &mut s.frame.slide, &s.frame.mag);
    let mut c = Ctx {
        weapon: weapon.clone(),
        slot_comp: slot_comp.clone(),
        holder,
        free,
        dt,
        cfg,
        snd: cfg.g("sounds"),
        sc,
        la: sc.v3("localAxis").unwrap_or([0.0, 0.0, -1.0]),
        b,
        c: cq,
        a,
        h,
        empty: w0_is_empty(weapon),
        sp: None,
        db: None,
        sb: None,
        g_weapon: false,
        gf: a,
        gd: [0.0; 3],
        gs: [0.0; 3],
    };
    slide_lock(s, &c);
    press_tick(s, &c);
    slide_hold(s, &c);
    slide_basis(s, &mut c);
    slide_ammo(s, &c);
    if c.sp.is_some() && c.db.is_some() {
        if s.frame.slide.grabbed {
            slide_grabbed(s, &c);
        } else {
            slide_free(s, &c);
        }
    }
    chamber_round(s, &c);
    fade_tick(s, dt, sc.fo("blendTime"));
    belt_mag_taken(s.frame.mag.state == "hand" && s.frame.mag.from_belt);
    super::reload_mag::frame(s, &c);
}
/// A freshly drawn gun: chambered, the rig signatures registered, our magazine home.
fn new_weapon(s: &mut State, cfg: &J, wkey: Option<u64>) {
    s.frame.mag.btn_was = super::belt::shared(157) > 0.5;
    let sl = &mut s.frame.slide;
    sl.weapon_key = wkey;
    sl.chambered = true;
    (sl.eject_t, sl.chase_v) = (None, None);
    (sl.fired, sl.ammo_last, sl.fired_at, sl.cycle_held, sl.racked, sl.cycle_hid) = (false, None, None, false, false, None);
    (sl.locked, sl.lock_base, sl.user_closed, sl.catch_off) = (false, None, false, false);
    sl.tagged = false;
    rig_write_clear();
    native::<()>("VRRigReset", &[]);
    register_rig_list(super::reload_data::data().g("rigs").arr().iter().chain(cfg.g("rigs").arr().iter()));
    mag_home(s);
}
/// The known rig signatures (rigs.lua's list, the reload data's "rigs"): the recorder registers them before a take,
/// as the mod's registerRigs did.
pub(super) fn register_rigs() {
    register_rig_list(super::reload_data::data().g("rigs").arr().iter());
}
/// VRRigSignature for each rig of the list.
fn register_rig_list<'a>(rigs: impl Iterator<Item = &'a J>) {
    for r in rigs {
        let n = r.g("names").arr();
        let name = |k: usize| -> (i32, super::belt::ArgStr) {
            let e = n.get(k);
            (e.map_or(-1, |e| e.i(1).as_f64().unwrap_or(-1.0) as i32), super::belt::tdb_str(e.and_then(|e| e.i(2).as_str()).unwrap_or("")))
        };
        let (n1, n2, n3, n4) = (name(0), name(1), name(2), name(3));
        let slot = native::<i32>(
            "VRRigSignature",
            &[
                Value::Int32(r.f("which", 0.0) as i32),
                Value::Int32(r.f("bones", 0.0) as i32),
                Value::Int32(n1.0),
                n1.1.value(),
                Value::Int32(n2.0),
                n2.1.value(),
                Value::Int32(n3.0),
                n3.1.value(),
                Value::Int32(n4.0),
                n4.1.value(),
            ],
        );
        if slot == Some(-1) {
            cvr_host::log!("[Reload] RIG SIGNATURE REJECTED (table full): which={} bones={}\n", r.f("which", -1.0), r.f("bones", -1.0));
        }
    }
}
/// The slide's lock: an empty gun's slide on its stop, the slide-release click (shared [159]) closing it.
fn slide_lock(s: &mut State, c: &Ctx) {
    let sc = c.sc;
    if sc.g("lockLocal").present() && sc.g("restLocal").present() {
        if !c.empty {
            (s.frame.slide.locked, s.frame.slide.user_closed) = (false, false);
        } else if slide_locked_now(c) && !s.frame.slide.user_closed {
            s.frame.slide.locked = true;
        }
        let click = super::belt::shared(159) > 0.5;
        let edge = click && !s.frame.slide.click_prev;
        s.frame.slide.click_prev = click;
        let sl = &s.frame.slide;
        if edge && c.empty && sl.locked && !(sl.grabbed || sl.releasing) {
            (s.frame.slide.user_closed, s.frame.slide.locked) = (true, false);
            play_snd(&c.weapon, snd_of(c, "slideRelease"));
            if sc.s("pressPose").and_then(super::reload_data::pose).is_some() {
                (s.press.t, s.press.hand) = (0.0, Some(c.holder));
            }
            if let Some(p) = s.frame.mag.pending.filter(|p| *p > 0) {
                set_mag_ammo(s, &c.weapon, p);
                s.frame.mag.pending = None;
            }
        }
        if s.frame.slide.user_closed {
            s.frame.slide.locked = false;
        }
    } else {
        s.frame.slide.locked = false;
    }
}
/// The slide-release press pose on the gun hand: in, held, out.
fn press_tick(s: &mut State, c: &Ctx) {
    if s.press.t < 0.0 {
        return;
    }
    let sc = c.sc;
    let (ti, th, to) = (sc.f("pressIn", 0.07), sc.f("pressHold", 0.08), sc.f("pressOut", 0.18));
    s.press.t += c.dt;
    let t = s.press.t;
    let a = if t < ti {
        let u = t / ti;
        u * u
    } else if t < ti + th {
        1.0
    } else {
        let u = (t - ti - th) / to;
        if u >= 1.0 {
            0.0
        } else {
            1.0 - u * u * (3.0 - 2.0 * u)
        }
    }
    .min(1.0);
    let pp = sc.s("pressPose").and_then(super::reload_data::pose);
    let Some(hand) = s.press.hand else { return };
    if t >= ti + th + to || pp.is_none() || s.fade.hand == Some(hand) {
        if s.press.on {
            clear_grip(hand);
        }
        s.press = Press { t: -1.0, hand: None, on: false };
    } else if !s.press.on {
        if let Some(pp) = &pp {
            apply_grip(hand, pp, a);
        }
        s.press.on = true;
    } else {
        native::<i32>("VRReloadFingerBlend", &[Value::Int32(hand as i32), Value::Float(a as f32)]);
    }
}
/// The weapon's own reload stage held while our slide holds an empty gun; the manual cycle; the idle slide home.
fn slide_hold(s: &mut State, c: &Ctx) {
    let sc = c.sc;
    let has_slide = c.cfg.g("slide").present();
    let now = super::belt::now();
    if has_slide && sc.g("lockLocal").present() && sc.g("restLocal").present() {
        let sl = &s.frame.slide;
        if c.empty && (sl.user_closed || sl.grabbed || sl.releasing) {
            hold_reload_stage(&c.weapon);
            s.frame.slide.tagged = true;
        } else if sl.tagged {
            push_anim_event(&c.weapon, "InterruptReload");
            s.frame.slide.tagged = false;
        }
    }
    let hold = sc.f("cycleHold", 2.8);
    let sl = &s.frame.slide;
    if has_slide && sc.b("manual") && !(sl.grabbed || sl.releasing) && (sl.fired || now - sl.fired_at.unwrap_or(-99.0) < hold) {
        drive_slide(s, c, 0.0);
        s.frame.slide.cycle_held = true;
    } else if s.frame.slide.cycle_held {
        s.frame.slide.cycle_held = false;
    }
    let hide_bones = sc.g("cycleHide").arr();
    if !hide_bones.is_empty() {
        let sl = &s.frame.slide;
        let hide = sl.fired || now - sl.fired_at.unwrap_or(-99.0) < hold;
        if hide != (sl.cycle_hid == Some(true)) {
            s.frame.slide.cycle_hid = Some(hide);
            let which = sc.f("which", 0.0) as i32;
            for bn in hide_bones {
                rig_write_scale(which, bn.as_f64().unwrap_or(0.0) as i32, if hide { 0.001 } else { 0.0 });
            }
        }
    }
    let sl = &s.frame.slide;
    if has_slide && !(sl.grabbed || sl.releasing || sl.locked || (sc.g("lockLocal").present() && sl.user_closed) || sl.cycle_held) {
        if s.frame.slide.drain > 0.0 {
            s.frame.slide.drain -= c.dt;
            drive_slide(s, c, 0.0);
        } else if !release_slide(s, c) {
            drive_slide(s, c, 0.0);
        }
    }
}
/// The slide's frame: its slot, the magazine-ward direction off the bore, the weapon's own basis when asked.
fn slide_basis(s: &mut State, c: &mut Ctx) {
    let sc = c.sc;
    c.sp = slot_model(&c.slot_comp, sc.s("slot"), c.b, c.c);
    if let Some(sp) = c.sp {
        let mslot = c.cfg.g("mag").s("slot").unwrap_or("vrp_mag_std");
        if let Some(mg) = slot_model(&c.slot_comp, Some(mslot), c.b, c.c) {
            let a = c.a;
            let dm = [mg[0] - sp[0], mg[1] - sp[1], mg[2] - sp[2]];
            let dd = dm[0] * a[0] + dm[1] * a[1] + dm[2] * a[2];
            let p = [dm[0] - dd * a[0], dm[1] - dd * a[1], dm[2] - dd * a[2]];
            let pl = len3(p);
            if pl > 1e-4 {
                let db = p.map(|v| v / pl);
                let sb = [a[1] * db[2] - a[2] * db[1], a[2] * db[0] - a[0] * db[2], a[0] * db[1] - a[1] * db[0]];
                (c.db, c.sb) = (Some(db), Some(sb));
                let u = db.map(|v| -v);
                let cz = u[0] * a[1] - u[1] * a[0];
                s.frame.slide.g_loc = Some([-cz, -u[2], -a[2]]);
            }
        }
    }
    (c.gd, c.gs) = (c.db.unwrap_or([0.0; 3]), c.sb.unwrap_or([0.0; 3]));
    if c.cfg.s("basis") == Some("weapon") {
        if let Ok(wq0) = m::<[f32; 4]>(&c.weapon, "GetWorldOrientation", &[]) {
            let w = qmul(c.c, f4(wq0));
            c.g_weapon = true;
            c.gf = qrot(w, [1.0, 0.0, 0.0]);
            c.gd = qrot(w, [0.0, 1.0, 0.0]);
            c.gs = qrot(w, [0.0, 0.0, 1.0]);
            if c.db.is_none() {
                (c.db, c.sb) = (Some(c.gd), Some(c.gs));
            }
        }
    }
}
/// The magazine's count: a manual gun's shot seen as a drop that is not ours; the chamber shown by it.
fn slide_ammo(s: &mut State, c: &Ctx) {
    let sc = c.sc;
    s.frame.slide.ammo = mag_ammo(&c.weapon);
    let now = super::belt::now();
    let sl = &mut s.frame.slide;
    if sc.b("manual") {
        if let (Some(a), Some(last)) = (sl.ammo, sl.ammo_last) {
            if a < last && s.frame.mag.state == "in" && now - sl.own_spend_at.unwrap_or(-9.0) > 0.4 {
                (sl.fired, sl.chambered, sl.fired_at) = (true, false, Some(now));
            }
        }
        if sl.ammo.is_some() {
            sl.ammo_last = sl.ammo;
        }
    }
    let sl = &s.frame.slide;
    if let (Some(a), None, None) = (sl.ammo, sl.eject_t, sl.chase_v) {
        if a <= 0 {
            if sl.chambered || sl.seated_shown != Some(false) {
                s.frame.slide.chambered = false;
                show_seated(s, c, 0.001);
            }
        } else if !sl.chambered && !sl.grabbed && sl.rack <= 0.0 && !sl.fired {
            s.frame.slide.chambered = true;
            show_seated(s, c, 1.0);
        }
    }
}
/// The slide held: let go -- the spring home; else the rack by the hand's pull along the bore.
fn slide_grabbed(s: &mut State, c: &Ctx) {
    let sc = c.sc;
    let hh = s.frame.slide.hand.unwrap_or(c.free);
    let travel = sc.f("travel", 1.0);
    if !squeeze(sc.s("grabWith"), hh) || s.frame.slide.trip {
        let sl = &mut s.frame.slide;
        (sl.grabbed, sl.releasing, sl.trip) = (false, true, false);
        if let (true, Some(rl), Some(lk)) = (sl.locked, sc.v3("restLocal"), sc.v3("lockLocal")) {
            sl.rack += len3([rl[0] - lk[0], rl[1] - lk[1], rl[2] - lk[2]]);
            sl.lock_base = None;
            sl.racked = true;
        }
        sl.catch_off = false;
        play_snd(&c.weapon, snd_of(c, "slideRelease"));
        s.frame.slide.snd_rider = false;
        fade_out(s);
        stop_model(hh, false, [0.0; 3]);
        stop_rot(hh, 0, [0.0, 0.0, 0.0, 1.0]);
        s.frame.slide.hand = None;
        return;
    }
    let Some(hg) = hand_model(hh) else { return };
    let a = c.a;
    let hr = s.frame.slide.hr;
    let pull = (hg[0] - hr[0]) * a[0] + (hg[1] - hr[1]) * a[1] + (hg[2] - hr[2]) * a[2];
    let mut rack = -pull * sc.f("pullDirection", 1.0);
    let (mut cap, mut lo, mut stop) = (travel, 0.0, 0.0);
    if let (true, Some(rl), Some(lk)) = (s.frame.slide.locked, sc.v3("restLocal"), sc.v3("lockLocal")) {
        stop = len3([rl[0] - lk[0], rl[1] - lk[1], rl[2] - lk[2]]);
        cap = sc.f("lockPull", 0.002);
        if s.frame.slide.catch_off {
            lo = -stop;
        }
    }
    rack = rack.clamp(lo, cap.max(lo));
    s.frame.slide.rack = rack;
    drive_slide(s, c, rack);
    if s.frame.slide.locked && rack >= cap {
        s.frame.slide.catch_off = true;
    }
    if s.frame.slide.catch_off || rack >= sc.f("chamberAt", 0.6) * travel {
        s.frame.slide.racked = true;
    }
    if s.frame.slide.catch_off && stop > 0.0 && rack <= -stop + 0.002 {
        let sl = &mut s.frame.slide;
        (sl.locked, sl.lock_base, sl.catch_off) = (false, None, false);
        (sl.rack, sl.user_closed) = (0.0, true);
        drive_slide(s, c, 0.0);
        play_snd(&c.weapon, snd_of(c, "slideRelease"));
        if let Some(p) = s.frame.mag.pending.filter(|p| *p > 0) {
            set_mag_ammo(s, &c.weapon, p);
            s.frame.mag.pending = None;
        }
    }
    if !s.frame.slide.moved && rack > sc.f("pullSndAt", 0.05) * travel {
        s.frame.slide.moved = true;
        play_snd(&c.weapon, snd_of(c, "slidePull"));
    }
    if !s.frame.slide.snd_rider && rack > sc.f("riderSndAt", 0.72) * travel {
        s.frame.slide.snd_rider = true;
        play_snd(&c.weapon, snd_of(c, "slideRider"));
    }
    let eject_at = sc.f("ejectAt", 0.85) * travel;
    let fx = sc.s("ejectFx");
    let has_round = sc.g("chamberRound").present();
    // the live round thrown out only while there is one, and only once the game's consume event is queued (main's
    // d495625fb); a gun with `ejectLiveRound` throws the port's own intact cartridge as a loose body
    let live = sc.g("ejectLiveRound");
    if live != &J::Bool(false) && s.frame.slide.chambered && s.frame.slide.ammo.unwrap_or(0) > 0 && rack >= eject_at && consume_ammo(&c.weapon, 1) {
        s.frame.slide.chambered = false;
        s.frame.slide.eject_t = has_round.then_some(0.0);
        s.frame.slide.spent = Some(true);
        s.frame.slide.own_spend_at = Some(super::belt::now());
        play_snd(&c.weapon, snd_of(c, "eject"));
        let mut visible = has_round;
        if live == &J::Bool(true) {
            visible = super::reload_shotgun::eject_pistol_round(s, &c.weapon, &c.slot_comp, sc.s("slot"));
        }
        if !visible {
            play_fx(&c.weapon, fx);
        }
        if visible || fx.is_some() {
            let mut cs = s.frame.slide.case_snd.take();
            case_land(s, &c.weapon, &mut cs);
            s.frame.slide.case_snd = cs;
        }
    } else if s.frame.slide.fired && rack >= eject_at {
        s.frame.slide.fired = false;
        play_snd(&c.weapon, snd_of(c, "eject"));
        play_fx(&c.weapon, fx);
        if fx.is_some() || has_round {
            let mut cs = s.frame.slide.case_snd.take();
            case_land(s, &c.weapon, &mut cs);
            s.frame.slide.case_snd = cs;
        }
    }
    if sc.b("gripSnap") {
        let Some(grip) = s.frame.slide.grip.clone() else { return };
        let t = grip_target(c, &grip, false);
        s.owned_hand = Some(hh);
        let pw = pull_in(s, "slide", c.dt);
        let mut p = t;
        if let (Some(hr), true) = (hand_model(hh), pw < 0.999) {
            p = [hr[0] + (t[0] - hr[0]) * pw, hr[1] + (t[1] - hr[1]) * pw, hr[2] + (t[2] - hr[2]) * pw];
        }
        stop_model(hh, true, p);
        if let Some(gq) = grip.v4("rot") {
            let bq = basis_quat(c.gf, c.gd, c.gs);
            let mut r = qmul(bq, gq);
            if let (Some(hq), true) = (native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(hh as i32)]).map(f4), pw < 0.999) {
                let d2 = hq[0] * r[0] + hq[1] * r[1] + hq[2] * r[2] + hq[3] * r[3];
                let sg = if d2 < 0.0 { -1.0 } else { 1.0 };
                let q = [0, 1, 2, 3].map(|i| hq[i] + (r[i] * sg - hq[i]) * pw);
                let l2 = q.iter().map(|v| v * v).sum::<f64>().sqrt();
                if l2 > 1e-6 {
                    r = q.map(|v| v / l2);
                }
            }
            stop_rot(hh, 1, r);
        }
    }
}
/// The slide free: the nearest recorded grip (turned round the part, canted with the gun) previewed and taken.
fn slide_free(s: &mut State, c: &Ctx) {
    let sc = c.sc;
    let free = c.free;
    let hx = c.h;
    let sp = c.sp.unwrap_or([0.0; 3]);
    let travel = sc.f("travel", 1.0);
    if !grip_of(free) {
        s.frame.mag.grip_lock = false;
    }
    let (mut best, mut best_style, mut best_d) = (None::<J>, "-".to_string(), 1e9f64);
    let mut dbg_per = String::new();
    let grips = sc.g("grips");
    if grips.is_object() && s.frame.mag.state != "hand" {
        let hq = native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(free as i32)]).map(f4);
        let bq = basis_quat(c.gf, c.gd, c.gs);
        let mut cant: Option<Q> = None;
        let mut cant_angle = 0.0;
        {
            let u = if c.g_weapon { c.gs } else { c.gd.map(|v| -v) };
            let a = c.a;
            let au = u[0] * a[0] + u[1] * a[1] + u[2] * a[2];
            let p = [u[0] - a[0] * au, u[1] - a[1] * au, u[2] - a[2] * au];
            let w = [-a[0] * a[2], -a[1] * a[2], 1.0 - a[2] * a[2]];
            if len3(p) > 1e-3 && len3(w) > 0.3 {
                let cr = [p[1] * w[2] - p[2] * w[1], p[2] * w[0] - p[0] * w[2], p[0] * w[1] - p[1] * w[0]];
                let phi = (cr[0] * a[0] + cr[1] * a[1] + cr[2] * a[2]).atan2(p[0] * w[0] + p[1] * w[1] + p[2] * w[2]);
                if phi.abs() > 2f64.to_radians() {
                    cant_angle = phi;
                    let sn = (phi * 0.5).sin();
                    cant = Some([a[0] * sn, a[1] * sn, a[2] * sn, (phi * 0.5).cos()]);
                }
            }
        }
        if let Some(obj) = grips.as_object() {
            for (style, recorded) in obj {
                let list = hand_grips(recorded, free, c.g_weapon, sc.s("handleSide"));
                let (mut gd, mut ga, mut g_tilt) = (None::<f64>, None::<f64>, false);
                for g in &list {
                    let t = grip_target(c, g, true);
                    let r = g.v4("rot").map(|q| qmul(bq, q));
                    // the nearest point between the rolled and the upright candidates too: the two ends alone reject a
                    // wrist midway through a 90-degree cant while it is still on the slide (main's grip_arc.lua)
                    for vi in 1..=(if cant.is_some() { 3 } else { 1 }) {
                        let (mut v, mut vq) = (t, r);
                        if vi > 1 {
                            let mut cq = cant.unwrap_or([0.0, 0.0, 0.0, 1.0]);
                            if vi == 3 {
                                let a = c.a;
                                let phi = super::reload_flick::closest(a, cant_angle, [t[0] - sp[0], t[1] - sp[1], t[2] - sp[2]], [hx[0] - sp[0], hx[1] - sp[1], hx[2] - sp[2]]);
                                let sn = (phi * 0.5).sin();
                                cq = [a[0] * sn, a[1] * sn, a[2] * sn, (phi * 0.5).cos()];
                            }
                            let o = qrot(cq, [t[0] - sp[0], t[1] - sp[1], t[2] - sp[2]]);
                            v = [sp[0] + o[0], sp[1] + o[1], sp[2] + o[2]];
                            vq = r.map(|q| qmul(cq, q));
                        }
                        let d = len3([hx[0] - v[0], hx[1] - v[1], hx[2] - v[2]]);
                        if gd.is_none_or(|x| d < x) {
                            (gd, g_tilt) = (Some(d), vi > 1);
                        }
                        if d < best_d {
                            let mut ang_ok = true;
                            if let (Some(hq), Some(vq)) = (hq, vq) {
                                let dq = (hq[0] * vq[0] + hq[1] * vq[1] + hq[2] * vq[2] + hq[3] * vq[3]).abs().min(1.0);
                                let an = 2.0 * dq.acos().to_degrees();
                                if ga.is_none_or(|x| an < x) {
                                    ga = Some(an);
                                }
                                ang_ok = an <= sc.f("styleAngleMax", 60.0);
                            }
                            if ang_ok {
                                (best, best_style, best_d) = (Some(g.clone()), style.clone(), d);
                            }
                        }
                    }
                }
                if let Some(gd) = gd {
                    dbg_per.push_str(&format!(" {style}{}:d={gd:.3}", if g_tilt { "~" } else { "" }));
                    if let Some(ga) = ga {
                        dbg_per.push_str(&format!("/a={ga:.0}"));
                    }
                }
            }
        }
    }
    let have_best = best_d < 1e9;
    s.frame.slide.dist = if have_best { best_d } else { -1.0 };
    s.frame.slide.why = format!(
        "grips={} magSt={} wb={} best={:.3}{}",
        if grips.is_object() { "Y" } else { "n" },
        s.frame.mag.state,
        if c.g_weapon { "Y" } else { "n" },
        if have_best { best_d } else { -1.0 },
        if dbg_per.is_empty() { " none".to_string() } else { dbg_per }
    );
    if best.is_some() {
        s.frame.slide.style = best_style;
    }
    let mut md = s.frame.mag.snap_d.unwrap_or(-1.0);
    let grab_with = sc.s("grabWith");
    if grab_with == Some("trigger") && s.frame.mag.snap_dpre.unwrap_or(-1.0) >= 0.0 {
        md = s.frame.mag.snap_dpre.unwrap_or(-1.0);
    }
    let mag_nearer = md >= 0.0 && md < best_d;
    // the two-hand hold owns the support hand: no slide preview or grab under it (main's 87693549b)
    let two_hand_owns_support = native::<bool>("GetVRTwoHandActive", &[]) == Some(true);
    if two_hand_owns_support && s.frame.mag.slide_has_pose {
        fade_out(s);
        s.frame.mag.slide_has_pose = false;
    }
    let prev_r = sc.f("previewRadius", 0.05);
    s.frame.slide.lt_near = grab_with == Some("trigger") && free == aim_hand() && (s.frame.slide.grabbed || (best.is_some() && best_d <= prev_r));
    match &best {
        Some(g) if best_d <= prev_r && !mag_nearer && !two_hand_owns_support => {
            let touch = f64::from(super::belt::shared(if free == 0 { 154 } else { 160 }));
            if !(grab_with == Some("trigger") && touch <= sc.f("touchAt", 0.08)) {
                fade_in(s, free, g.s("pose"));
                s.frame.mag.slide_has_pose = true;
                let gt = grip_target(c, g, true);
                reach_mark(free, gt, Some(hx));
            }
            if squeeze(grab_with, free) && (grab_with == Some("trigger") || !s.frame.mag.grip_lock) {
                // a shot may have been seen earlier in this same tick: the stroke chosen now is kept
                let base: &'static J = if c.cfg.g("slide").present() { c.cfg.g("slide") } else { &EMPTY };
                let stroke = slide_stroke(base, &mut s.frame.slide, &s.frame.mag);
                let sl = &mut s.frame.slide;
                (sl.grabbed, sl.releasing) = (true, false);
                sl.active_stroke = Some(stroke);
                sl.hand = Some(free);
                sl.grip = Some(g.clone());
                sl.hr = hx;
                if grab_with != Some("trigger") {
                    s.frame.mag.grip_lock = true;
                }
                let sl = &mut s.frame.slide;
                sl.drain = 0.0;
                sl.lock_base = if sl.locked { sc.v3("lockLocal") } else { None };
                if w0_is_empty(&c.weapon) && !sl.tagged {
                    push_anim_event(&c.weapon, "Reload");
                    s.frame.slide.tagged = true;
                }
                drive_slide(s, c, 0.0);
            }
        }
        _ => {
            if s.frame.mag.slide_has_pose {
                fade_out(s);
                s.frame.mag.slide_has_pose = false;
            }
        }
    }
    if s.frame.slide.releasing {
        s.frame.slide.rack -= sc.f("releaseSpeed", 1.5) * c.dt;
        if s.frame.slide.rack <= 0.0 {
            (s.frame.slide.rack, s.frame.slide.releasing) = (0.0, false);
            let dr = sc.g("riders").arr().iter().map(|rd| rd.fo("delayHome").or(rd.fo("delay")).unwrap_or(0.0)).fold(0.0, f64::max);
            s.frame.slide.drain = if dr > 0.0 { dr + 0.05 } else { 0.0 };
            if sc.g("lockLocal").present() {
                s.frame.slide.user_closed = true;
            }
            if s.frame.slide.locked {
                (s.frame.slide.locked, s.frame.slide.lock_base) = (false, None);
                play_snd(&c.weapon, snd_of(c, "slideRelease"));
            }
            if let (Some(p), true) = (s.frame.mag.pending.filter(|p| *p > 0), s.frame.slide.racked) {
                set_mag_ammo(s, &c.weapon, p);
                s.frame.mag.pending = None;
            }
            s.frame.slide.racked = false;
            drive_slide(s, c, 0.0);
            if c.cfg.g("rotator").present() && s.frame.slide.moved {
                s.frame.rot_t = 0.0;
            }
            s.frame.slide.moved = false;
        } else {
            let r = s.frame.slide.rack;
            drive_slide(s, c, r);
        }
    } else if s.frame.rot_t >= 0.0 {
        drive_slide(s, c, 0.0);
    }
    let sl = &s.frame.slide;
    if !sl.chambered && sl.eject_t.is_none() && !sl.grabbed && !sl.fired && sl.rack < 0.5 * travel && sl.ammo.unwrap_or(0) > 0 {
        s.frame.slide.chambered = true;
        show_seated(s, c, 1.0);
        s.frame.slide.chase_v = sc.g("chamberRound").present().then(|| travel * sc.f("sign", 1.0));
    }
}
/// The chamber round: the ejected one flung and tumbling, the next one chasing home behind the slide.
fn chamber_round(s: &mut State, c: &Ctx) {
    let sc = c.sc;
    let cr = sc.g("chamberRound");
    if !cr.present() {
        return;
    }
    let which = sc.f("which", 0.0) as i32;
    let bones: Vec<i32> = cr.g("bones").arr().iter().map(|b| b.as_f64().unwrap_or(0.0) as i32).collect();
    if let Some(t) = s.frame.slide.eject_t {
        let t2 = t + c.dt;
        s.frame.slide.eject_t = Some(t2);
        if t2 >= cr.f("flingTime", 0.2) {
            s.frame.slide.eject_t = None;
            show_seated(s, c, 0.001);
            release_round_pose(s, which, &bones);
        } else {
            let v0 = cr.g("v0");
            let (side, up, back) = (v0.f("side", 1.1), v0.f("up", 1.9), v0.f("back", 0.5));
            let g = s.frame.slide.g_loc.unwrap_or([0.0, -1.0, 0.0]);
            let gk = 0.5 * cr.f("gravity", 9.81) * t2 * t2;
            let p = [side * t2 + g[0] * gk, up * t2 + g[1] * gk, -back * t2 + g[2] * gk];
            let ang = cr.f("spin", 900.0) * t2;
            for b in &bones {
                rig_write(which, *b, p);
                rig_write_rot(which, *b, ang, [1.0, 0.0, 0.0]);
            }
        }
    }
    if let Some(cv) = s.frame.slide.chase_v {
        let d3 = s.frame.slide.rack * sc.f("sign", 1.0);
        let cv = cv + (d3 - cv) * (1.0 - (-c.dt / cr.f("chaseLag", 0.08)).exp());
        s.frame.slide.chase_v = Some(cv);
        let rv = cr.v3("rideVec").unwrap_or(c.la);
        for b in &bones {
            rig_write(which, *b, rv.map(|v| v * cv));
        }
        if cv.abs() < 0.1 * sc.f("travel", 1.0) {
            s.frame.slide.chase_v = None;
            release_round_pose(s, which, &bones);
        }
    }
}
