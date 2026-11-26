//! The projection's small arithmetic the XR loop and the stereo path share, operation for operation with the headers
//! it came from: the pipeline prediction (include/Runtimes/PipelinePrediction.hpp) -- the render camera aimed at its
//! estimated presentation slot, the extra horizon bounded and slewed -- and the runtime FOV correction
//! (include/Runtimes/RuntimeFovCorrection.hpp) -- a canted or pitched headset's frusta recentred onto the eyes' axes,
//! the de-canted span, the panel's covering FOV -- and the asymmetric projection's eye windows
//! (include/Runtimes/AsymmetricProjection.hpp): each eye's own frustum as tangents, the canted eyes turned back, the
//! inner edges guarded, one vertical span, fitted to the render target's pixels.
use cvr_body::cmath::{atan, atan2, sqrt, tan};
use cvr_body::xr::Quat;

extern "C" {
    /// The C runtime's double exp, the one std::exp(double) calls.
    #[link_name = "exp"]
    fn crt_exp(x: f64) -> f64;
}
fn exp(x: f64) -> f64 {
    // SAFETY: a pure C runtime function.
    unsafe { crt_exp(x) }
}
/// std::clamp(v, lo, hi): `v < lo ? lo : hi < v ? hi : v`.
fn clamp_d(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}
fn clamp_f(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if hi < v {
        hi
    } else {
        v
    }
}

/// cvr::tracking::PipelinePrediction: the lead beyond the display time the XR cycle located at.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PipelinePrediction {
    pub lead_ns: f64,
    pub last_display: i64,
}
impl PipelinePrediction {
    pub fn reset(&mut self) {
        self.lead_ns = 0.0;
        self.last_display = 0;
    }
    /// The display time to aim at: `display` plus the fitted lead (plausible fits only, at most two periods and
    /// 25 ms), filtered with a 100 ms time constant and slewed at most 10% of a step per step, scaled by `strength`.
    pub fn target(&mut self, display: i64, period: i64, fitted: i64, valid: bool, strength: f32) -> i64 {
        if display <= 0 || !(4_000_000..=40_000_000).contains(&period) {
            self.reset();
            return display;
        }
        if self.last_display != 0 && (display <= self.last_display || display - self.last_display > 250_000_000) {
            self.reset();
        }
        let step = if self.last_display != 0 {
            let d = display - self.last_display;
            (if 40_000_000 < d { 40_000_000 } else { d }) as f64
        } else {
            period as f64
        };
        self.last_display = display;
        let twice = 2.0 * period as f64;
        let maximum = if 25_000_000.0 < twice { 25_000_000.0 } else { twice };
        // A fit beyond the short pipeline bound is no license to extrapolate.
        let plausible = valid && fitted >= display - period && fitted <= display + 4 * period;
        let desired = if plausible { clamp_d((fitted - display) as f64, 0.0, maximum) } else { 0.0 };
        let filtered = self.lead_ns + (desired - self.lead_ns) * (1.0 - exp(-step / 100_000_000.0));
        let slew = step * 0.10;
        self.lead_ns = clamp_d(filtered, self.lead_ns - slew, self.lead_ns + slew);
        let scale = if strength.is_finite() { clamp_f(strength, 0.0, 1.0) } else { 0.0 };
        display + (clamp_d(self.lead_ns, 0.0, maximum) * scale as f64) as i64
    }
}

/// XrFovf.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fov {
    pub angle_left: f32,
    pub angle_right: f32,
    pub angle_up: f32,
    pub angle_down: f32,
}
/// RuntimeFovCorrection.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RuntimeFovCorrection {
    pub eye: [Fov; 2],
    /// Left eye +yawDelta, right eye -yawDelta.
    pub yaw_delta_rad: f32,
    /// Both eyes pitched up by +pitchDelta.
    pub pitch_delta_rad: f32,
    pub yaw_enabled: bool,
    pub pitch_enabled: bool,
}
pub const RUNTIME_FOV_DELTA_THRESHOLD_RAD: f32 = 0.017000001;

