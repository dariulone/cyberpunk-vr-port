//! The exported state of the native early depth and the CPU collector probe (formerly definitions in
//! src/Render/NativeEarlyDepth.cpp and src/Stereo/CpuCollectorProbe.cpp), defined on the Rust side and exported from
//! rust/exported-data.txt; the C++ sources keep declaring them, the tools read them by name. The exports whose type
//! is a C++ record (a depth comparison, the peer's changed bounds) stay the .cpp's.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicU32, AtomicU64};

// ---- src/Render/NativeEarlyDepth.cpp: the early depth: its request and state, the sources, coverage, masks and failures, the optimized and native counts, the GPU times, the instance rows, the run's state, stats, CPU times and rows, the native camera, the skinning, the recording and the material capture

#[no_mangle]
pub static CyberpunkVR_EarlyDepthRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthStats: [AtomicU32; 16] = [const { AtomicU32::new(0) }; 16];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthResult: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthSourceCapacity: u32 = 1024;
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthInstanceCapacity: u32 = 4096;
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthSources: [[u64; 2]; 1024] = [[0; 2]; 1024];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthCoverage: [[u32; 2]; 1024] = [[0; 2]; 1024];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthMasks: [[u32; 2]; 1024] = [[0; 2]; 1024];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthMaskStats: [u32; 14] = [0; 14];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthExtraCoverage: [[u32; 2]; 1024] = [[0; 2]; 1024];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthFailureData: [u64; 6] = [0; 6];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthWholeTarget: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRejectCommit: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthOptimized: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthNativeCounts: [u32; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthNativeDifferences: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthGpuTimes: [u64; 6] = [0; 6];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthGpuInstances: [u64; 4] = [0; 4];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthInstanceReadScope: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthInstanceRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthInstanceBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthInstanceCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthInstanceBytes: u32 = 0;
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunStop: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunStats: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunArenas: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunDeferred: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunInstanceProofs: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunCpu: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthRunCpuFrequency: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunGeometryChecks: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthRunRows: [[u64; 16]; 1200] = [[0; 16]; 1200];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthRunFrameStarts: [u64; 1200] = [0; 1200];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthNativeCamera: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthNativeCameraStats: [u64; 3] = [0; 3];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunNativeCamera: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthCameraStages: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthCameraStageMasks: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthCameraBindingInfo: [u64; 16] = [0; 16];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunCameraStageMasks: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthSkinning: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthSkinningStats: [u64; 6] = [0; 6];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthShaderProofs: [u64; 4] = [0; 4];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunSkinningStats: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthRunShaderProofs: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthBindingChecks: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthRecording: [u64; 6] = [0; 6];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthRecordingDetails: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthMaterialBindings: [u64; 12] = [0; 12];
#[no_mangle]
pub static CyberpunkVR_EarlyDepthMaterialCapture: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthMaterialRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthMaterialBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthMaterialCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_EarlyDepthMaterialBytes: u32 = 0;

// ---- src/Stereo/CpuCollectorProbe.cpp: the CPU collector probe: its request, state and results, the query, fine and scene stats, and the peer's changes

#[no_mangle]
pub static CyberpunkVR_CpuCollectorRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_CpuCollectorState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_CpuCollectorCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorResult: [u32; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorQueryResult: [u32; 8] = [0; 8];
#[no_mangle]
pub static CyberpunkVR_CpuCollectorQueryCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_CpuCollectorFineCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorFineResult: [u32; 12] = [0; 12];
#[no_mangle]
pub static CyberpunkVR_CpuCollectorSceneCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorSceneResult: [u32; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorSceneControl: [u32; 8] = [0; 8];
#[no_mangle]
pub static CyberpunkVR_CpuCollectorPeerCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorPeerResult: [u32; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorPeerQuery: [u32; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorPeerFine: [u32; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorPeerScene: [u32; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorPeerControl: [u32; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorPeerPlanes: [f32; 64] = [0.0; 64];
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorPeerInputDiff: [u32; 10] = [0; 10];
#[no_mangle]
pub static mut CyberpunkVR_CpuCollectorPeerChangesCount: u32 = 0;

/// NativeEarlyDepth.cpp's ClearOutput, for the exports above and the four the .cpp still defines (the material
/// descriptors, the alpha, skinned-alpha and alpha-proof stats): every table and stat zeroed, the result S_FALSE (1),
/// the state 1 (cleared). The .cpp keeps its own material watch and the depth comparison record.
///
/// # Safety
/// The four pointers are the .cpp's live exports.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_clear_output(descriptors: *mut u64, alpha: *mut [u64; 6], skinned_alpha: *mut [u64; 6], alpha_proofs: *mut [u64; 6]) {
    use core::sync::atomic::Ordering::SeqCst;
    // SAFETY: the early depth's exports, written as the .cpp wrote them.
    unsafe {
        CyberpunkVR_EarlyDepthMaterialRows = 0;
        CyberpunkVR_EarlyDepthMaterialBlob = 0;
        CyberpunkVR_EarlyDepthMaterialCount = 0;
        CyberpunkVR_EarlyDepthMaterialBytes = 0;
        *descriptors = 0;
        for value in &CyberpunkVR_EarlyDepthStats {
            value.store(0, SeqCst);
        }
        CyberpunkVR_EarlyDepthSources = [[0; 2]; 1024];
        CyberpunkVR_EarlyDepthCoverage = [[0; 2]; 1024];
        CyberpunkVR_EarlyDepthMasks = [[0; 2]; 1024];
        CyberpunkVR_EarlyDepthMaskStats = [0; 14];
        CyberpunkVR_EarlyDepthExtraCoverage = [[0; 2]; 1024];
        CyberpunkVR_EarlyDepthFailureData = [0; 6];
        CyberpunkVR_EarlyDepthNativeCounts = [0; 8];
        CyberpunkVR_EarlyDepthNativeDifferences = [0; 4];
        CyberpunkVR_EarlyDepthOptimized = [0; 8];
        CyberpunkVR_EarlyDepthGpuTimes = [0; 6];
        CyberpunkVR_EarlyDepthGpuInstances = [0; 4];
        CyberpunkVR_EarlyDepthNativeCameraStats = [0; 3];
        CyberpunkVR_EarlyDepthCameraStageMasks = [0; 4];
        CyberpunkVR_EarlyDepthCameraBindingInfo = [0; 16];
        CyberpunkVR_EarlyDepthSkinningStats = [0; 6];
        CyberpunkVR_EarlyDepthShaderProofs = [0; 4];
        CyberpunkVR_EarlyDepthBindingChecks = [0; 4];
        CyberpunkVR_EarlyDepthMaterialBindings = [0; 12];
        *alpha = [0; 6];
        *skinned_alpha = [0; 6];
        *alpha_proofs = [0; 6];
        CyberpunkVR_EarlyDepthRecording = [0; 6];
        CyberpunkVR_EarlyDepthRecordingDetails = [0; 8];
        CyberpunkVR_EarlyDepthResult = 1; // S_FALSE
    }
    CyberpunkVR_EarlyDepthState.store(1, SeqCst);
}
/// NativeEarlyDepth.cpp's FrameBoundary at a run's start: the run's row count, stats, arenas, deferrals, instance and
/// shader proofs, native camera, camera stage masks, skinning stats, CPU times and geometry checks zeroed, the CPU
/// frequency taken, the stop flag and the state cleared (the binding checks and the live failure stay the .cpp's).
#[no_mangle]
pub extern "C" fn cvr_ned_reset_run() {
    use core::sync::atomic::Ordering::SeqCst;
    CyberpunkVR_EarlyDepthRunCount.store(0, SeqCst);
    for group in [&CyberpunkVR_EarlyDepthRunStats[..], &CyberpunkVR_EarlyDepthRunArenas, &CyberpunkVR_EarlyDepthRunDeferred, &CyberpunkVR_EarlyDepthRunInstanceProofs,
        &CyberpunkVR_EarlyDepthRunNativeCamera, &CyberpunkVR_EarlyDepthRunCameraStageMasks, &CyberpunkVR_EarlyDepthRunSkinningStats, &CyberpunkVR_EarlyDepthRunShaderProofs,
        &CyberpunkVR_EarlyDepthRunCpu]
    {
        for value in group {
            value.store(0, SeqCst);
        }
    }
    for value in &CyberpunkVR_EarlyDepthRunGeometryChecks {
        value.store(0, SeqCst);
    }
    let mut frequency = 0i64;
    // SAFETY: a local output; the frequency export written as the .cpp wrote it.
    unsafe {
        let _ = windows::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency);
        CyberpunkVR_EarlyDepthRunCpuFrequency = frequency as u64;
    }
    CyberpunkVR_EarlyDepthRunStop.store(0, SeqCst);
    CyberpunkVR_EarlyDepthState.store(0, SeqCst);
}
/// Draw's MAIN rows of one item: its source sequence, coverage and extra coverage, the extra instances counted and the
/// covered remainder, each mask counted (3 both eyes, else MAIN only) and each run of equal masks.
pub struct MaskRows<'a> {
    pub sources: &'a mut [[u64; 2]],
    pub coverage: &'a mut [[u32; 2]],
    pub extra_coverage: &'a mut [[u32; 2]],
    pub masks: &'a mut [[u32; 2]],
    pub stats: &'a mut [u32; 14],
}
/// The rows of `item` (nothing past the tables).
pub fn main_masks(rows: MaskRows<'_>, item: usize, sequence: u64, instances: u32, extra: u32, masks: &[u8]) {
    let (Some(source), Some(coverage), Some(extra_coverage), Some(counts)) =
        (rows.sources.get_mut(item), rows.coverage.get_mut(item), rows.extra_coverage.get_mut(item), rows.masks.get_mut(item))
    else {
        return;
    };
    source[0] = sequence;
    coverage[1] = instances;
    extra_coverage[1] = extra;
    let stats = rows.stats;
    stats[10] = stats[10].wrapping_add(extra);
    stats[11] = stats[11].wrapping_add(u32::from(extra != 0));
    stats[13] = stats[12].wrapping_sub(stats[10]);
    for (i, &mask) in masks.iter().enumerate() {
        let at = if mask == 3 { 0 } else { 1 };
        counts[at] = counts[at].wrapping_add(1);
        stats[4 + at] = stats[4 + at].wrapping_add(1);
        if i == 0 || mask != masks[i - 1] {
            stats[6] = stats[6].wrapping_add(1);
        }
    }
}
/// Draw's MAIN rows of `item` into the exports.
///
/// # Safety
/// `masks` holds `count` bytes; the early depth's lock is held.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_main_masks(item: usize, sequence: u64, instances: u32, extra: u32, masks: *const u8, count: usize) {
    let masks = if masks.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(masks, count) } };
    // SAFETY: the exports, under the .cpp's lock.
    let rows = unsafe {
        MaskRows {
            sources: &mut *core::ptr::addr_of_mut!(CyberpunkVR_EarlyDepthSources),
            coverage: &mut *core::ptr::addr_of_mut!(CyberpunkVR_EarlyDepthCoverage),
            extra_coverage: &mut *core::ptr::addr_of_mut!(CyberpunkVR_EarlyDepthExtraCoverage),
            masks: &mut *core::ptr::addr_of_mut!(CyberpunkVR_EarlyDepthMasks),
            stats: &mut *core::ptr::addr_of_mut!(CyberpunkVR_EarlyDepthMaskStats),
        }
    };
    main_masks(rows, item, sequence, instances, extra, masks);
}
#[cfg(test)]
mod tests {
    //! The initial values the .cpp's definitions gave.
    use super::*;
    use core::sync::atomic::Ordering::Relaxed;

    #[test]
    fn main_mask_rows() {
        let (mut sources, mut coverage, mut extra, mut masks, mut stats) = ([[0u64; 2]; 4], [[0u32; 2]; 4], [[0u32; 2]; 4], [[0u32; 2]; 4], [0u32; 14]);
        stats[12] = 10;
        let rows = MaskRows { sources: &mut sources, coverage: &mut coverage, extra_coverage: &mut extra, masks: &mut masks, stats: &mut stats };
        main_masks(rows, 2, 77, 5, 2, &[3, 3, 1, 1, 3, 2, 2]);
        assert_eq!((sources[2][0], coverage[2][1], extra[2][1]), (77, 5, 2));
        assert_eq!(masks[2], [3, 4], "3 both eyes, the others MAIN only");
        assert_eq!((stats[4], stats[5], stats[6], stats[10], stats[11], stats[13]), (3, 4, 4, 2, 1, 8), "four runs; the remainder");
        let rows = MaskRows { sources: &mut sources, coverage: &mut coverage, extra_coverage: &mut extra, masks: &mut masks, stats: &mut stats };
        main_masks(rows, 9, 1, 1, 1, &[3]);
        assert_eq!(stats[10], 2, "nothing past the tables");
    }
    #[test]
    fn initial_values() {
        assert_eq!(CyberpunkVR_EarlyDepthInstanceReadScope.load(Relaxed), 1);
        assert_eq!(CyberpunkVR_EarlyDepthCameraStages.load(Relaxed), 1);
        // SAFETY: read by value; no test writes them.
        let (sources, coverage, capacity, instances) = unsafe {
            { let (sources, coverage) = (CyberpunkVR_EarlyDepthSources, CyberpunkVR_EarlyDepthCoverage); (sources.len(), coverage[0].len(), CyberpunkVR_EarlyDepthSourceCapacity, CyberpunkVR_EarlyDepthInstanceCapacity) }
        };
        assert_eq!((sources, coverage, capacity, instances), (1024, 2, 1024, 4096));
    }
    #[test]
    fn run_start_resets() {
        CyberpunkVR_EarlyDepthRunStats[3].store(9, Relaxed);
        CyberpunkVR_EarlyDepthRunCpu[9].store(4, Relaxed);
        CyberpunkVR_EarlyDepthRunGeometryChecks[1].store(2, Relaxed);
        CyberpunkVR_EarlyDepthRunCount.store(5, Relaxed);
        CyberpunkVR_EarlyDepthRunStop.store(1, Relaxed);
        cvr_ned_reset_run();
        assert_eq!(
            (CyberpunkVR_EarlyDepthRunStats[3].load(Relaxed), CyberpunkVR_EarlyDepthRunCpu[9].load(Relaxed), CyberpunkVR_EarlyDepthRunGeometryChecks[1].load(Relaxed)),
            (0, 0, 0)
        );
        assert_eq!((CyberpunkVR_EarlyDepthRunCount.load(Relaxed), CyberpunkVR_EarlyDepthRunStop.load(Relaxed)), (0, 0));
        // SAFETY: read by value.
        assert!(unsafe { CyberpunkVR_EarlyDepthRunCpuFrequency } != 0, "the CPU frequency taken");
    }
    #[test]
    fn clears() {
        // SAFETY: only this test writes these.
        unsafe {
            CyberpunkVR_EarlyDepthMasks[5][1] = 9;
            CyberpunkVR_EarlyDepthResult = -7;
            CyberpunkVR_EarlyDepthMaterialCount = 3;
        }
        CyberpunkVR_EarlyDepthStats[2].store(4, Relaxed);
        let (mut descriptors, mut a, mut s, mut p) = (5u64, [1u64; 6], [2u64; 6], [3u64; 6]);
        // SAFETY: local stand-ins for the .cpp's four exports.
        unsafe { cvr_ned_clear_output(&mut descriptors, &mut a, &mut s, &mut p) };
        // SAFETY: read by value.
        let (mask, result, count) = unsafe { (CyberpunkVR_EarlyDepthMasks[5][1], CyberpunkVR_EarlyDepthResult, CyberpunkVR_EarlyDepthMaterialCount) };
        assert_eq!((mask, result, count, descriptors, a, s, p), (0, 1, 0, 0, [0; 6], [0; 6], [0; 6]));
        assert_eq!((CyberpunkVR_EarlyDepthStats[2].load(Relaxed), CyberpunkVR_EarlyDepthState.load(Relaxed)), (0, 1));
    }
}
