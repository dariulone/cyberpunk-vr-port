//! The exact barrel crosshair (src/Overlay/OverlayDebugDraw.cpp, `DrawBarrelCrosshair`): the point the bullet reaches
//! at the dot distance -- from the muzzle the camera publishes with its own quaternion in one seqlocked frame, the
//! arm launcher's line when it is selected, the shot leaving the sight line while one is looked through -- seen from
//! each eye's own position, so both eyes mark the same place in the world; else (no world data, or the world path
//! off) the barrel direction projected once with the zero-distance parallax for the second eye. Its NDC is published
//! for the second-eye pass (MAIN's in X/Y, the second eye's X carried across the asymmetric windows and nudged), and
//! it is drawn red on the background list: in both halves of the backbuffer under engine stereo, else once. The world
//! path's state is logged when it changes.
//!
//! The Rust build draws it here, on cvr-camera's barrel_dot and sight_line arithmetic; the C++ half lends what it owns
//! through the attached runtime (the shared slots, the switch, engine stereo, the asymmetric windows, the overlay's
//! projection and its tangents, the runtime IPD).

use crate::eye_windows as ew;
use crate::imgui::{background_draw_list_handle, Io};
use crate::overlay_exports::{
    CyberpunkVR_BarrelDotDistM, CyberpunkVR_BarrelDotEyeSign, CyberpunkVR_BarrelDotNdcX, CyberpunkVR_BarrelDotNdcX2, CyberpunkVR_BarrelDotNdcY,
    CyberpunkVR_BarrelDotOffX2, CyberpunkVR_BarrelDotRadiusPx, CyberpunkVR_BarrelDotTick, CyberpunkVR_BarrelDotWorld, CyberpunkVR_SightZeroMeters,
};
use core::sync::atomic::{AtomicPtr, Ordering::Acquire};
use cvr_camera::barrel_dot::{camera_right, eye_pixel, eye_ray, half_ipd, head_center, main_sign, ndc, parallax};
use cvr_camera::overlay_projection::rotate;
use cvr_camera::projection::{map_ndc_x, map_ndc_y, EyeWindow};
use cvr_camera::sight_line::sight_line_offset;
use std::sync::Mutex;

/// What the crosshair reads of the C++ half.
#[repr(C)]
pub struct CrosshairRuntime {
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
    /// GetOverlayProjTans.
    proj_tans: extern "C" fn(f32, f32, *mut f32, *mut f32) -> bool,
    /// OpenXRManager::GetRuntimeIpd.
    runtime_ipd: extern "C" fn() -> f32,
}
// SAFETY: the table holds plain functions and addresses of statics.
unsafe impl Sync for CrosshairRuntime {}
/// The table, built here (formerly OverlayDebugDraw.cpp's, attached at its static initialisation): the services are
/// eye_windows.rs's, the switches ImGuiOverlay.cpp's.
static RT: CrosshairRuntime = CrosshairRuntime {
    shared_slot: ew::shared_slot,
    draw_barrel_cross: &raw const ew::g_drawBarrelCross,
    engine_stereo: ew::engine_stereo,
    one_picture: ew::one_picture,
    asym_windows: ew::asym_windows,
    project: ew::project,
    proj_tans: ew::proj_tans,
    runtime_ipd: ew::runtime_ipd,
};
static RUNTIME: AtomicPtr<CrosshairRuntime> = AtomicPtr::new((&raw const RT).cast_mut());

/// camera::BarrelFrame.
#[repr(C)]
#[derive(Default)]
struct BarrelFrame {
    cam_quat: [f32; 4],
    muzzle_fwd: [f32; 3],
}

#[allow(non_upper_case_globals)]
extern "C" {
    fn CyberpunkVRPort_HalfIpd() -> f32;
    fn cvr_camera_barrel_frame_read(out: *mut BarrelFrame) -> bool;
    fn cvr_cyberware_launcher_line(origin: *mut f32, direction: *mut f32) -> bool;
    static g_lastLocatePosFP: [i32; 3];
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_WeaponClass: i32;
    static CyberpunkVR_SightDot: i32;
    static CyberpunkVR_HitscanFromSightLine: i32;
    static CyberpunkVR_SightLineTick: u64;
    static CyberpunkVR_SightLineOffset: [f32; 3];
}

/// A C++ global, read as the .cpp reads it.
///
/// # Safety
/// `value` is a live global.
unsafe fn get<T: Copy>(value: *const T) -> T {
    unsafe { core::ptr::read_volatile(value) }
}
/// A C++ global, written as the .cpp writes it.
///
/// # Safety
/// `value` is a live global.
unsafe fn set<T: Copy>(value: *mut T, v: T) {
    unsafe { core::ptr::write_volatile(value, v) }
}

/// The function statics the .cpp keeps.
struct State {
    muzzle: [f32; 3],
    dbg_ly: [f32; 2],
    dbg_why: f32,
    said: i32,
}
static STATE: Mutex<State> = Mutex::new(State { muzzle: [0.0; 3], dbg_ly: [0.0; 2], dbg_why: 0.0, said: -1 });