/// ComputeRuntimeFovCorrection: each frustum recentred onto its eye's axis, vertically by the mean pitch and
/// horizontally by the mean cant, when either passes about a degree.
pub fn runtime_fov_correction(left: &Fov, right: &Fov) -> RuntimeFovCorrection {
    let mut out = RuntimeFovCorrection { eye: [*left, *right], ..Default::default() };
    let (up_l, down_l, up_r, down_r) = (left.angle_up, -left.angle_down, right.angle_up, -right.angle_down);
    let delta_v = ((up_l + up_r) - (down_l + down_r)) * 0.25;
    if delta_v.abs() > RUNTIME_FOV_DELTA_THRESHOLD_RAD {
        out.pitch_enabled = false;
        out.pitch_delta_rad = delta_v;
        out.eye[0].angle_up -= delta_v;
        out.eye[0].angle_down -= delta_v;
        out.eye[1].angle_up -= delta_v;
        out.eye[1].angle_down -= delta_v;
    }
    let (left_l, right_l, left_r, right_r) = (-left.angle_left, left.angle_right, -right.angle_left, right.angle_right);
    let delta_h = ((left_l + right_r) - (left_r + right_l)) * 0.25;
    if delta_h.abs() > RUNTIME_FOV_DELTA_THRESHOLD_RAD {
        out.yaw_enabled = false;
        out.yaw_delta_rad = delta_h;
        out.eye[0].angle_left += delta_h;
        out.eye[0].angle_right += delta_h;
        out.eye[1].angle_left -= delta_h;
        out.eye[1].angle_right -= delta_h;
    }
    out
}
/// GetCorrectedGameHorizontalFovDeg: the de-canted span's mean width, in degrees.
pub fn corrected_game_horizontal_fov_deg(corr: &RuntimeFovCorrection) -> f32 {
    let h0 = corr.eye[0].angle_right - corr.eye[0].angle_left;
    let h1 = corr.eye[1].angle_right - corr.eye[1].angle_left;
    ((h0 + h1) * 0.5) * (180.0 / 3.1415926535)
}
/// GetPanelHalfAngles: the largest half-angles over both eyes.
pub fn panel_half_angles(left: &Fov, right: &Fov) -> (f32, f32) {
    let h = left.angle_left.abs().max(left.angle_right.abs()).max(right.angle_left.abs().max(right.angle_right.abs()));
    let v = left.angle_up.abs().max(left.angle_down.abs()).max(right.angle_up.abs().max(right.angle_down.abs()));
    (h, v)
}
/// GetPanelCoveringHorizontalFovDeg: the horizontal FOV whose frustum at `aspect` covers both panels, or 0.
pub fn panel_covering_horizontal_fov_deg(left: &Fov, right: &Fov, aspect: f32) -> f32 {
    if !(aspect > 0.01 && aspect < 100.0) {
        return 0.0;
    }
    let (half_h, half_v) = panel_half_angles(left, right);
    if !(half_h > 0.0) || !(half_v > 0.0) {
        return 0.0;
    }
    let half = half_h.max(atan(tan(half_v) * aspect));
    let deg = 2.0 * half * (180.0 / 3.1415926535);
    if deg > 1.0 && deg < 170.0 {
        deg
    } else {
        0.0
    }
}

/// cvr::projection::EyeWindow: a frustum as tangents (left, right, down, up).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EyeWindow {
    pub l: f32,
    pub r: f32,
    pub d: f32,
    pub u: f32,
}
/// cvr::projection::EngineProjection.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineProjection {
    pub vfov_deg: f32,
    pub aspect: f32,
    pub shift_x: f32,
    pub shift_y: f32,
}

/// EyeYawsAgainstHead: each eye's yaw against the eyes' mean orientation (a canted panel), when it is a plausible cant
/// (0.002 .. 0.35 rad); 0 otherwise.
pub fn eye_yaws_against_head(a: Quat, b: Quat) -> [f32; 2] {
    let mut yaw = [0.0f32; 2];
    let (mut bx, mut by, mut bz, mut bw) = (b.x, b.y, b.z, b.w);
    if a.x * bx + a.y * by + a.z * bz + a.w * bw < 0.0 {
        bx = -bx;
        by = -by;
        bz = -bz;
        bw = -bw;
    }
    let (mut hx, mut hy, mut hz, mut hw) = (a.x + bx, a.y + by, a.z + bz, a.w + bw);
    let n = sqrt(hx * hx + hy * hy + hz * hz + hw * hw);
    if !(n > 1e-6) {
        return yaw;
    }
    hx /= n;
    hy /= n;
    hz /= n;
    hw /= n;
    let eyes = [a, Quat { x: bx, y: by, z: bz, w: bw }];
    for (e, q) in eyes.iter().enumerate() {
        let rw = hw * q.w + hx * q.x + hy * q.y + hz * q.z;
        let rx = hw * q.x - hx * q.w - hy * q.z + hz * q.y;
        let ry = hw * q.y + hx * q.z - hy * q.w - hz * q.x;
        let rz = hw * q.z - hx * q.y + hy * q.x - hz * q.w;
        let fx = -2.0 * (rx * rz + rw * ry);
        let fz = -(1.0 - 2.0 * (rx * rx + ry * ry));
        let y = atan2(fx, -fz);
        yaw[e] = if y.is_finite() && y.abs() > 0.002 && y.abs() < 0.35 { y } else { 0.0 };
    }
    yaw
}

