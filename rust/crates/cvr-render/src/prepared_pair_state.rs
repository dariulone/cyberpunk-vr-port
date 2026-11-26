//! The prepared pair's exported state (formerly definitions in src/Render/NativePreparedPair.cpp): the pair's request,
//! state, selection and input fault, its native export and actual membership switches, and the published stats and
//! tables -- the pair's candidates, state mismatches, GPU, colour, depth, mesh, VRS and residency results, the batch's
//! sources, plan and descriptors, and the current sequence's baseline, native exit, rate, attribution and acceptance --
//! defined on the Rust side and exported from rust/exported-data.txt; the C++ pair declares them in its header, the
//! tools read them by name. And `cvr_npp_reset_current`: StartCurrentSequence's reset of them for a new current
//! sequence.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicU32, Ordering::SeqCst};

#[no_mangle]
pub static CyberpunkVR_PreparedPairRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedPairState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedPairSelection: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedPairSelectionCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedPairInputFault: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedNativeExportEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedActualMembershipEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparedActualMembershipStats: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_PreparedActualMembershipRows: [u64; 16384] = [0; 16384];
#[no_mangle]
pub static mut CyberpunkVR_PreparedActualMembershipCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparedActualMembershipRowBytes: u32 = 256;
#[no_mangle]
pub static mut CyberpunkVR_PreparedNativeExport: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairCandidates: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairStateMismatch: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairVrs: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairVrsGpu: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairGpu: [u64; 36] = [0; 36];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairColor: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairDepthState: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairMeshStats: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairMeshResults: [u64; 512] = [0; 512];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairInputStats: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparedBatchStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparedBatchSources: [u64; 1024] = [0; 1024];
#[no_mangle]
pub static mut CyberpunkVR_PreparedBatchPlan: [u64; 4096] = [0; 4096];
#[no_mangle]
pub static mut CyberpunkVR_PreparedBatchSourceCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparedBatchPlanCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparedBatchDescriptors: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_PreparedPairResidency: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparedCurrent: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparedBaseline: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_PreparedNativeExit: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_PreparedNativeExitSamples: [u64; 160] = [0; 160];
#[no_mangle]
pub static mut CyberpunkVR_PreparedNativeRate: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_PreparedVrsAttribution: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_PreparedNativeAcceptance: [u64; 8] = [0; 8];

/// StartCurrentSequence's reset for a sequence of `count` sources at `frame`: whether any source exits natively, whether
/// the first refines VRS.
#[no_mangle]
pub extern "C" fn cvr_npp_reset_current(frame: u32, native_exit: bool, allow_vrs_refinement: bool, count: usize) {
    // SAFETY: the pair's exports, written under its mutex as the .cpp wrote them.
    unsafe {
        CyberpunkVR_PreparedPairStats = [0; 16];
        CyberpunkVR_PreparedCurrent = [0; 16];
        CyberpunkVR_PreparedNativeExport = [0; 24];
        CyberpunkVR_PreparedNativeExport[0] = u64::from(CyberpunkVR_PreparedNativeExportEnable.load(SeqCst) != 0);
        CyberpunkVR_PreparedNativeExport[12] = u64::from(frame);
        CyberpunkVR_PreparedPairCandidates = [0; 16];
        CyberpunkVR_PreparedPairStateMismatch = [0; 24];
        CyberpunkVR_PreparedPairGpu = [0; 36];
        CyberpunkVR_PreparedPairColor = [0; 12];
        CyberpunkVR_PreparedPairDepthState = [0; 16];
        CyberpunkVR_PreparedPairMeshStats = [0; 12];
        CyberpunkVR_PreparedPairMeshResults = [0; 512];
        CyberpunkVR_PreparedPairVrs = [0; 8];
        CyberpunkVR_PreparedPairVrsGpu = [0; 4];
        CyberpunkVR_PreparedPairInputStats = [0; 8];
        CyberpunkVR_PreparedPairResidency = [0; 8];
        CyberpunkVR_PreparedBatchStats = [0; 16];
        CyberpunkVR_PreparedBaseline = [0; 24];
        CyberpunkVR_PreparedNativeExit = [0; 24];
        CyberpunkVR_PreparedNativeExit[23] = 1;
        CyberpunkVR_PreparedNativeExit[0] = u64::from(native_exit);
        CyberpunkVR_PreparedNativeExitSamples = [0; 160];
        CyberpunkVR_PreparedNativeRate = [0; 16];
        CyberpunkVR_PreparedVrsAttribution = [0; 12];
        CyberpunkVR_PreparedNativeRate[0] = CyberpunkVR_PreparedNativeExit[0];
        CyberpunkVR_PreparedNativeAcceptance = [0; 8];
        CyberpunkVR_PreparedNativeAcceptance[0] = u64::from(allow_vrs_refinement);
        CyberpunkVR_PreparedBatchDescriptors = [0; 4];
        CyberpunkVR_PreparedBatchSourceCount = 0;
        CyberpunkVR_PreparedBatchPlanCount = 0;
        CyberpunkVR_PreparedCurrent[0] = 1;
        CyberpunkVR_PreparedCurrent[1] = u64::from(frame);
        CyberpunkVR_PreparedCurrent[8] = count as u64;
    }
    CyberpunkVR_PreparedPairSelectionCount.store(0, SeqCst);
    CyberpunkVR_PreparedPairState.store(0, SeqCst);
}

