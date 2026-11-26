//! The geometry leases (src/Render/NativeGeometryLeases.cpp): each recording list's bound index and vertex buffers
//! (slots 0..15) with their strong native leases -- at most 512 lists, a 32-slot range checked, slots 16 and up left
//! alone; the diagnostic draws' rows -- each consumed buffer whose bound view is exactly the draw's and whose lease
//! holds its range owned once per resource (at most 8192), the index's span inside its view, the input slot mask
//! supported, every used slot owned, all of them kept by the list, the draw's state known; each buffer's state at the
//! draw (through the .cpp's reader); the draw's freeze when asked -- and the leases handed back: a complete draw's by
//! its sequence, the list's current exact bindings for an indirect call.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the native draws, their instances and states, the freeze, the
//! IA decoding and the exports; tools/single_pass_tests/geometry_lease_test runs its cases against this in its Rust
//! variant, and the unit test below runs it on WARP.

use cvr_host::fast::FastMap;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::Mutex;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_UINT};

use crate::geometry_commands::DrawRecord;

pub const MAX_LISTS: usize = 512;
pub const MAX_RESOURCES: usize = 8192;
pub const DRAW_CAPACITY: usize = 4096;
pub const MAX_DRAW_INSTANCE_BYTES: usize = 16 * 1024;
pub const MAX_INSTANCE_BYTES: usize = 32 * 1024 * 1024;

/// geometry::NativeBuffer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeBuffer {
    pub resource: u64,
    pub gpu: u64,
    pub bytes: u64,
    pub registry: u64,
    pub context: u64,
    pub handle: u32,
    pub kind: u32,
}
/// geometry::BufferLease (its resource a reference the holder owns).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Lease {
    pub resource: *mut c_void,
    pub gpu: u64,
    pub bytes: u64,
    pub heap_type: u32,
    pub flags: u32,
    pub native: NativeBuffer,
}
/// geometry::DrawLeases.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawLeases {
    pub sequence: u32,
    pub index: u32,
    pub used_mask: u32,
    pub owned_mask: u32,
    pub vertices: [u32; 16],
    pub flags: u32,
    pub frame: u32,
    pub side: u32,
    pub reserved: u32,
}
/// geometry::ResourceRecord.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceRecord {
    pub resource: u64,
    pub gpu: u64,
    pub bytes: u64,
    pub heap_type: u32,
    pub flags: u32,
}
/// geometry::ReadState.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadState {
    pub value: u32,
    pub status: u32,
    pub kind: u32,
}
/// geometry::DrawReadStates: the index's, then VB0..15.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawReadStates {
    pub sequence: u32,
    pub buffers: [ReadState; 17],
}
/// What the .cpp publishes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Published {
    pub draws: u64,
    pub draw_count: u32,
    pub resource_count: u32,
    pub resources: u64,
    pub read_states: u64,
}
/// A snapshot's outcome for the .cpp.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SnapshotOut {
    pub flags: u32,
    pub freeze: bool,
    pub states: DrawReadStates,
}
const _: () = assert!(
    core::mem::size_of::<NativeBuffer>() == 48
        && core::mem::size_of::<Lease>() == 80
        && core::mem::size_of::<DrawLeases>() == 96
        && core::mem::size_of::<ResourceRecord>() == 32
        && core::mem::size_of::<DrawReadStates>() == 208
        && core::mem::size_of::<Published>() == 32
);

