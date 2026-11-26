//! The objects a command list uses, kept alive until the GPU has run it (formerly src/Render/CommandResources.cpp).
//!
//! A list's recording owns its batch of objects; the registry holds only a weak reference to the recording, and the
//! list itself holds the strong one as private data (an IUnknown owner set with SetPrivateDataInterface), so
//! destroying or resetting the list expires the entry. A submission owns its batches only: the lists of one
//! ExecuteCommandLists are prepared into a submission before it, and after it the submission goes into the queue's
//! flight, released once the queue's own fence passes its value. A failed fence (a removed device) keeps every
//! submission alive for good and refuses new recordings.
use cvr_host::fast::FastMap;
use core::ffi::c_void;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};

use windows::core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandList, ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12GraphicsCommandList, D3D12_FENCE_FLAG_NONE,
};

/// One recording's objects.
pub struct Batch {
    resources: Vec<IUnknown>,
}
// SAFETY: the objects are free-threaded D3D12/COM objects, only AddRef'd and Released here.
unsafe impl Send for Batch {}
unsafe impl Sync for Batch {}

/// What a list holds while it records: its batch.
struct Recording {
    batch: Arc<Batch>,
}

/// The batches of one ExecuteCommandLists.
pub type Submission = Vec<Arc<Batch>>;

/// The private-data key the recording owner is set under (the C++ build's GUID).
const RECORDING_KEY: GUID = GUID::from_values(0x29c6b412, 0xb5a9, 0x4b64, [0x9f, 0x1c, 0x40, 0x3a, 0x97, 0xae, 0x07, 0x32]);

// ---- the recording owner: a minimal IUnknown

#[repr(C)]
struct Owner {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    recording: Arc<Recording>,
}
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn owner_query(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    if out.is_null() {
        return windows::Win32::Foundation::E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live Owner.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || *iid != IID_IUNKNOWN {
            return windows::Win32::Foundation::E_NOINTERFACE;
        }
        *out = this;
        owner_add_ref(this);
    }
    HRESULT(0)
}
unsafe extern "system" fn owner_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live Owner.
    unsafe { (*(this as *const Owner)).refs.fetch_add(1, Ordering::Relaxed) + 1 }
}
unsafe extern "system" fn owner_release(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live Owner; the last release frees it (it came from Box::into_raw).
    unsafe {
        let n = (*(this as *const Owner)).refs.fetch_sub(1, Ordering::AcqRel) - 1;
        if n == 0 {
            drop(Box::from_raw(this as *mut Owner));
        }
        n
    }
}
static OWNER_VTBL: IUnknown_Vtbl = IUnknown_Vtbl { QueryInterface: owner_query, AddRef: owner_add_ref, Release: owner_release };
// SAFETY: the vtable holds plain function pointers.
unsafe impl Sync for Owner {}

/// A COM owner of `recording` (one reference, held by the returned IUnknown).
fn owner(recording: Arc<Recording>) -> IUnknown {
    let raw = Box::into_raw(Box::new(Owner { vtbl: &OWNER_VTBL, refs: AtomicU32::new(1), recording }));
    // SAFETY: a fresh object with one reference, handed to the IUnknown that releases it.
    unsafe { IUnknown::from_raw(raw as *mut c_void) }
}

// ---- the registry

struct Flight {
    value: u64,
    _submission: Submission,
}
struct QueueState {
    _queue: Option<ID3D12CommandQueue>,
    fence: Option<ID3D12Fence>,
    value: u64,
    flights: VecDeque<Flight>,
}
struct State {
    recordings: FastMap<usize, Weak<Recording>>,
    queues: FastMap<usize, QueueState>,
    failed_submissions: Vec<Submission>,
}
// SAFETY: the COM objects inside are free-threaded; every access is under the mutex.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static HAVE_RECORDINGS: AtomicBool = AtomicBool::new(false);
static HAVE_FLIGHTS: AtomicBool = AtomicBool::new(false);
static FAILED: AtomicBool = AtomicBool::new(false);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let state = guard.get_or_insert_with(|| State { recordings: cvr_host::fast::fast_map(), queues: cvr_host::fast::fast_map(), failed_submissions: Vec::new() });
    f(state)
}

fn prune(s: &mut State) {
    s.recordings.retain(|_, w| w.strong_count() > 0);
    HAVE_RECORDINGS.store(!s.recordings.is_empty(), Ordering::Release);
}

