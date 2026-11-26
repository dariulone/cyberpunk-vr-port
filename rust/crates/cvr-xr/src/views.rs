//! The eyes' views (OpenXRManager's m_views under m_viewMutex, and the sequential-sync eye poses and FOVs): what
//! xrLocateViews last gave, kept here; in the Rust build the C++ half reads and writes them through a fixed-capacity
//! list over this memory (include/Runtimes/OpenXRManager.hpp XrViewList). And the eyes' getters: an eye's offset from
//! the eyes' centre, an eye's FOV.
#![cfg_attr(test, allow(dead_code, unused_imports))]
use openxr_sys as xr;

use crate::srw::SrwLock;

/// The most views kept (PRIMARY_STEREO has two).
pub const MAX_VIEWS: usize = 4;
/// OpenXRManager's view members (CvrXrViewsShared). `count` and the arrays are read and written under `lock`.
#[repr(C)]
pub struct ViewsShared {
    pub lock: SrwLock,
    pub count: core::cell::UnsafeCell<u32>,
    pub views: core::cell::UnsafeCell<[xr::View; MAX_VIEWS]>,
    pub synced_poses: core::cell::UnsafeCell<[xr::Posef; 2]>,
    pub synced_fovs: core::cell::UnsafeCell<[xr::Fovf; 2]>,
    pub synced_views_valid: core::cell::UnsafeCell<bool>,
}
// SAFETY: every access is under `lock`, the C++ half's m_viewMutex (or on the one thread that resizes the list).
unsafe impl Sync for ViewsShared {}
const _: () = assert!(core::mem::size_of::<xr::View>() == 64);
const _: () = assert!(core::mem::offset_of!(ViewsShared, views) == 16 && core::mem::offset_of!(ViewsShared, synced_poses) == 272);
const _: () = assert!(core::mem::size_of::<ViewsShared>() == 368);

const NO_VIEW: xr::View = xr::View {
    ty: xr::StructureType::VIEW,
    next: core::ptr::null_mut(),
    pose: xr::Posef { orientation: xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 0.0 }, position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 } },
    fov: xr::Fovf { angle_left: 0.0, angle_right: 0.0, angle_up: 0.0, angle_down: 0.0 },
};
const NO_POSE: xr::Posef = xr::Posef { orientation: xr::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 0.0 }, position: xr::Vector3f { x: 0.0, y: 0.0, z: 0.0 } };
const NO_FOV: xr::Fovf = xr::Fovf { angle_left: 0.0, angle_right: 0.0, angle_up: 0.0, angle_down: 0.0 };
pub static VIEWS: ViewsShared = ViewsShared {
    lock: SrwLock::new(),
    count: core::cell::UnsafeCell::new(0),
    views: core::cell::UnsafeCell::new([NO_VIEW; MAX_VIEWS]),
    synced_poses: core::cell::UnsafeCell::new([NO_POSE; 2]),
    synced_fovs: core::cell::UnsafeCell::new([NO_FOV; 2]),
    synced_views_valid: core::cell::UnsafeCell::new(false),
};
#[no_mangle]
pub extern "C" fn cvr_xr_views_shared() -> *const ViewsShared {
    &VIEWS
}

#[cfg(not(test))]
extern "C" {
    fn GetSyncSequential() -> i32;
}

/// The located views' list sized to the runtime's view count, at most MAX_VIEWS, the views it gains blank (the C++
/// half's XrViewList::resize; the mono submission's resources size it each cycle).
pub fn resize(n: u32) {
    let n = (n as usize).min(MAX_VIEWS);
    let _g = VIEWS.lock.lock();
    // SAFETY: under the view lock.
    unsafe {
        let count = *VIEWS.count.get() as usize;
        for v in (*VIEWS.views.get()).iter_mut().take(n).skip(count) {
            *v = NO_VIEW;
        }
        *VIEWS.count.get() = n as u32;
    }
}

/// The two eyes' poses and FOVs as the getters see them: the sequential-sync ones when that mode has them, else the
/// located views; none with fewer than two views.
#[cfg(not(test))]
fn eyes() -> Option<([xr::Posef; 2], [xr::Fovf; 2])> {
    let _g = VIEWS.lock.lock();
    // SAFETY: under the view lock; a plain C getter.
    unsafe {
        let synced = GetSyncSequential() != 0 && crate::tracking::synced_pose_valid();
        if synced {
            return Some((*VIEWS.synced_poses.get(), *VIEWS.synced_fovs.get()));
        }
        let n = *VIEWS.count.get();
        if n < 2 {
            return None;
        }
        let v = &*VIEWS.views.get();
        Some(([v[0].pose, v[1].pose], [v[0].fov, v[1].fov]))
    }
}

