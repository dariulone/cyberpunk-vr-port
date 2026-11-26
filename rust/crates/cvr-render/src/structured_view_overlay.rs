//! The structured snapshot view's plan and overlay (src/Render/StructuredSnapshotView.cpp): a plan of private t5
//! versions -- each a distinct valid occurrence scope (its b4 and completion scopes the same), the whole original view
//! (8192 records of 128 bytes over 1 MiB, as its read plan describes), the private b4 bytes matched on the GPU and
//! equal to the plan's, the dense private range owned by a finalized copy of exactly this occurrence (at most 4 KiB,
//! 128-aligned, inside a flagless DEFAULT buffer, as the read plan reads), a certified completion on a DIRECT queue of
//! the buffer's device, and a reservation inside the budget --; the overlay that adopts the source copies at their
//! original positions once every source fence completed (one recording per backing buffer, ranges in order), or creates
//! its own DEFAULT buffer and records the selected copies once (a certified current DIRECT recording of the overlay's
//! device; every source still finalized and completed), finishing PIXEL_SHADER_RESOURCE | COPY_SOURCE; the private
//! views it exposes and their validation (scope, SRV, offsets, records, b4 and copy recording).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the read plan's check (multilayer_selectors), the overlay's COM
//! owner, its stage and destination, the command-resource retention and the internal-commands scope; the unit tests
//! below build, adopt, create and record on WARP, and tools/single_pass_tests/structured_snapshot_view_test runs them
//! in its Rust variant (--gpu and --native).

use crate::structured_snapshot::{reserve_records, same_buffer_view, valid_view_scope, ViewMapping, ViewScope, VIEW_ELEMENTS, VIEW_STRIDE};
use core::ffi::c_void;
use windows::core::{IUnknown, Interface};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

/// structured_snapshot_view::DomainBytes / MaxBytes / B4Bytes / MaxRecords.
pub const DOMAIN_BYTES: u64 = VIEW_ELEMENTS * VIEW_STRIDE;
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
pub const B4_BYTES: usize = 228;
pub const MAX_RECORDS: usize = 32;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_FAIL: i32 = 0x8000_4005u32 as i32;

/// structured_snapshot_view::Failure.
pub mod failure {
    pub const NONE: u32 = 0;
    pub const SCOPE: u32 = 2;
    pub const VIEW: u32 = 3;
    pub const B4: u32 = 4;
    pub const RANGE: u32 = 6;
    pub const OWNERS: u32 = 7;
    pub const COMPLETION: u32 = 8;
    pub const CAPACITY: u32 = 9;
    pub const DEVICE: u32 = 10;
    pub const RECORDING: u32 = 11;
    pub const ALREADY_RECORDED: u32 = 12;
    pub const CHANGED: u32 = 14;
}

/// An Input's facts, lent by the .cpp.
#[repr(C)]
pub struct Input {
    pub scope: ViewScope,
    pub b4_scope: ViewScope,
    pub completion_scope: ViewScope,
    pub dense: *mut c_void,
    pub dense_owner: *mut c_void,
    pub dense_source: u64,
    pub dense_private: u64,
    pub dense_copied: u64,
    pub copy_owner: *mut c_void,
    pub copy_list: u64,
    pub copy_generation: u64,
    pub copy_occurrence: u64,
    pub copy_source: u64,
    pub copy_private: u64,
    pub copy_copied: u64,
    pub queue: *mut c_void,
    pub fence: *mut c_void,
    pub value: u64,
    pub first_element: u64,
    pub resource_width: u64,
    pub num_elements: u32,
    pub stride: u32,
    pub b4: *const u8,
    pub b4_bytes: usize,
    pub plan_b4: *const u8,
    pub certified: bool,
    pub finalized: bool,
    pub b4_verified: bool,
    pub bounded: bool,
    pub plan_view: bool,
}
/// The checked read plan's facts.
#[repr(C)]
pub struct Read {
    pub resource_offset: u64,
    pub byte_extent: u32,
    pub first_record: u32,
    pub record_span: u32,
    pub records: *const u32,
    pub count: usize,
}
/// A recording destination's facts.
#[repr(C)]
pub struct Destination {
    pub list: u64,
    pub generation: u64,
    pub allocator: *mut c_void,
    pub queue: *mut c_void,
    pub certified: bool,
}
/// structured_snapshot_view::PrivateView (its owners' pointers written by the .cpp).
#[repr(C)]
pub struct PrivateView {
    pub scope: ViewScope,
    pub resource: *mut c_void,
    pub owner: *mut c_void,
    pub srv: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub source_offset: u64,
    pub dense_offset: u64,
    pub target_offset: u64,
    pub copied_bytes: u64,
    pub copy_list: u64,
    pub copy_generation: u64,
    pub version: u32,
    pub first_record: u32,
    pub record_span: u32,
    pub record_count: u32,
    pub records: [u32; MAX_RECORDS],
    pub b4: [u8; B4_BYTES],
}
const _: () = assert!(
    core::mem::size_of::<Input>() == 296
        && core::mem::size_of::<Read>() == 40
        && core::mem::size_of::<Destination>() == 40
        && core::mem::size_of::<PrivateView>() == 520
        && core::mem::offset_of!(PrivateView, srv) == 56
        && core::mem::offset_of!(PrivateView, records) == 160
        && core::mem::offset_of!(PrivateView, b4) == 288
);

