//! The instance snapshot view's rules (`include/Render/InstanceSnapshotView.hpp`, `detail`, and
//! `src/Render/InstanceSnapshotView.cpp`'s `detail::Layout`): a capture scope (nonzero root, list, generation and
//! occurrence, eye 0 or 1); the native instance stream's view shape (a 4-aligned buffer of 48-byte instances in 6 MiB
//! or 64-byte ones in 2 MiB); the copy of `count` instances from `firstInstance` reserved at its original offset inside a
//! whole-view window after `cursor`, within the budget (64 MiB at most); and the input layout's arithmetic (per-vertex
//! elements outside slot 7, per-instance ones only in slot 7 at step rate 1 and inside the stride, consistent classes and
//! steps per slot, aligned offsets, the appended ones placed as D3D12 places them).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the detail functions forward to `cvr_isv_*`;
//! `tools/single_pass_tests/instance_snapshot_view_test` runs its cases against them in its Rust variant.

use windows::Win32::Graphics::Dxgi::Common::*;

pub const SLOT: u32 = 7;
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
const SLOT_COUNT: usize = 32;
const APPEND_ALIGNED_ELEMENT: u32 = 0xFFFF_FFFF;
const PER_VERTEX: u32 = 0;
const PER_INSTANCE: u32 = 1;

/// instance_snapshot_view::Scope.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub root: u64,
    pub list: u64,
    pub generation: u64,
    pub occurrence: u64,
    pub eye: u32,
}
/// instance_snapshot_view::Mapping.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mapping {
    pub source_offset: u64,
    pub target_offset: u64,
    pub view_offset: u64,
    pub copy_bytes: u64,
    pub next_free: u64,
    pub view_end: u64,
}
/// D3D12_VERTEX_BUFFER_VIEW.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VertexBufferView {
    pub buffer_location: u64,
    pub size_in_bytes: u32,
    pub stride_in_bytes: u32,
}
/// One input element's layout fields (shader_snapshot::Input, its semantic reduced to what the rule reads).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct InputElement {
    pub semantic_bytes: usize,
    pub semantic_nul: bool,
    pub index: u32,
    pub format: u32,
    pub slot: u32,
    pub offset: u32,
    pub classification: u32,
    pub step: u32,
}
const _: () = assert!(
    core::mem::size_of::<Scope>() == 40
        && core::mem::size_of::<Mapping>() == 48
        && core::mem::size_of::<VertexBufferView>() == 16
        && core::mem::size_of::<InputElement>() == 40
);