/// StartInternal's first resets: the export row (requested, frame), the current row (current, frame, sources) and, for a
/// current sequence, the pair's statistics.
#[no_mangle]
pub extern "C" fn cvr_npp_reset_start(frame: u32, current: bool, export_requested: bool, sources: usize) {
    // SAFETY: the pair's exports, written under its mutex as the .cpp wrote them.
    unsafe {
        CyberpunkVR_PreparedNativeExport = [0; 24];
        CyberpunkVR_PreparedNativeExport[0] = u64::from(export_requested);
        CyberpunkVR_PreparedNativeExport[12] = u64::from(frame);
        CyberpunkVR_PreparedCurrent = [0; 16];
        CyberpunkVR_PreparedCurrent[0] = u64::from(current);
        CyberpunkVR_PreparedCurrent[1] = u64::from(frame);
        CyberpunkVR_PreparedCurrent[8] = sources as u64;
        if current {
            CyberpunkVR_PreparedPairStats = [0; 16];
        }
    }
}
/// StartInternal's output resets: the rates, queries, colors, depth state, mesh, inputs, residency, baseline and native
/// exit rows; the native exit mode and the refinement policy.
#[no_mangle]
pub extern "C" fn cvr_npp_reset_outputs(native_exit: bool, allow_vrs_refinement: bool) {
    // SAFETY: as above.
    unsafe {
        CyberpunkVR_PreparedPairVrs = [0; 8];
        CyberpunkVR_PreparedPairVrsGpu = [0; 4];
        CyberpunkVR_PreparedPairGpu = [0; 36];
        CyberpunkVR_PreparedPairColor = [0; 12];
        CyberpunkVR_PreparedPairDepthState = [0; 16];
        CyberpunkVR_PreparedPairMeshStats = [0; 12];
        CyberpunkVR_PreparedPairMeshResults = [0; 512];
        CyberpunkVR_PreparedPairInputStats = [0; 8];
        CyberpunkVR_PreparedPairResidency = [0; 8];
        CyberpunkVR_PreparedBaseline = [0; 24];
        CyberpunkVR_PreparedNativeExit = [0; 24];
        CyberpunkVR_PreparedNativeExit[23] = 1;
        CyberpunkVR_PreparedNativeExitSamples = [0; 160];
        CyberpunkVR_PreparedNativeRate = [0; 16];
        CyberpunkVR_PreparedVrsAttribution = [0; 12];
        CyberpunkVR_PreparedNativeAcceptance = [0; 8];
        CyberpunkVR_PreparedNativeAcceptance[0] = u64::from(allow_vrs_refinement);
        CyberpunkVR_PreparedNativeExit[0] = u64::from(native_exit);
        CyberpunkVR_PreparedNativeRate[0] = u64::from(native_exit);
    }
}
/// StartInternal's batch resets.
#[no_mangle]
pub extern "C" fn cvr_npp_reset_batch() {
    // SAFETY: as above.
    unsafe {
        CyberpunkVR_PreparedBatchStats = [0; 16];
        CyberpunkVR_PreparedBatchSourceCount = 0;
        CyberpunkVR_PreparedBatchPlanCount = 0;
        CyberpunkVR_PreparedBatchDescriptors = [0; 4];
    }
}
/// A pair request's resets (NativeRetainedGeometry.cpp's FrameBoundary): the selection, candidates, mismatches, queries,
/// colors, depth state, mesh, rates and batch rows.
#[no_mangle]
pub extern "C" fn cvr_npp_reset_request() {
    CyberpunkVR_PreparedPairSelectionCount.store(0, SeqCst);
    // SAFETY: as above.
    unsafe {
        CyberpunkVR_PreparedPairCandidates = [0; 16];
        CyberpunkVR_PreparedPairStateMismatch = [0; 24];
        CyberpunkVR_PreparedPairGpu = [0; 36];
        CyberpunkVR_PreparedPairColor = [0; 12];
        CyberpunkVR_PreparedPairDepthState = [0; 16];
        CyberpunkVR_PreparedPairMeshStats = [0; 12];
        CyberpunkVR_PreparedPairMeshResults = [0; 512];
        CyberpunkVR_PreparedPairVrs = [0; 8];
        CyberpunkVR_PreparedBatchStats = [0; 16];
        CyberpunkVR_PreparedBatchSourceCount = 0;
        CyberpunkVR_PreparedBatchPlanCount = 0;
    }
}

#[cfg(test)]
mod tests {
    //! The reset against hand-set values.
    use super::*;
    use core::sync::atomic::Ordering::Relaxed;

