//! THE WORLD MARKERS' ARITHMETIC (formerly include/Render/WorldMarkerProjection.hpp, WorldMarkerReprojection.hpp,
//! WorldMarkerScale.hpp and WorldMarkerOwnership.hpp): the flat ink canvas's camera recognised and moved per eye, a
//! marker's depth carried through the shared UI mesh, the canvas lifted back into the world through the camera the HUD
//! was laid out with, the marker's scale and the owner a flattened effect keeps. Every float and double step is the
//! C++ one, in its order (tools/world_marker_tests checked the C++; the tests below are its cases).
use cvr_body::cmath;

// ---- WorldMarkerProjection.hpp

/// Flat ink vertices normally have z=0: a bounded inverse forward depth rides through the shared UI mesh there; the
/// replacement VS removes it before projection.
pub fn encode_depth(inverse_depth: f32) -> f32 {
    if inverse_depth.is_finite() && inverse_depth > 0.0 && inverse_depth <= 20.0 {
        -65536.0 - 4096.0 * inverse_depth
    } else {
        0.0
    }
}
pub fn decode_depth(value: f32) -> f32 {
    if value < -65536.0 && value >= -147456.0 {
        (-value - 65536.0) / 4096.0
    } else {
        0.0
    }
}
/// A text glyph's matrix (112 bytes) carrying a depth.
pub fn text_matrix_depth(p: &[f32], bytes: usize) -> f32 {
    if bytes != 112 || p.len() < 16 || p[8] != 0.0 || p[9] != 0.0 || p[10] != 1.0 || p[15] != 1.0 {
        return 0.0;
    }
    decode_depth(p[11])
}
pub fn reproject_text_matrix(p: &mut [f32], pixels_per_inverse_depth: f32) {
    let depth = text_matrix_depth(p, 112);
    if depth == 0.0 {
        return;
    }
    p[11] = 0.0;
    if pixels_per_inverse_depth.is_finite() {
        p[3] += depth * pixels_per_inverse_depth;
    }
}
/// The point's depth along the game camera's forward (axes: X right, Y forward, Z up); 0 behind or too near.
pub fn forward_depth(point: &[f32; 3], eye: &[f32; 3], q: &[f32; 4]) -> f32 {
    let norm = q[0] as f64 * q[0] as f64
        + q[1] as f64 * q[1] as f64
        + q[2] as f64 * q[2] as f64
        + q[3] as f64 * q[3] as f64;
    if !norm.is_finite() || !(0.99..=1.01).contains(&norm) {
        return 0.0;
    }
    let forward = [
        2.0 * (q[0] * q[1] - q[2] * q[3]),
        1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]),
        2.0 * (q[1] * q[2] + q[0] * q[3]),
    ];
    let mut depth = 0.0f32;
    for i in 0..3 {
        depth += (point[i] - eye[i]) * forward[i];
    }
    if depth.is_finite() && depth >= 0.05 {
        depth
    } else {
        0.0
    }
}
pub fn stereo_scale(
    ipd: f32,
    vertical_fov_deg: f32,
    aspect: f32,
    zoom: f32,
    main_right: bool,
) -> f32 {
    if !ipd.is_finite()
        || ipd <= 0.0
        || ipd > 0.5
        || !vertical_fov_deg.is_finite()
        || vertical_fov_deg <= 1.0
        || vertical_fov_deg >= 170.0
        || !aspect.is_finite()
        || aspect <= 0.0
        || !zoom.is_finite()
        || zoom <= 0.0
    {
        return 0.0;
    }
    let scale = ipd * zoom / (cmath::tan(vertical_fov_deg * 0.008_726_646_26) * aspect);
    if main_right {
        scale
    } else {
        -scale
    }
}
/// The flat ink camera's matrix for a w x h canvas, exactly as the engine writes it.
pub fn is_ink_matrix(m: &[f32], w: f32, h: f32) -> bool {
    let expected = [
        2.0 / w,
        0.0,
        0.0,
        -1.0,
        0.0,
        -2.0 / h,
        0.0,
        1.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ];
    (0..16).all(|i| m[i].is_finite() && (m[i] - expected[i]).abs() <= 1e-7)
}
/// The camera block as 2.5 uploads it: 848 bytes and the eye the pass draws (0 one view, 1 left, 2 right in the
/// engine's own stereo frame), uploaded rounded to 864.
pub const INK_CAMERA_BYTES: u32 = 864;
pub const INK_CAMERA_EYE: usize = 848;
/// The game's flat ink camera, never a perspective, shadow or world camera.
pub fn is_ink_camera(p: &[f32], bytes: usize) -> bool {
    if bytes != INK_CAMERA_BYTES as usize || p.len() < 190 {
        return false;
    }
    let (w, h) = (p[188], p[189]);
    if !((640.0..=16384.0).contains(&w) && (360.0..=16384.0).contains(&h)) {
        return false;
    }
    is_ink_matrix(p, w, h)
}
pub fn ink_camera_eye(data: &[u8]) -> i32 {
    i32::from_ne_bytes(data[INK_CAMERA_EYE..INK_CAMERA_EYE + 4].try_into().unwrap())
}
/// Squeezes clip x about the canvas centre and moves the centre to (shift_x, shift_y), in the camera's first matrix
/// and in every other copy of the canvas matrix the block carries (floats 0, 32, 48, 64 and 112); `content`, when
/// narrower than the canvas, is the width the layout fills from its left edge. Returns the copies moved.
#[allow(clippy::too_many_arguments)]
pub fn shift_ink_camera(
    out: &mut [f32],
    source: &[f32],
    bytes: usize,
    squeeze: f32,
    shift_x: f32,
    shift_y: f32,
    content: f32,
) -> i32 {
    let (w, h) = (source[188], source[189]);
    let wide = if content > 0.0 && content < w {
        w / content
    } else {
        1.0
    };
    let mut copies = 0;
    let mut m = 0;
    while m + 16 <= bytes / 4 {
        if m != 0 && !is_ink_matrix(&source[m..], w, h) {
            m += 16;
            continue;
        }
        out[m] *= squeeze * wide;
        out[m + 3] = out[m + 3] * squeeze * wide + squeeze * (wide - 1.0) + shift_x;
        out[m + 7] += shift_y;
        if m != 0 {
            copies += 1;
        }
        m += 16;
    }
    copies
}

