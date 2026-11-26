//! THE GEOMETRY FREEZE (formerly src/Render/NativeGeometrySnapshots.cpp and GeometrySnapshot.cpp's Rust-build half): while
//! the native probe records with the freeze requested, each admitted draw's index, vertex and instance bytes are copied
//! into a private buffer at the draw (geometry_snapshot.rs' plan, buffers and copies; at most 2048 draws and 256 MiB of
//! worst-case pairs), the copy's owner kept with the list's recording; at the list's single submission the queue's own
//! fence is signalled; once the recording is gone the snapshot is sealed with that fence and its bytes read back without
//! waiting. Two read snapshots of one frame pair into the constant bank's geometry commands.
//!
//! The rows (sequence, status 1 recorded, 2 submitted, 3 read, 4 refused, 5 resubmitted, 6 abandoned, 7 failed; the
//! result and bytes) are published as CyberpunkVR_GeometryFreeze*. The .cpp lends those exports (the plugin's are
//! declared_moved_exports.rs', a Rust test variant's its reference definitions), the draw's C++ copy (its leases
//! rewritten onto the private buffer, its instances filled on read) and the pair's C++ commands.
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering::*};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_NOINTERFACE, E_OUTOFMEMORY, E_POINTER, E_UNEXPECTED, S_FALSE, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Threading::{AcquireSRWLockExclusive, ReleaseSRWLockExclusive, SRWLOCK};

use crate::geometry_commands::DrawRecord;
use crate::geometry_snapshot::{self as gsn, Slot};

/// geometry_snapshots::Record.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Record {
    pub sequence: u32,
    pub status: u32,
    pub result: i32,
    pub reserved: u32,
    pub bytes: u64,
}
const _: () = assert!(core::mem::size_of::<Record>() == 24);
const LIMIT: u64 = 256 * 1024 * 1024;

