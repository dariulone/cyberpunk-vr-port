//! THE BLOCKS ONE VIEW LENDS THE OTHER ON THE WAY TO THE GPU (part of formerly src/Stereo/Grading.cpp).
//!
//!   sway       the frame constants' clocks, accumulators and dither slice (480 bytes): MAIN's authored values lent
//!              to the second view (mode 1), one set per frame for whichever view asks first (mode 2), MAIN's whole
//!              block (mode 4), the slice forced to a constant in both (mode 3)
//!   fog jitter the player view's volumetric-fog block's four advancing fields, second view -> MAIN (off: unproven)
//!   jitter     CameraShaderConsts' four advancing scalars, second view -> MAIN (off: they turned out geometric)
//!   cascs      the cascade SAMPLING matrices (CSConstants, 928 bytes at PrepareSceneRendering), second view -> MAIN,
//!              paired by ordinal within each view's turn, the scale checked per cascade (ships)
//!   cascr      the cascade CAMERA (848 bytes at RenderShadowCascade) keyed by cascade index, second view -> MAIN:
//!              there is one rasterisation, and camera and sampling matrix must belong to the same view (ships)
//!   dither lock the frame constants shipped with the slice locked on the constant uploader too (off)
//!
//! Every read of the engine's block is guarded; Grading.cpp's upload detours call these by their C++ names.
#![allow(unsafe_op_in_unsafe_fn, non_upper_case_globals)]
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Mutex;

