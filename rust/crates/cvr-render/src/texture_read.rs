//! THE NATIVE TEXTURES' READ RULES (the pure part of include/Render/NativeTextureRead.hpp's detail namespace and
//! texture_read::ReadState): which native resource states allow a copy or a shader read, which texture shapes and views
//! the single-pass readers accept, the copyable footprints of a view's subresources, the fixed-asset record's owner, and
//! the engine's own per-list state of a native texture read from its state map. The header forwards to these in the
//! Rust build; tools/single_pass_tests/native_texture_state_test runs its cases against them (its Rust variant).
use core::ffi::c_void;
use windows::core::Interface as _;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// texture_read::NativeTexture.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NativeTexture {
    pub registry: u64,
    pub resource: u64,
    pub handle: u32,
    pub reserved: u32,
}
/// texture_read::State: a native state and how it was observed (1 the map's value, 2 a per-subresource value, 3 not
/// readable, 4 a pending batch, 5 absent, 6 the map unreadable, 7 the subresource table unreadable, 8 not tracked).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct State {
    pub value: u32,
    pub status: u32,
}
/// detail::Allocation.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Allocation {
    pub heap: u64,
    pub offset: u64,
    pub bytes: u64,
    pub kind: u32,
}
/// detail::MipFootprint.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MipFootprint {
    pub mip: u32,
    pub layout: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    pub rows: u32,
    pub row_bytes: u64,
    pub bytes: u64,
}
const _: () = assert!(
    core::mem::size_of::<NativeTexture>() == 24
        && core::mem::size_of::<State>() == 8
        && core::mem::size_of::<Allocation>() == 32
        && core::mem::size_of::<MipFootprint>() == 64
);

pub const SHADER_READS: u32 = (D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0
    | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0) as u32;
pub const ALLOWED_READS: u32 = SHADER_READS | D3D12_RESOURCE_STATE_COPY_SOURCE.0 as u32;