/// The .cpp's exports, draw copies and pair (static storage, attached at its static initialisation).
#[repr(C)]
pub struct Host {
    pub enable: *const AtomicU32,
    pub seq: *const AtomicU32,
    pub count: *const AtomicU32,
    pub pending: *const AtomicU32,
    pub failure: *const AtomicU32,
    pub records: *mut u64,
    pub bytes: *mut u64,
    /// A heap copy of the draw's geometry::DrawResources without its instances.
    pub draw_new: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    /// The copy's draw record.
    pub draw_record: unsafe extern "C" fn(*mut c_void) -> *mut DrawRecord,
    /// The private buffer as the copy's index lease and every input slot's vertex lease.
    pub draw_owned: unsafe extern "C" fn(*mut c_void, *mut c_void, u64),
    /// The read instance bytes into the copy.
    pub draw_instances: unsafe extern "C" fn(*mut c_void, *const u8, usize),
    pub draw_free: unsafe extern "C" fn(*mut c_void),
    /// GeometrySnapshot::Pair's commands from the two copies (left the VRCAM eye) into a GeometryCommandPair.
    pub pair: unsafe extern "C" fn(*const c_void, *const c_void, *mut c_void) -> bool,
}
static HOST: AtomicPtr<Host> = AtomicPtr::new(null_mut());
/// # Safety
/// `host` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gfz_attach(host: *const Host) {
    HOST.store(host.cast_mut(), Release);
}
fn host() -> Option<&'static Host> {
    // SAFETY: attached once with static storage.
    let attached = unsafe { HOST.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::HOST));
    attached
}
/// The plugin's exports and draw copies (formerly lent by src/Render/NativeGeometrySnapshots.cpp): the copies are
/// geometry_leases.rs' DrawResources, the pair two of them. The Rust test variants attach the .cpp's.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use super::Host;
    use crate::declared_moved_exports::*;
    use crate::geometry_commands::DrawRecord;
    use crate::geometry_leases::{BufferLease, DrawResources};
    use core::ffi::c_void;
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D12::*;
    /// A heap copy of the source without its instances.
    unsafe extern "C" fn draw_new(source: *const c_void) -> *mut c_void {
        // SAFETY: the snapshot's source.
        let mut copy = unsafe { &*source.cast::<DrawResources>() }.clone();
        copy.instances.clear();
        Box::into_raw(Box::new(copy)).cast()
    }
    unsafe extern "C" fn draw_record(copy: *mut c_void) -> *mut DrawRecord {
        // SAFETY: a copy of draw_new's.
        unsafe { &raw mut (*copy.cast::<DrawResources>()).draw }
    }
    /// The private buffer as the copy's index lease and every input slot's vertex lease.
    unsafe extern "C" fn draw_owned(copy: *mut c_void, resource: *mut c_void, bytes: u64) {
        // SAFETY: a copy of draw_new's; the live private buffer.
        let (copy, resource) = unsafe { (&mut *copy.cast::<DrawResources>(), ID3D12Resource::from_raw_borrowed(&resource)) };
        let Some(resource) = resource else { return };
        // SAFETY: a live buffer.
        let gpu = unsafe { resource.GetGPUVirtualAddress() };
        let owned = BufferLease { resource: Some(resource.clone()), gpu, bytes, heap_type: D3D12_HEAP_TYPE_DEFAULT.0 as u32, flags: 0, native: Default::default() };
        copy.index = owned.clone();
        for slot in 0..16 {
            if copy.draw.input_slot_mask & (1 << slot) != 0 {
                copy.vertices[slot] = owned.clone();
            }
        }
    }
    unsafe extern "C" fn draw_instances(copy: *mut c_void, bytes: *const u8, count: usize) {
        // SAFETY: a copy of draw_new's; the read instances.
        let copy = unsafe { &mut *copy.cast::<DrawResources>() };
        copy.instances = if bytes.is_null() || count == 0 { Vec::new() } else { unsafe { core::slice::from_raw_parts(bytes, count) }.to_vec() };
    }
    unsafe extern "C" fn draw_free(copy: *mut c_void) {
        // SAFETY: a copy of draw_new's, freed once.
        drop(unsafe { Box::from_raw(copy.cast::<DrawResources>()) });
    }
    /// GeometrySnapshot::Pair's commands: the left (VRCAM) eye's index and vertex streams on both, into the caller's
    /// Option<[DrawResources; 2]>.
    unsafe extern "C" fn pair(left: *const c_void, right: *const c_void, out: *mut c_void) -> bool {
        // SAFETY: two copies of draw_new's.
        let (a, b) = unsafe { (&*left.cast::<DrawResources>(), &*right.cast::<DrawResources>()) };
        let mut second = b.clone();
        second.index = a.index.clone();
        second.draw.index = a.draw.index;
        for slot in 0..16 {
            if a.draw.input_slot_mask & (1 << slot) != 0 {
                second.vertices[slot] = a.vertices[slot].clone();
                second.draw.vertices[slot] = a.draw.vertices[slot];
            }
        }
        // SAFETY: the caller's output (copy_pair_resources').
        unsafe { *out.cast::<Option<[DrawResources; 2]>>() = Some([a.clone(), second]) };
        true
    }
    // SAFETY: immutable; its pointers name process-lifetime exports.
    unsafe impl Sync for Host {}
    pub static HOST: Host = Host {
        enable: &raw const CyberpunkVR_GeometryFreezeEnable,
        seq: &raw const CyberpunkVR_GeometryFreezeSeq,
        count: &raw const CyberpunkVR_GeometryFreezeCount,
        pending: &raw const CyberpunkVR_GeometryFreezePending,
        failure: &raw const CyberpunkVR_GeometryFreezeFailure,
        records: &raw mut CyberpunkVR_GeometryFreezeRecords,
        bytes: &raw mut CyberpunkVR_GeometryFreezeBytes,
        draw_new,
        draw_record,
        draw_owned,
        draw_instances,
        draw_free,
        pair,
    };
}
/// GeometrySnapshot::Capture for the plugin's snapshots (geometry_leases.rs' natives): the source's leases and their
/// read states as the freeze's slots.
#[cfg(all(feature = "rust-build", not(test)))]
pub fn capture_resources(source: &crate::geometry_leases::DrawResources, states: &[crate::geometry_leases::ReadState; 17], rebase: bool) {
    let slots: [Slot; 17] = core::array::from_fn(|slot| {
        let lease = if slot == 0 { &source.index } else { &source.vertices[slot - 1] };
        Slot { resource: lease.raw(), gpu: lease.gpu, bytes: lease.bytes, heap_type: lease.heap_type, status: states[slot].status, value: states[slot].value, _pad: 0 }
    });
    // SAFETY: the source, its record and the slots live for the call; the plugin host copies the source.
    unsafe { cvr_gfz_capture((source as *const crate::geometry_leases::DrawResources).cast(), &source.draw, &slots, rebase) };
}
/// CopyPair for the plugin's constant bank (constant_bank.rs): the two read snapshots' paired copies.
#[cfg(all(feature = "rust-build", not(test)))]
pub fn copy_pair_resources(left: u32, right: u32) -> Option<[crate::geometry_leases::DrawResources; 2]> {
    let mut out: Option<[crate::geometry_leases::DrawResources; 2]> = None;
    // SAFETY: the plugin host's pair writes this Option.
    let paired = unsafe { cvr_gfz_copy_pair(left, right, (&raw mut out).cast()) };
    if paired {
        out
    } else {
        None
    }
}
fn word(value: *const AtomicU32) -> &'static AtomicU32 {
    // SAFETY: the lent exports live for the process.
    unsafe { &*value }
}

