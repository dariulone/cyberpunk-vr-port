//! The retained capture's prepared pair sources (formerly NativeRetainedGeometry.cpp's StagePairSource,
//! BuildStagedSources, ValidateStagedSources, StartCurrentSources, the pair request and the staged immutables, with
//! the eyes' pass and tail checkpoints and the sources' VRS captures they read). A child of
//! retained_geometry_native.rs (its work, journal, sources and side tables) next to retained_sources.rs.

use super::sources::*;
use super::*;
use crate::pair_source::{Checkpoint, InputCheck, Material, NativeExit, PairSource, Vrs};
use std::sync::Arc;
use windows::Win32::Graphics::Direct3D12::*;

const STAGED_ROWS: u32 = 11;
const MATERIAL_ROWS_KIND: u32 = 15;
const INSTANCE_GPU_ROWS_KIND: u32 = 20;
const IA_SELECTIONS_KIND: u32 = 22;
const IA_COMMANDS_KIND: u32 = 36;

fn raw_of<T: Interface>(value: &Option<T>) -> *mut c_void {
    value.as_ref().map_or(null_mut(), |v| v.as_raw())
}
fn device_raw(w: &WorkScalars) -> *mut c_void {
    raw_of(&w.device)
}
fn recording_copy(r: &Recording) -> crate::native_recording::Lease {
    crate::native_recording::Lease { token: raw_of(&r.token), generation: r.generation, context: r.context, list: r.list, registry: r.registry }
}

// ---- the eyes' checkpoints and the sources' VRS captures

