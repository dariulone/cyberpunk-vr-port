//! The hand locator (src/Overlay/OverlayDebugDraw.cpp, `DrawHandLocatorOverlay`): where the plugin thinks each hand
//! is, drawn on the background list (projected for one eye, so the second-eye pass skips it) -- the Im3d hand proxy
//! when asked for, the wire model (cvr-camera's hand_wire) with its centre, name and axis labels when the proxy is off
//! or the debug axes are on, and the laser sight down each hand's pointing axis (forty short segments to the ray's
//! length, so it still draws when part of it falls behind the eye, and an aim dot at 4 m).
//!
//! The Rust build draws it here, the Im3d proxy's lists included (RenderIm3dToDrawList: cvr-camera's im3d_shapes), from
//! the published hand frame and the overlay's projection; the C++ half keeps Im3d's calls (the library) as services.

use crate::imgui::{background_draw_list_handle, DrawList, Io};
use core::ffi::CStr;
use cvr_camera::hand_wire::{labels, wire};
use cvr_camera::overlay_projection::{im3d_shapes, Im3dShape, Im3dVertex};

/// Im3d::DrawList.
#[repr(C)]
pub struct Im3dList {
    layer: u32,
    primitive: u32,
    vertices: *const Im3dVertex,
    count: u32,
}
const _: () = assert!(core::mem::size_of::<Im3dList>() == 24 && core::mem::size_of::<Im3dVertex>() == 20);

/// What the locator reads of the C++ half; a hand is 0 left, 1 right.
#[repr(C)]
pub struct LocatorRuntime {
    /// GetPublishedHandFrame into the C++ half's frame: bit 0 the left hand valid, bit 1 the right (0 none).
    frame: extern "C" fn() -> u32,
    /// ProjectHandLocalPoint for a hand of that frame.
    project_local: extern "C" fn(i32, f32, f32, f32, f32, f32, *mut [f32; 2]) -> bool,
    /// AbstractHandPointToHeadSpace(0, t, 0) then ProjectIm3dPointToScreen.
    laser_point: extern "C" fn(i32, f32, f32, f32, *mut [f32; 2]) -> bool,
    /// Im3d's frame: the app data for this display and NewFrame; EmitHandProxyIm3d (its geometry cvr-camera's
    /// hand_proxy); EndFrame; the frame's draw lists (their count through the pointer).
    im3d_begin: extern "C" fn(f32, f32),
    emit_proxy: extern "C" fn(i32, f32, bool),
    im3d_end: extern "C" fn(),
    im3d_lists: extern "C" fn(*mut u32) -> *const Im3dList,
    /// ProjectHeadSpacePointToScreen.
    project_head: extern "C" fn(f32, f32, f32, f32, f32, *mut [f32; 2]) -> bool,
    /// The overlay's switches.
    show: *const bool,
    scale: *const f32,
    proxy3d: *const bool,
    debug_axes: *const bool,
    aim_ray: *const bool,
    aim_ray_length: *const f32,
}
// SAFETY: functions and the overlay's switches, which live for the process; read only.
unsafe impl Sync for LocatorRuntime {}

/// IM_COL32.
const fn col32(r: u32, g: u32, b: u32, a: u32) -> u32 {
    (a << 24) | (b << 16) | (g << 8) | r
}
const WHITE: u32 = col32(255, 255, 255, 255);
const LASER_SEGMENTS: i32 = 40;

/// A C++ switch, read as the .cpp reads it.
///
/// # Safety
/// `value` is a live global.
unsafe fn get<T: Copy>(value: *const T) -> T {
    unsafe { core::ptr::read_volatile(value) }
}

