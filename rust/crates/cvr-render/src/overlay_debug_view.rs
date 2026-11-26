//! THE COLLISION VIEW in the overlay (main's src/Overlay/OverlayDebugDraw.cpp, `DrawDebugView`): the view's lines
//! (cvr-rig's debug_view, read through cvr_debug_view_copy), each projected from each eye's own position, the way the
//! barrel dot is, so that it stands in the world in both eyes; cut at a near plane 5 cm ahead, drawn 1.2 px on the
//! background list in the view's palette.
use crate::eye_windows as ew;
use crate::imgui::{background_draw_list_handle, Io};
use crate::overlay_exports::CyberpunkVR_BarrelDotEyeSign;
use cvr_camera::barrel_dot::{camera_right, half_ipd, head_center, main_sign};
use cvr_camera::overlay_projection::rotate;
use cvr_camera::projection::{map_ndc_x, map_ndc_y, EyeWindow};

/// cvr-rig's debug_view::view::Segment.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Segment {
    a: [f32; 3],
    b: [f32; 3],
    color: u32,
}
const MAX_SEGMENTS: usize = 12000;
/// camera::BarrelFrame.
#[repr(C)]
#[derive(Default)]
struct BarrelFrame {
    cam_quat: [f32; 4],
    muzzle_fwd: [f32; 3],
}
#[allow(non_upper_case_globals)]
extern "C" {
    fn cvr_debug_view_copy(out: *mut Segment, capacity: i32) -> i32;
    fn CyberpunkVRPort_HalfIpd() -> f32;
    fn cvr_camera_barrel_frame_read(out: *mut BarrelFrame) -> bool;
    static g_lastLocatePosFP: [i32; 3];
    static CyberpunkVR_MainIsRightEye: i32;
}
const HALF_IPD_SLOT: i32 = 95;
const fn col(r: u32, g: u32, b: u32, a: u32) -> u32 {
    (a << 24) | (b << 16) | (g << 8) | r
}
/// The view's colours by index: white, green, red, blue, yellow, cyan, magenta, orange, the world's statics, its
/// dynamics.
const PALETTE: [u32; 10] = [
    col(255, 255, 255, 255),
    col(60, 255, 90, 200),
    col(255, 60, 60, 230),
    col(90, 140, 255, 230),
    col(255, 230, 60, 230),
    col(60, 240, 240, 200),
    col(255, 70, 230, 220),
    col(255, 150, 40, 255),
    col(170, 175, 195, 110),
    col(255, 215, 70, 220),
];
const NEAR_PLANE: f32 = 0.05;
static SEGMENTS: std::sync::Mutex<Vec<Segment>> = std::sync::Mutex::new(Vec::new());

/// DrawDebugView.
#[no_mangle]
pub extern "C" fn cvr_overlay_debug_view() {
    let mut segments = SEGMENTS.lock().unwrap_or_else(|e| e.into_inner());
    if segments.len() != MAX_SEGMENTS {
        segments.resize(MAX_SEGMENTS, Segment::default());
    }
    // SAFETY: the buffer holds MAX_SEGMENTS.
    let count = unsafe { cvr_debug_view_copy(segments.as_mut_ptr(), MAX_SEGMENTS as i32) };
    if count <= 0 {
        return;
    }
    let mut bf = BarrelFrame::default();
    // SAFETY: a plain read into a local.
    if !unsafe { cvr_camera_barrel_frame_read(&mut bf) } {
        return;
    }
    // SAFETY: the camera hook's published head (a C++ global, volatile).
    let fixed = unsafe {
        let head = core::ptr::addr_of!(g_lastLocatePosFP).cast::<i32>();
        [core::ptr::read_volatile(head), core::ptr::read_volatile(head.add(1)), core::ptr::read_volatile(head.add(2))]
    };
    let (hc, real) = head_center(fixed);
    if !real {
        return;
    }
    let engine_stereo = ew::engine_stereo();
    if engine_stereo && ew::one_picture() {
        return;
    }
    let back = Io::get().display_size();
    let display = [if engine_stereo { back[0] * 0.5 } else { back[0] }, back[1]];
    if display[0] <= 1.0 || display[1] <= 1.0 {
        return;
    }
    let Some(dl) = background_draw_list_handle() else { return };
    let q = bf.cam_quat;
    let rgt = camera_right(q);
    // SAFETY: a plain call.
    let half = half_ipd(unsafe { CyberpunkVRPort_HalfIpd() }, ew::shared_slot(HALF_IPD_SLOT));
    // SAFETY: C++ globals (dllexport live controls).
    let (main_is_right, eye_sign) = unsafe {
        (core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_MainIsRightEye)) != 0, core::ptr::read_volatile(core::ptr::addr_of!(CyberpunkVR_BarrelDotEyeSign)))
    };
    let main_eye = usize::from(main_is_right);
    let s_main = main_sign(engine_stereo, main_is_right, eye_sign);
    let mut windows = [EyeWindow::default(); 2];
    let mapped = engine_stereo && ew::asym_windows(&mut windows);
    let inverse = [-q[0], -q[1], -q[2], q[3]];
    let eyes: Vec<usize> = if engine_stereo { vec![0, 1] } else { vec![main_eye] };
    for eye in eyes {
        let sign = if eye == main_eye { s_main } else { -s_main };
        let e = [hc[0] + rgt[0] * half * sign, hc[1] + rgt[1] * half * sign, hc[2] + rgt[2] * half * sign];
        let left = if engine_stereo { eye as f32 * display[0] } else { 0.0 };
        dl.push_clip_rect([left, 0.0], [left + display[0], display[1]], true);
        let to_screen = |l: [f32; 3]| -> Option<[f32; 2]> {
            let mut sc = [0.0f32; 2];
            if !ew::project(l[0], l[2], -l[1], display[0], display[1], &mut sc) {
                return None;
            }
            if mapped && eye != main_eye {
                let mut nx = (sc[0] / display[0]) * 2.0 - 1.0;
                let mut ny = 1.0 - (sc[1] / display[1]) * 2.0;
                nx = map_ndc_x(nx, &windows[main_eye], &windows[1 - main_eye]);
                ny = map_ndc_y(ny, &windows[main_eye], &windows[1 - main_eye]);
                sc = [(nx * 0.5 + 0.5) * display[0], (0.5 - ny * 0.5) * display[1]];
            }
            Some([left + sc[0], sc[1]])
        };
        for s in &segments[..count as usize] {
            let mut a = rotate([s.a[0] - e[0], s.a[1] - e[1], s.a[2] - e[2]], inverse);
            let mut b = rotate([s.b[0] - e[0], s.b[1] - e[1], s.b[2] - e[2]], inverse);
            // the camera's frame is X right, Y forward, Z up
            if a[1] < NEAR_PLANE && b[1] < NEAR_PLANE {
                continue;
            }
            if a[1] < NEAR_PLANE {
                let t = (NEAR_PLANE - a[1]) / (b[1] - a[1]);
                a = [a[0] + (b[0] - a[0]) * t, NEAR_PLANE, a[2] + (b[2] - a[2]) * t];
            } else if b[1] < NEAR_PLANE {
                let t = (NEAR_PLANE - b[1]) / (a[1] - b[1]);
                b = [b[0] + (a[0] - b[0]) * t, NEAR_PLANE, b[2] + (a[2] - b[2]) * t];
            }
            let (Some(pa), Some(pb)) = (to_screen(a), to_screen(b)) else { continue };
            dl.line(pa, pb, PALETTE[if (s.color as usize) < PALETTE.len() { s.color as usize } else { 0 }], 1.2);
        }
        dl.pop_clip_rect();
    }
}
