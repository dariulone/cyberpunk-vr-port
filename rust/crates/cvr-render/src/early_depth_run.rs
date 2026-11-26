//! The early depth run's completion (src/Render/NativeEarlyDepth.cpp, `CompleteRunWork`): a completed frame's pass
//! counts added to the run's totals -- the instance and shader proofs, the skinning and binding statistics (their
//! failure words kept when set), the native camera's presence and draws, the camera stage masks --, the frame's row (its
//! frame, final statistics, native and optimized counts, GPU spans) and start appended under the run's sequence, and the
//! run statistics (frames, optimized or reference, native draws, compared and skipped draws); at most 1200 rows.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the pass's read and hands the counts over through
//! `cvr_ned_complete_run` with the exports as pointers; the unit test below covers the totals, the row and the limit.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering::Release, Ordering::SeqCst};

/// The run's limit of rows.
pub const MAX_RUN_ROWS: u32 = 1200;
/// A completed frame's counts.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RunWork {
    pub frame: u32,
    pub final_stats: [u32; 16],
    pub native: [u32; 8],
    pub optimized: [u64; 8],
    pub times: [u64; 6],
    pub instance_proofs: [u64; 2],
    pub shader_proofs: [u64; 4],
    pub skin_stats: [u64; 6],
    pub binding_stats: [u64; 4],
    pub camera_stage_masks: [u64; 4],
    pub native_camera_draws: u64,
    pub begin_qpc: u64,
    pub native_camera: bool,
}
/// The run's exports.
#[repr(C)]
pub struct RunExports {
    pub instance_proofs: *const [AtomicU64; 2],
    pub shader_proofs: *const [AtomicU64; 4],
    pub skinning_stats: *const [AtomicU64; 6],
    pub binding_checks: *const [AtomicU64; 4],
    pub native_camera: *const [AtomicU64; 2],
    pub camera_stage_masks: *const [AtomicU64; 4],
    pub seq: *const AtomicU32,
    pub rows: *mut [[u64; 16]; MAX_RUN_ROWS as usize],
    pub frame_starts: *mut [u64; MAX_RUN_ROWS as usize],
    pub count: *const AtomicU32,
    pub stats: *const [AtomicU64; 16],
}
const _: () = assert!(
    core::mem::size_of::<RunWork>() == 400
        && core::mem::offset_of!(RunWork, optimized) == 104
        && core::mem::offset_of!(RunWork, native_camera) == 392
        && core::mem::size_of::<RunExports>() == 88
);
/// The views of the exports.
pub struct Run<'a> {
    pub instance_proofs: &'a [AtomicU64; 2],
    pub shader_proofs: &'a [AtomicU64; 4],
    pub skinning_stats: &'a [AtomicU64; 6],
    pub binding_checks: &'a [AtomicU64; 4],
    pub native_camera: &'a [AtomicU64; 2],
    pub camera_stage_masks: &'a [AtomicU64; 4],
    pub seq: &'a AtomicU32,
    pub rows: &'a mut [[u64; 16]; MAX_RUN_ROWS as usize],
    pub frame_starts: &'a mut [u64; MAX_RUN_ROWS as usize],
    pub count: &'a AtomicU32,
    pub stats: &'a [AtomicU64; 16],
}
fn add(value: &AtomicU64, by: u64) {
    value.fetch_add(by, SeqCst);
}
/// CompleteRunWork's totals and row (false at the run's limit: nothing written).
pub fn complete(w: &RunWork, r: Run<'_>) -> bool {
    let index = r.count.load(SeqCst);
    if index >= MAX_RUN_ROWS {
        return false;
    }
    for i in 0..2 {
        add(&r.instance_proofs[i], w.instance_proofs[i]);
    }
    for i in 0..4 {
        add(&r.shader_proofs[i], w.shader_proofs[i]);
    }
    for i in 0..5 {
        add(&r.skinning_stats[i], w.skin_stats[i]);
    }
    if w.skin_stats[5] != 0 {
        r.skinning_stats[5].store(w.skin_stats[5], SeqCst);
    }
    for i in 0..3 {
        add(&r.binding_checks[i], w.binding_stats[i]);
    }
    if w.binding_stats[3] != 0 {
        r.binding_checks[3].store(w.binding_stats[3], SeqCst);
    }
    add(&r.native_camera[0], u64::from(w.native_camera));
    add(&r.native_camera[1], w.native_camera_draws);
    for i in 0..4 {
        add(&r.camera_stage_masks[i], w.camera_stage_masks[i]);
    }
    let (n, o, t) = (&w.native, &w.optimized, &w.times);
    let row = [
        u64::from(w.frame),
        u64::from(w.final_stats[2]),
        u64::from(w.final_stats[3]),
        u64::from(n[1]),
        u64::from(n[2]),
        o[2],
        o[3],
        o[4],
        o[5],
        o[1],
        o[7],
        o[6],
        t[1].wrapping_sub(t[0]),
        t[2],
        t[4].wrapping_sub(t[3]),
        t[5],
    ];
    r.seq.fetch_add(1, SeqCst);
    r.rows[index as usize] = row;
    r.frame_starts[index as usize] = w.begin_qpc;
    r.count.store(index + 1, SeqCst);
    add(&r.stats[2], 1);
    add(&r.stats[if o[1] != 0 { 3 } else { 4 }], 1);
    add(&r.stats[9], u64::from(n[1]));
    add(&r.stats[10], u64::from(n[2]));
    add(&r.stats[11], o[2].wrapping_add(o[3]).wrapping_add(if o[1] != 0 { o[4] } else { o[5] }));
    add(&r.stats[12], o[6]);
    r.seq.fetch_add(1, Release);
    true
}
/// CompleteRunWork (true when written; false at the run's limit).
///
/// # Safety
/// `w` and the exports are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_complete_run(w: *const RunWork, e: *const RunExports) -> bool {
    // SAFETY: as documented.
    unsafe {
        let e = &*e;
        complete(
            &*w,
            Run {
                instance_proofs: &*e.instance_proofs,
                shader_proofs: &*e.shader_proofs,
                skinning_stats: &*e.skinning_stats,
                binding_checks: &*e.binding_checks,
                native_camera: &*e.native_camera,
                camera_stage_masks: &*e.camera_stage_masks,
                seq: &*e.seq,
                rows: &mut *e.rows,
                frame_starts: &mut *e.frame_starts,
                count: &*e.count,
                stats: &*e.stats,
            },
        )
    }
}