fn collect(s: &mut State) {
    let mut pending = false;
    for state in s.queues.values_mut() {
        // SAFETY: the queue's own fence.
        let done = state.fence.as_ref().map_or(0, |f| unsafe { f.GetCompletedValue() });
        while state.flights.front().is_some_and(|f| f.value <= done) {
            state.flights.pop_front();
        }
        pending |= !state.flights.is_empty();
    }
    HAVE_FLIGHTS.store(pending, Ordering::Release);
}

/// KeepCommandResources: `objects` live at least as long as every submission of `list`'s current recording. False
/// when an object is null, the list cannot carry the owner, or after a failed fence.
pub fn keep(list: &ID3D12GraphicsCommandList, objects: &[*mut c_void]) -> bool {
    if FAILED.load(Ordering::Relaxed) {
        return false;
    }
    with_state(|s| {
        prune(s);
        let key = list.as_raw() as usize;
        let mut resources: Vec<IUnknown> = s.recordings.get(&key).and_then(Weak::upgrade).map(|r| r.batch.resources.clone()).unwrap_or_default();
        for &object in objects {
            // SAFETY: the caller's live COM object (or null).
            let Some(o) = (unsafe { IUnknown::from_raw_borrowed(&object) }) else { return false };
            if !resources.iter().any(|r| r.as_raw() == object) {
                resources.push(o.clone());
            }
        }
        if resources.is_empty() {
            return false;
        }
        let recording = Arc::new(Recording { batch: Arc::new(Batch { resources }) });
        let owner = owner(recording.clone());
        let previous = s.recordings.insert(key, Arc::downgrade(&recording));
        // SAFETY: the list's private data takes its own reference to the owner.
        if unsafe { list.SetPrivateDataInterface(&RECORDING_KEY, &owner) }.is_err() {
            match previous {
                Some(p) => {
                    s.recordings.insert(key, p);
                }
                None => {
                    s.recordings.remove(&key);
                }
            }
            return false;
        }
        HAVE_RECORDINGS.store(true, Ordering::Release);
        true
    })
}

/// HasCommandResource: `object` is in `list`'s current recording (not a prior submitted one).
pub fn has(list: &ID3D12GraphicsCommandList, object: *mut c_void) -> bool {
    if object.is_null() || !HAVE_RECORDINGS.load(Ordering::Acquire) {
        return false;
    }
    with_state(|s| {
        s.recordings
            .get(&(list.as_raw() as usize))
            .and_then(Weak::upgrade)
            .is_some_and(|r| r.batch.resources.iter().any(|o| o.as_raw() == object))
    })
}

/// PrepareCommandResources: the batches of `lists` that are recording, before they are executed.
pub fn prepare(lists: &[*mut c_void]) -> Submission {
    if !HAVE_RECORDINGS.load(Ordering::Acquire) {
        return Vec::new();
    }
    with_state(|s| lists.iter().filter_map(|l| s.recordings.get(&(*l as usize)).and_then(Weak::upgrade)).map(|r| r.batch.clone()).collect())
}

/// SubmitCommandResources: after the execute, `submission` flies until `queue`'s fence passes its value. Without a
/// fence (or when signalling fails: a removed device) it is kept for good and new recordings are refused.
pub fn submit(queue: Option<&ID3D12CommandQueue>, submission: Submission) {
    if submission.is_empty() {
        return;
    }
    with_state(|s| {
        collect(s);
        let key = queue.map_or(0, |q| q.as_raw() as usize);
        let state = s.queues.entry(key).or_insert_with(|| QueueState { _queue: None, fence: None, value: 0, flights: VecDeque::new() });
        if state.fence.is_none() {
            if let Some(q) = queue {
                // SAFETY: the live queue's device; a fence of its own.
                unsafe {
                    let mut device: Option<ID3D12Device> = None;
                    if let (Ok(()), Some(device)) = (q.GetDevice(&mut device), device.as_ref()) {
                        if let Ok(fence) = device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) {
                            let _ = fence.SetName(windows::core::w!("CVR_command_resource_lifetime"));
                            state._queue = Some(q.clone());
                            state.fence = Some(fence);
                        }
                    }
                }
            }
        }
        // SAFETY: the queue and its fence.
        let signalled = match (queue, state.fence.as_ref()) {
            (Some(q), Some(f)) => unsafe { q.Signal(f, state.value + 1) }.is_ok(),
            _ => false,
        };
        if !signalled {
            s.failed_submissions.push(submission);
            FAILED.store(true, Ordering::Relaxed);
            return;
        }
        state.value += 1;
        let value = state.value;
        state.flights.push_back(Flight { value, _submission: submission });
        HAVE_FLIGHTS.store(true, Ordering::Release);
    })
}

