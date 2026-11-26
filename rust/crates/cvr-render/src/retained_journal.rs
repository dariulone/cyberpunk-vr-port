//! The retained geometry's journal (src/Render/NativeRetainedGeometry.cpp, `Work`'s published rows and blobs) and its
//! publication (`PublishSources`): every table a capture records -- the source stream, its indirect bindings and
//! snapshots, CBVs, tables, details, CBV snapshots, selectors, T5 copies, source assembly, overlays, IA, instances,
//! materials, borrowed and fixed tables, native tables -- as whole rows of its width, and the byte blobs beside them;
//! one journal per capture, rows only appended at the end (a std::vector's data pointer: none until it holds memory,
//! kept by clear). PublishSources points every table's export at its rows and count and fills the stats rows: the
//! stream's state (failed 4, compiled 3, both eyes sealed 2, else 1) with its summary and compiled source count, the
//! indirect bindings, and -- each when its capture is on -- the CBVs, snapshots (with the sealed eyes), tables (with
//! the cache's tables and descriptors), details, CBV snapshots (failed 4, payload 3, sealed 2; the finalized arenas),
//! selectors (complete 3, payload 2; the slot cache), the source assembly (truncated 4, complete 3; its cache and
//! bank) and the T5 overlay; then the instance overlay (its stage, both eyes' versions and bytes, the highest signaled
//! and the lowest completed fence value).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Work's row and blob members are facades over this journal (the C++ half keeps
//! their container interface) and PublishSources forwards here with the values held by C++ objects; the unit tests
//! below cover the journal and each stats row.
//!
//! And Poll's payloads (once both eyes' submissions completed): the instance readbacks copied into the GPU instance
//! blob and compared with the recorded instance bytes, the verification planes compared, the CBV snapshot copies read
//! into their blob (all or none; then PublishSources), the T5 snapshot copies read and compared with their candidates,
//! the selector inputs, commands and T5 views evaluated over them, and the full-T5 copies compared -- each refusal
//! raising the capture's Fail with its reason; the readbacks are mapped and unmapped here (D3D12 buffers).

#![allow(static_mut_refs)]
use crate::retained_exports::*;
use crate::retained_payloads::{full_t5_rows, plane_differences, Plane};
use crate::retained_publish::{summarize, Kind};
use crate::retained_selectors::{match_input, selector_command, t5_view};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{ID3D12Fence, ID3D12Resource, D3D12_RANGE};

/// The row tables (Work's member, row width), in the members' order.
pub const ROWS: [(&str, usize); 57] = [
    ("shaderRows", 11),
    ("guardedRows", 16),
    ("fullT5Rows", 16),
    ("borrowedTableRows", 16),
    ("borrowedRangeRows", 12),
    ("borrowedInventoryRows", 24),
    ("borrowedPageableRows", 16),
    ("fixedRangeRows", 24),
    ("fixedAssetRows", 16),
    ("fixedSlotRows", 8),
    ("fixedDescriptorRows", 24),
    ("stagedRows", 8),
    ("nativeTableRows", 16),
    ("nativeTableEntries", 8),
    ("nativeTableRings", 16),
    ("materialRows", 24),
    ("materialBufferRows", 40),
    ("materialBufferCopies", 16),
    ("materialSourceRows", 5),
    ("instanceRows", 48),
    ("instanceGpuRows", 24),
    ("instanceOverlayRows", 24),
    ("iaSelections", 5),
    ("privateGeometryRows", 16),
    ("sourceRows", 48),
    ("indirectRows", 56),
    ("snapshotRows", 24),
    ("cbvRows", 24),
    ("cbvProducers", 40),
    ("cbvDiagnostics", 16),
    ("detailRows", 128),
    ("tableRows", 16),
    ("tableCommands", 24),
    ("cbvSnapshotRows", 32),
    ("cbvSnapshotCommands", 24),
    ("cbvCopyOwners", 8),
    ("iaCommands", 16),
    ("iaViews", 16),
    ("iaReuse", 8),
    ("iaTransfers", 5),
    ("iaObservations", 67),
    ("selectorCommands", 32),
    ("t5Rows", 32),
    ("selectorInputs", 24),
    ("selectorCoverage", 28),
    ("selectorCacheEvents", 36),
    ("selectorSlots", 16),
    ("t5NativeRows", 96),
    ("t5SnapshotRows", 32),
    ("t5CopyDiagnostics", 32),
    ("t5MarkDiagnostics", 32),
    ("t5Pieces", 16),
    ("t5Coverage", 20),
    ("assemblyCommands", 32),
    ("assemblyTables", 16),
    ("assemblyMissingCbvs", 16),
    ("overlayRows", 32),
];
/// The blobs.
pub const BLOBS: [&str; 8] = ["instanceBlob", "instanceGpuBlob", "instanceOverlayBlob", "cbvBlob", "selectorBlob", "t5Blob", "t5IntentBlob", "overlayBlob"];

/// A capture's journal.
pub struct Journal {
    rows: Vec<Vec<u64>>,
    blobs: Vec<Vec<u8>>,
}
impl Default for Journal {
    fn default() -> Self {
        Journal { rows: ROWS.iter().map(|_| Vec::new()).collect(), blobs: BLOBS.iter().map(|_| Vec::new()).collect() }
    }
}
impl Journal {
    fn width(kind: u32) -> usize {
        ROWS[kind as usize].1
    }
    /// The table's rows (whole rows of its width).
    pub fn rows(&self, kind: u32) -> &[u64] {
        &self.rows[kind as usize]
    }
    pub fn count(&self, kind: u32) -> usize {
        self.rows[kind as usize].len() / Self::width(kind)
    }
    /// Appends whole rows.
    pub fn append(&mut self, kind: u32, rows: &[u64]) {
        debug_assert_eq!(rows.len() % Self::width(kind), 0);
        self.rows[kind as usize].extend_from_slice(rows);
    }
    /// Room for `count` rows in all.
    pub fn reserve(&mut self, kind: u32, count: usize) {
        let want = count.saturating_mul(Self::width(kind));
        let table = &mut self.rows[kind as usize];
        if want > table.len() {
            table.reserve(want - table.len());
        }
    }
    pub fn clear(&mut self, kind: u32) {
        self.rows[kind as usize].clear();
    }
    pub fn blob(&self, blob: u32) -> &[u8] {
        &self.blobs[blob as usize]
    }
    pub fn append_blob(&mut self, blob: u32, bytes: &[u8]) {
        self.blobs[blob as usize].extend_from_slice(bytes);
    }
    pub fn clear_blob(&mut self, blob: u32) {
        self.blobs[blob as usize].clear();
    }
    /// The export's row pointer: none until the table holds memory.
    fn ptr(&self, kind: u32) -> u64 {
        let table = &self.rows[kind as usize];
        if table.capacity() == 0 { 0 } else { table.as_ptr() as u64 }
    }
    fn len(&self, kind: u32) -> u32 {
        self.count(kind) as u32
    }
    fn ptr_blob(&self, blob: u32) -> u64 {
        let bytes = &self.blobs[blob as usize];
        if bytes.capacity() == 0 { 0 } else { bytes.as_ptr() as u64 }
    }
    fn len_blob(&self, blob: u32) -> u32 {
        self.blobs[blob as usize].len() as u32
    }
    fn kind(name: &str) -> u32 {
        ROWS.iter().position(|(n, _)| *n == name).unwrap() as u32
    }
}

/// One eye's instance capture as PublishSources reads it (`present`: it has one).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PublishEye {
    pub present: u64,
    pub versions: u64,
    pub copied: u64,
    pub required: u64,
    pub signaled: u64,
    pub completed: u64,
}
/// PublishSources' inputs beyond the journal: the capture's switches and outcomes and the values its C++ objects hold
/// (the table and assembly caches' counts, the assembly bank's, the T5 overlay's bytes, the eyes' instance captures).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Publish {
    pub frame: u32,
    pub source_count: u32,
    pub overlay_stage: u32,
    pub instance_overlay_stage: u32,
    pub bank_result: i32,
    pub arenas_finalized: u32,
    pub source_failed: bool,
    pub source_compiled: bool,
    pub sealed: [bool; 2],
    pub source_cbvs: bool,
    pub cbv_failed: bool,
    pub source_snapshots: bool,
    pub snapshot_failed: bool,
    pub source_tables: bool,
    pub tables_failed: bool,
    pub source_details: bool,
    pub details_failed: bool,
    pub source_cbv_snapshots: bool,
    pub cbv_snapshots_failed: bool,
    pub cbv_payload_complete: bool,
    pub source_selectors: bool,
    pub complete: bool,
    pub assembly_truncated: bool,
    pub assembly_complete: bool,
    pub bank_attempted: bool,
    pub bank_present: bool,
    pub overlay_present: bool,
    /// TableCount, DescriptorCount, SlotTableCount, SlotDescriptorCount.
    pub table_cache: [u64; 4],
    /// UniqueDescriptorCount, TableCount, HitCount, MissCount.
    pub assembly_cache: [u64; 4],
    /// DrawCount, DescriptorCount.
    pub bank: [u64; 2],
    pub bank_failure: [u64; 4],
    /// RequiredBytes, CopiedBytes.
    pub overlay_bytes: [u64; 2],
    pub instance: [PublishEye; 2],
}
const _: () = assert!(core::mem::size_of::<PublishEye>() == 48 && core::mem::size_of::<Publish>() == 272 && core::mem::offset_of!(Publish, table_cache) == 48);