/// OpenXRManager::GetCurrentEyeCenterOffset: one eye's position from the eyes' centre.
///
/// # Safety
/// `out` is an XrVector3f.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_eye_center_offset(eye: i32, out: *mut xr::Vector3f) -> bool {
    if out.is_null() || !(0..=1).contains(&eye) {
        return false;
    }
    let Some((p, _)) = eyes() else { return false };
    let c = [
        (p[0].position.x + p[1].position.x) * 0.5,
        (p[0].position.y + p[1].position.y) * 0.5,
        (p[0].position.z + p[1].position.z) * 0.5,
    ];
    let e = p[eye as usize].position;
    // SAFETY: the caller's vector.
    unsafe { *out = xr::Vector3f { x: e.x - c[0], y: e.y - c[1], z: e.z - c[2] } };
    true
}

/// OpenXRManager::GetCurrentEyeFov.
///
/// # Safety
/// `out` is an XrFovf.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_eye_fov(eye: i32, out: *mut xr::Fovf) -> bool {
    if out.is_null() || !(0..=1).contains(&eye) {
        return false;
    }
    let _g = VIEWS.lock.lock();
    // SAFETY: under the view lock; a plain C getter; the caller's FOV.
    unsafe {
        let synced = GetSyncSequential() != 0 && crate::tracking::synced_pose_valid();
        if !synced && eye as u32 >= *VIEWS.count.get() {
            return false;
        }
        *out = if synced { (*VIEWS.synced_fovs.get())[eye as usize] } else { (*VIEWS.views.get())[eye as usize].fov };
    }
    true
}

/// What the FOV line last said (OpenXRManager::MaybeLogRuntimeFovDetails logs only on a change).
struct FovLog {
    init: bool,
    eyes: [cvr_camera::projection::Fov; 2],
    h: f32,
    v: f32,
    ipd: f32,
    forced: f32,
}
static FOV_LOG: std::sync::Mutex<FovLog> = std::sync::Mutex::new(FovLog {
    init: false,
    eyes: [cvr_camera::projection::Fov { angle_left: 0.0, angle_right: 0.0, angle_up: 0.0, angle_down: 0.0 }; 2],
    h: 0.0,
    v: 0.0,
    ipd: 0.0,
    forced: 0.0,
});
/// A new session: the next FOV line is logged whatever it says.
#[no_mangle]
pub extern "C" fn cvr_xr_reset_fov_log() {
    FOV_LOG.lock().unwrap_or_else(|p| p.into_inner()).init = false;
}

#[cfg(not(test))]
extern "C" {
    fn GetForcedFov() -> f32;
}

/// OpenXRManager::MaybeLogRuntimeFovDetails: the runtime's raw frusta, its FOV and IPD, the de-canted span and the
/// cant and pitch corrections, the forced FOV -- logged when any of it moves by more than 0.01.
///
/// # Safety
/// `left` and `right` are XrFovf.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_log_runtime_fov(left: *const cvr_camera::projection::Fov, right: *const cvr_camera::projection::Fov, hfov: f32, vfov: f32, ipd: f32) {
    use cvr_camera::projection::{corrected_game_horizontal_fov_deg, runtime_fov_correction};
    // SAFETY: the caller's frusta; a plain C getter.
    let (Some(l), Some(r), forced) = (unsafe { left.as_ref() }, unsafe { right.as_ref() }, unsafe { GetForcedFov() }) else { return };
    let corr = runtime_fov_correction(l, r);
    let de_canted = corrected_game_horizontal_fov_deg(&corr);
    let mut log = FOV_LOG.lock().unwrap_or_else(|p| p.into_inner());
    let moved = |a: f32, b: f32| (a - b).abs() > 0.01;
    let e = log.eyes;
    let changed = !log.init
        || moved(e[0].angle_left, l.angle_left)
        || moved(e[0].angle_right, l.angle_right)
        || moved(e[0].angle_up, l.angle_up)
        || moved(e[0].angle_down, l.angle_down)
        || moved(e[1].angle_left, r.angle_left)
        || moved(e[1].angle_right, r.angle_right)
        || moved(e[1].angle_up, r.angle_up)
        || moved(e[1].angle_down, r.angle_down)
        || moved(log.h, hfov)
        || moved(log.v, vfov)
        || moved(log.ipd, ipd)
        || moved(log.forced, forced);
    if !changed {
        return;
    }
    *log = FovLog { init: true, eyes: [*l, *r], h: hfov, v: vfov, ipd, forced };
    let deg = |a: f32| a * (180.0f32 / 3.1415926535);
    cvr_host::log!(
        "OpenXRManager[FOV]: raw left=(L={:.3} R={:.3} U={:.3} D={:.3}) right=(L={:.3} R={:.3} U={:.3} D={:.3}) runtimeHFov={:.3} runtimeVFov={:.3} runtimeIPD={:.4} deCantedHFov={:.3} correctionYaw={:.3} correctionPitch={:.3} xr_force_fov={:.3} useRuntimeProjection={}\n",
        deg(l.angle_left),
        deg(l.angle_right),
        deg(l.angle_up),
        deg(l.angle_down),
        deg(r.angle_left),
        deg(r.angle_right),
        deg(r.angle_up),
        deg(r.angle_down),
        hfov,
        vfov,
        ipd,
        de_canted,
        deg(corr.yaw_delta_rad),
        deg(corr.pitch_delta_rad),
        forced,
        if forced <= 1.0 { 1 } else { 0 }
    );
}

