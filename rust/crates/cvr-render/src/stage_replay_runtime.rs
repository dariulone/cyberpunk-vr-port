//! The stage replay's runtime (formerly src/Render/StageReplay.cpp; include/Render/StageReplay.hpp says what and why):
//! three layers --
//!   1. chained command-list hooks, installed at runtime over whatever already occupies the slots (the plugin's own
//!      hooks or the driver); they mirror each list's graphics state (the shadows) and, inside a consumer scope,
//!      append the call to the scope's stream;
//!   2. the consumer scope, entered from the 0x334CC0 hook (src/Stereo/Grading.cpp): census counting, source
//!      capture, target skip, target replay (the source's partitions re-issued into the target's list, camera tables
//!      rebuilt from the engine's shader-visible ring with the target's view descriptors, the target's state restored);
//!   3. the poll, once per Present: the census window, hook installation;
//! and VRCAM's G-buffer partitions trampoline at the scene builder. All of it is inert until CyberpunkVR_StageReplayMode
//! or a census request is set (the partitions trampoline is installed at boot).
//!
//! The .cpp keeps the device hooks' list vtable entries and the log, lent through the runtime, the boot hook's
//! registration and the forwarders.

use crate::camera_binding::{self as binding, Context};
use crate::stage_replay::{self as rules, op, Command, Settings, Touched, NO_PAYLOAD, VIEW_DESCRIPTORS};
use core::cell::{Cell, UnsafeCell};
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::{Mutex, OnceLock};
use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;

#[cfg(feature = "rust-build")]
use crate::used_moved_exports_rest as x;
#[cfg(not(feature = "rust-build"))]
use test_exports as x;

/// What the .cpp lends: the device hooks' command-list vtables (each row: the vtable, then whether the list has the
/// ID3D12GraphicsCommandList1, 2, 4, 5 and 7 methods) and the log.
#[repr(C)]
pub struct Runtime {
    pub entries: extern "C" fn(*mut [u64; 6], u32) -> u32,
    pub log: extern "C" fn(*const c_char),
}
/// The plugin's table, built here (formerly src/Render/StageReplay.cpp's); a test attaches its own.
#[cfg(feature = "rust-build")]
mod plugin {
    use super::Runtime;
    use crate::list_detours::{head, tail};
    use crate::list_vtable::Entry;
    /// Each row: the vtable, then whether the list has the ID3D12GraphicsCommandList1, 2, 4, 5 and 7 methods (their
    /// originals: AtomicCopyBufferUINT, WriteBufferImmediate, BeginRenderPass, RSSetShadingRate, Barrier).
    extern "C" fn entries(out: *mut [u64; 6], max: u32) -> u32 {
        let mut list = [core::ptr::null::<Entry>(); 16];
        // SAFETY: sixteen slots.
        let n = unsafe { crate::list_vtable::cvr_lvt_entries(list.as_mut_ptr(), max.min(16)) };
        for (i, entry) in list.iter().take(n as usize).enumerate() {
            let has = |slot: usize| (slot != 0) as u64;
            // SAFETY: a published entry (or null); the caller's `max` rows.
            unsafe {
                *out.add(i) = match entry.as_ref() {
                    Some(e) => [e.head[0] as u64, has(e.tail[tail::ATOMIC_COPY32]), has(e.tail[tail::WRITE_IMMEDIATE]),
                                has(e.tail[tail::BEGIN_RENDER_PASS]), has(e.head[head::SHADING_RATE]),
                                has(e.tail[tail::ENHANCED_BARRIER])],
                    None => [0; 6],
                };
            }
        }
        n
    }
    pub(super) static RT: Runtime = Runtime { entries, log: crate::stereo_shared::stereo_log };
}
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn rt() -> Option<&'static Runtime> {
    // SAFETY: attached once with a static runtime.
    unsafe { RUNTIME.load(Acquire).as_ref() }
}
fn log(text: &str) {
    if let (Some(r), Ok(text)) = (rt(), std::ffi::CString::new(text)) {
        (r.log)(text.as_ptr());
    }
}

const MAX_ORDINALS: u32 = 8;
mod fallback {
    pub const NONE: u32 = 0;
    pub const NO_CAPTURE: u32 = 1;
    pub const STALE_FRAME: u32 = 2;
    pub const CAPTURE_OVERFLOW: u32 = 3;
    pub const CAPTURE_BLOCKING: u32 = 4;
    pub const CAPTURE_LIST: u32 = 5;
    pub const BUSY: u32 = 6;
    pub const NO_CONTEXT: u32 = 7;
    pub const SHADOW_INVALID: u32 = 8;
    pub const SHADOW_OVERFLOW: u32 = 9;
    pub const NO_HEAP: u32 = 10;
    pub const CAMERA: u32 = 11;
    pub const CAPACITY: u32 = 12;
    pub const ORDINAL: u32 = 13;
    pub const LEADER: u32 = 14;
}
/// The census row's columns.
mod col {
    pub const CALLS: usize = 0;
    pub const TICKS: usize = 1;
    pub const COMMANDS: usize = 2;
    pub const DRAWS: usize = 3;
    pub const BLOCKING_OPS: usize = 4;
    pub const FOREIGN_OPS: usize = 5;
    pub const LAST_ENGINE_FRAME: usize = 6;
    pub const MAX_ORDINAL: usize = 7;
    pub const PEER_DONE: usize = 8;
    pub const PEER_PENDING: usize = 9;
    pub const CAM_VS: usize = 10;
    pub const CAM_PS: usize = 11;
    pub const CAMERA_SLOTS: usize = 12;
    pub const CONTEXT_OK: usize = 13;
    pub const COPIES: usize = 14;
    pub const COPIED_DESCRIPTORS: usize = 15;
    pub const TABLES: usize = 16;
    pub const TABLES_MATCHED: usize = 17;
    pub const CAMERA_TABLES: usize = 18;
    pub const SKIPPED: usize = 19;
    pub const REPLAYED: usize = 20;
    pub const FALLBACKS: usize = 21;
    pub const LAST_FALLBACK: usize = 22;
    pub const REPLAY_TICKS: usize = 23;
    pub const ARGS0: usize = 24;
    pub const CAMERA_PARAMS: usize = 28;
    pub const ENTRY_LIST: usize = 29;
    pub const SHADOW_VALID: usize = 30;
    pub const PACKETS: usize = 31;
}
mod kind {
    pub const FORWARD: u32 = 0;
    pub const CAPTURE: u32 = 1;
    pub const SKIP: u32 = 2;
    pub const REPLAY: u32 = 3;
    pub const FALLBACK: u32 = 4;
    pub const PRESENT: u32 = 5;
}

fn now() -> i64 {
    let mut t = 0i64;
    // SAFETY: one output.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut t) };
    t
}
// The export tables' atomic views (std::atomic_ref in the .cpp).
fn add(p: *mut u64, v: u64) {
    // SAFETY: a field of an export table, live for the process.
    unsafe { AtomicU64::from_ptr(p) }.fetch_add(v, Relaxed);
}
fn add32(p: *mut u32, v: u32) {
    // SAFETY: as add.
    unsafe { AtomicU32::from_ptr(p) }.fetch_add(v, Relaxed);
}
fn store(p: *mut u64, v: u64) {
    // SAFETY: as add.
    unsafe { AtomicU64::from_ptr(p) }.store(v, Relaxed);
}
fn max(p: *mut u64, v: u64) {
    // SAFETY: as add.
    unsafe { AtomicU64::from_ptr(p) }.fetch_max(v, Relaxed);
}
macro_rules! table {
    ($name:ident) => {
        // The export, live for the process (its fields used atomically or by one writer).
        core::ptr::addr_of_mut!(x::$name)
    };
}
fn stats(i: usize) -> *mut u64 {
    // SAFETY: inside the 32 stats.
    unsafe { table!(CyberpunkVR_StageReplayStats).cast::<u64>().add(i) }
}
fn summary(i: usize) -> *mut u64 {
    // SAFETY: inside the 16 summary words.
    unsafe { table!(CyberpunkVR_StageCensusSummary).cast::<u64>().add(i) }
}
fn camera_diag(i: usize) -> *mut u64 {
    // SAFETY: inside the 12 diagnostics.
    unsafe { table!(CyberpunkVR_StageReplayCameraDiag).cast::<u64>().add(i) }
}
fn base() -> u64 {
    // SAFETY: the executable's own module.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 }
}
fn read_safe(address: u64, out: &mut [u8]) -> bool {
    address != 0 && cvr_engine::stack::read(address, out)
}
unsafe extern "C-unwind" fn read_fn(address: usize, out: *mut c_void, bytes: usize) -> bool {
    // SAFETY: the caller's output of `bytes`.
    read_safe(address as u64, unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), bytes) })
}
/// The native renderer frame clock (renderer+0x4CC8), tying a source capture to the target of the same frame.
fn engine_frame() -> u32 {
    crate::stage_replay_ring::cvr_srg_engine_frame(base())
}

