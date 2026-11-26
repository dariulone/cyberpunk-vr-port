//! The retained capture's work and its sources, owned here (formerly NativeRetainedGeometry.cpp's Work, Eye,
//! Work::Source and AppendSource): a work's scalars, its two eyes, its journal and its sources; a source's raw
//! mirrors (the C++ records this module's captures fill) hold their COM references as the C++ members held them, and
//! a source releases them when dropped as the C++ destructor did. AppendSource records the source at the actual draw:
//! its row, its current IA leases, its indirect binding and evidence, its bindings and draw state, its graph parent,
//! its shader snapshot, the CBV, table, material, VRS, private-CBV, T5, instance, IA and borrowed captures, and for a
//! pure indirect draw its argument snapshot at the actual call.
use super::*;
use crate::geometry_leases::{BufferLease, DrawResources, DrawState};
use crate::retained_journal::Journal;
use windows::core::HRESULT;
use windows::Win32::Foundation::{E_ACCESSDENIED, E_INVALIDARG, S_OK};

// ---- the work

/// A work (formerly the .cpp's Work): its scalars first (the capture's `*mut WorkScalars` is this record).
#[repr(C)]
pub(super) struct OwnedWork {
    scalars: WorkScalars,
    eyes: [EyeScalars; 2],
    journal: Box<Journal>,
    sources: Vec<Box<SourceScalars>>,
}
impl Drop for OwnedWork {
    fn drop(&mut self) {
        // The C++ members' order: the sources before the eyes, the journal and the scalars.
        self.sources.clear();
    }
}
fn new_eye() -> EyeScalars {
    // SAFETY: an eye's all-zero record is its C++ default (null references, zero scalars) but for the two below.
    let mut eye: EyeScalars = unsafe { core::mem::zeroed() };
    eye.tail_first = u64::MAX;
    eye.seal_reason = u32::MAX;
    eye
}
/// A new work (its scalars), its eyes and journal wired.
pub(super) fn work_new() -> *mut WorkScalars {
    let mut owned = Box::new(OwnedWork {
        // SAFETY: the scalars' all-zero record is their C++ default (no device, every switch off, zero counters).
        scalars: unsafe { core::mem::zeroed() },
        eyes: [new_eye(), new_eye()],
        journal: Box::default(),
        sources: Vec::new(),
    });
    owned.scalars.journal = (&mut *owned.journal as *mut Journal).cast();
    owned.scalars.eye = [&mut owned.eyes[0] as *mut EyeScalars, &mut owned.eyes[1] as *mut EyeScalars];
    Box::into_raw(owned).cast()
}
/// Deletes a work this module made (never the installed one).
pub(super) fn work_free(value: *mut WorkScalars) {
    if !value.is_null() {
        // SAFETY: made by work_new.
        drop(unsafe { Box::from_raw(value.cast::<OwnedWork>()) });
    }
}
/// Installs `next` (null: none) as the capture's work, deleting the previous one.
pub(super) fn work_install(next: *mut WorkScalars) {
    let previous = core::mem::replace(&mut state().work, next);
    if previous != next {
        work_free(previous);
    }
}
fn owned_work() -> Option<&'static mut OwnedWork> {
    // SAFETY: the installed work (made by work_new), alive until replaced under the lock.
    unsafe { state().work.cast::<OwnedWork>().as_mut() }
}
/// The work's source `index`, None past its end (sources only append under the lock; each is boxed, so a reference
/// stays valid while the work lives).
pub(super) fn source_at(index: usize) -> Option<&'static mut SourceScalars> {
    owned_work()?.sources.get_mut(index).map(|s| {
        let raw: *mut SourceScalars = &mut **s;
        // SAFETY: the boxed source, alive with the work.
        unsafe { &mut *raw }
    })
}
pub(super) fn sources_len() -> usize {
    owned_work().map_or(0, |w| w.sources.len())
}
/// The selector catalog (phase 0, false: invalid) or the reservations (phase 1) of a new work.
pub(super) fn work_prepare(pending: *mut WorkScalars, phase: u32) -> bool {
    if phase == 0 {
        // SAFETY: the host's own function.
        return !unsafe { (host().selector_catalog)() }.is_null();
    }
    // SAFETY: a new work this module made.
    let owned = unsafe { &mut *pending.cast::<OwnedWork>() };
    let (p, journal) = (&owned.scalars, &mut *owned.journal);
    const MAX: usize = MAX_SOURCE_COMMANDS;
    let views = IA_MAX_VIEWS;
    if p.source_stream {
        owned.sources.reserve(MAX);
        for kind in [SOURCE_ROWS, INDIRECT_ROWS, SNAPSHOT_ROWS] {
            journal.reserve(kind, MAX);
        }
        if p.source_cbvs {
            journal.reserve(CBV_ROWS, MAX * 16);
            journal.reserve(CBV_DIAGNOSTICS, MAX * 16);
        }
    }
    if p.source_tables {
        journal.reserve(TABLE_COMMANDS, MAX);
        journal.reserve(TABLE_ROWS, MAX * 32);
    }
    if p.source_details {
        journal.reserve(DETAIL_ROWS, MAX * 16);
    }
    if p.source_cbv_snapshots {
        journal.reserve(CBV_SNAPSHOT_COMMANDS, MAX);
        journal.reserve(CBV_SNAPSHOT_ROWS, MAX * 16);
    }
    if p.source_ia_snapshots {
        journal.reserve(IA_COMMANDS, MAX);
        journal.reserve(IA_OBSERVATIONS, MAX * views);
        journal.reserve(IA_VIEWS, MAX * views);
    }
    true
}
const INDIRECT_ROWS: u32 = 25;
const CBV_DIAGNOSTICS: u32 = 29;
const TABLE_ROWS: u32 = 31;
const TABLE_COMMANDS: u32 = 32;
const CBV_SNAPSHOT_ROWS: u32 = 33;
const CBV_SNAPSHOT_COMMANDS: u32 = 34;
const IA_OBSERVATIONS: u32 = 40;
const IA_COMMANDS: u32 = 36;

