//! prepared_pair::Source owned on the Rust side (formerly built by NativeRetainedGeometry.cpp's StagePairSource and
//! lent by NativePreparedPair.cpp's Aux): its owned and original geometry, recording, queue, root, shader snapshot,
//! private CBVs, tables, materials, input checks, T5 views, pass baseline, native exit, VRS capture and owners; the
//! pass checkpoints (GeometryTargetCheckpoint), the VRS snapshots (StereoVrsSnapshot) and the native exits
//! (prepared_pair::NativeExit) it shares; and the lent views prepared_pair_native.rs reads (SourceView and its arrays).
//! Every plain row's COM pointers are held by the source (`held`), as the C++ copies' ComPtrs were.

use core::ffi::c_void;
use std::sync::Arc;

use windows::core::{IUnknown, Interface};
use windows::Win32::Graphics::Direct3D12::*;

use cvr_gpu::scene_state::GeometryTargetsRaw;

use crate::geometry_leases::DrawResources;
use crate::native_exit::NativeExitPlane;
use crate::prepared_pair_native as npp;
use crate::shared_table::SharedTable;

pub(crate) fn held(raw: *mut c_void) -> Option<IUnknown> {
    // SAFETY: a live COM object the source's rows name (or null).
    unsafe { IUnknown::from_raw_borrowed(&raw) }.cloned()
}

// ---- the shared pass objects

/// GeometryTargetCheckpoint: the Rust checkpoint, its captured targets referenced.
pub struct Checkpoint {
    rust: *mut cvr_gpu::geometry_checkpoint::Checkpoint,
    targets: GeometryTargetsRaw,
    resources: Vec<ID3D12Resource>,
    captured: bool,
}
// SAFETY: used under the capture's and the pair's locks only.
unsafe impl Send for Checkpoint {}
// SAFETY: as above.
unsafe impl Sync for Checkpoint {}
impl Drop for Checkpoint {
    fn drop(&mut self) {
        // SAFETY: the checkpoint this holder created, freed once.
        unsafe { cvr_gpu::geometry_checkpoint::cvr_geometry_checkpoint_free(self.rust) };
    }
}
impl Checkpoint {
    /// Create (the shape's private copies) then Capture (the native targets copied on `list`).
    pub fn capture(device: *mut c_void, list: *mut c_void, targets: &GeometryTargetsRaw) -> Result<Arc<Checkpoint>, u32> {
        let mut rust = core::ptr::null_mut();
        // SAFETY: a live device and the shape.
        if unsafe { cvr_gpu::geometry_checkpoint::cvr_geometry_checkpoint_create(device, targets, &mut rust) }.is_err() || rust.is_null() {
            return Err(2);
        }
        let mut value = Checkpoint { rust, targets: unsafe { core::mem::zeroed() }, resources: Vec::new(), captured: false };
        let mut out: GeometryTargetsRaw = unsafe { core::mem::zeroed() };
        // SAFETY: the live checkpoint, list and targets; a local output.
        if list.is_null()
            || !unsafe { cvr_gpu::geometry_checkpoint::cvr_geometry_checkpoint_capture(rust, list, targets) }
            || !unsafe { cvr_gpu::geometry_checkpoint::cvr_geometry_checkpoint_targets(rust, &mut out) }
        {
            return Err(3);
        }
        for &r in &out.resources {
            let raw = r as *mut c_void;
            if !raw.is_null() {
                // SAFETY: the reference the targets call handed over.
                value.resources.push(unsafe { ID3D12Resource::from_raw(raw) });
            }
        }
        value.targets = out;
        value.captured = true;
        Ok(Arc::new(value))
    }
    pub fn captured(&self) -> bool {
        self.captured
    }
    pub fn targets(&self) -> &GeometryTargetsRaw {
        &self.targets
    }
    /// Targets().resources[slot].
    pub fn resource(&self, slot: usize) -> Option<ID3D12Resource> {
        held(self.targets.resources[slot] as *mut c_void).and_then(|u| u.cast().ok())
    }
}
/// StereoVrsSnapshot: the Rust snapshot.
pub struct Vrs(*mut cvr_gpu::vrs_snapshot::Snapshot);
// SAFETY: as Checkpoint.
unsafe impl Send for Vrs {}
// SAFETY: as above.
unsafe impl Sync for Vrs {}
impl Drop for Vrs {
    fn drop(&mut self) {
        // SAFETY: the snapshot this holder created, freed once.
        unsafe { cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_free(self.0) };
    }
}
impl Vrs {
    /// Initialize + Capture of the rate image (eye 0); None when either failed.
    pub fn capture(device: *mut c_void, list: *mut c_void, image: *mut c_void) -> Option<Arc<Vrs>> {
        let value = Vrs(cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_new());
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: the new snapshot, a live device, list and image.
        let ok = unsafe { cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_initialize(value.0, device, image) }.is_ok()
            && unsafe { cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_capture(value.0, list, image, 0) }.is_ok();
        ok.then(|| Arc::new(value))
    }
    pub fn raw(&self) -> *mut c_void {
        self.0.cast()
    }
    /// SourceImage.
    pub fn source_image(&self) -> *mut c_void {
        let mut frozen = core::ptr::null_mut();
        // SAFETY: the live snapshot; a local output.
        unsafe { cvr_gpu::vrs_snapshot::cvr_vrs_snapshot_info(self.0, &mut frozen, core::ptr::null_mut(), core::ptr::null_mut()) };
        frozen
    }
}
/// prepared_pair::NativeExit.
pub struct NativeExit {
    pub readback: Option<ID3D12Resource>,
    pub width: u32,
    pub height: u32,
    pub eye: u32,
    pub source_side: u32,
    pub frame: u32,
    pub list: u64,
    pub generation: u64,
    pub planes: Vec<NativeExitPlane>,
}