pub fn copy_read_mask(before: State) -> Option<u32> {
    if before.status != 1 && before.status != 2 && before.status != 5 {
        return None;
    }
    let previous = if before.status == 5 { 0 } else { before.value };
    if previous & !ALLOWED_READS != 0 {
        return None;
    }
    Some(previous | ALLOWED_READS)
}
pub fn has_read_state(state: State, required: u32) -> bool {
    (state.status == 1 || state.status == 2)
        && state.value & !ALLOWED_READS == 0
        && state.value & required == required
}
pub fn source_read_state(state: State, subresources: u32) -> bool {
    subresources != 0
        && (state.status == 1 || (state.status == 2 && subresources == 1))
        && state.value & SHADER_READS != 0
        && state.value & !ALLOWED_READS == 0
}
pub fn writable_state(state: State, expected: u32) -> bool {
    (expected == D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32
        || expected == D3D12_RESOURCE_STATE_DEPTH_WRITE.0 as u32)
        && state.status == 1
        && state.value == expected
}
pub fn known_allocation(a: &Allocation) -> bool {
    a.kind == 1
        || (a.kind == 2
            && a.heap != 0
            && a.bytes != 0
            && a.bytes != u64::MAX
            && a.offset <= u64::MAX - a.bytes)
}
pub fn could_alias(source: &Allocation, other: &Allocation) -> bool {
    if source.kind == 1 || other.kind == 1 {
        return false;
    }
    if source.heap != 0 && other.heap != 0 && source.heap != other.heap {
        return false;
    }
    if !known_allocation(source) || !known_allocation(other) {
        return true;
    }
    source.offset < other.offset.wrapping_add(other.bytes)
        && other.offset < source.offset.wrapping_add(source.bytes)
}
pub fn alias_affects(
    resource: u64,
    source: &Allocation,
    before: u64,
    from: &Allocation,
    after: u64,
    to: &Allocation,
) -> bool {
    if resource == before || resource == after {
        return true;
    }
    if before == 0 && after == 0 {
        return source.kind != 1;
    }
    (before != 0 && could_alias(source, from)) || (after != 0 && could_alias(source, to))
}
pub fn alpha_format(format: DXGI_FORMAT) -> bool {
    format == DXGI_FORMAT_BC1_UNORM_SRGB || format == DXGI_FORMAT_BC7_UNORM_SRGB
}
pub fn typed_format(format: DXGI_FORMAT) -> bool {
    format == DXGI_FORMAT_R8_UNORM
        || format == DXGI_FORMAT_R16_UNORM
        || format == DXGI_FORMAT_R16G16_UNORM
        || format == DXGI_FORMAT_R16G16B16A16_UNORM
        || format == DXGI_FORMAT_R11G11B10_FLOAT
        || format == DXGI_FORMAT_R32_UINT
}
pub fn immutable_shape(d: &D3D12_RESOURCE_DESC) -> bool {
    d.Dimension.0 >= D3D12_RESOURCE_DIMENSION_TEXTURE1D.0
        && d.Dimension.0 <= D3D12_RESOURCE_DIMENSION_TEXTURE3D.0
        && d.Width != 0
        && d.Height != 0
        && d.DepthOrArraySize != 0
        && d.MipLevels != 0
        && d.Format != DXGI_FORMAT_UNKNOWN
        && d.SampleDesc.Count == 1
        && d.SampleDesc.Quality == 0
        && d.Flags == D3D12_RESOURCE_FLAG_NONE
        && d.Layout == D3D12_TEXTURE_LAYOUT_UNKNOWN
}
pub fn shape(d: &D3D12_RESOURCE_DESC) -> bool {
    if d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.Width == 0
        || d.Width > 4096
        || d.Height == 0
        || d.Height > 4096
        || d.DepthOrArraySize == 0
        || d.DepthOrArraySize > 2048
        || d.SampleDesc.Count != 1
        || d.SampleDesc.Quality != 0
        || d.Flags != D3D12_RESOURCE_FLAG_NONE
        || d.Layout != D3D12_TEXTURE_LAYOUT_UNKNOWN
        || d.MipLevels == 0
    {
        return false;
    }
    if !typed_format(d.Format) && !alpha_format(d.Format) {
        return false;
    }
    if alpha_format(d.Format) && (d.Width % 4 != 0 || d.Height % 4 != 0) {
        return false;
    }
    let mut size = d.Width.max(d.Height as u64);
    let mut levels = 1u32;
    while size > 1 {
        size >>= 1;
        levels += 1;
    }
    (d.MipLevels as u32) <= levels
        && d.MipLevels as u32 * d.DepthOrArraySize as u32 <= D3D12_REQ_SUBRESOURCES
}
pub fn view_mips(
    d: &D3D12_RESOURCE_DESC,
    view: &D3D12_SHADER_RESOURCE_VIEW_DESC,
) -> Option<(u32, u32)> {
    // SAFETY: the union member of a TEXTURE2D view.
    let t = unsafe { view.Anonymous.Texture2D };
    if !shape(d)
        || d.DepthOrArraySize != 1
        || view.ViewDimension != D3D12_SRV_DIMENSION_TEXTURE2D
        || view.Format != d.Format
        || view.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
        || t.PlaneSlice != 0
        || t.MostDetailedMip >= d.MipLevels as u32
    {
        return None;
    }
    let remaining = d.MipLevels as u32 - t.MostDetailedMip;
    let visible = if t.MipLevels == u32::MAX {
        remaining
    } else {
        t.MipLevels
    };
    if visible == 0 || visible > remaining {
        return None;
    }
    Some((t.MostDetailedMip, visible))
}
pub fn view_subresources(
    d: &D3D12_RESOURCE_DESC,
    view: &D3D12_SHADER_RESOURCE_VIEW_DESC,
) -> Option<Vec<u32>> {
    if !shape(d)
        || view.Format != d.Format
        || view.Shader4ComponentMapping != D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING
    {
        return None;
    }
    let (first, count, slice, slices, clamp);
    if view.ViewDimension == D3D12_SRV_DIMENSION_TEXTURE2D {
        let (f, c) = view_mips(d, view)?;
        first = f;
        count = c;
        slice = 0;
        slices = 1;
        // SAFETY: the union member of a TEXTURE2D view.
        clamp = unsafe { view.Anonymous.Texture2D.ResourceMinLODClamp };
    } else if view.ViewDimension == D3D12_SRV_DIMENSION_TEXTURE2DARRAY {
        // SAFETY: the union member of a TEXTURE2DARRAY view.
        let a = unsafe { view.Anonymous.Texture2DArray };
        if a.PlaneSlice != 0
            || a.MostDetailedMip >= d.MipLevels as u32
            || a.ArraySize == 0
            || a.FirstArraySlice >= d.DepthOrArraySize as u32
            || a.ArraySize > d.DepthOrArraySize as u32 - a.FirstArraySlice
        {
            return None;
        }
        first = a.MostDetailedMip;
        let remaining = d.MipLevels as u32 - first;
        count = if a.MipLevels == u32::MAX {
            remaining
        } else {
            a.MipLevels
        };
        if count == 0 || count > remaining {
            return None;
        }
        slice = a.FirstArraySlice;
        slices = a.ArraySize;
        clamp = a.ResourceMinLODClamp;
    } else {
        return None;
    }
    // C++ compares the float clamp with the integer expression MipLevels-1 promoted to float.
    if !clamp.is_finite() || clamp < 0.0 || clamp > (d.MipLevels as i32 - 1) as f32 {
        return None;
    }
    let mut indices = Vec::with_capacity(count as usize * slices as usize);
    for layer in 0..slices {
        for mip in 0..count {
            indices.push(first + mip + (slice + layer) * d.MipLevels as u32);
        }
    }
    Some(indices)
}
pub fn footprint_range(
    layout: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    rows: u32,
    row_bytes: u64,
    destination_bytes: u64,
) -> Option<u64> {
    let f = &layout.Footprint;
    if layout.Offset % D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT as u64 != 0
        || rows == 0
        || row_bytes == 0
        || f.Depth != 1
        || f.RowPitch == 0
        || f.RowPitch % D3D12_TEXTURE_DATA_PITCH_ALIGNMENT != 0
        || row_bytes > f.RowPitch as u64
        || layout.Offset > destination_bytes
    {
        return None;
    }
    let before_last = f.RowPitch as u64 * (rows as u64 - 1);
    if before_last > u64::MAX - row_bytes
        || before_last + row_bytes > destination_bytes - layout.Offset
    {
        return None;
    }
    Some(before_last + row_bytes)
}
pub fn subresource_layout(
    device: &ID3D12Device,
    d: &D3D12_RESOURCE_DESC,
    subresource: u32,
    offset: u64,
) -> Option<MipFootprint> {
    if !shape(d)
        || subresource >= d.MipLevels as u32 * d.DepthOrArraySize as u32
        || offset % D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT as u64 != 0
    {
        return None;
    }
    let mut next = MipFootprint {
        mip: subresource,
        ..Default::default()
    };
    let mut total = 0u64;
    // SAFETY: one subresource's outputs.
    unsafe {
        device.GetCopyableFootprints(
            d,
            subresource,
            1,
            offset,
            Some(&mut next.layout),
            Some(&mut next.rows),
            Some(&mut next.row_bytes),
            Some(&mut total),
        )
    };
    if total == 0
        || total == u64::MAX
        || next.layout.Offset != offset
        || next.layout.Footprint.Format != d.Format
    {
        return None;
    }
    next.bytes = footprint_range(&next.layout, next.rows, next.row_bytes, u64::MAX)?;
    Some(next)
}
pub fn view_layouts(
    device: &ID3D12Device,
    d: &D3D12_RESOURCE_DESC,
    view: &D3D12_SHADER_RESOURCE_VIEW_DESC,
    mut offset: u64,
) -> Option<Vec<MipFootprint>> {
    if offset % D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT as u64 != 0 {
        return None;
    }
    let indices = view_subresources(d, view)?;
    let align = D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT as u64;
    let mut result = Vec::with_capacity(indices.len());
    for (index, &subresource) in indices.iter().enumerate() {
        let layout = subresource_layout(device, d, subresource, offset)?;
        let end = layout.layout.Offset + layout.bytes;
        result.push(layout);
        if index + 1 < indices.len() {
            if end > u64::MAX - (align - 1) {
                return None;
            }
            offset = (end + align - 1) & !(align - 1);
        }
    }
    Some(result)
}
pub fn fixed_owner(record: &[u8; 0x58]) -> u64 {
    let owner = u64::from_le_bytes(record[0x10..0x18].try_into().unwrap());
    let binding = u32::from_le_bytes(record[0x48..0x4C].try_into().unwrap());
    let flags = u16::from_le_bytes(record[0x56..0x58].try_into().unwrap());
    if record[8] == 1 && binding == SHADER_READS && (flags == 0x41 || flags == 0xc1) {
        owner
    } else {
        0
    }
}
/// texture_read::ReadMemory.
pub type ReadMemory = unsafe extern "C" fn(*mut c_void, u64, *mut c_void, usize) -> bool;
/// ReadState: the native texture's state on this list's context, from the context's state map (or its per-subresource
/// table). Only a terminated bucket chain proves absence; a bounded walk can also end because the table is cyclic,
/// corrupt or too long to verify.
pub fn read_state(
    source: &NativeTexture,
    context: u64,
    list: u64,
    read: Option<ReadMemory>,
    user: *mut c_void,
) -> State {
    let memory = |address: u64, out: *mut c_void, bytes: usize| {
        // SAFETY: the caller's reader writes at most `bytes` bytes into `out`.
        address != 0
            && address <= u64::MAX - bytes as u64
            && read.is_some_and(|r| unsafe { r(user, address, out, bytes) })
    };
    let value = |v: u32, s: u32| State {
        value: v,
        status: s,
    };
    if source.registry == 0
        || source.resource == 0
        || source.handle == 0
        || source.handle > 32768
        || context == 0
        || list == 0
        || context > u64::MAX - 0x630
    {
        return value(0, 3);
    }
    let offset = 0x2f1d8 + (source.handle as u64 - 1) * 0xb0;
    if source.registry > u64::MAX - offset - 0x58 {
        return value(0, 3);
    }
    let mut record = [0u8; 0x58];
    if !memory(
        source.registry + offset,
        record.as_mut_ptr().cast(),
        record.len(),
    ) {
        return value(0, 3);
    }
    let resource = u64::from_le_bytes(record[0..8].try_into().unwrap());
    let mut actual_list = 0u64;
    if resource != source.resource
        || !memory(context + 0x30, (&mut actual_list as *mut u64).cast(), 8)
        || actual_list != list
    {
        return value(0, 3);
    }
    if record[0x56] & 0x40 != 0 {
        return value(0, 8);
    }
    let mut pending = 0u32;
    if !memory(context + 0x534, (&mut pending as *mut u32).cast(), 4) {
        return value(0, 3);
    }
    if pending != 0 {
        return value(0, 4);
    }
    #[repr(C)]
    #[derive(Default)]
    struct Map {
        buckets: u64,
        count: u32,
        bucket_count: u32,
        data: u64,
        capacity: u32,
        stride: u32,
    }
    let mut map = Map::default();
    if !memory(context + 0x538, (&mut map as *mut Map).cast(), 32) {
        return value(0, 6);
    }
    if map.count > map.capacity
        || map.count > map.bucket_count
        || map.capacity > 1048576
        || map.bucket_count > 1048576
    {
        return value(0, 6);
    }
    let empty_storage =
        map.buckets == 0 && map.bucket_count == 0 && map.data == 0 && map.capacity == 0;
    if empty_storage {
        if map.stride != 0 && (map.stride < 20 || map.stride > 64 || map.stride % 4 != 0) {
            return value(0, 6);
        }
        return value(0, 5);
    }
    if map.bucket_count == 0
        || map.capacity == 0
        || map.stride < 20
        || map.stride > 64
        || map.stride % 4 != 0
        || map.buckets == 0
        || map.data == 0
    {
        return value(0, 6);
    }
    if map.count == 0 {
        return value(0, 5);
    }
    let bucket = (source.handle % map.bucket_count) as u64 * 4;
    if map.buckets > u64::MAX - bucket - 4 {
        return value(0, 6);
    }
    let mut at = 0u32;
    if !memory(map.buckets + bucket, (&mut at as *mut u32).cast(), 4) {
        return value(0, 6);
    }
    let mut visited = 0u32;
    while visited < map.count && visited < 64 && at != u32::MAX {
        if at >= map.capacity || map.data > u64::MAX - at as u64 * map.stride as u64 - 20 {
            return value(0, 6);
        }
        let mut entry = [0u32; 5];
        if !memory(
            map.data + at as u64 * map.stride as u64,
            entry.as_mut_ptr().cast(),
            20,
        ) {
            return value(0, 6);
        }
        if entry[1] == source.handle && entry[2] == source.handle {
            if entry[4] != 0x80000000 {
                return value(entry[4], 1);
            }
            let first = entry[3] & 65535;
            let count = entry[3] >> 16;
            if first == 65535 || count != 1 {
                return value(0, 7);
            }
            let (mut data, mut capacity, mut size, mut state) = (0u64, 0u32, 0u32, 0u32);
            if !memory(context + 0x598, (&mut data as *mut u64).cast(), 8)
                || data == 0
                || !memory(context + 0x5a0, (&mut capacity as *mut u32).cast(), 4)
                || !memory(context + 0x5a4, (&mut size as *mut u32).cast(), 4)
                || first >= size
                || size > capacity
                || size > 65535
                || data > u64::MAX - first as u64 * 4 - 4
                || !memory(data + first as u64 * 4, (&mut state as *mut u32).cast(), 4)
                || state == 0x80000000
            {
                return value(0, 7);
            }
            return value(state, 2);
        }
        at = entry[0];
        visited += 1;
    }
    value(0, if at == u32::MAX { 5 } else { 6 })
}