/// The stats rows PublishSources fills.
pub struct Stats<'a> {
    pub stream: &'a mut [u64; 16],
    pub indirect: &'a mut [u64; 16],
    pub cbvs: &'a mut [u64; 16],
    pub snapshots: &'a mut [u64; 16],
    pub tables: &'a mut [u64; 16],
    pub details: &'a mut [u64; 16],
    pub cbv_snapshots: &'a mut [u64; 16],
    pub selectors: &'a mut [u64; 16],
    pub assembly: &'a mut [u64; 16],
    pub assembly_cache: &'a mut [u64; 4],
    pub bank: &'a mut [u64; 6],
    pub bank_failure: &'a mut [u64; 4],
    pub overlay: &'a mut [u64; 16],
    pub instance_overlay: &'a mut [u64; 8],
}

fn state(failed: bool, done: bool, sealed: [bool; 2]) -> u64 {
    if failed {
        4
    } else if done {
        3
    } else if sealed[0] && sealed[1] {
        2
    } else {
        1
    }
}
/// PublishSources' stats rows (the caller has checked the source stream is on).
pub fn publish_stats(j: &Journal, p: &Publish, s: Stats) {
    let k = Journal::kind;
    let stream = &mut *s.stream;
    stream[0] = state(p.source_failed, p.source_compiled, p.sealed);
    stream[1] = p.frame as u64;
    stream[2] = j.count(k("sourceRows")) as u64;
    stream[3..12].fill(0);
    summarize(Kind::Stream, j.rows(k("sourceRows")), &[], stream);
    stream[11] = if p.source_compiled { p.source_count as u64 } else { 0 };
    // Ownership of views/state is not a frozen input or native suppression permit.
    stream[13..16].fill(0);
    let indirect = &mut *s.indirect;
    indirect[0] = stream[0];
    indirect[1] = p.frame as u64;
    indirect[2] = j.count(k("indirectRows")) as u64;
    indirect[3..16].fill(0);
    summarize(Kind::Indirect, j.rows(k("indirectRows")), &[], indirect);
    if p.source_cbvs {
        let cbvs = &mut *s.cbvs;
        cbvs.fill(0);
        cbvs[0] = if p.cbv_failed { 4 } else { stream[0] };
        cbvs[1] = p.frame as u64;
        cbvs[2] = j.count(k("sourceRows")) as u64;
        cbvs[3] = j.count(k("cbvRows")) as u64;
        summarize(Kind::Cbv, j.rows(k("cbvRows")), &[], cbvs);
    }
    if p.source_snapshots {
        let snapshots = &mut *s.snapshots;
        snapshots.fill(0);
        snapshots[0] = if p.snapshot_failed { 4 } else { stream[0] };
        snapshots[1] = p.frame as u64;
        snapshots[2] = j.count(k("indirectRows")) as u64;
        snapshots[3] = j.count(k("snapshotRows")) as u64;
        summarize(Kind::Snapshots, j.rows(k("snapshotRows")), &[], snapshots);
        snapshots[10] = p.sealed[0] as u64 + p.sealed[1] as u64;
        snapshots[11] = stream[11];
    }
    if p.source_tables {
        let tables = &mut *s.tables;
        tables.fill(0);
        tables[0] = if p.tables_failed { 4 } else { stream[0] };
        tables[1] = p.frame as u64;
        tables[2] = j.count(k("sourceRows")) as u64;
        tables[3] = j.count(k("tableCommands")) as u64;
        summarize(Kind::Tables, j.rows(k("tableCommands")), j.rows(k("tableRows")), tables);
        tables[8] = p.table_cache[0];
        tables[9] = p.table_cache[1];
    }
    if p.source_details {
        let details = &mut *s.details;
        details.fill(0);
        details[0] = if p.details_failed { 4 } else { stream[0] };
        details[1] = p.frame as u64;
        details[2] = j.count(k("sourceRows")) as u64;
        details[3] = j.count(k("detailRows")) as u64;
        summarize(Kind::Details, j.rows(k("detailRows")), &[], details);
    }
    if p.source_cbv_snapshots {
        let snapshots = &mut *s.cbv_snapshots;
        snapshots.fill(0);
        snapshots[0] = state(p.cbv_snapshots_failed, p.cbv_payload_complete, p.sealed);
        snapshots[1] = p.frame as u64;
        snapshots[2] = j.count(k("cbvSnapshotCommands")) as u64;
        snapshots[3] = j.count(k("cbvSnapshotRows")) as u64;
        summarize(Kind::CbvSnapshots, j.rows(k("cbvSnapshotRows")), j.rows(k("cbvSnapshotCommands")), snapshots);
        snapshots[9] += p.arenas_finalized as u64;
        // DEFAULT-only snapshots are partial input progress, never admission.
        snapshots[13..16].fill(0);
    }
    if p.source_selectors {
        let selectors = &mut *s.selectors;
        selectors.fill(0);
        selectors[0] = if p.complete {
            3
        } else if p.cbv_payload_complete {
            2
        } else {
            1
        };
        selectors[1] = p.frame as u64;
        selectors[2] = j.count(k("selectorCommands")) as u64;
        selectors[3] = j.count(k("selectorInputs")) as u64;
        selectors[4] = j.count(k("selectorSlots")) as u64;
        selectors[5] = j.count(k("t5Rows")) as u64;
        summarize(Kind::Selectors, j.rows(k("selectorCommands")), j.rows(k("selectorInputs")), selectors);
        selectors[13] = p.table_cache[2];
        selectors[14] = p.table_cache[3];
        // Index evaluation and compact descriptor ownership are not execution permission.
        selectors[15] = 0;
        let assembly = &mut *s.assembly;
        assembly.fill(0);
        assembly[0] = if p.assembly_truncated {
            4
        } else if p.assembly_complete {
            3
        } else {
            1
        };
        assembly[1] = p.frame as u64;
        assembly[2] = j.count(k("assemblyCommands")) as u64;
        summarize(Kind::Assembly, j.rows(k("assemblyCommands")), &[], assembly);
        assembly[7] = j.count(k("assemblyTables")) as u64;
        assembly[12] = p.assembly_truncated as u64;
        // Bank creation does not certify a later input epoch or execute geometry.
        assembly[13..16].fill(0);
        *s.assembly_cache = p.assembly_cache;
        let (draws, descriptors) = if p.bank_present { (p.bank[0], p.bank[1]) } else { (0, 0) };
        *s.bank = [p.bank_attempted as u64, p.bank_present as u64, draws, descriptors, p.bank_result as u32 as u64, 0];
        *s.bank_failure = p.bank_failure;
        let overlay = &mut *s.overlay;
        overlay.fill(0);
        overlay[0] = p.overlay_stage as u64;
        overlay[1] = p.frame as u64;
        overlay[2] = j.count(k("overlayRows")) as u64;
        if p.overlay_present {
            overlay[3] = p.overlay_bytes[0];
            overlay[4] = p.overlay_bytes[1];
        }
        overlay[11] = j.count(k("overlayRows")) as u64;
        overlay[13] = (p.overlay_stage != 0) as u64;
        summarize(Kind::Overlay, j.rows(k("overlayRows")), &[], overlay);
    }
    let instance = &mut *s.instance_overlay;
    instance.fill(0);
    instance[0] = p.instance_overlay_stage as u64;
    for eye in p.instance.iter().filter(|e| e.present != 0) {
        instance[1] = instance[1].wrapping_add(eye.versions);
        instance[2] = instance[2].wrapping_add(eye.copied);
        instance[3] = instance[3].wrapping_add(eye.required);
        instance[4] = instance[4].max(eye.signaled);
        instance[5] = if instance[5] != 0 { instance[5].min(eye.completed) } else { eye.completed };
    }
}