// ---- WorldMarkerReprojection.hpp

/// The camera a canvas was laid out with: its origin and axes in the world, its projection's terms and the canvas.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct SourceProjection {
    pub origin: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub forward: [f32; 3],
    pub p00: f32,
    pub p11: f32,
    pub p20: f32,
    pub p21: f32,
    pub width: f32,
    pub height: f32,
}
const _: () = assert!(core::mem::size_of::<SourceProjection>() == 72);

impl SourceProjection {
    pub fn valid(&self) -> bool {
        self.width > 0.0 && self.height > 0.0 && self.p00 > 0.0 && self.p11 > 0.0
    }
    fn bits(&self) -> [u32; 18] {
        let mut out = [0u32; 18];
        let all = [self.origin, self.right, self.up, self.forward];
        for (i, v) in all.iter().flatten().enumerate() {
            out[i] = v.to_bits();
        }
        for (i, v) in [
            self.p00,
            self.p11,
            self.p20,
            self.p21,
            self.width,
            self.height,
        ]
        .iter()
        .enumerate()
        {
            out[12 + i] = v.to_bits();
        }
        out
    }
}
pub const WORLD_CAMERA_MAGIC: f32 = 538012.0;
pub const WORLD_TEXT_MAGIC: f32 = -16384.0;
pub const WORLD_CAMERA_BYTES: u32 = 944;

/// The eye packet the replacement shaders read.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct WorldMarkerCamera {
    pub origin: [f32; 4],
    pub right: [f32; 4],
    pub up: [f32; 4],
    pub forward: [f32; 4],
    pub projection: [f32; 4],
}
const _: () = assert!(core::mem::size_of::<WorldMarkerCamera>() == 80);

fn read_f32(b: &[u8], at: usize) -> f32 {
    f32::from_ne_bytes(b[at..at + 4].try_into().unwrap())
}
fn read_i32(b: &[u8], at: usize) -> i32 {
    i32::from_ne_bytes(b[at..at + 4].try_into().unwrap())
}
fn read_u32(b: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(b[at..at + 4].try_into().unwrap())
}

