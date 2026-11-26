//! The FBT calibration helpers' arithmetic (src/Fbt/Game/Helpers.cpp, `WithAlpha`, `Whiter` and `Project`'s eye
//! space): a colour with its alpha scaled (clamped to 0..255); a colour mixed toward white by a share (each of red,
//! green and blue, alpha kept; ImGui's RGBA packing, red lowest); and a world point in one eye's camera frame -- offset
//! from the located camera by half an IPD along its right axis (left eye minus, right eye plus, none in mono) and turned
//! by the camera's inverse rotation (overlay_projection's rotate); and the projected point's place in the frame -- with
//! the asymmetric projection on, the other eye's point carried from the main eye's window to its own through NDC
//! (projection's map_ndc_x / map_ndc_y) -- in that eye's half of the backbuffer. And `DrawFbtCalibrationHelpers`' world
//! geometry: the camera's pitch (its forward, y, against the horizon), step 1's horizon line (3 m ahead along the
//! view's heading, 4 m wide, at the eyes' height; none looking straight up or down), a head-relative OpenXR point on the
//! located camera (OpenXR x, y, z -> camera x, -z, y), a STAGE point relative to the headset located in STAGE, and the
//! countdown's whole seconds (at least 1).
//!
//! The Rust build's helpers (cvr-render overlay_fbt_helpers) draw with these; the unit tests below cover each.

use crate::overlay_projection::rotate;
use crate::projection::{map_ndc_x, map_ndc_y, EyeWindow};

/// WithAlpha.
pub fn with_alpha(c: u32, k: f32) -> u32 {
    let a = (((c >> 24) & 0xFF) as f32 * k) as i32;
    (c & !0xFF00_0000) | ((a.clamp(0, 255) as u32) << 24)
}
/// Whiter.
pub fn whiter(c: u32, k: f32) -> u32 {
    let mix = |shift: u32| {
        let v = ((c >> shift) & 0xFF) as i32;
        ((v + ((255 - v) as f32 * k) as i32) as u32) << shift
    };
    mix(0) | mix(8) | mix(16) | (c & 0xFF00_0000)
}
/// Project's eye space: `eye` 0 left, 1 right.
pub fn eye_local(cam: [f32; 3], quat: [f32; 4], right: [f32; 3], half_ipd: f32, stereo: bool, eye: i32, world: [f32; 3]) -> [f32; 3] {
    let sign = if stereo { if eye == 0 { -1.0f32 } else { 1.0 } } else { 0.0 };
    let d = [
        world[0] - (cam[0] + right[0] * half_ipd * sign),
        world[1] - (cam[1] + right[1] * half_ipd * sign),
        world[2] - (cam[2] + right[2] * half_ipd * sign),
    ];
    rotate(d, [-quat[0], -quat[1], -quat[2], quat[3]])
}

/// Project's screen placement of the projected point `s` (eye-local pixels).
#[allow(clippy::too_many_arguments)]
pub fn eye_screen(stereo: bool, asym: bool, eye: i32, main_eye: i32, windows: &[EyeWindow; 2], size: [f32; 2], s: [f32; 2]) -> [f32; 2] {
    let mut s = s;
    if stereo && asym && eye != main_eye && (0..2).contains(&eye) && (0..2).contains(&main_eye) {
        let (from, to) = (&windows[main_eye as usize], &windows[eye as usize]);
        let nx = map_ndc_x((s[0] / size[0]) * 2.0 - 1.0, from, to);
        let ny = map_ndc_y(1.0 - 2.0 * s[1] / size[1], from, to);
        s = [(nx * 0.5 + 0.5) * size[0], (0.5 - ny * 0.5) * size[1]];
    }
    [s[0] + eye as f32 * size[0], s[1]]
}

