//! The instance snapshot view's GPU half (src/Render/InstanceSnapshotView.cpp): the CPU-upload overlay and the native
//! GPU capture arena.
//!
//! The overlay: created on a single-node device with each version's pipeline on it, its DEFAULT buffer of the plan's
//! width in COPY_DEST and its UPLOAD buffer of the copied bytes, the DEFAULT range addressable, the versions' bytes
//! uploaded at their offsets; recorded once on a certified current DIRECT recording whose list, allocator and queue are
//! of its device -- the versions still qualified, the overlay, the allocator and the queue kept by the list -- each
//! version's copy into its target offset and the whole buffer to VERTEX_AND_CONSTANT_BUFFER | COPY_SOURCE; its
//! destination remembered for the submission, the same recording's consumer and a completed later consumer.
//!
//! The arena: created on a single-node device for a complete native recording of one eye, a DEFAULT buffer of at most
//! half the view budget in COPY_DEST; a capture admitted on that recording's DIRECT list for a valid scope of it and a
//! pipeline of its device, reserved after the cursor, the arena kept by the list, the stage Recording while the source
//! callback records -- then the cursor, width and copied bytes advanced, or the arena failed; finalized to GENERIC_READ
//! on that list with a capture; its views checked for the same recording's consumer, after a certified submission on a
//! DIRECT queue of its device, and after that fence completed.
//!
//! Both owners' views: recorded, the owner's buffer, the version's scope, view at its mapped offset, original view,
//! draw and mapping, and the copies' recording.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the plan's versions (their pipelines and shader snapshots) and
//! wraps this core in its owner objects; tools/single_pass_tests/instance_snapshot_view_test runs its WARP cases against
//! it in its Rust variant, and the unit tests below run the core on WARP.

use core::ffi::c_void;
use core::mem::ManuallyDrop;
use core::sync::atomic::{AtomicU32, Ordering::*};
use std::sync::Mutex;

use windows::core::{IUnknown, Interface, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, S_OK};
use windows::Win32::Graphics::Direct3D12::*;

use crate::instance_snapshot_view::{reserve, valid_scope, Mapping, Scope, VertexBufferView, MAX_BYTES};
use crate::native_recording::Lease;

pub const MAX_VERSIONS: usize = 2048;
pub const CREATED: u32 = 0;
pub const RECORDING: u32 = 1;
pub const RECORDED: u32 = 2;
pub const FAILED: u32 = 3;
// instance_snapshot_view::Failure.
pub const NONE: u32 = 0;
pub const RECORDING_FAILURE: u32 = 9;
pub const ALREADY_RECORDED: u32 = 10;
pub const RETENTION: u32 = 11;
pub const CHANGED: u32 = 12;

/// D3D12_DRAW_INDEXED_ARGUMENTS.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawArgs {
    pub index_count: u32,
    pub instance_count: u32,
    pub start_index: u32,
    pub base_vertex: i32,
    pub start_instance: u32,
}
/// DestinationRecording, lent.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Destination {
    pub list: u64,
    pub generation: u64,
    pub allocator: *mut c_void,
    pub queue: *mut c_void,
    pub certified: bool,
}
/// DestinationCompletion, lent.
#[repr(C)]
pub struct Completion {
    pub recording: Destination,
    pub fence: *mut c_void,
    pub value: u64,
    pub certified: bool,
}
/// NativeCompletion, lent.
#[repr(C)]
pub struct NativeCompletion {
    pub recording: Lease,
    pub queue: *mut c_void,
    pub fence: *mut c_void,
    pub value: u64,
    pub certified: bool,
}
/// PrivateView, lent (its shader snapshot opaque).
#[repr(C)]
pub struct PrivateView {
    pub scope: Scope,
    pub resource: *mut c_void,
    pub owner: *mut c_void,
    pub pipeline: *mut c_void,
    pub shader: [usize; 2],
    pub vertex: VertexBufferView,
    pub original: VertexBufferView,
    pub draw: DrawArgs,
    pub mapping: Mapping,
    pub copy_list: u64,
    pub copy_generation: u64,
    pub version: u32,
}
/// A version's coordinates.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VersionView {
    pub scope: Scope,
    pub mapping: Mapping,
    pub original: VertexBufferView,
    pub draw: DrawArgs,
}
/// An overlay version's upload: its copy and bytes, and its pipeline.
#[repr(C)]
pub struct Upload {
    pub target: u64,
    pub upload: u64,
    pub bytes: u64,
    pub data: *const u8,
    pub size: usize,
    pub pipeline: *mut c_void,
}
/// The owner's buffer and its copies' recording.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Label {
    pub resource: *mut c_void,
    pub list: u64,
    pub generation: u64,
}
const _: () = assert!(
    core::mem::size_of::<DrawArgs>() == 20
        && core::mem::size_of::<Destination>() == 40
        && core::mem::size_of::<Completion>() == 64
        && core::mem::size_of::<NativeCompletion>() == 72
        && core::mem::size_of::<PrivateView>() == 208
        && core::mem::offset_of!(PrivateView, mapping) == 136
        && core::mem::offset_of!(PrivateView, version) == 200
        && core::mem::size_of::<VersionView>() == 128
        && core::mem::size_of::<Upload>() == 48
        && core::mem::size_of::<Label>() == 24
);

