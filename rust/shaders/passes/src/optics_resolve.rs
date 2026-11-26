//! The optics' source resolves (src/Shaders/camera_source_resolve_cs.hlsl, lens_source_resolve_cs.hlsl, formerly
//! DXC's): the camera scope's image accumulated over frames as TAA would have (the scope's RTT view keeps no history),
//! history reprojected by the camera's rotation, resampled by a five-tap Catmull-Rom, clipped to the current 3x3 in
//! tonemapped YCoCg; an isolated dark dot taken as the mean of its brighter neighbours. The operations in the HLSL's
//! order (DXC compiled both: no reassociation, `lerp` as `x + s * (y - x)`, `dot` and `length` one Dot each).
//!
//! The root signature is embedded (the plugin makes it from the DXIL): the builder's ROOT_SIGNATURES table.
//! Bindings (descriptor set 0): 0 the constants (b0, 20 root constants), 1 current (t0), 2 history (t1), 3 the history
//! out (u0), 4 the linear clamp sampler (s0, static).
use crate::common::{div, dot2, dot3, fmax, fmin, length_of2, max3, min3};
use spirv_std::glam::{IVec2, UVec2, UVec3, Vec2, Vec3, Vec3Swizzles, Vec4, Vec4Swizzles};
#[cfg(target_arch = "spirv")]
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image, Sampler};

type Texture = Image!(2D, type = f32, sampled);
type OutColour = Image!(2D, format = rgba16f, sampled = false);

/// b0 (camera): uint2 size; float2 inverseSize; float alpha, motionAlpha, gamma; uint reset; float4 row0..2 (current
/// NDC (x, y, 1) -> previous homogeneous NDC).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Camera {
    pub size_x: u32,
    pub size_y: u32,
    pub inverse_x: f32,
    pub inverse_y: f32,
    pub alpha: f32,
    pub motion_alpha: f32,
    pub gamma: f32,
    pub reset: u32,
    pub row0: Vec4,
    pub row1: Vec4,
    pub row2: Vec4,
}

fn peak(c: Vec3) -> f32 {
    fmax(c.x, fmax(c.y, c.z))
}
fn compress(c: Vec3) -> Vec3 {
    c / (1.0 + peak(c))
}
fn expand(c: Vec3) -> Vec3 {
    c / fmax(1.0 - peak(c), 1e-4)
}
fn to_ycocg(c: Vec3) -> Vec3 {
    Vec3::new(dot3(c, Vec3::new(0.25, 0.5, 0.25)), dot3(c, Vec3::new(0.5, 0.0, -0.5)), dot3(c, Vec3::new(-0.25, 0.5, -0.25)))
}
fn from_ycocg(c: Vec3) -> Vec3 {
    Vec3::new(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z)
}
fn finite3(c: Vec3) -> bool {
    let f = |x: f32| x.to_bits() & 0x7f80_0000 != 0x7f80_0000;
    f(c.x) && f(c.y) && f(c.z)
}
/// HLSL's lerp as DXC writes it: x + s * (y - x).
fn lerp(x: f32, y: f32, s: f32) -> f32 {
    x + s * (y - x)
}
fn lerp3(x: Vec3, y: Vec3, s: f32) -> Vec3 {
    x + s * (y - x)
}
/// a / b per component, kept divisions (common::div).
fn div3(a: Vec3, b: f32) -> Vec3 {
    Vec3::new(div(a.x, b), div(a.y, b), div(a.z, b))
}
fn saturate(x: f32) -> f32 {
    fmin(fmax(x, 0.0), 1.0)
}

fn load(current: &Texture, p: IVec2, size: IVec2) -> Vec3 {
    let q = IVec2::new(p.x.max(0).min(size.x - 1), p.y.max(0).min(size.y - 1));
    let c: Vec4 = current.fetch(q);
    let c = c.xyz();
    let keep = if finite3(c) { 1.0 } else { 0.0 };
    to_ycocg(compress(max3(c, Vec3::ZERO) * keep))
}

