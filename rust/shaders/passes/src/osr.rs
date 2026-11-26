//! The made eye's reprojection (rust/crates/cvr-gpu/src/osr.rs), formerly its HLSL: the drawn eye's depth, and the
//! made eye's previous frame, splatted forward into the made eye's depth grid (Clear, Splat); per pixel the splatted
//! depth traced back for its colour (Resolve); the holes -- background this eye sees and the drawn eye does not --
//! filled with the background beside them, mirrored across the edge (Inpaint). The arithmetic as FXC compiled the
//! HLSL (its listing, `fxc /T cs_5_0 /O3`): FXC fuses a product and a sum into `mad` (each lerp, the camera rays,
//! `uv * size - j`) and moves the constants last (`(c + j) + 0.5`, `mad(uv, size, -j) - 0.5`); written the HLSL's
//! way the made eye's colours came one step apart on WARP (osr_parity). `mad` here is `f32::mul_add`: GLSL's Fma,
//! HLSL's mad, DXIL's FMad -- the instruction FXC wrote.
//!
//! Bindings (descriptor set 0): 0 the constants (b0, a root CBV); 1..6 SrcColor, SrcDepth, HistColor, HistDepth,
//! ResColor, ResDepth (t0..t5, a table); 7..12 StereoT, HistT, OutColor, OutDepth, Final, NearD (u0..u5, a table);
//! 13 the linear clamp sampler (s0, static).
use spirv_std::glam::{IVec2, UVec2, UVec3, UVec4, Vec2, Vec3, Vec4, Vec4Swizzles};
use spirv_std::num_traits::Float;
use crate::common::{dot3, length3};
use spirv_std::{spirv, Image, Sampler};

type Texture = Image!(2D, type = f32, sampled);
type Counter = Image!(2D, format = r32ui, sampled = false);
type OutColour = Image!(2D, format = rgba16f, sampled = false);
type OutScalar = Image!(2D, format = r32f, sampled = false);

/// cbuffer C: three cameras (position, right, up, forward, tangents), the depth model, the sizes, the jitters, the
/// flags and the edge test.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Constants {
    pub s_pos: Vec4,
    pub s_right: Vec4,
    pub s_up: Vec4,
    pub s_fwd: Vec4,
    pub s_tan: Vec4,
    pub d_pos: Vec4,
    pub d_right: Vec4,
    pub d_up: Vec4,
    pub d_fwd: Vec4,
    pub d_tan: Vec4,
    pub h_pos: Vec4,
    pub h_right: Vec4,
    pub h_up: Vec4,
    pub h_fwd: Vec4,
    pub h_tan: Vec4,
    /// A, B, tolerance, history valid
    pub depth_ab: Vec4,
    /// depth w, h; colour W, H
    pub sizes: UVec4,
    /// source jitter (depth texels), history jitter
    pub jit: Vec4,
    /// debug, pass (0 stereo, 1 history), longest fill run, history near limit (cm)
    pub flags: UVec4,
    /// edge distance (made-eye pixels), made-eye pixels per unit tangent
    pub edge: Vec4,
}

/// Below every real depth's bits: geometry wins over sky in the max.
const SKY: u32 = 1;