#[derive(Default)]
struct HeldDestination {
    list: u64,
    generation: u64,
    allocator: Option<ID3D12CommandAllocator>,
    queue: Option<ID3D12CommandQueue>,
}
struct HeldLease {
    token: Option<IUnknown>,
    generation: u64,
    context: u64,
    list: u64,
    registry: u64,
}
#[derive(Default)]
struct Counts {
    cursor: u64,
    width: u64,
    copied: u64,
}
/// An overlay's or an arena's GPU half.
pub struct Core {
    stage: AtomicU32,
    device: ID3D12Device,
    resource: ID3D12Resource,
    upload: Option<ID3D12Resource>,
    copies: Vec<(u64, u64, u64)>,
    destination: Mutex<HeldDestination>,
    recording: Option<HeldLease>,
    eye: u32,
    capacity: u64,
    counts: Mutex<Counts>,
}

fn borrowed<T: Interface>(raw: *mut c_void) -> Option<T> {
    // SAFETY: a live interface of that kind or null, borrowed for the call.
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}
fn raw<T: Interface>(object: &Option<T>) -> *mut c_void {
    object.as_ref().map_or(core::ptr::null_mut(), |o| o.as_raw())
}
/// SameDevice: a device child of `device`.
pub fn same_device(device: &ID3D12Device, child: *mut c_void) -> bool {
    let Some(child) = borrowed::<ID3D12DeviceChild>(child) else { return false };
    let mut actual: Option<ID3D12Device> = None;
    // SAFETY: a live device child; the output a local.
    unsafe { child.GetDevice(&mut actual) }.is_ok() && actual.map(|d| d.as_raw()) == Some(device.as_raw())
}
fn buffer(device: &ID3D12Device, bytes: u64, heap: D3D12_HEAP_TYPE, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource, HRESULT> {
    let desc = crate::readback_buffers::buffer_desc(bytes);
    let properties = D3D12_HEAP_PROPERTIES { Type: heap, CreationNodeMask: 1, VisibleNodeMask: 1, ..Default::default() };
    let mut out: Option<ID3D12Resource> = None;
    // SAFETY: a live device; the descriptions are locals.
    unsafe { device.CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut out) }.map_err(|e| e.code())?;
    out.ok_or(E_FAIL)
}
fn transition(list: &ID3D12GraphicsCommandList, resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) {
    let pointer = resource.as_raw();
    // SAFETY: the barrier borrows the resource's interface pointer for the call (no reference is taken or dropped).
    unsafe {
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: core::mem::transmute_copy(&pointer),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: before,
                    StateAfter: after,
                }),
            },
        };
        list.ResourceBarrier(&[barrier]);
    }
}
fn direct_list(list: &ID3D12GraphicsCommandList) -> bool {
    // SAFETY: a live list.
    (unsafe { list.GetType() }) == D3D12_COMMAND_LIST_TYPE_DIRECT
}
fn direct_queue(queue: *mut c_void) -> bool {
    // SAFETY: a live queue.
    borrowed::<ID3D12CommandQueue>(queue).is_some_and(|q| unsafe { q.GetDesc() }.Type == D3D12_COMMAND_LIST_TYPE_DIRECT)
}
fn keep(list: &ID3D12GraphicsCommandList, objects: &[*mut c_void]) -> bool {
    cvr_gpu::command_resources::keep(list, objects)
}
fn completed(fence: *mut c_void, value: u64) -> bool {
    // SAFETY: a live fence.
    borrowed::<ID3D12Fence>(fence).is_some_and(|f| {
        let done = unsafe { f.GetCompletedValue() };
        done != u64::MAX && done >= value
    })
}
fn same_view(a: &VertexBufferView, b: &VertexBufferView) -> bool {
    a.buffer_location == b.buffer_location && a.size_in_bytes == b.size_in_bytes && a.stride_in_bytes == b.stride_in_bytes
}
fn same_recording(a: &Lease, b: &HeldLease) -> bool {
    !a.token.is_null()
        && a.token == raw(&b.token)
        && a.list != 0
        && a.list == b.list
        && a.generation != 0
        && a.generation == b.generation
        && a.context != 0
        && a.context == b.context
        && a.registry != 0
        && a.registry == b.registry
}
fn complete_lease(a: &Lease) -> bool {
    !a.token.is_null() && a.list != 0 && a.generation != 0 && a.context != 0 && a.registry != 0
}

