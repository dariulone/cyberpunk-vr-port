//! reload/shotgun.lua: the shotguns -- a break-action (the top lever thumbed by R3, the barrels dropping open on
//! their hinge, the spent cases sliding out under gravity and the gun's own acceleration, shells from the belt or the
//! body taken one or two at a time and magnetised into the empty chambers, a flick of the wrist shutting it) and a pump
//! (the fore-end worked by the free hand's grip with the trigger, or the two-hand hold, ejecting and chambering on its
//! stroke; shells, single or a recorded handful rolled forward between the fingers, fed up the tube's loading gate or
//! into the open port). Spent cases and thrown shells fall as small bodies with their own contacts.
use super::belt::{conj, f3, f4, m, native, player, qmul, qrot, Q, V3};
use super::reload_core::*;
use super::reload_data::{self as data, Lua, Pose};
use crate::script_call::Value;
use red4ext_abi::handle::Handle;
use serde_json::Value as J;

const G: f64 = 9.81;
type F = [f64; 7];
const ID: Q = [0.0, 0.0, 0.0, 1.0];

fn fr(p: V3, q: Q) -> F {
    [p[0], p[1], p[2], q[0], q[1], q[2], q[3]]
}
fn fp(f: &F) -> V3 {
    [f[0], f[1], f[2]]
}
fn fq(f: &F) -> Q {
    [f[3], f[4], f[5], f[6]]
}
fn fcomp(a: &F, b: &F) -> F {
    let x = qrot(fq(a), fp(b));
    fr([a[0] + x[0], a[1] + x[1], a[2] + x[2]], qmul(fq(a), fq(b)))
}
fn finv(a: &F) -> F {
    let c = conj(fq(a));
    fr(qrot(c, [-a[0], -a[1], -a[2]]), c)
}
/// The entity's transform that puts its bind point `bp` (turned by `bq`) on the frame.
fn unbind(f: &F, bp: V3, bq: Q) -> F {
    let q = qmul(fq(f), conj(bq));
    let x = qrot(q, bp);
    fr([f[0] - x[0], f[1] - x[1], f[2] - x[2]], q)
}
fn wrist_for(sl: &F, rel: &F) -> F {
    let q = qmul(fq(sl), conj(fq(rel)));
    let x = qrot(q, fp(rel));
    fr([sl[0] - x[0], sl[1] - x[1], sl[2] - x[2]], q)
}
fn to_model(pw: &F, a: &F) -> F {
    let c = conj(fq(pw));
    fr(qrot(c, [a[0] - pw[0], a[1] - pw[1], a[2] - pw[2]]), qmul(c, fq(a)))
}
fn len3v(x: f64, y: f64, z: f64) -> f64 {
    (x * x + y * y + z * z).sqrt()
}
fn qnorm(q: Q) -> Q {
    let l = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    if l < 1e-9 {
        return ID;
    }
    q.map(|v| v / l)
}
/// The shortest turn from `a` to `b` (unit vectors).
fn qswing(a: V3, b: V3) -> Q {
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    if d < -0.999999 {
        let p = if a[0].abs() > 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
        let c = [a[1] * p[2] - a[2] * p[1], a[2] * p[0] - a[0] * p[2], a[0] * p[1] - a[1] * p[0]];
        let l = len3(c);
        return [c[0] / l, c[1] / l, c[2] / l, 0.0];
    }
    qnorm([a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0], 1.0 + d])
}
fn qnlerp(a: Q, b: Q, t: f64) -> Q {
    let s = if a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3] < 0.0 { -1.0 } else { 1.0 };
    qnorm([0, 1, 2, 3].map(|i| a[i] + (b[i] * s - a[i]) * t))
}
fn smooth(u: f64) -> f64 {
    if u <= 0.0 {
        0.0
    } else if u >= 1.0 {
        1.0
    } else {
        u * u * (3.0 - 2.0 * u)
    }
}
fn jf7(j: &J) -> Option<F> {
    let a = j.as_array()?;
    let mut f = [0.0; 7];
    for (k, v) in f.iter_mut().enumerate() {
        *v = a.get(k)?.as_f64()?;
    }
    Some(f)
}
fn hand_slot(side: usize) -> &'static str {
    if side == 0 {
        "AttachmentSlots.WeaponLeft"
    } else {
        "AttachmentSlots.WeaponRight"
    }
}
/// The player's or the weapon's frame in the world.
fn world_of(o: &Handle) -> Option<F> {
    let p = m::<[f32; 4]>(o, "GetWorldPosition", &[]).ok()?;
    let q = m::<[f32; 4]>(o, "GetWorldOrientation", &[]).ok()?;
    Some(fr(f3(p), f4(q)))
}
fn slot_frame(slot_comp: &Handle, name: &str) -> Option<F> {
    slot_world(slot_comp, name).map(|(p, q)| fr(p, q))
}
fn wrist_raw(side: usize) -> Option<F> {
    let p = raw_hand(side).filter(|p| p[3] >= 0.5)?;
    let q = native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(side as i32)])?;
    Some(fr(f3(p), f4(q)))
}
/// The hand slot in the wrist's frame.
fn wrist_to_slot(side: usize) -> Option<F> {
    let (bw, bs) = if side == 0 { (23, 26) } else { (24, 28) };
    let (pw, ps) = (bone_pos(bw)?, bone_pos(bs)?);
    let (qw, qs) = (bone_rot(bw)?, bone_rot(bs)?);
    if pw[3] < 0.5 || ps[3] < 0.5 {
        return None;
    }
    let c = conj(qw);
    Some(fr(qrot(c, [f64::from(ps[0] - pw[0]), f64::from(ps[1] - pw[1]), f64::from(ps[2] - pw[2])]), qmul(c, qs)))
}
fn hold_frame(t: &J) -> F {
    fr(t.v3("off").unwrap_or([0.0; 3]), t.v4("rot").unwrap_or(ID))
}
fn write_off(which: i32, bone: i32) {
    rig_write_off(which, bone);
}
fn track_set(which: i32, idx: i32, v: f64) {
    track_write(which, idx, v, 1);
}
fn track_free(which: i32, idx: i32) {
    track_write(which, idx, 1.0, 0);
}
fn ji(j: &J, k: &str, d: i32) -> i32 {
    j.f(k, f64::from(d)) as i32
}