/// PublishSources: every table's export, then the stats rows.
///
/// # Safety
/// The caller holds the capture's lock (the exports are written as PublishSources wrote them).
unsafe fn publish(j: &Journal, p: &Publish) {
    unsafe {
        CyberpunkVR_RetainedFullT5Rows = j.ptr(2); // fullT5Rows
        CyberpunkVR_RetainedFullT5Count = j.len(2); // fullT5Rows
        CyberpunkVR_RetainedBorrowedTablesRows = j.ptr(3); // borrowedTableRows
        CyberpunkVR_RetainedBorrowedTablesCount = j.len(3); // borrowedTableRows
        CyberpunkVR_RetainedBorrowedRangeRows = j.ptr(4); // borrowedRangeRows
        CyberpunkVR_RetainedBorrowedRangeCount = j.len(4); // borrowedRangeRows
        CyberpunkVR_RetainedBorrowedInventoryRows = j.ptr(5); // borrowedInventoryRows
        CyberpunkVR_RetainedBorrowedInventoryCount = j.len(5); // borrowedInventoryRows
        CyberpunkVR_RetainedPageableUseRows = j.ptr(6); // borrowedPageableRows
        CyberpunkVR_RetainedPageableUseCount = j.len(6); // borrowedPageableRows
        CyberpunkVR_RetainedFixedAssetsRows = j.ptr(7); // fixedRangeRows
        CyberpunkVR_RetainedFixedAssetsCount = j.len(7); // fixedRangeRows
        CyberpunkVR_RetainedFixedAssetRows = j.ptr(8); // fixedAssetRows
        CyberpunkVR_RetainedFixedAssetCount = j.len(8); // fixedAssetRows
        CyberpunkVR_RetainedFixedSlotRows = j.ptr(9); // fixedSlotRows
        CyberpunkVR_RetainedFixedSlotCount = j.len(9); // fixedSlotRows
        CyberpunkVR_RetainedFixedDescriptorRows = j.ptr(10); // fixedDescriptorRows
        CyberpunkVR_RetainedFixedDescriptorCount = j.len(10); // fixedDescriptorRows
        CyberpunkVR_RetainedNativeTableRows = j.ptr(12); // nativeTableRows
        CyberpunkVR_RetainedNativeTableCount = j.len(12); // nativeTableRows
        CyberpunkVR_RetainedNativeTableEntries = j.ptr(13); // nativeTableEntries
        CyberpunkVR_RetainedNativeTableEntryCount = j.len(13); // nativeTableEntries
        CyberpunkVR_RetainedNativeTableRings = j.ptr(14); // nativeTableRings
        CyberpunkVR_RetainedNativeTableRingCount = j.len(14); // nativeTableRings
        CyberpunkVR_RetainedSourceStreamRows = j.ptr(24); // sourceRows
        CyberpunkVR_RetainedSourceStreamCount = j.len(24); // sourceRows
        CyberpunkVR_RetainedIndirectBindingsRows = j.ptr(25); // indirectRows
        CyberpunkVR_RetainedIndirectBindingsCount = j.len(25); // indirectRows
        if p.source_cbvs {
            CyberpunkVR_RetainedSourceCbvRows = j.ptr(27); // cbvRows
            CyberpunkVR_RetainedSourceCbvCount = j.len(27); // cbvRows
            CyberpunkVR_RetainedSourceCbvDiagnostics = j.ptr(29); // cbvDiagnostics
            CyberpunkVR_RetainedSourceCbvProducerRows = j.ptr(28); // cbvProducers
            CyberpunkVR_RetainedSourceCbvProducerCount = j.len(28); // cbvProducers
            CyberpunkVR_RetainedSelectorCoverageRows = j.ptr(44); // selectorCoverage
            CyberpunkVR_RetainedSelectorCoverageCount = j.len(44); // selectorCoverage
            CyberpunkVR_RetainedSelectorCacheEventRows = j.ptr(45); // selectorCacheEvents
            CyberpunkVR_RetainedSelectorCacheEventCount = j.len(45); // selectorCacheEvents
        }
        if p.source_snapshots {
            CyberpunkVR_RetainedIndirectSnapshotsRows = j.ptr(26); // snapshotRows
            CyberpunkVR_RetainedIndirectSnapshotsCount = j.len(26); // snapshotRows
        }
        if p.source_tables {
            CyberpunkVR_RetainedSourceTablesRows = j.ptr(31); // tableRows
            CyberpunkVR_RetainedSourceTablesCount = j.len(31); // tableRows
            CyberpunkVR_RetainedSourceTablesCommands = j.ptr(32); // tableCommands
            CyberpunkVR_RetainedSourceTablesCommandCount = j.len(32); // tableCommands
        }
        if p.source_details {
            CyberpunkVR_RetainedSourceDetailsRows = j.ptr(30); // detailRows
            CyberpunkVR_RetainedSourceDetailsCount = j.len(30); // detailRows
        }
        if p.source_cbv_snapshots {
            CyberpunkVR_RetainedSourceCbvSnapshotsRows = j.ptr(33); // cbvSnapshotRows
            CyberpunkVR_RetainedSourceCbvSnapshotsCount = j.len(33); // cbvSnapshotRows
            CyberpunkVR_RetainedSourceCbvSnapshotsCommands = j.ptr(34); // cbvSnapshotCommands
            CyberpunkVR_RetainedSourceCbvCopyOwnerRows = j.ptr(35); // cbvCopyOwners
            CyberpunkVR_RetainedSourceCbvCopyOwnerCount = j.len(35); // cbvCopyOwners
            CyberpunkVR_RetainedSourceCbvSnapshotsCommandCount = j.len(34); // cbvSnapshotCommands
            CyberpunkVR_RetainedSourceCbvSnapshotsBlob = j.ptr_blob(3); // cbvBlob
            CyberpunkVR_RetainedSourceCbvSnapshotsBlobBytes = j.len_blob(3); // cbvBlob
        }
        if p.source_selectors {
            CyberpunkVR_RetainedSourceSelectorsCommands = j.ptr(41); // selectorCommands
            CyberpunkVR_RetainedSourceSelectorsCommandCount = j.len(41); // selectorCommands
            CyberpunkVR_RetainedSourceSelectorsInputs = j.ptr(43); // selectorInputs
            CyberpunkVR_RetainedSourceSelectorsInputCount = j.len(43); // selectorInputs
            CyberpunkVR_RetainedSourceSelectorsSlots = j.ptr(46); // selectorSlots
            CyberpunkVR_RetainedSourceSelectorsSlotCount = j.len(46); // selectorSlots
            CyberpunkVR_RetainedSourceSelectorsT5 = j.ptr(42); // t5Rows
            CyberpunkVR_RetainedSourceSelectorsT5Count = j.len(42); // t5Rows
            CyberpunkVR_RetainedSourceSelectorsBlob = j.ptr_blob(4); // selectorBlob
            CyberpunkVR_RetainedSourceSelectorsBlobBytes = j.len_blob(4); // selectorBlob
            CyberpunkVR_RetainedT5NativeRows = j.ptr(47); // t5NativeRows
            CyberpunkVR_RetainedT5NativeCount = j.len(47); // t5NativeRows
            CyberpunkVR_RetainedT5SnapshotRows = j.ptr(48); // t5SnapshotRows
            CyberpunkVR_RetainedT5SnapshotCount = j.len(48); // t5SnapshotRows
            CyberpunkVR_RetainedT5SnapshotBlob = j.ptr_blob(5); // t5Blob
            CyberpunkVR_RetainedT5SnapshotBlobBytes = j.len_blob(5); // t5Blob
            CyberpunkVR_RetainedT5IntentBlob = j.ptr_blob(6); // t5IntentBlob
            CyberpunkVR_RetainedT5IntentBlobBytes = j.len_blob(6); // t5IntentBlob
            CyberpunkVR_RetainedT5CopyDiagnosticRows = j.ptr(49); // t5CopyDiagnostics
            CyberpunkVR_RetainedT5MarkDiagnosticRows = j.ptr(50); // t5MarkDiagnostics
            CyberpunkVR_RetainedT5DiagnosticCount = j.len(49); // t5CopyDiagnostics
            CyberpunkVR_RetainedT5PieceRows = j.ptr(51); // t5Pieces
            CyberpunkVR_RetainedT5PieceCount = j.len(51); // t5Pieces
            CyberpunkVR_RetainedT5CoverageRows = j.ptr(52); // t5Coverage
            CyberpunkVR_RetainedT5CoverageCount = j.len(52); // t5Coverage
            CyberpunkVR_RetainedSourceAssemblyCommands = j.ptr(53); // assemblyCommands
            CyberpunkVR_RetainedSourceAssemblyCommandCount = j.len(53); // assemblyCommands
            CyberpunkVR_RetainedSourceAssemblyTables = j.ptr(54); // assemblyTables
            CyberpunkVR_RetainedSourceAssemblyTableCount = j.len(54); // assemblyTables
            CyberpunkVR_RetainedSourceAssemblyMissingCbvs = j.ptr(55); // assemblyMissingCbvs
            CyberpunkVR_RetainedSourceAssemblyMissingCbvCount = j.len(55); // assemblyMissingCbvs
            CyberpunkVR_RetainedT5OverlayRows = j.ptr(56); // overlayRows
            CyberpunkVR_RetainedT5OverlayCount = j.len(56); // overlayRows
            CyberpunkVR_RetainedT5OverlayBlob = j.ptr_blob(7); // overlayBlob
            CyberpunkVR_RetainedT5OverlayBlobBytes = j.len_blob(7); // overlayBlob
        }
        CyberpunkVR_RetainedIaCommands = j.ptr(36); // iaCommands
        CyberpunkVR_RetainedIaCommandCount = j.len(36); // iaCommands
        CyberpunkVR_RetainedIaObservations = j.ptr(40); // iaObservations
        CyberpunkVR_RetainedIaObservationCount = j.len(40); // iaObservations
        CyberpunkVR_RetainedIaViews = j.ptr(37); // iaViews
        CyberpunkVR_RetainedIaViewCount = j.len(37); // iaViews
        CyberpunkVR_RetainedIaReuseRows = j.ptr(38); // iaReuse
        CyberpunkVR_RetainedIaReuseCount = j.len(38); // iaReuse
        CyberpunkVR_RetainedIaTransferRows = j.ptr(39); // iaTransfers
        CyberpunkVR_RetainedIaTransferCount = j.len(39); // iaTransfers
        CyberpunkVR_RetainedInstanceRows = j.ptr(19); // instanceRows
        CyberpunkVR_RetainedInstanceCount = j.len(19); // instanceRows
        CyberpunkVR_RetainedInstanceBlob = j.ptr_blob(0); // instanceBlob
        CyberpunkVR_RetainedInstanceBlobBytes = j.len_blob(0); // instanceBlob
        CyberpunkVR_RetainedInstanceGpuRows = j.ptr(20); // instanceGpuRows
        CyberpunkVR_RetainedInstanceGpuCount = j.len(20); // instanceGpuRows
        CyberpunkVR_RetainedInstanceGpuBlob = j.ptr_blob(1); // instanceGpuBlob
        CyberpunkVR_RetainedInstanceGpuBlobBytes = j.len_blob(1); // instanceGpuBlob
        CyberpunkVR_RetainedInstanceOverlayRows = j.ptr(21); // instanceOverlayRows
        CyberpunkVR_RetainedInstanceOverlayCount = j.len(21); // instanceOverlayRows
        CyberpunkVR_RetainedInstanceOverlayBlob = j.ptr_blob(2); // instanceOverlayBlob
        CyberpunkVR_RetainedInstanceOverlayBlobBytes = j.len_blob(2); // instanceOverlayBlob
        CyberpunkVR_RetainedIaSelections = j.ptr(22); // iaSelections
        CyberpunkVR_RetainedIaSelectionCount = j.len(22); // iaSelections
        CyberpunkVR_RetainedPrivateGeometryRows = j.ptr(23); // privateGeometryRows
        CyberpunkVR_RetainedPrivateGeometryCount = j.len(23); // privateGeometryRows
        CyberpunkVR_RetainedMaterialRows = j.ptr(15); // materialRows
        CyberpunkVR_RetainedMaterialCount = j.len(15); // materialRows
        CyberpunkVR_RetainedMaterialSources = j.ptr(18); // materialSourceRows
        CyberpunkVR_RetainedMaterialSourceCount = j.len(18); // materialSourceRows
        CyberpunkVR_RetainedMaterialBufferRows = j.ptr(16); // materialBufferRows
        CyberpunkVR_RetainedMaterialBufferCount = j.len(16); // materialBufferRows
        CyberpunkVR_RetainedMaterialBufferCopyRows = j.ptr(17); // materialBufferCopies
        CyberpunkVR_RetainedMaterialBufferCopyCount = j.len(17); // materialBufferCopies
        publish_stats(
            j,
            p,
            Stats {
                stream: &mut CyberpunkVR_RetainedSourceStreamStats,
                indirect: &mut CyberpunkVR_RetainedIndirectBindingsStats,
                cbvs: &mut CyberpunkVR_RetainedSourceCbvStats,
                snapshots: &mut CyberpunkVR_RetainedIndirectSnapshotsStats,
                tables: &mut CyberpunkVR_RetainedSourceTablesStats,
                details: &mut CyberpunkVR_RetainedSourceDetailsStats,
                cbv_snapshots: &mut CyberpunkVR_RetainedSourceCbvSnapshotsStats,
                selectors: &mut CyberpunkVR_RetainedSourceSelectorsStats,
                assembly: &mut CyberpunkVR_RetainedSourceAssemblyStats,
                assembly_cache: &mut CyberpunkVR_RetainedSourceAssemblyCacheStats,
                bank: &mut CyberpunkVR_RetainedSourceAssemblyBankStats,
                bank_failure: &mut CyberpunkVR_RetainedSourceAssemblyBankFailure,
                overlay: &mut CyberpunkVR_RetainedT5OverlayStats,
                instance_overlay: &mut CyberpunkVR_RetainedInstanceOverlayStats,
            },
        );
    }
}

