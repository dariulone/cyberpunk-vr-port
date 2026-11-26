//! The foveated passes (rust/crates/cvr-gpu/src/foveated_gpu.rs), formerly its HLSL: Prepare -- the centre of an eye
//! resampled into the smaller input the upscaler takes (colour by a bilinear tap, depth and motion by the pixel, a
//! downsampled depth choosing its surface with its motion at a discontinuity) -- and Composite -- the upscaled centre
//! blended into the periphery over a border of 4% of the region.
//!
//! Bindings (descriptor set 0): 0 the constants (b0), 1-5 Color, Depth, Motion, Center, Peripheral (t0-t4), 6-9
//! ColorOut, DepthOut, MotionOut, Output (u0-u3), 10 the linear clamp sampler (s0). The storage images are declared in
//! float formats so that naga writes HLSL's format-free RWTexture2D<float4 / float / float2>, as the HLSL had them.
use spirv_std::glam::{vec2, vec4, IVec2, UVec2, UVec3, Vec4};
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image, Sampler};

type Picture = Image!(2D, type = f32, sampled);
type OutColor = Image!(2D, format = rgba16f, sampled = false);
type OutDepth = Image!(2D, format = r32f, sampled = false);
type OutMotion = Image!(2D, format = rg16f, sampled = false);

/// b0, one scalar a field: HLSL's packing of float4 sourceRect; uint4 dimensions (prepared input w/h, original render
/// w/h); uint4 bases (original colour and depth origins); uint2 motionBase; float2 pixelScale; uint4 flags (low-res
/// motion, inverted depth, original output w/h); uint4 region (output x/y/w/h).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Constants {
    pub rect_x: f32,
    pub rect_y: f32,
    pub rect_w: f32,
    pub rect_h: f32,
    pub input_w: u32,
    pub input_h: u32,
    pub render_w: u32,
    pub render_h: u32,
    pub color_base_x: u32,
    pub color_base_y: u32,
    pub depth_base_x: u32,
    pub depth_base_y: u32,
    pub motion_base_x: u32,
    pub motion_base_y: u32,
    pub pixel_scale_x: f32,
    pub pixel_scale_y: f32,
    pub low_res_motion: u32,
    pub inverted_depth: u32,
    pub output_w: u32,
    pub output_h: u32,
    pub region_x: u32,
    pub region_y: u32,
    pub region_w: u32,
    pub region_h: u32,
}

fn saturate(x: f32) -> f32 {
    x.max(0.0).min(1.0)
}
fn clamp2(v: IVec2, hi: IVec2) -> IVec2 {
    IVec2::new(v.x.max(0).min(hi.x), v.y.max(0).min(hi.y))
}

