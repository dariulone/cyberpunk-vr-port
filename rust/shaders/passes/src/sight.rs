//! The holographic sight's reticle quad (src/Shaders/sight_reflex_vs.hlsl, formerly DXC's): the engine's vertex shader
//! reproduced -- dequantised, skinned by four bones read as raw dwords from the skinning buffer, placed by the instance
//! transform -- passing on what the pixel shader needs and the engine's could not give it: the glass's world size per uv
//! unit along its own axes, the sight's optical axis (the slab's thin axis) in the glass's tangent frame, and the eye's
//! ray to the pixel kept camera-relative before interpolation (a 5 cm ray quantised into visible uv steps otherwise).
//! (The HLSL's comments have the whole reasoning and the measured vertex data.)
//!
//! The engine's input layout (POSITION .. INSTANCE_SKINNING_DATA, SV_VertexID) and its pixel shader's input
//! (SV_Position, TEXCOORD0..4, SV_ClipDistance0) are the builder's VERTEX_SIGNATURES; the skinning buffer is a typed
//! Buffer<uint4> at t10 (the builder's TYPED_BUFFERS: naga has no texel buffers).
//! Bindings (descriptor set 0): 7 MaterialModifiers (b7), 0 GlobalShaderConsts (b0), 1 CameraShaderConsts (b1),
//! 5 FrequentVertex (b5), 10 the skinning matrices (t10).
use crate::common::{cross3, dot3, dot4, fmax, length_of3, normalize3};
use spirv_std::arch::IndexUnchecked;
use spirv_std::glam::{UVec4, Vec2, Vec3, Vec4, Vec4Swizzles};
use spirv_std::spirv;

/// A constant buffer of N float4 rows.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Rows<const N: usize> {
    pub v: [Vec4; N],
}

/// One 3x4 skinning matrix, read as raw dwords exactly the way the original does.
fn load_bone(bones: &[u32], bi: u32, sk: UVec4) -> (Vec4, Vec4, Vec4) {
    let b = (bi * sk.y) + sk.x;
    let (i0, i1, i2) = (b >> 2, (b + 16) >> 2, (b + 32) >> 2);
    let at = |i: u32| unsafe { *bones.index_unchecked(i as usize) };
    let row = |i: u32| Vec4::new(f32::from_bits(at(i)), f32::from_bits(at(i + 1)), f32::from_bits(at(i + 2)), f32::from_bits(at(i + 3)));
    (row(i0), row(i1), row(i2))
}

/// mul(R, v) of a 3x3 by rows.
fn mul(r: &[Vec3; 3], v: Vec3) -> Vec3 {
    Vec3::new(dot3(r[0], v), dot3(r[1], v), dot3(r[2], v))
}

