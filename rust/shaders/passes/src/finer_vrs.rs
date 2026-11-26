//! The finer of several variable-rate shading images (rust/crates/cvr-gpu/src/finer_vrs.rs), formerly its HLSL: per
//! tile the finest rate of every input (the smaller of each axis), an input's invalid rate counted in the status and
//! taken as the finest.
//!
//! Bindings (descriptor set 0): 0 the parameters (b0), 1 the inputs (t0..t511, one table), 2 the output rate (u0), 3
//! the status (u1, a root UAV: no size -- read unchecked).
use spirv_std::arch::{atomic_or, IndexUnchecked};
use spirv_std::glam::{IVec2, UVec2, UVec3, UVec4};
use spirv_std::memory::{Scope, Semantics};
use spirv_std::{spirv, Image};

type Rate = Image!(2D, type = u32, sampled);
type OutRate = Image!(2D, format = r8ui, sampled = false);

/// b0: uint width, height, count, padding.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Parameters {
    pub width: u32,
    pub height: u32,
    pub count: u32,
    pub padding: u32,
}

/// status.Store2(0, uint2(0, 0)).
#[spirv(compute(threads(1, 1, 1)))]
pub fn initialize(#[spirv(storage_buffer, descriptor_set = 0, binding = 3)] status: &mut [u32]) {
    unsafe {
        *status.index_unchecked_mut(0) = 0;
        *status.index_unchecked_mut(1) = 0;
    }
}

#[spirv(compute(threads(8, 8, 1)))]
pub fn reduce(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Parameters,
    #[spirv(descriptor_set = 0, binding = 1)] inputs: &[Rate; 512],
    #[spirv(descriptor_set = 0, binding = 2)] output: &OutRate,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 3)] status: &mut [u32],
) {
    if id.x >= k.width || id.y >= k.height {
        return;
    }
    let (mut fx, mut fy) = (2u32, 2u32);
    let mut i = 0u32;
    while i < k.count {
        let texel: UVec4 = inputs[i as usize].fetch(IVec2::new(id.x as i32, id.y as i32));
        let mut rate = texel.x;
        let valid = rate == 0 || rate == 1 || rate == 4 || rate == 5 || rate == 6 || rate == 9 || rate == 10;
        if !valid {
            unsafe { atomic_or::<u32, { Scope::QueueFamily as u32 }, { Semantics::NONE.bits() }>(status.index_unchecked_mut(0), 1) };
            rate = 0;
        }
        fx = fx.min(rate >> 2);
        fy = fy.min(rate & 3);
        i += 1;
    }
    unsafe { output.write(UVec2::new(id.x, id.y), (fx << 2) | fy) };
}
