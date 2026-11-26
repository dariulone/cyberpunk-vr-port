//! The descriptor audit's borrowed source range walk (src/Render/StereoDescriptorAudit.cpp,
//! `descriptors::CaptureBorrowedSourceRanges`): under the metadata lock, after the shader ranges are planned, each bound
//! shader-visible heap's cookie (taken now) known with its whole extent in the store, and for every used range a bound
//! table of the root (a non-zero count) inside the heap of its type on a slot boundary, its range identity in the store
//! (its heap, stride and CPU address the heap's); a failure reports its diagnostic stage (6 a heap, 10 a range, with the
//! range's index and, once resolved, its address and the counts).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the walk forwards to `cvr_sda_borrowed_walk` on the .cpp's Rust store (the heap
//! cookies written back, the ranges into the .cpp's array). No C++ test reaches the walk (it needs a live recording);
//! the unit test below walks a WARP heap through each stage.

use core::ffi::c_void;

use windows::Win32::Graphics::Direct3D12::*;

use crate::audit_lifetimes::heap_known;
use crate::audit_values::cookie;
use crate::descriptor_store::{BorrowedUsedRange, RangeIdentity, Store};

/// One bound heap as the .cpp lends it; the walk writes its cookie.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Heap {
    pub object: *mut c_void,
    pub kind: u32,
    pub count: u32,
    pub cpu: u64,
    pub gpu: u64,
    pub stride: u32,
    pub cookie: u64,
}
const _: () = assert!(core::mem::size_of::<Heap>() == 48);

/// BorrowedSourceRanges::Data::Range.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub used: BorrowedUsedRange,
    pub heap_index: u32,
    pub table: u64,
    pub identity: RangeIdentity,
}
const _: () = assert!(core::mem::size_of::<Range>() == 88);

/// A failure's BorrowedStage arguments.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stage {
    pub stage: u32,
    pub index: u64,
    pub gpu: u64,
    pub expected: u64,
    pub actual: u64,
}
const _: () = assert!(core::mem::size_of::<Stage>() == 40);

fn stage(stage: u32, index: usize, gpu: u64) -> Stage {
    Stage { stage, index: index as u64, gpu, expected: 0, actual: 0 }
}

/// The walk: `out` holds one range per used range.
pub fn walk(store: &Store, heaps: &mut [Heap], used: &[BorrowedUsedRange], table_mask: u64, tables: &[u64; 64], out: &mut [Range]) -> Result<(), Stage> {
    for (i, heap) in heaps.iter_mut().enumerate() {
        heap.cookie = cookie(heap.object);
        if heap.cookie == 0 || !heap_known(heap.cookie) {
            return Err(stage(6, i, heap.gpu));
        }
        let full = store.range(heap.gpu, heap.count);
        if !full.valid || full.heap != heap.cookie || full.cpu != heap.cpu || full.stride != heap.stride || full.count != heap.count {
            return Err(stage(6, i, heap.gpu));
        }
    }
    for (n, (use_, slot)) in used.iter().zip(out.iter_mut()).enumerate() {
        if use_.parameter >= 64 || table_mask & (1 << use_.parameter) == 0 || use_.count == 0 {
            return Err(stage(10, n, 0));
        }
        let kind = if use_.kind == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER.0 as u32 { D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER } else { D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV };
        let Some(index) = heaps.iter().position(|h| h.kind == kind.0 as u32) else {
            return Err(stage(10, n, 0));
        };
        let heap = &heaps[index];
        let table = tables[use_.parameter as usize];
        let stride = heap.stride as u64;
        let delta = use_.offset as u64 * stride;
        if table == 0 || table < heap.gpu || (table - heap.gpu) % stride != 0 || table - heap.gpu >= heap.count as u64 * stride || table > u64::MAX - delta {
            return Err(stage(10, n, 0));
        }
        let range = store.range(table + delta, use_.count);
        if !range.valid || range.heap != heap.cookie || range.stride != heap.stride || range.cpu != heap.cpu.wrapping_add(range.gpu.wrapping_sub(heap.gpu)) {
            return Err(Stage { stage: 10, index: n as u64, gpu: table + delta, expected: use_.count as u64, actual: range.count as u64 });
        }
        *slot = Range { used: *use_, heap_index: index as u32, table, identity: range };
    }
    Ok(())
}