use cvr_engine::code;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_SwayTimeMask: AtomicU32;
    static CyberpunkVR_DebugSwayTimeFills: AtomicU64;
    static CyberpunkVR_SwayDitherConst: AtomicU32;
    static CyberpunkVR_FogJitterFix: AtomicI32;
    static mut CyberpunkVR_FogBlockDump: [[f32; 96]; 2];
    static CyberpunkVR_DebugFogJitterFixes: AtomicU64;
    static CyberpunkVR_DebugJitterFixes: AtomicU64;
    static CyberpunkVR_DebugJitterUnstable: AtomicU64;
    static CyberpunkVR_CascSampleLend: AtomicI32;
    static CyberpunkVR_CascSampleProbe: AtomicI32;
    static CyberpunkVR_DebugCascSampleLends: AtomicU64;
    static CyberpunkVR_DebugCascSampleSkips: AtomicU64;
    static CyberpunkVR_CascRenderLend: AtomicI32;
    static CyberpunkVR_DebugCascRenderLends: AtomicU64;
    static CyberpunkVR_DitherLock: AtomicI32;
    fn cvr_cpp_stereo_log(text: *const c_char);
    fn cvr_mono_cascades_source_sampling(m: *const f32, count: u32);
    fn cvr_mono_cascades_main_sampling(m: *const f32, count: u32);
    fn cvr_mono_cascades_camera(index: u32, data: *const c_void, bytes: usize, source: bool);
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
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
fn c(spec: &core::ffi::CStr, v: f32) -> String {
    cvr_crt::format(spec, &[cvr_crt::Arg::F(v as f64)])
        .to_string_lossy()
        .into_owned()
}
/// `n` bytes of the engine's block, read guarded.
fn read_block<const N: usize>(src: *const c_void) -> Option<[u8; N]> {
    let mut out = [0u8; N];
    code::read_bytes(src as usize, &mut out).then_some(out)
}
fn f32_at(b: &[u8], i: usize) -> f32 {
    f32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])
}
fn put_u32(b: &mut [u8], i: usize, v: u32) {
    b[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
}

const PREPARE_SCENE_NODE_WORK_RVA: usize = 0x5A_764C;
const VOLUMETRIC_FOG_NODE_RVA: usize = 0xDD_B6B8;
const CASCADE_NODE_RVA: usize = 0x4E_DFE0;
fn node_is(work: usize, base: usize, rva: usize) -> bool {
    base != 0 && work > base && (work - base) as u32 == rva as u32
}

// ---- sway ------------------------------------------------------------------------------------------------------

const FRAME_CONSTS: usize = 480;
/// Float indices into the frame constants; bits of CyberpunkVR_SwayTimeMask address this table in order.
const SWAY_FIELDS: [usize; 10] = [2, 6, 7, 38, 39, 112, 113, 0, 4, 3];
type SwaySet = [f32; SWAY_FIELDS.len()];

struct SwayState {
    /// Mode 1: MAIN's most recent authored values.
    time: Option<SwaySet>,
    /// Mode 2: staged from MAIN for the NEXT frame, and in force for `frame_id`.
    next: Option<SwaySet>,
    frame: Option<SwaySet>,
    frame_id: u32,
    /// Mode 4: MAIN's whole block.
    block: Option<[u8; FRAME_CONSTS]>,
}
static SWAY: Mutex<SwayState> = Mutex::new(SwayState {
    time: None,
    next: None,
    frame: None,
    frame_id: 0,
    block: None,
});

fn sway_time_read(src: *const c_void) -> Option<SwaySet> {
    let b = read_block::<FRAME_CONSTS>(src)?;
    Some(core::array::from_fn(|i| f32_at(&b, SWAY_FIELDS[i])))
}
unsafe fn sway_time_write(dst: *mut c_void, v: &SwaySet) {
    let mask = CyberpunkVR_SwayTimeMask.load(Relaxed);
    let f = dst.cast::<f32>();
    for (i, field) in SWAY_FIELDS.iter().enumerate() {
        if mask & (1 << i) != 0 {
            *f.add(*field) = v[i];
        }
    }
    diag(&CyberpunkVR_DebugSwayTimeFills);
}
/// The 480-byte block copied, guarded.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_sway_time_copy(src: *const c_void, dst: *mut c_void) -> bool {
    let Some(b) = read_block::<FRAME_CONSTS>(src) else {
        return false;
    };
    core::ptr::copy_nonoverlapping(b.as_ptr(), dst.cast::<u8>(), FRAME_CONSTS);
    true
}
#[no_mangle]
pub extern "C" fn cvr_grading_sway_time_capture(src: *const c_void) {
    let Some(v) = sway_time_read(src) else { return };
    if v[0] == 0.0 {
        return; // nothing to lend yet
    }
    lock(&SWAY).time = Some(v);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_sway_time_apply(src: *const c_void, dst: *mut c_void) -> bool {
    let Some(v) = lock(&SWAY).time else {
        return false;
    };
    if !cvr_grading_sway_time_copy(src, dst) {
        return false;
    }
    sway_time_write(dst, &v);
    true
}
/// MAIN's whole block, kept once its clock is authored (zeros lent at load time would freeze the second eye).
#[no_mangle]
pub extern "C" fn cvr_grading_sway_block_capture(src: *const c_void) {
    let Some(b) = read_block::<FRAME_CONSTS>(src) else {
        return;
    };
    if f32_at(&b, 2) == 0.0 {
        return;
    }
    lock(&SWAY).block = Some(b);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_sway_block_apply(dst: *mut c_void) -> bool {
    let Some(b) = lock(&SWAY).block else {
        return false;
    };
    core::ptr::copy_nonoverlapping(b.as_ptr(), dst.cast::<u8>(), FRAME_CONSTS);
    diag(&CyberpunkVR_DebugSwayTimeFills);
    true
}
/// The dither slice's counter pair ([28].x/.y) as a constant, written as raw bits (the shader reads asuint()).
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_sway_dither_force(dst: *mut c_void) {
    let v = CyberpunkVR_SwayDitherConst.load(Relaxed);
    let b = core::slice::from_raw_parts_mut(dst.cast::<u8>(), FRAME_CONSTS);
    put_u32(b, 112, v);
    put_u32(b, 113, v);
}
/// Mode 2: one set per frame for whichever view asks first, staged from MAIN's authored block one frame ahead.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_sway_frame_apply(
    src: *const c_void,
    dst: *mut c_void,
    vrcam: bool,
    frame_id: u32,
) -> bool {
    let authored = sway_time_read(src);
    let use_set = {
        let mut s = lock(&SWAY);
        if frame_id != s.frame_id {
            s.frame_id = frame_id;
            if let Some(next) = s.next {
                s.frame = Some(next);
            }
        }
        if let Some(a) = authored.filter(|a| !vrcam && a[0] != 0.0) {
            s.next = Some(a);
        }
        s.frame
    };
    let Some(v) = use_set else { return false };
    if !cvr_grading_sway_time_copy(src, dst) {
        return false;
    }
    sway_time_write(dst, &v);
    true
}

// ---- the fog block's jitter ------------------------------------------------------------------------------------

const FOG_BLOCK: usize = 384;
/// [10].x/.y/.z and [11].z.
const FOG_JITTER_FIELDS: [usize; 4] = [40, 41, 42, 46];
static FOG_JITTER: Mutex<Option<[u32; 4]>> = Mutex::new(None);

/// The PLAYER VIEW's block: a reflection-probe face uploads the same size under the same node, and its basis is
/// exactly axis-aligned (every component 0 or +-1).
#[no_mangle]
pub extern "C" fn cvr_grading_fog_block_is_player_view(src: *const c_void) -> bool {
    let Some(b) = read_block::<36>((src as usize + 16 * 4) as *const c_void) else {
        return false;
    };
    (0..9).any(|i| {
        let a = f32_at(&b, i).abs();
        a > 1e-4 && a < 0.9999
    })
}
/// This upload is the player view's fog block (the per-side dump taken on the way, with diagnostics).
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_fog_block_here(
    size: u32,
    src: *const c_void,
    work: usize,
    base: usize,
    side: i32,
) -> bool {
    let fog_node = node_is(work, base, VOLUMETRIC_FOG_NODE_RVA);
    if size as usize == FOG_BLOCK
        && !src.is_null()
        && diagnostics()
        && (side == 0 || side == 1)
        && fog_node
        && cvr_grading_fog_block_is_player_view(src)
    {
        if let Some(b) = read_block::<FOG_BLOCK>(src) {
            let dump = &mut *(&raw mut CyberpunkVR_FogBlockDump);
            for i in 0..96 {
                dump[side as usize][i] = f32_at(&b, i);
            }
        }
    }
    if CyberpunkVR_FogJitterFix.load(Relaxed) == 0
        || size as usize != FOG_BLOCK
        || src.is_null()
        || base == 0
        || !fog_node
    {
        return false;
    }
    cvr_grading_fog_block_is_player_view(src)
}
#[no_mangle]
pub extern "C" fn cvr_grading_fog_jitter_capture(src: *const c_void) {
    let Some(b) = read_block::<FOG_BLOCK>(src) else {
        return;
    };
    *lock(&FOG_JITTER) = Some(
        FOG_JITTER_FIELDS
            .map(|i| u32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])),
    );
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_fog_jitter_apply(
    src: *const c_void,
    dst: *mut c_void,
) -> bool {
    let Some(v) = *lock(&FOG_JITTER) else {
        return false;
    };
    let Some(mut b) = read_block::<FOG_BLOCK>(src) else {
        return false;
    };
    for (k, i) in FOG_JITTER_FIELDS.iter().enumerate() {
        put_u32(&mut b, *i, v[k]);
    }
    core::ptr::copy_nonoverlapping(b.as_ptr(), dst.cast::<u8>(), FOG_BLOCK);
    diag(&CyberpunkVR_DebugFogJitterFixes);
    true
}