// ---- the snapshot's owner: the COM object the list's recording keeps (GeometrySnapshot::Storage) --------------------

struct Inner {
    snapshot: gsn::Snapshot,
    device: ID3D12Device,
    fence: Option<ID3D12Fence>,
    value: u64,
    recording: usize,
    draw: *mut c_void,
    ready: bool,
}
#[repr(C)]
struct Storage {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    inner: UnsafeCell<Inner>,
}
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn storage_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live Storage.
    unsafe {
        *out = null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return E_NOINTERFACE;
        }
        *out = this;
        storage_add_ref(this);
    }
    S_OK
}
unsafe extern "system" fn storage_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live Storage.
    unsafe { (*(this as *const Storage)).refs.fetch_add(1, Relaxed) + 1 }
}
unsafe extern "system" fn storage_release(this: *mut c_void) -> u32 {
    // SAFETY: a live Storage; the last release frees it (its draw copy through the host).
    unsafe {
        let n = (*(this as *const Storage)).refs.fetch_sub(1, AcqRel) - 1;
        if n == 0 {
            let storage = Box::from_raw(this as *mut Storage);
            let draw = (*storage.inner.get()).draw;
            if !draw.is_null() {
                if let Some(h) = host() {
                    (h.draw_free)(draw);
                }
            }
        }
        n
    }
}
static STORAGE_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: storage_query, AddRef: storage_add_ref, Release: storage_release };
fn inner(owner: &IUnknown) -> &mut Inner {
    // SAFETY: a Storage this module made; touched only under the freeze's lock (or by its last release).
    unsafe { &mut *(*(owner.as_raw() as *const Storage)).inner.get() }
}
fn on_recording(owner: &IUnknown) -> bool {
    // SAFETY: the recorded list as the tracker's key; the owner live.
    unsafe { cvr_gpu::command_resources::cvr_gpu_has_command_resource(inner(owner).recording as *mut c_void, owner.as_raw()) }
}

