//! Finding the game's depth buffer, and the queue it is written on (formerly the depth half of
//! src/Hooks/DepthCapture.cpp; the descriptor-audit hooks there dispatch to C++ consumers and stay).
//!
//! The depth buffer is not handed to anyone: it is bound. So the way to find it is to watch what the engine binds --
//! CreateDepthStencilView records which resource a descriptor names, OMSetRenderTargets records which descriptor is
//! bound when, and ResourceBarrier records the state it rests in. Three hooks, one answer.
//!
//! THE PICK. The scene depth is the largest depth-stencil bound to OM among the formats that can serve as a depth
//! layer (the R32 and R32G8X24 families; the biggest depth-stencil of all is the 16-bit shadow atlas) and, while the
//! render graph's view key is known, of MAIN's view only (else of the presented size). It is STICKY: the choice
//! reopens only after the current pick has gone unbound for a few presents (MAIN owns two same-size depth-stencils,
//! and an alternating pick never warms the capture up). The slot owns one reference, so the pointer stays valid after
//! the game frees its own.
//!
//! THE MOMENT. The barrier that makes the pick shader-readable for the engine's own post passes is where the depth is
//! copied (the OpenXR capture, a copy on the same list); waiting until Present finds it back in DEPTH_WRITE.
//!
//! THE QUEUE. The list that bound the pick is remembered; the queue that executes it is the depth-writer queue, on which
//! a resolve is FIFO-ordered after the write with no cross-queue wait. The old cross-queue signal tracker is kept only
//! for an explicit diagnostic A/B (CyberpunkVR_LegacyQueueSignalTracking).
//!
//! The exports, the ExecuteCommandLists counters SwapChain.cpp reports, the OpenXR capture, the view key and the
//! internal-command flag stay in the C++ half, which attaches them; the hooks are patched through cvr-engine's vtable
//! registry.
use cvr_host::fast::{FastMap, FastSet};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use std::sync::{Mutex, MutexGuard};

use cvr_engine::vtable;
use cvr_host::log;
use windows::Win32::Graphics::Direct3D12::{D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_RESOURCE_DESC};
use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::SystemInformation::GetTickCount64;

/// ID3D12Device::CreateDepthStencilView, ID3D12GraphicsCommandList::OMSetRenderTargets and ResourceBarrier,
/// ID3D12CommandQueue::ExecuteCommandLists and Signal.
pub const CREATE_DSV_SLOT: usize = 21;
const OM_SET_RENDER_TARGETS_SLOT: usize = 46;
const RESOURCE_BARRIER_SLOT: usize = 26;
const EXECUTE_COMMAND_LISTS_SLOT: usize = 10;
const SIGNAL_SLOT: usize = 14;

const TRANSITION: u32 = 0;
/// A queue that has not signalled for this long is not part of the current frame's work.
const SIGNAL_FRESH_MS: u64 = 250;
/// And one this old is gone for good.
const SIGNAL_DEAD_MS: u64 = 5000;

/// What the C++ half attaches.
#[repr(C)]
pub struct Runtime {
    pub verbose_log: *const i32,
    pub runtime_diagnostics: *const i32,
    pub legacy_signal_tracking: *const AtomicU32,
    pub ecl_timing: *const AtomicU64,
    pub ecl_total_calls: *const AtomicU64,
    pub ecl_total_lists: *const AtomicU64,
    pub ecl_present_calls: *const AtomicU64,
    pub ecl_present_lists: *const AtomicU64,
    pub ecl_distinct_queues: *const AtomicU32,
    pub ecl_seen_queues: *const AtomicPtr<c_void>,
    pub present_queue: *const AtomicPtr<c_void>,
    pub waits_issued: *mut u64,
    pub waits_skipped: *mut u64,
    pub waits_dropped: *mut u64,
    /// The DSV's other observers (scene state, mono cascades, hidden area); false when the call is the port's own
    /// recording (cvr::stereo::probe::internalCommands) and the view is not to be tracked.
    pub dsv_observers:
        unsafe extern "C" fn(resource: *mut c_void, desc: *const c_void, dest: usize) -> bool,
    pub internal_commands: extern "C" fn() -> bool,
    pub view_key_hook_active: extern "C" fn() -> bool,
    pub main_view_recording: extern "C" fn() -> bool,
}
// SAFETY: exported variables and functions of the C++ half, alive for the process.
unsafe impl Sync for Runtime {}
unsafe impl Send for Runtime {}

