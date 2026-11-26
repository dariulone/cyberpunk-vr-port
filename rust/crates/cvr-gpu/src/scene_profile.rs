//! The scene GPU profile (formerly src/Render/SceneGpuProfile.cpp): on request, the engine's command lists get a
//! timestamp pair per range of draws/dispatches of one render-graph node, side and pipeline (CyberpunkVR_SceneGpuRequest:
//! the next N lists; CyberpunkVR_SceneGpuFrameRequest: every list begun in the next N whole frames), optionally a
//! pipeline-statistics query per range and an occlusion query per draw, all in one query heap and readback buffer per
//! profile (a slice per list). Present maps the completed lists into CyberpunkVR_SceneGpuReport, grouped by key.
//!
//! The switches and the report stay exported by the C++ half, which attaches them here at its static initialisation
//! (with the Present counter the frame mode counts by); until then everything reads off.
use cvr_host::fast::FastMap;
use core::ffi::c_void;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use cvr_crt::{format, Arg};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

/// Ranges per list, and lists per profile: one query heap and one readback buffer per profile, a slice of each per list
/// (a committed readback buffer per list cost a 64 KiB allocation each).
const CAPACITY: usize = 256;
const POOL_LISTS: u32 = 1536;
/// Pipeline-statistics ranges per list (88 bytes each in the readback).
const STATS_CAPACITY: u32 = 64;
/// UINT64s in D3D12_QUERY_DATA_PIPELINE_STATISTICS.
const STATS_COUNT: usize = 11;
const STATS_BYTES: usize = STATS_COUNT * 8;
/// Occlusion-queried draws per list.
const DRAW_CAPACITY: usize = 2048;

// ---- the exports (Render/SceneGpuProfile.cpp)

/// CvrSceneProfileExports: the addresses of the C++ half's switches, report and Present counter.
#[repr(C)]
pub struct Exports {
    request: *const AtomicU32,
    report_seq: *const AtomicU32,
    group_mode: *const AtomicU32,
    node_filter: *const AtomicU32,
    viewport_detail: *const AtomicU32,
    frame_request: *const AtomicU32,
    pipeline_stats: *const AtomicU32,
    draw_occlusion: *const AtomicU32,
    report: *mut u8,
    report_len: usize,
    present_count: *const AtomicU64,
}
static OFF: AtomicU32 = AtomicU32::new(0);
static SINK: AtomicU32 = AtomicU32::new(0);
static NO_PRESENTS: AtomicU64 = AtomicU64::new(0);
struct Detached(Exports);
// SAFETY: the pointers name the statics above.
unsafe impl Sync for Detached {}
static DETACHED: Detached = Detached(Exports {
    request: &SINK,
    report_seq: &SINK,
    group_mode: &OFF,
    node_filter: &OFF,
    viewport_detail: &OFF,
    frame_request: &SINK,
    pipeline_stats: &OFF,
    draw_occlusion: &OFF,
    report: core::ptr::null_mut(),
    report_len: 0,
    present_count: &NO_PRESENTS,
});
static EXPORTS: AtomicPtr<Exports> = AtomicPtr::new(core::ptr::null_mut());
fn exports() -> &'static Exports {
    let p = EXPORTS.load(Ordering::Acquire);
    if p.is_null() {
        &DETACHED.0
    } else {
        // SAFETY: the C++ half's table, alive for the process.
        unsafe { &*p }
    }
}
fn a32(p: *const AtomicU32) -> &'static AtomicU32 {
    // SAFETY: an exported std::atomic<uint32_t> or a static here.
    unsafe { &*p }
}
/// The Present count (Hooks/SwapChain.cpp CyberpunkVR_PresentTiming[0]).
fn serial() -> u64 {
    // SAFETY: an exported std::atomic<uint64_t> or a static here.
    unsafe { &*exports().present_count }.load(Ordering::Relaxed)
}

/// # Safety
/// `exports` is a CvrSceneProfileExports table alive for the process.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_profile_attach(exports: *const Exports) {
    EXPORTS.store(exports as *mut Exports, Ordering::Release);
}

/// scene_profile::active.
pub static ACTIVE: AtomicBool = AtomicBool::new(false);

// ---- the viewport class of a draw

