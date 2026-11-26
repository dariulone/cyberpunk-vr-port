//! Eager native depth preparation (formerly src/Render/NativeDepthPreparation.cpp and the Snapshot/Cache classes of
//! include/Render/NativeDepthPreparation.hpp). At MAIN's depth-preparation stage the port snapshots the vrcam
//! view's 28 packet buckets twice (they must agree), sorts its own copy with the engine's finalizer, and at the
//! vrcam stage compares: mode 1 only verifies that the engine produced the same sorted packets, mode 2 (after 30
//! verified frames) hands the engine the presorted packets instead. Resource-dependent filtering always runs at its
//! original time; a changed input set falls back to the engine's own sort.
//!
//! The exports (`CyberpunkVR_EagerDepth*`) and the preparation run's state stay in the C++ half, which attaches them
//! here at its static initialisation. Native callbacks may unwind (a C++ exception) through these frames: the
//! cache then counts a failure and forgets its pending frame, as the C++ catch-all did before rethrowing.
use core::ffi::c_void;
use std::collections::HashSet;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcess;

/// Packet: one 16-byte, 16-aligned render packet.
#[repr(C, align(16))]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Packet {
    pub first: u64,
    pub second: u64,
}
/// Array: the engine's packet array header.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Array {
    pub data: *mut Packet,
    pub capacity: u32,
    pub count: u32,
}
const _: () = assert!(core::mem::size_of::<Packet>() == 16 && core::mem::size_of::<Array>() == 16);

pub type Read = unsafe extern "C-unwind" fn(u64, *mut c_void, usize) -> bool;
pub type Prepare = unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, *mut c_void, u32, u32);
pub type Gather = unsafe extern "C-unwind" fn(*mut c_void, *mut c_void) -> *mut c_void;
pub type Filter = unsafe extern "C-unwind" fn(*mut c_void, i8, u32, u32) -> i64;
pub type Finalize = unsafe extern "C-unwind" fn(*mut c_void, i8, i64, i64);
/// Api: a reader and the engine's four preparation steps.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Api {
    pub read: Option<Read>,
    pub prepare: Option<Prepare>,
    pub gather: Option<Gather>,
    pub filter: Option<Filter>,
    pub finalize: Option<Finalize>,
}

pub const MAX_PACKETS: usize = 32768;
pub const MAIN: usize = 0;
pub const PREPARED: usize = 1;
pub const VRCAM: usize = 2;
pub const INPUT_SAME: usize = 3;
pub const OUTPUT_SAME: usize = 4;
pub const CHANGED: usize = 5;
pub const MISSING: usize = 6;
pub const REPLACED: usize = 7;
pub const FAILURE: usize = 8;
pub const OUTPUT_DIFFERENT: usize = 9;
pub const STALE: usize = 10;
pub const TOO_LATE: usize = 11;
pub const RAW_COUNT: usize = 12;
pub const FINAL_COUNT: usize = 13;
pub const NATIVE_COUNT: usize = 14;
pub const FIRST_DIFFERENCE: usize = 15;
pub const COUNTER_COUNT: usize = 16;

/// The view's packet buckets: 28 heads at storage+0x34C0, the storage at view+0x2C40 (game 2.5).
const STORAGE_AT: u64 = 0x2C40;
const BUCKETS_AT: u64 = 0x34C0;

fn read_into<T>(read: Read, address: u64, value: &mut T) -> bool {
    // SAFETY: the reader writes at most size_of::<T>() bytes into the value.
    unsafe { read(address, (value as *mut T).cast(), core::mem::size_of::<T>()) }
}

