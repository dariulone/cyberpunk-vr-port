//! The bounded GPU stage profile (formerly src/Render/GpuStageProfile.cpp): on request (CyberpunkVR_GpuStageRequest,
//! at most 32 batches), each capture/submit/HUD command list records a timestamp pair per stage into a query heap and
//! readback buffer of its own -- kept alive with the list's recording, so neither a list reset nor another recording
//! overwrites an in-flight slot -- and Present maps only completed readbacks into CyberpunkVR_GpuStageReport.
//! Unarmed, it makes no D3D call and no allocation.
//!
//! The switch, the report and its sequence stay exported by the C++ half, which attaches them here at its static
//! initialisation (until then they read idle).
use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use cvr_crt::{format, Arg};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

/// Stage: CaptureMain, CaptureSecond, CaptureDepth, SubmitColor, SubmitDepth, SecondHud, SecondBlit, HudSprites,
/// HudBlur, HudStyle, HudSubmit.
const NAMES: [&core::ffi::CStr; 11] = [
    c"CaptureMain",
    c"CaptureSecond",
    c"CaptureDepth",
    c"SubmitColor",
    c"SubmitDepth",
    c"SecondHud",
    c"SecondBlit",
    c"HudSprites",
    c"HudBlur",
    c"HudStyle",
    c"HudSubmit",
];
const STAGES: usize = NAMES.len();
const MAX_PAIRS: usize = 16;

// ---- the exports (Render/GpuStageProfile.cpp)

static DETACHED_REQUEST: AtomicU32 = AtomicU32::new(0);
static DETACHED_SEQ: AtomicU32 = AtomicU32::new(0);
static REQUEST: AtomicPtr<AtomicU32> = AtomicPtr::new(core::ptr::null_mut());
static SEQ: AtomicPtr<AtomicU32> = AtomicPtr::new(core::ptr::null_mut());
static REPORT: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
static REPORT_LEN: AtomicUsize = AtomicUsize::new(0);

fn exported(p: &AtomicPtr<AtomicU32>, detached: &'static AtomicU32) -> &'static AtomicU32 {
    let p = p.load(Ordering::Acquire);
    if p.is_null() {
        detached
    } else {
        // SAFETY: an exported std::atomic<uint32_t>, alive for the process.
        unsafe { &*p }
    }
}
fn request() -> &'static AtomicU32 {
    exported(&REQUEST, &DETACHED_REQUEST)
}
fn seq() -> &'static AtomicU32 {
    exported(&SEQ, &DETACHED_SEQ)
}

/// The C++ half's exports: CyberpunkVR_GpuStageRequest, CyberpunkVR_GpuStageReportSeq, CyberpunkVR_GpuStageReport.
///
/// # Safety
/// The pointers live for the process; `report` is `report_len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stage_attach(request: *const AtomicU32, seq: *const AtomicU32, report: *mut u8, report_len: usize) {
    REPORT_LEN.store(report_len, Ordering::Relaxed);
    REPORT.store(report, Ordering::Relaxed);
    SEQ.store(seq as *mut AtomicU32, Ordering::Relaxed);
    REQUEST.store(request as *mut AtomicU32, Ordering::Release);
}

// ---- the profile

static BUDGET: AtomicU32 = AtomicU32::new(0);
static OUTSTANDING: AtomicU32 = AtomicU32::new(0);
static CANCELLED: AtomicU32 = AtomicU32::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Default)]
struct Statistic {
    count: u32,
    total: f64,
    peak: f64,
}
struct Profile {
    statistics: [Statistic; STAGES],
    completed: u32,
    invalid: u32,
    pending: Vec<Arc<Batch>>,
}
static PROFILE: Mutex<Profile> = Mutex::new(Profile {
    statistics: [Statistic {
        count: 0,
        total: 0.0,
        peak: 0.0,
    }; STAGES],
    completed: 0,
    invalid: 0,
    pending: Vec::new(),
});
fn profile() -> MutexGuard<'static, Profile> {
    PROFILE.lock().unwrap_or_else(|p| p.into_inner())
}

