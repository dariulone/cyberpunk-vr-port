//! The menu panel's pose (formerly OpenXRManager::ComputeMenuQuadPose): anchored 1.5 m in front of the player when a
//! menu opens, then holding still while the head turns within a free-look cone (xr_menu_follow_deg) and easing back
//! after three seconds at rest or a turn past it; translation follows the head. A dropped head holds the last pose.
//!
//! The state (the follow, its clock, the latched pose, the projection path's per-eye anchor) is owned here; in the
//! Rust build OpenXRManager's members are references to it. The XR thread alone touches it.
use cvr_body::cmath::{clamp, cos, sin};
use cvr_body::xr::{rotate_vector, Quat, Vec3};

use crate::hands::Pose;
use crate::hud::{cvr_hud_head_yaw, DisplayClock, Follow};

/// OpenXRManager's menu members (include/Runtimes/OpenXRManager.hpp CvrXrMenuShared).
#[repr(C)]
pub struct MenuShared {
    pub follow: Follow,
    pub clock: DisplayClock,
    pub quad_pose: Pose,
    pub eye_anchor_valid: bool,
    pub eye_anchor_yaw: f32,
}
const _: () = assert!(core::mem::offset_of!(MenuShared, quad_pose) == 40 && core::mem::size_of::<MenuShared>() == 80);
struct Menu(core::cell::UnsafeCell<MenuShared>);
// SAFETY: the XR frame thread alone reads and writes it (the C++ half's members had the same rule).
unsafe impl Sync for Menu {}
static MENU: Menu = Menu(core::cell::UnsafeCell::new(MenuShared {
    follow: Follow { yaw: 0.0, valid: false, following: false, delay_pending: false, still_yaw: 0.0, still_seconds: 0.0 },
    clock: DisplayClock { last: 0, elapsed: 0.0 },
    quad_pose: Pose { orientation: Quat { x: 0.0, y: 0.0, z: 0.0, w: 0.0 }, position: Vec3 { x: 0.0, y: 0.0, z: 0.0 } },
    eye_anchor_valid: false,
    eye_anchor_yaw: 0.0,
}));
#[no_mangle]
pub extern "C" fn cvr_xr_menu_shared() -> *mut MenuShared {
    MENU.0.get()
}

/// The panel's pose from the head and the cone: the yaw follows, the 1.5 m forward offset is turned by it.
pub fn menu_pose(m: &mut MenuShared, head: Option<Pose>, base: Option<Pose>, follow_deg: f32, display_time: i64) -> Pose {
    if head.is_none() {
        // Head tracking dropped: hold the last pose, never snap to the base.
        m.follow.reset_delay();
        m.clock = DisplayClock::default();
        if m.follow.valid {
            return m.quad_pose;
        }
    }
    let identity = Pose { orientation: Quat::IDENTITY, position: Vec3::default() };
    let r = head.or(base).unwrap_or(identity);
    let o = r.orientation;
    let head_yaw = cvr_hud_head_yaw(o.x, o.y, o.z, o.w, if m.follow.valid { m.follow.yaw } else { 0.0 });
    m.clock.step(display_time);
    let cone = clamp(follow_deg, 5.0, 90.0) * 0.01745329252;
    let yaw = m.follow.update(head_yaw, cone, m.clock.elapsed, true);
    let q = Quat { x: 0.0, y: sin(yaw * 0.5), z: 0.0, w: cos(yaw * 0.5) };
    let fwd = rotate_vector(q, Vec3 { x: 0.0, y: 0.0, z: -1.5 });
    let pose = Pose { orientation: q, position: Vec3 { x: r.position.x + fwd.x, y: r.position.y + fwd.y, z: r.position.z + fwd.z } };
    m.quad_pose = pose;
    pose
}

#[cfg(not(test))]
extern "C" {
    fn GetMenuFollowDeg() -> f32;
}