/// A lease held here.
#[derive(Clone, Default)]
struct Held {
    resource: Option<ID3D12Resource>,
    gpu: u64,
    bytes: u64,
    heap_type: u32,
    flags: u32,
    native: NativeBuffer,
}
impl Held {
    fn taken(lease: &Lease) -> Held {
        // SAFETY: the caller's lease holds a live resource or null.
        let resource = unsafe { ID3D12Resource::from_raw_borrowed(&lease.resource) }.cloned();
        Held { resource, gpu: lease.gpu, bytes: lease.bytes, heap_type: lease.heap_type, flags: lease.flags, native: lease.native }
    }
    fn raw(&self) -> *mut c_void {
        self.resource.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw())
    }
    /// Lent: the resource not referenced.
    fn lent(&self) -> Lease {
        Lease { resource: self.raw(), gpu: self.gpu, bytes: self.bytes, heap_type: self.heap_type, flags: self.flags, native: self.native }
    }
    /// Given: the resource referenced for the receiver.
    fn given(&self) -> Lease {
        let resource = self.resource.clone().map_or(core::ptr::null_mut(), |r| r.into_raw());
        Lease { resource, ..self.lent() }
    }
    fn matches(&self, address: u64, bytes: u64) -> bool {
        self.resource.is_some() && bytes != 0 && address >= self.gpu && address - self.gpu <= self.bytes && bytes <= self.bytes - (address - self.gpu)
    }
}
#[derive(Default)]
struct State {
    index: Held,
    index_view: D3D12_INDEX_BUFFER_VIEW,
    vertices: [Held; 16],
    views: [D3D12_VERTEX_BUFFER_VIEW; 16],
}
#[derive(Default)]
struct Inner {
    states: FastMap<u64, State>,
    draws: Vec<DrawLeases>,
    resources: Vec<ResourceRecord>,
    owners: Vec<Held>,
    ids: FastMap<u64, u32>,
    read_states: Vec<DrawReadStates>,
}
// SAFETY: the D3D12 resources are free-threaded; every access is under the lock.
unsafe impl Send for Inner {}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SNAPSHOTTING: AtomicBool = AtomicBool::new(false);
static INNER: Mutex<Option<Inner>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Inner) -> R) -> R {
    let mut guard = INNER.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Default::default))
}
fn same_index(a: &D3D12_INDEX_BUFFER_VIEW, b: &D3D12_INDEX_BUFFER_VIEW) -> bool {
    a.BufferLocation == b.BufferLocation && a.SizeInBytes == b.SizeInBytes && a.Format == b.Format
}
fn same_vertex(a: &D3D12_VERTEX_BUFFER_VIEW, b: &D3D12_VERTEX_BUFFER_VIEW) -> bool {
    a.BufferLocation == b.BufferLocation && a.SizeInBytes == b.SizeInBytes && a.StrideInBytes == b.StrideInBytes
}
fn index_bytes(view: &D3D12_INDEX_BUFFER_VIEW) -> u64 {
    if view.Format == DXGI_FORMAT_R16_UINT {
        2
    } else if view.Format == DXGI_FORMAT_R32_UINT {
        4
    } else {
        0
    }
}
/// Own: the lease's resource once (its id; 0 for none); Err past the resource limit.
fn own(inner: &mut Inner, lease: &Held) -> Result<u32, ()> {
    let raw = lease.raw();
    if raw.is_null() {
        return Ok(0);
    }
    if let Some(&id) = inner.ids.get(&(raw as u64)) {
        return Ok(id);
    }
    if inner.resources.len() >= MAX_RESOURCES {
        return Err(());
    }
    let id = inner.resources.len() as u32;
    inner.owners.push(lease.clone());
    inner.resources.push(ResourceRecord { resource: raw as u64, gpu: lease.gpu, bytes: lease.bytes, heap_type: lease.heap_type, flags: lease.flags });
    inner.ids.insert(raw as u64, id);
    Ok(id)
}