/// Snapshot: the packets of 28 bucket lists, read twice (both reads must agree), owned and 16-aligned.
#[derive(Default, Clone)]
pub struct Snapshot {
    pub packets: Vec<Packet>,
}
impl Snapshot {
    pub fn capture(&mut self, buckets: u64, read: Option<Read>) -> bool {
        let (mut first, mut second) = (Vec::new(), Vec::new());
        if !Self::once(buckets, read, &mut first) || !Self::once(buckets, read, &mut second) || first != second {
            return false;
        }
        self.packets = first;
        true
    }
    fn once(buckets: u64, read: Option<Read>, output: &mut Vec<Packet>) -> bool {
        let mut heads = [0u64; 28];
        let Some(read) = read else { return false };
        if buckets == 0 || !read_into(read, buckets, &mut heads) {
            return false;
        }
        let mut visited = HashSet::new();
        for &head in &heads {
            let mut node = head;
            while node != 0 {
                let mut header = [0u8; 40];
                if node & 7 != 0 || visited.len() >= 4096 || !visited.insert(node) || !read_into(read, node, &mut header) {
                    return false;
                }
                let next = u64::from_le_bytes(header[8..16].try_into().unwrap_or_default());
                let data = u64::from_le_bytes(header[16..24].try_into().unwrap_or_default());
                let count = u32::from_le_bytes(header[32..36].try_into().unwrap_or_default()) as usize;
                if count > MAX_PACKETS - output.len() || (count != 0 && (data == 0 || data & 15 != 0)) {
                    return false;
                }
                let offset = output.len();
                output.resize(offset + count, Packet::default());
                // SAFETY: `count` packets of room at `offset`.
                if count != 0 && !unsafe { read(data, output.as_mut_ptr().add(offset).cast(), count * 16) } {
                    return false;
                }
                let mut after = [0u8; 40];
                if !read_into(read, node, &mut after) || after != header {
                    return false;
                }
                node = next;
            }
        }
        let mut after = [0u64; 28];
        read_into(read, buckets, &mut after) && after == heads
    }
}

/// OwnedBuckets::Node: the engine's {unused, next, data, unused, count} bucket node.
#[repr(C)]
struct Node {
    unused: u64,
    next: u64,
    data: *const Packet,
    unused2: u64,
    count: u32,
    padding: u32,
}
const _: () = assert!(core::mem::size_of::<Node>() == 40 && core::mem::offset_of!(Node, count) == 32);

/// Hands `packets` to the engine's gather as one bucket list (borrowed only for this synchronous call).
fn gather_owned(gather: Gather, packets: &[Packet], output: *mut c_void) {
    let node = Node {
        unused: 0,
        next: 0,
        data: packets.as_ptr(),
        unused2: 0,
        count: packets.len() as u32,
        padding: 0,
    };
    let mut heads: [*const Node; 28] = [core::ptr::null(); 28];
    if node.count != 0 {
        heads[0] = &node;
    }
    // SAFETY: the engine's gather reads the 28 heads and the node, both alive for the call.
    unsafe { gather(heads.as_mut_ptr().cast(), output) };
}

fn read_array(read: Read, output: *mut c_void, header: &mut Array, packets: &mut Vec<Packet>) -> bool {
    if !read_into(read, output as u64, header) || header.count > header.capacity || header.count as usize > MAX_PACKETS {
        return false;
    }
    packets.resize(header.count as usize, Packet::default());
    // SAFETY: room for `count` packets.
    header.count == 0 || unsafe { read(header.data as u64, packets.as_mut_ptr().cast(), packets.len() * 16) }
}

#[derive(Default)]
struct Pending {
    valid: bool,
    frame: u32,
    view: u64,
    storage: u64,
    input: Snapshot,
    sorted: Vec<Packet>,
}

/// Cache: MAIN's presorted copy of the vrcam view's packets, and the counters.
#[derive(Default)]
pub struct Cache {
    pub counts: [u32; COUNTER_COUNT],
    pending: Pending,
}

/// Counts a failure and forgets the pending frame when a native callback unwinds through the cache.
struct FailureOnUnwind(*mut Cache);
impl Drop for FailureOnUnwind {
    fn drop(&mut self) {
        // SAFETY: reached only while unwinding out of Cache::intercept, whose borrow of the cache has ended.
        unsafe {
            (*self.0).counts[FAILURE] += 1;
            (*self.0).pending = Pending::default();
        }
    }
}

