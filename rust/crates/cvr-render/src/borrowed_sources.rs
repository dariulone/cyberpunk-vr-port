//! THE BORROWED SOURCE RANGES AND FIXED RANGES (formerly src/Render/StereoDescriptorAudit.cpp's BorrowedSourceRanges and
//! BorrowedFixedRange glue: CaptureBorrowedSourceRanges, ValidateBorrowedSourceRanges, DescribeBorrowedSourceRanges,
//! InventoryBorrowedSourceRange, CaptureBorrowedFixedRange and DescribeBorrowedFixedRange) for their Rust caller, the
//! retained capture: the draw's current family (families_native.rs) and recording checked around the capture of
//! audit_borrowed_ranges.rs, its ranges described, a fixed range's slots classified and its assets pinned and
//! acquired as immutable leases (texture_pins.rs, texture_immutable.rs). The .cpp keeps its versions for the Rust
//! test variants only.
use core::ffi::c_void;
use core::ptr::null_mut;
use std::collections::BTreeMap;
use std::sync::Arc;

use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::audit_borrowed_ranges::{self as bsr, stage, Borrowed};
use crate::descriptor_audit as core_;
use crate::descriptor_store::{BorrowedDescriptorInventory, BorrowedShaderUse, RangeIdentity, Value, SLOT_LIMIT};
use crate::families_native::Family;
use crate::native_recording::{Context, Lease};
use cvr_gpu::scene_state::Bindings;

fn stage_at(value: u32) {
    stage(value, u64::MAX, 0, 0, 0);
}
/// A held recording lease (native_recording::Lease's copy: its token referenced).
pub struct Recording(pub Lease);
impl Recording {
    pub fn copy(lease: &Lease) -> Recording {
        if !lease.token.is_null() {
            crate::structured_snapshot_receipt::add_ref(lease.token);
        }
        Recording(Lease { token: lease.token, generation: lease.generation, context: lease.context, list: lease.list, registry: lease.registry })
    }
}
impl Drop for Recording {
    fn drop(&mut self) {
        if !self.0.token.is_null() {
            crate::structured_snapshot_receipt::release(self.0.token);
        }
    }
}
fn game_base() -> u64 {
    // SAFETY: the process's own module.
    unsafe { GetModuleHandleW(None) }.map_or(0, |m| m.0 as u64)
}
fn current_context() -> Context {
    let c = crate::camera_upload::current(game_base());
    Context { context: c.context, state: c.state, list: c.list }
}
fn validate_recording(lease: &Lease, list: *mut c_void, context: &Context) -> bool {
    // SAFETY: the records and the live list.
    unsafe { crate::native_recording::cvr_native_recording_validate_recording(lease, list, context) }
}
fn source_bindings(list: *mut c_void) -> Option<Bindings> {
    let mut actual = Bindings::default();
    // SAFETY: a writable record.
    unsafe { cvr_gpu::scene_state::cvr_scene_capture_source_bindings(list as usize, &mut actual) }.then_some(actual)
}
/// SameBorrowedBindings.
fn same_bindings(a: &Bindings, b: &Bindings) -> bool {
    if a.root != b.root || a.heap_count != b.heap_count || a.heaps != b.heaps || a.table_mask != b.table_mask {
        return false;
    }
    (0..64).all(|p| a.table_mask & (1u64 << p) == 0 || a.tables[p] == b.tables[p])
}
fn shader_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
/// CurrentBorrowedFamily: the list's current draw state's pipeline, its bound family the pipeline's own, with this
/// source root and authenticated shader hashes and uses.
fn current_family(list: *mut c_void, root: usize) -> Option<(Arc<Family>, *mut c_void)> {
    let current = crate::stereo_probe_state::current_state(list)?;
    if current.list != list as u64 || current.root != root as u64 || current.pipeline == 0 {
        return None;
    }
    let pso = current.pipeline as *mut c_void;
    let bound = crate::families_native::bound(list)?;
    let looked = crate::families_native::lookup_raw(pso)?;
    if !Arc::ptr_eq(&bound, &looked)
        || bound.source_root.as_raw() as usize != root
        || !shader_hash(&bound.root_sha)
        || !shader_hash(&bound.vertex_sha)
        || !(if bound.pass.pixel_shader { shader_hash(&bound.pixel_sha) } else { bound.pixel_sha.is_empty() })
        || bound.uses.is_empty()
        || bound.uses.len() > 4096
    {
        return None;
    }
    Some((bound, pso))
}

