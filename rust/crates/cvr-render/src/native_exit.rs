//! The prepared pair's native exit comparison (`include/Render/NativePreparedPair.hpp`, `prepared_pair::
//! native_exit_detail`): the five exit planes (three colors, depth and stencil) and their index; the readback buffer's
//! shape (at most 512 MiB, row-major, no flags); a plane's bounds inside its readback (the eye's extent up to 4096, the
//! component and stride per plane, aligned placement and pitch); the plane's agreement with its expected copy footprint;
//! the complete plane set; the per-pixel comparison of the reference and optimized planes against the native one (the
//! first differing pixel and its values, and the counts); the statistics' accumulation; the VRS rate values and the
//! always-finer rule; the rate map's shape; the attribution of color differences inside or outside the refined tiles;
//! and the acceptance row (native rate evidence, attribution, unexplained differences saturating, pair differences,
//! strict or approved refinement).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the detail functions forward to `cvr_nx_*`; the plane and comparison records are
//! mirrored here. `tools/single_pass_tests/native_exit_compare_test` runs its cases against them in its Rust variant.

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

pub const MAX_READBACK_BYTES: u64 = 512 * 1024 * 1024;
const PLACEMENT_ALIGNMENT: u64 = 512;
const PITCH_ALIGNMENT: u32 = 256;

/// prepared_pair::NativeExitPlane.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct NativeExitPlane {
    pub slot: u32,
    pub plane: u32,
    pub rows: u32,
    pub stride: u32,
    pub component: u32,
    pub bytes: u32,
    pub layout: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
}
/// native_exit_detail::Difference.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Difference {
    pub count: u64,
    pub value: u64,
    pub native_value: u64,
    pub x: u32,
    pub y: u32,
}
impl Default for Difference {
    fn default() -> Self {
        Difference { count: 0, value: 0, native_value: 0, x: u32::MAX, y: u32::MAX }
    }
}
/// native_exit_detail::Comparison.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Comparison {
    pub reference: Difference,
    pub optimized: Difference,
}
/// A plane and its readback bytes, borrowed for the call.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlaneData {
    pub row: *const NativeExitPlane,
    pub data: *const u8,
    pub bytes: usize,
}
const _: () = assert!(
    core::mem::size_of::<NativeExitPlane>() == 56
        && core::mem::offset_of!(NativeExitPlane, layout) == 24
        && core::mem::size_of::<Difference>() == 32
        && core::mem::size_of::<Comparison>() == 64
        && core::mem::size_of::<PlaneData>() == 24
);

