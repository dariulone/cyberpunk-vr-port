//! THE SKINNED DEPTH DRAWS' READ PLAN (include/Render/SkinningReadPlan.hpp): the native skinned depth contracts -- 1
//! (depth) and 2 (alpha depth: also its UV / normal / tangent inputs) by the exact vertex shader hash and every
//! expected input element (name, index, slot, offset, format, per-vertex / per-instance class and step rate, each
//! present once); the constant words both contracts read (a zero cull count bypasses b8, the modifier at byte 24 still
//! selects pre / post-cull placement) and their equality between draws; and the merged bone ranges every instance's
//! {base, stride} reaches inside the raw bone buffer view (base + index*stride + {0, 16, 32} for 255 bones). The
//! header forwards to these in the Rust build; tools/single_pass_tests/skinning_read_plan_test runs its cases against
//! them.
use core::ffi::{c_char, CStr};

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

pub const DEPTH_SHADER: &str = "906dcb2a6f034360c3b1d00067dbf12b9f6ee79a9789e73fae258688dee00d0f";
pub const ALPHA_SHADER: &str = "02f81c75e38964877d832f7378a39251132ab93580dfe89017e65d01fc22bc4c";

struct Input {
    name: &'static [u8],
    index: u32,
    slot: u32,
    offset: u32,
    format: DXGI_FORMAT,
    instance: bool,
    alpha_only: bool,
}
const fn input(
    name: &'static [u8],
    index: u32,
    slot: u32,
    offset: u32,
    format: DXGI_FORMAT,
    instance: bool,
    alpha_only: bool,
) -> Input {
    Input {
        name,
        index,
        slot,
        offset,
        format,
        instance,
        alpha_only,
    }
}
const INPUTS: [Input; 13] = [
    input(
        b"POSITION",
        0,
        0,
        0,
        DXGI_FORMAT_R16G16B16A16_SNORM,
        false,
        false,
    ),
    input(
        b"BLENDINDICES",
        0,
        0,
        8,
        DXGI_FORMAT_R8G8B8A8_UINT,
        false,
        false,
    ),
    input(
        b"BLENDINDICES",
        1,
        0,
        12,
        DXGI_FORMAT_R8G8B8A8_UINT,
        false,
        false,
    ),
    input(
        b"BLENDWEIGHT",
        0,
        0,
        16,
        DXGI_FORMAT_R8G8B8A8_UNORM,
        false,
        false,
    ),
    input(
        b"BLENDWEIGHT",
        1,
        0,
        20,
        DXGI_FORMAT_R8G8B8A8_UNORM,
        false,
        false,
    ),
    input(
        b"EXTRA_DATA",
        0,
        0,
        24,
        DXGI_FORMAT_R16G16B16A16_FLOAT,
        false,
        false,
    ),
    input(
        b"INSTANCE_TRANSFORM",
        0,
        7,
        0,
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        true,
        false,
    ),
    input(
        b"INSTANCE_TRANSFORM",
        1,
        7,
        16,
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        true,
        false,
    ),
    input(
        b"INSTANCE_TRANSFORM",
        2,
        7,
        32,
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        true,
        false,
    ),
    input(
        b"INSTANCE_SKINNING_DATA",
        0,
        7,
        48,
        DXGI_FORMAT_R32G32B32A32_UINT,
        true,
        false,
    ),
    input(b"TEXCOORD", 0, 1, 0, DXGI_FORMAT_R16G16_FLOAT, false, true),
    input(
        b"NORMAL",
        0,
        2,
        0,
        DXGI_FORMAT_R10G10B10A2_UNORM,
        false,
        true,
    ),
    input(
        b"TANGENT",
        0,
        2,
        4,
        DXGI_FORMAT_R10G10B10A2_UNORM,
        false,
        true,
    ),
];

