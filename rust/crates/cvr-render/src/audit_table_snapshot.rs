//! The table metadata snapshot (formerly src/Render/StereoDescriptorAudit.cpp's TableMetadataSnapshot::Data,
//! CaptureTableMetadata, ValidateTableMetadataNoOverwrite and DescribeTableMetadata): the declared ranges of a root's
//! supplied tables over its held shader-visible heaps, with their canonical values, captured under one borrowed-range
//! pin and the metadata lock; physically re-checked (each heap's shape on its device) and checked for no overwrite
//! (root layout, heap cookies and ranges, every range's identity at the pinned epoch) before it is described.
//!
//! The snapshot holds its root, device and heaps (COM references), never a resource. The .cpp keeps the public class
//! (its pimpl now this handle) and copies the summary out.

use crate::audit_heaps::HeapShape;
use crate::audit_metadata::{Heap, Range, Walk, WalkView};
use crate::descriptor_audit as core_;
use crate::descriptor_store::Value;
use crate::source_layout::Binding;
use core::cell::Cell;
use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// TableMetadataFailure.
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const COVERAGE: u32 = 2;
    pub const ROOT_LAYOUT: u32 = 3;
    pub const HEAP: u32 = 4;
    pub const CHANGED: u32 = 7;
    pub const EXCEPTION: u32 = 8;
}
thread_local! {
    static FAILURE: Cell<u32> = const { Cell::new(0) };
}
fn set(value: u32) {
    FAILURE.with(|f| f.set(value));
}
pub fn last_failure() -> u32 {
    FAILURE.with(Cell::get)
}

struct SnapHeap {
    object: ID3D12DescriptorHeap,
    desc: D3D12_DESCRIPTOR_HEAP_DESC,
    cookie: u64,
    cpu: u64,
    gpu: u64,
    stride: u32,
}
impl SnapHeap {
    fn lent(&self) -> Heap {
        Heap { kind: self.desc.Type.0 as u32, count: self.desc.NumDescriptors, cookie: self.cookie, cpu: self.cpu, gpu: self.gpu, stride: self.stride }
    }
    fn current_cookie(&self) -> u64 {
        self.object.cast::<ID3D12Object>().map_or(0, |o| core_::object_cookie(&o))
    }
}
/// TableMetadataSummary's scalars and its two arrays, as the .cpp copies them out.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Summary {
    pub root: u64,
    pub tracking_epoch: u64,
    pub root_parameters: u32,
    pub root_flags: u32,
    pub layout_reasons: u32,
    pub bound_tables: u32,
    pub unbound_tables: u32,
    pub unknown_flags: u32,
    pub unknown_slots: u64,
    pub complete: bool,
    pub ranges: *const Range,
    pub range_count: usize,
    pub values: *const Value,
    pub value_count: usize,
}
const _: () = assert!(core::mem::size_of::<Summary>() == 88);
/// TableMetadataSnapshot::Data.
pub struct Snapshot {
    root: ID3D12RootSignature,
    device: ID3D12Device,
    heaps: Vec<SnapHeap>,
    ranges_layout: Vec<Binding>,
    supported: bool,
    parameters: u32,
    reasons: u32,
    flags: u32,
    summary: Summary,
    ranges: Vec<Range>,
    values: Vec<Value>,
    pinned: bool,
}
impl Drop for Snapshot {
    fn drop(&mut self) {
        if self.pinned {
            core_::tracking_call(3, 0);
        }
    }
}
impl Snapshot {
    fn physical(&self) -> bool {
        self.heaps.iter().all(|heap| {
            let d = &heap.desc;
            let shape = HeapShape { kind: d.Type.0 as u32, count: d.NumDescriptors, flags: d.Flags.0 as u32, node_mask: d.NodeMask, cpu: heap.cpu, gpu: heap.gpu, stride: heap.stride, _pad: 0 };
            // SAFETY: the held device and heap.
            unsafe { crate::audit_heaps::cvr_sda_same_heap(self.device.as_raw(), heap.object.as_raw(), &shape) }
        })
    }
    /// MatchesLocked: under the metadata lock.
    fn matches_locked(&self) -> bool {
        let epoch = self.summary.tracking_epoch;
        if !self.pinned || !core_::tracking_call(6, epoch) || !core_::track_versions() || core_::overflow() || core_::tracking_failure() != 0 {
            return false;
        }
        if self.heaps.len() > 2 {
            return false;
        }
        let (mut lent, mut current) = ([Heap::default(); 2], [0u64; 2]);
        for (i, heap) in self.heaps.iter().enumerate() {
            current[i] = heap.current_cookie();
            lent[i] = heap.lent();
        }
        // SAFETY: our arrays, the store under the lock, the held root.
        let matches = unsafe {
            crate::audit_snapshots::cvr_sda_metadata_matches(
                core_::store(),
                self.root.as_raw(),
                self.ranges_layout.as_ptr(),
                self.ranges_layout.len(),
                self.parameters,
                self.flags,
                self.reasons,
                self.supported,
                lent.as_ptr(),
                current.as_ptr(),
                self.heaps.len(),
                self.ranges.as_ptr(),
                self.ranges.len(),
            )
        };
        matches && core_::tracking_call(6, epoch)
    }
    fn describe(&self) -> Summary {
        Summary { ranges: self.ranges.as_ptr(), range_count: self.ranges.len(), values: self.values.as_ptr(), value_count: self.values.len(), ..self.summary }
    }
}
/// BoundTableMetadata.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bound {
    pub parameter: u32,
    pub table: u64,
}
const _: () = assert!(core::mem::size_of::<Bound>() == 16);
struct WalkGuard(*mut Walk);
impl Drop for WalkGuard {
    fn drop(&mut self) {
        // SAFETY: the walk's own handle (null frees nothing).
        unsafe { crate::audit_metadata::cvr_sda_metadata_free(self.0) };
    }
}
fn borrow_raw<T: Interface>(raw: &*mut c_void) -> Option<T> {
    // SAFETY: a live object the caller holds.
    unsafe { T::from_raw_borrowed(raw) }.cloned()
}