#[derive(Clone, Copy, Default)]
struct Pair {
    stage: u32,
    ended: bool,
}
struct Recording {
    /// The list, used only while recording (no reference of its own, as the C++ kept it).
    list: *mut c_void,
    heap: Option<ID3D12QueryHeap>,
    readback: Option<ID3D12Resource>,
    fence: Option<ID3D12Fence>,
    value: u64,
    frequency: u64,
    pairs: [Pair; MAX_PAIRS],
    used: usize,
    submitted: bool,
}
/// Batch: one list's timestamp pairs.
pub struct Batch {
    counted: AtomicBool,
    recording: Mutex<Recording>,
}
// SAFETY: free-threaded D3D12 objects under the mutex.
unsafe impl Send for Batch {}
unsafe impl Sync for Batch {}
impl Default for Recording {
    fn default() -> Self {
        Recording {
            list: core::ptr::null_mut(),
            heap: None,
            readback: None,
            fence: None,
            value: 0,
            frequency: 0,
            pairs: [Pair::default(); MAX_PAIRS],
            used: 0,
            submitted: false,
        }
    }
}
impl Batch {
    fn lock(&self) -> MutexGuard<'_, Recording> {
        self.recording.lock().unwrap_or_else(|p| p.into_inner())
    }
}
impl Drop for Batch {
    fn drop(&mut self) {
        if self.counted.load(Ordering::Relaxed) {
            if !self.recording.get_mut().unwrap_or_else(|p| p.into_inner()).submitted {
                CANCELLED.fetch_add(1, Ordering::Relaxed);
            }
            OUTSTANDING.fetch_sub(1, Ordering::Release);
        }
    }
}

fn publish(p: &Profile) {
    let report = REPORT.load(Ordering::Relaxed);
    let len = REPORT_LEN.load(Ordering::Relaxed);
    seq().fetch_add(1, Ordering::AcqRel);
    let (budget, outstanding) = (BUDGET.load(Ordering::SeqCst), OUTSTANDING.load(Ordering::SeqCst));
    let mut text = format(
        c"{\"pending\":%s,\"remaining\":%u,\"outstanding\":%u,\"batches\":%u,\"cancelled\":%u,\"invalid\":%u,\"stages\":[",
        &[
            Arg::S(if budget != 0 || outstanding != 0 {
                c"true".as_ptr()
            } else {
                c"false".as_ptr()
            }),
            Arg::I(budget as i32),
            Arg::I(outstanding as i32),
            Arg::I(p.completed as i32),
            Arg::I(CANCELLED.load(Ordering::SeqCst) as i32),
            Arg::I(p.invalid as i32),
        ],
    )
    .into_bytes();
    for (i, s) in p.statistics.iter().enumerate() {
        text.extend_from_slice(
            format(
                c"%s{\"name\":\"%s\",\"count\":%u,\"mean_us\":%.3f,\"max_us\":%.3f}",
                &[
                    Arg::S(if i != 0 { c",".as_ptr() } else { c"".as_ptr() }),
                    Arg::S(NAMES[i].as_ptr()),
                    Arg::I(s.count as i32),
                    Arg::F(if s.count != 0 { s.total / s.count as f64 } else { 0.0 }),
                    Arg::F(s.peak),
                ],
            )
            .as_bytes(),
        );
    }
    text.extend_from_slice(b"]}");
    if !report.is_null() && len > 0 {
        // snprintf's truncation: as much as fits, then the terminator.
        let n = text.len().min(len - 1);
        // SAFETY: the exported report buffer of `len` bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(text.as_ptr(), report, n);
            *report.add(n) = 0;
        }
    }
    seq().fetch_add(1, Ordering::Release);
}