/// GeometrySnapshot::Capture: the draw's plan, its private buffer (the copy's leases rewritten onto it), the owner kept
/// with the list's recording and the copy commands.
fn capture(h: &Host, list: &ID3D12GraphicsCommandList, source: *const c_void, draw: &DrawRecord, slots: &[Slot; 17], rebase: bool) -> Result<IUnknown, i32> {
    let snapshot = gsn::plan(list, draw, slots, rebase)?;
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live list; a local output.
    unsafe { list.GetDevice(&mut device) }.map_err(|e| e.code().0)?;
    let device = device.ok_or(E_FAIL.0)?;
    // SAFETY: the .cpp's copy of the live draw.
    let copy = unsafe { (h.draw_new)(source) };
    if copy.is_null() {
        return Err(E_OUTOFMEMORY.0);
    }
    let storage = Box::new(Storage {
        vtbl: &STORAGE_VTBL,
        refs: AtomicU32::new(1),
        inner: UnsafeCell::new(Inner { snapshot, device, fence: None, value: 0, recording: list.as_raw() as usize, draw: copy, ready: false }),
    });
    // SAFETY: the box's one reference becomes the IUnknown's.
    let owner = unsafe { IUnknown::from_raw(Box::into_raw(storage).cast()) };
    let i = inner(&owner);
    let resource = {
        let _internal = cvr_host::probe::Internal::new();
        // SAFETY: the copy's own record.
        i.snapshot.create(unsafe { &mut *(h.draw_record)(copy) })?
    };
    // SAFETY: the copy and the private buffer (the copy's leases take their references).
    unsafe { (h.draw_owned)(copy, resource.as_raw(), i.snapshot.bytes) };
    if !cvr_gpu::command_resources::keep(list, &[owner.as_raw()]) {
        return Err(E_OUTOFMEMORY.0);
    }
    i.snapshot.record(list);
    Ok(owner)
}

// ---- the freeze -------------------------------------------------------------------------------------------------------

struct Entry {
    owner: Option<IUnknown>,
    list: usize,
    index: u32,
    fence: Option<ID3D12Fence>,
    value: u64,
    generation: u64,
    submitting: bool,
    sealed: bool,
}
pub struct EntryCell(UnsafeCell<Entry>);
// SAFETY: touched only under the freeze's lock.
unsafe impl Send for EntryCell {}
// SAFETY: as above.
unsafe impl Sync for EntryCell {}
impl EntryCell {
    #[allow(clippy::mut_from_ref)]
    fn get(&self, _: &Locked) -> &mut Entry {
        // SAFETY: under the lock.
        unsafe { &mut *self.0.get() }
    }
}
struct Queue {
    _owner: ID3D12CommandQueue,
    fence: Option<ID3D12Fence>,
    value: u64,
}
struct State {
    active: bool,
    records: Vec<Record>,
    entries: BTreeMap<u32, Arc<EntryCell>>,
    queues: HashMap<usize, Queue>,
    generation: u64,
    charged: u64,
}
struct StateCell(UnsafeCell<Option<State>>);
// SAFETY: touched only under the lock.
unsafe impl Sync for StateCell {}
static STATE: StateCell = StateCell(UnsafeCell::new(None));
struct Lock(UnsafeCell<SRWLOCK>);
// SAFETY: an SRW lock, used only through its acquire and release.
unsafe impl Sync for Lock {}
static LOCK: Lock = Lock(UnsafeCell::new(SRWLOCK { Ptr: null_mut() }));
struct Locked;
impl Drop for Locked {
    fn drop(&mut self) {
        // SAFETY: acquired by lock() on this thread.
        unsafe { ReleaseSRWLockExclusive(LOCK.0.get()) };
    }
}
fn lock() -> Locked {
    // SAFETY: the static lock.
    unsafe { AcquireSRWLockExclusive(LOCK.0.get()) };
    Locked
}
fn state(_: &Locked) -> &'static mut State {
    // SAFETY: under the lock.
    unsafe {
        (*STATE.0.get()).get_or_insert_with(|| State {
            active: false,
            records: Vec::new(),
            entries: BTreeMap::new(),
            queues: HashMap::new(),
            generation: 0,
            charged: 0,
        })
    }
}
fn publish(h: &Host, s: &State) {
    // SAFETY: the exported row pointer.
    unsafe { *h.records = s.records.as_ptr() as u64 };
    word(h.count).store(s.records.len() as u32, SeqCst);
    word(h.pending).store(s.records.iter().filter(|r| r.status == 1 || r.status == 2).count() as u32, SeqCst);
}
fn seq(h: &Host) {
    word(h.seq).fetch_add(1, SeqCst);
}

