//! The GPU queue timeline, a diagnostic: when each queue of the game actually runs its batches (formerly
//! src/Render/GpuQueueTimeline.cpp; include/Render/GpuQueueTimeline.hpp calls it; read by
//! tools/hitch_tests/gpu_queue_timeline.py).
//!
//! nvidia-smi says how much of the time the GPU is busy, not where it waits. Asked for through
//! CyberpunkVR_GpuTimelineRequest (a number of submissions), the next ExecuteCommandLists calls on the direct and
//! compute queues get two pre-recorded command lists added to the same call, each writing a GPU timestamp: one before
//! the game's lists, one after. With the CPU time of each call, the queues' clock calibration and the Present times, a
//! tool can tell for every gap on a queue whether the GPU had nothing to run (the batch was submitted after the queue
//! went idle) or was held by a fence. Nothing is installed in a normal run: the lists are created on the Present after
//! the first request (a one-off stall), and the hook does nothing while no capture is on.
//!
//! Records, slots and the capture count move under the C++ image submission mutex (cvr::camera::
//! ImageSubmissionMutex): the inner submission hook takes it too, and the XR submit holds it around its own
//! ExecuteCommandLists, so a mutex of our own taken first would deadlock against it.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};
use windows::core::{Interface, HRESULT, PCWSTR};
use windows::Win32::Foundation::{E_FAIL, HMODULE};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameA, GetModuleHandleExW, GetModuleHandleW,
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Memory::{
    VirtualProtect, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

const RECORDS: u32 = 16384;
/// Timed submissions per queue type.
const PAIRS: u32 = 4096;
/// Distinct queues per type.
const QUEUES: usize = 8;

fn log(text: &str) {
    cvr_host::log::write(text);
}

/// The C++ half: the exports, the image submission mutex and the swap chain's queue.
#[repr(C)]
pub struct Runtime {
    pub request: *const AtomicU32,
    /// 0 idle, 1 capturing, 2 waiting for the GPU, 3 done (records valid), 4 failed.
    pub state: *const AtomicU32,
    pub count: *const AtomicU32,
    /// Per queue type (0 direct, 1 compute): [t*3] timestamp frequency, [t*3+1] GPU timestamp and [t*3+2] QPC of one
    /// calibration; [6] QPC frequency.
    pub calib: *mut u64,
    /// Per record, 6 words: kind (0 direct, 1 compute, 2 Present) | lists << 8, queue pointer, QPC when the call came
    /// in, QPC when the original returned, GPU timestamp before the lists, GPU timestamp after them. Kinds 3 (queue
    /// Signal) and 4 (queue Wait): queue pointer, QPC, fence pointer, fence value, 0.
    pub records: *mut u64,
    pub lock: unsafe extern "C" fn(),
    pub unlock: unsafe extern "C" fn(),
    pub present_queue: unsafe extern "C" fn() -> *mut c_void,
}
// SAFETY: process-lifetime exports and plain functions.
unsafe impl Sync for Runtime {}
/// The plugin's table, built here (formerly the C++ half's); a test program attaches its own.
#[cfg(feature = "plugin-tables")]
mod plugin {
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
    unsafe extern "C" {
        static CyberpunkVR_GpuTimelineRequest: AtomicU32;
        static CyberpunkVR_GpuTimelineState: AtomicU32;
        static CyberpunkVR_GpuTimelineCount: AtomicU32;
        static mut CyberpunkVR_GpuTimelineCalib: [u64; 8];
        static mut CyberpunkVR_GpuTimelineRecords: [u64; 16384 * 6];
        fn cvr_image_submission_lock();
        fn cvr_image_submission_unlock();
        /// std::atomic<void*> g_presentQueue (src/Hooks/SwapChain.cpp).
        #[link_name = "?g_presentQueue@@3U?$atomic@PEAX@std@@A"]
        static g_presentQueue: AtomicPtr<c_void>;
    }
    unsafe extern "C" fn present_queue() -> *mut c_void {
        // SAFETY: the swapchain hooks' global.
        unsafe { g_presentQueue.load(Ordering::SeqCst) }
    }
    pub(super) static RT: super::Runtime = super::Runtime {
        request: &raw const CyberpunkVR_GpuTimelineRequest,
        state: &raw const CyberpunkVR_GpuTimelineState,
        count: &raw const CyberpunkVR_GpuTimelineCount,
        calib: (&raw mut CyberpunkVR_GpuTimelineCalib).cast(),
        records: (&raw mut CyberpunkVR_GpuTimelineRecords).cast(),
        lock: cvr_image_submission_lock,
        unlock: cvr_image_submission_unlock,
        present_queue,
    };
}
#[cfg(feature = "plugin-tables")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "plugin-tables"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `runtime` and everything it points to live for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_queue_timeline_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Ordering::Release);
}
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: the attached runtime.
    unsafe { RUNTIME.load(Ordering::Acquire).as_ref() }
}
/// The image submission mutex, held while alive.
struct Submission<'a>(&'a Runtime);
impl<'a> Submission<'a> {
    fn lock(r: &'a Runtime) -> Self {
        // SAFETY: the C++ recursive mutex's lock.
        unsafe { (r.lock)() };
        Submission(r)
    }
}
impl Drop for Submission<'_> {
    fn drop(&mut self) {
        // SAFETY: the lock this guard took.
        unsafe { (self.0.unlock)() };
    }
}
fn count(r: &Runtime) -> &AtomicU32 {
    // SAFETY: an attached export.
    unsafe { &*r.count }
}
fn state(r: &Runtime) -> &AtomicU32 {
    // SAFETY: an attached export.
    unsafe { &*r.state }
}
fn record_slot(r: &Runtime, record: u32) -> *mut u64 {
    // SAFETY: the attached RECORDS x 6 export; record < RECORDS.
    unsafe { r.records.add(record as usize * 6) }
}