impl Cache {
    pub fn reset(&mut self) {
        self.counts = [0; COUNTER_COUNT];
        self.pending = Pending::default();
    }

    /// Intercept: true when the stage was handled here (the engine's preparation ran inside), false when the caller
    /// must run it itself.
    ///
    /// # Safety
    /// The API's functions are the engine's (or compatible) and `context`, `buckets` and `output` are the stage's.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn intercept(
        &mut self,
        api: &Api,
        frame: u32,
        main_view: u64,
        eye_view: u64,
        before_main_draw: bool,
        mode: u32,
        context: *mut c_void,
        buckets: *mut c_void,
        output: *mut c_void,
        flags0: u32,
        flags1: u32,
    ) -> bool {
        let this: *mut Cache = self;
        let guard = FailureOnUnwind(this);
        // SAFETY: the caller's stage; the guard only acts if this call unwinds.
        let handled = unsafe {
            (*this).intercept_stage(
                api,
                frame,
                main_view,
                eye_view,
                before_main_draw,
                mode,
                context,
                buckets,
                output,
                flags0,
                flags1,
            )
        };
        core::mem::forget(guard);
        handled
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn intercept_stage(
        &mut self,
        api: &Api,
        frame: u32,
        main_view: u64,
        eye_view: u64,
        before_main_draw: bool,
        mode: u32,
        context: *mut c_void,
        buckets: *mut c_void,
        output: *mut c_void,
        flags0: u32,
        flags1: u32,
    ) -> bool {
        let (Some(read), Some(prepare), Some(gather), Some(filter), Some(finalize)) = (api.read, api.prepare, api.gather, api.filter, api.finalize) else {
            return false;
        };
        if frame == 0 || main_view == 0 || eye_view == 0 || !(1..=2).contains(&mode) || flags0 != 0 || flags1 != 0 {
            return false;
        }
        let mut args = [0u8; 32];
        let (mut view, mut storage) = (0u64, 0u64);
        if !read_into(read, context as u64, &mut args) {
            return false;
        }
        let stage = u32::from_le_bytes(args[16..20].try_into().unwrap_or_default());
        let work = u64::from_le_bytes(args[0..8].try_into().unwrap_or_default());
        if stage != 3
            || args[24] != 0
            || args[25] != 0
            || args[26] != 1
            || work == 0
            || !read_into(read, work.wrapping_add(24), &mut view)
            || (view != main_view && view != eye_view)
            || !read_into(read, view.wrapping_add(STORAGE_AT), &mut storage)
            || storage == 0
            || buckets as u64 != storage.wrapping_add(BUCKETS_AT)
        {
            return false;
        }
        if view == main_view {
            self.counts[MAIN] += 1;
            self.pending = Pending::default();
            if !before_main_draw {
                self.counts[TOO_LATE] += 1;
                return false;
            }
            let mut peer_storage = 0u64;
            if !read_into(read, eye_view.wrapping_add(STORAGE_AT), &mut peer_storage)
                || peer_storage == 0
                || peer_storage == storage
                || !self.pending.input.capture(peer_storage.wrapping_add(BUCKETS_AT), Some(read))
            {
                self.counts[FAILURE] += 1;
                return false;
            }
            if self.pending.input.packets.is_empty() {
                self.pending = Pending::default();
                return false;
            }
            self.pending.sorted = self.pending.input.packets.clone();
            let len = self.pending.sorted.len() as u32;
            let mut own = Array {
                data: self.pending.sorted.as_mut_ptr(),
                capacity: len,
                count: len,
            };
            // Filtering dereferences live renderer resources; only sorting is pure packet work and may run before
            // the peer's normal stage.
            // SAFETY: the engine's finalizer sorts the port's own array in place.
            unsafe { finalize((&mut own as *mut Array).cast(), 0, 0, 0) };
            if own.data != self.pending.sorted.as_mut_ptr() || own.count as usize != self.pending.sorted.len() {
                self.counts[FAILURE] += 1;
                self.pending = Pending::default();
                return false;
            }
            self.pending.frame = frame;
            self.pending.view = eye_view;
            self.pending.storage = peer_storage;
            self.pending.valid = true;
            self.counts[PREPARED] += 1;
            self.counts[RAW_COUNT] = self.pending.input.packets.len() as u32;
            self.counts[FINAL_COUNT] = own.count;
            // SAFETY: MAIN's own stage, unchanged.
            unsafe { prepare(context, buckets, output, flags0, flags1) };
            return true;
        }
        self.counts[VRCAM] += 1;
        if !self.pending.valid {
            self.counts[MISSING] += 1;
            return false;
        }
        if self.pending.frame != frame || self.pending.view != view || self.pending.storage != storage {
            self.counts[STALE] += 1;
            self.pending = Pending::default();
            return false;
        }
        // Preserve resource-dependent filtering at its original time. If it changes the input set, finish with the
        // normal native sort.
        // SAFETY: the vrcam stage's own gather and filter.
        unsafe {
            gather(buckets, output);
            filter(output, 0, flags0, flags1);
        }
        let mut filtered = Array {
            data: core::ptr::null_mut(),
            capacity: 0,
            count: 0,
        };
        let mut current = Vec::new();
        if !read_array(read, output, &mut filtered, &mut current) {
            self.counts[FAILURE] += 1;
            // SAFETY: the stage's own finalizer completes it.
            unsafe { finalize(output, 0, 0, 0) };
            self.pending = Pending::default();
            return true;
        }
        let same = current == self.pending.input.packets;
        self.counts[if same { INPUT_SAME } else { CHANGED }] += 1;
        if same && mode == 2 {
            gather_owned(gather, &self.pending.sorted, output);
        } else if filtered.count != 0 {
            // SAFETY: the stage's own finalizer.
            unsafe { finalize(output, 0, 0, 0) };
        }
        if !same {
            self.pending = Pending::default();
            return true;
        }
        let mut actual = Array {
            data: core::ptr::null_mut(),
            capacity: 0,
            count: 0,
        };
        let mut result = Vec::new();
        let mut valid = read_into(read, output as u64, &mut actual) && actual.count <= actual.capacity && actual.count as usize <= MAX_PACKETS;
        if valid {
            result.resize(actual.count as usize, Packet::default());
            // SAFETY: room for `count` packets.
            valid = actual.count == 0 || unsafe { read(actual.data as u64, result.as_mut_ptr().cast(), result.len() * 16) };
        }
        self.counts[NATIVE_COUNT] = if valid { actual.count } else { 0 };
        self.counts[FIRST_DIFFERENCE] = u32::MAX;
        if valid && result == self.pending.sorted {
            self.counts[if mode == 2 { REPLACED } else { OUTPUT_SAME }] += 1;
        } else {
            self.counts[OUTPUT_DIFFERENT] += 1;
            if let Some(i) = result.iter().zip(&self.pending.sorted).position(|(a, b)| a != b) {
                self.counts[FIRST_DIFFERENCE] = i as u32;
            }
            if mode == 2 {
                gather_owned(gather, &current, output);
                if filtered.count != 0 {
                    // SAFETY: the stage's own finalizer.
                    unsafe { finalize(output, 0, 0, 0) };
                }
            }
        }
        self.pending = Pending::default();
        true
    }
}