/// The camera's pitch, degrees (up positive).
pub fn pitch_deg(quat: [f32; 4]) -> f32 {
    rotate([0.0, 1.0, 0.0], quat)[2].min(1.0).max(-1.0).asin() * 57.29578
}
/// Step 1's horizon line: its two ends, left then right.
pub fn horizon(cam: [f32; 3], quat: [f32; 4]) -> Option<[[f32; 3]; 2]> {
    let looking = rotate([0.0, 1.0, 0.0], quat);
    let fl = looking[0].hypot(looking[1]);
    if fl <= 0.05 || fl.is_nan() {
        return None;
    }
    let f = [looking[0] / fl, looking[1] / fl];
    let right = [f[1], -f[0]];
    let end = |side: f32| [cam[0] + f[0] * 3.0 + right[0] * side, cam[1] + f[1] * 3.0 + right[1] * side, cam[2]];
    Some([end(-2.0), end(2.0)])
}
/// A head-relative OpenXR point on the located camera.
pub fn head_to_world(cam: [f32; 3], quat: [f32; 4], p: [f32; 3]) -> [f32; 3] {
    let w = rotate([p[0], -p[2], p[1]], quat);
    [cam[0] + w[0], cam[1] + w[1], cam[2] + w[2]]
}
/// A STAGE point relative to the headset in STAGE (orientation x, y, z, w; position), OpenXR axes.
pub fn stage_to_head(p: [f32; 3], orientation: [f32; 4], position: [f32; 3]) -> [f32; 3] {
    let q = orientation;
    rotate([p[0] - position[0], p[1] - position[1], p[2] - position[2]], [-q[0], -q[1], -q[2], q[3]])
}
/// The countdown's seconds shown.
pub fn countdown_seconds(countdown: f32) -> i32 {
    let seconds = countdown.ceil() as i32;
    if seconds > 0 { seconds } else { 1 }
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(with_alpha(0xF0_10_20_30, 0.5), 0x78_10_20_30);
        assert_eq!(with_alpha(0xFF_10_20_30, 2.0), 0xFF_10_20_30);
        assert_eq!(with_alpha(0xFF_10_20_30, -1.0), 0x00_10_20_30);
        assert_eq!(whiter(0x80_00_7F_FF, 0.5), 0x80_7F_BF_FF);
        assert_eq!(whiter(0x80_10_20_30, 0.0), 0x80_10_20_30);
        assert_eq!(whiter(0x80_10_20_30, 1.0), 0x80_FF_FF_FF);
    }
    #[test]
    fn eyes() {
        let identity = [0.0, 0.0, 0.0, 1.0];
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(near(eye_local([1.0, 2.0, 3.0], identity, [1.0, 0.0, 0.0], 0.03, true, 0, [1.0, 3.0, 3.0]), [0.03, 1.0, 0.0]));
        assert!(near(eye_local([1.0, 2.0, 3.0], identity, [1.0, 0.0, 0.0], 0.03, true, 1, [1.0, 3.0, 3.0]), [-0.03, 1.0, 0.0]));
        assert_eq!(eye_local([1.0, 2.0, 3.0], identity, [1.0, 0.0, 0.0], 0.03, false, 1, [1.0, 3.0, 3.0]), [0.0, 1.0, 0.0]);
        // A camera turned 90 degrees about z sees a point along +x straight ahead (+y).
        let s = core::f32::consts::FRAC_1_SQRT_2;
        let l = eye_local([0.0; 3], [0.0, 0.0, s, s], [0.0, 1.0, 0.0], 0.0, false, 0, [0.0, 1.0, 0.0]);
        assert!((l[0] - 1.0).abs() < 1e-6 && l[1].abs() < 1e-6, "{l:?}");
    }
    #[test]
    fn screens() {
        let same = EyeWindow { l: -1.0, r: 1.0, d: -1.0, u: 1.0 };
        let shifted = EyeWindow { l: -1.0, r: 0.5, d: -1.0, u: 1.0 };
        let windows = [same, shifted];
        // The main eye, mono or no asymmetric projection: only the eye's half.
        assert_eq!(eye_screen(true, true, 0, 0, &windows, [100.0, 50.0], [25.0, 10.0]), [25.0, 10.0]);
        assert_eq!(eye_screen(true, false, 1, 0, &windows, [100.0, 50.0], [25.0, 10.0]), [125.0, 10.0]);
        assert_eq!(eye_screen(false, true, 1, 0, &windows, [100.0, 50.0], [25.0, 10.0]), [125.0, 10.0]);
        // The other eye: NDC 0 in the main window lies right of the narrower window's centre.
        let p = eye_screen(true, true, 1, 0, &windows, [100.0, 50.0], [50.0, 25.0]);
        let expected_nx = map_ndc_x(0.0, &same, &shifted);
        assert!((p[0] - (100.0 + (expected_nx * 0.5 + 0.5) * 100.0)).abs() < 1e-4 && (p[1] - 25.0).abs() < 1e-4, "{p:?}");
        assert!(p[0] > 150.0);
    }
    #[test]
    fn geometry() {
        let identity = [0.0, 0.0, 0.0, 1.0];
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        assert_eq!(pitch_deg(identity), 0.0);
        // Turned 45 degrees down about x: the pitch -45, the horizon still ahead along +y.
        let (s, c) = (-(core::f32::consts::FRAC_PI_8).sin(), (core::f32::consts::FRAC_PI_8).cos());
        let down = [s, 0.0, 0.0, c];
        assert!((pitch_deg(down) + 45.0).abs() < 1e-3, "{}", pitch_deg(down));
        let h = horizon([1.0, 2.0, 3.0], down).unwrap();
        assert!(near(h[0], [-1.0, 5.0, 3.0]) && near(h[1], [3.0, 5.0, 3.0]), "{h:?}");
        // Straight down: no heading, no line.
        let h = core::f32::consts::FRAC_1_SQRT_2;
        assert_eq!(horizon([0.0; 3], [-h, 0.0, 0.0, h]), None);
        // OpenXR forward (-z) is the camera's forward (+y), OpenXR up (+y) its up (+z).
        assert_eq!(head_to_world([1.0, 2.0, 3.0], identity, [0.5, 2.0, -1.0]), [1.5, 3.0, 5.0]);
        // A headset turned 90 degrees about OpenXR's up (y): a STAGE point to its left is straight ahead (-z).
        let yaw = [0.0, h, 0.0, h];
        assert!(near(stage_to_head([-1.0, 1.0, 0.0], yaw, [0.0, 1.0, 0.0]), [0.0, 0.0, -1.0]), "{:?}", stage_to_head([-1.0, 1.0, 0.0], yaw, [0.0, 1.0, 0.0]));
        assert_eq!((countdown_seconds(2.1), countdown_seconds(3.0), countdown_seconds(0.0), countdown_seconds(-0.5)), (3, 3, 1, 1));
    }
}