/// The material readback's slot stride.
pub const MATERIAL_SLOT: usize = 512;
/// SealMaterials' readback: each sealed row's payload (`logical` bytes at its 512-byte slot) appended to `blob`, its
/// offset and size in the row, and whether the row's hinted texture index (with the texture descriptor held) is the
/// payload's first word; the blob's bytes, or the mapping's failure.
pub fn seal_read(readback: &windows::Win32::Graphics::Direct3D12::ID3D12Resource, rows: &mut [[u64; 29]], sealed: u64, logical: usize, blob: &mut [u8]) -> Result<usize, i32> {
    use windows::Win32::Graphics::Direct3D12::D3D12_RANGE;
    let mut data = core::ptr::null_mut();
    // SAFETY: a live READBACK buffer of a slot per row.
    if let Err(error) = unsafe { readback.Map(0, Some(&D3D12_RANGE { Begin: 0, End: rows.len() * MATERIAL_SLOT }), Some(&mut data)) } {
        return Err(error.code().0);
    }
    let none = D3D12_RANGE { Begin: 0, End: 0 };
    if data.is_null() {
        // SAFETY: as above.
        unsafe { readback.Unmap(0, Some(&none)) };
        return Err(0x8000_4005u32 as i32);
    }
    let mut bytes = 0usize;
    for (i, row) in rows.iter_mut().enumerate() {
        if row[13] != sealed {
            continue;
        }
        row[15] = bytes as u64;
        row[16] = logical as u64;
        // SAFETY: the mapping holds a slot per row; the blob a payload per row.
        unsafe { core::ptr::copy_nonoverlapping((data as *const u8).add(i * MATERIAL_SLOT), blob.as_mut_ptr().add(bytes), logical) };
        let actual = u32::from_le_bytes(blob[bytes..bytes + 4].try_into().unwrap());
        row[28] = u64::from(row[27] & 1 != 0 && row[26] == u64::from(actual));
        bytes += logical;
    }
    // SAFETY: as above.
    unsafe { readback.Unmap(0, Some(&none)) };
    Ok(bytes)
}
/// SealMaterials' readback (S_OK with the blob's bytes, or the failure).
///
/// # Safety
/// A live readback; `rows` holds `count` rows and `blob` `count * logical` bytes; `bytes` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_seal_read(readback: *mut core::ffi::c_void, rows: *mut [u64; 29], count: usize, sealed: u32, logical: u32, blob: *mut u8, bytes: *mut usize) -> i32 {
    use windows::core::Interface;
    // SAFETY: as documented.
    let Some(readback) = (unsafe { windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&readback) }) else { return 0x8007_0057u32 as i32 };
    let (rows, blob) = if count == 0 { (&mut [][..], &mut [][..]) } else { unsafe { (core::slice::from_raw_parts_mut(rows, count), core::slice::from_raw_parts_mut(blob, count * logical as usize)) } };
    match seal_read(readback, rows, u64::from(sealed), logical as usize, blob) {
        Ok(n) => {
            unsafe { *bytes = n };
            0
        }
        Err(hr) => hr,
    }
}

