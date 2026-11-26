//! The stereo module's CPU profiler (formerly src/Stereo/Profiler.cpp): where the two-view CPU cost goes. QPC
//! accumulators split MAIN against VRCAM, drained once per frame from the Present hook; the per-node table keyed by the
//! node's work-function RVA (inclusive and self time per view, first-seen order); the per-(pass, node) pair table that
//! makes the views' passes comparable by content; the node timeline ring; the node cut set (a node skipped for one view
//! by RVA, live, from the overlay); the overlay's snapshots and the pipe-delimited audit dump tools/node_audit_md.py
//! reads.
//!
//! What other stereo files write -- the window accumulators, the node and pass tables, the first-seen counters -- and
//! every exported switch, counter and function stays defined in C++ (src/Stereo/Profiler.cpp, NodeDispatch.cpp, with C
//! linkage through include/Stereo/StereoInternal.hpp); Profiler.cpp forwards its functions here. The log goes through
//! the stereo module's own `cvr::log` (its switch and its rate cap are one for the whole module).
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_char, CStr};
use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::ffi::CString;
use std::sync::OnceLock;

/// cvr::detail::ProfNode: one work function's row (NodeDispatch.cpp owns the table).
#[repr(C)]
pub struct ProfNode {
    rva: AtomicUsize,
    ticks_main: AtomicI64,
    ticks_vrcam: AtomicI64,
    self_main: AtomicI64,
    self_vrcam: AtomicI64,
    calls_main: AtomicU32,
    calls_vrcam: AtomicU32,
    ord_main: AtomicU32,
    ord_vrcam: AtomicU32,
}
impl ProfNode {
    /// An empty row: the table's initial state.
    pub const fn zero() -> Self {
        ProfNode {
            rva: AtomicUsize::new(0),
            ticks_main: AtomicI64::new(0),
            ticks_vrcam: AtomicI64::new(0),
            self_main: AtomicI64::new(0),
            self_vrcam: AtomicI64::new(0),
            calls_main: AtomicU32::new(0),
            calls_vrcam: AtomicU32::new(0),
            ord_main: AtomicU32::new(0),
            ord_vrcam: AtomicU32::new(0),
        }
    }
}
/// cvr::detail::ProfPass: one SceneDrv pass id's row.
#[repr(C)]
pub struct ProfPass {
    ticks_main: AtomicI64,
    ticks_vrcam: AtomicI64,
    calls_main: AtomicU32,
    calls_vrcam: AtomicU32,
}
impl ProfPass {
    /// An empty row: the table's initial state.
    pub const fn zero() -> Self {
        ProfPass { ticks_main: AtomicI64::new(0), ticks_vrcam: AtomicI64::new(0), calls_main: AtomicU32::new(0), calls_vrcam: AtomicU32::new(0) }
    }
    /// One dispatch of this pass on `vrcam`'s side, `dt` ticks inclusive.
    pub fn add(&self, vrcam: bool, dt: i64) {
        if vrcam {
            self.ticks_vrcam.fetch_add(dt, Relaxed);
            self.calls_vrcam.fetch_add(1, Relaxed);
        } else {
            self.ticks_main.fetch_add(dt, Relaxed);
            self.calls_main.fetch_add(1, Relaxed);
        }
    }
}
/// cvr::detail::ProfSpan: one span of the node timeline.
#[repr(C)]
pub struct ProfSpan {
    start: i64,
    end: i64,
    rva: u32,
    kind: u32,
    thread: u32,
    reserved: u32,
}
const _: () = assert!(core::mem::size_of::<ProfNode>() == 56 && core::mem::size_of::<ProfPass>() == 24 && core::mem::size_of::<ProfSpan>() == 32);
pub(crate) const SPAN_COUNT: u64 = 1 << 19;

