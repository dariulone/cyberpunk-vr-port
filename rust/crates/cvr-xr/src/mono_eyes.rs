//! The eyes a presented mono frame is submitted with (formerly inline in OpenXRManager::OnPresent): for each eye the
//! pose and FOV the picture was actually rendered with, so the compositor reprojects what is in the pixels.
//!
//! The frame's own slot (its per-eye poses and FOVs, located together) gives the eye offsets and the FOVs; the centre
//! is the pose the engine was read back rendering with (or the pose bound to the frame, or the interval's ring
//! entry), put back into the tracking space; the second eye takes its own read-back when there is one. The FOV is
//! the asymmetric window when that projection is on, else the forced or de-canted, rendered-frustum FOV
//! (ApplyForcedProjectionFov).
#![cfg_attr(test, allow(dead_code, unused_imports))]
#![allow(non_upper_case_globals)]
use cvr_body::cmath::{atan, tan};
use cvr_body::xr::{conjugate_quat, rotate_vector, Quat, Vec3};
use cvr_camera::projection::{runtime_fov_correction, to_fov, windows, Fov};
use openxr_sys as xr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::hands::Pose;

const PI_C: f32 = 3.1415926535;

/// What the forced FOV decided, for its log line.
pub struct SubmitFov {
    pub fov: Fov,
    /// The rendered frustum was described (the game's FOV is known): log it.
    pub described: bool,
    pub yaw_enabled: bool,
}

/// ApplyForcedProjectionFov. Without xr_force_fov (`forced` <= 1 or >= 170): the eye's de-canted frustum, re-described
/// as the engine rendered it -- the game's horizontal FOV about the eye's horizontal centre, and symmetric vertically
/// about the camera axis once the rendered vertical is known (the runtime's own centre before that). With it: the
/// historical forced symmetric projection, its vertical from the render aspect (the frustum's own before one is known).
pub fn forced_projection_fov(source: &Fov, pair: Option<&[Fov; 2]>, eye: usize, width: f32, height: f32, forced: f32, game_h: f32, game_v: f32) -> SubmitFov {
    if forced <= 1.0 || forced >= 170.0 {
        let Some(pair) = pair.filter(|_| eye <= 1) else {
            return SubmitFov { fov: *source, described: false, yaw_enabled: false };
        };
        let corr = runtime_fov_correction(&pair[0], &pair[1]);
        let mut fov = corr.eye[eye];
        if game_h > 1.0 {
            let want_half_h = (game_h * PI_C / 180.0) * 0.5;
            let h_center = (fov.angle_right + fov.angle_left) * 0.5;
            let real_half_v = (fov.angle_up - fov.angle_down) * 0.5;
            let v_center = (fov.angle_up + fov.angle_down) * 0.5;
            let have_v = game_v > 1.0 && game_v < 179.0;
            let want_half_v = if have_v { (game_v * PI_C / 180.0) * 0.5 } else { real_half_v };
            fov.angle_left = h_center - want_half_h;
            fov.angle_right = h_center + want_half_h;
            let centre = if have_v { 0.0 } else { v_center };
            fov.angle_up = centre + want_half_v;
            fov.angle_down = centre - want_half_v;
            return SubmitFov { fov, described: true, yaw_enabled: corr.yaw_enabled };
        }
        return SubmitFov { fov, described: false, yaw_enabled: corr.yaw_enabled };
    }
    let mut aspect = if height > 1.0 { width / height } else { 0.0 };
    if !(aspect > 0.01 && aspect < 10.0) {
        let (tl, tr, td, tu) = (tan(-source.angle_left), tan(source.angle_right), tan(-source.angle_down), tan(source.angle_up));
        let v = tu + td;
        aspect = if v > 1.0e-5 { (tl + tr) / v } else { 1.0 };
    }
    let half_h = (forced * PI_C / 180.0) * 0.5;
    let half_v = atan(tan(half_h) / aspect);
    SubmitFov { fov: Fov { angle_left: -half_h, angle_right: half_h, angle_up: half_v, angle_down: -half_v }, described: false, yaw_enabled: false }
}

