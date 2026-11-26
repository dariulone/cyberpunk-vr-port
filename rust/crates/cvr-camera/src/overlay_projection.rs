//! The overlay's projection math (src/Overlay/OverlayProjection.cpp, `overlay::`): an abstract hand point mapped into
//! the controller's frame (x = -z for the left hand, z for the right; y = -y; z = -x), a vector rotated by a unit
//! quaternion, the overlay's projection window (the main eye's own off-centre window with the asymmetric projection,
//! otherwise the symmetric frustum of the game's horizontal FOV -- else the runtime's, else 100 degrees -- with the
//! vertical tangent from the back buffer's aspect; both tangents divided by any finite ADS zoom above 0.05), and a
//! head-space or world point projected onto the back buffer through it (at least 0.01 m in front of the eye).
//!
//! In the Rust build (`CVR_RUST_VRIK`) MapAbstractHandPoint, RotateVectorByQuaternion, GetOverlayProjWindow's arithmetic
//! and the two point projections forward to `cvr_ovp_*` (the .cpp still reads the FOVs, the asymmetric windows and the
//! zoom); the unit tests below cover them. The hand proxy's geometry (EmitHandProxyIm3d: its boxes and axes through the
//! hand, in Im3d's colours) and what RenderIm3dToDrawList draws of Im3d's lists are here too (`hand_proxy`,
//! `im3d_shapes`).

/// MapAbstractHandPoint.
pub fn map_hand(left: bool, h: [f32; 3]) -> [f32; 3] {
    [if left { -h[2] } else { h[2] }, -h[1], -h[0]]
}
/// RotateVectorByQuaternion (q = x, y, z, w).
pub fn rotate(v: [f32; 3], q: [f32; 4]) -> [f32; 3] {
    let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
    let tx = 2.0f32 * (qy * v[2] - qz * v[1]);
    let ty = 2.0f32 * (qz * v[0] - qx * v[2]);
    let tz = 2.0f32 * (qx * v[1] - qy * v[0]);
    [v[0] + qw * tx + (qy * tz - qz * ty), v[1] + qw * ty + (qz * tx - qx * tz), v[2] + qw * tz + (qx * ty - qy * tx)]
}

/// The projection window: half tangents and the straight-ahead NDC centre.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Window {
    pub tan_x: f32,
    pub tan_y: f32,
    pub centre_x: f32,
    pub centre_y: f32,
}

/// GetOverlayProjWindow's arithmetic: `asymmetric` the main eye's window (l, r, u, d) when the asymmetric projection is
/// on, else the game's (`game_fov`) and runtime's (`runtime_fov`) horizontal FOVs in degrees.
pub fn window(width: f32, height: f32, asymmetric: Option<[f32; 4]>, game_fov: f32, runtime_fov: f32, ads: f32) -> Option<Window> {
    if width <= 1.0 || height <= 1.0 {
        return None;
    }
    let mut w = Window::default();
    let (mut tx, mut ty);
    if let Some([l, r, u, d]) = asymmetric {
        tx = 0.5f32 * (r - l);
        ty = 0.5f32 * (u - d);
        w.centre_x = -(r + l) / (r - l);
        w.centre_y = -(u + d) / (u - d);
    } else {
        let mut fov = game_fov;
        if fov <= 1.0 {
            fov = runtime_fov;
        }
        if fov <= 1.0 {
            fov = 100.0;
        }
        tx = ((fov * 0.5f32) * (3.1415926535f32 / 180.0f32)).tan();
        ty = tx * (height / width);
    }
    if ads.is_finite() && ads > 0.05 {
        tx /= ads;
        ty /= ads;
    }
    if tx <= 0.0001 || ty <= 0.0001 {
        return None;
    }
    w.tan_x = tx;
    w.tan_y = ty;
    Some(w)
}
/// A head-space point projected onto the back buffer (`width` x `height`) through `w`.
pub fn project_head(p: [f32; 3], width: f32, height: f32, w: &Window) -> Option<[f32; 2]> {
    if width <= 1.0 || height <= 1.0 {
        return None;
    }
    let forward = -p[2];
    if forward <= 0.01 {
        return None;
    }
    let ndc_x = p[0] / (forward * w.tan_x) + w.centre_x;
    let ndc_y = p[1] / (forward * w.tan_y) + w.centre_y;
    Some([(ndc_x * 0.5f32 + 0.5f32) * width, (-ndc_y * 0.5f32 + 0.5f32) * height])
}
/// A world point projected through the head pose (position, orientation x, y, z, w).
pub fn project_world(head: [f32; 7], p: [f32; 3], width: f32, height: f32, w: &Window) -> Option<[f32; 2]> {
    let delta = [p[0] - head[0], p[1] - head[1], p[2] - head[2]];
    let view = rotate(delta, [-head[3], -head[4], -head[5], head[6]]);
    project_head(view, width, height, w)
}