impl Core {
    fn new(device: &ID3D12Device, resource: ID3D12Resource) -> Core {
        Core {
            stage: AtomicU32::new(CREATED),
            device: device.clone(),
            resource,
            upload: None,
            copies: Vec::new(),
            destination: Mutex::new(HeldDestination::default()),
            recording: None,
            eye: 0,
            capacity: 0,
            counts: Mutex::new(Counts::default()),
        }
    }
    /// The overlay: its buffers and uploads.
    pub fn overlay(device: &ID3D12Device, required: u64, copied: u64, uploads: &[Upload]) -> Result<Core, HRESULT> {
        // SAFETY: a live device.
        if unsafe { device.GetNodeCount() } != 1 || uploads.iter().any(|u| !same_device(device, u.pipeline)) {
            return Err(E_INVALIDARG);
        }
        let (resource, upload) = {
            let _internal = cvr_host::probe::Internal::new();
            let resource = buffer(device, required, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST)?;
            (resource, buffer(device, copied, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ)?)
        };
        // SAFETY: a live buffer.
        let gpu = unsafe { resource.GetGPUVirtualAddress() };
        if gpu == 0 || gpu > u64::MAX - required {
            return Err(E_INVALIDARG);
        }
        let mut mapped = core::ptr::null_mut();
        // SAFETY: a live UPLOAD buffer of `copied` bytes.
        unsafe { upload.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut mapped)) }.map_err(|e| e.code())?;
        if mapped.is_null() {
            // SAFETY: as above.
            unsafe { upload.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
            return Err(E_FAIL);
        }
        for u in uploads {
            if u.size != 0 {
                // SAFETY: the plan's bytes land inside the upload at its offsets (the plan placed them so).
                unsafe { core::ptr::copy_nonoverlapping(u.data, mapped.cast::<u8>().add(u.upload as usize), u.size) };
            }
        }
        // SAFETY: as above.
        unsafe { upload.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: copied as usize })) };
        let mut core = Core::new(device, resource);
        core.upload = Some(upload);
        core.copies = uploads.iter().map(|u| (u.target, u.upload, u.bytes)).collect();
        Ok(core)
    }
    /// The overlay's recording (its failure, NONE when recorded); `owner` is the overlay object the list keeps.
    pub fn record(&self, list: &ID3D12GraphicsCommandList, owner: *mut c_void, destination: &Destination, qualified: bool) -> u32 {
        let list_raw = list.as_raw() as u64;
        if !destination.certified
            || destination.generation == 0
            || destination.list != list_raw
            || destination.allocator.is_null()
            || destination.queue.is_null()
            || !direct_list(list)
            || !direct_queue(destination.queue)
            || !same_device(&self.device, list.as_raw())
            || !same_device(&self.device, destination.allocator)
            || !same_device(&self.device, destination.queue)
        {
            return RECORDING_FAILURE;
        }
        if self.stage.load(Acquire) != CREATED {
            return ALREADY_RECORDED;
        }
        if !qualified {
            return CHANGED;
        }
        if !keep(list, &[owner, destination.allocator, destination.queue]) {
            return RETENTION;
        }
        if self.stage.compare_exchange(CREATED, RECORDING, AcqRel, Acquire).is_err() {
            return ALREADY_RECORDED;
        }
        *self.destination.lock().unwrap() = HeldDestination {
            list: destination.list,
            generation: destination.generation,
            allocator: borrowed(destination.allocator),
            queue: borrowed(destination.queue),
        };
        {
            let _internal = cvr_host::probe::Internal::new();
            for &(target, upload, bytes) in &self.copies {
                // SAFETY: a live recording list; both buffers live, the ranges the plan's.
                unsafe { list.CopyBufferRegion(&self.resource, target, self.upload.as_ref(), upload, bytes) };
            }
            transition(list, &self.resource, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER | D3D12_RESOURCE_STATE_COPY_SOURCE);
        }
        self.stage.store(RECORDED, Release);
        NONE
    }
    /// A failure while recording: the overlay fails only while Recording, the arena always.
    pub fn fail(&self, always: bool) {
        if always || self.stage.load(SeqCst) == RECORDING {
            self.stage.store(FAILED, Release);
        }
    }
    /// The buffer and the copies' recording; whether recorded.
    pub fn label(&self) -> (Label, bool) {
        let (list, generation) = match &self.recording {
            Some(recording) => (recording.list, recording.generation),
            None => {
                let held = self.destination.lock().unwrap();
                (held.list, held.generation)
            }
        };
        (Label { resource: self.resource.as_raw(), list, generation }, self.stage.load(Acquire) == RECORDED)
    }
    /// A view of a version: recorded, this buffer, the version's coordinates and the copies' recording.
    pub fn view_valid(&self, view: &PrivateView, version: &VersionView) -> bool {
        let (label, recorded) = self.label();
        // SAFETY: a live buffer.
        let expected = VertexBufferView {
            buffer_location: unsafe { self.resource.GetGPUVirtualAddress() }.wrapping_add(version.mapping.view_offset),
            ..version.original
        };
        recorded
            && view.resource == label.resource
            && view.scope == version.scope
            && same_view(&view.vertex, &expected)
            && same_view(&view.original, &version.original)
            && view.draw == version.draw
            && view.mapping == version.mapping
            && view.copy_list == label.list
            && view.copy_generation == label.generation
    }
    /// The overlay's submission on `queue` of `destination`.
    pub fn submitted(&self, destination: &Destination, queue: *mut c_void) -> bool {
        let held = self.destination.lock().unwrap();
        self.stage.load(Acquire) == RECORDED
            && destination.certified
            && destination.list == held.list
            && destination.generation == held.generation
            && destination.allocator == raw(&held.allocator)
            && destination.queue == raw(&held.queue)
            && queue == raw(&held.queue)
            && same_device(&self.device, queue)
    }
    /// The overlay view's consumer on `list` of `destination` (the view checked already).
    pub fn for_recording(&self, view: &PrivateView, list: *mut c_void, owner: *mut c_void, destination: &Destination) -> bool {
        let Some(commands) = borrowed::<ID3D12GraphicsCommandList>(list) else { return false };
        let (allocator, queue) = {
            let held = self.destination.lock().unwrap();
            (raw(&held.allocator), raw(&held.queue))
        };
        view.copy_list == list as u64
            && destination.list == view.copy_list
            && destination.generation == view.copy_generation
            && destination.certified
            && destination.allocator == allocator
            && destination.queue == queue
            && cvr_gpu::command_resources::has(&commands, owner)
            && same_device(&self.device, list)
    }
    /// The overlay's copies completed for a later consumer.
    pub fn after_completion(&self, completion: &Completion) -> bool {
        if !completion.certified || completion.fence.is_null() || completion.value == 0 {
            return false;
        }
        let recording = &completion.recording;
        {
            let held = self.destination.lock().unwrap();
            if !recording.certified
                || recording.list != held.list
                || recording.generation != held.generation
                || recording.allocator != raw(&held.allocator)
                || recording.queue != raw(&held.queue)
                || !same_device(&self.device, completion.fence)
            {
                return false;
            }
        }
        completed(completion.fence, completion.value)
    }

    /// The arena: its buffer for `recording`'s `eye`.
    pub fn arena(device: &ID3D12Device, recording: &Lease, eye: u32, capacity: u64) -> Result<Core, HRESULT> {
        // SAFETY: a live device.
        if unsafe { device.GetNodeCount() } != 1 || !complete_lease(recording) || eye > 1 || capacity == 0 || capacity > MAX_BYTES / 2 {
            return Err(E_INVALIDARG);
        }
        let resource = {
            let _internal = cvr_host::probe::Internal::new();
            buffer(device, capacity, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COPY_DEST)?
        };
        let mut core = Core::new(device, resource);
        core.recording = Some(HeldLease {
            token: borrowed(recording.token),
            generation: recording.generation,
            context: recording.context,
            list: recording.list,
            registry: recording.registry,
        });
        core.eye = eye;
        core.capacity = capacity;
        Ok(core)
    }
    /// A capture's admission and reservation (the stage then Recording); `versions` the captures so far, `bytes`
    /// whether the input carries a CPU observation.
    #[allow(clippy::too_many_arguments)]
    pub fn admit(&self, list: &ID3D12GraphicsCommandList, owner: *mut c_void, scope: &Scope, pipeline: *mut c_void, original: &VertexBufferView, draw: &DrawArgs, versions: usize, bytes: bool) -> Option<Mapping> {
        let recording = self.recording.as_ref()?;
        if self.stage.load(SeqCst) != CREATED
            || versions >= MAX_VERSIONS
            || bytes
            || !valid_scope(scope)
            || scope.list != recording.list
            || scope.generation != recording.generation
            || scope.eye != self.eye
            || list.as_raw() as u64 != recording.list
            || !direct_list(list)
            || !same_device(&self.device, list.as_raw())
            || pipeline.is_null()
            || draw.index_count == 0
            || !same_device(&self.device, pipeline)
        {
            return None;
        }
        let cursor = self.counts.lock().unwrap().cursor;
        let mapping = reserve(cursor, original, draw.start_instance, draw.instance_count, self.capacity)?;
        if !keep(list, &[owner]) {
            return None;
        }
        self.stage.store(RECORDING, SeqCst);
        Some(mapping)
    }
    /// A capture's end: recorded, the cursor, width and copied bytes advanced; else the arena failed.
    pub fn commit(&self, mapping: Option<&Mapping>) {
        match mapping {
            Some(m) => {
                let mut counts = self.counts.lock().unwrap();
                counts.cursor = m.next_free;
                counts.width = counts.width.max(m.view_end);
                counts.copied = counts.copied.wrapping_add(m.copy_bytes);
                self.stage.store(CREATED, SeqCst);
            }
            None => self.stage.store(FAILED, SeqCst),
        }
    }
    /// The arena's finalization on its recording's list.
    pub fn finalize(&self, list: &ID3D12GraphicsCommandList, owner: *mut c_void, versions: usize) -> bool {
        let Some(recording) = self.recording.as_ref() else { return false };
        if self.stage.load(SeqCst) != CREATED
            || versions == 0
            || list.as_raw() as u64 != recording.list
            || !direct_list(list)
            || !same_device(&self.device, list.as_raw())
            || !keep(list, &[owner])
        {
            return false;
        }
        let _internal = cvr_host::probe::Internal::new();
        transition(list, &self.resource, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_GENERIC_READ);
        self.stage.store(RECORDED, SeqCst);
        true
    }
    /// The arena's width (0) or copied bytes (1).
    pub fn bytes(&self, which: u32) -> u64 {
        let counts = self.counts.lock().unwrap();
        if which == 0 {
            counts.width
        } else {
            counts.copied
        }
    }
    /// The arena view's consumer on `list` of `recording` (the view checked already).
    pub fn native_for_recording(&self, list: *mut c_void, owner: *mut c_void, recording: &Lease) -> bool {
        let (Some(held), Some(commands)) = (self.recording.as_ref(), borrowed::<ID3D12GraphicsCommandList>(list)) else { return false };
        same_recording(recording, held) && cvr_gpu::command_resources::has(&commands, owner) && same_device(&self.device, list)
    }
    /// The arena view after its certified submission (and, with `complete`, its fence completed).
    pub fn native_submitted(&self, completion: &NativeCompletion, complete: bool) -> bool {
        let Some(held) = self.recording.as_ref() else { return false };
        completion.certified
            && completion.value != 0
            && completion.value != u64::MAX
            && !completion.queue.is_null()
            && !completion.fence.is_null()
            && same_recording(&completion.recording, held)
            && same_device(&self.device, completion.fence)
            && same_device(&self.device, completion.queue)
            && direct_queue(completion.queue)
            && (!complete || completed(completion.fence, completion.value))
    }
}