/// ResetCommandResources: after a successful Reset the list's recording is dropped (its submissions keep theirs).
pub fn reset(list: &ID3D12GraphicsCommandList) {
    if !HAVE_RECORDINGS.load(Ordering::Acquire) {
        return;
    }
    let tracked = with_state(|s| {
        let tracked = s.recordings.remove(&(list.as_raw() as usize)).is_some();
        HAVE_RECORDINGS.store(!s.recordings.is_empty(), Ordering::Release);
        tracked
    });
    if tracked {
        // SAFETY: clearing the list's private data releases the owner.
        let _ = unsafe { list.SetPrivateDataInterface(&RECORDING_KEY, None) };
    }
}

/// CollectCommandResources: the flights the GPU has finished released, expired recordings forgotten.
pub fn collect_all() {
    if !HAVE_FLIGHTS.load(Ordering::Acquire) && !HAVE_RECORDINGS.load(Ordering::Acquire) {
        return;
    }
    with_state(|s| {
        collect(s);
        prune(s);
    });
}

// ---- the C ABI (include/Render/CommandResources.hpp)

/// # Safety
/// `list` is an ID3D12GraphicsCommandList or null; `objects` holds `count` IUnknown pointers.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_keep_command_resources(list: *mut c_void, objects: *const *mut c_void, count: usize) -> bool {
    // SAFETY: the caller's list and object array.
    unsafe {
        let Some(list) = ID3D12GraphicsCommandList::from_raw_borrowed(&list) else { return false };
        let objects = if objects.is_null() { &[][..] } else { core::slice::from_raw_parts(objects, count) };
        keep(list, objects)
    }
}
/// # Safety
/// `list` is an ID3D12GraphicsCommandList or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_has_command_resource(list: *mut c_void, object: *mut c_void) -> bool {
    // SAFETY: the caller's list.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.is_some_and(|l| has(l, object))
}
/// The submission of `lists` (null when none of them is recording); freed by submitting it or cvr_gpu_free_submission.
///
/// # Safety
/// `lists` holds `count` ID3D12CommandList pointers (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_prepare_command_resources(count: u32, lists: *const *mut ID3D12CommandList) -> *mut Submission {
    if lists.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the caller's array.
    let lists = unsafe { core::slice::from_raw_parts(lists as *const *mut c_void, count as usize) };
    let s = prepare(lists);
    if s.is_empty() {
        core::ptr::null_mut()
    } else {
        Box::into_raw(Box::new(s))
    }
}
/// A copy of a submission (its batches shared), or null.
///
/// # Safety
/// `s` came from this module or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_clone_submission(s: *const Submission) -> *mut Submission {
    // SAFETY: the caller's submission.
    match unsafe { s.as_ref() } {
        Some(s) => Box::into_raw(Box::new(s.clone())),
        None => core::ptr::null_mut(),
    }
}
/// # Safety
/// `s` came from this module (and is not used again) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_free_submission(s: *mut Submission) {
    if !s.is_null() {
        // SAFETY: a submission this module boxed, freed once.
        drop(unsafe { Box::from_raw(s) });
    }
}
/// Takes ownership of `s`.
///
/// # Safety
/// `queue` is an ID3D12CommandQueue or null; `s` came from this module (and is not used again) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_submit_command_resources(queue: *mut c_void, s: *mut Submission) {
    if s.is_null() {
        return;
    }
    // SAFETY: a submission this module boxed, taken once; the caller's queue.
    unsafe {
        let s = *Box::from_raw(s);
        submit(ID3D12CommandQueue::from_raw_borrowed(&queue), s);
    }
}
/// # Safety
/// `list` is an ID3D12GraphicsCommandList or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_reset_command_resources(list: *mut c_void) {
    // SAFETY: the caller's list.
    if let Some(l) = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) } {
        reset(l);
    }
}
#[no_mangle]
pub extern "C" fn cvr_gpu_collect_command_resources() {
    collect_all();
}
