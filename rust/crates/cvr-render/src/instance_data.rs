//! THE STEREO DRAWS' INSTANCE DATA RANGE (include/Render/StereoInstanceData.hpp): the byte range of a draw's instance
//! batch inside its instance buffer -- slot 7 bound with a 48 (transform) or 64 (with skin metadata) byte stride, an
//! UPLOAD heap (or an observed DEFAULT one when allowed), the batch from its first instance within the view and the
//! resource, at most 16 KiB -- and the exact equality of two draws' batches (never a prefix or a hash). The header
//! forwards to these in the Rust build (the draw record's fields are lent, not mirrored);
//! tools/single_pass_tests/instance_data_test runs its cases against them.

pub const MAX_BYTES: u64 = 16 * 1024;
const HEAP_DEFAULT: u32 = 1;
const HEAP_UPLOAD: u32 = 2;

/// The draw record's fields Range reads.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct RangeInput {
    pub buffer_location: u64,
    pub size_in_bytes: u32,
    pub stride: u32,
    pub instance_resource: u64,
    pub instance_gpu_base: u64,
    pub instance_bytes_total: u64,
    pub vertex_mask: u32,
    pub instances: u32,
    pub first_instance: u32,
    pub heap_type: u32,
}
const _: () = assert!(core::mem::size_of::<RangeInput>() == 56);

/// Range: the batch's offset and bytes in the instance resource.
pub fn range(d: &RangeInput, allow_observed_default: bool) -> Option<(u64, usize)> {
    if d.vertex_mask & (1 << 7) == 0
        || d.instances == 0
        || (d.stride != 48 && d.stride != 64)
        || (d.heap_type != HEAP_UPLOAD && !(allow_observed_default && d.heap_type == HEAP_DEFAULT))
        || d.instance_resource == 0
        || d.buffer_location < d.instance_gpu_base
    {
        return None;
    }
    let begin = d.first_instance as u64 * d.stride as u64;
    let total = d.instances as u64 * d.stride as u64;
    let size = d.size_in_bytes as u64;
    if total > MAX_BYTES || begin > size || total > size - begin {
        return None;
    }
    let base = d.buffer_location - d.instance_gpu_base;
    if base > d.instance_bytes_total
        || begin > d.instance_bytes_total - base
        || total > d.instance_bytes_total - base - begin
    {
        return None;
    }
    Some((base + begin, total as usize))
}

/// Equal: the same instance count and stride and byte-identical batches.
pub fn equal(
    a_instances: u32,
    a_stride: u32,
    x: &[u8],
    b_instances: u32,
    b_stride: u32,
    y: &[u8],
) -> bool {
    a_instances != 0
        && a_instances == b_instances
        && (a_stride == 48 || a_stride == 64)
        && a_stride == b_stride
        && x.len() as u64 == a_instances as u64 * a_stride as u64
        && x.len() as u64 <= MAX_BYTES
        && x.len() == y.len()
        && x == y
}

/// Range (`offset` / `bytes` untouched on a refusal, as by the C++ rules).
///
/// # Safety
/// `input` is valid; `offset` and `bytes` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_id_range(
    input: *const RangeInput,
    allow_observed_default: bool,
    offset: *mut u64,
    bytes: *mut usize,
) -> bool {
    unsafe {
        match range(&*input, allow_observed_default) {
            Some((o, n)) => {
                *offset = o;
                *bytes = n;
                true
            }
            None => false,
        }
    }
}

/// Equal.
///
/// # Safety
/// `x` holds `x_bytes` and `y` holds `y_bytes`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_id_equal(
    a_instances: u32,
    a_stride: u32,
    x: *const u8,
    x_bytes: usize,
    b_instances: u32,
    b_stride: u32,
    y: *const u8,
    y_bytes: usize,
) -> bool {
    unsafe {
        let view = |p: *const u8, n: usize| {
            if n == 0 || p.is_null() {
                &[][..]
            } else {
                core::slice::from_raw_parts(p, n)
            }
        };
        equal(
            a_instances,
            a_stride,
            view(x, x_bytes),
            b_instances,
            b_stride,
            view(y, y_bytes),
        )
    }
}