/// # Safety
/// `core` is a live core.
unsafe fn core_of<'a>(core: *const c_void) -> &'a Core {
    // SAFETY: as documented.
    unsafe { &*(core as *const Core) }
}
fn list_of(list: *mut c_void) -> Option<ID3D12GraphicsCommandList> {
    borrowed(list)
}
fn hresult(result: Result<Core, HRESULT>, out: *mut *mut c_void) -> HRESULT {
    match result {
        Ok(core) => {
            // SAFETY: the caller's slot.
            unsafe { *out = Box::into_raw(Box::new(core)).cast() };
            S_OK
        }
        Err(hr) => hr,
    }
}

/// The overlay's core into `out`.
///
/// # Safety
/// A live device; `uploads` holds `count`; `out` a slot.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_overlay_new(device: *mut c_void, required: u64, copied: u64, uploads: *const Upload, count: usize, out: *mut *mut c_void) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    let uploads = if uploads.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(uploads, count) } };
    hresult(Core::overlay(&device, required, copied, uploads), out)
}
/// The arena's core into `out`.
///
/// # Safety
/// A live device; `recording` a lease; `out` a slot.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_arena_new(device: *mut c_void, recording: *const Lease, eye: u32, capacity: u64, out: *mut *mut c_void) -> HRESULT {
    // SAFETY: as documented.
    let (Some(device), Some(recording)) = (borrowed::<ID3D12Device>(device), unsafe { recording.as_ref() }) else { return E_INVALIDARG };
    hresult(Core::arena(&device, recording, eye, capacity), out)
}
/// Frees a core (null allowed).
///
/// # Safety
/// `core` is null or from cvr_isv_*_new, not used afterwards.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_free(core: *mut c_void) {
    if !core.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Box::from_raw(core as *mut Core) });
    }
}
/// The overlay's recording.
///
/// # Safety
/// A live core and list; `destination` lent.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_overlay_record(core: *mut c_void, list: *mut c_void, owner: *mut c_void, destination: *const Destination, qualified: bool) -> u32 {
    let Some(list) = list_of(list) else { return 1 };
    // SAFETY: as documented.
    unsafe { core_of(core).record(&list, owner, &*destination, qualified) }
}
/// A failure while recording.
///
/// # Safety
/// A live core.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_fail(core: *mut c_void, always: bool) {
    // SAFETY: as documented.
    unsafe { core_of(core) }.fail(always);
}
/// The label; whether recorded.
///
/// # Safety
/// A live core; `out` a label.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_label(core: *const c_void, out: *mut Label) -> bool {
    // SAFETY: as documented.
    let (label, recorded) = unsafe { core_of(core) }.label();
    unsafe { *out = label };
    recorded
}
/// A view of a version against its owner's core.
///
/// # Safety
/// A live core; `view` and `version` lent.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_view_valid(core: *const c_void, view: *const PrivateView, version: *const VersionView) -> bool {
    // SAFETY: as documented.
    unsafe { core_of(core).view_valid(&*view, &*version) }
}
/// The overlay's checks: 0 its submission on `queue`, 1 its view's consumer on `list`, 2 a completed later consumer.
///
/// # Safety
/// A live core; the pointers the kind reads are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_overlay_valid(core: *const c_void, kind: u32, destination: *const Destination, queue: *mut c_void, list: *mut c_void, owner: *mut c_void, view: *const PrivateView, completion: *const Completion) -> bool {
    // SAFETY: as documented.
    let core = unsafe { core_of(core) };
    match kind {
        0 => unsafe { core.submitted(&*destination, queue) },
        1 => unsafe { core.for_recording(&*view, list, owner, &*destination) },
        _ => unsafe { core.after_completion(&*completion) },
    }
}
/// A capture's admission into `mapping`.
///
/// # Safety
/// A live core and list; the scope, views and draw lent.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_arena_admit(core: *mut c_void, list: *mut c_void, owner: *mut c_void, scope: *const Scope, pipeline: *mut c_void, original: *const VertexBufferView, draw: *const DrawArgs, versions: usize, bytes: bool, mapping: *mut Mapping) -> bool {
    let Some(list) = list_of(list) else { return false };
    // SAFETY: as documented.
    match unsafe { core_of(core).admit(&list, owner, &*scope, pipeline, &*original, &*draw, versions, bytes) } {
        Some(m) => {
            unsafe { *mapping = m };
            true
        }
        None => false,
    }
}
/// A capture's end (null: failed).
///
/// # Safety
/// A live core; `mapping` null or lent.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_arena_commit(core: *mut c_void, mapping: *const Mapping) {
    // SAFETY: as documented.
    unsafe { core_of(core).commit(mapping.as_ref()) };
}
/// The arena's finalization.
///
/// # Safety
/// A live core and list.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_arena_finalize(core: *mut c_void, list: *mut c_void, owner: *mut c_void, versions: usize) -> bool {
    let Some(list) = list_of(list) else { return false };
    // SAFETY: as documented.
    unsafe { core_of(core) }.finalize(&list, owner, versions)
}
/// The arena's width (0) or copied bytes (1).
///
/// # Safety
/// A live core.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_arena_bytes(core: *const c_void, which: u32) -> u64 {
    // SAFETY: as documented.
    unsafe { core_of(core) }.bytes(which)
}
/// The arena's checks: 1 its view's consumer on `list` of `recording`, 2 after a submission, 3 after its completion.
///
/// # Safety
/// A live core; the pointers the kind reads are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_isv_arena_valid(core: *const c_void, kind: u32, list: *mut c_void, owner: *mut c_void, recording: *const Lease, completion: *const NativeCompletion) -> bool {
    // SAFETY: as documented.
    let core = unsafe { core_of(core) };
    match kind {
        1 => unsafe { core.native_for_recording(list, owner, &*recording) },
        2 => unsafe { core.native_submitted(&*completion, false) },
        _ => unsafe { core.native_submitted(&*completion, true) },
    }
}

