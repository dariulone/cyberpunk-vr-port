//! The exported state of five native stereo sources (formerly definitions in src/Render/RegisteredConstantUpload.cpp,
//! NativePreparation.cpp, NativeConstantCapture.cpp, StereoGpuProbe.cpp and NativeStereoFamilies.cpp), defined on the
//! Rust side and exported from rust/exported-data.txt; the C++ sources keep declaring them in their headers, the tools
//! read them by name. The exports whose type is a C++ record (a depth comparison, a probe difference, a family record)
//! stay the .cpp's.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicU32, AtomicU64};

// ---- src/Render/RegisteredConstantUpload.cpp: the registered constant upload: its intent switch and cache stats, the registered copies' counts, reads, timeline, signals and trace

#[no_mangle]
pub static CyberpunkVR_RegisteredIntentEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RegisteredIntentCacheStats: [u64; 10] = [0; 10];
#[no_mangle]
pub static mut CyberpunkVR_RegisteredStructuredIntentStats: [u64; 2] = [0; 2];
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyRejected: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyChanged: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyUnscoped: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyPendingReads: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyOrderRejected: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyReads: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyTimelineResults: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyTimelineStats: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];
#[no_mangle]
pub static CyberpunkVR_RegisteredCopySignals: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyWaits: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyCpuSignals: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyTraceSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyTraceCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RegisteredCopyTraceOverflow: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RegisteredCopyTraceData: u64 = 0;

// ---- src/Render/NativePreparation.cpp: the native preparation: its request, state and records, the pair's request, state, data, eligibility and root, the graph parents, and the run's state, failures, depth and eye copies

#[no_mangle]
pub static CyberpunkVR_PreparationRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationPacketCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationRecords: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationPackets: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_PreparationPairRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationPairState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationPairCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationPairData: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparationPairEligibility: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_PreparationPairRoot: [u64; 8] = [0; 8];
#[no_mangle]
pub static CyberpunkVR_PreparationCameraTrace: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationAllStages: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphParents: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphParentCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_PreparationGraphParentBytes: u32 = 80;
#[no_mangle]
pub static CyberpunkVR_PreparationPairVerified: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationRootSkipped: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationRootCompatible: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationPairMergeLimit: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_PreparationPairMerged: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationPairCompleted: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationPairGeneration: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationRunRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationRunState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationRunStop: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationRunFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationRunFrames: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationRunDepth: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_PreparationRunEyeCopies: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparationRunEyeSubmissions: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/NativeConstantCapture.cpp: the native constant capture: its switch, sequence and rows, the bank's request, sequences, pairs and results, and the comparison's state and result

#[no_mangle]
pub static CyberpunkVR_ConstantCaptureEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantCaptureSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantCaptureFailure: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantDrawCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantBindingCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantPayloadCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantBlobBytes: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantPendingLists: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_ConstantDraws: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBindings: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantPayloads: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBlob: u64 = 0;
#[no_mangle]
pub static CyberpunkVR_ConstantBankRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantBankDone: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankSequences: [u32; 4096] = [0; 4096];
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankDescriptors: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankDraws: u32 = 0;
#[no_mangle]
pub static CyberpunkVR_ConstantBankPairCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantBankPrepareGeometry: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankPreparedDraws: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankInstanceBytes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankGeometryResult: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankPassResult: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankPassDraws: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankPassStage: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankPassValidation: [u32; 3] = [0; 3];
#[no_mangle]
pub static CyberpunkVR_ConstantBankCompareDepth: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantCompareState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantCompareToken: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantCompareSeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_ConstantCompareResult: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantCompareStage: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankBytes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_ConstantBankResult: i32 = 0;

// ---- src/Render/StereoGpuProbe.cpp: the stereo GPU probe: its request and state, the group, draws and view masks, the differences, the prepare gate, the scene route, the timing and the VRS images