// ---- the source

/// A captured indirect source (CapturedIndirectSource): the replayed operands (owned DEFAULT buffers), the receipt and
/// the capture's owner.
pub struct IndirectSnapshot {
    pub replay: PureIndirect,
    pub receipt: crate::geometry_commands::SnapshotReceipt,
    pub owner: Option<IUnknown>,
}
fn release(object: *mut c_void) {
    crate::structured_snapshot_receipt::release(object);
}
fn release_address(object: u64) {
    release(object as *mut c_void);
}
/// source_cbv::SourceCbvBacking's references (its resource, origin owner and three recordings' tokens).
pub(super) fn release_cbv_backing(b: &crate::source_cbv_snapshot::SourceCbvBacking) {
    release_address(b.resource);
    release_address(b.origin_owner);
    release_address(b.recording.token);
    release_address(b.creation_recording.token);
    release_address(b.writer_recording.token);
}
/// source_srv::SourceSrvBacking's references (its binding's resource, its recording's token).
fn release_srv_backing(b: &crate::source_srv_observe::Backing) {
    release(b.binding.resource);
    release(b.recording.token);
}
/// geometry::IndirectBinding's references (its arguments' resource, its signature, its recording's token).
fn release_binding(b: &crate::ia_binding::Binding) {
    release(b.arguments.resource);
    release(b.signature);
    release(b.recording.token);
}
impl SourceScalars {
    /// A source's C++ defaults (all-zero mirrors but the T5 row, the backing's cached slot index and the bindings'
    /// blend factor), its geometry's draw the source draw.
    fn new(draw: &DrawRecord) -> Box<SourceScalars> {
        /// SAFETY (each use): a raw mirror whose all-zero record is its C++ default.
        fn zero<T>() -> T {
            unsafe { core::mem::zeroed() }
        }
        let mut t5_native: crate::source_srv_observe::Backing = zero();
        t5_native.slot.cached_index = u32::MAX;
        Box::new(SourceScalars {
            indirect_proof: zero(),
            cbv_prepared: zero(),
            ia_prepared: zero(),
            t5_prepared: zero(),
            full_t5_prepared: zero(),
            multilayer_t5: zero(),
            material_rows: Vec::new(),
            material_owners: Vec::new(),
            parent: GraphParentRecord::default(),
            cbvs: Vec::new(),
            private_cbvs: Vec::new(),
            t5_native,
            t5_private: zero(),
            full_t5_private: zero(),
            t5_row: usize::MAX,
            t5_original_certified: false,
            material_buffers: Vec::new(),
            material_truncated: false,
            material_inputs_ready: false,
            ia_views: Vec::new(),
            bindings: cvr_gpu::scene_state::Bindings::default(),
            root: null_mut(),
            geometry: DrawResources::new(*draw),
            indirect: None,
            shader: None,
            held: Vec::new(),
            indirect_binding: zero(),
            indirect_snapshot: IndirectSnapshot { replay: PureIndirect::default(), receipt: Default::default(), owner: None },
        })
    }
    /// The source's shader snapshot: a pointer to its heap `std::shared_ptr<const Snapshot>` (a null one when none).
    pub fn shader_shared(&self) -> *const c_void {
        static NONE: [usize; 2] = [0, 0];
        self.shader.as_ref().map_or(NONE.as_ptr().cast(), |s| s.shared().cast_const())
    }
}
impl Drop for SourceScalars {
    fn drop(&mut self) {
        release_binding(&self.indirect_binding);
        for m in &self.material_buffers {
            release_srv_backing(&m.backing);
            release(m.copy.owner);
            release(m.copy.resource);
        }
        for r in [&self.full_t5_private, &self.t5_private] {
            release(r.resource);
            release(r.owner);
        }
        release_srv_backing(&self.t5_native);
        for c in &self.private_cbvs {
            release_address(c.resource);
            release_address(c.owner);
        }
        for b in &self.cbvs {
            release_cbv_backing(b);
        }
        for l in &self.material_owners {
            release(l.owner);
            release(l.source.resource);
        }
        release_address(self.multilayer_t5.resource);
        release(self.full_t5_prepared.owner);
        release(self.t5_prepared.owner);
        release(self.ia_prepared.owner);
        release_address(self.cbv_prepared.owner);
        release(self.indirect_proof.resource);
        release(self.indirect_proof.allocation_owner);
    }
}

