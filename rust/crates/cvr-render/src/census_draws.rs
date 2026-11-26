//! WHAT THE DRAWS SAY, PER VIEW (part of formerly src/Stereo/CommandListCensus.cpp).
//!
//!   sight axis  the holographic sight's instance row read for each view -- rotation and, in the rows' .w, the world
//!               translation as int32 fixed point at 1/131072 (the per-view rebase cancels) -- from the upload ring
//!               when it is mapped, else read back from the GPU; both views re-armed together every few seconds
//!   LUT lend    the second view's grading LUT built from MAIN's source table, lent per root parameter inside
//!               GenerateTonemappingLUT and only while MAIN's table is ONE build old (a recycled handle is garbage)
//!   draw census the nodes that draw for one view and never for the other, and the >=3x imbalanced ones
//!   cascade     what each view puts into its own shadow cascade, per interval; the cascade draws by side
#![allow(unsafe_op_in_unsafe_fn, non_upper_case_globals)]
use core::cell::Cell;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::census_pso::{cvr_census_buf_for_va, cvr_census_filled_cpu_for_va};
use crate::single_pass::Hook;
use cvr_engine::code;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_SightAxisRepeatMs: AtomicI32;
    static CyberpunkVR_LutSrcLend: AtomicU32;
    static CyberpunkVR_DebugLutSrcLearn: AtomicU64;
    static CyberpunkVR_DebugLutSrcLend: AtomicU64;
    static CyberpunkVR_DebugLutSrcStale: AtomicU64;
    static CyberpunkVR_CascSideCensus: AtomicI32;
    static CyberpunkVR_DebugCascadeDrawsSide0: AtomicU64;
    static CyberpunkVR_DebugCascadeDrawsSide1: AtomicU64;
    static CyberpunkVR_DebugCascadeDrawsSideX: AtomicU64;
    static g_game_device: *mut c_void;
    fn cvr_cpp_command_list_hook_entry(list: *mut c_void) -> *const Hook;
    fn cvr_cpp_stereo_log(text: *const c_char);
    /// ViewConstants.cpp's upload_cpu_for_va: the mapped upload ring's bytes at a GPU VA, or null.
    fn cvr_cpp_upload_cpu_for_va(va: u64, need: u64) -> *const u8;
}
fn log(text: &str) {
    if let Ok(text) = std::ffi::CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
fn diag(counter: &AtomicU64) {
    if diagnostics() {
        counter.fetch_add(1, SeqCst);
    }
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn fmt(spec: &core::ffi::CStr, v: f64) -> String {
    cvr_crt::format(spec, &[cvr_crt::Arg::F(v)])
        .to_string_lossy()
        .into_owned()
}
fn side(vrcam: bool) -> &'static str {
    if vrcam {
        "VRCAM"
    } else {
        "MAIN "
    }
}
fn rows(r: &[f32; 12]) -> String {
    let f = |i: usize| fmt(c"%+.6f", r[i] as f64);
    format!(
        "row0=({} {} {})  row1=({} {} {})  row2=({} {} {})",
        f(0),
        f(1),
        f(2),
        f(4),
        f(5),
        f(6),
        f(8),
        f(9),
        f(10)
    )
}

// ---- the sight's axis ------------------------------------------------------------------------------------------

struct Axis {
    rb: [usize; 2],
    map: [usize; 2],
    when: [u64; 2],
    inst: [u32; 2],
}
static AXIS: Mutex<Axis> = Mutex::new(Axis {
    rb: [0; 2],
    map: [0; 2],
    when: [0; 2],
    inst: [0; 2],
});
/// 0 idle, 1 copy recorded, 2 reported.
static AXIS_STATE: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];