type ExecuteFn = unsafe extern "system" fn(*mut c_void, u32, *const *mut c_void);
type SignalFn = unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> HRESULT;

#[derive(Default)]
struct TypeState {
    heap: Option<ID3D12QueryHeap>,
    readback: Option<ID3D12Resource>,
    allocator: Option<ID3D12CommandAllocator>,
    /// 2 per pair: before, after.
    lists: Vec<ID3D12GraphicsCommandList>,
    resolve: Option<ID3D12GraphicsCommandList>,
    /// Queues that ran timed submissions.
    queues: [usize; QUEUES],
    /// The first code outside this DLL that submitted, per queue.
    callers: [[usize; 4]; QUEUES],
    queue_fences: Vec<ID3D12Fence>,
    queue_fence_value: [u64; QUEUES],
    done: Option<ID3D12Fence>,
    done_value: u64,
    used: usize,
    next: u32,
    calibrated: bool,
}
/// Under the image submission mutex (or before a capture starts, on the Present thread).
struct Types(UnsafeCell<Option<[TypeState; 2]>>);
// SAFETY: every access is under the C++ image submission mutex or the Present thread before any capture.
unsafe impl Sync for Types {}
static TYPES: Types = Types(UnsafeCell::new(None));
unsafe fn types() -> &'static mut [TypeState; 2] {
    (*TYPES.0.get()).get_or_insert_with(Default::default)
}
static SLOTS: Types2 = Types2(UnsafeCell::new(None));
struct Types2(UnsafeCell<Option<Vec<u32>>>);
// SAFETY: as Types.
unsafe impl Sync for Types2 {}
/// Record -> pair index.
unsafe fn slots() -> &'static mut Vec<u32> {
    (*SLOTS.0.get()).get_or_insert_with(|| vec![0; RECORDS as usize])
}
static READY: AtomicBool = AtomicBool::new(false);
/// Submissions still to time.
static CAPTURE: AtomicU32 = AtomicU32::new(0);

fn qpc() -> u64 {
    let mut t = 0i64;
    // SAFETY: an out-parameter.
    let _ = unsafe { QueryPerformanceCounter(&mut t) };
    t as u64
}