// ---- the C ABI (the C++ facades; every call under the capture's lock)

#[no_mangle]
pub extern "C-unwind" fn cvr_nrg_journal_new() -> *mut Journal {
    Box::into_raw(Box::default())
}
/// # Safety
/// `journal` came from cvr_nrg_journal_new (or is null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_journal_free(journal: *mut Journal) {
    if !journal.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(journal) });
    }
}
/// Whether `kind` is a table of rows of `width`.
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg_journal_width(kind: u32, width: usize) -> bool {
    ROWS.get(kind as usize).is_some_and(|(_, w)| *w == width)
}
/// The table's rows (null until it holds memory) and their count.
///
/// # Safety
/// `journal` is live; `count` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_rows(journal: *mut Journal, kind: u32, count: *mut usize) -> *mut u64 {
    // SAFETY: as documented.
    let (j, count) = unsafe { (&mut *journal, &mut *count) };
    if kind as usize >= ROWS.len() {
        *count = 0;
        return core::ptr::null_mut();
    }
    *count = j.count(kind);
    let table = &mut j.rows[kind as usize];
    if table.capacity() == 0 { core::ptr::null_mut() } else { table.as_mut_ptr() }
}
/// # Safety
/// `journal` is live; `rows` holds `count` whole rows.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_rows_append(journal: *mut Journal, kind: u32, rows: *const u64, count: usize) {
    if kind as usize >= ROWS.len() || count == 0 {
        return;
    }
    // SAFETY: as documented.
    unsafe { (*journal).append(kind, core::slice::from_raw_parts(rows, count * Journal::width(kind))) };
}
/// # Safety
/// `journal` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_rows_reserve(journal: *mut Journal, kind: u32, count: usize) {
    if (kind as usize) < ROWS.len() {
        // SAFETY: as documented.
        unsafe { (*journal).reserve(kind, count) };
    }
}
/// # Safety
/// `journal` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_rows_clear(journal: *mut Journal, kind: u32) {
    if (kind as usize) < ROWS.len() {
        // SAFETY: as documented.
        unsafe { (*journal).clear(kind) };
    }
}
/// The blob's bytes (null until it holds memory) and their count.
///
/// # Safety
/// `journal` is live; `bytes` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_blob(journal: *mut Journal, blob: u32, bytes: *mut usize) -> *mut u8 {
    // SAFETY: as documented.
    let (j, bytes) = unsafe { (&mut *journal, &mut *bytes) };
    let Some(value) = j.blobs.get_mut(blob as usize) else {
        *bytes = 0;
        return core::ptr::null_mut();
    };
    *bytes = value.len();
    if value.capacity() == 0 { core::ptr::null_mut() } else { value.as_mut_ptr() }
}
/// # Safety
/// `journal` is live; `data` holds `count` bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_blob_append(journal: *mut Journal, blob: u32, data: *const u8, count: usize) {
    if (blob as usize) < BLOBS.len() && count != 0 {
        // SAFETY: as documented.
        unsafe { (*journal).append_blob(blob, core::slice::from_raw_parts(data, count)) };
    }
}
/// # Safety
/// `journal` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_blob_clear(journal: *mut Journal, blob: u32) {
    if (blob as usize) < BLOBS.len() {
        // SAFETY: as documented.
        unsafe { (*journal).clear_blob(blob) };
    }
}
/// PublishSources (the C++ half checked the capture and its source stream).
///
/// # Safety
/// `journal` is live; `values` is valid; the capture's lock is held.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_publish_sources(journal: *const Journal, values: *const Publish) {
    // SAFETY: as documented.
    unsafe { publish(&*journal, &*values) };
}

// ---- Poll's payloads (once both eyes' submissions completed: the readbacks read into the journal)