/// Whatever landed in the readbacks, printed once the copy has surely retired (300 ms).
#[no_mangle]
pub unsafe extern "C" fn cvr_census_sight_axis_drain() {
    let a = AXIS.lock().unwrap_or_else(|p| p.into_inner());
    for v in 0..2 {
        if AXIS_STATE[v].load(Acquire) != 1 || tick().wrapping_sub(a.when[v]) < 300 {
            continue;
        }
        AXIS_STATE[v].store(2, Release);
        if a.map[v] == 0 {
            continue;
        }
        let r: [f32; 12] = core::ptr::read_unaligned(a.map[v] as *const [f32; 12]);
        let u: [u32; 12] = core::ptr::read_unaligned(a.map[v] as *const [u32; 12]);
        // The rows' .w: the instance's translation, int32 fixed point at 1/131072 -- the sight's WORLD position.
        let w = |i: usize| fmt(c"%.5f", (u[i] as i32) as f64 / 131072.0);
        log(&format!(
            "[sightaxis] {} inst={}  {}  world=({} {} {})",
            side(v == 1),
            a.inst[v],
            rows(&r),
            w(3),
            w(7),
            w(11)
        ));
    }
}
/// The instance row at `va` copied into this view's readback (buffers need no barrier), both views re-armed
/// together every CyberpunkVR_SightAxisRepeatMs so a pair is at worst one frame apart.
#[no_mangle]
pub unsafe extern "C" fn cvr_census_sight_axis_readback(
    list: *mut c_void,
    vrcam: bool,
    sinst: u32,
    va: u64,
) {
    let v = vrcam as usize;
    let repeat = CyberpunkVR_SightAxisRepeatMs.load(Relaxed);
    if repeat > 0 && !vrcam {
        static ARMED: AtomicU64 = AtomicU64::new(0);
        let now = tick();
        if AXIS_STATE[0].load(Acquire) == 2
            && AXIS_STATE[1].load(Acquire) == 2
            && now.wrapping_sub(ARMED.load(Relaxed)) > repeat as u64
        {
            ARMED.store(now, Relaxed);
            AXIS_STATE[0].store(0, Release);
            AXIS_STATE[1].store(0, Release);
        }
    }
    if AXIS_STATE[v].load(Acquire) != 0 {
        return;
    }
    let Some(hook) = cvr_cpp_command_list_hook_entry(list).as_ref() else {
        return;
    };
    let device = g_game_device;
    if hook.cbr_original == 0 || device.is_null() {
        return;
    }
    let mut off = 0u64;
    let res = cvr_census_buf_for_va(va, 64, &mut off);
    if res.is_null() {
        static LAST: AtomicU64 = AtomicU64::new(0);
        let now = tick();
        let last = LAST.load(Relaxed);
        if last == 0 || now.wrapping_sub(last) > 10000 {
            LAST.store(now, Relaxed);
            log(&format!(
                "[sightaxis] va={va:X} not in any recorded buffer (created before the hook?)"
            ));
        }
        return;
    }
    let mut a = AXIS.lock().unwrap_or_else(|p| p.into_inner());
    if a.rb[v] == 0 {
        let Some(dev) = ID3D12Device::from_raw_borrowed(&device) else {
            return;
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_READBACK,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
            ..Default::default()
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: 64,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        };
        let mut rb: Option<ID3D12Resource> = None;
        if dev
            .CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &desc,
                D3D12_RESOURCE_STATE_COPY_DEST,
                None,
                &mut rb,
            )
            .is_err()
        {
            return;
        }
        let Some(rb) = rb else { return };
        let mut m = core::ptr::null_mut();
        let mapped = rb.Map(0, None, Some(&mut m)).is_ok();
        a.rb[v] = rb.into_raw() as usize;
        if !mapped {
            a.map[v] = 0;
            return;
        }
        a.map[v] = m as usize;
    }
    let copy: unsafe extern "system" fn(*mut c_void, *mut c_void, u64, *mut c_void, u64, u64) =
        core::mem::transmute(hook.cbr_original);
    copy(list, a.rb[v] as *mut c_void, 0, res, off, 64);
    a.inst[v] = sinst;
    a.when[v] = tick();
    AXIS_STATE[v].store(1, Release);
}
/// The instance row read straight from the upload ring (or the last upload copied there), once per view; the
/// caller falls back to the GPU readback when neither has it.
#[no_mangle]
pub unsafe extern "C" fn cvr_census_sight_axis_note(
    vrcam: bool,
    sinst: u32,
    inst_va: u64,
    inst_stride: u32,
) {
    static DONE: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
    let v = vrcam as usize;
    if DONE[v].load(Relaxed) != 0 || inst_va == 0 || inst_stride < 48 {
        return;
    }
    let va = inst_va + sinst as u64 * inst_stride as u64;
    let mut p = cvr_cpp_upload_cpu_for_va(va, inst_stride as u64);
    if p.is_null() {
        p = cvr_census_filled_cpu_for_va(va, inst_stride as u64);
    }
    if p.is_null() {
        return;
    }
    let Some(r) = code::read::<[f32; 12]>(p as usize) else {
        return;
    };
    DONE[v].store(1, Relaxed);
    log(&format!(
        "[sightaxis] {} inst={sinst}  {}",
        side(vrcam),
        rows(&r)
    ));
}