// ---------------------------------------------------------------------------------------------------------------------
// The stream: fixed capacity, never reallocated past its reserve's first slice while recording would need it; an
// overflow is sticky and makes it unusable for replay.
// ---------------------------------------------------------------------------------------------------------------------
#[derive(Default)]
pub struct Stream {
    commands: Vec<Command>,
    payload: Vec<u8>,
    max_commands: usize,
    max_payload: usize,
    overflow: bool,
    blocking: u32,
    draws: u32,
}
impl Stream {
    pub fn reserve(&mut self, commands: usize, payload: usize) {
        self.commands.reserve(commands.min(2048));
        self.payload.reserve(payload.min(64 << 10));
        (self.max_commands, self.max_payload) = (commands, payload);
    }
    pub fn clear(&mut self) {
        self.commands.clear();
        self.payload.clear();
        (self.overflow, self.blocking, self.draws) = (false, 0, 0);
    }
    /// Push: `bytes` of payload (8-byte aligned) at `data`, its offset in command.d.
    pub fn push(&mut self, mut command: Command, data: *const u8, bytes: usize) -> bool {
        if self.overflow {
            return false;
        }
        if self.commands.len() >= self.max_commands {
            self.overflow = true;
            return false;
        }
        if !data.is_null() && bytes != 0 {
            let offset = (self.payload.len() + 7) & !7;
            if offset + bytes > self.max_payload {
                self.overflow = true;
                return false;
            }
            self.payload.resize(offset + bytes, 0);
            // SAFETY: the caller's `bytes` at `data`.
            self.payload[offset..].copy_from_slice(unsafe { core::slice::from_raw_parts(data, bytes) });
            command.d = offset as u64;
        } else if bytes == 0 && data.is_null() && command.d == 0 {
            command.d = NO_PAYLOAD;
        }
        if rules::blocking(command.op) {
            self.blocking += 1;
        }
        if rules::is_draw(command.op) {
            self.draws += 1;
        }
        self.commands.push(command);
        true
    }
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }
    pub fn payload(&self, offset: u64, bytes: usize) -> Option<*const u8> {
        // SAFETY: inside the payload.
        rules::payload_in_bounds(offset, bytes, self.payload.len()).then(|| unsafe { self.payload.as_ptr().add(offset as usize) })
    }
    pub fn overflowed(&self) -> bool {
        self.overflow
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The list shadow: graphics state since the list's last Reset, mirrored from the API calls -- what the engine believes
// is bound, so replay restores exactly these values afterwards. A field never set since Reset is never restored.
// ---------------------------------------------------------------------------------------------------------------------
mod param_kind {
    pub const TABLE: u8 = 1;
    pub const CBV: u8 = 2;
    pub const SRV: u8 = 3;
    pub const UAV: u8 = 4;
    pub const CONSTANTS: u8 = 5;
}
#[derive(Clone, Copy, Default)]
struct Param {
    kind: u8,
    value: u64,
    words: [u32; 16],
    word_mask: u16,
}
#[derive(Clone)]
struct Shadow {
    #[allow(dead_code)]
    list: u64,
    generation: u32,
    valid: bool,
    overflow: bool,
    pso: usize,
    pso_set: bool,
    root: usize,
    root_set: bool,
    params: [Param; 64],
    topology: i32,
    topology_set: bool,
    index: D3D12_INDEX_BUFFER_VIEW,
    index_set: bool,
    index_null: bool,
    vertices: [D3D12_VERTEX_BUFFER_VIEW; 16],
    vertex_mask: u32,
    viewports: [D3D12_VIEWPORT; 16],
    viewport_count: u32,
    viewports_set: bool,
    scissors: [RECT; 16],
    scissor_count: u32,
    scissors_set: bool,
    blend: [f32; 4],
    blend_set: bool,
    stencil: u32,
    stencil_set: bool,
    depth_min: f32,
    depth_max: f32,
    depth_bounds_set: bool,
    heaps: [usize; 2],
    heap_count: u32,
    heaps_set: bool,
    rate: i32,
    combiners: [i32; 2],
    rate_set: bool,
    rate_image: usize,
    rate_image_set: bool,
    view_mask: u32,
    view_mask_set: bool,
}
impl Default for Shadow {
    fn default() -> Self {
        Shadow {
            list: 0,
            generation: 0,
            valid: false,
            overflow: false,
            pso: 0,
            pso_set: false,
            root: 0,
            root_set: false,
            params: [Param::default(); 64],
            topology: 0,
            topology_set: false,
            index: Default::default(),
            index_set: false,
            index_null: false,
            vertices: [Default::default(); 16],
            vertex_mask: 0,
            viewports: [Default::default(); 16],
            viewport_count: 0,
            viewports_set: false,
            scissors: [Default::default(); 16],
            scissor_count: 0,
            scissors_set: false,
            blend: [0.0; 4],
            blend_set: false,
            stencil: 0,
            stencil_set: false,
            depth_min: 0.0,
            depth_max: 1.0,
            depth_bounds_set: false,
            heaps: [0; 2],
            heap_count: 0,
            heaps_set: false,
            rate: 0,
            combiners: [0; 2],
            rate_set: false,
            rate_image: 0,
            rate_image_set: false,
            view_mask: 0,
            view_mask_set: false,
        }
    }
}
impl Shadow {
    fn reset(&mut self, list: u64, initial: usize, generation: u32) {
        *self = Shadow { list, valid: true, generation, ..Shadow::default() };
        if initial != 0 {
            (self.pso, self.pso_set) = (initial, true);
        }
    }
    fn set_root(&mut self, root: usize) {
        (self.root, self.root_set) = (root, true);
        self.params = [Param::default(); 64];
    }
    fn set_view(&mut self, param: u32, kind: u8, value: u64) {
        let Some(p) = self.params.get_mut(param as usize) else {
            self.overflow = true;
            return;
        };
        (p.kind, p.value, p.word_mask) = (kind, value, 0);
    }
    fn set_constants(&mut self, param: u32, count: u32, words: *const u32, offset: u32) {
        if param as usize >= self.params.len() || words.is_null() || offset.wrapping_add(count) > 16 || offset > 16 {
            self.overflow = true;
            return;
        }
        let p = &mut self.params[param as usize];
        if p.kind != param_kind::CONSTANTS {
            *p = Param { kind: param_kind::CONSTANTS, ..Param::default() };
        }
        for i in 0..count {
            // SAFETY: the caller's `count` words.
            p.words[(offset + i) as usize] = unsafe { *words.add(i as usize) };
            p.word_mask |= 1 << (offset + i);
        }
    }
    fn set_vertices(&mut self, start: u32, count: u32, views: *const D3D12_VERTEX_BUFFER_VIEW) {
        for i in 0..count {
            let slot = start.wrapping_add(i);
            if slot >= 16 {
                self.overflow = true;
                return;
            }
            // SAFETY: the caller's `count` views.
            self.vertices[slot as usize] = if views.is_null() { Default::default() } else { unsafe { *views.add(i as usize) } };
            self.vertex_mask |= 1 << slot;
        }
    }
    fn set_viewports(&mut self, count: u32, v: *const D3D12_VIEWPORT) {
        if count > 16 {
            self.overflow = true;
            return;
        }
        (self.viewport_count, self.viewports_set) = (count, true);
        for i in 0..count as usize {
            // SAFETY: the caller's `count` viewports.
            self.viewports[i] = unsafe { *v.add(i) };
        }
    }
    fn set_scissors(&mut self, count: u32, r: *const RECT) {
        if count > 16 {
            self.overflow = true;
            return;
        }
        (self.scissor_count, self.scissors_set) = (count, true);
        for i in 0..count as usize {
            // SAFETY: the caller's `count` rectangles.
            self.scissors[i] = unsafe { *r.add(i) };
        }
    }
    fn set_heaps(&mut self, count: u32, h: *const usize) {
        if count > 2 {
            self.overflow = true;
            return;
        }
        (self.heap_count, self.heaps_set, self.heaps) = (count, true, [0; 2]);
        for i in 0..count as usize {
            // SAFETY: the caller's `count` heaps.
            self.heaps[i] = unsafe { *h.add(i) };
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Chained hooks
// ---------------------------------------------------------------------------------------------------------------------
const SLOT_COUNT: usize = 81;
struct ChainSlots {
    vtable: AtomicUsize,
    prev: [AtomicUsize; SLOT_COUNT],
}
#[derive(Clone, Copy, Default)]
struct ChainMeta {
    last: usize,
    list1: bool,
    list2: bool,
    list4: bool,
    list5: bool,
    list7: bool,
    installed: bool,
}
static CHAINS: [ChainSlots; 16] = [const { ChainSlots { vtable: AtomicUsize::new(0), prev: [const { AtomicUsize::new(0) }; SLOT_COUNT] } }; 16];
static CHAIN_META: Mutex<[ChainMeta; 16]> = Mutex::new([ChainMeta { last: 0, list1: false, list2: false, list4: false, list5: false, list7: false, installed: false }; 16]);
static CHAIN_COUNT: AtomicU32 = AtomicU32::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static TRACKING: AtomicBool = AtomicBool::new(false);
/// Bumped whenever tracking (re)starts: a mirror is only trusted when its Reset was observed in the current generation.
static GENERATION: AtomicU32 = AtomicU32::new(1);

fn prev(this: *mut c_void, slot: usize) -> usize {
    // SAFETY: a live COM object's vtable pointer.
    let vt = unsafe { *this.cast::<usize>() };
    let n = CHAIN_COUNT.load(Acquire) as usize;
    CHAINS[..n].iter().find(|c| c.vtable.load(Relaxed) == vt).map_or(0, |c| c.prev[slot].load(Relaxed))
}
macro_rules! forward {
    ($this:ident, $slot:expr, fn($($t:ty),*) $(-> $r:ty)?, ($($a:expr),*)) => {{
        let p = prev($this, $slot);
        if p == 0 {
            return Default::default();
        }
        // SAFETY: the slot's predecessor, of the method's signature.
        let f: unsafe extern "system" fn(*mut c_void $(, $t)*) $(-> $r)? = unsafe { core::mem::transmute(p) };
        unsafe { f($this $(, $a)*) }
    }};
}

// ---------------------------------------------------------------------------------------------------------------------
// Shadows: one mirror per live command list, open addressing over the list pointer; a slot whose list has not been
// Reset for 240 presents is taken over by a new list (838: the table filled up without that).
// ---------------------------------------------------------------------------------------------------------------------
const SHADOW_CAPACITY: u32 = 1024;
const SHADOW_PROBE: u32 = 64;
const SHADOW_STALE_FRAMES: u32 = 240;
struct ShadowSlot {
    key: AtomicU64,
    last_reset: AtomicU32,
    shadow: UnsafeCell<Shadow>,
}
// SAFETY: a slot's shadow is written by the one thread recording its list.
unsafe impl Sync for ShadowSlot {}
static SHADOWS: OnceLock<Box<[ShadowSlot]>> = OnceLock::new();
static PRESENTS: AtomicU32 = AtomicU32::new(1);
thread_local! {
    static T_SHADOW: Cell<(u64, *const ShadowSlot)> = const { Cell::new((0, core::ptr::null())) };
    static T_REPLAYING: Cell<bool> = const { Cell::new(false) };
    static T_SCOPE: Cell<*mut Scope> = const { Cell::new(core::ptr::null_mut()) };
}
fn slot_for(list: *mut c_void, claim: bool) -> Option<&'static ShadowSlot> {
    let key = list as u64;
    let (cached_key, cached) = T_SHADOW.with(Cell::get);
    // SAFETY: slots live for the process.
    if let Some(slot) = unsafe { cached.as_ref() } {
        // The cached slot is re-checked: it may have been handed to another list since.
        if cached_key == key && slot.key.load(Relaxed) == key {
            return Some(slot);
        }
    }
    let shadows = SHADOWS.get()?;
    let now = PRESENTS.load(Relaxed);
    let h = ((key >> 4) as u32).wrapping_mul(2_654_435_761);
    let remember = |slot: &'static ShadowSlot| {
        T_SHADOW.with(|t| t.set((key, slot)));
        Some(slot)
    };
    let take = |slot: &'static ShadowSlot| {
        // SAFETY: the slot just claimed for this list.
        unsafe { *slot.shadow.get() = Shadow { list: key, ..Shadow::default() } };
        slot.last_reset.store(now, Relaxed);
        add(summary(4), 1);
        remember(slot)
    };
    let mut stale: Option<(&'static ShadowSlot, u64)> = None;
    for probe in 0..SHADOW_PROBE {
        let slot = &shadows[(h.wrapping_add(probe) & (SHADOW_CAPACITY - 1)) as usize];
        let current = slot.key.load(Acquire);
        if current == key {
            return remember(slot);
        }
        if current == 0 {
            if !claim {
                break; // keys are never removed, so the key is not further on
            }
            match slot.key.compare_exchange(0, key, AcqRel, Acquire) {
                Ok(_) => return take(slot),
                Err(seen) if seen == key => return remember(slot),
                Err(_) => continue,
            }
        }
        if claim && stale.is_none() && now.wrapping_sub(slot.last_reset.load(Relaxed)) > SHADOW_STALE_FRAMES {
            stale = Some((slot, current));
        }
    }
    if let Some((slot, stale_key)) = stale.filter(|_| claim) {
        if slot.key.compare_exchange(stale_key, key, AcqRel, Acquire).is_ok() {
            add(summary(7), 1); // slots taken over
            return take(slot);
        }
    }
    add(summary(5), 1);
    None
}
fn shadow_for(list: *mut c_void, claim: bool) -> Option<&'static mut Shadow> {
    // SAFETY: this thread records the list.
    slot_for(list, claim).map(|s| unsafe { &mut *s.shadow.get() })
}
fn note_reset(list: *mut c_void) {
    if let Some(slot) = slot_for(list, false) {
        slot.last_reset.store(PRESENTS.load(Relaxed), Relaxed);
    }
}
/// Track: replay is tracked too -- whatever it leaves bound is the list's real state for the next stage.
fn track(list: *mut c_void, f: impl FnOnce(&mut Shadow)) {
    if !TRACKING.load(Relaxed) {
        return;
    }
    if let Some(s) = shadow_for(list, true) {
        f(s);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Captures
// ---------------------------------------------------------------------------------------------------------------------
mod cap_state {
    pub const EMPTY: u32 = 0;
    pub const WRITING: u32 = 1;
    pub const COMPLETE: u32 = 2;
    pub const READING: u32 = 3;
}
#[derive(Default)]
struct CaptureData {
    stream: Stream,
    frame: u32,
    stage: u32,
    ordinal: u32,
    list: u64,
    cam_vs: u64,
    cam_ps: u64,
    span_begin: u64,
    span_end: u64,
    view: [u64; VIEW_DESCRIPTORS],
    foreign: bool,
}
struct CaptureSlot {
    state: AtomicU32,
    data: UnsafeCell<CaptureData>,
}
// SAFETY: the data is owned by whoever moved the state to writing or reading.
unsafe impl Sync for CaptureSlot {}
impl CaptureSlot {
    fn data(&self) -> &mut CaptureData {
        // SAFETY: as Sync's.
        unsafe { &mut *self.data.get() }
    }
}
const MAX_CAPTURE_SLOTS: usize = 96;
static CAPTURE_MUTEX: Mutex<Vec<Box<CaptureSlot>>> = Mutex::new(Vec::new());
static CAPTURES: [AtomicPtr<CaptureSlot>; 64 * MAX_ORDINALS as usize * 2] = [const { AtomicPtr::new(core::ptr::null_mut()) }; 64 * MAX_ORDINALS as usize * 2];
static INCREMENT: AtomicU32 = AtomicU32::new(0);
static DEVICE: AtomicUsize = AtomicUsize::new(0);
static COPY_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
type CopyDescriptors = unsafe extern "system" fn(usize, u32, *const usize, *const u32, u32, *const usize, *const u32, i32);

/// Slot: one per (stage, ordinal, frame parity), so a capture of frame N survives while N+1 is recorded.
fn slot(stage: u32, ordinal: u32, frame: u32) -> Option<&'static CaptureSlot> {
    if stage >= 64 || ordinal >= MAX_ORDINALS {
        return None;
    }
    let index = ((stage * MAX_ORDINALS + ordinal) * 2 + (frame & 1)) as usize;
    // SAFETY: published slots live for the process.
    if let Some(s) = unsafe { CAPTURES[index].load(Acquire).as_ref() } {
        return Some(s);
    }
    let mut storage = CAPTURE_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = unsafe { CAPTURES[index].load(Acquire).as_ref() } {
        return Some(s);
    }
    // Every (stage, ordinal, frame parity) in use needs its own slot (run 831 lost stages at a cap of 32).
    if storage.len() >= MAX_CAPTURE_SLOTS {
        return None;
    }
    let mut data = CaptureData::default();
    data.stream.reserve(32768, 2 << 20);
    let boxed = Box::new(CaptureSlot { state: AtomicU32::new(cap_state::EMPTY), data: UnsafeCell::new(data) });
    let raw: *const CaptureSlot = &*boxed;
    storage.push(boxed);
    CAPTURES[index].store(raw.cast_mut(), Release);
    // SAFETY: kept in the storage for the process.
    Some(unsafe { &*raw })
}

// ---------------------------------------------------------------------------------------------------------------------
// The scope
// ---------------------------------------------------------------------------------------------------------------------
const MAX_GROUPS: usize = 24;
#[derive(Clone, Copy)]
struct Scope {
    stream: *mut Stream,
    row: *mut u64,
    ops: *mut u32,
    capture: *const CaptureSlot,
    list: u64,
    heap_cpu: u64,
    heap_gpu: u64,
    stride: u32,
    heap_count: u32,
    cam_vs: u64,
    cam_ps: u64,
    /// The source's view descriptors (capture).
    view: [u64; VIEW_DESCRIPTORS],
    preserve_camera: bool,
    state: u64,
    layout: u64,
    cpu_table: u64,
    /// The layout's descriptor-table groups: (root parameter, CPU-table start, count).
    groups: [[u32; 3]; MAX_GROUPS],
    group_count: u32,
}
impl Default for Scope {
    fn default() -> Self {
        Scope {
            stream: core::ptr::null_mut(),
            row: core::ptr::null_mut(),
            ops: core::ptr::null_mut(),
            capture: core::ptr::null(),
            list: 0,
            heap_cpu: 0,
            heap_gpu: 0,
            stride: 0,
            heap_count: 0,
            cam_vs: 0,
            cam_ps: 0,
            view: [0; VIEW_DESCRIPTORS],
            preserve_camera: false,
            state: 0,
            layout: 0,
            cpu_table: 0,
            groups: [[0; 3]; MAX_GROUPS],
            group_count: 0,
        }
    }
}
fn row_add(row: *mut u64, column: usize, v: u64) {
    if !row.is_null() {
        // SAFETY: a census row of 32 columns.
        add(unsafe { row.add(column) }, v);
    }
}
fn volatile_u64(address: u64) -> u64 {
    // SAFETY: the engine's live binding state (the .cpp's plain volatile read).
    unsafe { core::ptr::read_volatile(address as *const u64) }
}
/// The layout pointer changes with every program switch: its groups read without allocating, the last 8 layouts
/// remembered per thread (run 826: ~100 ns per command before).
fn refresh_scope_groups(s: &mut Scope) {
    struct Entry {
        layout: u64,
        groups: [[u32; 3]; MAX_GROUPS],
        count: u32,
    }
    thread_local! {
        static CACHE: core::cell::RefCell<([Entry; 8], u32)> = const { core::cell::RefCell::new(([const { Entry { layout: 0, groups: [[0; 3]; MAX_GROUPS], count: 0 } }; 8], 0)) };
    }
    s.group_count = 0;
    s.layout = volatile_u64(s.state + 0x68);
    s.cpu_table = volatile_u64(s.state + 0x28);
    if s.layout == 0 || s.layout > u64::MAX - 0x473 {
        return;
    }
    CACHE.with(|c| {
        let (cache, next) = &mut *c.borrow_mut();
        if let Some(e) = cache.iter().find(|e| e.layout == s.layout) {
            (s.groups, s.group_count) = (e.groups, e.count);
            return;
        }
        let e = &mut cache[(*next % 8) as usize];
        *next = next.wrapping_add(1);
        e.layout = s.layout;
        // SAFETY: the entry's outputs.
        unsafe { crate::stage_replay_layout::cvr_srl_groups(s.layout, &mut e.groups, &mut e.count) };
        (s.groups, s.group_count) = (e.groups, e.count);
    });
}
/// RecordCameraTable: at the source's bind of a table, a table holding any of the source's view descriptors is kept
/// as payload [count, descriptors...] so the replay can rebuild it with the target's.
fn record_camera_table(s: &mut Scope, c: &mut Command, buffer: &mut [u64; 257]) -> usize {
    if volatile_u64(s.state + 0x68) != s.layout {
        refresh_scope_groups(s);
    }
    for k in 0..s.group_count as usize {
        let [param, start, count] = s.groups[k];
        if param != c.a || s.cpu_table == 0 || count == 0 || count > 256 {
            continue;
        }
        let mut view = false;
        for i in 0..count as usize {
            buffer[1 + i] = volatile_u64(s.cpu_table + (u64::from(start) + i as u64) * 8);
            view |= rules::contains(&s.view, buffer[1 + i]);
        }
        if !view {
            return 0;
        }
        buffer[0] = count.into();
        c.flags |= 8;
        row_add(s.row, col::CAMERA_TABLES, 1);
        return (count as usize + 1) * 8;
    }
    0
}
fn record(list: *mut c_void, mut c: Command, data: *const u8, bytes: usize) {
    let s = T_SCOPE.with(Cell::get);
    if s.is_null() || T_REPLAYING.with(Cell::get) {
        return;
    }
    // SAFETY: the thread's live scope.
    let s = unsafe { &mut *s };
    if !s.ops.is_null() && c.op < 48 {
        // SAFETY: a census ops row of 48.
        add32(unsafe { s.ops.add(c.op as usize) }, 1);
    }
    if !list.is_null() && list as u64 != s.list {
        c.flags |= 1;
        row_add(s.row, col::FOREIGN_OPS, 1);
        // SAFETY: the scope's capture, written by this thread.
        if let Some(capture) = unsafe { s.capture.as_ref() } {
            capture.data().foreign = true;
        }
    }
    row_add(s.row, col::COMMANDS, 1);
    if rules::is_draw(c.op) {
        row_add(s.row, col::DRAWS, 1);
    }
    if rules::blocking(c.op) {
        row_add(s.row, col::BLOCKING_OPS, 1);
    }
    if c.op == op::ROOT_TABLE {
        row_add(s.row, col::TABLES, 1);
    }
    // SAFETY: the scope's stream, written by this thread.
    let Some(stream) = (unsafe { s.stream.as_mut() }) else { return };
    if c.op == op::ROOT_TABLE && s.capture.is_null() && s.stride != 0 && c.b >= s.heap_gpu && c.b < s.heap_gpu + u64::from(s.heap_count) * u64::from(s.stride) {
        let index = (c.b - s.heap_gpu) / u64::from(s.stride);
        c.c = index;
        // The flush copies a table immediately before binding it; look back a little.
        let commands = stream.commands();
        for (seen, i) in (0..commands.len()).rev().enumerate() {
            if seen >= 256 {
                break;
            }
            if commands[i].op == op::COPY && u64::from(commands[i].a) == index {
                c.count = (i + 1) as u16;
                row_add(s.row, col::TABLES_MATCHED, 1);
                if commands[i].flags & 2 != 0 {
                    row_add(s.row, col::CAMERA_TABLES, 1);
                    c.flags |= 2;
                }
                break;
            }
        }
    } else if c.op == op::ROOT_TABLE {
        c.c = u64::MAX;
    }
    if c.op == op::ROOT_TABLE && s.preserve_camera && s.state != 0 {
        let mut buffer = [0u64; 257];
        let table = record_camera_table(s, &mut c, &mut buffer);
        if table != 0 {
            stream.push(c, buffer.as_ptr().cast(), table);
            return;
        }
    }
    // Diagnostic (census streams only): every table's contents from the CPU table.
    if c.op == op::ROOT_TABLE && s.capture.is_null() && s.state != 0 && x::CyberpunkVR_StageReplayTableDump.load(Relaxed) != 0 {
        let mut buffer = [0u64; 257];
        // SAFETY: our buffer.
        let table = unsafe { crate::stage_replay_layout::cvr_srl_any_table(s.state, c.a.into(), &mut buffer) };
        if table != 0 {
            c.flags |= 16;
            stream.push(c, buffer.as_ptr().cast(), table);
            return;
        }
    }
    stream.push(c, data, bytes);
}
fn cmd(op: u8, flags: u8, count: u16, a: u32, b: u64, c: u64) -> Command {
    Command { op, flags, count, a, b, c, d: 0 }
}

// ---------------------------------------------------------------------------------------------------------------------
// Hook bodies: each forwards first, then records/tracks; replay calls skip the recording.
// ---------------------------------------------------------------------------------------------------------------------
type This = *mut c_void;
fn generation() -> u32 {
    GENERATION.load(Relaxed)
}
unsafe extern "system" fn h_reset(this: This, allocator: usize, pso: usize) -> i32 {
    let p = prev(this, 10);
    if p == 0 {
        return windows::Win32::Foundation::E_FAIL.0;
    }
    // SAFETY: the predecessor of Reset.
    let f: unsafe extern "system" fn(This, usize, usize) -> i32 = unsafe { core::mem::transmute(p) };
    let out = unsafe { f(this, allocator, pso) };
    if out >= 0 {
        track(this, |s| s.reset(this as u64, pso, generation()));
        if TRACKING.load(Relaxed) {
            note_reset(this);
        }
    }
    out
}
unsafe extern "system" fn h_clear_state(this: This, pso: usize) {
    forward!(this, 11, fn(usize), (pso));
    track(this, |s| s.reset(this as u64, pso, generation()));
    if TRACKING.load(Relaxed) {
        note_reset(this);
    }
    record(this, cmd(OP_RESET, 0, 0, 0, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_draw(this: This, v: u32, i: u32, sv: u32, si: u32) {
    forward!(this, 12, fn(u32, u32, u32, u32), (v, i, sv, si));
    record(this, cmd(op::DRAW, 0, 0, v, u64::from(i) | u64::from(sv) << 32, si.into()), core::ptr::null(), 0);
}
unsafe extern "system" fn h_draw_indexed(this: This, n: u32, i: u32, si: u32, bv: i32, sinst: u32) {
    forward!(this, 13, fn(u32, u32, u32, i32, u32), (n, i, si, bv, sinst));
    record(this, cmd(op::DRAW_INDEXED, 0, 0, n, u64::from(i) | u64::from(si) << 32, u64::from(bv as u32) | u64::from(sinst) << 32), core::ptr::null(), 0);
}
unsafe extern "system" fn h_dispatch(this: This, x: u32, y: u32, z: u32) {
    forward!(this, 14, fn(u32, u32, u32), (x, y, z));
    record(this, cmd(op::DISPATCH, 0, 0, x, y.into(), z.into()), core::ptr::null(), 0);
}
unsafe extern "system" fn h_copy_buffer(this: This, d: usize, doff: u64, s: usize, soff: u64, n: u64) {
    forward!(this, 15, fn(usize, u64, usize, u64, u64), (d, doff, s, soff, n));
    let mut c = cmd(OP_COPY_BUFFER, 0, 0, n as u32, d as u64, doff);
    c.d = s as u64;
    record(this, c, core::ptr::null(), 0);
}
unsafe extern "system" fn h_copy_texture(this: This, d: *const D3D12_TEXTURE_COPY_LOCATION, x: u32, y: u32, z: u32, s: usize, bx: usize) {
    forward!(this, 16, fn(*const D3D12_TEXTURE_COPY_LOCATION, u32, u32, u32, usize, usize), (d, x, y, z, s, bx));
    // SAFETY: the caller's copy location.
    let resource = if d.is_null() { 0 } else { (unsafe { core::ptr::read(d as *const usize) }) as u64 };
    record(this, cmd(OP_COPY_TEXTURE, 0, 0, 0, resource, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_copy_resource(this: This, d: usize, s: usize) {
    forward!(this, 17, fn(usize, usize), (d, s));
    record(this, cmd(OP_COPY_TEXTURE, 0, 0, 1, d as u64, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_topology(this: This, t: i32) {
    forward!(this, 20, fn(i32), (t));
    track(this, |s| (s.topology, s.topology_set) = (t, true));
    record(this, cmd(op::TOPOLOGY, 0, 0, t as u32, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_viewports(this: This, n: u32, v: *const D3D12_VIEWPORT) {
    forward!(this, 21, fn(u32, *const D3D12_VIEWPORT), (n, v));
    track(this, |s| {
        if !v.is_null() {
            s.set_viewports(n, v)
        }
    });
    let bytes = if v.is_null() { 0 } else { n as usize * core::mem::size_of::<D3D12_VIEWPORT>() };
    record(this, cmd(op::VIEWPORTS, 0, n as u16, 0, 0, 0), v.cast(), bytes);
}
unsafe extern "system" fn h_scissors(this: This, n: u32, r: *const RECT) {
    forward!(this, 22, fn(u32, *const RECT), (n, r));
    track(this, |s| {
        if !r.is_null() {
            s.set_scissors(n, r)
        }
    });
    let bytes = if r.is_null() { 0 } else { n as usize * core::mem::size_of::<RECT>() };
    record(this, cmd(op::SCISSORS, 0, n as u16, 0, 0, 0), r.cast(), bytes);
}
unsafe extern "system" fn h_blend(this: This, f: *const f32) {
    forward!(this, 23, fn(*const f32), (f));
    track(this, |s| {
        // SAFETY: the caller's four factors, or the default.
        s.blend = if f.is_null() { [1.0; 4] } else { unsafe { *f.cast::<[f32; 4]>() } };
        s.blend_set = true;
    });
    record(this, cmd(op::BLEND_FACTOR, if f.is_null() { 4 } else { 0 }, 0, 0, 0, 0), f.cast(), if f.is_null() { 0 } else { 16 });
}
unsafe extern "system" fn h_stencil(this: This, reference: u32) {
    forward!(this, 24, fn(u32), (reference));
    track(this, |s| (s.stencil, s.stencil_set) = (reference, true));
    record(this, cmd(op::STENCIL_REF, 0, 0, reference, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_pipeline(this: This, pso: usize) {
    forward!(this, 25, fn(usize), (pso));
    track(this, |s| (s.pso, s.pso_set) = (pso, true));
    record(this, cmd(op::PIPELINE, 0, 0, 0, pso as u64, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_barrier(this: This, n: u32, b: *const D3D12_RESOURCE_BARRIER) {
    forward!(this, 26, fn(u32, *const D3D12_RESOURCE_BARRIER), (n, b));
    // The first barrier's transition resource (the union's first field after type and flags).
    let first = if n != 0 && !b.is_null() { (unsafe { *(b as *const u8).add(8).cast::<usize>() }) as u64 } else { 0 };
    let bytes = if b.is_null() { 0 } else { n.min(64) as usize * core::mem::size_of::<D3D12_RESOURCE_BARRIER>() };
    record(this, cmd(OP_BARRIER, 0, n as u16, 0, first, 0), b.cast(), bytes);
}
unsafe extern "system" fn h_bundle(this: This, bundle: usize) {
    forward!(this, 27, fn(usize), (bundle));
    track(this, |s| s.valid = false);
    record(this, cmd(OP_BUNDLE, 0, 0, 0, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_heaps(this: This, n: u32, h: *const usize) {
    forward!(this, 28, fn(u32, *const usize), (n, h));
    track(this, |s| {
        if !h.is_null() {
            s.set_heaps(n, h)
        }
    });
    record(this, cmd(op::HEAPS, 0, n as u16, 0, 0, 0), h.cast(), if h.is_null() { 0 } else { n as usize * 8 });
}
unsafe extern "system" fn h_compute_root(this: This, r: usize) {
    forward!(this, 29, fn(usize), (r));
    record(this, cmd(op::COMPUTE_STATE, 0, 0, 29, r as u64, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_graphics_root(this: This, r: usize) {
    forward!(this, 30, fn(usize), (r));
    track(this, |s| s.set_root(r));
    record(this, cmd(op::ROOT_SIGNATURE, 0, 0, 0, r as u64, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_compute_table(this: This, i: u32, h: u64) {
    forward!(this, 31, fn(u32, u64), (i, h));
    record(this, cmd(op::COMPUTE_STATE, 0, 0, 31, h, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_graphics_table(this: This, i: u32, h: u64) {
    forward!(this, 32, fn(u32, u64), (i, h));
    track(this, |s| s.set_view(i, param_kind::TABLE, h));
    record(this, cmd(op::ROOT_TABLE, 0, 0, i, h, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_compute_constant(this: This, i: u32, v: u32, o: u32) {
    forward!(this, 33, fn(u32, u32, u32), (i, v, o));
    record(this, cmd(op::COMPUTE_STATE, 0, 0, 33, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_graphics_constant(this: This, i: u32, v: u32, o: u32) {
    forward!(this, 34, fn(u32, u32, u32), (i, v, o));
    track(this, |s| s.set_constants(i, 1, &v, o));
    record(this, cmd(op::ROOT_CONSTANT, 0, 0, i, v.into(), o.into()), core::ptr::null(), 0);
}
unsafe extern "system" fn h_compute_constants(this: This, i: u32, n: u32, v: *const c_void, o: u32) {
    forward!(this, 35, fn(u32, u32, *const c_void, u32), (i, n, v, o));
    record(this, cmd(op::COMPUTE_STATE, 0, 0, 35, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_graphics_constants(this: This, i: u32, n: u32, v: *const c_void, o: u32) {
    forward!(this, 36, fn(u32, u32, *const c_void, u32), (i, n, v, o));
    track(this, |s| s.set_constants(i, n, v.cast(), o));
    record(this, cmd(op::ROOT_CONSTANTS, 0, n as u16, i, 0, o.into()), v.cast(), if v.is_null() { 0 } else { n as usize * 4 });
}
unsafe fn root_view(this: This, slot: usize, kind_op: u8, shadow_kind: u8, i: u32, a: u64) {
    forward!(this, slot, fn(u32, u64), (i, a));
    if shadow_kind != 0 {
        track(this, |s| s.set_view(i, shadow_kind, a));
    }
    record(this, cmd(kind_op, 0, 0, i, a, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_view37(this: This, i: u32, a: u64) {
    unsafe { root_view(this, 37, op::COMPUTE_STATE, 0, i, a) }
}
unsafe extern "system" fn h_view38(this: This, i: u32, a: u64) {
    unsafe { root_view(this, 38, op::ROOT_CBV, param_kind::CBV, i, a) }
}
unsafe extern "system" fn h_view39(this: This, i: u32, a: u64) {
    unsafe { root_view(this, 39, op::COMPUTE_STATE, 0, i, a) }
}
unsafe extern "system" fn h_view40(this: This, i: u32, a: u64) {
    unsafe { root_view(this, 40, op::ROOT_SRV, param_kind::SRV, i, a) }
}
unsafe extern "system" fn h_view41(this: This, i: u32, a: u64) {
    unsafe { root_view(this, 41, op::COMPUTE_STATE, 0, i, a) }
}
unsafe extern "system" fn h_view42(this: This, i: u32, a: u64) {
    unsafe { root_view(this, 42, op::ROOT_UAV, param_kind::UAV, i, a) }
}
unsafe extern "system" fn h_index_buffer(this: This, v: *const D3D12_INDEX_BUFFER_VIEW) {
    forward!(this, 43, fn(*const D3D12_INDEX_BUFFER_VIEW), (v));
    track(this, |s| {
        (s.index_set, s.index_null) = (true, v.is_null());
        if !v.is_null() {
            // SAFETY: the caller's view.
            s.index = unsafe { *v };
        }
    });
    let bytes = if v.is_null() { 0 } else { core::mem::size_of::<D3D12_INDEX_BUFFER_VIEW>() };
    record(this, cmd(op::INDEX_BUFFER, if v.is_null() { 4 } else { 0 }, 0, 0, 0, 0), v.cast(), bytes);
}
unsafe extern "system" fn h_vertex_buffers(this: This, start: u32, n: u32, v: *const D3D12_VERTEX_BUFFER_VIEW) {
    forward!(this, 44, fn(u32, u32, *const D3D12_VERTEX_BUFFER_VIEW), (start, n, v));
    track(this, |s| s.set_vertices(start, n, v));
    let bytes = if v.is_null() { 0 } else { n as usize * core::mem::size_of::<D3D12_VERTEX_BUFFER_VIEW>() };
    record(this, cmd(op::VERTEX_BUFFERS, if v.is_null() { 4 } else { 0 }, n as u16, start, 0, 0), v.cast(), bytes);
}
unsafe extern "system" fn h_so_targets(this: This, start: u32, n: u32, v: *const c_void) {
    forward!(this, 45, fn(u32, u32, *const c_void), (start, n, v));
    record(this, cmd(OP_OTHER, 0, 0, 45, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_targets(this: This, n: u32, rt: *const usize, single: i32, ds: *const usize) {
    forward!(this, 46, fn(u32, *const usize, i32, *const usize), (n, rt, single, ds));
    // SAFETY: the caller's handles.
    let (first, depth) = unsafe { (if n != 0 && !rt.is_null() { *rt as u64 } else { 0 }, if ds.is_null() { 0 } else { *ds as u64 }) };
    record(this, cmd(OP_TARGETS, 0, n as u16, 0, first, depth), core::ptr::null(), 0);
}
unsafe extern "system" fn h_clear_depth(this: This, h: usize, f: i32, d: f32, s: u8, n: u32, r: *const RECT) {
    forward!(this, 47, fn(usize, i32, f32, u8, u32, *const RECT), (h, f, d, s, n, r));
    record(this, cmd(OP_CLEAR, 0, 0, 47, h as u64, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_clear_rtv(this: This, h: usize, c: *const f32, n: u32, r: *const RECT) {
    forward!(this, 48, fn(usize, *const f32, u32, *const RECT), (h, c, n, r));
    record(this, cmd(OP_CLEAR, 0, 0, 48, h as u64, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_clear_uav_uint(this: This, g: u64, c: usize, r: usize, v: *const u32, n: u32, rc: *const RECT) {
    forward!(this, 49, fn(u64, usize, usize, *const u32, u32, *const RECT), (g, c, r, v, n, rc));
    record(this, cmd(OP_CLEAR, 0, 0, 49, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_clear_uav_float(this: This, g: u64, c: usize, r: usize, v: *const f32, n: u32, rc: *const RECT) {
    forward!(this, 50, fn(u64, usize, usize, *const f32, u32, *const RECT), (g, c, r, v, n, rc));
    record(this, cmd(OP_CLEAR, 0, 0, 50, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_discard(this: This, r: usize, region: *const c_void) {
    forward!(this, 51, fn(usize, *const c_void), (r, region));
    record(this, cmd(OP_DISCARD, 0, 0, 0, r as u64, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_begin_query(this: This, h: usize, t: i32, i: u32) {
    forward!(this, 52, fn(usize, i32, u32), (h, t, i));
    record(this, cmd(OP_QUERY, 0, 0, t as u32, h as u64, i.into()), core::ptr::null(), 0);
}
unsafe extern "system" fn h_end_query(this: This, h: usize, t: i32, i: u32) {
    forward!(this, 53, fn(usize, i32, u32), (h, t, i));
    record(this, cmd(OP_QUERY, 1, 0, t as u32, h as u64, i.into()), core::ptr::null(), 0);
}
unsafe extern "system" fn h_resolve_query(this: This, h: usize, t: i32, i: u32, n: u32, r: usize, o: u64) {
    forward!(this, 54, fn(usize, i32, u32, u32, usize, u64), (h, t, i, n, r, o));
    record(this, cmd(OP_RESOLVE, 0, 0, t as u32, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_predication(this: This, r: usize, o: u64, operation: i32) {
    forward!(this, 55, fn(usize, u64, i32), (r, o, operation));
    record(this, cmd(OP_PREDICATION, 0, 0, operation as u32, r as u64, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_set_marker(this: This, m: u32, d: *const c_void, n: u32) {
    forward!(this, 56, fn(u32, *const c_void, u32), (m, d, n));
    record(this, cmd(op::MARKER, 0, 0, 56, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_begin_event(this: This, m: u32, d: *const c_void, n: u32) {
    forward!(this, 57, fn(u32, *const c_void, u32), (m, d, n));
    record(this, cmd(op::MARKER, 0, 0, 57, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_end_event(this: This) {
    forward!(this, 58, fn(), ());
    record(this, cmd(op::MARKER, 0, 0, 58, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_indirect(this: This, sig: usize, max_count: u32, args: usize, arg_offset: u64, count: usize, count_offset: u64) {
    forward!(this, 59, fn(usize, u32, usize, u64, usize, u64), (sig, max_count, args, arg_offset, count, count_offset));
    let extra = [arg_offset, count as u64, count_offset];
    record(this, cmd(op::INDIRECT, 0, 0, max_count, sig as u64, args as u64), extra.as_ptr().cast(), 24);
}
unsafe extern "system" fn h_depth_bounds(this: This, lo: f32, hi: f32) {
    forward!(this, 62, fn(f32, f32), (lo, hi));
    track(this, |s| (s.depth_min, s.depth_max, s.depth_bounds_set) = (lo, hi, true));
    record(this, cmd(op::DEPTH_BOUNDS, 0, 0, 0, u64::from(lo.to_bits()) | u64::from(hi.to_bits()) << 32, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_sample_positions(this: This, a: u32, b: u32, p: *mut c_void) {
    forward!(this, 63, fn(u32, u32, *mut c_void), (a, b, p));
    record(this, cmd(OP_OTHER, 0, 0, 63, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_view_mask(this: This, mask: u32) {
    forward!(this, 65, fn(u32), (mask));
    track(this, |s| (s.view_mask, s.view_mask_set) = (mask, true));
    record(this, cmd(op::VIEW_INSTANCE_MASK, 0, 0, mask, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_immediate(this: This, n: u32, p: *const c_void, m: *const c_void) {
    forward!(this, 66, fn(u32, *const c_void, *const c_void), (n, p, m));
    record(this, cmd(OP_IMMEDIATE, 0, n as u16, 0, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_begin_render_pass(this: This, n: u32, rt: *const c_void, ds: *const c_void, f: i32) {
    forward!(this, 68, fn(u32, *const c_void, *const c_void, i32), (n, rt, ds, f));
    record(this, cmd(OP_TARGETS, 0, n as u16, 68, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_end_render_pass(this: This) {
    forward!(this, 69, fn(), ());
    record(this, cmd(OP_TARGETS, 0, 0, 69, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_pipeline1(this: This, so: usize) {
    forward!(this, 75, fn(usize), (so));
    record(this, cmd(OP_OTHER, 0, 0, 75, 0, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_shading_rate(this: This, rate: i32, c: *const i32) {
    forward!(this, 77, fn(i32, *const i32), (rate, c));
    track(this, |s| {
        s.rate = rate;
        // SAFETY: the caller's two combiners.
        s.combiners = if c.is_null() { [0; 2] } else { unsafe { [*c, *c.add(1)] } };
        s.rate_set = true;
    });
    // SAFETY: as above.
    let packed = if c.is_null() { u64::MAX } else { unsafe { u64::from(*c as u32) | u64::from(*c.add(1) as u32) << 32 } };
    record(this, cmd(op::SHADING_RATE, 0, 0, rate as u32, packed, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_shading_image(this: This, image: usize) {
    forward!(this, 78, fn(usize), (image));
    track(this, |s| (s.rate_image, s.rate_image_set) = (image, true));
    record(this, cmd(op::SHADING_IMAGE, 0, 0, 0, image as u64, 0), core::ptr::null(), 0);
}
unsafe extern "system" fn h_barrier7(this: This, n: u32, g: *const c_void) {
    forward!(this, 80, fn(u32, *const c_void), (n, g));
    record(this, cmd(OP_BARRIER, 7, n as u16, 0, 0, 0), core::ptr::null(), 0);
}
/// ResolveSubresource (19): five pointer-sized arguments, no floats -- forwarded signature-agnostically.
unsafe extern "system" fn h_other19(this: This, a: usize, b: usize, c: usize, d: usize, e: usize) {
    forward!(this, 19, fn(usize, usize, usize, usize, usize), (a, b, c, d, e));
    record(this, cmd(OP_OTHER, 0, 0, 19, 0, 0), core::ptr::null(), 0);
}
const OP_TARGETS: u8 = 28;
const OP_BARRIER: u8 = 29;
const OP_COPY_BUFFER: u8 = 30;
const OP_COPY_TEXTURE: u8 = 31;
const OP_CLEAR: u8 = 32;
const OP_DISCARD: u8 = 33;
const OP_QUERY: u8 = 34;
const OP_RESOLVE: u8 = 35;
const OP_IMMEDIATE: u8 = 36;
const OP_PREDICATION: u8 = 37;
const OP_BUNDLE: u8 = 38;
const OP_RESET: u8 = 39;
const OP_OTHER: u8 = 40;

type HookSlot = (usize, usize);
fn hook_slots(meta: &ChainMeta) -> Vec<HookSlot> {
    let base: [HookSlot; 49] = [
        (10, h_reset as *const () as usize),
        (11, h_clear_state as *const () as usize),
        (12, h_draw as *const () as usize),
        (13, h_draw_indexed as *const () as usize),
        (14, h_dispatch as *const () as usize),
        (15, h_copy_buffer as *const () as usize),
        (16, h_copy_texture as *const () as usize),
        (17, h_copy_resource as *const () as usize),
        (20, h_topology as *const () as usize),
        (21, h_viewports as *const () as usize),
        (22, h_scissors as *const () as usize),
        (23, h_blend as *const () as usize),
        (24, h_stencil as *const () as usize),
        (25, h_pipeline as *const () as usize),
        (26, h_barrier as *const () as usize),
        (27, h_bundle as *const () as usize),
        (28, h_heaps as *const () as usize),
        (29, h_compute_root as *const () as usize),
        (30, h_graphics_root as *const () as usize),
        (31, h_compute_table as *const () as usize),
        (32, h_graphics_table as *const () as usize),
        (33, h_compute_constant as *const () as usize),
        (34, h_graphics_constant as *const () as usize),
        (35, h_compute_constants as *const () as usize),
        (36, h_graphics_constants as *const () as usize),
        (37, h_view37 as *const () as usize),
        (38, h_view38 as *const () as usize),
        (39, h_view39 as *const () as usize),
        (40, h_view40 as *const () as usize),
        (41, h_view41 as *const () as usize),
        (42, h_view42 as *const () as usize),
        (43, h_index_buffer as *const () as usize),
        (44, h_vertex_buffers as *const () as usize),
        (45, h_so_targets as *const () as usize),
        (46, h_targets as *const () as usize),
        (47, h_clear_depth as *const () as usize),
        (48, h_clear_rtv as *const () as usize),
        (49, h_clear_uav_uint as *const () as usize),
        (50, h_clear_uav_float as *const () as usize),
        (51, h_discard as *const () as usize),
        (52, h_begin_query as *const () as usize),
        (53, h_end_query as *const () as usize),
        (54, h_resolve_query as *const () as usize),
        (55, h_predication as *const () as usize),
        (56, h_set_marker as *const () as usize),
        (57, h_begin_event as *const () as usize),
        (58, h_end_event as *const () as usize),
        (59, h_indirect as *const () as usize),
        // ResolveSubresource (19); CopyTiles (18) has six arguments and stays unchained.
        (19, h_other19 as *const () as usize),
    ];
    let mut slots: Vec<HookSlot> = base.to_vec();
    if meta.list1 {
        slots.extend([(62, h_depth_bounds as *const () as usize), (63, h_sample_positions as *const () as usize), (65, h_view_mask as *const () as usize)]);
    }
    if meta.list2 {
        slots.push((66, h_immediate as *const () as usize));
    }
    if meta.list4 {
        slots.extend([(68, h_begin_render_pass as *const () as usize), (69, h_end_render_pass as *const () as usize), (75, h_pipeline1 as *const () as usize)]);
    }
    if meta.list5 {
        slots.extend([(77, h_shading_rate as *const () as usize), (78, h_shading_image as *const () as usize)]);
    }
    if meta.list7 {
        slots.push((80, h_barrier7 as *const () as usize));
    }
    slots.retain(|&(slot, _)| slot <= meta.last);
    slots
}
/// SetInstalled: over whatever occupies each slot now, its predecessor recorded BEFORE the slot points at us;
/// uninstalling puts the predecessor back only where the slot still holds our hook.
fn set_installed(index: usize, meta: &mut ChainMeta, install: bool) {
    if meta.installed == install {
        return;
    }
    let chain = &CHAINS[index];
    let vt = chain.vtable.load(Relaxed) as *mut usize;
    let mut old = 0u32;
    let span = (meta.last - 10 + 1) * 8;
    // SAFETY: the vtable's slots 10..=last; the protection restored below.
    if unsafe { windows_sys::Win32::System::Memory::VirtualProtect(vt.add(10).cast(), span, windows_sys::Win32::System::Memory::PAGE_READWRITE, &mut old) } == 0 {
        return;
    }
    for (slot, hook) in hook_slots(meta) {
        // SAFETY: a slot of the writable vtable.
        let address = unsafe { AtomicUsize::from_ptr(vt.add(slot)) };
        if install {
            let current = address.load(SeqCst);
            if current == hook {
                continue;
            }
            chain.prev[slot].store(current, Release);
            address.swap(hook, SeqCst);
        } else {
            let _ = address.compare_exchange(hook, chain.prev[slot].load(Relaxed), SeqCst, SeqCst);
        }
    }
    let mut ignored = 0u32;
    // SAFETY: as above.
    unsafe { windows_sys::Win32::System::Memory::VirtualProtect(vt.add(10).cast(), span, old, &mut ignored) };
    meta.installed = install;
    add(summary(3), 1);
    log(&format!("[stage-replay] {} command-list vtable {:016X} (last slot {})", if install { "chained" } else { "unchained" }, vt as usize, meta.last));
}
fn install_on(metas: &mut [ChainMeta; 16], entry: &[u64; 6]) {
    let vt = entry[0] as usize;
    let n = CHAIN_COUNT.load(Acquire) as usize;
    if let Some(i) = (0..n).find(|&i| CHAINS[i].vtable.load(Relaxed) == vt) {
        set_installed(i, &mut metas[i], true);
        return;
    }
    if n >= CHAINS.len() {
        return;
    }
    let meta = &mut metas[n];
    (meta.list1, meta.list2, meta.list4, meta.list5, meta.list7) = (entry[1] != 0, entry[2] != 0, entry[3] != 0, entry[4] != 0, entry[5] != 0);
    meta.last = if meta.list7 {
        80
    } else if meta.list5 {
        78
    } else if meta.list4 {
        76
    } else if meta.list2 {
        66
    } else if meta.list1 {
        65
    } else {
        59
    };
    let chain = &CHAINS[n];
    chain.vtable.store(vt, Relaxed);
    for s in 0..=meta.last.min(SLOT_COUNT - 1) {
        // SAFETY: the vtable's slots.
        chain.prev[s].store(unsafe { *(vt as *const usize).add(s) }, Relaxed);
    }
    // Publish every predecessor before the first detour becomes reachable.
    CHAIN_COUNT.store(n as u32 + 1, Release);
    set_installed(n, meta, true);
}
fn install_hooks() {
    let mut metas = CHAIN_META.lock().unwrap_or_else(|p| p.into_inner());
    SHADOWS.get_or_init(|| (0..SHADOW_CAPACITY).map(|_| ShadowSlot { key: AtomicU64::new(0), last_reset: AtomicU32::new(0), shadow: UnsafeCell::new(Shadow::default()) }).collect());
    let mut entries = [[0u64; 6]; 16];
    let n = rt().map_or(0, |r| (r.entries)(entries.as_mut_ptr(), 16)).min(16) as usize;
    for entry in &entries[..n] {
        if entry[0] != 0 {
            install_on(&mut metas, entry);
        }
    }
    // SAFETY: a summary word.
    unsafe { *summary(2) = CHAIN_COUNT.load(Acquire).into() };
}
fn any_installed() -> bool {
    let metas = CHAIN_META.lock().unwrap_or_else(|p| p.into_inner());
    metas[..CHAIN_COUNT.load(Acquire) as usize].iter().any(|m| m.installed)
}
fn uninstall_hooks() {
    let mut metas = CHAIN_META.lock().unwrap_or_else(|p| p.into_inner());
    for i in 0..CHAIN_COUNT.load(Acquire) as usize {
        set_installed(i, &mut metas[i], false);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Census bookkeeping
// ---------------------------------------------------------------------------------------------------------------------
static CENSUS: AtomicBool = AtomicBool::new(false);
static CENSUS_FRAMES: AtomicU32 = AtomicU32::new(0);
static STREAM_TAKEN: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
struct CensusStreams([UnsafeCell<Stream>; 2]);
// SAFETY: a census stream is taken by one consumer call per window (STREAM_TAKEN).
unsafe impl Sync for CensusStreams {}
static CENSUS_STREAMS: OnceLock<CensusStreams> = OnceLock::new();
/// The ordinal of a call among the same (side, stage) calls of the same engine frame: frame << 32 | count per row.
static ORDINALS: [AtomicU64; 128] = [const { AtomicU64::new(0) }; 128];
/// Completion of (side, stage, ordinal) for the current frame, used only to report CPU order.
static DONE: [AtomicU64; 128 * MAX_ORDINALS as usize] = [const { AtomicU64::new(0) }; 128 * MAX_ORDINALS as usize];
/// Calls of (side, stage) in the last completed frame: how many source partitions a target waits for.
static LAST_COUNT: [AtomicU32; 128] = [const { AtomicU32::new(0) }; 128];
/// Per target stage: frame << 2 | state (1 replay in progress, 2 replayed, 3 failed).
static CLAIM: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];
fn next_ordinal(row: u32, frame: u32) -> u32 {
    crate::stage_replay_frame::next_ordinal(&ORDINALS[row as usize], &LAST_COUNT[row as usize], frame)
}
#[allow(clippy::too_many_arguments)]
fn log_event(frame: u32, side: u32, stage: u32, ordinal: u32, kind: u32, start: i64, end: i64, span_begin: u64, span_end: u64) {
    if !CENSUS.load(Relaxed) {
        return;
    }
    let i = x::CyberpunkVR_StageEventCount.fetch_add(1, Relaxed);
    if i >= 16384 {
        return;
    }
    // SAFETY: six words of the events table.
    let e = unsafe { core::slice::from_raw_parts_mut(table!(CyberpunkVR_StageEvents).cast::<u64>().add(i as usize * 6), 6) };
    // SAFETY: a plain call.
    let thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    e.copy_from_slice(&[crate::stage_replay_ring::cvr_srg_event_key(frame, side, stage, ordinal, kind), thread.into(), start as u64, end as u64, span_begin, span_end]);
}
#[derive(Clone, Copy, Default)]
struct CameraInfo {
    vs: u64,
    ps: u64,
    packed: u64,
    ok: bool,
}
fn read_camera(ctx: &Context) -> CameraInfo {
    if !ctx.valid() {
        return CameraInfo::default();
    }
    let (vs, ps) = (binding::resolve(Some(read_fn), ctx.state, 0), binding::resolve(Some(read_fn), ctx.state, 1));
    let (v, p) = (vs.unwrap_or_default(), ps.unwrap_or_default());
    CameraInfo {
        vs: v.descriptor,
        ps: p.descriptor,
        packed: crate::stage_replay_ring::cvr_srg_camera_packed(vs.is_some(), v.parameter, v.index, ps.is_some(), p.parameter, p.index),
        ok: vs.is_some() || ps.is_some(),
    }
}
/// The view descriptors of a native context, read at consumer entry.
fn read_view(ctx: &Context, cam: &CameraInfo) -> [u64; VIEW_DESCRIPTORS] {
    let mut v = [0u64; VIEW_DESCRIPTORS];
    if ctx.valid() {
        // SAFETY: our output.
        unsafe { crate::stage_replay_reads::cvr_srr_view(ctx.state, cam.vs, cam.ps, &mut v) };
    }
    v
}
#[derive(Clone, Copy, Default)]
struct HeapInfo {
    heap: usize,
    cpu: u64,
    gpu: u64,
    count: u32,
}
fn heap_info_of(h: usize) -> HeapInfo {
    let raw = h as *mut c_void;
    // SAFETY: a heap the engine bound.
    let Some(heap) = (unsafe { ID3D12DescriptorHeap::from_raw_borrowed(&raw) }) else { return HeapInfo::default() };
    let desc = unsafe { heap.GetDesc() };
    if desc.Type != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV || desc.Flags.0 & D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE.0 == 0 {
        return HeapInfo::default();
    }
    HeapInfo { heap: h, cpu: unsafe { heap.GetCPUDescriptorHandleForHeapStart() }.ptr as u64, gpu: unsafe { heap.GetGPUDescriptorHandleForHeapStart() }.ptr, count: desc.NumDescriptors }
}
fn shader_heap(shadow: Option<&Shadow>) -> HeapInfo {
    let Some(s) = shadow.filter(|s| s.heaps_set) else { return HeapInfo::default() };
    s.heaps[..(s.heap_count as usize).min(2)].iter().map(|&h| heap_info_of(h)).find(|i| i.heap != 0).unwrap_or_default()
}
/// ReplayHeap: the heap the replayed stream binds -- the shader-visible one every capture binds, agreeing with the
/// mirror when there is one.
fn replay_heap(shadow: &Shadow, caps: &[&CaptureSlot]) -> HeapInfo {
    let mut info = shader_heap(Some(shadow));
    for cap in caps {
        let stream = &cap.data().stream;
        for c in stream.commands().iter().filter(|c| c.op == op::HEAPS) {
            let heaps = stream.payload(c.d, c.count as usize * 8).map(|p| p.cast::<u64>());
            let mut bound = HeapInfo::default();
            if let Some(heaps) = heaps {
                for i in 0..c.count as usize {
                    if bound.heap != 0 {
                        break;
                    }
                    // SAFETY: inside the payload.
                    bound = heap_info_of(unsafe { *heaps.add(i) } as usize);
                }
            }
            if bound.heap == 0 || (info.heap != 0 && info.heap != bound.heap) {
                return HeapInfo::default();
            }
            info = bound;
        }
    }
    info
}
/// RingReserve: descriptors for rebuilt tables from the engine's shader-visible ring (the non-waiting path).
fn ring_reserve(heap: &HeapInfo, count: u32) -> Option<u32> {
    let base = base();
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    if !*COMPATIBLE.get_or_init(|| crate::stage_replay_ring::cvr_srg_ring_code(base)) {
        add(camera_diag(8), 1);
        return None;
    }
    let (mut ring, mut capacity) = (0u64, 0u64);
    // SAFETY: our outputs.
    let refused = unsafe { crate::stage_replay_ring::cvr_srg_ring_find(base, heap.heap as u64, heap.count, count, &mut ring, &mut capacity) };
    if refused != 0 {
        add(camera_diag(refused as usize), 1);
        return None;
    }
    // SAFETY: the engine's ring reservation (non-waiting).
    let reserve: unsafe extern "C" fn(*mut c_void, u64, u64, *mut c_void) -> u64 = unsafe { core::mem::transmute((base + 0x419DF4) as usize) };
    let r = unsafe { reserve(ring as *mut c_void, count.into(), 1, core::ptr::null_mut()) };
    if !crate::stage_replay_ring::cvr_srg_ring_reserved(r, count, capacity) {
        add(camera_diag(10), 1);
        return None;
    }
    add(camera_diag(11), count.into());
    Some(r as u32)
}
fn ensure_device(list: usize) {
    if DEVICE.load(Acquire) != 0 {
        return;
    }
    let _lock = CAPTURE_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
    if DEVICE.load(Acquire) != 0 {
        return;
    }
    let raw = list as *mut c_void;
    // SAFETY: the engine's live list.
    let Some(list) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw) }) else { return };
    let mut device: Option<ID3D12Device> = None;
    if unsafe { list.GetDevice(&mut device) }.is_err() {
        return;
    }
    let Some(device) = device else { return };
    INCREMENT.store(unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV) }, Relaxed);
    let vt = unsafe { *(device.as_raw() as *const *mut *mut c_void) };
    let original = cvr_engine::vtable::cvr_vtable_original(vt, 23) as usize;
    // SAFETY: the device vtable's CopyDescriptors.
    COPY_ORIGINAL.store(if original != 0 { original } else { unsafe { *vt.add(23) as usize } }, Relaxed);
    // The game's device outlives the plugin's use of it; the reference is kept.
    DEVICE.store(device.into_raw() as usize, Release);
}

// ---------------------------------------------------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------------------------------------------------
fn note_fallback(row: *mut u64, why: u32) {
    add(stats(7), 1);
    add(stats(16 + why.min(15) as usize), 1);
    row_add(row, col::FALLBACKS, 1);
    if !row.is_null() {
        // SAFETY: a census row.
        store(unsafe { row.add(col::LAST_FALLBACK) }, why.into());
    }
}
/// A command list method through the vtable (its hooks included, as the .cpp's calls).
macro_rules! vcall {
    ($list:expr, $slot:expr, fn($($t:ty),*), ($($a:expr),*)) => {{
        let list: usize = $list;
        // SAFETY: the live list's vtable method of this signature.
        let f: unsafe extern "system" fn(usize $(, $t)*) = unsafe { core::mem::transmute(*(*(list as *const *const usize)).add($slot)) };
        unsafe { f(list $(, $a)*) }
    }};
}
fn cast_list<T: Interface>(list: usize) -> Option<T> {
    let raw = list as *mut c_void;
    // SAFETY: the engine's live list.
    unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw) }.and_then(|l| l.cast::<T>().ok())
}
fn restore(list: usize, s: &Shadow, t: &Touched) {
    let mut ops = 0u64;
    if t.heaps && s.heaps_set {
        vcall!(list, 28, fn(u32, *const usize), (s.heap_count, s.heaps.as_ptr()));
        ops += 1;
    }
    if (t.root || t.params || t.heaps) && s.root_set {
        vcall!(list, 30, fn(usize), (s.root));
        ops += 1;
        for (i, p) in s.params.iter().enumerate() {
            let i = i as u32;
            match p.kind {
                param_kind::TABLE => vcall!(list, 32, fn(u32, u64), (i, p.value)),
                param_kind::CBV => vcall!(list, 38, fn(u32, u64), (i, p.value)),
                param_kind::SRV => vcall!(list, 40, fn(u32, u64), (i, p.value)),
                param_kind::UAV => vcall!(list, 42, fn(u32, u64), (i, p.value)),
                param_kind::CONSTANTS => {
                    for w in 0..16u32 {
                        if p.word_mask & (1 << w) != 0 {
                            vcall!(list, 34, fn(u32, u32, u32), (i, p.words[w as usize], w));
                            ops += 1;
                        }
                    }
                    continue;
                }
                _ => continue,
            }
            ops += 1;
        }
    }
    if t.pso && s.pso_set {
        vcall!(list, 25, fn(usize), (s.pso));
        ops += 1;
    }
    if t.topology && s.topology_set {
        vcall!(list, 20, fn(i32), (s.topology));
        ops += 1;
    }
    if t.index && s.index_set {
        vcall!(list, 43, fn(*const D3D12_INDEX_BUFFER_VIEW), (if s.index_null { core::ptr::null() } else { &s.index }));
        ops += 1;
    }
    if t.vertices {
        for slot in 0..16u32 {
            if s.vertex_mask & (1 << slot) != 0 {
                vcall!(list, 44, fn(u32, u32, *const D3D12_VERTEX_BUFFER_VIEW), (slot, 1, &s.vertices[slot as usize]));
                ops += 1;
            }
        }
    }
    if t.viewports && s.viewports_set {
        vcall!(list, 21, fn(u32, *const D3D12_VIEWPORT), (s.viewport_count, s.viewports.as_ptr()));
        ops += 1;
    }
    if t.scissors && s.scissors_set {
        vcall!(list, 22, fn(u32, *const RECT), (s.scissor_count, s.scissors.as_ptr()));
        ops += 1;
    }
    if t.blend && s.blend_set {
        vcall!(list, 23, fn(*const f32), (s.blend.as_ptr()));
        ops += 1;
    }
    if t.stencil && s.stencil_set {
        vcall!(list, 24, fn(u32), (s.stencil));
        ops += 1;
    }
    if t.depth_bounds && s.depth_bounds_set {
        if let Some(l1) = cast_list::<ID3D12GraphicsCommandList1>(list) {
            vcall!(l1.as_raw() as usize, 62, fn(f32, f32), (s.depth_min, s.depth_max));
            ops += 1;
        }
    }
    if (t.rate && s.rate_set) || (t.rate_image && s.rate_image_set) {
        if let Some(l5) = cast_list::<ID3D12GraphicsCommandList5>(list) {
            if t.rate && s.rate_set {
                vcall!(l5.as_raw() as usize, 77, fn(i32, *const i32), (s.rate, s.combiners.as_ptr()));
                ops += 1;
            }
            if t.rate_image && s.rate_image_set {
                vcall!(l5.as_raw() as usize, 78, fn(usize), (s.rate_image));
                ops += 1;
            }
        }
    }
    if t.view_mask && s.view_mask_set {
        if let Some(l1) = cast_list::<ID3D12GraphicsCommandList1>(list) {
            vcall!(l1.as_raw() as usize, 65, fn(u32), (s.view_mask));
            ops += 1;
        }
    }
    add(stats(10), ops);
}
/// Blocking ops other than the barriers AllowBarriers lets through.
fn replayable(s: &Stream, allow_barriers: bool) -> bool {
    if s.blocking == 0 {
        return true;
    }
    allow_barriers && !s.commands().iter().any(|c| rules::blocking(c.op) && c.op != OP_BARRIER)
}
/// IssueStream: `camera_tables` holds (command index, target GPU table) for every camera-table bind, ascending.
fn issue_stream(list: usize, s: &Stream, camera_tables: &[(usize, u64)], issued: &mut u32) {
    let mut next_table = 0;
    for (index, c) in s.commands().iter().enumerate() {
        let payload = |bytes: usize| s.payload(c.d, bytes);
        match c.op {
            op::PIPELINE => vcall!(list, 25, fn(u64), (c.b)),
            op::ROOT_SIGNATURE => vcall!(list, 30, fn(u64), (c.b)),
            op::ROOT_TABLE => {
                let mut gpu = c.b;
                if next_table < camera_tables.len() && camera_tables[next_table].0 == index {
                    gpu = camera_tables[next_table].1; // the target's camera
                    next_table += 1;
                }
                vcall!(list, 32, fn(u32, u64), (c.a, gpu));
            }
            op::ROOT_CONSTANT => vcall!(list, 34, fn(u32, u32, u32), (c.a, c.b as u32, c.c as u32)),
            op::ROOT_CONSTANTS => {
                if let Some(p) = payload(c.count as usize * 4) {
                    vcall!(list, 36, fn(u32, u32, *const u8, u32), (c.a, c.count.into(), p, c.c as u32));
                }
            }
            op::ROOT_CBV => vcall!(list, 38, fn(u32, u64), (c.a, c.b)),
            op::ROOT_SRV => vcall!(list, 40, fn(u32, u64), (c.a, c.b)),
            op::ROOT_UAV => vcall!(list, 42, fn(u32, u64), (c.a, c.b)),
            op::TOPOLOGY => vcall!(list, 20, fn(u32), (c.a)),
            op::INDEX_BUFFER => {
                let view = if c.flags & 4 != 0 { core::ptr::null() } else { payload(core::mem::size_of::<D3D12_INDEX_BUFFER_VIEW>()).unwrap_or(core::ptr::null()) };
                vcall!(list, 43, fn(*const u8), (view));
            }
            op::VERTEX_BUFFERS => {
                let views = if c.flags & 4 != 0 { core::ptr::null() } else { payload(c.count as usize * core::mem::size_of::<D3D12_VERTEX_BUFFER_VIEW>()).unwrap_or(core::ptr::null()) };
                vcall!(list, 44, fn(u32, u32, *const u8), (c.a, c.count.into(), views));
            }
            op::VIEWPORTS => {
                if let Some(p) = payload(c.count as usize * core::mem::size_of::<D3D12_VIEWPORT>()) {
                    vcall!(list, 21, fn(u32, *const u8), (c.count.into(), p));
                }
            }
            op::SCISSORS => {
                if let Some(p) = payload(c.count as usize * core::mem::size_of::<RECT>()) {
                    vcall!(list, 22, fn(u32, *const u8), (c.count.into(), p));
                }
            }
            op::BLEND_FACTOR => {
                let factors = if c.flags & 4 != 0 { core::ptr::null() } else { payload(16).unwrap_or(core::ptr::null()) };
                vcall!(list, 23, fn(*const u8), (factors));
            }
            op::STENCIL_REF => vcall!(list, 24, fn(u32), (c.a)),
            op::DEPTH_BOUNDS => {
                if let Some(l1) = cast_list::<ID3D12GraphicsCommandList1>(list) {
                    vcall!(l1.as_raw() as usize, 62, fn(f32, f32), (f32::from_bits(c.b as u32), f32::from_bits((c.b >> 32) as u32)));
                }
            }
            op::HEAPS => {
                if let Some(p) = payload(c.count as usize * 8) {
                    vcall!(list, 28, fn(u32, *const u8), (c.count.into(), p));
                }
            }
            op::SHADING_RATE => {
                if let Some(l5) = cast_list::<ID3D12GraphicsCommandList5>(list) {
                    if c.b == u64::MAX {
                        vcall!(l5.as_raw() as usize, 77, fn(u32, *const i32), (c.a, core::ptr::null()));
                    } else {
                        let combiners = [c.b as u32 as i32, (c.b >> 32) as u32 as i32];
                        vcall!(l5.as_raw() as usize, 77, fn(u32, *const i32), (c.a, combiners.as_ptr()));
                    }
                }
            }
            op::SHADING_IMAGE => {} // per eye: the target keeps its own image
            op::VIEW_INSTANCE_MASK => {
                if let Some(l1) = cast_list::<ID3D12GraphicsCommandList1>(list) {
                    vcall!(l1.as_raw() as usize, 65, fn(u32), (c.a));
                }
            }
            op::DRAW => {
                vcall!(list, 12, fn(u32, u32, u32, u32), (c.a, c.b as u32, (c.b >> 32) as u32, c.c as u32));
                *issued += 1;
            }
            op::DRAW_INDEXED => {
                vcall!(list, 13, fn(u32, u32, u32, i32, u32), (c.a, c.b as u32, (c.b >> 32) as u32, c.c as u32 as i32, (c.c >> 32) as u32));
                *issued += 1;
            }
            op::INDIRECT => {
                if let Some(extra) = payload(24).map(|p| p.cast::<u64>()) {
                    // SAFETY: the payload's three words.
                    let (arg_offset, count, count_offset) = unsafe { (*extra, *extra.add(1), *extra.add(2)) };
                    vcall!(list, 59, fn(u64, u32, u64, u64, u64, u64), (c.b, c.a, c.c, arg_offset, count, count_offset));
                    *issued += 1;
                }
            }
            _ => {} // markers, copies, compute state and (when allowed) barriers are not replayed
        }
    }
}
/// Replay: the source partitions re-issued, in source slice order, into the target's list, then the target's mirrored
/// state restored; with `camera`, every camera-table bind gets a table rebuilt with the target's view descriptors.
#[allow(clippy::too_many_arguments)]
fn replay(list: usize, caps: &[&CaptureSlot], shadow: &Shadow, camera: bool, cam_vs: u64, cam_ps: u64, target: &[u64; VIEW_DESCRIPTORS], row: *mut u64) -> bool {
    let mut camera_tables: Vec<Vec<(usize, u64)>> = vec![Vec::new(); caps.len()];
    if camera {
        if cam_vs == 0 || cam_ps == 0 {
            add(camera_diag(0), 1);
            note_fallback(row, fallback::CAMERA);
            return false;
        }
        let heap = replay_heap(shadow, caps);
        let (device, copy, increment) = (DEVICE.load(Acquire), COPY_ORIGINAL.load(Relaxed), INCREMENT.load(Relaxed));
        if heap.heap == 0 || copy == 0 || device == 0 || increment == 0 {
            add(camera_diag(1), 1);
            note_fallback(row, fallback::NO_HEAP);
            return false;
        }
        // Unique substituted lists, laid out back to back in one ring reservation.
        struct Unique {
            hash: u64,
            offset: u32,
            count: u32,
        }
        let (mut unique, mut flat, mut uses): (Vec<Unique>, Vec<u64>, Vec<(usize, usize, usize)>) = (Vec::new(), Vec::new(), Vec::new());
        let mut sources = [0u64; 256];
        for (k, cap) in caps.iter().enumerate() {
            let data = cap.data();
            for (i, c) in data.stream.commands().iter().enumerate() {
                if c.op != op::ROOT_TABLE || c.flags & 8 == 0 {
                    continue;
                }
                // SAFETY: inside the payload.
                let n = data.stream.payload(c.d, 8).map_or(0, |h| unsafe { *h.cast::<u64>() } as u32);
                let payload = if n != 0 && n <= 256 { data.stream.payload(c.d, (n as usize + 1) * 8) } else { None };
                let Some(payload) = payload else {
                    note_fallback(row, fallback::CAMERA);
                    return false;
                };
                // SAFETY: the payload's n descriptors after its count.
                sources[..n as usize].copy_from_slice(unsafe { core::slice::from_raw_parts(payload.cast::<u64>().add(1), n as usize) });
                if rules::substitute_view(&mut sources[..n as usize], &data.view, target) <= 0 {
                    add(camera_diag(2), 1);
                    note_fallback(row, fallback::CAMERA);
                    return false;
                }
                let mut h = 0xCBF2_9CE4_8422_2325u64;
                for &s in &sources[..n as usize] {
                    h = (h ^ s).wrapping_mul(0x100_0000_01B3);
                }
                h = (h ^ u64::from(n)).wrapping_mul(0x100_0000_01B3);
                let found = unique.iter().position(|u| u.hash == h && u.count == n && flat[u.offset as usize..(u.offset + n) as usize] == sources[..n as usize]);
                let found = match found {
                    Some(u) => {
                        add(camera_diag(4), 1);
                        u
                    }
                    None => {
                        unique.push(Unique { hash: h, offset: flat.len() as u32, count: n });
                        flat.extend_from_slice(&sources[..n as usize]);
                        add(camera_diag(3), 1);
                        unique.len() - 1
                    }
                };
                uses.push((k, i, found));
                store(camera_diag(5), cam_vs);
                store(camera_diag(6), data.cam_vs);
                store(camera_diag(7), n.into());
            }
        }
        if !flat.is_empty() {
            if flat.len() > 8192 {
                note_fallback(row, fallback::CAPACITY);
                return false;
            }
            let Some(first) = ring_reserve(&heap, flat.len() as u32) else {
                note_fallback(row, fallback::CAPACITY);
                return false;
            };
            let src: Vec<usize> = flat.iter().map(|&d| d as usize).collect();
            let ones = vec![1u32; flat.len()];
            let dst = (heap.cpu + u64::from(first) * u64::from(increment)) as usize;
            let dst_count = flat.len() as u32;
            // SAFETY: the device's original CopyDescriptors over our ring reservation.
            let copy: CopyDescriptors = unsafe { core::mem::transmute(copy) };
            unsafe { copy(device, 1, &dst, &dst_count, flat.len() as u32, src.as_ptr(), ones.as_ptr(), D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV.0) };
            for &(cap, command, u) in &uses {
                camera_tables[cap].push((command, heap.gpu + u64::from(first + unique[u].offset) * u64::from(increment)));
            }
            add(stats(8), unique.len() as u64);
            add(stats(9), flat.len() as u64);
        }
    }
    // The state to restore TO, taken before the replayed commands move the mirror.
    let entry = Box::new(shadow.clone());
    T_REPLAYING.with(|r| r.set(true));
    let (mut issued, mut touched) = (0u32, Touched::default());
    for (k, cap) in caps.iter().enumerate() {
        let stream = &cap.data().stream;
        issue_stream(list, stream, &camera_tables[k], &mut issued);
        let t = rules::touched(stream.commands());
        touched.pso |= t.pso;
        touched.root |= t.root;
        touched.params |= t.params;
        touched.topology |= t.topology;
        touched.index |= t.index;
        touched.vertices |= t.vertices;
        touched.viewports |= t.viewports;
        touched.scissors |= t.scissors;
        touched.blend |= t.blend;
        touched.stencil |= t.stencil;
        touched.depth_bounds |= t.depth_bounds;
        touched.heaps |= t.heaps;
        touched.rate |= t.rate;
        touched.rate_image |= t.rate_image;
        touched.view_mask |= t.view_mask;
    }
    restore(list, &entry, &touched);
    T_REPLAYING.with(|r| r.set(false));
    add(stats(5), issued.into());
    true
}

// ---------------------------------------------------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------------------------------------------------
pub fn active() -> bool {
    ACTIVE.load(Relaxed)
}
pub fn graph_salt() -> u64 {
    if x::CyberpunkVR_VrcamGbufferPartitionsState.load(Relaxed) == 1 {
        0x9E37_79B9_7F4A_7C15
    } else {
        0
    }
}
/// Copy (the device CopyDescriptors hooks): only CBV/SRV/UAV copies into the scope's shader-visible heap, kept by
/// census streams (a capture takes camera-table contents from the context's CPU table at each bind instead).
///
/// # Safety
/// The device call's arguments.
#[allow(clippy::too_many_arguments)]
pub unsafe fn copy(n: u32, dest: *const u64, sizes: *const u32, m: u32, src: *const u64, counts: *const u32, kind: i32) {
    let s = T_SCOPE.with(Cell::get);
    if s.is_null() || T_REPLAYING.with(Cell::get) {
        return;
    }
    // SAFETY: the thread's live scope.
    let s = unsafe { &mut *s };
    if s.stream.is_null() || kind != D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV.0 || s.stride == 0 || dest.is_null() || src.is_null() {
        return;
    }
    row_add(s.row, col::COPIES, 1);
    if !s.capture.is_null() {
        return;
    }
    let mut flat = [0u64; 512];
    let mut rows = vec![crate::stage_replay_copies::CopyRow::default(); n as usize];
    let heap = crate::stage_replay_copies::Heap { cpu: s.heap_cpu, count: s.heap_count, stride: s.stride, camera_vs: s.cam_vs, camera_ps: s.cam_ps };
    // SAFETY: the device call's arrays and our outputs.
    let kept = unsafe { crate::stage_replay_copies::cvr_srx_copy_plan(&heap, n, dest, sizes, m, src, counts, &mut flat, rows.as_mut_ptr()) };
    for row in &rows[..kept as usize] {
        let c = cmd(op::COPY, row.flags as u8, row.size as u16, row.index, 0, 0);
        row_add(s.row, col::COPIED_DESCRIPTORS, row.size.into());
        record(core::ptr::null_mut(), c, flat[row.offset as usize..].as_ptr().cast(), row.size as usize * 8);
    }
}

struct Ticket {
    scope: Scope,
    previous: *mut Scope,
    capture: Option<&'static CaptureSlot>,
    row: *mut u64,
    start: i64,
    span_begin: u64,
    span_end: u64,
    row_index: u32,
    ordinal: u32,
    frame: u32,
    stage: u32,
    kind: u32,
    active: bool,
    census_stream: bool,
    side: i32,
    context: u64,
    state: u64,
}
impl Default for Ticket {
    fn default() -> Self {
        Ticket {
            scope: Scope::default(),
            previous: core::ptr::null_mut(),
            capture: None,
            row: core::ptr::null_mut(),
            start: 0,
            span_begin: 0,
            span_end: 0,
            row_index: 0,
            ordinal: 0,
            frame: 0,
            stage: 0,
            kind: 0,
            active: false,
            census_stream: false,
            side: -1,
            context: 0,
            state: 0,
        }
    }
}
/// AcquireSources: every source partition of (stage, frame) acquired for reading, waiting up to `wait_us`; on failure
/// nothing stays acquired and the reason is returned.
fn acquire_sources(stage: u32, frame: u32, first: u32, expected: u32, wait_us: u32, allow_barriers: bool, out: &mut Vec<&'static CaptureSlot>) -> u32 {
    out.clear();
    if expected == 0 || first >= MAX_ORDINALS || expected > MAX_ORDINALS - first {
        return fallback::NO_CAPTURE;
    }
    let deadline = now() + i64::from(wait_us) * 10; // QPC is 10 MHz on supported Windows
    loop {
        let mut pending = false;
        let mut why = fallback::NONE;
        for o in first..first + expected {
            let Some(cap) = slot(stage, o, frame) else {
                why = fallback::ORDINAL;
                break;
            };
            if let Err(state) = cap.state.compare_exchange(cap_state::COMPLETE, cap_state::READING, AcqRel, Acquire) {
                pending = true;
                why = if state == cap_state::WRITING { fallback::BUSY } else { fallback::NO_CAPTURE };
                break;
            }
            out.push(cap);
            let data = cap.data();
            if !rules::same_frame(data.frame, frame, true) {
                pending = true;
                why = fallback::STALE_FRAME;
                break;
            }
            if data.stream.overflowed() {
                why = fallback::CAPTURE_OVERFLOW;
                break;
            }
            if !replayable(&data.stream, allow_barriers) {
                why = fallback::CAPTURE_BLOCKING;
                break;
            }
            if data.foreign {
                why = fallback::CAPTURE_LIST;
                break;
            }
        }
        if why == fallback::NONE {
            out.sort_by_key(|c| c.data().span_begin);
            return why;
        }
        for cap in out.iter() {
            cap.state.store(cap_state::COMPLETE, Release);
        }
        out.clear();
        if !pending || wait_us == 0 || now() >= deadline {
            return why;
        }
        core::hint::spin_loop();
    }
}
/// SourcePartitioned: several source calls of a stage are partitions of ONE pass (their spans tile one packet array)
/// or separate passes (paired by ordinal).
fn source_partitioned(stage: u32, frame: u32, expected: u32) -> bool {
    if expected <= 1 {
        return true;
    }
    if expected > MAX_ORDINALS {
        return false;
    }
    let mut spans = [[0u64; 2]; MAX_ORDINALS as usize];
    for o in 0..expected {
        let Some(cap) = slot(stage, o, frame) else { return true };
        let data = cap.data();
        if cap.state.load(Acquire) != cap_state::COMPLETE || !rules::same_frame(data.frame, frame, true) {
            return true;
        }
        spans[o as usize] = [data.span_begin, data.span_end];
    }
    // SAFETY: our spans.
    unsafe { crate::stage_replay_copies::cvr_srx_tiled(spans.as_mut_ptr(), expected as usize) }
}
fn census_rows(index: u32) -> *mut u64 {
    // SAFETY: a row of the census table.
    unsafe { table!(CyberpunkVR_StageCensusRows).cast::<[u64; 32]>().add(index as usize).cast() }
}
fn census_ops(index: u32) -> *mut u32 {
    // SAFETY: a row of the census ops table.
    unsafe { table!(CyberpunkVR_StageCensusOps).cast::<[u32; 48]>().add(index as usize).cast() }
}
fn view_diag(side: usize, view: &[u64; VIEW_DESCRIPTORS], stage: u32) {
    // SAFETY: the export's row.
    let row = unsafe { &mut (*table!(CyberpunkVR_StageReplayViewDiag))[side] };
    row[..VIEW_DESCRIPTORS].copy_from_slice(view);
    row[7] = stage.into();
}
/// Enter: true when the native consumer must NOT run (skipped, or replayed).
fn enter(t: &mut Ticket, side: i32, node: u32, arguments: u64, span: u64) -> bool {
    *t = Ticket { side: -1, ..Ticket::default() };
    if !ACTIVE.load(Relaxed) {
        return false;
    }
    let mut stage = 0xFFu32;
    let mut args = [0u8; 32];
    if arguments != 0 && read_safe(arguments, &mut args) {
        stage = u64::from_le_bytes(args[16..24].try_into().unwrap()) as u32;
    }
    if node != 0x33DDA4 || !(0..=1).contains(&side) {
        return false;
    }
    let stage64 = stage.min(63);
    (t.row_index, t.side, t.stage) = (side as u32 * 64 + stage64, side, stage);
    let census = CENSUS.load(Relaxed);
    t.row = if census { census_rows(t.row_index) } else { core::ptr::null_mut() };
    t.frame = engine_frame();
    t.ordinal = next_ordinal(t.row_index, t.frame);
    let mut span_words = [0u8; 16];
    if span != 0 && read_safe(span, &mut span_words) {
        t.span_begin = u64::from_le_bytes(span_words[..8].try_into().unwrap());
        t.span_end = u64::from_le_bytes(span_words[8..].try_into().unwrap());
    }
    let settings = Settings { mode: x::CyberpunkVR_StageReplayMode.load(Relaxed), stages: x::CyberpunkVR_StageReplayStages.load(Relaxed), source: x::CyberpunkVR_StageReplaySource.load(Relaxed) };
    let action = rules::decide(&settings, side, node, stage);
    // SAFETY: a summary word.
    let mirrors_back = unsafe { *summary(0) } >= 3; // list mirrors re-established after the hooks went in
    let want_stream = census && stage == x::CyberpunkVR_StageStreamStage.load(Relaxed) && t.ordinal == 0 && mirrors_back;
    if !census && action == rules::action::FORWARD {
        return false;
    }
    let ctx = crate::camera_upload::current(base());
    let list = if ctx.valid() { ctx.list as usize } else { 0 };
    if list != 0 {
        ensure_device(list);
    }
    let shadow = if list != 0 { shadow_for(list as *mut c_void, false) } else { None };
    let shadow_valid = shadow.as_ref().is_some_and(|s| s.valid && s.generation == GENERATION.load(Relaxed));
    let cam = read_camera(&ctx);
    if !t.row.is_null() {
        let row = t.row;
        // SAFETY: the census row's columns.
        let at = |c: usize| unsafe { row.add(c) };
        add(at(col::CALLS), 1);
        store(at(col::LAST_ENGINE_FRAME), t.frame.into());
        max(at(col::MAX_ORDINAL), u64::from(t.ordinal) + 1);
        let peer = (1 - side as u32) * 64 + stage64;
        if DONE[(peer * MAX_ORDINALS + t.ordinal.min(MAX_ORDINALS - 1)) as usize].load(Acquire) == (u64::from(t.frame) << 1 | 1) {
            add(at(col::PEER_DONE), 1);
        } else {
            add(at(col::PEER_PENDING), 1);
        }
        if cam.ok {
            store(at(col::CAM_VS), cam.vs);
            store(at(col::CAM_PS), cam.ps);
            store(at(col::CAMERA_SLOTS), cam.packed);
        }
        if ctx.valid() {
            add(at(col::CONTEXT_OK), 1);
        }
        for i in 0..4 {
            store(at(col::ARGS0 + i), u64::from_le_bytes(args[i * 8..i * 8 + 8].try_into().unwrap()));
        }
        let params = if ctx.valid() {
            let mut groups = Box::new(unsafe { core::mem::zeroed::<crate::stage_replay_camera::Groups>() });
            // SAFETY: our output.
            unsafe { crate::stage_replay_camera::cvr_srr_camera_groups(ctx.state, &mut *groups) };
            if groups.complete && groups.params != 0 { groups.params } else { 0 }
        } else {
            0
        };
        store(at(col::CAMERA_PARAMS), params);
        store(at(col::ENTRY_LIST), ctx.list);
        if shadow_valid {
            add(at(col::SHADOW_VALID), 1);
        }
        if t.span_end >= t.span_begin {
            add(at(col::PACKETS), (t.span_end - t.span_begin) / 16);
        }
    }
    let heap = shader_heap(shadow.as_deref());
    let fill = |sc: &mut Scope, row: *mut u64, row_index: u32| {
        sc.row = row;
        sc.ops = if census { census_ops(row_index) } else { core::ptr::null_mut() };
        sc.list = ctx.list;
        (sc.heap_cpu, sc.heap_gpu, sc.heap_count) = (heap.cpu, heap.gpu, heap.count);
        sc.stride = INCREMENT.load(Relaxed);
        (sc.cam_vs, sc.cam_ps) = (cam.vs, cam.ps);
        sc.state = if ctx.valid() { ctx.state } else { 0 };
    };
    let begin = |t: &mut Ticket, kind: u32| {
        t.kind = kind;
        t.previous = T_SCOPE.with(|s| s.replace(&mut t.scope));
        t.active = true;
        t.start = now();
    };
    if action == rules::action::SKIP {
        let now = now();
        add(stats(3), 1);
        row_add(t.row, col::SKIPPED, 1);
        log_event(t.frame, side as u32, stage, t.ordinal, kind::SKIP, now, now, t.span_begin, t.span_end);
        return true;
    }
    if action == rules::action::REPLAY {
        let start = now();
        let source_count = LAST_COUNT[(settings.source * 64 + stage64) as usize].load(Relaxed);
        let paired = !source_partitioned(stage, t.frame, source_count);
        if paired {
            add(stats(11), 1);
        }
        // Partitions: one target call per frame replays the whole source stage; the others follow.
        if stage < 64 && !paired {
            let claim = &CLAIM[stage as usize];
            let mut current = claim.load(Acquire);
            let mine = u64::from(t.frame) << 2 | 1;
            let mut claimed = false;
            while !claimed && (current >> 2) as u32 != t.frame {
                match claim.compare_exchange_weak(current, mine, AcqRel, Acquire) {
                    Ok(_) => claimed = true,
                    Err(seen) => current = seen,
                }
            }
            if !claimed {
                let deadline = start + 50_000; // 5 ms at 10 MHz
                while (current >> 2) as u32 == t.frame && current & 3 == 1 && now() < deadline {
                    core::hint::spin_loop();
                    current = claim.load(Acquire);
                }
                if (current >> 2) as u32 == t.frame && current & 3 == 2 {
                    add(stats(3), 1);
                    row_add(t.row, col::SKIPPED, 1);
                    log_event(t.frame, side as u32, stage, t.ordinal, kind::SKIP, start, now(), t.span_begin, t.span_end);
                    return true;
                }
                // The leading partition failed (its reason is recorded) or did not finish in time.
                note_fallback(t.row, if (current >> 2) as u32 == t.frame && current & 3 == 3 { fallback::LEADER } else { fallback::BUSY });
                let (row, row_index) = (t.row, t.row_index);
                fill(&mut t.scope, row, row_index);
                begin(t, kind::FALLBACK);
                return false;
            }
        }
        let mut replayed = false;
        let why = if list == 0 {
            fallback::NO_CONTEXT
        } else if !shadow_valid {
            fallback::SHADOW_INVALID
        } else if shadow.as_ref().is_some_and(|s| s.overflow) {
            fallback::SHADOW_OVERFLOW
        } else {
            let mut caps = Vec::new();
            let target_count = LAST_COUNT[t.row_index as usize].load(Relaxed);
            let why = if paired && (t.ordinal >= source_count || target_count != source_count) {
                fallback::ORDINAL
            } else {
                acquire_sources(
                    stage,
                    t.frame,
                    if paired { t.ordinal } else { 0 },
                    if paired { 1 } else { source_count },
                    x::CyberpunkVR_StageReplayWaitUs.load(Relaxed),
                    x::CyberpunkVR_StageReplayAllowBarriers.load(Relaxed) != 0,
                    &mut caps,
                )
            };
            if why == fallback::NONE {
                let camera = settings.mode == 4;
                let mut target = [0u64; VIEW_DESCRIPTORS];
                if camera {
                    target = read_view(&ctx, &cam);
                    view_diag(1, &target, stage);
                }
                let s = shadow.as_deref().expect("a valid shadow");
                replayed = replay(list, &caps, s, camera, cam.vs, cam.ps, &target, t.row);
                for cap in &caps {
                    cap.state.store(cap_state::COMPLETE, Release);
                }
            }
            why
        };
        if stage < 64 && !paired {
            CLAIM[stage as usize].store(u64::from(t.frame) << 2 | if replayed { 2 } else { 3 }, Release);
        }
        if !replayed {
            if why != fallback::NONE {
                note_fallback(t.row, why);
            }
            let (row, row_index) = (t.row, t.row_index);
            fill(&mut t.scope, row, row_index); // native consumer, still counted by a running census
            begin(t, kind::FALLBACK);
            return false;
        }
        let end = now();
        add(stats(4), 1);
        add(stats(6), (end - start) as u64);
        row_add(t.row, col::REPLAYED, 1);
        row_add(t.row, col::REPLAY_TICKS, (end - start) as u64);
        log_event(t.frame, side as u32, stage, t.ordinal, kind::REPLAY, start, end, t.span_begin, t.span_end);
        return true;
    }
    // Forward with a scope: census counting, census stream, or source capture.
    let (row, row_index) = (t.row, t.row_index);
    fill(&mut t.scope, row, row_index);
    if action == rules::action::CAPTURE && settings.mode >= 3 {
        let cap = if t.ordinal < MAX_ORDINALS { slot(stage, t.ordinal, t.frame) } else { None };
        if let Some(cap) = cap {
            let st = cap.state.load(Acquire);
            if (st == cap_state::EMPTY || st == cap_state::COMPLETE) && cap.state.compare_exchange(st, cap_state::WRITING, AcqRel, Acquire).is_ok() {
                let data = cap.data();
                data.stream.clear();
                (data.frame, data.stage, data.ordinal) = (t.frame, stage, t.ordinal);
                (data.list, data.cam_vs, data.cam_ps, data.foreign) = (ctx.list, cam.vs, cam.ps, false);
                data.view = read_view(&ctx, &cam);
                t.scope.view = data.view;
                view_diag(0, &data.view, stage);
                (data.span_begin, data.span_end) = (t.span_begin, t.span_end);
                t.capture = Some(cap);
                t.scope.stream = &mut data.stream;
                t.scope.capture = cap;
                t.scope.preserve_camera = settings.mode == 4;
                add(stats(0), 1);
            } else {
                add(stats(2), 1);
            }
        }
    } else if want_stream && STREAM_TAKEN[side as usize].swap(1, SeqCst) == 0 {
        let streams = CENSUS_STREAMS.get_or_init(|| {
            let make = || {
                let mut s = Stream::default();
                s.reserve(32768, 2 << 20);
                UnsafeCell::new(s)
            };
            CensusStreams([make(), make()])
        });
        // SAFETY: taken by this call for the window.
        let stream = unsafe { &mut *streams.0[side as usize].get() };
        stream.clear();
        t.scope.stream = stream;
        t.census_stream = true;
        (t.context, t.state) = (ctx.context, ctx.state);
        // SAFETY: the export's row.
        let info = unsafe { &mut (*table!(CyberpunkVR_StageStreamInfo))[side as usize] };
        (info[5], info[6], info[7], info[8], info[9]) = (ctx.list, t.frame.into(), stage.into(), cam.vs, cam.ps);
        (info[10], info[11], info[12]) = (heap.cpu, heap.gpu, INCREMENT.load(Relaxed).into());
        let mut slice = crate::stage_replay_reads::Slice::default();
        if ctx.valid() {
            // SAFETY: our output.
            unsafe { crate::stage_replay_reads::cvr_srr_slice(ctx.state, &mut slice) };
        }
        (info[13], info[14], info[15]) = (slice.heap, u64::from(slice.base) | u64::from(slice.capacity) << 32, slice.cursor.into());
    }
    let k = if t.capture.is_some() { kind::CAPTURE } else { kind::FORWARD };
    begin(t, k);
    false
}
fn leave(t: &mut Ticket) {
    if !t.active {
        return;
    }
    T_SCOPE.with(|s| s.set(t.previous));
    let end = now();
    row_add(t.row, col::TICKS, (end - t.start) as u64);
    if let Some(cap) = t.capture {
        if cap.data().stream.overflowed() {
            add(stats(1), 1);
        }
        cap.state.store(cap_state::COMPLETE, Release);
    }
    if t.census_stream && t.side >= 0 {
        let side = t.side as usize;
        // SAFETY: taken by this call for the window.
        let s = unsafe { &*CENSUS_STREAMS.get().expect("taken above").0[side].get() };
        // SAFETY: the exports' rows.
        let info = unsafe { &mut (*table!(CyberpunkVR_StageStreamInfo))[side] };
        (info[0], info[1], info[2], info[3], info[4]) = (s.commands.as_ptr() as u64, s.commands.len() as u64, s.payload.as_ptr() as u64, s.payload.len() as u64, s.overflow.into());
        unsafe { (*table!(CyberpunkVR_StageStreamContextAddr))[side] = [t.context, t.state] };
        let context_dump = unsafe { &mut (*table!(CyberpunkVR_StageStreamContextDump))[side] };
        // SAFETY: the dump's words as bytes.
        if t.context == 0 || !read_safe(t.context, unsafe { core::slice::from_raw_parts_mut(context_dump.as_mut_ptr().cast(), 256 * 8) }) {
            *context_dump = [0; 256];
        }
        let state_dump = unsafe { &mut (*table!(CyberpunkVR_StageStreamStateDump))[side] };
        if t.state == 0 || !read_safe(t.state, unsafe { core::slice::from_raw_parts_mut(state_dump.as_mut_ptr().cast(), 64 * 8) }) {
            *state_dump = [0; 64];
        }
    }
    if t.side >= 0 && t.ordinal < MAX_ORDINALS {
        DONE[(t.row_index * MAX_ORDINALS + t.ordinal) as usize].store(u64::from(t.frame) << 1 | 1, Release);
    }
    log_event(t.frame, t.side as u32, t.stage, t.ordinal, t.kind, t.start, end, t.span_begin, t.span_end);
}
pub type ConsumerFn = unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void) -> i64;
/// Consume: one call of the packet consumer 0x334CC0 counted, captured, skipped or replayed.
///
/// # Safety
/// `forward` is the consumer with the engine's own arguments.
pub unsafe fn consume(side: i32, node: u32, renderer: *mut c_void, arguments: *const c_void, span: *const c_void, forward: ConsumerFn) -> i64 {
    let mut t = Ticket::default();
    if enter(&mut t, side, node, arguments as u64, span as u64) {
        return 0;
    }
    // SAFETY: as documented.
    let result = unsafe { forward(renderer, arguments, span) };
    leave(&mut t);
    result
}
/// InstallVrcamPartitions: the 15 bytes at 0x2277A46 (the 2.5 scene builder's worker count, `cmp [rsi+0FA4h], ebx;
/// mov edi, cs:dword_143C14B50; cmovz edi, ebx`) jump to a check that keeps the worker count when the view's AA mode
/// is 1 only if the graph is VRCAM's or the scope's (the subview's camera name at [r12-2588h]).
pub fn install_vrcam_partitions() -> bool {
    let module = base();
    let site = module + 0x2277A46;
    const EXPECTED: [u8; 15] = [0x39, 0x9E, 0xA4, 0x0F, 0x00, 0x00, 0x8B, 0x3D, 0xFE, 0xD0, 0x99, 0x01, 0x0F, 0x44, 0xFB];
    let mut code_now = [0u8; 15];
    // SAFETY: the executable's own code.
    unsafe { core::ptr::copy_nonoverlapping(site as *const u8, code_now.as_mut_ptr(), 15) };
    if code_now != EXPECTED {
        x::CyberpunkVR_VrcamGbufferPartitionsState.store(2, SeqCst);
        return false;
    }
    let Some(code) = cvr_engine::trampoline::allocate(site as usize, 128) else {
        x::CyberpunkVR_VrcamGbufferPartitionsState.store(2, SeqCst);
        return false;
    };
    let workers = (site + 12).wrapping_add(i32::from_le_bytes(code_now[8..12].try_into().unwrap()) as i64 as u64);
    let mut b: Vec<u8> = Vec::with_capacity(128);
    let mov_rax = |b: &mut Vec<u8>, value: u64| {
        b.extend_from_slice(&[0x48, 0xB8]);
        b.extend_from_slice(&value.to_le_bytes());
    };
    mov_rax(&mut b, workers);
    b.extend_from_slice(&[0x8B, 0x38]); // mov edi, [rax]
    b.extend_from_slice(&[0x39, 0x9E, 0xA4, 0x0F, 0x00, 0x00]); // cmp [rsi+0FA4h], ebx
    b.push(0x75); // jnz done
    let to_done1 = b.len();
    b.push(0);
    let keys = [core::ptr::addr_of!(x::CyberpunkVR_VrcamGbufferPartitionsKey) as u64, core::ptr::addr_of!(x::CyberpunkVR_OpticsGbufferPartitionsKey) as u64];
    let mut to_done = [0usize; 2];
    for (k, key) in keys.iter().enumerate() {
        mov_rax(&mut b, *key);
        b.extend_from_slice(&[0x48, 0x8B, 0x00]); // mov rax, [rax]
        b.extend_from_slice(&[0x48, 0x85, 0xC0]); // test rax, rax
        b.push(0x74); // jz next key
        let to_next = b.len();
        b.push(0);
        b.extend_from_slice(&[0x49, 0x3B, 0x84, 0x24, 0x78, 0xDA, 0xFF, 0xFF]); // cmp rax, [r12-2588h]
        b.push(0x74); // jz done
        to_done[k] = b.len();
        b.push(0);
        b[to_next] = (b.len() - (to_next + 1)) as u8;
    }
    b.extend_from_slice(&[0x89, 0xDF]); // one: mov edi, ebx
    b[to_done1] = (b.len() - (to_done1 + 1)) as u8;
    for at in to_done {
        b[at] = (b.len() - (at + 1)) as u8;
    }
    b.push(0xE9); // done: jmp past the site
    let rel = (site + 15) as i64 - (code as i64 + b.len() as i64 + 4);
    b.extend_from_slice(&(rel as i32).to_le_bytes());
    // SAFETY: fresh arena memory; then the verified site, which no thread executes during boot.
    unsafe {
        cvr_engine::trampoline::write_code(code, &b);
        cvr_engine::trampoline::patch_jump(site as usize, 15, code);
    }
    true
}
/// The poll (once per Present): the census window, the hooks installed while wanted (and the mirrors' generation
/// bumped when tracking restarts), removed when not.
pub fn poll() {
    add(summary(1), 1);
    #[cfg(feature = "rust-build")]
    {
        extern "C-unwind" {
            fn cvr_srg_apply_partitions(want: u32);
        }
        // SAFETY: stage_replay_frame's publication of the partitions' keys.
        unsafe { cvr_srg_apply_partitions(x::CyberpunkVR_VrcamGbufferPartitions.load(Relaxed)) };
    }
    let mode = x::CyberpunkVR_StageReplayMode.load(Relaxed);
    let request = x::CyberpunkVR_StageCensusRequest.load(Relaxed);
    if request != 0 && x::CyberpunkVR_StageCensusState.load(Relaxed) != 1 {
        // Start a window: clear rows, streams, events and take flags.
        CENSUS.store(false, Release);
        // SAFETY: the export tables.
        unsafe {
            *table!(CyberpunkVR_StageCensusRows) = [[0; 32]; 128];
            *table!(CyberpunkVR_StageCensusOps) = [[0; 48]; 128];
            *table!(CyberpunkVR_StageStreamInfo) = [[0; 16]; 2];
        }
        x::CyberpunkVR_StageEventCount.store(0, SeqCst);
        for f in &STREAM_TAKEN {
            f.store(0, SeqCst);
        }
        CENSUS_FRAMES.store(request, SeqCst);
        x::CyberpunkVR_StageCensusRequest.store(0, SeqCst);
        x::CyberpunkVR_StageCensusState.store(1, SeqCst);
        // SAFETY: a summary word.
        unsafe { *summary(0) = 0 };
        CENSUS.store(true, Release);
    } else if CENSUS.load(Relaxed) {
        // SAFETY: a summary word.
        unsafe { *summary(0) += 1 };
        let now = now();
        log_event(engine_frame(), 0xFF, 0xFF, 0, kind::PRESENT, now, now, 0, 0);
        if CENSUS_FRAMES.fetch_sub(1, SeqCst) <= 1 {
            CENSUS.store(false, Release);
            x::CyberpunkVR_StageCensusState.store(2, SeqCst);
        }
    }
    let wanted = mode != 0 || CENSUS.load(Relaxed);
    let tracking = TRACKING.load(Relaxed);
    if wanted {
        if !tracking {
            GENERATION.fetch_add(1, AcqRel); // mirrors from before are not trusted
        }
        install_hooks(); // idempotent; picks up vtables created later
    }
    // Census needs the bound shader-visible heap (from the mirror) to place descriptor copies.
    TRACKING.store(wanted, Release);
    ACTIVE.store(wanted, Release);
    if !wanted && any_installed() {
        uninstall_hooks();
    }
    // SAFETY: a summary word.
    unsafe { *summary(6) = engine_frame().into() };
    PRESENTS.fetch_add(1, Relaxed);
}

// ---- the C ABI

/// # Safety
/// `runtime` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_srt_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime.cast_mut(), Release);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_srt_active() -> bool {
    active()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_srt_graph_salt() -> u64 {
    graph_salt()
}
/// # Safety
/// As consume.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_srt_consume(side: i32, node: u32, renderer: *mut c_void, arguments: *const c_void, span: *const c_void, forward: ConsumerFn) -> i64 {
    unsafe { consume(side, node, renderer, arguments, span, forward) }
}
/// # Safety
/// The device call's arguments.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_srt_copy(n: u32, dest: *const u64, sizes: *const u32, m: u32, src: *const u64, counts: *const u32, kind: i32) {
    unsafe { copy(n, dest, sizes, m, src, counts, kind) }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_srt_install_partitions() -> bool {
    install_vrcam_partitions()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_srt_poll() {
    poll();
}

#[cfg(not(feature = "rust-build"))]
#[allow(non_upper_case_globals, dead_code)]
mod test_exports {
    //! The exports' stand-ins for the unit tests (the plugin's are used_moved_exports_rest's).
    use core::sync::atomic::{AtomicU32, AtomicU64};
    pub static CyberpunkVR_StageReplayMode: AtomicU32 = AtomicU32::new(0);
    pub static CyberpunkVR_StageReplayStages: AtomicU64 = AtomicU64::new(0);
    pub static CyberpunkVR_StageReplaySource: AtomicU32 = AtomicU32::new(1);
    pub static CyberpunkVR_StageCensusRequest: AtomicU32 = AtomicU32::new(0);
    pub static CyberpunkVR_StageCensusState: AtomicU32 = AtomicU32::new(0);
    pub static CyberpunkVR_StageStreamStage: AtomicU32 = AtomicU32::new(u32::MAX);
    pub static mut CyberpunkVR_StageCensusRows: [[u64; 32]; 128] = [[0; 32]; 128];
    pub static mut CyberpunkVR_StageCensusOps: [[u32; 48]; 128] = [[0; 48]; 128];
    pub static mut CyberpunkVR_StageCensusSummary: [u64; 16] = [0; 16];
    pub static mut CyberpunkVR_StageStreamInfo: [[u64; 16]; 2] = [[0; 16]; 2];
    pub static mut CyberpunkVR_StageStreamContextDump: [[u64; 256]; 2] = [[0; 256]; 2];
    pub static mut CyberpunkVR_StageStreamStateDump: [[u64; 64]; 2] = [[0; 64]; 2];
    pub static mut CyberpunkVR_StageStreamContextAddr: [[u64; 2]; 2] = [[0; 2]; 2];
    pub static mut CyberpunkVR_StageReplayStats: [u64; 32] = [0; 32];
    pub static mut CyberpunkVR_StageEvents: [u64; 98304] = [0; 98304];
    pub static CyberpunkVR_StageEventCount: AtomicU32 = AtomicU32::new(0);
    pub static CyberpunkVR_VrcamGbufferPartitions: AtomicU32 = AtomicU32::new(0);
    pub static CyberpunkVR_VrcamGbufferPartitionsState: AtomicU32 = AtomicU32::new(0);
    pub static mut CyberpunkVR_VrcamGbufferPartitionsKey: u64 = 0;
    pub static mut CyberpunkVR_OpticsGbufferPartitionsKey: u64 = 0;
    pub static CyberpunkVR_StageReplayAllowBarriers: AtomicU32 = AtomicU32::new(0);
    pub static CyberpunkVR_StageReplayTableDump: AtomicU32 = AtomicU32::new(0);
    pub static mut CyberpunkVR_StageReplayViewDiag: [[u64; 8]; 2] = [[0; 8]; 2];
    pub static CyberpunkVR_StageReplayWaitUs: AtomicU32 = AtomicU32::new(0);
    pub static mut CyberpunkVR_StageReplayCameraDiag: [u64; 12] = [0; 12];
}

#[cfg(test)]
mod tests {
    //! The stream's limits and payloads, the shadow's setters and overflow, the recording into a scope (foreign
    //! lists, the table look-back, the census rows), the capture slots, the partition claim's sources, the poll's census
    //! window, and the hooks chained over a fabricated vtable -- forwarding to their predecessors, mirroring, recording.
    use super::*;
    /// The tests share the tracking switch and the chains.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn stream_shadow_and_recording() {
        let mut s = Stream::default();
        s.reserve(3, 16);
        let words = [1u32, 2, 3];
        assert!(s.push(cmd(op::ROOT_CONSTANTS, 0, 3, 0, 0, 0), words.as_ptr().cast(), 12));
        assert!(s.push(cmd(op::DRAW, 0, 0, 3, 0, 0), core::ptr::null(), 0));
        assert_eq!((s.commands()[0].d, s.commands()[1].d, s.draws), (0, NO_PAYLOAD, 1));
        assert!(!s.push(cmd(op::ROOT_CONSTANTS, 0, 3, 0, 0, 0), words.as_ptr().cast(), 12) && s.overflowed(), "past the payload limit: sticky");
        assert!(s.payload(0, 12).is_some() && s.payload(8, 12).is_none());
        // The shadow: constants past word 16 overflow; a root resets the parameters.
        let mut sh = Shadow::default();
        sh.set_constants(2, 2, words.as_ptr(), 14);
        assert_eq!((sh.params[2].kind, sh.params[2].word_mask, sh.overflow), (param_kind::CONSTANTS, 0xC000, false));
        sh.set_constants(2, 3, words.as_ptr(), 14);
        assert!(sh.overflow);
        sh.set_root(0x77);
        assert_eq!((sh.root, sh.params[2].kind), (0x77, 0));
        sh.set_vertices(15, 2, core::ptr::null());
        assert!(sh.vertex_mask == 1 << 15);
        // Recording: a scope with a census row; a foreign list flagged; a table bind matched to its copy.
        let mut stream = Stream::default();
        stream.reserve(64, 4096);
        let mut row = [0u64; 32];
        let mut ops = [0u32; 48];
        let mut scope = Scope { stream: &mut stream, row: row.as_mut_ptr(), ops: ops.as_mut_ptr(), list: 0x100, heap_gpu: 0x1000, stride: 32, heap_count: 16, ..Scope::default() };
        T_SCOPE.with(|t| t.set(&mut scope));
        record(core::ptr::null_mut(), cmd(op::COPY, 2, 1, 3, 0, 0), core::ptr::null(), 0);
        record(0x100 as *mut c_void, cmd(op::ROOT_TABLE, 0, 0, 5, 0x1000 + 3 * 32, 0), core::ptr::null(), 0);
        record(0x200 as *mut c_void, cmd(op::DRAW, 0, 0, 3, 0, 0), core::ptr::null(), 0);
        T_SCOPE.with(|t| t.set(core::ptr::null_mut()));
        let c = stream.commands();
        assert_eq!((c[1].c, c[1].count, c[1].flags), (3, 1, 2), "the table at index 3, matched to the copy, a camera table");
        assert_eq!(c[2].flags & 1, 1, "another list's draw is foreign");
        assert_eq!((row[col::COMMANDS], row[col::TABLES], row[col::TABLES_MATCHED], row[col::CAMERA_TABLES], row[col::FOREIGN_OPS], row[col::DRAWS]), (3, 1, 1, 1, 1, 1));
        assert_eq!(ops[op::DRAW as usize], 1);
        record(core::ptr::null_mut(), cmd(op::DRAW, 0, 0, 3, 0, 0), core::ptr::null(), 0);
        assert_eq!(stream.commands().len(), 3, "no scope: nothing recorded");
    }

    #[test]
    fn captures_sources_and_census_window() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        // Two partitions of stage 9, frame 4, both complete and tiling one span: acquired in span order.
        for (o, span) in [(1u32, [0x200u64, 0x300]), (0, [0x100, 0x200])] {
            let cap = slot(9, o, 4).expect("a slot");
            let data = cap.data();
            data.stream.clear();
            (data.frame, data.span_begin, data.span_end, data.foreign) = (4, span[0], span[1], false);
            cap.state.store(cap_state::COMPLETE, SeqCst);
        }
        assert!(source_partitioned(9, 4, 2), "tiled spans are partitions");
        let mut caps = Vec::new();
        assert_eq!(acquire_sources(9, 4, 0, 2, 0, false, &mut caps), fallback::NONE);
        assert_eq!(caps.iter().map(|c| c.data().span_begin).collect::<Vec<_>>(), [0x100, 0x200]);
        assert!(caps.iter().all(|c| c.state.load(SeqCst) == cap_state::READING));
        for c in &caps {
            c.state.store(cap_state::COMPLETE, SeqCst);
        }
        // Another frame's capture is stale; a writing one is busy; nothing stays acquired.
        assert_eq!(acquire_sources(9, 6, 0, 2, 0, false, &mut caps), fallback::STALE_FRAME);
        slot(9, 1, 4).unwrap().state.store(cap_state::WRITING, SeqCst);
        assert_eq!(acquire_sources(9, 4, 0, 2, 0, false, &mut caps), fallback::BUSY);
        assert_eq!(slot(9, 0, 4).unwrap().state.load(SeqCst), cap_state::COMPLETE);
        // The census window: a request of two presents opens it, the second present closes it.
        x::CyberpunkVR_StageCensusRequest.store(2, SeqCst);
        poll();
        assert!(CENSUS.load(SeqCst) && active() && x::CyberpunkVR_StageCensusState.load(SeqCst) == 1);
        poll();
        poll();
        assert!(!CENSUS.load(SeqCst) && x::CyberpunkVR_StageCensusState.load(SeqCst) == 2);
        poll();
        assert!(!active(), "nothing wanted any more");
    }

    #[test]
    fn hooks_chain_over_a_vtable() {
        let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        // A fabricated list whose vtable's slots 10..59 lead to counters; the hooks chain over it.
        static CALLS: AtomicU32 = AtomicU32::new(0);
        unsafe extern "system" fn topology(_: This, _: i32) {
            CALLS.fetch_add(1, SeqCst);
        }
        unsafe extern "system" fn any(_: This) {}
        let mut vtable = [any as *const () as usize; 60];
        vtable[20] = topology as *const () as usize;
        let vt = vtable.as_mut_ptr();
        let object = [vt as usize];
        let list = object.as_ptr() as *mut c_void;
        let mut metas = [ChainMeta::default(); 16];
        let before = CHAIN_COUNT.load(SeqCst) as usize;
        install_on(&mut metas, &[vt as u64, 0, 0, 0, 0, 0]);
        assert_eq!(vtable[20], h_topology as *const () as usize, "chained");
        assert_eq!(CHAINS[before].prev[20].load(SeqCst), topology as *const () as usize, "the predecessor kept");
        TRACKING.store(true, SeqCst);
        SHADOWS.get_or_init(|| (0..SHADOW_CAPACITY).map(|_| ShadowSlot { key: AtomicU64::new(0), last_reset: AtomicU32::new(0), shadow: UnsafeCell::new(Shadow::default()) }).collect());
        vcall!(list as usize, 20, fn(i32), (4));
        assert_eq!(CALLS.load(SeqCst), 1, "forwarded to the predecessor");
        assert_eq!(shadow_for(list, false).map(|s| (s.topology, s.topology_set)), Some((4, true)), "mirrored");
        set_installed(before, &mut metas[before], false);
        assert_eq!(vtable[20], topology as *const () as usize, "unchained");
        TRACKING.store(false, SeqCst);
    }
}
