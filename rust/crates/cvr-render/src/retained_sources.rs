//! THE RETAINED SOURCES' OWN STATE AND CAPTURE (formerly src/Render/NativeRetainedGeometry.cpp's source half, moving
//! here in one slice -- see the scratch plan): the selector catalog, the intent candidates, each source's captured
//! tables, dependencies, selector inputs, result, slots, T5 read plan and candidate, CaptureTables and the selector
//! capture. A child of retained_geometry_native.rs (its work, journal, sources and side tables).
//!
use super::*;
use windows::Win32::Graphics::Direct3D12::*;
use crate::multilayer_selectors as ml;
use crate::source_selector_plan as sel;

// ---- small helpers

fn release_raw(raw: *mut c_void) {
    if !raw.is_null() {
        // SAFETY: a reference this value holds, released once.
        drop(unsafe { IUnknown::from_raw(raw) });
    }
}
fn text(s: &str) -> sel::Text {
    s.as_bytes().to_vec()
}
fn family_key(f: &crate::families_native::Family) -> sel::FamilyKey {
    sel::FamilyKey { root: text(&f.root_sha), vertex: text(&f.vertex_sha), pixel: text(&f.pixel_sha) }
}
fn ml_str(t: &[u8]) -> ml::Str {
    ml::Str { ptr: t.as_ptr(), len: t.len() }
}

// ---- the selector catalog (source_selectors::Catalog, loaded once for the process)

/// The process's selector catalog (the .cpp reads the CVR_STEREO_SOURCE_SELECTORS file through jsoncpp and hands the
/// tree to source_selector_plan.rs); None when absent or invalid.
pub(super) fn selector_catalog() -> Option<&'static sel::Catalog> {
    static CATALOG: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    // SAFETY: the host's loader (its handle kept for the process).
    let handle = *CATALOG.get_or_init(|| unsafe { (host().selector_catalog)() } as usize);
    sel::catalog_of(handle as *mut c_void)
}

// ---- the intent candidate (source_resource_intent::Candidate)

pub(super) struct Candidate {
    captured: crate::source_intent::Captured,
    _destination: ID3D12Resource,
    view_bytes: usize,
}
impl Candidate {
    /// CaptureAtDraw (`structured` false) / CaptureStructuredAtDraw.
    fn capture(resource: *mut c_void, address: u64, bytes: usize, structured: bool) -> Option<Candidate> {
        // SAFETY: the source's live resource (or null).
        let destination = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }.cloned()?;
        let captured = if structured {
            crate::source_intent::capture_structured_at_draw(resource, address, bytes)
        } else {
            crate::source_intent::capture_at_draw(resource, address, bytes)
        }?;
        Some(Candidate { captured, _destination: destination, view_bytes: bytes })
    }
    /// operator bool.
    pub(super) fn valid(&self) -> bool {
        self.view_bytes != 0
    }
    pub(super) fn bytes(&self) -> &[u8] {
        let start = self.captured.data_offset;
        self.captured.payload.bytes.get(start..start.saturating_add(self.view_bytes)).unwrap_or(&[])
    }
    pub(super) fn copy_sequence(&self) -> u64 {
        self.captured.payload.sequence
    }
    pub(super) fn producer_list(&self) -> u64 {
        self.captured.payload.producer_list
    }
    pub(super) fn view_offset(&self) -> u64 {
        self.captured.view_offset as u64
    }
    pub(super) fn composed(&self) -> bool {
        self.captured.composed
    }
    pub(super) fn pieces(&self) -> &[crate::source_intent::PieceReceipt] {
        &self.captured.pieces
    }
}
fn valid_bytes(c: &Option<Candidate>) -> Option<&[u8]> {
    c.as_ref().filter(|c| c.valid()).map(Candidate::bytes)
}

// ---- a source's selector state

/// Work::Source::SelectorInput.
pub(super) struct SelectorInput {
    pub key: sel::CbvKey,
    pub intent: Option<Candidate>,
    pub row: usize,
}
/// source_selectors::Result.
#[derive(Clone, Default)]
pub(super) struct SelectorResult {
    pub family: sel::FamilyKey,
    pub evaluated_sites: u32,
    pub suppressed: u32,
    pub complete: bool,
    pub permission: bool,
    pub permitted: bool,
    pub selections: Vec<sel::SiteSelection>,
    pub tables: Vec<sel::TableSelection>,
}
/// multilayer_selectors::StructuredView.
#[derive(Clone, Default)]
pub(super) struct StructuredView {
    pub key: sel::CbvKey,
    pub first_element: u64,
    pub num_elements: u32,
    pub structure_byte_stride: u32,
    pub resource_width: u64,
    pub format: u32,
    pub dimension: u32,
    pub component_mapping: u32,
    pub flags: u32,
    pub explicit_description: bool,
}
impl StructuredView {
    fn lend(&self) -> ml::View {
        ml::View {
            root: ml_str(&self.key.root),
            visibility: self.key.visibility,
            shader_register: self.key.shader_register,
            space: self.key.space,
            parameter: self.key.parameter,
            table_offset: self.key.table_offset,
            num_elements: self.num_elements,
            first_element: self.first_element,
            resource_width: self.resource_width,
            structure_byte_stride: self.structure_byte_stride,
            format: self.format,
            dimension: self.dimension,
            component_mapping: self.component_mapping,
            flags: self.flags,
            explicit_description: self.explicit_description,
        }
    }
}
/// multilayer_selectors::ReadPlan.
#[derive(Clone)]
pub(super) struct ReadPlan {
    pub family: sel::FamilyKey,
    pub b4_key: sel::CbvKey,
    pub b4_bytes: [u8; ml::B4_BYTES],
    pub view: StructuredView,
    pub numbers: ml::PlanNumbers,
    pub records: Vec<u32>,
    pub superset: bool,
    pub coverage_complete: bool,
}
fn identity(family: &sel::FamilyKey, key: &sel::CbvKey, b4: &[u8]) -> ml::Identity {
    ml::Identity {
        family_root: ml_str(&family.root),
        family_vertex: ml_str(&family.vertex),
        family_pixel: ml_str(&family.pixel),
        b4_root: ml_str(&key.root),
        b4_visibility: key.visibility,
        b4_register: key.shader_register,
        b4_space: key.space,
        b4_parameter: key.parameter,
        b4_table_offset: key.table_offset,
        b4: b4.as_ptr(),
        b4_len: b4.len(),
    }
}
/// multilayer_selectors::PlanRead.
pub(super) fn plan_read(family: &sel::FamilyKey, key: &sel::CbvKey, b4: &[u8], view: &StructuredView) -> Option<ReadPlan> {
    let lent = view.lend();
    let ident = identity(family, key, b4);
    let mut numbers = ml::PlanNumbers::default();
    let mut failure = ml::Error { message: core::ptr::null(), length: 0 };
    // SAFETY: the lent views live for the call; local outputs.
    let records = unsafe { ml::cvr_ml_plan(&ident, &lent, &mut numbers, &mut failure) };
    if records.is_null() {
        return None;
    }
    // SAFETY: the plan's records, freed once.
    let records = *unsafe { Box::from_raw(records) };
    let mut b4_bytes = [0u8; ml::B4_BYTES];
    let n = b4.len().min(ml::B4_BYTES);
    b4_bytes[..n].copy_from_slice(&b4[..n]);
    Some(ReadPlan {
        family: family.clone(),
        b4_key: key.clone(),
        b4_bytes,
        view: view.clone(),
        numbers,
        records,
        superset: true,
        coverage_complete: true,
    })
}
/// multilayer_selectors::Enumerate of the plan over the snapshot (its view, its resource offset, its bytes or none).
pub(super) fn enumerate(plan: &ReadPlan, snapshot: &StructuredView, offset: u64, bytes: Option<&[u8]>) -> Option<SelectorResult> {
    let ident = identity(&plan.family, &plan.b4_key, &plan.b4_bytes);
    let (plan_view, snapshot_view) = (plan.view.lend(), snapshot.lend());
    let mut out = ml::EnumerationView { selections: core::ptr::null(), selection_count: 0, offsets: core::ptr::null(), offset_count: 0, uses: core::ptr::null(), use_count: 0 };
    let mut failure = ml::Error { message: core::ptr::null(), length: 0 };
    let (data, count) = bytes.map_or((core::ptr::null(), 0), |b| (b.as_ptr(), b.len()));
    // SAFETY: the lent views live for the call; local outputs.
    let enumeration = unsafe {
        ml::cvr_ml_enumerate(&ident, &plan_view, &plan.numbers, plan.records.as_ptr(), plan.records.len(), &snapshot_view, offset, data, count, &mut out, &mut failure)
    };
    if enumeration.is_null() {
        return None;
    }
    let slice = |p: *const u32, n: usize| if p.is_null() || n == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(p, n) } };
    // SAFETY: the enumeration's arrays (alive until freed below).
    let selections = if out.selections.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(out.selections, out.selection_count) } };
    let uses = if out.uses.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(out.uses, out.use_count) } };
    let mut result = SelectorResult { family: plan.family.clone(), ..Default::default() };
    for s in selections {
        result.selections.push(sel::SiteSelection { site_index: s.site, source_dword: s.value, shader_index: s.selected, descriptor_offset: s.selected });
    }
    let offsets = slice(out.offsets, out.offset_count);
    if !offsets.is_empty() {
        result.tables.push(sel::TableSelection {
            key: sel::TableKey { root: text(ml::ROOT_SHA256), visibility: 5, parameter: 31, space: 1, shader_register_base: 0, original_table_offset: 0, declared_extent: ml::DECLARED_EXTENT },
            offsets: offsets.to_vec(),
            count: offsets[offsets.len() - 1] + 1,
            uses: uses.iter().map(|u| sel::SlotInterval { offset: u.offset, count: u.count, kind: 0 }).collect(),
        });
    }
    // SAFETY: the enumeration, freed once.
    unsafe { ml::cvr_ml_enumeration_free(enumeration) };
    result.evaluated_sites = result.selections.len() as u32;
    result.complete = true;
    Some(result)
}

// ---- the owned descriptor rows (OwnedSourceTable, OwnedSourceSlots, SourceResourceBinding: their references held)

/// OwnedSourceTable (its cpu heap, owner and provenance referenced), laid out as the .cpp's and native_descriptor_arena.rs'.
#[repr(transparent)]
pub(super) struct OwnedTable(pub crate::audit_source_tables::OwnedTable);
impl Drop for OwnedTable {
    fn drop(&mut self) {
        for raw in [self.0.cpu_heap, self.0.owner, self.0.provenance] {
            release_raw(raw);
        }
    }
}
/// descriptors::OwnedSourceSlots.
pub(super) struct OwnedSlots {
    pub parameter: u32,
    pub count: u32,
    pub cpu_heap: *mut c_void,
    pub owner: *mut c_void,
    pub identity: u64,
    pub version: u64,
    pub source_gpu: u64,
    pub source_heap: u64,
    pub slots: Vec<crate::audit_source_tables::OwnedSlot>,
}
impl Drop for OwnedSlots {
    fn drop(&mut self) {
        release_raw(self.cpu_heap);
        release_raw(self.owner);
    }
}
/// SourceResourceBinding (its resource and counter referenced).
#[repr(transparent)]
pub(super) struct OwnedDependency(pub crate::audit_source_tables::Dependency);
impl Drop for OwnedDependency {
    fn drop(&mut self) {
        release_raw(self.0.resource);
        release_raw(self.0.counter);
    }
}
/// What a capture emitted (the rows, references handed over).
#[derive(Default)]
struct Emitted {
    tables: Vec<OwnedTable>,
    slots: Vec<OwnedSlots>,
    dependencies: Vec<OwnedDependency>,
}
unsafe extern "C-unwind" fn emit(context: *mut c_void, which: u32, row: *const c_void) {
    // SAFETY: the capture's Emitted; the row as `which` names it, its references handed over.
    let out = unsafe { &mut *context.cast::<Emitted>() };
    unsafe {
        match which {
            0 => out.tables.push(OwnedTable(*row.cast::<crate::audit_source_tables::OwnedTable>())),
            1 => {
                let r = &*row.cast::<crate::audit_source_tables::OwnedSlotsRow>();
                let slots = if r.slots.is_null() || r.slot_count == 0 { Vec::new() } else { core::slice::from_raw_parts(r.slots, r.slot_count).to_vec() };
                out.slots.push(OwnedSlots {
                    parameter: r.parameter,
                    count: r.count,
                    cpu_heap: r.cpu_heap,
                    owner: r.owner,
                    identity: r.identity,
                    version: r.version,
                    source_gpu: r.source_gpu,
                    source_heap: r.source_heap,
                    slots,
                });
            }
            _ => out.dependencies.push(OwnedDependency(core::ptr::read(row.cast::<crate::audit_source_tables::Dependency>()))),
        }
    }
}
/// descriptors::SourceTableFailure, the last capture's.
fn source_table_failure() -> [u32; 3] {
    let mut failure = [0u32; 3];
    // SAFETY: a writable record.
    unsafe { crate::audit_source_tables::cvr_sda_st_failure(core::ptr::null(), &mut failure) };
    failure
}
fn refuse_source_tables(reason: u32) -> bool {
    let set = [reason, u32::MAX, u32::MAX];
    // SAFETY: a readable record.
    unsafe { crate::audit_source_tables::cvr_sda_st_failure(&set, core::ptr::null_mut()) };
    false
}

/// A source's own state on this side (CaptureTables' and the selectors').
#[derive(Default)]
pub(super) struct SourceCapture {
    pub tables: Vec<OwnedTable>,
    pub finite_dependencies: Vec<OwnedDependency>,
    pub sparse_dependencies: Vec<OwnedDependency>,
    pub selector_inputs: Vec<SelectorInput>,
    pub selector_result: SelectorResult,
    pub selector_slots: Vec<OwnedSlots>,
    pub t5_plan: Option<ReadPlan>,
    pub t5_candidate: Option<Candidate>,
    /// PrepareT5Overlay's private view.
    pub t5_view: Option<HeldView>,
    /// BuildGuardedSources' plan (its checks and T5 view).
    pub guarded_plan: Option<GuardedPlan>,
    /// BuildSourceAssemblies' plan.
    pub binding_plan: Option<crate::assembly_build::Plan>,
    /// The eye's pass-depth checkpoint at the append, the source's VRS capture and shader snapshot.
    pub pass_depth: Option<std::sync::Arc<crate::pair_source::Checkpoint>>,
    pub vrs: Option<std::sync::Arc<crate::pair_source::Vrs>>,
    pub shader: Option<crate::shader_snapshot_native::Source>,
    /// BuildStagedSources' prepared pair source.
    pub staged: Option<crate::pair_source::PairSource>,
}

