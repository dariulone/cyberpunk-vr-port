//! Scene camera comfort (formerly include/Camera/SceneCameraComfort.hpp): how much of a scene camera's own turning
//! reaches the headset.
//!
//! A scene that drives the camera puts its orientation under the head, and the head composes on top. On a flat screen
//! a slow pan, a handheld sway or a push-in tilt is framing; in the headset it turns the world round a player who is
//! not turning (the post-prologue montage is 23 camera animations with a cut every 0.5-1.7 s, every shot moving). So
//! the view holds its orientation while the scene camera turns within `dead_deg` of it; past that it eases after the
//! scene camera at `catch_up_sec`, and the gap never grows past `dead_deg + max_extra_deg`. A cut is not a turn: a
//! turn larger than `cut_deg` between two steps, or a camera jump of `cut_metres`, starts a new shot, taken at once.
//! Position is not filtered.
use cvr_body::cmath::{acos, atan2, clamp, cos, exp, max, min, sin, sqrt};

pub type Quat = [f32; 4]; // x, y, z, w

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneComfortSettings {
    /// xr_scene_cam_comfort_deg; 0 switches the filter off.
    pub dead_deg: f32,
    pub catch_up_sec: f32,
    pub max_extra_deg: f32,
    pub cut_deg: f32,
    pub cut_metres: f32,
}
impl Default for SceneComfortSettings {
    fn default() -> Self {
        SceneComfortSettings { dead_deg: 30.0, catch_up_sec: 0.35, max_extra_deg: 15.0, cut_deg: 35.0, cut_metres: 1.0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SceneCameraComfort {
    held: Quat,
    prev_scene: Quat,
    prev_pos: [f32; 3],
    lag_deg: f32,
    cuts: u32,
    valid: bool,
    have_pos: bool,
}
impl Default for SceneCameraComfort {
    fn default() -> Self {
        Self::new()
    }
}
impl SceneCameraComfort {
    pub const fn new() -> Self {
        SceneCameraComfort { held: [0.0, 0.0, 0.0, 1.0], prev_scene: [0.0, 0.0, 0.0, 1.0], prev_pos: [0.0; 3], lag_deg: 0.0, cuts: 0, valid: false, have_pos: false }
    }
    pub fn reset(&mut self) {
        self.valid = false;
        self.have_pos = false;
        self.lag_deg = 0.0;
    }
    pub fn valid(&self) -> bool {
        self.valid
    }
    pub fn lag_deg(&self) -> f32 {
        self.lag_deg
    }
    pub fn cuts(&self) -> u32 {
        self.cuts
    }
    /// `scene`: the scene camera's world orientation this step; `position`: its world position in metres, when known
    /// (a cut is then also told by a jump); `dt`: seconds since the previous step. The orientation to compose onto.
    pub fn step(&mut self, scene: &Quat, position: Option<&[f32; 3]>, dt: f32, s: &SceneComfortSettings) -> Quat {
        let Some(q) = normalize(scene) else { return *scene };
        if !(s.dead_deg > 0.0) || !s.dead_deg.is_finite() {
            self.reset();
            return q;
        }
        let mut dt = if !dt.is_finite() || dt < 0.0 { 0.0 } else { dt };
        dt = min(dt, 0.1); // a hitch or a load screen is not a long catch-up
        let mut cut = !self.valid;
        if self.valid && angle_deg(&self.prev_scene, &q) > s.cut_deg {
            cut = true;
        }
        if let (true, Some(p), true) = (self.valid, position, self.have_pos) {
            let (dx, dy, dz) = (p[0] - self.prev_pos[0], p[1] - self.prev_pos[1], p[2] - self.prev_pos[2]);
            let d2 = dx * dx + dy * dy + dz * dz;
            if !d2.is_finite() || d2 > s.cut_metres * s.cut_metres {
                cut = true;
            }
        }
        self.prev_scene = q;
        self.have_pos = position.is_some();
        if let Some(p) = position {
            self.prev_pos = *p;
        }
        if cut {
            if self.valid {
                self.cuts += 1;
            }
            self.held = q;
            self.valid = true;
            self.lag_deg = 0.0;
            return q;
        }
        let mut lag = angle_deg(&self.held, &q);
        if lag > s.dead_deg {
            let excess = lag - s.dead_deg;
            let tau = max(s.catch_up_sec, 1e-3);
            let mut step = excess * (1.0 - exp(-dt / tau));
            let cap = s.dead_deg + max(s.max_extra_deg, 0.0);
            if lag - step > cap {
                step = lag - cap;
            }
            self.held = slerp(&self.held, q, clamp(step / lag, 0.0, 1.0));
            lag = angle_deg(&self.held, &q);
        }
        self.lag_deg = lag;
        self.held
    }
    /// The same filter for a heading alone (radians about the vertical), for scenes whose camera base is a yaw.
    pub fn step_yaw(&mut self, yaw: f32, dt: f32, s: &SceneComfortSettings) -> f32 {
        let q = [0.0, 0.0, sin(yaw * 0.5), cos(yaw * 0.5)];
        let out = self.step(&q, None, dt, s);
        2.0 * atan2(out[2], out[3])
    }
}

/// The relative rotation's angle, from its vector part: acos of a float dot loses about 0.02 deg near identity.
pub fn angle_deg(a: &Quat, b: &Quat) -> f32 {
    let x = a[3] * b[0] - a[0] * b[3] - a[1] * b[2] + a[2] * b[1];
    let y = a[3] * b[1] + a[0] * b[2] - a[1] * b[3] - a[2] * b[0];
    let z = a[3] * b[2] - a[0] * b[1] + a[1] * b[0] - a[2] * b[3];
    let w = a[3] * b[3] + a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    2.0 * atan2(sqrt(x * x + y * y + z * z), w.abs()) * 57.295_779_51
}
fn normalize(q: &Quat) -> Option<Quat> {
    let n = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if !n.is_finite() || n < 0.25 || n > 4.0 {
        return None;
    }
    let inv = 1.0 / sqrt(n);
    Some([q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv])
}
fn slerp(a: &Quat, mut b: Quat, t: f32) -> Quat {
    let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if d < 0.0 {
        d = -d;
        for v in b.iter_mut() {
            *v = -*v;
        }
    }
    let mut r = [0.0f32; 4];
    if d > 0.9995 {
        for i in 0..4 {
            r[i] = a[i] + t * (b[i] - a[i]);
        }
    } else {
        let th = acos(min(d, 1.0));
        let sn = sin(th);
        let (wa, wb) = (sin((1.0 - t) * th) / sn, sin(t * th) / sn);
        for i in 0..4 {
            r[i] = wa * a[i] + wb * b[i];
        }
    }
    normalize(&r).unwrap_or(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn yaw(deg: f32) -> Quat {
        let h = deg.to_radians() * 0.5;
        [0.0, 0.0, h.sin(), h.cos()]
    }
    #[test]
    fn holds_within_the_dead_zone() {
        let mut c = SceneCameraComfort::new();
        let s = SceneComfortSettings::default();
        assert_eq!(c.step(&yaw(0.0), None, 0.016, &s), yaw(0.0));
        let held = c.step(&yaw(20.0), None, 0.016, &s);
        assert!(angle_deg(&held, &yaw(0.0)) < 1e-3, "a 20 degree pan inside 30 does not reach the headset");
        assert!((c.lag_deg() - 20.0).abs() < 0.01);
    }
    #[test]
    fn a_cut_is_taken_at_once() {
        let mut c = SceneCameraComfort::new();
        let s = SceneComfortSettings::default();
        c.step(&yaw(0.0), Some(&[0.0; 3]), 0.016, &s);
        let after = c.step(&yaw(10.0), Some(&[5.0, 0.0, 0.0]), 0.016, &s);
        assert!(angle_deg(&after, &yaw(10.0)) < 1e-3, "a jump of five metres is a new shot");
        assert_eq!(c.cuts(), 1);
    }
    #[test]
    fn the_lag_is_capped() {
        let mut c = SceneCameraComfort::new();
        let s = SceneComfortSettings::default();
        c.step(&yaw(0.0), None, 0.016, &s);
        for k in 1..30 {
            c.step(&yaw(k as f32 * 2.0), None, 0.016, &s);
        }
        assert!(c.lag_deg() <= s.dead_deg + s.max_extra_deg + 1e-3);
    }
}
