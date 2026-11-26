//! The world markers' vertex shaders (src/Render/WorldMarkers.hlsl, formerly DXC's): the 2.5 flat ink family
//! replaced -- sprites, procedural quads and the four stereo variants -- placing a marker that carries the port's
//! depth tag at its point's depth in each eye, and a vertex that carries world coordinates (its alpha's sign bit) by
//! the eye's own camera. The operations in the HLSL's order (DXC compiled both); the engine's input layout (POSITION,
//! TEXCOORD, COLOR) and its pixel shaders' input (SV_Position, TEXCOORD0, TEXCOORD1) are the builder's SIGNATURES.
//!
//! Bindings (descriptor set 0): 1 CameraShaderConsts (b1, with the port's marker camera after the engine's rows),
//! 5 the object's affine rows (b5), 7 the material (b7).
use crate::common::dot3;
use spirv_std::glam::{Vec2, Vec3, Vec3Swizzles, Vec4, Vec4Swizzles};
use spirv_std::spirv;

/// b1: float4 camera[53]; int cameraEye; float3 markerPadding; float4 markerEyePosition, markerRight, markerUp,
/// markerForward, markerProjection.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Camera {
    pub camera: [Vec4; 53],
    pub eye: i32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub padding_z: f32,
    pub eye_position: Vec4,
    pub right: Vec4,
    pub up: Vec4,
    pub forward: Vec4,
    pub projection: Vec4,
}
/// b5: float4 object[7].
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Object {
    pub v: [Vec4; 7],
}
/// b7: float4 material[32].
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Material {
    pub v: [Vec4; 32],
}

/// HLSL's dot of two float4: one Dot4.
#[cfg(target_arch = "spirv")]
fn dot4(a: Vec4, b: Vec4) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%a = OpLoad _ {a}",
            "%b = OpLoad _ {b}",
            "%result = OpDot typeof*{result} %a %b",
            "OpStore {result} %result",
            a = in(reg) &a,
            b = in(reg) &b,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
fn dot4(a: Vec4, b: Vec4) -> f32 {
    a.dot(b)
}

fn marker_depth(value: f32) -> f32 {
    if value < -65536.0 && value >= -147456.0 {
        (-value - 65536.0) / 4096.0
    } else {
        0.0
    }
}
/// Glyphs carry their anchor in the affine matrix instead of each vertex (native composition batches these matrices
/// without the ordinary constant uploaders): decoded here as well, before the ink camera.
fn text_depth(o: &Object) -> f32 {
    if o.v[2].x == 0.0 && o.v[2].y == 0.0 && o.v[2].z == 1.0 {
        marker_depth(o.v[2].w)
    } else {
        0.0
    }
}
fn world_vertex(alpha: f32) -> bool {
    alpha.to_bits() & 0x8000_0000 != 0
}
fn world_text(o: &Object) -> bool {
    o.v[2].z == -16384.0
}

fn project(position: Vec3, world_vertex: bool, c: &Camera, o: &Object) -> Vec4 {
    if world_vertex {
        // Never interpret cached world coordinates as canvas pixels if this pass has no matching eye camera.
        if c.eye_position.w != 538012.0 {
            return Vec4::new(0.0, 0.0, -1.0, 1.0);
        }
        // Sprites carry world coordinates directly; text shares local glyph vertices and carries the lifted world
        // plane in its affine matrix.
        let mut world = position;
        if world_text(o) {
            let glyph = Vec4::new(position.x, position.y, 0.0, 1.0);
            world = Vec3::new(dot4(o.v[0], glyph), dot4(o.v[1], glyph), dot4(o.v[2], glyph));
        }
        let delta = world - c.eye_position.xyz();
        let view = Vec3::new(dot3(delta, c.right.xyz()), dot3(delta, c.up.xyz()), dot3(delta, c.forward.xyz()));
        if view.z <= 0.01 {
            return Vec4::new(0.0, 0.0, -1.0, 1.0);
        }
        let xy = view.xy() * c.projection.xy() + view.z * c.projection.zw();
        return Vec4::new(xy.x, xy.y, 0.0, view.z);
    }
    let mut position = position;
    let vertex_depth = marker_depth(position.z);
    if vertex_depth > 0.0 {
        position.z = 0.0;
    }
    let text = text_depth(o);
    let inv_depth = if text > 0.0 { text } else { vertex_depth };
    let local = Vec4::new(position.x, position.y, position.z, 1.0);
    let mut world = Vec3::new(dot4(o.v[0], local), dot4(o.v[1], local), dot4(o.v[2], local));
    if text > 0.0 {
        world.z = dot3(o.v[2].xyz(), position);
    }
    let p = Vec4::new(world.x, world.y, world.z, 1.0);
    let mut clip = Vec4::new(dot4(c.camera[0], p), dot4(c.camera[1], p), dot4(c.camera[2], p), dot4(c.camera[3], p));
    // Zero in MAIN; signed IPD/tan(horizontal half-FOV) in VRCAM's flat camera.
    if inv_depth > 0.0 {
        clip.x += c.camera[0].z * inv_depth * clip.w;
    }
    clip
}