// ---- the cycle's view locate

/// cvr::gaze::FromWindows: one binocular gaze ray through the vertical centre of the rendered views (the windows' up
/// and down tangents), as its tangent and NDC height; none when the windows are degenerate.
pub fn gaze_from_windows(up0: f32, down0: f32, up1: f32, down1: f32) -> [f32; 2] {
    let middle = (up0 + down0 + up1 + down1) * 0.25;
    let half = (up0 - down0 + up1 - down1) * 0.25;
    if !middle.is_finite() || !half.is_finite() || half < 0.05 || middle.abs() > 1.0 {
        return [0.0; 2];
    }
    [middle, middle / half]
}

/// What one cycle's xrLocateViews gave: the views located, and with two or more the runtime's mean FOVs, its IPD and
/// the gaze centre the asymmetric windows put the view at (zero without them).
#[repr(C)]
pub struct LocatedViews {
    pub count: u32,
    pub stats: bool,
    pub hfov_deg: f32,
    pub vfov_deg: f32,
    pub ipd: f32,
    /// cvr::gaze::Center: tangentY, ndcY.
    pub gaze: [f32; 2],
}

#[cfg(not(test))]
extern "system" {
    fn xrLocateViews(
        session: xr::Session,
        info: *const xr::ViewLocateInfo,
        state: *mut xr::ViewState,
        capacity: u32,
        count: *mut u32,
        views: *mut xr::View,
    ) -> xr::Result;
}

/// FrameThreadMain's view locate: every view at the cycle's instant in LOCAL space, into the views kept here (under
/// the view lock); then the runtime's mean FOVs and IPD, the gaze centre and the FOV line. `asym_*` is
/// cvr::projection's state (Active, and Windows' guard and target).
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn cvr_xr_locate_views(display_time: i64, asym_active: bool, asym_guard: f32, asym_target: f32) -> LocatedViews {
    use core::sync::atomic::Ordering;
    use openxr_sys::Handle;
    let mut out = LocatedViews { count: 0, stats: false, hfov_deg: 0.0, vfov_deg: 0.0, ipd: 0.0, gaze: [0.0; 2] };
    let session = crate::frame::SESSION.load(Ordering::Acquire);
    let local = crate::frame::LOCAL_SPACE.load(Ordering::Acquire);
    let _g = VIEWS.lock.lock();
    // SAFETY: under the view lock; the session's handles; the views array holds MAX_VIEWS.
    unsafe {
        let capacity = *VIEWS.count.get();
        let info = xr::ViewLocateInfo {
            ty: xr::StructureType::VIEW_LOCATE_INFO,
            next: core::ptr::null(),
            view_configuration_type: xr::ViewConfigurationType::PRIMARY_STEREO,
            display_time: xr::Time::from_nanos(display_time),
            space: xr::Space::from_raw(local),
        };
        let mut state: xr::ViewState = core::mem::zeroed();
        state.ty = xr::StructureType::VIEW_STATE;
        let views = &mut *VIEWS.views.get();
        let res = xrLocateViews(xr::Session::from_raw(session), &info, &mut state, capacity, &mut out.count, views.as_mut_ptr());
        if res.into_raw() < 0 {
            cvr_host::log!("OpenXRManager: xrLocateViews failed (res={})\n", res.into_raw());
            out.count = 0;
            return out;
        }
        if out.count < 2 {
            return out;
        }
        let deg = 180.0f32 / 3.1415926535;
        let (v0, v1) = (&views[0], &views[1]);
        let hfov0 = (v0.fov.angle_right - v0.fov.angle_left) * deg;
        let hfov1 = (v1.fov.angle_right - v1.fov.angle_left) * deg;
        let vfov0 = (v0.fov.angle_up - v0.fov.angle_down) * deg;
        let vfov1 = (v1.fov.angle_up - v1.fov.angle_down) * deg;
        let (dx, dy, dz) = (v1.pose.position.x - v0.pose.position.x, v1.pose.position.y - v0.pose.position.y, v1.pose.position.z - v0.pose.position.z);
        out.stats = true;
        out.hfov_deg = (hfov0 + hfov1) * 0.5;
        out.vfov_deg = (vfov0 + vfov1) * 0.5;
        out.ipd = (dx * dx + dy * dy + dz * dz).sqrt();
        let fov = |f: &xr::Fovf| cvr_camera::projection::Fov { angle_left: f.angle_left, angle_right: f.angle_right, angle_up: f.angle_up, angle_down: f.angle_down };
        let pair = [fov(&v0.fov), fov(&v1.fov)];
        if asym_active {
            let q = |o: &xr::Quaternionf| cvr_body::xr::Quat { x: o.x, y: o.y, z: o.z, w: o.w };
            let ori = [q(&v0.pose.orientation), q(&v1.pose.orientation)];
            if let Some(w) = cvr_camera::projection::windows(&pair, Some(&ori), asym_guard, asym_target) {
                out.gaze = gaze_from_windows(w[0].u, w[0].d, w[1].u, w[1].d);
            }
        }
        cvr_xr_log_runtime_fov(&pair[0], &pair[1], out.hfov_deg, out.vfov_deg, out.ipd);
    }
    out
}