thread_local! {
    // A list is recorded by one thread at a time, so the last viewport of the list this thread records is enough.
    static VIEWPORT_LIST: Cell<usize> = const { Cell::new(0) };
    static VIEWPORT_DETAIL: Cell<u32> = const { Cell::new(0) };
}
/// MSVC's float-to-unsigned conversion (through a 64-bit truncation).
fn to_u32(v: f32) -> u32 {
    v as i64 as u32
}
/// Viewport: the list's last RSSetViewports, for CyberpunkVR_SceneGpuViewportDetail.
pub fn viewport(list: usize, viewports: &[D3D12_VIEWPORT]) {
    let e = exports();
    if !ACTIVE.load(Ordering::Relaxed) || a32(e.viewport_detail).load(Ordering::Relaxed) == 0 {
        return;
    }
    let Some(v) = viewports.first() else {
        VIEWPORT_LIST.set(0);
        return;
    };
    let (x, width) = (to_u32(v.TopLeftX + 0.5), to_u32(v.Width + 0.5));
    let kind = if x == 0 {
        1u32
    } else if x.abs_diff(width) <= 1 {
        2
    } else {
        3
    };
    VIEWPORT_LIST.set(list);
    VIEWPORT_DETAIL.set(0x100000 | (kind << 16) | (width & 0xFFFF));
}
/// DrawDetail: 0x100000 | kind << 16 | width for the list's last viewport, else 0.
pub fn draw_detail(list: usize) -> u32 {
    if VIEWPORT_LIST.get() == list && a32(exports().viewport_detail).load(Ordering::Relaxed) != 0 {
        VIEWPORT_DETAIL.get()
    } else {
        0
    }
}

// ---- the profile

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    node: u32,
    side: i32,
    pso: u64,
    detail: u32,
}
struct Pool {
    heap: ID3D12QueryHeap,
    readback: ID3D12Resource,
    stats: Option<(ID3D12QueryHeap, ID3D12Resource)>,
    occ: Option<(ID3D12QueryHeap, ID3D12Resource)>,
    device: usize,
    next: Mutex<u32>,
}
struct Range {
    key: Key,
    commands: u32,
    stat: Option<u32>,
}
struct Batch {
    pool: Arc<Pool>,
    slice: u32,
    fence: Option<ID3D12Fence>,
    value: u64,
    frequency: u64,
    ranges: Vec<Range>,
    stats_used: u32,
    /// Per occlusion-queried draw: the viewport kind (1, 2, 3; 0 unknown), and the range it was drawn in.
    draw_kind: Vec<u8>,
    draw_range: Vec<u32>,
    draw_open: bool,
    closed: bool,
    claimed: bool,
    invalid: bool,
    range_open: bool,
    compute: bool,
    stat_open: bool,
}
impl Batch {
    fn query(&self, index: usize) -> u32 {
        self.slice * CAPACITY as u32 * 2 + index as u32
    }
    fn stat_query(&self, index: u32) -> u32 {
        self.slice * STATS_CAPACITY + index
    }
    fn draw_query(&self, index: usize) -> u32 {
        self.slice * DRAW_CAPACITY as u32 + index as u32
    }
}
/// Batches are shared between the recordings, the submissions and the pending list; every access is under PROFILE.
type Shared = Arc<Mutex<Batch>>;

#[derive(Clone, Copy, Default)]
struct Statistic {
    ranges: u32,
    commands: u32,
    lists: u32,
    sum_ms: f64,
    max_ms: f64,
    stats: [u64; STATS_COUNT],
    has_stats: bool,
    occ: [u64; 5],
}
struct Profile {
    remaining: u32,
    completed: u32,
    discarded: u32,
    frame_mode: bool,
    exhausted: bool,
    start_serial: u64,
    end_serial: u64,
    deadline: Option<Instant>,
    pool: Option<Arc<Pool>>,
    lists: FastMap<usize, Shared>,
    pending: Vec<Shared>,
    stats_mode: bool,
    occ_mode: bool,
    /// By viewport kind: draws, 0, 1-64, 65-4096, more samples.
    occ_counts: [[u64; 5]; 4],
    statistics: BTreeMap<Key, Statistic>,
}
// SAFETY: free-threaded D3D12 objects; every access is under the mutex.
unsafe impl Send for Profile {}
static PROFILE: Mutex<Option<Profile>> = Mutex::new(None);
fn lock() -> MutexGuard<'static, Option<Profile>> {
    PROFILE.lock().unwrap_or_else(|p| p.into_inner())
}
fn with<R>(f: impl FnOnce(&mut Profile) -> R) -> R {
    let mut g = lock();
    let p = g.get_or_insert_with(|| Profile {
        remaining: 0,
        completed: 0,
        discarded: 0,
        frame_mode: false,
        exhausted: false,
        start_serial: 0,
        end_serial: 0,
        deadline: None,
        pool: None,
        lists: cvr_host::fast::fast_map(),
        pending: Vec::new(),
        stats_mode: false,
        occ_mode: false,
        occ_counts: [[0; 5]; 4],
        statistics: BTreeMap::new(),
    });
    f(p)
}
fn batch_lock(b: &Shared) -> MutexGuard<'_, Batch> {
    b.lock().unwrap_or_else(|p| p.into_inner())
}

