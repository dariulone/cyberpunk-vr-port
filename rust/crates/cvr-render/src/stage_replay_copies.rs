//! The stage replay's partitions and census copies (src/Render/StageReplay.cpp, `SourcePartitioned` and `CensusCopy`):
//! a stage's source calls are partitions of one pass when their spans, sorted, tile end to start; and a census copy
//! keeps each destination range inside the scope's shader-visible heap as a copy command (its first descriptor's heap
//! index and size, flagged 2 when it carries a camera descriptor) over the flattened source descriptors (at most 512),
//! stopping at the first range the sources do not cover.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_srx_tiled` and `cvr_srx_copy_plan` (the recording itself
//! stays the .cpp's); the unit tests below cover each case.

/// The spans tile end to start once sorted.
pub fn tiled(spans: &mut [[u64; 2]]) -> bool {
    spans.sort_unstable();
    spans.windows(2).all(|pair| pair[0][1] == pair[1][0])
}
/// One kept copy: the destination's heap index, its size, its flags and its sources' offset in the flattened list.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopyRow {
    pub index: u32,
    pub size: u32,
    pub flags: u32,
    pub offset: u32,
}
/// The scope's heap and camera descriptors.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Heap {
    pub cpu: u64,
    pub count: u32,
    pub stride: u32,
    pub camera_vs: u64,
    pub camera_ps: u64,
}
/// CensusCopy's plan: flattens the sources into `flat` and returns the kept rows.
pub fn copy_plan(heap: &Heap, dest: &[u64], sizes: Option<&[u32]>, src: &[u64], counts: Option<&[u32]>, flat: &mut [u64; 512]) -> Vec<CopyRow> {
    let stride = u64::from(heap.stride);
    let mut total = 0usize;
    'flatten: for (r, &start) in src.iter().enumerate() {
        let c = counts.map_or(1, |c| c[r]);
        for i in 0..c {
            if total >= flat.len() {
                break 'flatten;
            }
            flat[total] = start.wrapping_add(u64::from(i).wrapping_mul(stride));
            total += 1;
        }
    }
    let mut rows = Vec::new();
    let mut consumed = 0usize;
    let end = heap.cpu.wrapping_add(u64::from(heap.count).wrapping_mul(stride));
    for (r, &start) in dest.iter().enumerate() {
        let size = sizes.map_or(1, |s| s[r]) as usize;
        if consumed + size > total {
            break;
        }
        if start >= heap.cpu && start < end {
            let camera = flat[consumed..consumed + size].iter().any(|&d| (heap.camera_vs != 0 && d == heap.camera_vs) || (heap.camera_ps != 0 && d == heap.camera_ps));
            rows.push(CopyRow {
                index: ((start - heap.cpu) / stride) as u32,
                size: size as u32,
                flags: if camera { 2 } else { 0 },
                offset: consumed as u32,
            });
        }
        consumed += size;
    }
    rows
}

// ---- the C ABI

/// # Safety
/// `spans` holds `count` begin/end pairs, sorted in place.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_srx_tiled(spans: *mut [u64; 2], count: usize) -> bool {
    let spans = if spans.is_null() || count == 0 { &mut [][..] } else { unsafe { core::slice::from_raw_parts_mut(spans, count) } };
    tiled(spans)
}
/// # Safety
/// `dest`/`sizes` hold `n` entries and `src`/`counts` `m` (sizes and counts may be null); `flat` holds 512 descriptors and
/// `rows` `n` rows. Returns the kept rows' count.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_srx_copy_plan(
    heap: *const Heap,
    n: u32,
    dest: *const u64,
    sizes: *const u32,
    m: u32,
    src: *const u64,
    counts: *const u32,
    flat: *mut [u64; 512],
    rows: *mut CopyRow,
) -> u32 {
    let part = |p: *const u64, k: u32| if p.is_null() || k == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(p, k as usize) } };
    let opt = |p: *const u32, k: u32| (!p.is_null() && k != 0).then(|| unsafe { core::slice::from_raw_parts(p, k as usize) });
    let plan = copy_plan(unsafe { &*heap }, part(dest, n), opt(sizes, n), part(src, m), opt(counts, m), unsafe { &mut *flat });
    for (i, row) in plan.iter().enumerate() {
        unsafe { *rows.add(i) = *row };
    }
    plan.len() as u32
}

#[cfg(test)]
mod tests {
    //! Each case.
    use super::*;

    #[test]
    fn tiles() {
        assert!(tiled(&mut [[30, 40], [10, 20], [20, 30]]));
        assert!(!tiled(&mut [[30, 40], [10, 20], [21, 30]]));
        assert!(tiled(&mut [[5, 9]]));
        assert!(tiled(&mut []));
    }
    #[test]
    fn copies() {
        let heap = Heap { cpu: 0x1000, count: 16, stride: 32, camera_vs: 0x5040, camera_ps: 0 };
        let mut flat = [0u64; 512];
        // Two sources of 2 and 1 descriptors; three destinations of 1, 1 and 1 (the second outside the heap).
        let rows = copy_plan(&heap, &[0x1040, 0x9000, 0x11E0], None, &[0x5000, 0x6000], Some(&[2, 1]), &mut flat);
        assert_eq!(flat[..3], [0x5000, 0x5020, 0x6000]);
        assert_eq!(rows, vec![CopyRow { index: 2, size: 1, flags: 0, offset: 0 }, CopyRow { index: 15, size: 1, flags: 0, offset: 2 }]);
        // One destination of two covers the camera descriptor.
        let heap = Heap { camera_vs: 0x5020, ..heap };
        let rows = copy_plan(&heap, &[0x1000], Some(&[2]), &[0x5000], Some(&[2]), &mut flat);
        assert_eq!(rows, vec![CopyRow { index: 0, size: 2, flags: 2, offset: 0 }]);
        // A destination the sources no longer cover ends the plan.
        let rows = copy_plan(&heap, &[0x1000, 0x1020], Some(&[1, 2]), &[0x5000], Some(&[2]), &mut flat);
        assert_eq!(rows.len(), 1);
        // The heap's end is exclusive.
        let rows = copy_plan(&heap, &[0x1200], None, &[0x5000], None, &mut flat);
        assert!(rows.is_empty());
        // At most 512 flattened sources.
        let rows = copy_plan(&heap, &[0x1000], Some(&[512]), &[0x5000], Some(&[600]), &mut flat);
        assert_eq!((rows.len(), flat[511]), (1, 0x5000 + 511 * 32));
        let rows = copy_plan(&heap, &[0x1000], Some(&[513]), &[0x5000], Some(&[600]), &mut flat);
        assert!(rows.is_empty());
    }
}