#[spirv(vertex)]
#[allow(clippy::too_many_arguments)]
pub fn reflex_vs(
    position: Vec3,
    blend_indices: UVec4,
    blend_weight: Vec4,
    texcoord: Vec2,
    normal: Vec3,
    tangent: Vec4,
    _color: Vec4,
    _texcoord_1: Vec2,
    instance_0: Vec4,
    instance_1: Vec4,
    instance_2: Vec4,
    skinning: UVec4,
    #[spirv(uniform, descriptor_set = 0, binding = 7)] k15: &Rows<32>,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k20: &Rows<33>,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] k25: &Rows<53>,
    #[spirv(uniform, descriptor_set = 0, binding = 5)] k30: &Rows<7>,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 10)] bones: &[u32],
    #[spirv(position)] out_pos: &mut Vec4,
    out_t0: &mut Vec4,
    out_t1: &mut Vec4,
    out_t2: &mut Vec4,
    out_t3: &mut Vec4,
    out_t4: &mut Vec4,
    #[spirv(clip_distance)] out_clip: &mut [f32; 1],
) {
    let rebase = UVec4::new(k25.v[38].x.to_bits(), k25.v[38].y.to_bits(), k25.v[38].z.to_bits(), k25.v[38].w.to_bits());
    // Dequantise: _30_m0[4] is the bounding box, _30_m0[5] its centre.
    let lp = (k30.v[4].xyz() * position) + k30.v[5].xyz();
    let wsum = fmax(1e-05, dot4(blend_weight, Vec4::ONE));
    let w = blend_weight / wsum;
    let (b0r0, b0r1, b0r2) = load_bone(bones, blend_indices.x, skinning);
    let (b1r0, b1r1, b1r2) = load_bone(bones, blend_indices.y, skinning);
    let (b2r0, b2r1, b2r2) = load_bone(bones, blend_indices.z, skinning);
    let (b3r0, b3r1, b3r2) = load_bone(bones, blend_indices.w, skinning);
    let m0 = b0r0 * w.x + b1r0 * w.y + b2r0 * w.z + b3r0 * w.w;
    let m1 = b0r1 * w.x + b1r1 * w.y + b2r1 * w.z + b3r1 * w.w;
    let m2 = b0r2 * w.x + b1r2 * w.y + b2r2 * w.z + b3r2 * w.w;
    let sp = Vec3::new(m0.w + dot3(m0.xyz(), lp), m1.w + dot3(m1.xyz(), lp), m2.w + dot3(m2.xyz(), lp));
    // Instance placement: the translation is int32 fixed point at 1/131072, rebased per frame.
    let fixed = |t: f32, base: u32| (t.to_bits().wrapping_sub(base) as i32) as f32 * 7.62939453125e-06;
    let ip = Vec3::new(
        dot3(instance_0.xyz(), sp) + fixed(instance_0.w, rebase.x),
        dot3(instance_1.xyz(), sp) + fixed(instance_1.w, rebase.y),
        dot3(instance_2.xyz(), sp) + fixed(instance_2.w, rebase.z),
    );
    let wp = k25.v[37].xyz() + ip;
    // The short eye-to-lens vector kept camera-relative BEFORE interpolation (strict: no reassociation).
    let eye_offset = k25.v[37].xyz() - k25.v[36].xyz();
    let eye_ray = ip + eye_offset;
    // The skin+instance rotation, applied to the tangent frame.
    let r = [
        Vec3::new(dot3(instance_0.xyz(), m0.xyz()), dot3(instance_0.xyz(), m1.xyz()), dot3(instance_0.xyz(), m2.xyz())),
        Vec3::new(dot3(instance_1.xyz(), m0.xyz()), dot3(instance_1.xyz(), m1.xyz()), dot3(instance_1.xyz(), m2.xyz())),
        Vec3::new(dot3(instance_2.xyz(), m0.xyz()), dot3(instance_2.xyz(), m1.xyz()), dot3(instance_2.xyz(), m2.xyz())),
    ];
    let n_l = (normal * 2.0) - 1.0;
    let t_l = (tangent.xyz() * 2.0) - 1.0;
    let h_l = (tangent.w * 2.0) - 1.0;
    let b_l = cross3(n_l, t_l) * h_l;
    // The UNNORMALISED transformed axes: their length is the skinning scale.
    let t_w_raw = mul(&r, t_l);
    let b_w_raw = mul(&r, b_l);
    let n_w = normalize3(mul(&r, n_l));
    let b_w = normalize3(b_w_raw);
    let t_w = normalize3(t_w_raw);
    *out_pos = if k15.v[1].x > 0.5 && k20.v[31].y.to_bits() == 2 {
        Vec4::new(0.0, 0.0, 0.0, 1.0)
    } else {
        Vec4::new(
            dot3(k25.v[28].xyz(), ip) + k25.v[28].w,
            dot3(k25.v[29].xyz(), ip) + k25.v[29].w,
            dot3(k25.v[30].xyz(), ip) + k25.v[30].w,
            dot3(k25.v[31].xyz(), ip) + k25.v[31].w,
        )
    };
    *out_t0 = Vec4::new(texcoord.x, texcoord.y, dot3(k25.v[26].xyz(), ip) + k25.v[26].w, 0.0);
    // THE ADDITION: the glass measured along its own uv axes, in world units (2 * box: POSITION is SNORM16).
    let extent = 2.0 * k30.v[4].xyz();
    // THE SIGHT'S OPTICAL AXIS: the slab's thin axis, pointing the normal's way, in the glass's tangent frame.
    let e = k30.v[4].xyz();
    let mut axis_l = Vec3::new(1.0, 0.0, 0.0);
    if e.y <= e.x && e.y <= e.z {
        axis_l = Vec3::new(0.0, 1.0, 0.0);
    } else if e.z <= e.x && e.z <= e.y {
        axis_l = Vec3::new(0.0, 0.0, 1.0);
    }
    if dot3(axis_l, n_l) < 0.0 {
        axis_l = -axis_l;
    }
    let bore_w = normalize3(mul(&r, axis_l));
    let bn = dot3(bore_w, n_w);
    let bore = if bn.abs() > 1e-4 { Vec2::new(dot3(bore_w, t_w) / bn, dot3(bore_w, b_w) / bn) } else { Vec2::ZERO };
    *out_t1 = Vec4::new(length_of3(extent * t_l) * length_of3(t_w_raw), length_of3(extent * b_l) * length_of3(b_w_raw), bore.x, bore.y);
    *out_t2 = Vec4::new(n_w.x, n_w.y, n_w.z, b_w.x);
    *out_t3 = Vec4::new(b_w.y, b_w.z, t_w.x, t_w.y);
    *out_t4 = Vec4::new(t_w.z, eye_ray.x, eye_ray.y, eye_ray.z);
    out_clip[0] = dot4(k25.v[50], Vec4::new(wp.x, wp.y, wp.z, 1.0));
}