/// The engine camera block's view (`camera`: at least 0x380 bytes): the quaternion at 0x10, the projection at 0x190,
/// the fixed-point position at 0; `unjitter` takes the upscaler's jitter (0x370, its dimensions 0x378) off.
pub fn read_projection(
    camera: &[u8],
    width: f32,
    height: f32,
    unjitter: bool,
) -> Option<SourceProjection> {
    if !width.is_finite()
        || !height.is_finite()
        || width < 1.0
        || height < 1.0
        || width > 16384.0
        || height > 16384.0
    {
        return None;
    }
    let q: [f32; 4] = core::array::from_fn(|i| read_f32(camera, 0x10 + 4 * i));
    let p: [f32; 16] = core::array::from_fn(|i| read_f32(camera, 0x190 + 4 * i));
    let position: [i32; 3] = core::array::from_fn(|i| read_i32(camera, 4 * i));
    let mut norm = 0.0f64;
    for v in q {
        if !v.is_finite() {
            return None;
        }
        norm += v as f64 * v as f64;
    }
    if !(0.99..=1.01).contains(&norm)
        || !p[0].is_finite()
        || !p[5].is_finite()
        || p[0] <= 0.0
        || p[5] <= 0.0
        || (p[11] - 1.0).abs() > 1e-5
        || p[15] != 0.0
    {
        return None;
    }
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let mut value = SourceProjection::default();
    for i in 0..3 {
        value.origin[i] = (position[i] as f64 / 131072.0) as f32;
    }
    value.right = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
    ];
    value.forward = [
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
    ];
    value.up = [
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
    ];
    value.p00 = p[0];
    value.p11 = p[5];
    value.p20 = p[8];
    value.p21 = p[9];
    value.width = width;
    value.height = height;
    if unjitter {
        let jitter = [read_f32(camera, 0x370), read_f32(camera, 0x374)];
        let dimensions = [read_u32(camera, 0x378), read_u32(camera, 0x37C)];
        if dimensions[0] > 1 && dimensions[1] > 1 {
            value.p20 -= 2.0 * jitter[0] / dimensions[0] as f32;
            value.p21 -= 2.0 * jitter[1] / dimensions[1] as f32;
        }
    }
    if !value.p20.is_finite() || !value.p21.is_finite() {
        return None;
    }
    Some(value)
}
pub fn source_depth(source: &SourceProjection, point: &[f32; 3]) -> f32 {
    let mut depth = 0.0f64;
    for i in 0..3 {
        depth += (point[i] as f64 - source.origin[i] as f64) * source.forward[i] as f64;
    }
    if depth.is_finite() && depth >= 0.05 {
        depth as f32
    } else {
        0.0
    }
}
pub fn canvas_to_world(
    source: &SourceProjection,
    inverse_depth: f32,
    x: f32,
    y: f32,
) -> Option<[f32; 3]> {
    canvas_to_world_written(source, inverse_depth, x, y)
        .filter(|world| world.iter().all(|v| v.is_finite()))
}
/// canvas_to_world before its finiteness check: what C++ writes to `world` past the arguments (None: nothing written).
pub fn canvas_to_world_written(
    source: &SourceProjection,
    inverse_depth: f32,
    x: f32,
    y: f32,
) -> Option<[f32; 3]> {
    if !source.valid()
        || !inverse_depth.is_finite()
        || inverse_depth <= 0.0
        || inverse_depth > 20.0
        || !x.is_finite()
        || !y.is_finite()
    {
        return None;
    }
    let vx = ((2.0 * x as f64 / source.width as f64 - 1.0) - source.p20 as f64) / source.p00 as f64;
    let vy =
        ((1.0 - 2.0 * y as f64 / source.height as f64) - source.p21 as f64) / source.p11 as f64;
    let world: [f32; 3] = core::array::from_fn(|i| {
        (source.origin[i] as f64
            + (source.forward[i] as f64 + vx * source.right[i] as f64 + vy * source.up[i] as f64)
                / inverse_depth as f64) as f32
    });
    Some(world)
}
/// A marker's world point with the canvas pixel the source camera puts it on: the point its geometry hangs from.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct MarkerAnchor {
    pub world: [f32; 3],
    pub canvas: [f32; 2],
    pub valid: bool,
}
pub fn anchor_of(source: &SourceProjection, point: &[f32; 3]) -> MarkerAnchor {
    let mut anchor = MarkerAnchor::default();
    if !source.valid() {
        return anchor;
    }
    let mut v = [0.0f64; 3];
    for i in 0..3 {
        let d = point[i] as f64 - source.origin[i] as f64;
        v[0] += d * source.right[i] as f64;
        v[1] += d * source.up[i] as f64;
        v[2] += d * source.forward[i] as f64;
    }
    if !v[2].is_finite() || v[2] < 0.05 {
        return anchor;
    }
    anchor.canvas[0] = ((v[0] / v[2] * source.p00 as f64 + source.p20 as f64 + 1.0)
        * source.width as f64
        * 0.5) as f32;
    anchor.canvas[1] = ((1.0 - v[1] / v[2] * source.p11 as f64 - source.p21 as f64)
        * source.height as f64
        * 0.5) as f32;
    anchor.world = *point;
    anchor.valid = anchor.canvas[0].is_finite() && anchor.canvas[1].is_finite();
    anchor
}
/// The source's picture plane without its roll: right level with the world (game Z up), up across it and the
/// forward. Straight up or down there is no level right, and the source's own axes stay.
pub fn level_axes(source: &SourceProjection) -> ([f32; 3], [f32; 3]) {
    let f = &source.forward;
    let length = (f[0] as f64 * f[0] as f64 + f[1] as f64 * f[1] as f64).sqrt();
    if !(length > 1e-3) {
        return (source.right, source.up);
    }
    let right = [
        (f[1] as f64 / length) as f32,
        (-f[0] as f64 / length) as f32,
        0.0,
    ];
    let up = [
        right[1] * f[2] - right[2] * f[1],
        right[2] * f[0] - right[0] * f[2],
        right[0] * f[1] - right[1] * f[0],
    ];
    (right, up)
}
/// The marker hangs level from its point: the point stays exact, the pixels around it go out along the level axes, at
/// its depth.
pub fn canvas_to_world_level(
    source: &SourceProjection,
    anchor: &MarkerAnchor,
    inverse_depth: f32,
    x: f32,
    y: f32,
) -> Option<[f32; 3]> {
    canvas_to_world_level_written(source, anchor, inverse_depth, x, y)
        .filter(|world| world.iter().all(|v| v.is_finite()))
}
/// canvas_to_world_level before its finiteness check (None: nothing written).
pub fn canvas_to_world_level_written(
    source: &SourceProjection,
    anchor: &MarkerAnchor,
    inverse_depth: f32,
    x: f32,
    y: f32,
) -> Option<[f32; 3]> {
    if !anchor.valid
        || !source.valid()
        || !inverse_depth.is_finite()
        || inverse_depth <= 0.0
        || inverse_depth > 20.0
        || !x.is_finite()
        || !y.is_finite()
    {
        return None;
    }
    let (right, up) = level_axes(source);
    let vx = 2.0 * (x - anchor.canvas[0]) as f64 / (source.width * source.p00) as f64;
    let vy = -2.0 * (y - anchor.canvas[1]) as f64 / (source.height * source.p11) as f64;
    let world: [f32; 3] = core::array::from_fn(|i| {
        (anchor.world[i] as f64 + (vx * right[i] as f64 + vy * up[i] as f64) / inverse_depth as f64)
            as f32
    });
    Some(world)
}
/// The text builder rounds the matrix's translation to the canvas pixel while the HUD snaps; the record's own
/// translation goes back where the builder rounds from.
pub fn text_translation_exact(matrix: &mut [f32], x: f32, y: f32) -> bool {
    if !x.is_finite()
        || !y.is_finite()
        || (matrix[12] - x).abs() > 0.5001
        || (matrix[13] - y).abs() > 0.5001
    {
        return false;
    }
    matrix[12] = x;
    matrix[13] = y;
    true
}
/// A glyph's column-major affine matrix lifted into the world (the unused local-Z coefficient marks it); with a valid
/// anchor the glyphs hang level from it.
pub fn text_to_world(
    matrix: &mut [f32; 16],
    source: &SourceProjection,
    inverse_depth: f32,
    anchor: Option<&MarkerAnchor>,
) -> bool {
    if matrix[2] != 0.0
        || matrix[6] != 0.0
        || matrix[10] != 1.0
        || matrix[14] != 0.0
        || matrix[3] != 0.0
        || matrix[7] != 0.0
        || matrix[11] != 0.0
        || matrix[15] != 1.0
    {
        return false;
    }
    let level = anchor.filter(|a| a.valid);
    let origin = match level {
        Some(a) => canvas_to_world_level(source, a, inverse_depth, matrix[12], matrix[13]),
        None => canvas_to_world(source, inverse_depth, matrix[12], matrix[13]),
    };
    let Some(origin) = origin else { return false };
    let (right, up) = if level.is_some() {
        level_axes(source)
    } else {
        (source.right, source.up)
    };
    let x = 2.0 / (source.width * source.p00 * inverse_depth) as f64;
    let y = -2.0 / (source.height * source.p11 * inverse_depth) as f64;
    let mut result = [0.0f32; 16];
    result[15] = 1.0;
    for i in 0..3 {
        result[i] =
            (x * right[i] as f64 * matrix[0] as f64 + y * up[i] as f64 * matrix[1] as f64) as f32;
        result[4 + i] =
            (x * right[i] as f64 * matrix[4] as f64 + y * up[i] as f64 * matrix[5] as f64) as f32;
        result[12 + i] = origin[i];
    }
    result[10] = WORLD_TEXT_MAGIC;
    *matrix = result;
    true
}
pub fn marker_camera(eye: &SourceProjection) -> WorldMarkerCamera {
    let mut result = WorldMarkerCamera::default();
    result.origin[..3].copy_from_slice(&eye.origin);
    result.origin[3] = WORLD_CAMERA_MAGIC;
    result.right[..3].copy_from_slice(&eye.right);
    result.up[..3].copy_from_slice(&eye.up);
    result.forward[..3].copy_from_slice(&eye.forward);
    result.projection = [eye.p00, eye.p11, eye.p20, eye.p21];
    result
}