fn g(v: f64) -> String {
    format(c"%g", &[Arg::F(v)]).to_string_lossy().into_owned()
}
fn publish(p: &Profile) {
    use std::fmt::Write;
    let e = exports();
    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"active\":{},\"remaining\":{},\"pending\":{},\"recordings\":{},\"completed\":{},\"discarded\":{},\"mode\":\"{}\",\"frames\":{},\"exhausted\":{},\"groups\":[",
        ACTIVE.load(Ordering::SeqCst),
        p.remaining,
        p.pending.len(),
        p.lists.len(),
        p.completed,
        p.discarded,
        if p.frame_mode { "frames" } else { "lists" },
        if p.frame_mode { p.end_serial.wrapping_sub(p.start_serial) } else { 0 },
        p.exhausted
    );
    for (i, (key, value)) in p.statistics.iter().enumerate() {
        if i != 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"node\":{},\"side\":{},\"pso\":{},\"ranges\":{},\"commands\":{},\"lists\":{},\"detail\":{},\"sum_ms\":{},\"max_ms\":{}",
            key.node,
            key.side,
            key.pso,
            value.ranges,
            value.commands,
            value.lists,
            key.detail,
            g(value.sum_ms),
            g(value.max_ms)
        );
        if value.has_stats {
            let list: Vec<String> = value.stats.iter().map(u64::to_string).collect();
            let _ = write!(out, ",\"ps\":[{}]", list.join(","));
        }
        if value.occ[0] != 0 {
            let list: Vec<String> = value.occ.iter().map(u64::to_string).collect();
            let _ = write!(out, ",\"occ\":[{}]", list.join(","));
        }
        out.push('}');
    }
    out.push(']');
    if p.occ_mode {
        out.push_str(",\"occ\":{");
        for (k, counts) in p.occ_counts.iter().enumerate() {
            let list: Vec<String> = counts.iter().map(u64::to_string).collect();
            let _ = write!(out, "{}\"{}\":[{}]", if k != 0 { "," } else { "" }, k, list.join(","));
        }
        out.push('}');
    }
    out.push('}');
    if out.len() >= e.report_len {
        out = "{\"error\":\"report overflow\"}".to_string();
    }
    let seq = a32(e.report_seq);
    seq.fetch_add(1, Ordering::AcqRel);
    if !e.report.is_null() && out.len() < e.report_len {
        // SAFETY: the exported report buffer of report_len bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(out.as_ptr(), e.report, out.len());
            *e.report.add(out.len()) = 0;
        }
    }
    seq.fetch_add(1, Ordering::Release);
}