// ---- the projection jitter -------------------------------------------------------------------------------------

const CAM_BLOCK: usize = 848;
const JITTER_FIELDS: [usize; 4] = [3, 18, 67, 115];
static JITTER: Mutex<Option<[u32; 4]>> = Mutex::new(None);

/// Captured, and the assumption checked: a value that differs between a view's uploads in one frame is per-pass.
#[no_mangle]
pub extern "C" fn cvr_grading_jitter_capture(src: *const c_void) {
    let Some(b) = read_block::<CAM_BLOCK>(src) else {
        return;
    };
    let v = JITTER_FIELDS
        .map(|i| u32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]));
    let mut j = lock(&JITTER);
    if j.is_some_and(|old| old != v) {
        // SAFETY: an export of the C++ half.
        diag(unsafe { &CyberpunkVR_DebugJitterUnstable });
    }
    *j = Some(v);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_jitter_apply(src: *const c_void, dst: *mut c_void) -> bool {
    let Some(v) = *lock(&JITTER) else {
        return false;
    };
    let Some(mut b) = read_block::<CAM_BLOCK>(src) else {
        return false;
    };
    for (k, i) in JITTER_FIELDS.iter().enumerate() {
        put_u32(&mut b, *i, v[k]);
    }
    core::ptr::copy_nonoverlapping(b.as_ptr(), dst.cast::<u8>(), CAM_BLOCK);
    diag(&CyberpunkVR_DebugJitterFixes);
    true
}