// Queue Signal / Wait, recorded while a capture runs: which fence holds a held batch, and when its signal was issued.
// Patched into the queue vtables on the first request; outside a capture the hooks only forward.
struct SyncHook {
    table: AtomicUsize,
    signal: AtomicUsize,
    wait: AtomicUsize,
}
static SYNC_HOOKS: [SyncHook; 4] = [const {
    SyncHook {
        table: AtomicUsize::new(0),
        signal: AtomicUsize::new(0),
        wait: AtomicUsize::new(0),
    }
}; 4];
static SYNC_HOOK_COUNT: AtomicUsize = AtomicUsize::new(0);
unsafe fn sync_hook_of(queue: *mut c_void) -> Option<&'static SyncHook> {
    let table = *(queue as *const usize);
    SYNC_HOOKS[..SYNC_HOOK_COUNT.load(Ordering::Acquire)]
        .iter()
        .find(|h| h.table.load(Ordering::Relaxed) == table)
}
fn record_sync(kind: u64, queue: *mut c_void, fence: *mut c_void, value: u64) {
    let Some(r) = runtime() else { return };
    if CAPTURE.load(Ordering::Relaxed) == 0 {
        return;
    }
    let record = count(r).fetch_add(1, Ordering::SeqCst);
    if record >= RECORDS {
        count(r).store(RECORDS, Ordering::SeqCst);
        return;
    }
    let at = record_slot(r, record);
    // SAFETY: one record of the export.
    unsafe {
        core::ptr::copy_nonoverlapping(
            [kind, queue as u64, qpc(), fence as u64, value, 0].as_ptr(),
            at,
            6,
        )
    };
}
unsafe extern "system" fn timeline_signal(
    queue: *mut c_void,
    fence: *mut c_void,
    value: u64,
) -> HRESULT {
    let hook = sync_hook_of(queue);
    record_sync(3, queue, fence, value);
    match hook.map(|h| h.signal.load(Ordering::Relaxed)) {
        Some(f) if f != 0 => core::mem::transmute::<usize, SignalFn>(f)(queue, fence, value),
        _ => E_FAIL,
    }
}
unsafe extern "system" fn timeline_wait(
    queue: *mut c_void,
    fence: *mut c_void,
    value: u64,
) -> HRESULT {
    let hook = sync_hook_of(queue);
    record_sync(4, queue, fence, value);
    match hook.map(|h| h.wait.load(Ordering::Relaxed)) {
        Some(f) if f != 0 => core::mem::transmute::<usize, SignalFn>(f)(queue, fence, value),
        _ => E_FAIL,
    }
}
unsafe fn hook_sync(queue: *mut c_void) {
    if queue.is_null() || sync_hook_of(queue).is_some() {
        return;
    }
    let n = SYNC_HOOK_COUNT.load(Ordering::SeqCst);
    if n >= 4 {
        return;
    }
    let table = *(queue as *const *mut usize);
    let mut old = PAGE_PROTECTION_FLAGS(0);
    if VirtualProtect(
        table.add(14).cast(),
        2 * 8,
        PAGE_EXECUTE_READWRITE,
        &mut old,
    )
    .is_err()
    {
        return;
    }
    let hook = &SYNC_HOOKS[n];
    hook.table.store(table as usize, Ordering::Relaxed);
    hook.signal.store(*table.add(14), Ordering::Relaxed);
    hook.wait.store(*table.add(15), Ordering::Relaxed);
    SYNC_HOOK_COUNT.store(n + 1, Ordering::Release);
    *table.add(14) = timeline_signal as *const () as usize;
    *table.add(15) = timeline_wait as *const () as usize;
    let _ = VirtualProtect(table.add(14).cast(), 2 * 8, old, &mut old);
}

