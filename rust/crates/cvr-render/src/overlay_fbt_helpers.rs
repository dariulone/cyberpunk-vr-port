//! The full-body tracking calibration helpers (src/Fbt/Game/Helpers.cpp, `DrawFbtCalibrationHelpers` and the neon
//! drawing under it), drawn into both eyes while calibration mode is on, on the background list -- no depth test, so
//! they stay visible through V's body:
//!   step 1 (head): a horizon line 3 m ahead at eye height, level in the world: yellow, green once the gaze is level;
//!   step 2 (body): a green neon light at each tracker and controller, a red neon line from each tracker to the bone it
//!     drives (V held in the rig's reference pose: the entity transform on the bone's reference model position), and
//!     an arrow ahead pointing down until the player looks 30 degrees down;
//!   step 3 (the T-pose): the latin cross standing in front of the player, a ring on its top and at its arms' ends, the
//!     head and the two controllers on its plane -- red off their marks, yellow near, green on them -- and the
//!     countdown's seconds over the top;
//! and under them the step's instruction, then in red why the last pull was not taken. The neon motif: three soft
//! glows widening around a shape in its colour, the shape, a whiter core; the texts on black plates bordered so.
//!
//! The Rust build draws them here (the colours and the eye arithmetic are cvr-camera's fbt_helpers); the C++ half
//! keeps the calibration, the trackers, the typeface and the screen projection, and lends them through the attached
//! runtime.

use crate::imgui::{background_draw_list_handle, DrawList, Font, Io, Vec2, DRAW_CLOSED};
use core::ffi::{c_char, c_void, CStr};
use cvr_camera::fbt_helpers::{countdown_seconds, eye_local, eye_screen, head_to_world, horizon, pitch_deg, stage_to_head, whiter, with_alpha};
use cvr_camera::overlay_projection::rotate;
use cvr_camera::projection::EyeWindow;
use std::ffi::CString;

/// cvr::fbt::game::CrossView.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CrossView {
    pub(crate) base: [f32; 3],
    pub(crate) right: [f32; 3],
    pub(crate) tip_y: f32,
    pub(crate) bar_y: f32,
    pub(crate) half_span: f32,
    pub(crate) dot: [[f32; 3]; 3],
    pub(crate) state: [i32; 3],
    pub(crate) countdown: f32,
}
const _: () = assert!(core::mem::size_of::<CrossView>() == 88);
impl CrossView {
    /// `CrossView{}`: nothing tracked, no countdown.
    pub(crate) const EMPTY: CrossView =
        CrossView { base: [0.0; 3], right: [0.0; 3], tip_y: 0.0, bar_y: 0.0, half_span: 0.0, dot: [[0.0; 3]; 3], state: [-1; 3], countdown: -1.0 };
}
impl Default for CrossView {
    fn default() -> Self {
        CrossView::EMPTY
    }
}
/// XrPosef.
#[repr(C)]
#[derive(Default)]
pub struct Posef {
    orientation: [f32; 4],
    position: [f32; 3],
}

/// What the helpers read of the C++ half.
#[repr(C)]
pub struct FbtHelpersRuntime {
    /// cvr::fbt::game::Calibrating.
    calibrating: extern "C" fn() -> bool,
    /// VRIK_ReadFbtReference: the hips' and the feet's (left, right) reference model positions.
    reference: extern "C" fn(*mut [[f32; 3]; 3]) -> bool,
    /// engine_stereo::Enabled and OnePicture.
    engine_stereo: extern "C" fn() -> bool,
    one_picture: extern "C" fn() -> bool,
    /// projection::Active() && OpenXRManager::GetAsymmetricEyeWindows.
    asym_windows: extern "C" fn(*mut [EyeWindow; 2]) -> bool,
    /// ProjectHeadSpacePointToScreen.
    project: extern "C" fn(f32, f32, f32, f32, f32, *mut [f32; 2]) -> bool,
    /// CalibrationStep: 0 off, 1 head, 2 body, 3 the T-pose.
    step: extern "C" fn() -> i32,
    /// StepText(slot), or RefusalText() for -1.
    text: extern "C" fn(i32) -> *const c_char,
    /// trackers::Read (0..2: waist, left foot, right foot) or ReadHand (3, 4): whether tracked; the head-relative
    /// position (OpenXR axes) written either way.
    device: extern "C" fn(i32, *mut [f32; 3]) -> bool,
    /// ReadCross && trackers::ReadHeadStage.
    cross: extern "C" fn(*mut CrossView, *mut Posef) -> bool,
    /// The texts' typeface: the game's, or ImGui's own without it.
    font: extern "C" fn() -> *mut c_void,
    /// kCalibrationLevelDeg: step 1's gaze within this of level.
    level_deg: f32,
}

