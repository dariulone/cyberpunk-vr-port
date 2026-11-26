//! The G-buffer material contract (`include/Render/GBufferMaterialContract.hpp`): for the exact G-buffer shader pair,
//! the dependencies one draw's material (292 bytes), modifiers (16) and global frame word (4) describe -- four texture
//! selectors (five with the optional texture), each below the PS's 32768 declared `t0, space1` SRVs, the required `s0`
//! sampler and the `t67` dither texture at the frame's slice -- after the floats the shader reads are all finite; and
//! whether a main and a peer draw's inputs are the same bytes. The caller establishes the shader pair and the bytes'
//! freshness; this proves neither descriptor validity nor GPU content lifetime.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `MatchesShaders`, `Analyze` and `SameInputs` forward to `cvr_gm_*`;
//! `tools/single_pass_tests/gbuffer_material_contract_test` runs its cases against them in its Rust variant.

pub const VERTEX_SHADER_SHA256: &str =
    "fe53e834aca0d9dacef4628fa10d2a6bbb698b02498673de99562733dd2904d1";
pub const PIXEL_SHADER_SHA256: &str =
    "8c5a09dee942322418112778412c6e08c43e1342f5bee642acdee6ff041e6cb5";
pub const MATERIAL_BYTES: usize = 292;
pub const MODIFIER_BYTES: usize = 16;
pub const GLOBAL_FRAME_BYTES: usize = 4;
pub const SELECTOR_OFFSETS: [u32; 5] = [0, 32, 80, 128, 160];
/// The exact PS declares `t0, space1` as `[32768 x Texture2D<float4>]`.
pub const DESCRIPTOR_COUNT: u32 = 32768;

/// gbuffer_material::TextureUse.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct TextureUse {
    pub selector_offset: u32,
    pub index: u32,
}
/// gbuffer_material::SamplerUse.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SamplerUse {
    pub shader_register: u32,
    pub register_space: u32,
    pub required: bool,
}
/// gbuffer_material::DitherUse.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct DitherUse {
    pub shader_register: u32,
    pub register_space: u32,
    pub mip: u32,
    pub slice: u32,
    pub coordinate_mask: u32,
    pub required: bool,
}
/// gbuffer_material::Analysis.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Analysis {
    pub textures: [TextureUse; 5],
    pub texture_count: u32,
    pub sampler: SamplerUse,
    pub dither: DitherUse,
    pub alpha_test_enabled: bool,
    pub optional_texture_enabled: bool,
    pub neutral_dither: bool,
}
const _: () = assert!(core::mem::size_of::<Analysis>() == 84);

/// gbuffer_material::Failure.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    None,
    MaterialSize,
    ModifierSize,
    GlobalFrameSize,
    NonFiniteModifier,
    NonFiniteMaterial,
    TextureIndex,
}

/// gbuffer_material::Inputs.
#[derive(Clone, Copy, Debug)]
pub struct Inputs<'a> {
    pub material: &'a [u8],
    pub modifiers: &'a [u8],
    pub global_frame_slice: &'a [u8],
}

/// gbuffer_material::MatchesShaders.
pub fn matches_shaders(vertex: &[u8], pixel: &[u8]) -> bool {
    vertex == VERTEX_SHADER_SHA256.as_bytes() && pixel == PIXEL_SHADER_SHA256.as_bytes()
}

/// detail::Word: a little-endian word.
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
/// detail::Scalar.
fn scalar(bytes: &[u8], offset: usize) -> f32 {
    f32::from_bits(word(bytes, offset))
}

