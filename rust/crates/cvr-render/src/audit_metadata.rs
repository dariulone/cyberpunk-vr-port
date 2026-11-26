//! The descriptor audit's table metadata walk (src/Render/StereoDescriptorAudit.cpp, `descriptors::CaptureTableMetadata`):
//! under the metadata lock, a root layout's declared tables against the bound ones and the shader-visible heaps --
//! the layout's unknown flags (other root parameters, direct heap indexing, unbound tables), each declared range valid
//! (a parameter of the root, a known range type and visibility, a non-zero count; no bound table undeclared), each heap
//! known with its whole extent in the store, and for every bound declared range its table inside its heap (on a slot
//! boundary), its range identity in the store and the canonical value of each slot (unknown, wrong-kind and implicit
//! views flagged); at most 256 ranges and 65536 values.
//!
//! In the Rust build (`CVR_RUST_VRIK`) CaptureTableMetadata's walk forwards to `cvr_sda_metadata_walk` on the .cpp's
//! Rust store (the heaps' cookies read by the .cpp); the result lives in Rust until the .cpp copies it into its summary
//! and frees it. `tools/single_pass_tests/table_metadata_test` runs its cases against it in its Rust variant.

use windows::Win32::Graphics::Direct3D12::*;

use crate::audit_lifetimes::heap_known;
use crate::descriptor_store::{RangeIdentity, Store, Value};
use crate::source_layout::Binding;

/// TableMetadataSlotLimit.
pub const SLOT_LIMIT: usize = 65536;
const RANGE_LIMIT: usize = 256;

/// TableMetadataFailure.
const ROOT_LAYOUT: u32 = 3;
const HEAP: u32 = 4;
const BOUNDS: u32 = 5;
const CAPACITY: u32 = 6;

/// TableMetadataUnknown.
const OTHER_ROOT_PARAMETERS: u32 = 1;
const DIRECT_INDEXING: u32 = 2;
const UNBOUND_TABLES: u32 = 4;
const UNKNOWN_SLOTS: u32 = 8;
const WRONG_DESCRIPTOR_KIND: u32 = 16;
const IMPLICIT_VIEWS: u32 = 32;

/// One shader-visible heap as the .cpp lends it (its cookie 0 when unreadable).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Heap {
    pub kind: u32,
    pub count: u32,
    pub cookie: u64,
    pub cpu: u64,
    pub gpu: u64,
    pub stride: u32,
}
const _: () = assert!(core::mem::size_of::<Heap>() == 40);

/// TableMetadataSummary::Range.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub parameter: u32,
    pub kind: u32,
    pub space: u32,
    pub shader_register: u32,
    pub declared_count: u32,
    pub table_offset: u32,
    pub visibility: u32,
    pub first_value: u32,
    pub table: u64,
    pub physical_remainder: bool,
    pub identity: RangeIdentity,
}
const _: () = assert!(core::mem::size_of::<Range>() == 96);

/// The walk's summary parts.
#[derive(Debug, Default)]
pub struct Walk {
    pub ranges: Vec<Range>,
    pub values: Vec<Value>,
    pub unknown_slots: u64,
    pub unknown_flags: u32,
    pub unbound_tables: u32,
}