/// One eye's readbacks as Poll reads them (resource addresses, 0 when absent): the instance readback and its bytes, the
/// fence and its signaled value, the verification readback with its planes and target resources, the CBV and T5
/// snapshot readbacks, and the full-T5 readbacks with their bytes (`full_ready`: the arena exists and is finalized).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PayloadEye {
    pub instance_readback: u64,
    pub instance_bytes: u64,
    pub fence: u64,
    pub signaled: u64,
    pub readback: u64,
    pub planes: *const Plane,
    pub plane_count: usize,
    pub targets: [u64; 5],
    pub cbv_readback: u64,
    pub t5_readback: u64,
    pub full_original: u64,
    pub full_private: u64,
    pub full_reference_bytes: u64,
    pub full_arena_bytes: u64,
    pub t5_aborted: bool,
    pub full_ready: bool,
}
/// A source's selector input: its row and its owned intent bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PayloadInput {
    pub row: u64,
    pub intent: *const u8,
    pub intent_len: usize,
    pub has_intent: bool,
}
/// A source as Poll reads it: its T5 candidate's bytes, its selector inputs, its T5 row and plan extent.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PayloadSource {
    pub t5_candidate: *const u8,
    pub t5_candidate_len: usize,
    pub has_t5_candidate: bool,
    pub inputs: *const PayloadInput,
    pub input_count: usize,
    pub t5_row: u64,
    pub t5_byte_extent: u64,
}
/// Poll's payload inputs; `fail` is the capture's Fail, `publish` its PublishSources; the two switches are the
/// capture's own (Poll sets them).
#[repr(C)]
pub struct Payloads {
    pub eyes: [PayloadEye; 2],
    pub sources: *const PayloadSource,
    pub source_count: usize,
    pub mode: u32,
    pub source_cbv_snapshots: bool,
    pub source_selectors: bool,
    pub full_t5: bool,
    pub cbv_snapshots_failed: *mut bool,
    pub cbv_payload_complete: *mut bool,
    pub fail: extern "C" fn(u32),
    pub publish: extern "C" fn(),
}
const _: () = assert!(
    core::mem::size_of::<PayloadEye>() == 152
        && core::mem::size_of::<PayloadInput>() == 32
        && core::mem::size_of::<PayloadSource>() == 56
        && core::mem::size_of::<Payloads>() == 360
);

/// The readbacks' memory (the plugin's: the D3D12 resources and fences).
pub trait Readbacks {
    /// The buffer's width (0 for none).
    fn width(&mut self, resource: u64) -> u64;
    /// Maps [0, end) for reading (null when it fails).
    fn map(&mut self, resource: u64, end: u64) -> *const u8;
    fn unmap(&mut self, resource: u64);
    fn completed(&mut self, fence: u64) -> u64;
}
/// Poll's payload outcome: done, or the device removed (Fail(90) raised; Poll then completes the capture).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polled {
    Done,
    Removed,
}

fn as_rows<const N: usize>(v: &[u64]) -> &[[u64; N]] {
    v.as_chunks::<N>().0
}
fn as_rows_mut<const N: usize>(v: &mut [u64]) -> &mut [[u64; N]] {
    v.as_chunks_mut::<N>().0
}
/// # Safety
/// `data` holds `len` bytes (or is null).
unsafe fn bytes<'a, T>(data: *const T, len: usize) -> &'a [T] {
    // SAFETY: as documented.
    if data.is_null() || len == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, len) } }
}
fn blob_index(name: &str) -> usize {
    BLOBS.iter().position(|b| *b == name).unwrap()
}