// ---- the work's own (the table plans by pipeline, the source table cache)

/// The work's table plans (source_tables::Plan by pipeline) and its source table cache (audit_source_tables.rs').
pub(super) struct WorkSources {
    pub table_plans: std::collections::HashMap<usize, crate::assembly_build::SourcePlan>,
    pub table_cache: *mut crate::audit_source_tables::Cache,
    /// The assembly passes' CPU table cache (CpuTableCache's storage).
    pub assembly_cache: Option<crate::assembly_pass::Cache>,
    /// The guarded pass's tables (one list per source) and the binding pass's (one per assembled source, by position).
    pub guarded_tables: Option<Vec<Vec<crate::shared_table::SharedTable>>>,
    pub binding_tables: Option<Vec<Vec<crate::shared_table::SharedTable>>>,
    pub binding_index: Vec<Option<usize>>,
    /// The assembly bank (the binding bank's source pass over the assembled sources).
    pub bank: Option<IUnknown>,
    /// PrepareT5Overlay's overlay (present once attempted).
    pub t5_overlay: Option<T5Overlay>,
}
impl Default for WorkSources {
    fn default() -> Self {
        WorkSources {
            table_plans: Default::default(),
            table_cache: core::ptr::null_mut(),
            assembly_cache: None,
            guarded_tables: None,
            binding_tables: None,
            binding_index: Vec::new(),
            bank: None,
            t5_overlay: None,
        }
    }
}
impl Drop for WorkSources {
    fn drop(&mut self) {
        // SAFETY: the cache this work's captures made (or null), freed once.
        unsafe { crate::audit_source_tables::cvr_sda_st_cache_free(self.table_cache) };
    }
}
impl WorkSources {
    /// The source's assembled binding tables (none when it was not assembled).
    pub(super) fn binding_tables_of(&self, index: usize) -> Option<&Vec<crate::shared_table::SharedTable>> {
        let at = (*self.binding_index.get(index)?)?;
        self.binding_tables.as_ref()?.get(at)
    }
    /// SourceTableCache's counts: 0 tables, 1 descriptors, 2 slot tables, 3 slot descriptors.
    pub(super) fn cache_count(&self, which: u32) -> u32 {
        // SAFETY: the cache (or null).
        unsafe { crate::audit_source_tables::cvr_sda_st_cache_count(self.table_cache, which) }
    }
}

// ---- CaptureTables

/// The source's shader snapshot's serialized root (None when the source holds no snapshot).
fn shader_root(source: &SourceScalars, pipeline: *mut c_void) -> Option<Vec<u8>> {
    // SAFETY: the source's std::shared_ptr<const Snapshot> (its pointer first).
    let held = unsafe { *source.shader_shared().cast::<*const c_void>() };
    if held.is_null() {
        return None;
    }
    let snapshot = crate::shader_snapshot_native::source(pipeline)?;
    let mirror = &snapshot.info().mirror;
    if mirror.root.is_null() || mirror.root_len == 0 {
        return Some(Vec::new());
    }
    // SAFETY: the snapshot's root bytes, alive with it.
    Some(unsafe { core::slice::from_raw_parts(mirror.root, mirror.root_len) }.to_vec())
}
/// The serialized root decoded at version 1.1 (its deserializer kept with the description).
pub(super) struct DecodedRoot {
    _reader: ID3D12VersionedRootSignatureDeserializer,
    desc: *const D3D12_VERSIONED_ROOT_SIGNATURE_DESC,
}
impl DecodedRoot {
    pub(super) fn new(bytes: &[u8]) -> Option<DecodedRoot> {
        if bytes.is_empty() {
            return None;
        }
        let mut raw = core::ptr::null_mut();
        // SAFETY: the serialized root's bytes; a local output.
        unsafe { D3D12CreateVersionedRootSignatureDeserializer(bytes.as_ptr().cast(), bytes.len(), &ID3D12VersionedRootSignatureDeserializer::IID, &mut raw) }.ok()?;
        if raw.is_null() {
            return None;
        }
        // SAFETY: the new deserializer's one reference.
        let reader = unsafe { ID3D12VersionedRootSignatureDeserializer::from_raw(raw) };
        // SAFETY: the reader's description (alive with it).
        let desc = unsafe { reader.GetRootSignatureDescAtVersion(D3D_ROOT_SIGNATURE_VERSION_1_1) }.ok()?;
        Some(DecodedRoot { _reader: reader, desc })
    }
    pub(super) fn desc(&self) -> &D3D12_ROOT_SIGNATURE_DESC1 {
        // SAFETY: the reader's version-1.1 description, alive with it.
        unsafe { &(*self.desc).Anonymous.Desc_1_1 }
    }
}
/// The family's uses as the assembly rules read them (the same 24-byte records).
pub(super) fn family_uses(f: &crate::families_native::Family) -> &[crate::early_depth_checks::ResourceUse] {
    // SAFETY: the same layout (families::ResourceUse).
    unsafe { core::slice::from_raw_parts(f.uses.as_ptr().cast(), f.uses.len()) }
}
fn bindings_view(b: &cvr_gpu::scene_state::Bindings) -> crate::audit_source_tables::Bindings {
    crate::audit_source_tables::Bindings {
        root: b.root as *mut c_void,
        tables: b.tables.as_ptr(),
        table_mask: b.table_mask,
        heaps: b.heaps.as_ptr().cast(),
        heap_count: b.heap_count,
        heaps_seen: b.heaps_seen,
    }
}
/// The CBV provider of a source table capture: the source's backing of this exact descriptor (references given).
#[allow(clippy::too_many_arguments)]
unsafe extern "C-unwind" fn provide_cbv(
    context: *const c_void,
    _list: *mut c_void,
    _parameter: u32,
    _offset: u32,
    stamp: *const crate::descriptor_audit::DescriptorStamp,
    origin: *mut c_void,
    resource: *mut *mut c_void,
    owner: *mut *mut c_void,
) -> u32 {
    // SAFETY: the capture's source and stamp.
    let source = unsafe { &*context.cast::<SourceScalars>() };
    // SAFETY: the stamp's 176 bytes.
    let wanted = unsafe { core::slice::from_raw_parts(stamp.cast::<u8>(), 176) };
    for cbv in source.cbvs.as_slice() {
        // SAFETY: the same 176-byte DescriptorStamp record.
        let held = unsafe { core::slice::from_raw_parts((&raw const cbv.stamp).cast::<u8>(), 176) };
        if held == wanted && cbv.origin_owner as *mut c_void == origin && cbv.resource != 0 {
            let (r, o) = (cbv.resource as *mut c_void, cbv.origin_owner as *mut c_void);
            // SAFETY: the backing's live resource and owner, referenced for the caller.
            unsafe {
                *resource = ID3D12Resource::from_raw_borrowed(&r).cloned().map_or(core::ptr::null_mut(), |v| v.into_raw());
                *owner = IUnknown::from_raw_borrowed(&o).cloned().map_or(core::ptr::null_mut(), |v| v.into_raw());
            }
            return 1;
        }
    }
    0
}
const TABLE_ROWS: u32 = 31;
const TABLE_COMMANDS: u32 = 32;
pub(super) fn journal_of(w: &WorkScalars) -> &'static mut crate::retained_journal::Journal {
    // SAFETY: the Work's journal, alive while the work is; the capture's lock held.
    unsafe { &mut *w.journal.cast::<crate::retained_journal::Journal>() }
}
/// CaptureTables (formerly the .cpp's): the source's finite descriptor tables at the draw (the root's planned domains,
/// the CBV backings the source's own), a receipt per source and a row per table.
pub(super) fn capture_tables(state: &mut WorkSources, w: &mut WorkScalars, list: *mut c_void, source: &SourceScalars, cap: &mut SourceCapture, row: &[u64; 48]) {
    let mut receipt = [0u64; 24];
    receipt[0] = row[0];
    receipt[1] = row[1];
    receipt[4] = u64::from(u32::MAX);
    receipt[5] = u64::from(u32::MAX);
    receipt[11] = row[11];
    receipt[12] = row[6];
    receipt[13] = row[7];
    receipt[14] = row[8];
    receipt[15] = source.cbvs.as_slice().len() as u64;
    let pipeline = row[10] as *mut c_void;
    let family = crate::families_native::lookup_raw(pipeline);
    let root_bytes = shader_root(source, pipeline);
    match (&family, &root_bytes) {
        (Some(f), Some(root)) if f.source_root.as_raw() == source.root => {
            if !state.table_plans.contains_key(&(pipeline as usize)) {
                match DecodedRoot::new(root).and_then(|d| crate::assembly_build::source_plan(d.desc(), family_uses(f))) {
                    Some(plan) => {
                        state.table_plans.insert(pipeline as usize, plan);
                    }
                    None => {
                        receipt[3] = 102;
                        w.tables_failed = true;
                    }
                }
            }
            if receipt[3] == 0 {
                let plan = &state.table_plans[&(pipeline as usize)];
                receipt[6] = plan.finite.len() as u64;
                receipt[10] = plan.deferred.len() as u64;
                let uses: Vec<Vec<crate::source_layout::SlotUse>> = plan
                    .finite
                    .iter()
                    .map(|t| t.uses.iter().map(|&(offset, count, kind)| crate::source_layout::SlotUse { offset, count, kind }).collect())
                    .collect();
                for t in &plan.finite {
                    receipt[9] += t.uses.iter().map(|u| u64::from(u.1)).sum::<u64>();
                }
                let requests: Vec<crate::audit_source_tables::Request> = plan
                    .finite
                    .iter()
                    .zip(&uses)
                    .map(|(t, u)| crate::audit_source_tables::Request { parameter: t.parameter, count: t.count, uses: u.as_ptr(), use_count: u.len() })
                    .collect();
                let mut out = Emitted::default();
                let captured = if !cap.tables.is_empty() || !cap.finite_dependencies.is_empty() || requests.len() > 64 {
                    refuse_source_tables(1)
                } else {
                    let view = bindings_view(&source.bindings);
                    let device = w.device.as_ref().map_or(core::ptr::null_mut(), |d| d.as_raw());
                    // SAFETY: the live list, the lent views and requests, this capture's provider and sink.
                    unsafe {
                        crate::audit_source_tables::cvr_sda_st_capture(
                            false,
                            device,
                            list,
                            &view,
                            requests.as_ptr(),
                            requests.len(),
                            provide_cbv,
                            (source as *const SourceScalars).cast(),
                            &mut state.table_cache,
                            true,
                            emit,
                            (&raw mut out).cast(),
                        )
                    }
                };
                if !captured {
                    let failure = source_table_failure();
                    receipt[4] = u64::from(failure[1]);
                    receipt[5] = u64::from(failure[2]);
                    receipt[3] = u64::from(failure[0]);
                    w.tables_failed = true;
                } else {
                    cap.tables = out.tables;
                    cap.finite_dependencies = out.dependencies;
                    receipt[2] = 1;
                    receipt[7] = cap.tables.len() as u64;
                    let journal = journal_of(w);
                    for table in &cap.tables {
                        let t = &table.0;
                        let mut value = [0u64; 16];
                        value[..10].copy_from_slice(&[
                            row[0],
                            row[1],
                            u64::from(t.parameter),
                            u64::from(t.count),
                            t.cpu_heap as u64,
                            t.owner as u64,
                            t.identity,
                            t.version,
                            t.source_gpu,
                            t.source_heap,
                        ]);
                        let at = plan.finite.iter().position(|f| f.parameter == t.parameter).unwrap_or(0);
                        let planned: Vec<crate::retained_capture::SlotUse> =
                            uses[at].iter().map(|u| crate::retained_capture::SlotUse { offset: u.offset, count: u.count, kind: u.kind as i32 }).collect();
                        // SAFETY: the planned uses and a writable row.
                        unsafe { crate::retained_capture::cvr_nrg_table_uses(planned.as_ptr(), planned.len(), &mut value) };
                        receipt[8] += u64::from(t.count);
                        journal.append(TABLE_ROWS, &value);
                    }
                }
            }
        }
        _ => {
            receipt[3] = 101;
            w.tables_failed = true;
        }
    }
    journal_of(w).append(TABLE_COMMANDS, &receipt);
}

// ---- the selector capture (CaptureSelectorInput, CaptureMultilayerT5, CaptureSelectedSlots, CaptureMultilayerSelectors,
// CaptureSelectors)

const SELECTOR_COMMANDS: u32 = 41;
const SELECTOR_INPUTS: u32 = 43;
const SELECTOR_COVERAGE: u32 = 44;
const SELECTOR_CACHE_EVENTS: u32 = 45;
const SELECTOR_SLOTS: u32 = 46;
const T5_PIECES: u32 = 51;
const SELECTOR_BLOB: u32 = 4;
/// The multilayer family's pixel shader (the one unsupported family the selectors read through T5).
const MULTILAYER_PIXEL: &str = "9a4947a3ec0eb3d4605d88b663bb27a0bc809fe8a3d91c4ae67886bd77148db8";

