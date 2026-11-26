//! THE SCENE ACCESS TRACE'S OBSERVER FRONT (formerly src/Render/NativeSceneAccess.cpp): the observer lock, the epoch an
//! observation belongs to, each call's attribution (the list's current scene child and recording, unless the port's own
//! commands record), the sequence counter around every write with the stats row published, the breadcrumb write's
//! destination resolved through breadcrumb_capture.rs, and the failure paths (an observation that panics fails the
//! trace; one that cannot even start makes the observer unavailable). The trace itself is scene_access_observer.rs'.
//!
//! The C++ API (include/Render/NativeSceneAccess.hpp) forwards here; the scene child's description and the bound targets
//! (still C++, NativeDefinitionTracking.cpp and StereoSceneState.cpp) come in through the attached host table.
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering::{AcqRel, Acquire, Release, SeqCst}};
use std::sync::{Mutex, MutexGuard};

use windows::core::{IUnknown, Interface};
use windows::Win32::Graphics::Direct3D12::*;

use crate::scene_access_observer::{abi, cvr_sa_counters, At, Counters};

#[allow(non_upper_case_globals)]
extern "C" {
    /// The trace's write sequence (odd while a write is in flight), read by the tools.
    static CyberpunkVR_SceneAccessSeq: AtomicU32;
}

/// What the C++ half lends.
#[repr(C)]
pub struct Host {
    /// scene_child::DescribeCurrentAtList: into the zeroed `*out` the list's current child and recording (the token an
    /// owned reference, null when none); whether it matched.
    pub at: unsafe extern "C" fn(*mut c_void, *mut At) -> bool,
    /// scene_state::CaptureBoundResources: the bound render targets and depth as nine owned references (null when none).
    pub bound: unsafe extern "C" fn(u32, *const D3D12_CPU_DESCRIPTOR_HANDLE, i32, *const D3D12_CPU_DESCRIPTOR_HANDLE, *mut [u64; 9]) -> bool,
}
static HOST: AtomicPtr<Host> = AtomicPtr::new(core::ptr::null_mut());
/// The C++ half's host table (static storage).
///
/// # Safety
/// `host` lives for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sao_attach(host: *const Host) {
    HOST.store(host as *mut Host, SeqCst);
}
/// The plugin's own host (formerly lent by src/Render/NativeSceneAccess.cpp). The Rust test variants attach theirs.
#[cfg(feature = "rust-build")]
mod plugin {
    use super::{At, Host};
    use crate::native_recording::{Context, Lease};
    use core::ffi::c_void;
    use windows::Win32::Graphics::Direct3D12::D3D12_CPU_DESCRIPTOR_HANDLE;
    fn validate(lease: &Lease, list: *mut c_void, context: &Context) -> bool {
        // SAFETY: the records and the live list.
        unsafe { crate::native_recording::cvr_native_recording_validate_recording(lease, list, context) }
    }
    fn current(base: u64) -> Context {
        let c = crate::camera_upload::current(base);
        Context { context: c.context, state: c.state, list: c.list }
    }
    /// scene_child::DescribeCurrentAtList: the current child described before and after the list's recording is captured
    /// and validated (the context and the recording checked again); into `*out` the child and the recording (its token
    /// an owned reference). False leaves `*out` as it was (zeroed).
    unsafe extern "C" fn at(list: *mut c_void, out: *mut At) -> bool {
        use crate::definition_tracking::scene_child::describe_current;
        let Some(before) = describe_current() else { return false };
        if list.is_null() {
            return false;
        }
        // SAFETY: the module base, written once at load.
        let base = unsafe { core::ptr::read_volatile(&raw const crate::stereo_shared::g_exe_base) } as u64;
        let context = current(base);
        let mut lease = Lease { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 };
        // SAFETY: the live list, the context and a writable lease (its token ours).
        let captured = context.list == list as u64 && unsafe { crate::native_recording::cvr_native_recording_capture(list, &context, &mut lease) };
        let ok = captured
            && validate(&lease, list, &context)
            && describe_current() == Some(before)
            && current(base) == context
            && validate(&lease, list, &context);
        if !ok {
            crate::structured_snapshot_receipt::release(lease.token);
            return false;
        }
        // SAFETY: the caller's zeroed At; the same 136-byte scene_child::Identity.
        unsafe {
            let out = &mut *out;
            out.child = core::mem::transmute_copy(&before);
            out.generation = lease.generation;
            out.context = lease.context;
            out.list = lease.list;
            out.registry = lease.registry;
            out.token = lease.token as u64;
        }
        true
    }
    /// scene_state::CaptureBoundResources: nine owned references (null when none).
    unsafe extern "C" fn bound(count: u32, colors: *const D3D12_CPU_DESCRIPTOR_HANDLE, contiguous: i32, depth: *const D3D12_CPU_DESCRIPTOR_HANDLE, out: *mut [u64; 9]) -> bool {
        let mut raw = [0usize; 9];
        // SAFETY: the call's descriptor arrays (a handle is its address) and a local output.
        let known = unsafe { cvr_gpu::scene_state::cvr_scene_capture_bound_resources(count, colors.cast(), contiguous != 0, depth.cast(), &mut raw) };
        // SAFETY: the caller's output takes the references.
        unsafe { *out = raw.map(|r| r as u64) };
        known
    }
    pub static HOST: Host = Host { at, bound };
}
fn host() -> Option<&'static Host> {
    // SAFETY: attached once with static storage.
    let attached = unsafe { HOST.load(SeqCst).as_ref() };
    #[cfg(feature = "rust-build")]
    let attached = attached.or(Some(&plugin::HOST));
    attached
}