// ---- the source

/// prepared_pair::Material: the binding, the immutable lease and the buffer copy, their references held.
pub struct Material {
    pub binding: crate::material_buffer_snapshot::SourceResourceBinding,
    pub immutable: crate::texture_immutable::ImmutableLease,
    pub buffer: Option<crate::material_snapshot_receipt::Copy>,
    held: Vec<IUnknown>,
}
impl Material {
    pub fn new(
        binding: crate::material_buffer_snapshot::SourceResourceBinding,
        immutable: crate::texture_immutable::ImmutableLease,
        buffer: Option<crate::material_snapshot_receipt::Copy>,
    ) -> Material {
        let mut keep: Vec<IUnknown> = [binding.resource as *mut c_void, binding.counter as *mut c_void, immutable.owner, immutable.source.resource].into_iter().filter_map(held).collect();
        if let Some(b) = &buffer {
            keep.extend([b.owner, b.resource].into_iter().filter_map(held));
        }
        Material { binding, immutable, buffer, held: keep }
    }
}
/// source_binding_assembly::ShaderInputCheck.
pub struct InputCheck {
    pub resource: *mut c_void,
    pub owner: *mut c_void,
    pub offset: u64,
    pub expected: Vec<u8>,
}
/// prepared_pair::Source.
pub struct PairSource {
    pub ordinal: u64,
    pub recording: crate::native_recording::Lease,
    pub queue: Option<ID3D12CommandQueue>,
    pub original: DrawResources,
    pub owned: DrawResources,
    pub root: *mut c_void,
    pub shader: Option<crate::shader_snapshot_native::Source>,
    pub cbvs: Vec<npp::PrivateCbv>,
    pub cbv_receipt: npp::PreparedCopies,
    pub tables: Vec<SharedTable>,
    pub reference_tables: Vec<SharedTable>,
    pub materials: Vec<Material>,
    pub input_checks: Vec<InputCheck>,
    pub t5: Option<npp::T5View>,
    pub full_t5: Option<npp::FullT5>,
    pub rate_image: Option<ID3D12Resource>,
    pub pass_baseline: Option<Arc<Checkpoint>>,
    pub native_exit: Option<Arc<NativeExit>>,
    pub allow_vrs_refinement: bool,
    pub pass_depth: Option<ID3D12Resource>,
    pub rate_capture: Option<Arc<Vrs>>,
    pub owners: Vec<IUnknown>,
    /// The references the plain rows point at (the recording's token, the root, the CBVs, the receipt, the checks, T5).
    pub held: Vec<IUnknown>,
}
impl PairSource {
    /// The plain rows' references held (once the rows are final).
    pub fn hold_rows(&mut self) {
        let mut keep: Vec<*mut c_void> = vec![self.recording.token, self.root, self.cbv_receipt.owner];
        for c in &self.cbvs {
            keep.extend([c.resource, c.owner]);
        }
        for c in &self.input_checks {
            keep.extend([c.resource, c.owner]);
        }
        if let Some(t) = &self.t5 {
            keep.extend([t.resource, t.owner]);
        }
        if let Some(f) = &self.full_t5 {
            keep.extend([f.copy.owner, f.range.resource, f.range.owner]);
        }
        self.held = keep.into_iter().filter_map(held).collect();
    }
    /// The source's shader identity (the C++ Snapshot's address) and serialized root.
    fn shader_view(&self) -> (*const c_void, *const u8, usize) {
        self.shader.as_ref().map_or((core::ptr::null(), core::ptr::null(), 0), |s| {
            let m = &s.info().mirror;
            (m.identity, m.root, m.root_len)
        })
    }
}

