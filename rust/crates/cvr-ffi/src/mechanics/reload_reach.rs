//! A development aid (CVR_RELOAD_REACH=1): what a hand must do to work the held gun, for a test that drives the
//! simulator's controllers as a player would. Every 100 ms log/reload_reach.json says, for the part the free hand works
//! (a slide or a pump's fore-end): its nearest grip as the reload offers it (the wrist's point and turn in model space),
//! the hand's own, the distance and the reason the reload gives, the part's state (taken, how far back), and -- through
//! the inverse of the left hand's view frame (cvr-rig reach_dev) -- the head-relative controller pose that puts the
//! wrist on that grip and the one a full stroke back, beside the head-relative pose the hand has now. The test turns
//! those into tracking-space poses with the head it set; nothing here moves anything.
use super::belt::{V3, Q};
use std::sync::Mutex;
use std::time::Instant;

/// The nearest grip of the free hand's part this frame.
#[derive(Clone, Copy, Default)]
pub(super) struct Near {
    pub d: f64,
    pub at: V3,
    pub rot: Option<Q>,
}

/// What the frame knows of the part.
pub(super) struct Part<'a> {
    pub kind: &'a str,
    pub weapon: &'a str,
    pub free: usize,
    pub near: Option<Near>,
    pub hand: V3,
    pub hand_rot: Option<Q>,
    pub axis: V3,
    pub travel: f64,
    pub radius: f64,
    pub angle_max: f64,
    pub grab_with: &'a str,
    pub grabbed: bool,
    pub rack: f64,
    pub why: &'a str,
    pub exit: &'a str,
}

pub(super) fn on() -> bool {
    cvr_rig::vrik::reach_dev::on()
}

fn qmul(a: Q, b: Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn conj(q: Q) -> Q {
    [-q[0], -q[1], -q[2], q[3]]
}
fn qrot(q: Q, v: V3) -> V3 {
    let p = qmul(qmul(q, [v[0], v[1], v[2], 0.0]), conj(q));
    [p[0], p[1], p[2]]
}

/// The head-relative controller pose (position, orientation as OpenXR's x, y, z, w) that puts the left wrist at `at`
/// turned `rot` (model space): VRIK_BuildViewHandTarget undone.
fn left_local(at: V3, rot: Q) -> Option<(V3, Q)> {
    let v = cvr_rig::vrik::reach_dev::left_view()?;
    let f = |a: [f32; 3]| a.map(f64::from);
    let g = |a: [f32; 4]| a.map(f64::from);
    let (vp, rv, off, wrist, s) = (f(v.vp), g(v.rv), f(v.off), g(v.wrist), f64::from(v.scale));
    if s.abs() < 1e-6 {
        return None;
    }
    let hm = qmul(rot, conj(wrist));
    let mr = qmul(conj(rv), hm);
    let local_rot = [mr[0], mr[2], -mr[1], mr[3]];
    let m = qrot(conj(rv), [at[0] - vp[0] - off[0], at[1] - vp[1] - off[1], at[2] - vp[2] - off[2]]);
    Some(([m[0] / s, m[2] / s, -m[1] / s], local_rot))
}

fn shared(i: i32) -> f64 {
    // SAFETY: the plugin's snapshot of the shared hand block.
    f64::from(unsafe { cvr_rig::vrik::snapshot::SharedPose(i) })
}

/// A pointer beam as a menu drew it this frame (the lobby's): kept per frame -- the last 900 -- and written to
/// log/menu_rays.json every 100 ms, so a test sees the beam through a fast head turn frame by frame
/// (t, hand, origin, hit, the eye).
pub(super) fn ray_sample(hand: usize, origin: Option<V3>, hit: Option<V3>, eye: Option<V3>) {
    static RAYS: Mutex<(Vec<serde_json::Value>, Option<Instant>, Option<Instant>)> = Mutex::new((Vec::new(), None, None));
    let mut g = RAYS.lock().unwrap_or_else(|e| e.into_inner());
    let t0 = *g.2.get_or_insert_with(Instant::now);
    let r = |v: f64| (v * 100_000.0).round() / 100_000.0;
    let v3 = |v: Option<V3>| v.map(|v| serde_json::json!([r(v[0]), r(v[1]), r(v[2])]));
    g.0.push(serde_json::json!([r(t0.elapsed().as_secs_f64()), hand, v3(origin), v3(hit), v3(eye)]));
    if g.0.len() > 900 {
        let extra = g.0.len() - 900;
        g.0.drain(..extra);
    }
    if g.1.is_some_and(|t| t.elapsed().as_millis() < 100) {
        return;
    }
    g.1 = Some(Instant::now());
    let path = cvr_host::paths::dir(cvr_host::paths::Folder::Log).join("menu_rays.json");
    let _ = std::fs::write(path, serde_json::Value::Array(g.0.clone()).to_string());
}
/// The frame's part written out, at most every 100 ms.
pub(super) fn publish(p: &Part) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    static START: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed().as_millis() < 100) {
            return;
        }
        *last = Some(Instant::now());
    }
    let t = START.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Instant::now).elapsed().as_secs_f64();
    let r = |v: f64| (v * 100_000.0).round() / 100_000.0;
    let v3 = |v: V3| serde_json::json!([r(v[0]), r(v[1]), r(v[2])]);
    let q4 = |q: Q| serde_json::json!([r(q[0]), r(q[1]), r(q[2]), r(q[3])]);
    let mut j = serde_json::json!({
        "t": r(t),
        "kind": p.kind,
        "weapon": p.weapon,
        "free": p.free,
        "hand": v3(p.hand),
        "axis": v3(p.axis),
        "travel": r(p.travel),
        "radius": r(p.radius),
        "angle_max": r(p.angle_max),
        "grab_with": p.grab_with,
        "grabbed": p.grabbed,
        "rack": r(p.rack),
        "why": p.why,
        "exit": p.exit,
        "current_local": [[r(shared(1)), r(shared(2)), r(shared(3))], [r(shared(4)), r(shared(5)), r(shared(6)), r(shared(7))]],
    });
    if let Some(q) = p.hand_rot {
        j["hand_rot"] = q4(q);
    }
    if let Some(n) = p.near {
        j["near"] = serde_json::json!({ "d": r(n.d), "at": v3(n.at) });
        if let Some(q) = n.rot {
            j["near"]["rot"] = q4(q);
            if let Some(a) = p.hand_rot {
                let dq = (a[0] * q[0] + a[1] * q[1] + a[2] * q[2] + a[3] * q[3]).abs().min(1.0);
                j["near"]["angle"] = serde_json::json!(r(2.0 * dq.acos().to_degrees()));
            }
        }
        if p.free == 0 {
            let rot = n.rot.or(p.hand_rot).unwrap_or([0.0, 0.0, 0.0, 1.0]);
            if let Some((lp, lq)) = left_local(n.at, rot) {
                j["want_local"] = serde_json::json!([v3(lp), q4(lq)]);
            }
            // a full stroke back: the grip moved against the axis by the part's travel
            let back = [n.at[0] - p.axis[0] * p.travel, n.at[1] - p.axis[1] * p.travel, n.at[2] - p.axis[2] * p.travel];
            if let Some((lp, lq)) = left_local(back, rot) {
                j["pull_local"] = serde_json::json!([v3(lp), q4(lq)]);
            }
        }
    }
    let path = cvr_host::paths::dir(cvr_host::paths::Folder::Log).join("reload_reach.json");
    let _ = std::fs::write(path, j.to_string());
}