/// The journal table's row count (its width given).
fn rows_count(w: &WorkScalars, kind: u32, width: usize) -> usize {
    journal_of(w).rows(kind).len() / width
}
/// CaptureSelectorInput: the CBV requirement's backing at the draw (located through the root, its current coverage and
/// cache events read), its bytes captured when the descriptor still matches; an input and its rows.
fn capture_selector_input(w: &mut WorkScalars, source: &SourceScalars, cap: &mut SourceCapture, command: &[u64; 48], position: u64, key: &sel::CbvKey, reflected: u32) {
    let journal = journal_of(w);
    let mut row = [0u64; 24];
    row[..7].copy_from_slice(&[command[0], command[1], u64::from(key.visibility), u64::from(key.shader_register), u64::from(key.parameter), u64::from(key.table_offset), u64::from(reflected)]);
    row[11] = position;
    row[21] = command[7];
    row[22] = command[8];
    let index = journal.rows(SELECTOR_INPUTS).len() / 24;
    let mut input = SelectorInput { key: key.clone(), intent: None, row: index };
    let mut coverage = [0u64; 28];
    coverage[..3].copy_from_slice(&[index as u64, command[0], command[1]]);
    let mut events = [0u64; 36];
    events[..3].copy_from_slice(&[index as u64, command[0], command[1]]);
    let stage = if key.visibility == 1 { 0 } else { 1 };
    let found = source.cbvs.as_slice().iter().find(|cbv| cbv.stage == stage && cbv.shader_register == key.shader_register);
    if let Some(cbv) = found {
        let located = crate::descriptor_audit::find_binding(command[11], 2, key.space, key.shader_register, key.visibility);
        if located == Some((key.parameter, key.table_offset)) {
            let resource = cbv.resource as *mut c_void;
            row[17] = cbv.stamp.gpu;
            row[18] = cbv.stamp.heap;
            row[19] = cbv.resource;
            row[20] = cbv.stamp.value[4];
            let mut current = [0u64; 24];
            if crate::source_intent::query_generic_coverage(resource, cbv.source_offset, reflected as usize, &mut current) {
                coverage[3] = 1;
                coverage[4..28].copy_from_slice(&current);
            }
            let mut history = [0u64; 32];
            if crate::source_intent::query_cache_events(resource, cbv.source_offset, reflected as usize, &mut history) {
                events[3] = 1;
                events[4..36].copy_from_slice(&history);
            }
            // SAFETY: the same 176-byte DescriptorStamp record.
            let stamp = unsafe { &*(&raw const cbv.stamp).cast::<crate::descriptor_audit::DescriptorStamp>() };
            if position != 0 && crate::descriptor_audit::matches_descriptor(stamp) {
                if let Some(intent) = Candidate::capture(resource, cbv.stamp.value[4], reflected as usize, false) {
                    row[7] = 1;
                    row[8] = intent.copy_sequence();
                    row[9] = intent.producer_list();
                    row[10] = intent.copy_sequence();
                    row[12] = intent.view_offset();
                    row[15] = journal.blob(SELECTOR_BLOB).len() as u64 + 1;
                    row[16] = intent.bytes().len() as u64;
                    journal.append_blob(SELECTOR_BLOB, intent.bytes());
                    input.intent = Some(intent);
                }
            }
        }
    }
    cap.selector_inputs.push(input);
    journal.append(SELECTOR_INPUTS, &row);
    journal.append(SELECTOR_COVERAGE, &coverage);
    journal.append(SELECTOR_CACHE_EVENTS, &events);
}
unsafe extern "C-unwind" fn no_provider(
    _: *const c_void,
    _: *mut c_void,
    _: u32,
    _: u32,
    _: *const crate::descriptor_audit::DescriptorStamp,
    _: *mut c_void,
    _: *mut *mut c_void,
    _: *mut *mut c_void,
) -> u32 {
    0
}
/// CaptureSelectedSlots: the result's selected SRV slots captured at the draw (each table located where the key says).
fn capture_selected_slots(state: &mut WorkSources, w: &mut WorkScalars, list: *mut c_void, source: &SourceScalars, cap: &mut SourceCapture, source_row: &[u64; 48], command: &mut [u64; 32]) -> bool {
    command[8] = u64::from(cap.selector_result.evaluated_sites);
    if cap.selector_result.tables.is_empty() {
        command[2] = 1;
        return true;
    }
    let uses: Vec<Vec<crate::source_layout::SlotUse>> = cap
        .selector_result
        .tables
        .iter()
        .map(|t| t.uses.iter().map(|u| crate::source_layout::SlotUse { offset: u.offset, count: u.count, kind: 0 }).collect())
        .collect();
    let mut current = true;
    for t in &cap.selector_result.tables {
        let k = &t.key;
        current &= crate::descriptor_audit::find_binding(source_row[11], 0, k.space, k.shader_register_base, k.visibility) == Some((k.parameter, k.original_table_offset));
    }
    let requests: Vec<crate::audit_source_tables::Request> = cap
        .selector_result
        .tables
        .iter()
        .zip(&uses)
        .map(|(t, u)| crate::audit_source_tables::Request { parameter: t.key.parameter, count: t.count, uses: u.as_ptr(), use_count: u.len() })
        .collect();
    let mut out = Emitted::default();
    let captured = current
        && if !cap.selector_slots.is_empty() || !cap.sparse_dependencies.is_empty() || requests.len() > 64 {
            refuse_source_tables(1)
        } else {
            let view = bindings_view(&source.bindings);
            let device = w.device.as_ref().map_or(core::ptr::null_mut(), |d| d.as_raw());
            // SAFETY: the live list, the lent views and requests, this capture's sink.
            unsafe {
                crate::audit_source_tables::cvr_sda_st_capture(
                    true,
                    device,
                    list,
                    &view,
                    requests.as_ptr(),
                    requests.len(),
                    no_provider,
                    core::ptr::null(),
                    &mut state.table_cache,
                    true,
                    emit,
                    (&raw mut out).cast(),
                )
            }
        };
    if !captured {
        command[2] = 5;
        command[3] = u64::from(source_table_failure()[0]);
        return false;
    }
    cap.selector_slots = out.slots;
    cap.sparse_dependencies = out.dependencies;
    command[2] = 1;
    command[9] = cap.selector_slots.len() as u64;
    let journal = journal_of(w);
    for table in &cap.selector_slots {
        for slot in &table.slots {
            let mut row = [0u64; 16];
            row[..15].copy_from_slice(&[
                source_row[0],
                source_row[1],
                u64::from(table.parameter),
                u64::from(slot.offset),
                u64::from(slot.compact_index),
                slot.kind as u32 as u64,
                slot.stamp.gpu,
                slot.stamp.heap,
                slot.stamp.value[0],
                slot.stamp.value[2],
                table.cpu_heap as u64,
                table.owner as u64,
                table.identity,
                table.version,
                u64::from(table.count),
            ]);
            journal.append(SELECTOR_SLOTS, &row);
            command[10] += 1;
        }
    }
    true
}
/// CaptureMultilayerSelectors: the multilayer family's T5 read plan from its B4 input, the T5 bytes it reads captured,
/// the records enumerated into the result, the selected slots captured.
#[allow(clippy::too_many_arguments)]
fn capture_multilayer_selectors(
    state: &mut WorkSources,
    w: &mut WorkScalars,
    list: *mut c_void,
    source: &SourceScalars,
    cap: &mut SourceCapture,
    source_row: &[u64; 48],
    family: &crate::families_native::Family,
    command: &mut [u64; 32],
) {
    command[25] = 1;
    let binding = &source.multilayer_t5;
    let first = cap.selector_inputs.first().and_then(|i| valid_bytes(&i.intent).map(|b| (i.key.clone(), b.to_vec())));
    let raw = binding.resource as *mut c_void;
    // SAFETY: the binding's live resource (or null).
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }).cloned() else {
        command[2] = 3;
        return;
    };
    let Some((b4_key, b4)) = first else {
        command[2] = 3;
        return;
    };
    let view = &binding.view;
    // SAFETY: the SRV's buffer view (a structured buffer's).
    let buffer = unsafe { view.Anonymous.Buffer };
    // SAFETY: a live resource.
    let width = unsafe { resource.GetDesc() }.Width;
    let input = StructuredView {
        key: sel::CbvKey { root: text(&family.root_sha), visibility: 5, shader_register: 5, space: 0, parameter: 23, table_offset: 5 },
        first_element: buffer.FirstElement,
        num_elements: buffer.NumElements,
        structure_byte_stride: buffer.StructureByteStride,
        resource_width: width,
        format: view.Format.0 as u32,
        dimension: view.ViewDimension.0 as u32,
        component_mapping: view.Shader4ComponentMapping,
        flags: buffer.Flags.0 as u32,
        explicit_description: true,
    };
    let Some(plan) = plan_read(&family_key(family), &b4_key, &b4, &input) else {
        cap.t5_plan = None;
        command[2] = 4;
        command[3] = 1;
        return;
    };
    let mut bytes: Option<Vec<u8>> = None;
    if plan.numbers.byte_extent != 0 {
        // SAFETY: a live resource.
        let address = unsafe { resource.GetGPUVirtualAddress() };
        let captured = (address <= u64::MAX - plan.numbers.resource_offset)
            .then(|| Candidate::capture(raw, address + plan.numbers.resource_offset, plan.numbers.byte_extent as usize, true))
            .flatten();
        let Some(candidate) = captured else {
            cap.t5_plan = Some(plan);
            command[2] = 3;
            command[3] = 2;
            return;
        };
        if candidate.pieces().len() > 65536usize.saturating_sub(rows_count(w, T5_PIECES, 16)) {
            cap.t5_plan = Some(plan);
            cap.t5_candidate = None;
            command[2] = 3;
            command[3] = 4;
            return;
        }
        bytes = Some(candidate.bytes().to_vec());
        cap.t5_candidate = Some(candidate);
    }
    let result = enumerate(&plan, &input, plan.numbers.resource_offset, bytes.as_deref());
    cap.t5_plan = Some(plan);
    let Some(result) = result else {
        command[2] = 4;
        command[3] = 3;
        return;
    };
    cap.selector_result = result;
    capture_selected_slots(state, w, list, source, cap, source_row, command);
}
/// CaptureSelectors (formerly the .cpp's): the family's selector recipe from the catalog; its CBV inputs captured and
/// evaluated (or, for the multilayer family, the T5 path), the selected slots captured; a command row.
pub(super) fn capture_selectors(state: &mut WorkSources, w: &mut WorkScalars, list: *mut c_void, source: *mut SourceScalars, cap: &mut SourceCapture, source_row: &[u64; 48]) {
    let mut command = [0u64; 32];
    command[0] = source_row[0];
    command[1] = source_row[1];
    command[13] = source_row[6];
    command[14] = source_row[7];
    command[15] = source_row[8];
    command[16] = source_row[11];
    command[17] = source_row[12];
    command[20] = u64::from(crate::native_state_exports::CyberpunkVR_RegisteredCopyCount.load(SeqCst));
    command[21] = u64::from(crate::native_state_exports::CyberpunkVR_RegisteredCopyRejected.load(SeqCst));
    command[22] = u64::from(crate::native_state_exports::CyberpunkVR_RegisteredCopyChanged.load(SeqCst));
    let family = crate::families_native::lookup_raw(source_row[10] as *mut c_void);
    let plan = family.as_ref().and_then(|f| selector_catalog().and_then(|c| c.find(&family_key(f))));
    // SAFETY: the .cpp's source being appended.
    let scalars = unsafe { &*source };
    match (&family, plan) {
        (Some(f), Some(plan)) if source_row[44] == 0 => {
            command[4] = f.id;
            command[18] = u64::from(plan.number);
            command[5] = source_row[0] + 1;
            if !plan.supported {
                command[2] = 2;
                if f.pixel_sha == MULTILAYER_PIXEL {
                    let key = sel::CbvKey { root: text(&f.root_sha), visibility: 5, shader_register: 4, space: 0, parameter: 19, table_offset: 0 };
                    capture_selector_input(w, scalars, cap, source_row, command[5], &key, 228);
                    // CaptureMultilayerT5: the multilayer T5's capture with the first input's intent bytes.
                    let first = cap.selector_inputs.first().and_then(|i| valid_bytes(&i.intent));
                    let (bytes, len) = first.map_or((core::ptr::null(), 0), |b| (b.as_ptr(), b.len()));
                    // SAFETY: the live list, the source being appended, its row, the intent's bytes and its tables.
                    unsafe { super::capture_multilayer_t5(list, source, source_row, bytes, len, first.is_some(), tables_view(&cap.tables)) };
                    command[6] = 1;
                    capture_multilayer_selectors(state, w, list, scalars, cap, source_row, f, &mut command);
                }
                for input in &cap.selector_inputs {
                    command[7] += u64::from(valid_bytes(&input.intent).is_some());
                }
            } else {
                command[6] = plan.cbvs.len() as u64;
                for requirement in &plan.cbvs {
                    capture_selector_input(w, scalars, cap, source_row, command[5], &requirement.key, requirement.reflected_bytes);
                }
                let bytes: Vec<(sel::CbvKey, Option<&[u8]>)> = cap.selector_inputs.iter().filter_map(|i| valid_bytes(&i.intent).map(|b| (i.key.clone(), Some(b)))).collect();
                command[7] = bytes.len() as u64;
                if bytes.len() != plan.cbvs.len() {
                    command[2] = 3;
                } else {
                    match sel::evaluate(plan, &bytes) {
                        Err(_) => command[2] = 4,
                        Ok(e) => {
                            cap.selector_result = SelectorResult {
                                family: e.family,
                                evaluated_sites: e.evaluated_sites,
                                selections: e.selections,
                                tables: e.tables,
                                complete: true,
                                ..Default::default()
                            };
                            capture_selected_slots(state, w, list, scalars, cap, source_row, &mut command);
                        }
                    }
                }
            }
        }
        _ => command[2] = 6,
    }
    journal_of(w).append(SELECTOR_COMMANDS, &command);
}

// ---- the T5 overlay (OverlayB4, PrepareT5Overlay)

const CBV_SNAPSHOT_ROWS_KIND: u32 = 33;
const T5_SNAPSHOT_ROWS_KIND: u32 = 48;
const OVERLAY_ROWS: u32 = 56;
const CBV_BLOB: u32 = 3;
const T5_BLOB: u32 = 5;
const OVERLAY_BLOB: u32 = 7;

