//! The frame generator's own passes (rust/crates/cvr-render/src/framegen_generator.rs), formerly its HLSL: the colour
//! packed to the generator's size (Pack), the engine's motion flow taken over (Flow), the scene-cut test against the
//! previous frame (SceneCut, a group's reduction), and the depth and motion the generator takes (PrepareInputs: the
//! nearest depth of a downsampled 2x2 and its motion). The arithmetic as FXC's listing has it (`fxc /T cs_5_0 /O3`).
//!
//! Bindings (descriptor set 0): 0 the parameters (b0, 6 root constants); 1..3 t0..t2 (colour, raw flow, previous --
//! PrepareInputs: source depth, source motion); 4..6 u0..u2 (packed, flow, scene cut -- PrepareInputs: target depth,
//! target motion); 7 the linear clamp sampler (s0, static).
use crate::common::dot3;
use spirv_std::arch::workgroup_memory_barrier_with_group_sync;
use spirv_std::glam::{IVec2, IVec4, UVec2, UVec3, Vec2, Vec3, Vec4, Vec4Swizzles};
#[cfg(target_arch = "spirv")]
use spirv_std::num_traits::Float;
use spirv_std::{spirv, Image, Sampler};

type Texture = Image!(2D, type = f32, sampled);
type IntTexture = Image!(2D, type = i32, sampled);
type OutColour = Image!(2D, format = rgba16f, sampled = false);
type OutFlow = Image!(2D, format = rg16i, sampled = false);
type OutCut = Image!(2D, format = r32ui, sampled = false);
type OutScalar = Image!(2D, format = r32f, sampled = false);
type OutPair = Image!(2D, format = rg16f, sampled = false);

/// b0: uint sw, sh, dw, dh, reset, clearScd (PrepareInputs: ..., inverted, padding).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Params {
    pub sw: u32,
    pub sh: u32,
    pub dw: u32,
    pub dh: u32,
    pub reset: u32,
    pub clear_scd: u32,
}

