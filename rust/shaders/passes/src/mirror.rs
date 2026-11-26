//! The desktop mirror's present conversion (rust/crates/cvr-render/src/mirror.rs), formerly its HLSL: the mirror
//! texture (a packed-float HDR target, not copy-compatible with the 8-bit backbuffer) loaded by the pixel, encoded to
//! sRGB and dithered with two hashed noises (the second weighted towards the ends of the range). The arithmetic as
//! FXC's listing has it (`fxc /T ps_5_0 /E PSMain`, the default /O1): `510 - 510e` is `mad(-e, 510, 510)`, `(u * 2) * v`
//! a `dp2` of (u, u) and (v, v), the lerp and the blend `mad`s.
//!
//! Bindings (descriptor set 0): 0 the time (b0, one root constant), 1 the mirror texture (t0).
use crate::common::{dot2, dot3};
use spirv_std::glam::{IVec2, Vec2, Vec3, Vec4, Vec4Swizzles};
#[cfg(target_arch = "spirv")]
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image};

type Texture = Image!(2D, type = f32, sampled);

/// b0: float gT.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Time {
    pub t: f32,
}

/// The full-screen triangle: uv = ((vid << 1) & 2, vid & 2), position mad(uv.x, 2, -1), mad(-uv.y, 2, 1).
#[spirv(vertex)]
pub fn vs(#[spirv(vertex_index)] vid: u32, #[spirv(position)] pos: &mut Vec4, uv: &mut Vec2) {
    let u = ((vid << 1) & 2) as f32;
    let v = (vid & 2) as f32;
    *pos = Vec4::new(u.mul_add(2.0, -1.0), (-v).mul_add(2.0, 1.0), 0.0, 1.0);
    *uv = Vec2::new(u, v);
}

fn frac(x: f32) -> f32 {
    x - x.floor()
}
fn mad3(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    Vec3::new(a.x.mul_add(b.x, c.x), a.y.mul_add(b.y, c.y), a.z.mul_add(b.z, c.z))
}

/// One of the two noises at a pixel p and the time t: d = dot((a, t), (a.yx + 33.33, t + 33.33)) with a = frac(p * k),
/// u = a.x + d, v = a.y + d, w = u + v; (frac((d + t) * w), frac(dp2((u, u), (v, v))), frac(u * w)) - 0.5.
fn noise(p: Vec2, t: f32) -> Vec3 {
    let ax = frac(p.x * 211.1488);
    let ay = frac(p.y * 210.944);
    let d = dot3(Vec3::new(ax, ay, t), Vec3::new(ay + 33.33, ax + 33.33, t + 33.33));
    let u = ax + d;
    let v = ay + d;
    let w = v + u;
    Vec3::new(frac((t + d) * w), frac(dot2(Vec2::new(u, u), Vec2::new(v, v))), frac(u * w)) - 0.5
}

#[spirv(fragment)]
pub fn ps(
    #[spirv(frag_coord)] pos: Vec4,
    _uv: Vec2,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] time: &Time,
    #[spirv(descriptor_set = 0, binding = 1)] mirror: &Texture,
    out: &mut Vec4,
) {
    let texel: Vec4 = mirror.fetch(IVec2::new(pos.x as i32, pos.y as i32));
    let c = texel.xyz().max(Vec3::ZERO);
    // sRGB: lerp(c * 12.92, 1.055 * pow(c, 1 / 2.4) - 0.055, step(0.0031308, c)) as FXC wrote it
    let powered = Vec3::new((c.x.log2() * (1.0 / 2.4)).exp2(), (c.y.log2() * (1.0 / 2.4)).exp2(), (c.z.log2() * (1.0 / 2.4)).exp2());
    let high = mad3(powered, Vec3::splat(1.055), Vec3::splat(-0.055));
    let rise = mad3(-c, Vec3::splat(12.92), high);
    let step = Vec3::new(
        if c.x >= 0.0031308 { 1.0 } else { 0.0 },
        if c.y >= 0.0031308 { 1.0 } else { 0.0 },
        if c.z >= 0.0031308 { 1.0 } else { 0.0 },
    );
    let e = mad3(step, rise, c * 12.92);
    // the second noise's weight: min(min(1, 510e), 510 - 510e)
    let edge = (e * 510.0).min(Vec3::ONE).min(mad3(-e, Vec3::splat(510.0), Vec3::splat(510.0)));
    let t = frac(time.t * 6.2272);
    let p = pos.xy();
    let n1 = noise(p, t);
    let n2 = noise(p + 64.0, t);
    let o = mad3(mad3(edge, n2, n1), Vec3::splat(1.0 / 255.0), e);
    *out = Vec4::new(o.x, o.y, o.z, 1.0);
}