/// The plugin's table, built here (formerly src/Hooks/DepthCapture.cpp's); a test program attaches its own.
#[cfg(feature = "plugin-tables")]
mod plugin {
    use super::Runtime;
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64};
    unsafe extern "C" {
        #[link_name = "?g_verboseLog@@3HC"]
        static g_verboseLog: i32;
        static CyberpunkVR_RuntimeDiagnostics: i32;
        static CyberpunkVR_LegacyQueueSignalTracking: AtomicU32;
        static CyberpunkVR_EclTiming: [AtomicU64; 8];
        #[link_name = "?g_eclTotalCalls@@3U?$atomic@_K@std@@A"]
        static g_eclTotalCalls: AtomicU64;
        #[link_name = "?g_eclTotalLists@@3U?$atomic@_K@std@@A"]
        static g_eclTotalLists: AtomicU64;
        #[link_name = "?g_eclPresentCalls@@3U?$atomic@_K@std@@A"]
        static g_eclPresentCalls: AtomicU64;
        #[link_name = "?g_eclPresentLists@@3U?$atomic@_K@std@@A"]
        static g_eclPresentLists: AtomicU64;
        #[link_name = "?g_eclDistinctQueues@@3U?$atomic@I@std@@A"]
        static g_eclDistinctQueues: AtomicU32;
        /// std::atomic<void*> g_eclSeenQueues[16] (src/Hooks/SwapChain.cpp).
        #[link_name = "?g_eclSeenQueues@@3PAU?$atomic@PEAX@std@@A"]
        static g_eclSeenQueues: [AtomicPtr<c_void>; 16];
        #[link_name = "?g_presentQueue@@3U?$atomic@PEAX@std@@A"]
        static g_presentQueue: AtomicPtr<c_void>;
        static mut CyberpunkVR_DebugQueueWaitsIssued: u64;
        static mut CyberpunkVR_DebugQueueWaitsSkipped: u64;
        static mut CyberpunkVR_DebugQueueWaitsDropped: u64;
        safe fn CyberpunkVR_ViewKeyHookActive() -> i32;
        safe fn CyberpunkVR_IsMainViewRecording() -> i32;
        /// The native probe's binding observation (cvr-render stereo_probe.rs).
        safe fn cvr_observe_bindings() -> bool;
        /// The hidden area's descriptor -> target map (cvr-render hidden_area_runtime.rs).
        fn cvr_hidden_area_dsv_created(resource: *mut c_void, desc: *const c_void, handle: usize);
    }
    unsafe extern "C-unwind" {
        /// The native probe's switch (cvr-render stereo_probe_creation.rs).
        safe fn cvr_nspc_enabled() -> bool;
    }
    /// The DSV's other observers (formerly src/Hooks/DepthCapture.cpp's cvr_cpp_depth_dsv_observers): the scene state
    /// while the native probe or its bindings observe views, then -- unless the call is the port's own recording -- the
    /// mono cascades and the hidden area.
    unsafe extern "C" fn dsv_observers(resource: *mut c_void, desc: *const c_void, dest: usize) -> bool {
        if cvr_observe_bindings() || cvr_nspc_enabled() {
            // SAFETY: the device call's view, as the hook received it.
            unsafe { crate::scene_state::cvr_scene_dsv_created(resource as usize, desc.cast(), dest) };
        }
        if cvr_host::probe::internal() {
            return false;
        }
        // SAFETY: the device call's view, as the hook received it.
        unsafe {
            crate::mono_cascades::cvr_mono_cascades_dsv_created(resource, desc.cast(), dest);
            cvr_hidden_area_dsv_created(resource, desc, dest);
        }
        true
    }
    /// cvr::stereo::probe::internalCommands, this thread's.
    extern "C" fn internal_commands() -> bool {
        cvr_host::probe::internal()
    }
    extern "C" fn view_key_hook_active() -> bool {
        CyberpunkVR_ViewKeyHookActive() != 0
    }
    extern "C" fn main_view_recording() -> bool {
        CyberpunkVR_IsMainViewRecording() != 0
    }
    pub(super) static RT: Runtime = Runtime {
        verbose_log: &raw const g_verboseLog,
        runtime_diagnostics: &raw const CyberpunkVR_RuntimeDiagnostics,
        legacy_signal_tracking: &raw const CyberpunkVR_LegacyQueueSignalTracking,
        ecl_timing: (&raw const CyberpunkVR_EclTiming).cast(),
        ecl_total_calls: &raw const g_eclTotalCalls,
        ecl_total_lists: &raw const g_eclTotalLists,
        ecl_present_calls: &raw const g_eclPresentCalls,
        ecl_present_lists: &raw const g_eclPresentLists,
        ecl_distinct_queues: &raw const g_eclDistinctQueues,
        ecl_seen_queues: (&raw const g_eclSeenQueues).cast(),
        present_queue: &raw const g_presentQueue,
        waits_issued: &raw mut CyberpunkVR_DebugQueueWaitsIssued,
        waits_skipped: &raw mut CyberpunkVR_DebugQueueWaitsSkipped,
        waits_dropped: &raw mut CyberpunkVR_DebugQueueWaitsDropped,
        dsv_observers,
        internal_commands,
        view_key_hook_active,
        main_view_recording,
    };
}
#[cfg(feature = "plugin-tables")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "plugin-tables"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());

