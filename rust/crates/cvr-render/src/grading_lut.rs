//! THE GRADING COMPOSER AND GenerateTonemappingLUT (part of formerly src/Stereo/Grading.cpp).
//!
//!   compose  sub_140C944A4's composed 40-byte grading block and its arguments per view, the differing dwords every
//!            4 s ([grade]; the scanner tint, parked)
//!   LUT      sub_140F60474: the LUT source's freshness for the lend (census_draws), the optics view's viewData
//!            snapshot, the grading-CB capture around the build -- every CBV Capture.cpp's CreateCBV hook sees while
//!            this thread builds, per view, diffed every 5 s ([gradecb]) -- and the source lend: MAIN remembers where
//!            it grades from (viewData+0x640) and the second view builds from it, written into its slot for the call
//!            and restored after (off)
#![allow(unsafe_op_in_unsafe_fn, non_upper_case_globals)]
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::Mutex;

use crate::capture_hooks::{tls_ptr, Tls};
use crate::census_draws::cvr_census_lut_src_build_begin;
use cvr_engine::code;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_GradingProbe: AtomicI32;
    static CyberpunkVR_GradingSrcOff: AtomicU32;
    static CyberpunkVR_GradingSrcLend: AtomicU32;
    static CyberpunkVR_DebugGradingLends: AtomicU64;
    static CyberpunkVR_DebugGradingSrcMain: AtomicU64;
    static CyberpunkVR_DebugGradingSrcVrcam: AtomicU64;
    static CyberpunkVR_GradingSrcDisplace: AtomicU32;
    static CyberpunkVR_OpticsDataProbe: AtomicI32;
    static mut CyberpunkVR_OpticsViewData: [[u8; 4096]; 3];
    static CyberpunkVR_OpticsDataSeq: [AtomicU32; 3];
    fn cvr_cpp_upload_cpu_for_va(va: u64, need: u64) -> *const u8;
    static g_vrcam_ctx_key: AtomicU64;
    fn cvr_cpp_stereo_log(text: *const c_char);
}
fn log(text: &str) {
    if let Ok(text) = std::ffi::CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
fn diag(counter: &AtomicU64) {
    // SAFETY: an export of the C++ half.
    if unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) } != 0 {
        counter.fetch_add(1, SeqCst);
    }
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn g5(v: f32) -> String {
    cvr_crt::format(c"%.5g", &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}

type ComposeFn = unsafe extern "C" fn(*mut c_void, *mut c_void, u8, f32, *mut c_void, *mut u8);
type TonemapFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> i64;
/// The composer's and the LUT build's trampolines (registered from engine_detours.rs).
#[no_mangle]
pub static cvr_grading_lut_originals: [core::sync::atomic::AtomicUsize; 2] = [const { core::sync::atomic::AtomicUsize::new(0) }; 2];
#[repr(C)]
pub struct Runtime {
    exe_base: *const usize,
    /// cname_hash("vr_optics_feed").
    optics_feed_hash: u64,
}
// SAFETY: the game module base's global, for the process.
unsafe impl Sync for Runtime {}
/// The table, built here (formerly Stereo/Grading.cpp's).
static RT: Runtime = Runtime {
    exe_base: (&raw const crate::stereo_shared::g_exe_base).cast(),
    optics_feed_hash: crate::stereo_shared::OPTICS_FEED_HASH,
};
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const RT).cast_mut());
#[no_mangle]
pub extern "C" fn cvr_grading_lut_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Release);
}
fn rt<'a>() -> &'a Runtime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*RUNTIME.load(Acquire) }
}

// ---- the composer ----------------------------------------------------------------------------------------------

