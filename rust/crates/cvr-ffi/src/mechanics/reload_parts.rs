//! reload.lua's M.frame, part C: the rotator's flourish, the hatch (a magazine cover, or a revolver's crane swung by
//! the button and shut by a flick of the wrist), the revolver's cases, the cylinder turned by the free hand's pad, the
//! hammer (single / double action on the trigger's pull, the thumb's cocking sequence, the trigger finger's pose), the
//! weapon's own animation held off while any of it is ours, and the rig released when nothing is.
use super::belt::{native, qmul, qrot, V3};
use super::reload_core::*;
use super::reload_data::{self as data, Lua};
use super::reload_frame::{capture_crane_rest, cylinder_lateral_flick, drive_crane_rest, Ctx};
use crate::script_call::Value;
use serde_json::Value as J;

fn axis(j: &J, k: &str, d: V3) -> V3 {
    j.v3(k).unwrap_or(d)
}
fn which_bone(j: &J) -> (i32, i32) {
    (j.f("which", 0.0) as i32, j.f("bone", 0.0) as i32)
}
pub(super) fn frame(s: &mut State, c: &Ctx) {
    let cfg = c.cfg;
    let dt = c.dt;
    let snd = c.snd;
    // the rotator's half-turn after a rack
    let rc = cfg.g("rotator");
    if rc.present() && s.frame.rot_t >= 0.0 {
        s.frame.rot_t += dt;
        let frac = s.frame.rot_t / rc.f("spinTime", 0.28);
        let (which, bone) = which_bone(rc);
        let ax = axis(rc, "spinAxis", [0.0, 0.0, 1.0]);
        if frac >= 1.0 {
            s.frame.rot_t = -1.0;
            rig_write_rot(which, bone, 0.0, ax);
        } else {
            rig_write_rot(which, bone, rc.f("spinAngle", 180.0) * (std::f64::consts::PI * frac).sin(), ax);
        }
    }
    hatch(s, c);
    // the revolver's cases, falling
    let ec = if cfg.g("mag").g("eject").present() { cfg.g("mag").g("eject") } else { cfg.g("eject") };
    let cyl = if ec.present() { slot_world(&c.slot_comp, ec.s("slot").unwrap_or("vrp_cylinder")) } else { None };
    super::reload_revolver::tick(s, dt, cyl);
    spin(s, c);
    let mc2 = cfg.g("hammer");
    if !mc2.present() {
        let sl = &s.frame.slide;
        let mode = i32::from(c.sc.b("manual") && (sl.fired || sl.grabbed || sl.releasing));
        set_trg(s, mode);
    } else {
        hammer(s, c, mc2);
    }
    if cfg.g("hatch").present() {
        let (which, bone) = which_bone(cfg.g("hatch"));
        let q: Vec<f64> = (3..7).map(|f| rig_bone(which, bone, f).unwrap_or(0.0)).collect();
        let w = q[3].abs().min(1.0);
        let a = (2.0 * w.acos()).to_degrees();
        let sn = (1.0 - w * w).max(1e-9).sqrt();
        s.frame.hatch.read = Some(format!("{a:.0}/({:+.2},{:+.2},{:+.2})", q[0] / sn, q[1] / sn, q[2] / sn));
    }
    if cfg.b("ownAnim") {
        let mine = s.frame.hatch.p > 0.0 || s.frame.ham.p > 0.0 || s.frame.spin.wrote;
        if mine {
            hold_reload_stage(&c.weapon);
            s.own_tagged = true;
        } else if s.own_tagged {
            push_anim_event(&c.weapon, "InterruptReload");
            s.own_tagged = false;
        }
    }
    let f = &s.frame;
    let active = f.slide.grabbed
        || f.slide.releasing
        || f.rot_t >= 0.0
        || f.slide.eject_t.is_some()
        || f.slide.chase_v.is_some()
        || f.mag.state != "in"
        || f.hatch.p > 0.0
        || f.ham.p > 0.0
        || f.spin.wrote;
    if !active && s.frame.slide.was_active {
        rig_write_clear();
        s.frame.slide.seated_shown = None;
    }
    s.frame.slide.was_active = active;
    let _ = snd;
}
/// The hatch: open while the magazine is out (or, manual, toggled by the button and shut by a flick).
fn hatch(s: &mut State, c: &Ctx) {
    let hc = c.cfg.g("hatch");
    if !hc.present() {
        return;
    }
    let dt = c.dt;
    let mut want = if s.frame.mag.state != "in" { 1.0 } else { 0.0 };
    if hc.b("shutOnGlide") && s.frame.mag.state == "glide" {
        want = 0.0;
    }
    let manual = hc.b("manual");
    if hc.g("sounds").present() && !manual && Some(want) != s.frame.hatch.want_prev {
        if s.frame.hatch.want_prev.is_some() {
            play_snd(&c.weapon, hc.g("sounds").s(if want > 0.5 { "open" } else { "shut" }));
        }
        s.frame.hatch.want_prev = Some(want);
    }
    if manual {
        capture_crane_rest(&mut s.frame.hatch, hc, dt);
        let b2 = super::belt::shared(157) > 0.5;
        if b2 && !s.frame.hatch.btn_prev {
            s.frame.hatch.open = !s.frame.hatch.open && s.frame.hatch.rest.is_some();
            play_snd(&c.weapon, c.snd.s(if s.frame.hatch.open { "hatchOpen" } else { "hatchShut" }));
        }
        s.frame.hatch.btn_prev = b2;
        s.frame.hatch.open_t = if s.frame.hatch.open { s.frame.hatch.open_t + dt } else { 0.0 };
        if let (true, Some(fs)) = (s.frame.hatch.open, hc.fo("flickSpeed")) {
            let armed = s.frame.hatch.open_t > hc.f("flickArm", 0.30);
            let v = native::<[f32; 4]>("VRHandMotion", &[Value::Int32(c.holder as i32), Value::Bool(false)]);
            if armed && cylinder_lateral_flick(&mut s.frame.hatch, v, c.gf, dt, fs) {
                s.frame.hatch.open = false;
                play_snd(&c.weapon, c.snd.s("hatchShut"));
            } else if !armed {
                (s.frame.hatch.flick_travel, s.frame.hatch.flick_samples) = (0.0, 0);
            }
        } else {
            (s.frame.hatch.flick_travel, s.frame.hatch.flick_samples) = (0.0, 0);
        }
        want = if s.frame.hatch.open { 1.0 } else { 0.0 };
    }
    let mut p = s.frame.hatch.p;
    if p != want {
        let t = if want > p { hc.f("openTime", 0.17) } else { hc.f("closeTime", 0.13) };
        let step = dt / t.max(1e-3);
        p = if want > p { (p + step).min(1.0) } else { (p - step).max(0.0) };
        s.frame.hatch.p = p;
    }
    let (a, wu) = (hc.f("angle", 90.0), hc.f("windUp", 0.0));
    let ang = if p < 0.20 {
        -wu * (p / 0.20)
    } else if p < 0.40 {
        -wu
    } else {
        -wu + (a + wu) * (std::f64::consts::PI * 0.5 * ((p - 0.40) / 0.60)).sin()
    };
    let (which, bone) = which_bone(hc);
    let ax = axis(hc, "axis", [1.0, 0.0, 0.0]);
    if p <= 0.0 {
        if s.frame.hatch.wrote {
            if manual {
                rig_write_off(which, bone);
            } else {
                rig_write_rot(which, bone, 0.0, ax);
            }
            s.frame.hatch.wrote = false;
        }
    } else if manual {
        s.frame.hatch.wrote = drive_crane_rest(&mut s.frame.hatch, hc, ang);
    } else {
        rig_write_rot(which, bone, ang * hc.f("sign", 1.0), ax);
        s.frame.hatch.wrote = true;
    }
}
/// The cylinder turned by the free hand's pad on it, only with the crane fully open (main's 60ad61ac9: the frame locks
/// it at the latch).
fn spin(s: &mut State, c: &Ctx) {
    let sc2 = c.cfg.g("spin");
    if !sc2.present() {
        s.frame.spin.pp = None;
        s.frame.spin.hand = None;
        return;
    }
    let dt = c.dt;
    let free = c.free;
    // A SPEEDLOADER JUST HOME: the hand that pushed it is still squeezed round its knob, on the cylinder's axis, and
    // the magazine's hold lets go of it this frame (the stopped hand springs back to the tracked one) -- read as a turn,
    // that could throw the cylinder round. Only a fresh squeeze after the grip has opened turns it.
    let ms = s.frame.mag.state;
    if ms == "in" && matches!(s.frame.spin.mag_prev, "hand" | "glide") {
        s.frame.spin.seat_hold = true;
        (s.frame.spin.pp, s.frame.spin.hand) = (None, None);
    }
    s.frame.spin.mag_prev = ms;
    if s.frame.spin.seat_hold && !grip_of(free) {
        s.frame.spin.seat_hold = false;
    }
    let spin_ok = s.frame.hatch.open && s.frame.hatch.p > 0.9;
    if !spin_ok {
        s.frame.spin.dbg = "locked".into();
    }
    let mut driven = false;
    if spin_ok {
        let tr = sc2.s("slot").and_then(|n| slot_world(&c.slot_comp, n));
        let hp = pad_model(free).or_else(|| hand_model(free));
        if tr.is_none() {
            s.frame.spin.dbg = "noslot".into();
        } else if hp.is_none() {
            s.frame.spin.dbg = "nohand".into();
        }
        if let (Some((cp, cq)), Some(hp)) = (tr, hp) {
            let ax3 = axis(sc2, "axis", [0.0, 1.0, 0.0]);
            let cm = qrot(c.c, [cp[0] - c.b[0], cp[1] - c.b[1], cp[2] - c.b[2]]);
            let mq = qmul(c.c, cq);
            let ax = qrot(mq, ax3);
            let v = [hp[0] - cm[0], hp[1] - cm[1], hp[2] - cm[2]];
            let along = v[0] * ax[0] + v[1] * ax[1] + v[2] * ax[2];
            let r = [v[0] - along * ax[0], v[1] - along * ax[1], v[2] - along * ax[2]];
            let rad = len3(r);
            let grip = grip_of(free);
            s.frame.spin.dbg = format!("rad={rad:.3} al={along:+.3} g={} w={:.0}", if grip { "Y" } else { "n" }, s.frame.spin.w);
            let mag_in_hand = (s.frame.mag.state == "hand" || s.frame.mag.state == "glide") && s.frame.mag.hand.is_none_or(|h| h == free);
            let (mut r7, mut a7) = (sc2.f("radius", 0.09), sc2.f("reach", 0.06));
            if s.frame.spin.on {
                let wd = sc2.f("widen", 1.5);
                (r7, a7) = (r7 * wd, a7 * wd);
            }
            let in_zone = rad > 0.005 && rad <= r7 && along.abs() <= a7 && !mag_in_hand;
            s.frame.spin.on = in_zone;
            if sc2.g("pose").present() && in_zone {
                fade_in(s, free, sc2.s("pose"));
                s.frame.spin.faded = true;
            } else if s.frame.spin.faded {
                fade_out(s);
                s.frame.spin.faded = false;
            }
            // the pad turns the cylinder from its flutes, not from the axis: a hand within `minRadius` of it (the
            // speedloader's knob, the ejector rod) has no lever, and its jitter divided by a near-zero radius was a
            // full-speed spin
            let lever = sc2.f("minRadius", 0.02);
            if grip && in_zone && !s.frame.spin.seat_hold && rad >= lever {
                let t7 = [ax[1] * r[2] - ax[2] * r[1], ax[2] * r[0] - ax[0] * r[2], ax[0] * r[1] - ax[1] * r[0]];
                let tl = len3(t7);
                if let (true, Some(pp), true) = (tl > 1e-6, s.frame.spin.pp, s.frame.spin.hand == Some(free) && dt > 1e-4) {
                    let t = t7.map(|x| x / tl);
                    let pc = s.frame.spin.pc;
                    let vh = [(hp[0] - pp[0]) - (cm[0] - pc[0]), (hp[1] - pp[1]) - (cm[1] - pc[1]), (hp[2] - pp[2]) - (cm[2] - pc[2])];
                    let vt = (vh[0] * t[0] + vh[1] * t[1] + vh[2] * t[2]) / dt;
                    let mx = sc2.f("maxRate", 1440.0);
                    let w = ((vt / rad.max(lever)).to_degrees() * sc2.f("gain", 1.0)).clamp(-mx, mx);
                    s.frame.spin.ang += w * dt;
                    s.frame.spin.w = s.frame.spin.w * 0.5 + w * 0.5;
                }
                s.frame.spin.pp = Some(hp);
                s.frame.spin.pc = cm;
                s.frame.spin.hand = Some(free);
                driven = true;
            } else {
                (s.frame.spin.pp, s.frame.spin.hand) = (None, None);
            }
        }
    } else {
        (s.frame.spin.pp, s.frame.spin.hand) = (None, None);
        s.frame.spin.on = false;
        if s.frame.spin.faded {
            fade_out(s);
            s.frame.spin.faded = false;
        }
    }
    if s.frame.hatch.p <= 0.0 {
        // the frame locks the cylinder at the latch, before this frame's pose is written
        (s.frame.spin.ang, s.frame.spin.w, s.frame.spin.det) = (0.0, 0.0, None);
    } else if !driven {
        let mut w = s.frame.spin.w;
        if w.abs() > 1.0 {
            s.frame.spin.ang += w * dt;
            w -= sc2.f("drag", 1.2) * w * dt;
            let cou = sc2.f("stop", 220.0) * dt;
            w = if w > 0.0 { (w - cou).max(0.0) } else { (w + cou).min(0.0) };
            s.frame.spin.w = w;
        } else {
            s.frame.spin.w = 0.0;
            // stopped: the nearest chamber eased in line with the barrel (`settle` deg/s), so the crane shuts on a
            // chamber -- its latch puts the turn back to zero, which is then a whole number of chambers and unseen
            let det = sc2.f("detent", 0.0);
            let rate = sc2.f("settle", 240.0) * dt;
            if det > 0.0 && rate > 0.0 {
                let want = (s.frame.spin.ang / det).round() * det;
                let d = want - s.frame.spin.ang;
                s.frame.spin.ang = if d.abs() <= rate { want } else { s.frame.spin.ang + rate * d.signum() };
            }
        }
    }
    let n5 = (s.frame.spin.ang / sc2.f("detent", 60.0)).floor();
    if s.frame.spin.det != Some(n5) {
        if s.frame.spin.det.is_some() {
            play_snd(&c.weapon, c.snd.s("spin"));
        }
        s.frame.spin.det = Some(n5);
    }
    let (which, bone) = which_bone(sc2);
    let ax4 = axis(sc2, "axis", [0.0, 1.0, 0.0]);
    if s.frame.spin.ang.abs() > 0.05 {
        rig_write_rot(which, bone, s.frame.spin.ang, ax4);
        s.frame.spin.wrote = true;
    } else if s.frame.spin.wrote {
        rig_write_rot(which, bone, 0.0, ax4);
        s.frame.spin.wrote = false;
    }
}
/// The hammer: cocked by the thumb (the click [159]), single action from cocked, double action through the pull.
fn hammer(s: &mut State, c: &Ctx, mc2: &J) {
    let dt = c.dt;
    let holder = c.holder;
    let snd = c.snd;
    let poses = mc2.g("poses");
    let clk = super::belt::shared(159) > 0.5;
    if clk && !s.frame.ham.prev {
        s.frame.ham.want = if s.frame.ham.want > 0.5 { 0.0 } else { 1.0 };
        s.frame.ham.seq = if poses.present() { Some(0.0) } else { None };
        s.frame.ham.sounded = false;
    }
    s.frame.ham.prev = clk;
    let ta = f64::from(super::belt::shared(if holder == 0 { 154 } else { 160 }));
    let crane_open = s.frame.hatch.p > 0.9;
    let (sa, da, rel) = (mc2.f("saBreak", 0.85), mc2.f("daBreak", 0.85), mc2.f("release", 0.20));
    let cocked = s.frame.ham.want > 0.5 || s.frame.ham.p > 0.9;
    let brk = if cocked { sa } else { da };
    if ta <= rel + 0.000001 {
        s.frame.ham.fired = false;
    }
    if let Some(fall) = s.frame.ham.fall {
        let fall = fall + dt;
        let dn = mc2.f("dryDrop", 0.035);
        if fall >= dn {
            (s.frame.ham.p, s.frame.ham.want, s.frame.ham.fall) = (0.0, 0.0, None);
        } else {
            s.frame.ham.fall = Some(fall);
            s.frame.ham.p = s.frame.ham.p0 * (1.0 - fall / dn);
        }
    } else if ta > 0.02 && !s.frame.ham.fired {
        s.frame.ham.p = if cocked { 1.0 } else { (ta / brk).min(1.0) };
        if ta >= brk {
            (s.frame.ham.p0, s.frame.ham.fall, s.frame.ham.fired) = (s.frame.ham.p, Some(0.0), true);
            play_snd(&c.weapon, snd.s("dryFire").or(snd.s("hammer")));
            s.frame.ham.seq = None;
            if s.frame.ham.key.take().is_some() {
                clear_grip(holder);
            }
        }
    }
    let mode = if crane_open {
        1
    } else if s.frame.ham.fired {
        2
    } else if ta > 0.0 {
        if ta >= brk {
            2
        } else {
            1
        }
    } else {
        0
    };
    set_trg(s, mode);
    s.frame.ham.dbg = format!("{ta:.2} {}{} m{mode}", if cocked { "SA" } else { "DA" }, if s.frame.ham.fired { "!" } else { "" });
    let a2 = s.frame.slide.ammo;
    if let (Some(last), Some(a2)) = (s.frame.ham.last, a2) {
        if a2 < last {
            s.frame.ham.want = 0.0;
        }
    }
    s.frame.ham.last = a2;
    if poses.present() && s.frame.ham.fall.is_none() && ta <= 0.02 {
        let (rt, pt, aw) = (mc2.f("reachTime", 0.24), mc2.f("pressTime", 0.10), mc2.f("awayTime", 0.22));
        let key = [
            (0.0, "over", 0.0),
            (rt * 0.55, "over", 1.0),
            (rt, "hold", 1.0),
            (rt + pt, "press", 1.0),
            (rt + pt * 2.0, "hold", 1.0),
            (rt + pt * 2.0 + aw, "hold", 0.0),
        ];
        if let Some(seq) = s.frame.ham.seq {
            let t3 = seq + dt;
            s.frame.ham.seq = Some(t3);
            if t3 >= key[key.len() - 1].0 {
                if s.frame.ham.key.take().is_some() {
                    clear_grip(holder);
                }
                s.frame.ham.seq = None;
            } else {
                let (mut a, mut b, mut u) = (key[0], key[0], 0.0);
                for k in 0..key.len() - 1 {
                    if t3 >= key[k].0 && t3 <= key[k + 1].0 {
                        let span = key[k + 1].0 - key[k].0;
                        (a, b) = (key[k], key[k + 1]);
                        u = if span > 1e-6 { (t3 - key[k].0) / span } else { 0.0 };
                        break;
                    }
                }
                let us = u * u * (3.0 - 2.0 * u);
                let pa = poses.s(a.1).and_then(data::pose);
                let pb = poses.s(b.1).and_then(data::pose);
                let al = a.2 + (b.2 - a.2) * us;
                if let Some(pose) = mix_pose(pa.as_ref(), pb.as_ref(), us) {
                    apply_grip(holder, &pose, al);
                    s.frame.ham.key = Some("ham".into());
                }
            }
        } else if s.frame.ham.key.take().is_some() {
            clear_grip(holder);
        }
    }
    let tp = mc2.s("trigPose").and_then(data::pose);
    match tp {
        Some(tp) if s.frame.ham.seq.is_none() && s.fade.hand != Some(holder) => {
            let from = mc2.f("trigFrom", 0.10);
            let u = ((ta - from) / (brk - from).max(0.01)).clamp(0.0, 1.0);
            if u > 0.0 {
                apply_grip(holder, &tp, u);
                s.frame.ham.fin = true;
            } else if s.frame.ham.fin {
                clear_grip(holder);
                s.frame.ham.fin = false;
            }
        }
        _ => {
            if s.frame.ham.fin {
                clear_grip(holder);
                s.frame.ham.fin = false;
            }
        }
    }
    let freed = !poses.present() || s.frame.ham.seq.is_none_or(|q| q >= mc2.f("reachTime", 0.24));
    if freed && !s.frame.ham.sounded {
        play_snd(&c.weapon, snd.s("hammer"));
        s.frame.ham.sounded = true;
    }
    if freed && s.frame.ham.p != s.frame.ham.want && s.frame.ham.fall.is_none() && ta <= 0.02 {
        let t2 = if s.frame.ham.want > s.frame.ham.p { mc2.f("cockTime", 0.18) } else { mc2.f("dropTime", 0.05) };
        let st = dt / t2.max(1e-3);
        s.frame.ham.p = if s.frame.ham.want > s.frame.ham.p { (s.frame.ham.p + st).min(1.0) } else { (s.frame.ham.p - st).max(0.0) };
    }
    let (which, bone) = which_bone(mc2);
    let ax2 = axis(mc2, "axis", [1.0, 0.0, 0.0]);
    if s.frame.ham.p <= 0.0 {
        if s.frame.ham.wrote {
            rig_write_rot(which, bone, 0.0, ax2);
            s.frame.ham.wrote = false;
        }
    } else {
        let f2 = (std::f64::consts::PI * 0.5 * s.frame.ham.p).sin();
        rig_write_rot(which, bone, mc2.f("angle", 37.5) * mc2.f("sign", 1.0) * f2, ax2);
        s.frame.ham.wrote = true;
    }
}
/// M.wants: what the glow should show on the held gun ("out", "rack", "empty"), its magazine mesh and slide slot.
pub(super) fn wants(s: &mut State, weapon: &red4ext_abi::handle::Handle) -> Option<(Option<&'static str>, &'static str, Option<&'static str>)> {
    let cfg = cfg_for(s, weapon)?;
    let mc = cfg.g("mag");
    if !mc.present() || mc.g("enabled") == &J::Bool(false) || cfg.g("shotgun").present() {
        return None;
    }
    let mesh = mc.s("mesh")?;
    let f = &s.frame;
    let want = if f.mag.state != "in" {
        Some("out")
    } else if f.mag.pending.is_some_and(|p| p > 0) || f.slide.fired {
        Some("rack")
    } else if f.slide.ammo.is_some_and(|a| a <= 0) {
        Some("empty")
    } else {
        None
    };
    Some((want, mesh, cfg.g("slide").s("slot")))
}
/// M.seated: where the gun's magazine sits.
pub(super) fn seated(s: &mut State, weapon: &red4ext_abi::handle::Handle, slots: &red4ext_abi::handle::Handle) -> Option<(V3, super::belt::Q)> {
    let cfg = cfg_for(s, weapon)?;
    let mc = cfg.g("mag");
    if !mc.present() {
        return None;
    }
    seated_pose(weapon, slots, mc)
}
