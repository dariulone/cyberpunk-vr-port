//! The port's own shaders as DXIL, made from Rust (rust/shaders/passes, rust-gpu -> naga -> DXC) by
//! `cargo xtask shaders` and committed in ../shaders with MANIFEST.txt; embedded here 16-byte aligned, as the
//! reference build's shader arrays are.

/// A shader's bytes at a 16-byte boundary.
#[repr(C, align(16))]
pub struct Aligned<const N: usize>(pub [u8; N]);

macro_rules! dxil {
    ($($name:ident = $file:literal;)*) => {
        $(
            #[doc = concat!("rust/shaders -> ../shaders/", $file, ".dxil")]
            pub static $name: &[u8] = {
                const LEN: usize = include_bytes!(concat!("../shaders/", $file, ".dxil")).len();
                static BYTES: Aligned<LEN> = Aligned(*include_bytes!(concat!("../shaders/", $file, ".dxil")));
                &BYTES.0
            };
        )*
        /// Every embedded shader by its DXIL's name.
        pub static ALL: &[(&str, &[u8])] = &[$(($file, $name)),*];
    };
}

dxil! {
    SPACE_QUAD_VS = "space_quad_vs";
    SPACE_QUAD_FILL = "space_quad_fill";
    SPACE_QUAD_OVER = "space_quad_over";
    SPACE_QUAD_COPY = "space_quad_copy";
    SPACE_QUAD_LIGHT = "space_quad_light";
    SRGB_VS = "srgb_vs";
    SRGB_PS = "srgb_ps";
    FULLSCREEN_UV_VS = "fullscreen_uv_vs";
    SHARPEN_PS = "sharpen_ps";
    DEPTH_RESOLVE_PS = "depth_resolve_ps";
    GROUP_RESOLVE_VS = "group_resolve_vs";
    GROUP_RESOLVE_PS = "group_resolve_ps";
    GROUP_RESOLVE_DEPTH_PS = "group_resolve_depth_ps";
    COLOR_BLIT_VS = "color_blit_vs";
    COLOR_BLIT_PS = "color_blit_ps";
    COLOR_BLIT_OVERLAY_PS = "color_blit_overlay_ps";
    COLOR_BLIT_HUD_PS = "color_blit_hud_ps";
    COLOR_BLIT_DOT_VS = "color_blit_dot_vs";
    COLOR_BLIT_DOT_PS = "color_blit_dot_ps";
    FOVEATED_PREPARE_CS = "foveated_prepare_cs";
    FOVEATED_COMPOSITE_CS = "foveated_composite_cs";
    STEREO_DEPTH_CS = "stereo_depth_cs";
    DEPTH_COMMIT_GATE_DEPTH_CS = "depth_commit_gate_depth_cs";
    DEPTH_COMMIT_GATE_BUFFER_CS = "depth_commit_gate_buffer_cs";
    GEOMETRY_COMPARE_INITIALIZE_CS = "geometry_compare_initialize_cs";
    GEOMETRY_COMPARE_CS = "geometry_compare_cs";
    MULTILAYER_MEMBERSHIP_CS = "multilayer_membership_cs";
    FINER_VRS_INITIALIZE_CS = "finer_vrs_initialize_cs";
    FINER_VRS_REDUCE_CS = "finer_vrs_reduce_cs";
    OSR_CLEAR_CS = "osr_clear_cs";
    OSR_SPLAT_CS = "osr_splat_cs";
    OSR_RESOLVE_CS = "osr_resolve_cs";
    OSR_INPAINT_CS = "osr_inpaint_cs";
    FRAMEGEN_PACK_CS = "framegen_pack_cs";
    FRAMEGEN_FLOW_CS = "framegen_flow_cs";
    FRAMEGEN_SCENE_CUT_CS = "framegen_scene_cut_cs";
    FRAMEGEN_PREPARE_INPUTS_CS = "framegen_prepare_inputs_cs";
    FSR3_DILATE_CLEAR_CS = "fsr3_dilate_clear_cs";
    FSR3_DILATE_CS = "fsr3_dilate_cs";
    FSR3_DILATE_RESOLVE_CS = "fsr3_dilate_resolve_cs";
    FSR3_WARP_CS = "fsr3_warp_cs";
    NR_ENCODE_CS = "nr_encode_cs";
    NR_DECODE_CS = "nr_decode_cs";
    MIRROR_VS = "mirror_vs";
    MIRROR_PS = "mirror_ps";
    LENS_MASK_PS = "lens_mask_ps";
    LENS_VELOCITY_PS = "lens_velocity_ps";
    SIGHT_MASK_PS = "sight_mask_ps";
    LENS_SOURCE_RESOLVE_CS = "lens_source_resolve_cs";
    CAMERA_SOURCE_RESOLVE_CS = "camera_source_resolve_cs";
    WORLD_MARKER_SPRITE_VS = "world_marker_sprite_vs";
    WORLD_MARKER_PROCEDURAL_VS = "world_marker_procedural_vs";
    WORLD_MARKER_SPRITE_STEREO_VS = "world_marker_sprite_stereo_vs";
    WORLD_MARKER_TEXT_STEREO_VS = "world_marker_text_stereo_vs";
    WORLD_MARKER_SPRITE_STEREO_Z_VS = "world_marker_sprite_stereo_z_vs";
    WORLD_MARKER_SPRITE_STEREO_ONE_VS = "world_marker_sprite_stereo_one_vs";
    VISION_HIGHLIGHT_CS = "vision_highlight_cs";
    SIGHT_REFLEX_VS = "sight_reflex_vs";
    SIGHT_REFLEX_PS = "sight_reflex_ps";
    SIGHT_REFLEX_PS_SCOPE = "sight_reflex_ps_scope";
}