#[allow(non_upper_case_globals)]
extern "C" {
    static g_qpc_to_ms: f64;
    static g_prof_build_main_ticks: AtomicI64;
    static g_prof_build_vrcam_ticks: AtomicI64;
    static g_prof_disp_main_ticks: AtomicI64;
    static g_prof_disp_vrcam_ticks: AtomicI64;
    static g_prof_window_t0: AtomicI64;
    static g_prof_build_main_calls: AtomicU64;
    static g_prof_build_vrcam_calls: AtomicU64;
    static g_prof_disp_main_nodes: AtomicU64;
    static g_prof_disp_vrcam_nodes: AtomicU64;
    static g_prof_frames: AtomicU64;
    static g_prof_top_main: AtomicU64;
    static g_prof_top_vrcam: AtomicU64;
    static g_prof_nodes: [ProfNode; 512];
    static g_prof_scenepass: [ProfPass; 256];
    static g_prof_ord_main: AtomicU32;
    static g_prof_ord_vrcam: AtomicU32;

    static CyberpunkVR_ProfEnable: i32;
    static mut CyberpunkVR_ProfBuildMainMs: f64;
    static mut CyberpunkVR_ProfBuildVrcamMs: f64;
    static mut CyberpunkVR_ProfDispMainMs: f64;
    static mut CyberpunkVR_ProfDispVrcamMs: f64;
    static mut CyberpunkVR_ProfFrameMs: f64;
    static mut CyberpunkVR_ProfDispVrcamNodes: u32;
    static mut CyberpunkVR_ProfDispMainNodes: u32;
    static mut CyberpunkVR_NodeTimelineRing: [ProfSpan; SPAN_COUNT as usize];
    static CyberpunkVR_NodeTimelineCursor: AtomicU64;
    static CyberpunkVR_ProfDumpRequest: AtomicU32;
    static CyberpunkVR_ProfDumpCount: AtomicU32;
    static mut CyberpunkVR_DebugProfPairOverflow: u64;
    static CyberpunkVR_NodeCutEnable: i32;
    static CyberpunkVR_DebugNodeCutSkips: u64;
    static CyberpunkVR_NodeCutRequest: AtomicU64;
    static CyberpunkVR_NodeCutApplied: AtomicU32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    // The audit's configuration line.
    static CyberpunkVR_DistantReuseMode: u32;
    static CyberpunkVR_LocalShadowReuseMode: u32;
    static CyberpunkVR_GiReuseMode: u32;
    static CyberpunkVR_VrcamFlagMode: u32;
    static CyberpunkVR_OcclusionGateForce: u32;
    static CyberpunkVR_CullReuseMode: u32;
    static CyberpunkVR_LodThreshOverrideEnable: u32;
    static CyberpunkVR_LodThreshApplyMask: u32;
    static CyberpunkVR_LodThreshValue: f32;
    static CyberpunkVR_VrcamDlss: i32;
    static CyberpunkVR_VrcamComputeResolve: u32;
    static CyberpunkVR_DebugMainUpscalerGroups: u64;
    static CyberpunkVR_DebugMainAaMode: u32;
    static CyberpunkVR_DebugVrcamAaMode: u32;

    /// `cvr::log("%s", text)`: the stereo module's switch and rate cap, "[stereo] " before and a newline after.
    fn cvr_cpp_stereo_log(text: *const c_char);
}
#[link(name = "kernel32")]
extern "system" {
    fn QueryPerformanceCounter(count: *mut i64) -> i32;
    fn GetCurrentThreadId() -> u32;
}