const EQUIPPED_SLOT: i32 = 144;
const MUZZLE_SLOT: i32 = 200;
const HALF_IPD_SLOT: i32 = 95;
const LAUNCHER_CLASS: i32 = 5;
/// IM_COL32(255, 60, 60, 255).
const RED: u32 = (255 << 24) | (60 << 16) | (60 << 8) | 255;
const RADIUS: f32 = 3.0;

/// DrawBarrelCrosshair.
#[no_mangle]
pub extern "C" fn cvr_overlay_barrel_crosshair() {
    // Invalidate the second-eye/mirror publication before any early return.
    // SAFETY: a C++ global.
    unsafe { set(core::ptr::addr_of_mut!(CyberpunkVR_BarrelDotTick), 0) };
    let runtime = RUNTIME.load(Acquire);
    if runtime.is_null() {
        return;
    }
    // SAFETY: attached once at load, from a static.
    let r = unsafe { &*runtime };
    let slot = r.shared_slot;
    let equipped = slot(EQUIPPED_SLOT);
    let (mut launcher_pos, mut launcher_fwd) = ([0.0f32; 3], [0.0f32; 3]);
    // SAFETY: two writable vectors.
    let launcher = unsafe { cvr_cyberware_launcher_line(launcher_pos.as_mut_ptr(), launcher_fwd.as_mut_ptr()) };
    // SAFETY: the overlay's bool and C++ globals.
    let (cross, sight_dot, weapon_class) = unsafe { (*r.draw_barrel_cross, get(core::ptr::addr_of!(CyberpunkVR_SightDot)), get(core::ptr::addr_of!(CyberpunkVR_WeaponClass))) };
    if !launcher && (!(cross || sight_dot != 0) || !(equipped >= 0.9) || weapon_class == LAUNCHER_CLASS) {
        return;
    }
    let mut bf = BarrelFrame::default();
    // SAFETY: a plain read into a local.
    if !unsafe { cvr_camera_barrel_frame_read(&mut bf) } {
        return;
    }
    let mf = bf.muzzle_fwd;
    if mf[0] * mf[0] + mf[1] * mf[1] + mf[2] * mf[2] < 0.25 {
        return;
    }
    let q = bf.cam_quat;
    let Ok(mut state) = STATE.lock() else { return };
    // The muzzle position, latched: a zero read changes nothing.
    {
        let (rx, ry, rz) = (slot(MUZZLE_SLOT), slot(MUZZLE_SLOT + 1), slot(MUZZLE_SLOT + 2));
        if rx * rx + ry * ry + rz * rz > 1.0 {
            state.muzzle = [rx, ry, rz];
        }
    }
    // The shot's own origin: on the sight line while the eye is on a sight.
    // SAFETY: C++ globals.
    let lift = unsafe {
        let offset = core::ptr::addr_of!(CyberpunkVR_SightLineOffset).cast::<f32>();
        sight_line_offset(
            get(core::ptr::addr_of!(CyberpunkVR_HitscanFromSightLine)) != 0,
            get(core::ptr::addr_of!(CyberpunkVR_SightLineTick)),
            windows_sys::Win32::System::SystemInformation::GetTickCount64(),
            [get(offset), get(offset.add(1)), get(offset.add(2))],
            mf,
        )
    }
    .unwrap_or([0.0; 3]);
    let mp = if launcher { launcher_pos } else { [state.muzzle[0] + lift[0], state.muzzle[1] + lift[1], state.muzzle[2] + lift[2]] };
    // SAFETY: the camera hook's published head (a C++ global, volatile).
    let fixed = unsafe {
        let head = core::ptr::addr_of!(g_lastLocatePosFP).cast::<i32>();
        [get(head), get(head.add(1)), get(head.add(2))]
    };
    let (hc, head_real) = head_center(fixed);
    let have_world = (mp[0] * mp[0] + mp[1] * mp[1] + mp[2] * mp[2]) > 1.0 && head_real;
    let engine_stereo = (r.engine_stereo)();
    if engine_stereo && (r.one_picture)() {
        return;
    }
    let back = Io::get().display_size();
    let display = [if engine_stereo { back[0] * 0.5 } else { back[0] }, back[1]];
    if display[0] <= 1.0 || display[1] <= 1.0 {
        return;
    }
    let rgt = camera_right(q);
    // SAFETY: a plain call.
    let half = half_ipd(unsafe { CyberpunkVRPort_HalfIpd() }, slot(HALF_IPD_SLOT));
    // SAFETY: C++ globals (dllexport live controls).
    let (distance, eye_sign, off_x2, world, zero_m, main_is_right) = unsafe {
        (
            get(core::ptr::addr_of!(CyberpunkVR_BarrelDotDistM)),
            get(core::ptr::addr_of!(CyberpunkVR_BarrelDotEyeSign)),
            get(core::ptr::addr_of!(CyberpunkVR_BarrelDotOffX2)),
            get(core::ptr::addr_of!(CyberpunkVR_BarrelDotWorld)),
            get(core::ptr::addr_of!(CyberpunkVR_SightZeroMeters)),
            get(core::ptr::addr_of!(CyberpunkVR_MainIsRightEye)) != 0,
        )
    };
    let project = |p: [f32; 3]| -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        (r.project)(p[0], p[1], p[2], display[0], display[1], &mut out).then_some(out)
    };
    // The impact point's pixel as seen from one eye; the .cpp's diagnostics kept.
    let dot_for_eye = |state: &mut State, sign: f32| -> Option<[f32; 2]> {
        if !have_world {
            state.dbg_why = 1.0;
            return None;
        }
        if !(distance > 0.5) {
            state.dbg_why = 2.0;
            return None;
        }
        let local = eye_ray(mp, mf, distance, hc, rgt, half, sign, q);
        state.dbg_ly[usize::from(sign > 0.0)] = local[1];
        let at = project([local[0], local[2], -local[1]]);
        if at.is_none() {
            state.dbg_why = 3.0;
        }
        at
    };
    let v = rotate(mf, [-q[0], -q[1], -q[2], q[3]]);
    let s_main = main_sign(engine_stereo, main_is_right, eye_sign);
    let main = dot_for_eye(&mut state, s_main);
    let main_ok = main.is_some();
    let mut sc = main.unwrap_or([0.0; 2]);
    let sc_right = dot_for_eye(&mut state, -s_main).unwrap_or(sc);
    let world_ok = main_ok && world != 0;
    {
        let now = i32::from(world_ok);
        if now != state.said {
            state.said = now;
            // SAFETY: C++ globals.
            let (ndc_x, ndc_x2) = unsafe { (get(core::ptr::addr_of!(CyberpunkVR_BarrelDotNdcX)), get(core::ptr::addr_of!(CyberpunkVR_BarrelDotNdcX2))) };
            cvr_host::log::write(&format!(
                "BarrelDot: worldPath={now} why={:.0} ly=({:.3} {:.3}) v=({:.3} {:.3} {:.3}) fwd=({:.3} {:.3} {:.3}) D={distance:.1} \
                 ndc=({ndc_x:.5} {ndc_x2:.5}) muzzle=({:.3} {:.3} {:.3}) head=({:.3} {:.3} {:.3}) halfIpd={half:.4}\n",
                state.dbg_why, state.dbg_ly[0], state.dbg_ly[1], v[0], v[1], v[2], mf[0], mf[1], mf[2], mp[0], mp[1], mp[2], hc[0], hc[1], hc[2]
            ));
        }
    }
    drop(state);
    // With world data present the direction path is not a fallback, it is a wrong answer.
    if world != 0 && have_world && !world_ok {
        return;
    }
    if !world_ok {
        match project([v[0], v[2], -v[1]]) {
            Some(at) => sc = at,
            None => return,
        }
    }
    let [ndc_x, ndc_y] = ndc(sc, display);
    // The second eye: its own ray, or the zero distance's parallax; carried across the asymmetric windows.
    let (mut thx, mut thy) = (0.0f32, 0.0f32);
    let tans = zero_m > 0.1 && (r.proj_tans)(display[0], display[1], &mut thx, &mut thy);
    let dx = parallax(zero_m, tans.then_some(thx), if tans { (r.runtime_ipd)() } else { 0.0 }, main_is_right);
    let mut ndc_x2 = if world_ok { ndc(sc_right, display)[0] } else { ndc_x + dx };
    let main_eye = usize::from(main_is_right);
    let mut windows = [EyeWindow::default(); 2];
    if (r.asym_windows)(&mut windows) {
        ndc_x2 = map_ndc_x(ndc_x2, &windows[main_eye], &windows[1 - main_eye]);
    }
    let ndc_x2 = ndc_x2 + off_x2;
    // SAFETY: C++ globals (the second-eye pass reads them).
    unsafe {
        set(core::ptr::addr_of_mut!(CyberpunkVR_BarrelDotNdcX), ndc_x);
        set(core::ptr::addr_of_mut!(CyberpunkVR_BarrelDotNdcY), ndc_y);
        set(core::ptr::addr_of_mut!(CyberpunkVR_BarrelDotNdcX2), ndc_x2);
        set(core::ptr::addr_of_mut!(CyberpunkVR_BarrelDotRadiusPx), RADIUS);
        set(core::ptr::addr_of_mut!(CyberpunkVR_BarrelDotTick), windows_sys::Win32::System::SystemInformation::GetTickCount64());
    }
    let Some(dl) = background_draw_list_handle() else { return };
    if !engine_stereo {
        dl.circle_filled(sc, RADIUS, RED);
        return;
    }
    let mut second_y = if world_ok { 1.0 - 2.0 * sc_right[1] / display[1] } else { ndc_y };
    if (r.asym_windows)(&mut windows) {
        second_y = map_ndc_y(second_y, &windows[main_eye], &windows[1 - main_eye]);
    }
    for eye in 0..2usize {
        let point = if eye == main_eye { [ndc_x, ndc_y] } else { [ndc_x2, second_y] };
        let left = eye as f32 * display[0];
        dl.push_clip_rect([left, 0.0], [left + display[0], display[1]], true);
        dl.circle_filled(eye_pixel(point, eye as i32, display), RADIUS, RED);
        dl.pop_clip_rect();
    }
}