/// DrawHandLocatorOverlay.
#[no_mangle]
pub extern "C" fn cvr_overlay_hand_locator() {
    let r = &table::RT;
    // SAFETY: the overlay's switch.
    if !unsafe { get(r.show) } {
        return;
    }
    let hands = (r.frame)();
    let (has_left, has_right) = (hands & 1 != 0, hands & 2 != 0);
    if !has_left && !has_right {
        return;
    }
    let display = Io::get().display_size();
    if display[0] <= 1.0 || display[1] <= 1.0 {
        return;
    }
    let Some(dl) = background_draw_list_handle() else { return };
    (r.im3d_begin)(display[0], display[1]);
    let project = |hand: i32, p: [f32; 3]| -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        (r.project_local)(hand, p[0], p[1], p[2], display[0], display[1], &mut out).then_some(out)
    };
    let draw_wire = |hand: i32, body: u32, name: &CStr, left: bool| {
        // SAFETY: the overlay's scale.
        let s = unsafe { get(r.scale) };
        for bone in wire(s, left, body) {
            let Some(a) = project(hand, bone.a) else { continue };
            let Some(b) = project(hand, bone.b) else { continue };
            dl.line(a, b, bone.color, bone.thickness);
        }
        if let Some(centre) = project(hand, [0.0; 3]) {
            dl.circle_filled(centre, 4.0, body);
            dl.text_default([centre[0] + 8.0, centre[1] - 20.0], body, name);
        }
        for label in labels(s) {
            let Some(at) = project(hand, label.at) else { continue };
            let text = [label.text, 0];
            let text = CStr::from_bytes_with_nul(&text).unwrap_or(c"?");
            dl.text_default([at[0] + label.offset[0], at[1] + label.offset[1]], WHITE, text);
        }
    };
    // SAFETY: the overlay's switches.
    let (proxy3d, debug_axes) = unsafe { (get(r.proxy3d), get(r.debug_axes)) };
    for (has, hand, left, body, name) in [(has_left, 0, true, col32(0, 220, 255, 255), c"LEFT"), (has_right, 1, false, col32(255, 180, 0, 255), c"RIGHT")] {
        if !has {
            continue;
        }
        if proxy3d {
            // SAFETY: the overlay's scale.
            (r.emit_proxy)(hand, unsafe { get(r.scale) }, debug_axes);
        }
        if debug_axes || !proxy3d {
            draw_wire(hand, body, name, left);
        }
    }
    (r.im3d_end)();
    if proxy3d {
        // RenderIm3dToDrawList onto the background list.
        let mut count = 0u32;
        let lists = (r.im3d_lists)(&mut count);
        if !lists.is_null() {
            // SAFETY: Im3d's draw lists of the frame just ended.
            for list in unsafe { core::slice::from_raw_parts(lists, count as usize) } {
                if list.vertices.is_null() || list.count == 0 {
                    continue;
                }
                // SAFETY: the list's vertices.
                let vertices = unsafe { core::slice::from_raw_parts(list.vertices, list.count as usize) };
                let mut project = |p: [f32; 3]| {
                    let mut out = [0.0f32; 2];
                    (r.project_head)(p[0], p[1], p[2], display[0], display[1], &mut out).then_some(out)
                };
                im3d_shapes(list.primitive, vertices, &mut project, &mut |shape| match shape {
                    Im3dShape::Triangle(a, b, c, color) => dl.triangle_filled(a, b, c, color),
                    Im3dShape::Circle(p, radius, color) => dl.circle_filled(p, radius, color),
                });
            }
        }
    }
    // SAFETY: the overlay's switches.
    if unsafe { get(r.aim_ray) } {
        // SAFETY: the overlay's length.
        let length = unsafe { get(r.aim_ray_length) };
        let max_len = if length > 0.5 { length } else { 8.0 };
        let laser = |dl: DrawList, hand: i32, color: u32, thickness: f32| {
            let mut previous: Option<[f32; 2]> = None;
            for i in 0..=LASER_SEGMENTS {
                let t = (i as f32 / LASER_SEGMENTS as f32) * max_len;
                let mut at = [0.0f32; 2];
                if (r.laser_point)(hand, t, display[0], display[1], &mut at) {
                    if let Some(p) = previous {
                        dl.line(p, at, color, thickness);
                    }
                    previous = Some(at);
                } else {
                    previous = None;
                }
            }
            let mut dot = [0.0f32; 2];
            if (r.laser_point)(hand, 4.0, display[0], display[1], &mut dot) {
                dl.circle_filled(dot, 6.0, color);
                dl.circle(dot, 10.0, WHITE, 2.0);
            }
        };
        if has_right {
            laser(dl, 1, col32(80, 255, 80, 255), 3.0);
        }
        if has_left {
            laser(dl, 0, col32(0, 200, 255, 160), 2.0);
        }
    }
}

// ---- the locator's table (formerly src/Overlay/OverlayDebugDraw.cpp's, with OverlayProjection.cpp's projection
// helpers it called): the published hand frame from cvr-xr, the projection from cvr-camera's overlay projection and the
// overlay's window; Im3d (the library) stays the C++ half's four services
mod table {
    use super::{Im3dList, LocatorRuntime};
    use core::cell::UnsafeCell;
    use cvr_camera::overlay_projection::{hand_proxy, map_hand, project_head, rotate, ProxyVertex};
    use cvr_xr::tracking::HeadPose;

    unsafe extern "C" {
        /// Im3d's app data for this display (the head at the origin looking down -z, y up, 90 Hz, perspective with
        /// the given vertical tan) and NewFrame.
        safe fn cvr_cpp_im3d_begin(width: f32, height: f32, proj_scale_y: f32);
        /// The hand proxy's seven boxes as triangles and, when asked, its axes as 2-pixel lines.
        fn cvr_cpp_im3d_proxy(vertices: *const ProxyVertex, axes: bool);
        /// Im3d::EndFrame.
        safe fn cvr_cpp_im3d_end();
        /// The frame's draw lists and their count.
        fn cvr_cpp_im3d_lists(count: *mut u32) -> *const Im3dList;
    }