fn log(text: String) {
    if let Ok(text) = CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
/// `%.<precision>f` as the C runtime prints it.
fn f(v: f64, precision: u8) -> String {
    let format: &CStr = match precision {
        1 => c"%.1f",
        3 => c"%.3f",
        _ => c"%.4f",
    };
    cvr_crt::format(format, &[cvr_crt::Arg::F(v)]).to_string_lossy().into_owned()
}
fn qpc_to_ms() -> f64 {
    // SAFETY: a plain read of a C++ global the stereo init sets once.
    unsafe { core::ptr::read_volatile(&raw const g_qpc_to_ms) }
}

/// prof_now: QPC ticks.
#[no_mangle]
pub extern "C" fn cvr_stereo_prof_now() -> i64 {
    let mut t = 0i64;
    // SAFETY: a plain system query into a local.
    unsafe { QueryPerformanceCounter(&mut t) };
    t
}

/// prof_timeline_add: one span into the ring tools/hitch_tests/node_timeline.py reads live.
#[no_mangle]
pub extern "C" fn cvr_stereo_prof_timeline_add(start: i64, end: i64, rva: u32, kind: u32, extra: u32) {
    // SAFETY: the C++ ring; a slot is written by whoever took it from the cursor.
    unsafe {
        let at = (CyberpunkVR_NodeTimelineCursor.fetch_add(1, Relaxed) & (SPAN_COUNT - 1)) as usize;
        let span = &mut (*(&raw mut CyberpunkVR_NodeTimelineRing))[at];
        span.start = start;
        span.end = end;
        span.rva = rva;
        span.kind = kind;
        span.thread = GetCurrentThreadId();
        span.reserved = extra;
    }
}

/// The last Present's QPC (frame wall time).
static FRAME_LAST: AtomicI64 = AtomicI64::new(0);

/// CyberpunkVR_ProfPublish: once per frame from the Present hook -- the window drained into the overlay's averages.
#[no_mangle]
pub extern "C" fn cvr_stereo_prof_publish() {
    // SAFETY: the C++ globals; the averages are written by this (Present) thread only.
    unsafe {
        let to_ms = qpc_to_ms();
        if core::ptr::read_volatile(&raw const CyberpunkVR_ProfEnable) == 0 || to_ms == 0.0 {
            return;
        }
        if CyberpunkVR_ProfDumpRequest.swap(0, AcqRel) != 0 {
            cvr_stereo_prof_dump_nodes();
            CyberpunkVR_ProfDumpCount.fetch_add(1, Release);
        }
        let now = cvr_stereo_prof_now();
        let prev = FRAME_LAST.swap(now, Relaxed);
        if prev != 0 {
            CyberpunkVR_ProfFrameMs = (now - prev) as f64 * to_ms;
        }
        g_prof_frames.fetch_add(1, Relaxed);
        // The audit window starts at the first published frame.
        let _ = g_prof_window_t0.compare_exchange(0, now, Relaxed, Relaxed);
        let bm = g_prof_build_main_ticks.swap(0, Relaxed);
        let bv = g_prof_build_vrcam_ticks.swap(0, Relaxed);
        let dm = g_prof_disp_main_ticks.swap(0, Relaxed);
        let dv = g_prof_disp_vrcam_ticks.swap(0, Relaxed);
        CyberpunkVR_ProfBuildMainMs = bm as f64 * to_ms;
        CyberpunkVR_ProfBuildVrcamMs = bv as f64 * to_ms;
        CyberpunkVR_ProfDispMainMs = dm as f64 * to_ms;
        CyberpunkVR_ProfDispVrcamMs = dv as f64 * to_ms;
        CyberpunkVR_ProfDispMainNodes = g_prof_disp_main_nodes.swap(0, Relaxed) as u32;
        CyberpunkVR_ProfDispVrcamNodes = g_prof_disp_vrcam_nodes.swap(0, Relaxed) as u32;
        g_prof_build_main_calls.swap(0, Relaxed);
        g_prof_build_vrcam_calls.swap(0, Relaxed);
    }
}

/// prof_node_add: `dt` inclusive and `self_` exclusive ticks of the node whose work function is `work`, for its view.
#[no_mangle]
pub extern "C" fn cvr_stereo_prof_node_add(work: usize, base: usize, dt: i64, self_: i64, vrcam: bool) {
    if work == 0 || base == 0 {
        return;
    }
    let rva = work.wrapping_sub(base);
    let h = ((rva >> 4) as u32).wrapping_mul(2654435761);
    // SAFETY: the C++ table of atomics.
    let nodes = unsafe { &*(&raw const g_prof_nodes) };
    for probe in 0..16u32 {
        let n = &nodes[(h.wrapping_add(probe) & 511) as usize];
        let mut cur = n.rva.load(Relaxed);
        if cur == 0 {
            cur = match n.rva.compare_exchange(0, rva, SeqCst, SeqCst) {
                Ok(_) => rva,
                Err(claimed) => claimed,
            };
        }
        if cur == rva {
            // SAFETY: the C++ first-seen counters.
            unsafe {
                if vrcam {
                    n.ticks_vrcam.fetch_add(dt, Relaxed);
                    n.self_vrcam.fetch_add(self_, Relaxed);
                    if n.calls_vrcam.fetch_add(1, Relaxed) == 0 {
                        n.ord_vrcam.store(g_prof_ord_vrcam.fetch_add(1, Relaxed) + 1, Relaxed);
                    }
                } else {
                    n.ticks_main.fetch_add(dt, Relaxed);
                    n.self_main.fetch_add(self_, Relaxed);
                    if n.calls_main.fetch_add(1, Relaxed) == 0 {
                        n.ord_main.store(g_prof_ord_main.fetch_add(1, Relaxed) + 1, Relaxed);
                    }
                }
            }
            return;
        }
    }
}

/// One (pass, node) row: which named nodes ran under which pass, per view.
struct ProfPair {
    key: AtomicU64,
    self_main: AtomicI64,
    self_vrcam: AtomicI64,
    calls_main: AtomicU32,
    calls_vrcam: AtomicU32,
    nested_main: AtomicU32,
    nested_vrcam: AtomicU32,
    owner_main: AtomicU32,
    owner_vrcam: AtomicU32,
}
impl ProfPair {
    const fn new() -> ProfPair {
        ProfPair {
            key: AtomicU64::new(0),
            self_main: AtomicI64::new(0),
            self_vrcam: AtomicI64::new(0),
            calls_main: AtomicU32::new(0),
            calls_vrcam: AtomicU32::new(0),
            nested_main: AtomicU32::new(0),
            nested_vrcam: AtomicU32::new(0),
            owner_main: AtomicU32::new(0),
            owner_vrcam: AtomicU32::new(0),
        }
    }
}
/// ~400 live rows expected (363 main / 320 vrcam).
static PAIRS: [ProfPair; 2048] = [const { ProfPair::new() }; 2048];

/// splitmix64's finaliser: the slot depends on the whole key, the pass id included.
fn pair_hash(mut x: u64) -> u32 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    x as u32
}
/// prof_pair_add.
#[no_mangle]
pub extern "C" fn cvr_stereo_prof_pair_add(rtid: u8, rva: u32, self_: i64, vrcam: bool, nested: bool, owner: bool) {
    if rva == 0 {
        return;
    }
    let key = ((rtid as u64) << 32) | rva as u64;
    let h = pair_hash(key);
    for probe in 0..64u32 {
        let p = &PAIRS[(h.wrapping_add(probe) & 2047) as usize];
        let mut cur = p.key.load(Relaxed);
        if cur == 0 {
            cur = match p.key.compare_exchange(0, key, SeqCst, SeqCst) {
                Ok(_) => key,
                Err(claimed) => claimed,
            };
        }
        if cur != key {
            continue;
        }
        if vrcam {
            p.self_vrcam.fetch_add(self_, Relaxed);
            p.calls_vrcam.fetch_add(1, Relaxed);
            if nested {
                p.nested_vrcam.fetch_add(1, Relaxed);
            }
            if owner {
                p.owner_vrcam.fetch_add(1, Relaxed);
            }
        } else {
            p.self_main.fetch_add(self_, Relaxed);
            p.calls_main.fetch_add(1, Relaxed);
            if nested {
                p.nested_main.fetch_add(1, Relaxed);
            }
            if owner {
                p.owner_main.fetch_add(1, Relaxed);
            }
        }
        return;
    }
    // Table full: rows would be lost; the dump says so.
    // SAFETY: the C++ diagnostics switch and counter.
    unsafe {
        if core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 {
            CyberpunkVR_DebugProfPairOverflow += 1;
        }
    }
}