// ---- the cascade sampling matrices -----------------------------------------------------------------------------

const CASC_SAMPLE: usize = 928;
/// cascadeMatrix[0] begins at byte 96; four affine matrices.
const CASC_MAT_FLOAT: usize = 24;
use crate::cascade_sampling::{
    active_cascade_matrices, cascs_mm, same_cascade_sampling_layout, CASC_MATRICES, CASC_MAT_FLOATS,
};
const CASC_SLOTS: usize = 4;

fn cascs_read_mat(src: *const c_void) -> Option<[f32; CASC_MAT_FLOATS]> {
    let b = read_block::<{ CASC_MAT_FLOATS * 4 }>(
        (src as usize + CASC_MAT_FLOAT * 4) as *const c_void,
    )?;
    Some(core::array::from_fn(|i| f32_at(&b, i)))
}
struct Cascs {
    slot: [[f32; CASC_MAT_FLOATS]; CASC_SLOTS],
    slot_all: [[u8; CASC_SAMPLE]; CASC_SLOTS],
    /// Slots the second view filled in its current turn, the next ordinal, and whose turn it is as last seen here.
    have_n: i32,
    idx: i32,
    side: i32,
    dbasis: [f32; CASC_MATRICES],
    dtrans: [f32; CASC_MATRICES],
    dmm: [f32; CASC_MATRICES],
    wskip: f32,
    /// mm between the second view's own slot 0 and slot k.
    spread: f32,
    pairs: u64,
    /// MAIN reached an ordinal the second view never filled.
    miss: u64,
    said: bool,
}
static CASCS: Mutex<Cascs> = Mutex::new(Cascs {
    slot: [[0.0; CASC_MAT_FLOATS]; CASC_SLOTS],
    slot_all: [[0; CASC_SAMPLE]; CASC_SLOTS],
    have_n: 0,
    idx: 0,
    side: -2,
    dbasis: [0.0; CASC_MATRICES],
    dtrans: [0.0; CASC_MATRICES],
    dmm: [0.0; CASC_MATRICES],
    wskip: 0.0,
    spread: 0.0,
    pairs: 0,
    miss: 0,
    said: false,
});