/// A plain repr(C) row copied (its COM pointers are held by the copy's `held` list).
fn plain<T>(value: &T) -> T {
    // SAFETY: the rows are plain data; the references they name are held separately.
    unsafe { core::ptr::read(value) }
}
impl Clone for Material {
    fn clone(&self) -> Material {
        Material { binding: self.binding, immutable: plain(&self.immutable), buffer: self.buffer.as_ref().map(plain), held: self.held.clone() }
    }
}
impl Clone for InputCheck {
    fn clone(&self) -> InputCheck {
        InputCheck { resource: self.resource, owner: self.owner, offset: self.offset, expected: self.expected.clone() }
    }
}
impl Clone for PairSource {
    fn clone(&self) -> PairSource {
        PairSource {
            ordinal: self.ordinal,
            recording: plain(&self.recording),
            queue: self.queue.clone(),
            original: self.original.clone(),
            owned: self.owned.clone(),
            root: self.root,
            shader: self.shader.clone(),
            cbvs: self.cbvs.iter().map(plain).collect(),
            cbv_receipt: plain(&self.cbv_receipt),
            tables: self.tables.clone(),
            reference_tables: self.reference_tables.clone(),
            materials: self.materials.clone(),
            input_checks: self.input_checks.clone(),
            t5: self.t5.as_ref().map(plain),
            full_t5: self.full_t5.as_ref().map(plain),
            rate_image: self.rate_image.clone(),
            pass_baseline: self.pass_baseline.clone(),
            native_exit: self.native_exit.clone(),
            allow_vrs_refinement: self.allow_vrs_refinement,
            pass_depth: self.pass_depth.clone(),
            rate_capture: self.rate_capture.clone(),
            owners: self.owners.clone(),
            held: self.held.clone(),
        }
    }
}

// ---- the lent views