/// Work-function RVA -> CRenderNode name: src/Stereo/NodeNames.inc (generated from the project's RE census), read in
/// its own order, searched as the C++ did.
fn node_names() -> &'static [(u32, CString)] {
    static NAMES: OnceLock<Vec<(u32, CString)>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let text = include_str!("../../../../src/Stereo/NodeNames.inc");
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("{ 0x") else { continue };
            let Some((hex, rest)) = rest.split_once(',') else { continue };
            let Some(start) = rest.find('"') else { continue };
            let Some(len) = rest[start + 1..].find('"') else { continue };
            if let (Ok(rva), Ok(name)) = (u32::from_str_radix(hex.trim(), 16), CString::new(&rest[start + 1..start + 1 + len])) {
                out.push((rva, name));
            }
        }
        out
    })
}
fn node_name(rva: u32) -> &'static CStr {
    let names = node_names();
    let (mut lo, mut hi) = (0i32, names.len() as i32 - 1);
    while lo <= hi {
        let mid = lo + ((hi - lo) >> 1);
        let m = names[mid as usize].0;
        if m == rva {
            return &names[mid as usize].1;
        }
        if m < rva {
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    c""
}
/// CyberpunkVR_ProfNodeName.
#[no_mangle]
pub extern "C" fn cvr_stereo_prof_node_name(rva: u32) -> *const c_char {
    node_name(rva).as_ptr()
}

// ---- the node cut set

static CUT_RVA: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];
/// (rtid << 8) | mode; rtid 0xFF any; mode 0 off, 1 VRCAM, 2 both, 3 MAIN.
static CUT_META: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];
static CUT_ACTIVE: AtomicI32 = AtomicI32::new(0);

