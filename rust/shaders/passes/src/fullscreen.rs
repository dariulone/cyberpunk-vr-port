//! The full-screen passes (rust/crates/cvr-gpu): the sRGB decode of an eye (srgb_pass.rs), the contrast-adaptive
//! sharpening (sharpen_pass.rs) and the depth copy into a depth target (depth_resolve.rs) -- formerly their HLSL.
//! Each draws one triangle over the target: (-1, 1), (3, 1), (-1, -3); the uv one, where it has one, (0, 0), (2, 0),
//! (0, 2).
use spirv_std::glam::{vec2, vec4, IVec2, UVec2, Vec2, Vec3, Vec4};
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image, Sampler};

type Picture = Image!(2D, type = f32, sampled);

fn corner(vertex_id: i32) -> Vec2 {
    match vertex_id {
        0 => vec2(-1.0, 1.0),
        1 => vec2(3.0, 1.0),
        _ => vec2(-1.0, -3.0),
    }
}
fn corner_uv(vertex_id: i32) -> Vec2 {
    match vertex_id {
        0 => vec2(0.0, 0.0),
        1 => vec2(2.0, 0.0),
        _ => vec2(0.0, 2.0),
    }
}
/// HLSL's lerp(a, b, t) as it computes it: a + t * (b - a).
fn lerp3(a: Vec3, b: Vec3, t: Vec3) -> Vec3 {
    a + t * (b - a)
}
/// HLSL's step(edge, x): 1 where x >= edge.
fn step3(edge: f32, x: Vec3) -> Vec3 {
    Vec3::new(if x.x >= edge { 1.0 } else { 0.0 }, if x.y >= edge { 1.0 } else { 0.0 }, if x.z >= edge { 1.0 } else { 0.0 })
}
/// HLSL's saturate: max then min (NaN to 0, as saturate makes it).
fn saturate(x: f32) -> f32 {
    x.max(0.0).min(1.0)
}
fn saturate3(v: Vec3) -> Vec3 {
    Vec3::new(saturate(v.x), saturate(v.y), saturate(v.z))
}

// ---------------------------------------------------------------- srgb_pass.rs: b0 one root constant, t0
/// The vertex shader of the sRGB decode: the corner only.
#[spirv(vertex)]
pub fn srgb_vs(#[spirv(vertex_index)] vertex_id: i32, #[spirv(position)] out_position: &mut Vec4) {
    let p = corner(vertex_id);
    *out_position = vec4(p.x, p.y, 0.0, 1.0);
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct ConversionParams {
    pub manually_decode_srgb: u32,
}

/// The eye's bytes decoded when the swapchain does not declare them sRGB: lerp(low, high, step(0.04045, value)).
#[spirv(fragment)]
pub fn srgb_ps(
    #[spirv(frag_coord)] position: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &ConversionParams,
    #[spirv(descriptor_set = 0, binding = 1)] source: &Picture,
    out_color: &mut Vec4,
) {
    let encoded: Vec4 = source.fetch(IVec2::new(position.x as i32, position.y as i32));
    let value = encoded.truncate();
    let color = if params.manually_decode_srgb != 0 {
        let low = value / 12.92;
        let high = Vec3::new(
            ((value.x + 0.055) / 1.055).powf(2.4),
            ((value.y + 0.055) / 1.055).powf(2.4),
            ((value.z + 0.055) / 1.055).powf(2.4),
        );
        lerp3(low, high, step3(0.04045, value))
    } else {
        value
    };
    *out_color = color.extend(encoded.w);
}

// ---------------------------------------------------------------- the passes with a uv: sharpen and depth resolve
/// The corner and its uv (sharpen_pass.rs and depth_resolve.rs share it).
#[spirv(vertex)]
pub fn uv_vs(#[spirv(vertex_index)] vertex_id: i32, #[spirv(position)] out_position: &mut Vec4, out_uv: &mut Vec2) {
    let p = corner(vertex_id);
    *out_position = vec4(p.x, p.y, 0.0, 1.0);
    *out_uv = corner_uv(vertex_id);
}

/// sharpen_pass.rs, b0: x = emphasis, y = 4 * amount, z = the target view is typed sRGB.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct FilterParams {
    pub param: Vec4,
}

/// AMD's contrast-adaptive sharpening (CAS), as the HLSL had it: the 3x3 neighbourhood by point taps of level 0, the
/// sharpening weight from the local contrast, mixed with the centre by the amount; decoded to linear when the target
/// encodes sRGB itself.
#[spirv(fragment)]
pub fn sharpen_ps(
    uv: Vec2,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] filter: &FilterParams,
    #[spirv(descriptor_set = 0, binding = 1)] source: &Picture,
    #[spirv(descriptor_set = 0, binding = 2)] point: &Sampler,
    out_color: &mut Vec4,
) {
    let size: UVec2 = source.query_size_lod(0);
    let inv = vec2(1.0 / size.x as f32, 1.0 / size.y as f32);
    let tap = |off: Vec2| -> Vec3 {
        let c: Vec4 = source.sample_by_lod(*point, uv + off, 0.0);
        c.truncate()
    };
    let a = tap(vec2(-inv.x, -inv.y));
    let b = tap(vec2(0.0, -inv.y));
    let c = tap(vec2(inv.x, -inv.y));
    let d = tap(vec2(-inv.x, 0.0));
    let e = tap(vec2(0.0, 0.0));
    let f = tap(vec2(inv.x, 0.0));
    let g = tap(vec2(-inv.x, inv.y));
    let hh = tap(vec2(0.0, inv.y));
    let i = tap(vec2(inv.x, inv.y));

    let mut mn = d.min(e).min(f.min(b)).min(hh);
    mn += mn.min(a.min(c).min(g.min(i)));
    let mut mx = d.max(e).max(f.max(b)).max(hh);
    mx += mx.max(a.max(c).max(g.max(i)));

    let m = mx.max(Vec3::splat(1e-5));
    let rcp_mx = Vec3::new(1.0 / m.x, 1.0 / m.y, 1.0 / m.z);
    let mut amp = saturate3(mn.min(Vec3::splat(2.0) - mx) * rcp_mx);
    amp = Vec3::new(amp.x.sqrt(), amp.y.sqrt(), amp.z.sqrt());

    let peak = -1.0 / (8.0 - 3.0 * saturate(filter.param.x));
    let wt = amp * peak;
    let denominator = Vec3::splat(1.0) + 4.0 * wt;
    let rcp_w = Vec3::new(1.0 / denominator.x, 1.0 / denominator.y, 1.0 / denominator.z);
    let sharp = saturate3((b * wt + d * wt + f * wt + hh * wt + e) * rcp_w);

    let mut out = saturate3(lerp3(e, sharp, Vec3::splat(filter.param.y)));
    if filter.param.z > 0.5 {
        let decode = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        out = Vec3::new(decode(out.x), decode(out.y), decode(out.z));
    }
    *out_color = out.extend(1.0);
}

/// depth_resolve.rs: the source depth (t0) by a point tap of level 0 (s0), written as the depth.
#[spirv(fragment(depth_replacing))]
pub fn depth_resolve_ps(
    uv: Vec2,
    #[spirv(descriptor_set = 0, binding = 1)] source: &Picture,
    #[spirv(descriptor_set = 0, binding = 2)] point: &Sampler,
    #[spirv(frag_depth)] out_depth: &mut f32,
) {
    let d: Vec4 = source.sample_by_lod(*point, uv, 0.0);
    *out_depth = d.x;
}