/// CaptureTableMetadata.
pub fn capture(root: *mut c_void, heaps: &[*mut c_void], bound: &[Bound]) -> Option<Box<Snapshot>> {
    set(failure::ARGUMENTS);
    if root.is_null() || heaps.len() > 2 || bound.len() > 64 || (!bound.is_empty() && heaps.is_empty()) {
        return None;
    }
    let (mut tables, mut bound_mask) = ([0u64; 64], 0u64);
    for binding in bound {
        if binding.parameter >= 64 || binding.table == 0 || bound_mask & (1 << binding.parameter) != 0 {
            return None;
        }
        bound_mask |= 1 << binding.parameter;
        tables[binding.parameter as usize] = binding.table;
    }
    set(failure::COVERAGE);
    if !core_::enabled() || !core_::tracking_call(1, 0) {
        return None;
    }
    let root_object: ID3D12RootSignature = borrow_raw(&root)?;
    set(failure::HEAP);
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: the held root.
    unsafe { root_object.GetDevice(&mut device) }.ok()?;
    let device = device?;
    if unsafe { device.GetNodeCount() } != 1 {
        return None;
    }
    let mut held: Vec<SnapHeap> = Vec::with_capacity(heaps.len());
    for &object in heaps {
        let mut shape = HeapShape::default();
        // SAFETY: the held device and heap.
        if object.is_null() || !unsafe { crate::audit_heaps::cvr_sda_shader_heap(device.as_raw(), object, crate::descriptor_store::SLOT_LIMIT as u32, true, &mut shape) } {
            return None;
        }
        let object: ID3D12DescriptorHeap = borrow_raw(&object)?;
        let desc = unsafe { object.GetDesc() };
        if held.iter().any(|prior| prior.object.as_raw() == object.as_raw() || prior.desc.Type == desc.Type) {
            return None;
        }
        held.push(SnapHeap { object, desc, cookie: 0, cpu: shape.cpu, gpu: shape.gpu, stride: shape.stride });
    }
    // SAFETY: an all-zero summary is its default.
    let mut next = Box::new(Snapshot {
        root: root_object,
        device,
        heaps: held,
        ranges_layout: Vec::new(),
        supported: false,
        parameters: 0,
        reasons: 0,
        flags: 0,
        summary: unsafe { core::mem::zeroed() },
        ranges: Vec::new(),
        values: Vec::new(),
        pinned: false,
    });
    {
        let _g = core_::guard();
        core_::drain_retired();
        set(failure::COVERAGE);
        if !core_::tracking_call(2, 0) {
            return None;
        }
        next.pinned = true;
        core_::select_storage();
        next.summary.root = root as u64;
        let epoch = core_::tracking().map_or(0, |t| t.epoch.load(core::sync::atomic::Ordering::Acquire));
        next.summary.tracking_epoch = epoch;
        if !core_::tracking_call(6, epoch) || !core_::track_versions() || core_::overflow() || core_::tracking_failure() != 0 {
            return None;
        }
        set(failure::ROOT_LAYOUT);
        let found = core_::root_view(root as u64)?;
        if found.parameters > 64 || found.count > 256 || found.reasons & !2 != 0 {
            return None;
        }
        next.ranges_layout = if found.count == 0 {
            Vec::new()
        } else {
            // SAFETY: the root's layout, stable under the lock.
            unsafe { core::slice::from_raw_parts(found.ranges, found.count) }.to_vec()
        };
        (next.supported, next.parameters, next.reasons, next.flags) = (found.supported, found.parameters, found.reasons, found.flags);
        if next.parameters < 64 && bound_mask >> next.parameters != 0 {
            return None;
        }
        let s = &mut next.summary;
        (s.root_parameters, s.root_flags, s.layout_reasons, s.bound_tables) = (found.parameters, found.flags, found.reasons, bound.len() as u32);
        let mut lent = [Heap::default(); 2];
        for (i, heap) in next.heaps.iter_mut().enumerate() {
            heap.cookie = heap.current_cookie();
            lent[i] = heap.lent();
        }
        // SAFETY: zeroed output the walk fills.
        let mut walk: WalkView = unsafe { core::mem::zeroed() };
        let outcome = unsafe {
            crate::audit_metadata::cvr_sda_metadata_walk(
                core_::store(),
                next.ranges_layout.as_ptr(),
                next.ranges_layout.len(),
                next.parameters,
                next.flags,
                next.reasons,
                bound_mask,
                tables.as_ptr(),
                lent.as_ptr(),
                next.heaps.len(),
                &mut walk,
            )
        };
        let _walk = WalkGuard(walk.handle);
        if outcome != 0 {
            set(outcome);
            return None;
        }
        let copy = |p: *const Range, n: usize| if n == 0 || p.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(p, n) }.to_vec() };
        next.ranges = copy(walk.ranges, walk.range_count);
        next.values = if walk.value_count == 0 || walk.values.is_null() { Vec::new() } else { unsafe { core::slice::from_raw_parts(walk.values, walk.value_count) }.to_vec() };
        let s = &mut next.summary;
        (s.unknown_flags, s.unbound_tables, s.unknown_slots) = (walk.unknown_flags, walk.unbound_tables, walk.unknown_slots);
        s.complete = s.unknown_flags == 0;
        set(failure::CHANGED);
        if !next.matches_locked() {
            return None;
        }
    }
    set(failure::HEAP);
    if !next.physical() {
        return None;
    }
    {
        let _g = core_::guard();
        core_::drain_retired();
        set(failure::CHANGED);
        if !next.matches_locked() {
            return None;
        }
    }
    set(failure::NONE);
    Some(next)
}
/// ValidateTableMetadataNoOverwrite.
pub fn validate(held: &Snapshot) -> bool {
    set(failure::HEAP);
    if !held.physical() {
        return false;
    }
    let _g = core_::guard();
    core_::drain_retired();
    set(failure::CHANGED);
    if !held.matches_locked() {
        return false;
    }
    set(failure::NONE);
    true
}

