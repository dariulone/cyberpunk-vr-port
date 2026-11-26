//! reload/revolver.lua: the revolver's cases. Ejected, each one is spawned in its chamber and slides out along the
//! cylinder's axis once the gun is tipped past its friction (in turn, staggered), pivots over the chamber's lip and
//! falls free, then is swept away. The live rounds among them go back to the cylinder if the crane is shut first.
use super::belt::{qmul, qrot, Q, V3};
use super::reload_core::{despawn, place_id, spawn_at, State};
use super::reload_data::Lua;
use serde_json::Value as J;

#[derive(Clone)]
pub(super) struct Case {
    id: Option<u64>,
    p: V3,
    v: V3,
    axis: V3,
    slide: f64,
    depth: f64,
    radial: V3,
    mu: f64,
    exit_speed: f64,
    release_tilt: f64,
    bias: f64,
    live: bool,
    q: Q,
    local_axis: V3,
    chamber: V3,
    mesh_at: V3,
    mrot: Q,
    wa: V3,
    w: f64,
    len: f64,
    t: f64,
    delay: f64,
    life: f64,
    free: Option<f64>,
    exit_started: bool,
    exit_sign: i32,
    armed: f64,
    sv: f64,
}
#[derive(Default)]
pub(super) struct Cases {
    list: Option<Vec<Case>>,
}
pub(super) fn tilt_threshold(mu: f64) -> f64 {
    (1.0 / mu).atan().cos()
}
/// M.eject: the cases spawned in their chambers (the last `live` of them live rounds).
pub(super) fn eject(s: &mut State, mc: &J, cp: V3, cq: Q, live: Option<i32>) {
    let ec = mc.g("eject");
    if !ec.present() {
        return;
    }
    let n = ec.f("count", 6.0) as i32;
    let live = live.unwrap_or(0).min(n);
    let mut list = Vec::new();
    for i in 1..=n {
        let is_live = i > n - live;
        let a = f64::from(i - 1) * (2.0 * std::f64::consts::PI / f64::from(n)) + ec.f("first", 0.0).to_radians();
        let rr = ec.f("radius", 0.0134);
        let (lx, ly) = (a.cos() * rr, a.sin() * rr);
        let lz = ec.f("seatZ", 0.0);
        let o = qrot(cq, [lx, ly, lz]);
        let mr = ec.v4("meshRot").unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let sq = qmul(cq, mr);
        let ma = ec.v3("meshAt").unwrap_or([0.0; 3]);
        let mm = qrot(sq, ma);
        let p = [cp[0] + o[0] - mm[0], cp[1] + o[1] - mm[1], cp[2] + o[2] - mm[2]];
        let path = if is_live { ec.s("liveEntity").or(ec.s("entity")) } else { ec.s("entity") };
        let Some(id) = path.and_then(|path| spawn_at(path, p, sq)) else { break };
        let axc = ec.v3("axis").unwrap_or([0.0, 0.0, 1.0]);
        let mut ax = qrot(cq, axc);
        if ax[2] > 0.0 {
            ax = ax.map(|v| -v);
        }
        let sp = ec.f("spread", 0.12);
        list.push(Case {
            id: Some(id),
            p,
            v: [0.0; 3],
            axis: ax,
            slide: 0.0,
            depth: ec.f("chamber", 0.048),
            radial: [o[0] / rr * sp, o[1] / rr * sp, o[2] / rr * sp],
            mu: ec.f("mu", 0.25),
            exit_speed: ec.f("minExitSpeed", 0.0),
            release_tilt: tilt_threshold(mc.f("dropMu", 0.25)),
            bias: ec.f("bias", 0.35),
            live: is_live,
            q: sq,
            local_axis: axc,
            chamber: [lx, ly, lz],
            mesh_at: ma,
            mrot: mr,
            wa: [0.0, 0.0, 1.0],
            w: 0.0,
            len: ec.f("caseLen", 0.035),
            t: 0.0,
            delay: ec.f("stagger", 0.03) * f64::from(i - 1),
            life: ec.f("life", 3.0),
            free: None,
            exit_started: false,
            exit_sign: 0,
            armed: 0.0,
            sv: 0.0,
        });
    }
    s.rev.list = (!list.is_empty()).then_some(list);
}
fn place(cyl: (V3, Q), c: &mut Case) {
    let (cp, cq) = cyl;
    let o = qrot(cq, c.chamber);
    let q = qmul(cq, c.mrot);
    let mm = qrot(q, c.mesh_at);
    let sl = c.slide;
    c.p = [0, 1, 2].map(|k| cp[k] + o[k] - mm[k] + c.axis[k] * sl);
    c.q = q;
    if let Some(id) = c.id {
        place_id(id, c.p, q);
    }
}
/// M.tick: every case one step on.
pub(super) fn tick(s: &mut State, dt: f64, cyl: Option<(V3, Q)>) {
    let Some(list) = s.rev.list.as_mut() else { return };
    let mut alive = 0;
    for c in list.iter_mut() {
        let Some(id) = c.id else { continue };
        c.t += dt;
        if c.free.is_some_and(|f| c.t - f >= c.life) {
            despawn(id);
            c.id = None;
        } else if c.free.is_none() && c.slide < c.depth {
            if let Some((_, cq)) = cyl {
                let mut n = qrot(cq, c.local_axis);
                if !c.exit_started && n[2] > 0.0 {
                    n = n.map(|v| -v);
                }
                if c.exit_started && c.exit_sign == -1 {
                    n = n.map(|v| -v);
                }
                c.axis = n;
            }
            let ct = -c.axis[2];
            let st = (1.0 - ct * ct).max(0.0).sqrt();
            let mut rw = 1.0;
            if let Some((_, cq)) = cyl {
                let r = qrot(cq, [c.chamber[0], c.chamber[1], 0.0]);
                let rl = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
                if rl > 1e-6 {
                    rw = 1.0 + c.bias * (r[2] / rl);
                }
            }
            let mut a = (9.81 * (ct - c.mu * rw * st)).max(0.0);
            if c.exit_speed > 0.0 {
                if !c.exit_started {
                    if cyl.is_some() && ct >= c.release_tilt {
                        c.armed += dt;
                    } else {
                        c.armed = 0.0;
                    }
                    if c.armed >= c.delay.max(0.025) {
                        c.exit_started = true;
                        c.sv = c.exit_speed;
                        let lz = cyl.map_or(0.0, |(_, cq)| qrot(cq, c.local_axis)[2]);
                        c.exit_sign = if lz > 0.0 { -1 } else { 1 };
                    } else {
                        a = 0.0;
                    }
                }
            } else {
                if a > 0.0 {
                    c.armed += dt;
                }
                if a > 0.0 && c.armed < c.delay {
                    a = 0.0;
                }
            }
            c.sv += a * dt;
            c.slide += c.sv * dt;
            if let Some(cy) = cyl {
                place(cy, c);
            }
            let half = c.len * 0.5;
            let over = c.slide - (c.depth - half);
            if over > 0.0 {
                let al = 9.81 * st * over / (c.len * c.len / 12.0 + over * over);
                c.w += al * dt;
                let (ax2, ay2) = (c.axis[1], -c.axis[0]);
                let tl = (ax2 * ax2 + ay2 * ay2).sqrt();
                if tl > 1e-6 {
                    c.wa = [ax2 / tl, ay2 / tl, 0.0];
                }
            }
            if c.slide >= c.depth {
                for k in 0..3 {
                    c.v[k] = c.axis[k] * c.sv + c.radial[k];
                }
                c.free = Some(c.t);
            }
            alive += 1;
        } else {
            c.v[2] -= 9.81 * dt;
            for k in 0..3 {
                c.p[k] += c.v[k] * dt;
            }
            let a = c.w * dt * 0.5;
            let sa = a.sin();
            let q = qmul(c.q, [c.wa[0] * sa, c.wa[1] * sa, c.wa[2] * sa, a.cos()]);
            let l = q.iter().map(|v| v * v).sum::<f64>().sqrt();
            if l > 1e-6 {
                c.q = q.map(|v| v / l);
            }
            place_id(id, c.p, c.q);
            alive += 1;
        }
    }
    if alive == 0 {
        s.rev.list = None;
    }
}
pub(super) fn clear(s: &mut State) {
    if let Some(list) = s.rev.list.take() {
        for c in list {
            if let Some(id) = c.id {
                despawn(id);
            }
        }
    }
}
/// M.heldLive: the live rounds still in their chambers.
pub(super) fn held_live(s: &State) -> i32 {
    s.rev.list.as_ref().map_or(0, |l| l.iter().filter(|c| c.id.is_some() && c.free.is_none() && c.live).count() as i32)
}
/// M.spent: every case out of the cylinder.
pub(super) fn spent(s: &State) -> bool {
    s.rev.list.as_ref().is_some_and(|l| l.iter().all(|c| !(c.id.is_some() && c.free.is_none())))
}
pub(super) fn diag(s: &State) -> String {
    let n = s.rev.list.as_ref().map_or(0, |l| l.len());
    let c = s.rev.list.as_ref().and_then(|l| l.first());
    format!(
        "rev[cases={n}{}]",
        c.map_or(String::new(), |c| format!(" t={:.2} slide={:.3}/{:.3} ct={:+.2} sv={:.2}", c.t, c.slide, c.depth, -c.axis[2], c.sv))
    )
}