/// detail::ValidScope.
pub fn valid_scope(scope: &Scope) -> bool {
    scope.root != 0
        && scope.list != 0
        && scope.generation != 0
        && scope.occurrence != 0
        && scope.eye < 2
}
/// detail::ViewShape.
pub fn view_shape(view: &VertexBufferView) -> bool {
    view.buffer_location != 0
        && view.buffer_location <= u64::MAX - view.size_in_bytes as u64
        && view.buffer_location % 4 == 0
        && ((view.stride_in_bytes == 48 && view.size_in_bytes == 6 * 1024 * 1024)
            || (view.stride_in_bytes == 64 && view.size_in_bytes == 2 * 1024 * 1024))
}
/// detail::Reserve.
pub fn reserve(
    cursor: u64,
    view: &VertexBufferView,
    first_instance: u32,
    count: u32,
    budget: u64,
) -> Option<Mapping> {
    if !view_shape(view) || count == 0 || budget == 0 || budget > MAX_BYTES || cursor > budget {
        return None;
    }
    let stride = view.stride_in_bytes as u64;
    let size = view.size_in_bytes as u64;
    let logical = first_instance as u64 * stride;
    let bytes = count as u64 * stride;
    if logical > size || bytes > size - logical {
        return None;
    }
    let remainder = cursor % stride;
    let padding = if remainder != 0 {
        stride - remainder
    } else {
        0
    };
    if padding > budget - cursor {
        return None;
    }
    let aligned = cursor + padding;
    let target = aligned.max(logical);
    let base = target - logical;
    if base > budget || size > budget - base || target > budget || bytes > budget - target {
        return None;
    }
    Some(Mapping {
        source_offset: logical,
        target_offset: target,
        view_offset: base,
        copy_bytes: bytes,
        next_free: target + bytes,
        view_end: base + size,
    })
}
/// ElementBytes: an input element format's bytes (0 for one the stream may not carry).
pub fn element_bytes(format: u32) -> u32 {
    let f = DXGI_FORMAT(format as i32);
    match f {
        DXGI_FORMAT_R32G32B32A32_FLOAT
        | DXGI_FORMAT_R32G32B32A32_UINT
        | DXGI_FORMAT_R32G32B32A32_SINT => 16,
        DXGI_FORMAT_R32G32B32_FLOAT | DXGI_FORMAT_R32G32B32_UINT | DXGI_FORMAT_R32G32B32_SINT => 12,
        DXGI_FORMAT_R32G32_FLOAT
        | DXGI_FORMAT_R32G32_UINT
        | DXGI_FORMAT_R32G32_SINT
        | DXGI_FORMAT_R16G16B16A16_FLOAT
        | DXGI_FORMAT_R16G16B16A16_UNORM
        | DXGI_FORMAT_R16G16B16A16_SNORM
        | DXGI_FORMAT_R16G16B16A16_UINT
        | DXGI_FORMAT_R16G16B16A16_SINT => 8,
        DXGI_FORMAT_R32_FLOAT
        | DXGI_FORMAT_R32_UINT
        | DXGI_FORMAT_R32_SINT
        | DXGI_FORMAT_R16G16_FLOAT
        | DXGI_FORMAT_R16G16_UNORM
        | DXGI_FORMAT_R16G16_SNORM
        | DXGI_FORMAT_R16G16_UINT
        | DXGI_FORMAT_R16G16_SINT
        | DXGI_FORMAT_R8G8B8A8_UNORM
        | DXGI_FORMAT_R8G8B8A8_SNORM
        | DXGI_FORMAT_R8G8B8A8_UINT
        | DXGI_FORMAT_R8G8B8A8_SINT
        | DXGI_FORMAT_R10G10B10A2_UNORM
        | DXGI_FORMAT_R10G10B10A2_UINT
        | DXGI_FORMAT_R11G11B10_FLOAT => 4,
        DXGI_FORMAT_R16_FLOAT
        | DXGI_FORMAT_R16_UNORM
        | DXGI_FORMAT_R16_SNORM
        | DXGI_FORMAT_R16_UINT
        | DXGI_FORMAT_R16_SINT
        | DXGI_FORMAT_R8G8_UNORM
        | DXGI_FORMAT_R8G8_SNORM
        | DXGI_FORMAT_R8G8_UINT
        | DXGI_FORMAT_R8G8_SINT => 2,
        DXGI_FORMAT_R8_UNORM | DXGI_FORMAT_R8_SNORM | DXGI_FORMAT_R8_UINT | DXGI_FORMAT_R8_SINT => {
            1
        }
        _ => 0,
    }
}
/// detail::Layout (`max_inputs`: shader_snapshot::MaxInputs).
pub fn layout(input: &[InputElement], stride: u32, max_inputs: usize) -> bool {
    if input.is_empty() || input.len() > max_inputs || (stride != 48 && stride != 64) {
        return false;
    }
    let mut next = [0u32; SLOT_COUNT];
    let mut classes = [0u32; SLOT_COUNT];
    let mut steps = [0u32; SLOT_COUNT];
    let mut seen = [false; SLOT_COUNT];
    let mut instance = false;
    for element in input {
        let bytes = element_bytes(element.format);
        let alignment = if bytes < 4 { bytes } else { 4 };
        if element.semantic_bytes == 0
            || element.semantic_bytes > 255
            || element.semantic_nul
            || element.slot as usize >= SLOT_COUNT
            || bytes == 0
            || (element.classification != PER_VERTEX && element.classification != PER_INSTANCE)
        {
            return false;
        }
        let slot = element.slot as usize;
        if seen[slot] && (classes[slot] != element.classification || steps[slot] != element.step) {
            return false;
        }
        seen[slot] = true;
        classes[slot] = element.classification;
        steps[slot] = element.step;
        if element.classification == PER_INSTANCE {
            if element.slot != SLOT || element.step != 1 {
                return false;
            }
            instance = true;
        } else if element.slot == SLOT || element.step != 0 {
            return false;
        }
        let mut offset = element.offset;
        if offset == APPEND_ALIGNED_ELEMENT {
            if next[slot] > u32::MAX - (alignment - 1) {
                return false;
            }
            offset = (next[slot] + alignment - 1) & !(alignment - 1);
        }
        if offset % alignment != 0 || offset > u32::MAX - bytes {
            return false;
        }
        if element.slot == SLOT && (offset > stride || bytes > stride - offset) {
            return false;
        }
        next[slot] = offset + bytes;
    }
    instance
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// detail::ValidScope (`which` 0, `value` a Scope) / ViewShape (1, `value` a view).
///
/// # Safety
/// `value` is a valid record of that kind.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_check(which: u32, value: *const core::ffi::c_void) -> bool {
    unsafe {
        if which == 0 {
            valid_scope(&*value.cast::<Scope>())
        } else {
            view_shape(&*value.cast::<VertexBufferView>())
        }
    }
}
/// detail::Reserve (`output` zeroed on a refusal).
///
/// # Safety
/// `view` is valid; `output` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_reserve(
    cursor: u64,
    view: *const VertexBufferView,
    first_instance: u32,
    count: u32,
    budget: u64,
    output: *mut Mapping,
) -> bool {
    unsafe {
        let mapping = reserve(cursor, &*view, first_instance, count, budget);
        *output = mapping.unwrap_or_default();
        mapping.is_some()
    }
}
/// detail::Layout.
///
/// # Safety
/// `input` holds `count` elements.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_layout(
    input: *const InputElement,
    count: usize,
    stride: u32,
    max_inputs: usize,
) -> bool {
    let input = if input.is_null() || count == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(input, count) }
    };
    layout(input, stride, max_inputs)
}