/// DepthContract: 1 depth, 2 alpha depth, 0 neither.
///
/// # Safety
/// The layout's element array holds its count; every non-null semantic name is a C string.
pub unsafe fn depth_contract(shader: &[u8], layout: &D3D12_INPUT_LAYOUT_DESC) -> u32 {
    let contract = if shader == DEPTH_SHADER.as_bytes() {
        1
    } else if shader == ALPHA_SHADER.as_bytes() {
        2
    } else {
        0
    };
    if contract == 0
        || layout.pInputElementDescs.is_null()
        || layout.NumElements == 0
        || layout.NumElements > 32
    {
        return 0;
    }
    let elements = unsafe {
        core::slice::from_raw_parts(layout.pInputElementDescs, layout.NumElements as usize)
    };
    for expected in &INPUTS {
        if expected.alpha_only && contract != 2 {
            continue;
        }
        let mut found = 0;
        for actual in elements {
            if actual.SemanticName.is_null() {
                return 0;
            }
            let name = unsafe { CStr::from_ptr(actual.SemanticName.0 as *const c_char) }.to_bytes();
            if expected.name != name || expected.index != actual.SemanticIndex {
                continue;
            }
            found += 1;
            let class = if expected.instance {
                D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA
            } else {
                D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA
            };
            if found != 1
                || actual.InputSlot != expected.slot
                || actual.AlignedByteOffset != expected.offset
                || actual.Format != expected.format
                || actual.InputSlotClass != class
                || actual.InstanceDataStepRate != expected.instance as u32
            {
                return 0;
            }
        }
        if found != 1 {
            return 0;
        }
    }
    contract
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
/// ZeroCull.
pub fn zero_cull(modifiers: &[u8; 448]) -> bool {
    word(modifiers, 16) & 0x7fff_ffff == 0
}
/// SameMaterial.
pub fn same_material(first: &[u8; 112], second: &[u8; 112]) -> bool {
    [64usize, 68, 72, 80, 84, 88]
        .iter()
        .all(|&o| first[o..o + 4] == second[o..o + 4])
}
/// SameModifiers.
pub fn same_modifiers(first: &[u8; 448], second: &[u8; 448]) -> bool {
    zero_cull(first)
        && zero_cull(second)
        && [16usize, 24, 32]
            .iter()
            .all(|&o| first[o..o + 4] == second[o..o + 4])
}

/// skinning::BoneRange.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct BoneRange {
    pub offset: u64,
    pub bytes: u64,
}

/// BoneRanges: the merged ranges of the view's raw buffer the instances' bones reach.
pub fn bone_ranges(
    contract: u32,
    instances: &[u8],
    view: &D3D12_SHADER_RESOURCE_VIEW_DESC,
    resource_bytes: u64,
) -> Option<Vec<BoneRange>> {
    if (contract != 1 && contract != 2)
        || instances.is_empty()
        || instances.len() % 64 != 0
        || instances.len() > 64 * 65536
        || view.ViewDimension != D3D12_SRV_DIMENSION_BUFFER
        || view.Format != DXGI_FORMAT_R32_TYPELESS
        || view.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
    {
        return None;
    }
    let buffer = unsafe { view.Anonymous.Buffer };
    if buffer.Flags != D3D12_BUFFER_SRV_FLAG_RAW
        || buffer.StructureByteStride != 0
        || buffer.NumElements == 0
        || buffer.FirstElement > u64::MAX / 4
        || buffer.FirstElement % 4 != 0
    {
        return None;
    }
    let begin = buffer.FirstElement * 4;
    let bytes = buffer.NumElements as u64 * 4;
    if begin > resource_bytes || bytes > resource_bytes - begin {
        return None;
    }
    let mut ranges = Vec::with_capacity(instances.len() / 64);
    for record in instances.chunks_exact(64) {
        let (base, stride) = (word(record, 48) as u64, word(record, 52) as u64);
        let size = stride * 255 + 48;
        if base % 4 != 0
            || stride % 4 != 0
            || base + size > u32::MAX as u64 + 1
            || base > bytes
            || size > bytes - base
        {
            return None;
        }
        ranges.push(BoneRange {
            offset: begin + base,
            bytes: size,
        });
    }
    ranges.sort_by_key(|range| range.offset);
    let mut output: Vec<BoneRange> = Vec::new();
    for range in ranges {
        match output.last_mut() {
            Some(last) if range.offset <= last.offset + last.bytes => {
                last.bytes = last.bytes.max(range.offset + range.bytes - last.offset);
            }
            _ => output.push(range),
        }
    }
    Some(output)
}

// ---- the C ABI ------------------------------------------------------------------------------------------------------

/// DepthContract.
///
/// # Safety
/// `shader` holds `length` bytes; `layout` is valid with its elements.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sk_depth_contract(
    shader: *const u8,
    length: usize,
    layout: *const D3D12_INPUT_LAYOUT_DESC,
) -> u32 {
    unsafe {
        let shader = if length == 0 || shader.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(shader, length)
        };
        depth_contract(shader, &*layout)
    }
}

/// ZeroCull (`which` 0, `a`), SameMaterial (1, 112-byte `a`, `b`) or SameModifiers (2, 448-byte `a`, `b`).
///
/// # Safety
/// `a` (and `b`) hold the sizes of `which`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sk_constants(which: u32, a: *const u8, b: *const u8) -> bool {
    unsafe {
        match which {
            0 => zero_cull(&*(a as *const [u8; 448])),
            1 => same_material(&*(a as *const [u8; 112]), &*(b as *const [u8; 112])),
            _ => same_modifiers(&*(a as *const [u8; 448]), &*(b as *const [u8; 448])),
        }
    }
}