fn module_of(address: usize) -> usize {
    let mut module = HMODULE::default();
    // SAFETY: an address lookup.
    let _ = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(address as *const u16),
            &mut module,
        )
    };
    module.0 as usize
}
/// Who submits on each queue: the first return address outside this DLL and the D3D12 runtime.
fn note_caller(s: &mut TypeState, q: usize) {
    let own = module_of(module_of as *const () as usize);
    // SAFETY: a module name lookup.
    let d3d12 =
        unsafe { GetModuleHandleW(windows::core::w!("d3d12.dll")) }.map_or(0, |m| m.0 as usize);
    let mut frames = [core::ptr::null_mut::<c_void>(); 16];
    // SAFETY: the frames buffer.
    let n = unsafe { RtlCaptureStackBackTrace(1, &mut frames, None) } as usize;
    let caller = frames[..n]
        .iter()
        .map(|&f| f as usize)
        .find(|&f| {
            let m = module_of(f);
            m != 0 && m != own && m != d3d12
        })
        .unwrap_or(if n != 0 { frames[n - 1] as usize } else { 0 });
    for slot in s.callers[q].iter_mut() {
        if *slot == caller {
            return;
        }
        if *slot == 0 {
            *slot = caller;
            return;
        }
    }
}
unsafe fn log_callers() {
    for (t, s) in types().iter().enumerate() {
        for q in 0..s.used {
            let Some(queue) =
                ID3D12CommandQueue::from_raw_borrowed(&(s.queues[q] as *mut c_void)).cloned()
            else {
                continue;
            };
            let desc = queue.GetDesc();
            let mut line = format!(
                "[gpu-timeline] {} queue {:016X} priority {} flags {}:",
                if t != 0 { "compute" } else { "direct" },
                s.queues[q],
                desc.Priority,
                desc.Flags.0 as u32
            );
            for &caller in s.callers[q].iter().filter(|&&c| c != 0) {
                if line.len() >= 512 - 80 {
                    continue;
                }
                let module = module_of(caller);
                let mut name = [0u8; 260];
                let len = if module != 0 {
                    GetModuleFileNameA(Some(HMODULE(module as *mut c_void)), &mut name) as usize
                } else {
                    0
                };
                let path = if len != 0 {
                    String::from_utf8_lossy(&name[..len]).into_owned()
                } else {
                    "?".to_owned()
                };
                let base = path.rsplit('\\').next().unwrap_or(&path);
                line.push_str(&format!(" {base}+{:#x}", caller.wrapping_sub(module)));
            }
            log(&format!("{line}\n"));
        }
    }
}
unsafe fn type_index(queue: &ID3D12CommandQueue) -> Option<usize> {
    match queue.GetDesc().Type {
        D3D12_COMMAND_LIST_TYPE_DIRECT => Some(0),
        D3D12_COMMAND_LIST_TYPE_COMPUTE => Some(1),
        _ => None,
    }
}
unsafe fn prepare(device: &ID3D12Device) -> windows::core::Result<()> {
    for (t, s) in types().iter_mut().enumerate() {
        let kind = if t == 0 {
            D3D12_COMMAND_LIST_TYPE_DIRECT
        } else {
            D3D12_COMMAND_LIST_TYPE_COMPUTE
        };
        let heap_desc = D3D12_QUERY_HEAP_DESC {
            Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
            Count: PAIRS * 2,
            NodeMask: 0,
        };
        let buffer = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: PAIRS as u64 * 2 * 8,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let readback = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let mut heap: Option<ID3D12QueryHeap> = None;
        device.CreateQueryHeap(&heap_desc, &mut heap)?;
        let heap = heap.ok_or(windows::core::Error::from(E_FAIL))?;
        let mut target: Option<ID3D12Resource> = None;
        device.CreateCommittedResource(
            &readback,
            D3D12_HEAP_FLAG_NONE,
            &buffer,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut target,
        )?;
        let target = target.ok_or(windows::core::Error::from(E_FAIL))?;
        let allocator: ID3D12CommandAllocator = device.CreateCommandAllocator(kind)?;
        s.done = Some(device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?);
        s.queue_fences = (0..QUEUES)
            .map(|_| device.CreateFence(0, D3D12_FENCE_FLAG_NONE))
            .collect::<windows::core::Result<_>>()?;
        s.lists.clear();
        for i in 0..PAIRS * 2 {
            let list: ID3D12GraphicsCommandList =
                device.CreateCommandList(0, kind, &allocator, None)?;
            list.EndQuery(&heap, D3D12_QUERY_TYPE_TIMESTAMP, i);
            list.Close()?;
            s.lists.push(list);
        }
        let resolve: ID3D12GraphicsCommandList =
            device.CreateCommandList(0, kind, &allocator, None)?;
        resolve.ResolveQueryData(&heap, D3D12_QUERY_TYPE_TIMESTAMP, 0, PAIRS * 2, &target, 0);
        resolve.Close()?;
        s.heap = Some(heap);
        s.readback = Some(target);
        s.allocator = Some(allocator);
        s.resolve = Some(resolve);
    }
    Ok(())
}
/// Under the submission mutex, the capture over: the first queue of each type waits for the others and resolves.
unsafe fn finish(r: &Runtime, original: ExecuteFn) {
    for s in types().iter_mut() {
        if s.used == 0 {
            continue;
        }
        let queue = |q: usize| {
            ID3D12CommandQueue::from_raw_borrowed(&(s.queues[q] as *mut c_void)).cloned()
        };
        let Some(resolver) = queue(0) else { continue };
        for q in 1..s.used {
            s.queue_fence_value[q] += 1;
            if let Some(other) = queue(q) {
                let _ = other.Signal(&s.queue_fences[q], s.queue_fence_value[q]);
                let _ = resolver.Wait(&s.queue_fences[q], s.queue_fence_value[q]);
            }
        }
        if let Some(resolve) = s.resolve.as_ref() {
            let lists = [resolve.as_raw()];
            original(resolver.as_raw(), 1, lists.as_ptr());
        }
        s.done_value += 1;
        if let Some(done) = s.done.as_ref() {
            let _ = resolver.Signal(done, s.done_value);
        }
    }
    state(r).store(2, Ordering::SeqCst);
}