#[allow(non_upper_case_globals)]
extern "C" {
    fn CyberpunkVRPort_HalfIpd() -> f32;
    static g_lastLocatePosFP: [i32; 3];
    static g_lastLocateQuat: [f32; 4];
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_PlayerEntityValid: i32;
    static CyberpunkVR_PlayerEntityPos: [f32; 3];
    static CyberpunkVR_PlayerEntityQuat: [f32; 4];
}
/// A C++ global, read as the .cpp reads it.
///
/// # Safety
/// `value` is a live global.
unsafe fn get<T: Copy>(value: *const T) -> T {
    unsafe { core::ptr::read_volatile(value) }
}

/// IM_COL32.
const fn col32(r: u32, g: u32, b: u32, a: u32) -> u32 {
    (a << 24) | (b << 16) | (g << 8) | r
}
const BALL_RADIUS_M: f32 = 0.04; // a Vive Tracker is about 8 cm across
const LINE_THICKNESS_PX: f32 = 6.0;
const LINE_COLOR: u32 = col32(255, 60, 60, 240);
// The arrow: head-locked, ARROW_DISTANCE_M ahead, hidden once the view is ARROW_HIDE_DEG below the horizon.
const ARROW_DISTANCE_M: f32 = 1.5;
const ARROW_HIDE_DEG: f32 = 30.0;
const ARROW_COLOR: u32 = col32(255, 220, 0, 235);
// Cyan for the cross (the VR menu's chrome), green on a mark reached.
const NEON_CYAN: u32 = col32(110, 220, 255, 240);
const NEON_GREEN: u32 = col32(60, 230, 90, 240);
const PLATE: u32 = col32(10, 10, 10, 200);
const PLATE_ROUNDING: f32 = 8.0;
// The lights: green (the trackers, a mark reached), yellow (near), red (off).
const LIGHT_GREEN: u32 = NEON_GREEN;
const LIGHT_YELLOW: u32 = ARROW_COLOR;
const LIGHT_RED: u32 = col32(255, 80, 70, 240);
const REFUSAL_COLOR: u32 = col32(255, 80, 70, 255);