/// Draw's PEER remainder: the instances of `instances` no run covers, in order, into `out` (room for runs + 1); the count.
pub fn remaining(runs: &[[u32; 2]], instances: u32, out: &mut [[u32; 2]]) -> usize {
    let (mut at, mut n) = (0u32, 0usize);
    let mut push = |first: u32, count: u32| {
        if let Some(slot) = out.get_mut(n) {
            *slot = [first, count];
            n += 1;
        }
    };
    for run in runs {
        if run[0] > at {
            push(at, run[0] - at);
        }
        at = run[0].wrapping_add(run[1]);
    }
    if at < instances {
        push(at, instances - at);
    }
    n
}
/// The PEER remainder.
///
/// # Safety
/// `runs` holds `count` runs and `out` room for `count + 1`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_remaining(runs: *const [u32; 2], count: usize, instances: u32, out: *mut [u32; 2]) -> usize {
    let runs = if runs.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(runs, count) } };
    // SAFETY: as documented.
    let out = unsafe { core::slice::from_raw_parts_mut(out, count + 1) };
    remaining(runs, instances, out)
}

#[cfg(test)]
mod tests {
    //! The totals, the row and the limit; the material readback.
    use super::*;

    #[test]
    fn peer_remainder() {
        let mut out = [[0u32; 2]; 4];
        assert_eq!(super::remaining(&[[2, 3], [5, 1], [9, 2]], 12, &mut out), 3);
        assert_eq!(out[..3], [[0, 2], [6, 3], [11, 1]], "the gaps before, between and after the runs");
        assert_eq!(super::remaining(&[[0, 12]], 12, &mut out), 0, "all covered");
        assert_eq!(super::remaining(&[], 4, &mut out), 1);
        assert_eq!(out[0], [0, 4]);
    }

    #[test]
    fn seals_materials() {
        use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
        use windows::Win32::Graphics::Direct3D12::*;
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let readback = crate::readback_buffers::buffer(device.as_ref().unwrap(), None, 3 * 512, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST).unwrap();
        let mut mapped = core::ptr::null_mut();
        unsafe { readback.Map(0, None, Some(&mut mapped)) }.unwrap();
        for slot in 0..3usize {
            let bytes = unsafe { core::slice::from_raw_parts_mut((mapped as *mut u8).add(slot * 512), 512) };
            bytes.fill(slot as u8 + 1);
            bytes[..4].copy_from_slice(&(slot as u32 * 10).to_le_bytes());
        }
        unsafe { readback.Unmap(0, None) };
        let mut rows = [[0u64; 29]; 3];
        for (slot, row) in rows.iter_mut().enumerate() {
            row[13] = if slot == 1 { 2 } else { 4 };
            row[27] = 1;
            row[26] = slot as u64 * 10;
        }
        rows[2][27] = 0;
        let mut blob = [0u8; 3 * 292];
        assert_eq!(seal_read(&readback, &mut rows, 4, 292, &mut blob), Ok(584), "the sealed rows only");
        assert_eq!((rows[0][15], rows[0][16], rows[2][15], rows[1][16]), (0, 292, 292, 0));
        assert_eq!((rows[0][28], rows[2][28]), (1, 0), "the hinted index confirmed only with the texture descriptor");
        assert_eq!((blob[4], blob[292 + 4], &blob[292..296]), (1, 3, &20u32.to_le_bytes()[..]));
    }