/// Begin: a new generation (the rows, entries and queue fences dropped), active when the freeze is on.
#[no_mangle]
pub extern "C-unwind" fn cvr_gfz_begin(enabled: bool) {
    let Some(h) = host() else { return };
    let l = lock();
    let s = state(&l);
    seq(h);
    s.active = enabled;
    s.generation += 1;
    s.records.clear();
    s.entries.clear();
    s.queues.clear();
    s.charged = 0;
    // SAFETY: the exported counter.
    unsafe { *h.bytes = 0 };
    word(h.failure).store(0, SeqCst);
    publish(h, s);
    seq(h);
}
/// Capture: one draw's snapshot (`source` the .cpp's geometry::DrawResources, `draw` its record, `slots` its leases and
/// read states).
///
/// # Safety
/// `source`, `draw` and `slots` are valid for the call; the draw's list is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gfz_capture(source: *const c_void, draw: *const DrawRecord, slots: *const [Slot; 17], rebase: bool) {
    let Some(h) = host() else { return };
    if word(h.enable).load(Relaxed) == 0 || cvr_host::probe::internal() {
        return;
    }
    // SAFETY: as documented.
    let (d, slots) = unsafe { (&*draw, &*slots) };
    let l = lock();
    let s = state(&l);
    if !s.active || word(h.failure).load(SeqCst) != 0 {
        return;
    }
    seq(h);
    let sequence = d.sequence as u32;
    if s.records.len() >= 2048 || s.entries.contains_key(&sequence) {
        word(h.failure).store(1, SeqCst);
    } else {
        // Reserve a worst-case pair of allocations before recording commands.
        let mut logical = u64::from(d.index.SizeInBytes);
        for slot in 0..16 {
            if d.input_slot_mask & (1 << slot) != 0 {
                logical += if slot == 7 {
                    u64::from(d.instances) * u64::from(d.vertices[7].StrideInBytes)
                } else {
                    u64::from(d.vertices[slot].SizeInBytes)
                };
            }
        }
        let charge = ((logical + 17 * 16 + 65535) & !65535u64) * 2;
        let raw_list = d.list as *mut c_void;
        // SAFETY: the draw's live list.
        let list = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw_list) };
        let captured = if charge > LIMIT - s.charged {
            Err(E_OUTOFMEMORY.0)
        } else {
            match list {
                Some(list) => capture(h, list, source, d, slots, rebase),
                None => Err(windows::Win32::Foundation::E_INVALIDARG.0),
            }
        };
        let (hr, bytes) = match &captured {
            Ok(owner) => (S_OK.0, inner(owner).snapshot.bytes),
            Err(hr) => (*hr, 0),
        };
        let row = Record { sequence, status: if hr >= 0 { 1 } else { 4 }, result: hr, reserved: 0, bytes };
        if let Ok(owner) = captured {
            s.charged += charge;
            // SAFETY: the exported counter.
            unsafe { *h.bytes += row.bytes };
            let entry = Entry {
                owner: Some(owner),
                list: d.list as usize,
                index: s.records.len() as u32,
                fence: None,
                value: 0,
                generation: s.generation,
                submitting: false,
                sealed: false,
            };
            s.entries.insert(sequence, Arc::new(EntryCell(UnsafeCell::new(entry))));
        }
        s.records.push(row);
    }
    publish(h, s);
    seq(h);
}
/// A submission's entries (BeforeSubmit's).
pub type Submission = Vec<Arc<EntryCell>>;
/// BeforeSubmit: the recorded entries whose list this submission executes once, marked submitting.
///
/// # Safety
/// `lists` holds `count` lists (or is null).
pub unsafe fn before_submit(count: u32, lists: *const *mut c_void) -> Submission {
    let mut result = Vec::new();
    let Some(h) = host() else { return result };
    if lists.is_null() || word(h.pending).load(Acquire) == 0 {
        return result;
    }
    // SAFETY: as documented.
    let lists = unsafe { core::slice::from_raw_parts(lists, count as usize) };
    let l = lock();
    let s = state(&l);
    seq(h);
    for cell in s.entries.values() {
        let e = cell.get(&l);
        let row = &mut s.records[e.index as usize];
        if (row.status != 1 && row.status != 2) || e.sealed {
            continue;
        }
        let occurrences = lists.iter().filter(|&&x| x as usize == e.list).count();
        if occurrences == 0 {
            continue;
        }
        if !e.owner.as_ref().is_some_and(on_recording) {
            if row.status == 1 && !e.submitting {
                row.status = 6;
                row.result = E_UNEXPECTED.0;
            }
            continue;
        }
        if row.status == 2 || e.submitting || occurrences != 1 {
            row.status = 5;
            row.result = E_UNEXPECTED.0;
            continue;
        }
        e.submitting = true;
        result.push(cell.clone());
    }
    publish(h, s);
    seq(h);
    result
}
/// Submitted: the queue's own fence signalled after the submission; its entries submitted (or failed).
pub fn submitted(queue: *mut c_void, mut submission: Submission) {
    if submission.is_empty() {
        return;
    }
    let Some(h) = host() else { return };
    let l = lock();
    let s = state(&l);
    seq(h);
    let generation = s.generation;
    submission.retain(|cell| cell.get(&l).generation == generation);
    if submission.is_empty() {
        seq(h);
        return;
    }
    let mut hr = E_FAIL;
    // SAFETY: the submission's live queue.
    if let Some(q) = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) } {
        let state = s.queues.entry(queue as usize).or_insert_with(|| Queue { _owner: q.clone(), fence: None, value: 0 });
        if state.fence.is_none() {
            let mut device: Option<ID3D12Device> = None;
            // SAFETY: the live queue's device; local outputs.
            match unsafe { q.GetDevice(&mut device) }.and_then(|()| device.ok_or_else(|| E_FAIL.into())) {
                Ok(device) => match unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) } {
                    Ok(fence) => state.fence = Some(fence),
                    Err(e) => hr = e.code(),
                },
                Err(e) => hr = e.code(),
            }
        }
        if let Some(fence) = &state.fence {
            // SAFETY: the queue's own fence.
            hr = match unsafe { q.Signal(fence, state.value + 1) } {
                Ok(()) => S_OK,
                Err(e) => e.code(),
            };
        }
        if hr.is_ok() {
            state.value += 1;
        }
        for cell in &submission {
            let e = cell.get(&l);
            e.submitting = false;
            let row = &mut s.records[e.index as usize];
            if row.status == 5 {
                continue;
            }
            if hr.is_err() {
                row.status = 7;
                row.result = hr.0;
            } else {
                row.status = 2;
                e.fence = state.fence.clone();
                e.value = state.value;
            }
        }
    } else {
        for cell in &submission {
            let e = cell.get(&l);
            e.submitting = false;
            let row = &mut s.records[e.index as usize];
            if row.status != 5 {
                row.status = 7;
                row.result = hr.0;
            }
        }
    }
    publish(h, s);
    seq(h);
}
/// GeometrySnapshot::Seal: the exact source submission's fence, once the source recording is gone.
fn seal(owner: &IUnknown, fence: Option<&ID3D12Fence>, value: u64) -> bool {
    let i = inner(owner);
    let Some(fence) = fence else { return false };
    if i.fence.is_some() || value == 0 || value == u64::MAX || on_recording(owner) {
        return false;
    }
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live fence.
    if unsafe { fence.GetDevice(&mut device) }.is_err() || device.as_ref().map(|d| d.as_raw()) != Some(i.device.as_raw()) {
        return false;
    }
    i.fence = Some(fence.clone());
    i.value = value;
    true
}
/// GeometrySnapshot::Read: S_FALSE until the sealed submission completes; the instances into the copy.
fn read(h: &Host, owner: &IUnknown) -> HRESULT {
    let i = inner(owner);
    if i.ready {
        return S_OK;
    }
    let Some(fence) = i.fence.clone() else { return S_FALSE };
    let hr = HRESULT(i.snapshot.read(&fence, i.value));
    if hr != S_OK {
        return hr;
    }
    let instances = i.snapshot.instances();
    // SAFETY: the copy and the snapshot's instance range.
    unsafe { (h.draw_instances)(i.draw, instances.as_ptr(), instances.len()) };
    i.ready = true;
    S_OK
}
/// Poll: abandoned recordings, the submitted entries sealed and read back.
#[no_mangle]
pub extern "C-unwind" fn cvr_gfz_poll() {
    let Some(h) = host() else { return };
    if word(h.pending).load(Acquire) == 0 {
        return;
    }
    let l = lock();
    let s = state(&l);
    seq(h);
    for cell in s.entries.values() {
        let e = cell.get(&l);
        let row = &mut s.records[e.index as usize];
        let Some(owner) = e.owner.clone() else { continue };
        if row.status == 1 && !e.submitting && !on_recording(&owner) {
            row.status = 6;
            row.result = E_UNEXPECTED.0;
        }
        if row.status != 2 || e.submitting {
            continue;
        }
        if !e.sealed {
            e.sealed = seal(&owner, e.fence.as_ref(), e.value);
        }
        if !e.sealed {
            continue;
        }
        let hr = read(h, &owner);
        if hr == S_FALSE {
            continue;
        }
        row.result = hr.0;
        row.status = if hr.is_ok() { 3 } else { 7 };
    }
    publish(h, s);
    seq(h);
}
/// CopyPair: the two read snapshots' paired commands (GeometrySnapshot::Pair) into the .cpp's GeometryCommandPair.
///
/// # Safety
/// `output` is a GeometryCommandPair.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gfz_copy_pair(left: u32, right: u32, output: *mut c_void) -> bool {
    let Some(h) = host() else { return false };
    let l = lock();
    let s = state(&l);
    if word(h.failure).load(SeqCst) != 0 {
        return false;
    }
    let (Some(a), Some(b)) = (s.entries.get(&left), s.entries.get(&right)) else { return false };
    let (a, b) = (a.get(&l), b.get(&l));
    if s.records[a.index as usize].status != 3 || s.records[b.index as usize].status != 3 {
        return false;
    }
    let (Some(a), Some(b)) = (a.owner.as_ref(), b.owner.as_ref()) else { return false };
    let (x, y) = (inner(a), inner(b));
    if !x.ready || !y.ready || x.device.as_raw() != y.device.as_raw() {
        return false;
    }
    // SAFETY: the copies' own records.
    let (dx, dy) = unsafe { (&*(h.draw_record)(x.draw), &*(h.draw_record)(y.draw)) };
    if !gsn::pair(&x.snapshot, dx, &y.snapshot, dy) {
        return false;
    }
    // SAFETY: as documented; the copies live with their owners.
    unsafe { (h.pair)(x.draw, y.draw, output) }
}