/// BorrowedSourceRanges::Data.
pub struct SourceRanges {
    rust: *mut Borrowed,
    pub family: Arc<Family>,
    pub recording: Recording,
}
// SAFETY: the capture's handle is only used under the retained capture's lock.
unsafe impl Send for SourceRanges {}
// SAFETY: as above.
unsafe impl Sync for SourceRanges {}
impl Drop for SourceRanges {
    fn drop(&mut self) {
        // SAFETY: the capture's own handle, freed once.
        unsafe { bsr::cvr_sda_bsr_free(self.rust) };
    }
}
/// BorrowedRangeSummary::Range.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct RangeRow {
    pub parameter: u32,
    pub kind: u32,
    pub first_offset: u32,
    pub count: u32,
    pub stride: u32,
    pub shader_declared_count: u32,
    pub table: u64,
    pub gpu: u64,
    pub heap: u64,
    pub version: u64,
    pub physical_remainder: bool,
}
/// BorrowedRangeSummary.
#[derive(Default)]
pub struct RangeSummary {
    pub list: u64,
    pub generation: u64,
    pub root: u64,
    pub pipeline: u64,
    pub family: u64,
    pub ranges: Vec<RangeRow>,
}
fn view(ranges: &SourceRanges) -> bsr::View {
    let mut v = bsr::View { root: 0, pipeline: 0, ranges: core::ptr::null(), range_count: 0 };
    // SAFETY: the live capture; a writable view (its ranges live with the capture).
    unsafe { bsr::cvr_sda_bsr_view(ranges.rust, &mut v) };
    v
}
fn view_ranges(v: &bsr::View) -> &[crate::audit_borrowed::Range] {
    if v.ranges.is_null() || v.range_count == 0 {
        &[]
    } else {
        // SAFETY: the capture's ranges.
        unsafe { core::slice::from_raw_parts(v.ranges, v.range_count) }
    }
}
/// DescribedRange.
fn described(range: &crate::audit_borrowed::Range) -> RangeRow {
    let (i, u) = (&range.identity, &range.used);
    RangeRow {
        parameter: u.parameter,
        kind: u.kind,
        first_offset: u.offset,
        count: i.count,
        stride: i.stride,
        shader_declared_count: u.shader_declared_count,
        table: range.table,
        gpu: i.gpu,
        heap: i.heap,
        version: i.version,
        physical_remainder: u.physical_remainder,
    }
}
/// CaptureBorrowedSourceRanges: at the source draw, its recording, bindings and family checked before and after the
/// capture; None (with the thread's stage) when refused.
pub fn capture(list: *mut c_void, recording: &Lease, bindings: &Bindings) -> Option<Arc<SourceRanges>> {
    stage_at(2);
    if !core_::enabled() || !core_::tracking_call(1, 0) {
        return None;
    }
    stage_at(1);
    if list.is_null()
        || recording.token.is_null()
        || recording.generation == 0
        || recording.list != list as u64
        || bindings.root == 0
        || bindings.heap_count == 0
        || bindings.heap_count > 2
    {
        return None;
    }
    // SAFETY: the live list.
    let direct = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.is_some_and(|l| unsafe { l.GetType() } == D3D12_COMMAND_LIST_TYPE_DIRECT);
    if !direct {
        return None;
    }
    let context = current_context();
    stage_at(3);
    if !context.valid() || !validate_recording(recording, list, &context) {
        return None;
    }
    stage_at(4);
    if !source_bindings(list).is_some_and(|actual| same_bindings(bindings, &actual)) {
        return None;
    }
    stage_at(5);
    let (family, pipeline) = current_family(list, bindings.root)?;
    let uses: Vec<BorrowedShaderUse> = family
        .uses
        .iter()
        .map(|u| BorrowedShaderUse { kind: u.kind, visibility: u.visibility, space: u.space, shader_register: u.shader_register, count: u.count })
        .collect();
    let heaps: [*mut c_void; 2] = [bindings.heaps[0] as *mut c_void, bindings.heaps[1] as *mut c_void];
    // SAFETY: the held list, root, pipeline and heaps; the bindings' tables and the uses.
    let handle = unsafe {
        bsr::cvr_sda_bsr_capture(list, bindings.root as *mut c_void, pipeline, heaps.as_ptr(), bindings.heap_count, bindings.table_mask, &bindings.tables, uses.as_ptr(), uses.len())
    };
    if handle.is_null() {
        return None;
    }
    let next = Arc::new(SourceRanges { rust: handle, family: family.clone(), recording: Recording::copy(recording) });
    stage_at(3);
    let again = current_context();
    if again != context || !validate_recording(recording, list, &context) {
        return None;
    }
    stage_at(4);
    if !source_bindings(list).is_some_and(|actual| same_bindings(bindings, &actual)) {
        return None;
    }
    stage_at(5);
    match current_family(list, bindings.root) {
        Some((f, p)) if Arc::ptr_eq(&f, &family) && p == pipeline => {}
        _ => return None,
    }
    // SAFETY: the live capture (the lock taken there).
    if !unsafe { bsr::cvr_sda_bsr_matches(next.rust, true) } {
        return None;
    }
    stage_at(0);
    Some(next)
}
/// ValidateBorrowedSourceRanges.
pub fn validate(ranges: &SourceRanges) -> bool {
    stage_at(1);
    // SAFETY: the live capture.
    unsafe { bsr::cvr_sda_bsr_validate(ranges.rust) }
}
/// DescribeBorrowedSourceRanges.
pub fn describe(ranges: &SourceRanges) -> Option<RangeSummary> {
    stage_at(1);
    if !validate(ranges) {
        return None;
    }
    let v = view(ranges);
    let summary = RangeSummary {
        list: ranges.recording.0.list,
        generation: ranges.recording.0.generation,
        root: v.root,
        pipeline: v.pipeline,
        family: ranges.family.id,
        ranges: view_ranges(&v).iter().map(described).collect(),
    };
    validate(ranges).then_some(summary)
}
/// BorrowedRangeInventory (its list, generation, root, pipeline and family are the summary's).
pub struct RangeInventory {
    pub list: u64,
    pub generation: u64,
    pub root: u64,
    pub pipeline: u64,
    pub family: u64,
    pub range: RangeRow,
    pub counts: BorrowedDescriptorInventory,
}
/// InventoryBorrowedSourceRange.
pub fn inventory(ranges: &SourceRanges, index: u32) -> Option<RangeInventory> {
    if !validate(ranges) {
        return None;
    }
    let mut counts = BorrowedDescriptorInventory::default();
    // SAFETY: the live capture; a writable count.
    if !unsafe { bsr::cvr_sda_bsr_inventory(ranges.rust, index, &mut counts) } {
        return None;
    }
    let v = view(ranges);
    let range = described(view_ranges(&v).get(index as usize)?);
    let next = RangeInventory {
        list: ranges.recording.0.list,
        generation: ranges.recording.0.generation,
        root: v.root,
        pipeline: v.pipeline,
        family: ranges.family.id,
        range,
        counts,
    };
    if !validate(ranges) {
        return None;
    }
    stage_at(0);
    Some(next)
}

