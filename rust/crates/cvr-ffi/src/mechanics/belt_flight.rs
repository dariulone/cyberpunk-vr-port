//! mods/cet/CyberpunkVRPort_TacticalBelt/grenade/flight.lua: the port's own thrown grenade -- a ballistic integrator
//! (the grenade's gravity, drag, spin) with a swept ray against the world (soft materials passed through, the player's
//! floor as a fallback), bounces until it rests, then the fuse; the prop entity is spawned at the hand and placed every
//! step (VREntitySetWorldTransform). Detonation is reported back to the grenade module, which sets off the real one.
use super::belt::{q4, v4, Cfg, WorldTransform, V3};
use crate::script_call::Value;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;

const RAY_PRESETS: [&str; 3] = ["Bullet", "Camera", "Sight Blocker"];
const SOFT: [&str; 9] = ["foliage", "grass", "leaf", "leaves", "bush", "plant", "vegetation", "tree", "fabric"];

pub(super) struct Flight {
    slot: Option<String>,
    t: f64,
    rest: Option<f64>,
    p: V3,
    v: V3,
    q: [f64; 4],
    w: V3,
    id: Option<u64>,
    ent: Handle,
}
#[derive(Default)]
pub(super) struct Flights {
    live: Vec<Flight>,
    ray_pick: usize,
    mat_seen: u32,
}
pub(super) struct Detonation {
    pub slot: Option<String>,
    pub at: V3,
}
fn is_soft(mat: &str) -> bool {
    let m = mat.to_lowercase();
    !m.is_empty() && SOFT.iter().any(|s| m.contains(s))
}
#[repr(C, align(16))]
struct TraceResult([f32; 24]);
impl Flights {
    pub(super) fn count(&self) -> usize {
        self.live.len()
    }
    pub(super) fn flying(&self) -> bool {
        !self.live.is_empty()
    }
    pub(super) fn slots_in_use(&self) -> Vec<String> {
        self.live.iter().filter_map(|f| f.slot.clone()).collect()
    }
    fn ray_hit(&mut self, a: V3, b: V3) -> Option<(V3, V3, String)> {
        let sq = super::game_system("GetSpatialQueriesSystem");
        if sq.is_null() {
            return None;
        }
        let (a4, b4) = (v4(a, 1.0), v4(b, 1.0));
        for n in 0..RAY_PRESETS.len() {
            let i = (self.ray_pick + n) % RAY_PRESETS.len();
            let mut tr = TraceResult([0.0; 24]);
            let hit = super::belt::m::<bool>(
                &sq,
                "SyncRaycastByQueryPreset",
                &[
                    Value::Ptr(a4.as_ptr().cast()),
                    Value::Ptr(b4.as_ptr().cast()),
                    Value::U64(CName::hashed(RAY_PRESETS[i]).0),
                    Value::Out((&mut tr as *mut TraceResult).cast()),
                    Value::Bool(true),
                ],
            );
            if hit == Ok(true) {
                self.ray_pick = i;
                let mat = CName(u64::from(tr.0[6].to_bits()) | (u64::from(tr.0[7].to_bits()) << 32)).text().to_string();
                if self.mat_seen < 6 {
                    self.mat_seen += 1;
                    super::belt_grenade::trace_line(&format!("flight: hit {mat} via {}", RAY_PRESETS[i]));
                }
                let t = &tr.0;
                return Some((
                    [f64::from(t[0]), f64::from(t[1]), f64::from(t[2])],
                    [f64::from(t[3]), f64::from(t[4]), f64::from(t[5])],
                    mat,
                ));
            }
        }
        None
    }
    fn ray_first_hard(&mut self, cfg: &Cfg, a: V3, b: V3) -> Option<(V3, V3, String)> {
        let s = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
        if len < 1e-6 {
            return None;
        }
        let u = [s[0] / len, s[1] / len, s[2] / len];
        let mut at = a;
        for _ in 0..4 {
            let (hp, hn, mat) = self.ray_hit(at, b)?;
            if !(cfg.not_false("grenade_flight_soft") && is_soft(&mat)) {
                return Some((hp, hn, mat));
            }
            let step = cfg.f("grenade_flight_soft_step", 0.05);
            at = [hp[0] + u[0] * step, hp[1] + u[1] * step, hp[2] + u[2] * step];
            if (b[0] - at[0]) * u[0] + (b[1] - at[1]) * u[1] + (b[2] - at[2]) * u[2] <= 0.0 {
                return None;
            }
        }
        None
    }
    fn floor_holds(&mut self, cfg: &Cfg, x: f64, y: f64, fz: f64) -> bool {
        let drop = cfg.f("grenade_flight_floor_drop", 0.05);
        let depth = cfg.f("grenade_flight_floor_probe", 50.0);
        let ground = |s: &mut Flights, gx: f64, gy: f64| s.ray_first_hard(cfg, [gx, gy, fz + 0.1], [gx, gy, fz - depth]).map(|h| h.0[2]);
        let below = ground(self, x, y);
        let Some(below) = below.filter(|b| *b < fz) else { return true };
        let pl = super::belt::player();
        let Ok(p) = super::belt::m::<[f32; 4]>(&pl, "GetWorldPosition", &[]) else { return true };
        let feet = ground(self, f64::from(p[0]), f64::from(p[1]));
        let holds = feet.is_none_or(|f| below >= fz.min(f) - drop);
        super::belt_grenade::trace_line(&format!(
            "flight: falling past the feet at {fz:.2}, ground {:.2} below it, under the feet {} -> floor {holds}",
            fz - below,
            feet.map_or("none".to_string(), |f| format!("{:.2}", fz - f))
        ));
        holds
    }
    /// M.launch: the prop spawned at the hand with the release velocity and a spin across it.
    pub(super) fn launch(&mut self, cfg: &Cfg, path: &str, slot: Option<String>, p: V3, q: [f64; 4], v: V3) -> bool {
        let sp = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let (mut ax, mut ay) = (-v[1], v[0]);
        let mut an = (ax * ax + ay * ay).sqrt();
        if an < 1e-4 {
            (ax, ay, an) = (1.0, 0.0, 1.0);
        }
        let rate = cfg.f("grenade_flight_spin", 0.9) * sp;
        let id = super::ent_spawn::spawn(path, p, q, 0);
        let Some(id) = id else {
            super::belt_grenade::trace_line("flight: spawn failed");
            return false;
        };
        self.live.push(Flight { slot, t: 0.0, rest: None, p, v, q, w: [ax / an * rate, ay / an * rate, 0.0], id: Some(id), ent: Handle::null() });
        true
    }
    fn step(&mut self, cfg: &Cfg, i: usize, h: f64) -> bool {
        let fuse = cfg.f("grenade_fuse_s", 2.5);
        {
            let f = &mut self.live[i];
            f.t += h;
            if let Some(rest) = f.rest {
                if f.t >= fuse || f.t - rest >= cfg.f("grenade_fuse_after_hit_s", 1.2) {
                    return true;
                }
            }
        }
        if self.live[i].rest.is_none() {
            let (p, nxt, r) = {
                let f = &mut self.live[i];
                f.v[2] += cfg.f("grenade_flight_gravity", -10.5) * h;
                let drag = (1.0 - cfg.f("grenade_flight_drag", 0.05) * h).max(0.0);
                f.v = f.v.map(|c| c * drag);
                if f.w.iter().map(|c| c * c).sum::<f64>() > 1e-8 {
                    let a = super::belt::qmul([f.w[0] * 0.5 * h, f.w[1] * 0.5 * h, f.w[2] * 0.5 * h, 0.0], f.q);
                    let nq = [f.q[0] + a[0], f.q[1] + a[1], f.q[2] + a[2], f.q[3] + a[3]];
                    let l = nq.iter().map(|c| c * c).sum::<f64>().sqrt();
                    if l > 1e-9 {
                        f.q = nq.map(|c| c / l);
                    }
                }
                (f.p, [f.p[0] + f.v[0] * h, f.p[1] + f.v[1] * h, f.p[2] + f.v[2] * h], cfg.f("grenade_flight_radius", 0.035))
            };
            let s = [nxt[0] - p[0], nxt[1] - p[1], nxt[2] - p[2]];
            let sl = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
            let mut hit = None;
            if sl > 1e-4 {
                let k = (sl + r) / sl;
                hit = self.ray_first_hard(cfg, p, [p[0] + s[0] * k, p[1] + s[1] * k, p[2] + s[2] * k]);
            }
            if hit.is_none() && self.live[i].v[2] < 0.0 && cfg.not_false("grenade_flight_floor") {
                let pl = super::belt::player();
                if let Ok(pp) = super::belt::m::<[f32; 4]>(&pl, "GetWorldPosition", &[]) {
                    let fz = f64::from(pp[2]) + cfg.f("grenade_flight_floor_lift", 0.02);
                    if nxt[2] - r <= fz && p[2] >= fz && self.floor_holds(cfg, nxt[0], nxt[1], fz) {
                        hit = Some(([nxt[0], nxt[1], fz], [0.0, 0.0, 1.0], "floor".into()));
                    }
                }
            }
            let f = &mut self.live[i];
            if let Some((hp, hn, _)) = hit {
                let e = cfg.f("grenade_flight_bounce", 0.35);
                let vn = f.v[0] * hn[0] + f.v[1] * hn[1] + f.v[2] * hn[2];
                f.p = [hp[0] + hn[0] * r, hp[1] + hn[1] * r, hp[2] + hn[2] * r];
                f.v = [(f.v[0] - 2.0 * vn * hn[0]) * e, (f.v[1] - 2.0 * vn * hn[1]) * e, (f.v[2] - 2.0 * vn * hn[2]) * e];
                f.w = f.w.map(|c| c * 0.5);
                if (f.v[0] * f.v[0] + f.v[1] * f.v[1] + f.v[2] * f.v[2]).sqrt() < cfg.f("grenade_flight_rest_ms", 1.2) {
                    f.rest = Some(f.t);
                    (f.v, f.w) = ([0.0; 3], [0.0; 3]);
                }
            } else {
                f.p = nxt;
            }
            if f.t >= fuse {
                return true;
            }
        }
        let f = &mut self.live[i];
        if f.ent.is_null() {
            f.ent = f.id.and_then(|id| super::find_entity(id).ok()).unwrap_or_else(Handle::null);
        }
        if !f.ent.is_null() {
            let t = transform(f.p, f.q);
            let mut bytes = [0u8; 0x20];
            for (k, w) in t.0.iter().enumerate() {
                bytes[k * 4..k * 4 + 4].copy_from_slice(&w.to_le_bytes());
            }
            crate::engine_objects::entity_set_world_transform(&f.ent, &bytes);
        }
        false
    }
    /// M.tick: every grenade in the air stepped (at most 40 ms a step); the ones done despawned and reported.
    pub(super) fn tick(&mut self, cfg: &Cfg, dt: f64) -> Vec<Detonation> {
        let mut done = Vec::new();
        if self.live.is_empty() {
            return done;
        }
        let h = if dt > 0.0 { dt.min(0.04) } else { 0.016 };
        for i in (0..self.live.len()).rev() {
            if self.step(cfg, i, h) {
                let f = self.live.remove(i);
                done.push(Detonation { slot: f.slot.clone(), at: f.p });
                despawn(f);
            }
        }
        done
    }
}
fn transform(p: V3, q: [f64; 4]) -> WorldTransform {
    let mut t = WorldTransform([0; 8]);
    for k in 0..3 {
        t.0[k] = ((p[k] * 131072.0) as i32) as u32;
    }
    let qv = q4(q);
    for k in 0..4 {
        t.0[4 + k] = qv[k].to_bits();
    }
    t
}
fn despawn(mut f: Flight) {
    if f.ent.is_null() {
        f.ent = f.id.and_then(|id| super::find_entity(id).ok()).unwrap_or_else(Handle::null);
    }
    if f.ent.is_null() {
        return;
    }
    super::ent_spawn::despawn(&f.ent);
}