fn runtime() -> Option<&'static Runtime> {
    // SAFETY: attached once with a pointer the C++ half keeps for the process.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}

#[no_mangle]
pub extern "C" fn cvr_depth_capture_attach(runtime: *mut Runtime) {
    RUNTIME.store(runtime, Ordering::Release);
}

unsafe fn get<T: Copy>(p: *const T) -> T {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(p) }
}
fn verbose(rt: &Runtime) -> bool {
    // SAFETY: g_verboseLog.
    unsafe { get(rt.verbose_log) != 0 }
}
fn diagnostics(rt: &Runtime) -> bool {
    // SAFETY: CyberpunkVR_RuntimeDiagnostics.
    unsafe { get(rt.runtime_diagnostics) != 0 }
}
fn bump(p: *mut u64) {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1)) };
}
fn now_ms() -> u64 {
    // SAFETY: no arguments.
    unsafe { GetTickCount64() }
}
fn qpc() -> i64 {
    let mut t = 0i64;
    // SAFETY: an out integer.
    let _ = unsafe { QueryPerformanceCounter(&mut t) };
    t
}

// ---- COM by vtable slot

unsafe fn vtable_of(object: *mut c_void) -> *mut *mut c_void {
    // SAFETY: a live COM object.
    unsafe { *(object as *mut *mut *mut c_void) }
}
unsafe fn add_ref(object: *mut c_void) {
    // SAFETY: IUnknown::AddRef.
    unsafe {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 =
            core::mem::transmute(*vtable_of(object).add(1));
        f(object);
    }
}
unsafe fn release(object: *mut c_void) {
    // SAFETY: IUnknown::Release.
    unsafe {
        let f: unsafe extern "system" fn(*mut c_void) -> u32 =
            core::mem::transmute(*vtable_of(object).add(2));
        f(object);
    }
}
unsafe fn resource_desc(resource: *mut c_void) -> D3D12_RESOURCE_DESC {
    let mut desc = D3D12_RESOURCE_DESC::default();
    // SAFETY: ID3D12Resource::GetDesc (this, the return buffer).
    unsafe {
        let f: unsafe extern "system" fn(
            *mut c_void,
            *mut D3D12_RESOURCE_DESC,
        ) -> *mut D3D12_RESOURCE_DESC = core::mem::transmute(*vtable_of(resource).add(10));
        f(resource, &mut desc);
    }
    desc
}
unsafe fn list_type(list: *mut c_void) -> i32 {
    // SAFETY: ID3D12CommandList::GetType.
    unsafe {
        let f: unsafe extern "system" fn(*mut c_void) -> i32 =
            core::mem::transmute(*vtable_of(list).add(8));
        f(list)
    }
}
unsafe fn original<F: Copy>(object: *mut c_void, slot: usize) -> Option<F> {
    // SAFETY: the slot's original has the hook's signature.
    unsafe {
        let p = vtable::original(vtable_of(object), slot);
        (!p.is_null()).then(|| core::mem::transmute_copy::<*mut c_void, F>(&p))
    }
}

// ---- the state

#[derive(Clone, Copy)]
struct Dsv {
    resource: usize,
    format: u32,
    width: u32,
    height: u32,
}
static DSVS: Mutex<Option<FastMap<usize, Dsv>>> = Mutex::new(None);
static DSV_COUNT: AtomicU32 = AtomicU32::new(0);

static DEPTH: AtomicUsize = AtomicUsize::new(0);
static DEPTH_WIDTH: AtomicU32 = AtomicU32::new(0);
static DEPTH_HEIGHT: AtomicU32 = AtomicU32::new(0);
static DEPTH_FORMAT: AtomicU32 = AtomicU32::new(0);
static DEPTH_AREA: AtomicU64 = AtomicU64::new(0);
/// D3D12_RESOURCE_STATE_COMMON until an explicit transition is observed.
static DEPTH_STATE: AtomicU32 = AtomicU32::new(0);
static DEPTH_SEEN: AtomicBool = AtomicBool::new(false);
static DEPTH_MISS_FRAMES: AtomicU32 = AtomicU32::new(0);
/// Guards the slot's one reference (taken, dropped, lent to the submit path).
static DEPTH_REF: Mutex<()> = Mutex::new(());
static PRESENT_WIDTH: AtomicU32 = AtomicU32::new(0);
static PRESENT_HEIGHT: AtomicU32 = AtomicU32::new(0);
static OM_CALLS: AtomicU64 = AtomicU64::new(0);
/// The list that last bound the pick, and the queue that executed it (the depth-writer queue).
static BINDER_LIST: AtomicUsize = AtomicUsize::new(0);
static WRITER_QUEUE: AtomicUsize = AtomicUsize::new(0);