/// Begin: a batch for `list` while a request is armed, else None (no D3D call).
pub fn begin(list: &ID3D12GraphicsCommandList) -> Option<Arc<Batch>> {
    if BUDGET.load(Ordering::Relaxed) == 0 {
        return None;
    }
    let batch = Arc::new(Batch {
        counted: AtomicBool::new(false),
        recording: Mutex::new(Recording::default()),
    });
    {
        let _p = profile();
        if BUDGET.load(Ordering::Relaxed) == 0 {
            return None;
        }
        BUDGET.fetch_sub(1, Ordering::Relaxed);
        OUTSTANDING.fetch_add(1, Ordering::Relaxed);
        batch.counted.store(true, Ordering::Relaxed);
    }
    // Each bounded sample owns its queries; neither a list reset nor another recording can overwrite an in-flight
    // timestamp/readback slot.
    // SAFETY (whole block): the live list and its device; descriptions on the stack.
    unsafe {
        if list.GetType() != D3D12_COMMAND_LIST_TYPE_DIRECT {
            return None;
        }
        let mut device: Option<ID3D12Device> = None;
        list.GetDevice(&mut device).ok()?;
        let device = device?;
        let query = D3D12_QUERY_HEAP_DESC {
            Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP,
            Count: (MAX_PAIRS * 2) as u32,
            NodeMask: 0,
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: (MAX_PAIRS * 2 * 8) as u64,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            ..Default::default()
        };
        let mut heap: Option<ID3D12QueryHeap> = None;
        device.CreateQueryHeap(&query, &mut heap).ok()?;
        let mut readback: Option<ID3D12Resource> = None;
        device
            .CreateCommittedResource(&properties, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut readback)
            .ok()?;
        let (heap, readback) = (heap?, readback?);
        // This also covers aborted recordings and possible command-list replay.
        if !crate::command_resources::keep(list, &[heap.as_raw(), readback.as_raw()]) {
            return None;
        }
        let mut r = batch.lock();
        r.heap = Some(heap);
        r.readback = Some(readback);
        r.list = list.as_raw();
    }
    Some(batch)
}

/// Scope begin: the stage's first timestamp; the pair's index, or None when the batch is full.
pub fn scope_begin(batch: &Batch, stage: u32) -> Option<usize> {
    let mut r = batch.lock();
    if r.used == MAX_PAIRS || stage as usize >= STAGES {
        return None;
    }
    let index = r.used;
    r.used += 1;
    r.pairs[index].stage = stage;
    // SAFETY: the list this batch was begun on, still recording.
    if let (Some(list), Some(heap)) = (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&r.list) }, &r.heap) {
        // SAFETY: the recording list and its query heap.
        unsafe { list.EndQuery(heap, D3D12_QUERY_TYPE_TIMESTAMP, (index * 2) as u32) };
    }
    Some(index)
}
/// Scope end: the pair's second timestamp, resolved into its readback slot.
pub fn scope_end(batch: &Batch, index: usize) {
    let mut r = batch.lock();
    // SAFETY: the list this batch was begun on, still recording.
    let list = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&r.list) };
    if let (Some(list), Some(heap), Some(readback)) = (list, &r.heap, &r.readback) {
        // SAFETY: the recording list, its query heap and readback buffer.
        unsafe {
            list.EndQuery(heap, D3D12_QUERY_TYPE_TIMESTAMP, (index * 2 + 1) as u32);
            list.ResolveQueryData(heap, D3D12_QUERY_TYPE_TIMESTAMP, (index * 2) as u32, 2, readback, (index * 2 * 8) as u64);
        }
    }
    r.pairs[index].ended = true;
}

/// Submitted: after a successful submission and Signal(`fence`, `value`) on `queue`.
pub fn submitted(batch: &Arc<Batch>, queue: &ID3D12CommandQueue, fence: &ID3D12Fence, value: u64) {
    {
        let mut r = batch.lock();
        if r.submitted || r.used == 0 || value == 0 {
            return;
        }
        // SAFETY: the live queue.
        match unsafe { queue.GetTimestampFrequency() } {
            Ok(f) if f != 0 => r.frequency = f,
            _ => return,
        }
        r.fence = Some(fence.clone());
        r.value = value;
        r.submitted = true;
    }
    profile().pending.push(batch.clone());
}