/// The pass-depth checkpoint (formerly the host's PassDepth): 0, or 2 (Create) / 3 (Capture) as the .cpp's.
pub(super) fn pass_depth(w: &WorkScalars, list: *mut c_void, side: u32, targets: &Targets) -> u32 {
    // SAFETY: layout-equal targets (raw_targets).
    match Checkpoint::capture(device_raw(w), list, unsafe { &*raw_targets(targets) }) {
        Ok(checkpoint) => {
            eye_rust(side).pass_depth = Some(checkpoint);
            0
        }
        Err(code) => code,
    }
}
/// The tail's baseline checkpoint (the .cpp's tail keeps its handoff, access mark and target lifetimes).
///
/// # Safety
/// `targets` is the draw's scene_state::GeometryTargets.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_tail_checkpoint(list: *mut c_void, side: u32, targets: *const Targets) -> bool {
    let Some(w) = work() else { return false };
    // SAFETY: as documented (layout-equal targets).
    match Checkpoint::capture(device_raw(w), list, unsafe { &*raw_targets(&*targets) }) {
        Ok(checkpoint) => {
            eye_rust(side).tail_baseline = Some(checkpoint);
            true
        }
        Err(_) => false,
    }
}
/// Whether the eye's tail baseline was captured.
#[no_mangle]
pub extern "C-unwind" fn cvr_nrg2_tail_baseline(side: u32) -> bool {
    eye_rust(side).tail_baseline.is_some()
}
/// The source's VRS capture at AppendSource (its rate image, when the IA snapshots are on).
///
/// # Safety
/// `list` is the live recording list; `source` the .cpp's source being appended; `row` its source row.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg2_capture_vrs(list: *mut c_void, source: *mut SourceScalars, row: *const [u64; 48]) {
    let Some(w) = work() else { return };
    // SAFETY: as documented.
    let index = unsafe { (*row)[0] } as usize;
    let Some(cap) = rust_side().captures.get_mut(index) else { return };
    // SAFETY: the source being appended.
    let image = geometry_state(unsafe { &*source }).rate.image as *mut c_void;
    if w.source_ia_snapshots && !image.is_null() {
        cap.vrs = Vrs::capture(device_raw(w), list, image);
    }
}
/// The source's geometry state (its command's DrawResources' DrawState).
fn geometry_state(source: &'static SourceScalars) -> &'static crate::geometry_leases::DrawState {
    &source.geometry.state
}
fn source_indirect(source: &SourceScalars) -> bool {
    source.indirect.is_some()
}

// ---- StagePairSource

fn private_cbv_rows(source: &SourceScalars) -> Vec<crate::prepared_pair_native::PrivateCbv> {
    source
        .private_cbvs
        .as_slice()
        .iter()
        // SAFETY: the same 72-byte PrivateCbv record (its references held by the pair source).
        .map(|c| unsafe { core::ptr::read((c as *const crate::source_cbv_snapshot::PrivateCbv).cast()) })
        .collect()
}
/// StagePairSource (formerly the .cpp's): the source's completed copies as a prepared pair source; 0 or its refusal
/// (1 inputs, 2 eye, 3 instances, 4 materials, 5 shape, 6 depth or rate, 7 full T5).
fn stage_pair_source(w: &mut WorkScalars, state: &WorkSources, ordinal: usize, cap: &SourceCapture) -> Result<PairSource, u32> {
    let saved = rows_mut::<48>(w, SOURCE_ROWS)[ordinal];
    let source = super::source(ordinal as u64).ok_or(1u32)?;
    let guarded = cap.guarded_plan.as_ref();
    let tables = state.guarded_tables.as_ref().and_then(|t| t.get(ordinal));
    if saved[1] > 1 || source_indirect(source) || cap.shader.is_none() || cap.pass_depth.is_none() || source.material_truncated || tables.is_none_or(|t| t.is_empty()) || guarded.is_none_or(|g| g.checks.is_empty()) {
        return Err(1);
    }
    let (guarded, tables) = (guarded.expect("checked"), tables.expect("checked"));
    let side = saved[1] as u32;
    let eye = w.eye(side as usize);
    let ia_commands = rows_mut::<16>(w, IA_COMMANDS_KIND);
    let selections = rows_mut::<5>(w, IA_SELECTIONS_KIND);
    if !eye.sealed || !eye.submission_certified || eye.source_queue.is_none() || !eye.ia_validated || ordinal >= ia_commands.len() || ordinal >= selections.len() {
        return Err(2);
    }
    let (copied, selection) = (ia_commands[ordinal], selections[ordinal]);
    if copied[6] == 0 || copied[14] == 0 || copied[7] != 0 || source.ia_prepared.owner.is_null() {
        return Err(2);
    }
    let mut owned = copied_geometry(source);
    // SAFETY: the proof's recorded draw, alive with the proof's owner.
    let mut original = crate::ia_snapshot_receipt::copied_shape_owned(&source.ia_prepared).map(|d| unsafe { (*d).clone() }).ok_or(5u32)?;
    let mut owners: Vec<IUnknown> = Vec::new();
    if selection[2] == 128 {
        let gpu_rows = rows_mut::<24>(w, INSTANCE_GPU_ROWS_KIND);
        if ordinal >= gpu_rows.len() || gpu_rows[ordinal][3] == 0 {
            return Err(3);
        }
        // SAFETY: a zeroed out-view the call fills.
        let mut view: InstanceViewOut = unsafe { core::mem::zeroed() };
        // SAFETY: the work's own call, under the capture's lock.
        if !unsafe { cvr_nrg2_instance_view(ordinal, 1, side, saved[6] as *mut c_void, &mut view) } {
            return Err(3);
        }
        let draw = D3D12_DRAW_INDEXED_ARGUMENTS {
            IndexCountPerInstance: view.draw.index_count,
            InstanceCount: view.draw.instance_count,
            StartIndexLocation: view.draw.start_index,
            BaseVertexLocation: view.draw.base_vertex,
            StartInstanceLocation: view.draw.start_instance,
        };
        if !crate::retained_staged::instance_view(&view.scope, &draw, &saved, ordinal as u64) {
            return Err(5);
        }
        // SAFETY: the view's live private resource.
        let resource = unsafe { ID3D12Resource::from_raw_borrowed(&view.resource) }.cloned().ok_or(3u32)?;
        // SAFETY: plain queries of a live resource.
        let (desc, gpu) = unsafe { (resource.GetDesc(), resource.GetGPUVirtualAddress()) };
        owned.vertices[7] = crate::geometry_leases::BufferLease {
            resource: Some(resource),
            gpu,
            bytes: desc.Width,
            heap_type: D3D12_HEAP_TYPE_DEFAULT.0 as u32,
            flags: desc.Flags.0 as u32,
            native: Default::default(),
        };
        owned.draw.vertices[7] = d3d_view(&view.vertex);
        owners.extend(crate::pair_source::held(view.owner));
    } else if selection[2] != 0 {
        return Err(3);
    }
    owned.instances.clear();
    original.instances.clear();
    // SAFETY: the same 48-byte PreparedCopies record.
    let cbv_receipt: crate::prepared_pair_native::PreparedCopies = unsafe { core::ptr::read((&raw const source.cbv_prepared).cast()) };
    let t5 = guarded.t5.map(|t| crate::prepared_pair_native::T5View { resource: t.resource as *mut c_void, owner: t.owner as *mut c_void, srv: t.srv, offset: t.offset, bytes: t.bytes });
    let mut full_t5 = None;
    if w.full_t5_enabled && saved[4] == 9 {
        // SAFETY: the same 72- and 40-byte records.
        let copy: crate::prepared_pair_native::PreparedCopy = unsafe { core::ptr::read((&raw const source.full_t5_prepared).cast()) };
        let range: crate::prepared_pair_native::PrivateRange = unsafe { core::ptr::read((&raw const source.full_t5_private).cast()) };
        let mut view = D3D12_SHADER_RESOURCE_VIEW_DESC::default();
        // SAFETY: the source's copy and range; a local view.
        let recorded = unsafe { crate::structured_snapshot_receipt::cvr_sbsn_recorded_view((&raw const source.full_t5_prepared).cast(), (&raw const source.full_t5_private).cast(), &mut view) };
        if !arena_op(side, 2, 1) || copy.owner.is_null() || !recorded {
            return Err(7);
        }
        owners.extend(crate::pair_source::held(copy.owner));
        owners.extend(crate::pair_source::held(range.owner));
        full_t5 = Some(crate::prepared_pair_native::FullT5 { copy, range, view });
    }
    let input_checks: Vec<InputCheck> = guarded
        .checks
        .iter()
        .map(|c| InputCheck { resource: c.resource as *mut c_void, owner: c.owner as *mut c_void, offset: c.offset, expected: c.expected.clone() })
        .collect();
    owners.extend(crate::pair_source::held(source.ia_prepared.owner));
    owners.extend(crate::pair_source::held(source.cbv_prepared.owner as *mut c_void));
    if let Some(t) = &t5 {
        owners.extend(crate::pair_source::held(t.owner));
    }
    let (rate_image, rate_capture) = match &cap.vrs {
        // SAFETY: the snapshot's frozen image (live with it), referenced for the source.
        Some(vrs) => (unsafe { ID3D12Resource::from_raw_borrowed(&vrs.source_image()) }.cloned(), Some(vrs.clone())),
        None => (None, None),
    };
    let baseline = cap.pass_depth.clone().expect("checked");
    let pass_depth = if baseline.captured() { baseline.resource(4) } else { None };
    if pass_depth.is_none() || (owned.state.rate.image != 0) != rate_image.is_some() {
        return Err(6);
    }
    let rows = rows_mut::<24>(w, MATERIAL_ROWS_KIND);
    let material_rows = source.material_rows.as_slice();
    let material_owners = source.material_owners.as_slice();
    let queue = raw_of(&eye.source_queue);
    let recording = recording_copy(&eye.recording);
    let mut materials = Vec::new();
    let mut dependency = 0usize;
    for group in dependencies_of(ordinal) {
        for item in group {
            if dependency >= material_owners.len() || dependency >= material_rows.len() {
                return Err(4);
            }
            let row = rows.get(material_rows[dependency]).copied().ok_or(4u32)?;
            // SAFETY: the same 56-byte ImmutableLease record (its references held by the material).
            let immutable: crate::texture_immutable::ImmutableLease = unsafe { core::ptr::read(&material_owners[dependency]) };
            dependency += 1;
            let mut buffer = None;
            if row[8] == 1 {
                if row[17] == 0 || item.resource != 0 || item.counter != 0 {
                    return Err(4);
                }
            } else if row[8] == 5 {
                // SAFETY: the lease and the eye's queue.
                if row[17] == 0 || immutable.owner.is_null() || !unsafe { crate::texture_immutable::cvr_tr_validate_immutable(&immutable, queue) } {
                    return Err(4);
                }
            } else if row[8] == 2 && item.parameter == 23 && item.offset == 5 {
                if t5.is_none() || !source.t5_original_certified || item.resource != source.multilayer_t5.resource {
                    return Err(4);
                }
            } else if row[8] == 2 && item.parameter == 23 && (item.offset == 4 || item.offset == 6) {
                let found = source.material_buffers.as_slice().iter().find(|b| b.backing.shader_register == item.offset).ok_or(4u32)?;
                // SAFETY: the copy, the binding, the eye's recording and queue (the same records).
                if !unsafe { crate::material_snapshot_receipt::cvr_mbsn_matches((&raw const found.copy).cast(), (item as *const crate::material_buffer_snapshot::SourceResourceBinding).cast(), true, (&raw const recording).cast(), queue) } {
                    return Err(4);
                }
                // SAFETY: the same 80-byte Copy record.
                buffer = Some(unsafe { core::ptr::read(&found.copy) });
            } else {
                return Err(4);
            }
            materials.push(Material::new(*item, immutable, buffer));
        }
    }
    if materials.len() != material_rows.len() {
        return Err(4);
    }
    let mut value = PairSource {
        ordinal: ordinal as u64,
        recording,
        queue: eye.source_queue.clone(),
        original,
        owned,
        root: source.root,
        shader: cap.shader.clone(),
        cbvs: private_cbv_rows(source),
        cbv_receipt,
        tables: tables.clone(),
        reference_tables: Vec::new(),
        materials,
        input_checks,
        t5,
        full_t5,
        rate_image,
        pass_baseline: Some(baseline),
        native_exit: None,
        allow_vrs_refinement: false,
        pass_depth,
        rate_capture,
        owners,
        held: Vec::new(),
    };
    value.hold_rows();
    Ok(value)
}

// ---- BuildStagedSources, ValidateStagedSources

fn publish_staged(w: &mut WorkScalars) {
    let rows = rows_mut::<8>(w, STAGED_ROWS);
    *ex!(CyberpunkVR_RetainedStagedRows) = rows.as_ptr() as u64;
    *ex!(CyberpunkVR_RetainedStagedCount) = rows.len() as u32;
}
/// BuildStagedSources: each source's prepared pair source staged after the guarded pass, a row per source.
pub(super) fn build_staged_sources(w: &mut WorkScalars) {
    let stats = ex!(CyberpunkVR_RetainedStagedStats);
    if w.failed || stats[0] != 0 || ex!(CyberpunkVR_RetainedGuardedStats)[0] != 2 {
        return;
    }
    let count = source_count();
    stats[0] = 1;
    stats[1] = u64::from(w.frame);
    stats[2] = count as u64;
    stats[7] = u64::from(w.payloads_processed);
    stats[8] = u64::from(w.cbv_payload_complete);
    stats[9] = u64::from(w.overlay_stage);
    if stats[7] != 0 || stats[8] != 0 || stats[9] != 0 {
        stats[0] = 3;
        fail(2420);
        return;
    }
    journal_of(w).reserve(STAGED_ROWS, count);
    let r = rust_side();
    for ordinal in 0..count {
        let saved = rows_mut::<48>(w, SOURCE_ROWS)[ordinal];
        let staged = match r.captures.get(ordinal) {
            Some(cap) => stage_pair_source(w, &r.sources_state, ordinal, cap),
            None => Err(1),
        };
        let reason = staged.as_ref().err().copied().unwrap_or(0);
        let materials = staged.as_ref().map_or(0, |s| s.materials.len() as u64);
        journal_of(w).append(STAGED_ROWS, &[ordinal as u64, saved[1], u64::from(reason == 0), u64::from(reason), saved[6], saved[7], ordinal as u64 + 1, materials]);
        match staged {
            Err(_) => stats[4] += 1,
            Ok(value) => {
                stats[3] += 1;
                stats[5] += value.materials.len() as u64;
                stats[6] += value.input_checks.len() as u64;
                r.captures[ordinal].staged = Some(value);
            }
        }
    }
    stats[0] = 2;
    publish_staged(w);
}
/// ValidateStagedSources (Poll's step 5): every completed source candidate staged, each staged source's geometry the
/// independently completed private geometry, its material buffers complete; false: the work fails 2421.
pub(super) fn validate_staged_sources() -> bool {
    let r = rust_side();
    for index in 0..source_count() {
        let Some(source) = super::source(index as u64) else { return false };
        let Some(cap) = r.captures.get(index) else { return false };
        let ready = cvr_nrg2_private_geometry(index);
        let bound = r.sources_state.binding_tables_of(index).is_some_and(|t| !t.is_empty());
        let guarded = r.sources_state.guarded_tables.as_ref().and_then(|t| t.get(index)).is_some_and(|t| !t.is_empty())
            && cap.guarded_plan.as_ref().is_some_and(|g| !g.checks.is_empty());
        let completed = !ready.is_null() && source.material_inputs_ready && !source_indirect(source) && cap.shader.is_some() && bound && cap.pass_depth.is_some() && guarded;
        if completed && cap.staged.is_none() {
            return false;
        }
        let Some(staged) = cap.staged.as_ref() else { continue };
        ex!(CyberpunkVR_RetainedStagedStats)[10] += 1;
        if ready.is_null() || !source.material_inputs_ready || !bound {
            return false;
        }
        // SAFETY: the private geometry's view (alive with the work).
        let ready = unsafe { &*ready };
        let a = &staged.owned;
        // SAFETY: the records (the leases in geometry::BufferLease's layout).
        if !unsafe { crate::retained_staged::cvr_nrg_same_staged(&a.draw, ready.draw, (&raw const a.index).cast(), ready.index.cast(), a.vertices.as_ptr().cast(), ready.vertices.cast()) } {
            return false;
        }
        for m in &staged.materials {
            if let Some(buffer) = &m.buffer {
                // SAFETY: the copy and the binding (the same records).
                if !unsafe { crate::material_snapshot_receipt::cvr_mbsn_matches((buffer as *const crate::material_snapshot_receipt::Copy).cast(), (&raw const m.binding).cast(), false, core::ptr::null(), null_mut()) } {
                    return false;
                }
            }
        }
        ex!(CyberpunkVR_RetainedStagedStats)[11] += 1;
    }
    true
}
/// The staged source's immutable materials (ObserveStagedResidency's).
pub(super) fn staged_immutables(ordinal: usize) -> Vec<*const crate::texture_immutable::ImmutableLease> {
    state()
        .rust
        .as_ref()
        .and_then(|r| r.captures.get(ordinal))
        .and_then(|c| c.staged.as_ref())
        .map(|s| s.materials.iter().filter(|m| !m.immutable.owner.is_null()).map(|m| &m.immutable as *const _).collect())
        .unwrap_or_default()
}

// ---- StartCurrentSources, the pair request

fn current_failure(w: &WorkScalars) {
    ex!(CyberpunkVR_RetainedCurrent)[15] = 2430;
    fail(2430);
    crate::prepared_pair_native::cvr_npp2_cancel_current(w.frame);
}
/// StartCurrentSources: the staged sources as the current prepared sequence on the queue of their submission (the
/// native tail's sources with its baseline and exit).
pub(super) fn start_current_sources(w: &mut WorkScalars, queue: *mut c_void) {
    if !w.current_enabled || w.current_started || w.failed || ex!(CyberpunkVR_RetainedStagedStats)[0] != 2 {
        return;
    }
    let stats = ex!(CyberpunkVR_RetainedCurrent);
    let (e0, e1) = (w.eye(0), w.eye(1));
    stats[1] = u64::from(w.frame);
    stats[2] = ex!(CyberpunkVR_RetainedStagedStats)[3];
    stats[3] = u64::from(!e0.submitted || !e1.submitted);
    stats[4] = u64::from(e0.submitted);
    stats[5] = u64::from(e1.submitted);
    stats[6] = u64::from(w.payloads_processed);
    stats[7] = u64::from(w.cbv_payload_complete);
    stats[8] = u64::from(w.overlay_stage);
    stats[12] = queue as u64;
    if queue.is_null() || stats[3] == 0 || stats[6] != 0 || stats[7] != 0 || stats[8] != 0 {
        return current_failure(w);
    }
    if [e0, e1].iter().any(|e| !e.sealed || !e.submission_certified || raw_of(&e.source_queue) != queue) {
        return current_failure(w);
    }
    let count = source_count();
    let mut exits: [Option<Arc<NativeExit>>; 2] = [None, None];
    if w.tail_enabled {
        stats[2] = 0;
        if w.source_failed || count != rows_mut::<48>(w, SOURCE_ROWS).len() {
            return current_failure(w);
        }
        for side in 0..2u32 {
            let eye = w.eye(side as usize);
            let tail = &mut ex!(CyberpunkVR_RetainedTail)[side as usize * 10..side as usize * 10 + 10];
            tail[9] = 1;
            let baseline = eye_rust(side).tail_baseline.clone();
            if baseline.as_ref().is_none_or(|b| !b.captured()) || eye.tail_first == u64::MAX || eye.readback.is_none() || eye.targets.colors != 3 || (eye.seal_reason != 0 && eye.seal_reason != 11) {
                return current_failure(w);
            }
            tail[4] = 0;
            for index in 0..count {
                let saved = rows_mut::<48>(w, SOURCE_ROWS)[index];
                if saved[1] != u64::from(side) || (index as u64) < eye.tail_first {
                    continue;
                }
                let staged = rust_side().captures.get(index).is_some_and(|c| c.staged.is_some());
                let indirect = super::source(index as u64).is_none_or(|s| source_indirect(s));
                if saved[2] != 0 || saved[4] != 9 || saved[44] != 0 || !staged || indirect {
                    return current_failure(w);
                }
                if tail[4] == 0 {
                    tail[2] = index as u64;
                }
                tail[3] = index as u64;
                tail[4] += 1;
            }
            if tail[4] == 0 || tail[2] != eye.tail_first {
                return current_failure(w);
            }
            // SAFETY: the eye's live depth target.
            let desc = eye.targets.resources[4].as_ref().map(|r| unsafe { r.GetDesc() }).unwrap_or_default();
            let planes = eye.planes[..eye.plane_count as usize]
                .iter()
                .map(|p| crate::native_exit::NativeExitPlane { slot: p.slot, plane: p.plane, rows: p.rows, stride: p.stride, component: p.component, bytes: p.bytes, layout: p.layouts[0] })
                .collect();
            exits[side as usize] = Some(Arc::new(NativeExit {
                readback: eye.readback.clone(),
                width: desc.Width as u32,
                height: desc.Height,
                eye: eye_index(side),
                source_side: side,
                frame: w.frame,
                list: eye.recording.list,
                generation: eye.recording.generation,
                planes,
            }));
            tail[8] = 1;
            tail[9] = 0;
            stats[2] += tail[4];
        }
    }
    let mut sources = Vec::with_capacity(stats[2] as usize);
    for cap in &rust_side().captures[..count.min(rust_side().captures.len())] {
        let Some(staged) = cap.staged.as_ref() else { continue };
        let mut value = staged.clone();
        if w.tail_enabled {
            let side = value.owned.draw.side;
            let eye = w.eye(side as usize);
            if value.ordinal < eye.tail_first {
                continue;
            }
            let baseline = eye_rust(side).tail_baseline.clone();
            value.pass_depth = baseline.as_ref().and_then(|b| b.resource(4));
            value.pass_baseline = baseline;
            value.native_exit = exits[side as usize].clone();
            value.allow_vrs_refinement = w.allow_vrs_refinement;
        }
        value.reference_tables = value.tables.clone();
        sources.push(value);
    }
    if sources.len() as u64 != stats[2] || !crate::prepared_pair_native::start_current_rust(device_raw(w), sources, queue, w.frame) {
        return current_failure(w);
    }
    w.current_started = true;
}
/// The pair request (formerly the host's): the completed staged sources with their assembled tables as references;
/// request 2 their whole sequence, else the requested compatible pair of a single family.
pub(super) fn pair_request(request: u32) {
    let (mut started, mut selected, mut selection) = (false, false, 0u32);
    crate::prepared_pair_state::cvr_npp_reset_request();
    let candidates_stats = ex!(CyberpunkVR_PreparedPairCandidates);
    if let Some(w) = work().filter(|w| w.complete && !w.failed) {
        let count = source_count();
        candidates_stats[0] = count as u64;
        let r = rust_side();
        let mut candidates = Vec::new();
        for ordinal in 0..count {
            let Some(source) = super::source(ordinal as u64) else { continue };
            let bound = r.sources_state.binding_tables_of(ordinal).filter(|t| !t.is_empty());
            let Some(staged) = r.captures.get(ordinal).and_then(|c| c.staged.as_ref()) else { continue };
            let Some(bound) = bound.filter(|_| !cvr_nrg2_private_geometry(ordinal).is_null() && source.material_inputs_ready) else { continue };
            candidates_stats[1] += 1;
            let mut value = staged.clone();
            candidates_stats[2] += 1;
            value.reference_tables = bound.clone();
            candidates.push(value);
        }
        let device = device_raw(w);
        if request == 2 {
            selected = true;
            started = crate::prepared_pair_native::start_rust(device, candidates, true, null_mut());
            selection = 1;
        } else {
            let requested = CyberpunkVR_PreparedPairSelection.load(SeqCst);
            'pairs: for left in candidates.iter().filter(|c| c.owned.draw.side == 1) {
                for right in candidates.iter().filter(|c| c.owned.draw.side == 0) {
                    let why = crate::prepared_pair_native::compatibility_rust(left, right);
                    if why != 0 {
                        candidates_stats[if why >= 10 { 4 } else { 3 + why as usize }] += 1;
                        continue;
                    }
                    candidates_stats[3] += 1;
                    let pipeline = raw_of(&left.owned.state.original);
                    let Some(family) = crate::families_native::lookup_raw(pipeline) else {
                        candidates_stats[13] += 1;
                        continue;
                    };
                    if !family.instance_rebase_safe {
                        candidates_stats[14] += 1;
                        continue;
                    }
                    if !family.pass.depth_state_known {
                        candidates_stats[15] += 1;
                        continue;
                    }
                    let current = selection;
                    selection += 1;
                    if current != requested {
                        continue;
                    }
                    selected = true;
                    if crate::prepared_pair_native::start_rust(device, vec![left.clone(), right.clone()], false, pipeline) {
                        started = true;
                        break 'pairs;
                    }
                }
            }
        }
    }
    CyberpunkVR_PreparedPairSelectionCount.store(selection, SeqCst);
    if !started {
        *ex!(CyberpunkVR_PreparedPairStats) = [0; 16];
        ex!(CyberpunkVR_PreparedPairStats)[10] = if selected { 27 } else { 24 };
        CyberpunkVR_PreparedPairState.store(6, SeqCst);
    }
}