/// BeforeSubmit for the .cpp's facade: each submitted entry boxed alone and handed to `push`.
///
/// # Safety
/// As before_submit; `push` takes each box.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gfz_before_submit(count: u32, lists: *const *mut c_void, push: unsafe extern "C" fn(*mut c_void, *mut c_void), user: *mut c_void) {
    // SAFETY: as documented.
    for entry in unsafe { before_submit(count, lists) } {
        // SAFETY: the facade's own sink.
        unsafe { push(user, Box::into_raw(Box::new(vec![entry])).cast()) };
    }
}
/// Submitted for the .cpp's facade: the boxed entries consumed together (one submission, one fence signal).
///
/// # Safety
/// `entries` holds `count` boxes from cvr_gfz_before_submit (nulls skipped); the queue live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gfz_submitted(queue: *mut c_void, entries: *const *mut c_void, count: usize) {
    let mut submission = Submission::new();
    if !entries.is_null() {
        // SAFETY: as documented.
        for &boxed in unsafe { core::slice::from_raw_parts(entries, count) } {
            if !boxed.is_null() {
                // SAFETY: as documented.
                submission.extend(*unsafe { Box::from_raw(boxed.cast::<Submission>()) });
            }
        }
    }
    submitted(queue, submission);
}
/// A boxed submission never submitted.
///
/// # Safety
/// As cvr_gfz_submitted.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gfz_submission_free(submission: *mut c_void) {
    if !submission.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(submission.cast::<Submission>()) });
    }
}