// ---- the C ABI (the header's inline functions in the Rust build)

#[no_mangle]
pub extern "C" fn cvr_tr_copy_read_mask(before: State, required: *mut u32) -> bool {
    let r = copy_read_mask(before);
    if !required.is_null() {
        // SAFETY: the caller's output.
        unsafe { *required = r.unwrap_or(0) };
    }
    r.is_some()
}
#[no_mangle]
pub extern "C" fn cvr_tr_has_read_state(state: State, required: u32) -> bool {
    has_read_state(state, required)
}
#[no_mangle]
pub extern "C" fn cvr_tr_source_read_state(state: State, subresources: u32) -> bool {
    source_read_state(state, subresources)
}
#[no_mangle]
pub extern "C" fn cvr_tr_writable_state(state: State, expected: u32) -> bool {
    writable_state(state, expected)
}
/// # Safety
/// `a` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_known_allocation(a: *const Allocation) -> bool {
    // SAFETY: as the caller promises.
    known_allocation(unsafe { &*a })
}
/// # Safety
/// The allocations are valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_could_alias(
    source: *const Allocation,
    other: *const Allocation,
) -> bool {
    // SAFETY: as the caller promises.
    could_alias(unsafe { &*source }, unsafe { &*other })
}
/// # Safety
/// The allocations are valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_alias_affects(
    resource: u64,
    source: *const Allocation,
    before: u64,
    from: *const Allocation,
    after: u64,
    to: *const Allocation,
) -> bool {
    // SAFETY: as the caller promises.
    alias_affects(
        resource,
        unsafe { &*source },
        before,
        unsafe { &*from },
        after,
        unsafe { &*to },
    )
}
#[no_mangle]
pub extern "C" fn cvr_tr_alpha_format(format: DXGI_FORMAT) -> bool {
    alpha_format(format)
}
#[no_mangle]
pub extern "C" fn cvr_tr_typed_format(format: DXGI_FORMAT) -> bool {
    typed_format(format)
}
/// # Safety
/// `desc` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_immutable_shape(desc: *const D3D12_RESOURCE_DESC) -> bool {
    // SAFETY: as the caller promises.
    immutable_shape(unsafe { &*desc })
}
/// # Safety
/// `desc` is valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_shape(desc: *const D3D12_RESOURCE_DESC) -> bool {
    // SAFETY: as the caller promises.
    shape(unsafe { &*desc })
}
/// # Safety
/// `desc` and `view` are valid; `first` and `count` receive the mips (0 on refusal).
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_view_mips(
    desc: *const D3D12_RESOURCE_DESC,
    view: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    first: *mut u32,
    count: *mut u32,
) -> bool {
    // SAFETY: as the caller promises.
    let r = view_mips(unsafe { &*desc }, unsafe { &*view });
    let (f, c) = r.unwrap_or((0, 0));
    unsafe {
        *first = f;
        *count = c;
    }
    r.is_some()
}
/// # Safety
/// `desc` and `view` are valid; `out` holds `capacity` values; `count` receives how many were written.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_view_subresources(
    desc: *const D3D12_RESOURCE_DESC,
    view: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    out: *mut u32,
    capacity: usize,
    count: *mut usize,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { *count = 0 };
    let Some(indices) = view_subresources(unsafe { &*desc }, unsafe { &*view }) else {
        return false;
    };
    if indices.len() > capacity {
        return false;
    }
    // SAFETY: `indices.len()` values fit in the caller's buffer.
    unsafe {
        core::ptr::copy_nonoverlapping(indices.as_ptr(), out, indices.len());
        *count = indices.len();
    }
    true
}
/// # Safety
/// `layout` is valid; `bytes` receives the range (0 on refusal).
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_footprint_range(
    layout: *const D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    rows: u32,
    row_bytes: u64,
    destination_bytes: u64,
    bytes: *mut u64,
) -> bool {
    // SAFETY: as the caller promises.
    let r = footprint_range(unsafe { &*layout }, rows, row_bytes, destination_bytes);
    unsafe { *bytes = r.unwrap_or(0) };
    r.is_some()
}
/// # Safety
/// `device` is a live device or null; `desc` valid; `out` receives the layout (all zero on refusal).
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_subresource_layout(
    device: *mut c_void,
    desc: *const D3D12_RESOURCE_DESC,
    subresource: u32,
    offset: u64,
    out: *mut MipFootprint,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { *out = MipFootprint::default() };
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return false;
    };
    match subresource_layout(device, unsafe { &*desc }, subresource, offset) {
        Some(layout) => {
            unsafe { *out = layout };
            true
        }
        None => false,
    }
}
/// # Safety
/// `device` is a live device or null; `desc` and `view` valid; `out` holds `capacity` layouts; `count` receives how many.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_view_layouts(
    device: *mut c_void,
    desc: *const D3D12_RESOURCE_DESC,
    view: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    offset: u64,
    out: *mut MipFootprint,
    capacity: usize,
    count: *mut usize,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { *count = 0 };
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return false;
    };
    let Some(layouts) = view_layouts(device, unsafe { &*desc }, unsafe { &*view }, offset) else {
        return false;
    };
    if layouts.len() > capacity {
        return false;
    }
    // SAFETY: `layouts.len()` values fit in the caller's buffer.
    unsafe {
        core::ptr::copy_nonoverlapping(layouts.as_ptr(), out, layouts.len());
        *count = layouts.len();
    }
    true
}
/// # Safety
/// `record` holds 0x58 bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_fixed_owner(record: *const [u8; 0x58]) -> u64 {
    // SAFETY: as the caller promises.
    fixed_owner(unsafe { &*record })
}
/// # Safety
/// `source` is valid; `read` reads this process's memory for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_tr_read_state(
    source: *const NativeTexture,
    context: u64,
    list: u64,
    read: Option<ReadMemory>,
    user: *mut c_void,
) -> State {
    // SAFETY: as the caller promises.
    read_state(unsafe { &*source }, context, list, read, user)
}
