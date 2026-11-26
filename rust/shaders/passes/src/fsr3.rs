//! FSR3's own passes (rust/crates/cvr-render/src/fsr3.rs; main's 2e123a4a3, iPowerTech's per-eye FSR3), formerly the
//! HLSL main compiled at first use:
//!
//!   DILATE (main's Fsr3Dilate.cpp, ported there from AMD's FidelityFX SDK, ffx_fsr3upscaler_reconstruct_dilated_
//!   velocity_and_previous_depth.h): the nearest depth of each pixel's 3x3 (closest to the camera), the motion of
//!   that same tap, and that depth scattered to where the motion says it was a frame before -- the three inputs
//!   FidelityFX's frame interpolation takes (dilated depth, dilated motion, reconstructed previous depth). The
//!   scatter is an atomic on the depth's bits, as main's: the maximum for an inverted depth, the minimum for one that
//!   is not (the nearest either way; a non-negative float's bits keep its order). The shader builder turns the stores
//!   to `scratch` into InterlockedMax (ATOMIC_MAX_STORES) and those to `scratch_min`, the same texture bound again,
//!   into InterlockedMin (ATOMIC_MIN_STORES). Cleared to the far value first, read back as a float after.
//!   WARP (main's Fsr3Extrapolate.cpp): the extrapolated frame -- each output pixel q takes the colour of the source
//!   pixel s whose motion carries it onto q, found by three fixed-point steps of s = q + factor * mv(s).
//!
//! Dilate's bindings (descriptor set 0): 0 the parameters (b0, 6 root constants); 1 t0 depth; 2 t1 motion; 3 u0
//! dilated depth; 4 u1 dilated motion; 5 u2 the scatter's scratch (r32ui); 6 u3 the previous depth; 7 u4 the scratch
//! again. Warp's: 0 the parameters (b0, 5 root constants); 1 t0 the colour; 2 t1 the dilated motion; 3 u0 the output;
//! 4 s0 linear clamp.
use spirv_std::glam::{IVec2, UVec2, UVec3, Vec2, Vec4};
#[cfg(target_arch = "spirv")]
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image, Sampler};

type Texture = Image!(2D, type = f32, sampled);
type OutScalar = Image!(2D, format = r32f, sampled = false);
type OutPair = Image!(2D, format = rg16f, sampled = false);
type Counter = Image!(2D, format = r32ui, sampled = false);
type OutColour = Image!(2D, format = rgba8, sampled = false);

/// Dilate's b0: uint RenderSize.xy, float MvScale.xy, uint InvertedDepth, float ClearValue (the far depth: 0 inverted,
/// 1 not).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DilateParams {
    pub width: u32,
    pub height: u32,
    pub mv_scale_x: f32,
    pub mv_scale_y: f32,
    pub inverted: u32,
    pub clear_value: f32,
}
/// Warp's b0: uint DisplaySize.xy, uint RenderSize.xy, float Factor.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WarpParams {
    pub display_w: u32,
    pub display_h: u32,
    pub render_w: u32,
    pub render_h: u32,
    pub factor: f32,
}

const TAPS: [IVec2; 9] = [
    IVec2::new(0, 0),
    IVec2::new(1, 0),
    IVec2::new(0, 1),
    IVec2::new(0, -1),
    IVec2::new(-1, 0),
    IVec2::new(-1, 1),
    IVec2::new(1, 1),
    IVec2::new(-1, -1),
    IVec2::new(1, -1),
];