#[derive(Clone)]
struct Version {
    scope: ViewScope,
    mapping: ViewMapping,
    dense: Option<ID3D12Resource>,
    dense_owner: Option<IUnknown>,
    dense_source: u64,
    dense_private: u64,
    dense_copied: u64,
    copy_owner: Option<IUnknown>,
    copy_list: u64,
    copy_generation: u64,
    copy_occurrence: u64,
    copy_source: u64,
    copy_private: u64,
    copy_copied: u64,
    completion_scope: ViewScope,
    queue: Option<ID3D12CommandQueue>,
    fence: Option<ID3D12Fence>,
    value: u64,
    certified: bool,
    finalized: bool,
    resource_offset: u64,
    byte_extent: u32,
    first_record: u32,
    record_span: u32,
    records: Vec<u32>,
    b4: [u8; B4_BYTES],
}
/// A plan's versions (detail::PlanData).
#[derive(Default)]
pub struct Plan {
    versions: Vec<Version>,
    pub width: u64,
    pub copied: u64,
}

fn borrowed<T: Interface>(address: *mut c_void) -> Option<T> {
    // SAFETY: a live COM object the caller holds for the call (or null).
    unsafe { T::from_raw_borrowed(&address) }.cloned()
}
fn same_device<T: Interface>(device: &ID3D12Device, child: Option<&T>) -> bool {
    let Some(child) = child.and_then(|c| c.cast::<ID3D12DeviceChild>().ok()) else { return false };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live device child; the output a local.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|a| a.as_raw() == device.as_raw())
}
fn bytes<'a>(data: *const u8, count: usize) -> &'a [u8] {
    // SAFETY: the .cpp lends `count` bytes at `data` for the call.
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
/// DenseRange: the private range owned by its finalized copy, inside a flagless DEFAULT buffer.
fn dense_range(v: &Version) -> bool {
    let Some(resource) = &v.dense else { return false };
    if v.dense_owner.is_none()
        || v.copy_owner.is_none()
        || v.copy_list != v.scope.list
        || v.copy_generation != v.scope.generation
        || v.copy_occurrence != v.scope.occurrence
        || v.copy_source != v.dense_source
        || v.copy_private != v.dense_private
        || v.copy_copied != v.dense_copied
        || v.dense_source != v.resource_offset
        || v.dense_copied != u64::from(v.byte_extent)
        || v.dense_copied == 0
        || v.dense_copied > 4096
        || v.dense_private % VIEW_STRIDE != 0
    {
        return false;
    }
    // SAFETY: a live resource; the outputs locals.
    let desc = unsafe { resource.GetDesc() };
    let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
        && desc.Flags.0 == 0
        && v.dense_private <= desc.Width
        && v.dense_copied <= desc.Width - v.dense_private
        && unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok()
        && heap.Type == D3D12_HEAP_TYPE_DEFAULT
}
/// Completion: the certified, finalized completion of this occurrence on a DIRECT queue of `device` (done, when asked).
fn completion(v: &Version, device: &ID3D12Device, require_done: bool) -> bool {
    if v.completion_scope != v.scope || !v.finalized || !v.certified || v.value == 0 {
        return false;
    }
    let (Some(fence), Some(queue)) = (&v.fence, &v.queue) else { return false };
    // SAFETY: a live queue.
    if !same_device(device, Some(fence)) || !same_device(device, Some(queue)) || unsafe { queue.GetDesc() }.Type != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return false;
    }
    // SAFETY: a live fence.
    let done = || unsafe { fence.GetCompletedValue() };
    !require_done || { let value = done(); value != u64::MAX && value >= v.value }
}
/// View: the version's private SRV.
fn view(v: &Version) -> D3D12_SHADER_RESOURCE_VIEW_DESC {
    D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_UNKNOWN,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV { FirstElement: v.mapping.private_first_element, NumElements: VIEW_ELEMENTS as u32, StructureByteStride: VIEW_STRIDE as u32, Flags: D3D12_BUFFER_SRV_FLAGS(0) },
        },
    }
}