/// Five bilinear taps of a Catmull-Rom filter: history is resampled every frame the camera moves and a bilinear lookup
/// would blur it a little more each time.
fn history_at(history: &Texture, linear: &Sampler, uv: Vec2, size: Vec2, inverse: Vec2) -> Vec3 {
    let position = uv * size;
    let centre = (position - 0.5).floor() + 0.5;
    let f = position - centre;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let t0 = (centre - 1.0) * inverse;
    let t3 = (centre + 2.0) * inverse;
    let t12 = (centre + w2 / w12) * inverse;
    let tap = |at: Vec2| -> Vec3 {
        let c: Vec4 = history.sample_by_lod(*linear, at, 0.0);
        c.xyz()
    };
    let mut sum = tap(Vec2::new(t12.x, t0.y)) * (w12.x * w0.y);
    sum += tap(Vec2::new(t0.x, t12.y)) * (w0.x * w12.y);
    sum += tap(t12) * (w12.x * w12.y);
    sum += tap(Vec2::new(t3.x, t12.y)) * (w3.x * w12.y);
    sum += tap(Vec2::new(t12.x, t3.y)) * (w12.x * w3.y);
    let weight = w12.x * w0.y + w0.x * w12.y + w12.x * w12.y + w3.x * w12.y + w12.x * w3.y;
    max3(sum / weight, Vec3::ZERO)
}
fn clip(value: Vec3, low: Vec3, high: Vec3) -> Vec3 {
    let centre = 0.5 * (low + high);
    let extent = 0.5 * (high - low) + 1e-5;
    let offset = value - centre;
    let units = (offset / extent).abs();
    let largest = fmax(units.x, fmax(units.y, units.z));
    if largest > 1.0 {
        centre + offset / largest
    } else {
        value
    }
}

/// b0 (lens): uint2 size; float2 inverseSize; float alpha, gamma; uint reset, clean; float2 jitter; float sharpen.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Lens {
    pub size_x: u32,
    pub size_y: u32,
    pub inverse_x: f32,
    pub inverse_y: f32,
    pub alpha: f32,
    pub gamma: f32,
    pub reset: u32,
    pub clean: u32,
    pub jitter_x: f32,
    pub jitter_y: f32,
    pub sharpen: f32,
}

/// The lens's Expand: the compressed peak kept below 0.99 (99x the tonemap knee) -- neon left the RGB cube after the
/// clip and divided by ~0.
fn expand_lens(c: Vec3) -> Vec3 {
    let mut c = max3(c, Vec3::ZERO);
    let p = peak(c);
    if p > 0.99 {
        c *= 0.99 / p;
    }
    c / (1.0 - peak(c))
}
fn saturate3(c: Vec3) -> Vec3 {
    Vec3::new(saturate(c.x), saturate(c.y), saturate(c.z))
}
fn load_lens(current: &Texture, p: IVec2, size: IVec2) -> Vec3 {
    let q = IVec2::new(p.x.max(0).min(size.x - 1), p.y.max(0).min(size.y - 1));
    let c: Vec4 = current.fetch(q);
    to_ycocg(compress(min3(max3(c.xyz(), Vec3::ZERO), Vec3::splat(65000.0))))
}