#[cfg(not(test))]
extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_BindPoseToImage: i32;
    static CyberpunkVR_VrcamOwnLabel: i32;
    static CyberpunkVR_PoseReadBack: i32;
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_PoseFrameLag: i32;
    static mut CyberpunkVR_DebugSlotHit: u64;
    static mut CyberpunkVR_DebugSlotMiss: u64;
    static mut CyberpunkVR_DebugSlotReused: u64;
    static mut CyberpunkVR_DebugPoseReadBack: u64;
    static mut CyberpunkVR_DebugPoseExact: u64;
    static mut CyberpunkVR_DebugPoseEstimated: u64;
    static mut CyberpunkVR_DebugPoseFromWrite: u64;
    static mut CyberpunkVR_DebugPoseFromSlot: u64;
    static mut CyberpunkVR_DebugVrcamLabelUsed: u64;
    fn GetRenderedCameraSeq() -> u32;
    fn GetRenderPoseSubmit() -> i32;
    fn GetForcedFov() -> f32;
    fn GetGameRenderFovDeg() -> f32;
    fn GetGameRenderVerticalFovDeg() -> f32;
    fn cvr_cpp_verbose_log() -> i32;
}

/// A C int knob, read once.
#[cfg(not(test))]
macro_rules! knob {
    ($g:ident) => {
        // SAFETY: a plain C int the panel or the launcher sets.
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!($g)) }
    };
}
/// One more on a diagnostic counter (CVR_DIAGNOSTIC: only while the runtime diagnostics are on).
#[cfg(not(test))]
macro_rules! count {
    ($g:ident) => {
        if knob!(CyberpunkVR_RuntimeDiagnostics) != 0 {
            // SAFETY: the C++ half's counter; only Present writes it.
            unsafe {
                let p = core::ptr::addr_of_mut!($g);
                core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1));
            }
        }
    };
}

/// The [SUBMITFOV] line's pacing: every 200th call when verbose, else one per 30 s while the diagnostics are on.
static SUBMITFOV_CALLS: AtomicU32 = AtomicU32::new(0);
static SUBMITFOV_LAST_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(not(test))]
fn submitfov_due() -> bool {
    // SAFETY: plain C getters.
    let verbose = unsafe { cvr_cpp_verbose_log() } != 0;
    if verbose && SUBMITFOV_CALLS.fetch_add(1, Ordering::Relaxed) % 200 != 0 {
        return false;
    }
    if knob!(CyberpunkVR_RuntimeDiagnostics) == 0 && !verbose {
        return false;
    }
    if verbose {
        return true;
    }
    // SAFETY: a plain clock read.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    let last = SUBMITFOV_LAST_MS.load(Ordering::Relaxed);
    if last != 0 && now - last < 30000 {
        return false;
    }
    SUBMITFOV_LAST_MS.store(now, Ordering::Relaxed);
    true
}

/// ApplyForcedProjectionFov with the live knobs, and its log line.
#[cfg(not(test))]
fn submit_fov(source: &Fov, pair: &[Fov; 2], eye: usize, width: f32, height: f32) -> Fov {
    // SAFETY: plain C getters.
    let (forced, game_h, game_v) = unsafe { (GetForcedFov(), GetGameRenderFovDeg(), GetGameRenderVerticalFovDeg()) };
    let s = forced_projection_fov(source, Some(pair), eye, width, height, forced, game_h, game_v);
    if s.described && submitfov_due() {
        let deg = 180.0f32 / PI_C;
        cvr_host::log!(
            "OpenXRManager[SUBMITFOV]: eye={} gameFov={:.2} -> submitHFov={:.2} submitVFov={:.2} (lensHFov={:.2} lensVFov={:.2} decantYaw={})\n",
            eye,
            game_h as f64,
            ((s.fov.angle_right - s.fov.angle_left) * deg) as f64,
            ((s.fov.angle_up - s.fov.angle_down) * deg) as f64,
            ((source.angle_right - source.angle_left) * deg) as f64,
            ((source.angle_up - source.angle_down) * deg) as f64,
            s.yaw_enabled as i32
        );
    }
    s.fov
}