fn readback_buffer(device: &ID3D12Device, width: u64) -> Option<ID3D12Resource> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: width,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_READBACK,
        ..Default::default()
    };
    let mut r: Option<ID3D12Resource> = None;
    // SAFETY: the live device; descriptions on the stack.
    unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut r) }.ok()?;
    r
}
fn query_heap(device: &ID3D12Device, kind: D3D12_QUERY_HEAP_TYPE, count: u32) -> Option<ID3D12QueryHeap> {
    let mut h: Option<ID3D12QueryHeap> = None;
    // SAFETY: the live device.
    unsafe {
        device.CreateQueryHeap(
            &D3D12_QUERY_HEAP_DESC {
                Type: kind,
                Count: count,
                NodeMask: 0,
            },
            &mut h,
        )
    }
    .ok()?;
    h
}
fn make_pool(device: &ID3D12Device, stats_mode: bool, occ_mode: bool) -> Option<Arc<Pool>> {
    let heap = query_heap(device, D3D12_QUERY_HEAP_TYPE_TIMESTAMP, POOL_LISTS * CAPACITY as u32 * 2)?;
    let readback = readback_buffer(device, POOL_LISTS as u64 * CAPACITY as u64 * 16)?;
    let occ = if occ_mode {
        Some((
            query_heap(device, D3D12_QUERY_HEAP_TYPE_OCCLUSION, POOL_LISTS * DRAW_CAPACITY as u32)?,
            readback_buffer(device, POOL_LISTS as u64 * DRAW_CAPACITY as u64 * 8)?,
        ))
    } else {
        None
    };
    let stats = if stats_mode {
        Some((
            query_heap(device, D3D12_QUERY_HEAP_TYPE_PIPELINE_STATISTICS, POOL_LISTS * STATS_CAPACITY)?,
            readback_buffer(device, POOL_LISTS as u64 * STATS_CAPACITY as u64 * STATS_BYTES as u64)?,
        ))
    } else {
        None
    };
    Some(Arc::new(Pool {
        heap,
        readback,
        stats,
        occ,
        device: device.as_raw() as usize,
        next: Mutex::new(0),
    }))
}
fn create(p: &mut Profile, list: &ID3D12GraphicsCommandList) -> Option<Shared> {
    // SAFETY (whole body): the live list and its device.
    unsafe {
        let kind = list.GetType();
        if kind != D3D12_COMMAND_LIST_TYPE_DIRECT && kind != D3D12_COMMAND_LIST_TYPE_COMPUTE {
            return None;
        }
        let mut device: Option<ID3D12Device> = None;
        list.GetDevice(&mut device).ok()?;
        let device = device?;
        if p.pool.as_ref().is_none_or(|pool| pool.device != device.as_raw() as usize) {
            p.pool = make_pool(&device, p.stats_mode, p.occ_mode);
        }
        let pool = p.pool.clone()?;
        let slice = {
            let mut next = pool.next.lock().unwrap_or_else(|e| e.into_inner());
            if *next >= POOL_LISTS {
                p.exhausted = true;
                p.remaining = 0;
                return None;
            }
            *next += 1;
            *next - 1
        };
        let keep = crate::command_resources::keep;
        if !keep(list, &[pool.heap.as_raw(), pool.readback.as_raw()]) {
            return None;
        }
        if let Some((h, r)) = &pool.stats {
            if !keep(list, &[h.as_raw(), r.as_raw()]) {
                return None;
            }
        }
        if let Some((h, r)) = &pool.occ {
            if !keep(list, &[h.as_raw(), r.as_raw()]) {
                return None;
            }
        }
        let occ = pool.occ.is_some();
        Some(Arc::new(Mutex::new(Batch {
            pool,
            slice,
            fence: None,
            value: 0,
            frequency: 0,
            ranges: Vec::with_capacity(CAPACITY),
            stats_used: 0,
            draw_kind: Vec::with_capacity(if occ { 256 } else { 0 }),
            draw_range: Vec::with_capacity(if occ { 256 } else { 0 }),
            draw_open: false,
            closed: false,
            claimed: false,
            invalid: false,
            range_open: false,
            compute: kind == D3D12_COMMAND_LIST_TYPE_COMPUTE,
            stat_open: false,
        })))
    }
}
fn end_stat(list: &ID3D12GraphicsCommandList, b: &mut Batch) {
    if !b.stat_open {
        return;
    }
    if let Some((heap, _)) = &b.pool.stats {
        // SAFETY: the recording list and its pool's statistics heap.
        unsafe { list.EndQuery(heap, D3D12_QUERY_TYPE_PIPELINE_STATISTICS, b.stat_query(b.stats_used - 1)) };
    }
    b.stat_open = false;
}