/// Poll's payloads: the instance readbacks against the recorded instance bytes, the verification planes, the CBV
/// snapshots' bytes (then PublishSources), the T5 snapshots' bytes against their candidates and the selector inputs,
/// commands and T5 views over them, and the full-T5 copies. The journal is reached only through `journal`, never held
/// across `fail` / `publish` (PublishSources reads it).
///
/// # Safety
/// `journal` is live; the payload arrays and the switch pointers are valid; the capture's lock is held.
pub unsafe fn poll_payloads(
    journal: *mut Journal,
    p: &Payloads,
    io: &mut dyn Readbacks,
    full_stats: &mut [u64; 16],
    different: &mut [u64; 12],
    compared: &mut [u64; 2],
) -> Polled {
    let k = Journal::kind;
    // SAFETY: the lent sources.
    let sources = unsafe { bytes(p.sources, p.source_count) };
    let (instance_blob, gpu_blob, cbv_blob, t5_blob) = (blob_index("instanceBlob"), blob_index("instanceGpuBlob"), blob_index("cbvBlob"), blob_index("t5Blob"));
    for side in 0..2u32 {
        let eye = &p.eyes[side as usize];
        if eye.instance_readback == 0 || eye.instance_bytes == 0 {
            continue;
        }
        let mapped = io.map(eye.instance_readback, eye.instance_bytes);
        if mapped.is_null() {
            (p.fail)(1801);
            continue;
        }
        // SAFETY: the mapped range [0, instance_bytes).
        let image = unsafe { bytes(mapped, eye.instance_bytes as usize) };
        // SAFETY: the journal, no other borrow live.
        let mut rows = core::mem::take(&mut unsafe { &mut *journal }.rows[k("instanceGpuRows") as usize]);
        for row in as_rows_mut::<24>(&mut rows) {
            if row[1] != side as u64 || row[2] == 0 || row[3] == 0 || row[4] == 0 {
                continue;
            }
            // SAFETY: as above.
            let j = unsafe { &mut *journal };
            let recorded = j.blobs[instance_blob].len() as u64;
            if row[10] > eye.instance_bytes || row[11] > eye.instance_bytes - row[10] || row[17] == 0 || row[17] - 1 > recorded || row[11] > recorded - (row[17] - 1) {
                (p.fail)(1802);
                continue;
            }
            let copied = &image[row[10] as usize..(row[10] + row[11]) as usize];
            row[18] = j.blobs[gpu_blob].len() as u64 + 1;
            j.blobs[gpu_blob].extend_from_slice(copied);
            let original = &j.blobs[instance_blob][(row[17] - 1) as usize..(row[17] - 1 + row[11]) as usize];
            row[5] = if copied != original { 2 } else { 1 };
            row[19] = eye.fence;
            row[20] = eye.signaled;
            row[21] = 1;
        }
        // SAFETY: as above.
        unsafe { &mut *journal }.rows[k("instanceGpuRows") as usize] = rows;
        io.unmap(eye.instance_readback);
    }
    for side in 0..2u32 {
        let eye = &p.eyes[side as usize];
        if io.completed(eye.fence) == u64::MAX {
            (p.fail)(90);
            return Polled::Removed;
        }
        if p.mode == 1 && eye.readback != 0 {
            let size = io.width(eye.readback);
            let mapped = io.map(eye.readback, size);
            if mapped.is_null() {
                (p.fail)(91);
                continue;
            }
            // SAFETY: the mapped readback and the eye's lent planes.
            let (image, planes) = unsafe { (bytes(mapped, size as usize), bytes(eye.planes, eye.plane_count)) };
            for plane in planes {
                let target = eye.targets.get(plane.slot as usize).copied().unwrap_or(0);
                let width = if target != 0 { io.width(target) } else { 0 };
                if !plane_differences(image, side, plane, width, different, compared) {
                    (p.fail)(91);
                }
            }
            io.unmap(eye.readback);
        }
    }
    // SAFETY: the capture's switches.
    if p.source_cbv_snapshots && !unsafe { *p.cbv_snapshots_failed } {
        let (mut mapped, mut widths, mut valid) = ([core::ptr::null::<u8>(); 2], [0u64; 2], true);
        for side in 0..2 {
            let readback = p.eyes[side].cbv_readback;
            if readback != 0 {
                widths[side] = io.width(readback);
                mapped[side] = io.map(readback, widths[side]);
                valid &= !mapped[side].is_null();
            }
        }
        if valid {
            // SAFETY: the journal, no other borrow live.
            let j = unsafe { &mut *journal };
            let mut rows = core::mem::take(&mut j.rows[k("cbvSnapshotRows") as usize]);
            for row in as_rows_mut::<32>(&mut rows) {
                if row[8] != 1 {
                    continue;
                }
                let side = row[1] as usize;
                if side >= 2
                    || mapped[side].is_null()
                    || p.eyes[side].cbv_readback == 0
                    || row[29] == 0
                    || row[30] == 0
                    || row[19] > widths[side]
                    || row[20] > widths[side] - row[19]
                {
                    valid = false;
                    break;
                }
                row[31] = j.blobs[cbv_blob].len() as u64 + 1;
                // SAFETY: inside the mapped range [0, width).
                let image = unsafe { bytes(mapped[side], widths[side] as usize) };
                j.blobs[cbv_blob].extend_from_slice(&image[row[19] as usize..(row[19] + row[20]) as usize]);
            }
            j.rows[k("cbvSnapshotRows") as usize] = rows;
        }
        for side in 0..2 {
            if !mapped[side].is_null() {
                io.unmap(p.eyes[side].cbv_readback);
            }
        }
        // SAFETY: the capture's switches; the journal, no other borrow live.
        unsafe {
            if !valid {
                (&mut *journal).blobs[cbv_blob].clear();
                *p.cbv_snapshots_failed = true;
                (p.fail)(1121);
            } else {
                *p.cbv_payload_complete = true;
            }
        }
        (p.publish)();
    }
    // SAFETY: the capture's switch.
    if p.source_selectors && unsafe { *p.cbv_payload_complete } {
        let (mut mapped, mut widths) = ([core::ptr::null::<u8>(); 2], [0u64; 2]);
        for side in 0..2 {
            let readback = p.eyes[side].t5_readback;
            if readback != 0 && !p.eyes[side].t5_aborted {
                widths[side] = io.width(readback);
                mapped[side] = io.map(readback, widths[side]);
                if mapped[side].is_null() {
                    (p.fail)(1451);
                }
            }
        }
        // SAFETY: the journal, no other borrow live.
        let mut rows = core::mem::take(&mut unsafe { &mut *journal }.rows[k("t5SnapshotRows") as usize]);
        for row in as_rows_mut::<32>(&mut rows) {
            if row[2] != 1 || row[23] == 0 || row[24] == 0 {
                continue;
            }
            let side = row[1] as usize;
            if side >= 2 || mapped[side].is_null() || p.eyes[side].t5_readback == 0 || row[17] > widths[side] || row[18] > widths[side] - row[17] {
                (p.fail)(1452);
                continue;
            }
            // SAFETY: inside the mapped range; the journal, no other borrow live.
            let (image, j) = unsafe { (bytes(mapped[side], widths[side] as usize), &mut *journal) };
            let copied = &image[row[17] as usize..(row[17] + row[18]) as usize];
            row[27] = j.blobs[t5_blob].len() as u64 + 1;
            j.blobs[t5_blob].extend_from_slice(copied);
            if let Some(source) = sources.get(row[0] as usize).filter(|s| s.has_t5_candidate && s.t5_candidate_len as u64 == row[18]) {
                // SAFETY: the candidate's owned bytes.
                row[28] = if unsafe { bytes(source.t5_candidate, source.t5_candidate_len) } != copied { 2 } else { 1 };
            }
        }
        // SAFETY: as above.
        unsafe { &mut *journal }.rows[k("t5SnapshotRows") as usize] = rows;
        for side in 0..2 {
            if !mapped[side].is_null() {
                io.unmap(p.eyes[side].t5_readback);
            }
        }
        // SAFETY: the journal, no callback below.
        let j = unsafe { &mut *journal };
        let mut inputs = core::mem::take(&mut j.rows[k("selectorInputs") as usize]);
        for source in sources {
            // SAFETY: the source's lent inputs.
            for input in unsafe { bytes(source.inputs, source.input_count) } {
                let Some(row) = as_rows_mut::<24>(&mut inputs).get_mut(input.row as usize) else { continue };
                if !input.has_intent {
                    continue;
                }
                // SAFETY: the input's owned intent bytes.
                let owned = unsafe { bytes(input.intent, input.intent_len) };
                match_input(row, as_rows::<32>(&j.rows[k("cbvSnapshotRows") as usize]), &j.blobs[cbv_blob], owned);
            }
        }
        let mut commands = core::mem::take(&mut j.rows[k("selectorCommands") as usize]);
        for command in as_rows_mut::<32>(&mut commands) {
            if command[2] != 1 {
                continue;
            }
            let Some(source) = sources.get(command[0] as usize) else { continue };
            // SAFETY: as above.
            let rows: Vec<u64> = unsafe { bytes(source.inputs, source.input_count) }.iter().map(|i| i.row).collect();
            selector_command(
                command,
                &rows,
                as_rows::<24>(&inputs),
                as_rows::<32>(&j.rows[k("t5Rows") as usize]),
                as_rows::<32>(&j.rows[k("t5SnapshotRows") as usize]),
                source.t5_row,
                source.t5_byte_extent,
            );
        }
        j.rows[k("selectorCommands") as usize] = commands;
        let mut views = core::mem::take(&mut j.rows[k("t5Rows") as usize]);
        for row in as_rows_mut::<32>(&mut views) {
            let Some(source) = sources.get(row[0] as usize) else { continue };
            // SAFETY: as above.
            let first = unsafe { bytes(source.inputs, source.input_count) }.first().map_or(u64::MAX, |i| i.row);
            t5_view(row, first, as_rows::<24>(&inputs));
        }
        j.rows[k("t5Rows") as usize] = views;
        j.rows[k("selectorInputs") as usize] = inputs;
    }
    if p.full_t5 {
        for side in 0..2u32 {
            let eye = &p.eyes[side as usize];
            let mut valid = eye.full_ready && eye.full_original != 0 && eye.full_private != 0;
            let (mut original, mut copied) = (core::ptr::null::<u8>(), core::ptr::null::<u8>());
            if valid {
                original = io.map(eye.full_original, eye.full_reference_bytes);
                valid = !original.is_null();
            }
            if valid {
                copied = io.map(eye.full_private, eye.full_arena_bytes);
                valid = !copied.is_null();
            }
            // SAFETY: the mapped ranges; the journal, no other borrow live.
            let (a, b, j) = unsafe {
                (
                    bytes(original, if original.is_null() { 0 } else { eye.full_reference_bytes as usize }),
                    bytes(copied, if copied.is_null() { 0 } else { eye.full_arena_bytes as usize }),
                    &mut *journal,
                )
            };
            valid = full_t5_rows(as_rows_mut::<16>(&mut j.rows[k("fullT5Rows") as usize]), side, valid, a, b, full_stats);
            if !original.is_null() {
                io.unmap(eye.full_original);
            }
            if !copied.is_null() {
                io.unmap(eye.full_private);
            }
            if !valid {
                if full_stats[11] == 0 {
                    full_stats[11] = 6;
                }
                (p.fail)(2706);
            }
        }
        if full_stats[10] != 0 || full_stats[3] == 0 || full_stats[9] != full_stats[3] {
            if full_stats[11] == 0 {
                full_stats[11] = 7;
            }
            (p.fail)(2707);
        }
    }
    Polled::Done
}

/// The plugin's readbacks (D3D12 buffers and fences by address).
struct PluginReadbacks;
fn resource(address: u64) -> Option<ID3D12Resource> {
    let raw = address as *mut core::ffi::c_void;
    // SAFETY: the capture's held readback or target, alive for the call.
    unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.cloned()
}
impl Readbacks for PluginReadbacks {
    fn width(&mut self, address: u64) -> u64 {
        // SAFETY: a live resource.
        resource(address).map_or(0, |r| unsafe { r.GetDesc() }.Width)
    }
    fn map(&mut self, address: u64, end: u64) -> *const u8 {
        let Some(r) = resource(address) else { return core::ptr::null() };
        let range = D3D12_RANGE { Begin: 0, End: end as usize };
        let mut mapped = core::ptr::null_mut();
        // SAFETY: a live readback buffer; the range inside it.
        match unsafe { r.Map(0, Some(&range), Some(&mut mapped)) } {
            Ok(()) => mapped as *const u8,
            Err(_) => core::ptr::null(),
        }
    }
    fn unmap(&mut self, address: u64) {
        if let Some(r) = resource(address) {
            // SAFETY: a buffer this read mapped; nothing written.
            unsafe { r.Unmap(0, Some(&D3D12_RANGE::default())) };
        }
    }
    fn completed(&mut self, address: u64) -> u64 {
        let raw = address as *mut core::ffi::c_void;
        // SAFETY: the eye's held fence.
        unsafe { ID3D12Fence::from_raw_borrowed(&raw) }.map_or(0, |f| unsafe { f.GetCompletedValue() })
    }
}
/// Poll's payloads; true when the device was removed (Fail(90) raised).
///
/// # Safety
/// `journal` is live; `payloads` is valid; the capture's lock is held.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_poll_payloads(journal: *mut Journal, payloads: *const Payloads) -> bool {
    // SAFETY: as documented; the exports are the capture's, under its lock.
    unsafe {
        poll_payloads(
            journal,
            &*payloads,
            &mut PluginReadbacks,
            &mut CyberpunkVR_RetainedFullT5Stats,
            &mut CyberpunkVR_RetainedGeometryDifferent,
            &mut CyberpunkVR_RetainedGeometryBytes,
        ) == Polled::Removed
    }
}

