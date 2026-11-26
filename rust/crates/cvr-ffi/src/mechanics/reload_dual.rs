//! dual_wield.lua: two guns, one per hand. The plugin is told (SetVRDualWield, the left gun's muzzle, both records);
//! the hand reload is let go. A gun's magazine drops on its own button (Y taps for the left, B for the right) -- the
//! rounds out, the magazine falling with the reload's contact physics -- and goes back with a sharp downward swing of
//! that hand (the rounds from the reserve), or with the game's own reload.
use super::belt::{f4, m, native, player, qmul, qrot, Q, V3};
use super::reload_core::*;
use super::reload_data::Lua;
use crate::script_call::Value;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use serde_json::Value as J;

const SLOT: [&str; 2] = ["AttachmentSlots.WeaponLeft", "AttachmentSlots.WeaponRight"];
const Y_TAPS: i32 = 57;
const B_BTN: i32 = 157;
const SWING_SPEED: f64 = 2.0;
const SWING_SMOOTH: f64 = 0.03;
const SWING_ARM: f64 = 0.25;

#[derive(Default)]
struct DHand {
    hidden: Option<Vec<Handle>>,
    out: bool,
    t: f64,
    key: Option<u64>,
    taps: Option<f32>,
    btn_was: bool,
    pz: Option<f64>,
    vz: f64,
}
struct Fall {
    id: u64,
    mc: &'static J,
    ctr: V3,
    skin: Option<String>,
    q: Q,
    c: V3,
    v: V3,
    w: V3,
    t: f64,
    touch: f64,
    rest: Option<f64>,
    q0: Q,
    q1: Q,
    c0: V3,
    c1: V3,
}
#[derive(Default)]
pub(super) struct Dual {
    pub active: bool,
    published: bool,
    hands: [DHand; 2],
    falls: Vec<Fall>,
    litter: Vec<(u64, u32)>,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Dual {}

fn gun_in(pl: &Handle, side: usize) -> Handle {
    super::belt::in_slot(pl, SLOT[side])
}
fn mag_parts(gun: &Handle) -> Vec<Handle> {
    super::belt::components(gun).into_iter().filter(|c| super::belt::comp_name(c).contains("mag_std")).collect()
}
fn hide_mag(h: &mut DHand, gun: &Handle) {
    if h.hidden.is_some() {
        return;
    }
    let mut list = Vec::new();
    for c in mag_parts(gun) {
        if m::<bool>(&c, "IsEnabled", &[]) == Ok(true) && super::belt::toggle(&c, false) {
            list.push(c);
        }
    }
    h.hidden = Some(list);
}
fn show_mag(h: &mut DHand) {
    if let Some(list) = h.hidden.take() {
        for c in list {
            super::belt::toggle(&c, true);
        }
    }
}
fn mag_skin(gun: &Handle) -> Option<String> {
    for c in mag_parts(gun) {
        // SAFETY: the mesh component's CName property.
        let v = unsafe { crate::script_call::property(c.instance, "meshAppearance") }.map(|p| CName(unsafe { *(p as *const u64) }).text().to_string());
        if let Some(v) = v.filter(|v| !v.is_empty() && v != "default") {
            return Some(v);
        }
    }
    None
}
/// Where the gun's magazine sits: (position, orientation, the gun's own orientation).
fn seated(gun: &Handle, mc: &J) -> Option<(V3, Q, Q)> {
    let slot = mc.s("slot").unwrap_or("vrp_mag_slot");
    for c in super::belt::components(gun) {
        if !super::belt::class_name(&c).contains("SlotComponent") {
            continue;
        }
        if let Some((p, _)) = slot_world(&c, slot) {
            let wq = f4(m::<[f32; 4]>(gun, "GetWorldOrientation", &[]).ok()?);
            let mut sq = wq;
            if let Some(sr) = mc.v4("seatRot") {
                sq = qmul(sq, sr);
            }
            let o = mc.v3("originOffset").map_or([0.0; 3], |o| qrot(sq, o));
            return Some(([p[0] + o[0], p[1] + o[1], p[2] + o[2]], sq, wq));
        }
    }
    None
}
fn despawn_l(d: &mut Dual, id: u64, tries: u32) {
    if !despawn(id) {
        d.litter.push((id, tries + 1));
    }
}
fn sweep_litter(d: &mut Dual) {
    let keep = std::mem::take(&mut d.litter);
    for (id, tries) in keep {
        if tries < 300 {
            despawn_l(d, id, tries);
        }
    }
}
fn dnum(mc: &J, k: &str) -> f64 {
    if let Some(v) = mc.g("drop").fo(k) {
        return v;
    }
    let d = &DROP;
    match k {
        "radius" => d.radius,
        "flat" => d.flat,
        "probe" => d.probe,
        "arm" => d.arm,
        "inertia" => d.inertia,
        "bounce" => d.bounce,
        "friction" => d.friction,
        "spinAir" => d.spin_air,
        "drag" => d.drag,
        "restE" => d.rest_e,
        "spinDamp" => d.spin_damp,
        "touchRest" => d.touch_rest,
        "restV" => d.rest_v,
        "settle" => d.settle,
        "maxV" => d.max_v,
        "maxW" => d.max_w,
        _ => 0.0,
    }
}
fn dvec(mc: &J, k: &str) -> V3 {
    if let Some(v) = mc.g("drop").v3(k) {
        return v;
    }
    match k {
        "centre" => DROP.centre,
        "long" => DROP.long,
        _ => DROP.thin,
    }
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn settle(f: &mut Fall, hp: V3, hn: V3) {
    let q = f.q;
    let (lg, th) = (dvec(f.mc, "long"), dvec(f.mc, "thin"));
    let a1 = qrot(q, th);
    let n = if dot(a1, hn) < 0.0 { hn.map(|v| -v) } else { hn };
    let mut h2 = qrot(q, lg);
    let d2 = dot(h2, n);
    h2 = [h2[0] - d2 * n[0], h2[1] - d2 * n[1], h2[2] - d2 * n[2]];
    let mut hl = len3(h2);
    if hl < 1e-3 {
        h2 = qrot(q, cross(th, lg));
        let d2 = dot(h2, n);
        h2 = [h2[0] - d2 * n[0], h2[1] - d2 * n[1], h2[2] - d2 * n[2]];
        hl = len3(h2);
    }
    if hl < 1e-3 {
        return;
    }
    h2 = h2.map(|v| v / hl);
    let y = cross(h2, n);
    let t = basis_quat(n, y, h2);
    let fl = dnum(f.mc, "flat");
    (f.q0, f.q1, f.c0) = (q, t, f.c);
    f.c1 = [hp[0] + hn[0] * fl, hp[1] + hn[1] * fl, hp[2] + hn[2] * fl];
    f.rest = Some(0.0);
    (f.v, f.w) = ([0.0; 3], [0.0; 3]);
}
fn apply_skin(f: &mut Fall) {
    let Some(skin) = f.skin.clone() else { return };
    let Ok(e) = super::find_entity(f.id) else { return };
    if e.is_null() {
        return;
    }
    for c in super::belt::components(&e) {
        if super::belt::comp_name(&c) == "mag_mesh" {
            super::belt::set_cname_prop(&c, "meshAppearance", CName::hashed(&skin).0);
            crate::engine_objects::component_refresh_appearance(&c);
        }
    }
    f.skin = None;
}
/// One step of a dropped magazine; false once it is taken away.
fn fall_step(s: &mut State, f: &mut Fall, dt: f64, floor_z: f64) -> bool {
    f.t += dt;
    apply_skin(f);
    let h = dt.min(0.04);
    let mc = f.mc;
    if let Some(rest) = f.rest {
        let rest = rest + h;
        f.rest = Some(rest);
        let t = (rest / dnum(mc, "settle")).min(1.0);
        f.q = qslerp(f.q0, f.q1, t);
        f.c = [0, 1, 2].map(|k| f.c0[k] + (f.c1[k] - f.c0[k]) * t);
    } else {
        f.v[2] -= mc.f("gravity", 9.81) * h;
        let dg = (1.0 - dnum(mc, "drag") * h).max(0.0);
        f.v = f.v.map(|v| v * dg);
        let sg = (1.0 - dnum(mc, "spinAir") * h).max(0.0);
        f.w = f.w.map(|v| v * sg);
        if dot(f.w, f.w) > 1e-8 {
            let a = qmul([f.w[0] * 0.5 * h, f.w[1] * 0.5 * h, f.w[2] * 0.5 * h, 0.0], f.q);
            let q = [f.q[0] + a[0], f.q[1] + a[1], f.q[2] + a[2], f.q[3] + a[3]];
            let l = q.iter().map(|v| v * v).sum::<f64>().sqrt();
            if l > 1e-9 {
                f.q = q.map(|v| v / l);
            }
        }
        let c = f.c;
        let nx = [c[0] + f.v[0] * h, c[1] + f.v[1] * h, c[2] + f.v[2] * h];
        let stp = [nx[0] - c[0], nx[1] - c[1], nx[2] - c[2]];
        let sl = len3(stp);
        let a1 = qrot(f.q, dvec(mc, "long"));
        let (rr, arm) = (dnum(mc, "probe"), dnum(mc, "arm"));
        let fz = floor_z + mc.f("floorLift", 0.02);
        let mut touching: Vec<(f64, V3, V3, V3)> = Vec::new();
        for pi in -1i32..=1 {
            let o = a1.map(|v| v * arm * f64::from(pi));
            let p = [c[0] + o[0], c[1] + o[1], c[2] + o[2]];
            let q = [nx[0] + o[0], nx[1] + o[1], nx[2] + o[2]];
            let mut hit = None;
            if sl > 1e-4 {
                let kk = (sl + rr) / sl;
                hit = ray_hit(s, p, [p[0] + stp[0] * kk, p[1] + stp[1] * kk, p[2] + stp[2] * kk]);
                if hit.is_some_and(|(_, hn)| dot(f.v, hn) > -1e-4) {
                    hit = None;
                }
            }
            if hit.is_none() && f.v[2] < 0.0 && q[2] - rr <= fz {
                hit = Some(([q[0], q[1], fz], [0.0, 0.0, 1.0]));
            }
            if let Some((hp, hn)) = hit {
                let pen = rr + 0.001 - dot([q[0] - hp[0], q[1] - hp[1], q[2] - hp[2]], hn);
                if pen > 0.0 {
                    touching.push((pen, hp, hn, o));
                }
            }
        }
        f.c = nx;
        let ii = dnum(mc, "inertia");
        for (pen, _, hn, r) in &touching {
            let corr = pen - 0.001;
            if corr > 0.0 {
                f.c = [0, 1, 2].map(|k| f.c[k] + hn[k] * corr * 0.8);
            }
            let cv = [f.v[0] + (f.w[1] * r[2] - f.w[2] * r[1]), f.v[1] + (f.w[2] * r[0] - f.w[0] * r[2]), f.v[2] + (f.w[0] * r[1] - f.w[1] * r[0])];
            let vn = dot(cv, *hn);
            if vn < 0.0 {
                let mu = dnum(mc, "friction");
                let e2 = if -vn > dnum(mc, "restE") { dnum(mc, "bounce") } else { 0.0 };
                let cr = cross(*r, *hn);
                let jn = -(1.0 + e2) * vn / (1.0 + dot(cr, cr) / ii);
                f.v = [0, 1, 2].map(|k| f.v[k] + hn[k] * jn);
                f.w = [0, 1, 2].map(|k| f.w[k] + cr[k] * jn / ii);
                let tv = [cv[0] - vn * hn[0], cv[1] - vn * hn[1], cv[2] - vn * hn[2]];
                let tl = len3(tv);
                if tl > 1e-6 {
                    let u = tv.map(|v| v / tl);
                    let du = cross(*r, u);
                    let jt = (tl / (1.0 + dot(du, du) / ii)).min(mu * jn);
                    f.v = [0, 1, 2].map(|k| f.v[k] - u[k] * jt);
                    f.w = [0, 1, 2].map(|k| f.w[k] - du[k] * jt / ii);
                }
                let dl = (1.0 - dnum(mc, "spinDamp")).max(0.0);
                f.w = f.w.map(|v| v * dl);
                let (wl, wm) = (len3(f.w), dnum(mc, "maxW"));
                if wl > wm {
                    f.w = f.w.map(|v| v / wl * wm);
                }
            }
        }
        if let Some((_, hp, hn, _)) = touching.last().copied() {
            f.touch += h;
            if (len3(f.v) < dnum(mc, "restV") && len3(f.w) < 3.0) || f.touch > dnum(mc, "touchRest") {
                settle(f, hp, hn);
            }
        }
    }
    let o = qrot(f.q, f.ctr);
    place_id(f.id, [f.c[0] - o[0], f.c[1] - o[1], f.c[2] - o[2]], f.q);
    if f.t >= mc.f("litterTime", 3.0) {
        return false;
    }
    true
}
fn spawn_fall(d: &mut Dual, gun: &Handle, mc: &'static J) {
    if !mc.g("entity").present() || mc.g("eject").present() {
        return;
    }
    let Some((p, sq, wq)) = seated(gun, mc) else { return };
    let Some(id) = mc.s("dropEntity").or(mc.s("entity")).and_then(|e| spawn_at(e, p, sq)) else { return };
    let cen = mc.g("drop").v3("centre").unwrap_or(DROP.centre);
    let ctr = mc.v3("originOffset").map_or(cen, |o| [cen[0] - o[0], cen[1] - o[1], cen[2] - o[2]]);
    let cc = qrot(sq, ctr);
    let push = mc.f("dropPush", 1.1);
    let k = mc.v3("wellAxis").map_or([0.0, 0.0, -push], |wa| qrot(wq, wa).map(|v| v * push));
    d.falls.push(Fall {
        id,
        mc,
        ctr,
        skin: mag_skin(gun),
        q: sq,
        c: [p[0] + cc[0], p[1] + cc[1], p[2] + cc[2]],
        v: k,
        w: [0.0; 3],
        t: 0.0,
        touch: 0.0,
        rest: None,
        q0: sq,
        q1: sq,
        c0: p,
        c1: p,
    });
}
fn sound(gun: &Handle, cfg: Option<&J>, which: &str) {
    if let Some(n) = cfg.and_then(|c| c.g("sounds").s(which)) {
        play_snd(gun, Some(n));
    }
}
fn side_name(side: usize) -> &'static str {
    if side == 0 {
        "left"
    } else {
        "right"
    }
}
fn drop_mag(s: &mut State, d: &mut Dual, side: usize, gun: &Handle) {
    let cfg = cfg_for(s, gun);
    let n = mag_ammo(gun).unwrap_or(0);
    if n > 0 {
        set_mag_ammo(s, gun, 0);
    }
    if let Some(mc) = cfg.map(|c| c.g("mag")).filter(|m| m.present()) {
        spawn_fall(d, gun, mc);
    }
    hide_mag(&mut d.hands[side], gun);
    sound(gun, cfg, "magOut");
    (d.hands[side].out, d.hands[side].t) = (true, 0.0);
    log!("[DualWield] {} magazine dropped ({n} rounds) (from Rust)\n", side_name(side));
}
fn return_mag(s: &mut State, d: &mut Dual, side: usize, gun: &Handle) -> bool {
    let n = mag_capacity(gun).min(reserve_ammo(gun));
    if n <= 0 {
        return false;
    }
    let cfg = cfg_for(s, gun);
    set_mag_ammo(s, gun, n);
    show_mag(&mut d.hands[side]);
    sound(gun, cfg, "magIn");
    d.hands[side].out = false;
    log!("[DualWield] {} magazine back ({n} rounds) (from Rust)\n", side_name(side));
    true
}
fn hand_vz(h: &mut DHand, side: usize, dt: f64) -> f64 {
    let p = raw_hand(side);
    let Some(p) = p.filter(|p| p[3] > 0.5 && dt > 1e-4) else {
        h.pz = None;
        return 0.0;
    };
    let z = f64::from(p[2]);
    if let Some(pz) = h.pz {
        let a = 1.0 - (-dt / SWING_SMOOTH).exp();
        h.vz += ((z - pz) / dt - h.vz) * a;
    } else {
        h.vz = 0.0;
    }
    h.pz = Some(z);
    h.vz
}
fn hand_reloading(pl: &Handle, side: usize) -> bool {
    let bb = super::belt::h(pl, "GetPlayerStateMachineBlackboard", &[]);
    if bb.is_null() {
        return false;
    }
    (|| -> Result<i32, String> {
        let defs = super::boards::defs()?;
        let dd = super::boards::definition(&defs, "PlayerStateMachine")?;
        let f = super::boards::field(dd, if side == 0 { "LeftWeapon" } else { "RightWeapon" })?;
        super::boards::get::<i32>(&bb, "GetInt", f)
    })()
    .is_ok_and(|v| v == 2)
}
fn hand_frame(s: &mut State, d: &mut Dual, side: usize, pl: &Handle, dual: bool, dt: f64) {
    let gun = if dual { gun_in(pl, side) } else { Handle::null() };
    let key = if gun.is_null() { None } else { super::belt::entity_hash(&gun) };
    let h = &mut d.hands[side];
    if h.out && (gun.is_null() || key != h.key) {
        show_mag(h);
        h.out = false;
    }
    h.key = key;
    let edge = if side == 0 {
        let taps = super::belt::shared(Y_TAPS);
        let e = h.taps.is_some_and(|t| t != taps);
        h.taps = Some(taps);
        e
    } else {
        let btn = super::belt::shared(B_BTN) > 0.5;
        let e = btn && !h.btn_was;
        h.btn_was = btn;
        e
    };
    let vz = hand_vz(h, side, dt);
    if gun.is_null() {
        return;
    }
    let reloading = hand_reloading(pl, side);
    if d.hands[side].out {
        d.hands[side].t += dt;
        let t = d.hands[side].t;
        if reloading || (t > 0.3 && mag_ammo(&gun).unwrap_or(0) > 0) {
            show_mag(&mut d.hands[side]);
            d.hands[side].out = false;
            log!("[DualWield] {} magazine back with the game's reload (from Rust)\n", side_name(side));
        } else if t > SWING_ARM && vz < -SWING_SPEED {
            return_mag(s, d, side, &gun);
        }
    } else if edge && !reloading {
        drop_mag(s, d, side, &gun);
    }
}
fn publish_left_muzzle(gun: &Handle) {
    let zero = || native::<()>("SetVRLeftMuzzle", &[Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0)]);
    // SAFETY: the gun, held.
    let Some(f) = (unsafe { crate::script_call::find_function(gun.instance, "GetMuzzleSlotWorldTransform", 0) }) else {
        zero();
        return;
    };
    let mut xf = WorldTransform([0; 8]);
    // SAFETY: the gun, held; the transform is a plain value written into our storage.
    if !unsafe { crate::script_call::invoke_into(gun.instance, f, &[], (&mut xf as *mut WorldTransform).cast()) } {
        zero();
        return;
    }
    let p = super::weapon::muzzle_words_position(&xf.0);
    let q = [4, 5, 6, 7].map(|i| f32::from_bits(xf.0[i]));
    native::<()>(
        "SetVRLeftMuzzle",
        &[Value::Float(p[0]), Value::Float(p[1]), Value::Float(p[2]), Value::Float(q[0]), Value::Float(q[1]), Value::Float(q[2]), Value::Float(q[3])],
    );
}
fn record_hash(gun: &Handle) -> i32 {
    super::belt::item_id(gun).map_or(0, |id| (id[0] & 0xFFFF_FFFF) as u32 as i32)
}
fn publish_records(pl: &Handle, left: &Handle) {
    let right = if left.is_null() { Handle::null() } else { gun_in(pl, 1) };
    let l = if left.is_null() { 0 } else { record_hash(left) };
    let r = if right.is_null() { 0 } else { record_hash(&right) };
    // the guns themselves first (the shot's AttackData names its weapon), then their records
    native::<()>("SetVRDualWieldWeapons", &[Value::Handle(left), Value::Handle(&right)]);
    native::<()>("SetVRDualWieldRecords", &[Value::Int32(l), Value::Int32(r)]);
}
/// M.frame: true while two guns are out.
pub(super) fn frame(d: &mut Dual, s: &mut State, dt: f64) -> bool {
    let pl = player();
    let dual = !pl.is_null() && super::belt::st::<bool>("GameObject", "IsDualWielding", &[Value::Handle(&pl)]) == Some(true);
    native::<()>("SetVRDualWield", &[Value::Int32(i32::from(dual))]);
    d.active = dual;
    let gun = if dual { gun_in(&pl, 0) } else { Handle::null() };
    if !gun.is_null() {
        publish_left_muzzle(&gun);
    } else if d.published {
        native::<()>("SetVRLeftMuzzle", &[Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0), Value::Float(0.0)]);
    }
    if !gun.is_null() || d.published {
        publish_records(&pl, &gun);
    }
    d.published = !gun.is_null();
    for side in 0..2 {
        hand_frame(s, d, side, &pl, dual, dt);
    }
    if !d.falls.is_empty() {
        let floor_z = m::<[f32; 4]>(&pl, "GetWorldPosition", &[]).map_or(0.0, |p| f64::from(p[2]));
        let falls = std::mem::take(&mut d.falls);
        for mut f in falls {
            if fall_step(s, &mut f, dt, floor_z) {
                d.falls.push(f);
            } else {
                despawn_l(d, f.id, 0);
            }
        }
    }
    sweep_litter(d);
    dual
}
pub(super) fn release(d: &mut Dual) {
    native::<()>("SetVRDualWieldWeapons", &[Value::Handle(&Handle::null()), Value::Handle(&Handle::null())]);
    for h in d.hands.iter_mut() {
        show_mag(h);
        h.out = false;
    }
    let falls = std::mem::take(&mut d.falls);
    for f in falls {
        despawn_l(d, f.id, 0);
    }
}