// ---- the fixed ranges

/// BorrowedFixedStatus.
pub mod status {
    pub const UNKNOWN: u32 = 0;
    pub const TYPED_NULL: u32 = 1;
    pub const IMMUTABLE: u32 = 2;
    pub const UNSUPPORTED_DESCRIPTOR: u32 = 3;
    pub const UNSUPPORTED_VIEW: u32 = 4;
    pub const UNSUPPORTED_SHAPE: u32 = 5;
    pub const MISSING_CANDIDATE: u32 = 6;
    pub const PIN_REFUSED: u32 = 7;
    pub const COOKIE_MISMATCH: u32 = 8;
    pub const IMMUTABLE_REFUSED: u32 = 9;
}
#[derive(Clone, Copy)]
pub struct FixedSlot {
    pub offset: u32,
    pub asset: u32,
    pub cookie: u64,
    pub candidate: u64,
    pub status: u32,
}
#[derive(Clone, Copy, Default)]
pub struct FixedAsset {
    pub cookie: u64,
    pub candidate: u64,
    pub actual_cookie: u64,
    pub status: u32,
    pub stage: u32,
    pub failure: u32,
    pub pin_details: [u64; 8],
}
#[derive(Clone, Copy)]
pub struct FixedSample {
    pub offset: u32,
    pub asset: u32,
    pub status: u32,
    pub canonical: Value,
}
/// BorrowedFixedRangeSummary.
#[derive(Default)]
pub struct FixedSummary {
    pub list: u64,
    pub generation: u64,
    pub root: u64,
    pub pipeline: u64,
    pub family: u64,
    pub range_index: u32,
    pub range: RangeRow,
    pub counts: [u64; 10],
    pub all_slots_qualified: bool,
    pub slots: Vec<FixedSlot>,
    pub assets: Vec<FixedAsset>,
    pub samples: Vec<FixedSample>,
}
/// texture_read::ImmutableLease, held (its owner and its resource referenced).
pub struct HeldImmutable(pub crate::texture_immutable::ImmutableLease);
impl Drop for HeldImmutable {
    fn drop(&mut self) {
        crate::structured_snapshot_receipt::release(self.0.owner);
        crate::structured_snapshot_receipt::release(self.0.source.resource);
    }
}
/// BorrowedFixedRange::Data.
pub struct FixedRange {
    pub witness: Arc<SourceRanges>,
    pub summary: FixedSummary,
    pub canonical: Vec<Value>,
    pub leases: Vec<HeldImmutable>,
}
/// The resource's identity cookie (cookieGuid's private data).
const COOKIE_GUID: GUID = GUID::from_values(0xc7ec4301, 0x790e, 0x4b32, [0x93, 0x87, 0x7c, 0xc1, 0x19, 0x52, 0xd1, 0x44]);
fn same_fixed_recording(a: &Lease, b: &Lease) -> bool {
    !a.token.is_null() && a.token == b.token && a.list == b.list && a.generation == b.generation && a.context == b.context && a.registry == b.registry
}
fn texture_failure() -> u32 {
    crate::texture_lease::cvr_tr_failure()
}
fn pin_details() -> [u64; 8] {
    let mut d = [0u64; 8];
    // SAFETY: a writable record.
    unsafe { crate::texture_pins::cvr_tr_fixed_pin_details(&mut d) };
    d
}
/// texture_read::FixedAssetPins (its owner and recording held).
struct Pins(crate::texture_pins::PinsData);
impl Drop for Pins {
    fn drop(&mut self) {
        crate::structured_snapshot_receipt::release(self.0.held);
        crate::structured_snapshot_receipt::release(self.0.recording.token);
    }
}
/// CaptureBorrowedFixedRange: the witness's physical-remainder SRV range `index`, its slots classified, each candidate
/// asset pinned, its cookie checked and acquired as an immutable lease, all of it revalidated.
pub fn capture_fixed(list: *mut c_void, recording: &Lease, witness: &Arc<SourceRanges>, index: u32) -> Option<FixedRange> {
    stage_at(1);
    if list.is_null()
        || cvr_host::probe::internal()
        || !core_::enabled()
        || !same_fixed_recording(recording, &witness.recording.0)
        || recording.list != list as u64
    {
        return None;
    }
    let context = current_context();
    stage_at(3);
    if !context.valid() || !validate_recording(recording, list, &context) {
        return None;
    }
    let range_summary = describe(witness)?;
    stage(13, u64::from(index), 0, 0, 0);
    let row = *range_summary.ranges.get(index as usize)?;
    if !row.physical_remainder || row.kind != D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0 as u32 {
        return None;
    }
    let v = view(witness);
    let range: RangeIdentity = view_ranges(&v)[index as usize].identity;
    if range.count == 0 || range.count as usize > SLOT_LIMIT {
        return None;
    }
    let n = range.count as usize;
    let mut summary = FixedSummary {
        list: range_summary.list,
        generation: range_summary.generation,
        root: range_summary.root,
        pipeline: range_summary.pipeline,
        family: range_summary.family,
        range_index: index,
        range: row,
        ..FixedSummary::default()
    };
    let mut canonical: Vec<Value> = vec![[0u64; 20]; n];
    let mut slots: Vec<FixedSlot> = Vec::with_capacity(n);
    {
        let _guard = core_::guard();
        core_::drain_retired();
        // SAFETY: the live capture, under the metadata lock.
        if !unsafe { bsr::cvr_sda_bsr_matches(witness.rust, false) } {
            return None;
        }
        let mut candidates = vec![0u64; n];
        // SAFETY: the store under its lock; the range and outputs of its count.
        unsafe { crate::audit_fixed::cvr_sda_fixed_snapshot(core_::store(), &range, canonical.as_mut_ptr(), candidates.as_mut_ptr()) };
        for i in 0..n {
            slots.push(FixedSlot { offset: row.first_offset + i as u32, asset: u32::MAX, cookie: canonical[i][2], candidate: candidates[i], status: status::UNKNOWN });
        }
        // SAFETY: as above.
        if !unsafe { bsr::cvr_sda_bsr_matches(witness.rust, false) } {
            return None;
        }
    }
    let mut assets: BTreeMap<(u64, u64), u32> = BTreeMap::new();
    let mut leases: Vec<Option<HeldImmutable>> = Vec::new();
    for i in 0..n {
        let value = canonical[i];
        let (class, view_desc) = crate::borrowed_fixed::classify(&value);
        let slot = &mut slots[i];
        use crate::borrowed_fixed::class;
        match class {
            class::UNKNOWN => continue,
            class::TYPED_NULL => {
                slot.status = status::TYPED_NULL;
                continue;
            }
            class::UNSUPPORTED_DESCRIPTOR => {
                slot.status = status::UNSUPPORTED_DESCRIPTOR;
                continue;
            }
            class::UNSUPPORTED_VIEW => {
                slot.status = status::UNSUPPORTED_VIEW;
                continue;
            }
            class::UNSUPPORTED_SHAPE => {
                slot.status = status::UNSUPPORTED_SHAPE;
                continue;
            }
            _ => {}
        }
        if slot.candidate == 0 {
            slot.status = status::MISSING_CANDIDATE;
            continue;
        }
        let key = (slot.cookie, slot.candidate);
        if let Some(&found) = assets.get(&key) {
            slot.asset = found;
        } else {
            slot.asset = summary.assets.len() as u32;
            assets.insert(key, slot.asset);
            let mut asset = FixedAsset { cookie: slot.cookie, candidate: slot.candidate, stage: 1, ..FixedAsset::default() };
            let mut lease: Option<HeldImmutable> = None;
            // SAFETY: zeroed pins are the empty record the pin fills.
            let mut pins = Pins(unsafe { core::mem::zeroed() });
            // SAFETY: the live list; the pins' record.
            if !unsafe { crate::texture_pins::cvr_tr_pin_fixed_asset(list, slot.candidate, &mut pins.0) } {
                asset.status = status::PIN_REFUSED;
                asset.failure = texture_failure();
                asset.pin_details = pin_details();
            } else {
                asset.pin_details = pin_details();
                asset.stage = 2;
                let mut resource: *mut c_void = null_mut();
                // SAFETY: the pins; the resource's reference ours.
                let pinned = unsafe { crate::texture_pins::cvr_tr_fixed_pinned_resource(&pins.0, &mut resource) };
                // SAFETY: the reference handed over (or null).
                let resource = unsafe { (!resource.is_null()).then(|| ID3D12Resource::from_raw(resource)) };
                if !pinned || resource.as_ref().map_or(0, |r| r.as_raw() as u64) != slot.candidate {
                    asset.status = status::PIN_REFUSED;
                    asset.failure = texture_failure();
                    if asset.failure == 0 {
                        asset.failure = 1;
                    }
                } else {
                    asset.stage = 3;
                    let r = resource.as_ref().expect("pinned");
                    let mut bytes = 8u32;
                    let mut cookie = 0u64;
                    // SAFETY: a local word and its size.
                    let read = unsafe { r.GetPrivateData(&COOKIE_GUID, &mut bytes, Some((&raw mut cookie).cast())) };
                    asset.actual_cookie = cookie;
                    if read.is_err() || bytes != 8 || cookie != slot.cookie {
                        asset.status = status::COOKIE_MISMATCH;
                        asset.failure = read.err().map_or(1, |e| e.code().0 as u32);
                    } else {
                        asset.stage = 4;
                        // SAFETY: zeroed is the empty lease the acquisition fills.
                        let mut held: crate::texture_immutable::ImmutableLease = unsafe { core::mem::zeroed() };
                        // SAFETY: the live list, the pins and the output lease.
                        if !unsafe { crate::texture_pins::cvr_tr_acquire_pinned_immutable(list, &pins.0, &mut held) } {
                            asset.status = status::IMMUTABLE_REFUSED;
                            asset.failure = texture_failure();
                            drop(HeldImmutable(held));
                        } else {
                            asset.status = status::IMMUTABLE;
                            asset.stage = 0;
                            asset.failure = 0;
                            lease = Some(HeldImmutable(held));
                        }
                    }
                }
            }
            summary.assets.push(asset);
            leases.push(lease);
        }
        let asset_status = summary.assets[slot.asset as usize].status;
        slot.status = asset_status;
        if slot.status == status::IMMUTABLE {
            let held = leases[slot.asset as usize].as_ref();
            let resource = held.map_or(null_mut(), |l| l.0.source.resource);
            // SAFETY: the lease's held resource (or null).
            let matches = unsafe { ID3D12Resource::from_raw_borrowed(&resource) }
                .is_some_and(|r| crate::borrowed_fixed::view_matches(&value, &unsafe { r.GetDesc() }, &view_desc));
            if !matches {
                slot.status = status::UNSUPPORTED_VIEW;
            }
        }
    }
    for i in 0..leases.len() {
        if summary.assets[i].status != status::IMMUTABLE {
            continue;
        }
        // SAFETY: the held lease (no queue).
        let valid = leases[i].as_ref().is_some_and(|l| unsafe { crate::texture_immutable::cvr_tr_validate_immutable(&l.0, null_mut()) });
        if !valid {
            let asset = &mut summary.assets[i];
            asset.status = status::IMMUTABLE_REFUSED;
            asset.stage = 5;
            asset.failure = texture_failure();
            for slot in slots.iter_mut().filter(|s| s.asset == i as u32 && s.status == status::IMMUTABLE) {
                slot.status = status::IMMUTABLE_REFUSED;
            }
        }
    }
    stage_at(3);
    if current_context() != context || !validate_recording(recording, list, &context) {
        return None;
    }
    {
        let _guard = core_::guard();
        core_::drain_retired();
        // SAFETY: the live capture under the lock; the store and the saved values.
        if !unsafe { bsr::cvr_sda_bsr_matches(witness.rust, false) }
            || !unsafe { crate::audit_fixed::cvr_sda_fixed_canonical(core_::store(), canonical.as_ptr(), canonical.len(), &range) }
        {
            return None;
        }
    }
    stage_at(3);
    if current_context() != context || !validate_recording(recording, list, &context) {
        return None;
    }
    let mut sampled = [0u32; 10];
    for (i, slot) in slots.iter().enumerate() {
        summary.counts[slot.status as usize] += 1;
        let s = &mut sampled[slot.status as usize];
        if *s < 4 {
            summary.samples.push(FixedSample { offset: slot.offset, asset: slot.asset, status: slot.status, canonical: canonical[i] });
        }
        *s += 1;
    }
    summary.all_slots_qualified = summary.counts[status::TYPED_NULL as usize] + summary.counts[status::IMMUTABLE as usize] == u64::from(range.count);
    summary.slots = slots;
    stage_at(0);
    Some(FixedRange { witness: witness.clone(), summary, canonical, leases: leases.into_iter().flatten().collect() })
}