static PATCHED_LISTS: Mutex<Option<FastSet<usize>>> = Mutex::new(None);
/// The first vtables seen of each kind, for the lock-free answer: direct lists hooked, other lists left alone.
static HOOKED_TABLES: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];
static OTHER_TABLES: [AtomicUsize; 8] = [const { AtomicUsize::new(0) }; 8];

fn remembered(slots: &[AtomicUsize; 8], table: usize) -> bool {
    slots.iter().any(|s| s.load(Ordering::Relaxed) == table)
}
fn remember(slots: &[AtomicUsize; 8], table: usize) {
    for s in slots {
        match s.compare_exchange(0, table, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(existing) if existing == table => return,
            Err(_) => {}
        }
    }
}
static DISTINCT_LIST_VTABLES: AtomicU32 = AtomicU32::new(0);

fn lock<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A format that can serve as a depth layer: the R32 family and the R32G8X24 family (resolved by the depth-plane
/// shader). The shadow atlas is R16 and never qualifies.
pub fn resolvable_depth(format: u32) -> bool {
    // R32G8X24_TYPELESS 19, D32_FLOAT_S8X24_UINT 20, R32_FLOAT_X8X24_TYPELESS 21, X32_TYPELESS_G8X24_UINT 22,
    // R32_TYPELESS 39, D32_FLOAT 40, R32_FLOAT 41.
    matches!(format, 19 | 20 | 21 | 22 | 39 | 40 | 41)
}

/// The area a bound depth-stencil competes with, or 0 when it is not a candidate: resolvable, and MAIN's while the
/// view key is known, else the presented size (or any size before one is known).
pub fn candidate_area(
    format: u32,
    width: u32,
    height: u32,
    present: (u32, u32),
    view_known: bool,
    main_view: bool,
) -> u64 {
    if !resolvable_depth(format) {
        return 0;
    }
    let size_known = present.0 != 0 && present.1 != 0;
    let exact = size_known && width == present.0 && height == present.1;
    let accept = if view_known {
        main_view
    } else {
        !size_known || exact
    };
    if accept {
        width as u64 * height as u64
    } else {
        0
    }
}

// ---- the hooks

/// HookedCreateDepthStencilView: which resource a DSV descriptor names.
///
/// # Safety
/// Called by D3D12's callers through the patched slot.
#[no_mangle]
pub unsafe extern "system" fn cvr_depth_create_dsv(
    device: *mut c_void,
    resource: *mut c_void,
    desc: *const c_void,
    dest: usize,
) {
    // SAFETY: the device's own method, then the observers with its arguments.
    unsafe {
        if let Some(f) = original::<
            unsafe extern "system" fn(*mut c_void, *mut c_void, *const c_void, usize),
        >(device, CREATE_DSV_SLOT)
        {
            f(device, resource, desc, dest);
        }
        let Some(rt) = runtime() else { return };
        if !(rt.dsv_observers)(resource, desc, dest) {
            return;
        }
        if resource.is_null() || dest == 0 {
            return;
        }
        let rd = resource_desc(resource);
        // D3D12_DEPTH_STENCIL_VIEW_DESC::Format is its first field.
        let format = if desc.is_null() {
            rd.Format.0 as u32
        } else {
            *(desc as *const u32)
        };
        let dsv = Dsv {
            resource: resource as usize,
            format,
            width: rd.Width as u32,
            height: rd.Height,
        };
        let mut map = lock(&DSVS);
        if map
            .get_or_insert_with(cvr_host::fast::fast_map)
            .insert(dest, dsv)
            .is_none()
        {
            DSV_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// HookedOMSetRenderTargets: the pick, made from what the frame binds.
///
/// # Safety
/// Called by D3D12's callers through the patched slot.
#[no_mangle]
pub unsafe extern "system" fn cvr_depth_om_set_render_targets(
    list: *mut c_void,
    count: u32,
    rtvs: *const usize,
    single: i32,
    dsv: *const usize,
) {
    // SAFETY: the list's own method; the DSV handle the caller passed.
    unsafe {
        if let Some(f) = original::<
            unsafe extern "system" fn(*mut c_void, u32, *const usize, i32, *const usize),
        >(list, OM_SET_RENDER_TARGETS_SLOT)
        {
            f(list, count, rtvs, single, dsv);
        }
        let Some(rt) = runtime() else { return };
        if diagnostics(rt) {
            OM_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        if (rt.internal_commands)() || dsv.is_null() || *dsv == 0 {
            return;
        }
        let Some(info) = lock(&DSVS).as_ref().and_then(|m| m.get(&*dsv).copied()) else {
            return;
        };
        if info.resource != 0 && info.resource == DEPTH.load(Ordering::Relaxed) {
            // Still in use: keep it, and remember the list (its queue is the depth writer's).
            BINDER_LIST.store(list as usize, Ordering::Relaxed);
            DEPTH_SEEN.store(true, Ordering::Relaxed);
        }
        let present = (
            PRESENT_WIDTH.load(Ordering::Relaxed),
            PRESENT_HEIGHT.load(Ordering::Relaxed),
        );
        let area = candidate_area(
            info.format,
            info.width,
            info.height,
            present,
            (rt.view_key_hook_active)(),
            (rt.main_view_recording)(),
        );
        if area > DEPTH_AREA.load(Ordering::Relaxed) {
            // The slot owns one reference: the pointer stays valid after the game frees its own.
            let _guard = lock(&DEPTH_REF);
            if area > DEPTH_AREA.load(Ordering::Relaxed) {
                if info.resource != 0 {
                    add_ref(info.resource as *mut c_void);
                }
                let old = DEPTH.swap(info.resource, Ordering::Relaxed);
                DEPTH_AREA.store(area, Ordering::Relaxed);
                DEPTH_WIDTH.store(info.width, Ordering::Relaxed);
                DEPTH_HEIGHT.store(info.height, Ordering::Relaxed);
                DEPTH_FORMAT.store(info.format, Ordering::Relaxed);
                if old != 0 {
                    release(old as *mut c_void);
                }
            }
        }
    }
}

/// D3D12_RESOURCE_BARRIER as its transition is read here.
#[repr(C)]
struct Barrier {
    kind: u32,
    flags: u32,
    resource: usize,
    subresource: u32,
    before: u32,
    after: u32,
    pad: u32,
}

/// HookedResourceBarrier: the state the pick rests in, and the moment it becomes shader-readable.
///
/// # Safety
/// Called by D3D12's callers through the patched slot.
#[no_mangle]
pub unsafe extern "system" fn cvr_depth_resource_barrier(
    list: *mut c_void,
    count: u32,
    barriers: *const c_void,
) {
    // SAFETY: the list's own method; the caller's barriers.
    unsafe {
        if let Some(f) = original::<unsafe extern "system" fn(*mut c_void, u32, *const c_void)>(
            list,
            RESOURCE_BARRIER_SLOT,
        ) {
            f(list, count, barriers);
        }
        let depth = DEPTH.load(Ordering::Relaxed);
        if depth == 0 || barriers.is_null() {
            return;
        }
        let barriers = core::slice::from_raw_parts(barriers as *const Barrier, count as usize);
        for b in barriers {
            if b.kind == TRANSITION && b.resource == depth {
                // The scene depth's state as the engine leaves it (the legacy single-view snapshot that copied it
                // here had no reader left: the stereo depth layer replaced it).
                DEPTH_STATE.store(b.after, Ordering::Relaxed);
            }
        }
    }
}

/// TryHookCmdListVtable: every distinct direct-list vtable gets the OM and barrier hooks (more than one records the
/// scene).
///
/// # Safety
/// `list` is null or a live command list.
pub unsafe fn hook_list_vtable(rt: &Runtime, list: *mut c_void) {
    if list.is_null() {
        return;
    }
    // SAFETY: a live command list.
    unsafe {
        let table = vtable_of(list);
        // Every submission passes here, list by list, with a handful of vtables: the ones already decided are
        // answered without a COM call or a lock.
        if remembered(&HOOKED_TABLES, table as usize) || remembered(&OTHER_TABLES, table as usize) {
            return;
        }
        if list_type(list) != D3D12_COMMAND_LIST_TYPE_DIRECT.0 {
            remember(&OTHER_TABLES, table as usize);
            return;
        }
        let first = lock(&PATCHED_LISTS)
            .get_or_insert_with(cvr_host::fast::fast_set)
            .insert(table as usize);
        remember(&HOOKED_TABLES, table as usize);
        if !first {
            return;
        }
        let n = DISTINCT_LIST_VTABLES.fetch_add(1, Ordering::Relaxed) + 1;
        if vtable::patch(
            table,
            OM_SET_RENDER_TARGETS_SLOT,
            cvr_depth_om_set_render_targets as *mut c_void,
        ) && verbose(rt)
        {
            log!(
                "[DEPTH-DIAG] Hooked OMSetRenderTargets on direct-list vtable={:016X} (distinctVtables={})\n",
                table as usize,
                n
            );
        }
        vtable::patch(
            table,
            RESOURCE_BARRIER_SLOT,
            cvr_depth_resource_barrier as *mut c_void,
        );
    }
}

fn record_queue(rt: &Runtime, queue: *mut c_void) {
    if !diagnostics(rt) {
        return;
    }
    for i in 0..16 {
        // SAFETY: the C++ half's sixteen slots.
        let slot = unsafe { &*rt.ecl_seen_queues.add(i) };
        let seen = slot.load(Ordering::Relaxed);
        if seen == queue {
            return;
        }
        if seen.is_null() {
            if slot
                .compare_exchange(
                    core::ptr::null_mut(),
                    queue,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                // SAFETY: the C++ half's counter.
                unsafe { (*rt.ecl_distinct_queues).fetch_add(1, Ordering::Relaxed) };
                return;
            }
            if slot.load(Ordering::Relaxed) == queue {
                return;
            }
        }
    }
}

type ExecuteFn = unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void);

/// HookedExecuteCommandLists: the lists' vtables hooked, the depth-writer queue found, the counters SwapChain.cpp
/// reports, and the GPU timeline's stamps around the original when a capture is on.
///
/// # Safety
/// Called by D3D12's callers through the patched slot.
#[no_mangle]
pub unsafe extern "system" fn cvr_depth_execute_command_lists(
    queue: *mut c_void,
    count: u32,
    lists: *const *mut c_void,
) {
    let start = qpc();
    let Some(rt) = runtime() else { return };
    // SAFETY: the C++ half's counters, the caller's lists, the queue's own method.
    unsafe {
        if diagnostics(rt) {
            (*rt.ecl_total_calls).fetch_add(1, Ordering::Relaxed);
            (*rt.ecl_total_lists).fetch_add(count as u64, Ordering::Relaxed);
        }
        record_queue(rt, queue);
        if (*rt.legacy_signal_tracking).load(Ordering::Relaxed) != 0 {
            hook_queue_signal(queue);
        } else {
            clear_legacy_signals();
        }
        if !lists.is_null() {
            let binder = BINDER_LIST.load(Ordering::Relaxed);
            for i in 0..count as usize {
                let list = *lists.add(i);
                hook_list_vtable(rt, list);
                if binder != 0 && list as usize == binder {
                    WRITER_QUEUE.store(queue as usize, Ordering::Relaxed);
                }
            }
        }
        if verbose(rt) && (*rt.ecl_total_calls).load(Ordering::Relaxed) % 600 == 1 {
            log!(
                "[DEPTH-DIAG] status: sceneDepth res={:016X} {}x{} fmt={} | distinctDSV={} omCalls={} cmdVtables={}\n",
                DEPTH.load(Ordering::Relaxed),
                DEPTH_WIDTH.load(Ordering::Relaxed),
                DEPTH_HEIGHT.load(Ordering::Relaxed),
                DEPTH_FORMAT.load(Ordering::Relaxed),
                DSV_COUNT.load(Ordering::Relaxed),
                OM_CALLS.load(Ordering::Relaxed),
                DISTINCT_LIST_VTABLES.load(Ordering::Relaxed)
            );
        }
        if queue == (*rt.present_queue).load(Ordering::Relaxed) && diagnostics(rt) {
            (*rt.ecl_present_calls).fetch_add(1, Ordering::Relaxed);
            (*rt.ecl_present_lists).fetch_add(count as u64, Ordering::Relaxed);
        }
        let original = original::<ExecuteFn>(queue, EXECUTE_COMMAND_LISTS_SLOT);
        (*rt.ecl_timing.add(2)).fetch_add((qpc() - start) as u64, Ordering::Relaxed);
        if let Some(f) = original {
            if !crate::queue_timeline::cvr_queue_timeline_execute(queue, count, lists, Some(f)) {
                f(queue, count, lists);
            }
        }
    }
}

/// InstallCommandQueueDiagHook: the present queue remembered, ExecuteCommandLists hooked (all queues of a type share
/// one vtable).
///
/// # Safety
/// `queue` is null or a live command queue.
#[no_mangle]
pub unsafe extern "C" fn cvr_depth_install_queue_hook(queue: *mut c_void) {
    if queue.is_null() {
        return;
    }
    let Some(rt) = runtime() else { return };
    // SAFETY: the C++ half's atomic; a live queue.
    unsafe {
        (*rt.present_queue).store(queue, Ordering::Relaxed);
        vtable::patch(
            vtable_of(queue),
            EXECUTE_COMMAND_LISTS_SLOT,
            cvr_depth_execute_command_lists as *mut c_void,
        );
    }
}

// ---- the legacy cross-queue signal tracker (diagnostic A/B only)

#[derive(Clone, Copy)]
struct QueueSignal {
    fence: usize,
    value: u64,
    tick_ms: u64,
}
static SIGNALS: Mutex<Option<FastMap<usize, QueueSignal>>> = Mutex::new(None);
static HAVE_SIGNALS: AtomicBool = AtomicBool::new(false);
static SIGNAL_HOOKED: Mutex<Option<FastSet<usize>>> = Mutex::new(None);

fn clear_legacy_signals() {
    if !HAVE_SIGNALS.load(Ordering::Acquire) {
        return;
    }
    let mut signals = lock(&SIGNALS);
    if let Some(map) = signals.as_mut() {
        for entry in map.values() {
            if entry.fence != 0 {
                // SAFETY: the reference the tracker took.
                unsafe { release(entry.fence as *mut c_void) };
            }
        }
        map.clear();
    }
    HAVE_SIGNALS.store(false, Ordering::Release);
}

type SignalFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32;

/// HookedQueueSignal: the latest signal of each queue, while the diagnostic asks for it.
///
/// # Safety
/// Called by D3D12's callers through the patched slot.
#[no_mangle]
pub unsafe extern "system" fn cvr_depth_queue_signal(
    queue: *mut c_void,
    fence: *mut c_void,
    value: u64,
) -> i32 {
    // SAFETY: the queue's own method; the caller's fence.
    unsafe {
        let hr = match original::<SignalFn>(queue, SIGNAL_SLOT) {
            Some(f) => f(queue, fence, value),
            None => 0x8000_4005_u32 as i32, // E_FAIL
        };
        let Some(rt) = runtime() else { return hr };
        if (*rt.legacy_signal_tracking).load(Ordering::Relaxed) == 0 {
            clear_legacy_signals();
            return hr;
        }
        if hr >= 0 && !fence.is_null() {
            let mut signals = lock(&SIGNALS);
            let entry = signals
                .get_or_insert_with(cvr_host::fast::fast_map)
                .entry(queue as usize)
                .or_insert(QueueSignal {
                    fence: 0,
                    value: 0,
                    tick_ms: 0,
                });
            if entry.fence != fence as usize {
                if entry.fence != 0 {
                    release(entry.fence as *mut c_void);
                }
                add_ref(fence);
                entry.fence = fence as usize;
            }
            entry.value = value;
            entry.tick_ms = now_ms();
            HAVE_SIGNALS.store(true, Ordering::Release);
        }
        hr
    }
}

unsafe fn hook_queue_signal(queue: *mut c_void) {
    if queue.is_null() {
        return;
    }
    // SAFETY: a live queue.
    unsafe {
        let table = vtable_of(queue);
        if !lock(&SIGNAL_HOOKED)
            .get_or_insert_with(cvr_host::fast::fast_set)
            .insert(table as usize)
        {
            return;
        }
        vtable::patch(table, SIGNAL_SLOT, cvr_depth_queue_signal as *mut c_void);
    }
}

/// CyberpunkVRPort_WaitOnAllGameSignals: the consumer queue waits on every game queue's latest signal that is still
/// fresh (a queue the engine stopped feeding would hang the device), dropping the ones long gone.
///
/// # Safety
/// `consumer` is null or a live command queue.
#[no_mangle]
pub unsafe extern "C" fn cvr_depth_wait_on_all_game_signals(consumer: *mut c_void) {
    let Some(rt) = runtime() else { return };
    // SAFETY: the C++ half's flag; the tracker's own references; live queues and fences.
    unsafe {
        if consumer.is_null() || (*rt.legacy_signal_tracking).load(Ordering::Relaxed) == 0 {
            return;
        }
        let now = now_ms();
        let mut snapshot = Vec::new();
        {
            let mut signals = lock(&SIGNALS);
            if let Some(map) = signals.as_mut() {
                map.retain(|&queue, s| {
                    let age = now.saturating_sub(s.tick_ms);
                    if s.fence != 0 && age > SIGNAL_DEAD_MS {
                        release(s.fence as *mut c_void);
                        if diagnostics(rt) {
                            bump(rt.waits_dropped);
                        }
                        return false;
                    }
                    if queue != consumer as usize && s.fence != 0 {
                        if age <= SIGNAL_FRESH_MS {
                            add_ref(s.fence as *mut c_void);
                            snapshot.push(*s);
                        } else if diagnostics(rt) {
                            bump(rt.waits_skipped);
                        }
                    }
                    true
                });
            }
        }
        for s in snapshot {
            let fence = s.fence as *mut c_void;
            // ID3D12Fence::GetCompletedValue (slot 8); ID3D12CommandQueue::Wait (slot 15).
            let completed: unsafe extern "system" fn(*mut c_void) -> u64 =
                core::mem::transmute(*vtable_of(fence).add(8));
            if completed(fence) < s.value {
                let wait: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32 =
                    core::mem::transmute(*vtable_of(consumer).add(15));
                wait(consumer, fence, s.value);
                if diagnostics(rt) {
                    bump(rt.waits_issued);
                }
            } else if diagnostics(rt) {
                bump(rt.waits_skipped);
            }
            release(fence);
        }
    }
}

// ---- what the present and the submit path ask

/// The presented size for the pick, and the choice reopened only when the current pick has gone quiet (resetting
/// unconditionally let it alternate between MAIN's two same-size depth buffers).
#[no_mangle]
pub extern "C" fn cvr_depth_present(width: u32, height: u32, size_known: bool) {
    if size_known {
        PRESENT_WIDTH.store(width, Ordering::Relaxed);
        PRESENT_HEIGHT.store(height, Ordering::Relaxed);
    }
    if DEPTH_SEEN.swap(false, Ordering::Relaxed) {
        DEPTH_MISS_FRAMES.store(0, Ordering::Relaxed);
    } else if DEPTH_MISS_FRAMES.fetch_add(1, Ordering::Relaxed) >= 8 {
        DEPTH_MISS_FRAMES.store(0, Ordering::Relaxed);
        DEPTH_AREA.store(0, Ordering::Relaxed);
    }
}

#[no_mangle]
pub extern "C" fn cvr_depth_resource() -> *mut c_void {
    DEPTH.load(Ordering::Relaxed) as *mut c_void
}
/// The pick with a reference of the caller's own.
#[no_mangle]
pub extern "C" fn cvr_depth_acquire() -> *mut c_void {
    let _guard = lock(&DEPTH_REF);
    let depth = DEPTH.load(Ordering::Relaxed) as *mut c_void;
    if !depth.is_null() {
        // SAFETY: the slot's reference keeps it alive while the lock is held.
        unsafe { add_ref(depth) };
    }
    depth
}
#[no_mangle]
pub extern "C" fn cvr_depth_writer_queue() -> *mut c_void {
    WRITER_QUEUE.load(Ordering::Relaxed) as *mut c_void
}
#[no_mangle]
pub extern "C" fn cvr_depth_state() -> u32 {
    DEPTH_STATE.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_depth_width() -> u32 {
    DEPTH_WIDTH.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_depth_height() -> u32 {
    DEPTH_HEIGHT.load(Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn cvr_depth_format() -> u32 {
    DEPTH_FORMAT.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_depth_layer_formats_compete() {
        // The 16-bit shadow atlas never; the R32 and R32G8X24 families always.
        assert_eq!(candidate_area(53, 2048, 2048, (0, 0), false, false), 0);
        assert_eq!(
            candidate_area(19, 1920, 1080, (0, 0), false, false),
            1920 * 1080
        );
        assert_eq!(
            candidate_area(39, 1920, 1080, (0, 0), false, false),
            1920 * 1080
        );
        assert!(!resolvable_depth(45));
    }

    #[test]
    fn the_view_decides_when_known_else_the_presented_size() {
        // View key known: MAIN's only, whatever the size.
        assert_eq!(
            candidate_area(19, 2444, 2444, (1920, 1080), true, true),
            2444 * 2444
        );
        assert_eq!(candidate_area(19, 1920, 1080, (1920, 1080), true, false), 0);
        // Unknown: the presented size only, once it is known.
        assert_eq!(
            candidate_area(19, 2444, 2444, (1920, 1080), false, false),
            0
        );
        assert_eq!(
            candidate_area(19, 1920, 1080, (1920, 1080), false, false),
            1920 * 1080
        );
    }

    #[test]
    fn the_pick_reopens_after_eight_quiet_presents() {
        DEPTH_AREA.store(100, Ordering::Relaxed);
        DEPTH_SEEN.store(true, Ordering::Relaxed);
        cvr_depth_present(0, 0, false);
        assert_eq!(DEPTH_AREA.load(Ordering::Relaxed), 100);
        for _ in 0..9 {
            cvr_depth_present(4128, 2232, true);
        }
        assert_eq!(DEPTH_AREA.load(Ordering::Relaxed), 0);
        assert_eq!(PRESENT_WIDTH.load(Ordering::Relaxed), 4128);
    }
}