/// Begin: every list's bindings, row, owner and state cleared (the resources' and owners' row 0 empty); `enabled`
/// both the bindings and the diagnostic rows.
pub fn begin(enabled: bool) {
    with(|inner| {
        ACTIVE.store(enabled, SeqCst);
        SNAPSHOTTING.store(enabled, SeqCst);
        inner.states.clear();
        inner.draws.clear();
        inner.resources = vec![ResourceRecord::default()];
        inner.owners = vec![Held::default()];
        inner.ids.clear();
        inner.read_states.clear();
    });
}
/// The bindings' lifetime: 0 BeginBindings, 1 EndBindings, 2 End, 3 ClearBindings, 4 Reset of `list`.
pub fn bindings(op: u32, list: u64) {
    match op {
        0 => with(|inner| {
            SNAPSHOTTING.store(false, SeqCst);
            ACTIVE.store(true, SeqCst);
            inner.states.clear();
        }),
        1 => with(|inner| {
            if !SNAPSHOTTING.load(SeqCst) {
                ACTIVE.store(false, SeqCst);
                inner.states.clear();
            }
        }),
        2 => {
            ACTIVE.store(false, SeqCst);
            SNAPSHOTTING.store(false, SeqCst);
            with(|inner| inner.states.clear());
        }
        3 => with(|inner| inner.states.clear()),
        _ => {
            if ACTIVE.load(SeqCst) {
                with(|inner| {
                    inner.states.remove(&list);
                });
            }
        }
    }
}
/// The list's index lease and view; false at the list limit.
pub fn index(list: u64, view: Option<&D3D12_INDEX_BUFFER_VIEW>, lease: &Lease) -> bool {
    if !ACTIVE.load(Relaxed) {
        return true;
    }
    with(|inner| {
        if !ACTIVE.load(Relaxed) {
            return true;
        }
        if inner.states.len() >= MAX_LISTS && !inner.states.contains_key(&list) {
            return false;
        }
        let state = inner.states.entry(list).or_default();
        state.index = Held::taken(lease);
        state.index_view = view.copied().unwrap_or_default();
        true
    })
}
/// The list's vertex leases and views from `start`: 0 taken, 1 at the list limit, 2 a range past 32 slots.
pub fn vertices(list: u64, start: u32, count: u32, views: Option<&[D3D12_VERTEX_BUFFER_VIEW]>, leases: &[Lease]) -> u32 {
    if !ACTIVE.load(Relaxed) {
        return 0;
    }
    if start > 32 || count > 32 - start {
        return 2;
    }
    if start >= 16 {
        return 0;
    }
    let count = count.min(16 - start) as usize;
    with(|inner| {
        if !ACTIVE.load(Relaxed) {
            return 0;
        }
        if inner.states.len() >= MAX_LISTS && !inner.states.contains_key(&list) {
            return 1;
        }
        let state = inner.states.entry(list).or_default();
        for i in 0..count {
            let slot = start as usize + i;
            state.vertices[slot] = leases.get(i).map(Held::taken).unwrap_or_default();
            state.views[slot] = views.and_then(|v| v.get(i)).copied().unwrap_or_default();
        }
        0
    })
}
/// A diagnostic draw's row: 0 appended, 1 a limit (nothing appended), 2 not snapshotting. `read` gives a buffer's state
/// at the draw; with `freeze` and a complete row the freeze's leases are given into `index` and `vertices`.
#[allow(clippy::too_many_arguments)]
pub fn snapshot(
    draw: &DrawRecord,
    instances: usize,
    instance_bytes: usize,
    state_ok: bool,
    freeze: bool,
    read: &dyn Fn(u64, &Lease) -> ReadState,
    out: &mut SnapshotOut,
    frozen_index: &mut Option<Lease>,
    frozen_vertices: &mut [Option<Lease>; 16],
) -> u32 {
    if !SNAPSHOTTING.load(Relaxed) {
        return 2;
    }
    with(|inner| {
        if inner.draws.len() >= DRAW_CAPACITY {
            return 1;
        }
        let mut row = DrawLeases { sequence: draw.sequence as u32, used_mask: draw.input_slot_mask, frame: draw.frame, side: draw.side, ..Default::default() };
        let mut states = DrawReadStates { sequence: row.sequence, ..Default::default() };
        if let Some(state) = inner.states.get(&draw.list).map(|s| (s.index.clone(), s.index_view, s.vertices.clone(), s.views)) {
            let (index, index_view, leases, views) = state;
            states.buffers[0] = read(draw.list, &index.lent());
            for slot in 0..16 {
                if row.used_mask & (1 << slot) != 0 {
                    states.buffers[slot + 1] = read(draw.list, &leases[slot].lent());
                }
            }
            let size = index_bytes(&draw.index);
            let first = u64::from(draw.first_index) * size;
            if size != 0
                && draw.indices != 0
                && first <= u64::from(draw.index.SizeInBytes)
                && u64::from(draw.indices) * size <= u64::from(draw.index.SizeInBytes) - first
                && same_index(&draw.index, &index_view)
                && index.matches(draw.index.BufferLocation, u64::from(draw.index.SizeInBytes))
            {
                let Ok(id) = own(inner, &index) else { return 1 };
                row.index = id;
                row.flags |= 1;
            }
            if draw.flags & 128 != 0 && draw.input_slot_mask & 0xFFFF_0000 == 0 {
                row.flags |= 2;
            }
            for slot in 0..16 {
                if draw.input_slot_mask & (1 << slot) == 0 {
                    continue;
                }
                let view = &draw.vertices[slot];
                if same_vertex(view, &views[slot]) && leases[slot].matches(view.BufferLocation, u64::from(view.SizeInBytes)) {
                    let Ok(id) = own(inner, &leases[slot]) else { return 1 };
                    row.vertices[slot] = id;
                    row.owned_mask |= 1 << slot;
                }
            }
            if row.flags & 2 != 0 && row.owned_mask == row.used_mask {
                row.flags |= 4;
            }
            if row.flags == 7 {
                // SAFETY: the draw's list, live while it records.
                let raw = draw.list as *mut c_void;
                let list = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw) };
                let mut kept = list.is_some_and(|l| cvr_gpu::command_resources::keep(l, &[index.raw()]));
                for slot in 0..16 {
                    if row.used_mask & (1 << slot) != 0 {
                        kept &= list.is_some_and(|l| cvr_gpu::command_resources::keep(l, &[leases[slot].raw()]));
                    }
                }
                if kept {
                    row.flags |= 8;
                }
            }
        }
        if instances > MAX_DRAW_INSTANCE_BYTES || instance_bytes + instances > MAX_INSTANCE_BYTES {
            return 1;
        }
        if state_ok {
            row.flags |= 16;
        }
        out.freeze = freeze && row.flags == 31;
        if out.freeze {
            *frozen_index = Some(inner.owners[row.index as usize].given());
            for slot in 0..16 {
                if row.used_mask & (1 << slot) != 0 {
                    frozen_vertices[slot] = Some(inner.owners[row.vertices[slot] as usize].given());
                }
            }
        }
        out.flags = row.flags;
        out.states = states;
        inner.read_states.push(states);
        inner.draws.push(row);
        0
    })
}
/// CopyDraw's leases of a complete draw `sequence` (and its row's place), given.
pub fn copy_draw(sequence: u32) -> Option<(usize, Lease, [Option<Lease>; 16])> {
    with(|inner| {
        let at = inner.draws.iter().position(|row| row.sequence == sequence && row.flags & 15 == 15)?;
        let row = inner.draws[at];
        if row.index == 0 || row.index as usize >= inner.owners.len() {
            return None;
        }
        let mut vertices: [Option<Held>; 16] = Default::default();
        for slot in 0..16 {
            if row.used_mask & (1 << slot) != 0 {
                if row.vertices[slot] == 0 || row.vertices[slot] as usize >= inner.owners.len() {
                    return None;
                }
                vertices[slot] = Some(inner.owners[row.vertices[slot] as usize].clone());
            }
        }
        Some((at, inner.owners[row.index as usize].given(), vertices.map(|v| v.map(|h| h.given()))))
    })
}
/// CopyCurrentBindings: the list's exact bound leases for `draw` (its IB when indexed, each used slot's VB), given.
pub fn current(list: u64, draw: &DrawRecord, indexed: bool, internal: bool) -> Option<(Option<Lease>, [Option<Lease>; 16])> {
    if list == 0 || draw.list != list || internal || !ACTIVE.load(Relaxed) || draw.flags & 128 == 0 || draw.input_slot_mask & !65535 != 0 {
        return None;
    }
    with(|inner| {
        let state = inner.states.get(&list)?;
        if !ACTIVE.load(Relaxed) {
            return None;
        }
        let mut index = None;
        if indexed {
            let bytes = index_bytes(&draw.index);
            if bytes == 0
                || draw.index.BufferLocation % bytes != 0
                || u64::from(draw.index.SizeInBytes) % bytes != 0
                || !same_index(&draw.index, &state.index_view)
                || !state.index.matches(draw.index.BufferLocation, u64::from(draw.index.SizeInBytes))
            {
                return None;
            }
            index = Some(state.index.clone());
        }
        let mut vertices: [Option<Held>; 16] = Default::default();
        for slot in 0..16 {
            if draw.input_slot_mask & (1 << slot) == 0 {
                continue;
            }
            let view = &draw.vertices[slot];
            if !same_vertex(view, &state.views[slot]) || !state.vertices[slot].matches(view.BufferLocation, u64::from(view.SizeInBytes)) {
                return None;
            }
            vertices[slot] = Some(state.vertices[slot].clone());
        }
        Some((index.map(|h| h.given()), vertices.map(|v| v.map(|h| h.given()))))
    })
}
/// The rows to publish.
pub fn published() -> Published {
    with(|inner| Published {
        draws: inner.draws.as_ptr() as u64,
        draw_count: inner.draws.len() as u32,
        resource_count: inner.resources.len() as u32,
        resources: inner.resources.as_ptr() as u64,
        read_states: inner.read_states.as_ptr() as u64,
    })
}

