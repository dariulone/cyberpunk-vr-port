//! Mono sun cascades (formerly src/Render/MonoCascades.cpp, MonoCascadeImage.cpp and the policy of
//! include/Render/MonoCascadePolicy.hpp): the second view's shadow-cascade atlas is drawn once (the source side), each
//! slice copied into a committed image of the port's as its producer finishes it, and MAIN's own cascade clear and
//! nodes are skipped by restoring that image -- only while the pair's sampling matrices, cameras, atlas layout and
//! clear depth all match, every slice was captured, and nothing wrote, discarded or aliased the atlas in between.
//! Anything else falls back to MAIN's native draws (a rejected node gets its native clear back first).
//!
//! The switch and the report stay exported by the C++ half, which attaches them here at its static initialisation with
//! its queries (the probe's internal-command flag, engine stereo's second-view wish); until then everything is off.
use cvr_host::fast::FastMap;
use core::ffi::c_void;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

// ---- the C++ half's exports and queries (Render/MonoCascades.cpp)

/// CvrMonoCascadeAttach.
#[repr(C)]
pub struct Attach {
    mono_cascades: *const AtomicU32,
    report_seq: *const AtomicU32,
    report: *mut u8,
    report_len: usize,
    second_view_wanted: Option<extern "C" fn() -> bool>,
    internal_commands: Option<extern "C" fn() -> bool>,
    set_internal_commands: Option<extern "C" fn(bool) -> bool>,
}
static ATTACH: AtomicPtr<Attach> = AtomicPtr::new(core::ptr::null_mut());
fn attach() -> Option<&'static Attach> {
    // SAFETY: the C++ half's table, alive for the process.
    unsafe { ATTACH.load(Ordering::Acquire).as_ref() }
}
/// # Safety
/// `table` is a CvrMonoCascadeAttach alive for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_attach(table: *const Attach) {
    ATTACH.store(table as *mut Attach, Ordering::Release);
}
fn switch() -> u32 {
    // SAFETY: the exported std::atomic<uint32_t>.
    attach().map_or(0, |a| unsafe { &*a.mono_cascades }.load(Ordering::Relaxed))
}
/// stereo::probe::internalCommands: the port's own commands, which the hooks leave alone.
fn internal() -> bool {
    attach().and_then(|a| a.internal_commands).is_some_and(|f| f())
}
/// stereo::probe::Scope.
struct Internal(bool);
impl Internal {
    fn new() -> Self {
        Internal(attach().and_then(|a| a.set_internal_commands).is_some_and(|f| f(true)))
    }
}
impl Drop for Internal {
    fn drop(&mut self) {
        if let Some(f) = attach().and_then(|a| a.set_internal_commands) {
            f(self.0);
        }
    }
}
/// Off in engine stereo, the descriptor and allocation tracking too: the engine draws the cascades once for both eyes,
/// so there is no second view to lend them to.
fn second_view() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| attach().and_then(|a| a.second_view_wanted).is_some_and(|f| f()))
}

// ---- the policy (MonoCascadePolicy.hpp)

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Allocation {
    heap: usize,
    offset: u64,
    bytes: u64,
    known: bool,
    committed: bool,
}
impl Allocation {
    fn could_alias(&self, other: &Allocation) -> bool {
        if self.known && self.committed {
            return false;
        }
        if !self.known || !other.known {
            return true;
        }
        if other.committed || self.heap != other.heap {
            return false;
        }
        if self.bytes == 0 || other.bytes == 0 || self.offset.checked_add(self.bytes).is_none() || other.offset.checked_add(other.bytes).is_none() {
            return true;
        }
        self.offset < other.offset + other.bytes && other.offset < self.offset + self.bytes
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Atlas {
    resource: usize,
    width: u32,
    height: u32,
    format: u32,
    slices: u32,
    mips: u32,
    mip: u32,
    first: u32,
    count: u32,
}
impl Atlas {
    fn mask(&self) -> u32 {
        if self.count != 0 && self.count <= 8 && self.first.wrapping_add(self.count) <= 8 {
            ((1u32 << self.count) - 1) << self.first
        } else {
            0
        }
    }
    fn valid(&self) -> bool {
        self.resource != 0
            && self.width != 0
            && self.height != 0
            && self.width <= 16384
            && self.height <= 16384
            && self.slices <= 8
            && self.mips == 1
            && self.mip == 0
            && self.mask() != 0
            && self.first.wrapping_add(self.count) <= self.slices
    }
    fn same_layout(&self, a: &Atlas) -> bool {
        self.width == a.width && self.height == a.height && self.format == a.format && self.slices == a.slices && self.mips == a.mips && self.mip == a.mip
    }
    fn same_resource(&self, a: &Atlas) -> bool {
        self.resource == a.resource && self.same_layout(a)
    }
}
/// A source preparation starts a new lease. MAIN can consume it exactly once. No state from a previous view pair
/// survives a new preparation or invalidation.
#[derive(Clone, Copy, Default)]
struct Policy {
    cycle: u64,
    atlas: Atlas,
    active: u32,
    complete: u32,
    camera: u32,
    clear_bits: u32,
    claimed: u32,
    clear: bool,
    main_sampling: bool,
    consumed: bool,
    reusing: bool,
    invalid: bool,
}
impl Policy {
    fn begin(&mut self, count: u32) {
        let next = self.cycle + 1;
        *self = Policy {
            cycle: next,
            active: if (1..=4).contains(&count) { count } else { 0 },
            ..Default::default()
        };
    }
    fn invalidate(&mut self) {
        self.invalid = true;
    }
    fn source_clear(&mut self, a: &Atlas, bits: u32) -> bool {
        if self.active == 0 || !a.valid() || a.first != 0 || a.count < self.active || self.clear {
            self.invalidate();
            return false;
        }
        self.atlas = *a;
        self.clear_bits = bits;
        self.clear = true;
        self.invalid = false;
        true
    }
    fn camera(&mut self, index: u32, stable: bool) {
        if index < self.active && stable {
            self.camera |= 1 << index;
        } else {
            self.invalidate();
        }
    }
    fn complete(&mut self, index: u32, valid: bool) {
        if index < self.active && valid {
            self.complete |= 1 << index;
        } else {
            self.invalidate();
        }
    }
    fn main_sampling(&mut self, count: u32, matching: bool) {
        self.main_sampling = matching && count == self.active;
        if !self.main_sampling {
            self.invalidate();
        }
    }
    fn ready(&self) -> bool {
        let mask = if self.active != 0 { (1u32 << self.active) - 1 } else { 0 };
        mask != 0 && self.clear && !self.invalid && self.complete == mask && self.camera == mask && self.main_sampling && !self.consumed
    }
    fn main_clear(&mut self, a: &Atlas, bits: u32) -> bool {
        let matched = self.ready() && self.atlas.same_layout(a) && self.atlas.first == a.first && self.atlas.count == a.count && bits == self.clear_bits;
        self.consumed = true;
        self.reusing = matched;
        matched
    }
    fn claim(&mut self, index: u32) -> bool {
        if !self.reusing || index >= self.active || self.claimed & (1 << index) != 0 {
            return false;
        }
        self.claimed |= 1 << index;
        true
    }
}

// ---- the committed copy of the atlas (MonoCascadeImage.cpp)

/// A transition of one subresource (or all), borrowing the resource; nothing when the states are equal.
fn barrier(list: &ID3D12GraphicsCommandList, resource: &ID3D12Resource, sub: u32, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) {
    if before == after {
        return;
    }
    let b = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: sub,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    };
    // SAFETY: the recording list.
    unsafe { list.ResourceBarrier(&[b]) };
}
/// A scene-profile range of the port's own copies (node 0x4EDFE0).
struct ProfileRange<'a> {
    list: &'a ID3D12GraphicsCommandList,
    active: bool,
}
impl<'a> ProfileRange<'a> {
    fn new(list: &'a ID3D12GraphicsCommandList, side: i32, detail: u32) -> Self {
        let active = crate::scene_profile::ACTIVE.load(Ordering::Relaxed);
        if active {
            crate::scene_profile::mark(list, 0x4EDFE0, side, 0, detail);
        }
        ProfileRange { list, active }
    }
}
impl Drop for ProfileRange<'_> {
    fn drop(&mut self) {
        if self.active {
            crate::scene_profile::end_range(self.list);
        }
    }
}
fn copy(list: &ID3D12GraphicsCommandList, to: &ID3D12Resource, from: &ID3D12Resource, sub: u32) {
    // SAFETY (whole body): borrowed resources in the locations, the recording list.
    unsafe {
        let location = |r: &ID3D12Resource| D3D12_TEXTURE_COPY_LOCATION {
            pResource: core::mem::transmute_copy(r),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: sub },
        };
        list.CopyTextureRegion(&location(to), 0, 0, 0, &location(from), None);
    }
}

