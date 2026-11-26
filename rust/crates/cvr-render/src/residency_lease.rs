//! The GPU residency lease's storage (src/Render/GpuResidencyLease.cpp, `gpu::residency::Lease`): the admission of an
//! acquisition (healthy accounting; no held or unfinished storage; a single-node device; 1..UINT_MAX sources), the
//! owners it makes resident (each pageable once by its COM identity), their one atomic MakeResident, the group
//! submission (a DIRECT queue; distinct, acquired members of the queue's device whose borrowed targets still
//! authenticate -- every member checked before any is committed), the fence association (the queue it was submitted
//! to, a fence of its device), cancellation and retirement (each owner evicted once; a failed Evict leaves the
//! accounting unhealthy), and the quarantine's accounting (unhealthy and counted; the storage is never released).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the native authentication of material sources and borrowed
//! targets (texture_read and target_lifetime), their records and the quarantined list; the unit test below runs the
//! lease on WARP, and tools/single_pass_tests/gpu_residency_lease_test runs it through the .cpp in its Rust variant.

use crate::residency_transaction::{state, Call, Transaction};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use windows::core::{IUnknown, Interface};
use windows::Win32::Graphics::Direct3D12::*;

const S_OK: i32 = 0;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_UNEXPECTED: i32 = 0x8000_FFFFu32 as i32;
const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;

static HEALTHY: AtomicBool = AtomicBool::new(true);
static QUARANTINED: AtomicU64 = AtomicU64::new(0);

/// Healthy: no uncertain accounting or abandoned acquisition so far.
pub fn healthy() -> bool {
    HEALTHY.load(Ordering::Acquire)
}
/// QuarantinedCount.
pub fn quarantined() -> u64 {
    QUARANTINED.load(Ordering::Acquire)
}

/// A lease's SDK residency.
pub struct Storage {
    pub transaction: Transaction,
    device: ID3D12Device,
    queue: Option<ID3D12CommandQueue>,
    fence: Option<ID3D12Fence>,
    owners: Vec<Option<ID3D12Pageable>>,
    identities: Vec<IUnknown>,
}

fn same_device(device: &ID3D12Device, child: &ID3D12DeviceChild) -> bool {
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live device child; the output a local.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.is_some_and(|a| a.as_raw() == device.as_raw())
}
/// SameOwner: two pageables of one COM identity.
pub fn same_owner(a: Option<&ID3D12Pageable>, b: Option<&ID3D12Pageable>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    match (a.cast::<IUnknown>(), b.cast::<IUnknown>()) {
        (Ok(first), Ok(second)) => first.as_raw() == second.as_raw(),
        _ => false,
    }
}

/// An acquisition's admission (Acquire / AcquireBorrowedTargets before any authentication).
pub fn admit(device: Option<&ID3D12Device>, count: usize, held: bool, terminal: u32) -> Result<Storage, i32> {
    if !healthy() {
        return Err(E_ACCESSDENIED);
    }
    let Some(device) = device else { return Err(E_INVALIDARG) };
    // SAFETY: a live device.
    if held || (terminal != state::EMPTY && terminal != state::RETIRED) || count == 0 || count > u32::MAX as usize || unsafe { device.GetNodeCount() } != 1 {
        return Err(E_INVALIDARG);
    }
    Ok(Storage { transaction: Transaction::default(), device: device.clone(), queue: None, fence: None, owners: Vec::new(), identities: Vec::new() })
}