// ---- the C ABI (the leases written into the .cpp's default, empty BufferLeases)

/// # Safety
/// `slot` is an empty lease of the caller's.
unsafe fn put(slot: *mut Lease, lease: Option<Lease>) {
    if let (Some(lease), false) = (lease, slot.is_null()) {
        // SAFETY: as documented.
        unsafe { *slot = lease };
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_gl_begin(enabled: bool) {
    begin(enabled);
}
/// 0 active, 1 snapshotting.
#[no_mangle]
pub extern "C-unwind" fn cvr_gl_flag(which: u32) -> bool {
    if which == 0 {
        ACTIVE.load(Relaxed)
    } else {
        SNAPSHOTTING.load(Relaxed)
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_gl_bindings(op: u32, list: *mut c_void) {
    bindings(op, list as u64);
}
/// # Safety
/// `view` null or live; `lease` live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gl_index(list: *mut c_void, view: *const D3D12_INDEX_BUFFER_VIEW, lease: *const Lease) -> bool {
    // SAFETY: as documented.
    unsafe { index(list as u64, view.as_ref(), &*lease) }
}
/// # Safety
/// `views` null or holding `count`; `leases` holding 16.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gl_vertices(list: *mut c_void, start: u32, count: u32, views: *const D3D12_VERTEX_BUFFER_VIEW, leases: *const Lease) -> u32 {
    let n = count.min(16) as usize;
    // SAFETY: as documented.
    let views = if views.is_null() { None } else { Some(unsafe { core::slice::from_raw_parts(views, n) }) };
    let leases = if leases.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(leases, 16) } };
    vertices(list as u64, start, count, views, leases)
}
/// A buffer's state at the draw, from the .cpp.
pub type ReadFn = unsafe extern "C-unwind" fn(list: *mut c_void, lease: *const Lease, out: *mut ReadState);
/// # Safety
/// `draw` live; `index` an empty lease and `vertices` 16 of them; `out` writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_gl_snapshot(draw: *const DrawRecord, instances: usize, instance_bytes: usize, state_ok: bool, freeze: bool, read_state: ReadFn, index: *mut Lease, vertices: *mut Lease, out: *mut SnapshotOut) -> u32 {
    let read = |list: u64, lease: &Lease| {
        let mut state = ReadState::default();
        // SAFETY: the .cpp's reader of a lent lease.
        unsafe { read_state(list as *mut c_void, lease, &mut state) };
        state
    };
    let (mut frozen_index, mut frozen_vertices) = (None, [None; 16]);
    // SAFETY: as documented.
    let result = unsafe { snapshot(&*draw, instances, instance_bytes, state_ok, freeze, &read, &mut *out, &mut frozen_index, &mut frozen_vertices) };
    unsafe {
        put(index, frozen_index);
        for (slot, lease) in frozen_vertices.into_iter().enumerate() {
            put(vertices.add(slot), lease);
        }
    }
    result
}
/// # Safety
/// `index` an empty lease, `vertices` 16 of them, `at` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gl_copy_draw(sequence: u32, index: *mut Lease, vertices: *mut Lease, at: *mut usize) -> bool {
    let Some((row, lease, leases)) = copy_draw(sequence) else { return false };
    // SAFETY: as documented.
    unsafe {
        put(index, Some(lease));
        for (slot, lease) in leases.into_iter().enumerate() {
            put(vertices.add(slot), lease);
        }
        *at = row;
    }
    true
}
/// # Safety
/// `draw` live; `index` an empty lease and `vertices` 16 of them.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gl_current(list: *mut c_void, draw: *const DrawRecord, indexed: bool, internal: bool, index: *mut Lease, vertices: *mut Lease) -> bool {
    // SAFETY: as documented.
    let Some((lease, leases)) = current(list as u64, unsafe { &*draw }, indexed, internal) else { return false };
    unsafe {
        put(index, lease);
        for (slot, lease) in leases.into_iter().enumerate() {
            put(vertices.add(slot), lease);
        }
    }
    true
}
/// # Safety
/// `out` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gl_published(out: *mut Published) {
    // SAFETY: as documented.
    unsafe { *out = published() };
}