/// The sun-cascade setup: 928 bytes at PrepareSceneRendering with a valid active cascade prefix (the node uploads
/// this size twice per view per frame).
#[no_mangle]
pub extern "C" fn cvr_grading_cascs_block_here(
    size: u32,
    src: *const c_void,
    work: usize,
    base: usize,
) -> bool {
    // SAFETY: an export of the C++ half.
    if unsafe { CyberpunkVR_CascSampleLend.load(Relaxed) } == 0
        || size as usize != CASC_SAMPLE
        || src.is_null()
        || base == 0
    {
        return false;
    }
    node_is(work, base, PREPARE_SCENE_NODE_WORK_RVA)
        && cascs_read_mat(src).is_some_and(|m| active_cascade_matrices(&m) > 0)
}
/// A turn begins when the side changes at this node; within a turn the uploads are numbered from zero.
#[no_mangle]
pub extern "C" fn cvr_grading_cascs_ordinal(side: i32) -> i32 {
    let mut s = lock(&CASCS);
    if side != s.side {
        s.side = side;
        s.idx = 0;
        if side == 1 {
            s.have_n = 0; // the second view starts filling its slots afresh
        }
    }
    s.idx += 1;
    s.idx - 1
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_cascs_capture(src: *const c_void, slot: i32, side: i32) {
    if !(0..CASC_SLOTS as i32).contains(&slot) {
        return;
    }
    let Some(mat) = cascs_read_mat(src) else {
        return;
    };
    let Some(all) = read_block::<CASC_SAMPLE>(src) else {
        return;
    };
    let mut s = lock(&CASCS);
    let k = slot as usize;
    s.slot[k] = mat;
    s.slot_all[k] = all;
    if slot == 0 && side == 1 {
        cvr_mono_cascades_source_sampling(mat.as_ptr(), active_cascade_matrices(&mat) as u32);
    }
    if slot + 1 > s.have_n {
        s.have_n = slot + 1;
    }
    if slot > 0 {
        for m in 0..CASC_MATRICES {
            let mm = cascs_mm(&s.slot[0][m * 16..], &s.slot[k][m * 16..]);
            if mm > s.spread {
                s.spread = mm;
            }
        }
    }
}
/// MAIN's upload given the second view's matrices of the matching slot (the last filled one when its turn ran
/// short -- coverage beats pairing: the slots hold one matrix, and missing a slot makes MAIN alternate between two
/// shadow transforms), when every active cascade's scale matches.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_cascs_apply(
    src: *const c_void,
    dst: *mut c_void,
    slot: i32,
    side: i32,
) -> bool {
    let mut s = lock(&CASCS);
    if s.have_n <= 0 || slot < 0 {
        return false;
    }
    let mut slot = slot;
    if slot >= CASC_SLOTS as i32 || slot >= s.have_n {
        s.miss += 1;
        slot = s.have_n - 1;
    }
    let k = slot as usize;
    let Some(mine) = cascs_read_mat(src) else {
        return false;
    };
    let mut worst = 0.0f32;
    if !same_cascade_sampling_layout(&mine, &s.slot[k], &mut worst) {
        if worst > s.wskip {
            s.wskip = worst; // a skip says HOW far off
        }
        diag(&CyberpunkVR_DebugCascSampleSkips);
        return false;
    }
    // The pair noted BEFORE the lend.
    s.pairs += 1;
    for m in 0..CASC_MATRICES {
        let (pa, pb) = (&mine[m * 16..m * 16 + 16], &s.slot[k][m * 16..m * 16 + 16]);
        let mut db = 0f32;
        for r in 0..3 {
            for col in 0..3 {
                db = db.max((pa[r * 4 + col] - pb[r * 4 + col]).abs());
            }
        }
        let dt = (0..3)
            .map(|col| (pa[12 + col] - pb[12 + col]).abs())
            .fold(0f32, f32::max);
        let mm = cascs_mm(pa, pb);
        s.dbasis[m] = s.dbasis[m].max(db);
        s.dtrans[m] = s.dtrans[m].max(dt);
        s.dmm[m] = s.dmm[m].max(mm);
    }
    let lend = CyberpunkVR_CascSampleLend.load(Relaxed);
    if lend >= 2 {
        core::ptr::copy_nonoverlapping(s.slot_all[k].as_ptr(), dst.cast::<u8>(), CASC_SAMPLE);
    } else {
        let Some(mut b) = read_block::<CASC_SAMPLE>(src) else {
            return false;
        };
        for (i, v) in s.slot[k].iter().enumerate() {
            b[(CASC_MAT_FLOAT + i) * 4..(CASC_MAT_FLOAT + i) * 4 + 4]
                .copy_from_slice(&v.to_le_bytes());
        }
        core::ptr::copy_nonoverlapping(b.as_ptr(), dst.cast::<u8>(), CASC_SAMPLE);
    }
    diag(&CyberpunkVR_DebugCascSampleLends);
    if side == 0 {
        let shipped = dst.cast::<u8>().add(CASC_MAT_FLOAT * 4).cast::<f32>();
        cvr_mono_cascades_main_sampling(shipped, active_cascade_matrices(&s.slot[k]) as u32);
    }
    if !s.said {
        s.said = true;
        cascs_report_locked(&mut s, true);
    }
    true
}
/// The first landing says so whatever the DEBUG box says (a lend that never matched is otherwise indistinguishable
/// from one that worked); the repeating line stays behind the probe.
#[no_mangle]
pub extern "C" fn cvr_grading_cascs_report(first: bool) {
    let mut s = lock(&CASCS);
    cascs_report_locked(&mut s, first);
}
fn cascs_report_locked(s: &mut Cascs, first: bool) {
    // SAFETY: exports of the C++ half.
    let (probe, lends, skips, mode) = unsafe {
        (
            CyberpunkVR_CascSampleProbe.load(Relaxed),
            CyberpunkVR_DebugCascSampleLends.load(Relaxed),
            CyberpunkVR_DebugCascSampleSkips.load(Relaxed),
            CyberpunkVR_CascSampleLend.load(Relaxed),
        )
    };
    if !first {
        if probe == 0 {
            return;
        }
        static LAST: AtomicU64 = AtomicU64::new(0);
        let now = tick();
        let last = LAST.load(Relaxed);
        if last != 0 && now.wrapping_sub(last) < 5000 {
            return;
        }
        LAST.store(now, Relaxed);
    }
    let e = |v: f32| c(c"%.2e", v);
    let mm = |v: f32| c(c"%.1f", v);
    log(&format!(
        "[cascsample] cascadeMatrix, MAIN against the second view BEFORE the lend, worst in this interval: casc0 basis={} trans={} ({} mm along the light) | casc1 basis={} trans={} ({} mm) | casc2 basis={} trans={} ({} mm) | casc3 basis={} trans={} ({} mm) | pairs={} lends={} skipped={} (worst scale mismatch {}) unpaired={} | the second view's OWN slots differ by {} mm | mode={}",
        e(s.dbasis[0]),
        e(s.dtrans[0]),
        mm(s.dmm[0]),
        e(s.dbasis[1]),
        e(s.dtrans[1]),
        mm(s.dmm[1]),
        e(s.dbasis[2]),
        e(s.dtrans[2]),
        mm(s.dmm[2]),
        e(s.dbasis[3]),
        e(s.dtrans[3]),
        mm(s.dmm[3]),
        s.pairs,
        lends,
        skips,
        c(c"%.3f", s.wskip),
        s.miss,
        mm(s.spread),
        mode
    ));
    if first {
        return; // the first line's numbers stay in the interval it reports
    }
    s.dbasis = [0.0; CASC_MATRICES];
    s.dtrans = [0.0; CASC_MATRICES];
    s.dmm = [0.0; CASC_MATRICES];
    s.wskip = 0.0;
    s.spread = 0.0;
    s.pairs = 0;
    s.miss = 0;
}