#[spirv(compute(threads(8, 8, 1)))]
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Constants,
    #[spirv(descriptor_set = 0, binding = 1)] color: &Picture,
    #[spirv(descriptor_set = 0, binding = 2)] depth_t: &Picture,
    #[spirv(descriptor_set = 0, binding = 3)] motion_t: &Picture,
    #[spirv(descriptor_set = 0, binding = 6)] color_out: &OutColor,
    #[spirv(descriptor_set = 0, binding = 7)] depth_out: &OutDepth,
    #[spirv(descriptor_set = 0, binding = 8)] motion_out: &OutMotion,
    #[spirv(descriptor_set = 0, binding = 10)] linear: &Sampler,
) {
    if id.x >= k.input_w || id.y >= k.input_h {
        return;
    }
    let at = UVec2::new(id.x, id.y);
    let uv = vec2((id.x as f32 + 0.5) / k.input_w as f32, (id.y as f32 + 0.5) / k.input_h as f32);
    let pixel = vec2(k.rect_x, k.rect_y) + uv * vec2(k.rect_w, k.rect_h);
    let size: UVec2 = color.query_size_lod(0);
    let c: Vec4 = color.sample_by_lod(
        *linear,
        vec2((k.color_base_x as f32 + pixel.x) / size.x as f32, (k.color_base_y as f32 + pixel.y) / size.y as f32),
        0.0,
    );
    unsafe { color_out.write(at, c) };
    let last = IVec2::new(k.render_w as i32 - 1, k.render_h as i32 - 1);
    let base = IVec2::new(k.depth_base_x as i32, k.depth_base_y as i32);
    let mut p = clamp2(IVec2::new(pixel.x.floor() as i32, pixel.y.floor() as i32), last);
    let first: Vec4 = depth_t.fetch(base + p);
    let mut depth = first.x;
    // Downsampling selects a surface and its motion together at discontinuities: the 2x2 written out (a loop whose
    // result is read after it loses its last iteration in naga's HLSL).
    if k.rect_w / k.input_w as f32 > 1.0 || k.rect_h / k.input_h as f32 > 1.0 {
        let corner = IVec2::new((pixel.x - 0.5).floor() as i32, (pixel.y - 0.5).floor() as i32);
        let mut consider = |o: IVec2| {
            let q = clamp2(corner + o, last);
            let s: Vec4 = depth_t.fetch(base + q);
            let d = s.x;
            let nearer = if k.inverted_depth != 0 { d > depth } else { d < depth };
            if nearer {
                depth = d;
                p = q;
            }
        };
        consider(IVec2::new(0, 0));
        consider(IVec2::new(1, 0));
        consider(IVec2::new(0, 1));
        consider(IVec2::new(1, 1));
    }
    unsafe { depth_out.write(at, depth) };
    let scale = if k.low_res_motion != 0 {
        vec2(k.render_w as f32, k.render_h as f32)
    } else {
        vec2(k.output_w as f32, k.output_h as f32)
    };
    let mpos = vec2((p.x as f32 + 0.5) / k.render_w as f32, (p.y as f32 + 0.5) / k.render_h as f32) * scale;
    let m = UVec2::new(k.motion_base_x + mpos.x as u32, k.motion_base_y + mpos.y as u32);
    let motion: Vec4 = motion_t.fetch(IVec2::new(m.x as i32, m.y as i32));
    unsafe { motion_out.write(at, vec2(motion.x, motion.y) * vec2(k.pixel_scale_x, k.pixel_scale_y)) };
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn composite(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Constants,
    #[spirv(descriptor_set = 0, binding = 4)] center: &Picture,
    #[spirv(descriptor_set = 0, binding = 5)] peripheral_t: &Picture,
    #[spirv(descriptor_set = 0, binding = 9)] output: &OutColor,
    #[spirv(descriptor_set = 0, binding = 10)] linear: &Sampler,
) {
    if id.x >= k.render_w || id.y >= k.render_h {
        return;
    }
    let uv = vec2((id.x as f32 + 0.5) / k.render_w as f32, (id.y as f32 + 0.5) / k.render_h as f32);
    let peripheral: Vec4 = peripheral_t.sample_by_lod(*linear, uv, 0.0);
    let local = vec2(id.x as f32 + 0.5 - k.region_x as f32, id.y as f32 + 0.5 - k.region_y as f32);
    let edge = local.min(vec2(k.region_w as f32, k.region_h as f32) - local);
    let blend = saturate(edge.x.min(edge.y) / (0.04 * k.region_w.min(k.region_h) as f32).max(1.0));
    let mut value = peripheral;
    if blend > 0.0 {
        let source = vec2(
            (id.x as f32 + 0.5) * k.input_w as f32 / k.render_w as f32,
            (id.y as f32 + 0.5) * k.input_h as f32 / k.render_h as f32,
        );
        let center_uv = vec2((source.x - k.rect_x) / k.rect_w, (source.y - k.rect_y) / k.rect_h);
        let c: Vec4 = center.sample_by_lod(*linear, center_uv, 0.0);
        value = peripheral + blend * (c - peripheral);
    }
    if k.low_res_motion != 0 && edge.x >= 0.0 && edge.y >= 0.0 && edge.x.min(edge.y) < 3.0 {
        value = vec4(1.0, 0.0, 0.0, 1.0);
    }
    unsafe { output.write(UVec2::new(id.x, id.y), value) };
}