/// What the C++ half attaches: the exports and the preparation run's state (src/Render/NativePreparation.cpp).
#[repr(C)]
pub struct Attach {
    pub mode: *const AtomicU32,
    pub compatible: *const AtomicU32,
    pub verified: *const AtomicU32,
    pub stats: *mut u32,
    pub run_active: extern "C" fn() -> bool,
    pub active: extern "C" fn() -> bool,
    pub pair_state: *const AtomicU32,
    pub run_frames: *const AtomicU32,
    pub pair_data: *const u64,
    pub run_depth: *const AtomicU32,
}
static ATTACHED: AtomicPtr<Attach> = AtomicPtr::new(core::ptr::null_mut());
/// The plugin's own table (formerly lent by src/Render/NativeDepthPreparation.cpp): the exports Rust defines and the
/// preparation run's state. The Rust test variants still attach theirs.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    use super::Attach;
    use crate::declared_moved_exports::{CyberpunkVR_EagerDepthCompatible, CyberpunkVR_EagerDepthMode, CyberpunkVR_EagerDepthVerified};
    use crate::native_state_exports::{
        CyberpunkVR_PreparationPairData, CyberpunkVR_PreparationPairState, CyberpunkVR_PreparationRunDepth, CyberpunkVR_PreparationRunFrames,
        CyberpunkVR_PreparationRunState, CyberpunkVR_PreparationState,
    };
    use crate::typed_moved_exports::CyberpunkVR_EagerDepthStats;
    use core::sync::atomic::Ordering::{Acquire, Relaxed};
    /// preparation::RunActive and preparation::Active.
    extern "C" fn run_active() -> bool {
        CyberpunkVR_PreparationRunState.load(Acquire) == 1
    }
    extern "C" fn active() -> bool {
        CyberpunkVR_PreparationState.load(Relaxed) == 1
    }
    // SAFETY: immutable; its pointers name process-lifetime exports.
    unsafe impl Sync for Attach {}
    pub static ATTACH: Attach = Attach {
        mode: &raw const CyberpunkVR_EagerDepthMode,
        compatible: &raw const CyberpunkVR_EagerDepthCompatible,
        verified: &raw const CyberpunkVR_EagerDepthVerified,
        stats: (&raw mut CyberpunkVR_EagerDepthStats).cast(),
        run_active,
        active,
        pair_state: &raw const CyberpunkVR_PreparationPairState,
        run_frames: &raw const CyberpunkVR_PreparationRunFrames,
        pair_data: (&raw const CyberpunkVR_PreparationPairData).cast(),
        run_depth: (&raw const CyberpunkVR_PreparationRunDepth).cast(),
    };
}
fn attach() -> Option<&'static Attach> {
    // SAFETY: a table the C++ half attached for the process's lifetime.
    let attached = unsafe { ATTACHED.load(Ordering::Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::ATTACH));
    attached
}
/// # Safety
/// `table` and everything it points to live for the rest of the process.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_eager_depth_attach(table: *const Attach) {
    ATTACHED.store(table.cast_mut(), Ordering::Release);
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
fn with_cache<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut guard = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(Cache::default))
}