// ---- the C ABI

/// # Safety
/// `out` has room for three floats.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ovp_map_hand(left: bool, hx: f32, hy: f32, hz: f32, out: *mut f32) {
    let v = map_hand(left, [hx, hy, hz]);
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), out, 3) };
}
/// # Safety
/// `out` has room for three floats.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ovp_rotate(vx: f32, vy: f32, vz: f32, qx: f32, qy: f32, qz: f32, qw: f32, out: *mut f32) {
    let v = rotate([vx, vy, vz], [qx, qy, qz, qw]);
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), out, 3) };
}
/// `asymmetric` (l, r, u, d) or null.
///
/// # Safety
/// `asymmetric` is null or holds four floats; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ovp_window(width: f32, height: f32, asymmetric: *const [f32; 4], game_fov: f32, runtime_fov: f32, ads: f32, out: *mut Window) -> bool {
    match window(width, height, unsafe { asymmetric.as_ref() }.copied(), game_fov, runtime_fov, ads) {
        Some(w) => {
            unsafe { *out = w };
            true
        }
        None => false,
    }
}
/// A head-space point (`world` false) or a world point through `head` (position, orientation x, y, z, w).
///
/// # Safety
/// `head` holds seven floats when `world`; `w` is valid; `out` has room for two floats.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_ovp_project(world: bool, head: *const [f32; 7], px: f32, py: f32, pz: f32, width: f32, height: f32, w: *const Window, out: *mut f32) -> bool {
    let w = unsafe { &*w };
    let screen = if world { project_world(unsafe { *head }, [px, py, pz], width, height, w) } else { project_head([px, py, pz], width, height, w) };
    match screen {
        Some(s) => {
            unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), out, 2) };
            true
        }
        None => false,
    }
}

// ---- the Im3d hand proxy and the Im3d lists on the overlay (EmitHandProxyIm3d, RenderIm3dToDrawList)

/// Im3d::Color(r, g, b, a): rgba8 with r the most significant byte, each channel x 255 truncated.
pub fn im3d_color(r: f32, g: f32, b: f32, a: f32) -> u32 {
    ((r * 255.0f32) as u32) << 24 | ((g * 255.0f32) as u32) << 16 | ((b * 255.0f32) as u32) << 8 | (a * 255.0f32) as u32
}
/// Color::getABGR: the ImGui colour (IM_COL32) of an Im3d one.
pub fn im3d_abgr(v: u32) -> u32 {
    (v & 0xff00_0000) >> 24 | (v & 0x00ff_0000) >> 8 | (v & 0x0000_ff00) << 8 | (v & 0x0000_00ff) << 24
}
/// Im3d's Color_Red, Color_Green and Color_Blue.
pub const IM3D_RED: u32 = 0xff00_00ff;
pub const IM3D_GREEN: u32 = 0x00ff_00ff;
pub const IM3D_BLUE: u32 = 0x0000_ffff;
/// AbstractHandPointToHeadSpace: an abstract hand point through the hand's pose (position, orientation x, y, z, w).
pub fn hand_point(hand: [f32; 7], left: bool, h: [f32; 3]) -> [f32; 3] {
    let d = rotate(map_hand(left, h), [hand[3], hand[4], hand[5], hand[6]]);
    [hand[0] + d[0], hand[1] + d[1], hand[2] + d[2]]
}

/// One vertex of the hand proxy: head space and its Im3d colour.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProxyVertex {
    pub p: [f32; 3],
    pub color: u32,
}
/// The proxy's boxes -- the palm, four fingers, the thumb as two volumes -- each six quads as two triangles.
pub const PROXY_BOXES: usize = 7;
pub const PROXY_BOX_VERTICES: usize = 36;
/// The debug axes: three lines (x red, y green, the pointing -z blue).
pub const PROXY_AXIS_VERTICES: usize = 6;
pub const PROXY_VERTICES: usize = PROXY_BOXES * PROXY_BOX_VERTICES + PROXY_AXIS_VERTICES;

