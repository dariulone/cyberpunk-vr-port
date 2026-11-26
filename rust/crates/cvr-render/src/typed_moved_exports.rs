//! The exported variables the generated moves left in C++ (used_moved_exports, used_moved_exports_rest), defined in
//! Rust by hand: the std::atomic<float> ones (an AtomicU32 holding the float's bits: std::atomic<float> is the float's
//! four bytes), the JSON report buffers (a char array holding its initial string, zero-filled), and the ones sized or
//! initialized by the C++ half's named constants (include/Render/SsrWideMerge.hpp's block sizes, NativeDepthPreparation's
//! counter count, HudClearance's hand-out scale, FrameGraph.cpp's bootstrap flags, EngineRvas' volumetric fog node).
//! Each source keeps an extern declaration in its Rust branch. A C++ program including the sources' headers and a Rust
//! program built from this module print every variable's bytes; they are equal.
#![allow(non_upper_case_globals)]
use core::sync::atomic::AtomicU32;

/// A char buffer of `N` bytes holding `text`, zero-filled.
const fn text<const N: usize>(text: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < text.len() {
        out[i] = text[i];
        i += 1;
    }
    out
}
/// include/Render/SsrWideMerge.hpp.
const BLOCK_BYTES: usize = 704;
const AO_BLOCK_BYTES: usize = 240;
const SHADOW_BLOCK_BYTES: usize = 64;
const EXPOSURE_CELLS: usize = 60;

// ---- src/Natives/HudClearance.cpp
/// HudClearance.hpp's kHandOutScale.
#[no_mangle]
pub static mut CyberpunkVR_HudClearanceHandsScale: f32 = 2.0;
// ---- src/Render/CameraSourceResolve.cpp
#[no_mangle]
pub static CyberpunkVR_OpticsCameraAlpha: AtomicU32 = AtomicU32::new(0.12f32.to_bits());
#[no_mangle]
pub static CyberpunkVR_OpticsCameraMotionAlpha: AtomicU32 = AtomicU32::new(0.35f32.to_bits());
#[no_mangle]
pub static CyberpunkVR_OpticsCameraGamma: AtomicU32 = AtomicU32::new(1.25f32.to_bits());
#[no_mangle]
pub static CyberpunkVR_OpticsCameraResolveMicroseconds: AtomicU32 = AtomicU32::new(0);
// ---- src/Render/FixedVrs.cpp
#[no_mangle]
pub static mut CyberpunkVR_FixedVrsReport: [u8; 1024] = text(b"{}");
// ---- src/Render/GpuStageProfile.cpp
#[no_mangle]
pub static mut CyberpunkVR_GpuStageReport: [u8; 2048] = text(br#"{"status":"idle"}"#);
// ---- src/Render/HiddenArea.cpp
#[no_mangle]
pub static CyberpunkVR_HiddenAreaRadius: AtomicU32 = AtomicU32::new(115.0f32.to_bits());
// ---- src/Render/LensSourceResolve.cpp
#[no_mangle]
pub static CyberpunkVR_OpticsResolveAlpha: AtomicU32 = AtomicU32::new(0.2f32.to_bits());
#[no_mangle]
pub static CyberpunkVR_OpticsResolveGamma: AtomicU32 = AtomicU32::new(1.0f32.to_bits());
#[no_mangle]
pub static CyberpunkVR_OpticsResolveMicroseconds: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_OpticsLensSharpen: AtomicU32 = AtomicU32::new(0.35f32.to_bits());
// ---- src/Render/MonoCascades.cpp
#[no_mangle]
pub static mut CyberpunkVR_MonoCascadeReport: [u8; 4096] = text(b"{}");
// ---- src/Render/NativeDepthPreparation.cpp
/// One count per eager_depth::Counter (CounterCount, 16).
#[no_mangle]
pub static mut CyberpunkVR_EagerDepthStats: [u32; 16] = [0; 16];
// ---- src/Render/SceneGpuProfile.cpp
#[no_mangle]
pub static mut CyberpunkVR_SceneGpuReport: [u8; 262144] = text(br#"{"active":false}"#);
// ---- src/Render/SsrWideMerge.cpp
#[no_mangle]
pub static mut CyberpunkVR_DebugSsrBlocks: [[f32; BLOCK_BYTES / 4]; 2] = [[0.0; BLOCK_BYTES / 4]; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugSsprBlocks: [[f32; BLOCK_BYTES / 4]; 2] = [[0.0; BLOCK_BYTES / 4]; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugAoBlocks: [[f32; AO_BLOCK_BYTES / 4]; 2] = [[0.0; AO_BLOCK_BYTES / 4]; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugShadowBlocks: [[u32; SHADOW_BLOCK_BYTES / 4]; 2] = [[0; SHADOW_BLOCK_BYTES / 4]; 2];
#[no_mangle]
pub static mut CyberpunkVR_DebugExpoCells: [u32; 2 * EXPOSURE_CELLS * 3] = [0; 2 * EXPOSURE_CELLS * 3];
// ---- src/Stereo/EngineStereo.cpp
#[no_mangle]
pub static mut CyberpunkVR_DebugEngineStereoLoadingLayer: [i32; 2] = [i32::MIN, -1];
// ---- src/Stereo/FrameGraph.cpp
/// kBootstrapMainFlags' two words.
#[no_mangle]
pub static mut CyberpunkVR_DebugFgMainF0: u64 = 0x7800_017F_AD75_FF51;
#[no_mangle]
pub static mut CyberpunkVR_DebugFgMainF1: u64 = 0x0000_0000_A0BF_8010;
// ---- src/Stereo/Grading.cpp
/// EngineRvas.hpp's VOLUMETRIC_FOG_NODE_RVA.
#[no_mangle]
pub static mut CyberpunkVR_BlockDiffNode: u32 = 0xDDB6B8;
/// SsrWideMerge.hpp's kAoBlockBytes.
#[no_mangle]
pub static mut CyberpunkVR_AsymAoBlockSize: u32 = AO_BLOCK_BYTES as u32;
