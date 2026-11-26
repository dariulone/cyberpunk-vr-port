//! main's reload/mag_flick.lua, reload/grip_arc.lua and reload/mag_release.lua: B owns a pistol magazine's release, and
//! for its first 220 ms the controller can direct that same falling prop outward with a flick (the XR samples before
//! the IK and the recoil, 26cdb7f83); the slide's grip tested at the nearest point of the cant-relief arc, not only at
//! its two ends (9528baf08's arc); a B edge hands a seated pistol magazine straight to an available grip (H3VR2's
//! MagazineRelease.OnMagazineRemove -> GrabMagazine -> ForceGrab, 4a50f93cb).
use super::belt::{native, V3};
use crate::script_call::{static_function, Value};
use red4ext_abi::handle::Handle;

/// The flick's sample stamps wrap at 2^24 ms.
const WRAP: f32 = 16777216.0;
struct Armed {
    drop: u64,
    side: usize,
    origin: f32,
    seq: f32,
    start: f32,
    stamp: f32,
    p: V3,
    head: V3,
    ax: f64,
    az: f64,
    distance: f64,
    duration: f64,
    steps: u32,
    elapsed: f64,
}
#[derive(Default)]
pub(super) struct Flick {
    armed: Option<Armed>,
    pub releases: u32,
    pub flicks: u32,
    pub last_delta: f64,
}
fn v3(v: [f32; 4]) -> V3 {
    [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]
}
fn finite3(v: [f32; 4]) -> bool {
    v[0].is_finite() && v[1].is_finite() && v[2].is_finite()
}
fn paused() -> bool {
    super::requests_handler().ok().filter(|r| !r.is_null()).is_some_and(|r| super::belt::m::<bool>(&r, "IsGamePaused", &[]).unwrap_or(false))
}
/// The weapon is a handgun (WeaponObject.IsOfType(id, Wea_Handgun)).
pub(super) fn is_handgun(weapon: &Handle) -> bool {
    let Ok(id) = super::belt::m::<[u64; 2]>(weapon, "GetItemID", &[]) else { return false };
    let Some(t) = crate::script_call::enum_value("gamedataItemType", "Wea_Handgun") else { return false };
    static_function("WeaponObject", "IsOfType")
        // SAFETY: a static function (no context).
        .and_then(|f| unsafe { crate::script_call::invoke::<bool>(core::ptr::null_mut(), f, &[Value::Ptr(id.as_ptr().cast()), Value::U64(t as u64)]) })
        .unwrap_or(false)
}
/// readFrame: the physical weapon hand's throw frame -- the tracking origin, the sample clock, the hand, the
/// tracking-to-world yaw, the head's direction and position -- while the gun is the trigger hand's and its trigger is
/// off; none from an older or partial installation (the plain B drop then).
fn sample(side: usize) -> Option<Vec<[f32; 4]>> {
    if native::<bool>("VRWeaponTriggerEnabled", &[]) != Some(true) || paused() {
        return None;
    }
    let weapon_side = if native::<bool>("VRWeaponInLeftHand", &[]) == Some(true) { 0 } else { 1 };
    if weapon_side != side || super::belt::shared(if side == 1 { 160 } else { 154 }) > 0.2 {
        return None;
    }
    let f = super::aim_threat::native_frame("VRMeleeThrowFrame")?;
    (f.len() >= 6 && finite3(f[0]) && finite3(f[1]) && finite3(f[4]) && finite3(f[5]) && f[2][0].is_finite() && f[2][1].is_finite()).then_some(f)
}
impl Flick {
    pub(super) fn cancel(&mut self) {
        self.armed = None;
    }
    /// M.begin: a pistol's B release armed from the gaze's lateral axis at that moment.
    pub(super) fn begin(&mut self, weapon: &Handle, side: usize, drop: Option<u64>) {
        self.armed = None;
        let Some(drop) = drop else { return };
        if !is_handgun(weapon) {
            return;
        }
        self.releases += 1;
        self.last_delta = 0.0;
        let Some(f) = sample(side) else { return };
        let gaze = f[4];
        let horizontal = (f64::from(gaze[0]).powi(2) + f64::from(gaze[2]).powi(2)).sqrt();
        if horizontal < 0.4 {
            return;
        }
        let sign = if side == 1 { 1.0 } else { -1.0 };
        self.armed = Some(Armed {
            drop,
            side,
            origin: f[0][2],
            seq: f[0][1],
            start: f[0][0],
            stamp: f[0][0],
            p: v3(f[1]),
            head: v3(f[5]),
            ax: -sign * f64::from(gaze[2]) / horizontal,
            az: sign * f64::from(gaze[0]) / horizontal,
            distance: 0.0,
            duration: 0.0,
            steps: 0,
            elapsed: 0.0,
        });
    }
    /// M.update: the falling magazine's velocity pushed outward once two consecutive samples moved the hand 6.5 cm
    /// along the armed lateral axis (common head and hand translation and a controller turning at rest count nothing).
    pub(super) fn update(&mut self, drop: u64, side: usize, held: bool, dt: f64, velocity: &mut V3, max_speed: f64) {
        let Some(a) = self.armed.as_mut() else { return };
        if !held || a.drop != drop || a.side != side || !dt.is_finite() || dt <= 0.0 || dt > 0.08 {
            self.armed = None;
            return;
        }
        a.elapsed += dt;
        let Some(f) = sample(side) else {
            self.armed = None;
            return;
        };
        if a.elapsed > 0.22 || f[0][2] != a.origin || (f[0][0] - a.start).rem_euclid(WRAP) > 220.0 {
            self.armed = None;
            return;
        }
        if f[0][1] == a.seq {
            return;
        }
        let seconds = f64::from((f[0][0] - a.stamp).rem_euclid(WRAP)) * 0.001;
        if seconds <= 0.0 || seconds > 0.08 {
            self.armed = None;
            return;
        }
        let (p, h) = (v3(f[1]), v3(f[5]));
        let (dx, dy, dz) = (p[0] - a.p[0], p[1] - a.p[1], p[2] - a.p[2]);
        let (hx, hz) = (h[0] - a.head[0], h[2] - a.head[2]);
        (a.p, a.head, a.stamp, a.seq) = (p, h, f[0][0], f[0][1]);
        let length2 = dx * dx + dy * dy + dz * dz;
        if length2 > (4.0 * seconds).powi(2) {
            // a tracking jump
            self.armed = None;
            return;
        }
        let outward = dx * a.ax + dz * a.az;
        let travel = outward.min(outward - hx * a.ax - hz * a.az);
        if travel < 0.6 * seconds || outward * outward < 0.64 * length2 {
            (a.distance, a.duration, a.steps) = (0.0, 0.0, 0);
            return;
        }
        (a.distance, a.duration, a.steps) = (a.distance + travel, a.duration + seconds, a.steps + 1);
        if a.steps < 2 || a.distance < 0.065 {
            return;
        }
        let (ax, az, distance, duration) = (a.ax, a.az, a.distance, a.duration);
        self.armed = None;
        let yaw = f[2];
        let (yx, yy) = (f64::from(yaw[0]), f64::from(yaw[1]));
        let (wx, wy) = (yx * ax + yy * az, yy * ax - yx * az);
        let impulse = (0.7 + distance / duration).min(2.0);
        let (vx, vy, vz) = (velocity[0] + wx * impulse, velocity[1] + wy * impulse, velocity[2]);
        let scale = (max_speed / (vx * vx + vy * vy + vz * vz + 1e-12).sqrt()).min(1.0);
        self.last_delta = (vx * scale - velocity[0]) * wx + (vy * scale - velocity[1]) * wy;
        *velocity = [vx * scale, vy * scale, vz * scale];
        self.flicks += 1;
    }
}
/// grip_arc.lua A.closest: the wrist's nearest angle on the cant-relief arc [0, phi] about `a` (atan2 wraps at pi: both
/// representations and the ends compared before the bounded arc is chosen).
pub(super) fn closest(a: V3, phi: f64, t: V3, h: V3) -> f64 {
    let ta = t[0] * a[0] + t[1] * a[1] + t[2] * a[2];
    let ha = h[0] * a[0] + h[1] * a[1] + h[2] * a[2];
    let p = [t[0] - a[0] * ta, t[1] - a[1] * ta, t[2] - a[2] * ta];
    let q = [h[0] - a[0] * ha, h[1] - a[1] * ha, h[2] - a[2] * ha];
    if p[0] * p[0] + p[1] * p[1] + p[2] * p[2] < 1e-8 || q[0] * q[0] + q[1] * q[1] + q[2] * q[2] < 1e-8 {
        return 0.0;
    }
    let c = [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]];
    let theta = (c[0] * a[0] + c[1] * a[1] + c[2] * a[2]).atan2(p[0] * q[0] + p[1] * q[1] + p[2] * q[2]);
    let (lo, hi) = (phi.min(0.0), phi.max(0.0));
    let (mut best, mut score) = (0.0, f64::NEG_INFINITY);
    for k in [-1.0, 0.0, 1.0] {
        let candidate = (theta + k * 2.0 * std::f64::consts::PI).clamp(lo, hi);
        let alignment = (candidate - theta).cos();
        if alignment > score {
            (best, score) = (candidate, alignment);
        }
    }
    best
}
/// mag_release.lua: a logical B edge with a grip available and the seated magazine in reach takes a handgun's magazine
/// into that hand (not while the slide, the rotator or a hatch is busy, nor with two hands on the gun).
pub(super) fn catch_seated_mag(weapon: &Handle, edge: bool, grip_available: bool, in_reach: bool, busy: bool) -> bool {
    if !(edge && grip_available && in_reach) || busy {
        return false;
    }
    if native::<bool>("GetVRTwoHandActive", &[]) == Some(true) {
        return false;
    }
    is_handgun(weapon)
}