/// EmitHandProxyIm3d's geometry at `scale`: each box's corners through the hand (EmitHandBox) and its quads as
/// EmitIm3dQuad's triangles (a, b, c and a, c, d) in the box's colour -- cyan shades for the left hand, amber for the
/// right -- then the axes' line ends.
pub fn hand_proxy(hand: [f32; 7], left: bool, scale: f32) -> [ProxyVertex; PROXY_VERTICES] {
    let s = scale;
    let (palm, finger, thumb) = if left {
        (im3d_color(0.10, 0.85, 1.00, 0.32), im3d_color(0.35, 0.92, 1.00, 0.50), im3d_color(0.22, 0.72, 1.00, 0.55))
    } else {
        (im3d_color(1.00, 0.72, 0.15, 0.32), im3d_color(1.00, 0.86, 0.35, 0.50), im3d_color(1.00, 0.58, 0.22, 0.55))
    };
    // (min, max, colour)
    let boxes: [([f32; 3], [f32; 3], u32); PROXY_BOXES] = [
        ([-0.034 * s, -0.052 * s, -0.014 * s], [0.034 * s, 0.044 * s, 0.014 * s], palm),
        ([-0.028 * s, 0.044 * s, -0.010 * s], [-0.018 * s, 0.102 * s, 0.010 * s], finger),
        ([-0.012 * s, 0.044 * s, -0.010 * s], [-0.002 * s, 0.116 * s, 0.010 * s], finger),
        ([0.004 * s, 0.044 * s, -0.010 * s], [0.014 * s, 0.128 * s, 0.010 * s], finger),
        ([0.020 * s, 0.044 * s, -0.010 * s], [0.030 * s, 0.112 * s, 0.010 * s], finger),
        ([0.018 * s, -0.004 * s, -0.010 * s], [0.046 * s, 0.018 * s, 0.010 * s], thumb),
        ([0.042 * s, 0.012 * s, -0.010 * s], [0.068 * s, 0.036 * s, 0.010 * s], thumb),
    ];
    let point = |x: f32, y: f32, z: f32| hand_point(hand, left, [x, y, z]);
    let mut out = [ProxyVertex::default(); PROXY_VERTICES];
    for (index, &(lo, hi, color)) in boxes.iter().enumerate() {
        let p000 = point(lo[0], lo[1], lo[2]);
        let p100 = point(hi[0], lo[1], lo[2]);
        let p110 = point(hi[0], hi[1], lo[2]);
        let p010 = point(lo[0], hi[1], lo[2]);
        let p001 = point(lo[0], lo[1], hi[2]);
        let p101 = point(hi[0], lo[1], hi[2]);
        let p111 = point(hi[0], hi[1], hi[2]);
        let p011 = point(lo[0], hi[1], hi[2]);
        let quads = [
            [p000, p100, p110, p010],
            [p001, p011, p111, p101],
            [p000, p001, p101, p100],
            [p010, p110, p111, p011],
            [p000, p010, p011, p001],
            [p100, p101, p111, p110],
        ];
        for (q, [a, b, c, d]) in quads.into_iter().enumerate() {
            for (k, p) in [a, b, c, a, c, d].into_iter().enumerate() {
                out[index * PROXY_BOX_VERTICES + q * 6 + k] = ProxyVertex { p, color };
            }
        }
    }
    let axes = [
        (point(-0.060 * s, 0.0, 0.0), IM3D_RED),
        (point(0.060 * s, 0.0, 0.0), IM3D_RED),
        (point(0.0, -0.060 * s, 0.0), IM3D_GREEN),
        (point(0.0, 0.060 * s, 0.0), IM3D_GREEN),
        (point(0.0, 0.0, 0.0), IM3D_BLUE),
        (point(0.0, 0.0, -0.180 * s), IM3D_BLUE),
    ];
    for (k, (p, color)) in axes.into_iter().enumerate() {
        out[PROXY_BOXES * PROXY_BOX_VERTICES + k] = ProxyVertex { p, color };
    }
    out
}