impl Storage {
    /// AddOwner: a pageable kept once by its identity.
    pub fn add(&mut self, pageable: Option<&ID3D12Pageable>) -> i32 {
        let Some(pageable) = pageable else { return E_NOINTERFACE };
        let Ok(identity) = pageable.cast::<IUnknown>() else { return E_NOINTERFACE };
        if self.identities.iter().any(|saved| saved.as_raw() == identity.as_raw()) {
            return S_OK;
        }
        self.owners.push(Some(pageable.clone()));
        self.identities.push(identity);
        S_OK
    }
    pub fn objects(&self) -> usize {
        self.owners.len()
    }
    /// AcquireStorage's transaction: one MakeResident of every owner (all or nothing).
    pub fn acquire(&mut self) -> i32 {
        if !healthy() {
            return E_ACCESSDENIED;
        }
        let (device, owners) = (&self.device, &self.owners);
        self.transaction.acquire(owners.len(), || {
            // SAFETY: a live device; the owners live pageables (an Option<interface> is its pointer).
            Call::Returned(unsafe { (Interface::vtable(device).MakeResident)(device.as_raw(), owners.len() as u32, owners.as_ptr() as *const *mut c_void) }.0)
        })
    }
    /// AssociateFence (a submitted transaction).
    pub fn associate_fence(&mut self, queue: *mut c_void, fence: Option<&ID3D12Fence>, value: u64, signal_result: i32) -> i32 {
        if self.transaction.state != state::SUBMITTED {
            return E_UNEXPECTED;
        }
        let ours = self.queue.as_ref().map(|q| q.as_raw());
        let result = if signal_result < 0 {
            signal_result
        } else if queue.is_null() || ours != Some(queue) || !fence.is_some_and(|f| same_device(&self.device, f)) {
            E_INVALIDARG
        } else {
            signal_result
        };
        let hr = self.transaction.associate_fence(value, result);
        if hr >= 0 {
            self.fence = fence.cloned();
        }
        hr
    }
    /// Cancel: only an acquired transaction, every owner evicted once.
    pub fn cancel(&mut self) -> i32 {
        let (device, owners) = (&self.device, &self.owners);
        self.transaction.cancel(|index| evict(device, owners, index))
    }
    /// Retire: a submitted transaction without its fence quarantines; a fenced one releases at or past its signal.
    pub fn retire(&mut self) -> i32 {
        if self.transaction.state == state::SUBMITTED {
            self.transaction.quarantine();
            return E_UNEXPECTED;
        }
        let Some(fence) = self.fence.as_ref().filter(|_| self.transaction.state == state::FENCED) else { return E_ACCESSDENIED };
        // SAFETY: a live fence.
        let completed = unsafe { fence.GetCompletedValue() };
        let (device, owners) = (&self.device, &self.owners);
        self.transaction.retire(completed, |index| evict(device, owners, index))
    }
}
/// ReleaseOwner: one Evict; a failure leaves the accounting unhealthy.
fn evict(device: &ID3D12Device, owners: &[Option<ID3D12Pageable>], index: usize) -> Call {
    // SAFETY: a live device; one live pageable.
    let hr = unsafe { (Interface::vtable(device).Evict)(device.as_raw(), 1, &owners[index] as *const _ as *const *mut c_void) }.0;
    if hr < 0 {
        HEALTHY.store(false, Ordering::Release);
    }
    Call::Returned(hr)
}
/// Quarantine's accounting: false for an empty or retired transaction (released instead); otherwise unhealthy,
/// quarantined and counted.
pub fn quarantine(storage: &mut Storage) -> bool {
    let now = storage.transaction.state;
    if now == state::EMPTY || now == state::RETIRED {
        return false;
    }
    HEALTHY.store(false, Ordering::Release);
    storage.transaction.quarantine();
    QUARANTINED.fetch_add(1, Ordering::AcqRel);
    true
}

