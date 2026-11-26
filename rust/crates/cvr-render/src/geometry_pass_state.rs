//! The stereo geometry pass's eye state comparison (src/Render/StereoGeometryPass.cpp, `stereo::StateDifferences`): the
//! two eyes' draw states as difference bits -- 1 no original pipeline, 2 different pipelines, 4 no or different
//! topology, 8 a different stencil reference, 16 a different blend factor, 32 a shading rate not known for either, 64 a
//! shading rate image on either, 128 a different shading rate or combiners.
//!
//! In the Rust build (`CVR_RUST_VRIK`) StateDifferences forwards to `cvr_sgp_state_differences` (scene_state's Rate
//! mirrored); `tools/single_pass_tests/geometry_pass_test` runs its cases against it in its Rust variant, and the unit
//! test below covers each bit.

use cvr_gpu::scene_state::Rate;

/// One eye's state as StateDifferences reads it.
#[derive(Clone, Copy, Debug, Default)]
pub struct EyeState {
    pub original: u64,
    pub rate_image: u64,
    pub rate: Rate,
    pub topology: i32,
    pub stencil: u32,
    pub blend: [f32; 4],
}

/// StateDifferences.
pub fn state_differences(a: &EyeState, b: &EyeState) -> u32 {
    (a.original == 0) as u32
        | ((a.original != b.original) as u32) << 1
        | ((a.topology == 0 || a.topology != b.topology) as u32) << 2
        | ((a.stencil != b.stencil) as u32) << 3
        | ((a.blend != b.blend) as u32) << 4
        | ((!a.rate.known || !b.rate.known) as u32) << 5
        | ((a.rate.image != 0 || b.rate.image != 0 || a.rate_image != 0 || b.rate_image != 0) as u32) << 6
        | ((a.rate.value != b.rate.value || a.rate.combiners != b.rate.combiners) as u32) << 7
}

// ---- the C ABI

/// # Safety
/// The rates are valid; the blend factors hold four floats each.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sgp_state_differences(
    a_original: u64,
    b_original: u64,
    a_rate_image: u64,
    b_rate_image: u64,
    a_rate: *const Rate,
    b_rate: *const Rate,
    a_topology: i32,
    b_topology: i32,
    a_stencil: u32,
    b_stencil: u32,
    a_blend: *const [f32; 4],
    b_blend: *const [f32; 4],
) -> u32 {
    let a = EyeState { original: a_original, rate_image: a_rate_image, rate: unsafe { *a_rate }, topology: a_topology, stencil: a_stencil, blend: unsafe { *a_blend } };
    let b = EyeState { original: b_original, rate_image: b_rate_image, rate: unsafe { *b_rate }, topology: b_topology, stencil: b_stencil, blend: unsafe { *b_blend } };
    state_differences(&a, &b)
}

#[cfg(test)]
mod tests {
    //! Each difference bit.
    use super::*;

    #[test]
    fn bits() {
        let known = Rate { known: true, ..Rate::default() };
        let a = EyeState { original: 1, topology: 4, stencil: 0, blend: [1.0; 4], rate: known, ..Default::default() };
        assert_eq!(state_differences(&a, &a), 0);
        assert_eq!(state_differences(&EyeState { original: 0, ..a }, &a), 1 | 2);
        assert_eq!(state_differences(&a, &EyeState { topology: 5, ..a }), 4);
        assert_eq!(state_differences(&EyeState { topology: 0, ..a }, &EyeState { topology: 0, ..a }), 4);
        assert_eq!(state_differences(&a, &EyeState { stencil: 1, ..a }), 8);
        assert_eq!(state_differences(&a, &EyeState { blend: [1.0, 1.0, 1.0, 0.0], ..a }), 16);
        assert_eq!(state_differences(&a, &EyeState { rate: Rate::default(), ..a }), 32);
        assert_eq!(state_differences(&a, &EyeState { rate_image: 9, ..a }), 64);
        assert_eq!(state_differences(&a, &EyeState { rate: Rate { value: 5, ..known }, ..a }), 128);
        assert_eq!(state_differences(&a, &EyeState { rate: Rate { combiners: [1, 0], ..known }, ..a }), 128);
        let nan = EyeState { blend: [f32::NAN; 4], ..a };
        assert_eq!(state_differences(&nan, &nan), 16);
    }
}
