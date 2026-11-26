//! The left gun's barrel dot (src/Overlay/OverlayDebugDraw.cpp, `DrawLeftBarrelDot`): with two guns in hand, the mark
//! DrawBarrelCrosshair makes for the right barrel, made for the left one the reload mod publishes (shared slots 165
//! two guns out, 166..168 the muzzle, 169..171 its forward, 172 valid) -- the point its bullet reaches at the dot
//! distance, seen from each eye and drawn green on the background list: once in the first eye without engine stereo,
//! else in both halves of the backbuffer, the second eye's carried across the asymmetric windows and nudged by the
//! second-eye offset.
//!
//! The Rust build draws it here, on cvr-camera's barrel_dot arithmetic; the C++ half lends what it owns through the
//! attached runtime (the shared slots, the switch, engine stereo, the asymmetric windows and the head-space
//! projection).

use crate::eye_windows as ew;
use crate::imgui::{background_draw_list_handle, Io};
use crate::overlay_exports::{CyberpunkVR_BarrelDotDistM, CyberpunkVR_BarrelDotEyeSign, CyberpunkVR_BarrelDotOffX2};
use core::sync::atomic::{AtomicPtr, Ordering::Acquire};
use cvr_camera::barrel_dot::{camera_right, eye_pixel, eye_ray, half_ipd, head_center, main_sign, muzzle_forward, ndc};
use cvr_camera::projection::{map_ndc_x, map_ndc_y, EyeWindow};

/// What the dot reads of the C++ half.
#[repr(C)]
pub struct BarrelRuntime {
    /// OpenXRManager::GetSharedSlot.
    shared_slot: extern "C" fn(i32) -> f32,
    /// overlay::g_drawBarrelCross.
    draw_barrel_cross: *const bool,
    /// engine_stereo::Enabled and OnePicture.
    engine_stereo: extern "C" fn() -> bool,
    one_picture: extern "C" fn() -> bool,
    /// projection::Active() && OpenXRManager::GetAsymmetricEyeWindows.
    asym_windows: extern "C" fn(*mut [EyeWindow; 2]) -> bool,
    /// ProjectHeadSpacePointToScreen.
    project: extern "C" fn(f32, f32, f32, f32, f32, *mut [f32; 2]) -> bool,
}
// SAFETY: the table holds plain functions and addresses of statics.
unsafe impl Sync for BarrelRuntime {}
/// The table, built here (formerly OverlayDebugDraw.cpp's, attached at its static initialisation): the services are
/// eye_windows.rs's, the switches ImGuiOverlay.cpp's.
static RT: BarrelRuntime = BarrelRuntime {
    shared_slot: ew::shared_slot,
    draw_barrel_cross: &raw const ew::g_drawBarrelCross,
    engine_stereo: ew::engine_stereo,
    one_picture: ew::one_picture,
    asym_windows: ew::asym_windows,
    project: ew::project,
};
static RUNTIME: AtomicPtr<BarrelRuntime> = AtomicPtr::new((&raw const RT).cast_mut());

/// camera::BarrelFrame.
#[repr(C)]
#[derive(Default)]
struct BarrelFrame {
    cam_quat: [f32; 4],
    muzzle_fwd: [f32; 3],
}
const _: () = assert!(core::mem::size_of::<BarrelFrame>() == 28);

#[allow(non_upper_case_globals)]
extern "C" {
    fn CyberpunkVRPort_HalfIpd() -> f32;
    fn cvr_camera_barrel_frame_read(out: *mut BarrelFrame) -> bool;
    static g_lastLocatePosFP: [i32; 3];
    static CyberpunkVR_MainIsRightEye: i32;
}

/// vrshared's left-gun slots.
const DUAL_WIELD_ACTIVE: i32 = 165;
const LEFT_MUZZLE_POS: i32 = 166;
const LEFT_MUZZLE_FWD: i32 = 169;
const LEFT_MUZZLE_VALID: i32 = 172;
const HALF_IPD_SLOT: i32 = 95;
/// IM_COL32(60, 255, 90, 255).
const GREEN: u32 = (255 << 24) | (90 << 16) | (255 << 8) | 60;
const RADIUS: f32 = 3.0;

/// A C++ global, read as the .cpp reads it (volatile).
///
/// # Safety
/// `value` is a live global.
unsafe fn volatile<T: Copy>(value: *const T) -> T {
    unsafe { core::ptr::read_volatile(value) }
}