/// Poll: nonblocking, maps only completed readbacks (Present).
pub fn poll() {
    if !ACTIVE.load(Ordering::Relaxed) && request().load(Ordering::Relaxed) == 0 {
        return;
    }
    let mut p = profile();
    let mut changed = false;
    let pending = core::mem::take(&mut p.pending);
    let mut keep = Vec::with_capacity(pending.len());
    for batch in pending {
        let r = batch.lock();
        let Some(fence) = &r.fence else {
            drop(r);
            keep.push(batch);
            continue;
        };
        // SAFETY: the live fence.
        let done = unsafe { fence.GetCompletedValue() };
        if done < r.value {
            drop(r);
            keep.push(batch);
            continue;
        }
        let mut mapped: *mut c_void = core::ptr::null_mut();
        let range = D3D12_RANGE { Begin: 0, End: r.used * 2 * 8 };
        let readback = r.readback.as_ref().filter(|_| done != u64::MAX);
        // SAFETY: the completed readback buffer.
        if let Some(readback) = readback.filter(|b| unsafe { b.Map(0, Some(&range), Some(&mut mapped)) }.is_ok()) {
            // SAFETY: the mapped range holds `used` pairs.
            let ticks = unsafe { core::slice::from_raw_parts(mapped as *const u64, r.used * 2) };
            for i in 0..r.used {
                if !r.pairs[i].ended || ticks[i * 2 + 1] < ticks[i * 2] {
                    p.invalid += 1;
                    continue;
                }
                let us = (ticks[i * 2 + 1] - ticks[i * 2]) as f64 * 1e6 / r.frequency as f64;
                let s = &mut p.statistics[r.pairs[i].stage as usize];
                s.count += 1;
                s.total += us;
                s.peak = s.peak.max(us);
            }
            let none = D3D12_RANGE { Begin: 0, End: 0 };
            // SAFETY: the buffer mapped above.
            unsafe { readback.Unmap(0, Some(&none)) };
            p.completed += 1;
        } else {
            p.invalid += 1;
        }
        changed = true;
    }
    p.pending = keep; // submissions arrive under the same lock, so none came in meanwhile
    let idle = BUDGET.load(Ordering::SeqCst) == 0 && OUTSTANDING.load(Ordering::Acquire) == 0;
    if idle {
        let asked = request().swap(0, Ordering::AcqRel);
        if asked != 0 {
            p.statistics = [Statistic::default(); STAGES];
            p.completed = 0;
            p.invalid = 0;
            CANCELLED.store(0, Ordering::SeqCst);
            BUDGET.store(asked.min(32), Ordering::Release);
            ACTIVE.store(true, Ordering::Relaxed);
            changed = true;
        } else if ACTIVE.swap(false, Ordering::Relaxed) {
            changed = true;
        }
    }
    if changed {
        publish(&p);
    }
}

// ---- the C ABI (include/Render/GpuStageProfile.hpp; BatchPtr there holds one Arc reference)

/// # Safety
/// `list` is an ID3D12GraphicsCommandList or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stage_begin(list: *mut c_void) -> *const Batch {
    if BUDGET.load(Ordering::Relaxed) == 0 {
        return core::ptr::null();
    }
    // SAFETY: the caller's list.
    match unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) }.and_then(begin) {
        Some(b) => Arc::into_raw(b),
        None => core::ptr::null(),
    }
}
/// # Safety
/// `batch` is a reference from cvr_gpu_stage_begin/_clone, or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stage_clone(batch: *const Batch) -> *const Batch {
    if !batch.is_null() {
        // SAFETY: one more reference to a live Arc.
        unsafe { Arc::increment_strong_count(batch) };
    }
    batch
}
/// # Safety
/// `batch` is a reference from cvr_gpu_stage_begin/_clone (released once), or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stage_free(batch: *const Batch) {
    if !batch.is_null() {
        // SAFETY: the caller's reference, released once.
        drop(unsafe { Arc::from_raw(batch) });
    }
}
/// # Safety
/// `batch` is a live reference or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stage_scope_begin(batch: *const Batch, stage: u32) -> u32 {
    // SAFETY: the caller's reference.
    unsafe { batch.as_ref() }.and_then(|b| scope_begin(b, stage)).map_or(u32::MAX, |i| i as u32)
}
/// # Safety
/// `batch` is a live reference; `index` came from cvr_gpu_stage_scope_begin on it.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stage_scope_end(batch: *const Batch, index: u32) {
    // SAFETY: the caller's reference.
    if let Some(b) = unsafe { batch.as_ref() } {
        if (index as usize) < MAX_PAIRS {
            scope_end(b, index as usize);
        }
    }
}
/// # Safety
/// `batch` is a live reference or null; `queue`/`fence` an ID3D12CommandQueue/ID3D12Fence or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_stage_submitted(batch: *const Batch, queue: *mut c_void, fence: *mut c_void, value: u64) {
    if batch.is_null() {
        return;
    }
    // SAFETY: the caller's objects; the Arc is borrowed (one more reference taken for the pending list).
    unsafe {
        let arc = core::mem::ManuallyDrop::new(Arc::from_raw(batch));
        if let (Some(q), Some(f)) = (ID3D12CommandQueue::from_raw_borrowed(&queue), ID3D12Fence::from_raw_borrowed(&fence)) {
            submitted(&arc, q, f, value);
        }
    }
}
#[no_mangle]
pub extern "C" fn cvr_gpu_stage_poll() {
    poll()
}