/// node_cut_match: whether a rule skips this node for this view.
#[no_mangle]
pub extern "C" fn cvr_stereo_node_cut_match(rva: u32, rtid: u8, vrcam: bool) -> bool {
    if CUT_ACTIVE.load(Relaxed) <= 0 {
        return false;
    }
    for i in 0..64 {
        if CUT_RVA[i].load(Relaxed) != rva {
            continue;
        }
        let meta = CUT_META[i].load(Relaxed);
        let (mode, mrt) = (meta & 0xFF, meta >> 8);
        if mode == 0 || (mrt != 0xFF && mrt != rtid as u32) {
            continue;
        }
        if mode == 2 || (mode == 1 && vrcam) || (mode == 3 && !vrcam) {
            return true;
        }
    }
    false
}
/// CyberpunkVR_NodeCutGet.
#[no_mangle]
pub extern "C" fn cvr_stereo_node_cut_get(rva: u32, rtid: u32) -> i32 {
    for i in 0..64 {
        if CUT_RVA[i].load(Relaxed) != rva {
            continue;
        }
        let meta = CUT_META[i].load(Relaxed);
        if meta >> 8 == rtid {
            return (meta & 0xFF) as i32;
        }
    }
    0
}
/// CyberpunkVR_NodeCutSet: a rule armed, changed or cleared; every change logged.
#[no_mangle]
pub extern "C" fn cvr_stereo_node_cut_set(rva: u32, rtid: u32, mode: i32) {
    if rva == 0 {
        return;
    }
    let mut slot = None;
    for i in 0..64 {
        if CUT_RVA[i].load(Relaxed) == rva && CUT_META[i].load(Relaxed) >> 8 == rtid {
            slot = Some(i);
            break;
        }
    }
    if slot.is_none() {
        if mode == 0 {
            return;
        }
        for i in 0..64 {
            if CUT_RVA[i].compare_exchange(0, rva, SeqCst, SeqCst).is_ok() {
                CUT_META[i].store(rtid << 8, Relaxed);
                slot = Some(i);
                break;
            }
        }
    }
    let Some(slot) = slot else {
        log("[nodecut] table full".into());
        return;
    };
    CUT_META[slot].store((rtid << 8) | (mode as u32 & 0xFF), Release);
    let mut active = 0;
    for i in 0..64 {
        if CUT_RVA[i].load(Relaxed) != 0 && CUT_META[i].load(Relaxed) & 0xFF != 0 {
            active += 1;
        }
    }
    CUT_ACTIVE.store(active, Release);
    let what = match mode {
        1 => "cut-vrcam",
        2 => "cut-both",
        3 => "cut-main",
        _ => "off",
    };
    log(format!("[nodecut] rva=+0x{rva:X} rtid={rtid} mode={mode} ({what}) active={active}"));
}
/// CyberpunkVR_NodeCutPoll: the script's packed request (bits 0..31 RVA, 32..39 rtId, 40..47 mode, bit 63 set).
#[no_mangle]
pub extern "C" fn cvr_stereo_node_cut_poll() {
    // SAFETY: the C++ request and its acknowledgement.
    unsafe {
        let request = CyberpunkVR_NodeCutRequest.swap(0, AcqRel);
        if request >> 63 == 0 {
            return;
        }
        cvr_stereo_node_cut_set(request as u32, ((request >> 32) & 0xFF) as u32, ((request >> 40) & 0xFF) as i32);
        CyberpunkVR_NodeCutApplied.fetch_add(1, Release);
    }
}

