//! The space quads (rust/crates/cvr-gpu/src/space_quad.rs): a quad given by its four corners in clip space, the
//! picture on it read through its mips by each pixel's footprint (s0, anisotropic), or at one level by one bilinear tap
//! for the resample pass (s1). Formerly the HLSL in space_quad.rs (VSMain, PSFill, PSOver, PSCopy, PSLight).
//!
//! Bindings (descriptor set 0): 0 the quad's constants (b0, 28 root constants), 1 the picture (t0), 2 the footprint
//! sampler (s0), 3 the linear sampler (s1).
use spirv_std::glam::{vec2, Vec2, Vec4};
use spirv_std::{spirv, Image, Sampler};

/// b0: the four corners as clip coordinates (top left, top right, bottom left, bottom right); the picture's part on the
/// quad (u0, v0, u1, v1); x = opacity, y = black point; the fill's colour.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Quad {
    pub corner: [Vec4; 4],
    pub uv: Vec4,
    pub params: Vec4,
    pub colour: Vec4,
}

type Picture = Image!(2D, type = f32, sampled);

/// The corner the vertex is, and its place in the picture: lerp(uv.xy, uv.zw, (id & 1, id >> 1)) as HLSL's lerp
/// computes it, a + t * (b - a).
#[spirv(vertex)]
pub fn vs(
    #[spirv(vertex_index)] id: i32,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] quad: &Quad,
    #[spirv(position)] out_position: &mut Vec4,
    out_uv: &mut Vec2,
) {
    *out_position = quad.corner[id as usize];
    let t = vec2((id & 1) as f32, (id >> 1) as f32);
    let a = vec2(quad.uv.x, quad.uv.y);
    *out_uv = a + t * (vec2(quad.uv.z, quad.uv.w) - a);
}

/// A solid fill in the quad's colour (blended by its alpha).
#[spirv(fragment)]
pub fn fill(#[spirv(uniform, descriptor_set = 0, binding = 0)] quad: &Quad, out_color: &mut Vec4) {
    *out_color = quad.colour;
}

/// The picture over what is there, at the quad's opacity.
#[spirv(fragment)]
pub fn over(
    uv: Vec2,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] quad: &Quad,
    #[spirv(descriptor_set = 0, binding = 1)] picture: &Picture,
    #[spirv(descriptor_set = 0, binding = 2)] footprint: &Sampler,
    out_color: &mut Vec4,
) {
    let c: Vec4 = picture.sample(*footprint, uv);
    *out_color = c.truncate().extend(quad.params.x);
}

/// The resample: one level, one bilinear tap, opaque.
#[spirv(fragment)]
pub fn copy(
    uv: Vec2,
    #[spirv(descriptor_set = 0, binding = 1)] picture: &Picture,
    #[spirv(descriptor_set = 0, binding = 3)] linear: &Sampler,
    out_color: &mut Vec4,
) {
    let c: Vec4 = picture.sample_by_lod(*linear, uv, 0.0);
    *out_color = c.truncate().extend(1.0);
}

/// The picture as light (added): below the black point a pixel fades out by its brightest channel, the rest scaled
/// by the opacity.
#[spirv(fragment)]
pub fn light(
    uv: Vec2,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] quad: &Quad,
    #[spirv(descriptor_set = 0, binding = 1)] picture: &Picture,
    #[spirv(descriptor_set = 0, binding = 2)] footprint: &Sampler,
    out_color: &mut Vec4,
) {
    let sampled: Vec4 = picture.sample(*footprint, uv);
    let mut c = sampled.truncate();
    let black = quad.params.y;
    if black > 0.0 {
        let peak = c.x.max(c.y.max(c.z));
        // HLSL's saturate: max then min, NaN to 0 as saturate makes it (fmax/fmin return the number)
        let k = ((peak - black) / (peak * (1.0 - black)).max(1e-5)).max(0.0).min(1.0);
        c *= k;
    }
    *out_color = (c * quad.params.x).extend(0.0);
}
