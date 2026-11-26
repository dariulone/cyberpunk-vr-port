//! The borrowed source ranges (formerly src/Render/StereoDescriptorAudit.cpp's BorrowedSourceRanges::Data, the
//! capture after CaptureBorrowedSourceRanges's recording, bindings and family checks, and the held halves of
//! ValidateBorrowedSourceRanges, DescribeBorrowedSourceRanges and InventoryBorrowedSourceRange) and the thread's
//! borrowed stage with its details.
//!
//! A capture holds its device, list, root, pipeline and shader-visible heaps (COM references) and the ranges the
//! family's shader uses reach through the bound tables, planned against the root's layout and walked in the store
//! under one borrowed-range pin; it matches while the pin's epoch is stable, the heaps' cookies and every range's
//! identity unchanged. The descriptor inventory of a physical-remainder range is cached per identity (at most the
//! store's slot limit across the capture). The .cpp keeps the family, the recording lease and the bindings beside
//! this handle.

use crate::audit_borrowed::{Heap, Range, Stage};
use crate::audit_heaps::HeapShape;
use crate::descriptor_audit as core_;
use crate::descriptor_store::{same_borrowed_range, BorrowedDescriptorInventory, BorrowedRootRange, BorrowedShaderUse, BorrowedUsedRange, RangeIdentity, SLOT_LIMIT};
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::Ordering::Acquire;
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

thread_local! {
    static STAGE: Cell<u32> = const { Cell::new(0) };
    static DETAILS: Cell<[u64; 4]> = const { Cell::new([0; 4]) };
}
/// BorrowedStage: the thread's stage and its four details.
pub fn stage(value: u32, index: u64, gpu: u64, expected: u64, actual: u64) {
    STAGE.with(|s| s.set(value));
    DETAILS.with(|d| d.set([index, gpu, expected, actual]));
}
fn stage_at(value: u32) {
    stage(value, u64::MAX, 0, 0, 0);
}
fn epoch() -> u64 {
    core_::tracking().map_or(0, |t| t.epoch.load(Acquire))
}
/// borrowedDetails=BorrowedTrackingDetails(...): why the tracking refused, with its counters.
fn tracking_details(pinned: bool, saved: u64, empty: bool) -> [u64; 4] {
    let (pins, mutations) = core_::tracking().map_or((0, 0), |t| (t.pins.load(Acquire), t.mutations.load(Acquire)));
    let mut stats = [0u64; 6];
    let mut out = [0u64; 4];
    // SAFETY: our outputs; the store.
    unsafe {
        crate::descriptor_store::cvr_ds_stats(core_::store(), &mut stats);
        crate::descriptor_store::cvr_ds_tracking_details(
            pinned,
            core_::tracking_call(1, 0),
            saved,
            epoch(),
            pins,
            mutations,
            core_::track_versions(),
            stats[0] as u32,
            core_::tracking_failure(),
            empty,
            &mut out,
        )
    };
    out
}
fn set_details(details: [u64; 4]) {
    DETAILS.with(|d| d.set(details));
}