/// DrawLeftBarrelDot.
#[no_mangle]
pub extern "C" fn cvr_overlay_left_barrel_dot() {
    let runtime = RUNTIME.load(Acquire);
    if runtime.is_null() {
        return;
    }
    // SAFETY: attached once at load, from a static.
    let r = unsafe { &*runtime };
    // SAFETY: a C++ bool the overlay owns.
    if !unsafe { *r.draw_barrel_cross } {
        return;
    }
    let slot = r.shared_slot;
    if slot(DUAL_WIELD_ACTIVE) < 0.5 || slot(LEFT_MUZZLE_VALID) < 0.5 {
        return;
    }
    let mp = [slot(LEFT_MUZZLE_POS), slot(LEFT_MUZZLE_POS + 1), slot(LEFT_MUZZLE_POS + 2)];
    let Some(mf) = muzzle_forward(mp, [slot(LEFT_MUZZLE_FWD), slot(LEFT_MUZZLE_FWD + 1), slot(LEFT_MUZZLE_FWD + 2)]) else { return };
    let mut bf = BarrelFrame::default();
    // SAFETY: a plain read into a local.
    if !unsafe { cvr_camera_barrel_frame_read(&mut bf) } {
        return;
    }
    // SAFETY: the camera hook's published head (a C++ global, volatile).
    let fixed = unsafe {
        let head = core::ptr::addr_of!(g_lastLocatePosFP).cast::<i32>();
        [volatile(head), volatile(head.add(1)), volatile(head.add(2))]
    };
    let (hc, real) = head_center(fixed);
    if !real {
        return;
    }
    let engine_stereo = (r.engine_stereo)();
    if engine_stereo && (r.one_picture)() {
        return;
    }
    let back = Io::get().display_size();
    let display = [if engine_stereo { back[0] * 0.5 } else { back[0] }, back[1]];
    if display[0] <= 1.0 || display[1] <= 1.0 {
        return;
    }
    let rgt = camera_right(bf.cam_quat);
    // SAFETY: a plain call.
    let half = half_ipd(unsafe { CyberpunkVRPort_HalfIpd() }, slot(HALF_IPD_SLOT));
    // SAFETY: C++ globals (dllexport live controls).
    let (distance, eye_sign, off_x2, main_is_right) =
        unsafe {
            (
                volatile(core::ptr::addr_of!(CyberpunkVR_BarrelDotDistM)),
                volatile(core::ptr::addr_of!(CyberpunkVR_BarrelDotEyeSign)),
                volatile(core::ptr::addr_of!(CyberpunkVR_BarrelDotOffX2)),
                volatile(core::ptr::addr_of!(CyberpunkVR_MainIsRightEye)) != 0,
            )
        };
    if !(distance > 0.5) {
        return;
    }
    let dot_for_eye = |sign: f32| -> Option<[f32; 2]> {
        let local = eye_ray(mp, mf, distance, hc, rgt, half, sign, bf.cam_quat);
        let mut out = [0.0f32; 2];
        (r.project)(local[0], local[2], -local[1], display[0], display[1], &mut out).then_some(out)
    };
    let s_main = main_sign(engine_stereo, main_is_right, eye_sign);
    let Some(sc) = dot_for_eye(s_main) else { return };
    let sc_second = dot_for_eye(-s_main).unwrap_or(sc);
    let Some(dl) = background_draw_list_handle() else { return };
    if !engine_stereo {
        dl.circle_filled(sc, RADIUS, GREEN);
        return;
    }
    let main_eye = usize::from(main_is_right);
    let [mut x2, mut y2] = ndc(sc_second, display);
    let mut windows = [EyeWindow::default(); 2];
    if (r.asym_windows)(&mut windows) {
        x2 = map_ndc_x(x2, &windows[main_eye], &windows[1 - main_eye]);
        y2 = map_ndc_y(y2, &windows[main_eye], &windows[1 - main_eye]);
    }
    x2 += off_x2;
    let first = ndc(sc, display);
    for eye in 0..2usize {
        let left = eye as f32 * display[0];
        let at = eye_pixel(if eye == main_eye { first } else { [x2, y2] }, eye as i32, display);
        dl.push_clip_rect([left, 0.0], [left + display[0], display[1]], true);
        dl.circle_filled(at, RADIUS, GREEN);
        dl.pop_clip_rect();
    }
}