// ---- the tail's targets and the infix boundary (formerly the host's Tail and LifetimeValidate, and CurrentBoundary)

const TARGET_SLOTS: [u32; 4] = [0, 1, 2, 4];
fn target_state(slot: u32) -> u32 {
    if slot == 4 {
        D3D12_RESOURCE_STATE_DEPTH_WRITE.0 as u32
    } else {
        D3D12_RESOURCE_STATE_RENDER_TARGET.0 as u32
    }
}
/// The tail's baseline (formerly the host's Tail): the colour and depth targets writable and within the budget, the
/// handoff begun, the access marked, each target's identity and lifetime captured, the baseline checkpoint.
pub(super) fn tail(w: &mut WorkScalars, list: *mut c_void, side: u32, targets: &Targets) -> bool {
    if eye_rust(side).tail_baseline.is_some() {
        return true;
    }
    let value = w.eye(side as usize);
    let tail = &mut ex!(CyberpunkVR_RetainedTail)[side as usize * 10..side as usize * 10 + 10];
    tail[1] = u64::from(w.frame);
    tail[2] = source_count() as u64;
    tail[7] = 9;
    const BUDGET: u64 = 64 * 1024 * 1024;
    if targets.colors != 3 {
        tail[9] = 2;
        fail(2462);
        value.sealed = true;
        return false;
    }
    let Some(device) = w.device.clone() else { return false };
    for slot in TARGET_SLOTS {
        let resource = raw_of(&targets.resources[slot as usize]);
        // SAFETY: the live list and target.
        if !unsafe { crate::texture_writable::cvr_tr_validate_writable_target(list, resource, target_state(slot)) } {
            tail[9] = 3;
            fail(2463);
            value.sealed = true;
            return false;
        }
        let Some(r) = targets.resources[slot as usize].as_ref() else { return false };
        // SAFETY: plain queries of the live target and device.
        let mut desc = unsafe { r.GetDesc() };
        desc.Alignment = 0;
        desc.Flags = if slot == 4 { D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL } else { D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET };
        // SAFETY: as above.
        let allocation = unsafe { device.GetResourceAllocationInfo(0, &[desc]) };
        if allocation.SizeInBytes == 0 || allocation.SizeInBytes > BUDGET - tail[6] {
            tail[9] = 4;
            fail(2464);
            value.sealed = true;
            return false;
        }
        tail[6] += allocation.SizeInBytes;
    }
    let lease = recording_copy(&value.recording);
    crate::targets::handoff_begin(list, &lease, w.frame, side, raw_targets(targets).cast());
    crate::targets::mark_source(list, &lease, w.frame, side);
    for (index, slot) in TARGET_SLOTS.into_iter().enumerate() {
        let row = &mut ex!(CyberpunkVR_RetainedTargetLifetime)[(side as usize * 4 + index) * 16..(side as usize * 4 + index) * 16 + 16];
        row[0] = u64::from(side);
        row[1] = u64::from(slot);
        match crate::targets::Identity::capture(list, raw_of(&targets.resources[slot as usize]), target_state(slot)) {
            Some(identity) => {
                let lease = crate::targets::LifetimeLease::capture(&identity);
                row[2] = u64::from(lease.is_some());
                eye_rust(side).target_lifetimes[index] = lease.unwrap_or_default();
                if row[2] == 0 {
                    row[5] = u64::from(crate::targets::lifetime_failure());
                }
                let d = crate::targets::lifetime_details();
                row[6..16].copy_from_slice(&[d[0], d[1], d[6], d[7], d[3], d[4], d[5], d[8], d[9], d[11]]);
            }
            None => row[5] = 100 + u64::from(crate::texture_lease::cvr_tr_failure()),
        }
    }
    // SAFETY: as cvr_nrg2_tail_checkpoint's.
    if !unsafe { cvr_nrg2_tail_checkpoint(list, side, targets) } {
        tail[9] = 5;
        fail(2465);
        value.sealed = true;
        return false;
    }
    value.tail_first = source_count() as u64;
    tail[5] = 1;
    true
}
/// The eye's borrowed target `index` still valid (formerly the host's LifetimeValidate): false with its failure.
pub(super) fn lifetime_validate(side: u32, index: u32, failure: &mut u32, details: &mut [u64; 2]) -> bool {
    if eye_rust(side).borrowed_targets[index as usize].validate() {
        return true;
    }
    *failure = crate::targets::lifetime_failure();
    let d = crate::targets::lifetime_details();
    *details = [d[0], d[1]];
    false
}
fn infix_failure(w: Option<&mut WorkScalars>) -> u32 {
    let stats = ex!(CyberpunkVR_RetainedInfix);
    if stats[15] == 0 {
        stats[15] = 2450;
    }
    fail(stats[15] as u32);
    crate::target_handoff_observer::cvr_th2_finish_observation();
    crate::target_access_observer::events::cvr_ta2_finish();
    if let Some(w) = w {
        crate::prepared_pair_native::cvr_npp2_cancel_current(w.frame);
        w.current_started = false;
    }
    0
}
/// CurrentBoundary: the native suffix's boundary in this submission (both pending eyes' sources of one graph producer
/// each), the targets' access receipt and borrowed lifetimes, the native export prepared; the suffix's first index.
pub fn current_boundary(queue: *mut c_void, count: u32, lists: *const *mut c_void) -> u32 {
    let _guard = lock();
    let Some(w) = work() else { return 0 };
    if !w.infix_enabled || !w.current_started || w.current_submitted || w.failed || ex!(CyberpunkVR_RetainedInfix)[2] != 0 {
        return 0;
    }
    let stats = ex!(CyberpunkVR_RetainedInfix);
    stats[1] = u64::from(w.frame);
    stats[2] += 1;
    stats[6] = u64::from(count);
    let mut sources: [crate::submission_frontier::BoundarySource; 2] = unsafe { core::mem::zeroed() };
    let leases: [crate::native_recording::Lease; 2] = [recording_copy(&w.eye(0).recording), recording_copy(&w.eye(1).recording)];
    for side in 0..2u32 {
        let eye = w.eye(side as usize);
        if eye.submitted || !eye.sealed || !eye.submission_certified || raw_of(&eye.source_queue) != queue {
            return infix_failure(Some(w));
        }
        let mut first: Option<GraphParentRecord> = None;
        for index in 0..source_count() {
            let Some(source) = super::source(index as u64) else { continue };
            if geometry_of(source).prefix.draw.side != side {
                continue;
            }
            let parent = source.parent;
            if parent.status != 0 {
                return infix_failure(Some(w));
            }
            match &first {
                None => first = Some(parent),
                Some(p) => {
                    if parent.fingerprint != p.fingerprint || parent.handle != p.handle || parent.view_index != side || parent.slot != p.slot || parent.object != p.object {
                        return infix_failure(Some(w));
                    }
                }
            }
        }
        let Some(parent) = first else { return infix_failure(Some(w)) };
        let l = &leases[side as usize];
        sources[side as usize] = crate::submission_frontier::BoundarySource {
            list: l.list,
            context: l.context,
            generation: l.generation,
            token: l.token as u64,
            registry: l.registry,
            fingerprint: parent.fingerprint,
            work: parent.work,
            name: parent.name,
            slot: parent.slot,
            node: parent.handle,
            view: parent.view_index,
            status: parent.status,
        };
    }
    let mut boundary = crate::target_access::Boundary::default();
    let lease_ptrs: [*const crate::native_recording::Lease; 2] = [&leases[0], &leases[1]];
    // SAFETY: the queue, the lists and the local records.
    if !unsafe { crate::submission_frontier_owner::cvr_sfo_find_boundary(queue as u64, count, lists as u64, (&raw const sources).cast(), &lease_ptrs, cvr_host::probe::internal(), (&raw mut boundary).cast()) } {
        stats[15] = 2500 + u64::from(crate::submission_frontier_owner::cvr_sfo_last_boundary_failure());
        return infix_failure(Some(w));
    }
    let snapshots = crate::targets::snapshot_sources(queue, w.frame);
    let mut proof = null_mut();
    // SAFETY: the queue, its lists and a local output (the proof's reference handed over).
    let scope_valid = unsafe { crate::queue_residency::cvr_qr_capture(queue, count, lists as u64, &mut proof) } && !proof.is_null();
    // SAFETY: the new proof's one reference.
    let scope = (!proof.is_null()).then(|| unsafe { IUnknown::from_raw(proof) });
    // SAFETY: the scope's proof (or null).
    unsafe { cvr_nrg2_borrowed_boundary(scope.as_ref().map_or(null_mut(), |s| s.as_raw())) };
    let mut receipt = None;
    if let Some(snapshots) = &snapshots {
        match (&scope, scope_valid) {
            (Some(scope), true) => receipt = crate::targets::query_receipt(queue, snapshots, &boundary, scope),
            _ => {
                crate::targets::query(queue, snapshots, &boundary);
            }
        }
    }
    for side in 0..2u32 {
        for i in 0..4usize {
            let row = &mut ex!(CyberpunkVR_RetainedTargetLifetime)[(side as usize * 4 + i) * 16..(side as usize * 4 + i) * 16 + 16];
            if row[2] == 0 {
                continue;
            }
            if !scope_valid {
                row[5] = 400 + u64::from(crate::queue_residency::cvr_qr_last_failure());
                continue;
            }
            let bound = crate::targets::BorrowedScope::bind(&eye_rust(side).target_lifetimes[i], queue, scope.as_ref());
            row[3] = u64::from(bound.is_some());
            eye_rust(side).borrowed_targets[i] = bound.unwrap_or_default();
            if row[3] == 0 {
                row[5] = 200 + u64::from(crate::targets::lifetime_failure());
                let d = crate::targets::lifetime_details();
                row[6] = d[0];
                row[7] = d[1];
            }
        }
    }
    if crate::prepared_pair_native::cvr_npp2_wants_native_export(w.frame) {
        let lifetimes: [crate::targets::BorrowedScope; 8] = core::array::from_fn(|k| eye_rust((k / 4) as u32).borrowed_targets[k % 4].clone());
        crate::prepared_pair_native::prepare_native_export_rust(queue, w.frame, receipt, lifetimes);
    }
    stats[3] = u64::from(boundary.after_index);
    stats[4] = u64::from(boundary.source_indices[0]);
    stats[5] = u64::from(boundary.source_indices[1]);
    stats[7] = u64::from(count - boundary.after_index);
    stats[8] = boundary.batch;
    stats[9] = boundary.consumer;
    stats[10] = boundary.graph_epoch;
    stats[11] = boundary.fingerprint;
    w.infix_in_flight = true;
    boundary.after_index
}