/// The sources and their lent arrays (the views point into them), for prepared_pair_native.rs.
pub struct Lent {
    pub sources: Vec<PairSource>,
    pub views: Vec<npp::SourceView>,
    tables: Vec<Vec<crate::binding_bank::Table>>,
    references: Vec<Vec<crate::binding_bank::Table>>,
    materials: Vec<Vec<npp::MaterialView>>,
    inputs: Vec<Vec<npp::InputCheckView>>,
    exits: Vec<npp::ExitView>,
    owners: Vec<Vec<*mut c_void>>,
}
// SAFETY: the views' pointers are into this holder and its sources' owned objects.
unsafe impl Send for Lent {}
fn lease(l: &crate::geometry_leases::BufferLease) -> crate::geometry_commands::Lease {
    l.commands()
}
/// Aux's Lend: each source's view and its arrays.
pub fn lend(sources: Vec<PairSource>) -> Box<Lent> {
    let mut lent = Box::new(Lent {
        sources,
        views: Vec::new(),
        tables: Vec::new(),
        references: Vec::new(),
        materials: Vec::new(),
        inputs: Vec::new(),
        exits: Vec::new(),
        owners: Vec::new(),
    });
    let n = lent.sources.len();
    for s in &lent.sources {
        lent.tables.push(s.tables.iter().map(SharedTable::lend).collect());
        lent.references.push(s.reference_tables.iter().map(SharedTable::lend).collect());
        lent.materials.push(
            s.materials
                .iter()
                .map(|m| {
                    let b = &m.binding;
                    let buffer = m.buffer.as_ref();
                    npp::MaterialView {
                        parameter: b.parameter,
                        offset: b.offset,
                        kind: b.kind,
                        has_buffer: buffer.is_some(),
                        // SAFETY: the same 176-byte DescriptorStamp record.
                        stamp: unsafe { core::ptr::read((&raw const b.stamp).cast()) },
                        resource: b.resource as *mut c_void,
                        counter: b.counter as *mut c_void,
                        immutable_owner: m.immutable.owner,
                        immutable_resource: m.immutable.source.resource,
                        buffer_owner: buffer.map_or(core::ptr::null_mut(), |c| c.owner),
                        buffer_resource: buffer.map_or(core::ptr::null_mut(), |c| c.resource),
                        buffer_view: buffer.map_or_else(D3D12_SHADER_RESOURCE_VIEW_DESC::default, |c| c.view),
                        immutable: &m.immutable,
                        binding: &m.binding,
                        buffer: buffer.map_or(core::ptr::null(), |c| c as *const _),
                    }
                })
                .collect(),
        );
        lent.inputs.push(
            s.input_checks
                .iter()
                .map(|c| npp::InputCheckView { resource: c.resource, owner: c.owner, offset: c.offset, expected: c.expected.as_ptr(), expected_len: c.expected.len() })
                .collect(),
        );
        lent.exits.push(s.native_exit.as_ref().map_or_else(
            || npp::ExitView { identity: core::ptr::null(), readback: core::ptr::null_mut(), width: 0, height: 0, eye: 0, source_side: 0, frame: 0, list: 0, generation: 0, planes: core::ptr::null(), plane_count: 0 },
            |x| npp::ExitView {
                identity: Arc::as_ptr(x).cast(),
                readback: x.readback.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw()),
                width: x.width,
                height: x.height,
                eye: x.eye,
                source_side: x.source_side,
                frame: x.frame,
                list: x.list,
                generation: x.generation,
                planes: x.planes.as_ptr(),
                plane_count: x.planes.len(),
            },
        ));
        lent.owners.push(s.owners.iter().map(|o| o.as_raw()).collect());
    }
    for i in 0..n {
        let s = &lent.sources[i];
        let (shader, shader_root, shader_root_len) = s.shader_view();
        let state = &s.owned.state;
        let baseline = s.pass_baseline.as_ref();
        lent.views.push(npp::SourceView {
            ordinal: s.ordinal,
            recording: &s.recording,
            source_queue: s.queue.as_ref().map_or(core::ptr::null_mut(), |q| q.as_raw()),
            owned: crate::geometry_commands::Resources {
                index: lease(&s.owned.index),
                vertices: core::array::from_fn(|k| lease(&s.owned.vertices[k])),
                draw: &s.owned.draw,
                instances: s.owned.instances.as_ptr(),
                instance_count: s.owned.instances.len(),
            },
            owned_state: npp::DrawStateView {
                pipeline: state.original.as_ref().map_or(core::ptr::null_mut(), |p| p.as_raw()),
                rate_image: state.rate_image.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw()),
                rate: state.rate,
                topology: state.topology,
                stencil: state.stencil,
                blend: state.blend,
            },
            original_draw: &s.original.draw,
            original_pipeline: s.original.state.original.as_ref().map_or(core::ptr::null_mut(), |p| p.as_raw()),
            root: s.root,
            shader,
            shader_root,
            shader_root_len,
            cbvs: s.cbvs.as_ptr(),
            cbv_count: s.cbvs.len(),
            cbv_receipt: &s.cbv_receipt,
            tables: lent.tables[i].as_ptr(),
            table_count: lent.tables[i].len(),
            reference_tables: lent.references[i].as_ptr(),
            reference_table_count: lent.references[i].len(),
            materials: lent.materials[i].as_ptr(),
            material_count: lent.materials[i].len(),
            input_checks: lent.inputs[i].as_ptr(),
            input_check_count: lent.inputs[i].len(),
            t5: s.t5.as_ref().map_or(core::ptr::null(), |t| t as *const _),
            full_t5: s.full_t5.as_ref().map_or(core::ptr::null(), |t| t as *const _),
            rate_image: s.rate_image.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw()),
            baseline: baseline.map_or(core::ptr::null(), |b| Arc::as_ptr(b).cast()),
            baseline_captured: baseline.is_some_and(|b| b.captured()),
            allow_vrs_refinement: s.allow_vrs_refinement,
            baseline_targets: baseline.filter(|b| b.captured()).map_or_else(|| unsafe { core::mem::zeroed() }, |b| *b.targets()),
            native_exit: if s.native_exit.is_some() { &lent.exits[i] } else { core::ptr::null() },
            pass_depth: s.pass_depth.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw()),
            rate_capture: s.rate_capture.as_ref().map_or(core::ptr::null_mut(), |v| v.raw()),
            owners: lent.owners[i].as_ptr(),
            owner_count: lent.owners[i].len(),
        });
    }
    lent
}