// ---- the grading LUT's source table ----------------------------------------------------------------------------

/// GenerateTonemappingLUT (sub_140F60474).
const TONEMAP_LUT_RVA: usize = 0xF6_0474;
static LUT_TABLE: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
/// MAIN's LUT builds so far, and what the second view's previous build saw.
static MAIN_BUILDS: AtomicU64 = AtomicU64::new(0);
static MAIN_BUILDS_AT_VRCAM: AtomicU64 = AtomicU64::new(0);
thread_local! {
    /// Decided once per build, read by its binds.
    static LEND_FRESH: Cell<bool> = const { Cell::new(false) };
}
/// A LUT build starting on this thread (`side`: 0 MAIN, 1 the second view, -1 neither).
#[no_mangle]
pub extern "C" fn cvr_census_lut_src_build_begin(side: i32) {
    if side == 0 {
        MAIN_BUILDS.fetch_add(1, AcqRel);
        return;
    }
    if side < 0 {
        return;
    }
    let main = MAIN_BUILDS.load(Acquire);
    let fresh = main != MAIN_BUILDS_AT_VRCAM.swap(main, AcqRel);
    LEND_FRESH.with(|f| f.set(fresh));
    if !fresh && main != 0 {
        // SAFETY: an export of the C++ half.
        let stale = unsafe { CyberpunkVR_DebugLutSrcStale.fetch_add(1, SeqCst) } + 1;
        static LAST: AtomicU64 = AtomicU64::new(0);
        let now = tick();
        if now.wrapping_sub(LAST.load(Relaxed)) > 5000 {
            LAST.store(now, Relaxed);
            log(&format!(
                "[grading] LUT source not lent: MAIN has not built its LUT since the second view's last build ({stale} times so far)"
            ));
        }
    }
}
/// A compute descriptor-table bind: inside GenerateTonemappingLUT MAIN's table is learned per root parameter and
/// the second view's replaced with it while fresh. The handle to bind is returned.
#[no_mangle]
pub extern "C" fn cvr_census_lut_src_bind(
    index: u32,
    handle: u64,
    side: i32,
    work: usize,
    base: usize,
) -> u64 {
    // SAFETY: exports of the C++ half.
    unsafe {
        if CyberpunkVR_LutSrcLend.load(Relaxed) == 0
            || base == 0
            || index >= 8
            || work != base + TONEMAP_LUT_RVA
        {
            return handle;
        }
        if side == 0 {
            LUT_TABLE[index as usize].store(handle, Release);
            diag(&CyberpunkVR_DebugLutSrcLearn);
        } else if side > 0 {
            let main = LUT_TABLE[index as usize].load(Acquire);
            if main != 0 && LEND_FRESH.with(Cell::get) {
                diag(&CyberpunkVR_DebugLutSrcLend);
                return main;
            }
        }
    }
    handle
}

// ---- the draw census -------------------------------------------------------------------------------------------