/// The observer lock (the trace's state is only touched under it).
static LOCK: Mutex<()> = Mutex::new(());
fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
fn counters() -> &'static Counters {
    // SAFETY: the trace's counters live for the process; their plain fields are read under the lock.
    unsafe { &*cvr_sa_counters() }
}
fn active() -> bool {
    counters().active.load(Acquire)
}
/// One write: the sequence odd while it lasts, the stats row published before it turns even.
struct Write;
impl Write {
    fn new() -> Self {
        // SAFETY: the export lives for the process.
        unsafe { CyberpunkVR_SceneAccessSeq.fetch_add(1, AcqRel) };
        Write
    }
}
impl Drop for Write {
    fn drop(&mut self) {
        abi::cvr_sa_publish_stats();
        // SAFETY: as above.
        unsafe { CyberpunkVR_SceneAccessSeq.fetch_add(1, Release) };
    }
}
fn release(object: u64) {
    if object != 0 {
        // SAFETY: a reference the host handed over, released once.
        drop(unsafe { IUnknown::from_raw(object as *mut c_void) });
    }
}
fn capture_epoch() -> u64 {
    let _guard = lock();
    abi::cvr_sa_capture_epoch()
}
/// A call's attribution (its recording token an owned reference).
struct Attribution(At);
impl Drop for Attribution {
    fn drop(&mut self) {
        release(self.0.token);
    }
}
fn attribution(list: *mut c_void) -> Attribution {
    let mut at = At::default();
    if !cvr_host::probe::internal() {
        if let Some(h) = host() {
            // SAFETY: the host's own function with a live list and a zeroed At.
            at.matched = unsafe { (h.at)(list, &mut at) };
        }
    }
    Attribution(at)
}
fn epoch_is(epoch: u64) -> bool {
    active() && counters().epoch == epoch
}
/// Observe: the call attributed and handed to `f` under the lock, inside its write, if the trace is still the epoch's.
fn observe(list: *mut c_void, expected: u64, f: impl FnOnce(u64, &At)) {
    if !active() {
        return;
    }
    let mut epoch = expected;
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if epoch == 0 {
            epoch = capture_epoch();
        }
        if epoch == 0 {
            return;
        }
        let attributed = attribution(list);
        let _guard = lock();
        if !epoch_is(epoch) {
            return;
        }
        let _write = Write::new();
        let mut at = attributed.0;
        at.internal = cvr_host::probe::internal();
        f(list as u64, &at);
    }));
    if ran.is_err() {
        let _guard = lock();
        if !epoch_is(epoch) {
            return;
        }
        let _write = Write::new();
        abi::cvr_sa_fail(5);
    }
}
/// The epoch for a call that resolves things before it observes (a failure to read it makes the observer unavailable).
fn start_epoch() -> Option<u64> {
    match std::panic::catch_unwind(capture_epoch) {
        Ok(0) => None,
        Ok(epoch) => Some(epoch),
        Err(_) => {
            cvr_sao_unavailable();
            None
        }
    }
}
fn bound(count: u32, colors: *const D3D12_CPU_DESCRIPTOR_HANDLE, contiguous: i32, depth: *const D3D12_CPU_DESCRIPTOR_HANDLE) -> (bool, [u64; 9]) {
    let mut held = [0u64; 9];
    // SAFETY: the host's own function with the call's descriptor arrays and a local output.
    let known = host().is_some_and(|h| unsafe { (h.bound)(count, colors, contiguous, depth, &mut held) });
    (known, held)
}
fn release_all(held: [u64; 9]) {
    held.into_iter().for_each(release);
}