// ---- the C ABI

/// The walk: true with `out` filled (`used_count` ranges), otherwise false with `failure` set.
///
/// # Safety
/// `store` is the .cpp's Rust store under its metadata lock; `heaps` holds `heap_count` live heaps; `used` holds
/// `used_count` ranges and `out` room for as many; `tables` 64 addresses; `failure` is writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sda_borrowed_walk(
    store: *const Store,
    heaps: *mut Heap,
    heap_count: usize,
    used: *const BorrowedUsedRange,
    used_count: usize,
    table_mask: u64,
    tables: *const u64,
    out: *mut Range,
    failure: *mut Stage,
) -> bool {
    let heaps = if heaps.is_null() || heap_count == 0 { &mut [][..] } else { unsafe { core::slice::from_raw_parts_mut(heaps, heap_count) } };
    let (used, out) = if used.is_null() || out.is_null() || used_count == 0 {
        (&[][..], &mut [][..])
    } else {
        unsafe { (core::slice::from_raw_parts(used, used_count), core::slice::from_raw_parts_mut(out, used_count)) }
    };
    match walk(unsafe { &*store }, heaps, used, table_mask, unsafe { &*(tables as *const [u64; 64]) }, out) {
        Ok(()) => true,
        Err(stage) => {
            unsafe { *failure = stage };
            false
        }
    }
}

#[cfg(test)]
mod tests {
    //! A WARP heap's walk: unknown and unstored heaps, a resolved range, an unbound table, a table off a slot boundary
    //! and a range past the heap, each with its stage.
    use super::*;
    use crate::audit_lifetimes::heap_insert;
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    fn device() -> ID3D12Device {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }

    #[test]
    fn walks_used_ranges_and_reports_stages() {
        let device = device();
        let desc = D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, NumDescriptors: 16, Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, NodeMask: 0 };
        let heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap(&desc) }.unwrap();
        let stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) };
        let cpu = unsafe { heap.GetCPUDescriptorHandleForHeapStart() }.ptr as u64;
        let gpu = unsafe { heap.GetGPUDescriptorHandleForHeapStart() }.ptr;
        let step = stride as u64;
        let mut store = Store::new(false);
        let mut heaps = [Heap { object: heap.as_raw(), kind: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV.0 as u32, count: 16, cpu, gpu, stride, cookie: 0 }];
        let used = [BorrowedUsedRange { parameter: 2, kind: D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0 as u32, offset: 3, count: 4, shader_declared_count: 4, physical_remainder: false }];
        let mut tables = [0u64; 64];
        tables[2] = gpu + step;
        let mut out = [Range::default()];
        assert_eq!(walk(&store, &mut heaps, &used, 1 << 2, &tables, &mut out), Err(stage(6, 0, gpu)));
        let cookie = heaps[0].cookie;
        assert_ne!(cookie, 0);
        heap_insert(cookie);
        assert_eq!(walk(&store, &mut heaps, &used, 1 << 2, &tables, &mut out), Err(stage(6, 0, gpu)));
        assert!(store.heap_created(cpu, gpu, 16, stride, cookie));
        assert_eq!(walk(&store, &mut heaps, &used, 1 << 2, &tables, &mut out), Ok(()));
        assert_eq!((out[0].used, out[0].heap_index, out[0].table), (used[0], 0, gpu + step));
        assert_eq!((out[0].identity.gpu, out[0].identity.cpu, out[0].identity.count, out[0].identity.heap), (gpu + 4 * step, cpu + 4 * step, 4, cookie));
        assert_eq!(walk(&store, &mut heaps, &used, 0, &tables, &mut out), Err(stage(10, 0, 0)));
        tables[2] = gpu + 1;
        assert_eq!(walk(&store, &mut heaps, &used, 1 << 2, &tables, &mut out), Err(stage(10, 0, 0)));
        tables[2] = gpu + 14 * step;
        assert_eq!(
            walk(&store, &mut heaps, &used, 1 << 2, &tables, &mut out),
            Err(Stage { stage: 10, index: 0, gpu: gpu + 17 * step, expected: 4, actual: 0 })
        );
    }
}
