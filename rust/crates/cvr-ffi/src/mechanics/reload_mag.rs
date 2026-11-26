//! reload.lua's M.frame, part B: the magazine (a config with mag.enabled and mag.entity). It comes out by the free
//! hand's grip at its take point (or the release button drops it, or a tipped open cylinder spills it), rides the
//! hand -- on the player's carrier component, as an item in the hand slot, or as an entity placed every frame and led
//! by the player's own step -- in the hold the recorded stages give for its depth, and goes back in when it is brought
//! down its well: the hand magnetised onto the well's line (bent or turned where the well is), seated at the seat
//! fraction (or gliding home by itself), the rounds loaded from the reserve. Let go anywhere else it falls with its own
//! contact physics (three probes along it, impulses with friction and spin, settling flat) and is swept away. Empty,
//! a fresh one is taken from the belt's pouch or the body.
use super::belt::{f3, f4, m, native, qmul, qrot, V3, Q};
use super::reload_core::*;
use super::reload_data::{self as data, Lua, Pose};
use super::reload_frame::{Ctx, Glide};
use crate::script_call::Value;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use serde_json::Value as J;

const LEFT_SLOT: &str = "AttachmentSlots.WeaponLeft";
const RIGHT_SLOT: &str = "AttachmentSlots.WeaponRight";
fn hand_slot(side: usize) -> &'static str {
    if side == 0 {
        LEFT_SLOT
    } else {
        RIGHT_SLOT
    }
}
fn neg(q: Q) -> Q {
    [-q[0], -q[1], -q[2], q[3]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn qn(q: Q) -> Q {
    let l = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    if l > 1e-9 {
        q.map(|c| c / l)
    } else {
        q
    }
}
fn lerp_q(a: Q, b: Q, t: f64) -> Q {
    let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let sg = if d < 0.0 { -1.0 } else { 1.0 };
    [0, 1, 2, 3].map(|i| a[i] + (b[i] * sg - a[i]) * t)
}
/// A vector in the gun's basis (f, d, s) to model space.
fn in_basis(c: &Ctx, a: V3) -> V3 {
    [
        c.gf[0] * a[0] + c.gd[0] * a[1] + c.gs[0] * a[2],
        c.gf[1] * a[0] + c.gd[1] * a[1] + c.gs[1] * a[2],
        c.gf[2] * a[0] + c.gd[2] * a[1] + c.gs[2] * a[2],
    ]
}
/// The DROP body's number `k`, overridden by the config's mag.drop.
fn dcfg(mc: &J, k: &str) -> f64 {
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
fn dcfg3(mc: &J, k: &str) -> V3 {
    if let Some(v) = mc.g("drop").v3(k) {
        return v;
    }
    match k {
        "centre" => DROP.centre,
        "long" => DROP.long,
        "thin" => DROP.thin,
        _ => [0.0; 3],
    }
}
/// The magazine's local frame: the hold the stages give for the depth (or the tuner's pinned one), rolled when held.
#[allow(dead_code)]
struct MagCtx<'a> {
    mc: &'a J,
    w: V3,
    hm: Option<V3>,
    tw: Option<(V3, Q)>,
}
fn vpick<'a>(s: &State, stage: &'a J, key: &str) -> &'a J {
    if s.frame.mag.variant == Some(2) {
        let k2 = stage.g(&format!("{key}2"));
        if k2.present() {
            return k2;
        }
    }
    stage.g(key)
}
fn tuned(s: &State, o: V3, q: Q) -> (V3, Q) {
    let t = &s.tune;
    if t.d == [0.0; 3] && t.r == [0.0; 3] {
        return (o, q);
    }
    let o2 = add(o, t.d);
    let mut q2 = q;
    for (deg, ax) in [(t.r[0], [1.0, 0.0, 0.0]), (t.r[1], [0.0, 1.0, 0.0]), (t.r[2], [0.0, 0.0, 1.0])] {
        let h = deg.to_radians() * 0.5;
        let sn = h.sin();
        q2 = qmul(q2, [ax[0] * sn, ax[1] * sn, ax[2] * sn, h.cos()]);
    }
    (o2, q2)
}
fn stage_bracket<'a>(st: &'a [J], d: f64) -> (&'a J, &'a J, f64) {
    let first = &st[0];
    let last = &st[st.len() - 1];
    if d >= first.f("d", 0.0) {
        return (first, first, 0.0);
    }
    if d <= last.f("d", 0.0) {
        return (last, last, 0.0);
    }
    for i in 0..st.len() - 1 {
        let (a, b) = (&st[i], &st[i + 1]);
        if d <= a.f("d", 0.0) && d >= b.f("d", 0.0) {
            let span = a.f("d", 0.0) - b.f("d", 0.0);
            return (a, b, if span > 1e-6 { (a.f("d", 0.0) - d) / span } else { 0.0 });
        }
    }
    (first, first, 0.0)
}
fn hold_at_raw(s: &State, mc: &J) -> (V3, Q) {
    let st = mc.g("poseStages").arr();
    let fallback = || tuned(s, mc.v3("holdOffset").unwrap_or([0.0; 3]), mc.v4("holdRotBasis").unwrap_or([0.0, 0.0, 0.0, 1.0]));
    if st.is_empty() || !st[0].g("off").present() {
        return fallback();
    }
    let d = if s.tune.stage >= 0.0 { s.tune.stage } else { s.frame.mag.depth.unwrap_or(0.0).max(0.0) };
    let (a, b, t) = stage_bracket(st, d);
    let v3 = |j: &J| -> Option<V3> {
        let a = j.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    };
    let v4 = |j: &J| -> Option<Q> {
        let a = j.as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?, a.get(3)?.as_f64()?])
    };
    let (Some(ao), Some(bo)) = (v3(vpick(s, a, "off")), v3(vpick(s, b, "off"))) else { return fallback() };
    let ar = v4(vpick(s, a, "rot")).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let br = v4(vpick(s, b, "rot")).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    if t <= 0.001 {
        return tuned(s, ao, ar);
    }
    let o = [ao[0] + (bo[0] - ao[0]) * t, ao[1] + (bo[1] - ao[1]) * t, ao[2] + (bo[2] - ao[2]) * t];
    let q = lerp_q(ar, br, t);
    let l = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    tuned(s, o, if l > 1e-6 { q.map(|c| c / l) } else { q })
}
fn hold_at(s: &State, mc: &J) -> (V3, Q) {
    let (o, r) = hold_at_raw(s, mc);
    if let (Some(rr), true) = (s.frame.mag.roll, s.frame.mag.state == "hand" || s.frame.mag.state == "glide") {
        return (o, qmul(r, rr));
    }
    (o, r)
}
/// The player's hand slot (ItemAttachmentSlots WeaponLeft / WeaponRight or the config's) in the world.
fn hold_slot_tr(s: &mut State, mc: &J, side: usize) -> Option<(V3, Q)> {
    if s.frame.mag.slot_comp.is_null() {
        s.frame.mag.slot_comp = super::belt::components(&super::belt::player())
            .into_iter()
            .find(|c| super::belt::comp_name(c).contains("ItemAttachmentSlots"))
            .unwrap_or_else(Handle::null);
    }
    if s.frame.mag.slot_comp.is_null() {
        return None;
    }
    let nm = if side == 0 { mc.s("holdSlot").unwrap_or("WeaponLeft") } else { mc.s("holdSlotRight").unwrap_or("WeaponRight") };
    let r = slot_world(&s.frame.mag.slot_comp, nm);
    if r.is_none() {
        s.frame.mag.slot_comp = Handle::null();
    }
    r
}
fn mag_ent(s: &State) -> Handle {
    s.frame.mag.ent_id.and_then(|id| super::find_entity(id).ok()).unwrap_or_else(Handle::null)
}
/// The skin the gun's magazine is drawn in: its first mag_std component not on `default`.
fn mag_skin(weapon: &Handle) -> Option<String> {
    for c in super::belt::components(weapon) {
        if super::belt::comp_name(&c).contains("mag_std") {
            // SAFETY: the mesh component's CName property.
            let v = unsafe { crate::script_call::property(c.instance, "meshAppearance") }.map(|p| CName(unsafe { *(p as *const u64) }).text().to_string());
            if let Some(v) = v.filter(|v| !v.is_empty() && v != "default") {
                return Some(v);
            }
        }
    }
    None
}
fn apply_skin(s: &mut State) {
    let Some(skin) = s.frame.mag.skin_want.clone() else { return };
    let e = mag_ent(s);
    if e.is_null() {
        return;
    }
    for c in super::belt::components(&e) {
        if super::belt::comp_name(&c) == "mag_mesh" {
            super::belt::set_cname_prop(&c, "meshAppearance", CName::hashed(&skin).0);
            crate::engine_objects::component_refresh_appearance(&c);
        }
    }
    s.frame.mag.skin_want = None;
}
fn spawn_mag(s: &mut State, weapon: &Handle, mc: &J, p: V3, q: Q, ent: Option<&str>) -> bool {
    if s.frame.mag.ent_id.is_some() {
        return true;
    }
    let path = ent.or(mc.s("entity")).unwrap_or("");
    s.frame.mag.ent_id = spawn_at(path, p, q);
    s.frame.mag.hold = None;
    lead_reset(s, "hand");
    s.frame.mag.skin_want = mag_skin(weapon);
    apply_skin(s);
    s.frame.mag.ent_id.is_some()
}
fn despawn_mag(s: &mut State) {
    if let Some(id) = s.frame.mag.ent_id.take() {
        if !despawn(id) {
            s.litter.push(id);
        }
    }
}
/// placeMag: the held magazine where the hand has it -- on the carrier, the item, or as an entity led by the step.
#[allow(clippy::too_many_arguments)]
fn place_mag(s: &mut State, c: &Ctx, p: V3, q: Q, side: Option<usize>, l: Option<(V3, Q)>) -> bool {
    let on_carrier = side.is_some_and(|sd| s.hold.on[sd]) && l.is_some();
    let on_item = side.is_some_and(|sd| s.item.name[sd].is_some()) && l.is_some();
    let e = mag_ent(s);
    if e.is_null() && !on_carrier && !on_item {
        return false;
    }
    if !e.is_null() && s.frame.mag.skin_want.is_some() {
        apply_skin(s);
    }
    let iv = 1.0 / c.dt.max(1e-3);
    match &mut s.frame.mag.hold {
        Some(ph) => {
            let a = 0.45;
            for k in 0..3 {
                ph[7 + k] += ((p[k] - ph[k]) * iv - ph[7 + k]) * a;
            }
            let pq = [ph[3], ph[4], ph[5], ph[6]];
            let si = if q[0] * pq[0] + q[1] * pq[1] + q[2] * pq[2] + q[3] * pq[3] < 0.0 { -1.0 } else { 1.0 };
            let d = qmul(q.map(|v| v * si), neg(pq));
            for k in 0..3 {
                ph[10 + k] += (2.0 * d[k] * iv - ph[10 + k]) * a;
            }
            ph[..3].copy_from_slice(&p);
            ph[3..7].copy_from_slice(&q);
        }
        None => {
            let mut ph = [0.0; 13];
            ph[..3].copy_from_slice(&p);
            ph[3..7].copy_from_slice(&q);
            s.frame.mag.hold = Some(ph);
        }
    }
    if let (true, Some(sd), Some((lp, lk))) = (on_carrier, side, l) {
        hold_place(s, sd, lp, lk);
        s.frame.mag.diag.push_str(" carrier");
        return true;
    }
    if let (true, Some(sd), Some((lp, lk))) = (on_item, side, l) {
        let ok = item_place(s, sd, hand_slot(sd), lp, lk);
        s.frame.mag.diag.push_str(if ok { " item" } else { " item(waiting)" });
        return true;
    }
    let (da, dad) = fresh_delta(c.b);
    let (a, step) = lead_world(s, "hand", add(p, da));
    let anchor = if dad >= 0.0 {
        format!("{:.0}mm", 1000.0 * dad)
    } else if dad > -1.5 {
        "no-native".into()
    } else {
        "rejected".into()
    };
    let lead = 1000.0 * len3(sub(sub(a, p), da));
    s.frame.mag.diag.push_str(&format!(" anchor={anchor} step={:.0}mm lead={lead:.0}mm", 1000.0 * step));
    place_ent(&e, a, q)
}
/// gunMag: the gun's own magazine shown or hidden (its bones scaled, or its show track held at 0 by us).
fn gun_mag(mc: &J, show: bool) -> bool {
    let which = mc.f("which", 0.0) as i32;
    let hide = mc.g("hideScale").arr();
    if !hide.is_empty() {
        for bn in hide {
            rig_write_scale(which, bn.as_f64().unwrap_or(0.0) as i32, if show { 0.0 } else { 0.001 });
        }
        return true;
    }
    let trk = mc.f("showTrack", 0.0) as i32;
    if show {
        track_write(which, trk, 1.0, 0);
    } else {
        track_write(which, trk, 0.0, 1);
    }
    true
}
/// wristToSlot: the hand slot's pose in the wrist's frame (the measured one, or the rig's bind when it is off).
fn wrist_to_slot(side: usize, raw: bool) -> Option<(V3, Q)> {
    let (bw, bs) = if side == 0 { (23, 26) } else { (24, 28) };
    let (pw, ps) = (bone_pos(bw)?, bone_pos(bs)?);
    let (qw, qs) = (bone_rot(bw)?, bone_rot(bs)?);
    if pw[3] <= 0.5 || ps[3] <= 0.5 {
        return None;
    }
    let e = qmul(neg(qw), qs);
    let r = qrot(neg(qw), sub(f3(ps), f3(pw)));
    if !raw {
        if let Some(b) = slot_bind(side, r, e) {
            return Some(([b[0], b[1], b[2]], [b[3], b[4], b[5], b[6]]));
        }
    }
    Some((r, e))
}
fn origin_shift(mc: &J, q: Q) -> V3 {
    mc.v3("originOffset").map_or([0.0; 3], |o| qrot(q, o))
}
fn mag_base_off(s: &State, mc: &J) -> V3 {
    let st = mc.g("poseStages").arr();
    st.first()
        .and_then(|s0| {
            let a = vpick(s, s0, "off").as_array()?;
            Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
        })
        .or(mc.v3("holdOffset"))
        .unwrap_or([0.0; 3])
}
/// magFromRaw: where the held magazine would be from the raw (tracked) wrist.
fn mag_from_raw(s: &State, mc: &J, side: usize, base: bool) -> Option<V3> {
    let p = raw_hand(side)?;
    let q = f4(native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(side as i32)])?);
    let (r, e) = wrist_to_slot(side, false)?;
    let sq = qmul(q, e);
    let a2 = qrot(q, r);
    let ho = hand_hold(side, Some(if base { mag_base_off(s, mc) } else { hold_at(s, mc).0 })).unwrap_or([0.0; 3]);
    let o = qrot(sq, ho);
    Some(add(add(f3(p), a2), o))
}
/// The weapon's orientation in model space.
fn weapon_model_q(c: &Ctx) -> Option<Q> {
    m::<[f32; 4]>(&c.weapon, "GetWorldOrientation", &[]).ok().map(|wq| qmul(c.c, f4(wq)))
}
/// magWristTarget: the wrist pose that puts the held magazine seated in the gun (roll `extra` on top).
fn mag_wrist_target(s: &State, c: &Ctx, mc: &J, w: V3, side: usize, extra: Option<Q>) -> Option<(V3, Q)> {
    c.db?;
    let wm = weapon_model_q(c)?;
    let bq = basis_quat(c.gf, c.gd, c.gs);
    let r = qmul(neg(bq), wm);
    let (_, mut hb) = hold_at(s, mc);
    if let Some(x) = extra {
        hb = qmul(hb, x);
    }
    let k = hand_hold_rot(side, qmul(hb, r));
    let mut mq = wm;
    if let Some(sr) = mc.v4("seatRot") {
        mq = qmul(wm, sr);
    }
    let sq = qmul(mq, neg(k));
    let ho = hand_hold(side, Some(hold_at(s, mc).0)).unwrap_or([0.0; 3]);
    let o = qrot(sq, ho);
    let s2 = sub(w, o);
    let (rv, e) = wrist_to_slot(side, false)?;
    let t = qmul(sq, neg(e));
    let v = qrot(t, rv);
    Some((sub(s2, v), t))
}
fn roll_ways(mc: &J) -> Option<Vec<Q>> {
    if let Some(ws) = mc.g("rollWays").as_array() {
        return Some(ws.iter().filter_map(|w| Some([w.i(1).as_f64()?, w.i(2).as_f64()?, w.i(3).as_f64()?, w.i(4).as_f64()?])).collect());
    }
    let rs = mc.g("rollSym");
    if rs.present() {
        let a = rs.v3("axis").unwrap_or([0.0, 1.0, 0.0]);
        let n = rs.f("n", 2.0) as i32;
        let l = len3(a);
        if l < 1e-6 || n < 2 {
            return None;
        }
        return Some(
            (1..n)
                .map(|k| {
                    let h = std::f64::consts::PI * f64::from(k) / f64::from(n);
                    let sn = h.sin() / l;
                    [a[0] * sn, a[1] * sn, a[2] * sn, h.cos()]
                })
                .collect(),
        );
    }
    mc.v4("rollAlt").map(|q| vec![q])
}
fn pick_roll(s: &mut State, c: &Ctx, mc: &J, w: V3, side: usize) -> Option<Q> {
    let ways = roll_ways(mc)?;
    let Some(hq) = native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(side as i32)]).map(f4) else { return s.frame.mag.roll };
    let keep = s.frame.mag.roll.take();
    let (mut best, mut bd) = (None, None::<f64>);
    for k in 0..=ways.len() {
        let wy = if k > 0 { Some(ways[k - 1]) } else { None };
        if let Some((_, q)) = mag_wrist_target(s, c, mc, w, side, wy) {
            let d = (hq[0] * q[0] + hq[1] * q[1] + hq[2] * q[2] + hq[3] * q[3]).abs();
            if bd.is_none_or(|b| d > b) {
                (best, bd) = (wy, Some(d));
            }
        }
    }
    s.frame.mag.roll = keep;
    if bd.is_none() {
        return keep;
    }
    best
}
fn pick_variant(s: &mut State, c: &Ctx, mc: &J, w: V3, side: usize) -> Option<i32> {
    let st = mc.g("poseStages").arr();
    if !st.first().is_some_and(|s0| s0.g("off2").present()) {
        return None;
    }
    let hp = f3(raw_hand(side)?);
    let hq = f4(native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(side as i32)])?);
    let keep = s.frame.mag.variant;
    let (mut best, mut bi) = (None::<f64>, 1);
    for v in 1..=2 {
        s.frame.mag.variant = Some(v);
        if let Some((t, q)) = mag_wrist_target(s, c, mc, w, side, None) {
            let d = len3(sub(t, hp));
            let dt = (hq[0] * q[0] + hq[1] * q[1] + hq[2] * q[2] + hq[3] * q[3]).abs().min(1.0);
            let sc = d + 2.0 * dt.acos() * 0.05;
            if best.is_none_or(|b| sc < b) {
                (best, bi) = (Some(sc), v);
            }
        }
    }
    s.frame.mag.variant = keep;
    Some(bi)
}
fn mag_release(side: Option<usize>) {
    let Some(sd) = side else { return };
    stop_model(sd, false, [0.0; 3]);
    stop_rot(sd, 0, [0.0, 0.0, 0.0, 1.0]);
}
/// magMagnet: the hand pulled by `w` onto the wrist target (the gun hand never).
fn mag_magnet(s: &mut State, c: &Ctx, x: &MagCtx, side: usize, w: f64, at: Option<V3>, aq: Option<Q>) {
    let tw = x.tw;
    let target = at.or(tw.map(|t| t.0));
    let tq = aq.or(tw.map(|t| t.1));
    if side == c.holder {
        mag_release(s.frame.mag.mag_side);
        (s.frame.mag.mag_side, s.frame.mag.magnet) = (None, 0);
        return;
    }
    if s.frame.mag.mag_side.is_some_and(|m| m != side) {
        mag_release(s.frame.mag.mag_side);
        (s.frame.mag.mag_side, s.frame.mag.magnet) = (None, 0);
    }
    let (Some(t), Some(hm), true) = (target, x.hm, w > 0.0) else {
        if s.frame.mag.magnet > 0 {
            mag_release(Some(side));
        }
        (s.frame.mag.magnet, s.frame.mag.mag_side) = (0, None);
        return;
    };
    let p = add(hm, scale(sub(t, hm), w));
    s.owned_hand = Some(side);
    stop_model(side, true, p);
    if let (Some(hq), Some(tq)) = (native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(side as i32)]).map(f4), tq) {
        let q = lerp_q(hq, tq, w);
        let l = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        if l > 1e-6 {
            stop_rot(side, 1, q.map(|v| v / l));
        }
    }
    (s.frame.mag.magnet, s.frame.mag.mag_side) = (if w > 0.0 { 1 } else { 0 }, Some(side));
    s.frame.mag.magnet_w = w;
}
fn drop_centre(mc: &J) -> V3 {
    let c = dcfg3(mc, "centre");
    match mc.v3("originOffset") {
        Some(o) => sub(c, o),
        None => c,
    }
}
fn drop_clear(s: &mut State) {
    s.flick.cancel();
    let Some(id) = s.frame.mag.drop_id.take() else { return };
    if !despawn(id) {
        s.litter.push(id);
    }
    let mg = &mut s.frame.mag;
    (mg.drop_p, mg.drop_v, mg.drop_q, mg.drop_t) = (None, None, None, 0.0);
    (mg.drop_c, mg.drop_w, mg.drop_rest, mg.drop_hits, mg.drop_touch) = (None, None, None, 0, None);
}
/// dropSettle: the magazine laid flat on the surface it came to rest on, its long axis kept in that plane.
fn drop_settle(s: &mut State, mc: &J, hp: V3, hn: V3) {
    let Some(q) = s.frame.mag.drop_q else { return };
    let (lg, th) = (dcfg3(mc, "long"), dcfg3(mc, "thin"));
    let a1 = qrot(q, th);
    let n = if dot(a1, hn) < 0.0 { scale(hn, -1.0) } else { hn };
    let mut h2 = qrot(q, lg);
    let d2 = dot(h2, n);
    h2 = sub(h2, scale(n, d2));
    let mut hl = len3(h2);
    if hl < 1e-3 {
        h2 = qrot(q, cross(th, lg));
        let d2 = dot(h2, n);
        h2 = sub(h2, scale(n, d2));
        hl = len3(h2);
    }
    if hl < 1e-3 {
        return;
    }
    h2 = scale(h2, 1.0 / hl);
    let y = cross(h2, n);
    let t = basis_quat(n, y, h2);
    let fl = dcfg(mc, "flat");
    let mg = &mut s.frame.mag;
    mg.drop_q0 = Some(q);
    mg.drop_q1 = Some(t);
    mg.drop_c0 = mg.drop_c;
    mg.drop_c1 = Some(add(hp, scale(hn, fl)));
    mg.drop_rest = Some(0.0);
    mg.drop_v = Some([0.0; 3]);
    mg.drop_w = Some([0.0; 3]);
}
/// dropTick: one step of the falling magazine.
fn drop_tick(s: &mut State, c: &Ctx, mc: &J) {
    let (Some(id), Some(mut cp)) = (s.frame.mag.drop_id, s.frame.mag.drop_c) else { return };
    s.frame.mag.drop_t += c.dt;
    let h = c.dt.min(0.04);
    let mut v = s.frame.mag.drop_v.unwrap_or([0.0; 3]);
    let mut w = s.frame.mag.drop_w.unwrap_or([0.0; 3]);
    let mut q = s.frame.mag.drop_q.unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let ctr = drop_centre(mc);
    if let Some(rest) = s.frame.mag.drop_rest {
        let rest = rest + h;
        s.frame.mag.drop_rest = Some(rest);
        let t = (rest / dcfg(mc, "settle")).min(1.0);
        if let (Some(q0), Some(q1), Some(c0), Some(c1)) = (s.frame.mag.drop_q0, s.frame.mag.drop_q1, s.frame.mag.drop_c0, s.frame.mag.drop_c1) {
            q = qslerp(q0, q1, t);
            cp = [0, 1, 2].map(|k| c0[k] + (c1[k] - c0[k]) * t);
        }
    } else {
        v[2] -= mc.f("gravity", 9.81) * h;
        let dg = (1.0 - dcfg(mc, "drag") * h).max(0.0);
        v = scale(v, dg);
        let sg = (1.0 - dcfg(mc, "spinAir") * h).max(0.0);
        w = scale(w, sg);
        if dot(w, w) > 1e-8 {
            let a = qmul([w[0] * 0.5 * h, w[1] * 0.5 * h, w[2] * 0.5 * h, 0.0], q);
            q = qn([q[0] + a[0], q[1] + a[1], q[2] + a[2], q[3] + a[3]]);
        }
        let nxt = add(cp, scale(v, h));
        let stp = sub(nxt, cp);
        let sl = len3(stp);
        let a1 = qrot(q, dcfg3(mc, "long"));
        let rr = dcfg(mc, "probe");
        let arm = dcfg(mc, "arm");
        let fz = c.b[2] + mc.f("floorLift", 0.02);
        let mut touching: Vec<(f64, V3, V3, V3)> = Vec::new();
        for pi in -1i32..=1 {
            let o = scale(a1, arm * f64::from(pi));
            let p0 = add(cp, o);
            let q0 = add(nxt, o);
            let mut hit: Option<(V3, V3)> = None;
            if sl > 1e-4 {
                let kk = (sl + rr) / sl;
                hit = ray_hit(s, p0, add(p0, scale(stp, kk)));
                if hit.is_some_and(|(_, hn)| dot(v, hn) > -1e-4) {
                    hit = None;
                }
            }
            if hit.is_none() && v[2] < 0.0 && q0[2] - rr <= fz {
                hit = Some(([q0[0], q0[1], fz], [0.0, 0.0, 1.0]));
            }
            if let Some((hp2, hn2)) = hit {
                let pen = rr + 0.001 - dot(sub(q0, hp2), hn2);
                if pen > 0.0 {
                    touching.push((pen, hp2, hn2, o));
                }
            }
        }
        cp = nxt;
        let best = touching.last().copied();
        let ii = dcfg(mc, "inertia");
        for (pen, _, hn, r) in &touching {
            let corr = pen - 0.001;
            if corr > 0.0 {
                cp = add(cp, scale(*hn, corr * 0.8));
            }
            let cv = add(v, cross(w, *r));
            let vn = dot(cv, *hn);
            if vn < 0.0 {
                let mu = dcfg(mc, "friction");
                let e2 = if -vn > dcfg(mc, "restE") { dcfg(mc, "bounce") } else { 0.0 };
                let cr = cross(*r, *hn);
                let kn = 1.0 + dot(cr, cr) / ii;
                let jn = -(1.0 + e2) * vn / kn;
                v = add(v, scale(*hn, jn));
                w = add(w, scale(cr, jn / ii));
                let tv = sub(cv, scale(*hn, vn));
                let tl = len3(tv);
                if tl > 1e-6 {
                    let u = scale(tv, 1.0 / tl);
                    let du = cross(*r, u);
                    let kt = 1.0 + dot(du, du) / ii;
                    let jt = (tl / kt).min(mu * jn);
                    v = sub(v, scale(u, jt));
                    w = sub(w, scale(du, jt / ii));
                }
                let dl = (1.0 - dcfg(mc, "spinDamp")).max(0.0);
                w = scale(w, dl);
                let wl = len3(w);
                let wm = dcfg(mc, "maxW");
                if wl > wm {
                    w = scale(w, wm / wl);
                }
            }
            s.frame.mag.drop_hits += 1;
        }
        s.frame.mag.drop_q = Some(q);
        if let Some((_, hp, hn, _)) = best {
            let touch = s.frame.mag.drop_touch.unwrap_or(0.0) + h;
            s.frame.mag.drop_touch = Some(touch);
            if (len3(v) < dcfg(mc, "restV") && len3(w) < 3.0) || touch > dcfg(mc, "touchRest") {
                (s.frame.mag.drop_c, s.frame.mag.drop_v, s.frame.mag.drop_w) = (Some(cp), Some(v), Some(w));
                drop_settle(s, mc, hp, hn);
                v = s.frame.mag.drop_v.unwrap_or(v);
                w = s.frame.mag.drop_w.unwrap_or(w);
            }
        }
    }
    let o2 = qrot(q, ctr);
    let dp = sub(cp, o2);
    let mg = &mut s.frame.mag;
    (mg.drop_c, mg.drop_v, mg.drop_w, mg.drop_q, mg.drop_p) = (Some(cp), Some(v), Some(w), Some(q), Some(dp));
    if let Ok(e) = super::find_entity(id) {
        if !e.is_null() {
            place_ent(&e, dp, q);
        }
    }
    if s.frame.mag.drop_t >= mc.f("litterTime", 3.0) {
        drop_clear(s);
    }
}
#[allow(clippy::too_many_arguments)]
fn drop_release(s: &mut State, c: &Ctx, mc: &J, p: V3, q: Q, v: V3, w: V3, ent: Option<&str>) {
    drop_clear(s);
    if s.frame.mag.ent_id.is_none() {
        spawn_mag(s, &c.weapon, mc, p, q, ent.or(mc.s("dropEntity")));
    }
    let mg = &mut s.frame.mag;
    (mg.drop_id, mg.ent_id) = (mg.ent_id, None);
    mg.drop_q = Some(q);
    let o2 = qrot(q, drop_centre(mc));
    mg.drop_c = Some(add(p, o2));
    mg.drop_p = Some(p);
    let mut v = v;
    let vl = len3(v);
    if vl > dcfg(mc, "maxV") {
        v = scale(v, dcfg(mc, "maxV") / vl);
    }
    let mut w = w;
    let wl = len3(w);
    if wl > dcfg(mc, "maxW") {
        w = scale(w, dcfg(mc, "maxW") / wl);
    }
    let mg = &mut s.frame.mag;
    (mg.drop_v, mg.drop_w) = (Some(v), Some(w));
    (mg.drop_t, mg.drop_rest, mg.drop_hits, mg.drop_touch) = (0.0, None, 0, Some(0.0));
}
/// stagePose: the finger pose for the depth (two recorded stages blended), cached by its key.
fn stage_pose(s: &mut State, mc: &J, depth: Option<f64>) -> Option<(Pose, String)> {
    let st = mc.g("poseStages").arr();
    if st.is_empty() {
        return None;
    }
    let d = if s.tune.stage >= 0.0 { s.tune.stage } else { depth.unwrap_or(0.0).max(0.0) };
    let (a, b, t) = stage_bracket(st, d);
    let an = vpick(s, a, "pose").as_str().unwrap_or("").to_string();
    let bn = vpick(s, b, "pose").as_str().unwrap_or("").to_string();
    let key = format!("mag|{an}|{bn}|{t:.2}|{}", s.tune.rev);
    if s.frame.mag.stage_key.as_deref() == Some(key.as_str()) {
        return s.frame.mag.stage_pose.clone().map(|p| (p, key));
    }
    let pa = data::pose(&an)?;
    let pb = data::pose(&bn);
    let out = if pb.as_ref().is_some_and(|pb| t > 0.005 && pb.len() == pa.len()) { mix_pose(Some(&pa), pb.as_ref(), t).unwrap_or(pa) } else { pa };
    s.frame.mag.stage_key = Some(key.clone());
    s.frame.mag.stage_pose = Some(out.clone());
    Some((out, key))
}
fn mag_fade(s: &mut State, mc: &J, on: bool, side: usize, depth: Option<f64>) {
    if on {
        match stage_pose(s, mc, depth) {
            Some((ps, key)) => fade_in_table(s, side, &key, Some(&ps)),
            None => fade_in(s, side, mc.s("pose")),
        }
    } else if s.fade.key.as_deref() == mc.s("pose") || s.fade.key.as_deref().is_some_and(|k| k.starts_with("mag|")) {
        fade_out(s);
    }
}
/// handTarget: where the held magazine goes for the hand: (world position, world orientation, slot-local position and
/// orientation for a carrier or item).
fn hand_target(s: &mut State, c: &Ctx, mc: &J, side: usize) -> Option<(V3, Q, V3, Q)> {
    let Some((mut sp, mut sq)) = hold_slot_tr(s, mc, side) else {
        s.frame.mag.diag = "hold slot did not resolve".into();
        return None;
    };
    if c.db.is_none() {
        s.frame.mag.diag = "no gun basis".into();
        return None;
    }
    let fr = wrist_to_slot(side, true);
    let fb = fr.and_then(|(r, e)| slot_bind(side, r, e));
    if let (Some((fr_r, fr_e)), Some(b)) = (fr, fb) {
        let wq = qmul(sq, neg(fr_e));
        let a = qrot(wq, fr_r);
        let b2 = qrot(wq, [b[0], b[1], b[2]]);
        sp = add(sub(sp, a), b2);
        sq = qmul(wq, [b[3], b[4], b[5], b[6]]);
    }
    let Some(wm) = weapon_model_q(c) else {
        s.frame.mag.diag = "no weapon orientation".into();
        return None;
    };
    let (ho0, hb) = hold_at(s, mc);
    let ho = hand_hold(side, Some(ho0)).unwrap_or([0.0; 3]);
    let mut t = add(sp, qrot(sq, ho));
    let bq = basis_quat(c.gf, c.gd, c.gs);
    let r = qmul(neg(bq), wm);
    let mut k = hand_hold_rot(side, qmul(hb, r));
    let rq = qmul(sq, k);
    t = add(t, origin_shift(mc, rq));
    let ep = mag_ent(s);
    let got = (!ep.is_null()).then(|| m::<[f32; 4]>(&ep, "GetWorldPosition", &[]).ok()).flatten();
    s.frame.mag.diag = format!(
        "slot({:.2},{:.2},{:.2}) want({:.2},{:.2},{:.2}) got({})",
        sp[0],
        sp[1],
        sp[2],
        t[0],
        t[1],
        t[2],
        got.map_or("-".to_string(), |p| format!("{:.2},{:.2},{:.2}", p[0], p[1], p[2]))
    );
    let ol = qrot(k, mc.v3("originOffset").unwrap_or([0.0; 3]));
    let mut l0 = add(ho, ol);
    if let (Some((fr_r, fr_e)), Some(b)) = (fr, fb) {
        let bq7 = [b[3], b[4], b[5], b[6]];
        let p2 = qrot(bq7, l0);
        l0 = qrot(neg(fr_e), sub(add([b[0], b[1], b[2]], p2), fr_r));
        let c1 = qmul(bq7, k);
        k = qmul(neg(fr_e), c1);
    }
    Some((t, rq, l0, k))
}
fn pull_w(d: f64, r: f64, g: f64) -> f64 {
    if d < 0.0 || d > r {
        return 0.0;
    }
    ((1.0 - d / r) * g).min(1.0)
}
/// seatMag: the magazine home in the gun -- the rounds from the reserve (on the rack when the slide is on its stop).
fn seat_mag(s: &mut State, c: &Ctx, x: &MagCtx, hh: Option<usize>) {
    let mc = x.mc;
    despawn_mag(s);
    hold_hide(s, None);
    drop_clear(s);
    gun_mag(mc, true);
    mag_fade(s, mc, false, 0, None);
    item_drop(s, 0, LEFT_SLOT);
    item_drop(s, 1, RIGHT_SLOT);
    if let Some(h) = hh {
        mag_magnet(s, c, x, h, 0.0, None, None);
    }
    let mg = &mut s.frame.mag;
    (mg.state, mg.hand, mg.armed) = ("in", None, false);
    mg.depth = Some(0.0);
    mg.glide = None;
    mg.fresh_load = true;
    let cor = mc.g("chamberOnRack");
    let rack = cor == &J::Bool(true) || (cor.as_str() == Some("dry") && mg.dry);
    mg.dry = false;
    let w = &c.weapon;
    if s.frame.slide.locked || rack {
        let load = match s.frame.mag.carried {
            Some(l) => {
                give_reserve(w, l);
                l
            }
            None => mag_capacity(w).min(reserve_ammo(w)),
        };
        s.frame.mag.pending = Some(load);
        s.frame.mag.loaded = Some(load);
    } else if let Some(cr) = s.frame.mag.carried {
        give_reserve(w, cr);
        set_mag_ammo(s, w, cr);
    } else {
        let load = mag_capacity(w).min(reserve_ammo(w));
        set_mag_ammo(s, w, load);
        s.frame.mag.loaded = Some(load);
    }
    s.frame.mag.carried = None;
    s.frame.slide.user_closed = false;
    play_snd(w, c.snd.s("magIn"));
    s.frame.mag.grip_lock = true;
}
fn seated_world(c: &Ctx, mc: &J) -> Option<(V3, Q)> {
    seated_pose(&c.weapon, &c.slot_comp, mc)
}
/// The cylinder's slot in the world (the revolver's eject point).
fn cylinder(c: &Ctx, mc: &J) -> Option<(V3, Q)> {
    slot_world(&c.slot_comp, mc.g("eject").s("slot").unwrap_or("vrp_cylinder"))
}
/// The left trigger blocked while it takes a part (M.sgH.ltBlock).
pub(super) fn lt_block(s: &mut State, on: bool) {
    if !on && !s.sg_lt_on {
        return;
    }
    s.sg_lt_on = on;
    native::<()>("SetVRLeftTriggerBlock", &[Value::Int32(i32::from(on))]);
}