// ---- the overlay's live snapshots (ms per view-frame since the last dump)

/// CyberpunkVR_ProfSnapshotNodes: the costliest nodes by self time per view-frame, at most `maxn`.
///
/// # Safety
/// The outputs hold `maxn` elements each.
#[no_mangle]
pub unsafe extern "C" fn cvr_stereo_prof_snapshot_nodes(
    out_rva: *mut u32,
    out_msv: *mut f64,
    out_msm: *mut f64,
    out_cv: *mut u32,
    out_cm: *mut u32,
    mut maxn: i32,
) -> i32 {
    #[derive(Clone, Copy)]
    struct Row {
        rva: u32,
        mv: f64,
        mm: f64,
        cv: u32,
        cm: u32,
    }
    let to_ms = qpc_to_ms();
    let fmain = g_prof_top_main.load(Relaxed).max(1) as f64;
    let fvrcam = g_prof_top_vrcam.load(Relaxed).max(1) as f64;
    let nodes = &*(&raw const g_prof_nodes);
    let mut rows: Vec<Row> = Vec::with_capacity(512);
    for n in nodes.iter() {
        let w = n.rva.load(Relaxed);
        if w == 0 {
            continue;
        }
        let r = Row {
            rva: w as u32,
            mv: n.self_vrcam.load(Relaxed) as f64 * to_ms / fvrcam,
            mm: n.self_main.load(Relaxed) as f64 * to_ms / fmain,
            cv: n.calls_vrcam.load(Relaxed),
            cm: n.calls_main.load(Relaxed),
        };
        if r.mv + r.mm > 0.0 {
            rows.push(r);
        }
    }
    let n = rows.len() as i32;
    if maxn > n {
        maxn = n;
    }
    for i in 0..maxn.max(0) as usize {
        let mut best = i;
        for j in i + 1..rows.len() {
            if rows[j].mv + rows[j].mm > rows[best].mv + rows[best].mm {
                best = j;
            }
        }
        rows.swap(i, best);
        *out_rva.add(i) = rows[i].rva;
        *out_msv.add(i) = rows[i].mv;
        *out_msm.add(i) = rows[i].mm;
        *out_cv.add(i) = rows[i].cv;
        *out_cm.add(i) = rows[i].cm;
    }
    maxn
}
/// CyberpunkVR_ProfSnapshotPasses: the costliest scene passes per frame, at most `maxn`.
///
/// # Safety
/// The outputs hold `maxn` elements each.
#[no_mangle]
pub unsafe extern "C" fn cvr_stereo_prof_snapshot_passes(out_rtid: *mut u32, out_msv: *mut f64, out_msm: *mut f64, mut maxn: i32) -> i32 {
    let to_ms = qpc_to_ms();
    let frames = g_prof_frames.load(Relaxed).max(1) as f64;
    let passes = &*(&raw const g_prof_scenepass);
    let mut rows: Vec<(u32, f64, f64)> = Vec::with_capacity(128);
    for (i, p) in passes.iter().take(128).enumerate() {
        let mv = p.ticks_vrcam.load(Relaxed) as f64 * to_ms / frames;
        let mm = p.ticks_main.load(Relaxed) as f64 * to_ms / frames;
        if mv + mm <= 0.0 {
            continue;
        }
        rows.push((i as u32, mv, mm));
    }
    let n = rows.len() as i32;
    if maxn > n {
        maxn = n;
    }
    for i in 0..maxn.max(0) as usize {
        let mut best = i;
        for j in i + 1..rows.len() {
            if rows[j].1 + rows[j].2 > rows[best].1 + rows[best].2 {
                best = j;
            }
        }
        rows.swap(i, best);
        *out_rtid.add(i) = rows[i].0;
        *out_msv.add(i) = rows[i].1;
        *out_msm.add(i) = rows[i].2;
    }
    maxn
}