unsafe extern "C-unwind" fn read_memory(address: u64, destination: *mut c_void, bytes: usize) -> bool {
    let mut copied = 0usize;
    // SAFETY: a checked read of this process's memory into the caller's buffer.
    address != 0
        && address <= u64::MAX - bytes as u64
        && unsafe { ReadProcessMemory(GetCurrentProcess(), address as *const c_void, destination, bytes, Some(&mut copied)) }.is_ok()
        && copied == bytes
}

/// Compatible: the native bodies the cache drives are the ones it was written against (game 2.5). Existing detour
/// prologues are excluded; their native bodies remain intact.
fn compatible(t: &Attach) -> bool {
    static RESULT: OnceLock<bool> = OnceLock::new();
    let result = *RESULT.get_or_init(|| {
        const REGIONS: [(u64, usize, u64); 5] = [
            (0x229D400, 0x4A, 0xB0A35CF1F2670A13),
            (0x4EE6DC, 0x9D, 0xC31F8861D27704F0),
            (0x229D260, 0xE0, 0xD35284DC4BA124BE),
            (0x426B04, 0x3D5, 0x09821EC69F3EAB56),
            (0x4270A4, 0x91C, 0x4F9E97361DB83C3B),
        ];
        // SAFETY: the game's own module.
        let base = unsafe { GetModuleHandleW(None) }.map_or(0, |m| m.0 as u64);
        let mut bytes = [0u8; 4096];
        REGIONS.iter().all(|&(address, size, expected)| {
            // SAFETY: a checked read into the local buffer.
            let read = unsafe { read_memory(base.wrapping_add(address), bytes.as_mut_ptr().cast(), size) };
            read && bytes[..size]
                .iter()
                .fold(14695981039346656037u64, |h, &b| (h ^ b as u64).wrapping_mul(1099511628211))
                == expected
        })
    });
    // SAFETY: the attached export.
    unsafe { &*t.compatible }.store(if result { 1 } else { 2 }, Ordering::SeqCst);
    result
}