/// An Im3d vertex (Im3d::VertexData: the position and size, the rgba8 colour).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Im3dVertex {
    pub position_size: [f32; 4],
    pub color: u32,
}
/// What RenderIm3dToDrawList draws: filled triangles and filled circles, in ImGui colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Im3dShape {
    Triangle([f32; 2], [f32; 2], [f32; 2], u32),
    Circle([f32; 2], f32, u32),
}
/// RenderIm3dToDrawList for one Im3d draw list (`primitive` 0 triangles, 1 lines, 2 points), its vertices projected by
/// `project`: each whole triangle whose three corners project, in its first vertex's colour; each point that projects
/// as a circle of a tenth of its size (at least 1); the lines draw nothing (their AddLine is off in the .cpp).
pub fn im3d_shapes(primitive: u32, vertices: &[Im3dVertex], project: &mut impl FnMut([f32; 3]) -> Option<[f32; 2]>, out: &mut impl FnMut(Im3dShape)) {
    let at = |v: &Im3dVertex| [v.position_size[0], v.position_size[1], v.position_size[2]];
    match primitive {
        0 => {
            let mut i = 0;
            while i + 2 < vertices.len() {
                let t = &vertices[i..i + 3];
                i += 3;
                let Some(a) = project(at(&t[0])) else { continue };
                let Some(b) = project(at(&t[1])) else { continue };
                let Some(c) = project(at(&t[2])) else { continue };
                out(Im3dShape::Triangle(a, b, c, im3d_abgr(t[0].color)));
            }
        }
        2 => {
            for v in vertices {
                let Some(p) = project(at(v)) else { continue };
                out(Im3dShape::Circle(p, 1.0f32.max(v.position_size[3] * 0.1f32), im3d_abgr(v.color)));
            }
        }
        _ => {}
    }
}

/// EmitHandProxyIm3d's vertices: the boxes' triangles (36 each), then the axes' lines.
///
/// # Safety
/// `hand` holds seven floats; `out` has room for PROXY_VERTICES.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ovp_hand_proxy(hand: *const [f32; 7], left: bool, scale: f32, out: *mut ProxyVertex) {
    let v = hand_proxy(unsafe { *hand }, left, scale);
    unsafe { core::ptr::copy_nonoverlapping(v.as_ptr(), out, PROXY_VERTICES) };
}

#[cfg(test)]
mod proxy_tests {
    use super::*;

    #[test]
    fn hand_proxy_and_im3d_shapes() {
        assert_eq!(im3d_color(1.0, 0.5, 0.0, 0.32), 0xff7f_0051, "each channel x 255 truncated, r high");
        assert_eq!(im3d_abgr(0x1122_3344), 0x4433_2211);
        let hand = [1.0f32, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0];
        let left = hand_proxy(hand, true, 1.0);
        assert_eq!(left[0], ProxyVertex { p: [1.0 + 0.014, 2.0 + 0.052, 3.0 + 0.034], color: im3d_color(0.10, 0.85, 1.00, 0.32) }, "the palm's first corner");
        assert_eq!(left[1].p, hand_point(hand, true, [0.034, -0.052, -0.014]));
        assert_eq!((left[3].p, left[4].p, left[5].p), (left[0].p, left[2].p, hand_point(hand, true, [-0.034, 0.044, -0.014])), "a, b, c and a, c, d");
        assert_eq!(left[36].color, im3d_color(0.35, 0.92, 1.00, 0.50), "the first finger");
        let q = [0.0f32, 0.70710677, 0.0, 0.70710677];
        let right = hand_proxy([0.0, 0.0, 0.0, q[0], q[1], q[2], q[3]], false, 2.0);
        assert_eq!(right[6 * 36].color, im3d_color(1.00, 0.58, 0.22, 0.55), "the right thumb's second volume");
        assert_eq!(right[6 * 36].p, rotate(map_hand(false, [0.042 * 2.0, 0.012 * 2.0, -0.010 * 2.0]), q));
        assert_eq!(right[252], ProxyVertex { p: rotate(map_hand(false, [-0.12, 0.0, 0.0]), q), color: IM3D_RED });
        assert_eq!((right[256].color, right[257].p), (IM3D_BLUE, rotate(map_hand(false, [0.0, 0.0, -0.36]), q)), "the pointing axis");

        let vertex = |x: f32, size: f32, color: u32| Im3dVertex { position_size: [x, 0.0, -1.0, size], color };
        let mut project = |p: [f32; 3]| (p[0] >= 0.0).then_some([p[0], 10.0]);
        let mut shapes = Vec::new();
        let list = [vertex(0.0, 1.0, 0x1122_3344), vertex(1.0, 1.0, 0), vertex(2.0, 1.0, 0), vertex(3.0, 1.0, 0), vertex(-1.0, 1.0, 0), vertex(4.0, 1.0, 0), vertex(5.0, 1.0, 0)];
        im3d_shapes(0, &list, &mut project, &mut |s| shapes.push(s));
        assert_eq!(shapes, [Im3dShape::Triangle([0.0, 10.0], [1.0, 10.0], [2.0, 10.0], 0x4433_2211)], "a corner behind, an incomplete one");
        shapes.clear();
        im3d_shapes(1, &list, &mut project, &mut |s| shapes.push(s));
        assert!(shapes.is_empty(), "the lines draw nothing");
        im3d_shapes(2, &[vertex(1.0, 30.0, 0x1122_3344), vertex(-1.0, 30.0, 0), vertex(2.0, 5.0, 0)], &mut project, &mut |s| shapes.push(s));
        assert_eq!(shapes, [Im3dShape::Circle([1.0, 10.0], 30.0f32 * 0.1, 0x4433_2211), Im3dShape::Circle([2.0, 10.0], 1.0, 0)]);
    }
}