// ---- AppendSource

/// preparation::RevalidateGraphParent (indirect_copy's parent check).
unsafe extern "C" fn parent_current(parent: *const GraphParentRecord) -> bool {
    // SAFETY: the proof's parent record.
    crate::preparation_trace::revalidate_graph_parent(unsafe { &*parent })
}
/// gpu::KeepCommandResources for one object.
unsafe extern "C-unwind" fn keep_one(list: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: a live object.
    unsafe { keep(list, &object, 1) }
}
/// The capture's validation context: the argument (and optional count) proofs.
struct Validation<'a> {
    proofs: [Option<&'a crate::indirect_copy_native::Proof>; 2],
}
/// indirect_copy::Validate on the proof `which`.
unsafe extern "C-unwind" fn validate_copy(context: *mut c_void, list: *mut c_void, which: u32, offset: u64, bytes: u64) -> bool {
    // SAFETY: the capture's own context.
    let value = unsafe { &*context.cast::<Validation>() };
    let Some(proof) = value.proofs.get(which as usize).copied().flatten() else { return false };
    // SAFETY: a live list and the proof.
    unsafe { crate::indirect_copy_native::cvr_ic_validate(list, proof, offset, bytes, parent_current) }
}
/// StereoGeometryCommands::CaptureIndirectSource with the arguments' proof (no count proof): the argument snapshot at
/// the actual call into `output` (unchanged on failure).
fn capture_indirect_source(list: *mut c_void, source: &PureIndirect, arguments: &crate::indirect_copy_native::Proof, output: &mut IndirectSnapshot) -> HRESULT {
    if list.is_null() || output.owner.is_some() || output.replay.signature.is_some() || output.replay.arguments.is_some() || output.replay.count_buffer.is_some() {
        return E_INVALIDARG;
    }
    let lent_source = lend_indirect(source);
    let lent_arguments = crate::geometry_commands::Proof {
        resource: arguments.resource,
        state_known: arguments.has_state,
        state: windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_STATES(if arguments.has_state { arguments.state as i32 } else { 0 }),
        recording_generation: arguments.recording_generation,
        command_position: arguments.command_position,
        allocation_owner: arguments.allocation_owner,
    };
    let context = Validation { proofs: [Some(arguments), None] };
    // SAFETY: zeroed out-values the capture fills.
    let mut captured: crate::geometry_commands::Captured = unsafe { core::mem::zeroed() };
    // SAFETY: the lent views and the context live for the call.
    let hr = unsafe {
        crate::geometry_commands::cvr_sgc_capture_indirect(
            list,
            &lent_source,
            &lent_arguments,
            core::ptr::null(),
            describe_signature,
            validate_copy,
            (&context as *const Validation).cast_mut().cast(),
            keep_one,
            &mut captured,
        )
    };
    if hr.is_err() {
        return hr;
    }
    // SAFETY: the capture's references handed over (or null).
    let take = |p: *mut c_void| (!p.is_null()).then(|| unsafe { IUnknown::from_raw(p) });
    *output = IndirectSnapshot {
        replay: PureIndirect {
            signature: source.signature.clone(),
            max_command_count: source.max_command_count,
            arguments: take(captured.arguments).and_then(|o| o.cast().ok()),
            argument_offset: 0,
            count_buffer: take(captured.count_buffer).and_then(|o| o.cast().ok()),
            count_offset: 0,
        },
        receipt: captured.receipt,
        owner: take(captured.owner),
    };
    S_OK
}
/// An optional interface's pointer (lent).
fn ptr<T: Interface>(object: &Option<T>) -> *mut c_void {
    object.as_ref().map_or(null_mut(), |o| o.as_raw())
}
/// PureIndirectSource lent (geometry_commands' view).
pub(super) fn lend_indirect(source: &PureIndirect) -> crate::geometry_commands::IndirectView {
    crate::geometry_commands::IndirectView {
        signature: ptr(&source.signature),
        max_command_count: source.max_command_count,
        arguments: ptr(&source.arguments),
        argument_offset: source.argument_offset,
        count_buffer: ptr(&source.count_buffer),
        count_offset: source.count_offset,
    }
}
/// indirect_copy::Failure::Ownership.
const INDIRECT_OWNERSHIP: u64 = 13;
/// geometry::IndirectBindingFailure::Recording.
const BINDING_RECORDING: u64 = 6;
/// AppendSource (formerly the .cpp's): the source recorded at its actual draw (`indirect`: a pure indirect draw's
/// operands, `replay` its snapshot's replayed operands when captured).
pub(super) fn append_source(list: *mut c_void, side: u32, draw: &DrawRecord, indirect: Option<&PureIndirect>, replay: Option<&mut PureIndirect>) {
    let Some(w) = work() else { return };
    if !w.source_stream {
        return;
    }
    if sources_len() >= MAX_SOURCE_COMMANDS {
        w.source_failed = true;
        ex!(CyberpunkVR_RetainedSourceStreamStats)[12] = 901;
        publish_sources();
        return;
    }
    let mut source = SourceScalars::new(draw);
    let mut row = [0u64; 48];
    let captured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture_source(list, side, draw, indirect, replay, &mut source, &mut row)));
    let w = work().expect("capturing");
    if matches!(captured, Ok(Ok(()))) {
        if row[44] != 0 {
            w.source_failed = true;
            let stats = ex!(CyberpunkVR_RetainedSourceStreamStats);
            if stats[12] == 0 {
                stats[12] = row[44];
            }
        }
        if let Some(owned) = owned_work() {
            owned.sources.push(source);
            owned.journal.append(SOURCE_ROWS, &row);
        }
        publish_sources();
        return;
    }
    // What the C++ catch did: the source dropped, the stream failed (and the CBV snapshots' arena aborted).
    w.source_failed = true;
    if w.source_cbv_snapshots {
        w.cbv_snapshots_failed = true;
        w.eye(side as usize).cbv_arena_aborted = true;
        cvr_nrg2_cbv_arena(side, 0);
        fail(960);
    }
    let stats = ex!(CyberpunkVR_RetainedSourceStreamStats);
    if stats[12] == 0 {
        stats[12] = 906;
    }
    publish_sources();
}
/// AppendSource's try block; Err: what it threw.
fn capture_source(list: *mut c_void, side: u32, draw: &DrawRecord, indirect: Option<&PureIndirect>, replay: Option<&mut PureIndirect>, source: &mut SourceScalars, row: &mut [u64; 48]) -> Result<(), ()> {
    let w = work().expect("capturing");
    let journal: *mut Journal = w.journal.cast();
    // SAFETY: the work's journal.
    let journal = unsafe { &mut *journal };
    let lease = raw_lease(&w.eye(side as usize).recording);
    let tag = crate::preparation_trace::current_draw_tag();
    let header = crate::retained_source_rows::SourceHeader {
        index: journal.count(SOURCE_ROWS) as u64,
        side: u64::from(side),
        frame: u64::from(w.frame),
        stage: u64::from(tag.stage),
        scope: tag.scope,
        list: list as u64,
        generation: lease.generation,
        context: lease.context,
        registry: lease.registry,
    };
    let view = indirect.map(lend_indirect);
    // SAFETY: local records (the indirect fields and the caller RVA as well).
    let kind = unsafe { crate::retained_source_rows::cvr_nrg_source_row(row, &header, draw, view.as_ref().map_or(core::ptr::null(), |v| v as *const _)) };
    let reject = |row: &mut [u64; 48], reason: u64| {
        if row[44] == 0 {
            row[44] = reason;
        }
    };
    // geometry::CopyCurrentBindings: the exact bound leases, given.
    let indexed = indirect.is_none() || kind == 1;
    if let Some((index, vertices)) = crate::geometry_leases::current(list as u64, draw, indexed, cvr_host::probe::internal()) {
        source.geometry.index = index.map(BufferLease::given).unwrap_or_default();
        source.geometry.vertices = vertices.map(|v| v.map(BufferLease::given).unwrap_or_default());
        row[43] |= 1;
        row[28] = source.geometry.index.raw() as u64;
    } else {
        reject(row, 902);
    }
    if let Some(indirect) = indirect {
        source.indirect = Some(indirect.clone());
        let mut evidence = [0u64; 56];
        // SAFETY: local rows.
        unsafe { crate::retained_source_rows::cvr_nrg_source_heads(row, &mut evidence, null_mut()) };
        let v = view.as_ref().expect("an indirect view");
        // SAFETY: an empty binding the call fills (its references handed over).
        let mut pending: crate::ia_binding::Binding = unsafe { core::mem::zeroed() };
        // SAFETY: the call's live objects, the eye's lease and the empty binding.
        let bound = unsafe {
            crate::ia_binding::cvr_iab_bound_indirect(list, v.signature, v.max_command_count, v.arguments, v.argument_offset, v.count_buffer, v.count_offset, &lease, cvr_host::probe::internal(), &mut pending)
        };
        if bound {
            source.indirect_binding = pending;
            let b = &source.indirect_binding;
            let native = &b.arguments.native;
            let resource = b.arguments.resource;
            // SAFETY: the binding's live argument buffer.
            let (gpu, width) = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }.map_or((0, 0), |r| unsafe { (r.GetGPUVirtualAddress(), r.GetDesc().Width) });
            evidence[2] = u64::from(b.caller_rva);
            evidence[3] = 1;
            evidence[4] = b.record_address;
            evidence[5] = u64::from(native.handle);
            evidence[6] = u64::from(native.kind);
            evidence[7] = u64::from(b.state.value);
            evidence[8] = u64::from(b.state.status);
            evidence[9] = u64::from(b.state.kind);
            evidence[10] = native.gpu;
            evidence[11] = native.bytes;
            evidence[12] = gpu;
            evidence[13] = width;
            evidence[15] = b.argument_bytes;
            evidence[16] = u64::from(b.arguments.heap_type);
            evidence[17] = u64::from(b.arguments.flags);
            evidence[18] = u64::from(b.signature_kind);
            evidence[19] = u64::from(b.signature_stride);
            evidence[24] = native.registry;
            evidence[26] = u64::from(b.full_backing);
            // SAFETY: the live list, the eye's lease and the saved binding.
            evidence[27] = u64::from(unsafe { crate::ia_binding::cvr_iab_revalidate_indirect(list, &lease, b, cvr_host::probe::internal()) });
            for (n, chunk) in b.raw_record.chunks_exact(8).enumerate() {
                evidence[32 + n] = u64::from_le_bytes(chunk.try_into().expect("eight bytes"));
            }
        } else {
            release_binding(&pending);
        }
        if evidence[3] == 0 || evidence[27] == 0 {
            evidence[28] = u64::from(crate::ia_binding::cvr_iab_last_failure());
            if evidence[28] == BINDING_RECORDING {
                evidence[29] = u64::from(crate::native_recording::cvr_native_recording_last_failure());
            }
        }
        journal.append(INDIRECT_ROWS, &evidence);
    }
    // scene_state::CaptureSourceBindings and CaptureDrawState.
    let pipeline = draw.pipeline as *mut c_void;
    // SAFETY: the live list; the source's bindings written only on success.
    let bound = unsafe { cvr_gpu::scene_state::cvr_scene_capture_source_bindings(list as usize, &mut source.bindings) };
    if bound && source.bindings.root as u64 == draw.root && capture_draw_state(list, pipeline, &mut source.geometry.state) {
        let b = &source.bindings;
        source.root = b.root as *mut c_void;
        let mut held: Vec<IUnknown> = Vec::new();
        let hold = |held: &mut Vec<IUnknown>, object: usize| {
            let object = object as *mut c_void;
            // SAFETY: the bindings' live objects (or null); a reference each.
            if let Some(o) = unsafe { IUnknown::from_raw_borrowed(&object) } {
                held.push(o.clone());
            }
        };
        hold(&mut held, b.root);
        hold(&mut held, b.compute_root);
        for i in 0..(b.heap_count as usize).min(2) {
            hold(&mut held, b.heaps[i]);
        }
        source.held = held;
        row[43] |= 2 | 16;
        row[13] = b.table_mask;
        row[14] = b.compute_table_mask;
        row[15] = u64::from(b.heap_count);
        row[16] = b.topology as u32 as u64;
        row[17] = u64::from(b.stencil);
        row[18] = b.rate.value as u32 as u64;
    } else {
        reject(row, 903);
    }
    if crate::preparation_trace::current_graph_parent(side as i32, w.frame, &mut source.parent) {
        let p = &source.parent;
        row[43] |= 4;
        row[36] = p.manager;
        row[37] = p.fingerprint;
        row[38] = u64::from(p.handle);
        row[39] = u64::from(p.ordinal);
        row[40] = u64::from(p.view_index);
        row[41] = u64::from(p.slot);
        row[42] = p.name;
    } else {
        reject(row, 904);
    }
    source.shader = crate::shader_snapshot_native::source(pipeline);
    match source.shader.as_ref().map(|s| s.info()) {
        Some(info) if info.mirror.root_len != 0 => {
            row[43] |= 8;
            row[12] = info.id;
        }
        _ => reject(row, 905),
    }
    let raw_source: *mut SourceScalars = source;
    // SAFETY (the captures below): the live list, this source and its row; the captures run under the lock.
    unsafe {
        // The family's CBVs at the source (the rows, the backings kept).
        if !cvr_nrg2_capture_source_cbvs(list, side, raw_source, row, draw, (*raw_source).root) {
            return Err(()); // std::bad_alloc
        }
        sources::cvr_nrg2_capture_sources(list, raw_source, row); // CaptureTables, CaptureSelectors
        if work().is_some_and(|w| w.source_tables) {
            cvr_nrg2_capture_materials(list, raw_source, row);
        }
        pairs::cvr_nrg2_capture_vrs(list, raw_source, row);
        if work().is_some_and(|w| w.source_cbv_snapshots) && !cvr_nrg2_capture_cbv(list, side, raw_source, row) {
            return Err(()); // "Private CBV source row is missing"
        }
        cvr_nrg2_capture_t5(list, side, raw_source, row, false);
        cvr_nrg2_capture_t5(list, side, raw_source, row, true);
        if !cvr_nrg2_capture_instance(list, side, raw_source, row) {
            return Err(()); // "Native instance capture failed"
        }
        cvr_nrg2_capture_ia(list, side, raw_source, row, indexed);
        cvr_nrg2_capture_borrowed(list, side, raw_source, row);
    }
    let w = work().expect("capturing");
    if let (false, Some(indirect), true) = (w.failed, indirect, w.source_snapshots) {
        let position = row[0] + 1;
        let mut snapshot = [0u64; 24];
        // SAFETY: local rows.
        unsafe { crate::retained_source_rows::cvr_nrg_source_heads(row, null_mut(), &mut snapshot) };
        let mut result = E_ACCESSDENIED;
        // indirect_copy::ActualCallScope: entered here, left when this block ends.
        // SAFETY: zeroed stamp the call fills.
        let mut stamp: crate::indirect_copy::ScopeStamp = unsafe { core::mem::zeroed() };
        // SAFETY: a writable stamp.
        unsafe { crate::indirect_copy_native::cvr_ic_scope_enter(list as u64, lease.generation, position, &mut stamp) };
        struct Scope(crate::indirect_copy::ScopeStamp);
        impl Drop for Scope {
            fn drop(&mut self) {
                // SAFETY: the scope's own stamp.
                unsafe { crate::indirect_copy_native::cvr_ic_scope_exit(&self.0) };
            }
        }
        let scope = Scope(stamp);
        // SAFETY: the scope's stamp.
        let active = unsafe { crate::indirect_copy_native::cvr_ic_scope_active(&scope.0) };
        if active
            && row[44] == 0
            && !source.indirect_binding.arguments.resource.is_null()
            // SAFETY: the live list, the source's binding and parent, an empty proof the call fills.
            && unsafe { crate::indirect_copy_native::cvr_ic_prepare(list, &source.indirect_binding, &source.parent, position, &mut source.indirect_proof, parent_current) }
        {
            let proof = &source.indirect_proof;
            snapshot[3] = 1;
            snapshot[14] = u64::from(proof.state);
            snapshot[15] = proof.resource as u64;
            snapshot[16] = proof.allocation_owner as u64;
            let mut captured = IndirectSnapshot { replay: PureIndirect::default(), receipt: Default::default(), owner: None };
            result = capture_indirect_source(list, indirect, &source.indirect_proof, &mut captured);
            if result.is_ok() {
                source.indirect_snapshot = captured;
            }
        }
        snapshot[4] = u64::from(result.0 as u32);
        if result.is_ok() {
            let s = &source.indirect_snapshot;
            source.indirect = Some(s.replay.clone());
            if let Some(replay) = replay {
                *replay = s.replay.clone();
            }
            snapshot[2] = 1;
            snapshot[11] = ptr(&s.replay.arguments) as u64;
            snapshot[12] = s.replay.argument_offset;
            snapshot[13] = s.receipt.copied_bytes[0];
            snapshot[17] = ptr(&s.replay.count_buffer) as u64;
            snapshot[18] = s.receipt.copied_bytes[1];
        } else {
            w.snapshot_failed = true;
            snapshot[5] = u64::from(crate::indirect_copy_native::cvr_ic_last_failure());
            if snapshot[5] == 0 {
                snapshot[5] = INDIRECT_OWNERSHIP;
            }
        }
        journal.append(SNAPSHOT_ROWS, &snapshot);
        drop(scope);
    }
    Ok(())
}
/// scene_state::CaptureDrawState: at the actual draw, the list's draw state with its PSO (referenced) and rate image.
fn capture_draw_state(list: *mut c_void, original: *mut c_void, state: &mut DrawState) -> bool {
    if list.is_null() || original.is_null() {
        return false;
    }
    let mut raw = cvr_gpu::scene_state::DrawStateRaw::default();
    // SAFETY: the live list; a local output (its image a reference handed over).
    if !unsafe { cvr_gpu::scene_state::cvr_scene_capture_draw_state(list as usize, &mut raw) } {
        return false;
    }
    *state = DrawState {
        // SAFETY: the draw's live pipeline, referenced for the state.
        original: unsafe { ID3D12PipelineState::from_raw_borrowed(&original) }.cloned(),
        // SAFETY: as above (the image's reference handed over).
        rate_image: (raw.rate_image != 0).then(|| unsafe { ID3D12Resource::from_raw(raw.rate_image as *mut c_void) }),
        rate: raw.rate,
        topology: raw.topology,
        stencil: raw.stencil,
        blend: raw.blend_factor,
        targets: Default::default(),
    };
    true
}
