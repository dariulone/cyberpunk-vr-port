//! The stereo group resolve (rust/crates/cvr-gpu/src/group_resolve.rs): one eye's slice of the paired targets copied
//! into the engine's own -- the three colour targets and the depth, by the pixel -- where the stencil marks it drawn
//! (255 is reserved as untouched coverage; a native reference of 255 is rejected). Formerly its HLSL (VS, PS,
//! DepthPS).
//!
//! Bindings (descriptor set 0): 0 the eye (b0, one root constant), 1-3 the colour slices (t0-t2), 4 the depth (t3),
//! 5 the stencil (t4).
use spirv_std::glam::{vec4, IVec3, UVec4, Vec4};
use spirv_std::{spirv, Image};

type Slices = Image!(2D, type = f32, sampled, arrayed);
type StencilSlices = Image!(2D, type = u32, sampled, arrayed);

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Params {
    pub eye: u32,
}

/// float4(id == 2 ? 3 : -1, id == 1 ? 3 : -1, 0, 1).
#[spirv(vertex)]
pub fn vs(#[spirv(vertex_index)] id: i32, #[spirv(position)] out_position: &mut Vec4) {
    *out_position = vec4(if id == 2 { 3.0 } else { -1.0 }, if id == 1 { 3.0 } else { -1.0 }, 0.0, 1.0);
}

/// int4(int2(p.xy), Eye, 0).
fn at(position: Vec4, params: &Params) -> IVec3 {
    IVec3::new(position.x as i32, position.y as i32, params.eye as i32)
}

#[spirv(fragment(depth_replacing))]
#[allow(clippy::too_many_arguments)]
pub fn ps(
    #[spirv(frag_coord)] position: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &Params,
    #[spirv(descriptor_set = 0, binding = 1)] c0: &Slices,
    #[spirv(descriptor_set = 0, binding = 2)] c1: &Slices,
    #[spirv(descriptor_set = 0, binding = 3)] c2: &Slices,
    #[spirv(descriptor_set = 0, binding = 4)] depth: &Slices,
    #[spirv(descriptor_set = 0, binding = 5)] stencil: &StencilSlices,
    out0: &mut Vec4,
    out1: &mut Vec4,
    out2: &mut Vec4,
    #[spirv(frag_depth)] out_depth: &mut f32,
) {
    let q = at(position, params);
    let s: UVec4 = stencil.fetch(q);
    if s.y == 255 {
        spirv_std::arch::kill();
    }
    *out0 = c0.fetch(q);
    *out1 = c1.fetch(q);
    *out2 = c2.fetch(q);
    let d: Vec4 = depth.fetch(q);
    *out_depth = d.x;
}

#[spirv(fragment(depth_replacing))]
pub fn depth_ps(
    #[spirv(frag_coord)] position: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] params: &Params,
    #[spirv(descriptor_set = 0, binding = 4)] depth: &Slices,
    #[spirv(descriptor_set = 0, binding = 5)] stencil: &StencilSlices,
    #[spirv(frag_depth)] out_depth: &mut f32,
) {
    let q = at(position, params);
    let s: UVec4 = stencil.fetch(q);
    if s.y == 255 {
        spirv_std::arch::kill();
    }
    let d: Vec4 = depth.fetch(q);
    *out_depth = d.x;
}