    #[test]
    fn resets() {
        CyberpunkVR_PreparedNativeExportEnable.store(1, Relaxed);
        CyberpunkVR_PreparedPairState.store(5, Relaxed);
        CyberpunkVR_PreparedPairSelectionCount.store(3, Relaxed);
        // SAFETY: only this test touches the exports.
        unsafe {
            CyberpunkVR_PreparedPairMeshResults = [9; 512];
            CyberpunkVR_PreparedBatchPlanCount = 4;
            CyberpunkVR_PreparedActualMembershipCount = 6;
        }
        cvr_npp_reset_current(77, true, false, 12);
        // SAFETY: as above; read by value.
        let (export, current, exit, rate, acceptance, mesh, plan, membership) = unsafe {
            (
                CyberpunkVR_PreparedNativeExport,
                CyberpunkVR_PreparedCurrent,
                CyberpunkVR_PreparedNativeExit,
                CyberpunkVR_PreparedNativeRate,
                CyberpunkVR_PreparedNativeAcceptance,
                CyberpunkVR_PreparedPairMeshResults,
                CyberpunkVR_PreparedBatchPlanCount,
                CyberpunkVR_PreparedActualMembershipCount,
            )
        };
        assert_eq!((export[0], export[12], export[1]), (1, 77, 0));
        assert_eq!((current[0], current[1], current[8], current[2]), (1, 77, 12, 0));
        assert_eq!((exit[0], exit[23], rate[0], acceptance[0]), (1, 1, 1, 0));
        assert_eq!((mesh, plan), ([0; 512], 0));
        assert_eq!((CyberpunkVR_PreparedPairState.load(Relaxed), CyberpunkVR_PreparedPairSelectionCount.load(Relaxed)), (0, 0));
        // The membership is ResetMembership's, not this reset's.
        assert_eq!(membership, 6);
        // SAFETY: as above.
        assert_eq!(unsafe { CyberpunkVR_PreparedActualMembershipRowBytes }, 256);
        // StartInternal's resets, each against values set beforehand.
        // SAFETY: as above.
        unsafe {
            CyberpunkVR_PreparedPairStats = [5; 16];
            CyberpunkVR_PreparedNativeExport = [5; 24];
        }
        cvr_npp_reset_start(9, false, true, 3);
        // SAFETY: as above.
        let (export, current, stats) = unsafe { (CyberpunkVR_PreparedNativeExport, CyberpunkVR_PreparedCurrent, CyberpunkVR_PreparedPairStats) };
        assert_eq!((export[0], export[12], export[1], current[0], current[1], current[8], stats[0]), (1, 9, 0, 0, 9, 3, 5), "no current sequence keeps the statistics");
        cvr_npp_reset_start(9, true, false, 3);
        // SAFETY: as above.
        assert_eq!(unsafe { (CyberpunkVR_PreparedPairStats[0], CyberpunkVR_PreparedNativeExport[0], CyberpunkVR_PreparedCurrent[0]) }, (0, 0, 1));
        // SAFETY: as above.
        unsafe {
            CyberpunkVR_PreparedNativeExitSamples = [7; 160];
            CyberpunkVR_PreparedPairResidency = [7; 8];
            CyberpunkVR_PreparedBatchDescriptors = [7; 4];
            CyberpunkVR_PreparedBatchSourceCount = 7;
        }
        cvr_npp_reset_outputs(true, true);
        // SAFETY: as above.
        let (exit, rate, acceptance, samples, residency) = unsafe {
            (CyberpunkVR_PreparedNativeExit, CyberpunkVR_PreparedNativeRate, CyberpunkVR_PreparedNativeAcceptance, CyberpunkVR_PreparedNativeExitSamples, CyberpunkVR_PreparedPairResidency)
        };
        assert_eq!((exit[0], exit[23], rate[0], acceptance[0], samples[0], residency[0]), (1, 1, 1, 1, 0, 0));
        // SAFETY: as above.
        assert_eq!(unsafe { (CyberpunkVR_PreparedBatchDescriptors[0], CyberpunkVR_PreparedBatchSourceCount) }, (7, 7), "the batch reset is separate");
        cvr_npp_reset_batch();
        // SAFETY: as above.
        assert_eq!(unsafe { (CyberpunkVR_PreparedBatchDescriptors[0], CyberpunkVR_PreparedBatchSourceCount) }, (0, 0));
        CyberpunkVR_PreparedPairSelectionCount.store(4, Relaxed);
        // SAFETY: as above.
        unsafe {
            CyberpunkVR_PreparedPairCandidates = [8; 16];
            CyberpunkVR_PreparedPairStats = [8; 16];
        }
        cvr_npp_reset_request();
        // SAFETY: as above.
        let (candidates, stats) = unsafe { (CyberpunkVR_PreparedPairCandidates, CyberpunkVR_PreparedPairStats) };
        assert_eq!((CyberpunkVR_PreparedPairSelectionCount.load(Relaxed), candidates[0], stats[0]), (0, 0, 8), "a request keeps the statistics");
    }
}