/// Every toggle that can silently change what the audit measures: a capture taken with an experiment armed
/// invalidates the whole table.
unsafe fn log_config() {
    let r = |v: &u32| core::ptr::read_volatile(v);
    log(format!(
        "[prof] CFG|DistantReuse={}|LocalShadowReuse={}|GiReuse={}|VrcamFlagMode={}|OcclusionGateForce={}|CullReuseMode={}|NodeCutEnable={}|NodeCutSkips={}|LodOverride={}|LodMask={}|LodValue={}|VrcamDlss={}|VrcamComputeResolve={}|MainUpscalerGroups=0x{:03X}|MainAaMode={}|VrcamAaMode={}|frameMs={}",
        r(&*(&raw const CyberpunkVR_DistantReuseMode)),
        r(&*(&raw const CyberpunkVR_LocalShadowReuseMode)),
        r(&*(&raw const CyberpunkVR_GiReuseMode)),
        r(&*(&raw const CyberpunkVR_VrcamFlagMode)),
        r(&*(&raw const CyberpunkVR_OcclusionGateForce)),
        r(&*(&raw const CyberpunkVR_CullReuseMode)),
        core::ptr::read_volatile(&raw const CyberpunkVR_NodeCutEnable),
        core::ptr::read_volatile(&raw const CyberpunkVR_DebugNodeCutSkips),
        r(&*(&raw const CyberpunkVR_LodThreshOverrideEnable)),
        r(&*(&raw const CyberpunkVR_LodThreshApplyMask)),
        f(core::ptr::read_volatile(&raw const CyberpunkVR_LodThreshValue) as f64, 3),
        core::ptr::read_volatile(&raw const CyberpunkVR_VrcamDlss) as u32,
        r(&*(&raw const CyberpunkVR_VrcamComputeResolve)),
        // %X of a 64-bit argument: its low 32 bits.
        core::ptr::read_volatile(&raw const CyberpunkVR_DebugMainUpscalerGroups) as u32,
        r(&*(&raw const CyberpunkVR_DebugMainAaMode)),
        r(&*(&raw const CyberpunkVR_DebugVrcamAaMode)),
        f(core::ptr::read_volatile(&raw const CyberpunkVR_ProfFrameMs), 3),
    ));
}