#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeNativeReference: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeReferenceSkipped: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeQueueWaits: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeGroup: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeSourceDraws: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeMainDraws: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeMainViewMask: AtomicU32 = AtomicU32::new(4294967295);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeMainReferenceMask: AtomicU32 = AtomicU32::new(4294967295);
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeEyeDifferences: [u64; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeTargetDifferences: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeDifferenceCount: u32 = 0;
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbePrepareGateMs: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbePrepareGateReason: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeLateVisibility: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeMatchedDraws: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeFallbackDraws: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbePrepareGateStart: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbePrepareGateMain: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbePrepareGateRelease: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeResult: i32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeDifferentBytes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeComparedBytes: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeCoverage: [u64; 2] = [0; 2];
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeSceneDepth: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeSceneReject: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeSceneSize: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeScenePhase: [u32; 2] = [0; 2];
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeSceneRoute: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeSceneCommitted: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeSceneRouted: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeSceneSamples: [u64; 2] = [0; 2];
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeTiming: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeTimingDrops: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeTicks: [[u64; 2]; 7] = [[0; 2]; 7];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeFrequency: [u64; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeTimedCount: [[u32; 2]; 7] = [[0; 2]; 7];
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeMixedMaterials: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeFineRate: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeVrs: [[u32; 4]; 2] = [[0; 4]; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeVrsChanges: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeVrsImage: [u64; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeVrsSize: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeVrsCaptured: [u32; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeVrsDifference: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_StereoGpuProbeVrsHistogram: [[u64; 16]; 2] = [[0; 16]; 2];
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeDepthPrepass: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeStartIndices: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoGpuProbeStartRun: AtomicU32 = AtomicU32::new(0);

// ---- src/Render/NativeStereoFamilies.cpp: the stereo families: the asset status, the family counts, misses and failures, the native camera pipelines and the read and depth contracts

#[no_mangle]
pub static CyberpunkVR_StereoFamilyAssetStatus: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyAssetCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyNativeCameras: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilySeq: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyOverflow: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyMisses: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyFailures: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyHits: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyRootHits: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyObserve: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyBindingFailures: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_StereoFamilyUnmatched: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static CyberpunkVR_StereoFamilyRootMismatch: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
#[no_mangle]
pub static mut CyberpunkVR_StereoFamilyRecordBytes: u32 = 72;
#[no_mangle]
pub static mut CyberpunkVR_StereoFamilyNativeCameraRecordBytes: u32 = 16;
#[no_mangle]
pub static mut CyberpunkVR_StereoFamilyNativeCameraPipelines: [[u64; 2]; 4096] = [[0; 2]; 4096];
#[no_mangle]
pub static mut CyberpunkVR_StereoFamilySkinningReadContracts: [u32; 4096] = [0; 4096];
#[no_mangle]
pub static mut CyberpunkVR_StereoFamilyAlphaDepthContracts: [u32; 4096] = [0; 4096];
#[no_mangle]
pub static mut CyberpunkVR_StereoFamilyError: [u8; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_StereoFamilyManifestHash: [u8; 65] = [0; 65];

/// StereoGpuProbe.cpp's Draw at the probe's start: the VRS, scene, timing, difference, gate and draw counters (one
/// source draw).
#[no_mangle]
pub extern "C" fn cvr_sgp_reset_start() {
    use core::sync::atomic::Ordering::SeqCst;
    // SAFETY: the probe's exports, written under its mutex as the .cpp wrote them.
    unsafe {
        CyberpunkVR_StereoGpuProbeVrs = [[0; 4]; 2];
        CyberpunkVR_StereoGpuProbeVrsChanges = [0; 2];
        CyberpunkVR_StereoGpuProbeVrsImage = [0; 2];
        CyberpunkVR_StereoGpuProbeVrsSize = [0; 2];
        CyberpunkVR_StereoGpuProbeVrsCaptured = [0; 2];
        CyberpunkVR_StereoGpuProbeVrsDifference = 0;
        CyberpunkVR_StereoGpuProbeVrsHistogram = [[0; 16]; 2];
        CyberpunkVR_StereoGpuProbeSceneRouted = [0; 2];
        CyberpunkVR_StereoGpuProbeSceneSamples = [0; 2];
        CyberpunkVR_StereoGpuProbeTicks = [[0; 2]; 7];
        CyberpunkVR_StereoGpuProbeTimedCount = [[0; 2]; 7];
        CyberpunkVR_StereoGpuProbeFrequency = [0; 2];
        CyberpunkVR_StereoGpuProbeScenePhase = [0; 2];
        CyberpunkVR_StereoGpuProbeSceneSize = [0; 2];
        CyberpunkVR_StereoGpuProbeResult = 0;
        CyberpunkVR_StereoGpuProbeDifferentBytes = 0;
        CyberpunkVR_StereoGpuProbeComparedBytes = 0;
        CyberpunkVR_StereoGpuProbeCoverage = [0; 2];
        CyberpunkVR_StereoGpuProbeEyeDifferences = [0; 2];
        CyberpunkVR_StereoGpuProbeTargetDifferences = [0; 4];
        CyberpunkVR_StereoGpuProbeDifferenceCount = 0;
    }
    CyberpunkVR_StereoGpuProbeSceneCommitted.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbeTimingDrops.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbeSceneReject.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbeReferenceSkipped.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbeQueueWaits.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbeSourceDraws.store(1, SeqCst);
    CyberpunkVR_StereoGpuProbeMainDraws.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbeMatchedDraws.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbeFallbackDraws.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbePrepareGateReason.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbePrepareGateStart.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbePrepareGateMain.store(0, SeqCst);
    CyberpunkVR_StereoGpuProbePrepareGateRelease.store(0, SeqCst);
}

#[cfg(test)]
mod tests {
    //! The initial values the .cpp's definitions gave.
    use super::*;
    use core::sync::atomic::Ordering::Relaxed;

    #[test]
    fn probe_start_resets() {
        use core::sync::atomic::Ordering::Relaxed;
        // SAFETY: only this test touches the probe's exports.
        unsafe {
            CyberpunkVR_StereoGpuProbeVrsHistogram = [[3; 16]; 2];
            CyberpunkVR_StereoGpuProbeTicks = [[4; 2]; 7];
            CyberpunkVR_StereoGpuProbeResult = -5;
            CyberpunkVR_StereoGpuProbeTargetDifferences = [6; 4];
        }
        CyberpunkVR_StereoGpuProbeSourceDraws.store(9, Relaxed);
        CyberpunkVR_StereoGpuProbePrepareGateRelease.store(7, Relaxed);
        CyberpunkVR_StereoGpuProbeSceneReject.store(2, Relaxed);
        cvr_sgp_reset_start();
        // SAFETY: as above; read by value.
        let (histogram, ticks, result, targets) = unsafe {
            (CyberpunkVR_StereoGpuProbeVrsHistogram, CyberpunkVR_StereoGpuProbeTicks, CyberpunkVR_StereoGpuProbeResult, CyberpunkVR_StereoGpuProbeTargetDifferences)
        };
        assert_eq!((histogram, ticks, result, targets), ([[0; 16]; 2], [[0; 2]; 7], 0, [0; 4]));
        assert_eq!(
            (CyberpunkVR_StereoGpuProbeSourceDraws.load(Relaxed), CyberpunkVR_StereoGpuProbePrepareGateRelease.load(Relaxed), CyberpunkVR_StereoGpuProbeSceneReject.load(Relaxed)),
            (1, 0, 0),
            "one source draw: the probe's own"
        );
    }
    #[test]
    fn initial_values() {
        assert_eq!(CyberpunkVR_StereoGpuProbeMainViewMask.load(Relaxed), u32::MAX);
        assert_eq!(CyberpunkVR_PreparationPairMergeLimit.load(Relaxed), 1);
        // SAFETY: read by value; no test writes them.
        let (record, parent, camera, sequences) = unsafe {
            (CyberpunkVR_StereoFamilyRecordBytes, CyberpunkVR_PreparationGraphParentBytes, CyberpunkVR_StereoFamilyNativeCameraRecordBytes, { let sequences = CyberpunkVR_ConstantBankSequences; sequences.len() })
        };
        assert_eq!((record, parent, camera, sequences), (72, 80, 16, 4096));
    }
}