fn z_of(d: f32, k: &Constants) -> f32 {
    k.depth_ab.y / (d - k.depth_ab.x).max(1e-12)
}
fn d_of(z: f32, k: &Constants) -> f32 {
    k.depth_ab.y / z + k.depth_ab.x
}
/// a * b + c, one instruction (FXC's mad).
fn mad(a: f32, b: f32, c: f32) -> f32 {
    a.mul_add(b, c)
}
fn mad2(a: Vec2, b: Vec2, c: Vec2) -> Vec2 {
    Vec2::new(mad(a.x, b.x, c.x), mad(a.y, b.y, c.y))
}
/// s * v + c per component (FXC's `mad r.xyz, s.xxxx, v.xyz, c.xyz`).
fn mad3(s: f32, v: Vec3, c: Vec3) -> Vec3 {
    Vec3::new(mad(s, v.x, c.x), mad(s, v.y, c.y), mad(s, v.z, c.z))
}
/// v * s + c per component (a lerp's `mad r.xyz, (b - a).xyz, s, a.xyz`).
fn mad3s(v: Vec3, s: f32, c: Vec3) -> Vec3 {
    Vec3::new(mad(v.x, s, c.x), mad(v.y, s, c.y), mad(v.z, s, c.z))
}
/// lerp(t.x, t.y, uv.x), lerp(t.z, t.w, uv.y): mad(uv, t.yw - t.xz, t.xz).
fn tan_of(uv: Vec2, t: Vec4) -> Vec2 {
    mad2(uv, Vec2::new(t.y - t.x, t.w - t.z), Vec2::new(t.x, t.z))
}
/// The camera ray at tangents tn: F + tn.x * R + tn.y * U, as mad(tn.y, U, mad(tn.x, R, F)).
fn ray(tn: Vec2, r: Vec3, u: Vec3, f: Vec3) -> Vec3 {
    mad3(tn.y, u, mad3(tn.x, r, f))
}
/// floor(uv * size - 0.5 - j), as FXC: floor(mad(uv, size, -j) - 0.5).
fn cell_of(uv: Vec2, size: Vec2, j: Vec2) -> IVec2 {
    floor2(mad2(uv, size, -j) - 0.5)
}
fn uv_of(tn: Vec2, t: Vec4) -> Vec2 {
    Vec2::new((tn.x - t.x) / (t.y - t.x), (tn.y - t.z) / (t.w - t.z))
}
fn depth_size(k: &Constants) -> IVec2 {
    IVec2::new(k.sizes.x as i32, k.sizes.y as i32)
}
fn clamp2(c: IVec2, k: &Constants) -> IVec2 {
    let hi = depth_size(k) - IVec2::ONE;
    IVec2::new(c.x.max(0).min(hi.x), c.y.max(0).min(hi.y))
}
fn load(t: &Texture, at: IVec2) -> f32 {
    let v: Vec4 = t.fetch(at);
    v.x
}
fn any_outside(uv: Vec2) -> bool {
    uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0
}
fn floor2(v: Vec2) -> IVec2 {
    IVec2::new(v.x.floor() as i32, v.y.floor() as i32)
}
fn size_f(k: &Constants) -> Vec2 {
    Vec2::new(k.sizes.x as f32, k.sizes.y as f32)
}