/// Execute: the submission run with its two stamps when a capture is on (true); false leaves it to the caller.
///
/// # Safety
/// The arguments are ExecuteCommandLists'; `original` is the original.
#[no_mangle]
pub unsafe extern "C" fn cvr_queue_timeline_execute(
    queue: *mut c_void,
    n: u32,
    lists: *const *mut c_void,
    original: Option<ExecuteFn>,
) -> bool {
    let Some(r) = runtime() else { return false };
    let Some(original) = original else {
        return false;
    };
    if CAPTURE.load(Ordering::Relaxed) == 0 || lists.is_null() || n == 0 {
        return false;
    }
    let Some(command_queue) = ID3D12CommandQueue::from_raw_borrowed(&queue) else {
        return false;
    };
    let Some(t) = type_index(command_queue) else {
        return false;
    };
    let _lock = Submission::lock(r);
    if CAPTURE.load(Ordering::Relaxed) == 0 {
        return false;
    }
    let s = &mut types()[t];
    let q = (0..s.used)
        .find(|&q| s.queues[q] == queue as usize)
        .unwrap_or(s.used);
    if s.next >= PAIRS
        || count(r).load(Ordering::SeqCst) + 8 >= RECORDS
        || (q == s.used && q == QUEUES)
    {
        CAPTURE.store(0, Ordering::SeqCst);
        finish(r, original);
        return false;
    }
    if q == s.used {
        s.queues[s.used] = queue as usize;
        s.used += 1;
        hook_sync(queue);
    }
    note_caller(s, q);
    if !s.calibrated {
        let (mut gpu, mut cpu) = (0u64, 0u64);
        if let (Ok(frequency), Ok(())) = (
            command_queue.GetTimestampFrequency(),
            command_queue.GetClockCalibration(&mut gpu, &mut cpu),
        ) {
            *r.calib.add(t * 3) = frequency;
            *r.calib.add(t * 3 + 1) = gpu;
            *r.calib.add(t * 3 + 2) = cpu;
        }
        s.calibrated = true;
    }
    let pair = s.next;
    s.next += 1;
    // One call as the game made it, with the two stamps around its lists.
    let mut all = Vec::with_capacity(n as usize + 2);
    all.push(s.lists[pair as usize * 2].as_raw());
    all.extend((0..n as usize).map(|i| *lists.add(i)));
    all.push(s.lists[pair as usize * 2 + 1].as_raw());
    let call = qpc();
    original(queue, n + 2, all.as_ptr());
    let ret = qpc();
    let record = count(r).fetch_add(1, Ordering::SeqCst);
    if record < RECORDS {
        let at = record_slot(r, record);
        core::ptr::copy_nonoverlapping(
            [t as u64 | ((n as u64) << 8), queue as u64, call, ret, 0, 0].as_ptr(),
            at,
            6,
        );
        slots()[record as usize] = pair;
    }
    if CAPTURE.fetch_sub(1, Ordering::SeqCst) == 1 {
        finish(r, original);
    }
    true
}
/// Present: once per game Present, the frame boundary marked and the capture moved along.
#[no_mangle]
pub extern "C" fn cvr_queue_timeline_present() {
    let Some(r) = runtime() else { return };
    // SAFETY: the timeline's state under the submission mutex, or the Present thread before a capture starts.
    unsafe {
        if CAPTURE.load(Ordering::Relaxed) != 0 {
            let _lock = Submission::lock(r);
            if CAPTURE.load(Ordering::Relaxed) != 0 {
                let record = count(r).fetch_add(1, Ordering::SeqCst);
                if record < RECORDS {
                    let now = qpc();
                    core::ptr::copy_nonoverlapping(
                        [2, 0, now, now, 0, 0].as_ptr(),
                        record_slot(r, record),
                        6,
                    );
                    slots()[record as usize] = 0;
                }
            }
            return;
        }
        let current = state(r).load(Ordering::SeqCst);
        if current == 1 {
            // The last submission is in its finish.
            return;
        }
        if current == 2 {
            for s in types().iter() {
                if s.used != 0
                    && s.done
                        .as_ref()
                        .is_some_and(|d| d.GetCompletedValue() < s.done_value)
                {
                    return;
                }
            }
            // The GPU timestamps filled in from the readbacks.
            let mut n = count(r).load(Ordering::SeqCst);
            if n > RECORDS {
                n = RECORDS;
                count(r).store(n, Ordering::SeqCst);
            }
            let mut stamps: [*const u64; 2] = [core::ptr::null(); 2];
            for (t, s) in types().iter().enumerate() {
                let mut data: *mut c_void = core::ptr::null_mut();
                let range = D3D12_RANGE {
                    Begin: 0,
                    End: PAIRS as usize * 2 * 8,
                };
                if s.used != 0
                    && s.readback
                        .as_ref()
                        .is_some_and(|b| b.Map(0, Some(&range), Some(&mut data)).is_ok())
                {
                    stamps[t] = data as *const u64;
                }
            }
            for i in 0..n {
                let rec = record_slot(r, i);
                let kind = (*rec & 0xFF) as usize;
                if kind > 1 || stamps[kind].is_null() {
                    continue;
                }
                let slot = slots()[i as usize] as usize;
                *rec.add(4) = *stamps[kind].add(slot * 2);
                *rec.add(5) = *stamps[kind].add(slot * 2 + 1);
            }
            for (t, s) in types().iter().enumerate() {
                if !stamps[t].is_null() {
                    if let Some(b) = s.readback.as_ref() {
                        b.Unmap(0, Some(&D3D12_RANGE::default()));
                    }
                }
            }
            state(r).store(3, Ordering::SeqCst);
            let used = |t: usize| types()[t].used;
            log(&format!(
                "[gpu-timeline] {n} records captured (direct queues {}, compute queues {})\n",
                used(0),
                used(1)
            ));
            log_callers();
            return;
        }
        let want = (*r.request).swap(0, Ordering::SeqCst);
        if want == 0 {
            return;
        }
        let present = (r.present_queue)();
        if !READY.load(Ordering::SeqCst) {
            let device = ID3D12CommandQueue::from_raw_borrowed(&present).and_then(|q| {
                let mut device: Option<ID3D12Device> = None;
                q.GetDevice(&mut device).ok().and(device)
            });
            if device.is_none_or(|d| prepare(&d).is_err()) {
                state(r).store(4, Ordering::SeqCst);
                log("[gpu-timeline] could not create the timestamp lists\n");
                return;
            }
            READY.store(true, Ordering::SeqCst);
        }
        hook_sync(present);
        let mut frequency = 0i64;
        let _ = QueryPerformanceFrequency(&mut frequency);
        *r.calib.add(6) = frequency as u64;
        let _lock = Submission::lock(r);
        for s in types().iter_mut() {
            s.next = 0;
            s.used = 0;
            s.calibrated = false;
            s.callers = [[0; 4]; QUEUES];
        }
        count(r).store(0, Ordering::SeqCst);
        state(r).store(1, Ordering::SeqCst);
        CAPTURE.store(want, Ordering::SeqCst);
    }
}
