//! The retained geometry's exported state (formerly definitions in src/Render/NativeRetainedGeometry.cpp): the capture's
//! request, state and stage, its switches, and every published table -- a row pointer, its count and row size, and the
//! stats, failures and diagnostics around it -- defined on the Rust side and exported from rust/exported-data.txt; the
//! C++ capture keeps declaring them, the tools read them by name. And `cvr_nrg_reset_exports`: FrameBoundary's reset of
//! all of them when a new capture starts (each table emptied, its stats and failures zeroed, the switch-bearing stats
//! given the new capture's switches; the request, state, stage, switches and row sizes kept).
#![allow(non_upper_case_globals)]
use core::sync::atomic::AtomicU32;

#[no_mangle]
pub static CyberpunkVR_RetainedGeometryRequest: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RetainedGeometryState: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RetainedGeometryStage: AtomicU32 = AtomicU32::new(7);
#[no_mangle]
pub static CyberpunkVR_RetainedGeometryVerified: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedCurrentEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedInfixEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedTailEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_PreparedVrsRefinementEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_RetainedFullT5Enable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedFullT5Stats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedFullT5Rows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFullT5Count: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFullT5RowBytes: u32 = 128;
#[no_mangle]
pub static CyberpunkVR_RetainedBorrowedTablesEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedTablesStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedTablesRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedRangeRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedTablesCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedTablesRowBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedRangeCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedRangeRowBytes: u32 = 96;
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedInventoryRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedFailure: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedInventoryCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedBorrowedInventoryRowBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_RetainedPageableUseStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedPageableUseRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedPageableUseFailure: [u64; 24] = [0; 24];
#[no_mangle]
pub static mut CyberpunkVR_RetainedPageableSearch: [u64; 20] = [0; 20];
#[no_mangle]
pub static mut CyberpunkVR_RetainedPageableUseCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedPageableUseRowBytes: u32 = 128;
#[no_mangle]
pub static CyberpunkVR_RetainedFixedAssetsEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedAssetsStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedAssetsCounts: [u64; 10] = [0; 10];
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedAssetsRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedAssetRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedSlotRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedAssetsCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedAssetsRowBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedAssetCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedAssetRowBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedSlotCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedSlotRowBytes: u32 = 64;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedDescriptorRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedDescriptorCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedFixedDescriptorRowBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_RetainedTail: [u64; 20] = [0; 20];
#[no_mangle]
pub static mut CyberpunkVR_RetainedTargetLifetime: [u64; 128] = [0; 128];
#[no_mangle]
pub static mut CyberpunkVR_RetainedInfix: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedCurrent: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedBaseline: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGeometryStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGeometryDifferent: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGeometryBytes: [u64; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGeometryEnds: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGeometryIndirect: [u64; 2] = [0; 2];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGeometryIndirectRejected: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGeometryShaderRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedGeometryShaderCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedGeometryShaderRowBytes: u32 = 88;
#[no_mangle]
pub static CyberpunkVR_RetainedSourceStreamEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceStreamStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceStreamRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceStreamCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceStreamRowBytes: u32 = 384;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIndirectBindingsStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedIndirectBindingsRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIndirectBindingsCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIndirectBindingsRowBytes: u32 = 448;
#[no_mangle]
pub static CyberpunkVR_RetainedIndirectSnapshotsEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedIndirectSnapshotsStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedIndirectSnapshotsRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIndirectSnapshotsCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIndirectSnapshotsRowBytes: u32 = 192;
#[no_mangle]
pub static CyberpunkVR_RetainedSourceCbvEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvRowBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvProducerRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvProducerCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvProducerRowBytes: u32 = 320;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSelectorCoverageRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSelectorCoverageCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSelectorCoverageRowBytes: u32 = 224;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSelectorCacheEventRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSelectorCacheEventCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSelectorCacheEventRowBytes: u32 = 288;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvDiagnostics: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvDiagnosticBytes: u32 = 128;
#[no_mangle]
pub static CyberpunkVR_RetainedSourceTablesEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceTablesStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceTablesRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceTablesCommands: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceTablesCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceTablesCommandCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceTablesRowBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceTablesCommandBytes: u32 = 192;
#[no_mangle]
pub static CyberpunkVR_RetainedSourceDetailsEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceDetailsStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceDetailsRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceDetailsCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceDetailsRowBytes: u32 = 1024;
#[no_mangle]
pub static CyberpunkVR_RetainedSourceCbvSnapshotsEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvSnapshotsStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvSnapshotsRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvSnapshotsCommands: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvSnapshotsCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvSnapshotsCommandCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvSnapshotsRowBytes: u32 = 256;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvSnapshotsCommandBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvSnapshotsBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvSnapshotsBlobBytes: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvCopyOwnerRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvCopyOwnerCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceCbvCopyOwnerRowBytes: u32 = 64;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsCommands: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsInputs: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsSlots: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsT5: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsCommandCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsInputCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsSlotCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsT5Count: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsBlobBytes: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsCommandBytes: u32 = 256;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsInputBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsSlotBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceSelectorsT5Bytes: u32 = 256;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5NativeRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5SnapshotRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5SnapshotBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5IntentBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5NativeCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5SnapshotCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5SnapshotBlobBytes: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5IntentBlobBytes: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5NativeRowBytes: u32 = 768;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5SnapshotRowBytes: u32 = 256;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5CopyDiagnosticRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5MarkDiagnosticRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5DiagnosticCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5DiagnosticRowBytes: u32 = 256;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5PieceRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5PieceCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5PieceRowBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5CoverageRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5CoverageCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5CoverageRowBytes: u32 = 160;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyCommands: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyTables: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyMissingCbvs: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyCacheStats: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyBankStats: [u64; 6] = [0; 6];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyBankFailure: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5OverlayStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5OverlayRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5OverlayBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5OverlayCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5OverlayRowBytes: u32 = 256;
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5OverlayBlobBytes: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaCommands: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaObservations: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaViews: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaCommandCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaObservationCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaViewCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaCommandBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaObservationBytes: u32 = 536;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaViewBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceRowBytes: u32 = 384;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceBlobBytes: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceGpuRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceGpuBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceGpuCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceGpuRowBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceGpuBlobBytes: u32 = 0;
#[no_mangle]
pub static CyberpunkVR_RetainedInstanceGpuPhase: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceOverlayStats: [u64; 8] = [0; 8];
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceOverlayRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceOverlayCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceOverlayRowBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceOverlayBlob: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceOverlayBlobBytes: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceOverlayMode: u32 = 2;
#[no_mangle]
pub static mut CyberpunkVR_RetainedInstanceOverlayError: [u8; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_RetainedT5OverlayError: [u8; 256] = [0; 256];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGuardedStats: [u64; 16] = [0; 16];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGuardedCacheStats: [u64; 4] = [0; 4];
#[no_mangle]
pub static mut CyberpunkVR_RetainedGuardedRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedGuardedCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedGuardedRowBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedStagedStats: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_RetainedStagedRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedStagedCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedStagedRowBytes: u32 = 64;
#[no_mangle]
pub static mut CyberpunkVR_RetainedStagedResidency: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_RetainedStagedResidencyDetails: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_RetainedStagedMembershipDetails: [u64; 12] = [0; 12];
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaSelections: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedPrivateGeometryRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaSelectionCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaSelectionBytes: u32 = 40;
#[no_mangle]
pub static mut CyberpunkVR_RetainedPrivateGeometryCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedPrivateGeometryRowBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyCommandCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyTableCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyMissingCbvCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyCommandBytes: u32 = 256;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyTableBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceAssemblyMissingCbvBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialRowBytes: u32 = 192;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialSources: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialSourceCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialSourceBytes: u32 = 40;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialBufferRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialBufferCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialBufferRowBytes: u32 = 320;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialBufferCopyRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialBufferCopyCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedMaterialBufferCopyRowBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaReuseRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaTransferRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaReuseCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedIaTransferCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedNativeTableRows: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedNativeTableCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedNativeTableRowBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedNativeTableEntries: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedNativeTableEntryCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedNativeTableEntryBytes: u32 = 64;
#[no_mangle]
pub static mut CyberpunkVR_RetainedNativeTableRings: u64 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedNativeTableRingCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedNativeTableRingBytes: u32 = 128;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceBoundaryRows: [u64; 40] = [0; 40];
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceBoundaryCount: u32 = 0;
#[no_mangle]
pub static mut CyberpunkVR_RetainedSourceQueueOrder: [u64; 16] = [0; 16];

/// FrameBoundary's reset when a capture starts, with the new capture's switches: full T5, borrowed tables, fixed
/// assets, the owned current path, the certified infix, the native tail.
#[no_mangle]
pub extern "C" fn cvr_nrg_reset_exports(full_t5: bool, borrowed_tables: bool, fixed_assets: bool, current: bool, infix: bool, tail: bool) {
    // SAFETY: the capture's exports, written under its mutex as the .cpp wrote them.
    unsafe {
        CyberpunkVR_RetainedSourceStreamStats = [0; 16];
        CyberpunkVR_RetainedSourceStreamRows = 0;
        CyberpunkVR_RetainedSourceStreamCount = 0;
        CyberpunkVR_RetainedFullT5Stats = [0; 16];
        CyberpunkVR_RetainedFullT5Stats[0] = 1;
        CyberpunkVR_RetainedFullT5Stats[1] = u64::from(full_t5);
        CyberpunkVR_RetainedFullT5Rows = 0;
        CyberpunkVR_RetainedFullT5Count = 0;
        CyberpunkVR_RetainedBorrowedTablesStats = [0; 16];
        CyberpunkVR_RetainedBorrowedTablesStats[0] = 1;
        CyberpunkVR_RetainedBorrowedTablesStats[1] = u64::from(borrowed_tables);
        CyberpunkVR_RetainedBorrowedTablesRows = 0;
        CyberpunkVR_RetainedBorrowedRangeRows = 0;
        CyberpunkVR_RetainedBorrowedTablesCount = 0;
        CyberpunkVR_RetainedBorrowedRangeCount = 0;
        CyberpunkVR_RetainedBorrowedInventoryRows = 0;
        CyberpunkVR_RetainedBorrowedInventoryCount = 0;
        CyberpunkVR_RetainedBorrowedFailure = [0; 8];
        CyberpunkVR_RetainedPageableUseStats = [0; 16];
        CyberpunkVR_RetainedPageableUseStats[0] = 1;
        CyberpunkVR_RetainedPageableUseStats[1] = u64::from(borrowed_tables);
        CyberpunkVR_RetainedPageableUseFailure = [0; 24];
        CyberpunkVR_RetainedPageableUseRows = 0;
        CyberpunkVR_RetainedPageableUseCount = 0;
        CyberpunkVR_RetainedPageableSearch = [0; 20];
        CyberpunkVR_RetainedFixedAssetsStats = [0; 16];
        CyberpunkVR_RetainedFixedAssetsStats[0] = 1;
        CyberpunkVR_RetainedFixedAssetsStats[1] = u64::from(fixed_assets);
        CyberpunkVR_RetainedFixedAssetsCounts = [0; 10];
        CyberpunkVR_RetainedFixedAssetsRows = 0;
        CyberpunkVR_RetainedFixedAssetRows = 0;
        CyberpunkVR_RetainedFixedSlotRows = 0;
        CyberpunkVR_RetainedFixedAssetsCount = 0;
        CyberpunkVR_RetainedFixedAssetCount = 0;
        CyberpunkVR_RetainedFixedSlotCount = 0;
        CyberpunkVR_RetainedFixedDescriptorRows = 0;
        CyberpunkVR_RetainedFixedDescriptorCount = 0;
        CyberpunkVR_RetainedNativeTableRows = 0;
        CyberpunkVR_RetainedNativeTableCount = 0;
        CyberpunkVR_RetainedNativeTableEntries = 0;
        CyberpunkVR_RetainedNativeTableEntryCount = 0;
        CyberpunkVR_RetainedNativeTableRings = 0;
        CyberpunkVR_RetainedNativeTableRingCount = 0;
        CyberpunkVR_RetainedSourceBoundaryRows = [0; 40];
        CyberpunkVR_RetainedSourceBoundaryCount = 0;
        CyberpunkVR_RetainedSourceQueueOrder = [0; 16];
        CyberpunkVR_RetainedIndirectBindingsStats = [0; 16];
        CyberpunkVR_RetainedIndirectBindingsRows = 0;
        CyberpunkVR_RetainedIndirectBindingsCount = 0;
        CyberpunkVR_RetainedIndirectSnapshotsStats = [0; 16];
        CyberpunkVR_RetainedIndirectSnapshotsRows = 0;
        CyberpunkVR_RetainedIndirectSnapshotsCount = 0;
        CyberpunkVR_RetainedSourceCbvStats = [0; 16];
        CyberpunkVR_RetainedSourceCbvRows = 0;
        CyberpunkVR_RetainedSourceCbvCount = 0;
        CyberpunkVR_RetainedSourceCbvProducerRows = 0;
        CyberpunkVR_RetainedSourceCbvProducerCount = 0;
        CyberpunkVR_RetainedSelectorCoverageRows = 0;
        CyberpunkVR_RetainedSelectorCoverageCount = 0;
        CyberpunkVR_RetainedSelectorCacheEventRows = 0;
        CyberpunkVR_RetainedSelectorCacheEventCount = 0;
        CyberpunkVR_RetainedSourceCbvDiagnostics = 0;
        CyberpunkVR_RetainedSourceTablesStats = [0; 16];
        CyberpunkVR_RetainedSourceTablesRows = 0;
        CyberpunkVR_RetainedSourceTablesCount = 0;
        CyberpunkVR_RetainedSourceTablesCommands = 0;
        CyberpunkVR_RetainedSourceTablesCommandCount = 0;
        CyberpunkVR_RetainedSourceDetailsStats = [0; 16];
        CyberpunkVR_RetainedSourceDetailsRows = 0;
        CyberpunkVR_RetainedSourceDetailsCount = 0;
        CyberpunkVR_RetainedSourceCbvSnapshotsStats = [0; 16];
        CyberpunkVR_RetainedSourceCbvSnapshotsRows = 0;
        CyberpunkVR_RetainedSourceCbvSnapshotsCount = 0;
        CyberpunkVR_RetainedSourceCbvSnapshotsCommands = 0;
        CyberpunkVR_RetainedSourceCbvSnapshotsCommandCount = 0;
        CyberpunkVR_RetainedSourceCbvCopyOwnerRows = 0;
        CyberpunkVR_RetainedSourceCbvCopyOwnerCount = 0;
        CyberpunkVR_RetainedSourceCbvSnapshotsBlob = 0;
        CyberpunkVR_RetainedSourceCbvSnapshotsBlobBytes = 0;
        CyberpunkVR_RetainedSourceSelectorsStats = [0; 16];
        CyberpunkVR_RetainedSourceSelectorsCommands = 0;
        CyberpunkVR_RetainedSourceSelectorsInputs = 0;
        CyberpunkVR_RetainedSourceSelectorsSlots = 0;
        CyberpunkVR_RetainedSourceSelectorsT5 = 0;
        CyberpunkVR_RetainedSourceSelectorsBlob = 0;
        CyberpunkVR_RetainedSourceSelectorsCommandCount = 0;
        CyberpunkVR_RetainedSourceSelectorsInputCount = 0;
        CyberpunkVR_RetainedSourceSelectorsSlotCount = 0;
        CyberpunkVR_RetainedSourceSelectorsT5Count = 0;
        CyberpunkVR_RetainedSourceSelectorsBlobBytes = 0;
        CyberpunkVR_RetainedT5NativeRows = 0;
        CyberpunkVR_RetainedT5SnapshotRows = 0;
        CyberpunkVR_RetainedT5SnapshotBlob = 0;
        CyberpunkVR_RetainedT5IntentBlob = 0;
        CyberpunkVR_RetainedT5NativeCount = 0;
        CyberpunkVR_RetainedT5SnapshotCount = 0;
        CyberpunkVR_RetainedT5SnapshotBlobBytes = 0;
        CyberpunkVR_RetainedT5IntentBlobBytes = 0;
        CyberpunkVR_RetainedT5CopyDiagnosticRows = 0;
        CyberpunkVR_RetainedT5MarkDiagnosticRows = 0;
        CyberpunkVR_RetainedT5DiagnosticCount = 0;
        CyberpunkVR_RetainedT5PieceRows = 0;
        CyberpunkVR_RetainedT5PieceCount = 0;
        CyberpunkVR_RetainedT5CoverageRows = 0;
        CyberpunkVR_RetainedT5CoverageCount = 0;
        CyberpunkVR_RetainedSourceAssemblyStats = [0; 16];
        CyberpunkVR_RetainedSourceAssemblyCacheStats = [0; 4];
        CyberpunkVR_RetainedSourceAssemblyBankStats = [0; 6];
        CyberpunkVR_RetainedSourceAssemblyBankFailure = [0; 4];
        CyberpunkVR_RetainedT5OverlayStats = [0; 16];
        CyberpunkVR_RetainedT5OverlayRows = 0;
        CyberpunkVR_RetainedT5OverlayCount = 0;
        CyberpunkVR_RetainedT5OverlayBlob = 0;
        CyberpunkVR_RetainedT5OverlayBlobBytes = 0;
        CyberpunkVR_RetainedSourceAssemblyCommands = 0;
        CyberpunkVR_RetainedSourceAssemblyTables = 0;
        CyberpunkVR_RetainedSourceAssemblyMissingCbvs = 0;
        CyberpunkVR_RetainedSourceAssemblyCommandCount = 0;
        CyberpunkVR_RetainedSourceAssemblyTableCount = 0;
        CyberpunkVR_RetainedSourceAssemblyMissingCbvCount = 0;
        CyberpunkVR_RetainedIaCommands = 0;
        CyberpunkVR_RetainedIaObservations = 0;
        CyberpunkVR_RetainedIaViews = 0;
        CyberpunkVR_RetainedIaCommandCount = 0;
        CyberpunkVR_RetainedIaObservationCount = 0;
        CyberpunkVR_RetainedIaViewCount = 0;
        CyberpunkVR_RetainedInstanceRows = 0;
        CyberpunkVR_RetainedInstanceBlob = 0;
        CyberpunkVR_RetainedInstanceCount = 0;
        CyberpunkVR_RetainedInstanceBlobBytes = 0;
        CyberpunkVR_RetainedInstanceGpuRows = 0;
        CyberpunkVR_RetainedInstanceGpuBlob = 0;
        CyberpunkVR_RetainedInstanceGpuCount = 0;
        CyberpunkVR_RetainedInstanceGpuBlobBytes = 0;
        CyberpunkVR_RetainedInstanceOverlayStats = [0; 8];
        CyberpunkVR_RetainedInstanceOverlayRows = 0;
        CyberpunkVR_RetainedInstanceOverlayCount = 0;
        CyberpunkVR_RetainedInstanceOverlayBlob = 0;
        CyberpunkVR_RetainedInstanceOverlayBlobBytes = 0;
        CyberpunkVR_RetainedInstanceOverlayError = [0; 256];
        CyberpunkVR_RetainedT5OverlayError = [0; 256];
        CyberpunkVR_RetainedGuardedStats = [0; 16];
        CyberpunkVR_RetainedGuardedRows = 0;
        CyberpunkVR_RetainedGuardedCount = 0;
        CyberpunkVR_RetainedGuardedCacheStats = [0; 4];
        CyberpunkVR_RetainedStagedStats = [0; 12];
        CyberpunkVR_RetainedStagedRows = 0;
        CyberpunkVR_RetainedStagedCount = 0;
        CyberpunkVR_RetainedStagedResidency = [0; 12];
        CyberpunkVR_RetainedStagedResidencyDetails = [0; 12];
        CyberpunkVR_RetainedStagedMembershipDetails = [0; 12];
        CyberpunkVR_RetainedCurrent = [0; 16];
        CyberpunkVR_RetainedCurrent[0] = u64::from(current);
        CyberpunkVR_RetainedBaseline = [0; 16];
        CyberpunkVR_RetainedInfix = [0; 16];
        CyberpunkVR_RetainedInfix[0] = u64::from(infix);
        CyberpunkVR_RetainedTail = [0; 20];
        CyberpunkVR_RetainedTail[0] = u64::from(tail);
        CyberpunkVR_RetainedTail[10] = u64::from(tail);
        CyberpunkVR_RetainedTargetLifetime = [0; 128];
        CyberpunkVR_RetainedIaSelections = 0;
        CyberpunkVR_RetainedPrivateGeometryRows = 0;
        CyberpunkVR_RetainedMaterialRows = 0;
        CyberpunkVR_RetainedMaterialCount = 0;
        CyberpunkVR_RetainedMaterialSources = 0;
        CyberpunkVR_RetainedMaterialSourceCount = 0;
        CyberpunkVR_RetainedIaSelectionCount = 0;
        CyberpunkVR_RetainedPrivateGeometryCount = 0;
        CyberpunkVR_RetainedGeometryShaderRows = 0;
        CyberpunkVR_RetainedGeometryShaderCount = 0;
        CyberpunkVR_RetainedGeometryStats = [0; 16];
        CyberpunkVR_RetainedGeometryDifferent = [0; 12];
        CyberpunkVR_RetainedGeometryBytes = [0; 2];
        CyberpunkVR_RetainedGeometryEnds = [0; 4];
        CyberpunkVR_RetainedGeometryIndirect = [0; 2];
        CyberpunkVR_RetainedGeometryIndirectRejected = [0; 8];
    }
}

#[cfg(test)]
mod tests {
    //! The reset against hand-set values.
    use super::*;
    use core::sync::atomic::Ordering::Relaxed;

    #[test]
    fn resets() {
        // SAFETY: only this test touches the exports.
        unsafe {
            CyberpunkVR_RetainedTail = [7; 20];
            CyberpunkVR_RetainedFullT5Stats = [9; 16];
            CyberpunkVR_RetainedSourceSelectorsBlob = 5;
            CyberpunkVR_RetainedT5OverlayError[3] = b'x';
            CyberpunkVR_RetainedGeometryIndirectRejected = [1; 8];
            CyberpunkVR_RetainedMaterialBufferRows = 11;
        }
        cvr_nrg_reset_exports(true, false, true, false, true, true);
        // SAFETY: as above; read by value.
        let (tail, full, blob, error, rejected, buffer, current, infix, fixed, pageable) = unsafe {
            (
                CyberpunkVR_RetainedTail,
                CyberpunkVR_RetainedFullT5Stats,
                CyberpunkVR_RetainedSourceSelectorsBlob,
                CyberpunkVR_RetainedT5OverlayError,
                CyberpunkVR_RetainedGeometryIndirectRejected,
                CyberpunkVR_RetainedMaterialBufferRows,
                CyberpunkVR_RetainedCurrent,
                CyberpunkVR_RetainedInfix,
                CyberpunkVR_RetainedFixedAssetsStats,
                CyberpunkVR_RetainedPageableUseStats,
            )
        };
        let mut expected_tail = [0u64; 20];
        expected_tail[0] = 1;
        expected_tail[10] = 1;
        assert_eq!(tail, expected_tail);
        assert_eq!((full[0], full[1], full[2]), (1, 1, 0));
        assert_eq!((blob, error[3], rejected, current[0], infix[0], fixed[1], pageable[0], pageable[1]), (0, 0, [0; 8], 0, 1, 1, 1, 0));
        // What the .cpp's reset left: the material buffer rows, the stage and the row sizes.
        assert_eq!(buffer, 11);
        assert_eq!(CyberpunkVR_RetainedGeometryStage.load(Relaxed), 7);
        // SAFETY: as above.
        assert_eq!(unsafe { (CyberpunkVR_RetainedSourceStreamRowBytes, CyberpunkVR_RetainedInstanceOverlayMode) }, (384, 2));
    }
}