fn quat(q: &xr::Quaternionf) -> Quat {
    Quat { x: q.x, y: q.y, z: q.z, w: q.w }
}
fn vec(v: &xr::Vector3f) -> Vec3 {
    Vec3 { x: v.x, y: v.y, z: v.z }
}
fn fov_of(f: &xr::Fovf) -> Fov {
    Fov { angle_left: f.angle_left, angle_right: f.angle_right, angle_up: f.angle_up, angle_down: f.angle_down }
}
fn xr_fov(f: &Fov) -> xr::Fovf {
    xr::Fovf { angle_left: f.angle_left, angle_right: f.angle_right, angle_up: f.angle_up, angle_down: f.angle_down }
}
fn xr_pose(p: &Pose) -> xr::Posef {
    xr::Posef {
        orientation: xr::Quaternionf { x: p.orientation.x, y: p.orientation.y, z: p.orientation.z, w: p.orientation.w },
        position: xr::Vector3f { x: p.position.x, y: p.position.y, z: p.position.z },
    }
}

/// The eyes' poses about a centre: each eye keeps its offset from the eyes' centre in the head's own axes (`head` the
/// located head orientation), carried onto the centre it is labelled with.
pub fn eyes_about(src: &[xr::Posef; 2], centres: &[xr::Posef; 2]) -> [xr::Posef; 2] {
    let head_center = Vec3 {
        x: (src[0].position.x + src[1].position.x) * 0.5,
        y: (src[0].position.y + src[1].position.y) * 0.5,
        z: (src[0].position.z + src[1].position.z) * 0.5,
    };
    let inv = conjugate_quat(quat(&src[0].orientation));
    let mut out = *centres;
    for eye in 0..2 {
        let p = vec(&src[eye].position);
        let offset_head = rotate_vector(inv, Vec3 { x: p.x - head_center.x, y: p.y - head_center.y, z: p.z - head_center.z });
        let offset = rotate_vector(quat(&centres[eye].orientation), offset_head);
        out[eye].position.x += offset.x;
        out[eye].position.y += offset.y;
        out[eye].position.z += offset.z;
    }
    out
}

/// What Present knows about the frame: its serial, the size its FOV fits, and the asymmetric projection's state.
#[repr(C)]
pub struct MonoInputs {
    pub present: u64,
    pub fov_width: f32,
    pub fov_height: f32,
    /// cvr::projection::Windows' guard and square-pixel target.
    pub asym_guard: f32,
    pub asym_target: f32,
    /// cvr::projection::Active().
    pub asym_active: bool,
}

