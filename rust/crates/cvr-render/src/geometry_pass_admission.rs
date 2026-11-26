//! The stereo geometry pass's draw admission bits (src/Render/StereoGeometryPass.cpp, `StereoGeometryPass::Create`): a
//! paired draw is refused with its validation bits -- 1 an instance stream not safe to rebase, 2 a raster state not
//! fixed, 4 multisampling, 8 a non-triangle topology type, 16 a target count other than the pass's colors, 32 a depth
//! mismatch, 64 the main eye's original pipeline not the draw's (or on another device), 128 a topology neither a
//! triangle list nor strip, and the eyes' state differences above bit 8 (without the shading rate bits 32-128 for a
//! pass without a pixel shader).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the bits forward to `cvr_sgp_validation_bits` (the pipeline's device and the state
//! differences are the .cpp's inputs); `tools/single_pass_tests/geometry_pass_test` runs its cases against it in its
//! Rust variant, and the unit test below covers each bit.

use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP};
use windows::Win32::Graphics::Direct3D12::D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;

/// The inputs Create's validation bits read.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    pub rebase_safe: bool,
    pub fixed_raster: bool,
    pub sample_count: u32,
    pub sample_quality: u32,
    pub topology_type: i32,
    pub render_targets: u32,
    pub colors: u32,
    pub has_depth: bool,
    pub depth_format: bool,
    pub original_matches: bool,
    pub topology: i32,
    pub differences: u32,
    pub pixel_shader: bool,
}
const _: () = assert!(core::mem::size_of::<Inputs>() == 40 && core::mem::offset_of!(Inputs, topology) == 28);

/// Create's validation bits.
pub fn validation_bits(i: &Inputs) -> u32 {
    (!i.rebase_safe) as u32
        | ((!i.fixed_raster) as u32) << 1
        | ((i.sample_count != 1 || i.sample_quality != 0) as u32) << 2
        | ((i.topology_type != D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE.0) as u32) << 3
        | ((i.render_targets != i.colors) as u32) << 4
        | ((i.has_depth != i.depth_format) as u32) << 5
        | ((!i.original_matches) as u32) << 6
        | ((i.topology != D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST.0 && i.topology != D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP.0) as u32) << 7
        | (i.differences & if i.pixel_shader { u32::MAX } else { !224u32 }) << 8
}

// ---- the C ABI

/// # Safety
/// `inputs` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgp_validation_bits(inputs: *const Inputs) -> u32 {
    validation_bits(unsafe { &*inputs })
}

#[cfg(test)]
mod tests {
    //! Each bit and the shading rate mask.
    use super::*;

    #[test]
    fn bits() {
        let ok = Inputs {
            rebase_safe: true,
            fixed_raster: true,
            sample_count: 1,
            sample_quality: 0,
            topology_type: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE.0,
            render_targets: 2,
            colors: 2,
            has_depth: true,
            depth_format: true,
            original_matches: true,
            topology: D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST.0,
            differences: 0,
            pixel_shader: true,
        };
        assert_eq!(validation_bits(&ok), 0);
        assert_eq!(validation_bits(&Inputs { rebase_safe: false, ..ok }), 1);
        assert_eq!(validation_bits(&Inputs { fixed_raster: false, ..ok }), 2);
        assert_eq!(validation_bits(&Inputs { sample_quality: 1, ..ok }), 4);
        assert_eq!(validation_bits(&Inputs { topology_type: 2, ..ok }), 8);
        assert_eq!(validation_bits(&Inputs { colors: 1, ..ok }), 16);
        assert_eq!(validation_bits(&Inputs { depth_format: false, ..ok }), 32);
        assert_eq!(validation_bits(&Inputs { original_matches: false, ..ok }), 64);
        assert_eq!(validation_bits(&Inputs { topology: D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP.0, ..ok }), 0);
        assert_eq!(validation_bits(&Inputs { topology: 1, ..ok }), 128);
        assert_eq!(validation_bits(&Inputs { differences: 0xE8, ..ok }), 0xE8 << 8);
        assert_eq!(validation_bits(&Inputs { differences: 0xE8, pixel_shader: false, ..ok }), 0x08 << 8);
    }
}