// ---- WorldMarkerScale.hpp

/// Native cached mappins place their texture at screenAnchor + cacheBounds.min: scaling about this local point leaves
/// the anchor fixed, even if an animation changes the cached bounds.
pub fn cached_marker_pivot(
    width: f32,
    height: f32,
    offset_x: f32,
    offset_y: f32,
) -> (bool, f32, f32) {
    if !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
        || !offset_x.is_finite()
        || !offset_y.is_finite()
    {
        return (false, 0.0, 0.0);
    }
    let x = -offset_x / width;
    let y = -offset_y / height;
    (x.abs() <= 16.0 && y.abs() <= 16.0, x, y)
}
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MarkerScale {
    pub applied_x: f32,
    pub applied_y: f32,
    pub native_x: f32,
    pub native_y: f32,
    pub applied: bool,
}
impl Default for MarkerScale {
    fn default() -> Self {
        MarkerScale {
            applied_x: 0.0,
            applied_y: 0.0,
            native_x: 1.0,
            native_y: 1.0,
            applied: false,
        }
    }
}
impl MarkerScale {
    pub fn target(&self, current_x: f32, current_y: f32, factor: f32) -> Option<(f32, f32)> {
        if !current_x.is_finite()
            || !current_y.is_finite()
            || !factor.is_finite()
            || !(factor > 0.0)
        {
            return None;
        }
        if self.applied && current_x == self.applied_x && current_y == self.applied_y {
            return None;
        }
        Some((current_x * factor, current_y * factor))
    }
    /// `from`: the game's scale the target was made from.
    pub fn commit(&mut self, x: f32, y: f32, from_x: f32, from_y: f32) {
        *self = MarkerScale {
            applied_x: x,
            applied_y: y,
            native_x: from_x,
            native_y: from_y,
            applied: true,
        };
    }
    /// The game's scale back, while the widget still carries ours.
    pub fn restore(&self, current_x: f32, current_y: f32) -> Option<(f32, f32)> {
        if !self.applied || current_x != self.applied_x || current_y != self.applied_y {
            return None;
        }
        Some((self.native_x, self.native_y))
    }
}