#[cfg(test)]
mod tests {
    //! The journal's tables and blobs; each stats row from a small capture; Poll's payloads read from fake readbacks.
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static EVENTS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    }
    extern "C" fn fail(reason: u32) {
        EVENTS.with(|e| e.borrow_mut().push(reason));
    }
    extern "C" fn publish() {
        EVENTS.with(|e| e.borrow_mut().push(0));
    }
    fn events() -> Vec<u32> {
        EVENTS.with(|e| core::mem::take(&mut *e.borrow_mut()))
    }
    /// Readback buffers and fences by address.
    #[derive(Default)]
    struct Memory {
        buffers: HashMap<u64, Vec<u8>>,
        fences: HashMap<u64, u64>,
        mapped: Vec<u64>,
    }
    impl Readbacks for Memory {
        fn width(&mut self, r: u64) -> u64 {
            self.buffers.get(&r).map_or(0, |b| b.len() as u64)
        }
        fn map(&mut self, r: u64, end: u64) -> *const u8 {
            match self.buffers.get(&r) {
                Some(b) if end <= b.len() as u64 => {
                    self.mapped.push(r);
                    b.as_ptr()
                }
                _ => core::ptr::null(),
            }
        }
        fn unmap(&mut self, r: u64) {
            let at = self.mapped.iter().position(|&m| m == r).expect("unmapped twice");
            self.mapped.remove(at);
        }
        fn completed(&mut self, f: u64) -> u64 {
            self.fences.get(&f).copied().unwrap_or(1)
        }
    }
    fn eye() -> PayloadEye {
        PayloadEye {
            instance_readback: 0,
            instance_bytes: 0,
            fence: 0,
            signaled: 0,
            readback: 0,
            planes: core::ptr::null(),
            plane_count: 0,
            targets: [0; 5],
            cbv_readback: 0,
            t5_readback: 0,
            full_original: 0,
            full_private: 0,
            full_reference_bytes: 0,
            full_arena_bytes: 0,
            t5_aborted: false,
            full_ready: false,
        }
    }
    struct Case {
        j: Journal,
        io: Memory,
        failed: bool,
        complete: bool,
        sources: Vec<PayloadSource>,
        p: Payloads,
    }
    fn case() -> Box<Case> {
        let mut c = Box::new(Case {
            j: Journal::default(),
            io: Memory::default(),
            failed: false,
            complete: false,
            sources: Vec::new(),
            p: Payloads {
                eyes: [eye(), eye()],
                sources: core::ptr::null(),
                source_count: 0,
                mode: 0,
                source_cbv_snapshots: false,
                source_selectors: false,
                full_t5: false,
                cbv_snapshots_failed: core::ptr::null_mut(),
                cbv_payload_complete: core::ptr::null_mut(),
                fail,
                publish,
            },
        });
        c.p.cbv_snapshots_failed = &mut c.failed;
        c.p.cbv_payload_complete = &mut c.complete;
        c.p.eyes[0].fence = 0x10;
        c.p.eyes[1].fence = 0x11;
        c
    }
    fn poll(c: &mut Case) -> (Polled, [u64; 16]) {
        c.p.sources = c.sources.as_ptr();
        c.p.source_count = c.sources.len();
        let (mut stats, mut different, mut compared) = ([0u64; 16], [0u64; 12], [0u64; 2]);
        events();
        // SAFETY: the case's journal and arrays outlive the call.
        let polled = unsafe { poll_payloads(&mut c.j, &c.p, &mut c.io, &mut stats, &mut different, &mut compared) };
        assert!(c.io.mapped.is_empty(), "every mapped readback unmapped");
        (polled, stats)
    }

    #[test]
    fn instance_readbacks() {
        let mut c = case();
        let gpu = Journal::kind("instanceGpuRows");
        c.j.append_blob(blob_index("instanceBlob") as u32, &[9, 1, 2, 3, 4]);
        c.io.buffers.insert(0x100, vec![0, 1, 2, 3, 4, 5, 6, 7]);
        c.p.eyes[0].instance_readback = 0x100;
        c.p.eyes[0].instance_bytes = 8;
        c.p.eyes[0].signaled = 5;
        let same = row(gpu, &[(1, 0), (2, 1), (3, 1), (4, 1), (10, 1), (11, 4), (17, 2)]);
        let other = row(gpu, &[(1, 0), (2, 1), (3, 1), (4, 1), (10, 2), (11, 2), (17, 2)]);
        let beyond = row(gpu, &[(1, 0), (2, 1), (3, 1), (4, 1), (10, 6), (11, 4), (17, 2)]);
        let side1 = row(gpu, &[(1, 1), (2, 1), (3, 1), (4, 1), (10, 6), (11, 4), (17, 2)]);
        c.j.append(gpu, &[same, other, beyond, side1].concat());
        assert_eq!(poll(&mut c).0, Polled::Done);
        assert_eq!(events(), vec![1802], "the row beyond the readback refused");
        let rows = as_rows::<24>(c.j.rows(gpu));
        assert_eq!((rows[0][5], rows[0][18], rows[0][19], rows[0][20], rows[0][21]), (1, 1, 0x10, 5, 1), "the bytes as recorded");
        assert_eq!((rows[1][5], rows[1][18]), (2, 5), "other bytes; after the first copy");
        assert_eq!((rows[2][5], rows[3][5]), (0, 0), "refused; the other eye's readback absent");
        assert_eq!(c.j.blob(blob_index("instanceGpuBlob") as u32), &[1, 2, 3, 4, 2, 3]);
        c.io.fences.insert(0x11, u64::MAX);
        assert_eq!(poll(&mut c).0, Polled::Removed);
        assert_eq!(events().last(), Some(&90));
    }
    #[test]
    fn cbv_snapshots() {
        let mut c = case();
        let snapshots = Journal::kind("cbvSnapshotRows");
        c.p.source_cbv_snapshots = true;
        c.io.buffers.insert(0x200, vec![5, 6, 7, 8]);
        c.p.eyes[1].cbv_readback = 0x200;
        c.j.append(snapshots, &[row(snapshots, &[(1, 1), (8, 1), (19, 1), (20, 2), (29, 1), (30, 1)]), row(snapshots, &[(8, 2)])].concat());
        poll(&mut c);
        assert_eq!(events(), vec![0], "published once");
        assert!(c.complete && !c.failed);
        assert_eq!(as_rows::<32>(c.j.rows(snapshots))[0][31], 1);
        assert_eq!(c.j.blob(blob_index("cbvBlob") as u32), &[6, 7]);
        let mut c = case();
        c.p.source_cbv_snapshots = true;
        c.io.buffers.insert(0x200, vec![5, 6, 7, 8]);
        c.p.eyes[1].cbv_readback = 0x200;
        c.j.append(snapshots, &[row(snapshots, &[(1, 1), (8, 1), (19, 1), (20, 2), (29, 1), (30, 1)]), row(snapshots, &[(1, 1), (8, 1), (19, 3), (20, 2), (29, 1), (30, 1)])].concat());
        poll(&mut c);
        assert_eq!(events(), vec![1121, 0], "a copy beyond the readback: failed, then published");
        assert!(c.failed && !c.complete);
        assert!(c.j.blob(blob_index("cbvBlob") as u32).is_empty(), "the blob cleared");
    }
    #[test]
    fn t5_snapshots_and_full_copies() {
        let mut c = case();
        let t5 = Journal::kind("t5SnapshotRows");
        c.p.source_selectors = true;
        c.complete = true;
        c.io.buffers.insert(0x300, vec![1, 2, 3, 4]);
        c.p.eyes[0].t5_readback = 0x300;
        let candidate = [2u8, 3];
        c.sources.push(PayloadSource {
            t5_candidate: candidate.as_ptr(),
            t5_candidate_len: 2,
            has_t5_candidate: true,
            inputs: core::ptr::null(),
            input_count: 0,
            t5_row: u64::MAX,
            t5_byte_extent: 0,
        });
        c.j.append(t5, &[row(t5, &[(2, 1), (17, 1), (18, 2), (23, 1), (24, 1)]), row(t5, &[(2, 1), (17, 3), (18, 2), (23, 1), (24, 1)])].concat());
        poll(&mut c);
        assert_eq!(events(), vec![1452], "the second copy leaves the readback");
        let rows = as_rows::<32>(c.j.rows(t5));
        assert_eq!((rows[0][27], rows[0][28], rows[1][27]), (1, 1, 0), "the candidate's bytes matched");
        let mut c = case();
        c.p.full_t5 = true;
        let (_, stats) = poll(&mut c);
        assert_eq!(events(), vec![2706, 2706, 2707], "no arenas: each eye refused, then the whole");
        assert_eq!(stats[11], 6, "the first reason kept");
    }

    fn row(kind: u32, set: &[(usize, u64)]) -> Vec<u64> {
        let mut r = vec![0; Journal::width(kind)];
        for &(i, v) in set {
            r[i] = v;
        }
        r
    }
    #[test]
    fn tables_and_blobs() {
        let mut j = Journal::default();
        let source = Journal::kind("sourceRows");
        assert_eq!(Journal::width(source), 48);
        assert!(cvr_nrg_journal_width(source, 48) && !cvr_nrg_journal_width(source, 24) && !cvr_nrg_journal_width(99, 48));
        assert_eq!((j.ptr(source), j.len(source)), (0, 0), "no memory yet: a null pointer, as an empty std::vector's");
        j.reserve(source, 4);
        assert_ne!(j.ptr(source), 0);
        let held = j.ptr(source);
        j.append(source, &row(source, &[(0, 7)]));
        j.append(source, &[row(source, &[(0, 8)]), row(source, &[(0, 9)])].concat());
        assert_eq!(j.ptr(source), held, "rows within the reserved room do not move");
        assert_eq!(j.len(source), 3);
        assert_eq!(j.rows(source)[48 * 2], 9);
        j.clear(source);
        assert_eq!((j.len(source), j.ptr(source)), (0, held), "clear keeps the memory");
        let blob = BLOBS.iter().position(|b| *b == "cbvBlob").unwrap() as u32;
        j.append_blob(blob, &[1, 2, 3]);
        assert_eq!((j.blob(blob), j.len_blob(blob)), (&[1u8, 2, 3][..], 3));
        j.clear_blob(blob);
        assert_eq!(j.len_blob(blob), 0);
        // The C ABI's facade calls.
        let raw = cvr_nrg_journal_new();
        let mut n = 0usize;
        unsafe {
            assert!(cvr_nrg_rows(raw, source, &mut n).is_null());
            let r = row(source, &[(1, 5)]);
            cvr_nrg_rows_append(raw, source, r.as_ptr(), 1);
            let p = cvr_nrg_rows(raw, source, &mut n);
            assert_eq!((n, *p.add(1)), (1, 5));
            assert!(cvr_nrg_rows(raw, 99, &mut n).is_null() && n == 0);
            cvr_nrg_blob_append(raw, blob, [4u8, 5].as_ptr(), 2);
            assert_eq!(*cvr_nrg_blob(raw, blob, &mut n).add(1), 5);
            assert_eq!(n, 2);
            cvr_nrg_journal_free(raw);
        }
    }
    struct Rows {
        a: [[u64; 16]; 9],
        cache: [u64; 4],
        bank: [u64; 6],
        failure: [u64; 4],
        overlay: [u64; 16],
        instance: [u64; 8],
    }
    fn run(j: &Journal, p: &Publish, r: &mut Rows) {
        let [stream, indirect, cbvs, snapshots, tables, details, cbv_snapshots, selectors, assembly] = &mut r.a;
        publish_stats(
            j,
            p,
            Stats {
                stream,
                indirect,
                cbvs,
                snapshots,
                tables,
                details,
                cbv_snapshots,
                selectors,
                assembly,
                assembly_cache: &mut r.cache,
                bank: &mut r.bank,
                bank_failure: &mut r.failure,
                overlay: &mut r.overlay,
                instance_overlay: &mut r.instance,
            },
        );
    }
    fn rows() -> Rows {
        Rows { a: [[0xEE; 16]; 9], cache: [0xEE; 4], bank: [0xEE; 6], failure: [0xEE; 4], overlay: [0xEE; 16], instance: [0xEE; 8] }
    }
    fn capture() -> Journal {
        let mut j = Journal::default();
        let source = Journal::kind("sourceRows");
        j.append(source, &[row(source, &[(1, 0), (2, 1)]), row(source, &[(1, 1)])].concat());
        let indirect = Journal::kind("indirectRows");
        j.append(indirect, &row(indirect, &[(3, 1)]));
        j.append(Journal::kind("overlayRows"), &row(Journal::kind("overlayRows"), &[(28, 1)]));
        j.append(Journal::kind("assemblyTables"), &[0; 16]);
        j
    }
    #[test]
    fn stream_and_switched_rows() {
        let j = capture();
        let mut r = rows();
        let p = Publish { frame: 9, source_count: 5, sealed: [true, true], ..Default::default() };
        run(&j, &p, &mut r);
        let stream = r.a[0];
        assert_eq!(&stream[..3], &[2, 9, 2], "both eyes sealed: state 2; the frame; two sources");
        assert_eq!((stream[3], stream[4], stream[5], stream[6]), (1, 1, 1, 1), "one refused, one kept; one per side");
        assert_eq!(stream[11], 0, "not compiled: no sources counted");
        assert_eq!(stream[12], 0xEE, "word 12 is not PublishSources'");
        assert_eq!(&stream[13..], &[0, 0, 0]);
        assert_eq!(&r.a[1][..4], &[2, 9, 1, 1], "the indirect bindings: the stream's state, one with arguments");
        for off in 2..9 {
            assert_eq!(r.a[off], [0xEE; 16], "row {off}: its capture is off");
        }
        assert_eq!(r.bank, [0xEE; 6]);
        assert_eq!(r.instance, [0; 8], "the instance overlay always: stage 0, no eyes");
        let p = Publish { source_failed: true, source_compiled: true, source_count: 5, ..p };
        run(&j, &p, &mut r);
        assert_eq!((r.a[0][0], r.a[0][11]), (4, 5), "failed wins; compiled: its source count");
        let p = Publish { source_failed: false, ..p };
        run(&j, &p, &mut r);
        assert_eq!(r.a[0][0], 3);
    }
    #[test]
    fn capture_rows() {
        let j = capture();
        let mut r = rows();
        let p = Publish {
            frame: 3,
            source_count: 2,
            sealed: [true, false],
            source_cbvs: true,
            cbv_failed: true,
            source_snapshots: true,
            source_tables: true,
            table_cache: [11, 12, 13, 14],
            source_details: true,
            details_failed: true,
            source_cbv_snapshots: true,
            cbv_payload_complete: true,
            arenas_finalized: 2,
            source_selectors: true,
            assembly_complete: true,
            assembly_cache: [1, 2, 3, 4],
            bank_attempted: true,
            bank_present: true,
            bank: [6, 7],
            bank_result: -2147024809,
            bank_failure: [9, 8, 7, 6],
            overlay_stage: 2,
            overlay_present: true,
            overlay_bytes: [100, 50],
            instance_overlay_stage: 3,
            instance: [
                PublishEye { present: 1, versions: 2, copied: 10, required: 20, signaled: 5, completed: 4 },
                PublishEye { present: 1, versions: 1, copied: 1, required: 2, signaled: 7, completed: 3 },
            ],
            ..Default::default()
        };
        run(&j, &p, &mut r);
        assert_eq!(&r.a[2][..4], &[4, 3, 2, 0], "CBVs failed: 4; sources; no CBV rows");
        assert_eq!((r.a[3][0], r.a[3][10], r.a[3][11]), (1, 1, 0), "snapshots: the stream's state, one sealed eye");
        assert_eq!((r.a[4][0], r.a[4][8], r.a[4][9]), (1, 11, 12), "tables: the cache's tables and descriptors");
        assert_eq!(r.a[5][0], 4, "details failed");
        assert_eq!((r.a[6][0], r.a[6][9]), (3, 2), "CBV snapshots: the payload, two finalized arenas");
        assert_eq!((r.a[7][0], r.a[7][13], r.a[7][14], r.a[7][15]), (2, 13, 14, 0), "selectors: the payload, the slot cache");
        assert_eq!((r.a[8][0], r.a[8][7], r.a[8][12]), (3, 1, 0), "assembly complete with one table");
        assert_eq!(r.cache, [1, 2, 3, 4]);
        assert_eq!(r.bank, [1, 1, 6, 7, 0x8007_0057, 0]);
        assert_eq!(r.failure, [9, 8, 7, 6]);
        assert_eq!(&r.overlay[..5], &[2, 3, 1, 100, 50]);
        assert_eq!((r.overlay[11], r.overlay[12], r.overlay[13]), (1, 1, 1));
        assert_eq!(r.instance, [3, 3, 11, 22, 7, 3, 0, 0], "summed versions and bytes, the highest signaled, the lowest completed");
        let p = Publish { bank_present: false, assembly_truncated: true, overlay_present: false, instance: [PublishEye::default(); 2], ..p };
        run(&j, &p, &mut r);
        assert_eq!(r.bank, [1, 0, 0, 0, 0x8007_0057, 0], "no bank: no counts");
        assert_eq!((r.a[8][0], r.a[8][12]), (4, 1));
        assert_eq!(&r.overlay[3..5], &[0, 0]);
        assert_eq!(r.instance, [3, 0, 0, 0, 0, 0, 0, 0]);
    }
}