/// structured_snapshot_view::PrivateView held: its resource and owner referenced.
pub(super) struct HeldView(pub crate::structured_view_overlay::PrivateView);
impl Drop for HeldView {
    fn drop(&mut self) {
        release_raw(self.0.resource);
        release_raw(self.0.owner);
    }
}
/// The work's T5 overlay (structured_view_owner.rs' storage, none when its adoption was refused) -- present once
/// attempted, its required and copied bytes published.
pub(super) struct T5Overlay(pub Option<IUnknown>);
impl T5Overlay {
    pub(super) fn required_bytes(&self) -> u64 {
        // SAFETY: the live storage.
        self.0.as_ref().map_or(0, |s| unsafe { crate::structured_view_owner::cvr_sov2_count(s.as_raw(), 1) })
    }
    pub(super) fn copied_bytes(&self) -> u64 {
        // SAFETY: as above.
        self.0.as_ref().map_or(0, |s| unsafe { crate::structured_view_owner::cvr_sov2_count(s.as_raw(), 2) })
    }
}
/// OverlayB4: the source's private B4 CBV (its index among the source's private CBVs), its snapshot bytes and their
/// snapshot row -- the B4 selector input matched to its CBV snapshot row and the T5 plan's B4 bytes.
pub(super) fn overlay_b4(w: &WorkScalars, index: usize, cap: &SourceCapture) -> Option<(usize, Vec<u8>, usize)> {
    let source = source(index as u64)?;
    let journal = journal_of(w);
    let saved_rows = journal.rows(SOURCE_ROWS);
    let saved: &[u64] = saved_rows.get(index * 48..index * 48 + 48)?;
    let input = cap.selector_inputs.iter().find(|i| i.key.visibility == 5 && i.key.shader_register == 4 && i.key.space == 0 && i.key.parameter == 19 && i.key.table_offset == 0)?;
    let privates = source.private_cbvs.as_slice();
    let b4 = privates.iter().position(|c| c.parameter == 19 && c.offset == 0)?;
    let inputs = journal.rows(SELECTOR_INPUTS);
    let matched: &[u64; 24] = inputs.get(input.row * 24..input.row * 24 + 24)?.try_into().ok()?;
    let snapshots = journal.rows(CBV_SNAPSHOT_ROWS_KIND).len() / 32;
    // SAFETY: the matched input row.
    if !unsafe { crate::retained_overlay::cvr_nrg_b4_input(matched, index as u64, saved[1], snapshots as u64) } {
        return None;
    }
    let snapshot_index = (matched[14] as usize).wrapping_sub(1);
    let row: &[u64; 32] = journal.rows(CBV_SNAPSHOT_ROWS_KIND).get(snapshot_index * 32..snapshot_index * 32 + 32)?.try_into().ok()?;
    let c = &privates[b4];
    let prepared = &source.cbv_prepared;
    let copy = crate::retained_overlay::B4Copy {
        resource: c.resource,
        owner: c.owner,
        gpu: c.view_location,
        private_offset: c.private_offset,
        copied: c.copied_bytes,
        padded: u64::from(c.padded_bytes),
        reflected: u64::from(c.reflected_bytes),
        prepared_owner: prepared.owner as u64,
        prepared_list: prepared.list,
        prepared_generation: prepared.generation,
        prepared_occurrence: prepared.occurrence,
    };
    let blob = journal.blob(CBV_BLOB);
    // SAFETY: the snapshot row and the mirrored copy.
    if !unsafe { crate::retained_overlay::cvr_nrg_b4_row(row, index as u64, saved[1], &copy, saved[6], saved[7], blob.len() as u64) } {
        return None;
    }
    let start = (row[31] as usize).wrapping_sub(1);
    let bytes = blob.get(start..start.checked_add(row[20] as usize)?)?.to_vec();
    let plan = cap.t5_plan.as_ref();
    let expected: &[u8] = plan.map_or(&[0u8; ml::B4_BYTES][..], |p| &p.b4_bytes[..]);
    if bytes.len() > expected.len() || bytes[..] != expected[..bytes.len()] {
        return None;
    }
    Some((b4, bytes, snapshot_index))
}
/// structured_buffer_snapshot::RecordedView of the source's T5 copy and range.
fn recorded_view(source: &SourceScalars) -> Option<D3D12_SHADER_RESOURCE_VIEW_DESC> {
    let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
    // SAFETY: the source's mirrored copy and range; a local output.
    unsafe { crate::structured_snapshot_receipt::cvr_sbsn_recorded_view((&raw const source.t5_prepared).cast(), (&raw const source.t5_private).cast(), &mut view) }
        .then_some(view)
}
fn same_buffer_view(a: &D3D12_SHADER_RESOURCE_VIEW_DESC, b: &D3D12_SHADER_RESOURCE_VIEW_DESC) -> bool {
    // SAFETY: two descriptions.
    unsafe { crate::structured_snapshot::cvr_ssv_check(0, a, b, core::ptr::null()) }
}
fn overlay_error(message: &str) {
    let out = ex!(CyberpunkVR_RetainedT5OverlayError);
    let n = message.len().min(out.len() - 1);
    out[..n].copy_from_slice(&message.as_bytes()[..n]);
    out[n] = 0;
}
/// PrepareT5Overlay (formerly the .cpp's): every source with a T5 read adopted into one native structured view overlay,
/// each source's private view kept; false: not ready (or refused: overlay stage 3).
pub(super) fn prepare_t5_overlay(w: &mut WorkScalars, captures: &mut [SourceCapture], overlay: &mut Option<T5Overlay>) -> bool {
    if !w.source_selectors || w.overlay_stage == 2 {
        return true;
    }
    if w.overlay_stage == 3 {
        return false;
    }
    match adopt_t5_overlay(w, captures, overlay) {
        Ok(()) => {
            w.overlay_stage = 2;
            true
        }
        Err(message) => {
            overlay_error(&message);
            w.overlay_stage = 3;
            fail(1601);
            publish_sources();
            false
        }
    }
}
fn adopt_t5_overlay(w: &mut WorkScalars, captures: &mut [SourceCapture], overlay: &mut Option<T5Overlay>) -> Result<(), String> {
    use crate::structured_view_overlay as sov;
    let journal = journal_of(w);
    let mut facts: Vec<(sov::Input, Option<ReadPlan>)> = Vec::new();
    let mut held_b4: Vec<Vec<u8>> = Vec::new();
    let mut indices = Vec::new();
    for (index, cap) in captures.iter().enumerate() {
        let Some(plan) = cap.t5_plan.as_ref().filter(|p| p.numbers.byte_extent != 0) else { continue };
        let source = source(index as u64).ok_or("t5 source missing")?;
        let t5_row = source.t5_row;
        let rows = journal.rows(T5_SNAPSHOT_ROWS_KIND);
        let row = rows.get(t5_row * 32..t5_row * 32 + 32).ok_or_else(|| "Nonempty t5 has no copy receipt".to_string())?;
        let saved = journal.rows(SOURCE_ROWS).get(index * 48..index * 48 + 48).ok_or("source row missing")?.to_vec();
        let side = saved[1] as u32;
        let eye = w.eye(side as usize);
        if row[2] != 1 || row[23] == 0 || row[24] == 0 || row[28] != 1 || !source.t5_original_certified || !eye.submission_certified || eye.source_queue.is_none() || eye.signaled_value == 0 || eye.t5_aborted || !arena_op(side, 1, 1) {
            return Err(format!("t5 source ownership/completion differs at {index}"));
        }
        if recorded_view(source).is_none() {
            return Err(format!("t5 original-position receipt differs at {index}"));
        }
        let Some((_, b4_bytes, _)) = overlay_b4(w, index, cap) else { return Err(format!("t5 B4 source association differs at {index}")) };
        let scope = crate::structured_snapshot::ViewScope { root: saved[11], list: saved[6], generation: saved[7], occurrence: index as u64 + 1, eye: side };
        // PlanRead again over the private B4 bytes: the same plan (BuildPlan's check, judged at the push).
        let checked = plan_read(&plan.family, &plan.b4_key, &b4_bytes, &plan.view).filter(|c| {
            plan.superset
                && plan.coverage_complete
                && c.family == plan.family
                && c.b4_key == plan.b4_key
                && c.b4_bytes == plan.b4_bytes
                && c.numbers == plan.numbers
                && c.records == plan.records
                && same_view(&c.view, &plan.view)
                && !c.records.is_empty()
                && c.records.len() <= ml::MAX_RECORDS as usize
        });
        let dense = &source.t5_private;
        let copy = &source.t5_prepared;
        let input = sov::Input {
            scope,
            b4_scope: scope,
            completion_scope: scope,
            dense: dense.resource,
            dense_owner: dense.owner,
            dense_source: dense.source_offset,
            dense_private: dense.private_offset,
            dense_copied: dense.copied_bytes,
            copy_owner: copy.owner,
            copy_list: copy.list,
            copy_generation: copy.generation,
            copy_occurrence: copy.occurrence,
            copy_source: copy.source_offset,
            copy_private: copy.private_offset,
            copy_copied: copy.copied_bytes,
            queue: eye.source_queue.as_ref().map_or(core::ptr::null_mut(), |q| q.as_raw()),
            fence: eye.fence.as_ref().map_or(core::ptr::null_mut(), |f| f.as_raw()),
            value: eye.signaled_value,
            first_element: plan.view.first_element,
            resource_width: plan.view.resource_width,
            num_elements: plan.view.num_elements,
            stride: plan.view.structure_byte_stride,
            b4: core::ptr::null(),
            b4_bytes: 0,
            plan_b4: core::ptr::null(),
            certified: true,
            finalized: true,
            b4_verified: true,
            bounded: plan.coverage_complete,
            plan_view: true,
        };
        held_b4.push(b4_bytes);
        facts.push((input, checked));
        indices.push(index);
    }
    if indices.is_empty() {
        return Ok(());
    }
    *overlay = Some(T5Overlay(None));
    // The plan (structured_view_overlay.rs' versions), each input admitted then pushed (BuildPlan).
    let plan = sov::cvr_sov_plan_new();
    struct PlanGuard(*mut sov::Plan);
    impl Drop for PlanGuard {
        fn drop(&mut self) {
            // SAFETY: the plan made above, freed once.
            unsafe { crate::structured_view_overlay::cvr_sov_plan_free(self.0) };
        }
    }
    let _guard = PlanGuard(plan);
    // structured_snapshot_view::Failure: Arguments 1, ReadPlan 5.
    if facts.len() > 2048 {
        return Err("Native structured view adoption rejected: 1".into());
    }
    for (n, (input, checked)) in facts.iter_mut().enumerate() {
        let plan_b4 = captures[indices[n]].t5_plan.as_ref().map_or(core::ptr::null(), |p| p.b4_bytes.as_ptr());
        input.b4 = held_b4[n].as_ptr();
        input.b4_bytes = held_b4[n].len();
        input.plan_b4 = plan_b4;
        // SAFETY: the plan and the lent facts.
        let failure = unsafe { sov::cvr_sov_admit(plan, input) };
        if failure != 0 {
            return Err(format!("Native structured view adoption rejected: {failure}"));
        }
        let Some(checked) = checked.as_ref() else { return Err("Native structured view adoption rejected: 5".into()) };
        let read = sov::Read {
            resource_offset: checked.numbers.resource_offset,
            byte_extent: checked.numbers.byte_extent,
            first_record: checked.numbers.first_record,
            record_span: checked.numbers.record_span,
            records: checked.records.as_ptr(),
            count: checked.records.len(),
        };
        // SAFETY: as above; the overlay's default budget (structured_snapshot_view::MaxBytes).
        let failure = unsafe { sov::cvr_sov_push(plan, input, &read, crate::structured_view_overlay::MAX_BYTES) };
        if failure != 0 {
            return Err(format!("Native structured view adoption rejected: {failure}"));
        }
    }
    let device = w.device.as_ref().map_or(core::ptr::null_mut(), |d| d.as_raw());
    let mut out = core::ptr::null_mut();
    // SAFETY: the device and the checked plan.
    let failure = unsafe { crate::structured_view_owner::cvr_sov2_adopt(device, plan.cast_const(), &mut out) };
    if failure != 0 || out.is_null() {
        return Err(format!("Native structured view adoption rejected: {failure}"));
    }
    // SAFETY: the adopted owner's one reference.
    let owner = unsafe { IUnknown::from_raw(out) };
    *overlay = Some(T5Overlay(Some(owner.clone())));
    for (version, &index) in indices.iter().enumerate() {
        // SAFETY: a plain view; all zeroes before the call.
        let mut view: sov::PrivateView = unsafe { core::mem::zeroed() };
        let mut resource = core::ptr::null_mut();
        // SAFETY: the live owner; local outputs (the resource lent).
        if !unsafe { crate::structured_view_owner::cvr_sov2_view(owner.as_raw(), version as u32, &mut view, &mut resource) } {
            return Err(format!("Adopted structured view owner differs at {index}"));
        }
        // SAFETY: the lent private resource, referenced for the view (as its ComPtr did).
        view.resource = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }.cloned().map_or(core::ptr::null_mut(), |r| r.into_raw());
        view.owner = owner.clone().into_raw();
        let held = HeldView(view);
        // SAFETY: the view.
        if !unsafe { crate::structured_view_owner::cvr_sov2_validate(&held.0) } {
            return Err(format!("Adopted structured view owner differs at {index}"));
        }
        let source = source(index as u64).ok_or("t5 source missing")?;
        let recorded = recorded_view(source).ok_or_else(|| format!("Adopted structured source receipt differs at {index}"))?;
        if !same_buffer_view(&held.0.srv, &recorded) {
            return Err(format!("Adopted structured SRV fields differ at {index}"));
        }
        let Some((b4, _, b4_row)) = overlay_b4(w, index, &captures[index]) else { return Err(format!("Adopted structured B4 association differs at {index}")) };
        let saved = journal.rows(SOURCE_ROWS)[index * 48..index * 48 + 48].to_vec();
        let snapshot = &journal.rows(T5_SNAPSHOT_ROWS_KIND)[source.t5_row * 32..source.t5_row * 32 + 32];
        let blob = journal.blob(T5_BLOB);
        if snapshot[27] == 0 || snapshot[27] - 1 > blob.len() as u64 || held.0.copied_bytes > blob.len() as u64 - (snapshot[27] - 1) {
            return Err("Native structured view has no original GPU payload".into());
        }
        let start = snapshot[27] as usize - 1;
        let data = blob[start..start + held.0.copied_bytes as usize].to_vec();
        journal.append_blob(OVERLAY_BLOB, &data);
        let c = &source.private_cbvs.as_slice()[b4];
        let buffer = unsafe { held.0.srv.Anonymous.Buffer };
        let row: [u64; 32] = [
            index as u64,
            saved[1],
            1,
            0,
            saved[11],
            saved[6],
            saved[7],
            index as u64 + 1,
            held.0.resource as u64,
            held.0.owner as u64,
            source.t5_private.resource as u64,
            source.t5_private.owner as u64,
            source.t5_private.private_offset,
            held.0.source_offset,
            held.0.copied_bytes,
            held.0.target_offset,
            buffer.FirstElement,
            u64::from(buffer.NumElements),
            u64::from(buffer.StructureByteStride),
            c.resource,
            c.owner,
            source.cbv_prepared.owner as u64,
            c.view_location,
            c.private_offset,
            c.copied_bytes,
            b4_row as u64 + 1,
            1,
            1,
            0,
            held.0.copy_list,
            held.0.copy_generation,
            1,
        ];
        journal.append(OVERLAY_ROWS, &row);
        captures[index].t5_view = Some(held);
    }
    Ok(())
}
fn same_view(a: &StructuredView, b: &StructuredView) -> bool {
    a.key == b.key
        && a.first_element == b.first_element
        && a.num_elements == b.num_elements
        && a.structure_byte_stride == b.structure_byte_stride
        && a.resource_width == b.resource_width
        && a.format == b.format
        && a.dimension == b.dimension
        && a.component_mapping == b.component_mapping
        && a.flags == b.flags
        && a.explicit_description == b.explicit_description
}