fn store_stats(t: &Attach, counts: &[u32; COUNTER_COUNT]) {
    for (i, &c) in counts.iter().enumerate() {
        // SAFETY: the attached export has COUNTER_COUNT entries.
        unsafe { t.stats.add(i).write_volatile(c) };
    }
}

/// BeginRun: a fresh cache and statistics; a verifying run starts unverified.
pub fn begin_run() {
    let Some(t) = attach() else { return };
    with_cache(|cache| {
        cache.reset();
        store_stats(t, &[0; COUNTER_COUNT]);
        // SAFETY: the attached exports.
        if unsafe { &*t.mode }.load(Ordering::SeqCst) == 1 {
            unsafe { &*t.verified }.store(0, Ordering::SeqCst);
        }
    });
}
/// EndRun: forget the cache.
pub fn end_run() {
    with_cache(Cache::reset);
}

/// Intercept: the engine's depth-preparation stage, while a preparation run with a verified view pair is active.
///
/// # Safety
/// The engine's own stage functions and arguments.
#[allow(clippy::too_many_arguments)]
pub unsafe fn intercept(
    prepare: Option<Prepare>,
    gather: Option<Gather>,
    filter: Option<Filter>,
    finalize: Option<Finalize>,
    context: *mut c_void,
    buckets: *mut c_void,
    output: *mut c_void,
    flags0: u32,
    flags1: u32,
) -> bool {
    let Some(t) = attach() else { return false };
    // SAFETY: the attached exports and run state.
    let (mode_export, verified, pair_state) = unsafe { (&*t.mode, &*t.verified, &*t.pair_state) };
    let mode = mode_export.load(Ordering::Relaxed);
    if mode == 0
        || !(t.run_active)()
        || (t.active)()
        || pair_state.load(Ordering::SeqCst) != 2
        || (mode == 2 && verified.load(Ordering::SeqCst) == 0)
        || !compatible(t)
    {
        return false;
    }
    with_cache(|cache| {
        // SAFETY: the attached run state; the pair data is written by the preparation hooks.
        let (frame, main_view, eye_view, depth) = unsafe {
            (
                (*t.run_frames).load(Ordering::SeqCst),
                t.pair_data.add(4).read_volatile(),
                t.pair_data.add(3).read_volatile(),
                (*t.run_depth).load(Ordering::SeqCst),
            )
        };
        let api = Api {
            read: Some(read_memory),
            prepare,
            gather,
            filter,
            finalize,
        };
        // SAFETY: the engine's stage.
        let handled = unsafe {
            cache.intercept(
                &api,
                frame,
                main_view,
                eye_view,
                depth.wrapping_add(1) == frame,
                mode,
                context,
                buckets,
                output,
                flags0,
                flags1,
            )
        };
        store_stats(t, &cache.counts);
        if mode == 1 {
            if cache.counts[OUTPUT_DIFFERENT] != 0 || cache.counts[FAILURE] != 0 {
                verified.store(0, Ordering::SeqCst);
            } else if cache.counts[OUTPUT_SAME] >= 30 {
                verified.store(1, Ordering::SeqCst);
            }
        }
        handled
    })
}