/// Mark: a draw or dispatch of `node` (side 0..2) with `pso` on `list`; consecutive ones of the same key share a range.
pub fn mark(list: &ID3D12GraphicsCommandList, node: u32, side: i32, pso: u64, detail: u32) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let e = exports();
    let filter = a32(e.node_filter).load(Ordering::Relaxed);
    if filter != 0 && filter != node {
        end_range(list);
        return;
    }
    with(|p| {
        let key = list.as_raw() as usize;
        if !p.lists.contains_key(&key) {
            if p.remaining == 0 || node == 0 || !(0..=2).contains(&side) {
                return;
            }
            if p.frame_mode {
                // Only lists begun inside the whole frames asked for.
                let serial = serial();
                if serial <= p.start_serial {
                    return;
                }
                if serial > p.end_serial {
                    p.remaining = 0;
                    return;
                }
            } else {
                p.remaining -= 1;
            }
            let Some(batch) = create(p, list) else {
                p.discarded += 1;
                return;
            };
            p.lists.insert(key, batch);
        }
        let shared = p.lists[&key].clone();
        let mut b = batch_lock(&shared);
        if b.closed || b.invalid {
            return;
        }
        // 0x200000: recorded on a compute list (async compute: its span overlaps the graphics queue's work).
        let k = Key {
            node,
            side,
            pso: if a32(e.group_mode).load(Ordering::Relaxed) != 0 { 0 } else { pso },
            detail: detail | if b.compute { 0x200000 } else { 0 },
        };
        if b.range_open && b.ranges.last().is_some_and(|r| r.key == k) {
            if let Some(r) = b.ranges.last_mut() {
                r.commands += 1;
            }
            return;
        }
        let pool = b.pool.clone();
        // SAFETY (whole block): the recording list and its pool's heaps.
        unsafe {
            if b.range_open {
                list.EndQuery(&pool.heap, D3D12_QUERY_TYPE_TIMESTAMP, b.query(b.ranges.len() * 2 - 1));
            }
            end_stat(list, &mut b);
            if b.ranges.len() == CAPACITY {
                b.invalid = true;
                return;
            }
            list.EndQuery(&pool.heap, D3D12_QUERY_TYPE_TIMESTAMP, b.query(b.ranges.len() * 2));
            b.ranges.push(Range {
                key: k,
                commands: 1,
                stat: None,
            });
            b.range_open = true;
            if let Some((heap, _)) = &pool.stats {
                if !b.compute && b.stats_used < STATS_CAPACITY {
                    list.BeginQuery(heap, D3D12_QUERY_TYPE_PIPELINE_STATISTICS, b.stat_query(b.stats_used));
                    let used = b.stats_used;
                    if let Some(r) = b.ranges.last_mut() {
                        r.stat = Some(used);
                    }
                    b.stats_used += 1;
                    b.stat_open = true;
                }
            }
        }
    })
}
/// DrawBegin: an occlusion query around a draw of a profiled range (CyberpunkVR_SceneGpuDrawOcclusion).
pub fn draw_begin(list: &ID3D12GraphicsCommandList) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    with(|p| {
        if !p.occ_mode {
            return;
        }
        let Some(shared) = p.lists.get(&(list.as_raw() as usize)).cloned() else {
            return;
        };
        let mut b = batch_lock(&shared);
        // Only inside a profiled range (the node filter ends the range for other nodes).
        let Some((heap, _)) = b.pool.occ.clone() else { return };
        if b.closed || b.invalid || !b.range_open || b.compute || b.draw_open || b.draw_kind.len() >= DRAW_CAPACITY {
            return;
        }
        let detail = draw_detail(list.as_raw() as usize);
        b.draw_kind.push(if detail & 0x100000 != 0 { ((detail >> 16) & 3) as u8 } else { 0 });
        let range = (b.ranges.len() - 1) as u32;
        b.draw_range.push(range);
        // SAFETY: the recording list and its pool's occlusion heap.
        unsafe { list.BeginQuery(&heap, D3D12_QUERY_TYPE_OCCLUSION, b.draw_query(b.draw_kind.len() - 1)) };
        b.draw_open = true;
    })
}
pub fn draw_end(list: &ID3D12GraphicsCommandList) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    with(|p| {
        if !p.occ_mode {
            return;
        }
        let Some(shared) = p.lists.get(&(list.as_raw() as usize)).cloned() else {
            return;
        };
        let mut b = batch_lock(&shared);
        if !b.draw_open {
            return;
        }
        if let Some((heap, _)) = &b.pool.occ {
            // SAFETY: the recording list and its pool's occlusion heap.
            unsafe { list.EndQuery(heap, D3D12_QUERY_TYPE_OCCLUSION, b.draw_query(b.draw_kind.len() - 1)) };
        }
        b.draw_open = false;
    })
}
/// EndRange: the open range ends here (a command the profile does not attribute).
pub fn end_range(list: &ID3D12GraphicsCommandList) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    with(|p| {
        let Some(shared) = p.lists.get(&(list.as_raw() as usize)).cloned() else {
            return;
        };
        let mut b = batch_lock(&shared);
        if b.closed || b.invalid || !b.range_open {
            return;
        }
        let heap = b.pool.heap.clone();
        // SAFETY: the recording list and its pool's timestamp heap.
        unsafe { list.EndQuery(&heap, D3D12_QUERY_TYPE_TIMESTAMP, b.query(b.ranges.len() * 2 - 1)) };
        b.range_open = false;
        end_stat(list, &mut b);
    })
}
/// Close: the list's queries resolved into its slices of the readback buffers.
pub fn close(list: &ID3D12GraphicsCommandList) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    with(|p| {
        let Some(shared) = p.lists.get(&(list.as_raw() as usize)).cloned() else {
            return;
        };
        let mut b = batch_lock(&shared);
        if b.closed || b.invalid || b.ranges.is_empty() {
            return;
        }
        let pool = b.pool.clone();
        // SAFETY (whole block): the recording list and its pool's heaps and readback buffers.
        unsafe {
            if b.range_open {
                list.EndQuery(&pool.heap, D3D12_QUERY_TYPE_TIMESTAMP, b.query(b.ranges.len() * 2 - 1));
            }
            b.range_open = false;
            end_stat(list, &mut b);
            list.ResolveQueryData(
                &pool.heap,
                D3D12_QUERY_TYPE_TIMESTAMP,
                b.query(0),
                (b.ranges.len() * 2) as u32,
                &pool.readback,
                b.slice as u64 * CAPACITY as u64 * 16,
            );
            if let Some((heap, readback)) = &pool.occ {
                if b.draw_open {
                    list.EndQuery(heap, D3D12_QUERY_TYPE_OCCLUSION, b.draw_query(b.draw_kind.len() - 1));
                    b.draw_open = false;
                }
                if !b.draw_kind.is_empty() {
                    list.ResolveQueryData(
                        heap,
                        D3D12_QUERY_TYPE_OCCLUSION,
                        b.draw_query(0),
                        b.draw_kind.len() as u32,
                        readback,
                        b.slice as u64 * DRAW_CAPACITY as u64 * 8,
                    );
                }
            }
            if let Some((heap, readback)) = &pool.stats {
                if b.stats_used != 0 {
                    list.ResolveQueryData(
                        heap,
                        D3D12_QUERY_TYPE_PIPELINE_STATISTICS,
                        b.stat_query(0),
                        b.stats_used,
                        readback,
                        b.slice as u64 * STATS_CAPACITY as u64 * STATS_BYTES as u64,
                    );
                }
            }
        }
        b.closed = true;
    })
}
/// Reset: the list's recording is gone (an unsubmitted one counts as discarded).
pub fn reset(list: usize) {
    if VIEWPORT_LIST.get() == list {
        VIEWPORT_LIST.set(0);
    }
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    with(|p| {
        if let Some(shared) = p.lists.remove(&list) {
            if !batch_lock(&shared).claimed {
                p.discarded += 1;
            }
        }
    })
}