struct BorrowedHeap {
    object: ID3D12DescriptorHeap,
    desc: D3D12_DESCRIPTOR_HEAP_DESC,
    cookie: u64,
    cpu: u64,
    gpu: u64,
    stride: u32,
}
impl BorrowedHeap {
    fn lent(&self) -> Heap {
        Heap { object: self.object.as_raw(), kind: self.desc.Type.0 as u32, count: self.desc.NumDescriptors, cpu: self.cpu, gpu: self.gpu, stride: self.stride, cookie: self.cookie }
    }
}
struct Inventory {
    identity: RangeIdentity,
    counts: BorrowedDescriptorInventory,
    cookies: Vec<u64>,
}
/// BorrowedSourceRanges::Data's held half.
pub struct Borrowed {
    pinned: bool,
    epoch: u64,
    device: ID3D12Device,
    _list: ID3D12GraphicsCommandList,
    root: ID3D12RootSignature,
    pipeline: ID3D12PipelineState,
    heaps: Vec<BorrowedHeap>,
    ranges: Vec<Range>,
    /// The cached inventories and their slots, used under the metadata lock.
    inventory: Mutex<(Vec<Inventory>, u64)>,
}
impl Drop for Borrowed {
    fn drop(&mut self) {
        if self.pinned {
            core_::tracking_call(3, 0);
        }
    }
}
impl Borrowed {
    fn physical(&self) -> bool {
        stage_at(6);
        if self.heaps.is_empty() || self.heaps.len() > 2 {
            return false;
        }
        for (i, heap) in self.heaps.iter().enumerate() {
            stage(6, i as u64, heap.gpu, heap.cookie, 0);
            let d = &heap.desc;
            let shape = HeapShape { kind: d.Type.0 as u32, count: d.NumDescriptors, flags: d.Flags.0 as u32, node_mask: d.NodeMask, cpu: heap.cpu, gpu: heap.gpu, stride: heap.stride, _pad: 0 };
            // SAFETY: the held device and heap.
            if !unsafe { crate::audit_heaps::cvr_sda_same_heap(self.device.as_raw(), heap.object.as_raw(), &shape) } {
                return false;
            }
        }
        true
    }
    /// MatchesLocked: under the metadata lock.
    pub fn matches_locked(&self) -> bool {
        stage_at(2);
        if !core_::tracking_call(1, 0) {
            return false;
        }
        stage(7, u64::MAX, 0, self.epoch, epoch());
        if !self.pinned || !core_::tracking_call(6, self.epoch) || !core_::track_versions() || core_::overflow() || core_::tracking_failure() != 0 || self.ranges.is_empty() {
            set_details(tracking_details(self.pinned, self.epoch, self.ranges.is_empty()));
            return false;
        }
        if self.heaps.len() > 2 {
            return false;
        }
        let mut lent: [Heap; 2] = unsafe { core::mem::zeroed() };
        let mut current = [0u64; 2];
        for (i, heap) in self.heaps.iter().enumerate() {
            current[i] = heap.object.cast::<ID3D12Object>().map_or(0, |o| core_::object_cookie(&o));
            lent[i] = heap.lent();
        }
        // SAFETY: zeroed output; our arrays; the store under the lock.
        let mut failed: Stage = unsafe { core::mem::zeroed() };
        if !unsafe { crate::audit_snapshots::cvr_sda_borrowed_matches(core_::store(), lent.as_ptr(), current.as_ptr(), self.heaps.len(), self.ranges.as_ptr(), self.ranges.len(), &mut failed) } {
            stage(failed.stage, failed.index, failed.gpu, failed.expected, failed.actual);
            return false;
        }
        stage(7, u64::MAX, 0, self.epoch, epoch());
        if !core_::tracking_call(6, self.epoch) {
            set_details(tracking_details(self.pinned, self.epoch, self.ranges.is_empty()));
            return false;
        }
        true
    }
    /// The held half of ValidateBorrowedSourceRanges.
    pub fn validate(&self) -> bool {
        if !self.physical() {
            return false;
        }
        let _g = core_::guard();
        core_::drain_retired();
        if !self.matches_locked() {
            return false;
        }
        stage_at(0);
        true
    }
    /// The held half of InventoryBorrowedSourceRange: the range's descriptor inventory, cached per identity.
    pub fn inventory(&self, index: u32) -> Option<BorrowedDescriptorInventory> {
        stage(13, index.into(), 0, 0, 0);
        let range = self.ranges.get(index as usize).filter(|r| r.used.physical_remainder)?;
        let identity = range.identity;
        let (count, stride) = (u64::from(identity.count), u64::from(identity.stride));
        if count == 0 || count > SLOT_LIMIT as u64 || stride == 0 || identity.cpu == 0 || count > (u64::MAX - identity.cpu) / stride {
            return None;
        }
        let _g = core_::guard();
        core_::drain_retired();
        if !self.matches_locked() {
            return None;
        }
        let mut cache = self.inventory.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (cached, slots) = &mut *cache;
        let found = match cached.iter().position(|c| same_borrowed_range(&c.identity, &identity)) {
            Some(found) => found,
            None => {
                stage(13, index.into(), identity.gpu, SLOT_LIMIT as u64, *slots + count);
                if count > SLOT_LIMIT as u64 - *slots {
                    return None;
                }
                let mut captured = Inventory { identity, counts: BorrowedDescriptorInventory::default(), cookies: vec![0; count as usize] };
                // SAFETY: our outputs; the store under the lock.
                let unique = unsafe { crate::audit_inventory::cvr_sda_inventory(core_::store(), &identity, &mut captured.counts, captured.cookies.as_mut_ptr(), captured.cookies.len()) };
                captured.cookies.truncate(unique);
                cached.push(captured);
                *slots += count;
                cached.len() - 1
            }
        };
        let mut counts = cached[found].counts;
        core_::drain_retired();
        let cookies = &cached[found].cookies;
        // SAFETY: the cached cookies; our outputs.
        unsafe { crate::audit_inventory::cvr_sda_inventory_tracked(cookies.as_ptr(), cookies.len(), &mut counts.tracked_cookies, &mut counts.untracked_cookies) };
        drop(cache);
        self.matches_locked().then_some(counts)
    }
}
fn device_of<T: Interface>(child: &T) -> Option<ID3D12Device> {
    let child = child.cast::<ID3D12DeviceChild>().ok()?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a held child.
    unsafe { child.GetDevice(&mut device) }.ok()?;
    device
}
fn borrow_raw<T: Interface>(raw: &*mut c_void) -> Option<T> {
    // SAFETY: a live object the caller holds.
    unsafe { T::from_raw_borrowed(raw) }.cloned()
}