/// The magazine part of the frame.
pub(super) fn frame(s: &mut State, c: &Ctx) {
    let cfg = c.cfg;
    let mc = cfg.g("mag");
    if !(mc.b("enabled") && mc.g("entity").present()) {
        super::reload_parts::frame(s, c);
        return;
    }
    if let Some(w) = slot_model(&c.slot_comp, mc.s("slot"), c.b, c.c) {
        if super::ent_spawn::available() {
            mag_frame(s, c, mc, w);
        }
    }
    super::reload_parts::frame(s, c);
}
fn mag_frame(s: &mut State, c: &Ctx, mc: &J, w: V3) {
    let free = c.free;
    let snd = c.snd;
    let hm = hand_model(free);
    s.frame.mag.dist = hm.map_or(-1.0, |h| len3(sub(h, w)));
    let mut grip = hm.is_some() && grip_of(free);
    s.frame.mag.dbg2 = format!(
        " g={} lock={} free={free} H={} hand={}",
        if grip { "Y" } else { "n" },
        if s.frame.mag.grip_lock { "Y" } else { "n" },
        s.frame.mag.hand.map_or("nil".into(), |h| h.to_string()),
        if hm.is_some() { "Y" } else { "NIL" }
    );
    let btn = super::belt::shared(157) > 0.5;
    let mut btn_edge = btn && !s.frame.mag.btn_was;
    s.frame.mag.btn_was = btn;
    if !grip {
        s.frame.mag.grip_lock = false;
    }
    let grip_take = grip && !s.frame.mag.grip_lock;
    s.frame.mag.pose_ok = Some(mc.s("pose").and_then(data::pose).is_some());
    let tw = mag_wrist_target(s, c, mc, w, free, None);
    s.frame.mag.seat_wt = tw.filter(|_| s.frame.mag.state == "in").map(|(p, q)| [p[0], p[1], p[2], q[0], q[1], q[2], q[3]]);
    let mut x = MagCtx { mc, w, hm, tw };
    let mut ro = [0.0; 3];
    let mw = weapon_model_q(c);
    if let (Some(mw), Some(po)) = (mw, mc.v3("previewOff")) {
        ro = qrot(mw, po);
    }
    let mut snap_r = mc.f("snapRadius", 0.05);
    let mut snap_d = match (tw, hm) {
        (Some((t, _)), Some(h)) => len3(sub(sub(h, t), ro)),
        _ => -1.0,
    };
    s.frame.mag.tk = tw.map_or([0.0; 3], |(t, _)| add(t, ro));
    let mut tk_some = tw.is_some();
    let mut gap_v: Option<V3> = None;
    if let (Some(tp), Some(mw), Some(hm)) = (mc.v3("takePoint"), mw, hm) {
        let o5 = mc.v3("originOffset").unwrap_or([0.0; 3]);
        let mut mq = mw;
        if let Some(sr) = mc.v4("seatRot") {
            mq = qmul(mq, sr);
        }
        let b5 = qrot(mq, add(o5, tp));
        let rel = sub(sub(hm, w), b5);
        snap_d = len3(rel);
        snap_r = mc.fo("takeRadius").unwrap_or(0.09);
        s.frame.mag.tk = add(w, b5);
        tk_some = true;
        let e = qrot(neg(mq), rel);
        gap_v = Some(e);
        if let Some(r5) = mc.v3("takeRadius") {
            let u5 = ((e[0] / r5[0]).powi(2) + (e[1] / r5[1]).powi(2) + (e[2] / r5[2]).powi(2)).sqrt();
            snap_r = r5[0].max(r5[1]).max(r5[2]);
            snap_d = u5 * snap_r;
        }
        if let Some(tu) = mc.fo("takeUp") {
            if let Some(hq5) = native::<[f32; 4]>("VRHandRawRot", &[Value::Int32(free as i32)]).map(f4) {
                let pa = mc.v3("palmAxis").unwrap_or([0.0, 0.0, -1.0]);
                let p5 = qrot(hq5, pa);
                let u5 = qrot(mw, [0.0, 0.0, 1.0]);
                let dp5 = dot(p5, u5);
                s.frame.mag.palm_dot = Some(dp5);
                if dp5 < tu {
                    snap_d = -1.0;
                }
            }
        }
    }
    if let (Some(hm), Some(mw)) = (hm, mw) {
        let g7 = gap_v.or_else(|| tw.map(|(t, _)| qrot(neg(mw), sub(sub(hm, t), ro))));
        if let Some(g7) = g7 {
            s.frame.mag.dbg2.push_str(&format!(" snapD={snap_d:.3} gap=({:+.3},{:+.3},{:+.3}) palm={:.2}", g7[0], g7[1], g7[2], s.frame.mag.palm_dot.unwrap_or(-9.0)));
        }
    }
    if s.frame.mag.state == "in" && !mc.b("handPull") && !mc.g("eject").present() {
        // a B edge hands a seated pistol magazine to an available grip (main's mag_release.lua, 4a50f93cb)
        let busy = s.frame.slide.grabbed || s.frame.slide.releasing || s.frame.rot_t >= 0.0 || c.cfg.g("hatch").present();
        if !super::reload_flick::catch_seated_mag(&c.weapon, btn_edge, grip_take, snap_d >= 0.0 && snap_d < snap_r, busy) {
            snap_d = -1.0;
        }
    }
    s.frame.mag.snap_dpre = Some(snap_d);
    if s.frame.mag.state == "in" && mc.b("pullTrigger") {
        s.frame.mag.lt_near = snap_d >= 0.0 && snap_d <= snap_r;
        if !squeeze(Some("trigger"), free) {
            snap_d = -1.0;
        }
    } else {
        s.frame.mag.lt_near = false;
    }
    let ltn = s.frame.mag.lt_near || s.frame.slide.lt_near;
    lt_block(s, ltn);
    let mut snap_w = if snap_d >= 0.0 && snap_d <= snap_r { 1.0 - snap_d / snap_r } else { 0.0 };
    s.frame.mag.snap_d = Some(snap_d);
    {
        let j = s.frame.mag.jit.get_or_insert_with(Default::default);
        j.n += 1;
        if snap_w > 0.0 {
            j.on += 1;
        }
        for (k, v) in [(0, Some(snap_d)), (1, Some(snap_w)), (2, tw.map(|t| t.0[0])), (3, hm.map(|h| h[0]))] {
            if let Some(v) = v {
                if j.lo[k].is_none_or(|lo| v < lo) {
                    j.lo[k] = Some(v);
                }
                if j.hi[k].is_none_or(|hi| v > hi) {
                    j.hi[k] = Some(v);
                }
            }
        }
    }
    let sl_d = s.frame.slide.dist;
    if s.frame.slide.grabbed || s.frame.slide.releasing || (s.frame.mag.slide_has_pose && sl_d >= 0.0 && snap_d >= 0.0 && sl_d < snap_d) {
        snap_w = 0.0;
    }
    if s.frame.mag.state == "in" && (snap_w > 0.0 || s.frame.mag.lt_near) && !s.frame.slide.grabbed && tk_some {
        reach_mark(free, s.frame.mag.tk, hm);
    }
    // the released magazine directed by a flick while it falls clear (main's mag_flick.lua, 26cdb7f83)
    let mg = &s.frame.mag;
    if let (Some(id), "gone", None, 0, true) = (mg.drop_id, mg.state, mg.drop_rest, mg.drop_hits, mg.drop_touch.unwrap_or(0.0) == 0.0) {
        let mut v = s.frame.mag.drop_v.unwrap_or([0.0; 3]);
        let had = s.frame.mag.drop_v.is_some();
        s.flick.update(id, c.holder, btn, c.dt, &mut v, dcfg(mc, "maxV"));
        if had {
            s.frame.mag.drop_v = Some(v);
        }
    } else {
        s.flick.cancel();
    }
    drop_tick(s, c, mc);
    if s.frame.mag.state != "in" {
        let n = mag_ammo(&c.weapon);
        let t = super::belt::now();
        if let Some(n) = n.filter(|n| *n > 0) {
            if t - s.frame.mag.zero_at.unwrap_or(-1.0) > 0.3 {
                if set_mag_ammo(s, &c.weapon, 0) {
                    give_reserve(&c.weapon, n);
                }
                s.frame.mag.zero_at = Some(t);
            }
        }
    }
    let gain = mc.f("snapGain", 1.6);
    let mut catch_w = snap_w;
    if s.frame.mag.state == "in" && s.frame.slide.grabbed && s.frame.slide.hand == Some(free) {
        catch_w = 0.0;
    }
    let mut take_anywhere = false;
    if s.frame.mag.state == "gone" {
        let belt_d = belt_mag_dist(free);
        let at_body = body_mag_reach(free);
        let mut gone_d = mag_from_raw(s, mc, free, false).map_or(-1.0, |g| len3(sub(g, w)));
        if let Some(bd) = belt_d {
            gone_d = bd;
        }
        catch_w = pull_w(gone_d, if belt_d.is_some() { belt_mag_radius() } else { mc.f("seatSnapRadius", 0.12) }, gain);
        let sl_d2 = s.frame.slide.dist;
        let pr = c.sc.f("previewRadius", 0.05);
        let busy = s.frame.slide.grabbed
            || s.frame.slide.releasing
            || (s.frame.mag.slide_has_pose && sl_d2 >= 0.0 && ((at_body && sl_d2 <= pr) || (!at_body && gone_d >= 0.0 && sl_d2 < gone_d)));
        if at_body {
            catch_w = 1.0;
        }
        if busy {
            catch_w = 0.0;
        }
        s.frame.mag.ins_d = Some(gone_d);
        s.frame.mag.from_belt = belt_d.is_some();
        take_anywhere = !busy && (at_body || belt_d.is_none());
    }
    match s.frame.mag.state {
        "in" | "gone" => {
            mag_magnet(s, c, &x, free, 0.0, None, None);
            mag_fade(s, mc, catch_w > 0.0, free, None);
            if mc.g("eject").present() && s.frame.mag.state == "in" {
                let open = s.frame.hatch.p > 0.9;
                if open && !s.frame.mag.cases_out && !s.frame.mag.fresh_load {
                    if let Some((cp, cq)) = cylinder(c, mc) {
                        let live = s.frame.slide.ammo.unwrap_or(0);
                        gun_mag(mc, false);
                        super::reload_revolver::eject(s, mc, cp, cq, Some(live));
                        s.frame.mag.cases_out = true;
                    }
                } else if !open && s.frame.mag.cases_out {
                    let back = super::reload_revolver::held_live(s);
                    super::reload_revolver::clear(s);
                    gun_mag(mc, true);
                    s.frame.mag.cases_out = false;
                    set_mag_ammo(s, &c.weapon, back);
                }
                if s.frame.mag.cases_out && super::reload_revolver::spent(s) {
                    let mg = &mut s.frame.mag;
                    (mg.state, mg.hand, mg.carried) = ("gone", None, None);
                    mg.grip_lock = grip && !body_mag_reach(free);
                    set_mag_ammo(s, &c.weapon, 0);
                    play_snd(&c.weapon, snd.s("magOut"));
                    s.frame.mag.cases_out = false;
                }
                if !open {
                    s.frame.mag.fresh_load = false;
                }
            }
            let mut tipped = false;
            let mut thr = mc.fo("dropTilt");
            if let Some(mu) = mc.fo("dropMu") {
                thr = Some(super::reload_revolver::tilt_threshold(mu));
            }
            if let (Some(t), false) = (thr, mc.g("eject").present()) {
                tipped = s.frame.hatch.p > 0.9 && c.a[2] >= t;
            }
            // a B-assisted take that also won the slide / hand arbitration goes through the carrier and its rounds below
            if btn_edge && !mc.b("handPull") && !mc.g("eject").present() && grip_take && catch_w > 0.0 {
                btn_edge = false;
            }
            if ((btn_edge && thr.is_none()) || tipped) && s.frame.mag.state == "in" {
                if let Some((sx3, sq3)) = seated_world(c, mc) {
                    gun_mag(mc, false);
                    let push = mc.f("dropPush", 1.1);
                    let mut k = [0.0, 0.0, -push];
                    if let (Some(wa0), Ok(wq0)) = (mc.v3("wellAxis"), m::<[f32; 4]>(&c.weapon, "GetWorldOrientation", &[])) {
                        k = scale(qrot(f4(wq0), wa0), push);
                    }
                    if mc.g("eject").present() {
                        if let Some((cp, cq)) = cylinder(c, mc) {
                            super::reload_revolver::eject(s, mc, cp, cq, None);
                        }
                    } else {
                        drop_release(s, c, mc, sx3, sq3, k, [0.0; 3], None);
                        if btn_edge && !mc.b("handPull") && !c.cfg.g("hatch").present() {
                            let drop = s.frame.mag.drop_id;
                            s.flick.begin(&c.weapon, c.holder, drop);
                        }
                    }
                    let mg = &mut s.frame.mag;
                    (mg.state, mg.hand, mg.carried) = ("gone", None, None);
                    s.frame.mag.dry = mag_ammo(&c.weapon).unwrap_or(0) <= 0;
                    set_mag_ammo(s, &c.weapon, 0);
                    play_snd(&c.weapon, snd.s("magOut"));
                }
            } else if s.tune.hold && s.frame.mag.state != "hand" {
                if let Some((t, q, _, _)) = hand_target(s, c, mc, free) {
                    let skin = mag_skin(&c.weapon);
                    let mut took = mc.s("mesh").is_some_and(|mesh| hold_show(s, free, mesh, skin.as_deref()));
                    if took {
                        s.frame.mag.route = Some("carrier".into());
                    } else if let Some(item) = mc.s("item") {
                        if item_hold(s, free, item, hand_slot(free)) {
                            took = true;
                            s.frame.mag.route = Some("item".into());
                        }
                    }
                    if took || spawn_mag(s, &c.weapon, mc, t, q, None) {
                        if !took {
                            s.frame.mag.route = Some("entity".into());
                        }
                        let mg = &mut s.frame.mag;
                        (mg.state, mg.hand, mg.armed, mg.carried) = ("hand", Some(free), false, None);
                        mag_fade(s, mc, true, free, Some(1.0));
                    }
                }
            } else if grip_take && (catch_w > 0.0 || take_anywhere) {
                if let Some((t, q, _, _)) = hand_target(s, c, mc, free) {
                    let skin = mag_skin(&c.weapon);
                    let mut took = mc.s("mesh").is_some_and(|mesh| hold_show(s, free, mesh, skin.as_deref()));
                    if took {
                        s.frame.mag.route = Some("carrier".into());
                    }
                    if !took {
                        if let Some(item) = mc.s("item") {
                            took = item_hold(s, free, item, hand_slot(free));
                            if took {
                                s.frame.mag.route = Some("item".into());
                            }
                        }
                    }
                    if took || spawn_mag(s, &c.weapon, mc, t, q, None) {
                        if !took {
                            s.frame.mag.route = Some("entity".into());
                        }
                        let was_in = s.frame.mag.state == "in";
                        play_snd(&c.weapon, if was_in { snd.s("magOut") } else { snd.s("magGrab") });
                        if was_in {
                            let had = mag_ammo(&c.weapon).unwrap_or(0);
                            s.frame.mag.carried = (had > 0).then_some(had);
                            s.frame.mag.dry = had <= 0;
                            set_mag_ammo(s, &c.weapon, 0);
                            s.frame.mag.zero_at = Some(super::belt::now());
                        } else {
                            s.frame.mag.carried = None;
                        }
                        gun_mag(mc, false);
                        s.frame.mag.depth = Some(1.0);
                        s.frame.mag.variant = if mc.b("variantByHand") {
                            Some(if was_in { pick_variant(s, c, mc, w, free).unwrap_or(1) } else { 1 })
                        } else {
                            Some(if super::belt::now().to_bits() % 2 == 0 { 2 } else { 1 })
                        };
                        s.frame.mag.roll = pick_roll(s, c, mc, w, free);
                        s.frame.mag.ins_w = Some(0.0);
                        if was_in {
                            s.frame.mag.from_belt = false;
                        }
                        s.frame.mag.guide_out = was_in && mc.g("wellTurn").present();
                        (s.frame.mag.state, s.frame.mag.hand) = ("hand", Some(free));
                        s.frame.mag.armed = false;
                        mag_fade(s, mc, true, free, None);
                    }
                }
            }
        }
        "hand" => {
            let hh = s.frame.mag.hand.unwrap_or(free);
            if (mc.g("rollAlt").present() || mc.g("rollSym").present() || mc.g("rollWays").present()) && s.frame.mag.ins_w.unwrap_or(0.0) <= 0.0 {
                s.frame.mag.roll = pick_roll(s, c, mc, w, hh);
            }
            grip = grip_of(hh);
            x.hm = hand_model(free);
            let ht = hand_target(s, c, mc, hh);
            if let Some((t, q, l, k)) = ht {
                place_mag(s, c, t, q, Some(hh), Some((l, k)));
            }
            s.frame.mag.hand = Some(hh);
            let mr = mag_from_raw(s, mc, hh, true);
            let ins_r = mc.f("seatSnapRadius", 0.12);
            let run = mc.f("insertRun", 0.115);
            let mut wa = mc.v3("wellAxis");
            let mut alt_well = false;
            if let (Some(wa0), Some(mr)) = (wa, mr) {
                let axes = mc.g("wellAxes").arr();
                if !axes.is_empty() {
                    let mut pick = if s.frame.mag.in_well { Some(s.frame.mag.well_pick) } else { None };
                    if pick.is_none() {
                        let mut best_p: Option<f64> = None;
                        for k in 0..=axes.len() {
                            let a = if k == 0 { Some(wa0) } else { axes[k - 1].as_array().and_then(|a| Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])) };
                            let Some(a) = a else { continue };
                            let u = in_basis(c, a);
                            let v3 = sub(mr, w);
                            let dep = dot(v3, u);
                            let pp = len3(sub(v3, scale(u, dep)));
                            if dep > -0.02 && best_p.is_none_or(|b| pp < b) {
                                (pick, best_p) = (Some(k as i32), Some(pp));
                            }
                        }
                    }
                    s.frame.mag.well_pick = pick.unwrap_or(0);
                    if s.frame.mag.well_pick > 0 {
                        if let Some(a) = axes.get(s.frame.mag.well_pick as usize - 1).and_then(|a| a.as_array()).and_then(|a| Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])) {
                            wa = Some(a);
                            alt_well = true;
                        }
                    }
                }
            }
            let aw = wa.map(|wa| in_basis(c, wa));
            let (mut depth, mut perp, mut ins_w) = (-1.0, -1.0, 0.0);
            let mut g2: Option<V3> = None;
            if let (Some(mr), Some(aw)) = (mr, aw) {
                let v2 = sub(mr, w);
                depth = dot(v2, aw);
                perp = len3(sub(v2, scale(aw, depth)));
                let d2 = depth.max(0.0);
                let mut of = scale(aw, d2);
                let wb = if alt_well { &J::Null } else { mc.g("wellBend") };
                if let Some(o2) = wb.v3("axis") {
                    let b = wb.f("at", 0.015);
                    let u0 = in_basis(c, o2);
                    let ul = len3(u0);
                    if ul > 1e-6 {
                        let u = scale(u0, 1.0 / ul);
                        let t1 = depth.min(b);
                        let p1 = len3(sub(v2, scale(aw, t1)));
                        let kv = scale(aw, b);
                        let t2 = dot(sub(v2, kv), u).max(0.0);
                        let p2 = len3(sub(sub(v2, kv), scale(u, t2)));
                        if p2 < p1 {
                            (depth, perp) = (b + t2, p2);
                            of = add(kv, scale(u, t2));
                        } else {
                            if depth > b {
                                depth = b + t2;
                            }
                            perp = p1;
                            of = scale(aw, t1.max(0.0));
                        }
                    }
                }
                if depth > mc.f("armDist", ins_r) {
                    (s.frame.mag.armed, s.frame.mag.guide_out) = (true, false);
                }
                if let Some((t, _)) = tw {
                    g2 = Some(add(t, of));
                }
                let wt = if alt_well { &J::Null } else { mc.g("wellTurn") };
                if let (true, Some(gv), Some((_, tq))) = (wt.present(), g2, tw) {
                    if depth > 0.0 {
                        let fr = (depth / wt.f("to", 0.035)).min(1.0);
                        let a = wt.v3("axis").unwrap_or([-1.0, 0.0, 0.0]);
                        let u = in_basis(c, a);
                        let ul = len3(u);
                        if ul > 1e-6 {
                            let hh2 = (wt.f("deg", 0.0) * fr).to_radians() * 0.5;
                            let sn = hh2.sin() / ul;
                            let r = [u[0] * sn, u[1] * sn, u[2] * sn, hh2.cos()];
                            let pv = add(w, of);
                            let dv = qrot(r, sub(gv, pv));
                            g2 = Some(add(pv, dv));
                            s.frame.mag.turn_q = Some(qmul(r, tq));
                            s.frame.mag.turn_on = true;
                        }
                    }
                } else if s.frame.mag.turn_q.is_some() {
                    s.frame.mag.turn_on = false;
                }
                let mdep = mc.f("magnetDepth", run * 1.6);
                if (s.frame.mag.armed || s.frame.mag.guide_out) && depth >= -0.02 && depth <= mdep {
                    let r = if s.frame.mag.in_well && !s.frame.mag.guide_out { ins_r * mc.f("holdWiden", 2.2) } else { ins_r };
                    ins_w = pull_w(perp, r, mc.f("insertGain", gain));
                    s.frame.mag.in_well = ins_w > 0.0;
                    if ins_w <= 0.0 {
                        s.frame.mag.guide_out = false;
                    }
                } else {
                    s.frame.mag.in_well = false;
                }
            } else if let Some(mr) = mr {
                depth = len3(sub(mr, w));
                perp = depth;
                if depth > mc.f("armDist", ins_r) {
                    s.frame.mag.armed = true;
                }
                ins_w = if s.frame.mag.armed { pull_w(depth, ins_r, gain) } else { 0.0 };
            }
            if s.frame.slide.grabbed || s.frame.slide.releasing {
                ins_w = 0.0;
            }
            s.frame.mag.ins_w = Some(ins_w);
            let stage_d = if ins_w > 0.0 && aw.is_some() && depth >= 0.0 { depth } else { 1.0 };
            (s.frame.mag.ins_d, s.frame.mag.perp, s.frame.mag.depth) = (Some(depth), Some(perp), Some(stage_d));
            mag_fade(s, mc, true, hh, Some(stage_d));
            if let (Some(tq), true, true) = (s.frame.mag.turn_q, s.frame.mag.turn_on, mc.g("wellTurn").present()) {
                mag_magnet(s, c, &x, hh, ins_w, g2, Some(tq));
            } else {
                mag_magnet(s, c, &x, hh, ins_w, g2, None);
            }
            let seat = seated_world(c, mc);
            let seen_d = match (ht, seat) {
                (Some((t, ..)), Some((sx, _))) => len3(sub(t, sx)),
                _ => -1.0,
            };
            s.frame.mag.seen_d = Some(seen_d);
            let seat_now = if aw.is_some() && depth >= 0.0 {
                let mut frac = mc.f("seatAt", 0.70);
                if s.tune.seat >= 0.0 {
                    frac = s.tune.seat;
                }
                depth <= (1.0 - frac) * run && perp <= ins_r
            } else {
                seen_d >= 0.0 && seen_d <= mc.f("seatRadius", 0.03)
            };
            if s.frame.mag.armed && grip && seat_now {
                let gs = if mc.g("selfSeat").present() { seated_world(c, mc) } else { None };
                let e0 = if gs.is_some() { mag_ent(s) } else { Handle::null() };
                if let (Some((gp, gq)), false) = (gs, e0.is_null()) {
                    mag_fade(s, mc, false, hh, None);
                    mag_magnet(s, c, &x, hh, 0.0, None, None);
                    let (t0, q0) = ht.map(|h| (h.0, h.1)).unwrap_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0]));
                    let p5 = m::<[f32; 4]>(&e0, "GetWorldPosition", &[]).ok().map(f3).unwrap_or(t0);
                    let q5 = m::<[f32; 4]>(&e0, "GetWorldOrientation", &[]).ok().map(f4).unwrap_or(q0);
                    let d5 = qrot(neg(gq), sub(p5, gp));
                    let r5 = qmul(neg(gq), q5);
                    s.frame.mag.glide = Some(Glide { t: 0.0, from: d5, from_q: r5, dur: 0.0 });
                    lead_reset(s, "glide");
                    let mg = &mut s.frame.mag;
                    (mg.state, mg.hand, mg.armed) = ("glide", None, false);
                    mg.grip_lock = true;
                } else {
                    seat_mag(s, c, &x, Some(hh));
                }
            } else if !grip && !s.tune.hold {
                hold_hide(s, None);
                item_drop(s, hh, hand_slot(hh));
                mag_fade(s, mc, false, hh, None);
                mag_magnet(s, c, &x, hh, 0.0, None, None);
                let mut pose = ht.map(|h| (h.0, h.1));
                if pose.is_none() {
                    let e = mag_ent(s);
                    if let Ok(ep) = m::<[f32; 4]>(&e, "GetWorldPosition", &[]) {
                        pose = Some((f3(ep), [0.0, 0.0, 0.0, 1.0]));
                    }
                }
                match pose {
                    Some((p, q)) => {
                        let hv = s.frame.mag.hold;
                        let v = hv.map_or([0.0; 3], |h| [h[7], h[8], h[9]]);
                        let wv = hv.map_or([0.0; 3], |h| [h[10], h[11], h[12]]);
                        drop_release(s, c, mc, p, q, v, wv, mc.s("entity"));
                    }
                    None => despawn_mag(s),
                }
                let mg = &mut s.frame.mag;
                (mg.state, mg.hand, mg.armed, mg.carried) = ("gone", None, false, None);
                mg.depth = Some(0.0);
            }
        }
        "glide" => {
            let seat = seated_world(c, mc);
            match (&s.frame.mag.glide, seat) {
                (Some(g), Some((sp5, sq5))) => {
                    let tt = mc.g("selfSeat").f("time", 0.11);
                    let gt = g.t + c.dt;
                    let (d0, q0) = (g.from, g.from_q);
                    let f = (gt / tt.max(1e-3)).min(1.0);
                    let e5 = 1.0 - (1.0 - f) * (1.0 - f);
                    let k5 = 1.0 - e5;
                    let o5 = qrot(sq5, scale(d0, k5));
                    let qq = qslerp(q0, [0.0, 0.0, 0.0, 1.0], e5);
                    let wq = qmul(sq5, qq);
                    if let Some(g) = &mut s.frame.mag.glide {
                        g.t = gt;
                    }
                    let e5e = mag_ent(s);
                    if !e5e.is_null() {
                        let (gd, _) = fresh_delta(c.b);
                        let (gp, _) = lead_world(s, "glide", add(add(sp5, o5), gd));
                        place_ent(&e5e, gp, wq);
                    }
                    s.frame.mag.depth = Some(len3(scale(d0, k5)));
                    if f >= 1.0 {
                        seat_mag(s, c, &x, None);
                    }
                }
                _ => seat_mag(s, c, &x, None),
            }
        }
        _ => {}
    }
}