    /// The locator's frame (locatorFrame): the overlay thread's alone.
    struct Frame {
        head: HeadPose,
        hands: [HeadPose; 2],
    }
    struct Held(UnsafeCell<Frame>);
    // SAFETY: only the overlay's draw reads and writes it, one call at a time.
    unsafe impl Sync for Held {}
    // SAFETY: HeadPose is plain data; all zero is the C++ value-initialised pose.
    static FRAME: Held = Held(UnsafeCell::new(unsafe { core::mem::zeroed() }));
    fn hand(index: i32) -> HeadPose {
        // SAFETY: as above; the hand is 0 or 1.
        unsafe { (*FRAME.0.get()).hands[(index & 1) as usize] }
    }

    /// ProjectHeadSpacePointToScreen: the overlay's window for this display, then the head-space projection.
    fn project_head_space(p: [f32; 3], width: f32, height: f32, out: *mut [f32; 2]) -> bool {
        if out.is_null() || width <= 1.0 || height <= 1.0 {
            return false;
        }
        let Some(window) = crate::eye_windows::overlay_window(width, height) else { return false };
        match project_head(p, width, height, &window) {
            Some(screen) => {
                // SAFETY: the caller's output.
                unsafe { *out = screen };
                true
            }
            None => false,
        }
    }
    fn offset(pose: &HeadPose, v: [f32; 3]) -> [f32; 3] {
        let d = rotate(v, pose.ori);
        [pose.pos[0] + d[0], pose.pos[1] + d[1], pose.pos[2] + d[2]]
    }

    extern "C" fn frame() -> u32 {
        // SAFETY: the frame (as above) and cvr-xr's published poses.
        let f = unsafe { &mut *FRAME.0.get() };
        // SAFETY: a head and two hands to fill, no sequence or stamp asked for.
        if !unsafe { cvr_xr::published::cvr_xr_published_hand_frame(&mut f.head, f.hands.as_mut_ptr(), 0, core::ptr::null_mut(), core::ptr::null_mut()) } {
            return 0;
        }
        (f.hands[0].valid as u32) | ((f.hands[1].valid as u32) << 1)
    }
    /// ProjectHandLocalPoint: a point in the hand's frame, rotated and placed by the hand, projected from head space.
    extern "C" fn project_local(index: i32, x: f32, y: f32, z: f32, width: f32, height: f32, out: *mut [f32; 2]) -> bool {
        project_head_space(offset(&hand(index), [x, y, z]), width, height, out)
    }
    /// AbstractHandPointToHeadSpace(hand, left, 0, t, 0) then ProjectIm3dPointToScreen.
    extern "C" fn laser_point(index: i32, t: f32, width: f32, height: f32, out: *mut [f32; 2]) -> bool {
        let pose = hand(index);
        project_head_space(offset(&pose, map_hand(index == 0, [0.0, t, 0.0])), width, height, out)
    }
    extern "C" fn im3d_begin(width: f32, height: f32) {
        // the same game-render vertical tan as the point projectors, so Im3d's size attenuation agrees with them
        let mut tan_y = (50.0f32 * (3.1415926535f32 / 180.0)).tan();
        if width > 1.0 && height > 1.0 {
            if let Some(window) = crate::eye_windows::overlay_window(width, height) {
                tan_y = window.tan_y;
            }
        }
        cvr_cpp_im3d_begin(width, height, tan_y);
    }
    /// EmitHandProxyIm3d.
    extern "C" fn emit_proxy(index: i32, scale: f32, axes: bool) {
        let pose = hand(index);
        let mut h = [0.0f32; 7];
        h[..3].copy_from_slice(&pose.pos);
        h[3..].copy_from_slice(&pose.ori);
        let vertices = hand_proxy(h, index == 0, scale);
        // SAFETY: the proxy's full vertex set.
        unsafe { cvr_cpp_im3d_proxy(vertices.as_ptr(), axes) };
    }
    extern "C" fn im3d_end() {
        cvr_cpp_im3d_end();
    }
    extern "C" fn im3d_lists(count: *mut u32) -> *const Im3dList {
        // SAFETY: the caller's count.
        unsafe { cvr_cpp_im3d_lists(count) }
    }
    extern "C" fn project_head_point(x: f32, y: f32, z: f32, width: f32, height: f32, out: *mut [f32; 2]) -> bool {
        project_head_space([x, y, z], width, height, out)
    }

    use crate::moved_globals::{g_aimRayLenM, g_drawAimRay, g_drawHandDebugAxes, g_drawHandLocator, g_drawHandProxy3D, g_handLocatorScale};
    pub(super) static RT: LocatorRuntime = LocatorRuntime {
        frame,
        project_local,
        laser_point,
        im3d_begin,
        emit_proxy,
        im3d_end,
        im3d_lists,
        project_head: project_head_point,
        show: (&raw const g_drawHandLocator).cast::<bool>(),
        scale: (&raw const g_handLocatorScale).cast::<f32>(),
        proxy3d: (&raw const g_drawHandProxy3D).cast::<bool>(),
        debug_axes: (&raw const g_drawHandDebugAxes).cast::<bool>(),
        aim_ray: (&raw const g_drawAimRay).cast::<bool>(),
        aim_ray_length: (&raw const g_aimRayLenM).cast::<f32>(),
    };
}