struct Grade {
    out: [[u8; 40]; 2],
    a3: [u8; 2],
    a4: [f32; 2],
    a6: [u8; 2],
    seen: [bool; 2],
}
static GRADE: Mutex<Grade> = Mutex::new(Grade {
    out: [[0; 40]; 2],
    a3: [0; 2],
    a4: [0.0; 2],
    a6: [0; 2],
    seen: [false; 2],
});
fn grading_probe_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 4000 {
        return;
    }
    let (out, a3, a4, a6) = {
        let g = GRADE.lock().unwrap_or_else(|p| p.into_inner());
        if !g.seen[0] || !g.seen[1] {
            return;
        }
        (g.out, g.a3, g.a4, g.a6)
    };
    LAST.store(now, Relaxed);
    let mut line = String::new();
    for o in (0..40).step_by(4) {
        let m = u32::from_le_bytes([out[0][o], out[0][o + 1], out[0][o + 2], out[0][o + 3]]);
        let v = u32::from_le_bytes([out[1][o], out[1][o + 1], out[1][o + 2], out[1][o + 3]]);
        if m == v {
            continue;
        }
        if line.len() < 900 - 90 {
            line.push_str(&format!(
                "+{o:02X} M={m:08X}({}) V={v:08X}({})  ",
                g5(f32::from_bits(m)),
                g5(f32::from_bits(v))
            ));
        }
    }
    log(&format!(
        "[grade] a3 M={} V={} | a4 M={} V={} | a6 M={} V={} | differing dwords: {}",
        a3[0],
        a3[1],
        g5(a4[0]),
        g5(a4[1]),
        a6[0],
        a6[1],
        if line.is_empty() {
            "(none)"
        } else {
            line.as_str()
        }
    ));
}
/// Detour_GradingCompose: the original, then its output and arguments noted per view.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_compose(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: u8,
    a4: f32,
    a5: *mut c_void,
    a6: *mut u8,
) {
    let original: ComposeFn = core::mem::transmute(cvr_grading_lut_originals[0].load(Relaxed));
    original(a1, a2, a3, a4, a5, a6);
    if CyberpunkVR_GradingProbe.load(Relaxed) == 0 || a5.is_null() {
        return;
    }
    let v = *tls_ptr::<bool>(Tls::VrcamNodeActive) as usize;
    {
        let mut g = GRADE.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = [0u8; 40];
        if !code::read_bytes(a5 as usize, &mut out) {
            return;
        }
        let mark = if a6.is_null() {
            Some(0)
        } else {
            code::read::<u8>(a6 as usize)
        };
        let Some(mark) = mark else { return };
        g.out[v] = out;
        g.a6[v] = mark;
        g.a3[v] = a3;
        g.a4[v] = a4;
        g.seen[v] = true;
    }
    grading_probe_report();
}

// ---- GenerateTonemappingLUT ------------------------------------------------------------------------------------