/// The scope lens's source: the scene the glass magnifies, accumulated over frames by the engine's velocity before the
/// glass reads it (a sub-pixel light one jittered frame hits and the next misses would blink as an M x M blob), the
/// history unjittered and the image handed on jittered like the frame; in a clean lens sharpened by CAS. Only in
/// motion, unless clean. (lens_source_resolve_cs.hlsl has the whole reasoning.)
#[spirv(compute(threads(8, 8, 1)))]
#[allow(clippy::too_many_arguments)]
pub fn lens(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Lens,
    #[spirv(descriptor_set = 0, binding = 1)] current: &Texture,
    #[spirv(descriptor_set = 0, binding = 2)] velocity: &Texture,
    #[spirv(descriptor_set = 0, binding = 3)] history: &Texture,
    #[spirv(descriptor_set = 0, binding = 4)] history_out: &OutColour,
    #[spirv(descriptor_set = 0, binding = 5)] output: &OutColour,
    #[spirv(descriptor_set = 0, binding = 6)] linear: &Sampler,
) {
    let p = IVec2::new(id.x as i32, id.y as i32);
    if id.x >= k.size_x || id.y >= k.size_y {
        return;
    }
    let size = IVec2::new(k.size_x as i32, k.size_y as i32);
    let jitter = Vec2::new(k.jitter_x, k.jitter_y);
    let (mut m1, mut m2, mut sum) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO);
    let mut weight = 0.0f32;
    let mut n = 0i32;
    while n < 9 {
        let (x, y) = (n % 3 - 1, n / 3 - 1);
        let s = load_lens(current, p + IVec2::new(x, y), size);
        m1 += s;
        m2 += s * s;
        // sample position relative to this pixel's centre
        let d = Vec2::new(x as f32, y as f32) - jitter;
        let w = (-2.29 * dot2(d, d)).exp();
        sum += s * w;
        weight += w;
        n += 1;
    }
    m1 = div3(m1, 9.0);
    m2 = div3(m2, 9.0);
    let c = sum / weight;
    let sigma = max3(m2 - m1 * m1, Vec3::ZERO);
    let sigma = Vec3::new(sigma.x.sqrt(), sigma.y.sqrt(), sigma.z.sqrt());
    let raw = load_lens(current, p, size);
    let inverse = Vec2::new(k.inverse_x, k.inverse_y);
    let sizef = Vec2::new(k.size_x as f32, k.size_y as f32);
    let uv = (Vec2::new(p.x as f32, p.y as f32) + 0.5) * inverse;
    let motion: Vec4 = velocity.fetch(p);
    let motion = motion.xy();
    let previous = uv + motion;
    let moving = saturate(div(length_of2(motion * sizef) - 0.25, 1.25));
    let mut result = c;
    let mut shown = raw;
    if k.reset == 0 && previous.x >= 0.0 && previous.y >= 0.0 && previous.x <= 1.0 && previous.y <= 1.0 {
        let low = m1 - k.gamma * sigma;
        let high = m1 + k.gamma * sigma;
        result = lerp3(clip(to_ycocg(compress(history_at(history, linear, previous, sizef, inverse))), low, high), c, k.alpha);
        let at = previous - jitter * inverse;
        let blend = if k.clean != 0 { k.alpha } else { lerp(1.0, k.alpha, moving) };
        shown = lerp3(clip(to_ycocg(compress(history_at(history, linear, at, sizef, inverse))), low, high), raw, blend);
        if k.clean != 0 && k.sharpen > 0.0 {
            // AMD FidelityFX CAS, cross taps, in tonemapped RGB: the weight of the four neighbours follows the local
            // headroom, so edges sharpen and flat or already contrasted areas do not ring.
            let tap = |o: Vec2| -> Vec3 {
                let t: Vec4 = history.sample_by_lod(*linear, o, 0.0);
                saturate3(compress(t.xyz()))
            };
            let centre = saturate3(from_ycocg(shown));
            let north = tap(at - Vec2::new(0.0, inverse.y));
            let west = tap(at - Vec2::new(inverse.x, 0.0));
            let east = tap(at + Vec2::new(inverse.x, 0.0));
            let south = tap(at + Vec2::new(0.0, inverse.y));
            let lo = min3(centre, min3(min3(north, west), min3(east, south)));
            let hi = max3(centre, max3(max3(north, west), max3(east, south)));
            let headroom = saturate3(min3(lo, Vec3::ONE - hi) / max3(hi, Vec3::splat(1e-5)));
            let amp = Vec3::new(headroom.x.sqrt(), headroom.y.sqrt(), headroom.z.sqrt());
            let w = amp * (-1.0 / lerp(8.0, 5.0, saturate(k.sharpen)));
            shown = to_ycocg(saturate3((centre + (north + west + east + south) * w) / (1.0 + 4.0 * w)));
        }
    }
    let mut keep = expand_lens(from_ycocg(result));
    let mut show = expand_lens(from_ycocg(shown));
    if !finite3(keep) {
        keep = expand_lens(from_ycocg(c));
    }
    if !finite3(show) {
        show = expand_lens(from_ycocg(raw));
    }
    unsafe {
        history_out.write(UVec2::new(id.x, id.y), Vec4::new(keep.x, keep.y, keep.z, 1.0));
        output.write(UVec2::new(id.x, id.y), Vec4::new(show.x, show.y, show.z, 1.0));
    }
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn camera(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Camera,
    #[spirv(descriptor_set = 0, binding = 1)] current: &Texture,
    #[spirv(descriptor_set = 0, binding = 2)] history: &Texture,
    #[spirv(descriptor_set = 0, binding = 3)] history_out: &OutColour,
    #[spirv(descriptor_set = 0, binding = 4)] linear: &Sampler,
) {
    let p = IVec2::new(id.x as i32, id.y as i32);
    if id.x >= k.size_x || id.y >= k.size_y {
        return;
    }
    let size = IVec2::new(k.size_x as i32, k.size_y as i32);
    let n = [
        load(current, p + IVec2::new(-1, -1), size),
        load(current, p + IVec2::new(0, -1), size),
        load(current, p + IVec2::new(1, -1), size),
        load(current, p + IVec2::new(-1, 0), size),
        load(current, p + IVec2::new(1, 0), size),
        load(current, p + IVec2::new(-1, 1), size),
        load(current, p + IVec2::new(0, 1), size),
        load(current, p + IVec2::new(1, 1), size),
    ];
    let mut c = load(current, p, size);
    // Two darkest neighbour lumas, and the sum of the other six.
    let (mut d0, mut d1, mut total) = (1e9f32, 1e9f32, 0.0f32);
    let mut sum8 = Vec3::ZERO;
    let mut i = 0usize;
    while i < 8 {
        let l = n[i].x;
        sum8 += n[i];
        total += l;
        if l < d0 {
            d1 = d0;
            d0 = l;
        } else if l < d1 {
            d1 = l;
        }
        i += 1;
    }
    if c.x < 0.5 * d1 && d1 > 2e-3 {
        // Mean of the six brighter neighbours in luma, chroma from all eight.
        let bright = div(total - d0 - d1, 6.0);
        let chroma = sum8.yz() / 8.0;
        c = Vec3::new(bright, chroma.x, chroma.y);
    }
    let mut m1 = c;
    let mut m2 = c * c;
    let mut j = 0usize;
    while j < 8 {
        m1 += n[j];
        m2 += n[j] * n[j];
        j += 1;
    }
    m1 = div3(m1, 9.0);
    m2 = div3(m2, 9.0);
    let sigma = max3(m2 - m1 * m1, Vec3::ZERO);
    let sigma = Vec3::new(sigma.x.sqrt(), sigma.y.sqrt(), sigma.z.sqrt());
    let inverse = Vec2::new(k.inverse_x, k.inverse_y);
    let sizef = Vec2::new(k.size_x as f32, k.size_y as f32);
    let uv = (Vec2::new(p.x as f32, p.y as f32) + 0.5) * inverse;
    let ndc = Vec3::new(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 1.0);
    let h = Vec3::new(dot3(k.row0.xyz(), ndc), dot3(k.row1.xyz(), ndc), dot3(k.row2.xyz(), ndc));
    let mut result = c;
    if k.reset == 0 && h.z > 1e-4 {
        let q = h.xy() / h.z;
        let previous = Vec2::new(0.5 + 0.5 * q.x, 0.5 - 0.5 * q.y);
        if previous.x >= 0.0 && previous.y >= 0.0 && previous.x <= 1.0 && previous.y <= 1.0 {
            let moved = length_of2((previous - uv) * sizef);
            let a = lerp(k.alpha, k.motion_alpha, saturate(moved * 0.25));
            let low = m1 - k.gamma * sigma;
            let high = m1 + k.gamma * sigma;
            result = lerp3(clip(to_ycocg(compress(history_at(history, linear, previous, sizef, inverse))), low, high), c, a);
        }
    }
    let here: Vec4 = current.fetch(p);
    let out = expand(max3(from_ycocg(result), Vec3::ZERO));
    unsafe { history_out.write(UVec2::new(id.x, id.y), Vec4::new(out.x, out.y, out.z, here.w)) };
}