// ---- the assembly input (source_binding_assembly::Input), lent flat to assembly_build.rs (formerly BuildCore's marshalling)

use crate::assembly_build as ab;
use crate::geometry_pass_scope::Scope as AsmScope;
use crate::source_cbv_snapshot::{PreparedCopies, PrivateCbv as SnapCbv};
use crate::structured_view_overlay::PrivateView;

/// source_binding_assembly::PrivateCbv (the source's scope).
pub(super) struct CbvIn<'a> {
    pub value: Option<&'a SnapCbv>,
    pub copies: Option<&'a PreparedCopies>,
    pub finalized: bool,
}
/// source_binding_assembly::PrivateSrv (the source's scope).
#[derive(Clone)]
pub(super) struct SrvIn<'a> {
    pub parameter: u32,
    pub offset: u32,
    pub value: Option<&'a PrivateView>,
    pub original: &'a crate::material_buffer_snapshot::SrvBinding,
    pub original_owner: u64,
    pub original_identity: u64,
    pub original_version: u64,
    pub original_gpu: u64,
    pub original_heap: u64,
    pub resource_cookie: u64,
    pub original_count: u32,
    pub slot_certified: bool,
    pub b4: Option<&'a SnapCbv>,
    pub b4_copies: Option<&'a PreparedCopies>,
    pub b4_bytes: &'a [u8],
    pub b4_bytes_certified: bool,
}
/// source_binding_assembly::Input (one scope for the source and its selectors; no original CBVs).
pub(super) struct AsmInput<'a> {
    pub scope: AsmScope,
    pub root: *const D3D12_ROOT_SIGNATURE_DESC1,
    pub uses: &'a [crate::early_depth_checks::ResourceUse],
    pub finite: &'a [OwnedTable],
    pub sparse: &'a [OwnedSlots],
    pub cbvs: Vec<CbvIn<'a>>,
    pub family: &'a sel::FamilyKey,
    pub selectors: Option<&'a SelectorResult>,
    pub dynamic: bool,
    pub srvs: Vec<SrvIn<'a>>,
}
fn at<T>(value: Option<&T>) -> u64 {
    value.map_or(0, |v| v as *const T as u64)
}
fn pass_str(t: &[u8]) -> crate::geometry_pass_coverage::Str {
    crate::geometry_pass_coverage::Str { data: t.as_ptr(), len: t.len() }
}
/// The full T5 domain's record plan of a private view's B4 bytes (the only resource width the plan admits).
fn domain_plan(family: &sel::FamilyKey, b4: &[u8]) -> Option<ReadPlan> {
    let view = StructuredView {
        key: sel::CbvKey { root: text(ml::ROOT_SHA256), visibility: 5, shader_register: 5, space: 0, parameter: 23, table_offset: 5 },
        resource_width: crate::structured_view_overlay::DOMAIN_BYTES,
        num_elements: 8192,
        structure_byte_stride: 128,
        dimension: 1,
        component_mapping: 5768,
        explicit_description: true,
        ..Default::default()
    };
    let key = sel::CbvKey { root: text(ml::ROOT_SHA256), visibility: 5, shader_register: 4, space: 0, parameter: 19, table_offset: 0 };
    plan_read(family, &key, b4, &view)
}
/// BuildCore: the plan of the lent input (assembly_build.rs), `guarded` the guarded T5 view's and receipt's addresses.
fn build_plan(input: &AsmInput, guarded: Option<(u64, u64)>) -> Result<ab::Plan, u32> {
    let scope = input.scope;
    let finite: Vec<ab::Owned> = input
        .finite
        .iter()
        .map(|t| {
            let t = &t.0;
            ab::Owned {
                scope,
                present: 1,
                parameter: t.parameter,
                count: t.count,
                cpu_heap: t.cpu_heap as u64,
                owner: t.owner as u64,
                identity: t.identity,
                version: t.version,
                source_gpu: t.source_gpu,
                source_heap: t.source_heap,
                slots: core::ptr::null(),
                slot_count: 0,
            }
        })
        .collect();
    let sparse: Vec<ab::Owned> = input
        .sparse
        .iter()
        .map(|t| ab::Owned {
            scope,
            present: 1,
            parameter: t.parameter,
            count: t.count,
            cpu_heap: t.cpu_heap as u64,
            owner: t.owner as u64,
            identity: t.identity,
            version: t.version,
            source_gpu: t.source_gpu,
            source_heap: t.source_heap,
            // SAFETY: the same 192-byte OwnedSourceSlot records.
            slots: if t.slots.is_empty() { core::ptr::null() } else { t.slots.as_ptr().cast() },
            slot_count: t.slots.len(),
        })
        .collect();
    let cbvs: Vec<ab::PrivateCbv> = input
        .cbvs
        .iter()
        .map(|q| {
            // SAFETY: plain data, all zeroes first.
            let mut c: ab::PrivateCbv = unsafe { core::mem::zeroed() };
            c.scope = scope;
            c.value = at(q.value);
            c.copies = at(q.copies);
            if let Some(v) = q.value {
                c.parameter = v.parameter;
                c.offset = v.offset;
                c.reflected = v.reflected_bytes;
                c.padded = v.padded_bytes;
                c.private_offset = v.private_offset;
                c.copied = v.copied_bytes;
                c.view = D3D12_CONSTANT_BUFFER_VIEW_DESC { BufferLocation: v.view_location, SizeInBytes: v.view_bytes };
                c.resource = v.resource;
                c.owner = v.owner;
            }
            if let Some(k) = q.copies {
                c.copies_owner = k.owner;
                c.copies_list = k.list;
                c.copies_generation = k.generation;
                c.copies_occurrence = k.occurrence;
                c.copies_copied = k.copied_bytes;
                c.copies_count = k.count;
            }
            c.finalized = q.finalized;
            c
        })
        .collect();
    let plans: Vec<Option<ReadPlan>> = input.srvs.iter().map(|q| q.value.and_then(|v| domain_plan(input.family, &v.b4))).collect();
    let srvs: Vec<ab::PrivateSrv> = input
        .srvs
        .iter()
        .zip(&plans)
        .map(|(q, plan)| {
            // SAFETY: plain data, all zeroes first.
            let mut s: ab::PrivateSrv = unsafe { core::mem::zeroed() };
            s.scope = scope;
            s.value = at(q.value);
            s.parameter = q.parameter;
            s.offset = q.offset;
            if let Some(v) = q.value {
                s.view_scope = ab::ViewScope { root: v.scope.root, list: v.scope.list, generation: v.scope.generation, occurrence: v.scope.occurrence, eye: v.scope.eye };
                s.view_resource = v.resource as u64;
                s.view_owner = v.owner as u64;
                s.view_srv = v.srv;
                s.view_source_offset = v.source_offset;
                s.view_copied = v.copied_bytes;
                s.view_first_record = v.first_record;
                s.view_record_span = v.record_span;
                s.view_record_count = v.record_count;
                s.view_records = v.records;
                s.view_b4 = v.b4;
                // SAFETY: the view.
                s.view_valid = unsafe { crate::structured_view_owner::cvr_sov2_validate(v) };
                if let Some(p) = plan {
                    s.plan_ok = true;
                    s.plan_coverage_complete = p.coverage_complete;
                    s.plan_superset = p.superset;
                    s.plan_records = p.records.as_ptr();
                    s.plan_record_count = p.records.len();
                    s.plan_first_record = p.numbers.first_record;
                    s.plan_record_span = p.numbers.record_span;
                    s.plan_resource_offset = p.numbers.resource_offset;
                    s.plan_byte_extent = p.numbers.byte_extent;
                }
            }
            s.original_description = q.original.description;
            s.original_resource = q.original.resource;
            s.original_view = q.original.view;
            // SAFETY: the same 176-byte DescriptorStamp record.
            s.original_stamp = unsafe { core::ptr::read((&raw const q.original.stamp).cast::<ab::Stamp>()) };
            s.original_owner = q.original_owner;
            s.original_identity = q.original_identity;
            s.original_version = q.original_version;
            s.original_gpu = q.original_gpu;
            s.original_heap = q.original_heap;
            s.resource_cookie = q.resource_cookie;
            s.original_count = q.original_count;
            s.slot_certified = q.slot_certified;
            s.b4 = at(q.b4);
            s.b4_copies = at(q.b4_copies);
            if let Some(b) = q.b4 {
                s.b4_resource = b.resource;
                s.b4_owner = b.owner;
                s.b4_reflected = b.reflected_bytes;
                s.b4_copied = b.copied_bytes;
            }
            if let Some(k) = q.b4_copies {
                s.b4_copies_owner = k.owner;
            }
            s.b4_bytes = q.b4_bytes.as_ptr();
            s.b4_bytes_len = q.b4_bytes.len();
            s.b4_bytes_certified = q.b4_bytes_certified;
            s
        })
        .collect();
    let selections: Vec<crate::geometry_pass_coverage::Selection>;
    let intervals: Vec<Vec<crate::geometry_pass_coverage::Interval>>;
    let tables: Vec<crate::geometry_pass_coverage::Table>;
    let mut lent_selectors: Option<crate::geometry_pass_coverage::Selectors> = None;
    if let Some(r) = input.selectors {
        selections = r
            .selections
            .iter()
            .map(|s| crate::geometry_pass_coverage::Selection { site_index: s.site_index, source_dword: s.source_dword, shader_index: s.shader_index, descriptor_offset: s.descriptor_offset })
            .collect();
        intervals = r.tables.iter().map(|t| t.uses.iter().map(|u| crate::geometry_pass_coverage::Interval { offset: u.offset, count: u.count, kind: u.kind }).collect()).collect();
        tables = r
            .tables
            .iter()
            .zip(&intervals)
            .map(|(t, uses)| crate::geometry_pass_coverage::Table {
                root: pass_str(&t.key.root),
                visibility: t.key.visibility,
                parameter: t.key.parameter,
                space: t.key.space,
                shader_register_base: t.key.shader_register_base,
                original_table_offset: t.key.original_table_offset,
                declared_extent: t.key.declared_extent,
                offsets: t.offsets.as_ptr(),
                offset_count: t.offsets.len(),
                count: t.count,
                uses: uses.as_ptr(),
                use_count: uses.len(),
            })
            .collect();
        lent_selectors = Some(crate::geometry_pass_coverage::Selectors {
            family: [pass_str(&r.family.root), pass_str(&r.family.vertex), pass_str(&r.family.pixel)],
            evaluated_sites: r.evaluated_sites,
            suppressed: r.suppressed,
            complete: r.complete,
            permission: r.permission,
            permitted: r.permitted,
            selections: selections.as_ptr(),
            selection_count: selections.len(),
            tables: tables.as_ptr(),
            table_count: tables.len(),
        });
    }
    let lent = ab::Input {
        scope,
        root: input.root,
        uses: input.uses.as_ptr(),
        use_count: input.uses.len(),
        finite: finite.as_ptr(),
        finite_count: finite.len(),
        sparse: sparse.as_ptr(),
        sparse_count: sparse.len(),
        private_cbvs: cbvs.as_ptr(),
        private_cbv_count: cbvs.len(),
        original_cbvs: core::ptr::null(),
        original_cbv_count: 0,
        family: [pass_str(&input.family.root), pass_str(&input.family.vertex), pass_str(&input.family.pixel)],
        selector_scope: scope,
        selectors: lent_selectors.as_ref().map_or(core::ptr::null(), |s| s as *const _),
        dynamic_coverage_certified: input.dynamic,
        guarded: guarded.is_some(),
        private_srvs: srvs.as_ptr(),
        private_srv_count: srvs.len(),
        guarded_t5: guarded.map_or(0, |g| g.0),
        guarded_receipt: guarded.map_or(0, |g| g.1),
        multilayer: [pass_str(ml::ROOT_SHA256.as_bytes()), pass_str(ml::VERTEX_SHA256.as_bytes()), pass_str(ml::PIXEL_SHA256.as_bytes())],
    };
    // SAFETY: the lent arrays live for the call; the objects they name are the source's own.
    unsafe { ab::build_plugin(&lent) }
}

// ---- BuildGuarded (formerly source_binding_assembly::BuildGuarded)