// ---------------------------------------------------------------- the module's state
struct Body {
    id: Option<u64>,
    bind: V3,
    bq: Q,
    half: f64,
    c: V3,
    q: Q,
    v: V3,
    w: f64,
    wa: V3,
    t: f64,
    life: f64,
    rest: bool,
    hits: u32,
    lie_q: Q,
    lie_from: Q,
    lie_t: Option<f64>,
}
#[derive(Default, Clone)]
struct Chamber {
    st: &'static str,
    slide: f64,
    sv: f64,
    dbg: Option<String>,
    hide_next: bool,
}
struct HeldShell {
    b_part: bool,
    hold: F,
    fm: F,
    f: Option<F>,
}
struct Held {
    side: usize,
    list: Vec<HeldShell>,
    shown: [Option<bool>; 2],
    pose: Option<&'static str>,
    world: bool,
    v: V3,
    px: Option<V3>,
    parts: Option<[Handle; 2]>,
}
#[derive(Clone, Copy)]
struct BMag {
    si: usize,
    k: usize,
    d: f64,
    perp: f64,
    i: V3,
    a: V3,
    w: f64,
}
struct BNear {
    k: usize,
    d: f64,
    perp: f64,
    ang: f64,
}
#[derive(Default)]
struct Brk {
    ch: Vec<Chamber>,
    wrote: bool,
    held: Option<Held>,
    magnet: Option<BMag>,
    near: Option<BNear>,
    thumb_on: bool,
    thumb_t: Option<f64>,
    act: &'static str,
    t: f64,
    hinge: f64,
    hv: f64,
    lever: f64,
    q_prev: Option<Q>,
    weapon: Handle,
    holder: Option<usize>,
    ammo_last: Option<i32>,
    r3_prev: bool,
    settle: f64,
    lever_snd: bool,
    bounces: i32,
    flick_n: i32,
    close_from: Option<f64>,
    flick_at: f64,
    lever_kick: Option<f64>,
    cpos: Option<V3>,
    cvel: Option<V3>,
    cacc: Option<V3>,
    cm: Vec<F>,
    grip_prev: bool,
    release_t: Option<f64>,
    tagged: bool,
}
struct Multi {
    side: usize,
    n: usize,
    list: Vec<usize>,
    shown: [Option<bool>; 4],
    off: f64,
    state: &'static str,
    t: f64,
    u: f64,
    gone: f64,
    line: Option<&'static J>,
    seat: Option<V3>,
    pin_t: Option<F>,
    parts: Option<[Option<Handle>; 4]>,
}
impl Multi {
    fn new(side: usize, n: usize) -> Multi {
        Multi { side, n, list: (1..=n).collect(), shown: [None; 4], off: 0.0, state: "ready", t: 0.0, u: 0.0, gone: 0.0, line: None, seat: None, pin_t: None, parts: None }
    }
}
struct Push {
    t: f64,
    dur: f64,
    u0: f64,
    u1: f64,
    line: &'static J,
    mode: &'static str,
}
struct Shell {
    side: usize,
    hold: F,
    multi: Option<Multi>,
    sl: Option<F>,
    f: Option<F>,
    v: V3,
    px: Option<V3>,
    push: Option<Push>,
}
struct PMag {
    u: f64,
    perp: f64,
    w: f64,
    mode: &'static str,
}
struct PNear {
    u: f64,
    perp: f64,
    ang: f64,
    mode: &'static str,
}
#[derive(Default)]
struct Pump {
    s: f64,
    engaged: bool,
    back: bool,
    gate: f64,
    gate_t: Option<f64>,
    gate_want: Option<f64>,
    gate_on: bool,
    magnet: Option<PMag>,
    near: Option<PNear>,
    chambered: bool,
    spent_in: bool,
    ammo_last: Option<i32>,
    our_add: bool,
    wrote: bool,
    shell: Option<Shell>,
    cyc_held: Option<i32>,
    wpos: Option<V3>,
    wvel: Option<V3>,
    grip_prev: bool,
    along0: f64,
    s0: f64,
    via_hold: bool,
    wt: Option<F>,
    pw: Option<F>,
}
#[derive(Default)]
struct Fin {
    hand: Option<usize>,
    key: Option<String>,
    a: f64,
    dir: i32,
}
struct Preview {
    sc: &'static J,
    m: Multi,
    side: usize,
    t: f64,
    hold: f64,
    phase: &'static str,
}
#[derive(Default)]
pub(super) struct Shotgun {
    key: Option<u64>,
    cfg: Option<&'static J>,
    kind: Option<&'static str>,
    trg: i32,
    owned: Option<usize>,
    diag: String,
    flick_peak: f64,
    pin_side: Option<usize>,
    spring: (bool, f64, f64),
    fin: Fin,
    bodies: Vec<Body>,
    b: Brk,
    p: Pump,
    pv: Option<Preview>,
}
fn sounds(cfg: &'static J) -> &'static J {
    let sc = cfg.g("shotgun");
    if sc.g("sounds").present() {
        sc.g("sounds")
    } else {
        cfg.g("sounds")
    }
}

// ---------------------------------------------------------------- the free wrist pinned onto a target
fn unpin(g: &mut Shotgun) {
    let Some(s) = g.pin_side.take() else { return };
    stop_model(s, false, [0.0; 3]);
    stop_rot(s, 0, ID);
}
fn pin(g: &mut Shotgun, side: usize, w: f64, wr: Option<&F>, t: Option<&F>) -> Option<usize> {
    let (Some(wr), Some(t), true) = (wr, t, w > 0.0) else {
        unpin(g);
        return None;
    };
    if g.pin_side.is_some_and(|p| p != side) {
        unpin(g);
    }
    let x = [0, 1, 2].map(|k| wr[k] + (t[k] - wr[k]) * w);
    let q = qnlerp(fq(wr), fq(t), w);
    stop_model(side, true, x);
    stop_rot(side, 1, q);
    g.pin_side = Some(side);
    Some(side)
}
/// The pull onto the fore-end: a critically damped spring to 1 over ~180 ms.
fn spring(g: &mut Shotgun, dt: f64) -> f64 {
    if !g.spring.0 {
        g.spring = (true, 0.0, 0.0);
    }
    let wn = 6.283185307 / 0.180;
    let d = dt.clamp(0.0, 0.1);
    let n = ((d / 0.002).floor() as i32 + 1).min(64);
    let h = d / f64::from(n);
    let (_, mut x, mut v) = g.spring;
    for _ in 0..n {
        v += (-wn * wn * (x - 1.0) - 2.0 * 0.75 * wn * v) * h;
        x += v * h;
    }
    x = x.clamp(0.0, 1.0);
    g.spring = (true, x, v);
    x
}

// ---------------------------------------------------------------- the free hand's fingers
fn fin_want_table(g: &mut Shotgun, hand: usize, key: &str, pose: Option<&Pose>) -> bool {
    let Some(pose) = pose else { return false };
    if g.fin.hand != Some(hand) || g.fin.key.as_deref() != Some(key) {
        if let Some(h) = g.fin.hand.filter(|h| *h != hand) {
            clear_grip(h);
            g.fin.a = 0.0;
        }
        apply_grip(hand, pose, g.fin.a);
        (g.fin.hand, g.fin.key) = (Some(hand), Some(key.to_string()));
    }
    g.fin.dir = 1;
    true
}
fn fin_want(g: &mut Shotgun, hand: usize, key: Option<&str>) {
    let Some(key) = key else { return };
    let pose = data::pose(key);
    fin_want_table(g, hand, key, pose.as_ref());
}
fn fin_off(g: &mut Shotgun) {
    if g.fin.hand.is_some() {
        g.fin.dir = -1;
    }
}
fn fin_reset(g: &mut Shotgun) {
    if let Some(h) = g.fin.hand {
        clear_grip(h);
    }
    g.fin = Fin::default();
}
fn fin_tick(g: &mut Shotgun, dt: f64, t: f64) {
    let Some(h) = g.fin.hand else { return };
    if g.fin.dir == 0 {
        return;
    }
    g.fin.a += f64::from(g.fin.dir) * dt / t;
    if g.fin.a >= 1.0 {
        (g.fin.a, g.fin.dir) = (1.0, 0);
    } else if g.fin.a <= 0.0 {
        fin_reset(g);
        return;
    }
    native::<i32>("VRReloadFingerBlend", &[Value::Int32(h as i32), Value::Float(g.fin.a as f32)]);
}

// ---------------------------------------------------------------- the falling bodies (cases, thrown shells)
const MAXB: usize = 12;
const B_RADIUS: f64 = 0.012;
const B_BOUNCE: f64 = 0.28;
const B_FRICTION: f64 = 0.55;
const B_REST_V: f64 = 0.35;
const B_SETTLE: f64 = 0.20;
fn body_despawn(s: &mut State, b: &mut Body) {
    if let Some(id) = b.id.take() {
        if !despawn(id) {
            s.litter.push(id);
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn body_spawn(s: &mut State, g: &mut Shotgun, ent: Option<&str>, bind: V3, bq: Q, len: Option<f64>, f: &F, v: V3, w: f64, wa: Option<V3>) -> bool {
    let Some(ent) = ent else { return false };
    if g.bodies.len() >= MAXB {
        let mut b0 = g.bodies.remove(0);
        body_despawn(s, &mut b0);
    }
    let t = unbind(f, bind, bq);
    let Some(id) = spawn_at(ent, fp(&t), fq(&t)) else { return false };
    let half = len.unwrap_or(0.07) * 0.5;
    let c = qrot(fq(f), [0.0, 0.0, half]);
    g.bodies.push(Body {
        id: Some(id),
        bind,
        bq,
        half,
        c: [f[0] + c[0], f[1] + c[1], f[2] + c[2]],
        q: fq(f),
        v,
        w,
        wa: wa.unwrap_or([1.0, 0.0, 0.0]),
        t: 0.0,
        life: 7.0,
        rest: false,
        hits: 0,
        lie_q: ID,
        lie_from: ID,
        lie_t: None,
    });
    true
}
/// Every loose body despawned (main's M.clearLooseRounds).
fn body_clear(s: &mut State, g: &mut Shotgun) {
    for mut b in std::mem::take(&mut g.bodies) {
        body_despawn(s, &mut b);
    }
}
pub(super) fn clear_loose_rounds(s: &mut State) {
    with(s, body_clear);
}
/// The port's intact cartridge for a manual pistol rack (main's d495625fb): its mesh bind the inverse of the Overture
/// profile's meshRot; the loose-body solver follows the rim. False when the port or the gun cannot be placed.
const ROUND_BIND: V3 = [0.0, 0.0, 0.0134];
const ROUND_ROT: Q = [-std::f64::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f64::consts::FRAC_1_SQRT_2];
pub(super) fn eject_pistol_round(s: &mut State, weapon: &Handle, slots: &Handle, slide_slot: Option<&str>) -> bool {
    let frame = slot_frame(slots, "fx_ejection_port").or_else(|| slot_frame(slots, "vrp_chamber")).or_else(|| slide_slot.and_then(|n| slot_frame(slots, n)));
    let (Some(mut frame), Some(gun)) = (frame, world_of(weapon)) else { return false };
    let gq = fq(&gun);
    let q = qmul(gq, ROUND_ROT);
    frame[3..7].copy_from_slice(&q);
    let v = qrot(gq, [1.1, -0.5, 1.9]);
    let axis = qrot(gq, [1.0, 0.0, 0.0]);
    with(s, |s, g| body_spawn(s, g, Some("base\\vrport\\vrp_round_overture.ent"), ROUND_BIND, ROUND_ROT, Some(0.045), &frame, v, 14.0, Some(axis)))
}
fn body_place(b: &Body) {
    let Some(id) = b.id else { return };
    let h = qrot(b.q, [0.0, 0.0, b.half]);
    let f = fr([b.c[0] - h[0], b.c[1] - h[1], b.c[2] - h[2]], b.q);
    let t = unbind(&f, b.bind, b.bq);
    place_id(id, fp(&t), fq(&t));
}
fn body_tick(s: &mut State, g: &mut Shotgun, dt: f64) {
    let mut n = g.bodies.len();
    while n > 0 {
        n -= 1;
        g.bodies[n].t += dt;
        if g.bodies[n].id.is_none() || g.bodies[n].t >= g.bodies[n].life {
            let mut b = g.bodies.remove(n);
            body_despawn(s, &mut b);
            continue;
        }
        if !g.bodies[n].rest {
            let (p, mut nx) = {
                let b = &mut g.bodies[n];
                let p = b.c;
                b.v[2] -= G * dt;
                let nx = [p[0] + b.v[0] * dt, p[1] + b.v[1] * dt, p[2] + b.v[2] * dt];
                if b.w != 0.0 {
                    let h = b.w * dt * 0.5;
                    let sn = h.sin();
                    b.q = qnorm(qmul([b.wa[0] * sn, b.wa[1] * sn, b.wa[2] * sn, h.cos()], b.q));
                }
                (p, nx)
            };
            let hit = ray_hit(s, [p[0], p[1], p[2] + 0.02], [nx[0], nx[1], nx[2] - B_RADIUS]);
            let b = &mut g.bodies[n];
            if let Some((hp, nrm)) = hit {
                b.hits += 1;
                nx = [0, 1, 2].map(|k| hp[k] + nrm[k] * B_RADIUS);
                let vn = b.v[0] * nrm[0] + b.v[1] * nrm[1] + b.v[2] * nrm[2];
                if vn < 0.0 {
                    let t = [0, 1, 2].map(|k| b.v[k] - vn * nrm[k]);
                    let e = if -vn > 0.6 { B_BOUNCE } else { 0.0 };
                    b.v = [0, 1, 2].map(|k| t[k] * (1.0 - B_FRICTION) - vn * e * nrm[k]);
                }
                b.w *= 0.45;
                if len3(b.v) < B_REST_V {
                    let a = qrot(b.q, [0.0, 0.0, 1.0]);
                    let d = a[0] * nrm[0] + a[1] * nrm[1] + a[2] * nrm[2];
                    let mut fv = [a[0] - d * nrm[0], a[1] - d * nrm[1], a[2] - d * nrm[2]];
                    let mut fl = len3(fv);
                    if fl < 1e-4 {
                        (fv, fl) = ([1.0, 0.0, 0.0], 1.0);
                    }
                    let sw = qswing(a, fv.map(|v| v / fl));
                    b.lie_q = qnorm(qmul(sw, b.q));
                    b.lie_from = b.q;
                    b.lie_t = Some(0.0);
                    b.rest = true;
                    b.v = [0.0; 3];
                }
            }
            b.c = nx;
            body_place(b);
        } else if let Some(lt) = g.bodies[n].lie_t.filter(|t| *t < B_SETTLE) {
            let b = &mut g.bodies[n];
            let lt = lt + dt;
            b.lie_t = Some(lt);
            b.q = qnlerp(b.lie_from, b.lie_q, smooth(lt / B_SETTLE));
            body_place(b);
        }
    }
}

// ---------------------------------------------------------------- the break-action
fn b_chambers_from_ammo(g: &mut Shotgun, sc: &'static J, n: i32) {
    let count = sc.g("chambers").arr().len();
    g.b.ch.resize(count, Chamber { st: "spent", ..Default::default() });
    let mut live = n;
    for k in (0..count).rev() {
        let c = &mut g.b.ch[k];
        if live > 0 {
            c.st = "live";
            live -= 1;
        } else {
            c.st = "spent";
        }
        (c.slide, c.sv) = (0.0, 0.0);
    }
}
fn b_release(g: &mut Shotgun, sc: &'static J) {
    let fw = ji(sc, "frameWhich", 1);
    write_off(fw, ji(sc.g("hinge"), "bone", 0));
    let lv = sc.g("lever");
    if lv.present() {
        write_off(ji(lv, "which", 2), ji(lv, "bone", 0));
    }
    for rd in sc.g("riders").arr() {
        write_off(ji(rd, "which", fw), ji(rd, "bone", 0));
    }
    let mw = ji(sc, "magWhich", 0);
    for c in sc.g("chambers").arr() {
        track_free(mw, ji(c.g("live"), "track", 0));
        if c.g("spent").present() {
            track_free(mw, ji(c.g("spent"), "track", 0));
            write_off(mw, ji(c.g("spent"), "bone", 0));
        }
    }
    g.b.wrote = false;
}
fn chamber1_pos(sc: &'static J) -> V3 {
    sc.g("chambers").i(1).v3("pos").unwrap_or([0.0; 3])
}
fn b_held_drop(s: &mut State, g: &mut Shotgun, sc: &'static J, throw: bool) {
    let Some(h) = g.b.held.take() else { return };
    if throw && h.world {
        let bind = sc.v3("roundBind").unwrap_or_else(|| chamber1_pos(sc));
        let bq = sc.v4("bindRot").unwrap_or(ID);
        for sh in &h.list {
            if let Some(f) = sh.f {
                body_spawn(s, g, sc.s("roundEntity"), bind, bq, sc.fo("shellLen"), &f, h.v, 0.0, None);
            }
        }
    }
    item_drop(s, h.side, hand_slot(h.side));
    (g.b.magnet, g.b.near) = (None, None);
    unpin(g);
    fin_off(g);
    belt_mag_taken(false);
}
fn b_reset_all(s: &mut State, g: &mut Shotgun, sc: &'static J, weapon: Option<&Handle>) {
    if g.b.wrote {
        b_release(g, sc);
    }
    if g.b.held.is_some() {
        b_held_drop(s, g, sc, false);
    }
    g.b.held = None;
    if g.b.thumb_on {
        if let Some(h) = g.b.holder {
            clear_grip(h);
        }
    }
    (g.b.thumb_on, g.b.thumb_t) = (false, None);
    (g.b.act, g.b.t, g.b.hinge, g.b.hv, g.b.lever) = ("closed", 0.0, 0.0, 0.0, 0.0);
    g.b.wrote = false;
    g.b.q_prev = None;
    g.b.magnet = None;
    if let Some(w) = weapon {
        let n = mag_ammo(w).unwrap_or(0);
        b_chambers_from_ammo(g, sc, n);
    }
}
/// The thumb on the top lever: reach over, press, hold while the barrels drop, away.
fn b_thumb(g: &mut Shotgun, sc: &'static J, holder: usize, dt: f64) {
    let th = sc.g("thumb");
    let (true, Some(tt)) = (th.present(), g.b.thumb_t) else { return };
    let t = tt + dt;
    g.b.thumb_t = Some(t);
    let (rt, pt, ht, at) = (th.f("reach", 0.10), th.f("pressTime", 0.08), th.f("hold", 0.30), th.f("away", 0.18));
    let key = [(0.0, "over", 0.0), (rt, "over", 1.0), (rt + pt, "press", 1.0), (rt + pt + ht, "press", 1.0), (rt + pt + ht + at, "press", 0.0)];
    if t >= key[key.len() - 1].0 {
        if g.b.thumb_on {
            clear_grip(holder);
        }
        (g.b.thumb_on, g.b.thumb_t) = (false, None);
        return;
    }
    let (mut a, mut b, mut u) = (key[0], key[1], 0.0);
    for k in 0..key.len() - 1 {
        if t >= key[k].0 && t <= key[k + 1].0 {
            (a, b) = (key[k], key[k + 1]);
            let span = b.0 - a.0;
            u = if span > 1e-6 { (t - a.0) / span } else { 0.0 };
            break;
        }
    }
    let us = smooth(u);
    let pa = th.s(a.1).and_then(data::pose);
    let pb = th.s(b.1).and_then(data::pose);
    if let Some(pose) = mix_pose(pa.as_ref(), pb.as_ref(), us) {
        apply_grip(holder, &pose, a.2 + (b.2 - a.2) * us);
        g.b.thumb_on = true;
    }
}
/// The gun's pitch rate about its own x axis (degrees per second, up positive).
fn b_pitch_rate(g: &mut Shotgun, wm: &F, dt: f64) -> f64 {
    let q = fq(wm);
    let mut rate = 0.0;
    if let (Some(p), true) = (g.b.q_prev, dt > 1e-4) {
        let s = if q[0] * p[0] + q[1] * p[1] + q[2] * p[2] + q[3] * p[3] < 0.0 { -1.0 } else { 1.0 };
        let d = qmul(q.map(|v| v * s), conj(p));
        let vn = len3([d[0], d[1], d[2]]);
        if vn > 1e-7 {
            let mut ang = 2.0 * vn.atan2(d[3]);
            if ang > std::f64::consts::PI {
                ang -= 2.0 * std::f64::consts::PI;
            }
            let hx = qrot(q, [1.0, 0.0, 0.0]);
            rate = (ang / dt).to_degrees() * ((d[0] * hx[0] + d[1] * hx[1] + d[2] * hx[2]) / vn);
        }
    }
    g.b.q_prev = Some(q);
    rate
}
/// A spent case in an open chamber: held by the extractor, then sliding out under gravity and the gun's motion.
#[allow(clippy::too_many_arguments)]
fn b_case_tick(s: &mut State, g: &mut Shotgun, sc: &'static J, snd: &'static J, k: usize, cw: &F, dt: f64, acc: Option<V3>, open: f64) {
    if g.b.ch[k].st != "spent" {
        return;
    }
    let ej = sc.g("eject");
    let l = sc.fo("spentLen").or(sc.fo("shellLen")).unwrap_or(0.08);
    let lift = ej.f("extract", 0.008) * ((open - 0.5) * 2.0).clamp(0.0, 1.0);
    let c = &mut g.b.ch[k];
    if c.slide < lift {
        c.slide = lift;
        if c.sv < 0.0 {
            c.sv = 0.0;
        }
    }
    let o = qrot(fq(cw), [0.0, 0.0, -1.0]);
    let mut gv = [0.0, 0.0, -G];
    let ki = ej.f("inertia", 1.0);
    if let Some(a) = acc {
        gv = [gv[0] - a[0] * ki, gv[1] - a[1] * ki, gv[2] - a[2] * ki];
    }
    let along = gv[0] * o[0] + gv[1] * o[1] + gv[2] * o[2];
    let pv = [gv[0] - along * o[0], gv[1] - along * o[1], gv[2] - along * o[2]];
    let nrm = len3(pv);
    let (mu_k, mu_s) = (ej.f("mu", 0.30), ej.f("muStatic", 0.40));
    let mut sv = c.sv;
    let mut a;
    if sv > 1e-3 {
        a = along - mu_k * nrm;
    } else if sv < -1e-3 {
        a = along + mu_k * nrm;
    } else if along > mu_s * nrm {
        a = along - mu_k * nrm;
    } else if along < -mu_s * nrm && c.slide > lift + 1e-4 {
        a = along + mu_k * nrm;
    } else {
        (a, sv) = (0.0, 0.0);
    }
    if open < 0.98 {
        a = a.min(0.0);
    }
    sv += a * dt;
    c.slide += sv * dt;
    if c.slide <= lift {
        c.slide = lift;
        if sv < 0.0 {
            sv = 0.0;
        }
    }
    c.sv = sv;
    c.dbg = Some(format!("{:.0}mm a={a:+.1}", c.slide * 1000.0));
    if c.slide >= l {
        let f = fcomp(cw, &fr([0.0, 0.0, -c.slide], ID));
        let cv = g.b.cvel.unwrap_or([0.0; 3]);
        let wl = len3v(-o[1], o[0], 0.0);
        let wa = if wl > 1e-4 { [-o[1] / wl, o[0] / wl, 0.0] } else { [1.0, 0.0, 0.0] };
        let spin = ej.f("tumble", 6.0) * (nrm / G);
        let bind = sc.v3("caseBind").unwrap_or_else(|| chamber1_pos(sc));
        let v = [cv[0] + o[0] * sv, cv[1] + o[1] * sv, cv[2] + o[2] * sv];
        body_spawn(s, g, sc.s("caseEntity"), bind, sc.v4("bindRot").unwrap_or(ID), Some(l), &f, v, spin, Some(wa));
        let c = &mut g.b.ch[k];
        (c.st, c.slide, c.sv) = ("empty", 0.0, 0.0);
        c.hide_next = true;
        play_snd(&g.b.weapon, snd.s("caseOut"));
    }
}
fn b_chamber_frames(sc: &'static J, ms: &F) -> Vec<F> {
    let br = sc.v4("bindRot").unwrap_or(ID);
    sc.g("chambers").arr().iter().map(|c| fcomp(ms, &fr(c.v3("pos").unwrap_or([0.0; 3]), br))).collect()
}
fn b_chamber_look(g: &mut Shotgun, sc: &'static J) {
    let mw = ji(sc, "magWhich", 0);
    let br = sc.v4("bindRot").unwrap_or(ID);
    for (k, c) in sc.g("chambers").arr().iter().enumerate() {
        let Some(st) = g.b.ch.get_mut(k) else { continue };
        let lt = ji(c.g("live"), "track", 0);
        let sp = c.g("spent");
        match st.st {
            "live" => {
                track_set(mw, lt, 1.0);
                if sp.present() {
                    track_set(mw, ji(sp, "track", 0), 0.0);
                }
            }
            "spent" => {
                track_set(mw, lt, 0.0);
                if sp.present() {
                    track_set(mw, ji(sp, "track", 0), 1.0);
                    rig_write(mw, ji(sp, "bone", 0), qrot(br, [0.0, 0.0, -st.slide]));
                }
            }
            _ => {
                track_set(mw, lt, 0.0);
                if sp.present() {
                    if st.hide_next {
                        st.hide_next = false;
                    } else {
                        track_set(mw, ji(sp, "track", 0), 0.0);
                    }
                }
            }
        }
    }
    g.b.wrote = true;
}
/// The held shells' two meshes (shell_a, shell_b) on the item in the hand.
fn b_held_parts(h: &mut Held) -> Option<[Handle; 2]> {
    if let Some(p) = &h.parts {
        return Some(p.clone());
    }
    let e = super::belt::in_slot(&player(), hand_slot(h.side));
    if e.is_null() {
        return None;
    }
    let (mut a, mut b) = (None, None);
    for c in super::belt::components(&e) {
        let nm = super::belt::comp_name(&c);
        if nm.contains("shell_a") {
            a = Some(c);
        } else if nm.contains("shell_b") {
            b = Some(c);
        }
    }
    let parts = [a?, b?];
    for c in &parts {
        lod0(c);
    }
    h.parts = Some(parts.clone());
    Some(parts)
}
fn lod0(c: &Handle) {
    // SAFETY: the mesh component's LOD byte.
    if let Some(p) = unsafe { crate::script_call::property(c.instance, "forceLODLevel") } {
        unsafe { *(p as *mut i8) = 0 };
    }
}
fn b_held_place(g: &mut Shotgun, sc: &'static J) -> bool {
    let Some(h) = g.b.held.as_mut() else { return false };
    let Some(parts) = b_held_parts(h) else { return false };
    let br = sc.v4("bindRot").unwrap_or(ID);
    let mut used = [false; 2];
    for sh in &h.list {
        let i = usize::from(sh.b_part);
        used[i] = true;
        let bp = sc.g("meshBind").v3(if sh.b_part { "b" } else { "a" }).unwrap_or([0.0; 3]);
        let t = unbind(&sh.hold, bp, br);
        set_local(&parts[i], fp(&t), fq(&t));
    }
    for i in 0..2 {
        if h.shown[i] != Some(used[i]) {
            super::belt::toggle(&parts[i], used[i]);
            h.shown[i] = Some(used[i]);
        }
    }
    true
}
fn b_take(s: &mut State, g: &mut Shotgun, sc: &'static J, snd: &'static J, side: usize, n: usize) -> bool {
    let Some(item) = sc.s("item") else { return false };
    if !item_hold(s, side, item, hand_slot(side)) {
        return false;
    }
    let mk = |b_part: bool, t: &J| HeldShell { b_part, hold: hold_frame(t), fm: [0.0; 7], f: None };
    let (list, pose) = if n >= 2 {
        let c2 = sc.g("carry2");
        (vec![mk(false, c2.i(1)), mk(true, c2.i(2))], c2.s("pose"))
    } else {
        let c1 = sc.g("carry1");
        (vec![mk(false, c1)], c1.s("pose"))
    };
    g.b.held = Some(Held { side, list, shown: [None; 2], pose, world: false, v: [0.0; 3], px: None, parts: None });
    belt_mag_taken(true);
    play_snd(&g.b.weapon, snd.s("grab"));
    true
}
/// The held shells drawn onto the nearest empty chamber's line; seated past seatAt.
fn b_magnet(s: &mut State, g: &mut Shotgun, sc: &'static J, snd: &'static J, side: usize, wr: Option<F>, rel: Option<F>) {
    let (Some(wr), Some(rel), true) = (wr, rel, g.b.held.is_some()) else {
        unpin(g);
        return;
    };
    let mg = sc.g("magnet");
    let l = sc.f("shellLen", 0.08);
    let (r, reach, gain) = (mg.f("radius", 0.045), mg.f("reach", 0.07), mg.f("gain", 3.0));
    let cos_a = mg.f("angle", 55.0).to_radians().cos();
    let sl = fcomp(&wr, &rel);
    let prev = g.b.magnet;
    let mut best: Option<(BMag, f64)> = None;
    {
        let h = g.b.held.as_mut().unwrap_or_else(|| unreachable!());
        for (si, sh) in h.list.iter_mut().enumerate() {
            let f = fcomp(&sl, &sh.hold);
            sh.fm = f;
            let a = qrot(fq(&f), [0.0, 0.0, 1.0]);
            for (k, c) in g.b.ch.iter().enumerate() {
                if c.st != "empty" {
                    continue;
                }
                let Some(cm) = g.b.cm.get(k) else { continue };
                let i = qrot(fq(cm), [0.0, 0.0, 1.0]);
                let dv = [f[0] - cm[0], f[1] - cm[1], f[2] - cm[2]];
                let d = dv[0] * i[0] + dv[1] * i[1] + dv[2] * i[2];
                let perp = len3v(dv[0] - d * i[0], dv[1] - d * i[1], dv[2] - d * i[2]);
                let al = a[0] * i[0] + a[1] * i[1] + a[2] * i[2];
                let keep = prev.is_some_and(|p| p.si == si && p.k == k);
                let rr = if keep { r * mg.f("widen", 1.5) } else { r };
                if d > -(l + reach) && d < 0.02 && perp < rr && al > cos_a {
                    let score = perp - if keep { 0.01 } else { 0.0 };
                    if best.as_ref().is_none_or(|(_, bp)| score < *bp) {
                        best = Some((BMag { si, k, d, perp, i, a, w: 0.0 }, score));
                    }
                }
            }
        }
    }
    let Some((mut best, _)) = best else {
        let h = g.b.held.as_ref().unwrap_or_else(|| unreachable!());
        let mut near: Option<BNear> = None;
        for sh in &h.list {
            let f = sh.fm;
            let a = qrot(fq(&f), [0.0, 0.0, 1.0]);
            for (k, c) in g.b.ch.iter().enumerate() {
                if c.st != "empty" {
                    continue;
                }
                let Some(cm) = g.b.cm.get(k) else { continue };
                let i = qrot(fq(cm), [0.0, 0.0, 1.0]);
                let dv = [f[0] - cm[0], f[1] - cm[1], f[2] - cm[2]];
                let d = dv[0] * i[0] + dv[1] * i[1] + dv[2] * i[2];
                let perp = len3v(dv[0] - d * i[0], dv[1] - d * i[1], dv[2] - d * i[2]);
                let ang = (a[0] * i[0] + a[1] * i[1] + a[2] * i[2]).clamp(-1.0, 1.0).acos().to_degrees();
                if near.as_ref().is_none_or(|n| perp < n.perp) {
                    near = Some(BNear { k, d, perp, ang });
                }
            }
        }
        g.b.magnet = None;
        g.b.near = near;
        unpin(g);
        return;
    };
    g.b.near = None;
    let (shold, f) = {
        let h = g.b.held.as_ref().unwrap_or_else(|| unreachable!());
        (h.list[best.si].hold, h.list[best.si].fm)
    };
    let cm = g.b.cm[best.k];
    let dt0 = best.d.clamp(-(l + reach), 0.0);
    let sw = qswing(best.a, best.i);
    let q = qmul(sw, fq(&f));
    let ft = fr([cm[0] + best.i[0] * dt0, cm[1] + best.i[1] * dt0, cm[2] + best.i[2] * dt0], q);
    let slt = fcomp(&ft, &finv(&shold));
    let wt = wrist_for(&slt, &rel);
    let mut w = ((1.0 - best.perp / r) * gain).clamp(0.0, 1.0);
    if best.d < -l {
        w *= (1.0 + (best.d + l) / reach).clamp(0.0, 1.0);
    }
    best.w = w;
    g.b.magnet = Some(best);
    g.owned = pin(g, side, w, Some(&wr), Some(&wt));
    if best.d >= -l * (1.0 - mg.f("seatAt", 0.80)) && w > 0.5 {
        g.b.ch[best.k].st = "live";
        let empty = {
            let h = g.b.held.as_mut().unwrap_or_else(|| unreachable!());
            h.list.remove(best.si);
            h.list.is_empty()
        };
        let weapon = g.b.weapon.clone();
        let n = mag_ammo(&weapon).unwrap_or(0) + 1;
        set_mag_ammo(s, &weapon, n);
        g.b.ammo_last = Some(n);
        play_snd(&weapon, snd.s("shellIn"));
        g.b.magnet = None;
        unpin(g);
        g.owned = None;
        if empty {
            if let Some(h) = g.b.held.take() {
                item_drop(s, h.side, hand_slot(h.side));
            }
            belt_mag_taken(false);
            if let Some(rp) = sc.s("releasePose") {
                fin_want(g, side, Some(rp));
                g.b.release_t = Some(0.25);
            } else {
                fin_off(g);
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn b_frame(s: &mut State, g: &mut Shotgun, sc: &'static J, snd: &'static J, weapon: &Handle, slot_comp: &Handle, holder: usize, dt: f64) -> &'static str {
    let free = if holder == 1 { 0 } else { 1 };
    (g.b.weapon, g.b.holder) = (weapon.clone(), Some(holder));
    g.owned = None;
    let (Some(pw), Some(ww)) = (world_of(&player()), world_of(weapon)) else { return "sg:noframe" };
    let wm = to_model(&pw, &ww);
    let am = mag_ammo(weapon).unwrap_or(0);
    let last = *g.b.ammo_last.get_or_insert(am);
    if am < last && g.b.act == "closed" {
        let mut d = last - am;
        for c in g.b.ch.iter_mut() {
            if d > 0 && c.st == "live" {
                (c.st, c.slide, c.sv, c.dbg) = ("spent", 0.0, 0.0, None);
                d -= 1;
            }
        }
    } else if am > last {
        let live = g.b.ch.iter().filter(|c| c.st == "live").count() as i32;
        if am > live {
            let mut add = am - live;
            for c in g.b.ch.iter_mut().rev() {
                if add > 0 && c.st != "live" {
                    c.st = "live";
                    add -= 1;
                }
            }
        }
    }
    g.b.ammo_last = Some(am);
    let r3 = super::belt::shared(159) > 0.5;
    let r3_edge = r3 && !g.b.r3_prev;
    g.b.r3_prev = r3;
    let hc = sc.g("hinge");
    let th = sc.g("thumb");
    let (rt, pt) = (th.f("reach", 0.10), th.f("pressTime", 0.08));
    g.b.t += dt;
    let rate = b_pitch_rate(g, &wm, dt);
    if g.b.act == "open" && rate > g.flick_peak {
        g.flick_peak = rate;
    }
    match g.b.act {
        "closed" => {
            if r3_edge && g.b.settle <= 0.0 {
                (g.b.act, g.b.t) = ("unlatch", 0.0);
                g.b.thumb_t = Some(0.0);
                g.b.lever_snd = false;
            }
        }
        "unlatch" => {
            let u = ((g.b.t - rt * 0.6) / (rt * 0.4 + pt)).clamp(0.0, 1.0);
            g.b.lever = smooth(u);
            if u > 0.0 && !g.b.lever_snd {
                play_snd(weapon, snd.s("lever"));
                g.b.lever_snd = true;
            }
            if u >= 1.0 {
                (g.b.act, g.b.t, g.b.hv) = ("opening", 0.0, 0.0);
                play_snd(weapon, snd.s("open"));
            }
        }
        "opening" => {
            g.b.hv += hc.f("accel", 5200.0) * dt;
            g.b.hinge += g.b.hv * dt;
            let a = hc.f("angle", 55.0);
            if g.b.hinge >= a {
                g.b.hinge = a;
                g.b.hv = -g.b.hv * hc.f("bounce", 0.22);
                g.b.bounces += 1;
                if g.b.hv.abs() < 25.0 || g.b.bounces > 3 {
                    (g.b.hinge, g.b.hv, g.b.bounces) = (a, 0.0, 0);
                    (g.b.act, g.b.t) = ("open", 0.0);
                    g.flick_peak = 0.0;
                }
            }
        }
        "open" => {
            let fl = sc.g("flick");
            let busy = g.b.magnet.is_some();
            if rate >= fl.f("rate", 240.0) && rate <= fl.f("max", 2000.0) {
                g.b.flick_n += 1;
            } else {
                g.b.flick_n = 0;
            }
            if r3_edge || (g.b.t > fl.f("arm", 0.35) && f64::from(g.b.flick_n) >= fl.f("frames", 2.0) && !busy) {
                (g.b.act, g.b.t) = ("closing", 0.0);
                g.b.close_from = Some(g.b.hinge);
                g.b.flick_at = rate;
            }
        }
        "closing" => {
            let t = hc.f("closeTime", 0.07);
            let u = (g.b.t / t).clamp(0.0, 1.0);
            g.b.hinge = g.b.close_from.unwrap_or(hc.f("angle", 55.0)) * (1.0 - u * u);
            if u >= 1.0 {
                (g.b.hinge, g.b.act, g.b.t) = (0.0, "closed", 0.0);
                play_snd(weapon, snd.s("close"));
                g.b.lever_kick = Some(0.0);
                g.b.settle = 0.30;
                for c in g.b.ch.iter_mut() {
                    (c.slide, c.sv) = (0.0, 0.0);
                }
            }
        }
        _ => {}
    }
    if g.b.act == "opening" || g.b.act == "open" {
        let hold = g.b.thumb_t.is_some_and(|t| t < rt + pt + th.f("hold", 0.30));
        if !hold {
            g.b.lever = (g.b.lever - dt / sc.g("lever").f("back", 0.12)).max(0.0);
        }
    } else if g.b.act == "closed" {
        if let Some(k) = g.b.lever_kick {
            let k = k + dt;
            let u = k / 0.15;
            g.b.lever = if u < 1.0 { (std::f64::consts::PI * u).sin() * 0.12 } else { 0.0 };
            g.b.lever_kick = if u >= 1.0 { None } else { Some(k) };
        }
    }
    b_thumb(g, sc, holder, dt);
    let open = (g.b.hinge / hc.f("angle", 55.0)).clamp(0.0, 1.0);
    let ms = slot_frame(slot_comp, sc.s("slot").unwrap_or("vrp_mag_slot"));
    let active = g.b.act != "closed" || g.b.settle > 0.0 || g.b.held.is_some();
    if let (Some(ms), true) = (ms, active) {
        let cw = b_chamber_frames(sc, &ms);
        g.b.cm = cw.iter().map(|c| to_model(&pw, c)).collect();
        let mut acc = None;
        if let (Some(p0), Some(cp)) = (cw.first(), g.b.cpos) {
            if dt > 1e-4 {
                let mut v = [(p0[0] - cp[0]) / dt, (p0[1] - cp[1]) / dt, (p0[2] - cp[2]) / dt];
                if len3(v) > 20.0 {
                    v = [0.0; 3];
                }
                let mut cv = g.b.cvel.unwrap_or(v);
                let ac = [(v[0] - cv[0]) / dt, (v[1] - cv[1]) / dt, (v[2] - cv[2]) / dt];
                let a = 0.35;
                for k in 0..3 {
                    cv[k] += (v[k] - cv[k]) * a;
                }
                g.b.cvel = Some(cv);
                let mut sa = g.b.cacc.unwrap_or([0.0; 3]);
                for k in 0..3 {
                    sa[k] += (ac[k] - sa[k]) * a;
                }
                let al = len3(sa);
                if al > 60.0 {
                    sa = sa.map(|v| v * 60.0 / al);
                }
                g.b.cacc = Some(sa);
                acc = Some(sa);
            }
        }
        if let Some(p0) = cw.first() {
            g.b.cpos = Some(fp(p0));
        }
        if g.b.act != "closed" && g.b.act != "unlatch" {
            for (k, c) in cw.iter().enumerate().take(g.b.ch.len()) {
                b_case_tick(s, g, sc, snd, k, c, dt, acc, open);
            }
        }
    } else {
        (g.b.cpos, g.b.cvel, g.b.cacc) = (None, None, None);
    }
    let grip = grip_of(free);
    let grip_edge = grip && !g.b.grip_prev;
    g.b.grip_prev = grip;
    let need = g.b.ch.iter().filter(|c| c.st != "live").count();
    let wr = wrist_raw(free);
    let rel = wrist_to_slot(free);
    if g.b.held.is_none() && (g.b.act == "open" || g.b.act == "opening") && need > 0 && grip_edge {
        if let Some(wr) = wr {
            let ok = match belt_mag_dist(free) {
                Some(bd) => bd <= belt_mag_radius(),
                None => g.b.cm.first().is_some_and(|c| len3v(wr[0] - c[0], wr[1] - c[1], wr[2] - c[2]) > 0.30),
            };
            if ok {
                b_take(s, g, sc, snd, free, need.min(2));
            }
        }
    }
    if let Some(side) = g.b.held.as_ref().map(|h| h.side) {
        if side != free {
            b_held_drop(s, g, sc, false);
        } else {
            b_held_place(g, sc);
            let pose = g.b.held.as_ref().and_then(|h| h.pose);
            fin_want(g, free, pose);
            if let (Some(wr), Some(rel)) = (wr, rel) {
                let sl = fcomp(&wr, &rel);
                let h = g.b.held.as_mut().unwrap_or_else(|| unreachable!());
                h.world = true;
                for sh in h.list.iter_mut() {
                    let fm = fcomp(&sl, &sh.hold);
                    sh.f = Some(fcomp(&pw, &fm));
                }
                if let (Some(f1), true) = (h.list.first().and_then(|x| x.f), dt > 1e-4) {
                    if let Some(px) = h.px {
                        let a = 0.45;
                        for k in 0..3 {
                            h.v[k] += ((f1[k] - px[k]) / dt - h.v[k]) * a;
                        }
                    }
                    h.px = Some(fp(&f1));
                }
            }
            if !grip {
                b_held_drop(s, g, sc, true);
            } else if g.b.act == "open" && !g.b.cm.is_empty() {
                b_magnet(s, g, sc, snd, free, wr, rel);
            } else {
                unpin(g);
            }
        }
    } else {
        unpin(g);
        if let Some(rt) = g.b.release_t {
            let rt = rt - dt;
            g.b.release_t = if rt <= 0.0 { None } else { Some(rt) };
            if rt <= 0.0 {
                fin_off(g);
            }
        }
    }
    fin_tick(g, dt, 0.15);
    let fw = ji(sc, "frameWhich", 1);
    if g.b.hinge > 0.0 || g.b.wrote {
        rig_write_rot(fw, ji(hc, "bone", 0), g.b.hinge * hc.f("sign", 1.0), hc.v3("axis").unwrap_or([1.0, 0.0, 0.0]));
        g.b.wrote = true;
    }
    let lv = sc.g("lever");
    if lv.present() && (g.b.lever > 0.0 || g.b.wrote) {
        let ang = g.b.lever * lv.f("angle", 40.0) * lv.f("sign", 1.0);
        rig_write_rot(ji(lv, "which", 2), ji(lv, "bone", 0), ang, lv.v3("axis").unwrap_or([1.0, 0.0, 0.0]));
        g.b.wrote = true;
    }
    for rd in sc.g("riders").arr() {
        let mut f = g.b.lever;
        if rd.s("follow") == Some("latch") && (g.b.act == "opening" || g.b.act == "open") {
            f = f.max(rd.f("openHold", 0.75));
        }
        if f > 0.0 || g.b.wrote {
            let v = rd.v3("vec").unwrap_or([0.0; 3]);
            rig_write(ji(rd, "which", fw), ji(rd, "bone", 0), v.map(|x| x * f));
        }
    }
    if active && g.b.act != "closed" && g.b.act != "unlatch" {
        b_chamber_look(g, sc);
    }
    if g.b.act == "closed" {
        g.b.settle = (g.b.settle - dt).max(0.0);
        if g.b.wrote && g.b.settle <= 0.0 && g.b.lever_kick.is_none() && g.b.hinge <= 0.0 {
            b_release(g, sc);
        }
    }
    if sc.b("ownAnim") {
        if g.b.act != "closed" {
            hold_reload_stage(weapon);
            g.b.tagged = true;
        } else if g.b.tagged {
            push_anim_event(weapon, "InterruptReload");
            g.b.tagged = false;
        }
    }
    g.trg = i32::from(g.b.act != "closed");
    set_trg(s, g.trg);
    let cs: Vec<String> = g
        .b
        .ch
        .iter()
        .map(|c| format!("{}{}", &c.st[..1.min(c.st.len())], if c.st == "spent" { c.dbg.as_ref().map_or(String::new(), |d| format!("({d})")) } else { String::new() }))
        .collect();
    let mg = g.b.magnet.map_or("-".to_string(), |m| format!("k{} d={:.3} p={:.3} w={:.2}", m.k + 1, m.d, m.perp, m.w));
    g.diag = format!(
        "sg[break {} t={:.2} hinge={:.1} lever={:.2} ch={} am={am} held={} mag={mg} flick={:.0}/{:.0} bodies={}]",
        g.b.act,
        g.b.t,
        g.b.hinge,
        g.b.lever,
        cs.join(","),
        g.b.held.as_ref().map_or("-".to_string(), |h| h.list.len().to_string()),
        g.flick_peak,
        g.b.flick_at,
        g.bodies.len()
    );
    if super::reload_reach::on() {
        b_reach(g, sc, free, wr, rel);
    }
    match g.b.act {
        "unlatch" => "sg:break:unlatch",
        "opening" => "sg:break:opening",
        "open" => "sg:break:open",
        "closing" => "sg:break:closing",
        _ => "sg:break:closed",
    }
}

/// CVR_RELOAD_REACH: where the free hand goes next to load the open break-action (reload_reach.rs) -- with no shells in
/// it, onto the belt's shell pouch (the wrist put so the palm lands on the pouch, the hand's turn kept); with shells,
/// the wrist that seats the first one in the nearest empty chamber (the magnet's own target at full depth), its
/// approach ("pull") a shell's length and 3 cm out along the chamber's line. The state rides in "why".
fn b_reach(g: &Shotgun, sc: &'static J, free: usize, wr: Option<F>, rel: Option<F>) {
    let Some(wr) = wr else { return };
    let (hand, hq) = (fp(&wr), fq(&wr));
    let l = sc.f("shellLen", 0.08);
    let mut near = None;
    let mut axis = [0.0; 3];
    let mut travel = 0.0;
    let mut kind = "break";
    if let (Some(h), Some(rel)) = (g.b.held.as_ref(), rel) {
        let sl = fcomp(&wr, &rel);
        if let Some(sh) = h.list.first() {
            let f = fcomp(&sl, &sh.hold);
            let a = qrot(fq(&f), [0.0, 0.0, 1.0]);
            let mut best: Option<(f64, usize)> = None;
            for (k, c) in g.b.ch.iter().enumerate() {
                let Some(cm) = g.b.cm.get(k) else { continue };
                let d = len3v(f[0] - cm[0], f[1] - cm[1], f[2] - cm[2]);
                if c.st == "empty" && best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, k));
                }
            }
            if let Some((d, k)) = best {
                let cm = g.b.cm[k];
                let i = qrot(fq(&cm), [0.0, 0.0, 1.0]);
                let ft = fr(fp(&cm), qmul(qswing(a, i), fq(&f)));
                let wt = wrist_for(&fcomp(&ft, &finv(&sh.hold)), &rel);
                near = Some(super::reload_reach::Near { d, at: fp(&wt), rot: Some(fq(&wt)) });
                (axis, travel, kind) = (i, l + 0.03, "chamber");
            }
        }
    } else if let Some((t, palm)) = super::belt::mag_target(free) {
        let at = [hand[0] + t[0] - palm[0], hand[1] + t[1] - palm[1], hand[2] + t[2] - palm[2]];
        near = Some(super::reload_reach::Near { d: len3v(t[0] - palm[0], t[1] - palm[1], t[2] - palm[2]), at, rot: None });
        kind = "pouch";
    }
    super::reload_reach::publish(&super::reload_reach::Part {
        kind,
        weapon: "break",
        free,
        near,
        hand,
        hand_rot: Some(hq),
        axis,
        travel,
        radius: belt_mag_radius(),
        angle_max: 180.0,
        grab_with: "grip",
        grabbed: g.b.held.is_some(),
        rack: g.b.hinge,
        why: &g.diag,
        exit: g.b.act,
    });
}

// ---------------------------------------------------------------- the pump
fn p_release(g: &mut Shotgun, sc: &'static J) {
    let pc = sc.g("pump");
    let w = ji(pc, "which", 1);
    write_off(w, ji(pc, "bone", 0));
    for rd in pc.g("riders").arr() {
        write_off(ji(rd, "which", w), ji(rd, "bone", 0));
    }
    let gt = sc.g("gate");
    if gt.present() {
        write_off(ji(gt, "which", 1), ji(gt, "bone", 0));
    }
    g.p.wrote = false;
}
/// The recorded handful (reload/handful/<name>.lua) for a multi config.
fn m_data(mc: &J) -> Option<&'static J> {
    let d = data::data().g("handful").g(mc.s("data")?);
    d.g("hold").is_array().then_some(d)
}
fn m_move(d: &'static J, n: usize) -> Option<&'static [J]> {
    let mv = d.g("move").g(&n.to_string());
    let a = mv.as_array()?;
    (a.len() >= 2).then_some(a.as_slice())
}
fn m_move_frame(mv: &[J], u: f64) -> F {
    let n = mv.len();
    let x = u.clamp(0.0, 1.0) * (n - 1) as f64;
    let k = (x.floor() as usize).min(n - 2);
    let f = x - k as f64;
    let a = jf7(mv[k].g("shell")).unwrap_or([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    let b = jf7(mv[k + 1].g("shell")).unwrap_or(a);
    let q = qnlerp(fq(&a), fq(&b), f);
    fr([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f], q)
}
fn m_move_pose(mv: &[J], u: f64) -> Option<Pose> {
    let n = mv.len();
    let x = u.clamp(0.0, 1.0) * (n - 1) as f64;
    let k = (x.floor() as usize).min(n - 2);
    let pa = data::pose_j(mv[k].g("pose"));
    let pb = data::pose_j(mv[k + 1].g("pose"));
    mix_pose(pa.as_ref(), pb.as_ref(), x - k as f64)
}
fn m_fingers(g: &mut Shotgun, sc: &'static J, mt: &Multi, hand: usize) -> bool {
    let Some(d) = m_data(sc.g("multi")) else { return false };
    if !d.g("pose").present() {
        return false;
    }
    let n = mt.list.len();
    let mv = if mt.state != "ready" { m_move(d, n + 1) } else { None };
    if let Some(mv) = mv {
        let u = mt.u;
        if u <= 0.0 {
            let p = data::pose_j(d.g("pose").i(n + 1));
            return fin_want_table(g, hand, &format!("hf{}", n + 1), p.as_ref());
        }
        let p = m_move_pose(mv, u);
        return fin_want_table(g, hand, &format!("hfm{}:{u:.4}", n + 1), p.as_ref());
    }
    let p = data::pose_j(d.g("pose").i(n));
    fin_want_table(g, hand, &format!("hf{n}"), p.as_ref())
}
const MULTI_PARTS: [&str; 4] = ["shell_a", "shell_b", "shell_c", "shell_d"];
fn m_parts(mt: &mut Multi) -> Option<[Option<Handle>; 4]> {
    if let Some(p) = &mt.parts {
        return Some(p.clone());
    }
    let e = super::belt::in_slot(&player(), hand_slot(mt.side));
    if e.is_null() {
        return None;
    }
    let mut parts: [Option<Handle>; 4] = [None, None, None, None];
    for c in super::belt::components(&e) {
        let nm = super::belt::comp_name(&c);
        for (k, p) in MULTI_PARTS.iter().enumerate() {
            if nm.contains(p) {
                parts[k] = Some(c.clone());
            }
        }
    }
    if (0..mt.n.min(4)).any(|k| parts[k].is_none()) {
        return None;
    }
    for c in parts.iter().flatten() {
        lod0(c);
    }
    mt.parts = Some(parts.clone());
    Some(parts)
}
/// The hold of the i-th (1-based) shell left in the hand.
fn m_hold(sc: &'static J, mt: &Multi, i: usize) -> F {
    if let Some(d) = m_data(sc.g("multi")) {
        let n = mt.list.len();
        let mv = if mt.state != "ready" { m_move(d, n + 1) } else { None };
        if let (Some(mv), 1) = (mv, i) {
            return m_move_frame(mv, mt.u);
        }
        if let Some(h) = jf7(d.g("hold").i(n).i(i)) {
            return h;
        }
    }
    let c = sc.g("carry");
    let st = sc.g("multi").g("stack");
    let r = c.v4("rot").unwrap_or(ID);
    let off = c.v3("off").unwrap_or([0.0; 3]);
    let dv = qrot(r, st.v3("dir").unwrap_or([1.0, 0.0, 0.0]));
    let j = (i - 1) as f64 + mt.off;
    let step = st.f("step", 0.021);
    let mut x = [off[0] + dv[0] * step * j, off[1] + dv[1] * step * j, off[2] + dv[2] * step * j];
    if i == 1 && j > 0.0 && j < 1.0 {
        let lv = qrot(r, st.v3("lift").unwrap_or([0.0, 1.0, 0.0]));
        let a = (std::f64::consts::PI * j).sin() * st.f("liftBy", 0.008);
        x = [x[0] + lv[0] * a, x[1] + lv[1] * a, x[2] + lv[2] * a];
    }
    let h = st.f("spin", 120.0).to_radians() * j * 0.5;
    fr(x, qmul(r, [0.0, 0.0, h.sin(), h.cos()]))
}
fn m_place(sc: &'static J, mt: &mut Multi) -> bool {
    let Some(parts) = m_parts(mt) else { return false };
    let br = sc.v4("bindRot").unwrap_or(ID);
    let sb = sc.v3("shellBind").unwrap_or([0.0; 3]);
    let mut on = [false; 4];
    for (i, k) in mt.list.iter().enumerate() {
        let Some(slot) = k.checked_sub(1).filter(|s| *s < 4) else { continue };
        on[slot] = true;
        let hold = m_hold(sc, mt, i + 1);
        let t = unbind(&hold, sb, br);
        if let Some(c) = &parts[slot] {
            set_local(c, fp(&t), fq(&t));
        }
    }
    for (k, c) in parts.iter().enumerate() {
        let Some(c) = c else { continue };
        if mt.shown[k] != Some(on[k]) {
            super::belt::toggle(c, on[k]);
            mt.shown[k] = Some(on[k]);
        }
    }
    true
}
fn two_hand_slide(s: &mut State, v: V3) {
    let on = v != [0.0; 3];
    if !on && !s.sg_slide_on {
        return;
    }
    s.sg_slide_on = on;
    native::<()>("SetVRTwoHandSlide", &[Value::Float(v[0] as f32), Value::Float(v[1] as f32), Value::Float(v[2] as f32)]);
}
fn p_drop_shell(s: &mut State, g: &mut Shotgun, sc: &'static J, throw: bool) {
    let Some(sh) = g.p.shell.take() else { return };
    if throw {
        let ent = sc.s("roundEntity");
        let sb = sc.v3("shellBind").unwrap_or([0.0; 3]);
        let br = sc.v4("bindRot").unwrap_or(ID);
        if let Some(mt) = &sh.multi {
            if let (Some(sl), Some(pw)) = (sh.sl, g.p.pw) {
                for i in 1..=mt.list.len() {
                    let f = fcomp(&pw, &fcomp(&sl, &m_hold(sc, mt, i)));
                    body_spawn(s, g, ent, sb, br, sc.fo("shellLen"), &f, sh.v, 0.0, None);
                }
            }
        } else if let Some(f) = sh.f {
            body_spawn(s, g, ent, sb, br, sc.fo("shellLen"), &f, sh.v, 0.0, None);
        }
    }
    if sh.multi.is_some() {
        item_drop(s, sh.side, hand_slot(sh.side));
    } else {
        hold_hide(s, Some(sh.side));
    }
    super::belt::set_shell_taken(false, -1);
    g.p.magnet = None;
}
fn cycle_free_g(g: &mut Shotgun) {
    if let Some(idx) = g.p.cyc_held.take() {
        track_write(0, idx, 0.0, 0);
    }
}
fn p_reset_all(s: &mut State, g: &mut Shotgun, sc: &'static J, weapon: Option<&Handle>) {
    two_hand_slide(s, [0.0; 3]);
    cycle_free_g(g);
    super::reload_mag::lt_block(s, false);
    if g.p.wrote {
        p_release(g, sc);
    }
    if g.p.shell.is_some() {
        p_drop_shell(s, g, sc, false);
    }
    (g.p.s, g.p.engaged, g.p.back, g.p.gate, g.p.gate_t) = (0.0, false, false, 0.0, None);
    g.p.magnet = None;
    let am = weapon.and_then(mag_ammo).unwrap_or(0);
    (g.p.chambered, g.p.spent_in) = (am > 0, false);
    g.p.ammo_last = Some(am);
}
/// A round or a case out of the ejection port with its kick.
fn p_eject(s: &mut State, g: &mut Shotgun, sc: &'static J, ww: &F, live: bool) {
    let ej = sc.g("ejectPort");
    let p = ej.v3("pos").unwrap_or([0.012, 0.13, 0.055]);
    let br = sc.v4("bindRot").unwrap_or(ID);
    let f = fcomp(ww, &fr(p, br));
    let v = qrot(fq(ww), ej.v3("vel").unwrap_or([1.6, -0.3, 1.3]));
    let wa = qrot(fq(ww), [0.0, 0.0, 1.0]);
    let cv = g.p.wvel.unwrap_or([0.0; 3]);
    let (ent, bind, len) = if live {
        (sc.s("roundEntity"), sc.v3("shellBind").unwrap_or([0.0; 3]), sc.fo("shellLen"))
    } else {
        (sc.s("caseEntity"), sc.v3("caseBind").unwrap_or([0.0; 3]), sc.fo("spentLen"))
    };
    body_spawn(s, g, ent, bind, br, len, &f, [cv[0] + v[0], cv[1] + v[1], cv[2] + v[2]], ej.f("spin", 14.0), Some(wa));
}
fn p_write(g: &mut Shotgun, sc: &'static J) {
    let pc = sc.g("pump");
    let la = pc.v3("axis").unwrap_or([0.0; 3]);
    let r0 = pc.v3("restLocal").unwrap_or([0.0; 3]);
    let sv = g.p.s;
    let w = ji(pc, "which", 1);
    // ABSOLUTE translation (the last argument 1), the rotation left to the animation (a zero quaternion): the pump and
    // its riders are written where the rig puts them at rest plus the stroke. Written as an offset (0) the rest itself
    // was added onto the animated pose -- the fore-end 41 cm forward, the bolt off its rail -- and the identity
    // rotation turned the parts off their own.
    rig_write_abs(w, ji(pc, "bone", 0), [r0[0] + la[0] * sv, r0[1] + la[1] * sv, r0[2] + la[2] * sv], [0.0; 4], 1);
    for rd in pc.g("riders").arr() {
        let v = sv * rd.f("ratio", 1.0);
        let ax = rd.v3("axis").unwrap_or(la);
        let r1 = rd.v3("restLocal").unwrap_or([0.0; 3]);
        rig_write_abs(ji(rd, "which", w), ji(rd, "bone", 0), [r1[0] + ax[0] * v, r1[1] + ax[1] * v, r1[2] + ax[2] * v], [0.0; 4], 1);
    }
    g.p.wrote = true;
}
fn p_gate(g: &mut Shotgun, sc: &'static J, dt: f64) {
    let gt = sc.g("gate");
    if !gt.present() {
        return;
    }
    let mut want = g.p.gate_want.take().unwrap_or(0.0);
    if let Some(t) = g.p.gate_t {
        let t = t + dt;
        let u = t / gt.f("time", 0.25);
        if u >= 1.0 {
            g.p.gate_t = None;
        } else {
            g.p.gate_t = Some(t);
            want = want.max((std::f64::consts::PI * u).sin());
        }
    }
    g.p.gate = want;
    if want > 0.0 || g.p.gate_on {
        let ang = want * gt.f("angle", 20.0) * gt.f("sign", 1.0);
        rig_write_rot(ji(gt, "which", 1), ji(gt, "bone", 0), ang, gt.v3("axis").unwrap_or([1.0, 0.0, 0.0]));
        g.p.gate_on = want > 0.0;
        g.p.wrote = true;
    }
}
/// pLine: the frame against a loading line (the tube or the port): its fraction along, its distance off, the target
/// frame on the line and the line's direction.
fn p_line(l: &J, wm: &F, f: &F) -> Option<(f64, f64, F, V3)> {
    let wq = fq(wm);
    let from = qrot(wq, l.v3("from")?);
    let to = qrot(wq, l.v3("to")?);
    let fv = [from[0] + wm[0], from[1] + wm[1], from[2] + wm[2]];
    let tv = [to[0] + wm[0], to[1] + wm[1], to[2] + wm[2]];
    let mut dv = [tv[0] - fv[0], tv[1] - fv[1], tv[2] - fv[2]];
    let run = len3(dv);
    if run < 1e-5 {
        return None;
    }
    dv = dv.map(|v| v / run);
    let r = [f[0] - fv[0], f[1] - fv[1], f[2] - fv[2]];
    let d = r[0] * dv[0] + r[1] * dv[1] + r[2] * dv[2];
    let pv = [r[0] - d * dv[0], r[1] - d * dv[1], r[2] - d * dv[2]];
    let u = d / run;
    let uc = u.clamp(0.0, 1.0);
    let a = qmul(wq, l.v4("fromRot").unwrap_or(ID));
    let b = qmul(wq, l.v4("toRot").unwrap_or(ID));
    let q = qnlerp(a, b, uc);
    let t = fr([fv[0] + dv[0] * run * uc, fv[1] + dv[1] * run * uc, fv[2] + dv[2] * run * uc], q);
    Some((u, len3(pv), t, qrot(q, [0.0, 0.0, 1.0])))
}
fn p_at(l: &J, wm: &F, u: f64) -> F {
    let a = u.clamp(0.0, 1.0);
    let from = l.v3("from").unwrap_or([0.0; 3]);
    let to = l.v3("to").unwrap_or(from);
    let x = [from[0] + (to[0] - from[0]) * u, from[1] + (to[1] - from[1]) * u, from[2] + (to[2] - from[2]) * u];
    let wq = fq(wm);
    let w = qrot(wq, x);
    let qa = qmul(wq, l.v4("fromRot").unwrap_or(ID));
    let qb = qmul(wq, l.v4("toRot").unwrap_or(ID));
    fr([wm[0] + w[0], wm[1] + w[1], wm[2] + w[2]], qnlerp(qa, qb, a))
}
#[allow(clippy::too_many_arguments)]
fn p_frame(s: &mut State, g: &mut Shotgun, sc: &'static J, snd: &'static J, weapon: &Handle, holder: usize, dt: f64) -> &'static str {
    let free = if holder == 1 { 0 } else { 1 };
    if let Some(ct) = sc.fo("cycleTrack").map(|v| v as i32) {
        if g.p.cyc_held != Some(ct) {
            cycle_free_g(g);
            track_write(0, ct, 0.0, 1);
            g.p.cyc_held = Some(ct);
        }
    }
    g.owned = None;
    let pc = sc.g("pump");
    let (Some(pw), Some(ww)) = (world_of(&player()), world_of(weapon)) else { return "sg:noframe" };
    g.p.pw = Some(pw);
    let wm = to_model(&pw, &ww);
    if let (Some(wp), true) = (g.p.wpos, dt > 1e-4) {
        let a = 0.4;
        let mut cv = g.p.wvel.unwrap_or([0.0; 3]);
        for k in 0..3 {
            cv[k] += ((ww[k] - wp[k]) / dt - cv[k]) * a;
        }
        if len3(cv) > 20.0 {
            cv = [0.0; 3];
        }
        g.p.wvel = Some(cv);
    }
    g.p.wpos = Some(fp(&ww));
    let am = mag_ammo(weapon).unwrap_or(0);
    let last = *g.p.ammo_last.get_or_insert(am);
    if am < last && g.p.chambered {
        (g.p.chambered, g.p.spent_in) = (false, true);
    } else if am > last && !g.p.our_add {
        (g.p.chambered, g.p.spent_in) = (true, false);
    }
    g.p.our_add = false;
    g.p.ammo_last = Some(am);
    let cap = mag_capacity(weapon);
    let grip = grip_of(free);
    let grip_edge = grip && !g.p.grip_prev;
    g.p.grip_prev = grip;
    let work = grip && trig_of(free);
    let two_h = native::<bool>("GetVRTwoHandActive", &[]) == Some(true);
    let wr = wrist_raw(free);
    let rel = wrist_to_slot(free);
    let dir = pc.v3("dir").unwrap_or([0.0, 1.0, 0.0]);
    let bvec = qrot(fq(&wm), dir);
    let rw = pc.v3("restW").unwrap_or([0.0; 3]);
    let pg = hand_grip(pc.g("grip"), free, 1);
    let go = pg.v3("off").unwrap_or([0.0; 3]);
    let grot = pg.v4("rot").unwrap_or(ID);
    let sup = free == support_hand();
    let grip_at = |sv: f64| -> F {
        let x = [rw[0] + dir[0] * sv + go[0], rw[1] + dir[1] * sv + go[1], rw[2] + dir[2] * sv + go[2]];
        let w = qrot(fq(&wm), x);
        fr([wm[0] + w[0], wm[1] + w[1], wm[2] + w[2]], qmul(fq(&wm), grot))
    };
    let t0 = grip_at(g.p.s);
    let d_hand = wr.map_or(99.0, |w| len3v(w[0] - t0[0], w[1] - t0[1], w[2] - t0[2]));
    let mut wa = wr.map(|w| fp(&w));
    if two_h && sup {
        let c = native::<[f32; 4]>("GetVRTwoHandLeftCtrl", &[]).filter(|v| v[3] > 0.5).map(f3);
        wa = c.or(wa);
    }
    let along = wa.map_or(0.0, |w| (w[0] - wm[0]) * bvec[0] + (w[1] - wm[1]) * bvec[1] + (w[2] - wm[2]) * bvec[2]);
    let on_fore = (two_h && sup) || d_hand <= pc.f("grabRadius", 0.10);
    // CVR_RELOAD_REACH: the fore-end's grip for a test's hand (reload_reach.rs); worked with the grip and the trigger
    // held, a full stroke along the pump's direction
    if super::reload_reach::on() {
        let hq = native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(free as i32)]).map(f4);
        super::reload_reach::publish(&super::reload_reach::Part {
            kind: "pump",
            weapon: "shotgun",
            free,
            near: Some(super::reload_reach::Near { d: d_hand, at: fp(&t0), rot: Some(fq(&t0)) }),
            hand: wr.map_or([0.0; 3], |w| fp(&w)),
            hand_rot: hq,
            axis: [-bvec[0], -bvec[1], -bvec[2]],
            travel: pc.f("travel", 0.08),
            radius: pc.f("grabRadius", 0.10),
            angle_max: 180.0,
            grab_with: "grip+trigger",
            grabbed: g.p.engaged,
            rack: g.p.s,
            why: if on_fore { "on the fore-end" } else { "off the fore-end" },
            exit: "ok",
        });
    }
    let ltb = free == aim_hand() && grip && on_fore && g.p.shell.is_none();
    super::reload_mag::lt_block(s, ltb);
    if !g.p.engaged && on_fore && g.p.shell.is_none() && (g.p.spent_in || (!g.p.chambered && am > 0)) {
        if let Some(w) = wr {
            reach_mark(free, fp(&t0), Some(fp(&w)));
        }
    }
    let pose = pc.s("pose");
    if !g.p.engaged {
        if work && on_fore && g.p.shell.is_none() && wr.is_some() {
            (g.p.engaged, g.p.along0, g.p.s0, g.p.via_hold) = (true, along, g.p.s, two_h);
        }
    } else if !work || (g.p.via_hold && !two_h) {
        g.p.engaged = false;
        unpin(g);
        if g.fin.key.as_deref() == pose && pose.is_some() {
            fin_off(g);
        }
    }
    let travel = pc.f("travel", 0.08);
    if let (true, Some(w)) = (g.p.engaged, wr) {
        let sv = (g.p.s0 + (along - g.p.along0) * pc.f("gain", 1.0)).clamp(0.0, travel);
        let prev = g.p.s;
        g.p.s = sv;
        if sv > 0.2 * travel && prev <= 0.2 * travel && !g.p.back {
            play_snd(weapon, snd.s("pumpOpen"));
        }
        if sv >= 0.85 * travel && !g.p.back {
            g.p.back = true;
            if g.p.spent_in {
                p_eject(s, g, sc, &ww, false);
                g.p.spent_in = false;
            } else if g.p.chambered {
                p_eject(s, g, sc, &ww, true);
                g.p.chambered = false;
                set_mag_ammo(s, weapon, (am - 1).max(0));
                g.p.ammo_last = Some((am - 1).max(0));
            }
        }
        if g.p.back && sv <= 0.08 * travel {
            g.p.back = false;
            if !g.p.chambered && g.p.ammo_last.unwrap_or(0) > 0 {
                g.p.chambered = true;
            }
            play_snd(weapon, snd.s("pumpClose"));
        }
        if !g.p.via_hold {
            let t = grip_at(sv);
            let k = spring(g, dt);
            g.owned = pin(g, free, k, Some(&w), Some(&t));
            fin_want(g, free, pose);
        }
    } else if g.p.shell.is_none() {
        g.spring.0 = false;
        if g.pin_side == Some(free) {
            unpin(g);
        }
        if g.fin.key.as_deref() == pose && pose.is_some() {
            fin_off(g);
        }
    }
    if two_h && sup && g.p.s > 0.0005 {
        two_hand_slide(s, bvec.map(|v| v * g.p.s));
    } else {
        two_hand_slide(s, [0.0; 3]);
    }
    if g.p.shell.is_none() && !g.p.engaged && grip_edge {
        if let Some(w) = wr {
            let bd = super::belt::shell_dist(free as i32);
            let ok = if bd >= 0.0 { f64::from(bd) <= f64::from(super::belt::reach_radius_published()) } else { len3v(w[0] - wm[0], w[1] - wm[1], w[2] - wm[2]) > 0.35 };
            let mc = sc.g("multi");
            let n = if mc.present() { (mc.f("count", 1.0) as i32).min(cap - am) } else { 1 };
            let carry = hold_frame(sc.g("carry"));
            if ok && am < cap && n >= 2 && mc.s("item").is_some_and(|it| item_hold(s, free, it, hand_slot(free))) {
                g.p.shell = Some(Shell { side: free, hold: carry, multi: Some(Multi::new(free, n as usize)), sl: None, f: None, v: [0.0; 3], px: None, push: None });
                super::belt::set_shell_taken(true, free as i32);
                play_snd(weapon, snd.s("grab"));
            } else if ok && am < cap && sc.s("shellMesh").is_some_and(|mesh| hold_show(s, free, mesh, None)) {
                g.p.shell = Some(Shell { side: free, hold: carry, multi: None, sl: None, f: None, v: [0.0; 3], px: None, push: None });
                super::belt::set_shell_taken(true, free as i32);
                play_snd(weapon, snd.s("grab"));
            }
        }
    }
    if let Some(side) = g.p.shell.as_ref().map(|x| x.side) {
        if side != free {
            p_drop_shell(s, g, sc, false);
        } else {
            p_shell(s, g, sc, snd, weapon, &wm, wr, rel, free, grip, am, cap, travel, dt);
        }
    }
    fin_tick(g, dt, 0.15);
    p_gate(g, sc, dt);
    if g.p.engaged || g.p.s > 0.0005 || !g.p.chambered {
        p_write(g, sc);
    } else if g.p.wrote && !g.p.gate_on {
        p_release(g, sc);
    }
    g.trg = if g.p.chambered && g.p.s <= 0.003 { 0 } else { 1 };
    set_trg(s, g.trg);
    let yn = |b: bool| if b { "Y" } else { "n" };
    let shell = match &g.p.shell {
        Some(Shell { multi: Some(mt), .. }) => format!("{}:{}:{:.3}", mt.list.len(), mt.state, mt.gone),
        Some(_) => "Y".into(),
        None => "n".into(),
    };
    let mg = g.p.magnet.as_ref().map_or("-".to_string(), |m| format!("{} u={:.2} p={:.3} w={:.2}", m.mode, m.u, m.perp, m.w));
    g.diag = format!(
        "sg[pump s={:.0}mm eng={} back={} ch={} spent={} am={am}/{cap} shell={shell} mag={mg} gate={:.2} bodies={}]",
        g.p.s * 1000.0,
        yn(g.p.engaged),
        yn(g.p.back),
        yn(g.p.chambered),
        yn(g.p.spent_in),
        g.p.gate,
        g.bodies.len()
    );
    "sg:pump"
}
/// The shell (or handful) in the free hand: placed, its fingers, pushed home, rolled forward, magnetised to a line.
#[allow(clippy::too_many_arguments)]
fn p_shell(s: &mut State, g: &mut Shotgun, sc: &'static J, snd: &'static J, weapon: &Handle, wm: &F, wr: Option<F>, rel: Option<F>, free: usize, grip: bool, am: i32, cap: i32, travel: f64, dt: f64) {
    let sb = sc.v3("shellBind").unwrap_or([0.0; 3]);
    let br = sc.v4("bindRot").unwrap_or(ID);
    let Some(mut sh) = g.p.shell.take() else { return };
    if let Some(mt) = sh.multi.as_mut() {
        m_place(sc, mt);
    } else {
        let t = unbind(&sh.hold, sb, br);
        hold_place(s, free, fp(&t), fq(&t));
    }
    let fingered = sh.multi.as_ref().is_some_and(|mt| m_fingers(g, sc, mt, free));
    if !fingered {
        fin_want(g, free, sc.g("carry").s("pose"));
    }
    let mut fm = None;
    if let (Some(w), Some(r), Some(pw)) = (wr, rel, g.p.pw) {
        let sl = fcomp(&w, &r);
        sh.sl = Some(sl);
        let f = fcomp(&sl, &sh.hold);
        fm = Some(f);
        let fw = fcomp(&pw, &f);
        sh.f = Some(fw);
        if let (true, Some(px)) = (dt > 1e-4, sh.px) {
            let a = 0.45;
            for k in 0..3 {
                sh.v[k] += ((fw[k] - px[k]) / dt - sh.v[k]) * a;
            }
        }
        sh.px = Some(fp(&fw));
    }
    let mut seat_mode: Option<&'static str> = None;
    let mut dropped = false;
    if let Some(pu) = sh.push.as_mut() {
        pu.t += dt;
        let uu = pu.u0 + (pu.u1 - pu.u0) * smooth((pu.t / pu.dur).clamp(0.0, 1.0));
        let tt = p_at(pu.line, wm, uu);
        if let Some(r) = rel {
            let slt = fcomp(&tt, &finv(&sh.hold));
            let wt = wrist_for(&slt, &r);
            g.p.wt = Some(wt);
            g.owned = pin(g, free, 1.0, wr.as_ref(), Some(&wt));
        }
        if pu.mode == "tube" {
            g.p.gate_want = Some(1.0);
        }
        g.p.magnet = Some(PMag { u: uu, perp: 0.0, w: 1.0, mode: pu.mode });
        if pu.t >= pu.dur {
            seat_mode = Some(pu.mode);
        }
    } else if !grip {
        g.p.shell = Some(sh);
        p_drop_shell(s, g, sc, true);
        unpin(g);
        fin_off(g);
        dropped = true;
        sh = Shell { side: free, hold: [0.0; 7], multi: None, sl: None, f: None, v: [0.0; 3], px: None, push: None };
    } else if sh.multi.as_ref().is_some_and(|mt| mt.state != "ready") {
        let mc = sc.g("multi");
        g.p.magnet = None;
        let mt = sh.multi.as_mut().unwrap_or_else(|| unreachable!());
        if mt.state == "wait" {
            mt.t += dt;
            let rl = mc.f("release", 0.12);
            let w = if mt.pin_t.is_some() && mt.t < rl { 1.0 - smooth(mt.t / rl) } else { 0.0 };
            if w > 0.0 {
                let pt = mt.pin_t;
                g.owned = pin(g, free, w, wr.as_ref(), pt.as_ref());
            } else {
                unpin(g);
            }
            let mut gone = 0.0;
            if let (Some(seat), Some(w), Some(l)) = (mt.seat, wr, mt.line) {
                let from = l.v3("from").unwrap_or([0.0; 3]);
                let to = l.v3("to").unwrap_or(from);
                let mut fv = qrot(fq(wm), [to[0] - from[0], to[1] - from[1], to[2] - from[2]]);
                let fl = len3(fv);
                if fl > 1e-5 {
                    fv = fv.map(|v| v / fl);
                }
                let dv = [w[0] - seat[0], w[1] - seat[1], w[2] - seat[2]];
                let a = dv[0] * fv[0] + dv[1] * fv[1] + dv[2] * fv[2];
                gone = len3v(dv[0] - a * fv[0], dv[1] - a * fv[1], dv[2] - a * fv[2]) + (-a).max(0.0);
            }
            mt.gone = gone;
            if gone >= mc.f("away", 0.05) {
                (mt.state, mt.t) = ("roll", 0.0);
                unpin(g);
                play_snd(weapon, snd.s("roll"));
            }
        } else {
            mt.t += dt;
            let d = m_data(mc);
            let t = if let Some(d) = d { mc.fo("moveTime").or(d.fo("moveTime")).unwrap_or(0.42) } else { mc.f("rollTime", 0.22) };
            let u = (mt.t / t).clamp(0.0, 1.0);
            (mt.off, mt.u) = (1.0 - smooth(u), u);
            if u >= 1.0 {
                (mt.state, mt.off, mt.t, mt.u) = ("ready", 0.0, 0.0, 1.0);
            }
        }
    } else if let Some(fmv) = fm {
        let mg = sc.g("magnet");
        let (mut line, mut mode) = (sc.g("tube"), "tube");
        if sc.g("port").present() && !g.p.chambered && g.p.s >= 0.85 * travel {
            (line, mode) = (sc.g("port"), "port");
        }
        let pl = p_line(line, wm, &fmv);
        let a = qrot(fq(&fmv), [0.0, 0.0, 1.0]);
        let al = pl.map_or(-1.0, |(_, _, _, d)| a[0] * d[0] + a[1] * d[1] + a[2] * d[2]);
        g.p.near = pl.map(|(u, perp, _, _)| PNear { u, perp, ang: al.clamp(-1.0, 1.0).acos().to_degrees(), mode });
        let r = mg.f("radius", 0.05);
        let keep = g.p.magnet.is_some();
        let rr = if keep { r * mg.f("widen", 1.5) } else { r };
        match pl {
            Some((u, perp, tt, _)) if u > -mg.f("reach", 0.8) && u < 1.05 && perp < rr && al > mg.f("angle", 65.0).to_radians().cos() && (mode == "port" || am < cap) => {
                let mut w = ((1.0 - perp / r) * mg.f("gain", 3.0)).clamp(0.0, 1.0);
                if u < 0.0 {
                    w *= (1.0 + u / mg.f("reach", 0.8)).clamp(0.0, 1.0);
                }
                let slt = fcomp(&tt, &finv(&sh.hold));
                if let Some(rl) = rel {
                    let wt = wrist_for(&slt, &rl);
                    g.p.wt = Some(wt);
                    g.owned = pin(g, free, w, wr.as_ref(), Some(&wt));
                }
                g.p.magnet = Some(PMag { u, perp, w, mode });
                if mode == "tube" {
                    g.p.gate_want = Some(((u - 0.25) / 0.5).clamp(0.0, 1.0));
                }
                if u >= mg.f("seatAt", 0.80) && w > 0.5 {
                    play_snd(weapon, if mode == "port" { snd.s("shellInChamber").or(snd.s("shellIn")) } else { snd.s("shellIn") });
                    let u1 = if mode == "port" { mg.f("pushPort", 1.0) } else { mg.f("pushTo", 1.25) };
                    sh.push = Some(Push { t: 0.0, dur: mg.f("pushTime", 0.10), u0: u, u1: u1.max(u), line, mode });
                }
            }
            _ => {
                g.p.magnet = None;
                unpin(g);
            }
        }
    }
    if dropped {
        return;
    }
    if let Some(mode) = seat_mode {
        let n = am + 1;
        set_mag_ammo(s, weapon, n);
        g.p.our_add = true;
        g.p.ammo_last = Some(n);
        if mode == "port" {
            g.p.chambered = true;
        } else {
            g.p.gate_t = Some(0.5 * sc.g("gate").f("time", 0.25));
        }
        let more = sh.multi.as_ref().is_some_and(|mt| mt.list.len() > 1);
        if more {
            let line = sh.push.as_ref().map_or(sc.g("tube"), |p| p.line);
            let mt = sh.multi.as_mut().unwrap_or_else(|| unreachable!());
            mt.list.remove(0);
            (mt.state, mt.off, mt.t, mt.gone, mt.u) = ("wait", 1.0, 0.0, 0.0, 0.0);
            mt.line = Some(line);
            mt.seat = wr.map(|w| fp(&w));
            mt.pin_t = g.p.wt;
            sh.push = None;
            g.p.magnet = None;
            m_place(sc, mt);
        } else {
            if sh.multi.is_some() {
                item_drop(s, free, hand_slot(free));
            } else {
                hold_hide(s, Some(free));
            }
            super::belt::set_shell_taken(false, -1);
            g.p.magnet = None;
            unpin(g);
            g.owned = None;
            fin_off(g);
            return;
        }
    }
    g.p.shell = Some(sh);
}

// ---------------------------------------------------------------- the module's entry points
fn cleanup(s: &mut State, g: &mut Shotgun, cfg: &'static J, weapon: Option<&Handle>) {
    let sc = cfg.g("shotgun");
    match g.kind {
        Some("break") => b_reset_all(s, g, sc, weapon),
        Some("pump") => p_reset_all(s, g, sc, weapon),
        _ => {}
    }
    unpin(g);
    fin_reset(g);
    set_trg(s, 0);
    g.owned = None;
}
fn with<R>(s: &mut State, f: impl FnOnce(&mut State, &mut Shotgun) -> R) -> R {
    let mut g = std::mem::take(&mut s.sg);
    let r = f(s, &mut g);
    s.sg = g;
    r
}
/// M.frame: the shotgun's frame; (where it ended, the wrist this module pinned).
pub(super) fn frame(s: &mut State, weapon: &Handle, slot_comp: &Handle, holder: usize, dt: f64, cfg: &'static J) -> (&'static str, Option<usize>) {
    with(s, |s, g| {
        let sc = cfg.g("shotgun");
        let key = super::belt::entity_hash(weapon);
        let same_cfg = g.cfg.is_some_and(|c| std::ptr::eq(c, cfg));
        if key != g.key || !same_cfg {
            if let Some(old) = g.cfg {
                cleanup(s, g, old, None);
            }
            let kind = if sc.s("kind") == Some("break") { "break" } else { "pump" };
            (g.key, g.cfg, g.kind) = (key, Some(cfg), Some(kind));
            if kind == "break" {
                b_reset_all(s, g, sc, Some(weapon));
            } else {
                p_reset_all(s, g, sc, Some(weapon));
            }
        }
        let snd = sounds(cfg);
        let r = if g.kind == Some("break") { b_frame(s, g, sc, snd, weapon, slot_comp, holder, dt) } else { p_frame(s, g, sc, snd, weapon, holder, dt) };
        (r, g.owned)
    })
}
pub(super) fn lost(s: &mut State) {
    with(s, |s, g| {
        if let Some(old) = g.cfg {
            cleanup(s, g, old, None);
        }
        (g.key, g.cfg, g.kind) = (None, None, None);
    });
}
pub(super) fn cycle_free(s: &mut State) {
    cycle_free_g(&mut s.sg);
}
pub(super) fn yield_(s: &mut State, _weapon: &Handle) {
    with(s, |s, g| {
        if let Some(old) = g.cfg {
            cleanup(s, g, old, None);
        }
        g.key = None;
    });
}
pub(super) fn preview_stop(s: &mut State) {
    with(s, preview_stop_g);
}
fn preview_stop_g(s: &mut State, g: &mut Shotgun) {
    let Some(pv) = g.pv.take() else { return };
    item_drop(s, pv.side, hand_slot(pv.side));
    fin_reset(g);
}
#[allow(dead_code)]
/// M.previewHandful (test only): a pump's handful played in the hand without the gun.
pub(super) fn preview_handful(s: &mut State, sc: &'static J, side: usize, n: Option<usize>, hold: Option<f64>) -> &'static str {
    with(s, |s, g| {
        preview_stop_g(s, g);
        let mc = sc.g("multi");
        let Some(item) = mc.s("item") else { return "no multi on this gun" };
        if !item_hold(s, side, item, hand_slot(side)) {
            return "the item was not taken";
        }
        let mut mt = Multi::new(side, n.unwrap_or(mc.f("count", 2.0) as usize));
        mt.u = 1.0;
        g.pv = Some(Preview { sc, m: mt, side, t: 0.0, hold: hold.unwrap_or(1.2), phase: "hold" });
        "ok"
    })
}
fn preview_tick(s: &mut State, g: &mut Shotgun, dt: f64) {
    let Some(mut pv) = g.pv.take() else { return };
    let mc = pv.sc.g("multi");
    pv.t += dt;
    match pv.phase {
        "hold" => {
            if pv.t >= pv.hold {
                pv.t = 0.0;
                if !pv.m.list.is_empty() {
                    pv.m.list.remove(0);
                }
                if pv.m.list.is_empty() {
                    pv.phase = "empty";
                } else {
                    (pv.m.state, pv.m.off, pv.m.u, pv.phase) = ("wait", 1.0, 0.0, "gap");
                }
            }
        }
        "gap" => {
            if pv.t >= 0.35 {
                (pv.m.state, pv.phase, pv.t) = ("roll", "move", 0.0);
            }
        }
        "move" => {
            let t = if let Some(d) = m_data(mc) { mc.fo("moveTime").or(d.fo("moveTime")).unwrap_or(0.42) } else { mc.f("rollTime", 0.22) };
            let u = (pv.t / t).clamp(0.0, 1.0);
            (pv.m.off, pv.m.u) = (1.0 - smooth(u), u);
            if u >= 1.0 {
                (pv.m.state, pv.m.off, pv.m.u, pv.phase, pv.t) = ("ready", 0.0, 1.0, "hold", 0.0);
            }
        }
        "empty" if pv.t >= pv.hold => {
            g.pv = Some(pv);
            preview_stop_g(s, g);
            return;
        }
        _ => {}
    }
    m_place(pv.sc, &mut pv.m);
    if !pv.m.list.is_empty() {
        if !m_fingers(g, pv.sc, &pv.m, pv.side) {
            fin_want(g, pv.side, pv.sc.g("carry").s("pose"));
        }
    } else {
        fin_off(g);
    }
    fin_tick(g, dt, 0.15);
    g.pv = Some(pv);
}
/// M.tickBodies: the loose bodies, every frame whoever has the gun.
pub(super) fn tick_bodies(s: &mut State, dt: f64) {
    with(s, |s, g| body_tick(s, g, dt));
}
/// M.tickPreview: the handful played without a gun.
pub(super) fn tick_preview(s: &mut State, dt: f64) {
    with(s, |s, g| preview_tick(s, g, dt));
}
/// M.active: the shotgun's mechanism is ours.
pub(super) fn active(s: &State) -> bool {
    let g = &s.sg;
    match g.kind {
        Some("break") => g.b.act != "closed" || g.b.held.is_some() || g.b.settle > 0.0,
        Some("pump") => g.p.engaged || g.p.shell.is_some() || g.p.s > 0.0005,
        _ => false,
    }
}
pub(super) fn diag(s: &State) -> String {
    let g = &s.sg;
    let d = if g.diag.is_empty() { "-" } else { g.diag.as_str() };
    if let (Some(n), Some("break")) = (&g.b.near, g.kind) {
        return format!("{d} near[k{} d={:.3} p={:.3} a={:.0}]", n.k + 1, n.d, n.perp, n.ang);
    }
    if let (Some(q), Some("pump"), true) = (&g.p.near, g.kind, g.p.shell.is_some()) {
        return format!("{d} near[{} u={:.2} p={:.3} a={:.0}]", q.mode, q.u, q.perp, q.ang);
    }
    d.to_string()
}