/// CyberpunkVR_ProfDumpNodes: every node, scene pass and (pass, node) pair since the last dump, as window totals in
/// pipe-delimited lines, and every table reset.
#[no_mangle]
pub extern "C" fn cvr_stereo_prof_dump_nodes() {
    #[derive(Clone, Copy)]
    struct Row {
        rva: usize,
        tm: i64,
        tv: i64,
        sm: i64,
        sv: i64,
        cm: u32,
        cv: u32,
        om: u32,
        ov: u32,
    }
    // SAFETY: the C++ tables of atomics and counters.
    unsafe {
        let to_ms = qpc_to_ms();
        let nodes = &*(&raw const g_prof_nodes);
        let mut rows: Vec<Row> = Vec::with_capacity(512);
        for n in nodes.iter() {
            let w = n.rva.load(Relaxed);
            if w == 0 {
                continue;
            }
            let r = Row {
                rva: w,
                tm: n.ticks_main.swap(0, Relaxed),
                tv: n.ticks_vrcam.swap(0, Relaxed),
                sm: n.self_main.swap(0, Relaxed),
                sv: n.self_vrcam.swap(0, Relaxed),
                cm: n.calls_main.swap(0, Relaxed),
                cv: n.calls_vrcam.swap(0, Relaxed),
                om: n.ord_main.load(Relaxed),
                ov: n.ord_vrcam.load(Relaxed),
            };
            if r.cm | r.cv != 0 {
                rows.push(r);
            }
        }
        // By self time: inclusive double-counts parents, so self is the only column to rank or sum.
        for i in 0..rows.len() {
            let mut best = i;
            for j in i + 1..rows.len() {
                if rows[j].sm.wrapping_add(rows[j].sv) > rows[best].sm.wrapping_add(rows[best].sv) {
                    best = j;
                }
            }
            rows.swap(i, best);
        }
        let frames = g_prof_frames.swap(0, Relaxed);
        let top_main = g_prof_top_main.swap(0, Relaxed);
        let top_vrcam = g_prof_top_vrcam.swap(0, Relaxed);
        let now = cvr_stereo_prof_now();
        let t0 = g_prof_window_t0.swap(now, Relaxed);
        let win_ms = if t0 != 0 { (now - t0) as f64 * to_ms } else { 0.0 };
        log_config();
        log(format!(
            "[prof] AUDIT|nodes={}|presents={frames}|node_dispatches_main={top_main}|node_dispatches_vrcam={top_vrcam}|window_ms={}|totals_not_per_frame",
            rows.len(),
            f(win_ms, 1)
        ));
        log("[prof] HDR|ord_main|ord_vrcam|rva|name|calls_main|calls_vrcam|self_main_ms|self_vrcam_ms|incl_main_ms|incl_vrcam_ms".into());
        for r in &rows {
            let name = node_name(r.rva as u32).to_string_lossy();
            log(format!(
                "[prof] N|{}|{}|0x{:06X}|{}|{}|{}|{}|{}|{}|{}",
                r.om,
                r.ov,
                r.rva as u64,
                if name.is_empty() { "?" } else { &name },
                r.cm,
                r.cv,
                f(r.sm as f64 * to_ms, 4),
                f(r.sv as f64 * to_ms, 4),
                f(r.tm as f64 * to_ms, 4),
                f(r.tv as f64 * to_ms, 4)
            ));
        }
        // Scene passes: inclusive of the child nodes they dispatch.
        log("[prof] HDRP|rtid|calls_main|calls_vrcam|incl_main_ms|incl_vrcam_ms".into());
        let passes = &*(&raw const g_prof_scenepass);
        for (i, p) in passes.iter().enumerate() {
            let tm = p.ticks_main.swap(0, Relaxed);
            let tv = p.ticks_vrcam.swap(0, Relaxed);
            let cm = p.calls_main.swap(0, Relaxed);
            let cv = p.calls_vrcam.swap(0, Relaxed);
            if cm | cv == 0 {
                continue;
            }
            log(format!("[prof] P|{i}|{cm}|{cv}|{}|{}", f(tm as f64 * to_ms, 4), f(tv as f64 * to_ms, 4)));
        }
        // (pass, node) pairs: the pass content that makes passes comparable between the views.
        let overflow = core::ptr::read_volatile(&raw const CyberpunkVR_DebugProfPairOverflow);
        if overflow != 0 {
            log(format!("[prof] PAIRWARN|overflow={overflow}|rows_were_dropped_raise_g_prof_pairs"));
        }
        CyberpunkVR_DebugProfPairOverflow = 0;
        log("[prof] HDRR|rtid|rva|name|calls_main|calls_vrcam|nested_main|nested_vrcam|owner_main|owner_vrcam|self_main_ms|self_vrcam_ms".into());
        for p in PAIRS.iter() {
            let k = p.key.swap(0, Relaxed);
            let cm = p.calls_main.swap(0, Relaxed);
            let cv = p.calls_vrcam.swap(0, Relaxed);
            let nm = p.nested_main.swap(0, Relaxed);
            let nv = p.nested_vrcam.swap(0, Relaxed);
            let om = p.owner_main.swap(0, Relaxed);
            let ov = p.owner_vrcam.swap(0, Relaxed);
            let sm = p.self_main.swap(0, Relaxed);
            let sv = p.self_vrcam.swap(0, Relaxed);
            if k == 0 || cm | cv == 0 {
                continue;
            }
            let rva = (k & 0xFFFF_FFFF) as u32;
            let rtid = (k >> 32) as u32;
            let name = node_name(rva).to_string_lossy();
            log(format!(
                "[prof] R|{rtid}|0x{rva:06X}|{}|{cm}|{cv}|{nm}|{nv}|{om}|{ov}|{}|{}",
                if name.is_empty() { "?" } else { &name },
                f(sm as f64 * to_ms, 4),
                f(sv as f64 * to_ms, 4)
            ));
        }
    }
}