/// The per-eye poses, centres, FOVs and flags OnPresent hands CaptureMonoPresentedFrame; nothing is written with fewer
/// than two views located.
///
/// # Safety
/// `inputs` is valid; the outputs are arrays of two.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_mono_eye_poses(
    inputs: *const MonoInputs,
    poses: *mut xr::Posef,
    centres: *mut xr::Posef,
    fovs: *mut xr::Fovf,
    views_out: *mut bool,
) {
    // SAFETY: the caller's inputs.
    let Some(inp) = (unsafe { inputs.as_ref() }) else { return };
    if poses.is_null() || centres.is_null() || fovs.is_null() || views_out.is_null() {
        return;
    }
    let views = &crate::views::VIEWS;
    let _view_guard = views.lock.lock();
    // SAFETY: under the view lock.
    let (count, live) = unsafe { (*views.count.get(), *views.views.get()) };
    if count < 2 {
        return;
    }
    // SAFETY: a plain C getter.
    let render_head = crate::frame_identity::render_head_for_seq(unsafe { GetRenderedCameraSeq() });

    let slot = crate::frame_identity::slot_for(inp.present);
    match slot {
        None => count!(CyberpunkVR_DebugSlotMiss),
        Some((_, true)) => count!(CyberpunkVR_DebugSlotHit),
        Some((_, false)) => count!(CyberpunkVR_DebugSlotReused),
    }
    let (src_pose, src_fov) = match &slot {
        Some((s, _)) => (s.view_pose, s.view_fov),
        None => ([live[0].pose, live[1].pose], [live[0].fov, live[1].fov]),
    };
    let mut mono_center = match &slot {
        Some((s, _)) => s.head_pose_local,
        None => xr::Posef {
            orientation: live[0].pose.orientation,
            position: xr::Vector3f {
                x: (src_pose[0].position.x + src_pose[1].position.x) * 0.5,
                y: (src_pose[0].position.y + src_pose[1].position.y) * 0.5,
                z: (src_pose[0].position.z + src_pose[1].position.z) * 0.5,
            },
        },
    };

    // The pose the engine was read back rendering with, else the pose bound to this frame, else the interval's.
    let bind = knob!(CyberpunkVR_BindPoseToImage) != 0;
    let read_back = knob!(CyberpunkVR_PoseReadBack) != 0;
    let mut pending = None;
    if bind {
        if read_back {
            pending = crate::frame_identity::pop_rendered(false).filter(|(_, usable)| *usable).map(|(p, _)| p);
            if pending.is_some() {
                count!(CyberpunkVR_DebugPoseReadBack);
            }
        }
        if pending.is_none() {
            pending = crate::frame_identity::bound_or_ring_pose(inp.present, knob!(CyberpunkVR_PoseFrameLag));
        }
    }
    let exact = pending.and_then(|p| crate::frame::resolve_rendered(&p));
    if let Some(c) = &exact {
        mono_center = xr_pose(c);
        count!(CyberpunkVR_DebugPoseExact);
        count!(CyberpunkVR_DebugPoseFromWrite);
    } else if matches!(slot, Some((_, true))) {
        count!(CyberpunkVR_DebugPoseEstimated);
        count!(CyberpunkVR_DebugPoseFromSlot);
    }
    // SAFETY: a plain C getter.
    if exact.is_none() && unsafe { GetRenderPoseSubmit() } != 0 {
        if let Some(r) = render_head {
            mono_center = r;
        }
    }

    // The second eye's own read-back, when it has one.
    let mut vrcam_center = None;
    if bind && read_back && knob!(CyberpunkVR_VrcamOwnLabel) != 0 {
        if let Some((p, true)) = crate::frame_identity::pop_rendered(true) {
            vrcam_center = crate::frame::resolve_rendered(&p).map(|c| xr_pose(&c));
            if vrcam_center.is_some() {
                count!(CyberpunkVR_DebugVrcamLabelUsed);
            }
        }
    }
    let vrcam_eye = if knob!(CyberpunkVR_MainIsRightEye) != 0 { 0 } else { 1 };
    let pair = [fov_of(&src_fov[0]), fov_of(&src_fov[1])];
    let asym = if inp.asym_active {
        let ori = [quat(&src_pose[0].orientation), quat(&src_pose[1].orientation)];
        windows(&pair, Some(&ori), inp.asym_guard, inp.asym_target)
    } else {
        None
    };
    let mut centre = [mono_center; 2];
    if let Some(v) = vrcam_center {
        centre[vrcam_eye] = v;
    }
    let eyes = eyes_about(&src_pose, &centre);
    // SAFETY: the caller's arrays of two.
    unsafe {
        for eye in 0..2 {
            *centres.add(eye) = centre[eye];
            *poses.add(eye) = eyes[eye];
            *fovs.add(eye) = match &asym {
                Some(w) => xr_fov(&to_fov(&w[eye])),
                None => xr_fov(&submit_fov(&pair[eye], &pair, eye, inp.fov_width, inp.fov_height)),
            };
            // NOT BEFORE THE VIEW WAS LOCATED. The submit resources size the views before the first xrLocateViews
            // fills them: a Present in between had two views of zeros -- zero poses and a zero FOV -- and the frame
            // went to the runtime as it was. A located orientation is a unit quaternion, never all zero.
            let o = &src_pose[eye].orientation;
            *views_out.add(eye) = o.x != 0.0 || o.y != 0.0 || o.z != 0.0 || o.w != 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(q: [f32; 4], p: [f32; 3]) -> xr::Posef {
        xr::Posef { orientation: xr::Quaternionf { x: q[0], y: q[1], z: q[2], w: q[3] }, position: xr::Vector3f { x: p[0], y: p[1], z: p[2] } }
    }

    #[test]
    fn eyes_keep_their_head_relative_offsets() {
        let id = [0.0, 0.0, 0.0, 1.0];
        let src = [pose(id, [-0.032, 1.6, 0.0]), pose(id, [0.032, 1.6, 0.0])];
        // Same centre: the eyes as located.
        let c = pose(id, [0.0, 1.6, 0.0]);
        let e = eyes_about(&src, &[c, c]);
        assert!((e[0].position.x + 0.032).abs() < 1e-7 && (e[1].position.x - 0.032).abs() < 1e-7);
        // A centre turned 90 degrees about Y: the eyes' axis turns with it.
        let h = core::f32::consts::FRAC_1_SQRT_2;
        let t = pose([0.0, h, 0.0, h], [1.0, 1.0, 1.0]);
        let e = eyes_about(&src, &[t, t]);
        assert!((e[0].position.x - 1.0).abs() < 1e-6 && (e[0].position.z - 1.032).abs() < 1e-6, "{:?}", e[0].position);
        assert!((e[1].position.z - 0.968).abs() < 1e-6);
        assert_eq!(e[0].orientation.y, h);
    }

    #[test]
    fn forced_fov_paths() {
        let l = Fov { angle_left: -0.9, angle_right: 0.7, angle_up: 0.75, angle_down: -0.95 };
        let r = Fov { angle_left: -0.7, angle_right: 0.9, angle_up: 0.75, angle_down: -0.95 };
        // No game FOV yet: the de-canted eye as it is.
        let s = forced_projection_fov(&l, Some(&[l, r]), 0, 2000.0, 2000.0, 0.0, 0.0, 0.0);
        assert!(!s.described && (s.fov.angle_right - s.fov.angle_left - 1.6).abs() < 1e-6);
        // The game's 90 x 100 degrees, about the eye's horizontal centre and symmetric vertically.
        let s = forced_projection_fov(&l, Some(&[l, r]), 0, 2000.0, 2000.0, 0.0, 90.0, 100.0);
        assert!(s.described);
        assert!((s.fov.angle_right - s.fov.angle_left - core::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((s.fov.angle_up + s.fov.angle_down).abs() < 1e-6);
        assert!((s.fov.angle_up - 100.0f32.to_radians() * 0.5).abs() < 1e-5);
        // Forced 90 degrees on a 2:1 picture: half-vertical atan(tan(45)/2).
        let s = forced_projection_fov(&l, Some(&[l, r]), 1, 2000.0, 1000.0, 90.0, 90.0, 100.0);
        assert!((s.fov.angle_right - core::f32::consts::FRAC_PI_4).abs() < 1e-6);
        assert!((s.fov.angle_up - 0.5f32.atan()).abs() < 1e-6 && s.fov.angle_down == -s.fov.angle_up);
        // No usable size: the frustum's own aspect.
        let s = forced_projection_fov(&l, Some(&[l, r]), 0, 0.0, 0.0, 90.0, 0.0, 0.0);
        let aspect = ((0.9f32).tan() + (0.7f32).tan()) / ((0.75f32).tan() + (0.95f32).tan());
        assert!((s.fov.angle_up - (1.0 / aspect).atan()).abs() < 1e-6);
    }
}