/// Submission: the batches of the lists one ExecuteCommandLists submits.
pub struct Submission(Vec<Shared>);
/// BeforeSubmit: the closed, valid recordings among `lists` (a recording submitted twice is invalid).
pub fn before_submit(lists: &[usize]) -> Submission {
    let mut result = Vec::new();
    if !ACTIVE.load(Ordering::Relaxed) {
        return Submission(result);
    }
    with(|p| {
        for list in lists {
            let Some(shared) = p.lists.get(list).cloned() else { continue };
            let mut b = batch_lock(&shared);
            if b.claimed {
                b.invalid = true;
                continue;
            }
            if !b.closed || b.invalid {
                drop(b);
                p.discarded += 1;
                p.lists.remove(list);
                continue;
            }
            b.claimed = true;
            drop(b);
            result.push(shared);
        }
    });
    Submission(result)
}
/// Submitted: the submission is on `queue`; a fence of its own signals its end.
pub fn submitted(queue: &ID3D12CommandQueue, values: Submission) {
    if values.0.is_empty() {
        return;
    }
    // SAFETY (whole block): the live queue and its device.
    let (fence, frequency, ok) = unsafe {
        let mut device: Option<ID3D12Device> = None;
        let mut frequency = 0u64;
        let mut fence: Option<ID3D12Fence> = None;
        let ok = queue.GetDevice(&mut device).is_ok()
            && device.as_ref().is_some_and(|d| {
                fence = d.CreateFence(0, D3D12_FENCE_FLAG_NONE).ok();
                fence.is_some()
            })
            && queue.GetTimestampFrequency().inspect(|f| frequency = *f).is_ok()
            && frequency != 0
            && fence.as_ref().is_some_and(|f| queue.Signal(f, 1).is_ok());
        (fence, frequency, ok)
    };
    with(|p| {
        for shared in values.0 {
            {
                let mut b = batch_lock(&shared);
                b.frequency = frequency;
                b.fence = fence.clone();
                b.value = 1;
                if !ok {
                    b.invalid = true;
                    b.fence = None;
                }
            }
            p.pending.push(shared);
        }
    })
}