// The header's wrappers (NativeDepthPreparation.hpp in the Rust build).

/// # Safety
/// `read` reads this process's memory; `data`/`count` receive the packets of the returned handle.
#[unsafe(no_mangle)]
pub unsafe extern "C-unwind" fn cvr_eager_depth_capture(buckets: u64, read: Option<Read>, data: *mut *const Packet, count: *mut usize) -> *mut Snapshot {
    let mut snapshot = Snapshot::default();
    if !snapshot.capture(buckets, read) {
        return core::ptr::null_mut();
    }
    let snapshot = Box::new(snapshot);
    // SAFETY: the caller's outputs.
    unsafe {
        *data = snapshot.packets.as_ptr();
        *count = snapshot.packets.len();
    }
    Box::into_raw(snapshot)
}
/// # Safety
/// `s` came from cvr_eager_depth_capture (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_eager_depth_snapshot_free(s: *mut Snapshot) {
    if !s.is_null() {
        // SAFETY: the snapshot handed out once.
        drop(unsafe { Box::from_raw(s) });
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_eager_depth_cache_new() -> *mut Cache {
    Box::into_raw(Box::default())
}
/// # Safety
/// `c` came from cvr_eager_depth_cache_new (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_eager_depth_cache_free(c: *mut Cache) {
    if !c.is_null() {
        // SAFETY: the wrapper's cache, dropped once.
        drop(unsafe { Box::from_raw(c) });
    }
}
/// # Safety
/// `c` is a live cache; `counts` has COUNTER_COUNT entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_eager_depth_cache_reset(c: *mut Cache, counts: *mut u32) {
    // SAFETY: the wrapper's cache and counters.
    if let Some(c) = unsafe { c.as_mut() } {
        c.reset();
        unsafe { core::ptr::copy_nonoverlapping(c.counts.as_ptr(), counts, COUNTER_COUNT) };
    }
}
/// # Safety
/// As Cache::intercept; `c` is a live cache, `api` an API table and `counts` has COUNTER_COUNT entries.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_eager_depth_cache_intercept(
    c: *mut Cache,
    api: *const Api,
    frame: u32,
    main_view: u64,
    eye_view: u64,
    before_main_draw: bool,
    mode: u32,
    context: *mut c_void,
    buckets: *mut c_void,
    output: *mut c_void,
    flags0: u32,
    flags1: u32,
    counts: *mut u32,
) -> bool {
    // SAFETY: the wrapper's cache and the caller's API.
    let (Some(cache), Some(api)) = (unsafe { c.as_mut() }, unsafe { api.as_ref() }) else {
        return false;
    };
    // SAFETY: the caller's stage.
    let handled = unsafe {
        cache.intercept(
            api,
            frame,
            main_view,
            eye_view,
            before_main_draw,
            mode,
            context,
            buckets,
            output,
            flags0,
            flags1,
        )
    };
    // SAFETY: the caller's counters.
    unsafe { core::ptr::copy_nonoverlapping(cache.counts.as_ptr(), counts, COUNTER_COUNT) };
    handled
}
/// # Safety
/// As intercept.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_eager_depth_intercept(
    prepare: Option<Prepare>,
    gather: Option<Gather>,
    filter: Option<Filter>,
    finalize: Option<Finalize>,
    context: *mut c_void,
    buckets: *mut c_void,
    output: *mut c_void,
    flags0: u32,
    flags1: u32,
) -> bool {
    // SAFETY: the caller's stage.
    unsafe { intercept(prepare, gather, filter, finalize, context, buckets, output, flags0, flags1) }
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_eager_depth_begin_run() {
    begin_run();
}
#[unsafe(no_mangle)]
pub extern "C" fn cvr_eager_depth_end_run() {
    end_run();
}
