//! The C ABI of the render-parity data model, the fog history's arithmetic and the sun cascades' sampling matrices
//! (`fog_history` and `cascade_sampling`, already Rust's) for the C++ headers they came from:
//! `include/Stereo/RenderParity.hpp`, `include/Stereo/FogHistory.hpp` and `include/Stereo/CascadeSampling.hpp`. In the
//! Rust build (`CVR_RUST_VRIK`) their inline rules forward here (Grading.cpp and the asymmetric projection still read
//! them); `tools/render_parity_tests` runs its cases against them in its Rust variants.

use crate::cascade_sampling::{
    active_cascade_matrices, same_cascade_sampling_layout, CASC_MAT_FLOATS,
};
use crate::fog_history::{
    self as fog, AaSample, FogAllocation, FogCamera, FogGrid, FogProjection, FogSample,
    RenderOwner, SkyRadiance, SkyRadianceSample,
};

const _: () = assert!(
    core::mem::size_of::<RenderOwner>() == 56
        && core::mem::size_of::<AaSample>() == 72
        && core::mem::size_of::<SkyRadianceSample>() == 80
        && core::mem::size_of::<FogAllocation>() == 32
        && core::mem::size_of::<FogSample>() == 120
);

/// VrcamGraphHash (`which` 0), PreserveViewBit33 (1).
#[no_mangle]
pub extern "C-unwind" fn cvr_rpd_bits(which: u32, a: u64, b: u64) -> u64 {
    if which == 0 {
        fog::vrcam_graph_hash(a, b)
    } else {
        fog::preserve_view_bit33(a, b)
    }
}
/// IsMainAaContext.
#[no_mangle]
pub extern "C-unwind" fn cvr_rpd_is_main_aa_context(
    context: usize,
    name: u64,
    main_context: usize,
) -> bool {
    fog::is_main_aa_context(context, name, main_context)
}
/// RenderOwner's (`which` 0) / FogAllocation's (1) validity.
///
/// # Safety
/// `value` is a valid record of that kind.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rpd_valid(which: u32, value: *const core::ffi::c_void) -> bool {
    unsafe {
        if which == 0 {
            (*value.cast::<RenderOwner>()).valid()
        } else {
            (*value.cast::<FogAllocation>()).valid()
        }
    }
}
/// AaSample::Read (`out` written on success).
///
/// # Safety
/// The pointers are valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rpd_aa_read(
    sample: *const AaSample,
    now: *const RenderOwner,
    next: u32,
    out: *mut u32,
) -> bool {
    unsafe {
        match (*sample).read(&*now, next) {
            Some(mode) => {
                *out = mode;
                true
            }
            None => false,
        }
    }
}
/// ValidSkyRadiance.
///
/// # Safety
/// `value` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rpd_valid_sky(value: *const SkyRadiance) -> bool {
    unsafe { fog::valid_sky_radiance(&*value) }
}
/// SkyRadianceSample::Read (`out` written on success).
///
/// # Safety
/// The pointers are valid; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rpd_sky_read(
    sample: *const SkyRadianceSample,
    now: *const RenderOwner,
    next: u32,
    out: *mut SkyRadiance,
) -> bool {
    unsafe {
        match (*sample).read(&*now, next) {
            Some(color) => {
                *out = color;
                true
            }
            None => false,
        }
    }
}
/// FogSample::Eligible.
///
/// # Safety
/// The pointers are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rpd_fog_eligible(
    sample: *const FogSample,
    now: *const RenderOwner,
    registry: usize,
    next: u32,
    next_pose: u64,
    time_us: u64,
) -> bool {
    unsafe { (*sample).eligible(&*now, registry, next, next_pose, time_us) }
}
/// ReadFogGrid (`constants` null refused; `out` written on success).
///
/// # Safety
/// `constants` is null or holds the prepare-scene constants (at least 720 bytes); `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fog_read_grid(constants: *const u8, out: *mut FogGrid) -> bool {
    if constants.is_null() {
        return false;
    }
    unsafe {
        match fog::read_fog_grid(core::slice::from_raw_parts(constants, 43 * 16 + 32)) {
            Some(grid) => {
                *out = grid;
                true
            }
            None => false,
        }
    }
}
/// SameFogGrid (`which` 0) / SameFogFrustum (1).
///
/// # Safety
/// The arrays are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fog_same(
    which: u32,
    a: *const [f32; 16],
    b: *const [f32; 16],
) -> bool {
    unsafe {
        if which == 0 {
            fog::same_fog_grid(&*a, &*b)
        } else {
            fog::same_fog_frustum(&*a, &*b)
        }
    }
}
/// FogFrustum (`which` 0) / FogCurrentProjection (1) (`output` written on success).
///
/// # Safety
/// `camera` is valid; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fog_projection(
    which: u32,
    camera: *const FogCamera,
    output: *mut FogProjection,
) -> bool {
    unsafe {
        let result = if which == 0 {
            fog::fog_frustum(&*camera)
        } else {
            fog::fog_current_projection(&*camera)
        };
        match result {
            Some(projection) => {
                *output = projection;
                true
            }
            None => false,
        }
    }
}
/// ApplyFogHistoryProjection (`output` written on success).
///
/// # Safety
/// The pointers are valid; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fog_apply_history(
    native: *const FogCamera,
    source: *const FogProjection,
    output: *mut FogCamera,
) -> bool {
    unsafe {
        match fog::apply_fog_history_projection(&*native, &*source) {
            Some(camera) => {
                *output = camera;
                true
            }
            None => false,
        }
    }
}
/// CanBorrowFog.
///
/// # Safety
/// The pointers are valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_fog_can_borrow(
    sample: *const FogSample,
    owner: *const RenderOwner,
    registry: usize,
    frame: u32,
    pose: u64,
    time: u64,
    live: *const FogAllocation,
    scattering_output: *const FogAllocation,
    integrated_output: *const FogAllocation,
    source_grid: *const FogGrid,
    current_grid: *const FogGrid,
) -> bool {
    unsafe {
        fog::can_borrow_fog(
            &*sample,
            &*owner,
            registry,
            frame,
            pose,
            time,
            &*live,
            &*scattering_output,
            &*integrated_output,
            &*source_grid,
            &*current_grid,
        )
    }
}
/// ActiveCascadeMatrices (`matrices` null: 0).
///
/// # Safety
/// `matrices` is null or holds 64 floats.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_casc_active(matrices: *const [f32; CASC_MAT_FLOATS]) -> usize {
    if matrices.is_null() {
        return 0;
    }
    unsafe { active_cascade_matrices(&*matrices) }
}
/// SameCascadeSamplingLayout (`worst` null for none; a null matrix set has no active cascade).
///
/// # Safety
/// `a` and `b` are null or hold 64 floats; `worst` is null or writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_casc_same_layout(
    a: *const [f32; CASC_MAT_FLOATS],
    b: *const [f32; CASC_MAT_FLOATS],
    worst: *mut f32,
) -> bool {
    unsafe {
        let mut value = 0.0f32;
        let same = if a.is_null() || b.is_null() {
            value = 1.0;
            false
        } else {
            same_cascade_sampling_layout(&*a, &*b, &mut value)
        };
        if !worst.is_null() {
            *worst = value;
        }
        same
    }
}