fn on_screen(p: IVec2, w: u32, h: u32) -> bool {
    p.x >= 0 && p.y >= 0 && p.x < w as i32 && p.y < h as i32
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn dilate_clear(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &DilateParams,
    #[spirv(descriptor_set = 0, binding = 5)] scratch: &Counter,
) {
    if id.x < k.width && id.y < k.height {
        unsafe { scratch.write(UVec2::new(id.x, id.y), k.clear_value.to_bits()) };
    }
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn dilate_resolve(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &DilateParams,
    #[spirv(descriptor_set = 0, binding = 5)] scratch: &Counter,
    #[spirv(descriptor_set = 0, binding = 6)] previous: &OutScalar,
) {
    if id.x < k.width && id.y < k.height {
        let at = UVec2::new(id.x, id.y);
        let bits: u32 = scratch.read(at);
        unsafe { previous.write(at, f32::from_bits(bits)) };
    }
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn dilate(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &DilateParams,
    #[spirv(descriptor_set = 0, binding = 1)] depth: &Texture,
    #[spirv(descriptor_set = 0, binding = 2)] motion: &Texture,
    #[spirv(descriptor_set = 0, binding = 3)] dilated_depth: &OutScalar,
    #[spirv(descriptor_set = 0, binding = 4)] dilated_motion: &OutPair,
    #[spirv(descriptor_set = 0, binding = 5)] scratch: &Counter,
    #[spirv(descriptor_set = 0, binding = 7)] scratch_min: &Counter,
) {
    let pos = IVec2::new(id.x as i32, id.y as i32);
    if id.x >= k.width || id.y >= k.height {
        return;
    }
    let first: Vec4 = depth.fetch(pos);
    let mut nearest = first.x;
    let mut nearest_at = pos;
    let mut i = 1;
    while i < 9 {
        let p = pos + TAPS[i];
        if on_screen(p, k.width, k.height) {
            let d: Vec4 = depth.fetch(p);
            let closer = if k.inverted != 0 { d.x > nearest } else { d.x < nearest };
            if closer {
                nearest = d.x;
                nearest_at = p;
            }
        }
        i += 1;
    }
    let m: Vec4 = motion.fetch(nearest_at);
    let mut mv = Vec2::new(m.x * k.mv_scale_x, m.y * k.mv_scale_y);
    let at = UVec2::new(id.x, id.y);
    unsafe {
        dilated_depth.write(at, nearest);
        dilated_motion.write(at, mv);
    }
    // the dilated depth scattered to where the motion says it was a frame before
    let size = Vec2::new(k.width as f32, k.height as f32);
    let px = mv * size;
    if (px.x * px.x + px.y * px.y).sqrt() <= 0.1 {
        mv = Vec2::ZERO;
    }
    let uv = (Vec2::new(pos.x as f32, pos.y as f32) + 0.5) / size;
    let reproj = (uv + mv) * size - 0.5;
    let base = IVec2::new(reproj.x.floor() as i32, reproj.y.floor() as i32);
    let frac = reproj - Vec2::new(base.x as f32, base.y as f32);
    let weights = [(1.0 - frac.x) * (1.0 - frac.y), frac.x * (1.0 - frac.y), (1.0 - frac.x) * frac.y, frac.x * frac.y];
    let offsets = [IVec2::new(0, 0), IVec2::new(1, 0), IVec2::new(0, 1), IVec2::new(1, 1)];
    let bits = nearest.to_bits();
    let mut j = 0;
    while j < 4 {
        let p = base + offsets[j];
        if weights[j] > 0.01 && on_screen(p, k.width, k.height) {
            let at = UVec2::new(p.x as u32, p.y as u32);
            // the atomics (ATOMIC_MAX_STORES, ATOMIC_MIN_STORES)
            if k.inverted != 0 {
                unsafe { scratch.write(at, bits) };
            } else {
                unsafe { scratch_min.write(at, bits) };
            }
        }
        j += 1;
    }
}

/// The dilated motion at uv, point-sampled (the render size's pixel, held inside the texture).
fn motion_at(motion: &Texture, uv: Vec2, render: Vec2, k: &WarpParams) -> Vec2 {
    let mx = inside((uv.x * render.x) as i32, k.render_w as i32 - 1);
    let my = inside((uv.y * render.y) as i32, k.render_h as i32 - 1);
    let m: Vec4 = motion.fetch(IVec2::new(mx, my));
    Vec2::new(m.x, m.y)
}
fn inside(v: i32, most: i32) -> i32 {
    if v < 0 {
        0
    } else if v > most {
        most
    } else {
        v
    }
}
fn unit(v: f32) -> f32 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn warp(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &WarpParams,
    #[spirv(descriptor_set = 0, binding = 1)] colour: &Texture,
    #[spirv(descriptor_set = 0, binding = 2)] motion: &Texture,
    #[spirv(descriptor_set = 0, binding = 3)] output: &OutColour,
    #[spirv(descriptor_set = 0, binding = 4)] linear: &Sampler,
) {
    if id.x >= k.display_w || id.y >= k.display_h {
        return;
    }
    let display = Vec2::new(k.display_w as f32, k.display_h as f32);
    let render = Vec2::new(k.render_w as f32, k.render_h as f32);
    let q = (Vec2::new(id.x as f32, id.y as f32) + 0.5) / display;
    // the source pixel s that lands on q: s - factor * mv(s) = q; point-sampled vectors, so an object's edge keeps
    // one motion instead of a blend of its own and the background's
    // three steps, written out (no loop, no clamp: rust-gpu's clamp asserts its bounds, a path DXC then drops)
    let s = q + k.factor * motion_at(motion, q, render, k);
    let s = q + k.factor * motion_at(motion, s, render, k);
    let s = q + k.factor * motion_at(motion, s, render, k);
    let s = Vec2::new(unit(s.x), unit(s.y));
    let c: Vec4 = colour.sample_by_lod(*linear, s, 0.0);
    unsafe { output.write(UVec2::new(id.x, id.y), c) };
}