/// InterlockedMax(image[c], value), its result unused: written as a store, which the shader builder turns into the
/// image atomic (its ATOMIC_MAX_STORES: osr::splat's StereoT and HistT, the only stores to them there). spirv-std has no
/// image atomic, and rust-gpu's inline SPIR-V gives an OpImageTexelPointer's result the Function storage class.
fn image_max(image: &Counter, c: IVec2, value: u32) {
    unsafe { image.write(c, value) };
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn clear(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Constants,
    #[spirv(descriptor_set = 0, binding = 2)] src_depth: &Texture,
    #[spirv(descriptor_set = 0, binding = 7)] stereo_t: &Counter,
    #[spirv(descriptor_set = 0, binding = 8)] hist_t: &Counter,
    #[spirv(descriptor_set = 0, binding = 12)] near_d: &OutScalar,
) {
    if id.x >= k.sizes.x || id.y >= k.sizes.y {
        return;
    }
    let at = UVec2::new(id.x, id.y);
    unsafe {
        stereo_t.write(at, 0u32);
        hist_t.write(at, 0u32);
    }
    let p = IVec2::new(id.x as i32, id.y as i32);
    let mut nearest = 0.0f32;
    let mut n = 0i32;
    while n < 16 {
        nearest = nearest.max(load(src_depth, clamp2(p + IVec2::new((n & 3) - 1, (n >> 2) - 1), k)));
        n += 1;
    }
    unsafe { near_d.write(at, nearest) };
}

/// Every depth texel of the drawn eye (pass 0) or of the made eye's previous frame (pass 1) forward into the made
/// eye's depth grid; each cell keeps the nearest that lands on it (2x2 footprint, so no cracks open between texels).
#[spirv(compute(threads(8, 8, 1)))]
pub fn splat(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Constants,
    #[spirv(descriptor_set = 0, binding = 2)] src_depth: &Texture,
    #[spirv(descriptor_set = 0, binding = 4)] hist_depth: &Texture,
    #[spirv(descriptor_set = 0, binding = 7)] stereo_t: &Counter,
    #[spirv(descriptor_set = 0, binding = 8)] hist_t: &Counter,
) {
    if id.x >= k.sizes.x || id.y >= k.sizes.y {
        return;
    }
    let hist = k.flags.y != 0;
    let at = IVec2::new(id.x as i32, id.y as i32);
    let d = if hist { load(hist_depth, at) } else { load(src_depth, at) };
    let p0 = if hist { k.h_pos.xyz() } else { k.s_pos.xyz() };
    let r = if hist { k.h_right.xyz() } else { k.s_right.xyz() };
    let u = if hist { k.h_up.xyz() } else { k.s_up.xyz() };
    let f = if hist { k.h_fwd.xyz() } else { k.s_fwd.xyz() };
    let t = if hist { k.h_tan } else { k.s_tan };
    let j = if hist { k.jit.zw() } else { k.jit.xy() };
    let uv = (Vec2::new(id.x as f32, id.y as f32) + j + 0.5) / size_f(k);
    let dir = ray(tan_of(uv, t), r, u, f);
    // sky: a direction, no parallax
    let rel = if d <= 0.0 { dir } else { mad3(z_of(d, k), dir, p0) - k.d_pos.xyz() };
    let zd = dot3(rel, k.d_fwd.xyz());
    if zd <= 1e-4 {
        return;
    }
    let uvd = uv_of(Vec2::new(dot3(rel, k.d_right.xyz()), dot3(rel, k.d_up.xyz())) / zd, k.d_tan);
    let value = if d <= 0.0 { SKY } else { d_of(zd, k).max(2e-38).to_bits() };
    // floor(uvd * size - 0.5): one mad
    let b = floor2(mad2(uvd, size_f(k), Vec2::splat(-0.5)));
    let size = depth_size(k);
    let mut n = 0i32;
    while n < 4 {
        let c = b + IVec2::new(n & 1, n >> 1);
        if !(c.x < 0 || c.y < 0 || c.x >= size.x || c.y >= size.y) {
            if hist {
                image_max(hist_t, c, value);
            } else {
                image_max(stereo_t, c, value);
            }
        }
        n += 1;
    }
}

/// Whether a surface of the made eye's previous frame, at world point `w`, is still there: where this frame's drawn
/// eye sees that place it must not see past it (beyond the depth tolerance, on all four texels around it). Where the
/// drawn eye cannot tell, a surface nearer than the near limit (flags.w cm) is taken for the player's own hands,
/// weapon or body: it fails too. (The HLSL's comment has the whole reasoning.)
fn still_there(w: Vec3, k: &Constants, src_depth: &Texture) -> bool {
    let rel = w - k.s_pos.xyz();
    let zs = dot3(rel, k.s_fwd.xyz());
    let far_enough = zs >= k.flags.w as f32 * 0.01;
    if zs <= 1e-4 {
        return far_enough;
    }
    let uvs = uv_of(Vec2::new(dot3(rel, k.s_right.xyz()), dot3(rel, k.s_up.xyz())) / zs, k.s_tan);
    if any_outside(uvs) {
        return far_enough;
    }
    let b = cell_of(uvs, size_f(k), k.jit.xy());
    let mut seen = false;
    let mut hidden = false;
    let mut n = 0i32;
    while n < 4 {
        let c = clamp2(b + IVec2::new(n & 1, n >> 1), k);
        let ds = load(src_depth, c);
        if ds > 0.0 {
            let z = z_of(ds, k);
            if z <= zs * (1.0 + k.depth_ab.z) {
                if z >= zs * (1.0 - k.depth_ab.z) {
                    seen = true;
                } else {
                    hidden = true;
                }
            }
        }
        n += 1;
    }
    seen || (hidden && far_enough)
}

/// The made eye's ray at a splatted depth, traced into the drawn eye (hist false) or the made eye's previous frame:
/// taken when the depth there agrees (sky with sky); a nearer surface within reach of the sample makes it a hole
/// unless it lands within edge.x pixels in the made eye (the HLSL's comment has the whole reasoning).
#[allow(clippy::too_many_arguments)]
fn trace(
    v: u32,
    dir: Vec3,
    hist: bool,
    colour: &mut Vec4,
    k: &Constants,
    src_color: &Texture,
    src_depth: &Texture,
    hist_color: &Texture,
    hist_depth: &Texture,
    near_d: &OutScalar,
    linear: &Sampler,
) -> bool {
    *colour = Vec4::ZERO;
    let p0 = if hist { k.h_pos.xyz() } else { k.s_pos.xyz() };
    let r = if hist { k.h_right.xyz() } else { k.s_right.xyz() };
    let u = if hist { k.h_up.xyz() } else { k.s_up.xyz() };
    let f = if hist { k.h_fwd.xyz() } else { k.s_fwd.xyz() };
    let t = if hist { k.h_tan } else { k.s_tan };
    let rel = if v == SKY { dir } else { mad3(z_of(f32::from_bits(v), k), dir, k.d_pos.xyz()) - p0 };
    let zx = dot3(rel, f);
    if zx <= 1e-4 {
        return false;
    }
    let uvx = uv_of(Vec2::new(dot3(rel, r), dot3(rel, u)) / zx, t);
    if any_outside(uvx) {
        return false;
    }
    let j = if hist { k.jit.zw() } else { k.jit.xy() };
    let b = cell_of(uvx, size_f(k), j);
    let mut ok = false;
    let mut n = 0i32;
    while n < 4 {
        let c = clamp2(b + IVec2::new(n & 1, n >> 1), k);
        let ds = if hist { load(hist_depth, c) } else { load(src_depth, c) };
        if v == SKY {
            ok = ok || ds <= 0.0;
        } else if ds > 0.0 {
            ok = ok || (z_of(ds, k) - zx).abs() <= k.depth_ab.z * zx;
        }
        n += 1;
    }
    if !ok {
        return false;
    }
    if k.edge.x > 0.0 && !hist {
        // the drawn eye: only the nearest of the 16 texels can decide, made once per texel in Clear
        let dn: f32 = near_d.read(clamp2(b, k));
        if dn > 0.0 {
            let inv_z = if v == SKY { 0.0 } else { 1.0 / zx };
            let inv = 1.0 / z_of(dn, k);
            if inv > inv_z * (1.0 + k.depth_ab.z) && (inv - inv_z) * length3(p0 - k.d_pos.xyz()) * k.edge.y > k.edge.x {
                return false;
            }
        }
    }
    if k.edge.x > 0.0 && hist {
        let inv_z = if v == SKY { 0.0 } else { 1.0 / zx };
        let px_per_inv_z = length3(p0 - k.d_pos.xyz()) * k.edge.y;
        let mut e = 0i32;
        while e < 16 {
            let c = clamp2(b + IVec2::new((e & 3) - 1, (e >> 2) - 1), k);
            let ds = load(hist_depth, c);
            if ds > 0.0 {
                let zt = z_of(ds, k);
                let inv = 1.0 / zt;
                if inv > inv_z * (1.0 + k.depth_ab.z) {
                    if (inv - inv_z) * px_per_inv_z > k.edge.x {
                        return false;
                    }
                    let tt = tan_of((Vec2::new(c.x as f32, c.y as f32) + k.jit.zw() + 0.5) / size_f(k), k.h_tan);
                    let w = mad3(zt, ray(tt, k.h_right.xyz(), k.h_up.xyz(), k.h_fwd.xyz()), k.h_pos.xyz());
                    if !still_there(w, k, src_depth) {
                        return false;
                    }
                }
            }
            e += 1;
        }
    }
    let mut c: Vec4 = if hist {
        hist_color.sample_by_lod(*linear, uvx, 0.0)
    } else {
        src_color.sample_by_lod(*linear, uvx, 0.0)
    };
    c.w = 1.0;
    *colour = c;
    true
}

/// One candidate depth of a pixel: taken when it traces (the previous frame only with what is still there).
#[allow(clippy::too_many_arguments)]
fn try_candidate(
    v: u32,
    dir: Vec3,
    hist: bool,
    at: UVec2,
    k: &Constants,
    src_color: &Texture,
    src_depth: &Texture,
    hist_color: &Texture,
    hist_depth: &Texture,
    near_d: &OutScalar,
    linear: &Sampler,
    out_color: &OutColour,
    out_depth: &OutScalar,
) -> bool {
    if v == 0 {
        return false;
    }
    // The previous frame fills with what is still there only (StillThere); such a hole goes to the fill.
    if hist && v != SKY && !still_there(mad3(z_of(f32::from_bits(v), k), dir, k.d_pos.xyz()), k, src_depth) {
        return false;
    }
    let mut colour = Vec4::ZERO;
    if !trace(v, dir, hist, &mut colour, k, src_color, src_depth, hist_color, hist_depth, near_d, linear) {
        return false;
    }
    if hist && (k.flags.x & 1) != 0 {
        let rgb = colour.xyz();
        let rgb = mad3s(Vec3::new(0.0, 1.0, 0.0) - rgb, 0.35, rgb);
        colour = Vec4::new(rgb.x, rgb.y, rgb.z, colour.w);
    }
    unsafe {
        out_color.write(at, colour);
        out_depth.write(at, if v == SKY { 0.0 } else { f32::from_bits(v) });
    }
    true
}

/// Per pixel of the made eye: its cell's splatted depth (else the farthest, then the nearest of the 3x3) traced into
/// the drawn eye, then the same with the made eye's previous frame; what neither accounts for is a hole (alpha 0).
#[spirv(compute(threads(8, 8, 1)))]
#[allow(clippy::too_many_arguments)]
pub fn resolve(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Constants,
    #[spirv(descriptor_set = 0, binding = 1)] src_color: &Texture,
    #[spirv(descriptor_set = 0, binding = 2)] src_depth: &Texture,
    #[spirv(descriptor_set = 0, binding = 3)] hist_color: &Texture,
    #[spirv(descriptor_set = 0, binding = 4)] hist_depth: &Texture,
    #[spirv(descriptor_set = 0, binding = 7)] stereo_t: &Counter,
    #[spirv(descriptor_set = 0, binding = 8)] hist_t: &Counter,
    #[spirv(descriptor_set = 0, binding = 9)] out_color: &OutColour,
    #[spirv(descriptor_set = 0, binding = 10)] out_depth: &OutScalar,
    #[spirv(descriptor_set = 0, binding = 12)] near_d: &OutScalar,
    #[spirv(descriptor_set = 0, binding = 13)] linear: &Sampler,
) {
    if id.x >= k.sizes.z || id.y >= k.sizes.w {
        return;
    }
    let at = UVec2::new(id.x, id.y);
    let uv = (Vec2::new(id.x as f32, id.y as f32) + 0.5) / Vec2::new(k.sizes.z as f32, k.sizes.w as f32);
    let cell = uv * size_f(k);
    let hi_cell = depth_size(k) - IVec2::ONE;
    let c = IVec2::new((cell.x as i32).min(hi_cell.x), (cell.y as i32).min(hi_cell.y));
    let dir = ray(tan_of(uv, k.d_tan), k.d_right.xyz(), k.d_up.xyz(), k.d_fwd.xyz());
    let mut trip = 0u32;
    while trip < 2 {
        let hist = trip == 1;
        if hist && k.depth_ab.w <= 0.0 {
            break;
        }
        let own: u32 = if hist { hist_t.read(c) } else { stereo_t.read(c) };
        let mut lo = 0xffff_ffffu32;
        let mut hi = 0u32;
        let mut n = 0i32;
        while n < 9 {
            let q = clamp2(c + IVec2::new(n % 3 - 1, n / 3 - 1), k);
            let v: u32 = if hist { hist_t.read(q) } else { stereo_t.read(q) };
            if v != 0 {
                lo = lo.min(v);
                hi = hi.max(v);
            }
            n += 1;
        }
        let lo = if lo == 0xffff_ffff { 0 } else { lo };
        // the candidates in order: the cell's own, the 3x3's farthest, its nearest
        let mut i = 0u32;
        while i < 3 {
            let v = if i == 0 { own } else if i == 1 { lo } else { hi };
            if try_candidate(v, dir, hist, at, k, src_color, src_depth, hist_color, hist_depth, near_d, linear, out_color, out_depth) {
                return;
            }
            i += 1;
        }
        trip += 1;
    }
    unsafe {
        out_color.write(at, Vec4::ZERO);
        out_depth.write(at, 0.0f32);
    }
}

/// Whether a resolved pixel holds a colour (alpha over one half).
fn resolved(res_color: &Texture, q: IVec2) -> bool {
    let c: Vec4 = res_color.fetch(q);
    c.w > 0.5
}

/// The first resolved pixel from p along dirv within the run (0 = none): one pixel at a time near p, then in strides
/// of 4 (a hole's far bound can lie a couple of hundred pixels away), stepping back to the exact bound.
fn bound(p: IVec2, dirv: IVec2, run: i32, k: &Constants, res_color: &Texture) -> i32 {
    let mut n = 1i32;
    let mut last = 0i32;
    while n <= run {
        let q = p + dirv * n;
        if q.x < 0 || q.y < 0 || q.x >= k.sizes.z as i32 || q.y >= k.sizes.w as i32 {
            return 0;
        }
        if resolved(res_color, q) {
            let mut at = n;
            let mut i = n - 1;
            loop {
                if i <= last {
                    break;
                }
                if !resolved(res_color, p + dirv * i) {
                    break;
                }
                at = i;
                i -= 1;
            }
            return at;
        }
        last = n;
        n += if n < 16 { 1 } else { 4 };
    }
    0
}

/// A hole is background this eye sees and the drawn eye does not: along the row, the farther of the two resolved
/// pixels bounding it is the background; the hole takes the background mirrored across that edge, or the edge pixel
/// where the mirror lands on something nearer. Rows the run cannot close take the column's bounds the same way; with
/// neither closed, the farthest single bound found.
#[spirv(compute(threads(8, 8, 1)))]
pub fn inpaint(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Constants,
    #[spirv(descriptor_set = 0, binding = 1)] src_color: &Texture,
    #[spirv(descriptor_set = 0, binding = 5)] res_color: &Texture,
    #[spirv(descriptor_set = 0, binding = 6)] res_depth: &Texture,
    #[spirv(descriptor_set = 0, binding = 11)] fin: &OutColour,
    #[spirv(descriptor_set = 0, binding = 13)] linear: &Sampler,
) {
    if id.x >= k.sizes.z || id.y >= k.sizes.w {
        return;
    }
    let at = UVec2::new(id.x, id.y);
    let p = IVec2::new(id.x as i32, id.y as i32);
    let here: Vec4 = res_color.fetch(p);
    if here.w > 0.5 {
        unsafe { fin.write(at, Vec4::new(here.x, here.y, here.z, 1.0)) };
        return;
    }
    let run = k.flags.z as i32;
    let mut colour = Vec4::ZERO;
    let mut found = false;
    let mut single = IVec2::new(-1, -1);
    let mut single_depth = 1e30f32;
    let mut axis = 0u32;
    while axis < 2 && !found {
        let step = if axis == 0 { IVec2::new(1, 0) } else { IVec2::new(0, 1) };
        let lo = bound(p, -step, run, k, res_color);
        let hi = bound(p, step, run, k, res_color);
        if lo == 0 && hi == 0 {
            // nothing on this axis
        } else if lo == 0 || hi == 0 {
            let q = if lo != 0 { p - step * lo } else { p + step * hi };
            let dq = load(res_depth, q);
            if dq < single_depth {
                single_depth = dq;
                single = q;
            }
        } else {
            let use_lo = load(res_depth, p - step * lo) <= load(res_depth, p + step * hi);
            let dist = if use_lo { lo } else { hi };
            let dirv = if use_lo { -step } else { step };
            let edge = p + dirv * dist;
            let mirror = edge + dirv * dist;
            let edge_depth = load(res_depth, edge);
            colour = res_color.fetch(edge);
            let inside = mirror.x >= 0 && mirror.y >= 0 && mirror.x < k.sizes.z as i32 && mirror.y < k.sizes.w as i32;
            if inside && resolved(res_color, mirror) && load(res_depth, mirror) <= mad(edge_depth, 1.03, 1e-9) {
                colour = res_color.fetch(mirror);
            }
            found = true;
        }
        axis += 1;
    }
    // debug tint: magenta closed run, cyan lone bound, yellow the whole-image search
    let mut tint = Vec3::new(1.0, 0.0, 1.0);
    if !found && single.x >= 0 {
        colour = res_color.fetch(single);
        found = true;
        tint = Vec3::new(0.0, 1.0, 1.0);
    }
    // Nothing within the run (a hole against the image's edge): the farthest bound across the whole image. The drawn
    // eye's pixel at the same place is the last resort only: there it is mostly the near surface that opened the hole.
    if !found {
        let reach = k.sizes.z.max(k.sizes.w) as i32;
        let mut d = 0u32;
        while d < 4 {
            let dirv = if d == 0 {
                IVec2::new(-1, 0)
            } else if d == 1 {
                IVec2::new(1, 0)
            } else if d == 2 {
                IVec2::new(0, -1)
            } else {
                IVec2::new(0, 1)
            };
            let n = bound(p, dirv, reach, k, res_color);
            if n != 0 {
                let q = p + dirv * n;
                let dq = load(res_depth, q);
                if dq < single_depth {
                    single_depth = dq;
                    single = q;
                }
            }
            d += 1;
        }
        if single.x >= 0 {
            colour = res_color.fetch(single);
            found = true;
            tint = Vec3::new(1.0, 1.0, 0.0);
        }
    }
    if !found {
        colour = src_color.sample_by_lod(*linear, (Vec2::new(id.x as f32, id.y as f32) + 0.5) / Vec2::new(k.sizes.z as f32, k.sizes.w as f32), 0.0);
    }
    let mut rgb = colour.xyz();
    if (k.flags.x & 1) != 0 {
        rgb = mad3s(tint - rgb, 0.5, rgb);
    }
    unsafe { fin.write(at, Vec4::new(rgb.x, rgb.y, rgb.z, 1.0)) };
}