/// PlaneIndex: colors 0..2, depth 3, stencil 4.
pub fn plane_index(slot: u32, plane: u32) -> u32 {
    if slot < 3 && plane == 0 {
        slot
    } else if slot == 4 && plane < 2 {
        3 + plane
    } else {
        u32::MAX
    }
}
/// ReadbackShape.
pub fn readback_shape(desc: &D3D12_RESOURCE_DESC) -> bool {
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && desc.Width != 0
        && desc.Width <= MAX_READBACK_BYTES
        && desc.Height == 1
        && desc.DepthOrArraySize == 1
        && desc.MipLevels == 1
        && desc.Format == DXGI_FORMAT_UNKNOWN
        && desc.SampleDesc.Count == 1
        && desc.SampleDesc.Quality == 0
        && desc.Layout == D3D12_TEXTURE_LAYOUT_ROW_MAJOR
        && desc.Flags == D3D12_RESOURCE_FLAG_NONE
}
/// PlaneBounds: the end of the plane's payload inside the readback.
pub fn plane_bounds(row: &NativeExitPlane, width: u32, height: u32, buffer_bytes: u64) -> Option<u64> {
    if width == 0
        || width > 4096
        || height == 0
        || height > 4096
        || buffer_bytes == 0
        || buffer_bytes > MAX_READBACK_BYTES
        || plane_index(row.slot, row.plane) == u32::MAX
        || row.rows != height
    {
        return None;
    }
    let footprint = &row.layout.Footprint;
    let stencil = row.slot == 4 && row.plane != 0;
    let stride_ok = if row.slot < 3 { row.stride == 4 } else { row.stride == 4 || row.stride == 8 || (stencil && row.stride == 1) };
    if row.bytes != if stencil { 1 } else { 4 }
        || row.component != if stencil && row.stride == 8 { 4 } else { 0 }
        || !stride_ok
        || footprint.Format == DXGI_FORMAT_UNKNOWN
        || footprint.Width != width
        || footprint.Height != height
        || footprint.Depth != 1
        || row.layout.Offset % PLACEMENT_ALIGNMENT != 0
        || footprint.RowPitch == 0
        || footprint.RowPitch % PITCH_ALIGNMENT != 0
        || row.layout.Offset > buffer_bytes
    {
        return None;
    }
    let row_bytes = width as u64 * row.stride as u64;
    if row_bytes > footprint.RowPitch as u64 || row.component + row.bytes > row.stride {
        return None;
    }
    let payload = (row.rows - 1) as u64 * footprint.RowPitch as u64 + row_bytes;
    if payload > buffer_bytes - row.layout.Offset {
        return None;
    }
    Some(row.layout.Offset + payload)
}
/// ValidatePlane.
pub fn validate_plane(
    row: &NativeExitPlane,
    width: u32,
    height: u32,
    buffer_bytes: u64,
    expected: &D3D12_SUBRESOURCE_FOOTPRINT,
    expected_rows: u32,
    expected_row_bytes: u64,
) -> bool {
    let actual = &row.layout.Footprint;
    plane_bounds(row, width, height, buffer_bytes).is_some()
        && expected_rows == row.rows
        && expected_row_bytes == width as u64 * row.stride as u64
        && actual.Format == expected.Format
        && actual.Width == expected.Width
        && actual.Height == expected.Height
        && actual.Depth == expected.Depth
        && actual.RowPitch == expected.RowPitch
}
/// CompletePlanes: exactly the five planes, once each.
pub fn complete_planes(planes: &[NativeExitPlane]) -> bool {
    if planes.len() != 5 {
        return false;
    }
    let mut mask = 0u32;
    for row in planes {
        let index = plane_index(row.slot, row.plane);
        if index == u32::MAX || mask & (1 << index) != 0 {
            return false;
        }
        mask |= 1 << index;
    }
    mask == 31
}
fn at(row: &NativeExitPlane, y: u32, x: u32) -> usize {
    (row.layout.Offset + y as u64 * row.layout.Footprint.RowPitch as u64 + row.component as u64) as usize + x as usize * row.stride as usize
}
fn load(bytes: &[u8]) -> u64 {
    let mut word = [0u8; 8];
    word[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(word)
}
/// ComparePlane: the reference and optimized planes against the native one.
#[allow(clippy::too_many_arguments)]
pub fn compare_plane(
    width: u32,
    height: u32,
    reference: &NativeExitPlane,
    reference_data: &[u8],
    optimized: &NativeExitPlane,
    optimized_data: &[u8],
    native: &NativeExitPlane,
    native_data: &[u8],
) -> Option<Comparison> {
    for row in [reference, optimized, native] {
        if row.slot != native.slot || row.plane != native.plane || row.bytes != native.bytes {
            return None;
        }
    }
    plane_bounds(reference, width, height, reference_data.len() as u64)?;
    plane_bounds(optimized, width, height, optimized_data.len() as u64)?;
    plane_bounds(native, width, height, native_data.len() as u64)?;
    let mut result = Comparison::default();
    let bytes = native.bytes as usize;
    for y in 0..height {
        for x in 0..width {
            let target = &native_data[at(native, y, x)..][..bytes];
            let record = |value: &[u8], difference: &mut Difference| {
                if value == target {
                    return;
                }
                if difference.count == 0 {
                    difference.x = x;
                    difference.y = y;
                    difference.value = load(value);
                    difference.native_value = load(target);
                }
                difference.count += 1;
            };
            record(&reference_data[at(reference, y, x)..][..bytes], &mut result.reference);
            record(&optimized_data[at(optimized, y, x)..][..bytes], &mut result.optimized);
        }
    }
    Some(result)
}
/// Accumulate (unsigned wrapping, as in C++).
pub fn accumulate(stats: &mut [u64; 24], eye: u32, plane: &NativeExitPlane, width: u32, comparison: &Comparison) {
    let delta: usize = if plane.slot < 3 { 4 } else if plane.plane != 0 { 12 } else { 8 };
    let eye = eye as usize;
    stats[2] = stats[2].wrapping_add(1);
    stats[3] = stats[3].wrapping_add((width as u64).wrapping_mul(plane.rows as u64).wrapping_mul(plane.bytes as u64));
    let mut add = |index: usize, value: u64| {
        if let Some(slot) = stats.get_mut(index) {
            *slot = slot.wrapping_add(value);
        }
    };
    add(delta + eye, comparison.reference.count);
    add(delta + 2 + eye, comparison.optimized.count);
    add(16 + eye, comparison.reference.count);
    add(18 + eye, comparison.optimized.count);
}
/// RateValue: the D3D12 shading rates 1x1 .. 4x4.
pub fn rate_value(value: u8) -> bool {
    matches!(value, 0 | 1 | 4 | 5 | 6 | 9 | 10)
}
/// AlwaysFiner: every common rate is at least as fine as the original on both axes.
pub fn always_finer(common: &[u8], original: &[u8]) -> bool {
    if common.is_empty() || common.len() != original.len() {
        return false;
    }
    common.iter().zip(original).all(|(&c, &o)| rate_value(c) && rate_value(o) && (c >> 2) <= (o >> 2) && (c & 3) <= (o & 3))
}
/// RateMapShape.
pub fn rate_map_shape(width: u32, height: u32, tile: u32, map_width: u32, map_height: u32, bytes: usize) -> bool {
    width != 0
        && width <= 4096
        && height != 0
        && height <= 4096
        && tile != 0
        && tile <= 4096
        && map_width != 0
        && map_width <= 512
        && map_height != 0
        && map_height <= 512
        && map_width >= (width + tile - 1) / tile
        && map_height >= (height + tile - 1) / tile
        && bytes == map_width as usize * map_height as usize
}
/// AttributeColor: the differing color pixels inside and outside the refined tiles.
#[allow(clippy::too_many_arguments)]
pub fn attribute_color(
    width: u32,
    height: u32,
    reference: &NativeExitPlane,
    reference_data: &[u8],
    native: &NativeExitPlane,
    native_data: &[u8],
    tile: u32,
    map_width: u32,
    map_height: u32,
    common: &[u8],
    original: &[u8],
) -> Option<(u64, u64)> {
    if reference.slot >= 3
        || reference.slot != native.slot
        || reference.plane != 0
        || native.plane != 0
        || plane_bounds(reference, width, height, reference_data.len() as u64).is_none()
        || plane_bounds(native, width, height, native_data.len() as u64).is_none()
        || !rate_map_shape(width, height, tile, map_width, map_height, common.len())
        || !always_finer(common, original)
    {
        return None;
    }
    let (mut inside, mut outside) = (0u64, 0u64);
    let bytes = native.bytes as usize;
    for y in 0..height {
        let a = (reference.layout.Offset + y as u64 * reference.layout.Footprint.RowPitch as u64) as usize;
        let b = (native.layout.Offset + y as u64 * native.layout.Footprint.RowPitch as u64) as usize;
        for x in 0..width {
            let left = &reference_data[a + x as usize * reference.stride as usize..][..bytes];
            let right = &native_data[b + x as usize * native.stride as usize..][..bytes];
            if left != right {
                let cell = (y / tile) as usize * map_width as usize + (x / tile) as usize;
                if common[cell] != original[cell] {
                    inside += 1;
                } else {
                    outside += 1;
                }
            }
        }
    }
    Some((inside, outside))
}
/// Acceptance.
pub fn acceptance(allow_vrs_refinement: bool, native: &[u64; 24], native_rate: &[u64; 16], attribution: &[u64; 12], pair_different: &[u64; 2]) -> [u64; 8] {
    let mut result = [0u64; 8];
    result[0] = allow_vrs_refinement as u64;
    result[1] = (native_rate[0] == 1
        && native_rate[1] == 2
        && native_rate[3] == 10
        && native_rate[13] == 1
        && native_rate[14] == 1
        && native_rate[11] == 0
        && native_rate[12] == 0
        && native_rate[15] == 0) as u64;
    result[2] = (attribution[3] == 1 && attribution[4] == 1 && attribution[9] == 1 && attribution[10] == 1 && attribution[11] == 1) as u64;
    for eye in 0..2 {
        let mut unexplained = attribution[7 + eye];
        for index in [8usize, 10, 12, 14] {
            let value = native[index + eye];
            unexplained = if value > u64::MAX - unexplained { u64::MAX } else { unexplained + value };
        }
        result[3 + eye] = unexplained;
        result[5 + eye] = pair_different[eye];
    }
    let compared = native[0] == 1 && native[1] == 2 && native[2] == 10 && native[20] == 1;
    let strict = native[21] == 1 && native[16] == 0 && native[17] == 0 && native[18] == 0 && native[19] == 0;
    let approved = allow_vrs_refinement && result[1] != 0 && result[2] != 0 && result[3] == 0 && result[4] == 0;
    result[7] =
        (compared && native[22] == 0 && native_rate[15] == 0 && result[5] == 0 && result[6] == 0 && (strict || approved)) as u64;
    result
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
unsafe fn plane<'a>(value: &PlaneData) -> (&'a NativeExitPlane, &'a [u8]) {
    unsafe { (&*value.row, slice(value.data, value.bytes)) }
}
/// PlaneIndex.
#[no_mangle]
pub extern "C-unwind" fn cvr_nx_plane_index(slot: u32, plane: u32) -> u32 {
    plane_index(slot, plane)
}
/// ReadbackShape.
///
/// # Safety
/// `desc` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nx_readback_shape(desc: *const D3D12_RESOURCE_DESC) -> bool {
    readback_shape(unsafe { &*desc })
}
/// PlaneBounds (`end`, when not null, written on success).
///
/// # Safety
/// `row` is valid; `end` is null or writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nx_plane_bounds(row: *const NativeExitPlane, width: u32, height: u32, buffer_bytes: u64, end: *mut u64) -> bool {
    let Some(reached) = plane_bounds(unsafe { &*row }, width, height, buffer_bytes) else { return false };
    if !end.is_null() {
        unsafe { *end = reached };
    }
    true
}
/// ValidatePlane.
///
/// # Safety
/// `row` and `expected` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nx_validate_plane(
    row: *const NativeExitPlane,
    width: u32,
    height: u32,
    buffer_bytes: u64,
    expected: *const D3D12_SUBRESOURCE_FOOTPRINT,
    expected_rows: u32,
    expected_row_bytes: u64,
) -> bool {
    unsafe { validate_plane(&*row, width, height, buffer_bytes, &*expected, expected_rows, expected_row_bytes) }
}
/// CompletePlanes.
///
/// # Safety
/// `planes` holds `count` planes (or is null with none).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nx_complete_planes(planes: *const NativeExitPlane, count: usize) -> bool {
    complete_planes(unsafe { slice(planes, count) })
}
/// ComparePlane: `planes` reference, optimized, native (`result` always written).
///
/// # Safety
/// `planes` holds three valid planes with their bytes; `result` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nx_compare_plane(width: u32, height: u32, planes: *const [PlaneData; 3], result: *mut Comparison) -> bool {
    let planes = unsafe { &*planes };
    let ((reference, a), (optimized, b), (native, c)) = unsafe { (plane(&planes[0]), plane(&planes[1]), plane(&planes[2])) };
    let comparison = compare_plane(width, height, reference, a, optimized, b, native, c);
    unsafe { *result = comparison.unwrap_or_default() };
    comparison.is_some()
}
/// Accumulate.
///
/// # Safety
/// `stats`, `plane` and `comparison` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nx_accumulate(stats: *mut [u64; 24], eye: u32, plane: *const NativeExitPlane, width: u32, comparison: *const Comparison) {
    unsafe { accumulate(&mut *stats, eye, &*plane, width, &*comparison) }
}
/// RateValue.
#[no_mangle]
pub extern "C-unwind" fn cvr_nx_rate_value(value: u8) -> bool {
    rate_value(value)
}
/// AlwaysFiner.
///
/// # Safety
/// The maps hold their counts (or are null with none).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nx_always_finer(common: *const u8, common_bytes: usize, original: *const u8, original_bytes: usize) -> bool {
    unsafe { always_finer(slice(common, common_bytes), slice(original, original_bytes)) }
}
/// RateMapShape.
#[no_mangle]
pub extern "C-unwind" fn cvr_nx_rate_map_shape(width: u32, height: u32, tile: u32, map_width: u32, map_height: u32, bytes: usize) -> bool {
    rate_map_shape(width, height, tile, map_width, map_height, bytes)
}
/// AttributeColor: `planes` reference, native; `counts` inside, outside (always written).
///
/// # Safety
/// `planes` holds two valid planes with their bytes; the maps hold their counts; `counts` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_nx_attribute_color(
    width: u32,
    height: u32,
    planes: *const [PlaneData; 2],
    tile: u32,
    map_width: u32,
    map_height: u32,
    common: *const u8,
    common_bytes: usize,
    original: *const u8,
    original_bytes: usize,
    counts: *mut [u64; 2],
) -> bool {
    let planes = unsafe { &*planes };
    let ((reference, a), (native, b)) = unsafe { (plane(&planes[0]), plane(&planes[1])) };
    let (common, original) = unsafe { (slice(common, common_bytes), slice(original, original_bytes)) };
    let result = attribute_color(width, height, reference, a, native, b, tile, map_width, map_height, common, original);
    let (inside, outside) = result.unwrap_or((0, 0));
    unsafe { *counts = [inside, outside] };
    result.is_some()
}
/// Acceptance.
///
/// # Safety
/// The rows are valid; `result` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nx_acceptance(
    allow_vrs_refinement: bool,
    native: *const [u64; 24],
    native_rate: *const [u64; 16],
    attribution: *const [u64; 12],
    pair_different: *const [u64; 2],
    result: *mut [u64; 8],
) {
    unsafe { *result = acceptance(allow_vrs_refinement, &*native, &*native_rate, &*attribution, &*pair_different) };
}