// ---- the native draws' resources (geometry::DrawResources; the plugin's tables formerly NativeGeometryLeases.cpp's)

/// geometry::BufferLease owned: its resource a reference this lease holds (the C++ layout: a ComPtr is its pointer).
#[repr(C)]
#[derive(Clone, Default)]
pub struct BufferLease {
    pub resource: Option<ID3D12Resource>,
    pub gpu: u64,
    pub bytes: u64,
    pub heap_type: u32,
    pub flags: u32,
    pub native: NativeBuffer,
}
const _: () = assert!(core::mem::size_of::<BufferLease>() == 80 && core::mem::offset_of!(BufferLease, native) == 32);
impl BufferLease {
    /// A lent lease copied (its resource referenced again).
    pub fn copied(lease: &Lease) -> BufferLease {
        // SAFETY: the lent lease's live resource (or null).
        let resource = unsafe { ID3D12Resource::from_raw_borrowed(&lease.resource) }.cloned();
        BufferLease { resource, gpu: lease.gpu, bytes: lease.bytes, heap_type: lease.heap_type, flags: lease.flags, native: lease.native }
    }
    /// A given lease, its reference taken over.
    pub fn given(lease: Lease) -> BufferLease {
        // SAFETY: a given lease's own reference (or null).
        let resource = (!lease.resource.is_null()).then(|| unsafe { ID3D12Resource::from_raw(lease.resource) });
        BufferLease { resource, gpu: lease.gpu, bytes: lease.bytes, heap_type: lease.heap_type, flags: lease.flags, native: lease.native }
    }
    /// The resource lent (not referenced).
    pub fn raw(&self) -> *mut c_void {
        self.resource.as_ref().map_or(core::ptr::null_mut(), |r| r.as_raw())
    }
    /// As geometry_commands.rs lends it.
    pub fn commands(&self) -> crate::geometry_commands::Lease {
        crate::geometry_commands::Lease { resource: self.raw(), gpu: self.gpu, bytes: self.bytes }
    }
}
/// scene_state::DrawState owned.
#[derive(Clone)]
pub struct DrawState {
    pub original: Option<ID3D12PipelineState>,
    pub rate_image: Option<ID3D12Resource>,
    pub rate: cvr_gpu::scene_state::Rate,
    pub topology: i32,
    pub stencil: u32,
    pub blend: [f32; 4],
    pub targets: [Option<ID3D12Resource>; 4],
}
impl Default for DrawState {
    fn default() -> Self {
        DrawState { original: None, rate_image: None, rate: Default::default(), topology: 0, stencil: 0, blend: [1.0; 4], targets: Default::default() }
    }
}
/// geometry::DrawResources owned (its head -- the leases and the draw record -- in the C++ layout).
#[repr(C)]
#[derive(Clone)]
pub struct DrawResources {
    pub index: BufferLease,
    pub vertices: [BufferLease; 16],
    pub draw: DrawRecord,
    pub instances: Vec<u8>,
    pub state: DrawState,
}
const _: () = assert!(core::mem::offset_of!(DrawResources, vertices) == 80 && core::mem::offset_of!(DrawResources, draw) == 1360);
impl DrawResources {
    pub fn new(draw: DrawRecord) -> DrawResources {
        DrawResources { index: BufferLease::default(), vertices: Default::default(), draw, instances: Vec::new(), state: DrawState::default() }
    }
    /// As geometry_commands.rs lends it (alive while this is).
    pub fn lend(&self) -> crate::geometry_commands::Resources {
        crate::geometry_commands::Resources {
            index: self.index.commands(),
            vertices: core::array::from_fn(|slot| self.vertices[slot].commands()),
            draw: &self.draw,
            instances: self.instances.as_ptr(),
            instance_count: self.instances.len(),
        }
    }
}
/// The plugin's native draws, their instances and states (formerly NativeGeometryLeases.cpp's Begin, Snapshot and
/// CopyDraw with their tables, and the native probe's snapshot lambda). The Rust test variants keep the .cpp's.
#[cfg(all(feature = "rust-build", not(test)))]
pub mod natives {
    use super::*;
    use crate::declared_moved_exports::*;
    #[derive(Default)]
    struct Natives {
        draws: Vec<DrawRecord>,
        instances: Vec<Vec<u8>>,
        states: Vec<DrawState>,
        instance_bytes: usize,
    }
    // SAFETY: free-threaded D3D12 objects, touched only under the lock.
    unsafe impl Send for Natives {}
    static NATIVES: Mutex<Option<Natives>> = Mutex::new(None);
    fn natives() -> std::sync::MutexGuard<'static, Option<Natives>> {
        NATIVES.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn publish() {
        let p = published();
        // SAFETY: the exported rows' addresses, written under the lock as the .cpp wrote them.
        unsafe { CyberpunkVR_GeometryLeaseDraws = p.draws };
        CyberpunkVR_GeometryLeaseDrawCount.store(p.draw_count, SeqCst);
        // SAFETY: as above.
        unsafe { CyberpunkVR_GeometryLeaseResources = p.resources };
        CyberpunkVR_GeometryLeaseResourceCount.store(p.resource_count, SeqCst);
        // SAFETY: as above.
        unsafe { CyberpunkVR_GeometryLeaseReadStates = p.read_states };
    }
    /// geometry::Begin: the leases' state, the freeze's, the tables cleared; the IA sites' compatibility when enabled.
    pub fn begin(enabled: bool) {
        let mut guard = natives();
        CyberpunkVR_GeometryLeaseSeq.fetch_add(1, SeqCst);
        super::begin(enabled);
        CyberpunkVR_GeometryLeaseFailure.store(0, SeqCst);
        crate::geometry_freeze::cvr_gfz_begin(enabled && CyberpunkVR_GeometryFreezeEnable.load(SeqCst) != 0);
        *guard = Some(Natives::default());
        if enabled {
            CyberpunkVR_GeometryLeaseCompatible.store(if crate::ia_binding::native_ia_compatible() { 1 } else { 2 }, SeqCst);
        }
        publish();
        CyberpunkVR_GeometryLeaseSeq.fetch_add(1, SeqCst);
    }
    fn read_at_draw(list: u64, lease: &Lease) -> ReadState {
        // SAFETY: the same 80-byte geometry::BufferLease record, lent.
        let lease = unsafe { &*(lease as *const Lease).cast::<crate::ia_binding::OwnedLease>() };
        let state = crate::ia_binding::read_state_at_draw(list, lease);
        ReadState { value: state.value, status: state.status, kind: state.kind }
    }
    /// geometry::Snapshot: the draw's row (its leases' states read at the draw), its freeze when asked (the frozen leases
    /// given into the source), the draw, its instances and state kept.
    pub fn snapshot_draw(draw: &DrawRecord, instances: &[u8], state: &DrawState, freeze: bool) {
        if !SNAPSHOTTING.load(Relaxed) {
            return;
        }
        let mut guard = natives();
        if !SNAPSHOTTING.load(Relaxed) {
            return;
        }
        let n = guard.get_or_insert_with(Natives::default);
        CyberpunkVR_GeometryLeaseSeq.fetch_add(1, SeqCst);
        let original = state.original.as_ref().map_or(0, |o| o.as_raw() as u64);
        let state_ok = original == draw.pipeline && state.topology != 0 && state.rate.known;
        let frozen = freeze && CyberpunkVR_GeometryFreezeEnable.load(Relaxed) != 0;
        let mut out = SnapshotOut::default();
        let (mut index, mut vertices) = (None, [None; 16]);
        let result = snapshot(draw, instances.len(), n.instance_bytes, state_ok, frozen, &read_at_draw, &mut out, &mut index, &mut vertices);
        let mut source = DrawResources::new(*draw);
        if let Some(lease) = index {
            source.index = BufferLease::given(lease);
        }
        for (slot, lease) in vertices.into_iter().enumerate() {
            if let Some(lease) = lease {
                source.vertices[slot] = BufferLease::given(lease);
            }
        }
        if result == 1 {
            CyberpunkVR_GeometryLeaseFailure.store(1, SeqCst);
        } else if result == 0 {
            if out.freeze {
                source.state = state.clone();
                crate::geometry_freeze::capture_resources(&source, &out.states.buffers, true);
            }
            n.states.push(if out.flags & 16 != 0 { state.clone() } else { DrawState::default() });
            n.instances.push(instances.to_vec());
            n.instance_bytes += instances.len();
            n.draws.push(*draw);
        }
        publish();
        CyberpunkVR_GeometryLeaseSeq.fetch_add(1, SeqCst);
    }
    /// geometry::CopyDraw: a complete draw's leases (given), record, instances and state.
    pub fn copy_draw_resources(sequence: u32) -> Option<DrawResources> {
        let guard = natives();
        if CyberpunkVR_GeometryLeaseFailure.load(SeqCst) != 0 {
            return None;
        }
        let (at, index, vertices) = copy_draw(sequence)?;
        let index = BufferLease::given(index);
        let vertices = vertices.map(|v| v.map(BufferLease::given).unwrap_or_default());
        let n = guard.as_ref()?;
        Some(DrawResources { index, vertices, draw: *n.draws.get(at)?, instances: n.instances.get(at)?.clone(), state: n.states.get(at)?.clone() })
    }
    /// The native probe's capture begin (formerly its runtime lambda's geometry::Begin).
    #[no_mangle]
    pub extern "C-unwind" fn cvr_gl_begin_all(enabled: bool) {
        begin(enabled);
    }
    /// The native probe's geometry snapshot (formerly its runtime lambda): while the constant capture is on, the bound
    /// family's CBVs snapshotted and the draw's state and targets captured; the freeze for a depth-only, rebase-safe,
    /// CBV-only family.
    ///
    /// # Safety
    /// The probe's live list, pipeline and record; `batch` holds `bytes` (or is null).
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_gl_probe_snapshot(list: *mut c_void, pso: *mut c_void, record: *const DrawRecord, batch: *const u8, bytes: usize) {
        let capture = crate::native_state_exports::CyberpunkVR_ConstantCaptureEnable.load(Relaxed) != 0;
        let family = if capture { crate::families_native::bound(list) } else { None };
        if family.is_some() {
            // SAFETY: the probe's record.
            unsafe { crate::constant_capture_state::cvr_ncc2_snapshot_draw(record) };
        }
        // SAFETY: as documented.
        let draw = unsafe { &*record };
        let mut state = DrawState::default();
        let mut raw = cvr_gpu::scene_state::DrawStateRaw::default();
        // SAFETY: the live list; a local output.
        if capture && !list.is_null() && !pso.is_null() && unsafe { cvr_gpu::scene_state::cvr_scene_capture_draw_state(list as usize, &mut raw) } {
            // SAFETY: the live pipeline (referenced for the state); the raw image a reference handed over.
            state.original = unsafe { ID3D12PipelineState::from_raw_borrowed(&pso) }.cloned();
            state.rate_image = (raw.rate_image != 0).then(|| unsafe { ID3D12Resource::from_raw(raw.rate_image as *mut c_void) });
            state.rate = raw.rate;
            state.topology = raw.topology;
            state.stencil = raw.stencil;
            state.blend = raw.blend_factor;
            let targets = cvr_gpu::scene_state::DrawTargets { rt_count: draw.rt_count, rt_contiguous: draw.rt_contiguous, targets: draw.targets, depth: draw.depth, viewport: draw.viewport };
            let mut resources = [0usize; 4];
            // SAFETY: a local view and output.
            if unsafe { cvr_gpu::scene_state::cvr_scene_capture_targets(&targets, &mut resources) } {
                // SAFETY: references handed over.
                state.targets = resources.map(|r| (r != 0).then(|| unsafe { ID3D12Resource::from_raw(r as *mut c_void) }));
            }
        }
        let freeze = family.as_ref().is_some_and(|f| f.instance_rebase_safe && !f.pass.pixel_shader && draw.rt_count == 0 && f.uses.iter().all(|u| u.kind == 2));
        // SAFETY: as documented.
        let instances = if batch.is_null() || bytes == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(batch, bytes) } };
        snapshot_draw(draw, instances, &state, freeze);
    }
}