// ---- the cascade camera ----------------------------------------------------------------------------------------

const CASC_RENDER_SLOTS: usize = 8;
struct Cascr {
    cam: [[u8; CAM_BLOCK]; CASC_RENDER_SLOTS],
    have: [bool; CASC_RENDER_SLOTS],
    dmax: [f32; CASC_RENDER_SLOTS],
    pairs: u64,
    said: bool,
}
static CASCR: Mutex<Cascr> = Mutex::new(Cascr {
    cam: [[0; CAM_BLOCK]; CASC_RENDER_SLOTS],
    have: [false; CASC_RENDER_SLOTS],
    dmax: [0.0; CASC_RENDER_SLOTS],
    pairs: 0,
    said: false,
});
/// The 848-byte camera at RenderShadowCascade, keyed by CASCADE INDEX (two cascades upload it under one node).
#[no_mangle]
pub extern "C" fn cvr_grading_cascr_block_here(
    size: u32,
    src: *const c_void,
    work: usize,
    base: usize,
    cascade: i32,
) -> bool {
    // SAFETY: an export of the C++ half.
    if unsafe { CyberpunkVR_CascRenderLend.load(Relaxed) } == 0
        || size as usize != CAM_BLOCK
        || src.is_null()
        || base == 0
    {
        return false;
    }
    (0..CASC_RENDER_SLOTS as i32).contains(&cascade) && node_is(work, base, CASCADE_NODE_RVA)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_cascr_capture(src: *const c_void, idx: i32) {
    let Some(all) = read_block::<CAM_BLOCK>(src) else {
        return;
    };
    let mut s = lock(&CASCR);
    s.cam[idx as usize] = all;
    cvr_mono_cascades_camera(idx as u32, all.as_ptr().cast(), CAM_BLOCK, true);
    s.have[idx as usize] = true;
}
/// MAIN's cascade camera replaced with the second view's whole block, the gap before the copy recorded.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_cascr_apply(
    src: *const c_void,
    dst: *mut c_void,
    idx: i32,
) -> bool {
    let k = idx as usize;
    let mut s = lock(&CASCR);
    if !s.have[k] {
        return false;
    }
    let Some(mine) = read_block::<CAM_BLOCK>(src) else {
        return false;
    };
    let dmax = (0..CAM_BLOCK / 4)
        .map(|i| (f32_at(&mine, i) - f32_at(&s.cam[k], i)).abs())
        .fold(0f32, |a, d| if d > a { d } else { a });
    if dmax > s.dmax[k] {
        s.dmax[k] = dmax;
    }
    s.pairs += 1;
    core::ptr::copy_nonoverlapping(s.cam[k].as_ptr(), dst.cast::<u8>(), CAM_BLOCK);
    cvr_mono_cascades_camera(idx as u32, dst, CAM_BLOCK, false);
    diag(&CyberpunkVR_DebugCascRenderLends);
    if !s.said {
        s.said = true;
        cascr_report_locked(&mut s, true);
    }
    true
}
#[no_mangle]
pub extern "C" fn cvr_grading_cascr_report(first: bool) {
    let mut s = lock(&CASCR);
    cascr_report_locked(&mut s, first);
}
fn cascr_report_locked(s: &mut Cascr, first: bool) {
    if !first {
        // SAFETY: an export of the C++ half.
        if unsafe { CyberpunkVR_CascSampleProbe.load(Relaxed) } == 0 {
            return;
        }
        static LAST: AtomicU64 = AtomicU64::new(0);
        let now = tick();
        let last = LAST.load(Relaxed);
        if last != 0 && now.wrapping_sub(last) < 5000 {
            return;
        }
        LAST.store(now, Relaxed);
    }
    let f = |v: f32| c(c"%.4f", v);
    // SAFETY: an export of the C++ half.
    let lends = unsafe { CyberpunkVR_DebugCascRenderLends.load(Relaxed) };
    log(&format!(
        "[cascrender] 848B cascade camera, worst per-float gap between the views before the lend: casc0={} casc1={} casc2={} casc3={} | pairs={} lends={}",
        f(s.dmax[0]),
        f(s.dmax[1]),
        f(s.dmax[2]),
        f(s.dmax[3]),
        s.pairs,
        lends
    ));
    if first {
        return;
    }
    s.dmax = [0.0; CASC_RENDER_SLOTS];
    s.pairs = 0;
}