fn bucket(n: u64) -> usize {
    match n {
        0 => 1,
        1..=64 => 2,
        65..=4096 => 3,
        _ => 4,
    }
}
/// Reads one completed batch into the statistics (false: invalid).
fn harvest(p: &mut Profile, b: &Batch) -> bool {
    let mut invalid = b.invalid;
    if invalid || b.frequency == 0 {
        return !invalid;
    }
    let mut data: *mut c_void = core::ptr::null_mut();
    let offset = b.slice as usize * CAPACITY * 16;
    let range = D3D12_RANGE {
        Begin: offset,
        End: offset + b.ranges.len() * 16,
    };
    // SAFETY (whole body): readback buffers whose slices the GPU has finished writing; the mapped ranges hold what
    // the reads cover.
    unsafe {
        if b.pool.readback.Map(0, Some(&range), Some(&mut data)).is_ok() {
            let stamps = core::slice::from_raw_parts((data as *const u8).add(offset) as *const u64, b.ranges.len() * 2);
            invalid = (0..b.ranges.len()).any(|i| stamps[2 * i + 1] < stamps[2 * i]);
            if !invalid {
                let mut seen = BTreeSet::new();
                for (i, r) in b.ranges.iter().enumerate() {
                    let stat = p.statistics.entry(r.key).or_default();
                    stat.ranges += 1;
                    stat.commands += r.commands;
                    if seen.insert(r.key) {
                        stat.lists += 1;
                    }
                    let ms = (stamps[2 * i + 1] - stamps[2 * i]) as f64 * 1000.0 / b.frequency as f64;
                    stat.sum_ms += ms;
                    stat.max_ms = stat.max_ms.max(ms);
                }
            }
            b.pool.readback.Unmap(0, Some(&D3D12_RANGE::default()));
        } else {
            invalid = true;
        }
        if let (false, Some((_, readback)), false) = (invalid, &b.pool.occ, b.draw_kind.is_empty()) {
            let occ_offset = b.slice as usize * DRAW_CAPACITY * 8;
            let occ_range = D3D12_RANGE {
                Begin: occ_offset,
                End: occ_offset + b.draw_kind.len() * 8,
            };
            if readback.Map(0, Some(&occ_range), Some(&mut data)).is_ok() {
                let samples = core::slice::from_raw_parts((data as *const u8).add(occ_offset) as *const u64, b.draw_kind.len());
                for (d, &n) in samples.iter().enumerate() {
                    let k = bucket(n);
                    let c = &mut p.occ_counts[b.draw_kind[d] as usize];
                    c[0] += 1;
                    c[k] += 1;
                    if let Some(r) = b.ranges.get(b.draw_range[d] as usize) {
                        let g = &mut p.statistics.entry(r.key).or_default().occ;
                        g[0] += 1;
                        g[k] += 1;
                    }
                }
                readback.Unmap(0, Some(&D3D12_RANGE::default()));
            }
        }
        if let (false, Some((_, readback)), true) = (invalid, &b.pool.stats, b.stats_used != 0) {
            let stats_offset = b.slice as usize * STATS_CAPACITY as usize * STATS_BYTES;
            let stats_range = D3D12_RANGE {
                Begin: stats_offset,
                End: stats_offset + b.stats_used as usize * STATS_BYTES,
            };
            if readback.Map(0, Some(&stats_range), Some(&mut data)).is_ok() {
                let values = core::slice::from_raw_parts((data as *const u8).add(stats_offset) as *const u64, b.stats_used as usize * STATS_COUNT);
                for r in &b.ranges {
                    let Some(index) = r.stat else { continue };
                    let stat = p.statistics.entry(r.key).or_default();
                    stat.has_stats = true;
                    for k in 0..STATS_COUNT {
                        stat.stats[k] += values[index as usize * STATS_COUNT + k];
                    }
                }
                readback.Unmap(0, Some(&D3D12_RANGE::default()));
            }
        }
    }
    !invalid
}

/// Poll (Present): starts a requested profile, maps the completed lists, publishes the report.
pub fn poll() {
    let e = exports();
    if !ACTIVE.load(Ordering::Relaxed) && a32(e.request).load(Ordering::Relaxed) == 0 && a32(e.frame_request).load(Ordering::Relaxed) == 0 {
        return;
    }
    with(|p| {
        if !ACTIVE.load(Ordering::SeqCst) && p.pending.is_empty() {
            let frames = a32(e.frame_request).swap(0, Ordering::SeqCst).min(32);
            p.frame_mode = frames > 0;
            p.remaining = if p.frame_mode {
                POOL_LISTS
            } else {
                a32(e.request).swap(0, Ordering::SeqCst).min(256)
            };
            if p.remaining == 0 {
                return;
            }
            if p.frame_mode {
                p.start_serial = serial();
                p.end_serial = p.start_serial + frames as u64;
            }
            p.completed = 0;
            p.discarded = 0;
            p.exhausted = false;
            p.statistics.clear();
            p.lists.clear();
            p.pool = None;
            p.stats_mode = a32(e.pipeline_stats).load(Ordering::Relaxed) != 0;
            p.occ_mode = a32(e.draw_occlusion).load(Ordering::Relaxed) != 0;
            p.occ_counts = [[0; 5]; 4];
            ACTIVE.store(true, Ordering::SeqCst);
            p.deadline = Some(Instant::now() + Duration::from_secs(if p.frame_mode { 5 } else { 3 }));
        }
        // Frame mode stops taking lists once the last frame asked for has been presented.
        if p.frame_mode && serial() > p.end_serial {
            p.remaining = 0;
        }
        if p.deadline.is_some_and(|d| Instant::now() > d) {
            p.remaining = 0;
            let mut discarded = 0;
            p.lists.retain(|_, b| {
                let claimed = batch_lock(b).claimed;
                discarded += (!claimed) as u32;
                claimed
            });
            p.discarded += discarded;
        }
        let pending = core::mem::take(&mut p.pending);
        for shared in pending {
            let b = batch_lock(&shared);
            // SAFETY: the batch's own fence.
            if b.fence.as_ref().is_some_and(|f| unsafe { f.GetCompletedValue() } < b.value) {
                drop(b);
                p.pending.push(shared);
                continue;
            }
            if harvest(p, &b) {
                p.completed += 1;
            } else {
                p.discarded += 1;
            }
            drop(b);
            p.lists.retain(|_, other| !Arc::ptr_eq(other, &shared));
        }
        if p.remaining == 0 && p.pending.is_empty() && p.lists.is_empty() {
            ACTIVE.store(false, Ordering::SeqCst);
            p.pool = None;
        }
        publish(p);
    })
}