// The glow (under whatever is drawn next), then the tube: stroke(colour, thickness) draws the shape once.
fn neon_glow(color: u32, core: f32, stroke: &mut impl FnMut(u32, f32)) {
    stroke(with_alpha(color, 0.10), core + 14.0);
    stroke(with_alpha(color, 0.18), core + 8.0);
    stroke(with_alpha(color, 0.35), core + 4.0);
}
fn neon_tube(color: u32, core: f32, stroke: &mut impl FnMut(u32, f32)) {
    stroke(color, core);
    stroke(whiter(color, 0.6), (core * 0.4).max(1.0));
}
fn neon(color: u32, core: f32, mut stroke: impl FnMut(u32, f32)) {
    neon_glow(color, core, &mut stroke);
    neon_tube(color, core, &mut stroke);
}
/// A black plate with a thin neon border in the colour of its text; the glow stays outside the plate.
fn neon_plate(dl: DrawList, min: Vec2, max: Vec2, color: u32) {
    let mut rect = |c: u32, t: f32| dl.rect(min, max, c, PLATE_ROUNDING, 0, t);
    neon_glow(color, 1.5, &mut rect);
    dl.rect_filled_rounded(min, max, PLATE, PLATE_ROUNDING, 0);
    neon_tube(color, 1.5, &mut rect);
}
fn neon_line(dl: DrawList, a: Vec2, b: Vec2, color: u32, core: f32) {
    neon(color, core, |c, t| dl.line(a, b, c, t));
}
fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// One frame's view: the located camera, its right axis, the eyes.
struct EyeView {
    cam: [f32; 3],
    quat: [f32; 4],
    right: [f32; 3],
    half_ipd: f32,
    eye_size: Vec2,
    stereo: bool,
    asym: bool,
    main_eye: i32,
    windows: [EyeWindow; 2],
}
/// The drawing of one frame.
struct Painter<'a> {
    r: &'a FbtHelpersRuntime,
    v: EyeView,
    dl: DrawList,
    font: Font,
}
impl Painter<'_> {
    /// A world point as seen from one eye (0 the left half of the frame, 1 the right), in backbuffer pixels.
    fn project(&self, eye: i32, world: [f32; 3]) -> Option<Vec2> {
        let v = &self.v;
        let l = eye_local(v.cam, v.quat, v.right, v.half_ipd, v.stereo, eye, world);
        let mut s = [0.0f32; 2];
        if !(self.r.project)(l[0], l[2], -l[1], v.eye_size[0], v.eye_size[1], &mut s) {
            return None;
        }
        Some(eye_screen(v.stereo, v.asym, eye, v.main_eye, &v.windows, v.eye_size, s))
    }
    /// A point and the point a world radius to its right, as seen from the eye: the centre and the radius in pixels,
    /// when it is a sensible size.
    fn disc(&self, eye: i32, world: [f32; 3], radius: f32) -> Option<(Vec2, f32)> {
        let side = [world[0] + self.v.right[0] * radius, world[1] + self.v.right[1] * radius, world[2] + self.v.right[2] * radius];
        let c = self.project(eye, world)?;
        let edge = self.project(eye, side)?;
        let r = (edge[0] - c[0]).hypot(edge[1] - c[1]);
        (r > 1.0 && r <= 2000.0).then_some((c, r))
    }
    /// A neon light the size of the device at that distance: a soft halo fading out, a thin ring, a bright core,
    /// whiter in its centre.
    fn ball(&self, eye: i32, world: [f32; 3], color: u32, radius: f32) {
        let Some((c, r)) = self.disc(eye, world, radius) else { return };
        let dl = self.dl;
        dl.circle_filled_segments(c, r * 2.2, with_alpha(color, 0.08), 40);
        dl.circle_filled_segments(c, r * 1.6, with_alpha(color, 0.16), 40);
        dl.circle_filled_segments(c, r * 1.2, with_alpha(color, 0.30), 40);
        dl.circle_segments(c, r, color, 40, (r * 0.12).max(1.5));
        dl.circle_filled_segments(c, r * 0.70, color, 32);
        dl.circle_filled_segments(c, r * 0.38, whiter(color, 0.7), 24);
    }
    /// A ring of a world radius around a world point, facing the eye.
    fn ring(&self, eye: i32, world: [f32; 3], radius: f32, color: u32) {
        let Some((c, r)) = self.disc(eye, world, radius) else { return };
        let dl = self.dl;
        neon(color, 3.0, |col, t| dl.circle_segments(c, r, col, 32, t));
    }
    /// A text centred on a world point, on a dark plate.
    fn world_text(&self, eye: i32, world: [f32; 3], text: &CStr, size: f32, color: u32) {
        if text.is_empty() {
            return;
        }
        let Some(c) = self.project(eye, world) else { return };
        let extent = self.font.calc_text_size_a(size, f32::MAX, 0.0, text);
        let p = [c[0] - extent[0] * 0.5, c[1] - extent[1] * 0.5];
        neon_plate(self.dl, [p[0] - 12.0, p[1] - 6.0], [p[0] + extent[0] + 12.0, p[1] + extent[1] + 6.0], color);
        self.dl.text(self.font, size, p, color, text.as_ptr(), core::ptr::null(), 0.0);
    }
    /// A text on a dark plate, centred under the gaze: centred on that height or, given `top` (screen pixels, >= 0),
    /// its plate starting there. Returns the plate's bottom, for the next text to go under it (-1 when nothing was
    /// drawn).
    fn step_text(&self, eye: i32, text: &CStr, color: u32, top: f32) -> f32 {
        const PAD_X: f32 = 14.0;
        const PAD_Y: f32 = 10.0;
        if text.is_empty() {
            return -1.0;
        }
        // About 18 degrees below the gaze: clear of the horizon line and the arrow.
        let w = add3(rotate([0.0, ARROW_DISTANCE_M, -0.50], self.v.quat), self.v.cam);
        let Some(c) = self.project(eye, w) else { return -1.0 };
        let v = &self.v;
        let size = (v.eye_size[1] * 0.03).max(22.0);
        let extent = self.font.calc_text_size_a(size, f32::MAX, v.eye_size[0] * 0.6, text);
        let p = [c[0] - extent[0] * 0.5, if top >= 0.0 { top + PAD_Y } else { c[1] - extent[1] * 0.5 }];
        neon_plate(self.dl, [p[0] - PAD_X, p[1] - PAD_Y], [p[0] + extent[0] + PAD_X, p[1] + extent[1] + PAD_Y], color);
        self.dl.text(self.font, size, p, color, text.as_ptr(), core::ptr::null(), v.eye_size[0] * 0.6);
        p[1] + extent[1] + PAD_Y
    }
    /// The arrow down: one neon outline round the whole arrow (shaft and head), the dark fill in its two convex parts.
    fn arrow_down(&self, eye: i32) {
        let d = ARROW_DISTANCE_M;
        let outline: [[f32; 3]; 7] =
            [[-0.05, d, 0.30], [0.05, d, 0.30], [0.05, d, 0.04], [0.17, d, 0.04], [0.0, d, -0.22], [-0.17, d, 0.04], [-0.05, d, 0.04]];
        let mut p = [[0.0f32; 2]; 7];
        for (point, corner) in p.iter_mut().zip(outline) {
            let Some(s) = self.project(eye, add3(rotate(corner, self.v.quat), self.v.cam)) else { return };
            *point = s;
        }
        let dl = self.dl;
        let mut stroke = |c: u32, t: f32| dl.polyline(&p, c, DRAW_CLOSED, t);
        neon_glow(ARROW_COLOR, 2.5, &mut stroke);
        dl.convex_poly_filled(&[p[0], p[1], p[2], p[6]], PLATE);
        dl.convex_poly_filled(&[p[3], p[4], p[5]], PLATE);
        neon_tube(ARROW_COLOR, 2.5, &mut stroke);
    }
    /// A tracker's line to its bone, and a ring at the bone's end.
    fn line(&self, eye: i32, a: [f32; 3], b: [f32; 3]) {
        let (Some(pa), Some(pb)) = (self.project(eye, a), self.project(eye, b)) else { return };
        neon_line(self.dl, pa, pb, LINE_COLOR, LINE_THICKNESS_PX * 0.6);
        let dl = self.dl;
        neon(LINE_COLOR, 2.0, |c, t| dl.circle_segments(pb, LINE_THICKNESS_PX * 1.4, c, 20, t));
    }
}

