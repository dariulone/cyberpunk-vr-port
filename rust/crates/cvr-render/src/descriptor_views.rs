//! THE SOURCE DESCRIPTOR VIEWS (include/Render/SourceDescriptorViews.hpp): the canonical decoders of the 20 recorded
//! words of a CBV / SRV / UAV / sampler creation (descriptors::Value) back into the D3D12 view description -- every
//! union member with its exact payload words, unused words zero, 32-bit spans, typed formats outside buffers, the
//! optional creation metadata (dimension, format, shape, flags, layout, samples, heap), implicit (null-description)
//! views, counters only on structured buffers, sampler filters, address modes, comparison and border colours with their
//! exact float bits; a rejected value leaves the output untouched. The header forwards to these in the Rust build;
//! tools/single_pass_tests/source_descriptor_views_test runs its cases against them (its Rust variant).
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// descriptors::Value.
pub type Value = [u64; 20];
const U32_MAX: u64 = u32::MAX as u64;

/// descriptor_views::ResourceMetadata.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ResourceMetadata {
    pub described: bool,
    pub heap_described: bool,
    pub dimension: D3D12_RESOURCE_DIMENSION,
    pub format: DXGI_FORMAT,
    pub width: u64,
    pub height: u32,
    pub depth_or_array_size: u16,
    pub mip_levels: u16,
    pub flags: D3D12_RESOURCE_FLAGS,
    pub layout: D3D12_TEXTURE_LAYOUT,
    pub samples: DXGI_SAMPLE_DESC,
    pub heap_type: D3D12_HEAP_TYPE,
    pub cpu_page: D3D12_CPU_PAGE_PROPERTY,
    pub memory_pool: D3D12_MEMORY_POOL,
    pub buffer_gpu_va: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CbvResult {
    pub view: D3D12_CONSTANT_BUFFER_VIEW_DESC,
    pub resource_cookie: u64,
    pub origin_id: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SrvResult {
    pub resource_cookie: u64,
    pub implicit: bool,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub metadata: ResourceMetadata,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct UavResult {
    pub resource_cookie: u64,
    pub counter_cookie: u64,
    pub implicit: bool,
    pub view: D3D12_UNORDERED_ACCESS_VIEW_DESC,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SamplerResult {
    pub view: D3D12_SAMPLER_DESC,
}
const _: () = assert!(
    core::mem::size_of::<ResourceMetadata>() == 72
        && core::mem::offset_of!(ResourceMetadata, buffer_gpu_va) == 64
        && core::mem::size_of::<CbvResult>() == 32
        && core::mem::size_of::<SrvResult>() == 128
        && core::mem::offset_of!(SrvResult, metadata) == 56
        && core::mem::size_of::<UavResult>() == 64
        && core::mem::size_of::<SamplerResult>() == 52
);

fn u32_word(value: u64) -> bool {
    value <= U32_MAX
}
fn float(value: u64) -> f32 {
    f32::from_bits(value as u32)
}
/// detail::Format: a DXGI format value of the recorded range.
pub fn format(value: u64) -> bool {
    value <= 115 || (130..=132).contains(&value) || (189..=191).contains(&value)
}
/// detail::TypedFormat: a known format, not UNKNOWN and not TYPELESS.
pub fn typed_format(value: u64) -> bool {
    if !format(value) || value == DXGI_FORMAT_UNKNOWN.0 as u64 {
        return false;
    }
    const TYPELESS: [DXGI_FORMAT; 22] = [
        DXGI_FORMAT_R32G32B32A32_TYPELESS,
        DXGI_FORMAT_R32G32B32_TYPELESS,
        DXGI_FORMAT_R16G16B16A16_TYPELESS,
        DXGI_FORMAT_R32G32_TYPELESS,
        DXGI_FORMAT_R32G8X24_TYPELESS,
        DXGI_FORMAT_R10G10B10A2_TYPELESS,
        DXGI_FORMAT_R8G8B8A8_TYPELESS,
        DXGI_FORMAT_R16G16_TYPELESS,
        DXGI_FORMAT_R32_TYPELESS,
        DXGI_FORMAT_R24G8_TYPELESS,
        DXGI_FORMAT_R8G8_TYPELESS,
        DXGI_FORMAT_R16_TYPELESS,
        DXGI_FORMAT_R8_TYPELESS,
        DXGI_FORMAT_BC1_TYPELESS,
        DXGI_FORMAT_BC2_TYPELESS,
        DXGI_FORMAT_BC3_TYPELESS,
        DXGI_FORMAT_BC4_TYPELESS,
        DXGI_FORMAT_BC5_TYPELESS,
        DXGI_FORMAT_B8G8R8A8_TYPELESS,
        DXGI_FORMAT_B8G8R8X8_TYPELESS,
        DXGI_FORMAT_BC6H_TYPELESS,
        DXGI_FORMAT_BC7_TYPELESS,
    ];
    !TYPELESS.iter().any(|f| f.0 as u64 == value)
}
/// detail::Words: only the used words may be nonzero.
fn words(value: &Value, used: u32) -> bool {
    value
        .iter()
        .enumerate()
        .all(|(i, &w)| used & (1u32 << i) != 0 || w == 0)
}
/// detail::Payload: the payload words 7..=last are 32-bit (except the wide ones), the rest up to 12 zero.
fn payload(value: &Value, last: usize, wide: u32) -> bool {
    (7..=last).all(|i| wide & (1u32 << i) != 0 || u32_word(value[i]))
        && (last + 1..=12).all(|i| value[i] == 0)
}
/// detail::Span32: a 32-bit first/count span (count -1: the remaining ones).
fn span32(first: u64, count: u64) -> bool {
    u32_word(first) && u32_word(count) && (count == U32_MAX || first + count <= U32_MAX + 1)
}
/// detail::Buffer: raw (R32_TYPELESS, no stride), structured (UNKNOWN with a stride) or typed.
fn buffer(value: &Value, flag_word: usize) -> bool {
    if value[flag_word] > 1 || value[7] > u64::MAX - value[8] {
        return false;
    }
    if value[flag_word] != 0 {
        return value[4] == DXGI_FORMAT_R32_TYPELESS.0 as u64 && value[9] == 0;
    }
    if value[9] != 0 {
        value[4] == DXGI_FORMAT_UNKNOWN.0 as u64
    } else {
        typed_format(value[4])
    }
}
/// detail::Mapping: a valid 4-component shader mapping (the always-set bit, components 0..=5).
fn mapping(value: u64) -> bool {
    u32_word(value)
        && value & 0x1000 != 0
        && value & !0x1FFF == 0
        && (0..4).all(|i| (value >> (i * 3)) & 7 <= 5)
}
/// detail::Metadata: the optional resource (and heap) description recorded with the view.
fn metadata(value: &Value) -> Option<ResourceMetadata> {
    let mut result = ResourceMetadata::default();
    if value[1] & !15 != 0
        || (value[1] & 1 != 0) != (value[2] != 0)
        || value[3] != 0
        || (value[1] & 12 != 0 && value[1] & 1 == 0)
        || (value[1] & 8 != 0 && value[1] & 4 == 0)
    {
        return None;
    }
    if value[1] & 4 == 0 {
        return (13..20).all(|i| value[i] == 0).then_some(result);
    }
    let dimension = value[13] as u32;
    let fmt = (value[13] >> 32) as u32;
    let layout = (value[16] >> 32) as u32;
    if dimension < D3D12_RESOURCE_DIMENSION_BUFFER.0 as u32
        || dimension > D3D12_RESOURCE_DIMENSION_TEXTURE3D.0 as u32
        || !format(fmt as u64)
        || value[16] as u32 & !0x1FF != 0
        || layout > D3D12_TEXTURE_LAYOUT_64KB_STANDARD_SWIZZLE.0 as u32
        || (dimension != D3D12_RESOURCE_DIMENSION_BUFFER.0 as u32 && value[19] != 0)
    {
        return None;
    }
    result.described = true;
    result.dimension = D3D12_RESOURCE_DIMENSION(dimension as i32);
    result.format = DXGI_FORMAT(fmt as i32);
    result.width = value[14];
    result.height = value[15] as u32;
    result.depth_or_array_size = (value[15] >> 32) as u16;
    result.mip_levels = (value[15] >> 48) as u16;
    result.flags = D3D12_RESOURCE_FLAGS(value[16] as u32 as i32);
    result.layout = D3D12_TEXTURE_LAYOUT(layout as i32);
    result.samples = DXGI_SAMPLE_DESC {
        Count: value[17] as u32,
        Quality: (value[17] >> 32) as u32,
    };
    result.buffer_gpu_va = value[19];
    if value[1] & 8 != 0 {
        let (kind, page, pool) = (
            value[18] as u16,
            (value[18] >> 16) as u16,
            (value[18] >> 32) as u16,
        );
        if value[18] >> 48 != 0 || !(1..=5).contains(&kind) || page > 4 || pool > 2 {
            return None;
        }
        result.heap_described = true;
        result.heap_type = D3D12_HEAP_TYPE(kind as i32);
        result.cpu_page = D3D12_CPU_PAGE_PROPERTY(page as i32);
        result.memory_pool = D3D12_MEMORY_POOL(pool as i32);
    } else if value[18] != 0 {
        return None;
    }
    Some(result)
}
fn filter(value: u64) -> bool {
    value & !0x1D5 == 0 && (value & 0x40 == 0 || value & 0x15 == 0x15)
}
/// detail::ComparisonWord: inside the SDK enum's representable range (0 included).
fn comparison_word(value: u64) -> bool {
    value <= 15
}

/// DecodeCbv: a described CBV (a 256-aligned location and size of at most 64 KiB) with its optional origin labels.
pub fn decode_cbv(value: &Value) -> Option<CbvResult> {
    if value[0] != 1
        || !words(value, (1 << 0) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5))
        || (value[2] != 0) != (value[3] != 0)
        || value[4] == 0
        || value[4] & 255 != 0
        || value[5] == 0
        || value[5] > 65536
        || value[5] & 255 != 0
        || value[4] > u64::MAX - value[5]
    {
        return None;
    }
    Some(CbvResult {
        view: D3D12_CONSTANT_BUFFER_VIEW_DESC {
            BufferLocation: value[4],
            SizeInBytes: value[5] as u32,
        },
        resource_cookie: value[2],
        origin_id: value[3],
    })
}
/// DecodeSrv: an implicit SRV of a resource, or a described one of a known dimension (an acceleration structure only
/// at the caller's certified address).
pub fn decode_srv(value: &Value, acceleration: Option<u64>) -> Option<SrvResult> {
    if value[0] != 2 {
        return None;
    }
    let mut result = SrvResult {
        resource_cookie: value[2],
        implicit: value[1] & 2 != 0,
        ..Default::default()
    };
    result.metadata = metadata(value)?;
    if result.implicit {
        return (result.resource_cookie != 0 && (4..=12).all(|i| value[i] == 0)).then_some(result);
    }
    if !format(value[4]) || !u32_word(value[5]) || !mapping(value[6]) {
        return None;
    }
    let view = &mut result.view;
    view.Format = DXGI_FORMAT(value[4] as i32);
    view.ViewDimension = D3D12_SRV_DIMENSION(value[5] as i32);
    view.Shader4ComponentMapping = value[6] as u32;
    let w = |i: usize| value[i] as u32;
    match view.ViewDimension {
        D3D12_SRV_DIMENSION_BUFFER => {
            if !payload(value, 10, 1 << 7) || !buffer(value, 10) {
                return None;
            }
            view.Anonymous.Buffer = D3D12_BUFFER_SRV {
                FirstElement: value[7],
                NumElements: w(8),
                StructureByteStride: w(9),
                Flags: D3D12_BUFFER_SRV_FLAGS(w(10) as i32),
            };
        }
        D3D12_SRV_DIMENSION_TEXTURE1D => {
            if !payload(value, 9, 0) || !span32(value[7], value[8]) {
                return None;
            }
            view.Anonymous.Texture1D = D3D12_TEX1D_SRV {
                MostDetailedMip: w(7),
                MipLevels: w(8),
                ResourceMinLODClamp: float(value[9]),
            };
        }
        D3D12_SRV_DIMENSION_TEXTURE1DARRAY => {
            if !payload(value, 11, 0) || !span32(value[7], value[8]) || !span32(value[9], value[10])
            {
                return None;
            }
            view.Anonymous.Texture1DArray = D3D12_TEX1D_ARRAY_SRV {
                MostDetailedMip: w(7),
                MipLevels: w(8),
                FirstArraySlice: w(9),
                ArraySize: w(10),
                ResourceMinLODClamp: float(value[11]),
            };
        }
        D3D12_SRV_DIMENSION_TEXTURE2D => {
            if !payload(value, 10, 0) || !span32(value[7], value[8]) {
                return None;
            }
            view.Anonymous.Texture2D = D3D12_TEX2D_SRV {
                MostDetailedMip: w(7),
                MipLevels: w(8),
                PlaneSlice: w(9),
                ResourceMinLODClamp: float(value[10]),
            };
        }
        D3D12_SRV_DIMENSION_TEXTURE2DARRAY => {
            if !payload(value, 12, 0) || !span32(value[7], value[8]) || !span32(value[9], value[10])
            {
                return None;
            }
            view.Anonymous.Texture2DArray = D3D12_TEX2D_ARRAY_SRV {
                MostDetailedMip: w(7),
                MipLevels: w(8),
                FirstArraySlice: w(9),
                ArraySize: w(10),
                PlaneSlice: w(11),
                ResourceMinLODClamp: float(value[12]),
            };
        }
        D3D12_SRV_DIMENSION_TEXTURE2DMS => {
            if !payload(value, 6, 0) {
                return None;
            }
        }
        D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY => {
            if !payload(value, 8, 0) || !span32(value[7], value[8]) {
                return None;
            }
            view.Anonymous.Texture2DMSArray = D3D12_TEX2DMS_ARRAY_SRV {
                FirstArraySlice: w(7),
                ArraySize: w(8),
            };
        }
        D3D12_SRV_DIMENSION_TEXTURE3D => {
            if !payload(value, 9, 0) || !span32(value[7], value[8]) {
                return None;
            }
            view.Anonymous.Texture3D = D3D12_TEX3D_SRV {
                MostDetailedMip: w(7),
                MipLevels: w(8),
                ResourceMinLODClamp: float(value[9]),
            };
        }
        D3D12_SRV_DIMENSION_TEXTURECUBE => {
            if !payload(value, 9, 0) || !span32(value[7], value[8]) {
                return None;
            }
            view.Anonymous.TextureCube = D3D12_TEXCUBE_SRV {
                MostDetailedMip: w(7),
                MipLevels: w(8),
                ResourceMinLODClamp: float(value[9]),
            };
        }
        D3D12_SRV_DIMENSION_TEXTURECUBEARRAY => {
            if !payload(value, 11, 0)
                || !span32(value[7], value[8])
                || (value[10] != U32_MAX && value[9] + value[10] * 6 > U32_MAX + 1)
            {
                return None;
            }
            view.Anonymous.TextureCubeArray = D3D12_TEXCUBE_ARRAY_SRV {
                MostDetailedMip: w(7),
                MipLevels: w(8),
                First2DArrayFace: w(9),
                NumCubes: w(10),
                ResourceMinLODClamp: float(value[11]),
            };
        }
        D3D12_SRV_DIMENSION_RAYTRACING_ACCELERATION_STRUCTURE => {
            if !payload(value, 7, 1 << 7)
                || result.resource_cookie != 0
                || value[1] != 0
                || value[4] != DXGI_FORMAT_UNKNOWN.0 as u64
                || value[7] == 0
                || value[7] % D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BYTE_ALIGNMENT as u64 != 0
                || acceleration != Some(value[7])
            {
                return None;
            }
            view.Anonymous.RaytracingAccelerationStructure =
                D3D12_RAYTRACING_ACCELERATION_STRUCTURE_SRV { Location: value[7] };
            return Some(result);
        }
        _ => return None,
    }
    if view.ViewDimension != D3D12_SRV_DIMENSION_BUFFER && !typed_format(value[4]) {
        return None;
    }
    Some(result)
}
/// DecodeUav: an implicit UAV, or a described one (a counter only on a structured buffer).
pub fn decode_uav(value: &Value) -> Option<UavResult> {
    if value[0] != 3
        || value[1] & !3 != 0
        || (value[1] & 1 != 0) != (value[2] != 0)
        || value[6] != 0
    {
        return None;
    }
    if (12..20).any(|i| value[i] != 0) {
        return None;
    }
    let mut result = UavResult {
        resource_cookie: value[2],
        counter_cookie: value[3],
        implicit: value[1] & 2 != 0,
        ..Default::default()
    };
    if result.implicit {
        return (result.resource_cookie != 0
            && result.counter_cookie == 0
            && (4..=11).all(|i| value[i] == 0))
        .then_some(result);
    }
    if !format(value[4]) || !u32_word(value[5]) {
        return None;
    }
    let view = &mut result.view;
    view.Format = DXGI_FORMAT(value[4] as i32);
    view.ViewDimension = D3D12_UAV_DIMENSION(value[5] as i32);
    if result.counter_cookie != 0
        && (result.resource_cookie == 0 || view.ViewDimension != D3D12_UAV_DIMENSION_BUFFER)
    {
        return None;
    }
    let w = |i: usize| value[i] as u32;
    match view.ViewDimension {
        D3D12_UAV_DIMENSION_BUFFER => {
            if !payload(value, 11, (1 << 7) | (1 << 10)) || !buffer(value, 11) {
                return None;
            }
            if result.counter_cookie != 0 {
                if value[9] == 0
                    || value[4] != DXGI_FORMAT_UNKNOWN.0 as u64
                    || value[11] != 0
                    || value[10] % D3D12_UAV_COUNTER_PLACEMENT_ALIGNMENT as u64 != 0
                    || value[10] > u64::MAX - 4
                {
                    return None;
                }
            } else if value[10] != 0 {
                return None;
            }
            view.Anonymous.Buffer = D3D12_BUFFER_UAV {
                FirstElement: value[7],
                NumElements: w(8),
                StructureByteStride: w(9),
                CounterOffsetInBytes: value[10],
                Flags: D3D12_BUFFER_UAV_FLAGS(w(11) as i32),
            };
        }
        D3D12_UAV_DIMENSION_TEXTURE1D => {
            if !payload(value, 7, 0) {
                return None;
            }
            view.Anonymous.Texture1D = D3D12_TEX1D_UAV { MipSlice: w(7) };
        }
        D3D12_UAV_DIMENSION_TEXTURE1DARRAY => {
            if !payload(value, 9, 0) || !span32(value[8], value[9]) {
                return None;
            }
            view.Anonymous.Texture1DArray = D3D12_TEX1D_ARRAY_UAV {
                MipSlice: w(7),
                FirstArraySlice: w(8),
                ArraySize: w(9),
            };
        }
        D3D12_UAV_DIMENSION_TEXTURE2D => {
            if !payload(value, 8, 0) {
                return None;
            }
            view.Anonymous.Texture2D = D3D12_TEX2D_UAV {
                MipSlice: w(7),
                PlaneSlice: w(8),
            };
        }
        D3D12_UAV_DIMENSION_TEXTURE2DARRAY => {
            if !payload(value, 10, 0) || !span32(value[8], value[9]) {
                return None;
            }
            view.Anonymous.Texture2DArray = D3D12_TEX2D_ARRAY_UAV {
                MipSlice: w(7),
                FirstArraySlice: w(8),
                ArraySize: w(9),
                PlaneSlice: w(10),
            };
        }
        D3D12_UAV_DIMENSION_TEXTURE3D => {
            if !payload(value, 9, 0) || !span32(value[8], value[9]) {
                return None;
            }
            view.Anonymous.Texture3D = D3D12_TEX3D_UAV {
                MipSlice: w(7),
                FirstWSlice: w(8),
                WSize: w(9),
            };
        }
        _ => return None,
    }
    if view.ViewDimension != D3D12_UAV_DIMENSION_BUFFER && !typed_format(value[4]) {
        return None;
    }
    Some(result)
}
/// DecodeSampler: a sampler with a valid filter, address modes, anisotropy, comparison, ordered LOD range and (for
/// border addressing) a border colour inside [0, 1].
pub fn decode_sampler(value: &Value) -> Option<SamplerResult> {
    if value[0] != 4 || !words(value, 1 | (((1u32 << 17) - 1) & !15)) || !filter(value[4]) {
        return None;
    }
    if !(4..=16).all(|i| {
        if i == 10 {
            comparison_word(value[i])
        } else {
            u32_word(value[i])
        }
    }) {
        return None;
    }
    let (wrap, mirror_once) = (
        D3D12_TEXTURE_ADDRESS_MODE_WRAP.0 as u64,
        D3D12_TEXTURE_ADDRESS_MODE_MIRROR_ONCE.0 as u64,
    );
    if (5..=7).any(|i| value[i] < wrap || value[i] > mirror_once) {
        return None;
    }
    if value[4] & 0x40 != 0 && (value[9] == 0 || value[9] > 16) {
        return None;
    }
    if value[4] & 0x180 == 0x80
        && (value[10] < D3D12_COMPARISON_FUNC_NEVER.0 as u64
            || value[10] > D3D12_COMPARISON_FUNC_ALWAYS.0 as u64)
    {
        return None;
    }
    let view = D3D12_SAMPLER_DESC {
        Filter: D3D12_FILTER(value[4] as i32),
        AddressU: D3D12_TEXTURE_ADDRESS_MODE(value[5] as i32),
        AddressV: D3D12_TEXTURE_ADDRESS_MODE(value[6] as i32),
        AddressW: D3D12_TEXTURE_ADDRESS_MODE(value[7] as i32),
        MipLODBias: float(value[8]),
        MaxAnisotropy: value[9] as u32,
        ComparisonFunc: D3D12_COMPARISON_FUNC(value[10] as i32),
        BorderColor: [
            float(value[11]),
            float(value[12]),
            float(value[13]),
            float(value[14]),
        ],
        MinLOD: float(value[15]),
        MaxLOD: float(value[16]),
    };
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(view.MaxLOD >= view.MinLOD) {
        return None;
    }
    let border = D3D12_TEXTURE_ADDRESS_MODE_BORDER;
    if (view.AddressU == border || view.AddressV == border || view.AddressW == border)
        && !view.BorderColor.iter().all(|&c| (0.0..=1.0).contains(&c))
    {
        return None;
    }
    Some(SamplerResult { view })
}

// ---- the C ABI (the header's decoders in the Rust build; a refusal leaves the output untouched)

/// # Safety
/// `value` and `out` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_dv_decode_cbv(value: *const Value, out: *mut CbvResult) -> bool {
    // SAFETY: as the caller promises.
    match decode_cbv(unsafe { &*value }) {
        Some(r) => {
            unsafe { *out = r };
            true
        }
        None => false,
    }
}
/// # Safety
/// `value` and `out` valid; `acceleration` the caller's certified address or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_dv_decode_srv(
    value: *const Value,
    out: *mut SrvResult,
    acceleration: *const u64,
) -> bool {
    // SAFETY: as the caller promises.
    match decode_srv(
        unsafe { &*value },
        unsafe { acceleration.as_ref() }.copied(),
    ) {
        Some(r) => {
            unsafe { *out = r };
            true
        }
        None => false,
    }
}
/// # Safety
/// `value` and `out` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_dv_decode_uav(value: *const Value, out: *mut UavResult) -> bool {
    // SAFETY: as the caller promises.
    match decode_uav(unsafe { &*value }) {
        Some(r) => {
            unsafe { *out = r };
            true
        }
        None => false,
    }
}
/// # Safety
/// `value` and `out` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_dv_decode_sampler(
    value: *const Value,
    out: *mut SamplerResult,
) -> bool {
    // SAFETY: as the caller promises.
    match decode_sampler(unsafe { &*value }) {
        Some(r) => {
            unsafe { *out = r };
            true
        }
        None => false,
    }
}