// ---- WorldMarkerOwnership.hpp

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct MarkerOwner {
    pub root: usize,
    pub depth: f32,
    pub source: SourceProjection,
    pub anchor: MarkerAnchor,
}
/// A flattened effect can carry one exact widget owner. Mixed overlays keep their individual projections; never
/// guessed by screen-space bounds.
pub fn merge_effect_owner(common: &mut MarkerOwner, next: MarkerOwner) -> bool {
    if next.root == 0 || !next.depth.is_finite() || next.depth <= 0.0 {
        return false;
    }
    if common.root != 0 && common.root != next.root {
        return false;
    }
    if common.root == 0 {
        *common = next;
    } else {
        let bits = |v: &[f32]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
        if common.source.bits() != next.source.bits()
            || common.anchor.valid != next.anchor.valid
            || bits(&common.anchor.world) != bits(&next.anchor.world)
            || bits(&common.anchor.canvas) != bits(&next.anchor.canvas)
        {
            common.source = SourceProjection::default();
            common.anchor = MarkerAnchor::default();
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink_camera(width: f32, height: f32) -> [f32; 256] {
        let mut p = [0.0f32; 256];
        p[0] = 2.0 / width;
        p[3] = -1.0;
        p[5] = -2.0 / height;
        p[7] = 1.0;
        p[10] = 1.0;
        p[15] = 1.0;
        p[188] = width;
        p[189] = height;
        p
    }
    fn marker_source() -> SourceProjection {
        SourceProjection {
            origin: [-2400.0, -950.0, 9.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            forward: [0.0, 1.0, 0.0],
            p00: 0.45,
            p11: 0.835,
            p20: 0.0,
            p21: 0.0,
            width: 5296.0,
            height: 2864.0,
        }
    }
    fn identity() -> [f32; 16] {
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m
    }

    #[test]
    fn marker_size() {
        let mut size = MarkerScale::default();
        assert_eq!(
            size.target(1.0, 1.0, 0.7),
            Some((0.7, 0.7)),
            "world marker scale is not 0.7"
        );
        size.commit(0.7, 0.7, 1.0, 1.0);
        assert!(
            size.target(0.7, 0.7, 0.7).is_none(),
            "marker shrinks on every position update"
        );
        let (x, y) = size
            .target(1.4, 0.8, 0.7)
            .expect("game-authored scale update lost");
        assert!((x - 0.98).abs() < 1e-6 && (y - 0.56).abs() < 1e-6);
        size.commit(x, y, 1.4, 0.8);
        assert!(
            size.target(size.applied_x, size.applied_y, 0.7).is_none(),
            "nonuniform scale compounded"
        );
        assert!(
            size.target(f32::NAN, 1.0, 0.7).is_none(),
            "invalid widget scale accepted"
        );
        assert_eq!(
            size.restore(size.applied_x, size.applied_y),
            Some((1.4, 0.8)),
            "cached live root keeps the marker scale"
        );
        assert!(
            size.restore(0.9, 0.9).is_none(),
            "game-authored scale replaced on hand-back"
        );
        size = MarkerScale::default();
        assert!(size.restore(0.0, 0.0).is_none(), "unscaled root restored");
        assert_eq!(
            size.target(0.5, 0.5, 0.7),
            Some((0.35, 0.35)),
            "new widget inherited previous root ownership"
        );
        for width in [128.0f32, 256.0, 512.0] {
            for height in [128.0f32, 256.0, 512.0] {
                for scale in [0.364_053_76f32, 0.7, 1.0] {
                    let offset_x = -width * 0.5;
                    let offset_y = -height * 0.5 + 36.0;
                    let (ok, x, y) = cached_marker_pivot(width, height, offset_x, offset_y);
                    assert!(ok, "cached anchor rejected");
                    let (screen_x, screen_y) = (1780.9353f32, 1056.241f32);
                    let actual_x =
                        screen_x + offset_x + x * width + (-offset_x - x * width) * scale;
                    let actual_y =
                        screen_y + offset_y + y * height + (-offset_y - y * height) * scale;
                    assert!(
                        (actual_x - screen_x).abs() < 0.0002
                            && (actual_y - screen_y).abs() < 0.0002,
                        "cached animation/scale moves the NPC anchor"
                    );
                }
            }
        }
        assert!(
            !cached_marker_pivot(0.0, 256.0, 0.0, 0.0).0,
            "empty cache accepted"
        );
    }

    #[test]
    fn effect_owner() {
        let mut owner = MarkerOwner::default();
        let of = |root: usize, depth: f32| MarkerOwner {
            root,
            depth,
            ..Default::default()
        };
        assert!(
            merge_effect_owner(&mut owner, of(17, 0.5))
                && merge_effect_owner(&mut owner, of(17, 0.5))
        );
        assert!(
            owner.root == 17 && owner.depth == 0.5,
            "effect changes marker depth"
        );
        assert!(
            !merge_effect_owner(&mut owner, of(18, 0.5)),
            "unrelated markers at the same depth merged"
        );
        assert!(
            !merge_effect_owner(&mut owner, of(0, 0.0)),
            "ordinary HUD content inherited marker projection"
        );
        assert!(
            !merge_effect_owner(&mut owner, of(17, f32::NAN)),
            "invalid effect depth accepted"
        );
        let mut outer = MarkerOwner::default();
        assert!(
            merge_effect_owner(&mut outer, owner) && outer.root == 17,
            "nested effect loses original widget owner"
        );
    }

    #[test]
    fn depth() {
        let eye = [-1830.0f32, -2354.0, 31.0];
        let q = [0.0f32, 0.0, 0.0, 1.0];
        for depth in [0.05f32, 2.0, 10.0, 1000.0] {
            let point = [eye[0] + 100.0, eye[1] + depth, eye[2] - 20.0];
            assert!(
                (forward_depth(&point, &eye, &q) - depth).abs() < 0.0002,
                "radial distance used instead of forward depth"
            );
            assert!(
                (decode_depth(encode_depth(1.0 / depth)) - 1.0 / depth).abs() < 2e-6,
                "depth encoding precision"
            );
        }
        assert_eq!(
            forward_depth(&[-1830.0, -2355.0, 31.0], &eye, &q),
            0.0,
            "behind-camera marker accepted"
        );
        assert!(
            encode_depth(f32::NAN) == 0.0 && encode_depth(-1.0) == 0.0,
            "invalid marker depth accepted"
        );
        let yaw = [0.0f32, 0.0, 0.707_106_78, 0.707_106_78];
        assert!(
            (forward_depth(&[-1835.0, -2354.0, 31.0], &eye, &yaw) - 5.0).abs() < 1e-5,
            "rotated camera depth"
        );
    }

    #[test]
    fn camera() {
        let bytes = INK_CAMERA_BYTES as usize;
        let mut p = ink_camera(2560.0, 2560.0);
        assert!(is_ink_camera(&p, bytes), "ink camera missed");
        p[0] = 0.719;
        assert!(
            !is_ink_camera(&p, bytes),
            "world perspective camera accepted"
        );
        p = ink_camera(2560.0, 2560.0);
        p[3] = 0.0;
        assert!(!is_ink_camera(&p, bytes), "world/shadow matrix accepted");
        p = ink_camera(2560.0, 2560.0);
        p[0] = f32::NAN;
        assert!(!is_ink_camera(&p, bytes), "invalid camera accepted");
        p = ink_camera(2560.0, 2560.0);
        for m in [16usize, 80, 96, 128] {
            p[m] = 1.0;
            p[m + 5] = 1.0;
            p[m + 10] = 1.0;
            p[m + 15] = 1.0;
        }
        for m in [32usize, 48, 64, 112] {
            for i in 0..16 {
                p[m + i] = p[i];
            }
        }
        let mut out = p;
        assert_eq!(
            shift_ink_camera(&mut out, &p, 1024, 0.9, 0.25, -0.1, 0.0),
            4,
            "canvas matrix copies missed"
        );
        for m in [0usize, 32, 48, 64, 112] {
            assert!(
                out[m] == p[0] * 0.9
                    && out[m + 5] == p[5]
                    && (out[m + 3] - (-0.9 + 0.25)).abs() < 1e-6
                    && (out[m + 7] - 0.9).abs() < 1e-6,
                "copy not moved like the first matrix"
            );
        }
        for m in [16usize, 80, 96, 128] {
            assert_eq!(out[m..m + 16], p[m..m + 16], "non-canvas matrix changed");
        }
        assert!(
            out[188] == p[188] && out[189] == p[189],
            "canvas size changed"
        );
    }

    #[test]
    fn reprojection_math() {
        let source = marker_source();
        let point = canvas_to_world(
            &source,
            1.0 / 40.0,
            (1.0 + 0.45 * 0.2) * 2648.0,
            (1.0 - 0.835 * 0.075) * 1432.0,
        )
        .expect("source unprojection rejected");
        assert!(
            (point[0] + 2392.0).abs() < 0.0003
                && (point[1] + 910.0).abs() < 0.0003
                && (point[2] - 12.0).abs() < 0.0003
        );
        assert_eq!(
            source_depth(&source, &point),
            40.0,
            "source camera depth differs from projected anchor"
        );
        assert!(
            canvas_to_world(&source, 0.0, 0.0, 0.0).is_none(),
            "zero depth accepted"
        );
        let mut matrix = identity();
        matrix[12] = 2648.0;
        matrix[13] = 1432.0;
        assert!(
            text_to_world(&mut matrix, &source, 0.025, None),
            "text plane rejected"
        );
        assert!(
            matrix[12] == -2400.0 && matrix[13] == -910.0 && matrix[14] == 9.0,
            "text anchor differs from sprite anchor"
        );
        assert!(
            matrix[0] > 0.0 && matrix[6] < 0.0 && matrix[10] == WORLD_TEXT_MAGIC,
            "glyph axes/flag wrong"
        );
        assert!(
            !text_to_world(&mut matrix, &source, 0.025, None),
            "text transformed twice"
        );

        let mut native = [0u8; 1024];
        let q = [0.0f32, 0.0, 0.707_106_78, 0.707_106_78];
        let mut p = [0.0f32; 16];
        p[0] = 0.9;
        p[5] = 0.835;
        p[8] = 0.25 + 2.0 * 0.375 / 3072.0;
        p[9] = 0.18 + 2.0 * -0.125 / 1661.0;
        p[11] = 1.0;
        let put = |b: &mut [u8], at: usize, v: &[f32]| {
            for (i, f) in v.iter().enumerate() {
                b[at + 4 * i..at + 4 * i + 4].copy_from_slice(&f.to_ne_bytes());
            }
        };
        put(&mut native, 0x10, &q);
        put(&mut native, 0x190, &p);
        put(&mut native, 0x370, &[0.375, -0.125]);
        native[0x378..0x37C].copy_from_slice(&3072u32.to_ne_bytes());
        native[0x37C..0x380].copy_from_slice(&1661u32.to_ne_bytes());
        let eye =
            read_projection(&native, 5296.0, 2864.0, true).expect("native eye camera rejected");
        assert!(
            (eye.forward[0] + 1.0).abs() < 1e-6 && (eye.right[1] - 1.0).abs() < 1e-6,
            "native quaternion axes wrong"
        );
        assert!(
            (eye.p20 - 0.25).abs() < 1e-6 && (eye.p21 - 0.18).abs() < 1e-6,
            "DLSS jitter remains on HUD"
        );

        let mut owner = MarkerOwner {
            root: 17,
            depth: 0.025,
            source,
            ..Default::default()
        };
        let mut other = owner;
        other.source.origin[0] += 1.0;
        assert!(
            merge_effect_owner(&mut owner, other) && !owner.source.valid(),
            "effect silently combines two source poses"
        );

        let target = [-2392.0f32, -910.0, 12.0];
        let anchor = anchor_of(&source, &target);
        assert!(
            anchor.valid
                && (anchor.canvas[0] - (1.0 + 0.45 * 0.2) * 2648.0).abs() < 0.01
                && (anchor.canvas[1] - (1.0 - 0.835 * 0.075) * 1432.0).abs() < 0.01,
            "anchor canvas differs from the source projection"
        );
        for pixel in [anchor.canvas, [2600.0, 1300.0], [3100.0, 900.0]] {
            let plain = canvas_to_world(&source, 1.0 / 40.0, pixel[0], pixel[1])
                .expect("level lift rejected");
            let level = canvas_to_world_level(&source, &anchor, 1.0 / 40.0, pixel[0], pixel[1])
                .expect("level lift rejected");
            for i in 0..3 {
                assert!(
                    (plain[i] - level[i]).abs() < 0.002,
                    "level lift differs without roll"
                );
            }
        }
        for roll in [-0.4f32, 0.25] {
            let mut rolled = source;
            let (c, s) = (roll.cos(), roll.sin());
            for i in 0..3 {
                rolled.right[i] = c * source.right[i] + s * source.up[i];
                rolled.up[i] = c * source.up[i] - s * source.right[i];
            }
            let tilted = anchor_of(&rolled, &target);
            assert!(tilted.valid, "rolled anchor rejected");
            let at = canvas_to_world_level(
                &rolled,
                &tilted,
                1.0 / 40.0,
                tilted.canvas[0],
                tilted.canvas[1],
            )
            .expect("rolled anchor lift rejected");
            for i in 0..3 {
                assert!(
                    (at[i] - target[i]).abs() < 0.002,
                    "roll moves the marker's point"
                );
            }
            let above = canvas_to_world_level(
                &rolled,
                &tilted,
                1.0 / 40.0,
                tilted.canvas[0],
                tilted.canvas[1] - 100.0,
            )
            .expect("rolled lift rejected");
            assert!(
                (above[0] - at[0]).abs() < 1e-3
                    && (above[1] - at[1]).abs() < 1e-3
                    && above[2] - at[2] > 0.5,
                "roll turns the marker"
            );
            let mut text = identity();
            text[12] = tilted.canvas[0];
            text[13] = tilted.canvas[1];
            assert!(
                text_to_world(&mut text, &rolled, 1.0 / 40.0, Some(&tilted)),
                "level glyphs rejected"
            );
            assert!(
                text[2].abs() < 1e-6
                    && text[6] < 0.0
                    && (text[12] - target[0]).abs() < 0.002
                    && (text[14] - target[2]).abs() < 0.002,
                "roll turns the glyphs"
            );
        }
    }
}
