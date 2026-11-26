//! The optics' own pixel shaders drawn into the engine's glass and sight passes (src/Shaders/lens_mask_ps.hlsl,
//! lens_velocity_ps.hlsl, sight_mask_ps.hlsl, formerly DXC's): an upscaler history mask and the lens's velocity, each
//! redrawn right after the engine's own draw with the engine's vertex shader -- so the input signature is the native
//! glass PS's (SV_Position, TEXCOORD0..6) or sight PS's (SV_Position, TEXCOORD0..4), every element declared whether
//! read or not (the builder writes the input struct from its SIGNATURES table: naga drops what a shader does not read).
//!
//! Bindings (descriptor set 0): 0 GlobalShaderConstants (b0), 1 CameraShaderConstants (b1), 4 MaterialConstants /
//! ShaderSpecificConsts (b4) -- the engine's own constant buffers at the engine's registers.
use crate::common::dot3;
use spirv_std::arch::kill;
use spirv_std::glam::{UVec4, Vec2, Vec3, Vec3Swizzles, Vec4, Vec4Swizzles};
use spirv_std::spirv;

/// GlobalShaderConstants (b0): uint4 globals[12].
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Globals {
    pub v: [UVec4; 12],
}
/// CameraShaderConstants (b1): float4 camera[53].
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Camera {
    pub v: [Vec4; 53],
}
/// MaterialConstants (b4): float4 material[32].
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Material {
    pub v: [Vec4; 32],
}
/// ShaderSpecificConsts (b4) of the sight material: float4 _29_m0[21].
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Sight {
    pub v: [Vec4; 21],
}

/// Only lenses authored for VR (EyeRelief.w marker); globals[11].w selects the alternate projection path.
fn vr_lens(globals: &Globals, material: &Material) -> bool {
    material.v[31].w == 713.25 && globals.v[11].w == 0
}

/// The upscaler history mask of a VR scope lens: 1 (reject history) on the glass, nothing elsewhere.
#[spirv(fragment)]
pub fn lens_mask(
    #[spirv(frag_coord)] _pos: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] globals: &Globals,
    #[spirv(uniform, descriptor_set = 0, binding = 4)] material: &Material,
    out: &mut f32,
) {
    if !vr_lens(globals, material) {
        kill();
    }
    *out = 1.0;
}

/// The lens's velocity: the glass's own motion blended towards the scene's seen through it by the magnification.
#[spirv(fragment)]
#[allow(clippy::too_many_arguments)]
pub fn lens_velocity(
    #[spirv(frag_coord)] _pos: Vec4,
    #[spirv(location = 4)] current: Vec4,
    #[spirv(location = 5)] previous: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] globals: &Globals,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] camera: &Camera,
    #[spirv(uniform, descriptor_set = 0, binding = 4)] material: &Material,
    out: &mut Vec2,
) {
    if !vr_lens(globals, material) {
        kill();
    }
    let magnification = material.v[17].x;
    let here = current.xy() / current.w;
    let lens = (previous.xy() / previous.w - here) * Vec2::new(0.5, -0.5);
    // Distant point on this pixel's eye ray: rows 28/29/31 (x, y, w) are the current jittered view-projection,
    // [51].xy the NDC jitter the VS removed.
    let jittered = here + camera.v[51].xy();
    let (a, b, c) = (camera.v[28].xyz(), camera.v[29].xyz(), camera.v[31].xyz());
    let ray = b.cross(c) * jittered.x + c.cross(a) * jittered.y + a.cross(b);
    let before = Vec3::new(dot3(camera.v[16].xyz(), ray), dot3(camera.v[17].xyz(), ray), dot3(camera.v[19].xyz(), ray));
    if before.z.abs() < 1e-20 {
        kill();
    }
    let scene = (before.xy() / before.z - here) * Vec2::new(0.5, -0.5);
    *out = lens + magnification * (scene - lens);
}

/// The camera scope's image surface (ImageScale -713.25 / -714.25 / -716.25, and the integrated displays' -717.25):
/// 1 there, every other sight discarded.
#[spirv(fragment)]
pub fn sight_mask(
    #[spirv(frag_coord)] _pos: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 4)] sight: &Sight,
    out: &mut f32,
) {
    let scale = sight.v[6].x;
    if scale != -713.25 && scale != -714.25 && scale != -716.25 && scale != -717.25 {
        kill();
    }
    *out = 1.0;
}