/// The viewData accessor (sub_14033C248).
const VIEWDATA_GET_RVA: usize = 0x33_C248;
static MAIN_GRADING_SRC: AtomicUsize = AtomicUsize::new(0);
/// 0 MAIN, 1 the second view, -1 another or unknown (the work context's view key).
unsafe fn tonemap_view_kind(a2: *mut c_void) -> i32 {
    if a2.is_null() {
        return -1;
    }
    let Some(ctx) = code::read::<usize>(a2 as usize + 0x18).filter(|c| *c != 0) else {
        return -1;
    };
    match code::read::<u64>(ctx + 0x28) {
        Some(0) => 0,
        Some(k) if k == g_vrcam_ctx_key.load(SeqCst) => 1,
        _ => -1,
    }
}
unsafe fn viewdata(a2: *mut c_void) -> usize {
    let base = *rt().exe_base;
    if base == 0 {
        return 0;
    }
    code::guarded_call4(
        base + VIEWDATA_GET_RVA,
        a2,
        core::ptr::null_mut(),
        core::ptr::null_mut(),
        core::ptr::null_mut(),
    )
    .map_or(0, |v| v as usize)
}
/// Detour_TonemapLut.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_tonemap_lut(a1: *mut c_void, a2: *mut c_void) -> i64 {
    let r = rt();
    let original: TonemapFn = core::mem::transmute(cvr_grading_lut_originals[1].load(Relaxed));
    cvr_census_lut_src_build_begin(*tls_ptr::<i32>(Tls::ViewSide));
    if CyberpunkVR_OpticsDataProbe.load(Relaxed) != 0 {
        let known = *tls_ptr::<bool>(Tls::ActiveViewKnown);
        let key = *tls_ptr::<u64>(Tls::ActiveViewKey);
        let side = if !known {
            -1
        } else if key == 0 {
            0
        } else if key == g_vrcam_ctx_key.load(SeqCst) {
            1
        } else if key == r.optics_feed_hash {
            2
        } else {
            -1
        };
        if side >= 0 {
            let vd = viewdata(a2);
            if vd != 0 {
                let s = side as usize;
                CyberpunkVR_OpticsDataSeq[s].fetch_add(1, SeqCst);
                let _ = code::read_bytes(vd, &mut (*(&raw mut CyberpunkVR_OpticsViewData))[s]);
                CyberpunkVR_OpticsDataSeq[s].fetch_add(1, SeqCst);
            }
        }
    }
    if CyberpunkVR_GradeCbProbe.load(Relaxed) != 0 {
        let kind = tonemap_view_kind(a2);
        if kind >= 0 {
            return grade_cb_build(original, a1, a2, kind);
        }
    }
    if CyberpunkVR_GradingSrcLend.load(Relaxed) == 0 {
        return original(a1, a2);
    }
    let kind = tonemap_view_kind(a2);
    let vd = if kind >= 0 { viewdata(a2) } else { 0 };
    if vd == 0 {
        return original(a1, a2);
    }
    let slot = vd + CyberpunkVR_GradingSrcOff.load(Relaxed) as usize;
    if kind == 0 {
        // MAIN: remember where it grades from.
        let cur = code::read::<usize>(slot).unwrap_or(0);
        if cur != 0 {
            MAIN_GRADING_SRC.store(cur, Release);
            CyberpunkVR_DebugGradingSrcMain.store(cur as u64, Relaxed);
        }
        return original(a1, a2);
    }
    let lend = MAIN_GRADING_SRC.load(Acquire);
    let Some(cur) = code::read::<usize>(slot) else {
        return original(a1, a2);
    };
    CyberpunkVR_DebugGradingSrcVrcam.store(cur as u64, Relaxed);
    if lend == 0 || lend == cur {
        return original(a1, a2);
    }
    if cur != 0 && CyberpunkVR_GradingSrcDisplace.load(Relaxed) == 0 {
        return original(a1, a2);
    }
    diag(&CyberpunkVR_DebugGradingLends);
    // The second view's source written for the call and restored after it.
    if !code::write::<usize>(slot, lend) {
        return original(a1, a2);
    }
    let result = original(a1, a2);
    let _ = code::write::<usize>(slot, cur);
    result
}

// ---- the grading-CB capture ----------------------------------------------------------------------------------------
//
// Everything upstream measured EQUAL between the views, yet the tables came out neutral for the second view and graded
// for MAIN -- so the difference has to be in what the LUT's pipeline is handed: its constant buffers. They live in the
// upload ring and are bound in place, so CopyBufferRegion never sees them; CreateConstantBufferView hands over the GPU
// address and the ring is mapped. Every CBV created while GenerateTonemappingLUT is on the stack is captured, in
// creation order, per view, and diffed.

/// On (the default): the grading-LUT CB capture (the scanner tint, parked).
#[no_mangle]
pub static CyberpunkVR_GradeCbProbe: AtomicI32 = AtomicI32::new(1);
const GRADE_CB_MAX: usize = 512;
const GRADE_CB_SLOTS: usize = 8;
thread_local! {
    /// 0 MAIN, 1 the second view, -1 not in the build.
    static GRADE_CB_VIEW: core::cell::Cell<i32> = const { core::cell::Cell::new(-1) };
    static GRADE_CB_IDX: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}
struct GradeCbs {
    data: [[[u8; GRADE_CB_MAX]; GRADE_CB_SLOTS]; 2],
    len: [[u32; GRADE_CB_SLOTS]; 2],
    n: [u32; 2],
}
static GCB: Mutex<GradeCbs> = Mutex::new(GradeCbs {
    data: [[[0; GRADE_CB_MAX]; GRADE_CB_SLOTS]; 2],
    len: [[0; GRADE_CB_SLOTS]; 2],
    n: [0; 2],
});
fn gcb() -> std::sync::MutexGuard<'static, GradeCbs> {
    GCB.lock().unwrap_or_else(|p| p.into_inner())
}