/// ShaderInputCheck (its resource and owner: the source's own, alive with the work).
pub(super) struct InputCheck {
    pub resource: u64,
    pub owner: u64,
    pub offset: u64,
    pub expected: Vec<u8>,
}
/// GuardedT5View (lent to the .cpp as is).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GuardedT5 {
    pub resource: u64,
    pub owner: u64,
    pub srv: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub offset: u64,
    pub bytes: u64,
}
/// GuardedPlan (its plan ready and its checks non-empty).
pub(super) struct GuardedPlan {
    pub plan: ab::Plan,
    pub checks: Vec<InputCheck>,
    pub t5: Option<GuardedT5>,
}
/// PredictedT5.
pub(super) struct PredictedT5<'a> {
    pub original: SrvIn<'a>,
    pub copy: &'a crate::structured_snapshot_receipt::PrivateRange,
    pub receipt: &'a crate::structured_snapshot_receipt::PreparedCopy,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub expected: &'a [u8],
    pub recorded_view_certified: bool,
}
fn resource_desc(resource: u64) -> Option<D3D12_RESOURCE_DESC> {
    let raw = resource as *mut c_void;
    // SAFETY: a live resource the source holds.
    unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.map(|r| unsafe { r.GetDesc() })
}
const CBV_RANGE_TYPE: u32 = 2;
/// BuildGuarded: the predicted CBV bytes (and, for the multilayer family, the predicted T5) checked against the
/// source's private copies, the selectors evaluated from them, the plan built with the guarded validation.
pub(super) fn build_guarded(input: AsmInput, recipe: Option<&sel::FamilyPlan>, predicted: &[(sel::CbvKey, &[u8])], t5: Option<&PredictedT5>) -> Result<GuardedPlan, u32> {
    use ab::failure::*;
    let Some(recipe) = recipe.filter(|r| r.key == *input.family) else { return Err(ARGUMENTS) };
    if input.dynamic || input.selectors.is_some() || !input.srvs.is_empty() || predicted.is_empty() || predicted.len() > 2 || !ab::valid_scope(&input.scope) || input.root.is_null() {
        return Err(ARGUMENTS);
    }
    let mut checks = Vec::new();
    let mut bytes: Vec<(sel::CbvKey, Option<&[u8]>)> = Vec::new();
    let mut keys: Vec<&sel::CbvKey> = Vec::new();
    for (key, expected) in predicted {
        let found = input.cbvs.iter().find(|c| c.value.is_some_and(|v| v.parameter == key.parameter && v.offset == key.table_offset));
        if key.root != input.family.root || keys.contains(&key) {
            return Err(CBV_OWNER);
        }
        keys.push(key);
        let Some((cbv, value)) = found.and_then(|c| c.value.map(|v| (c, v))) else { return Err(CBV_OWNER) };
        if !cbv.finalized
            || cbv.copies.is_none_or(|k| k.owner == 0)
            || value.owner == 0
            || value.resource == 0
            || expected.is_empty()
            || expected.len() as u64 != value.copied_bytes
            || value.reflected_bytes as usize != expected.len()
        {
            return Err(CBV_OWNER);
        }
        if !input.uses.iter().any(|u| {
            u.kind == CBV_RANGE_TYPE && u.visibility == key.visibility && u.space == key.space && u.shader_register == key.shader_register && u.count == 1 && u.bytes as usize == expected.len()
        }) {
            return Err(DYNAMIC_COVERAGE);
        }
        bytes.push((key.clone(), Some(*expected)));
        checks.push(InputCheck { resource: value.resource, owner: value.owner, offset: value.private_offset, expected: expected.to_vec() });
    }
    let evaluated: SelectorResult;
    let mut t5_view = None;
    // The conditional private view (its address the guarded validation's): boxed for a stable address.
    let mut conditional: Option<Box<PrivateView>> = None;
    let mut replacement: Option<SrvIn> = None;
    let mut receipt_owner = 0u64;
    if recipe.supported {
        if t5.is_some() {
            return Err(DYNAMIC_COVERAGE);
        }
        let e = sel::evaluate(recipe, &bytes).map_err(|_| DYNAMIC_COVERAGE)?;
        evaluated = SelectorResult { family: e.family, evaluated_sites: e.evaluated_sites, selections: e.selections, tables: e.tables, complete: true, ..Default::default() };
    } else {
        let Some(t5) = t5.filter(|_| bytes.len() == 1) else { return Err(DYNAMIC_COVERAGE) };
        let original = &t5.original;
        if original.parameter != 23
            || original.offset != 5
            || original.value.is_some()
            || original.b4.is_some()
            || original.b4_copies.is_some()
            || original.b4_bytes_certified
            || !original.b4_bytes.is_empty()
            || !original.slot_certified
            || original.original.resource == 0
            || !crate::assembly_layout::exact_structured(&original.original.view)
        {
            return Err(DYNAMIC_COVERAGE);
        }
        let width = resource_desc(original.original.resource).map_or(0, |d| d.Width);
        let view = StructuredView {
            key: sel::CbvKey { root: input.family.root.clone(), visibility: 5, shader_register: 5, space: 0, parameter: 23, table_offset: 5 },
            resource_width: width,
            num_elements: 8192,
            structure_byte_stride: 128,
            dimension: 1,
            component_mapping: 5768,
            explicit_description: true,
            ..Default::default()
        };
        let read = plan_read(input.family, &bytes[0].0, bytes[0].1.unwrap_or(&[]), &view).filter(|r| r.coverage_complete && r.superset).ok_or(DYNAMIC_COVERAGE)?;
        let expected = (!t5.expected.is_empty()).then_some(t5.expected);
        evaluated = enumerate(&read, &view, read.numbers.resource_offset, expected).ok_or(DYNAMIC_COVERAGE)?;
        let (copy, receipt) = (t5.copy, t5.receipt);
        if read.numbers.byte_extent != 0 {
            let scope = &input.scope;
            if !t5.recorded_view_certified
                || copy.owner.is_null()
                || copy.resource.is_null()
                || receipt.owner.is_null()
                || copy.resource as u64 == original.original.resource
                || receipt.list != scope.list
                || receipt.generation != scope.generation
                || receipt.occurrence != scope.occurrence
                || receipt.source_offset != copy.source_offset
                || receipt.private_offset != copy.private_offset
                || receipt.copied_bytes != copy.copied_bytes
                || copy.source_offset != read.numbers.resource_offset
                || copy.copied_bytes != u64::from(read.numbers.byte_extent)
                || t5.expected.len() as u64 != copy.copied_bytes
                || copy.private_offset < copy.source_offset
                || copy.private_offset % 128 != 0
            {
                return Err(SRV_OWNER);
            }
            let raw = copy.resource;
            // SAFETY: the source's live private copy.
            let resource = unsafe { ID3D12Resource::from_raw_borrowed(&raw) }.cloned().ok_or(SRV_RANGE)?;
            // SAFETY: a live resource.
            let desc = unsafe { resource.GetDesc() };
            let start = copy.private_offset - copy.source_offset;
            let mut heap = D3D12_HEAP_PROPERTIES::default();
            let mut flags = D3D12_HEAP_FLAGS::default();
            if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
                || desc.Flags.0 != 0
                || desc.Width > crate::structured_view_overlay::MAX_BYTES
                || start > desc.Width
                || crate::structured_view_overlay::DOMAIN_BYTES > desc.Width - start
                // SAFETY: local outputs.
                || unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err()
                || heap.Type != D3D12_HEAP_TYPE_DEFAULT
            {
                return Err(SRV_RANGE);
            }
            let mut expected_view = original.original.view;
            expected_view.Anonymous.Buffer.FirstElement = start / 128;
            if !crate::assembly_layout::same_srv(&expected_view, &t5.view) {
                return Err(SRV_RANGE);
            }
            let Some(b4) = input.cbvs.iter().find(|c| c.value.is_some_and(|v| v.parameter == 19 && v.offset == 0)) else { return Err(SRV_RANGE) };
            // SAFETY: plain data, all zeroes first.
            let mut view: PrivateView = unsafe { core::mem::zeroed() };
            view.scope = crate::structured_snapshot::ViewScope { root: scope.root, list: scope.list, generation: scope.generation, occurrence: scope.occurrence, eye: scope.eye };
            view.resource = copy.resource;
            view.owner = copy.owner;
            view.srv = t5.view;
            view.source_offset = copy.source_offset;
            view.dense_offset = copy.private_offset;
            view.target_offset = copy.private_offset;
            view.copied_bytes = copy.copied_bytes;
            view.copy_list = receipt.list;
            view.copy_generation = receipt.generation;
            view.first_record = read.numbers.first_record;
            view.record_span = read.numbers.record_span;
            view.record_count = read.records.len() as u32;
            for (slot, record) in view.records.iter_mut().zip(&read.records) {
                *slot = *record;
            }
            view.b4 = read.b4_bytes;
            let view = Box::new(view);
            let mut srv = original.clone();
            srv.b4 = b4.value;
            srv.b4_copies = b4.copies;
            srv.b4_bytes = bytes[0].1.unwrap_or(&[]);
            receipt_owner = receipt.owner as u64;
            checks.push(InputCheck { resource: copy.resource as u64, owner: copy.owner as u64, offset: copy.private_offset, expected: t5.expected.to_vec() });
            t5_view = Some(GuardedT5 { resource: copy.resource as u64, owner: copy.owner as u64, srv: t5.view, offset: copy.private_offset, bytes: copy.copied_bytes });
            conditional = Some(view);
            replacement = Some(srv);
        } else if !t5.expected.is_empty() || !copy.resource.is_null() || !copy.owner.is_null() || !receipt.owner.is_null() {
            return Err(DYNAMIC_COVERAGE);
        }
    }
    let mut lent: AsmInput = input;
    if let (Some(srv), Some(view)) = (replacement, conditional.as_deref()) {
        lent.srvs = vec![SrvIn { value: Some(view), ..srv }];
    }
    lent.selectors = Some(&evaluated);
    let guard = (conditional.as_deref().map_or(0, |v| v as *const PrivateView as u64), receipt_owner);
    let plan = build_plan(&lent, Some(guard))?;
    if !plan.ready {
        return Err(NOT_READY);
    }
    Ok(GuardedPlan { plan, checks, t5: t5_view })
}

/// AssemblePass over Rust plans (shared_table.rs): each plan's tables with their SRV coverage receipts.
fn assemble_pass(w: &WorkScalars, plans: &[*const ab::Plan], cache: &mut Option<crate::assembly_pass::Cache>, failure: &mut u32) -> Result<Vec<Vec<crate::shared_table::SharedTable>>, i32> {
    let Some(device) = w.device.as_ref() else {
        *failure = ab::failure::ARGUMENTS;
        return Err(windows::Win32::Foundation::E_INVALIDARG.0);
    };
    // SAFETY: the work's plans, alive for the call.
    let plans: Vec<&ab::Plan> = plans.iter().map(|&p| unsafe { &*p }).collect();
    match crate::shared_table::assemble_pass(device, &plans, cache) {
        Ok(tables) => {
            *failure = ab::failure::NONE;
            Ok(tables)
        }
        Err((hr, reason)) => {
            *failure = reason;
            Err(hr.0)
        }
    }
}
fn cache_counts(cache: &Option<crate::assembly_pass::Cache>) -> [u64; 4] {
    let mut counts = [0u64; 4];
    // SAFETY: the cache and a local output.
    unsafe { crate::assembly_pass::cvr_sba_cache_counts(cache, &mut counts) };
    counts
}

// ---- BuildGuardedSources (formerly the .cpp's)