#[cfg(test)]
mod tests {
    //! The overlay and the arena on WARP: created, recorded once, the views and the checks.
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
    fn direct(device: &ID3D12Device) -> (ID3D12CommandQueue, ID3D12CommandAllocator, ID3D12GraphicsCommandList) {
        let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }) }.unwrap();
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        (queue, allocator, list)
    }
    fn fence_done(device: &ID3D12Device, queue: &ID3D12CommandQueue) -> ID3D12Fence {
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        unsafe { queue.Signal(&fence, 1) }.unwrap();
        while unsafe { fence.GetCompletedValue() } < 1 {
            std::thread::yield_now();
        }
        fence
    }

    #[test]
    fn overlay_records_once() {
        let device = warp();
        let (queue, allocator, list) = direct(&device);
        let pipeline_owner = device.clone();
        // A device child for the pipeline's device check: the allocator.
        let bytes = [7u8; 96];
        let uploads = [Upload { target: 0, upload: 0, bytes: 96, data: bytes.as_ptr(), size: 96, pipeline: allocator.as_raw() }];
        assert_eq!(Core::overlay(&device, 1024, 96, &[Upload { pipeline: core::ptr::null_mut(), ..uploads[0] }]).err(), Some(E_INVALIDARG), "a pipeline of no device");
        let core = Core::overlay(&device, 1024, 96, &uploads).unwrap();
        drop(pipeline_owner);
        let owner = allocator.as_raw();
        let destination = Destination { list: list.as_raw() as u64, generation: 5, allocator: allocator.as_raw(), queue: queue.as_raw(), certified: true };
        assert_eq!(core.record(&list, owner, &Destination { certified: false, ..destination }, true), RECORDING_FAILURE);
        assert_eq!(core.record(&list, owner, &Destination { generation: 0, ..destination }, true), RECORDING_FAILURE);
        assert_eq!(core.record(&list, owner, &Destination { list: 1, ..destination }, true), RECORDING_FAILURE);
        assert_eq!(core.record(&list, owner, &destination, false), CHANGED, "versions no longer qualified");
        assert!(!core.label().1);
        assert_eq!(core.record(&list, owner, &destination, true), NONE);
        assert_eq!(core.record(&list, owner, &destination, true), ALREADY_RECORDED, "recorded once");
        let (label, recorded) = core.label();
        assert!(recorded && label.resource == core.resource.as_raw() && label.list == destination.list && label.generation == 5);

        let version = VersionView {
            scope: Scope { root: 1, list: 2, generation: 3, occurrence: 4, eye: 0 },
            mapping: Mapping { source_offset: 0, target_offset: 0, view_offset: 64, copy_bytes: 96, next_free: 96, view_end: 1024 },
            original: VertexBufferView { buffer_location: 0x1000, size_in_bytes: 512, stride_in_bytes: 48 },
            draw: DrawArgs { index_count: 3, instance_count: 2, ..Default::default() },
        };
        let gpu = unsafe { core.resource.GetGPUVirtualAddress() };
        let mut view = PrivateView {
            scope: version.scope,
            resource: label.resource,
            owner,
            pipeline: core::ptr::null_mut(),
            shader: [0; 2],
            vertex: VertexBufferView { buffer_location: gpu + 64, ..version.original },
            original: version.original,
            draw: version.draw,
            mapping: version.mapping,
            copy_list: destination.list,
            copy_generation: 5,
            version: 0,
        };
        assert!(core.view_valid(&view, &version));
        view.vertex.buffer_location += 4;
        assert!(!core.view_valid(&view, &version), "a view off its mapped offset");
        view.vertex.buffer_location -= 4;
        view.copy_generation = 6;
        assert!(!core.view_valid(&view, &version), "another recording's copies");
        view.copy_generation = 5;
        assert!(core.for_recording(&view, list.as_raw(), owner, &destination), "the same recording's consumer");
        assert!(!core.for_recording(&view, list.as_raw(), owner, &Destination { generation: 6, ..destination }));
        unsafe { list.Close() }.unwrap();
        assert!(core.submitted(&destination, queue.as_raw()));
        assert!(!core.submitted(&destination, core::ptr::null_mut()));
        unsafe { queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>().unwrap())]) };
        let fence = fence_done(&device, &queue);
        let completion = Completion { recording: destination, fence: fence.as_raw(), value: 1, certified: true };
        assert!(core.after_completion(&completion));
        assert!(!core.after_completion(&Completion { value: 2, ..completion }), "not completed yet");
        assert!(!core.after_completion(&Completion { certified: false, ..completion }));
    }

    #[test]
    fn arena_captures() {
        let device = warp();
        let (queue, allocator, list) = direct(&device);
        let token: IUnknown = allocator.cast().unwrap();
        let lease = Lease { token: token.as_raw(), generation: 9, context: 3, list: list.as_raw() as u64, registry: 4 };
        assert_eq!(Core::arena(&device, &Lease { registry: 0, ..lease }, 0, 1 << 20).err(), Some(E_INVALIDARG));
        assert_eq!(Core::arena(&device, &lease, 2, 1 << 20).err(), Some(E_INVALIDARG));
        assert_eq!(Core::arena(&device, &lease, 0, MAX_BYTES).err(), Some(E_INVALIDARG));
        let core = Core::arena(&device, &lease, 1, 16 << 20).unwrap();
        let owner = allocator.as_raw();
        let scope = Scope { root: 1, list: lease.list, generation: 9, occurrence: 1, eye: 1 };
        let original = VertexBufferView { buffer_location: 0x10000, size_in_bytes: 6 * 1024 * 1024, stride_in_bytes: 48 };
        let draw = DrawArgs { index_count: 3, instance_count: 4, start_instance: 2, ..Default::default() };
        let pipeline = allocator.as_raw();
        assert!(!core.finalize(&list, owner, 0), "nothing captured");
        assert!(core.admit(&list, owner, &Scope { eye: 0, ..scope }, pipeline, &original, &draw, 0, false).is_none(), "another eye");
        assert!(core.admit(&list, owner, &scope, pipeline, &original, &draw, 0, true).is_none(), "a CPU observation");
        assert!(core.admit(&list, owner, &scope, pipeline, &original, &DrawArgs { index_count: 0, ..draw }, 0, false).is_none());
        assert!(core.admit(&list, owner, &scope, pipeline, &original, &draw, MAX_VERSIONS, false).is_none());
        let mapping = core.admit(&list, owner, &scope, pipeline, &original, &draw, 0, false).unwrap();
        assert_eq!((mapping.source_offset, mapping.target_offset, mapping.copy_bytes), (96, 96, 192));
        assert!(core.admit(&list, owner, &Scope { occurrence: 2, ..scope }, pipeline, &original, &draw, 1, false).is_none(), "while recording");
        core.commit(Some(&mapping));
        assert_eq!((core.bytes(0), core.bytes(1)), (mapping.view_end, 192));
        assert!(!core.label().1);
        assert!(core.finalize(&list, owner, 1));
        assert!(core.label().1 && !core.finalize(&list, owner, 1), "finalized once");
        assert!(core.native_for_recording(list.as_raw(), owner, &lease));
        assert!(!core.native_for_recording(list.as_raw(), owner, &Lease { generation: 10, ..lease }));
        unsafe { list.Close() }.unwrap();
        unsafe { queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>().unwrap())]) };
        let fence = fence_done(&device, &queue);
        let completion = NativeCompletion { recording: Lease { ..lease }, queue: queue.as_raw(), fence: fence.as_raw(), value: 1, certified: true };
        assert!(core.native_submitted(&completion, false) && core.native_submitted(&completion, true));
        assert!(!core.native_submitted(&NativeCompletion { value: 2, recording: Lease { ..lease }, ..completion }, true), "not completed yet");
        assert!(core.native_submitted(&NativeCompletion { value: 2, recording: Lease { ..lease }, ..completion }, false), "submitted is enough before completion");
        assert!(!core.native_submitted(&NativeCompletion { certified: false, recording: Lease { ..lease }, ..completion }, false));

        let failing = Core::arena(&device, &lease, 1, 16 << 20).unwrap();
        failing.commit(None);
        assert!(!failing.finalize(&list, owner, 1), "a failed capture");
    }
}