/// The sprite: its position, uv (with the vertex z where not a world or tagged vertex) and colour.
fn sprite(position: Vec3, uv: Vec2, color: Vec4, c: &Camera, o: &Object) -> (Vec4, Vec4, Vec3) {
    let mut color = color;
    let world = world_vertex(color.w) || world_text(o);
    if world_vertex(color.w) {
        color.w = color.w.abs();
    }
    let at = project(position, world, c, o);
    let z = if world || (position.z < -65536.0 && position.z >= -147456.0) { 0.0 } else { position.z };
    (at, Vec4::new(uv.x, uv.y, z, color.x), Vec3::new(color.y, color.z, color.w))
}

#[spirv(vertex)]
#[allow(clippy::too_many_arguments)]
pub fn sprite_main(
    position: Vec3,
    uv: Vec2,
    color: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] c: &Camera,
    #[spirv(uniform, descriptor_set = 0, binding = 5)] o: &Object,
    #[spirv(position)] out_position: &mut Vec4,
    out_uv: &mut Vec4,
    out_color: &mut Vec3,
) {
    let (p, t, k) = sprite(position, uv, color, c, o);
    *out_position = p;
    *out_uv = t;
    *out_color = k;
}

/// THE ENGINE'S STEREO TERM (WorldMarkers.hlsl has the whole reasoning): each draw moved by its stereo offset, right in
/// the left eye, left in the right one, nothing in a one-view frame; a marker carrying the port's depth tag takes
/// none of it.
#[allow(clippy::too_many_arguments)]
fn stereo_sprite(position: Vec3, uv: Vec2, color: Vec4, offset: f32, two_eyes_only: bool, c: &Camera, o: &Object) -> (Vec4, Vec4, Vec3) {
    let (mut p, t, k) = sprite(position, uv, color, c, o);
    if world_vertex(color.w) || world_text(o) || text_depth(o) > 0.0 || marker_depth(position.z) > 0.0 {
        return (p, t, k);
    }
    p.x += if c.eye == 1 {
        offset
    } else if c.eye == 0 || (two_eyes_only && c.eye != 2) {
        0.0
    } else {
        -offset
    };
    (p, t, k)
}

macro_rules! stereo_variant {
    ($name:ident, $offset:expr, $two:expr) => {
        #[spirv(vertex)]
        #[allow(clippy::too_many_arguments)]
        pub fn $name(
            position: Vec3,
            uv: Vec2,
            color: Vec4,
            #[spirv(uniform, descriptor_set = 0, binding = 1)] c: &Camera,
            #[spirv(uniform, descriptor_set = 0, binding = 5)] o: &Object,
            #[spirv(uniform, descriptor_set = 0, binding = 7)] m: &Material,
            #[spirv(position)] out_position: &mut Vec4,
            out_uv: &mut Vec4,
            out_color: &mut Vec3,
        ) {
            let offset: fn(&Material) -> f32 = $offset;
            let (p, t, k) = stereo_sprite(position, uv, color, offset(m), $two, c, o);
            *out_position = p;
            *out_uv = t;
            *out_color = k;
        }
    };
}
stereo_variant!(sprite_stereo_main, |m| if m.v[1].x != 0.0 { m.v[1].x } else { m.v[2].x }, false);
stereo_variant!(text_stereo_main, |m| if m.v[2].x != 0.0 { m.v[2].x } else { m.v[1].x }, false);
stereo_variant!(sprite_stereo_z_main, |m| if m.v[1].z != 0.0 { m.v[1].z } else { m.v[2].x }, false);
stereo_variant!(sprite_stereo_one_main, |m| m.v[1].x, true);

/// The procedural quad: its corner from the vertex id and the material's size.
#[spirv(vertex)]
#[allow(clippy::too_many_arguments)]
pub fn procedural_main(
    position: Vec3,
    uv: Vec2,
    color: Vec4,
    #[spirv(vertex_index)] id: u32,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] c: &Camera,
    #[spirv(uniform, descriptor_set = 0, binding = 5)] o: &Object,
    #[spirv(uniform, descriptor_set = 0, binding = 7)] m: &Material,
    #[spirv(position)] out_position: &mut Vec4,
    out_uv: &mut Vec4,
    out_color: &mut Vec4,
) {
    let mut color = color;
    let world = world_vertex(color.w) || world_text(o);
    if world_vertex(color.w) {
        color.w = color.w.abs();
    }
    *out_position = project(position, world, c, o);
    let corner = Vec2::new(
        if id == 1 || id == 2 || id == 4 { m.v[1].x } else { 0.0 },
        if id == 2 || id == 4 || id == 5 { m.v[1].y } else { 0.0 },
    );
    *out_uv = Vec4::new(uv.x, uv.y, corner.x, corner.y);
    *out_color = color;
}