/// Windows (with its guard and target already chosen): both eyes' windows, or None for frusta that are not.
pub fn windows(fov: &[Fov; 2], eye_orientation: Option<&[Quat; 2]>, guard: f32, target: f32) -> Option<[EyeWindow; 2]> {
    let yaw = eye_orientation.map_or([0.0; 2], |o| eye_yaws_against_head(o[0], o[1]));
    const MAX: f32 = 1.48; // 85 deg
    let mut w = [EyeWindow::default(); 2];
    let mut span = 0.0f32;
    for e in 0..2 {
        let (l, r) = (fov[e].angle_left + yaw[e], fov[e].angle_right + yaw[e]);
        let (u, d) = (fov[e].angle_up, fov[e].angle_down);
        if !l.is_finite() || !r.is_finite() || !u.is_finite() || !d.is_finite() || !(l > -MAX && l < r && r < MAX && d > -MAX && d < u && u < MAX) {
            return None;
        }
        w[e] = EyeWindow { l: tan(l), r: tan(r), d: tan(d), u: tan(u) };
        if !(w[e].r - w[e].l > 0.05) || !(w[e].u - w[e].d > 0.05) {
            return None;
        }
        span = span.max(w[e].u - w[e].d);
    }
    if guard > 0.0 {
        let ul = w[0].l.min(w[1].l);
        let ur = w[0].r.max(w[1].r);
        for e in &mut w {
            e.l += guard * (ul - e.l);
            e.r += guard * (ur - e.r);
        }
    }
    let fit = target > 0.2 && target < 5.0;
    if fit {
        for e in &w {
            span = span.max((e.r - e.l) / target);
        }
    }
    for e in &mut w {
        let c = 0.5 * (e.u + e.d);
        e.d = c - 0.5 * span;
        e.u = c + 0.5 * span;
        if fit {
            let h = (e.r - e.l).max(span * target);
            let m = 0.5 * (e.r + e.l);
            e.l = m - 0.5 * h;
            e.r = m + 0.5 * h;
        }
    }
    Some(w)
}
/// ToFov.
pub fn to_fov(w: &EyeWindow) -> Fov {
    Fov { angle_left: atan(w.l), angle_right: atan(w.r), angle_up: atan(w.u), angle_down: atan(w.d) }
}
/// ToEngine: the engine's vertical FOV, aspect and lens shift for a window.
pub fn to_engine(w: &EyeWindow) -> EngineProjection {
    let (h, v) = (w.r - w.l, w.u - w.d);
    EngineProjection {
        vfov_deg: 2.0 * atan(0.5 * v) * (180.0 / 3.14159265358979),
        aspect: h / v,
        shift_x: -(w.r + w.l) / h,
        shift_y: -(w.u + w.d) / v,
    }
}
/// MapNdcX / MapNdcY: an NDC coordinate in one window to the same direction's in another.
pub fn map_ndc_x(n: f32, from: &EyeWindow, to: &EyeWindow) -> f32 {
    let t = 0.5 * (n * (from.r - from.l) + (from.r + from.l));
    (2.0 * t - (to.r + to.l)) / (to.r - to.l)
}
pub fn map_ndc_y(n: f32, from: &EyeWindow, to: &EyeWindow) -> f32 {
    let t = 0.5 * (n * (from.u - from.d) + (from.u + from.d));
    (2.0 * t - (to.u + to.d)) / (to.u - to.d)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// tools/motion_latency_tests/prediction.cpp, case for case.
    #[test]
    fn the_prediction_cases() {
        for hz in [60i64, 72, 90, 120, 144] {
            let period = 1_000_000_000 / hz;
            let mut display = 1_000_000_000i64;
            let mut previous = 0i64;
            let mut p = PipelinePrediction::default();
            let bound = (2 * period).min(25_000_000);
            for _ in 0..240 {
                display += period;
                let lead = p.target(display, period, display + period * 3, true, 1.0) - display;
                assert!(lead >= 0 && lead <= bound, "prediction bound");
                assert!(((lead - previous).abs() as f64) <= period as f64 * 0.101 + 2.0, "prediction slew");
                previous = lead;
            }
            assert!(previous >= bound - 10, "stable fit converges");
            for _ in 0..240 {
                display += period;
                p.target(display, period, 0, false, 1.0);
            }
            display += period;
            assert!(p.target(display, period, 0, false, 1.0) - display < 10, "stale fit decays");
            p.reset();
            display += period;
            assert_eq!(p.target(display, period, display + period, true, 0.0), display, "A/B off is exact");
            display += period;
            assert_eq!(p.target(display, period, display + period, true, f32::NAN), display, "invalid strength");
        }
    }
    #[test]
    fn a_canted_headset_is_recentred() {
        let d = core::f32::consts::PI / 180.0;
        let left = Fov { angle_left: -54.0 * d, angle_right: 40.0 * d, angle_up: 43.98 * d, angle_down: -54.27 * d };
        let right = Fov { angle_left: -40.0 * d, angle_right: 54.0 * d, angle_up: 43.98 * d, angle_down: -54.27 * d };
        let c = runtime_fov_correction(&left, &right);
        assert!((c.yaw_delta_rad / d - 7.0).abs() < 1e-3 && (c.pitch_delta_rad / d + 5.145).abs() < 1e-3, "{c:?}");
        assert!((corrected_game_horizontal_fov_deg(&c) - 94.0).abs() < 1e-3);
    }
}
