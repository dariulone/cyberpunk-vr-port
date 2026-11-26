//! DLSS 5 NEURAL RENDERING's colour codec (rust/crates/cvr-render/src/neural_render.rs; main's 2e123a4a3,
//! src/Render/NeuralRender.cpp's kCodecHLSL, compiled there at first use). The model wants a display-encoded [0, 1]
//! picture; the game's DLSS output is scene-linear HDR.
//!
//!   ENCODE: the eye's area of the game's output (at the offset) -> the SDR proxy at (0, 0): divided by the paper white,
//!   Reinhard into [0, 1), sRGB-encoded.
//!   DECODE: the model's result at (0, 0) -> back onto the game's output at the offset: sRGB-decoded, inverse Reinhard,
//!   times the paper white; the luma ratio to the game's own pixel held within [0, 4] (near 1 the inverse explodes, and
//!   a model output blown up or collapsed written back frame after frame is what a driver reports as a GPU hang), a
//!   proxy collapsed to black keeping the game's pixel, blended by the colour strength.
//!
//! Bindings (descriptor set 0): 0 the parameters (b0, 6 root constants), 1 t0 the source, 2 u0 the destination (the
//! decode loads it too: RGBA16F, or R11G11B10 where the device can load it typed).
use spirv_std::glam::{UVec2, UVec3, Vec3, Vec4};
#[cfg(target_arch = "spirv")]
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image};

type Texture = Image!(2D, type = f32, sampled);
type Target = Image!(2D, format = rgba16f, sampled = false);

/// b0: uint W, H; float PaperWhite, ColorStrength; uint OffX, OffY.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Params {
    pub width: u32,
    pub height: u32,
    pub paper_white: f32,
    pub color_strength: f32,
    pub off_x: u32,
    pub off_y: u32,
}

fn saturate(v: f32) -> f32 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}
fn encode1(v: f32) -> f32 {
    let c = saturate(v);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}
fn decode1(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn luma(c: Vec3) -> f32 {
    c.x * 0.2126 + c.y * 0.7152 + c.z * 0.0722
}
fn positive(c: Vec3) -> Vec3 {
    Vec3::new(c.x.max(0.0), c.y.max(0.0), c.z.max(0.0))
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn encode(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Params,
    #[spirv(descriptor_set = 0, binding = 1)] source: &Texture,
    #[spirv(descriptor_set = 0, binding = 2)] target: &Target,
) {
    if id.x >= k.width || id.y >= k.height {
        return;
    }
    let pw = k.paper_white.max(1e-4);
    let s: Vec4 = source.fetch(UVec2::new(id.x + k.off_x, id.y + k.off_y));
    let c = positive(Vec3::new(s.x, s.y, s.z)) / pw;
    // Reinhard -> [0, 1)
    let c = c / (Vec3::ONE + c);
    unsafe { target.write(UVec2::new(id.x, id.y), Vec4::new(encode1(c.x), encode1(c.y), encode1(c.z), 1.0)) };
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn decode(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Params,
    #[spirv(descriptor_set = 0, binding = 1)] source: &Texture,
    #[spirv(descriptor_set = 0, binding = 2)] target: &Target,
) {
    if id.x >= k.width || id.y >= k.height {
        return;
    }
    let g = UVec2::new(id.x + k.off_x, id.y + k.off_y);
    let pw = k.paper_white.max(1e-4);
    let s: Vec4 = source.fetch(UVec2::new(id.x, id.y));
    let n = Vec3::new(decode1(saturate(s.x)), decode1(saturate(s.y)), decode1(saturate(s.z)));
    // still the game's own pixel
    let o: Vec4 = target.read(g);
    // a proxy collapsed to black keeps the game's pixel
    if luma(n) <= 1e-5 {
        return;
    }
    // inverse Reinhard
    let mut c = Vec3::new(n.x / (1.0 - n.x).max(1e-4), n.y / (1.0 - n.y).max(1e-4), n.z / (1.0 - n.z).max(1e-4)) * pw;
    // near n = 1 the inverse explodes: the luma ratio to the original held within [0, 4], the chroma kept
    let original = Vec3::new(o.x, o.y, o.z);
    let oy = luma(positive(original));
    let cy = luma(positive(c));
    if oy > 1e-5 && cy > 1e-5 {
        let ratio = cy / oy;
        let held = if ratio > 4.0 { 4.0 } else { ratio };
        c *= (held * oy) / cy;
    }
    let strength = k.color_strength.max(0.0);
    let blended = original + (c - original) * strength;
    unsafe { target.write(g, Vec4::new(blended.x.max(0.0), blended.y.max(0.0), blended.z.max(0.0), o.w)) };
}