/// OpenXRManager::ComputeMenuQuadPose.
///
/// # Safety
/// `head` and `out` are XrPosef; called on the XR frame thread.
#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_menu_quad_pose(head_located: bool, head: *const Pose, display_time: i64, out: *mut Pose) {
    // SAFETY: the caller's poses; the frame thread's own menu state.
    unsafe {
        let Some(out) = out.as_mut() else { return };
        let head = if head_located { head.as_ref().copied() } else { None };
        // The recenter base only for the FIRST anchor (the head is not there).
        let base = if head.is_none() { crate::tracking::base_and_origin().map(|(b, _)| b) } else { None };
        *out = menu_pose(&mut *MENU.0.get(), head, base, GetMenuFollowDeg(), display_time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_panel_sits_ahead_and_holds_when_the_head_drops() {
        let mut m = MenuShared {
            follow: Follow::default(),
            clock: DisplayClock::default(),
            quad_pose: Pose::default(),
            eye_anchor_valid: false,
            eye_anchor_yaw: 0.0,
        };
        let head = Pose { orientation: Quat::IDENTITY, position: Vec3 { x: 0.0, y: 1.7, z: 0.0 } };
        let p = menu_pose(&mut m, Some(head), None, 30.0, 1_000);
        assert!((p.position.z + 1.5).abs() < 1e-6 && (p.position.y - 1.7).abs() < 1e-6, "{p:?}");
        // the head drops: the same pose, even from elsewhere
        let held = menu_pose(&mut m, None, Some(Pose::default()), 30.0, 2_000);
        assert_eq!(held, p);
    }
}

/// A recenter (or the first base): the panel re-anchors dead ahead of the new forward -- the follow, its clock and the
/// projection path's per-eye anchor start again. XR frame thread only.
pub fn reset_anchor() {
    // SAFETY: the XR frame thread alone touches the menu state.
    let m = unsafe { &mut *MENU.0.get() };
    m.follow.valid = false;
    m.follow.following = false;
    m.follow.reset_delay();
    m.clock = DisplayClock::default();
    m.eye_anchor_valid = false;
}

/// The projection path's per-eye poses latched when a menu opens (OpenXRManager's m_menuEyePoses), re-latched past the
/// free-look cone. XR frame thread only, beside the rest of the menu state.
struct MenuEyes(core::cell::UnsafeCell<[openxr_sys::Posef; 2]>);
// SAFETY: the XR frame thread alone reads and writes it.
unsafe impl Sync for MenuEyes {}
static MENU_EYES: MenuEyes = MenuEyes(core::cell::UnsafeCell::new(
    [openxr_sys::Posef { orientation: openxr_sys::Quaternionf { x: 0.0, y: 0.0, z: 0.0, w: 0.0 }, position: openxr_sys::Vector3f { x: 0.0, y: 0.0, z: 0.0 } }; 2],
));

/// The head yaw an orientation looks along (atan2 of the forward's horizontal), as the projection path's anchor uses.
pub fn view_yaw(o: &openxr_sys::Quaternionf) -> f32 {
    let fx = -2.0 * (o.x * o.z + o.y * o.w);
    let fz = 2.0 * (o.x * o.x + o.y * o.y) - 1.0;
    cvr_body::cmath::atan2(-fx, -fz)
}

/// The menu's per-eye poses for the projection path: latched from the located views when the anchor is not valid or
/// the head turned past the free-look cone (xr_menu_follow_deg, 5..90 degrees), held otherwise. None before a latch.
#[cfg(not(test))]
pub fn menu_eye_pose(eye: usize) -> Option<openxr_sys::Posef> {
    // SAFETY: the XR frame thread alone touches the menu state; the views under their lock.
    unsafe {
        let m = &mut *MENU.0.get();
        let eyes = &mut *MENU_EYES.0.get();
        {
            let views = &crate::views::VIEWS;
            let _g = views.lock.lock();
            if *views.count.get() >= 2 {
                let v = &*views.views.get();
                let head_yaw = view_yaw(&v[0].pose.orientation);
                let start = clamp(GetMenuFollowDeg() * 0.01745329252, 0.0872, 1.5708);
                let mut off = head_yaw - m.eye_anchor_yaw;
                while off > 3.14159265 {
                    off -= 6.28318531;
                }
                while off < -3.14159265 {
                    off += 6.28318531;
                }
                if !m.eye_anchor_valid || off.abs() > start {
                    eyes[0] = v[0].pose;
                    eyes[1] = v[1].pose;
                    m.eye_anchor_yaw = head_yaw;
                    m.eye_anchor_valid = true;
                }
            }
        }
        (m.eye_anchor_valid && eye < 2).then(|| eyes[eye])
    }
}