/// A stage's code: the DXIL made from Rust, or the HLSL compiled by FXC (the reference, the way back).
pub enum Code {
    Dxil(&'static [u8]),
    Hlsl(windows::Win32::Graphics::Direct3D::ID3DBlob),
}
impl Code {
    /// The bytes as a pipeline description takes them (alive as long as the code).
    pub fn bytecode(&self) -> windows::Win32::Graphics::Direct3D12::D3D12_SHADER_BYTECODE {
        use windows::Win32::Graphics::Direct3D12::D3D12_SHADER_BYTECODE;
        match self {
            Code::Dxil(bytes) => D3D12_SHADER_BYTECODE { pShaderBytecode: bytes.as_ptr().cast(), BytecodeLength: bytes.len() },
            // SAFETY: the blob's own buffer, alive as long as the blob.
            Code::Hlsl(blob) => unsafe { D3D12_SHADER_BYTECODE { pShaderBytecode: blob.GetBufferPointer(), BytecodeLength: blob.GetBufferSize() } },
        }
    }
}
/// The stage's code: `dxil`, or with CVR_SHADER_HLSL the HLSL `hlsl` compiles.
pub fn pick(dxil: &'static [u8], hlsl: impl FnOnce() -> Option<windows::Win32::Graphics::Direct3D::ID3DBlob>) -> Option<Code> {
    if hlsl_for(dxil) {
        hlsl().map(Code::Hlsl)
    } else {
        Some(Code::Dxil(dxil))
    }
}

/// The shaders that draw with their HLSL: None none (CVR_SHADER_RUST=1: every pass with the DXIL made from Rust); an
/// empty list every shader (the default); else the DXIL names CVR_SHADER_HLSL lists.
fn hlsl_list() -> &'static Option<Vec<String>> {
    static LIST: std::sync::OnceLock<Option<Vec<String>>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        if std::env::var("CVR_SHADER_RUST").is_ok_and(|v| v == "1") {
            cvr_host::log!("[shaders] CVR_SHADER_RUST=1: the passes draw with the DXIL made from Rust\n");
            return None;
        }
        let value = std::env::var("CVR_SHADER_HLSL").unwrap_or_default();
        let names: Vec<String> = value.split(',').map(str::trim).filter(|n| !n.is_empty() && *n != "1").map(String::from).collect();
        Some(names)
    })
}

/// THE C++ BUILD'S SHADERS BY DEFAULT: every pass draws with the HLSL it had in the C++ build, compiled at first use as
/// there (the DXIL made from Rust showed artifacts in the headset the WARP bench's parity did not -- the driver compiles
/// DXBC and DXIL differently). CVR_SHADER_RUST=1 draws every pass with the DXIL made from Rust; CVR_SHADER_HLSL set to
/// a comma list of DXIL names (`osr_resolve_cs,osr_inpaint_cs`) keeps the HLSL for those only -- a difference narrowed
/// down to one stage.
pub fn hlsl_requested() -> bool {
    hlsl_list().is_some()
}

/// Whether the shader `dxil` (one of ALL) draws with its HLSL.
pub fn hlsl_for(dxil: &'static [u8]) -> bool {
    match hlsl_list() {
        None => false,
        Some(names) if names.is_empty() => true,
        Some(names) => ALL.iter().any(|&(name, bytes)| bytes.as_ptr() == dxil.as_ptr() && names.iter().any(|n| n == name)),
    }
}

/// The shaders of ALL a pipeline was made of in this process (bit n: ALL[n]), and those a pipeline was refused with.
static MADE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static REFUSED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Stage 8's witness in game (a real pipeline state for every shader made from Rust): `code`, a pipeline description's
/// shader, and whether the device made the pipeline. One of ALL: its name the first time it is made, or refused, into
/// the log. The device hooks call it for every creation (the passes' own pass through them too).
pub fn witness(code: *const core::ffi::c_void, made: bool) {
    use core::sync::atomic::Ordering::Relaxed;
    if code.is_null() {
        return;
    }
    let Some(i) = ALL.iter().position(|&(_, bytes)| bytes.as_ptr().cast() == code) else { return };
    let bit = 1u64 << i;
    let seen = if made { &MADE } else { &REFUSED };
    if seen.fetch_or(bit, Relaxed) & bit == 0 {
        let what = if made { "a pipeline made" } else { "a pipeline REFUSED" };
        let total = MADE.load(Relaxed).count_ones();
        cvr_host::log!("[shaders] {}: {what} (made from Rust: {total} of {})\n", ALL[i].0, ALL.len());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn signed_dxil_containers() {
        assert!(super::ALL.len() <= 64, "witness keeps a bit per shader");
        assert!(super::ALL.len() >= 54);
        for &(name, bytes) in super::ALL {
            assert_eq!(&bytes[..4], b"DXBC", "{name}");
            assert!(bytes[4..20].iter().any(|&b| b != 0), "{name}: no validator digest");
            assert_eq!(bytes.as_ptr() as usize % 16, 0, "{name}: not aligned");
            let size = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]) as usize;
            assert_eq!(size, bytes.len(), "{name}: container size");
        }
    }
}