// ---- the C ABI
/// # Safety
/// `heaps` holds `heap_count` heaps and `bound` `bound_count` tables; the caller holds the root and heaps.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_tm_capture(root: *mut c_void, heaps: *const *mut c_void, heap_count: usize, bound: *const Bound, bound_count: usize) -> *mut Snapshot {
    let heaps = if heap_count == 0 || heaps.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(heaps, heap_count) } };
    let bound = if bound_count == 0 || bound.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(bound, bound_count) } };
    if (heap_count != 0 && heaps.is_empty()) || (bound_count != 0 && bound.is_empty()) {
        set(failure::ARGUMENTS);
        return core::ptr::null_mut();
    }
    capture(root, heaps, bound).map_or(core::ptr::null_mut(), Box::into_raw)
}
/// # Safety
/// A capture's handle (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_tm_free(handle: *mut Snapshot) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}
/// # Safety
/// A live capture's handle (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_tm_validate(handle: *const Snapshot) -> bool {
    set(failure::ARGUMENTS);
    unsafe { handle.as_ref() }.is_some_and(validate)
}
/// The validated summary, its arrays the snapshot's own.
///
/// # Safety
/// A live capture's handle; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_tm_describe(handle: *const Snapshot, out: *mut Summary) -> bool {
    set(failure::ARGUMENTS);
    match unsafe { handle.as_ref() } {
        Some(held) if validate(held) => {
            unsafe { *out = held.describe() };
            true
        }
        _ => false,
    }
}
/// The thread's last failure; any `value` but u32::MAX sets it first (the .cpp's exception paths).
#[no_mangle]
pub extern "C-unwind" fn cvr_sda_tm_failure(value: u32) -> u32 {
    if value != u32::MAX {
        set(value);
    }
    last_failure()
}