/// A group member: its lease's identity and storage (null when it holds none).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Member {
    pub lease: *const c_void,
    pub storage: *mut Storage,
}
/// MarkSubmittedTogether: every member checked (`current` re-authenticates a member's borrowed targets) before the
/// queue is retained and each transaction marked submitted.
///
/// # Safety
/// Each storage pointer is null or a live storage, distinct for distinct leases.
pub unsafe fn submit_together(queue: Option<&ID3D12CommandQueue>, members: &[Member], mut current: impl FnMut(usize) -> bool) -> i32 {
    let Some(queue) = queue else { return E_INVALIDARG };
    // SAFETY: a live queue.
    if members.is_empty() || members.len() > u32::MAX as usize || unsafe { queue.GetDesc() }.Type != D3D12_COMMAND_LIST_TYPE_DIRECT {
        return E_INVALIDARG;
    }
    for (i, member) in members.iter().enumerate() {
        if member.lease.is_null() || members[..i].iter().any(|other| other.lease == member.lease) {
            return E_INVALIDARG;
        }
    }
    for (i, member) in members.iter().enumerate() {
        // SAFETY: as documented.
        let Some(storage) = (unsafe { member.storage.as_ref() }) else { return E_INVALIDARG };
        if storage.transaction.state != state::ACQUIRED || !same_device(&storage.device, queue) {
            return E_INVALIDARG;
        }
        if !current(i) {
            return E_ACCESSDENIED;
        }
    }
    for member in members {
        // SAFETY: as documented; distinct storages.
        unsafe { (*member.storage).queue = Some(queue.clone()) };
    }
    for member in members {
        // SAFETY: as above.
        unsafe { (*member.storage).transaction.mark_submitted() };
    }
    S_OK
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

fn borrowed<T: Interface>(address: *mut c_void) -> Option<T> {
    // SAFETY: a live COM object the caller holds for the call (or null).
    unsafe { T::from_raw_borrowed(&address) }.cloned()
}
/// An acquisition's admission: a new storage into `out`.
///
/// # Safety
/// `device` is null or live; `out` a pointer.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_begin(device: *mut c_void, count: usize, held: bool, terminal: u32, out: *mut *mut Storage) -> i32 {
    match admit(borrowed::<ID3D12Device>(device).as_ref(), count, held, terminal) {
        Ok(storage) => {
            // SAFETY: as documented.
            unsafe { *out = Box::into_raw(Box::new(storage)) };
            S_OK
        }
        Err(hr) => hr,
    }
}
/// Releases a storage (null: nothing).
///
/// # Safety
/// From cvr_rl_begin, once.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_free(storage: *mut Storage) {
    if !storage.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(storage) });
    }
}
/// AddOwner.
///
/// # Safety
/// A live storage; `pageable` null or live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_add(storage: *mut Storage, pageable: *mut c_void) -> i32 {
    // SAFETY: as documented.
    unsafe { (*storage).add(borrowed::<ID3D12Pageable>(pageable).as_ref()) }
}
/// AcquireStorage's MakeResident.
///
/// # Safety
/// A live storage.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_acquire(storage: *mut Storage) -> i32 {
    // SAFETY: as documented.
    unsafe { (*storage).acquire() }
}
/// The owners' count.
///
/// # Safety
/// A live storage.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_objects(storage: *const Storage) -> usize {
    // SAFETY: as documented.
    unsafe { (*storage).objects() }
}
/// The transaction's state.
///
/// # Safety
/// A live storage.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_status(storage: *const Storage) -> u32 {
    // SAFETY: as documented.
    unsafe { (*storage).transaction.state }
}
/// MarkSubmittedTogether (`current(context, index)` re-authenticates member `index`'s borrowed targets).
///
/// # Safety
/// As submit_together; `members` holds `count` members.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_submit(
    queue: *mut c_void,
    members: *const Member,
    count: usize,
    current: unsafe extern "C-unwind" fn(*mut c_void, usize) -> bool,
    context: *mut c_void,
) -> i32 {
    let members = if members.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(members, count) } };
    // SAFETY: as documented.
    unsafe { submit_together(borrowed::<ID3D12CommandQueue>(queue).as_ref(), members, |index| current(context, index)) }
}
/// AssociateFence.
///
/// # Safety
/// A live storage; `queue` and `fence` null or live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_fence(storage: *mut Storage, queue: *mut c_void, fence: *mut c_void, value: u64, signal_result: i32) -> i32 {
    // SAFETY: as documented.
    unsafe { (*storage).associate_fence(queue, borrowed::<ID3D12Fence>(fence).as_ref(), value, signal_result) }
}
/// Cancel (`which` 0) or Retire (1).
///
/// # Safety
/// A live storage.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_release(storage: *mut Storage, which: u32) -> i32 {
    // SAFETY: as documented.
    let storage = unsafe { &mut *storage };
    if which == 0 { storage.cancel() } else { storage.retire() }
}
/// Quarantine's accounting (false: release the storage instead).
///
/// # Safety
/// A live storage.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_quarantine(storage: *mut Storage) -> bool {
    // SAFETY: as documented.
    quarantine(unsafe { &mut *storage })
}
/// Healthy.
#[no_mangle]
pub extern "C-unwind" fn cvr_rl_healthy() -> bool {
    healthy()
}
/// QuarantinedCount.
#[no_mangle]
pub extern "C-unwind" fn cvr_rl_quarantined() -> u64 {
    quarantined()
}
/// SameOwner.
///
/// # Safety
/// Each null or live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_rl_same_owner(a: *mut c_void, b: *mut c_void) -> bool {
    same_owner(borrowed::<ID3D12Pageable>(a).as_ref(), borrowed::<ID3D12Pageable>(b).as_ref())
}

