//! The port's own shaders in Rust. Each module is one pass of the plugin, its entry points the stages the pass's
//! pipelines use; the descriptor bindings are the pass's own, mapped onto its D3D12 root signature's registers by
//! ../builder (the table there). Written for D3D12's conventions: clip space y up, SV_Position's pixel centres.
#![cfg_attr(target_arch = "spirv", no_std)]
// SPIR-V written by hand where spirv-std has no call (OpDot: HLSL's dot, see `dot3`)
#![cfg_attr(target_arch = "spirv", feature(asm_experimental_arch))]

pub mod color_blit;
pub mod common;
pub mod finer_vrs;
pub mod foveated;
pub mod framegen;
pub mod fsr3;
pub mod neural_render;
pub mod fullscreen;
pub mod group_resolve;
pub mod mirror;
pub mod optics;
pub mod optics_resolve;
pub mod osr;
pub mod sight;
pub mod sight_ps;
pub mod single_pass;
pub mod space_quad;
pub mod stereo_depth;
pub mod vision;
pub mod world_markers;
