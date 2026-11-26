//! The stereo geometry pass's source draw admission (src/Render/StereoGeometryPass.cpp,
//! `StereoGeometryPass::CreateSources`): a source draw replayed as recorded needs its required flags (bits 4 and 5, not
//! bit 6), a known shading rate, a fixed raster state, single sampling, a triangle topology type, at most eight render
//! targets and as many as the draw bound, a triangle list or strip, a stencil reference below 256, a finite non-empty
//! viewport inside the depth range with an ordered scissor, a finite blend factor, a valid shading rate and combiners.
//!
//! In the Rust build (`CVR_RUST_VRIK`) those checks forward to `cvr_sgp_source_admitted` (the pipeline, root and rate
//! image identities and devices stay the .cpp's); `tools/single_pass_tests/geometry_pass_test` reaches it in its Rust
//! variant, and the unit test below covers each condition.

use cvr_gpu::scene_state::Rate;
use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP};
use windows::Win32::Graphics::Direct3D12::*;

use crate::geometry_commands::DrawRecord;

/// The contract fields the admission reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct Contract {
    pub fixed_raster: bool,
    pub sample_count: u32,
    pub sample_quality: u32,
    pub topology_type: i32,
    pub render_targets: u32,
}

const RATES: [D3D12_SHADING_RATE; 7] = [
    D3D12_SHADING_RATE_1X1,
    D3D12_SHADING_RATE_1X2,
    D3D12_SHADING_RATE_2X1,
    D3D12_SHADING_RATE_2X2,
    D3D12_SHADING_RATE_2X4,
    D3D12_SHADING_RATE_4X2,
    D3D12_SHADING_RATE_4X4,
];

/// CreateSources' per-draw checks.
pub fn source_admitted(draw: &DrawRecord, rate: &Rate, topology: i32, stencil: u32, blend: &[f32; 4], contract: &Contract) -> bool {
    let (v, s) = (&draw.viewport, &draw.scissor);
    draw.flags & 48 == 48
        && draw.flags & 64 == 0
        && rate.known
        && contract.fixed_raster
        && contract.sample_count == 1
        && contract.sample_quality == 0
        && contract.topology_type == D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE.0
        && contract.render_targets <= 8
        && contract.render_targets == draw.rt_count
        && (topology == D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST.0 || topology == D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP.0)
        && stencil <= 255
        && [v.TopLeftX, v.TopLeftY, v.Width, v.Height, v.MinDepth, v.MaxDepth].iter().all(|x| x.is_finite())
        && v.Width > 0.0
        && v.Height > 0.0
        && v.MinDepth >= 0.0
        && v.MaxDepth <= 1.0
        && v.MinDepth <= v.MaxDepth
        && s.right >= s.left
        && s.bottom >= s.top
        && blend.iter().all(|x| x.is_finite())
        && RATES.iter().any(|r| r.0 == rate.value)
        && rate.combiners.iter().all(|&c| c >= D3D12_SHADING_RATE_COMBINER_PASSTHROUGH.0 && c <= D3D12_SHADING_RATE_COMBINER_SUM.0)
}

// ---- the C ABI

/// # Safety
/// `draw` and `rate` are valid; `blend` holds four floats.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sgp_source_admitted(
    draw: *const DrawRecord,
    rate: *const Rate,
    topology: i32,
    stencil: u32,
    blend: *const [f32; 4],
    fixed_raster: bool,
    sample_count: u32,
    sample_quality: u32,
    topology_type: i32,
    render_targets: u32,
) -> bool {
    let contract = Contract { fixed_raster, sample_count, sample_quality, topology_type, render_targets };
    unsafe { source_admitted(&*draw, &*rate, topology, stencil, &*blend, &contract) }
}

#[cfg(test)]
mod tests {
    //! Each admission condition.
    use super::*;

    #[test]
    fn admissions() {
        // SAFETY: a plain record, all zero.
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.flags = 48;
        d.rt_count = 1;
        d.viewport = D3D12_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: 8.0, Height: 8.0, MinDepth: 0.0, MaxDepth: 1.0 };
        d.scissor = windows::Win32::Foundation::RECT { left: 0, top: 0, right: 8, bottom: 8 };
        let rate = Rate { known: true, ..Rate::default() };
        let c = Contract { fixed_raster: true, sample_count: 1, sample_quality: 0, topology_type: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE.0, render_targets: 1 };
        let list = D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST.0;
        assert!(source_admitted(&d, &rate, list, 0, &[1.0; 4], &c));
        assert!(!source_admitted(&DrawRecord { flags: 48 | 64, ..d }, &rate, list, 0, &[1.0; 4], &c));
        assert!(!source_admitted(&DrawRecord { flags: 16, ..d }, &rate, list, 0, &[1.0; 4], &c));
        assert!(!source_admitted(&d, &Rate::default(), list, 0, &[1.0; 4], &c));
        assert!(!source_admitted(&d, &rate, 1, 0, &[1.0; 4], &c));
        assert!(!source_admitted(&d, &rate, list, 256, &[1.0; 4], &c));
        assert!(!source_admitted(&d, &rate, list, 0, &[1.0, 1.0, f32::INFINITY, 1.0], &c));
        assert!(!source_admitted(&d, &rate, list, 0, &[1.0; 4], &Contract { render_targets: 2, ..c }));
        assert!(!source_admitted(&d, &rate, list, 0, &[1.0; 4], &Contract { fixed_raster: false, ..c }));
        assert!(!source_admitted(&d, &Rate { value: 3, ..rate }, list, 0, &[1.0; 4], &c));
        assert!(!source_admitted(&d, &Rate { combiners: [0, 5], ..rate }, list, 0, &[1.0; 4], &c));
        let mut bad = d;
        bad.viewport.MaxDepth = f32::NAN;
        assert!(!source_admitted(&bad, &rate, list, 0, &[1.0; 4], &c));
        bad = d;
        bad.scissor.right = -1;
        assert!(!source_admitted(&bad, &rate, list, 0, &[1.0; 4], &c));
    }
}