// ---- the C++ API

/// Active.
#[no_mangle]
pub extern "C-unwind" fn cvr_sao_active() -> bool {
    active()
}
/// Arm.
#[no_mangle]
pub extern "C-unwind" fn cvr_sao_arm() {
    let ran = std::panic::catch_unwind(|| {
        let _guard = lock();
        let _write = Write::new();
        abi::cvr_sa_arm();
    });
    if ran.is_err() {
        counters().active.store(false, SeqCst);
    }
}
/// Finish.
#[no_mangle]
pub extern "C-unwind" fn cvr_sao_finish(aborted: bool) {
    let ran = std::panic::catch_unwind(|| {
        let _guard = lock();
        if counters().finished {
            return;
        }
        let _write = Write::new();
        abi::cvr_sa_finish(aborted);
    });
    if ran.is_err() {
        counters().active.store(false, SeqCst);
        let _guard = lock();
        let _write = Write::new();
        abi::cvr_sa_fail(5);
    }
}
/// ObserverUnavailable.
#[no_mangle]
pub extern "C-unwind" fn cvr_sao_unavailable() {
    counters().unavailable.store(true, SeqCst);
    if active() {
        let _guard = lock();
        let _write = Write::new();
        abi::cvr_sa_fail(6);
    }
}
/// Reset.
#[no_mangle]
pub extern "C-unwind" fn cvr_sao_reset(list: *mut c_void) {
    if !active() {
        return;
    }
    let ran = std::panic::catch_unwind(|| {
        let _guard = lock();
        if !active() {
            return;
        }
        let _write = Write::new();
        abi::cvr_sa_reset(list as u64);
    });
    if ran.is_err() {
        cvr_sao_unavailable();
    }
}
/// Close.
#[no_mangle]
pub extern "C-unwind" fn cvr_sao_close(list: *mut c_void, result: i32) {
    observe(list, 0, |value, _| abi::cvr_sa_close(value, result));
}
/// Operand.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C-unwind" fn cvr_sao_operand(list: *mut c_void, resource: *mut c_void, write: bool, tag: u32, subresource: u32, offset: u64, bytes: u64) {
    // SAFETY: the attribution lives through the call.
    observe(list, 0, |value, at| unsafe { abi::cvr_sa_operand(value, at, resource as u64, write, tag, subresource, offset, bytes) });
}
/// ImmediateWrite: the breadcrumb write's destination resolved (its buffer's offset, when it is one), then observed.
///
/// # Safety
/// `list` is a live ID3D12GraphicsCommandList2 or null; `parameters` and `modes` are the call's arrays (read guarded).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sao_immediate(list: *mut c_void, count: u32, parameters: u64, modes: u64, caller: u64) {
    if count == 0 || !active() {
        return;
    }
    let Some(epoch) = start_epoch() else {
        return;
    };
    let resolved = std::panic::catch_unwind(|| {
        let mut snapshot = crate::breadcrumb_capture::Snapshot::default();
        let mut found = None;
        // SAFETY: as documented; the captured state is released once below.
        let data = unsafe { crate::breadcrumb_capture::cvr_bcw_capture(list, count, parameters, modes, caller) };
        if !data.is_null() {
            // SAFETY: the live captured state and a default snapshot.
            if unsafe { crate::breadcrumb_capture::cvr_bcw_describe(data, &mut snapshot) }
                && snapshot.resource_desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
            {
                if let Some(resource) = &snapshot.creation.resource {
                    // SAFETY: a live resource the snapshot owns.
                    let (desc, gpu) = unsafe { (resource.GetDesc(), resource.GetGPUVirtualAddress()) };
                    let destination = snapshot.destination;
                    if count == 1
                        && desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER
                        && desc.Width == snapshot.resource_desc.Width
                        && desc.Width >= 4
                        && gpu != 0
                        && gpu <= u64::MAX - desc.Width
                        && destination >= gpu
                        && destination & 3 == 0
                        && destination - gpu <= desc.Width - 4
                    {
                        found = Some(destination - gpu);
                    }
                }
            }
            // SAFETY: as above.
            unsafe { crate::breadcrumb_capture::cvr_bcw_release(data) };
        }
        (snapshot, found)
    });
    let (snapshot, found) = resolved.unwrap_or_default();
    let creation = &snapshot.creation;
    let address = |o: Option<*mut c_void>| o.map_or(0, |p| p as u64);
    let (resource, heap) = (address(creation.resource.as_ref().map(|r| r.as_raw())), address(creation.heap.as_ref().map(|h| h.as_raw())));
    observe(list, epoch, |value, at| {
        // SAFETY: the snapshot keeps its resource and heap through the call.
        unsafe {
            abi::cvr_sa_immediate(
                value,
                at,
                found.is_some(),
                found.unwrap_or(0),
                resource,
                heap,
                creation.offset,
                creation.bytes,
                creation.kind,
                snapshot.resource_desc.Flags.0 as u32,
            )
        }
    });
}
/// Barriers.
///
/// # Safety
/// `barriers` holds `count` barriers or is null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sao_barriers(list: *mut c_void, count: u32, barriers: *const D3D12_RESOURCE_BARRIER) {
    // SAFETY: as documented.
    observe(list, 0, |value, at| unsafe { abi::cvr_sa_barriers(value, at, count, barriers) });
}
/// Targets: the bound render targets and depth observed.
///
/// # Safety
/// `colors` holds `count` handles (or one when contiguous); `depth` is a handle or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sao_targets(
    list: *mut c_void,
    count: u32,
    colors: *const D3D12_CPU_DESCRIPTOR_HANDLE,
    contiguous: i32,
    depth: *const D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    if !active() {
        return;
    }
    let Some(epoch) = start_epoch() else {
        return;
    };
    let (known, held) = bound(count, colors, contiguous, depth);
    // SAFETY: the held references live through the call.
    observe(list, epoch, |value, at| unsafe { abi::cvr_sa_targets(value, at, known, &held) });
    release_all(held);
}
/// ClearView: the cleared view's resource observed.
#[no_mangle]
pub extern "C-unwind" fn cvr_sao_clear_view(list: *mut c_void, view: D3D12_CPU_DESCRIPTOR_HANDLE, depth: bool) {
    if !active() {
        return;
    }
    let Some(epoch) = start_epoch() else {
        return;
    };
    let handle: *const D3D12_CPU_DESCRIPTOR_HANDLE = &view;
    let (known, held) = if depth { bound(0, core::ptr::null(), 0, handle) } else { bound(1, handle, 0, core::ptr::null()) };
    // SAFETY: the held references live through the call.
    observe(list, epoch, |value, at| unsafe { abi::cvr_sa_clear_view(value, at, known, &held) });
    release_all(held);
}
/// Shader.
#[no_mangle]
pub extern "C-unwind" fn cvr_sao_shader(list: *mut c_void, graphics: bool) {
    // SAFETY: the attribution lives through the call.
    observe(list, 0, |value, at| unsafe { abi::cvr_sa_shader(value, at, graphics) });
}
/// Unknown.
#[no_mangle]
pub extern "C-unwind" fn cvr_sao_unknown(list: *mut c_void, tag: u32) {
    // SAFETY: as above.
    observe(list, 0, |value, at| unsafe { abi::cvr_sa_unknown(value, at, tag) });
}