/// AtlasImage: the committed copy, with a DSV over all its slices.
struct AtlasImage {
    image: ID3D12Resource,
    descriptors: ID3D12DescriptorHeap,
    shape: D3D12_RESOURCE_DESC,
}
// SAFETY: free-threaded D3D12 objects; the image is never mutated after creation.
unsafe impl Send for AtlasImage {}
unsafe impl Sync for AtlasImage {}
impl AtlasImage {
    fn initialize(device: &ID3D12Device, source: &ID3D12Resource, format: DXGI_FORMAT) -> Option<AtlasImage> {
        // SAFETY (whole body): the live device and source.
        unsafe {
            let shape = source.GetDesc();
            if shape.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
                || shape.MipLevels != 1
                || shape.DepthOrArraySize > 8
                || shape.Width == 0
                || shape.Height == 0
                || shape.SampleDesc.Count != 1
                || (format != DXGI_FORMAT_D16_UNORM && format != DXGI_FORMAT_D32_FLOAT)
            {
                return None;
            }
            let mut d = shape;
            d.Alignment = 0;
            d.Flags = D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
            let size = device.GetResourceAllocationInfo(0, &[d]).SizeInBytes;
            if size == 0 || size > 512 * 1024 * 1024 {
                return None;
            }
            let _internal = Internal::new();
            let heap = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            };
            let mut image: Option<ID3D12Resource> = None;
            device
                .CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_COPY_SOURCE, None, &mut image)
                .ok()?;
            let image = image?;
            let _ = image.SetName(windows::core::w!("CVR_mono_sun_atlas_committed"));
            let descriptors: ID3D12DescriptorHeap = device
                .CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
                    NumDescriptors: 1,
                    ..Default::default()
                })
                .ok()?;
            let view = D3D12_DEPTH_STENCIL_VIEW_DESC {
                Format: format,
                ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                    Texture2DArray: D3D12_TEX2D_ARRAY_DSV {
                        ArraySize: d.DepthOrArraySize as u32,
                        ..Default::default()
                    },
                },
                ..Default::default()
            };
            device.CreateDepthStencilView(&image, Some(&view), descriptors.GetCPUDescriptorHandleForHeapStart());
            Some(AtlasImage { image, descriptors, shape })
        }
    }
    fn compatible(&self, source: &ID3D12Resource) -> bool {
        // SAFETY: the live source.
        let d = unsafe { source.GetDesc() };
        d.Dimension == self.shape.Dimension
            && d.Width == self.shape.Width
            && d.Height == self.shape.Height
            && d.Format == self.shape.Format
            && d.MipLevels == 1
            && d.DepthOrArraySize == self.shape.DepthOrArraySize
            && d.SampleDesc.Count == 1
    }
    fn clear(&self, list: &ID3D12GraphicsCommandList, depth: f32) -> bool {
        if !crate::command_resources::keep(list, &[self.image.as_raw(), self.descriptors.as_raw()]) {
            return false;
        }
        let _internal = Internal::new();
        let _timing = ProfileRange::new(list, 1, 201);
        let all = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        barrier(list, &self.image, all, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_DEPTH_WRITE);
        // SAFETY: the recording list and this image's DSV.
        unsafe {
            list.ClearDepthStencilView(
                self.descriptors.GetCPUDescriptorHandleForHeapStart(),
                D3D12_CLEAR_FLAG_DEPTH,
                depth,
                0,
                None,
            )
        };
        barrier(list, &self.image, all, D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_COPY_SOURCE);
        true
    }
    fn capture(&self, list: &ID3D12GraphicsCommandList, source: &ID3D12Resource, slice: u32, before: D3D12_RESOURCE_STATES) -> bool {
        if !self.compatible(source)
            || slice >= self.shape.DepthOrArraySize as u32
            || !crate::command_resources::keep(list, &[self.image.as_raw(), source.as_raw()])
        {
            return false;
        }
        let _internal = Internal::new();
        let _timing = ProfileRange::new(list, 1, 101 + slice);
        barrier(list, source, slice, before, D3D12_RESOURCE_STATE_COPY_SOURCE);
        barrier(list, &self.image, slice, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
        copy(list, &self.image, source, slice);
        barrier(list, &self.image, slice, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
        barrier(list, source, slice, D3D12_RESOURCE_STATE_COPY_SOURCE, before);
        true
    }
    fn restore(&self, list: &ID3D12GraphicsCommandList, destination: &ID3D12Resource, first: u32, count: u32) -> bool {
        if !self.compatible(destination)
            || count == 0
            || first.wrapping_add(count) > self.shape.DepthOrArraySize as u32
            || !crate::command_resources::keep(list, &[self.image.as_raw(), destination.as_raw()])
        {
            return false;
        }
        let _internal = Internal::new();
        let _timing = ProfileRange::new(list, 0, 200);
        for i in first..first + count {
            barrier(list, destination, i, D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_COPY_DEST);
            copy(list, destination, &self.image, i);
            barrier(list, destination, i, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_DEPTH_WRITE);
        }
        true
    }
}

// ---- the state

const SIGNATURE_KEY: GUID = GUID::from_values(0xc2e74a33, 0x985a, 0x414b, [0xa6, 0xdd, 0x7c, 0xcf, 0x0e, 0xbe, 0x64, 0x09]);
const ALLOCATION_KEY: GUID = GUID::from_values(0x1bbcdd19, 0xf3c8, 0x492b, [0x9a, 0xe1, 0xd7, 0x44, 0x40, 0x18, 0x7f, 0x6d]);
const CAMERA_BYTES: usize = 848;

#[derive(Clone, Copy, Default)]
struct View {
    atlas: Atlas,
    format: DXGI_FORMAT,
}
struct Recording {
    cycle: u64,
    source: Option<ID3D12Resource>,
    image: Option<Arc<AtlasImage>>,
    written: u32,
    copied: u32,
    states: [D3D12_RESOURCE_STATES; 8],
    split: [bool; 8],
}
// SAFETY: free-threaded D3D12 objects.
unsafe impl Send for Recording {}
type SharedRecording = Arc<Mutex<Recording>>;

#[derive(Default)]
struct Counters {
    source_draws: u64,
    main_draws: u64,
    skipped: u64,
    clear_skips: u64,
    fallbacks: u64,
    unknown_dsv: u64,
    discards: u64,
    invalid_writes: u64,
    bad_signatures: u64,
    bad_bindings: u64,
    bad_cameras: u64,
    copy_invalidations: u64,
    discard_invalidations: u64,
    alias_invalidations: u64,
    captured_slices: u64,
    restored_atlases: u64,
    capture_failures: u64,
}
#[derive(Default)]
struct State {
    descriptors: FastMap<usize, View>,
    policy: Policy,
    main_atlas: Atlas,
    source: Option<ID3D12Resource>,
    source_allocation: Allocation,
    atlas_image: Option<Arc<AtlasImage>>,
    copied_mask: u32,
    recordings: FastMap<usize, SharedRecording>,
    sampling: Vec<f32>,
    cameras: [Vec<u8>; 4],
    have_camera: [bool; 4],
    last_report: u64,
    c: Counters,
}
// SAFETY: free-threaded D3D12 objects; every access is under the mutex.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static ACTIVE_PAIR: AtomicBool = AtomicBool::new(false);
static SNAPSHOT_READY: AtomicBool = AtomicBool::new(false);
static HAVE_RECORDINGS: AtomicBool = AtomicBool::new(false);

struct Guard(MutexGuard<'static, Option<State>>);
impl core::ops::Deref for Guard {
    type Target = State;
    fn deref(&self) -> &State {
        self.0.as_ref().unwrap_or_else(|| unreachable!())
    }
}
impl core::ops::DerefMut for Guard {
    fn deref_mut(&mut self) -> &mut State {
        self.0.as_mut().unwrap_or_else(|| unreachable!())
    }
}
fn lock() -> Guard {
    let mut g = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if g.is_none() {
        *g = Some(State {
            sampling: vec![0.0; 64],
            cameras: core::array::from_fn(|_| vec![0; CAMERA_BYTES]),
            ..Default::default()
        });
    }
    Guard(g)
}
fn lock_recording(r: &SharedRecording) -> MutexGuard<'_, Recording> {
    r.lock().unwrap_or_else(|p| p.into_inner())
}

#[derive(Clone, Default)]
struct Node {
    active: bool,
    source: bool,
    valid: bool,
    bound: bool,
    had_binding: bool,
    reuse: bool,
    camera_known: bool,
    repaired: bool,
    index: u32,
    clear_bits: u32,
    cycle: u64,
    draws: u64,
    bad_signatures: u64,
    bad_bindings: u64,
    list: usize,
    view: View,
    expected: Atlas,
    dsv: usize,
    recording: Option<SharedRecording>,
}
thread_local! {
    static STACK: RefCell<Vec<Node>> = const { RefCell::new(Vec::new()) };
    static NODE: RefCell<Node> = RefCell::new(Node::default());
    static PENDING_REPAIR: Cell<bool> = const { Cell::new(false) };
    static PENDING_INDEX: Cell<u32> = const { Cell::new(0) };
    static PENDING_CYCLE: Cell<u64> = const { Cell::new(0) };
}

fn enabled() -> bool {
    second_view() && (ACTIVE_PAIR.load(Ordering::Relaxed) || switch() != 0) && !internal()
}
fn memory(resource: Option<&ID3D12Resource>) -> Allocation {
    let Some(r) = resource else { return Allocation::default() };
    let mut value = Allocation::default();
    let mut size = core::mem::size_of::<Allocation>() as u32;
    // SAFETY: the live resource and an out-buffer of the announced size.
    let ok = unsafe { r.GetPrivateData(&ALLOCATION_KEY, &mut size, Some(&mut value as *mut Allocation as *mut c_void)) }.is_ok();
    if ok && size as usize == core::mem::size_of::<Allocation>() {
        value
    } else {
        Allocation::default()
    }
}
fn backup_ready(s: &State) -> bool {
    s.policy.active != 0 && s.atlas_image.is_some() && s.copied_mask == (1u32 << s.policy.active) - 1
}
fn capture(s: &mut State, list: &ID3D12GraphicsCommandList, r: &mut Recording, mask: u32) {
    if r.cycle != s.policy.cycle {
        return;
    }
    for i in 0..8u32 {
        if (mask & r.written & !r.copied) & (1 << i) == 0 {
            continue;
        }
        let ok = !r.split[i as usize]
            && match (&r.image, &r.source) {
                (Some(image), Some(source)) => image.capture(list, source, i, r.states[i as usize]),
                _ => false,
            };
        if !ok {
            s.policy.invalidate();
            s.c.capture_failures += 1;
            continue;
        }
        r.copied |= 1 << i;
        s.copied_mask |= 1 << i;
        s.c.captured_slices += 1;
        SNAPSHOT_READY.store(backup_ready(s), Ordering::Release);
    }
}

// ---- the hooks

/// ResourceCreated: where a resource's memory is, for the aliasing checks.
pub fn resource_created(resource: &ID3D12Resource, heap: usize, offset: u64, bytes: u64) {
    if internal() || !second_view() {
        return;
    }
    let memory = Allocation {
        heap,
        offset,
        bytes,
        known: heap == 0 || (bytes != 0 && bytes != u64::MAX),
        committed: heap == 0,
    };
    // SAFETY: the live resource; the bytes are copied.
    let _ = unsafe {
        resource.SetPrivateData(
            &ALLOCATION_KEY,
            core::mem::size_of::<Allocation>() as u32,
            Some(&memory as *const Allocation as *const c_void),
        )
    };
}
/// DsvCreated: a depth view of a single-sample D16/D32 2D (array) texture is a possible cascade atlas.
pub fn dsv_created(resource: Option<&ID3D12Resource>, desc: Option<&D3D12_DEPTH_STENCIL_VIEW_DESC>, handle: usize) {
    if internal() || !second_view() {
        return;
    }
    let mut view = View::default();
    if let Some(resource) = resource {
        // SAFETY: the live resource.
        let d = unsafe { resource.GetDesc() };
        let format = desc.map_or(d.Format, |v| v.Format);
        if d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D
            && d.SampleDesc.Count == 1
            && (format == DXGI_FORMAT_D16_UNORM || format == DXGI_FORMAT_D32_FLOAT)
            && desc.is_none_or(|v| v.Flags == D3D12_DSV_FLAG_NONE)
        {
            view.atlas = Atlas {
                resource: resource.as_raw() as usize,
                width: d.Width as u32,
                height: d.Height,
                format: d.Format.0 as u32,
                slices: d.DepthOrArraySize as u32,
                mips: d.MipLevels as u32,
                mip: 0,
                first: 0,
                count: 1,
            };
            view.format = format;
            if let Some(v) = desc {
                // SAFETY: the union member the view dimension names.
                unsafe {
                    if v.ViewDimension == D3D12_DSV_DIMENSION_TEXTURE2DARRAY {
                        let a = v.Anonymous.Texture2DArray;
                        view.atlas.mip = a.MipSlice;
                        view.atlas.first = a.FirstArraySlice;
                        view.atlas.count = a.ArraySize;
                    } else if v.ViewDimension == D3D12_DSV_DIMENSION_TEXTURE2D {
                        view.atlas.mip = v.Anonymous.Texture2D.MipSlice;
                    } else {
                        view = View::default();
                    }
                }
            }
        }
    }
    let mut s = lock();
    if s.descriptors.len() >= 32768 && !s.descriptors.contains_key(&handle) {
        s.descriptors.clear();
    }
    if view.atlas.valid() {
        s.descriptors.insert(handle, view);
    } else {
        s.descriptors.remove(&handle);
    }
}
/// CopyDescriptors: DSV copies carry the atlas views along.
pub fn copy_descriptors(stride: usize, destinations: &[(usize, u32)], sources: &[(usize, u32)]) {
    if internal() || !second_view() {
        return;
    }
    let mut s = lock();
    let mut copied = Vec::new();
    for &(start, n) in sources {
        for k in 0..n as usize {
            if copied.len() >= 32768 {
                s.descriptors.clear();
                return;
            }
            copied.push(s.descriptors.get(&(start + k * stride)).copied().unwrap_or_default());
        }
    }
    let mut at = 0;
    for &(start, n) in destinations {
        for k in 0..n as usize {
            let handle = start + k * stride;
            match copied.get(at) {
                Some(v) if v.atlas.valid() => {
                    s.descriptors.insert(handle, *v);
                }
                _ => {
                    s.descriptors.remove(&handle);
                }
            }
            at += 1;
        }
    }
}
/// SourceSampling: the source side's cascade matrices start a new pair.
pub fn source_sampling(matrices: &[f32; 64], count: u32) {
    if !enabled() {
        return;
    }
    let mut s = lock();
    let on = switch() != 0;
    ACTIVE_PAIR.store(on, Ordering::Relaxed);
    s.policy.begin(if on { count } else { 0 });
    s.main_atlas = Atlas::default();
    s.copied_mask = 0;
    SNAPSHOT_READY.store(false, Ordering::Release);
    s.sampling.copy_from_slice(matrices);
    s.have_camera = [false; 4];
    s.source = None;
}
/// MainSampling: MAIN's cascade matrices, which must be the source's.
pub fn main_sampling(matrices: Option<&[f32; 64]>, count: u32) {
    if !enabled() {
        return;
    }
    let mut s = lock();
    let matching = matrices.is_some_and(|m| m.iter().zip(&s.sampling).all(|(a, b)| a.to_bits() == b.to_bits()));
    s.policy.main_sampling(count, matching);
}
pub fn begin_node(side: i32, index: i32) {
    NODE.with_borrow_mut(|node| STACK.with_borrow_mut(|stack| stack.push(core::mem::take(node))));
    if !enabled() || !(0..=1).contains(&side) || !(0..4).contains(&index) {
        return;
    }
    let s = lock();
    NODE.with_borrow_mut(|node| {
        node.active = true;
        node.source = side == 1;
        node.index = index as u32;
        node.cycle = s.policy.cycle;
        node.valid = s.policy.clear && !s.policy.invalid && node.index < s.policy.active;
        node.reuse = !node.source && PENDING_REPAIR.get() && PENDING_CYCLE.get() == s.policy.cycle && PENDING_INDEX.get() == node.index;
        PENDING_REPAIR.set(false);
        node.expected = if node.source { s.policy.atlas } else { s.main_atlas };
        node.clear_bits = s.policy.clear_bits;
    });
}
pub fn skip_node(side: i32, index: i32) -> bool {
    PENDING_REPAIR.set(false);
    if !enabled() || side != 0 || !(0..4).contains(&index) {
        return false;
    }
    let mut s = lock();
    if !s.policy.claim(index as u32) {
        return false;
    }
    if s.policy.invalid {
        PENDING_REPAIR.set(true);
        PENDING_CYCLE.set(s.policy.cycle);
        PENDING_INDEX.set(index as u32);
        return false;
    }
    s.c.skipped += 1;
    true
}
pub fn end_node() {
    let node = NODE.with_borrow_mut(|node| {
        let previous = STACK.with_borrow_mut(|stack| stack.pop()).unwrap_or_default();
        core::mem::replace(node, previous)
    });
    if node.active {
        let mut s = lock();
        if node.source {
            s.c.source_draws += node.draws;
            s.c.bad_signatures += node.bad_signatures;
            s.c.bad_bindings += node.bad_bindings;
            if node.cycle == s.policy.cycle {
                s.policy.complete(node.index, node.valid && node.had_binding && node.camera_known);
            }
        } else {
            s.c.main_draws += node.draws;
        }
    }
}
pub fn camera(index: u32, data: &[u8], source_side: bool) {
    if !enabled() || data.len() != CAMERA_BYTES {
        return;
    }
    let (active, node_index, node_source, node_cycle) = NODE.with_borrow(|n| (n.active, n.index, n.source, n.cycle));
    if !active || node_index != index || source_side != node_source {
        return;
    }
    let mut s = lock();
    let i = index as usize;
    if node_cycle != s.policy.cycle {
        NODE.with_borrow_mut(|n| n.valid = false);
        return;
    }
    if source_side {
        let same = !s.have_camera[i] || s.cameras[i] == data;
        if !same {
            s.c.bad_cameras += 1;
        }
        if !s.have_camera[i] {
            s.cameras[i].copy_from_slice(data);
            s.have_camera[i] = true;
        }
        NODE.with_borrow_mut(|n| {
            n.camera_known = same;
            n.valid &= same;
        });
        s.policy.camera(index, same);
    } else {
        let known = s.have_camera[i] && s.cameras[i] == data;
        NODE.with_borrow_mut(|n| n.camera_known = known);
    }
}
pub fn targets(list: usize, colors: u32, dsv: Option<usize>) {
    if !enabled() || !NODE.with_borrow(|n| n.active) {
        return;
    }
    let mut s = lock();
    let view = dsv.and_then(|h| s.descriptors.get(&h).copied()).unwrap_or_default();
    let (source, bound) = NODE.with_borrow_mut(|n| {
        n.list = list;
        n.view = view;
        n.dsv = dsv.unwrap_or(0);
        n.bound = colors == 0 && view.atlas.valid() && n.expected.same_resource(&view.atlas) && view.atlas.mask() == 1 << n.index;
        n.had_binding |= n.bound;
        (n.source, n.bound)
    });
    if dsv.is_some() && !view.atlas.valid() {
        s.c.unknown_dsv += 1;
    }
    if source && bound {
        if let Some(image) = s.atlas_image.clone() {
            let cycle = s.policy.cycle;
            let src = s.source.clone();
            let entry = s.recordings.entry(list).or_insert_with(|| fresh(cycle, src.clone(), image.clone()));
            if lock_recording(entry).cycle != cycle {
                *entry = fresh(cycle, src, image);
            }
            let r = entry.clone();
            HAVE_RECORDINGS.store(true, Ordering::Release);
            NODE.with_borrow_mut(|n| n.recording = Some(r));
        }
    }
}
fn fresh(cycle: u64, source: Option<ID3D12Resource>, image: Arc<AtlasImage>) -> SharedRecording {
    Arc::new(Mutex::new(Recording {
        cycle,
        source,
        image: Some(image),
        written: 0,
        copied: 0,
        states: [D3D12_RESOURCE_STATES(0); 8],
        split: [false; 8],
    }))
}
/// SkipDraw: never skips (reuse skips whole MAIN nodes, never a prefix of their draws); records which source slices
/// were written, and gives a rejected MAIN node its native clear back before its first native draw.
pub fn skip_draw(list: &ID3D12GraphicsCommandList) -> bool {
    if !enabled() || !NODE.with_borrow(|n| n.active) {
        return false;
    }
    let raw = list.as_raw() as usize;
    let node = NODE.with_borrow_mut(|n| {
        n.draws += 1;
        n.clone()
    });
    if node.source {
        let bound = node.bound && node.list == raw && node.recording.is_some();
        NODE.with_borrow_mut(|n| {
            n.valid &= bound;
            if !bound {
                n.bad_bindings += 1;
            }
        });
        if let (true, Some(rec)) = (bound, &node.recording) {
            let bit = 1u32 << node.index;
            let had = {
                let mut r = lock_recording(rec);
                r.written |= bit;
                r.states[node.index as usize] = D3D12_RESOURCE_STATE_DEPTH_WRITE;
                r.copied & bit != 0
            };
            if had {
                let mut s = lock();
                let mut r = lock_recording(rec);
                r.copied &= !bit;
                if r.cycle == s.policy.cycle {
                    s.copied_mask &= !bit;
                    SNAPSHOT_READY.store(false, Ordering::Release);
                }
            }
        }
        return false;
    }
    if node.reuse && !node.repaired && node.bound && node.list == raw {
        let _internal = Internal::new();
        lock().c.fallbacks += 1;
        // SAFETY: the recording list and the node's own DSV.
        unsafe {
            list.ClearDepthStencilView(
                D3D12_CPU_DESCRIPTOR_HANDLE { ptr: node.dsv },
                D3D12_CLEAR_FLAG_DEPTH,
                f32::from_bits(node.clear_bits),
                0,
                None,
            )
        };
        NODE.with_borrow_mut(|n| {
            n.repaired = true;
            n.reuse = false;
        });
    }
    false
}
/// SignatureCreated: whether a command signature holds nothing but a draw.
pub fn signature_created(signature: &ID3D12CommandSignature, desc: &D3D12_COMMAND_SIGNATURE_DESC) {
    // SAFETY: the caller's description and its argument array.
    let pure = unsafe {
        desc.NumArgumentDescs == 1
            && !desc.pArgumentDescs.is_null()
            && matches!(
                (*desc.pArgumentDescs).Type,
                D3D12_INDIRECT_ARGUMENT_TYPE_DRAW | D3D12_INDIRECT_ARGUMENT_TYPE_DRAW_INDEXED
            )
    } as u32;
    // SAFETY: the live signature; the bytes are copied.
    let _ = unsafe { signature.SetPrivateData(&SIGNATURE_KEY, 4, Some(&pure as *const u32 as *const c_void)) };
}
pub fn skip_indirect(list: &ID3D12GraphicsCommandList, signature: Option<&ID3D12CommandSignature>) -> bool {
    if !enabled() || !NODE.with_borrow(|n| n.active) {
        return false;
    }
    let mut pure = 0u32;
    let mut size = 4u32;
    // SAFETY: the live signature and a 4-byte out-buffer.
    let ok = signature.is_some_and(|s| unsafe { s.GetPrivateData(&SIGNATURE_KEY, &mut size, Some(&mut pure as *mut u32 as *mut c_void)) }.is_ok());
    if !ok || size != 4 || pure == 0 {
        NODE.with_borrow_mut(|n| {
            n.valid = false;
            n.bad_signatures += 1;
        });
    }
    skip_draw(list)
}
/// Clear: a cascade node's depth clear. The source side's starts a lease (and clears the copy); MAIN's is skipped when
/// the copy can be restored in its place.
#[allow(clippy::too_many_arguments)]
pub fn clear(list: &ID3D12GraphicsCommandList, dsv: usize, flags: D3D12_CLEAR_FLAGS, depth: f32, stencil: u8, rects: u32, side: i32, clear_node: bool) -> bool {
    if !enabled() || !clear_node || !(0..=1).contains(&side) {
        return false;
    }
    let mut s = lock();
    let view = s.descriptors.get(&dsv).copied().unwrap_or_default();
    if flags != D3D12_CLEAR_FLAG_DEPTH || stencil != 0 || rects != 0 || !view.atlas.valid() {
        s.policy.invalidate();
        s.c.fallbacks += 1;
        return false;
    }
    let bits = depth.to_bits();
    let raw = view.atlas.resource as *mut c_void;
    // SAFETY: the atlas the view was made for (a live resource the engine is clearing).
    let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&raw) }) else {
        return false;
    };
    if side == 1 {
        if s.policy.source_clear(&view.atlas, bits) {
            s.source = Some(resource.clone());
            s.source_allocation = memory(Some(resource));
            if !s.atlas_image.as_ref().is_some_and(|i| i.compatible(resource)) {
                let mut device: Option<ID3D12Device> = None;
                // SAFETY: the recording list.
                let image = unsafe { list.GetDevice(&mut device) }
                    .ok()
                    .and(device)
                    .and_then(|d| AtlasImage::initialize(&d, resource, view.format));
                let Some(image) = image else {
                    s.policy.invalidate();
                    s.c.capture_failures += 1;
                    return false;
                };
                s.atlas_image = Some(Arc::new(image));
            }
            if !s.atlas_image.as_ref().is_some_and(|i| i.clear(list, depth)) {
                s.policy.invalidate();
                s.c.capture_failures += 1;
            }
        }
        return false;
    }
    s.main_atlas = view.atlas;
    if !backup_ready(&s) {
        s.policy.invalidate();
    }
    if s.policy.main_clear(&view.atlas, bits) {
        if s.atlas_image
            .as_ref()
            .is_some_and(|i| i.restore(list, resource, view.atlas.first, view.atlas.count))
        {
            s.c.clear_skips += 1;
            s.c.restored_atlases += 1;
            return true;
        }
        s.policy.invalidate();
        s.policy.reusing = false;
        s.c.capture_failures += 1;
    }
    s.c.fallbacks += 1;
    false
}
/// Write: anything written into the atlas before the copy is complete breaks the lease.
pub fn write(resource: usize) {
    if !enabled() || resource == 0 || SNAPSHOT_READY.load(Ordering::Acquire) {
        return;
    }
    let mut s = lock();
    if s.policy.atlas.resource == resource && !backup_ready(&s) {
        s.policy.invalidate();
        s.c.invalid_writes += 1;
        s.c.copy_invalidations += 1;
    }
}
pub fn discard(resource: usize) -> bool {
    if !enabled() || SNAPSHOT_READY.load(Ordering::Acquire) {
        return false;
    }
    let mut s = lock();
    s.c.discards += 1;
    if s.policy.atlas.resource != resource {
        return false;
    }
    if !backup_ready(&s) {
        s.policy.invalidate();
        s.c.invalid_writes += 1;
        s.c.discard_invalidations += 1;
    }
    false
}
/// The resources an aliasing barrier names (raw, null for none).
fn aliasing(b: &D3D12_RESOURCE_BARRIER) -> (*mut c_void, *mut c_void) {
    // SAFETY: the aliasing member of an aliasing barrier; the pointers are read, not released.
    unsafe {
        let a = &b.Anonymous.Aliasing;
        (core::mem::transmute_copy(&a.pResourceBefore), core::mem::transmute_copy(&a.pResourceAfter))
    }
}
pub fn barriers(list: &[D3D12_RESOURCE_BARRIER]) {
    if !enabled() || SNAPSHOT_READY.load(Ordering::Acquire) {
        return;
    }
    for b in list.iter().filter(|b| b.Type == D3D12_RESOURCE_BARRIER_TYPE_ALIASING) {
        let mut s = lock();
        if !s.policy.clear {
            continue;
        }
        let (before, after) = aliasing(b);
        let atlas = s.policy.atlas.resource;
        let explicit_atlas = before as usize == atlas || after as usize == atlas;
        let a = s.source_allocation;
        // SAFETY: the barrier's live resources (or null).
        let mem = |p: *mut c_void| memory(unsafe { ID3D12Resource::from_raw_borrowed(&p) });
        let overlap = if before.is_null() && after.is_null() {
            !(a.known && a.committed)
        } else {
            (!before.is_null() && a.could_alias(&mem(before))) || (!after.is_null() && a.could_alias(&mem(after)))
        };
        // The native heap can be recycled while other cascades are recording. Each producer preserves its own slice
        // before relinquishing ownership.
        if explicit_atlas || overlap {
            s.c.alias_invalidations += 1;
        }
    }
}
pub fn before_barriers(list: &ID3D12GraphicsCommandList, barriers: &[D3D12_RESOURCE_BARRIER]) {
    if !enabled() || !HAVE_RECORDINGS.load(Ordering::Acquire) {
        return;
    }
    let mut s = lock();
    let Some(rec) = s.recordings.get(&(list.as_raw() as usize)).cloned() else {
        return;
    };
    let mut r = lock_recording(&rec);
    let source = r.source.as_ref().map_or(core::ptr::null_mut(), Interface::as_raw);
    for b in barriers {
        if b.Type == D3D12_RESOURCE_BARRIER_TYPE_ALIASING {
            capture(&mut s, list, &mut r, 255);
            continue;
        }
        if b.Type != D3D12_RESOURCE_BARRIER_TYPE_TRANSITION {
            continue;
        }
        // SAFETY: the transition member of a transition barrier; the pointer is read, not released.
        let (resource, sub, before, after) = unsafe {
            let t = &b.Anonymous.Transition;
            (
                core::mem::transmute_copy::<_, *mut c_void>(&t.pResource),
                t.Subresource,
                t.StateBefore,
                t.StateAfter,
            )
        };
        if resource != source {
            continue;
        }
        let mask = if sub == D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES {
            255
        } else if sub < 8 {
            1u32 << sub
        } else {
            0
        };
        if b.Flags != D3D12_RESOURCE_BARRIER_FLAG_END_ONLY
            && (before.0 & D3D12_RESOURCE_STATE_DEPTH_WRITE.0) != 0
            && (after.0 & D3D12_RESOURCE_STATE_DEPTH_WRITE.0) == 0
        {
            capture(&mut s, list, &mut r, mask);
        }
        for i in 0..8 {
            if mask & (1 << i) != 0 {
                r.split[i] = b.Flags == D3D12_RESOURCE_BARRIER_FLAG_BEGIN_ONLY;
                if !r.split[i] {
                    r.states[i] = after;
                }
            }
        }
    }
}
pub fn close(list: &ID3D12GraphicsCommandList) {
    if !enabled() || !HAVE_RECORDINGS.load(Ordering::Acquire) {
        return;
    }
    let mut s = lock();
    let key = list.as_raw() as usize;
    if let Some(rec) = s.recordings.get(&key).cloned() {
        let mut r = lock_recording(&rec);
        capture(&mut s, list, &mut r, 255);
        drop(r);
        s.recordings.remove(&key);
        HAVE_RECORDINGS.store(!s.recordings.is_empty(), Ordering::Release);
    }
}
pub fn reset(list: usize) {
    if internal() || !HAVE_RECORDINGS.load(Ordering::Acquire) {
        return;
    }
    let mut s = lock();
    s.recordings.remove(&list);
    HAVE_RECORDINGS.store(!s.recordings.is_empty(), Ordering::Release);
}
/// Poll (Present): the report, at most every 250 ms.
pub fn poll() {
    if !second_view() {
        return;
    }
    let mut s = lock();
    // SAFETY: plain Win32 query.
    let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    if now.wrapping_sub(s.last_report) < 250 {
        return;
    }
    s.last_report = now;
    let Some(a) = attach() else { return };
    let p = &s.policy;
    let c = &s.c;
    let text = format!(
        "{{\"mode\":{},\"cycle\":{},\"active\":{},\"width\":{},\"height\":{},\"slices\":{},\"complete\":{},\"camera\":{},\"main_sampling\":{},\"reusing\":{},\"invalid\":{},\"source_draws\":{},\"main_draws\":{},\"skipped_nodes\":{},\"clear_skips\":{},\"fallbacks\":{},\"unknown_dsv\":{},\"discards\":{},\"invalid_writes\":{},\"allocation_known\":{},\"committed\":{},\"bad_signatures\":{},\"bad_bindings\":{},\"bad_cameras\":{},\"copy_invalidations\":{},\"discard_invalidations\":{},\"alias_invalidations\":{},\"copied_mask\":{},\"captured_slices\":{},\"restored_atlases\":{},\"capture_failures\":{}}}",
        switch(),
        p.cycle,
        p.active,
        p.atlas.width,
        p.atlas.height,
        p.atlas.slices,
        p.complete,
        p.camera,
        p.main_sampling as i32,
        p.reusing as i32,
        p.invalid as i32,
        c.source_draws,
        c.main_draws,
        c.skipped,
        c.clear_skips,
        c.fallbacks,
        c.unknown_dsv,
        c.discards,
        c.invalid_writes,
        s.source_allocation.known as i32,
        s.source_allocation.committed as i32,
        c.bad_signatures,
        c.bad_bindings,
        c.bad_cameras,
        c.copy_invalidations,
        c.discard_invalidations,
        c.alias_invalidations,
        s.copied_mask,
        c.captured_slices,
        c.restored_atlases,
        c.capture_failures
    );
    // SAFETY: the exported sequence and report buffer.
    unsafe {
        let seq = &*a.report_seq;
        seq.fetch_add(1, Ordering::SeqCst);
        if !a.report.is_null() && a.report_len > 0 {
            let n = text.len().min(a.report_len - 1);
            core::ptr::copy_nonoverlapping(text.as_ptr(), a.report, n);
            *a.report.add(n) = 0;
        }
        seq.fetch_add(1, Ordering::SeqCst);
    }
}