#[cfg(test)]
mod tests {
    //! A lease on WARP: the admission, the owners kept once, the group's preflight and commit, the fence, retirement
    //! and cancellation. (The accounting stays healthy: nothing here quarantines.)
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
    fn buffer(device: &ID3D12Device) -> ID3D12Pageable {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 256,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let mut resource: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COMMON, None, &mut resource) }.unwrap();
        resource.unwrap().cast().unwrap()
    }
    fn queue(device: &ID3D12Device, kind: D3D12_COMMAND_LIST_TYPE) -> ID3D12CommandQueue {
        unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: kind, ..Default::default() }) }.unwrap()
    }
    #[test]
    fn lease() {
        let device = warp();
        assert_eq!(admit(Some(&device), 0, false, state::EMPTY).err(), Some(E_INVALIDARG), "no sources");
        assert_eq!(admit(Some(&device), 1, true, state::EMPTY).err(), Some(E_INVALIDARG), "a held storage");
        assert_eq!(admit(Some(&device), 1, false, state::QUARANTINED).err(), Some(E_INVALIDARG), "an unfinished lease");
        assert_eq!(admit(None, 1, false, state::EMPTY).err(), Some(E_INVALIDARG), "no device");
        let (a, b) = (buffer(&device), buffer(&device));
        let mut first = admit(Some(&device), 3, false, state::RETIRED).unwrap();
        assert_eq!(first.add(None), E_NOINTERFACE);
        for pageable in [&a, &b, &a] {
            assert_eq!(first.add(Some(pageable)), S_OK);
        }
        assert_eq!(first.objects(), 2, "one owner per COM identity");
        assert!(same_owner(Some(&a), Some(&a.cast::<ID3D12Resource>().unwrap().cast().unwrap())) && !same_owner(Some(&a), Some(&b)));
        assert_eq!(first.acquire(), S_OK);
        assert_eq!(first.transaction.state, state::ACQUIRED);
        let mut second = admit(Some(&device), 1, false, state::EMPTY).unwrap();
        second.add(Some(&b));
        let direct = queue(&device, D3D12_COMMAND_LIST_TYPE_DIRECT);
        let lease = |n: usize| n as *const c_void;
        let members = [Member { lease: lease(1), storage: &mut first }, Member { lease: lease(2), storage: &mut second }];
        assert_eq!(unsafe { submit_together(Some(&direct), &members, |_| true) }, E_INVALIDARG, "an unacquired member");
        assert_eq!(first.transaction.state, state::ACQUIRED, "nothing committed");
        assert_eq!(second.acquire(), S_OK);
        let members = [Member { lease: lease(1), storage: &mut first }, Member { lease: lease(2), storage: &mut second }];
        let copy = queue(&device, D3D12_COMMAND_LIST_TYPE_COPY);
        assert_eq!(unsafe { submit_together(Some(&copy), &members, |_| true) }, E_INVALIDARG, "not a DIRECT queue");
        assert_eq!(unsafe { submit_together(Some(&direct), &[members[0], members[0]], |_| true) }, E_INVALIDARG, "a member twice");
        assert_eq!(unsafe { submit_together(Some(&direct), &members, |index| index == 0) }, E_ACCESSDENIED, "a target no longer authenticates");
        assert_eq!((first.transaction.state, second.transaction.state), (state::ACQUIRED, state::ACQUIRED), "every member checked first");
        assert_eq!(unsafe { submit_together(Some(&direct), &members, |_| true) }, S_OK);
        assert_eq!((first.transaction.state, second.transaction.state), (state::SUBMITTED, state::SUBMITTED));
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        assert_eq!(first.associate_fence(copy.as_raw(), Some(&fence), 1, S_OK), E_INVALIDARG, "another queue");
        assert_eq!(first.transaction.state, state::QUARANTINED, "a failed association quarantines");
        assert_eq!(second.associate_fence(direct.as_raw(), Some(&fence), 1, S_OK), S_OK);
        assert_eq!(second.retire(), 1, "pending: S_FALSE");
        unsafe { direct.Signal(&fence, 1) }.unwrap();
        while unsafe { fence.GetCompletedValue() } < 1 {}
        assert_eq!((second.retire(), second.transaction.state), (S_OK, state::RETIRED));
        assert_eq!(second.retire(), E_ACCESSDENIED, "retired once");
        let mut third = admit(Some(&device), 1, false, state::EMPTY).unwrap();
        third.add(Some(&a));
        assert_eq!(third.acquire(), S_OK);
        assert_eq!((third.cancel(), third.transaction.state, third.transaction.attempted), (S_OK, state::RETIRED, 1));
    }
}