#[cfg(test)]
mod tests {
    //! The hand mapping, the rotation, the window's three FOV sources, its asymmetric centre and the zoom, and both
    //! projections.
    use super::*;

    #[test]
    fn hand_and_rotation() {
        assert_eq!(map_hand(true, [1.0, 2.0, 3.0]), [-3.0, -2.0, -1.0]);
        assert_eq!(map_hand(false, [1.0, 2.0, 3.0]), [3.0, -2.0, -1.0]);
        let half = core::f32::consts::FRAC_1_SQRT_2;
        let r = rotate([1.0, 0.0, 0.0], [0.0, half, 0.0, half]);
        assert!((r[0]).abs() < 1e-6 && (r[2] + 1.0).abs() < 1e-6);
    }
    #[test]
    fn windows() {
        let w = window(1000.0, 500.0, None, 90.0, 0.0, 1.0).unwrap();
        assert!((w.tan_x - 1.0).abs() < 1e-6 && (w.tan_y - 0.5).abs() < 1e-6 && w.centre_x == 0.0);
        assert_eq!(window(1000.0, 500.0, None, 0.0, 90.0, 1.0), Some(w));
        let fallback = window(1000.0, 1000.0, None, 0.0, 0.0, 0.0).unwrap();
        assert!((fallback.tan_x - (50.0f32 * (3.1415926535f32 / 180.0f32)).tan()).abs() < 1e-7);
        let zoomed = window(1000.0, 500.0, None, 90.0, 0.0, 2.0).unwrap();
        assert!((zoomed.tan_x - 0.5).abs() < 1e-6);
        let scope = window(1000.0, 500.0, None, 90.0, 0.0, 4.25).unwrap();
        assert!((scope.tan_x - 1.0 / 4.25).abs() < 1e-6);
        assert_eq!(window(1000.0, 500.0, None, 90.0, 0.0, 0.05).unwrap(), w);
        assert_eq!(window(1000.0, 500.0, None, 90.0, 0.0, f32::INFINITY).unwrap(), w);
        let asym = window(800.0, 800.0, Some([-1.0, 0.5, 1.0, -1.0]), 0.0, 0.0, 1.0).unwrap();
        assert_eq!(asym, Window { tan_x: 0.75, tan_y: 1.0, centre_x: -(-0.5f32) / 1.5, centre_y: 0.0 });
        assert!(window(1.0, 500.0, None, 90.0, 0.0, 1.0).is_none());
    }
    #[test]
    fn projections() {
        let w = Window { tan_x: 1.0, tan_y: 1.0, centre_x: 0.0, centre_y: 0.0 };
        assert_eq!(project_head([0.0, 0.0, -1.0], 100.0, 100.0, &w), Some([50.0, 50.0]));
        assert_eq!(project_head([1.0, 1.0, -1.0], 100.0, 100.0, &w), Some([100.0, 0.0]));
        assert_eq!(project_head([0.0, 0.0, -0.005], 100.0, 100.0, &w), None);
        let head = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        assert_eq!(project_world(head, [0.0, 1.0, -2.0], 100.0, 100.0, &w), Some([50.0, 50.0]));
    }
}