    #[test]
    fn completes_a_frame() {
        let atomics = |n: usize| -> Vec<AtomicU64> { (0..n).map(|_| AtomicU64::new(0)).collect() };
        let (instance, shader, skin, binding, camera, masks, stats) = (atomics(2), atomics(4), atomics(6), atomics(4), atomics(2), atomics(4), atomics(16));
        let (seq, count) = (AtomicU32::new(0), AtomicU32::new(0));
        let mut rows = Box::new([[0u64; 16]; MAX_RUN_ROWS as usize]);
        let mut starts = Box::new([0u64; MAX_RUN_ROWS as usize]);
        skin[5].store(77, SeqCst);
        let mut w = RunWork {
            frame: 9,
            optimized: [0, 1, 10, 20, 30, 40, 50, 60],
            times: [100, 150, 7, 200, 260, 8],
            instance_proofs: [1, 2],
            shader_proofs: [1, 1, 1, 1],
            skin_stats: [1, 1, 1, 1, 1, 0],
            binding_stats: [2, 2, 2, 5],
            camera_stage_masks: [1, 0, 1, 0],
            native_camera_draws: 3,
            begin_qpc: 1234,
            native_camera: true,
            ..RunWork::default()
        };
        w.final_stats[2] = 11;
        w.final_stats[3] = 12;
        w.native[1] = 4;
        w.native[2] = 5;
        let run = |w: &RunWork, rows: &mut [[u64; 16]; MAX_RUN_ROWS as usize], starts: &mut [u64; MAX_RUN_ROWS as usize]| {
            complete(
                w,
                Run {
                    instance_proofs: instance[..].try_into().unwrap(),
                    shader_proofs: shader[..].try_into().unwrap(),
                    skinning_stats: skin[..].try_into().unwrap(),
                    binding_checks: binding[..].try_into().unwrap(),
                    native_camera: camera[..].try_into().unwrap(),
                    camera_stage_masks: masks[..].try_into().unwrap(),
                    seq: &seq,
                    rows,
                    frame_starts: starts,
                    count: &count,
                    stats: stats[..].try_into().unwrap(),
                },
            )
        };
        assert!(run(&w, &mut rows, &mut starts));
        assert_eq!(rows[0], [9, 11, 12, 4, 5, 10, 20, 30, 40, 1, 60, 50, 50, 7, 60, 8]);
        assert_eq!((starts[0], count.load(SeqCst), seq.load(SeqCst)), (1234, 1, 2), "the row under the run's sequence");
        assert_eq!((skin[5].load(SeqCst), binding[3].load(SeqCst), binding[0].load(SeqCst)), (77, 5, 2), "an unset failure word keeps the run's");
        assert_eq!((camera[0].load(SeqCst), camera[1].load(SeqCst), instance[1].load(SeqCst)), (1, 3, 2));
        assert_eq!((stats[2].load(SeqCst), stats[3].load(SeqCst), stats[4].load(SeqCst), stats[9].load(SeqCst), stats[11].load(SeqCst), stats[12].load(SeqCst)), (1, 1, 0, 4, 60, 50));
        w.optimized[1] = 0;
        assert!(run(&w, &mut rows, &mut starts));
        assert_eq!((stats[4].load(SeqCst), stats[11].load(SeqCst), count.load(SeqCst)), (1, 60 + 70, 2), "a reference frame counts the skipped draws");
        count.store(MAX_RUN_ROWS, SeqCst);
        assert!(!run(&w, &mut rows, &mut starts), "the run's limit");
        assert_eq!(seq.load(SeqCst), 4, "nothing written at the limit");
    }
}