/// The layout of the root (`ranges`, `parameters`, `flags`, `reasons`) walked against the bound tables: the summary
/// parts, or the failure.
#[allow(clippy::too_many_arguments)]
pub fn walk(store: &Store, ranges: &[Binding], parameters: u32, flags: u32, reasons: u32, bound_mask: u64, tables: &[u64; 64], heaps: &[Heap]) -> Result<Walk, u32> {
    let mut out = Walk::default();
    if reasons & 2 != 0 || flags & D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE.0 as u32 != 0 {
        out.unknown_flags |= OTHER_ROOT_PARAMETERS;
    }
    if flags & (D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED.0 | D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED.0) as u32 != 0 {
        out.unknown_flags |= DIRECT_INDEXING;
    }
    let mut declared = 0u64;
    for range in ranges {
        if range.parameter >= parameters || range.kind > 3 || range.visibility > 7 || range.count == 0 {
            return Err(ROOT_LAYOUT);
        }
        declared |= 1 << range.parameter;
    }
    if bound_mask & !declared != 0 {
        return Err(ROOT_LAYOUT);
    }
    out.unbound_tables = (declared & !bound_mask).count_ones();
    if out.unbound_tables != 0 {
        out.unknown_flags |= UNBOUND_TABLES;
    }
    for heap in heaps {
        if heap.cookie == 0 || !heap_known(heap.cookie) {
            return Err(HEAP);
        }
        let full = store.range(heap.gpu, heap.count);
        if !full.valid || full.heap != heap.cookie || full.cpu != heap.cpu || full.stride != heap.stride || full.count != heap.count {
            return Err(HEAP);
        }
    }
    const KINDS: [u64; 4] = [2, 3, 1, 4];
    for declared in ranges {
        if bound_mask & (1 << declared.parameter) == 0 {
            continue;
        }
        let kind = if declared.kind == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 as u32 { D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER } else { D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV };
        let heap = heaps.iter().find(|h| h.kind == kind.0 as u32).ok_or(HEAP)?;
        let table = tables[declared.parameter as usize];
        let delta = declared.table_offset as u64 * heap.stride as u64;
        let stride = heap.stride as u64;
        if table < heap.gpu || table - heap.gpu >= heap.count as u64 * stride || (table - heap.gpu) % stride != 0 || table > u64::MAX - delta {
            return Err(BOUNDS);
        }
        let identity = store.range(table + delta, declared.count);
        if !identity.valid || identity.heap != heap.cookie || identity.stride != heap.stride || identity.cpu != heap.cpu.wrapping_add(identity.gpu.wrapping_sub(heap.gpu)) {
            return Err(BOUNDS);
        }
        if out.ranges.len() == RANGE_LIMIT || identity.count as usize > SLOT_LIMIT - out.values.len() {
            return Err(CAPACITY);
        }
        out.ranges.push(Range {
            parameter: declared.parameter,
            kind: declared.kind,
            space: declared.space,
            shader_register: declared.shader_register,
            declared_count: declared.count,
            table_offset: declared.table_offset,
            visibility: declared.visibility,
            first_value: out.values.len() as u32,
            table,
            physical_remainder: declared.count == u32::MAX,
            identity,
        });
        for index in 0..identity.count {
            let value = store.get(store.at(identity.cpu.wrapping_add(index as u64 * identity.stride as u64)));
            out.values.push(value.copied().unwrap_or_default());
            match value {
                Some(v) if v[0] != 0 && v[0] <= 4 => {
                    if v[0] != KINDS[declared.kind as usize] {
                        out.unknown_flags |= WRONG_DESCRIPTOR_KIND;
                    } else if (v[0] == 2 || v[0] == 3) && v[1] & 2 != 0 {
                        out.unknown_flags |= IMPLICIT_VIEWS;
                    }
                }
                _ => {
                    out.unknown_slots += 1;
                    out.unknown_flags |= UNKNOWN_SLOTS;
                }
            }
        }
    }
    Ok(out)
}

// ---- the C ABI

/// The walk's result as the .cpp copies it: `handle` frees it.
#[repr(C)]
pub struct WalkView {
    pub handle: *mut Walk,
    pub ranges: *const Range,
    pub range_count: usize,
    pub values: *const Value,
    pub value_count: usize,
    pub unknown_slots: u64,
    pub unknown_flags: u32,
    pub unbound_tables: u32,
}

/// The walk: 0 with `out` filled (free it with cvr_sda_metadata_free), or the failure (`out` left empty).
///
/// # Safety
/// `store` is the .cpp's Rust store under its metadata lock; `ranges` holds `count` bindings; `tables` 64 addresses;
/// `heaps` `heap_count` heaps; `out` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sda_metadata_walk(
    store: *const Store,
    ranges: *const Binding,
    count: usize,
    parameters: u32,
    flags: u32,
    reasons: u32,
    bound_mask: u64,
    tables: *const u64,
    heaps: *const Heap,
    heap_count: usize,
    out: *mut WalkView,
) -> u32 {
    let ranges = if ranges.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(ranges, count) } };
    let heaps = if heaps.is_null() || heap_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(heaps, heap_count) } };
    let tables = unsafe { &*(tables as *const [u64; 64]) };
    unsafe {
        *out = WalkView {
            handle: core::ptr::null_mut(),
            ranges: core::ptr::null(),
            range_count: 0,
            values: core::ptr::null(),
            value_count: 0,
            unknown_slots: 0,
            unknown_flags: 0,
            unbound_tables: 0,
        }
    };
    match walk(unsafe { &*store }, ranges, parameters, flags, reasons, bound_mask, tables, heaps) {
        Ok(result) => {
            let held = Box::new(result);
            unsafe {
                *out = WalkView {
                    ranges: held.ranges.as_ptr(),
                    range_count: held.ranges.len(),
                    values: held.values.as_ptr(),
                    value_count: held.values.len(),
                    unknown_slots: held.unknown_slots,
                    unknown_flags: held.unknown_flags,
                    unbound_tables: held.unbound_tables,
                    handle: Box::into_raw(held),
                }
            };
            0
        }
        Err(failure) => failure,
    }
}
/// # Safety
/// `handle` is null or a walk's handle, freed once.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_metadata_free(handle: *mut Walk) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}