// ---- the C ABI (include/Render/SceneGpuProfile.hpp)

#[no_mangle]
pub extern "C" fn cvr_scene_profile_active() -> *const AtomicBool {
    &ACTIVE
}
/// # Safety
/// `viewports` is `count` D3D12_VIEWPORTs or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_profile_viewport(list: *mut c_void, count: u32, viewports: *const D3D12_VIEWPORT) {
    let v: &[D3D12_VIEWPORT] = if count == 0 || viewports.is_null() {
        &[]
    } else {
        // SAFETY: the caller's viewports.
        unsafe { core::slice::from_raw_parts(viewports, count as usize) }
    };
    viewport(list as usize, v)
}
#[no_mangle]
pub extern "C" fn cvr_scene_profile_draw_detail(list: *mut c_void) -> u32 {
    draw_detail(list as usize)
}
/// Runs `f` on a borrowed list (nothing for null).
fn on_list(list: *mut c_void, f: impl FnOnce(&ID3D12GraphicsCommandList)) {
    // SAFETY: the caller's ID3D12GraphicsCommandList or null.
    if let Some(l) = unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&list) } {
        f(l)
    }
}
#[no_mangle]
pub extern "C" fn cvr_scene_profile_mark(list: *mut c_void, node: u32, side: i32, pso: *mut c_void, detail: u32) {
    if ACTIVE.load(Ordering::Relaxed) {
        on_list(list, |l| mark(l, node, side, pso as u64, detail));
    }
}
#[no_mangle]
pub extern "C" fn cvr_scene_profile_end_range(list: *mut c_void) {
    if ACTIVE.load(Ordering::Relaxed) {
        on_list(list, end_range);
    }
}
#[no_mangle]
pub extern "C" fn cvr_scene_profile_close(list: *mut c_void) {
    if ACTIVE.load(Ordering::Relaxed) {
        on_list(list, close);
    }
}
#[no_mangle]
pub extern "C" fn cvr_scene_profile_reset(list: *mut c_void) {
    reset(list as usize)
}
#[no_mangle]
pub extern "C" fn cvr_scene_profile_draw_begin(list: *mut c_void) {
    if ACTIVE.load(Ordering::Relaxed) {
        on_list(list, draw_begin);
    }
}
#[no_mangle]
pub extern "C" fn cvr_scene_profile_draw_end(list: *mut c_void) {
    if ACTIVE.load(Ordering::Relaxed) {
        on_list(list, draw_end);
    }
}
/// A submission (null when empty), freed by cvr_scene_profile_submitted or cvr_scene_profile_submission_free.
///
/// # Safety
/// `lists` is `count` command-list pointers or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_profile_before_submit(count: u32, lists: *const *mut c_void) -> *mut Submission {
    if !ACTIVE.load(Ordering::Relaxed) || count == 0 || lists.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the caller's list pointers.
    let keys: Vec<usize> = unsafe { core::slice::from_raw_parts(lists, count as usize) }
        .iter()
        .map(|&l| l as usize)
        .collect();
    let s = before_submit(&keys);
    if s.0.is_empty() {
        core::ptr::null_mut()
    } else {
        Box::into_raw(Box::new(s))
    }
}
/// # Safety
/// `s` is a live submission or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_profile_submission_len(s: *const Submission) -> usize {
    // SAFETY: the caller's submission.
    unsafe { s.as_ref() }.map_or(0, |s| s.0.len())
}
/// # Safety
/// `s` came from cvr_scene_profile_before_submit (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_profile_submission_free(s: *mut Submission) {
    if !s.is_null() {
        // SAFETY: the caller's submission, freed once.
        drop(unsafe { Box::from_raw(s) });
    }
}
/// Consumes `s`.
///
/// # Safety
/// `queue` is an ID3D12CommandQueue or null; `s` as cvr_scene_profile_submission_free.
#[no_mangle]
pub unsafe extern "C" fn cvr_scene_profile_submitted(queue: *mut c_void, s: *mut Submission) {
    if s.is_null() {
        return;
    }
    // SAFETY: the caller's submission (taken over) and queue.
    unsafe {
        let s = *Box::from_raw(s);
        if let Some(q) = ID3D12CommandQueue::from_raw_borrowed(&queue) {
            submitted(q, s);
        }
    }
}
#[no_mangle]
pub extern "C" fn cvr_scene_profile_poll() {
    poll()
}