impl Plan {
    /// BuildPlan's checks before the read plan's: the scope (distinct), the original view, the private b4.
    pub fn admit(&self, i: &Input) -> u32 {
        if !valid_view_scope(&i.scope) || i.b4_scope != i.scope || i.completion_scope != i.scope || self.versions.iter().any(|v| v.scope == i.scope) {
            return failure::SCOPE;
        }
        if i.first_element != 0 || u64::from(i.num_elements) != VIEW_ELEMENTS || u64::from(i.stride) != VIEW_STRIDE || i.resource_width != DOMAIN_BYTES || !i.plan_view {
            return failure::VIEW;
        }
        if !i.b4_verified || !i.bounded || i.b4_bytes != B4_BYTES || i.b4.is_null() || bytes(i.b4, B4_BYTES) != bytes(i.plan_b4, B4_BYTES) {
            return failure::B4;
        }
        failure::NONE
    }
    /// BuildPlan's checks after the read plan's: the owners, the device, the completion, the reservation.
    pub fn push(&mut self, i: &Input, read: &Read, budget: u64) -> u32 {
        let mut b4 = [0u8; B4_BYTES];
        b4.copy_from_slice(bytes(i.b4, B4_BYTES));
        // SAFETY: the .cpp lends the checked plan's records.
        let records = if read.records.is_null() || read.count == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(read.records, read.count) }.to_vec() };
        let mut v = Version {
            scope: i.scope,
            mapping: ViewMapping::default(),
            dense: borrowed(i.dense),
            dense_owner: borrowed(i.dense_owner),
            dense_source: i.dense_source,
            dense_private: i.dense_private,
            dense_copied: i.dense_copied,
            copy_owner: borrowed(i.copy_owner),
            copy_list: i.copy_list,
            copy_generation: i.copy_generation,
            copy_occurrence: i.copy_occurrence,
            copy_source: i.copy_source,
            copy_private: i.copy_private,
            copy_copied: i.copy_copied,
            completion_scope: i.completion_scope,
            queue: borrowed(i.queue),
            fence: borrowed(i.fence),
            value: i.value,
            certified: i.certified,
            finalized: i.finalized,
            resource_offset: read.resource_offset,
            byte_extent: read.byte_extent,
            first_record: read.first_record,
            record_span: read.record_span,
            records,
            b4,
        };
        if !dense_range(&v) {
            return failure::OWNERS;
        }
        let mut device: Option<ID3D12Device> = None;
        // SAFETY: the dense buffer is live (DenseRange held it).
        if unsafe { v.dense.as_ref().unwrap().GetDevice(&mut device) }.is_err() {
            return failure::DEVICE;
        }
        let Some(device) = device else { return failure::DEVICE };
        if !completion(&v, &device, false) {
            return failure::COMPLETION;
        }
        let cursor = self.versions.last().map_or(0, |last| last.mapping.next_free);
        let Some(mapping) = reserve_records(cursor, v.first_record, v.byte_extent, budget) else { return failure::CAPACITY };
        v.mapping = mapping;
        self.width = self.width.max(mapping.view_end);
        self.copied += mapping.copy_bytes;
        self.versions.push(v);
        failure::NONE
    }
    /// AdoptSources: the source copies at their original positions, every source fence completed, one recording per
    /// backing buffer and its ranges in order.
    pub fn adopt(&self, device: &ID3D12Device) -> Result<Plan, u32> {
        let mut plan = Plan { versions: self.versions.clone(), width: 0, copied: self.copied };
        struct Backing {
            resource: *mut c_void,
            scope: ViewScope,
            end: u64,
            width: u64,
        }
        let mut backings: Vec<Backing> = Vec::new();
        for v in &mut plan.versions {
            if !completion(v, device, true) {
                return Err(failure::COMPLETION);
            }
            let resource = v.dense.clone().filter(|r| same_device(device, Some(r))).ok_or(failure::DEVICE)?;
            let at = match backings.iter().position(|b| b.resource == resource.as_raw()) {
                Some(at) => at,
                None => {
                    backings.push(Backing { resource: resource.as_raw(), scope: ViewScope::default(), end: 0, width: 0 });
                    backings.len() - 1
                }
            };
            let backing = &mut backings[at];
            if backing.scope.list != 0 && (backing.scope.list != v.scope.list || backing.scope.generation != v.scope.generation || backing.scope.eye != v.scope.eye) {
                return Err(failure::SCOPE);
            }
            backing.scope = v.scope;
            if v.dense_private < v.dense_source || v.dense_private < backing.end {
                return Err(failure::RANGE);
            }
            // SAFETY: a live resource.
            let (first, width) = (v.dense_private - v.dense_source, unsafe { resource.GetDesc() }.Width);
            if first % VIEW_STRIDE != 0 || first > width || DOMAIN_BYTES > width - first || width > MAX_BYTES {
                return Err(failure::RANGE);
            }
            v.mapping = ViewMapping {
                target_offset: v.dense_private,
                private_first_element: first / VIEW_STRIDE,
                copy_bytes: v.dense_copied,
                next_free: v.dense_private + v.dense_copied,
                view_end: first + DOMAIN_BYTES,
            };
            backing.end = v.mapping.next_free;
            backing.width = backing.width.max(v.mapping.view_end);
        }
        plan.width = backings.iter().map(|b| b.width).sum();
        Ok(plan)
    }
    /// Create: the overlay's DEFAULT buffer of the plan's width (COPY_DEST), every version still admitted.
    pub fn create(&self, device: &ID3D12Device) -> Result<ID3D12Resource, i32> {
        // SAFETY: a live device.
        if self.versions.is_empty() || self.width == 0 || self.width > MAX_BYTES || unsafe { device.GetNodeCount() } != 1 {
            return Err(E_INVALIDARG);
        }
        if self.versions.iter().any(|v| !dense_range(v) || !same_device(device, v.dense.as_ref()) || !completion(v, device, false)) {
            return Err(E_INVALIDARG);
        }
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: self.width,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, CreationNodeMask: 1, VisibleNodeMask: 1, ..Default::default() };
        let mut out: Option<ID3D12Resource> = None;
        // SAFETY: a live device; the descriptions locals.
        unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut out) }.map_err(|e| e.code().0)?;
        out.ok_or(E_FAIL)
    }
    /// Record's checks: a certified current DIRECT recording of the overlay's device, not yet recorded, every source
    /// unchanged, finalized and completed.
    pub fn record_check(&self, device: &ID3D12Device, list: &ID3D12GraphicsCommandList, d: &Destination, created: bool, target: *mut c_void) -> u32 {
        let (allocator, queue) = (borrowed::<ID3D12CommandAllocator>(d.allocator), borrowed::<ID3D12CommandQueue>(d.queue));
        let (Some(allocator), Some(queue)) = (allocator, queue) else { return failure::RECORDING };
        // SAFETY: a live list and queue.
        if !d.certified
            || d.generation == 0
            || d.list != list.as_raw() as u64
            || unsafe { list.GetType() } != D3D12_COMMAND_LIST_TYPE_DIRECT
            || unsafe { queue.GetDesc() }.Type != D3D12_COMMAND_LIST_TYPE_DIRECT
            || !same_device(device, Some(list))
            || !same_device(device, Some(&allocator))
            || !same_device(device, Some(&queue))
        {
            return failure::RECORDING;
        }
        if !created {
            return failure::ALREADY_RECORDED;
        }
        for v in &self.versions {
            if v.dense.as_ref().map(|r| r.as_raw()) == Some(target) || !dense_range(v) {
                return failure::CHANGED;
            }
            if !completion(v, device, true) {
                return failure::COMPLETION;
            }
        }
        failure::NONE
    }
    /// Record's commands: each selected span copied into the overlay, which finishes PS_RESOURCE | COPY_SOURCE.
    pub fn record(&self, list: &ID3D12GraphicsCommandList, target: &ID3D12Resource) {
        for v in &self.versions {
            if let Some(dense) = &v.dense {
                // SAFETY: a live list and buffers (the record check held them).
                unsafe { list.CopyBufferRegion(target, v.mapping.target_offset, dense, v.dense_private, v.mapping.copy_bytes) };
            }
        }
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    // SAFETY: the target outlives the call; the barrier only borrows it.
                    pResource: unsafe { core::mem::transmute_copy(target) },
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: D3D12_RESOURCE_STATE_COPY_DEST,
                    StateAfter: D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE | D3D12_RESOURCE_STATE_COPY_SOURCE,
                }),
            },
        };
        // SAFETY: a live list; one barrier.
        unsafe { list.ResourceBarrier(&[barrier]) };
    }
    /// GetView: version `index` as a private view (its owners' pointers left to the .cpp); its dense buffer into `dense`.
    pub fn view(&self, index: u32, adopted: bool, list: u64, generation: u64) -> Option<(PrivateView, *mut c_void)> {
        let v = self.versions.get(index as usize)?;
        if v.records.len() > MAX_RECORDS {
            return None;
        }
        let mut records = [0u32; MAX_RECORDS];
        records[..v.records.len()].copy_from_slice(&v.records);
        let out = PrivateView {
            scope: v.scope,
            resource: core::ptr::null_mut(),
            owner: core::ptr::null_mut(),
            srv: view(v),
            source_offset: v.dense_source,
            dense_offset: v.dense_private,
            target_offset: v.mapping.target_offset,
            copied_bytes: v.mapping.copy_bytes,
            copy_list: if adopted { v.scope.list } else { list },
            copy_generation: if adopted { v.scope.generation } else { generation },
            version: index,
            first_record: v.first_record,
            record_span: v.record_span,
            record_count: v.records.len() as u32,
            records,
            b4: v.b4,
        };
        Some((out, v.dense.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw())))
    }
    /// ValidatePrivateView's association: the view is version `view.version` of this overlay, as GetView exposed it.
    pub fn matches(&self, p: &PrivateView, adopted: bool, target: *mut c_void, list: u64, generation: u64) -> bool {
        let Some((expected, dense)) = self.view(p.version, adopted, list, generation) else { return false };
        p.resource == if adopted { dense } else { target }
            && p.scope == expected.scope
            && same_buffer_view(&p.srv, &expected.srv)
            && p.source_offset == expected.source_offset
            && p.dense_offset == expected.dense_offset
            && p.target_offset == expected.target_offset
            && p.copied_bytes == expected.copied_bytes
            && p.first_record == expected.first_record
            && p.record_span == expected.record_span
            && p.record_count == expected.record_count
            && p.records == expected.records
            && p.b4 == expected.b4
            && p.copy_list == expected.copy_list
            && p.copy_generation == expected.copy_generation
    }
    pub fn mapping(&self, index: u32) -> Option<&ViewMapping> {
        self.versions.get(index as usize).map(|v| &v.mapping)
    }
    pub fn scope(&self, index: u32) -> Option<&ViewScope> {
        self.versions.get(index as usize).map(|v| &v.scope)
    }
    pub fn count(&self) -> usize {
        self.versions.len()
    }
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// A new, empty plan: one Arc reference (the C++ PlanData's; a created overlay holds another).
#[no_mangle]
pub extern "C-unwind" fn cvr_sov_plan_new() -> *mut Plan {
    std::sync::Arc::into_raw(std::sync::Arc::new(Plan::default())).cast_mut()
}
/// Releases a plan's reference (null: nothing).
///
/// # Safety
/// From cvr_sov_plan_new, once.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_plan_free(plan: *mut Plan) {
    if !plan.is_null() {
        // SAFETY: as documented.
        drop(unsafe { std::sync::Arc::from_raw(plan.cast_const()) });
    }
}
/// The plan behind a C++ PlanData's pointer, for its builder alone (null while it is shared).
///
/// # Safety
/// From cvr_sov_plan_new and live.
unsafe fn exclusive<'a>(plan: *mut Plan) -> Option<&'a mut Plan> {
    // SAFETY: as documented; the reference is given back below.
    let mut arc = core::mem::ManuallyDrop::new(unsafe { std::sync::Arc::from_raw(plan.cast_const()) });
    // SAFETY: the plan outlives the call (the caller's reference); unique while no overlay holds it.
    std::sync::Arc::get_mut(&mut arc).map(|p| unsafe { &mut *(p as *mut Plan) })
}
/// BuildPlan's checks before the read plan's (a Failure).
///
/// # Safety
/// A live plan; the input's lent objects and bytes live for the call.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_admit(plan: *const Plan, input: *const Input) -> u32 {
    // SAFETY: as documented.
    unsafe { (*plan).admit(&*input) }
}
/// BuildPlan's checks after the read plan's, the version appended (a Failure).
///
/// # Safety
/// As cvr_sov_admit; `read` lends the checked plan's records.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_push(plan: *mut Plan, input: *const Input, read: *const Read, budget: u64) -> u32 {
    // SAFETY: as documented (the plan is still the builder's).
    match unsafe { exclusive(plan) } {
        Some(plan) => unsafe { plan.push(&*input, &*read, budget) },
        None => 15,
    }
}
/// AdoptSources: `checked` adopted into the empty `out` (a Failure).
///
/// # Safety
/// Live plans; `device` live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_adopt(checked: *const Plan, device: *mut c_void, out: *mut Plan) -> u32 {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return failure::DEVICE };
    // SAFETY: as documented.
    match unsafe { (*checked).adopt(&device) } {
        // SAFETY: as documented (`out` is still the builder's).
        Ok(plan) => match unsafe { exclusive(out) } {
            Some(out) => {
                *out = plan;
                failure::NONE
            }
            None => 15,
        },
        Err(code) => code,
    }
}
/// Create: the overlay's buffer into `out` (its creation's reference).
///
/// # Safety
/// A live plan; `device` null or live; `out` a pointer.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_create(device: *mut c_void, plan: *const Plan, out: *mut *mut c_void) -> i32 {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    // SAFETY: as documented.
    match unsafe { (*plan).create(&device) } {
        Ok(resource) => {
            // SAFETY: as documented.
            unsafe { *out = resource.into_raw() };
            0
        }
        Err(hr) => hr,
    }
}
/// Record's checks (a Failure).
///
/// # Safety
/// A live plan, device and list; the destination's objects null or live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_record_check(device: *mut c_void, list: *mut c_void, destination: *const Destination, created: bool, plan: *const Plan, target: *mut c_void) -> u32 {
    let (Some(device), Some(list)) = (borrowed::<ID3D12Device>(device), borrowed::<ID3D12GraphicsCommandList>(list)) else { return failure::RECORDING };
    // SAFETY: as documented.
    unsafe { (*plan).record_check(&device, &list, &*destination, created, target) }
}
/// Record's commands.
///
/// # Safety
/// After cvr_sov_record_check admitted them.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_record(list: *mut c_void, plan: *const Plan, target: *mut c_void) {
    if let (Some(list), Some(target)) = (borrowed::<ID3D12GraphicsCommandList>(list), borrowed::<ID3D12Resource>(target)) {
        // SAFETY: as documented.
        unsafe { (*plan).record(&list, &target) }
    }
}
/// GetView into `out` (its owners' pointers untouched); the version's dense buffer into `dense`.
///
/// # Safety
/// A live plan; `out` a zeroed view, `dense` a pointer.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_view(plan: *const Plan, index: u32, adopted: bool, list: u64, generation: u64, out: *mut PrivateView, dense: *mut *mut c_void) -> bool {
    // SAFETY: as documented.
    let Some((view, resource)) = (unsafe { (*plan).view(index, adopted, list, generation) }) else { return false };
    // SAFETY: as documented: the owners' fields are written as they were (null).
    unsafe {
        let (owner, held) = ((*out).owner, (*out).resource);
        *out = PrivateView { owner, resource: held, ..view };
        *dense = resource;
    }
    true
}
/// ValidatePrivateView's association.
///
/// # Safety
/// A live plan and view.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_matches(plan: *const Plan, view: *const PrivateView, adopted: bool, target: *mut c_void, list: u64, generation: u64) -> bool {
    // SAFETY: as documented.
    unsafe { (*plan).matches(&*view, adopted, target, list, generation) }
}
/// Plan::VersionCount (`which` 0), RequiredBytes (1), CopiedBytes (2).
///
/// # Safety
/// A live plan.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_counts(plan: *const Plan, which: u32) -> u64 {
    // SAFETY: as documented.
    let plan = unsafe { &*plan };
    match which {
        0 => plan.count() as u64,
        1 => plan.width,
        _ => plan.copied,
    }
}
/// Plan::VersionMapping (`which` 0) or VersionScope (1): a pointer into the plan, null past its versions.
///
/// # Safety
/// A live plan.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sov_version(plan: *const Plan, index: u32, which: u32) -> *const c_void {
    // SAFETY: as documented.
    let plan = unsafe { &*plan };
    let found = if which == 0 { plan.mapping(index).map(|m| m as *const ViewMapping as *const c_void) } else { plan.scope(index).map(|s| s as *const ViewScope as *const c_void) };
    found.unwrap_or(core::ptr::null())
}