#[cfg(test)]
mod tests {
    //! WARP: a list's leases, a complete draw's row and its leases back, the refusals, the current bindings, Reset.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    fn release(lease: Option<Lease>) -> bool {
        match lease {
            Some(l) if !l.resource.is_null() => {
                // SAFETY: a given reference.
                drop(unsafe { ID3D12Resource::from_raw(l.resource) });
                true
            }
            _ => false,
        }
    }

    #[test]
    fn leases_rows_and_bindings() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        let buffer = crate::readback_buffers::buffer(&device, None, 4096, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ).unwrap();
        let gpu = unsafe { buffer.GetGPUVirtualAddress() };
        let lease = Lease { resource: buffer.as_raw(), gpu, bytes: 4096, heap_type: 2, flags: 0, native: NativeBuffer::default() };
        let id = list.as_raw() as u64;
        begin(true);
        let index_view = D3D12_INDEX_BUFFER_VIEW { BufferLocation: gpu + 512, SizeInBytes: 256, Format: DXGI_FORMAT_R16_UINT };
        let vertex = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: gpu + 512, SizeInBytes: 256, StrideInBytes: 16 };
        assert!(index(id, Some(&index_view), &lease));
        let mut leases = [Lease { resource: core::ptr::null_mut(), ..lease }; 16];
        leases[0] = lease;
        assert_eq!(vertices(id, 0, 1, Some(&[vertex]), &leases), 0);
        assert_eq!(vertices(id, 16, 17, None, &leases), 2, "past 32 slots");
        // SAFETY: plain data.
        let mut draw: DrawRecord = unsafe { core::mem::zeroed() };
        draw.sequence = 1;
        draw.frame = 1;
        draw.side = 1;
        draw.list = id;
        draw.index = index_view;
        draw.indices = 36;
        draw.flags = 128;
        draw.input_slot_mask = 1;
        draw.vertices[0] = vertex;
        let read = |_: u64, l: &Lease| ReadState { value: 3, status: u32::from(!l.resource.is_null()), kind: 1 };
        let mut out = SnapshotOut::default();
        let (mut fi, mut fv) = (None, [None; 16]);
        assert_eq!(snapshot(&draw, 0, 0, true, false, &read, &mut out, &mut fi, &mut fv), 0);
        assert_eq!(out.flags, 31, "owned, supported, complete, kept, state known");
        assert_eq!((out.states.buffers[0].status, out.states.buffers[1].status, out.states.buffers[2].status), (1, 1, 0));
        let p = published();
        assert_eq!((p.draw_count, p.resource_count), (1, 2), "the shared IB/VB resource owned once");
        let (at, il, vl) = copy_draw(1).unwrap();
        assert_eq!(at, 0);
        assert!(release(Some(il)) && release(vl[0]) && vl[1].is_none());
        draw.sequence = 2;
        draw.input_slot_mask = 3;
        assert_eq!(snapshot(&draw, 0, 0, true, false, &read, &mut out, &mut fi, &mut fv), 0);
        assert!(copy_draw(2).is_none(), "a missing used vertex stream");
        draw.sequence = 3;
        draw.input_slot_mask = 1;
        draw.first_index = 128;
        assert_eq!(snapshot(&draw, 0, 0, true, false, &read, &mut out, &mut fi, &mut fv), 0);
        assert!(copy_draw(3).is_none(), "the index range past its view");
        draw.first_index = 0;
        assert_eq!(snapshot(&draw, MAX_DRAW_INSTANCE_BYTES + 1, 0, true, false, &read, &mut out, &mut fi, &mut fv), 1, "an instance limit");
        draw.sequence = 4;
        assert_eq!(snapshot(&draw, 0, 0, true, true, &read, &mut out, &mut fi, &mut fv), 0);
        assert!(out.freeze && release(fi.take()) && release(fv[0].take()), "the freeze's leases given");

        bindings(0, 0);
        assert!(index(id, Some(&index_view), &lease));
        assert_eq!(vertices(id, 0, 1, Some(&[vertex]), &leases), 0);
        let (ci, cv) = current(id, &draw, true, false).unwrap();
        assert!(release(ci) && release(cv[0]));
        let mut wrong = draw;
        wrong.vertices[0].BufferLocation += 16;
        assert!(current(id, &wrong, true, false).is_none(), "a stale VB view");
        assert!(current(id, &draw, true, true).is_none(), "the port's own commands");
        assert!(current(id + 1, &draw, true, false).is_none(), "a foreign list");
        bindings(4, id);
        assert!(current(id, &draw, false, false).is_none(), "Reset cleared the list");
        bindings(2, 0);
        begin(false);
        assert_eq!(published().draw_count, 0);
    }
}