/// The capture after the .cpp's recording, bindings and family checks (stages 6..11).
#[allow(clippy::too_many_arguments)]
pub fn capture(list: *mut c_void, root: *mut c_void, pipeline: *mut c_void, heaps: &[*mut c_void], table_mask: u64, tables: &[u64; 64], uses: &[BorrowedShaderUse]) -> Option<Box<Borrowed>> {
    let (list, root, pipeline): (ID3D12GraphicsCommandList, ID3D12RootSignature, ID3D12PipelineState) = (borrow_raw(&list)?, borrow_raw(&root)?, borrow_raw(&pipeline)?);
    stage_at(6);
    let device = device_of(&list)?;
    if unsafe { device.GetNodeCount() } != 1 {
        return None;
    }
    if device_of(&root).as_ref() != Some(&device) || device_of(&pipeline).as_ref() != Some(&device) {
        return None;
    }
    let mut held: Vec<BorrowedHeap> = Vec::with_capacity(heaps.len());
    for (i, &object) in heaps.iter().enumerate() {
        stage(6, i as u64, 0, 0, 0);
        let mut shape = HeapShape::default();
        // SAFETY: the held device and heap.
        if object.is_null() || !unsafe { crate::audit_heaps::cvr_sda_shader_heap(device.as_raw(), object, SLOT_LIMIT as u32, false, &mut shape) } {
            return None;
        }
        let object: ID3D12DescriptorHeap = borrow_raw(&object)?;
        let desc = unsafe { object.GetDesc() };
        if held.iter().any(|prior| prior.desc.Type == desc.Type || prior.object.as_raw() == object.as_raw()) {
            return None;
        }
        held.push(BorrowedHeap { object, desc, cookie: 0, cpu: shape.cpu, gpu: shape.gpu, stride: shape.stride });
    }
    let root_key = root.as_raw() as u64;
    let mut next = Box::new(Borrowed { pinned: false, epoch: 0, device, _list: list, root, pipeline, heaps: held, ranges: Vec::new(), inventory: Mutex::new((Vec::new(), 0)) });
    {
        let _g = core_::guard();
        core_::drain_retired();
        stage_at(7);
        if !core_::tracking_call(2, 0) {
            let now = epoch();
            set_details(tracking_details(false, now, false));
            return None;
        }
        next.pinned = true;
        core_::select_storage();
        next.epoch = epoch();
        if !core_::tracking_call(6, next.epoch) || !core_::track_versions() || core_::overflow() || core_::tracking_failure() != 0 {
            set_details(tracking_details(next.pinned, next.epoch, false));
            return None;
        }
        let forbidden = (D3D12_ROOT_SIGNATURE_FLAG_LOCAL_ROOT_SIGNATURE.0 | D3D12_ROOT_SIGNATURE_FLAG_CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED.0 | D3D12_ROOT_SIGNATURE_FLAG_SAMPLER_HEAP_DIRECTLY_INDEXED.0) as u32;
        stage_at(8);
        let layout = core_::root_view(root_key).filter(|l| l.supported && l.parameters != 0 && l.parameters <= 64 && l.flags & forbidden == 0 && l.count != 0 && l.count <= 256)?;
        if layout.parameters < 64 && table_mask >> layout.parameters != 0 {
            return None;
        }
        // SAFETY: the root's layout, stable under the lock.
        let declarations: Vec<BorrowedRootRange> = unsafe { core::slice::from_raw_parts(layout.ranges, layout.count) }
            .iter()
            .map(|r| BorrowedRootRange { parameter: r.parameter, kind: r.kind, space: r.space, shader_register: r.shader_register, count: r.count, offset: r.table_offset, visibility: r.visibility })
            .collect();
        stage_at(9);
        let mut planned = [BorrowedUsedRange::default(); 256];
        let mut used = 0usize;
        // SAFETY: our arrays.
        if !unsafe {
            crate::descriptor_store::cvr_ds_plan(layout.parameters, layout.supported, layout.flags & forbidden != 0, declarations.as_ptr(), declarations.len(), uses.as_ptr(), uses.len(), planned.as_mut_ptr(), &mut used)
        } {
            return None;
        }
        let mut lent: [Heap; 2] = unsafe { core::mem::zeroed() };
        for (i, heap) in next.heaps.iter().enumerate() {
            lent[i] = Heap { cookie: 0, ..heap.lent() };
        }
        next.ranges = vec![unsafe { core::mem::zeroed() }; used];
        let mut failed: Stage = unsafe { core::mem::zeroed() };
        // SAFETY: our arrays, the store under the lock.
        let walked = unsafe {
            crate::audit_borrowed::cvr_sda_borrowed_walk(core_::store(), lent.as_mut_ptr(), next.heaps.len(), planned.as_ptr(), used, table_mask, tables.as_ptr(), next.ranges.as_mut_ptr(), &mut failed)
        };
        for (i, heap) in next.heaps.iter_mut().enumerate() {
            heap.cookie = lent[i].cookie;
        }
        if !walked {
            stage(failed.stage, failed.index, failed.gpu, failed.expected, failed.actual);
            return None;
        }
        if !next.matches_locked() {
            return None;
        }
    }
    if !next.physical() {
        return None;
    }
    Some(next)
}