// ---- the projection layer's views

/// What one cycle submits its projection views from (FrameThreadMain).
#[repr(C)]
pub struct ProjectionInputs {
    /// The captured frame's per-eye poses and FOVs (or frame generation's).
    pub poses: [xr::Posef; 2],
    pub fovs: [xr::Fovf; 2],
    pub has_view: [bool; 2],
    /// The eye MAIN's picture belongs to.
    pub main_eye: u32,
    pub view_count: u32,
    /// cvr::projection::Active().
    pub asym_active: bool,
    pub framegen_selected: bool,
    /// The second eye has its own picture (VRCAM's).
    pub second_picture: bool,
    pub menu_rect: bool,
    /// The runtime's IPD in metres.
    pub ipd: f32,
}

#[cfg(not(test))]
#[allow(non_upper_case_globals)]
extern "C" {
    static CyberpunkVR_MonoCyclopeanPose: i32;
    static CyberpunkVR_VrcamOwnLabel: i32;
    static CyberpunkVR_FlatDistanceM: f32;
    fn GetMenuFov() -> f32;
}

/// One eye's projection view as FrameThreadMain composes it: the captured pose and FOV (MAIN's window for an eye shown
/// MAIN's picture under the asymmetric projection); one orientation and the midpoint for both eyes when they share
/// one image (xr_mono_cyclopean, unless each eye has its own label); in a menu the latched eye poses and a square FOV;
/// the eye's whole swapchain image; and, when both eyes see one image, each frustum turned inward by half the vergence
/// of xr_flat_distance (MAIN's eye counted as the left). The view's type and chain are the caller's.
///
/// # Safety
/// `inputs` is valid; `view` is an XrCompositionLayerProjectionView.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_compose_projection_view(inputs: *const ProjectionInputs, eye: u32, swapchain: u64, width: u32, height: u32, view: *mut xr::CompositionLayerProjectionView) {
    use openxr_sys::Handle;
    use std::sync::atomic::{AtomicBool, Ordering};
    // SAFETY: the caller's inputs and view.
    let (Some(inp), Some(v)) = (unsafe { inputs.as_ref() }, unsafe { view.as_mut() }) else { return };
    let e = eye as usize;
    if e >= 2 {
        return;
    }
    v.pose = inp.poses[e];
    v.fov = inp.fovs[e];
    // AN EYE SHOWN MAIN'S PICTURE TAKES MAIN'S WINDOW.
    let main = inp.main_eye;
    if inp.asym_active && !inp.framegen_selected && !inp.second_picture && eye != main && main < inp.view_count {
        v.fov = inp.fovs[main as usize];
    }
    // SAFETY: the panel's knobs; plain C getters.
    let (cyclopean, own_label, flat) = unsafe {
        (
            core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_MonoCyclopeanPose)) != 0,
            core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_VrcamOwnLabel)) != 0,
            core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_FlatDistanceM)),
        )
    };
    let both = inp.has_view[0] && inp.has_view[1];
    if eye == 0 && both {
        // The angle between the two eye orientations the runtime handed us, said once.
        static CANT_LOGGED: AtomicBool = AtomicBool::new(false);
        if !CANT_LOGGED.swap(true, Ordering::Relaxed) {
            let (a, b) = (&inp.poses[0].orientation, &inp.poses[1].orientation);
            let dot = (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w).abs().min(1.0);
            cvr_host::log!(
                "OpenXRManager[CANT]: eye-pose orientations differ by {:.3} deg (cyclopean={} flatDist={:.2})\n",
                (2.0 * cvr_body::cmath::acos(dot) * 57.2957795f32) as f64,
                cyclopean as i32,
                flat as f64
            );
        }
    }
    // ONE ORIENTATION FOR BOTH EYES WHEN THEY SHARE ONE IMAGE.
    if cyclopean && !own_label && both {
        let (p0, p1) = (&inp.poses[0].position, &inp.poses[1].position);
        v.pose.orientation = inp.poses[0].orientation;
        v.pose.position = xr::Vector3f { x: (p0.x + p1.x) * 0.5, y: (p0.y + p1.y) * 0.5, z: (p0.z + p1.z) * 0.5 };
    }
    if inp.menu_rect {
        if let Some(p) = crate::menu::menu_eye_pose(e) {
            v.pose = p;
        }
        // SAFETY: a plain C getter.
        let menu_fov = unsafe { GetMenuFov() };
        if menu_fov > 1.0 && menu_fov < 170.0 {
            let half = (menu_fov * 3.1415926535 / 180.0) * 0.5;
            v.fov = xr::Fovf { angle_left: -half, angle_right: half, angle_up: half, angle_down: -half };
        }
    }
    v.sub_image.swapchain = xr::Swapchain::from_raw(swapchain);
    v.sub_image.image_rect = xr::Rect2Di { offset: xr::Offset2Di { x: 0, y: 0 }, extent: xr::Extent2Di { width: width as i32, height: height as i32 } };
    // FLAT CONTENT SITS AT OPTICAL INFINITY UNLESS IT IS GIVEN A DISTANCE: both eyes or neither.
    if !inp.second_picture && flat > 0.05 && inp.ipd > 0.03 && inp.ipd < 0.10 {
        let half = 0.5 * (inp.ipd / flat);
        let d = if eye == main { half } else { -half };
        v.fov.angle_left += d;
        v.fov.angle_right += d;
    }
    v.sub_image.image_array_index = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_resized_as_the_vector() {
        let count = || {
            let _g = VIEWS.lock.lock();
            // SAFETY: under the view lock.
            unsafe { *VIEWS.count.get() }
        };
        resize(2);
        assert_eq!(count(), 2);
        {
            let _g = VIEWS.lock.lock();
            // SAFETY: under the view lock.
            unsafe { (*VIEWS.views.get())[0].pose.position.x = 1.5 };
        }
        // Grown: the kept view stays, the new ones are blank; never past MAX_VIEWS.
        resize(9);
        assert_eq!(count(), MAX_VIEWS as u32);
        {
            let _g = VIEWS.lock.lock();
            // SAFETY: under the view lock.
            let views = unsafe { &*VIEWS.views.get() };
            assert_eq!(views[0].pose.position.x, 1.5);
            assert_eq!((views[3].ty, views[3].pose.position.x), (xr::StructureType::VIEW, 0.0));
        }
        resize(0);
        assert_eq!(count(), 0);
        // Grown again from none: the old view is blanked.
        resize(1);
        let _g = VIEWS.lock.lock();
        // SAFETY: under the view lock.
        assert_eq!(unsafe { (*VIEWS.views.get())[0].pose.position.x }, 0.0);
        // SAFETY: as above.
        unsafe { *VIEWS.count.get() = 0 };
    }

    #[test]
    fn gaze_centre_from_the_windows() {
        // Symmetric windows: the centre is the middle.
        assert_eq!(gaze_from_windows(1.0, -1.0, 1.0, -1.0), [0.0, 0.0]);
        // Shifted down by 0.2 in tangent with a half-height of 1: tangent -0.2, NDC -0.2.
        let g = gaze_from_windows(0.8, -1.2, 0.8, -1.2);
        assert!((g[0] + 0.2).abs() < 1e-6 && (g[1] + 0.2).abs() < 1e-6);
        // Degenerate windows publish no centre.
        assert_eq!(gaze_from_windows(0.01, 0.0, 0.01, 0.0), [0.0, 0.0]);
        assert_eq!(gaze_from_windows(f32::NAN, 0.0, 1.0, -1.0), [0.0, 0.0]);
    }
}