/// A text the C++ half lends, copied.
fn text(r: &FbtHelpersRuntime, slot: i32) -> CString {
    let p = (r.text)(slot);
    if p.is_null() {
        return CString::default();
    }
    // SAFETY: a C string the C++ half holds until its next call.
    unsafe { CStr::from_ptr(p) }.to_owned()
}

/// DrawFbtCalibrationHelpers.
#[no_mangle]
pub extern "C" fn cvr_overlay_fbt_helpers() {
    let r = &table::RT;
    if !(r.calibrating)() {
        return;
    }
    let mut reference = [[0.0f32; 3]; 3];
    // SAFETY: the C++ half's player globals.
    if !(r.reference)(&mut reference) || unsafe { get(&raw const CyberpunkVR_PlayerEntityValid) } == 0 {
        return;
    }
    let engine_stereo = (r.engine_stereo)();
    if engine_stereo && (r.one_picture)() {
        return;
    }
    let backbuffer = Io::get().display_size();
    let mut v = EyeView {
        cam: [0.0; 3],
        quat: [0.0, 0.0, 0.0, 1.0],
        right: [0.0; 3],
        half_ipd: 0.0325,
        eye_size: [if engine_stereo { backbuffer[0] * 0.5 } else { backbuffer[0] }, backbuffer[1]],
        stereo: engine_stereo,
        asym: false,
        main_eye: 0,
        windows: [EyeWindow::default(); 2],
    };
    if v.eye_size[0] <= 1.0 || v.eye_size[1] <= 1.0 {
        return;
    }
    // SAFETY: the located camera, published by the XR thread.
    let (position, quat) = unsafe { (get(&raw const g_lastLocatePosFP), get(&raw const g_lastLocateQuat)) };
    v.cam = position.map(|p| p as f32 / 131072.0);
    v.quat = quat;
    if v.cam[0] * v.cam[0] + v.cam[1] * v.cam[1] + v.cam[2] * v.cam[2] < 1.0 {
        return;
    }
    v.right = rotate([1.0, 0.0, 0.0], v.quat);
    // SAFETY: the C++ half's IPD and eye switch.
    let half = unsafe { CyberpunkVRPort_HalfIpd() };
    v.half_ipd = if half > 0.0001 { half } else { 0.0325 };
    v.main_eye = if unsafe { get(&raw const CyberpunkVR_MainIsRightEye) } != 0 { 1 } else { 0 };
    v.asym = (r.asym_windows)(&mut v.windows);

    // SAFETY: the C++ half's player globals.
    let (entity_pos, entity_quat) = unsafe { (get(&raw const CyberpunkVR_PlayerEntityPos), get(&raw const CyberpunkVR_PlayerEntityQuat)) };
    // A head-relative OpenXR pose on the located camera.
    let (cam, cam_quat) = (v.cam, v.quat);
    let to_world = |p: [f32; 3]| head_to_world(cam, cam_quat, p);
    let bone_world = |model: [f32; 3]| add3(rotate(model, entity_quat), entity_pos);
    let step = (r.step)();
    let step_text = text(r, step);
    let refusal = text(r, -1); // a pull not taken: why, under the instruction
    let mut tracker = [[0.0f32; 3]; 3];
    let mut bone = [[0.0f32; 3]; 3];
    let mut have = [false; 3];
    for role in 0..3 {
        let mut p = [0.0f32; 3];
        have[role] = (r.device)(role as i32, &mut p);
        tracker[role] = to_world(p);
        bone[role] = bone_world(reference[role]);
    }
    let mut hand = [[0.0f32; 3]; 2];
    let mut have_hand = [false; 2];
    for h in 0..2 {
        let mut p = [0.0f32; 3];
        have_hand[h] = (r.device)(3 + h as i32, &mut p);
        hand[h] = to_world(p);
    }
    // Step 3's cross: STAGE points -> head-relative (the headset in STAGE, located with the devices) -> the world.
    let mut cross = CrossView::default();
    let mut head = Posef::default();
    let have_cross = step == 3 && (r.cross)(&mut cross, &mut head);
    let stage_to_world = |p: [f32; 3]| to_world(stage_to_head(p, head.orientation, head.position));
    let on_cross = |lateral: f32, y: f32| {
        stage_to_world([cross.base[0] + cross.right[0] * lateral, y, cross.base[2] + cross.right[2] * lateral])
    };

    // Step 2: the arrow down until the player looks down; step 1: the horizon, green once the gaze is level.
    let pitch_deg = pitch_deg(cam_quat);
    let horizon = horizon(cam, cam_quat);
    let level = pitch_deg.abs() < r.level_deg;
    let arrow_down = step == 2 && pitch_deg > -ARROW_HIDE_DEG;

    // Background list, like the crosshair: the frame is split into the two eyes after this pass.
    let Some(dl) = background_draw_list_handle() else { return };
    let painter = Painter { r, v, dl, font: Font::from_raw((r.font)()) };
    let v = &painter.v;
    for eye in 0..(if engine_stereo { 2 } else { 1 }) {
        let left = eye as f32 * v.eye_size[0];
        dl.push_clip_rect([left, 0.0], [left + v.eye_size[0], v.eye_size[1]], true);
        if let (1, Some(horizon)) = (step, horizon) {
            if let (Some(a), Some(b)) = (painter.project(eye, horizon[0]), painter.project(eye, horizon[1])) {
                neon_line(dl, a, b, if level { NEON_GREEN } else { ARROW_COLOR }, 5.0);
            }
        }
        if step == 2 {
            for role in (0..3).filter(|&role| have[role]) {
                painter.line(eye, tracker[role], bone[role]);
            }
            for role in (0..3).filter(|&role| have[role]) {
                painter.ball(eye, tracker[role], LIGHT_GREEN, BALL_RADIUS_M);
            }
            for h in (0..2).filter(|&h| have_hand[h]) {
                painter.ball(eye, hand[h], LIGHT_GREEN, BALL_RADIUS_M);
            }
        }
        if arrow_down {
            painter.arrow_down(eye);
        }
        if have_cross {
            // The latin cross: the post from the floor to the top, the beam at the shoulders.
            let foot = on_cross(0.0, 0.0);
            let top = on_cross(0.0, cross.tip_y);
            let beam_l = on_cross(-cross.half_span, cross.bar_y);
            let beam_r = on_cross(cross.half_span, cross.bar_y);
            for (a, b) in [(foot, top), (beam_l, beam_r)] {
                if let (Some(a), Some(b)) = (painter.project(eye, a), painter.project(eye, b)) {
                    neon_line(dl, a, b, NEON_CYAN, 6.0);
                }
            }
            // The marks, then the dots: green on their mark, yellow near, red off.
            for (mark, state) in [top, beam_l, beam_r].into_iter().zip(cross.state) {
                painter.ring(eye, mark, 0.09, if state == 2 { NEON_GREEN } else { NEON_CYAN });
            }
            for (dot, state) in cross.dot.into_iter().zip(cross.state) {
                if state < 0 {
                    continue;
                }
                let color = match state {
                    2 => LIGHT_GREEN,
                    1 => LIGHT_YELLOW,
                    _ => LIGHT_RED,
                };
                painter.ball(eye, stage_to_world(dot), color, 0.06);
            }
            if cross.countdown >= 0.0 {
                let over = on_cross(0.0, cross.tip_y + 0.30);
                let n = CString::new(countdown_seconds(cross.countdown).to_string()).unwrap_or_default();
                painter.world_text(eye, over, &n, (v.eye_size[1] * 0.07).max(48.0), NEON_GREEN);
            }
        }
        // Why a pull was not taken: in red, right under the instruction (never over it, however many lines it takes).
        let below = painter.step_text(eye, &step_text, ARROW_COLOR, -1.0);
        painter.step_text(eye, &refusal, REFUSAL_COLOR, if below >= 0.0 { below + 12.0 } else { -1.0 });
        dl.pop_clip_rect();
    }
}