/// The capture around the original build: the view set for this thread, the build, the commit and the report.
unsafe fn grade_cb_build(original: TonemapFn, a1: *mut c_void, a2: *mut c_void, kind: i32) -> i64 {
    GRADE_CB_VIEW.with(|v| v.set(kind));
    GRADE_CB_IDX.with(|i| i.set(0));
    // SAFETY: the engine's own build, with its own arguments.
    let r = unsafe { original(a1, a2) };
    gcb().n[kind as usize] = GRADE_CB_IDX.with(|i| i.get());
    GRADE_CB_VIEW.with(|v| v.set(-1));
    grade_cb_report();
    r
}

/// From Capture.cpp's CreateCBV hook: the CBV's bytes while this thread builds a LUT.
#[no_mangle]
pub extern "C" fn cvr_grading_gcb_cbv_note(va: u64, size: u32) {
    // SAFETY: an export of ours, written by tools.
    if CyberpunkVR_GradeCbProbe.load(Relaxed) == 0 {
        return;
    }
    let view = GRADE_CB_VIEW.with(|v| v.get());
    let idx = GRADE_CB_IDX.with(|i| i.get());
    if view < 0 || size == 0 || size as usize > GRADE_CB_MAX || idx as usize >= GRADE_CB_SLOTS {
        return;
    }
    // SAFETY: the C++ half's ring lookup.
    let p = unsafe { cvr_cpp_upload_cpu_for_va(va, u64::from(size)) };
    if p.is_null() {
        return;
    }
    let mut tmp = [0u8; GRADE_CB_MAX];
    if code::read_bytes(p as usize, &mut tmp[..size as usize]) {
        let mut g = gcb();
        g.data[view as usize][idx as usize][..size as usize].copy_from_slice(&tmp[..size as usize]);
        g.len[view as usize][idx as usize] = size;
    }
    GRADE_CB_IDX.with(|i| i.set(idx + 1));
}

/// Every 5 s once both views were captured: per CBV the sizes, the differing dwords and the first of them.
fn grade_cb_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 5000 {
        return;
    }
    let (a, b, la, lb, na, nb) = {
        let g = gcb();
        let (na, nb) = (g.n[0], g.n[1]);
        if na == 0 || nb == 0 {
            return;
        }
        (g.data[0], g.data[1], g.len[0], g.len[1], na, nb)
    };
    LAST.store(now, Relaxed);
    // snprintf into 1200 bytes, each CBV's text only while 120 bytes are left.
    let mut line = String::new();
    let n = na.min(nb) as usize;
    let dword = |v: &[u8], o: usize| u32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]]);
    for k in 0..n.min(GRADE_CB_SLOTS) {
        if line.len() >= 1200 - 120 {
            break;
        }
        let len = la[k].min(lb[k]) as usize;
        let mut diffs = 0u32;
        let mut first = usize::MAX;
        let mut o = 0;
        while o + 4 <= len {
            if a[k][o..o + 4] != b[k][o..o + 4] {
                diffs += 1;
                if first == usize::MAX {
                    first = o;
                }
            }
            o += 4;
        }
        line.push_str(&format!("cb{}({}B/{}B d{}", k, la[k], lb[k], diffs));
        if diffs != 0 {
            let (im, iv) = (dword(&a[k], first), dword(&b[k], first));
            line.push_str(
                &cvr_crt::format(
                    c" @+%X M=%08X(%.5g) V=%08X(%.5g)",
                    &[
                        cvr_crt::Arg::U(first as u64),
                        cvr_crt::Arg::U(u64::from(im)),
                        cvr_crt::Arg::F(f64::from(f32::from_bits(im))),
                        cvr_crt::Arg::U(u64::from(iv)),
                        cvr_crt::Arg::F(f64::from(f32::from_bits(iv))),
                    ],
                )
                .to_string_lossy(),
            );
        }
        line.push_str(") ");
    }
    line.truncate(1199);
    log(&format!("[gradecb] MAIN {na} cbv / VRCAM {nb} cbv: {line}"));
}