fn id_f(id: UVec3) -> Vec2 {
    Vec2::new(id.x as f32, id.y as f32)
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn pack(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Params,
    #[spirv(descriptor_set = 0, binding = 1)] color: &Texture,
    #[spirv(descriptor_set = 0, binding = 4)] packed: &OutColour,
    #[spirv(descriptor_set = 0, binding = 7)] linear: &Sampler,
) {
    if id.x < k.dw && id.y < k.dh {
        let c: Vec4 = color.sample_by_lod(*linear, (id_f(id) + 0.5) / Vec2::new(k.dw as f32, k.dh as f32), 0.0);
        unsafe { packed.write(UVec2::new(id.x, id.y), Vec4::new(c.x, c.y, c.z, 1.0)) };
    }
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn flow(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Params,
    #[spirv(descriptor_set = 0, binding = 2)] raw_flow: &IntTexture,
    #[spirv(descriptor_set = 0, binding = 5)] flow: &OutFlow,
    #[spirv(descriptor_set = 0, binding = 6)] scd: &OutCut,
) {
    if id.x < k.dw && id.y < k.dh {
        let pixel = (id_f(id) + 0.5) * Vec2::new(k.sw as f32, k.sh as f32) / Vec2::new(k.dw as f32, k.dh as f32);
        let p = UVec2::new((pixel.x as u32).min(k.sw - 1), (pixel.y as u32).min(k.sh - 1));
        let value = if k.reset != 0 {
            IVec2::ZERO
        } else {
            let v: IVec4 = raw_flow.fetch(IVec2::new(p.x as i32, p.y as i32));
            IVec2::new(v.x, v.y)
        };
        unsafe { flow.write(UVec2::new(id.x, id.y), value) };
    }
    if k.clear_scd != 0 && id.x == 0 && id.y == 0 {
        unsafe {
            scd.write(UVec2::new(0, 0), 0u32);
            scd.write(UVec2::new(1, 0), 0u32);
            scd.write(UVec2::new(2, 0), 0u32);
        }
    }
}

/// The mean luminance difference of an 8x8 grid between this frame and the previous; over 0.45 (or a reset) a cut.
#[spirv(compute(threads(8, 8, 1)))]
#[allow(clippy::too_many_arguments)]
pub fn scene_cut(
    #[spirv(local_invocation_id)] id: UVec3,
    #[spirv(local_invocation_index)] index: u32,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Params,
    #[spirv(descriptor_set = 0, binding = 1)] color: &Texture,
    #[spirv(descriptor_set = 0, binding = 3)] previous: &Texture,
    #[spirv(descriptor_set = 0, binding = 6)] scd: &OutCut,
    #[spirv(descriptor_set = 0, binding = 7)] linear: &Sampler,
    #[spirv(workgroup)] difference: &mut [f32; 64],
) {
    let uv = (id_f(id) + 0.5) / 8.0;
    let a: Vec4 = color.sample_by_lod(*linear, uv, 0.0);
    let b: Vec4 = previous.sample_by_lod(*linear, uv, 0.0);
    difference[index as usize] = dot3((a.xyz() - b.xyz()).abs(), Vec3::new(0.2126, 0.7152, 0.0722));
    workgroup_memory_barrier_with_group_sync();
    let mut step = 32u32;
    while step != 0 {
        if index < step {
            difference[index as usize] += difference[(index + step) as usize];
        }
        workgroup_memory_barrier_with_group_sync();
        step /= 2;
    }
    if index == 0 {
        let cut = if k.reset != 0 || difference[0] / 64.0 > 0.45 { 15u32 } else { 0 };
        unsafe {
            scd.write(UVec2::new(0, 0), cut);
            scd.write(UVec2::new(1, 0), cut);
            scd.write(UVec2::new(2, 0), cut);
        }
    }
}

/// The generator's depth and motion at its size: the depth at the pixel (the nearest of the 2x2 under it when the
/// source is larger), the motion at that depth's place.
#[spirv(compute(threads(8, 8, 1)))]
pub fn prepare_inputs(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Params,
    #[spirv(descriptor_set = 0, binding = 1)] source_depth: &Texture,
    #[spirv(descriptor_set = 0, binding = 2)] source_motion: &Texture,
    #[spirv(descriptor_set = 0, binding = 4)] target_depth: &OutScalar,
    #[spirv(descriptor_set = 0, binding = 5)] target_motion: &OutPair,
) {
    if id.x >= k.dw || id.y >= k.dh {
        return;
    }
    // PrepareInputs's fifth constant: whether the depth is inverted (near is 1)
    let inverted = k.reset != 0;
    let pixel = (id_f(id) + 0.5) * Vec2::new(k.sw as f32, k.sh as f32) / Vec2::new(k.dw as f32, k.dh as f32);
    let hi = IVec2::new(k.sw as i32 - 1, k.sh as i32 - 1);
    let mut chosen = IVec2::new((pixel.x as i32).min(hi.x), (pixel.y as i32).min(hi.y));
    let first: Vec4 = source_depth.fetch(chosen);
    let mut depth = first.x;
    if k.sw > k.dw || k.sh > k.dh {
        let base = IVec2::new((pixel.x - 0.5).floor() as i32, (pixel.y - 0.5).floor() as i32);
        let mut n = 0i32;
        while n < 4 {
            let q = base + IVec2::new(n & 1, n >> 1);
            let p = IVec2::new(q.x.max(0).min(hi.x), q.y.max(0).min(hi.y));
            let z: Vec4 = source_depth.fetch(p);
            let nearer = if inverted { z.x > depth } else { z.x < depth };
            if nearer {
                depth = z.x;
                chosen = p;
            }
            n += 1;
        }
    }
    let size: UVec2 = source_motion.query_size_lod(0);
    let at = (Vec2::new(chosen.x as f32, chosen.y as f32) + 0.5) * Vec2::new(size.x as f32, size.y as f32)
        / Vec2::new(k.sw as f32, k.sh as f32);
    let motion_pixel = UVec2::new((at.x as u32).min(size.x - 1), (at.y as u32).min(size.y - 1));
    let m: Vec4 = source_motion.fetch(IVec2::new(motion_pixel.x as i32, motion_pixel.y as i32));
    unsafe {
        target_depth.write(UVec2::new(id.x, id.y), depth);
        target_motion.write(UVec2::new(id.x, id.y), Vec2::new(m.x, m.y));
    }
}