const GUARDED_ROWS: u32 = 1;
fn publish_guarded(w: &mut WorkScalars) {
    let rows = rows_mut::<16>(w, GUARDED_ROWS);
    *ex!(CyberpunkVR_RetainedGuardedRows) = rows.as_ptr() as u64;
    *ex!(CyberpunkVR_RetainedGuardedCount) = rows.len() as u32;
}
/// A source's scope as the assembly reads it (the saved row's root, list and generation; its ordinal).
fn asm_scope(saved: &[u64], index: usize) -> AsmScope {
    AsmScope { root: saved[11], list: saved[6], generation: saved[7], occurrence: index as u64 + 1, source: index as u64 + 1, eye: saved[1] as u32 }
}
/// The source's family, its shader root decoded (None: the Root refusal).
fn family_root(source: &SourceScalars, pipeline: *mut c_void) -> Option<(std::sync::Arc<crate::families_native::Family>, DecodedRoot)> {
    let family = crate::families_native::lookup_raw(pipeline)?;
    if family.source_root.as_raw() != source.root {
        return None;
    }
    let root = DecodedRoot::new(&shader_root(source, pipeline)?)?;
    Some((family, root))
}
/// BuildGuardedSources: every source's guarded plan (its predicted inputs), then one guarded pass.
pub(super) fn build_guarded_sources(state: &mut WorkSources, w: &mut WorkScalars, captures: &mut [SourceCapture]) {
    use ab::failure::*;
    let (e0, e1) = (w.eye(0), w.eye(1));
    if w.guarded_attempted || w.failed || !w.source_selectors || !w.source_cbv_snapshots || !w.source_tables || !e0.sealed || !e1.sealed || !e0.submission_certified || !e1.submission_certified {
        return;
    }
    w.guarded_attempted = true;
    let count = captures.len();
    let stats = ex!(CyberpunkVR_RetainedGuardedStats);
    stats[0] = 1;
    stats[1] = u64::from(w.frame);
    stats[2] = count as u64;
    stats[7] = u64::from(e0.sealed);
    stats[8] = u64::from(e1.sealed);
    stats[9] = u64::from(e0.submission_certified);
    stats[10] = u64::from(e1.submission_certified);
    stats[11] = u64::from(w.payloads_processed);
    stats[12] = u64::from(w.cbv_payload_complete);
    stats[13] = u64::from(w.overlay_stage);
    stats[14] = u64::MAX;
    let reject = |w: &mut WorkScalars, reason: u32, index: u64| {
        let stats = ex!(CyberpunkVR_RetainedGuardedStats);
        stats[0] = 3;
        stats[14] = index;
        stats[15] = u64::from(reason);
        fail(2400 + reason);
        publish_guarded(w);
    };
    if w.payloads_processed || w.cbv_payload_complete || w.overlay_stage != 0 || count == 0 || count != rows_mut::<48>(w, SOURCE_ROWS).len() {
        reject(w, SCOPE, u64::MAX);
        return;
    }
    journal_of(w).reserve(GUARDED_ROWS, count);
    for index in 0..count {
        let saved = rows_mut::<48>(w, SOURCE_ROWS)[index];
        if saved[0] != index as u64 || saved[1] > 1 {
            reject(w, SCOPE, index as u64);
            return;
        }
        let scope = asm_scope(&saved, index);
        let eye = w.eye(scope.eye as usize);
        let Some(source) = super::source(index as u64) else {
            reject(w, SCOPE, index as u64);
            return;
        };
        let cap = &mut captures[index];
        let mut row = [0u64; 16];
        row[..6].copy_from_slice(&[index as u64, u64::from(scope.eye), scope.list, scope.generation, scope.occurrence, scope.root]);
        row[11] = source.private_cbvs.as_slice().len() as u64;
        row[12] = cap.t5_plan.as_ref().map_or(0, |p| u64::from(p.numbers.byte_extent));
        row[13] = u64::from(eye.sealed);
        row[14] = u64::from(eye.submission_certified);
        row[15] = u64::from(w.payloads_processed);
        let result = guarded_source(source, cap, scope, &saved, eye);
        match result {
            Ok(plan) => {
                cap.guarded_plan = Some(plan);
                row[6] = 1;
                journal_of(w).append(GUARDED_ROWS, &row);
                ex!(CyberpunkVR_RetainedGuardedStats)[3] += 1;
            }
            Err(failure) => {
                cap.guarded_plan = None;
                row[7] = u64::from(failure);
                journal_of(w).append(GUARDED_ROWS, &row);
                reject(w, failure, index as u64);
                return;
            }
        }
    }
    let plans: Vec<*const ab::Plan> = captures.iter().map(|c| c.guarded_plan.as_ref().map_or(core::ptr::null(), |g| &g.plan as *const ab::Plan)).collect();
    // AssembleGuardedPass: every plan prepared with its checks.
    let mut failure = ARGUMENTS;
    let mut assembled = None;
    if !plans.is_empty() && plans.len() <= 2048 {
        failure = NOT_READY;
        if captures.iter().all(|c| c.guarded_plan.as_ref().is_some_and(|g| g.plan.ready && !g.checks.is_empty())) {
            assembled = assemble_pass(w, &plans, &mut state.assembly_cache, &mut failure).ok();
        }
    }
    let Some(tables) = assembled else {
        reject(w, if failure == NONE { SCOPE } else { failure }, u64::MAX);
        return;
    };
    let rows = rows_mut::<16>(w, GUARDED_ROWS);
    let stats = ex!(CyberpunkVR_RetainedGuardedStats);
    for (index, cap) in captures.iter().enumerate() {
        let guarded = cap.guarded_plan.as_ref().expect("prepared above");
        let row = &mut rows[index];
        row[8] = tables.get(index).map_or(0, Vec::len) as u64;
        row[9] = guarded.checks.len() as u64;
        row[10] += guarded.checks.iter().map(|c| c.expected.len() as u64).sum::<u64>();
        stats[4] += 1;
        stats[5] += row[9];
        stats[6] += row[10];
    }
    state.guarded_tables = Some(tables);
    *ex!(CyberpunkVR_RetainedGuardedCacheStats) = cache_counts(&state.assembly_cache);
    stats[0] = 2;
    publish_guarded(w);
}
/// One source's guarded plan (BuildGuardedSources' loop body).
fn guarded_source(source: &SourceScalars, cap: &SourceCapture, scope: AsmScope, saved: &[u64; 48], eye: &EyeScalars) -> Result<GuardedPlan, u32> {
    use ab::failure::*;
    let pipeline = saved[10] as *mut c_void;
    let (family, root) = family_root(source, pipeline).ok_or(ROOT)?;
    let key = family_key(&family);
    let recipe = selector_catalog().and_then(|c| c.find(&key));
    let finalized = !eye.cbv_arena_aborted && arena_op(scope.eye, arena::CBV, 1);
    let cbvs: Vec<CbvIn> = source.private_cbvs.as_slice().iter().map(|v| CbvIn { value: Some(v), copies: Some(&source.cbv_prepared), finalized }).collect();
    let predicted: Vec<(sel::CbvKey, &[u8])> = cap.selector_inputs.iter().filter_map(|i| valid_bytes(&i.intent).map(|b| (i.key.clone(), b))).collect();
    let mut t5 = None;
    if recipe.is_some_and(|r| !r.supported) {
        let Some(original) = cap.tables.iter().map(|t| &t.0).find(|t| t.parameter == 23).filter(|_| source.t5_original_certified) else { return Err(SRV_OWNER) };
        let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
        let mut certified = false;
        if cap.t5_plan.as_ref().is_some_and(|p| p.numbers.byte_extent != 0) && arena_op(scope.eye, arena::T5, 1) && !eye.t5_aborted {
            if let Some(recorded) = recorded_view(source) {
                view = recorded;
                certified = true;
            }
        }
        t5 = Some(PredictedT5 {
            original: SrvIn {
                parameter: 23,
                offset: 5,
                value: None,
                original: &source.multilayer_t5,
                original_owner: original.owner as u64,
                original_identity: original.identity,
                original_version: original.version,
                original_gpu: original.source_gpu,
                original_heap: original.source_heap,
                resource_cookie: source.multilayer_t5.description[2],
                original_count: original.count,
                slot_certified: source.t5_original_certified,
                b4: None,
                b4_copies: None,
                b4_bytes: &[],
                b4_bytes_certified: false,
            },
            copy: &source.t5_private,
            receipt: &source.t5_prepared,
            view,
            expected: valid_bytes(&cap.t5_candidate).unwrap_or(&[]),
            recorded_view_certified: certified,
        });
    }
    let input = AsmInput {
        scope,
        root: root.desc(),
        uses: family_uses(&family),
        finite: &cap.tables,
        sparse: &cap.selector_slots,
        cbvs,
        family: &key,
        selectors: None,
        dynamic: false,
        srvs: Vec::new(),
    };
    build_guarded(input, recipe, &predicted, t5.as_ref())
}

// ---- BuildSourceAssemblies (formerly the .cpp's)

const ASSEMBLY_COMMANDS: u32 = 53;
const ASSEMBLY_TABLES: u32 = 54;
const ASSEMBLY_MISSING: u32 = 55;
/// SourceAssemblyCommandLimit, SourceAssemblyDetailLimit.
const ASSEMBLY_COMMAND_LIMIT: usize = 2048;
const ASSEMBLY_DETAIL_LIMIT: usize = 65536;

/// One source's assembly row and plan (BuildSourceAssemblies' loop body); its failure.
fn assemble_source(w: &mut WorkScalars, index: usize, cap: &mut SourceCapture, row: &mut [u64; 32], truncated: &mut bool) -> u32 {
    use ab::failure::*;
    let saved = rows_mut::<48>(w, SOURCE_ROWS)[index];
    let Some(source) = super::source(index as u64) else { return SCOPE };
    let journal = journal_of(w);
    let counts = crate::retained_source_rows::AssemblyCounts {
        private_cbvs: source.private_cbvs.as_slice().len() as u64,
        tables: cap.tables.len() as u64,
        selector_slots: cap.selector_slots.len() as u64,
        assembly_tables: journal.count(ASSEMBLY_TABLES) as u64,
        missing_cbvs: journal.count(ASSEMBLY_MISSING) as u64,
        root: source.root as u64,
    };
    let scoped = crate::retained_source_rows::assembly_head(row, index as u64, &saved, &counts);
    let mut failure = NONE;
    let scope = AsmScope { root: saved[11], list: saved[6], generation: saved[7], occurrence: saved[0] + 1, source: saved[0] + 1, eye: saved[1] as u32 };
    let mut receipt = |table: [u64; 10], kind: u64, descriptors: u64, row: &mut [u64; 32]| {
        row[7] += table[1];
        if journal.count(ASSEMBLY_TABLES) >= ASSEMBLY_DETAIL_LIMIT {
            *truncated = true;
            failure = CAPACITY;
            return;
        }
        let mut value = [0u64; 16];
        value[..12].copy_from_slice(&[
            index as u64,
            saved[1],
            table[0],
            table[1],
            kind,
            descriptors,
            table[2],
            table[3],
            table[4],
            table[5],
            table[6],
            table[7],
        ]);
        journal.append(ASSEMBLY_TABLES, &value);
    };
    for t in &cap.tables {
        let t = &t.0;
        receipt(
            [u64::from(t.parameter), u64::from(t.count), t.cpu_heap as u64, t.owner as u64, t.identity, t.version, t.source_gpu, t.source_heap, 0, 0],
            1,
            u64::from(t.count),
            row,
        );
    }
    for t in &cap.selector_slots {
        row[20] += t.slots.len() as u64;
        receipt(
            [u64::from(t.parameter), u64::from(t.count), t.cpu_heap as u64, t.owner as u64, t.identity, t.version, t.source_gpu, t.source_heap, 0, 0],
            2,
            t.slots.len() as u64,
            row,
        );
    }
    let pipeline = saved[10] as *mut c_void;
    let family = crate::families_native::lookup_raw(pipeline);
    // The root decoded only past the family's own checks (the .cpp's short circuit).
    let decoded = family.as_ref().filter(|f| f.source_root.as_raw() == source.root).and_then(|_| shader_root(source, pipeline)).and_then(|bytes| DecodedRoot::new(&bytes));
    let domains = match (&family, &decoded) {
        (Some(f), Some(d)) => crate::assembly_build::source_plan(d.desc(), family_uses(f)),
        _ => None,
    };
    if !scoped {
        failure = SCOPE;
    } else if domains.is_none() {
        failure = ROOT;
    }
    let (Some(family), Some(decoded)) = (family, decoded) else { return failure };
    row[15] = family.id;
    let privates: Vec<crate::retained_assembly::PrivateCbv> = source
        .private_cbvs
        .as_slice()
        .iter()
        .map(|v| crate::retained_assembly::PrivateCbv { parameter: v.parameter, offset: v.offset, reflected: v.reflected_bytes, copied: v.copied_bytes, held: v.resource != 0 && v.owner != 0 })
        .collect();
    let originals: Vec<crate::retained_assembly::OriginalCbv> = source
        .cbvs
        .as_slice()
        .iter()
        .map(|v| crate::retained_assembly::OriginalCbv {
            gpu: v.stamp.gpu,
            list: v.recording.list,
            generation: v.recording.generation,
            resource: v.resource as *mut c_void,
            heap: v.stamp.heap,
            descriptor_bytes: u64::from(v.descriptor_bytes),
        })
        .collect();
    let first = journal.count(ASSEMBLY_MISSING);
    let room = 128usize.min(ASSEMBLY_DETAIL_LIMIT.saturating_sub(first));
    let mut rows = vec![[0u64; 16]; room];
    let prepared = &source.cbv_prepared;
    let stride = w.device.as_ref().map_or(0, |d| unsafe { d.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) });
    let lent = crate::retained_assembly::Input {
        root: decoded.desc(),
        uses: family_uses(&family).as_ptr(),
        use_count: family_uses(&family).len(),
        privates: privates.as_ptr(),
        private_count: privates.len(),
        originals: originals.as_ptr(),
        original_count: originals.len(),
        tables: source.bindings.tables.as_ptr(),
        table_count: source.bindings.tables.len(),
        source: index as u64,
        eye: saved[1],
        list: scope.list,
        generation: scope.generation,
        stride,
        owns_copies: prepared.owner != 0 && prepared.list == scope.list && prepared.generation == scope.generation && prepared.occurrence == scope.occurrence,
        out: rows.as_mut_ptr(),
        room,
    };
    let mut result = crate::retained_assembly::Output { requirements: 0, missing: 0, written: 0, failure: 0, truncated: false };
    // SAFETY: the lent arrays and rows live for the call.
    unsafe { crate::retained_assembly::cvr_nrg_missing_cbvs(&lent, &mut result) };
    for r in &rows[..result.written.min(room)] {
        journal.append(ASSEMBLY_MISSING, r);
    }
    row[31] = result.requirements;
    row[5] += result.missing;
    if result.failure != 0 {
        failure = result.failure;
    }
    if result.truncated {
        *truncated = true;
    }
    let side = saved[1] as u32;
    let eye = w.eye(side as usize);
    row[10] = u64::from(!eye.cbv_arena_aborted && arena_op(side, arena::CBV, 1));
    let dynamic = domains.as_ref().is_some_and(|d| d.deferred.iter().any(|u| u.2.count > 1));
    let key = family_key(&family);
    let catalog = selector_catalog().and_then(|c| c.find(&key));
    let multilayer = key.root == ml::ROOT_SHA256.as_bytes()
        && key.vertex == ml::VERTEX_SHA256.as_bytes()
        && key.pixel == ml::PIXEL_SHA256.as_bytes()
        && cap.t5_plan.as_ref().is_some_and(|p| p.family == key && p.coverage_complete);
    let selector_rows = rows_mut::<32>(w, SELECTOR_COMMANDS);
    crate::retained_source_rows::selector_certified(
        row,
        &saved,
        selector_rows,
        catalog.map_or(0, |c| if c.supported { 2 } else { 1 }),
        multilayer,
        cap.selector_result.complete && cap.selector_result.family == key,
    );
    let finalized = row[10] != 0;
    let t5_b4 = cap.t5_plan.as_ref().is_some_and(|p| p.numbers.byte_extent != 0).then(|| overlay_b4(w, index, cap));
    let mut input = AsmInput {
        scope,
        root: decoded.desc(),
        uses: family_uses(&family),
        finite: &cap.tables,
        sparse: &cap.selector_slots,
        cbvs: source.private_cbvs.as_slice().iter().map(|v| CbvIn { value: Some(v), copies: Some(&source.cbv_prepared), finalized }).collect(),
        family: &key,
        selectors: None,
        dynamic: false,
        srvs: Vec::new(),
    };
    if dynamic {
        input.selectors = Some(&cap.selector_result);
        input.dynamic = row[9] != 0;
    }
    if let Some(found) = &t5_b4 {
        let original = cap.tables.iter().map(|t| &t.0).find(|t| t.parameter == 23);
        match (w.overlay_stage == 2, cap.t5_view.as_ref(), source.t5_original_certified, original, found) {
            (true, Some(view), true, Some(original), Some((b4, bytes, _))) => {
                input.srvs.push(SrvIn {
                    parameter: 23,
                    offset: 5,
                    value: Some(&view.0),
                    original: &source.multilayer_t5,
                    original_owner: original.owner as u64,
                    original_identity: original.identity,
                    original_version: original.version,
                    original_gpu: original.source_gpu,
                    original_heap: original.source_heap,
                    resource_cookie: source.multilayer_t5.description[2],
                    original_count: original.count,
                    slot_certified: true,
                    b4: Some(&source.private_cbvs.as_slice()[*b4]),
                    b4_copies: Some(&source.cbv_prepared),
                    b4_bytes: bytes,
                    b4_bytes_certified: true,
                });
            }
            _ => failure = SRV_OWNER,
        }
    }
    if failure == NONE && (!w.cbv_payload_complete || w.cbv_snapshots_failed || (!source.private_cbvs.as_slice().is_empty() && row[10] == 0)) {
        failure = NOT_READY;
    }
    if failure == NONE {
        match build_plan(&input, None) {
            Ok(plan) => {
                row[2] = 1;
                row[4] = u64::from(plan.ready);
                if plan.missing.len() as u64 != row[5] {
                    failure = CBV_OWNER;
                    row[4] = 0;
                }
                cap.binding_plan = Some(plan);
            }
            Err(f) => {
                failure = f;
                cap.binding_plan = None;
            }
        }
    }
    failure
}
/// BuildSourceAssemblies: every source's binding plan and rows, then one pass over the ready plans and the bank.
pub(super) fn build_source_assemblies(state: &mut WorkSources, w: &mut WorkScalars, captures: &mut [SourceCapture]) {
    if !w.source_selectors || w.assembly_complete {
        return;
    }
    let count = captures.len();
    let journal = journal_of(w);
    journal.reserve(ASSEMBLY_COMMANDS, count.min(ASSEMBLY_COMMAND_LIMIT));
    journal.reserve(ASSEMBLY_TABLES, (count * 64).min(ASSEMBLY_DETAIL_LIMIT));
    journal.reserve(ASSEMBLY_MISSING, (count * 16).min(ASSEMBLY_DETAIL_LIMIT));
    let sources = rows_mut::<48>(w, SOURCE_ROWS).len();
    for index in 0..count {
        if journal.count(ASSEMBLY_COMMANDS) >= ASSEMBLY_COMMAND_LIMIT || index >= sources {
            w.assembly_truncated = true;
            break;
        }
        let mut row = [0u64; 32];
        let mut truncated = false;
        let failure = assemble_source(w, index, &mut captures[index], &mut row, &mut truncated);
        if truncated {
            w.assembly_truncated = true;
        }
        crate::retained_source_rows::assembly_tail(&mut row, failure, journal.count(ASSEMBLY_TABLES) as u64, journal.count(ASSEMBLY_MISSING) as u64);
        journal.append(ASSEMBLY_COMMANDS, &row);
    }
    let commands = rows_mut::<32>(w, ASSEMBLY_COMMANDS);
    let indices: Vec<usize> = (0..commands.len()).filter(|&n| commands[n][4] != 0).collect();
    let plans: Vec<*const ab::Plan> = indices.iter().map(|&n| captures[n].binding_plan.as_ref().map_or(core::ptr::null(), |p| p as *const ab::Plan)).collect();
    if !plans.is_empty() {
        let mut failure = 0u32;
        let assembled = assemble_pass(w, &plans, &mut state.assembly_cache, &mut failure);
        let result = match &assembled {
            Ok(_) => 0,
            Err(hr) => *hr,
        };
        let overlays = rows_mut::<32>(w, OVERLAY_ROWS);
        let tables = rows_mut::<16>(w, ASSEMBLY_TABLES);
        for (n, &index) in indices.iter().enumerate() {
            let row = &mut commands[index];
            crate::retained_source_rows::assembled(row, index as u64, result, failure, overlays);
            if let Ok(pass) = &assembled {
                for at in row[27] as usize..(row[27] + row[28]) as usize {
                    if let Some(table) = tables.get_mut(at) {
                        let heap = pass[n].iter().find(|t| t.parameter() == table[2] as u32).map_or(0, |t| t.heap() as u64);
                        if heap != 0 {
                            table[12] = heap;
                        }
                    }
                }
            }
        }
        w.assembly_descriptors = cache_counts(&state.assembly_cache)[0];
        if let Ok(pass) = assembled {
            state.binding_index = vec![None; count];
            for (n, &index) in indices.iter().enumerate() {
                state.binding_index[index] = Some(n);
            }
            w.assembly_bank_attempted = true;
            let roots: Vec<(*mut c_void, Vec<u8>)> = indices
                .iter()
                .map(|&i| {
                    let source = super::source(i as u64).expect("assembled");
                    (source.root, shader_root(source, rows_mut::<48>(w, SOURCE_ROWS)[i][10] as *mut c_void).unwrap_or_default())
                })
                .collect();
            let inputs: Vec<(*mut c_void, &[u8], &[crate::shared_table::SharedTable])> = roots.iter().zip(&pass).map(|((root, bytes), t)| (*root, bytes.as_slice(), t.as_slice())).collect();
            let device = w.device.clone().expect("assembled on the work's device");
            let (created, rejected) = crate::shared_table::create_source_pass(&device, &inputs);
            w.assembly_bank_result = created.as_ref().err().map_or(0, |hr| hr.0);
            w.assembly_bank_failure = [u64::from(rejected.stage), u64::from(rejected.input), u64::from(rejected.parameter), rejected.bits];
            if let Ok(bank) = created {
                state.bank = Some(bank);
                crate::retained_source_rows::banked(commands, &indices, tables);
            }
            state.binding_tables = Some(pass);
        }
    }
    w.assembly_complete = true;
}