static DRAWS: Mutex<Vec<(u32, [u64; 2])>> = Mutex::new(Vec::new());
/// One draw in node `rva` for a view; the report every 15 s once both views drew.
#[no_mangle]
pub extern "C" fn cvr_census_draw_note(rva: u32, vrcam: bool) {
    {
        let mut draws = DRAWS.lock().unwrap_or_else(|p| p.into_inner());
        let i = match draws.iter().position(|d| d.0 == rva) {
            Some(i) => i,
            None if draws.len() >= 96 => return,
            None => {
                draws.push((rva, [0; 2]));
                draws.len() - 1
            }
        };
        draws[i].1[vrcam as usize] += 1;
    }
    draw_report();
}
fn draw_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 15000 {
        return;
    }
    let draws = DRAWS.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if !draws.iter().any(|d| d.1[0] != 0) || !draws.iter().any(|d| d.1[1] != 0) {
        return;
    }
    LAST.store(now, Relaxed);
    let n = draws.len();
    for pass in 0..2 {
        let mut line = String::new();
        let mut c = 0;
        for (rva, hits) in &draws {
            let (mine, other) = (hits[pass], hits[pass ^ 1]);
            if mine == 0 || other != 0 {
                continue;
            }
            c += 1;
            if line.len() < 1100 - 28 {
                line.push_str(&format!("{rva:X}({mine}) "));
            }
        }
        let (a, b) = if pass == 1 {
            ("VRCAM", "MAIN")
        } else {
            ("MAIN", "VRCAM")
        };
        log(&format!(
            "[draw] nodes that DRAW for {a} and never for {b} ({c} of {n}): {}",
            if c != 0 { line.as_str() } else { "(none)" }
        ));
    }
    // A node can draw for both views and still do almost nothing for one of them.
    let mut line = String::new();
    let mut c = 0;
    for (rva, hits) in &draws {
        let (m, v) = (hits[0], hits[1]);
        if m == 0 || v == 0 {
            continue;
        }
        let (hi, lo) = (m.max(v), m.min(v));
        if hi < lo * 3 {
            continue;
        }
        c += 1;
        if line.len() < 1100 - 44 {
            line.push_str(&format!(
                "{rva:X}(M{m}/V{v} {}x) ",
                fmt(c"%.1f", hi as f64 / lo as f64)
            ));
        }
    }
    log(&format!(
        "[draw] nodes both views draw but >=3x imbalanced ({c}): {}",
        if c != 0 { line.as_str() } else { "(none)" }
    ));
}

// ---- the shadow cascades ---------------------------------------------------------------------------------------

static CASC_DRAWS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static CASC_INST: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static CASC_SUM: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
/// One cascade draw: counted, its instances summed, and an order-independent sum over its arguments (the views
/// submit in their own order; WHICH draws is the question).
#[no_mangle]
pub extern "C" fn cvr_census_cascade_note(idx: u32, inst: u32, sinst: u32, vrcam: bool) {
    let v = vrcam as usize;
    CASC_DRAWS[v].fetch_add(1, Relaxed);
    CASC_INST[v].fetch_add(inst as u64, Relaxed);
    CASC_SUM[v].fetch_add(
        (idx as u64)
            .wrapping_mul(31)
            .wrapping_add(inst.wrapping_mul(7) as u64)
            .wrapping_add(sinst as u64),
        Relaxed,
    );
}
/// Per 5 s, the counters reset after each report (two numbers from the same seconds compare); the first, partial
/// interval is dropped.
#[no_mangle]
pub extern "C" fn cvr_census_cascade_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 5000 {
        return;
    }
    LAST.store(now, Relaxed);
    let take = |a: &[AtomicU64; 2]| [a[0].swap(0, Relaxed), a[1].swap(0, Relaxed)];
    let (d, i, h) = (take(&CASC_DRAWS), take(&CASC_INST), take(&CASC_SUM));
    if last == 0 {
        return;
    }
    let avg = |i: u64, d: u64| fmt(c"%.2f", if d != 0 { i as f64 / d as f64 } else { 0.0 });
    log(&format!(
        "[cascade] per 5 s: MAIN draws={} inst={} | VRCAM draws={} inst={} | per-draw inst avg M={} V={} | argsum M={} V={}",
        d[0],
        i[0],
        d[1],
        i[1],
        avg(i[0], d[0]),
        avg(i[1], d[1]),
        h[0],
        h[1]
    ));
}
/// The cascade node's draws by side (0, 1, anything else), with diagnostics on.
#[no_mangle]
pub extern "C" fn cvr_census_cascade_draw_seen(work: usize, base: usize, side: i32) {
    const CASCADE_NODE_RVA: usize = 0x4E_DFE0;
    // SAFETY: exports of the C++ half.
    unsafe {
        if !diagnostics()
            || CyberpunkVR_CascSideCensus.load(Relaxed) == 0
            || base == 0
            || work != base + CASCADE_NODE_RVA
        {
            return;
        }
        let slot = match side {
            0 => &CyberpunkVR_DebugCascadeDrawsSide0,
            1 => &CyberpunkVR_DebugCascadeDrawsSide1,
            _ => &CyberpunkVR_DebugCascadeDrawsSideX,
        };
        slot.fetch_add(1, SeqCst);
    }
}