/// BoneRanges: the ranges (read them with cvr_sk_ranges, free them with cvr_sk_ranges_free) or null.
///
/// # Safety
/// `instances` holds `bytes`; `view` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sk_bone_ranges(
    contract: u32,
    instances: *const u8,
    bytes: usize,
    view: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    resource_bytes: u64,
) -> *mut Vec<BoneRange> {
    unsafe {
        let instances = if bytes == 0 || instances.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(instances, bytes)
        };
        match bone_ranges(contract, instances, &*view, resource_bytes) {
            Some(ranges) => Box::into_raw(Box::new(ranges)),
            None => core::ptr::null_mut(),
        }
    }
}

/// The ranges of cvr_sk_bone_ranges.
///
/// # Safety
/// `ranges` comes from cvr_sk_bone_ranges; `count` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sk_ranges(
    ranges: *const Vec<BoneRange>,
    count: *mut usize,
) -> *const BoneRange {
    unsafe {
        *count = (*ranges).len();
        (*ranges).as_ptr()
    }
}

/// # Safety
/// `ranges` comes from cvr_sk_bone_ranges (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sk_ranges_free(ranges: *mut Vec<BoneRange>) {
    if !ranges.is_null() {
        drop(unsafe { Box::from_raw(ranges) });
    }
}

// ---- the alpha depth contract (include/Render/AlphaDepthContract.hpp) ----------------------------------------------

/// alpha_depth::VertexShader / PixelShader.
pub const ALPHA_VERTEX_SHADER: &str =
    "7fa0e076ff234929afda3679349d06353d4936245785b547b339432e3b1fd9ae";
pub const ALPHA_PIXEL_SHADER: &str =
    "dc69ea3ee2d8d906ebcbba4bd8f8d750add2b80946b5de9c9d94360cb18339e5";
const ALPHA_INPUTS: [Input; 5] = [
    input(
        b"POSITION",
        0,
        0,
        0,
        DXGI_FORMAT_R16G16B16A16_SNORM,
        false,
        false,
    ),
    input(b"TEXCOORD", 0, 1, 0, DXGI_FORMAT_R16G16_FLOAT, false, false),
    input(
        b"INSTANCE_TRANSFORM",
        0,
        7,
        0,
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        true,
        false,
    ),
    input(
        b"INSTANCE_TRANSFORM",
        1,
        7,
        16,
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        true,
        false,
    ),
    input(
        b"INSTANCE_TRANSFORM",
        2,
        7,
        32,
        DXGI_FORMAT_R32G32B32A32_FLOAT,
        true,
        false,
    ),
];

/// alpha_depth::Contract: 1 the exact alpha shader pair and its consumed IA inputs, 2 the skinned alpha VS contract
/// with the same alpha PS, 0 neither. Unused native NORMAL / TANGENT / COLOR / TEXCOORD1 attributes are no read proof.
///
/// # Safety
/// The layout's element array holds its count; every non-null semantic name is a C string.
pub unsafe fn alpha_contract(vertex: &[u8], pixel: &[u8], layout: &D3D12_INPUT_LAYOUT_DESC) -> u32 {
    if pixel != ALPHA_PIXEL_SHADER.as_bytes() {
        return 0;
    }
    if vertex == ALPHA_SHADER.as_bytes() {
        return if unsafe { depth_contract(vertex, layout) } == 2 {
            2
        } else {
            0
        };
    }
    if vertex != ALPHA_VERTEX_SHADER.as_bytes()
        || layout.pInputElementDescs.is_null()
        || layout.NumElements == 0
        || layout.NumElements > 32
    {
        return 0;
    }
    let elements = unsafe {
        core::slice::from_raw_parts(layout.pInputElementDescs, layout.NumElements as usize)
    };
    for expected in &ALPHA_INPUTS {
        let mut found = 0;
        for actual in elements {
            if actual.SemanticName.is_null() {
                return 0;
            }
            let name = unsafe { CStr::from_ptr(actual.SemanticName.0 as *const c_char) }.to_bytes();
            if expected.name != name || expected.index != actual.SemanticIndex {
                continue;
            }
            found += 1;
            let class = if expected.instance {
                D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA
            } else {
                D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA
            };
            if found != 1
                || actual.InputSlot != expected.slot
                || actual.AlignedByteOffset != expected.offset
                || actual.Format != expected.format
                || actual.InputSlotClass != class
                || actual.InstanceDataStepRate != expected.instance as u32
            {
                return 0;
            }
        }
        if found != 1 {
            return 0;
        }
    }
    1
}

/// alpha_depth::Contract.
///
/// # Safety
/// The shader strings hold their lengths; `layout` is valid with its elements.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sk_alpha_contract(
    vertex: *const u8,
    vertex_length: usize,
    pixel: *const u8,
    pixel_length: usize,
    layout: *const D3D12_INPUT_LAYOUT_DESC,
) -> u32 {
    unsafe {
        let text = |p: *const u8, n: usize| {
            if n == 0 || p.is_null() {
                &[][..]
            } else {
                core::slice::from_raw_parts(p, n)
            }
        };
        alpha_contract(
            text(vertex, vertex_length),
            text(pixel, pixel_length),
            &*layout,
        )
    }
}