#[cfg(test)]
mod tests {
    //! A plan of two versions on WARP: the admission's refusals, the reservation, adoption, the overlay's buffer, the
    //! record check and commands, the views and their validation.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    fn warp() -> ID3D12Device {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    fn buffer(device: &ID3D12Device, bytes: u64) -> ID3D12Resource {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: bytes,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let mut out: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_SOURCE, None, &mut out) }.unwrap();
        out.unwrap()
    }
    struct Fixture {
        device: ID3D12Device,
        dense: ID3D12Resource,
        queue: ID3D12CommandQueue,
        fence: ID3D12Fence,
        b4: [u8; B4_BYTES],
    }
    fn input(f: &Fixture, occurrence: u64, private: u64) -> Input {
        let scope = ViewScope { root: 1, list: 2, generation: 3, occurrence, eye: 0 };
        let owner = f.dense.as_raw();
        Input {
            scope,
            b4_scope: scope,
            completion_scope: scope,
            dense: f.dense.as_raw(),
            dense_owner: owner,
            dense_source: 256,
            dense_private: private,
            dense_copied: 512,
            copy_owner: owner,
            copy_list: 2,
            copy_generation: 3,
            copy_occurrence: occurrence,
            copy_source: 256,
            copy_private: private,
            copy_copied: 512,
            queue: f.queue.as_raw(),
            fence: f.fence.as_raw(),
            value: 1,
            first_element: 0,
            resource_width: DOMAIN_BYTES,
            num_elements: VIEW_ELEMENTS as u32,
            stride: VIEW_STRIDE as u32,
            b4: f.b4.as_ptr(),
            b4_bytes: B4_BYTES,
            plan_b4: f.b4.as_ptr(),
            certified: true,
            finalized: true,
            b4_verified: true,
            bounded: true,
            plan_view: true,
        }
    }
    const RECORDS: [u32; 2] = [2, 5];
    fn read() -> Read {
        Read { resource_offset: 256, byte_extent: 512, first_record: 2, record_span: 4, records: RECORDS.as_ptr(), count: 2 }
    }
    #[test]
    fn plan_and_overlay() {
        let device = warp();
        let f = Fixture {
            dense: buffer(&device, 2 * 1024 * 1024),
            queue: unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }) }.unwrap(),
            fence: unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap(),
            b4: core::array::from_fn(|i| i as u8),
            device,
        };
        let mut plan = Plan::default();
        let first = input(&f, 7, 256);
        assert_eq!(plan.admit(&first), failure::NONE);
        assert_eq!(plan.push(&first, &read(), MAX_BYTES), failure::NONE);
        assert_eq!(plan.admit(&first), failure::SCOPE, "an occurrence twice");
        let mut other = [0u8; B4_BYTES];
        other[3] = 1;
        assert_eq!(plan.admit(&Input { plan_b4: other.as_ptr(), ..input(&f, 8, 1024 * 1024 + 256) }), failure::B4, "another b4");
        assert_eq!(plan.admit(&Input { stride: 64, ..input(&f, 8, 1024 * 1024 + 256) }), failure::VIEW);
        assert_eq!(plan.push(&Input { dense_private: 1024 * 1024 + 300, ..input(&f, 8, 1024 * 1024 + 256) }, &read(), MAX_BYTES), failure::OWNERS, "unaligned");
        assert_eq!(plan.push(&Input { copy_occurrence: 9, ..input(&f, 8, 1024 * 1024 + 256) }, &read(), MAX_BYTES), failure::OWNERS, "another copy");
        assert_eq!(plan.push(&Input { finalized: false, ..input(&f, 8, 1024 * 1024 + 256) }, &read(), MAX_BYTES), failure::COMPLETION);
        let second = input(&f, 8, 1024 * 1024 + 256);
        assert_eq!(plan.push(&second, &read(), MAX_BYTES), failure::NONE);
        assert_eq!((plan.count(), plan.copied), (2, 1024));
        let (a, b) = (*plan.mapping(0).unwrap(), *plan.mapping(1).unwrap());
        assert_eq!((a.target_offset, a.private_first_element, a.copy_bytes), (256, 0, 512), "the records at their original index");
        assert!(b.target_offset >= a.next_free && b.target_offset % VIEW_STRIDE == 0 && plan.width == b.view_end);
        assert_eq!(plan.adopt(&f.device).err(), Some(failure::COMPLETION), "the source fence pending");
        unsafe { f.queue.Signal(&f.fence, 1) }.unwrap();
        while unsafe { f.fence.GetCompletedValue() } < 1 {}
        let adopted = plan.adopt(&f.device).unwrap();
        let m = *adopted.mapping(1).unwrap();
        assert_eq!((m.target_offset, m.private_first_element, m.view_end), (1024 * 1024 + 256, 1024 * 1024 / VIEW_STRIDE, 2 * 1024 * 1024));
        assert_eq!(adopted.width, 2 * 1024 * 1024, "one backing: the widest view");
        let overlay = plan.create(&f.device).unwrap();
        assert_eq!(unsafe { overlay.GetDesc() }.Width, plan.width);
        let allocator: ID3D12CommandAllocator = unsafe { f.device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { f.device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let mut d = Destination { list: list.as_raw() as u64, generation: 4, allocator: allocator.as_raw(), queue: f.queue.as_raw(), certified: false };
        assert_eq!(plan.record_check(&f.device, &list, &d, true, overlay.as_raw()), failure::RECORDING, "uncertified");
        d.certified = true;
        assert_eq!(plan.record_check(&f.device, &list, &d, false, overlay.as_raw()), failure::ALREADY_RECORDED);
        assert_eq!(plan.record_check(&f.device, &list, &d, true, f.dense.as_raw()), failure::CHANGED, "the overlay is a source");
        assert_eq!(plan.record_check(&f.device, &list, &d, true, overlay.as_raw()), failure::NONE);
        plan.record(&list, &overlay);
        unsafe { list.Close() }.unwrap();
        let (mut view, dense) = plan.view(1, false, d.list, d.generation).unwrap();
        assert_eq!(dense, f.dense.as_raw());
        assert_eq!((view.copy_list, view.copy_generation, view.record_count, view.records[1], view.b4[5]), (d.list, 4, 2, 5, 5));
        assert_eq!(unsafe { view.srv.Anonymous.Buffer }.FirstElement, b.private_first_element);
        view.resource = overlay.as_raw();
        assert!(plan.matches(&view, false, overlay.as_raw(), d.list, d.generation));
        view.scope.occurrence += 1;
        assert!(!plan.matches(&view, false, overlay.as_raw(), d.list, d.generation), "another occurrence");
        view.scope.occurrence -= 1;
        view.b4[0] ^= 1;
        assert!(!plan.matches(&view, false, overlay.as_raw(), d.list, d.generation), "another b4");
        let (adopted_view, _) = adopted.view(0, true, 0, 0).unwrap();
        assert_eq!((adopted_view.copy_list, adopted_view.copy_generation), (2, 3), "adopted: the source's recording");
    }
}