// ---- the C ABI
/// What DescribeBorrowedSourceRanges and InventoryBorrowedSourceRange read.
#[repr(C)]
pub struct View {
    pub root: u64,
    pub pipeline: u64,
    pub ranges: *const Range,
    pub range_count: usize,
}
#[no_mangle]
pub extern "C-unwind" fn cvr_sda_bsr_stage(value: u32, index: u64, gpu: u64, expected: u64, actual: u64) {
    stage(value, index, gpu, expected, actual);
}
/// The thread's stage, its details into `details`.
///
/// # Safety
/// `details` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_bsr_last(details: *mut [u64; 4]) -> u32 {
    if let Some(d) = unsafe { details.as_mut() } {
        *d = DETAILS.with(Cell::get);
    }
    STAGE.with(Cell::get)
}
/// # Safety
/// The objects are held by the caller; `heaps` holds `heap_count`, `tables` 64 and `uses` `use_count`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sda_bsr_capture(
    list: *mut c_void,
    root: *mut c_void,
    pipeline: *mut c_void,
    heaps: *const *mut c_void,
    heap_count: u32,
    table_mask: u64,
    tables: *const [u64; 64],
    uses: *const BorrowedShaderUse,
    use_count: usize,
) -> *mut Borrowed {
    if heaps.is_null() || tables.is_null() || heap_count == 0 || heap_count > 2 || (use_count != 0 && uses.is_null()) {
        stage_at(6);
        return core::ptr::null_mut();
    }
    let heaps = unsafe { core::slice::from_raw_parts(heaps, heap_count as usize) };
    let uses = if use_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(uses, use_count) } };
    capture(list, root, pipeline, heaps, table_mask, unsafe { &*tables }, uses).map_or(core::ptr::null_mut(), Box::into_raw)
}
/// # Safety
/// A capture's handle (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_bsr_free(handle: *mut Borrowed) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}
/// # Safety
/// A live capture's handle.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_bsr_validate(handle: *const Borrowed) -> bool {
    unsafe { handle.as_ref() }.is_some_and(Borrowed::validate)
}
/// MatchesLocked, taking the lock (and draining the retired lifetimes) first unless the caller holds it.
///
/// # Safety
/// A live capture's handle.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_bsr_matches(handle: *const Borrowed, lock: bool) -> bool {
    let Some(held) = (unsafe { handle.as_ref() }) else { return false };
    if !lock {
        return held.matches_locked();
    }
    let _g = core_::guard();
    core_::drain_retired();
    held.matches_locked()
}
/// # Safety
/// A live capture's handle; `out` is writable. The ranges live as long as the handle.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_bsr_view(handle: *const Borrowed, out: *mut View) {
    if let (Some(held), Some(out)) = (unsafe { handle.as_ref() }, unsafe { out.as_mut() }) {
        *out = View { root: held.root.as_raw() as u64, pipeline: held.pipeline.as_raw() as u64, ranges: held.ranges.as_ptr(), range_count: held.ranges.len() };
    }
}
/// # Safety
/// A live capture's handle; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sda_bsr_inventory(handle: *const Borrowed, index: u32, out: *mut BorrowedDescriptorInventory) -> bool {
    match unsafe { handle.as_ref() }.and_then(|held| held.inventory(index)) {
        Some(counts) => {
            unsafe { *out = counts };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! The thread's stage and details, and the refusals before any object is touched.
    use super::*;

    #[test]
    fn stages_and_refusals() {
        stage(10, 3, 0x40, 7, 9);
        let mut details = [0u64; 4];
        assert_eq!(unsafe { cvr_sda_bsr_last(&mut details) }, 10);
        assert_eq!(details, [3, 0x40, 7, 9]);
        cvr_sda_bsr_stage(2, u64::MAX, 0, 0, 0);
        assert_eq!(unsafe { cvr_sda_bsr_last(core::ptr::null_mut()) }, 2);
        let tables = [0u64; 64];
        let heap = 0x1000 as *mut c_void;
        // No heaps, too many heaps, no tables: refused at the heaps' stage before any object is read.
        assert!(unsafe { cvr_sda_bsr_capture(heap, heap, heap, &heap, 0, 0, &tables, core::ptr::null(), 0) }.is_null());
        assert!(unsafe { cvr_sda_bsr_capture(heap, heap, heap, [heap; 3].as_ptr(), 3, 0, &tables, core::ptr::null(), 0) }.is_null());
        assert!(unsafe { cvr_sda_bsr_capture(heap, heap, heap, &heap, 1, 0, core::ptr::null(), core::ptr::null(), 0) }.is_null());
        assert_eq!(unsafe { cvr_sda_bsr_last(core::ptr::null_mut()) }, 6);
        assert!(!unsafe { cvr_sda_bsr_validate(core::ptr::null()) } && !unsafe { cvr_sda_bsr_matches(core::ptr::null(), true) });
        unsafe { cvr_sda_bsr_free(core::ptr::null_mut()) };
    }
}