// ---- the wiring: the capture's entry, the poll's steps, the readers this side and the .cpp's adapters

/// The capture's entry at AppendSource (after the source's CBVs): a fresh capture at the source's index (a failed
/// append's left over replaced), its tables and selectors captured.
///
/// # Safety
/// `list` is the live recording list; `source` the .cpp's source being appended; `row` its source row.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_capture_sources(list: *mut c_void, source: *mut SourceScalars, row: *const [u64; 48]) {
    let Some(w) = work() else { return };
    // SAFETY: as documented.
    let row = unsafe { &*row };
    let index = row[0] as usize;
    let r = rust_side();
    r.captures.truncate(index);
    r.captures.resize_with(index + 1, SourceCapture::default);
    let cap = &mut r.captures[index];
    cap.pass_depth = eye_rust((row[1] as u32).min(1)).pass_depth.clone();
    cap.shader = crate::shader_snapshot_native::source(row[10] as *mut c_void);
    if w.source_tables {
        // SAFETY: as documented.
        capture_tables(&mut r.sources_state, w, list, unsafe { &*source }, cap, row);
    }
    if w.source_selectors {
        capture_selectors(&mut r.sources_state, w, list, source, cap, row);
    }
}
/// The work's captures of its appended sources.
fn captures() -> &'static mut [SourceCapture] {
    let count = source_count();
    let r = rust_side();
    if r.captures.len() < count {
        r.captures.resize_with(count, SourceCapture::default);
    }
    &mut r.captures[..count]
}
fn capture_of(index: usize) -> Option<&'static SourceCapture> {
    state().rust.as_ref().and_then(|r| r.captures.get(index))
}
/// A capture's tables as the native descriptor arena reads them (the same 64-byte OwnedSourceTable records).
pub(super) fn tables_view(tables: &[OwnedTable]) -> &[crate::native_descriptor_arena::SourceTable] {
    // SAFETY: OwnedTable is transparent over the same record.
    unsafe { core::slice::from_raw_parts(tables.as_ptr().cast(), tables.len()) }
}
/// The source's captured tables (empty when none).
pub(super) fn tables_of(index: usize) -> &'static [crate::native_descriptor_arena::SourceTable] {
    capture_of(index).map_or(&[], |c| tables_view(&c.tables))
}
/// The source's finite and sparse dependencies (descriptors::SourceResourceBinding records).
pub(super) fn dependencies_of(index: usize) -> [&'static [crate::material_buffer_snapshot::SourceResourceBinding]; 2] {
    let view = |d: &'static [OwnedDependency]| -> &'static [crate::material_buffer_snapshot::SourceResourceBinding] {
        // SAFETY: OwnedDependency is transparent over the same 208-byte record.
        unsafe { core::slice::from_raw_parts(d.as_ptr().cast(), d.len()) }
    };
    capture_of(index).map_or([&[], &[]], |c| [view(&c.finite_dependencies), view(&c.sparse_dependencies)])
}
/// The source's selected slots whose descriptors changed since the capture (descriptors::MatchesSourceSlot).
pub(super) fn selector_mismatches(index: usize) -> u32 {
    capture_of(index).map_or(0, |c| c.selector_slots.iter().flat_map(|t| &t.slots).filter(|s| !crate::audit_source_tables::matches_slot(s)).count() as u32)
}
/// The source's T5 read plan as the snapshot reads it.
pub(super) fn t5_plan_of(index: usize) -> T5Plan {
    capture_of(index).and_then(|c| c.t5_plan.as_ref()).map_or_else(T5Plan::default, |p| T5Plan {
        resource_offset: p.numbers.resource_offset,
        byte_extent: u64::from(p.numbers.byte_extent),
        coverage_complete: p.coverage_complete,
    })
}
/// The source's T5 intent candidate as the snapshot reads it (its views live while the capture does).
pub(super) fn t5_candidate_of(index: usize) -> T5Candidate {
    let mut out = T5Candidate { present: false, composed: false, copy_sequence: 0, bytes: core::ptr::null(), byte_count: 0, pieces: core::ptr::null(), piece_count: 0 };
    if let Some(c) = capture_of(index).and_then(|c| c.t5_candidate.as_ref()) {
        let (bytes, pieces) = (c.bytes(), c.pieces());
        out.bytes = bytes.as_ptr();
        out.byte_count = bytes.len();
        out.pieces = pieces.as_ptr().cast();
        out.piece_count = pieces.len();
        if c.valid() {
            out.present = true;
            out.composed = c.composed();
            out.copy_sequence = c.copy_sequence();
        }
    }
    out
}
/// The counts PublishSources reports from this side: the T5 overlay, the table cache and the assembly cache.
pub(super) fn publish_counts(c: &mut PublishCounts) {
    let Some(r) = state().rust.as_ref() else { return };
    let s = &r.sources_state;
    if let Some(overlay) = &s.t5_overlay {
        c.overlay_present = true;
        c.overlay_bytes = [overlay.required_bytes(), overlay.copied_bytes()];
    }
    c.table_cache = core::array::from_fn(|which| u64::from(s.cache_count(which as u32)));
    c.assembly_cache = cache_counts(&s.assembly_cache);
    if let Some(bank) = &s.bank {
        c.bank_present = true;
        c.bank = crate::shared_table::bank_counts(bank);
    }
}
/// Poll's payload sources (each source's T5 candidate bytes, selector inputs, T5 row and plan extent), lent until the
/// next call.
pub(super) fn payload_sources() -> (*const crate::retained_journal::PayloadSource, usize) {
    struct Lent(Vec<crate::retained_journal::PayloadSource>, Vec<Vec<crate::retained_journal::PayloadInput>>);
    // SAFETY: the plain pointers lent only under the capture's lock.
    unsafe impl Send for Lent {}
    static LENT: std::sync::Mutex<Lent> = std::sync::Mutex::new(Lent(Vec::new(), Vec::new()));
    let mut lent = LENT.lock().unwrap_or_else(|e| e.into_inner());
    let lent = &mut *lent;
    lent.0.clear();
    lent.1.clear();
    for index in 0..source_count() {
        let cap = capture_of(index);
        let inputs: Vec<crate::retained_journal::PayloadInput> = cap
            .map(|c| {
                c.selector_inputs
                    .iter()
                    .map(|i| {
                        let bytes = valid_bytes(&i.intent).unwrap_or(&[]);
                        crate::retained_journal::PayloadInput { row: i.row as u64, intent: bytes.as_ptr(), intent_len: bytes.len(), has_intent: valid_bytes(&i.intent).is_some() }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let t5 = cap.and_then(|c| valid_bytes(&c.t5_candidate));
        let row = super::source(index as u64).map_or(usize::MAX, |s| s.t5_row);
        lent.1.push(inputs);
        let inputs = lent.1.last().expect("pushed");
        lent.0.push(crate::retained_journal::PayloadSource {
            t5_candidate: t5.map_or(core::ptr::null(), <[u8]>::as_ptr),
            t5_candidate_len: t5.map_or(0, <[u8]>::len),
            has_t5_candidate: t5.is_some(),
            inputs: inputs.as_ptr(),
            input_count: inputs.len(),
            t5_row: row as u64,
            t5_byte_extent: cap.and_then(|c| c.t5_plan.as_ref()).map_or(0, |p| u64::from(p.numbers.byte_extent)),
        });
    }
    (lent.0.as_ptr(), lent.0.len())
}
/// The source's material plan: its binding plan ready, its T5 view and the view's owner.
pub(super) fn material_plan_of(index: usize) -> MaterialPlan {
    let Some(c) = capture_of(index) else { return MaterialPlan::default() };
    MaterialPlan {
        ready: c.binding_plan.as_ref().is_some_and(|p| p.ready),
        t5_view: c.t5_view.is_some(),
        t5_view_owner: c.t5_view.as_ref().map_or(0, |v| v.0.owner as u64),
    }
}
/// Poll's step 0 (PrepareT5Overlay).
pub(super) fn poll_overlay(w: &mut WorkScalars) -> bool {
    let captures = captures();
    prepare_t5_overlay(w, captures, &mut rust_side().sources_state.t5_overlay)
}
/// Poll's step 4 (BuildSourceAssemblies).
pub(super) fn poll_assemblies(w: &mut WorkScalars) {
    let captures = captures();
    build_source_assemblies(&mut rust_side().sources_state, w, captures);
}
/// The submission's first step (BuildGuardedSources), before the .cpp's staged and current sources.
pub(super) fn guarded(w: &mut WorkScalars) {
    let captures = captures();
    build_guarded_sources(&mut rust_side().sources_state, w, captures);
}