// ---- the frame constants and the dither lock -------------------------------------------------------------------

/// The frame constants: the clock at [0].z runs into the hundreds of thousands of seconds.
#[no_mangle]
pub extern "C" fn cvr_grading_looks_like_frame_consts(src: *const c_void) -> bool {
    code::read::<f32>(src as usize + 2 * 4).is_some_and(|clock| clock > 1000.0)
}
/// `out` filled (480 bytes) with the frame constants shipped with the slice locked, carrying mode 4's mirroring
/// itself at PrepareSceneRendering (returning through its own buffer bypasses the path that would apply it).
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_dither_lock_build(
    src: *const c_void,
    out: *mut c_void,
    work: usize,
    base: usize,
    main_now: bool,
    vrcam_now: bool,
) -> bool {
    if CyberpunkVR_DitherLock.load(Relaxed) == 0 || src.is_null() || base == 0 {
        return false;
    }
    if !cvr_grading_looks_like_frame_consts(src) || !cvr_grading_sway_time_copy(src, out) {
        return false;
    }
    if node_is(work, base, PREPARE_SCENE_NODE_WORK_RVA) {
        if main_now {
            cvr_grading_sway_block_capture(src);
        } else if vrcam_now {
            cvr_grading_sway_block_apply(out);
        }
    }
    cvr_grading_sway_dither_force(out);
    true
}