#[cfg(test)]
mod tests {
    //! The argument and coverage refusals before any device or lock, the failure's thread-local.
    use super::*;

    #[test]
    fn refusals_and_failure() {
        let root = 0x1000 as *mut c_void;
        let bound = |parameter, table| Bound { parameter, table };
        assert!(capture(core::ptr::null_mut(), &[], &[]).is_none() && last_failure() == failure::ARGUMENTS);
        assert!(capture(root, &[root; 3], &[]).is_none() && last_failure() == failure::ARGUMENTS);
        assert!(capture(root, &[], &[bound(0, 0x40)]).is_none() && last_failure() == failure::ARGUMENTS); // tables without heaps
        assert!(capture(root, &[root], &[bound(64, 0x40)]).is_none() && last_failure() == failure::ARGUMENTS);
        assert!(capture(root, &[root], &[bound(3, 0)]).is_none() && last_failure() == failure::ARGUMENTS);
        assert!(capture(root, &[root], &[bound(3, 0x40), bound(3, 0x80)]).is_none() && last_failure() == failure::ARGUMENTS);
        // Unattached: the bindings are not observed, so a well-formed request stops at coverage.
        assert!(capture(root, &[root], &[bound(3, 0x40), bound(5, 0x80)]).is_none() && last_failure() == failure::COVERAGE);
        assert_eq!(cvr_sda_tm_failure(failure::EXCEPTION), failure::EXCEPTION);
        assert_eq!(cvr_sda_tm_failure(u32::MAX), failure::EXCEPTION);
        assert!(!unsafe { cvr_sda_tm_validate(core::ptr::null()) } && last_failure() == failure::ARGUMENTS);
        unsafe { cvr_sda_tm_free(core::ptr::null_mut()) };
    }
}