// ---- the C ABI (include/Render/MonoCascades.hpp)

/// Runs `f` on the borrowed list (None for null).
fn with_list<R>(list: *mut c_void, f: impl FnOnce(&ID3D12GraphicsCommandList) -> R) -> Option<R> {
    // SAFETY: the caller's ID3D12GraphicsCommandList or null, borrowed for the call.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.map(f)
}
/// # Safety
/// `resource` is an ID3D12Resource or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_resource_created(resource: *mut c_void, heap: *mut c_void, offset: u64, bytes: u64) {
    // SAFETY: the caller's resource.
    if let Some(r) = unsafe { ID3D12Resource::from_raw_borrowed(&resource) } {
        resource_created(r, heap as usize, offset, bytes);
    }
}
/// # Safety
/// `resource` is an ID3D12Resource or null; `desc` a view description or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_dsv_created(resource: *mut c_void, desc: *const D3D12_DEPTH_STENCIL_VIEW_DESC, handle: usize) {
    // SAFETY: the caller's objects.
    unsafe { dsv_created(ID3D12Resource::from_raw_borrowed(&resource), desc.as_ref(), handle) }
}
/// # Safety
/// The arrays hold `destinations`/`sources` entries (sizes/counts may be null: one each); `device` is an ID3D12Device.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_mono_cascades_copy_descriptors(
    device: *mut c_void,
    destinations: u32,
    dest: *const usize,
    sizes: *const u32,
    sources: u32,
    src: *const usize,
    counts: *const u32,
    kind: i32,
) {
    if D3D12_DESCRIPTOR_HEAP_TYPE(kind) != D3D12_DESCRIPTOR_HEAP_TYPE_DSV || dest.is_null() || src.is_null() {
        return;
    }
    // SAFETY: the caller's device and arrays.
    unsafe {
        let Some(device) = ID3D12Device::from_raw_borrowed(&device) else { return };
        if internal() || !second_view() {
            return;
        }
        let stride = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV) as usize;
        let ranges = |starts: *const usize, n: u32, lens: *const u32| -> Vec<(usize, u32)> {
            (0..n as usize)
                .map(|i| (*starts.add(i), if lens.is_null() { 1 } else { *lens.add(i) }))
                .collect()
        };
        copy_descriptors(stride, &ranges(dest, destinations, sizes), &ranges(src, sources, counts));
    }
}
/// # Safety
/// `matrices` is 64 floats or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_source_sampling(matrices: *const [f32; 64], count: u32) {
    // SAFETY: the caller's matrices.
    if let Some(m) = unsafe { matrices.as_ref() } {
        source_sampling(m, count);
    }
}
/// # Safety
/// `matrices` is 64 floats or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_main_sampling(matrices: *const [f32; 64], count: u32) {
    // SAFETY: the caller's matrices.
    main_sampling(unsafe { matrices.as_ref() }, count)
}
/// # Safety
/// `data` is `bytes` readable bytes or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_camera(index: u32, data: *const u8, bytes: usize, source: bool) {
    if data.is_null() {
        return;
    }
    // SAFETY: the caller's bytes.
    camera(index, unsafe { core::slice::from_raw_parts(data, bytes) }, source)
}
#[no_mangle]
pub extern "C" fn cvr_mono_cascades_begin_node(side: i32, index: i32) {
    begin_node(side, index)
}
#[no_mangle]
pub extern "C" fn cvr_mono_cascades_skip_node(side: i32, index: i32) -> bool {
    skip_node(side, index)
}
#[no_mangle]
pub extern "C" fn cvr_mono_cascades_end_node() {
    end_node()
}
/// # Safety
/// `dsv` is one handle or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_targets(list: *mut c_void, colors: u32, dsv: *const usize) {
    // SAFETY: the caller's handle.
    targets(list as usize, colors, unsafe { dsv.as_ref() }.copied())
}
#[no_mangle]
pub extern "C" fn cvr_mono_cascades_skip_draw(list: *mut c_void) -> bool {
    with_list(list, skip_draw).unwrap_or(false)
}
/// # Safety
/// `signature` is an ID3D12CommandSignature or null; `desc` its description.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_signature_created(signature: *mut c_void, desc: *const D3D12_COMMAND_SIGNATURE_DESC) {
    // SAFETY: the caller's objects.
    unsafe {
        if let (Some(s), Some(d)) = (ID3D12CommandSignature::from_raw_borrowed(&signature), desc.as_ref()) {
            signature_created(s, d);
        }
    }
}
/// # Safety
/// `signature` is an ID3D12CommandSignature or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_skip_indirect(list: *mut c_void, signature: *mut c_void) -> bool {
    // SAFETY: the caller's signature.
    with_list(list, |l| skip_indirect(l, unsafe { ID3D12CommandSignature::from_raw_borrowed(&signature) })).unwrap_or(false)
}
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn cvr_mono_cascades_clear(list: *mut c_void, dsv: usize, flags: i32, depth: f32, stencil: u8, rects: u32, side: i32, clear_node: bool) -> bool {
    with_list(list, |l| clear(l, dsv, D3D12_CLEAR_FLAGS(flags), depth, stencil, rects, side, clear_node)).unwrap_or(false)
}
#[no_mangle]
pub extern "C" fn cvr_mono_cascades_discard(resource: *mut c_void) -> bool {
    discard(resource as usize)
}
#[no_mangle]
pub extern "C" fn cvr_mono_cascades_write(resource: *mut c_void) {
    write(resource as usize)
}
/// # Safety
/// `list` is `count` barriers or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_barriers(count: u32, list: *const D3D12_RESOURCE_BARRIER) {
    if list.is_null() || count == 0 {
        return;
    }
    // SAFETY: the caller's barriers.
    barriers(unsafe { core::slice::from_raw_parts(list, count as usize) })
}
/// # Safety
/// `barriers` is `count` barriers or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_mono_cascades_before_barriers(list: *mut c_void, count: u32, barriers: *const D3D12_RESOURCE_BARRIER) {
    if barriers.is_null() || count == 0 {
        return;
    }
    // SAFETY: the caller's barriers.
    let barriers = unsafe { core::slice::from_raw_parts(barriers, count as usize) };
    with_list(list, |l| before_barriers(l, barriers));
}
#[no_mangle]
pub extern "C" fn cvr_mono_cascades_close(list: *mut c_void) {
    with_list(list, close);
}
#[no_mangle]
pub extern "C" fn cvr_mono_cascades_reset(list: *mut c_void) {
    reset(list as usize)
}
#[no_mangle]
pub extern "C" fn cvr_mono_cascades_poll() {
    poll()
}