// ---- the helpers' table (formerly src/Fbt/Game/Helpers.cpp's): the reference pose from cvr-rig, engine stereo, the eye
// windows, the overlay's projection and the trackers from cvr-xr, the calibration's step, texts and cross from
// fbt_calibration.rs; the game's typeface stays the C++ half's service
mod table {
    use super::{CrossView, FbtHelpersRuntime, Posef};
    use core::ffi::{c_char, c_void};
    use cvr_camera::overlay_projection::project_head;
    use cvr_camera::projection::EyeWindow;

    use crate::fbt_calibration as calibration;
    unsafe extern "C" {
        /// The game's typeface for the texts, or ImGui's own (src/Fbt/Game/Helpers.cpp).
        safe fn cvr_cpp_fbt_font() -> *mut c_void;
    }
    const ROLES: i32 = 3;

    extern "C" fn calibrating() -> bool {
        calibration::cvr_cpp_fbt_calibrating()
    }
    extern "C" fn reference(out: *mut [[f32; 3]; 3]) -> bool {
        let Some(r) = calibration::read_reference() else { return false };
        // SAFETY: the caller's three positions.
        unsafe { *out = [r.hips_pos, r.foot_pos[0], r.foot_pos[1]] };
        true
    }
    extern "C" fn engine_stereo() -> bool {
        crate::engine_stereo::cvr_engine_stereo_enabled()
    }
    /// engine_stereo::OnePicture: on, and not rendering both views or a loading screen up.
    extern "C" fn one_picture() -> bool {
        crate::engine_stereo::cvr_engine_stereo_enabled()
            && (!crate::engine_stereo::cvr_engine_stereo_rendering() || crate::engine_stereo::cvr_engine_stereo_loading_shown())
    }
    extern "C" fn asym_windows(out: *mut [EyeWindow; 2]) -> bool {
        // GetAsymmetricEyeWindows' default: fitted to the target
        // SAFETY: the caller's two windows.
        crate::eye_windows::projection_active() && unsafe { crate::eye_windows::cvr_xr_asym_eye_windows(out.cast(), true) }
    }
    /// ProjectHeadSpacePointToScreen.
    extern "C" fn project(x: f32, y: f32, z: f32, width: f32, height: f32, out: *mut [f32; 2]) -> bool {
        if out.is_null() || width <= 1.0 || height <= 1.0 {
            return false;
        }
        let Some(window) = crate::eye_windows::overlay_window(width, height) else { return false };
        match project_head([x, y, z], width, height, &window) {
            Some(screen) => {
                // SAFETY: the caller's output.
                unsafe { *out = screen };
                true
            }
            None => false,
        }
    }
    extern "C" fn step() -> i32 {
        calibration::cvr_cpp_fbt_calibration_step()
    }
    extern "C" fn text(slot: i32) -> *const c_char {
        calibration::text(slot)
    }
    /// The device's head-relative position, written whether or not it is tracked.
    extern "C" fn device(index: i32, out: *mut [f32; 3]) -> bool {
        // SAFETY: plain data, all zero the C++ value-initialised pose.
        let mut p: cvr_xr::trackers::Pose = unsafe { core::mem::zeroed() };
        // SAFETY: a local to fill.
        let tracked = unsafe {
            if index < ROLES {
                cvr_xr::trackers::cvr_trackers_read(index, &mut p)
            } else {
                cvr_xr::trackers::cvr_trackers_read_hand(index - ROLES, &mut p)
            }
        };
        // SAFETY: the caller's position.
        unsafe { *out = [p.pos.x, p.pos.y, p.pos.z] };
        tracked
    }
    extern "C" fn cross(view: *mut CrossView, head: *mut Posef) -> bool {
        // SAFETY: the caller's cross and head.
        calibration::read_cross(view) && unsafe { cvr_xr::trackers::cvr_trackers_read_head_stage(head.cast()) }
    }
    extern "C" fn font() -> *mut c_void {
        cvr_cpp_fbt_font()
    }
    pub(super) static RT: FbtHelpersRuntime = FbtHelpersRuntime {
        calibrating,
        reference,
        engine_stereo,
        one_picture,
        asym_windows,
        project,
        step,
        text,
        device,
        cross,
        font,
        level_deg: 8.0,
    };
}