/// gbuffer_material::Analyze.
pub fn analyze(input: &Inputs) -> Result<Analysis, Failure> {
    if input.material.len() != MATERIAL_BYTES {
        return Err(Failure::MaterialSize);
    }
    if input.modifiers.len() != MODIFIER_BYTES {
        return Err(Failure::ModifierSize);
    }
    if input.global_frame_slice.len() != GLOBAL_FRAME_BYTES {
        return Err(Failure::GlobalFrameSize);
    }
    if (0..MODIFIER_BYTES)
        .step_by(4)
        .any(|offset| !scalar(input.modifiers, offset).is_finite())
    {
        return Err(Failure::NonFiniteModifier);
    }
    const ALWAYS_FLOAT: [usize; 10] = [16, 20, 24, 48, 64, 96, 112, 144, 192, 288];
    let alpha = scalar(input.modifiers, 0) != 0.0;
    let optional = scalar(input.material, 192) > 0.0;
    let finite = |offset: &usize| scalar(input.material, *offset).is_finite();
    if !ALWAYS_FLOAT.iter().all(finite)
        || (alpha && !finite(&272))
        || (optional && ![176, 180, 184, 224].iter().all(finite))
    {
        return Err(Failure::NonFiniteMaterial);
    }

    let mut result = Analysis {
        texture_count: if optional { 5 } else { 4 },
        ..Default::default()
    };
    for i in 0..result.texture_count as usize {
        let index = word(input.material, SELECTOR_OFFSETS[i] as usize);
        if index >= DESCRIPTOR_COUNT {
            return Err(Failure::TextureIndex);
        }
        result.textures[i] = TextureUse {
            selector_offset: SELECTOR_OFFSETS[i],
            index,
        };
    }
    result.sampler = SamplerUse {
        shader_register: 0,
        register_space: 0,
        required: true,
    };
    result.dither = DitherUse {
        shader_register: 67,
        register_space: 0,
        mip: 0,
        slice: word(input.global_frame_slice, 0) & 63,
        coordinate_mask: 63,
        required: true,
    };
    result.alpha_test_enabled = alpha;
    result.optional_texture_enabled = optional;
    // Control values only: t67 still executes and remains a required dependency.
    result.neutral_dither = scalar(input.modifiers, 8) == 0.0 && scalar(input.modifiers, 12) == 0.0;
    Ok(result)
}

/// gbuffer_material::SameInputs: conservative byte equality, inactive material fields and the whole frame word included.
pub fn same_inputs(main: &Inputs, peer: &Inputs) -> bool {
    analyze(main).is_ok()
        && analyze(peer).is_ok()
        && main.material == peer.material
        && main.modifiers == peer.modifiers
        && main.global_frame_slice == peer.global_frame_slice
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// Lent bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawBytes {
    pub data: *const u8,
    pub size: usize,
}
/// Lent gbuffer_material::Inputs.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RawInputs {
    pub material: RawBytes,
    pub modifiers: RawBytes,
    pub global_frame_slice: RawBytes,
}
const _: () = assert!(core::mem::size_of::<RawInputs>() == 48);

unsafe fn bytes<'a>(raw: RawBytes) -> &'a [u8] {
    if raw.data.is_null() || raw.size == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(raw.data, raw.size) }
    }
}
unsafe fn inputs<'a>(raw: &RawInputs) -> Inputs<'a> {
    unsafe {
        Inputs {
            material: bytes(raw.material),
            modifiers: bytes(raw.modifiers),
            global_frame_slice: bytes(raw.global_frame_slice),
        }
    }
}

/// gbuffer_material::MatchesShaders.
///
/// # Safety
/// The strings are valid for their lengths.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gm_matches_shaders(
    vertex: *const u8,
    vertex_len: usize,
    pixel: *const u8,
    pixel_len: usize,
) -> bool {
    unsafe {
        matches_shaders(
            bytes(RawBytes {
                data: vertex,
                size: vertex_len,
            }),
            bytes(RawBytes {
                data: pixel,
                size: pixel_len,
            }),
        )
    }
}
/// gbuffer_material::Analyze (`output` zeroed on a failure).
///
/// # Safety
/// `input`'s bytes are valid for their sizes; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gm_analyze(
    input: *const RawInputs,
    output: *mut Analysis,
) -> u32 {
    unsafe {
        let result = analyze(&inputs(&*input));
        *output = result.unwrap_or_default();
        result.err().unwrap_or(Failure::None) as u32
    }
}
/// gbuffer_material::SameInputs.
///
/// # Safety
/// Both inputs' bytes are valid for their sizes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gm_same_inputs(
    main: *const RawInputs,
    peer: *const RawInputs,
) -> bool {
    unsafe { same_inputs(&inputs(&*main), &inputs(&*peer)) }
}
