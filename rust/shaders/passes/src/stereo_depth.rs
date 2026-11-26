//! The stereo depth resolve (rust/crates/cvr-gpu/src/stereo_depth.rs), formerly its HLSL: the rasterized depth sampled
//! at the jitter's offset, so it describes the unjittered colour and projection sent to XR -- interpolated where the
//! four texels lie on one planar surface (their distances within 5%), else the nearest texel (never a surface invented
//! between foreground and background).
//!
//! A MIDPOINT'S DEPTH (main's 0.2.4): B's depth moved the given share of B's motion vectors back toward A -- each pixel
//! takes what B had at s, where s plus that share of B's vector lands on the pixel (two steps of that fixed point).
//!
//! Bindings (descriptor set 0): 0 the parameters (b0, 12 root constants), 1 the source depth (t0), 2 B's motion vectors
//! (t1, read only for a midpoint), 3 the output (u0).
use spirv_std::glam::{IVec2, UVec2, UVec3, Vec2, Vec4};
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image};

type Depth = Image!(2D, type = f32, sampled);
type OutDepth = Image!(2D, format = r32f, sampled = false);

/// b0: uint2 size; float2 jitter; float nearPlane; float farPlane; uint inverted; uint warp; float2 motionToPixels;
/// uint2 motionSize.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Parameters {
    pub size_x: u32,
    pub size_y: u32,
    pub jitter_x: f32,
    pub jitter_y: f32,
    pub near_plane: f32,
    pub far_plane: f32,
    pub inverted: u32,
    pub warp: u32,
    pub motion_to_pixels_x: f32,
    pub motion_to_pixels_y: f32,
    pub motion_size_x: u32,
    pub motion_size_y: u32,
}

/// One step of the fixed point: the pixel B's vector at s comes from.
fn step(motion: &Depth, centre: Vec2, s: Vec2, k: &Parameters) -> Vec2 {
    let mx = ((s.x * k.motion_size_x as f32 / k.size_x as f32) as i32).max(0).min(k.motion_size_x as i32 - 1);
    let my = ((s.y * k.motion_size_y as f32 / k.size_y as f32) as i32).max(0).min(k.motion_size_y as i32 - 1);
    let v: Vec4 = motion.fetch(IVec2::new(mx, my));
    Vec2::new(centre.x - v.x * k.motion_to_pixels_x, centre.y - v.y * k.motion_to_pixels_y)
}

/// HLSL's isfinite / isinf by the bits (Rust's compare with a literal infinity, which naga refuses as a constant).
fn finite(x: f32) -> bool {
    x.to_bits() & 0x7f80_0000 != 0x7f80_0000
}
fn infinite(x: f32) -> bool {
    x.to_bits() & 0x7fff_ffff == 0x7f80_0000
}
fn clean(d: f32, k: &Parameters) -> f32 {
    if finite(d) && d >= 0.0 && d <= 1.0 {
        d
    } else if k.inverted != 0 {
        0.0
    } else {
        1.0
    }
}
fn distance_at(d: f32, k: &Parameters) -> f32 {
    let d = if k.inverted != 0 { 1.0 - d } else { d };
    if infinite(k.far_plane) {
        return k.near_plane / (1.0 - d).max(1e-20);
    }
    k.near_plane * k.far_plane / (k.far_plane - d * (k.far_plane - k.near_plane)).max(1e-20)
}
fn load(t: &Depth, at: IVec2, k: &Parameters) -> f32 {
    let p = IVec2::new(at.x.max(0).min(k.size_x as i32 - 1), at.y.max(0).min(k.size_y as i32 - 1));
    let v: Vec4 = t.fetch(p);
    clean(v.x, k)
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn main(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Parameters,
    #[spirv(descriptor_set = 0, binding = 1)] source: &Depth,
    #[spirv(descriptor_set = 0, binding = 2)] motion: &Depth,
    #[spirv(descriptor_set = 0, binding = 3)] output: &OutDepth,
) {
    if id.x >= k.size_x || id.y >= k.size_y {
        return;
    }
    let mut at = Vec2::new(id.x as f32, id.y as f32);
    if k.warp != 0 {
        // a midpoint's pixel shows what B had at s: two steps of the fixed point from the pixel itself
        let centre = at + Vec2::splat(0.5);
        let s = step(motion, centre, centre, k);
        let s = step(motion, centre, s, k);
        at = Vec2::new(
            (s.x - 0.5).max(0.0).min(k.size_x as f32 - 1.0),
            (s.y - 0.5).max(0.0).min(k.size_y as f32 - 1.0),
        );
    }
    // Native 0xC28EC0 adds +2*jitterX/W, +2*jitterY/H to NDC; CameraData already flips y for texture coordinates.
    let px = at.x + k.jitter_x;
    let py = at.y + k.jitter_y;
    let b = IVec2::new(px.floor() as i32, py.floor() as i32);
    let wx = px - px.floor();
    let wy = py - py.floor();
    let d0 = load(source, b, k);
    let d1 = load(source, b + IVec2::new(1, 0), k);
    let d2 = load(source, b + IVec2::new(0, 1), k);
    let d3 = load(source, b + IVec2::new(1, 1), k);
    let (z0, z1, z2, z3) = (distance_at(d0, k), distance_at(d1, k), distance_at(d2, k), distance_at(d3, k));
    let lo = z0.min(z1).min(z2.min(z3));
    let hi = z0.max(z1).max(z2.max(z3));
    // Device depth is affine over a planar surface: interpolated there, never blended across an edge.
    let value = if hi <= lo * 1.05 {
        let top = d0 + wx * (d1 - d0);
        let bottom = d2 + wx * (d3 - d2);
        top + wy * (bottom - top)
    } else {
        load(source, IVec2::new((px + 0.5).floor() as i32, (py + 0.5).floor() as i32), k)
    };
    unsafe { output.write(UVec2::new(id.x, id.y), value) };
}
