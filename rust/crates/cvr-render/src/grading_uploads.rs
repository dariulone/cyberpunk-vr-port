//! THE BUFFER UPLOADER (formerly src/Stereo/Grading.cpp's Detour_BufUpload, sub_140334328): every node's constant
//! blocks pass here -- the engine allocates, memcpys from the caller's pointer and commits -- so a block can be
//! rewritten by shipping a COPY of ours and the engine's state is never touched.
//!
//! In order: the camera block noted, the SSR uploads counted; at DrawConeAO the AO block centred on each eye's window
//! (and the eyes' AO made one wide display); at HistogramUpdate the exposure block centred or grown; the world
//! markers' constants rewritten; the HUD composite flattened; the projection jitter (off); the wide census; the fog
//! jitter (off); the cascade sampling matrices and camera lent second view -> MAIN; the checker probes; the frame
//! constants' sway modes (mode 4 ships: the second view gets MAIN's whole block) and dither lock; the retargetable
//! block diff; the grid probe and the height-mask kill (off). Anything untouched ships as the engine wrote it.
//!
//! The upload itself stays in Grading.cpp (ForwardBufUpload: the registered-constant and source-CBV writer scopes
//! around the original), reached through the runtime; also here: the optics view's data snapshots.
#![allow(unsafe_op_in_unsafe_fn, non_upper_case_globals)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use std::sync::Mutex;

use crate::block_rewrites::{
    cvr_ao_centre_write, cvr_expo_centre_write, cvr_expo_env_write, cvr_flatten_hud_composite,
};
use crate::capture_hooks::{tls_ptr, Tls};
use crate::grading_lends::*;
use crate::grading_probes::*;
use crate::ssr_merge::cvr_ssr_merge_prepare_cone_ao;
use crate::world_markers::cvr_world_markers_rewrite_constants;
use cvr_engine::code;

/// The buffer uploader's and the constant uploader's trampolines (registered from engine_detours.rs; Grading.cpp's
/// forwards call through them).
#[no_mangle]
pub static cvr_grading_upload_originals: [core::sync::atomic::AtomicUsize; 2] = [const { core::sync::atomic::AtomicUsize::new(0) }; 2];
/// The pixel stage's b6 uploader's original (sub_14041A56C), written by the install pass.
pub static PS_B6_ORIGINAL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// The shadow mask node's work function (sub_140777D0C).
const RENDER_SHADOWMASK_RVA: usize = 0x77_7D0C;
/// ssr_merge::kShadowBlockBytes: the contact-shadow settings block.
const SHADOW_BLOCK_BYTES: usize = 64;
#[repr(C, align(16))]
struct ShadowBlock([u8; SHADOW_BLOCK_BYTES]);
/// The pixel stage's b6 uploader (formerly Grading.cpp's Detour_PsB6Upload; sub_14041A56C: allocate, copy, bind as PS
/// b6). In the shadow mask node its block is the contact-shadow settings, whose padding carries the wide merge's stamp
/// and tag (ssr_merge.rs): the block copied under a guard, prepared, and uploaded in the engine's stead.
///
/// # Safety
/// The engine's arguments.
pub unsafe extern "C" fn cvr_grading_ps_b6_upload(size: usize, src: *mut c_void) {
    type Upload = unsafe extern "C" fn(usize, *mut c_void);
    // SAFETY: the install pass wrote the original before the hook went live.
    let original: Upload = unsafe { core::mem::transmute::<usize, Upload>(PS_B6_ORIGINAL.load(Relaxed)) };
    if size == SHADOW_BLOCK_BYTES && !src.is_null() {
        // SAFETY: the C++ half's global.
        let base = unsafe { *rt().exe_base };
        // SAFETY: this thread's node state.
        if base != 0 && unsafe { *tls_ptr::<usize>(Tls::CurrentNodeWork) } == base + RENDER_SHADOWMASK_RVA {
            let mut block = ShadowBlock([0; SHADOW_BLOCK_BYTES]);
            // SAFETY: this thread's node state; the block is the caller's 64 bytes.
            let prepared = cvr_engine::code::read_bytes(src as usize, &mut block.0)
                && unsafe {
                    crate::ssr_merge::cvr_ssr_merge_prepare_shadow_mask(
                        *tls_ptr::<i32>(Tls::ViewSide),
                        *tls_ptr::<usize>(Tls::CurrentNodeView),
                        block.0.as_mut_ptr(),
                        SHADOW_BLOCK_BYTES as u32,
                    )
                };
            if prepared {
                // SAFETY: the engine copies the block before returning.
                return unsafe { original(size, block.0.as_mut_ptr().cast()) };
            }
        }
    }
    // SAFETY: the engine's arguments.
    unsafe { original(size, src) }
}

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_JitterFix: AtomicI32;
    static CyberpunkVR_WideCensus: AtomicI32;
    static CyberpunkVR_CheckerProbe: AtomicI32;
    static CyberpunkVR_CheckerPhaseFix: AtomicI32;
    static CyberpunkVR_CheckerSize: AtomicU32;
    static CyberpunkVR_SwayTimeFix: AtomicI32;
    static CyberpunkVR_SwayDiff: AtomicI32;
    static CyberpunkVR_DebugSwayTimeFills: AtomicU64;
    static CyberpunkVR_DebugDitherLockBuf: AtomicU64;
    static CyberpunkVR_BlockDiff: AtomicI32;
    static CyberpunkVR_BlockDiffNode: AtomicU32;
    static CyberpunkVR_BlockDiffSize: AtomicU32;
    static CyberpunkVR_GridProbe: AtomicI32;
    static CyberpunkVR_MaskKill: AtomicI32;
    static CyberpunkVR_MaskKillOrigin: AtomicU32;
    static CyberpunkVR_DebugMaskKills: AtomicU64;
    static mut CyberpunkVR_DebugAsymAoUploads: [u32; 16];
    static CyberpunkVR_AsymAoBlockSize: AtomicU32;
    static mut CyberpunkVR_DebugAsymAoBlock: [[f32; 64]; 2];
    static mut CyberpunkVR_DebugExpoUploads: [u32; 16];
    static mut CyberpunkVR_DebugExpoBlocks: [[[f32; 16]; 3]; 2];
    static mut CyberpunkVR_DebugSsrUploads: [u32; 16];
    static mut CyberpunkVR_DebugCamBlocks: [CamBlockNote; 64];
    static CyberpunkVR_DebugCamBlockNext: AtomicU32;
    static CyberpunkVR_OpticsDataProbe: AtomicI32;
    static CyberpunkVR_OpticsMaterialProbe: AtomicI32;
    static mut CyberpunkVR_OpticsGradeData: [[u8; 688]; 3];
    static CyberpunkVR_OpticsGradeSeq: [AtomicU32; 3];
    static mut CyberpunkVR_OpticsMaterialData: [[f32; 84]; 3];
    static mut CyberpunkVR_OpticsMaterialSize: [u32; 3];
    static CyberpunkVR_OpticsMaterialSeq: [AtomicU32; 3];
    static mut CyberpunkVR_OpticsShadowData: [[f32; 232]; 3];
    static CyberpunkVR_OpticsShadowSeq: [AtomicU32; 3];
    static g_vrcam_ctx_key: AtomicU64;
}

/// The node-specific blocks: DrawConeAO, HistogramUpdate, SSR, planar SSR, PrepareSceneRendering, the shadow cascade.
const CONE_AO_NODE_RVA: usize = 0x77_7514;
const HISTOGRAM_UPDATE_NODE_RVA: usize = 0x77_6B04;
const SSR_NODE_RVA: usize = 0x77_A100;
const SSPR_NODE_RVA: usize = 0x77_D450;
const PREPARE_SCENE_NODE_WORK_RVA: usize = 0x5A_764C;
const CASCADE_NODE_RVA: usize = 0x4E_DFE0;
const RENDERER_GLOBAL_RVA: usize = 0x3BB_CAE8;
const AO_BLOCK_MAX: usize = 1024;
const EXPO_BLOCK_MAX: usize = 256;
const CAM_BLOCK: u32 = 848;
const CASC_SAMPLE: usize = 928;
/// World markers: the canvas camera and the text uploads, and the scratch for either.
const MARKER_CAMERA_BYTES: u32 = 864;
const MARKER_TEXT_BYTES: u32 = 112;
const MARKER_SCRATCH_BYTES: usize = 944;

/// One noted camera block (864 bytes) with its node and side, for a debugger.
#[repr(C)]
pub struct CamBlockNote {
    node: u32,
    side: i32,
    seq: u32,
    pad: u32,
    block: [u8; 864],
}

#[repr(C)]
pub struct Runtime {
    exe_base: *const usize,
    /// ForwardBufUpload(idx, size, src): the original with its scopes.
    forward_buf_upload: extern "C" fn(u32, u32, *mut c_void) -> i64,
    /// ForwardCbUpload(size, src): the constant uploader's original with its snapshots.
    forward_cb_upload: extern "C" fn(u32, *mut c_void) -> i64,
    /// cname_hash("vr_optics_feed").
    optics_feed_hash: u64,
}
// SAFETY: the game module base's global and functions, for the process.
unsafe impl Sync for Runtime {}
/// constants::RegisteredUploadScope (registered_copies.rs' entries): whether it was entered.
struct RegisteredUploadScope(bool);
unsafe fn registered_upload_scope_enter(this: *mut RegisteredUploadScope, handle: u32, bytes: u32, source: *const c_void) {
    // SAFETY: the caller's scope.
    unsafe { (*this).0 = crate::registered_copies::entries::cvr_rc2_scope_enter_handle(handle, bytes, source) };
}
unsafe fn registered_upload_scope_leave(this: *mut RegisteredUploadScope) {
    // SAFETY: the caller's scope.
    if unsafe { (*this).0 } {
        crate::registered_copies::entries::cvr_rc2_scope_leave();
    }
}
type BufUpload = unsafe extern "system" fn(u32, u32, *mut c_void) -> i64;
type CbUpload = unsafe extern "system" fn(u32, *mut c_void) -> i64;
/// ForwardBufUpload (formerly Stereo/Grading.cpp's): the optics snapshots, the registered constant scope, the fog
/// constants, the camera trace and the upload writer's stamp around the buffer uploader's original.
extern "C" fn forward_buf_upload(index: u32, size: u32, src: *mut c_void) -> i64 {
    // SAFETY: the engine's upload; the scopes stay in place until they close below, in the C++ destructors' order.
    unsafe {
        cvr_grading_optics_shadow_snapshot(size, src);
        cvr_grading_optics_material_snapshot(size, src);
        let mut constants = RegisteredUploadScope(false);
        registered_upload_scope_enter(&mut constants, index, size, src);
        if size == 848 || size == 1584 {
            crate::render_parity::cvr_rp_observe_fog_constants(size, src);
        }
        crate::peer_camera::cvr_pcam_trace(index, size, src);
        let mut writer = crate::source_cbv_binding::UploadWriterStamp::default();
        crate::source_cbv_observe::cvr_scbo_writer_enter(index, size, &mut writer);
        let original: BufUpload = core::mem::transmute(cvr_grading_upload_originals[0].load(Relaxed));
        let result = original(index, size, src);
        crate::source_cbv_observe::cvr_scbo_writer_complete(&mut writer);
        crate::source_cbv_observe::cvr_scbo_writer_exit(&writer);
        registered_upload_scope_leave(&mut constants);
        result
    }
}
/// ForwardCbUpload (formerly Stereo/Grading.cpp's): the optics snapshots, the fog constants and the camera trace before
/// the constant uploader's original.
extern "C" fn forward_cb_upload(size: u32, src: *mut c_void) -> i64 {
    // SAFETY: the engine's upload.
    unsafe {
        cvr_grading_optics_shadow_snapshot(size, src);
        cvr_grading_optics_material_snapshot(size, src);
        if size == 848 || size == 1584 {
            crate::render_parity::cvr_rp_observe_fog_constants(size, src);
        }
        crate::peer_camera::cvr_pcam_trace(u32::MAX, size, src);
        let original: CbUpload = core::mem::transmute(cvr_grading_upload_originals[1].load(Relaxed));
        original(size, src)
    }
}
/// The table, built here (formerly Stereo/Grading.cpp's).
static RT: Runtime = Runtime {
    exe_base: (&raw const crate::stereo_shared::g_exe_base).cast(),
    forward_buf_upload,
    forward_cb_upload,
    optics_feed_hash: crate::stereo_shared::OPTICS_FEED_HASH,
};
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const RT).cast_mut());
#[no_mangle]
pub extern "C" fn cvr_grading_uploads_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Release);
}
fn rt<'a>() -> &'a Runtime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*RUNTIME.load(Acquire) }
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

/// The recording thread's state the uploader reads.
struct Thread {
    work: usize,
    base: usize,
    side: i32,
    vrcam: bool,
    cascade: i32,
    known: bool,
    key: u64,
    view: usize,
}
impl Thread {
    unsafe fn now() -> Self {
        Thread {
            work: *tls_ptr::<usize>(Tls::CurrentNodeWork),
            base: *rt().exe_base,
            side: *tls_ptr::<i32>(Tls::ViewSide),
            vrcam: *tls_ptr::<bool>(Tls::VrcamNodeActive),
            cascade: *tls_ptr::<i32>(Tls::CascadeIdx),
            known: *tls_ptr::<bool>(Tls::ActiveViewKnown),
            key: *tls_ptr::<u64>(Tls::ActiveViewKey),
            view: *tls_ptr::<usize>(Tls::CurrentNodeView),
        }
    }
    fn rva(&self) -> u32 {
        if self.base != 0 && self.work > self.base {
            (self.work - self.base) as u32
        } else {
            0
        }
    }
    fn at(&self, rva: usize) -> bool {
        self.base != 0 && self.work > self.base && self.work - self.base == rva
    }
    fn main_now(&self) -> bool {
        self.known && self.key == 0
    }
    fn vrcam_now(&self) -> bool {
        // SAFETY: an extern "C" atomic of the C++ half.
        let k = unsafe { g_vrcam_ctx_key.load(SeqCst) };
        self.known && k != 0 && self.key == k
    }
    fn eye(&self) -> bool {
        self.side == 0 || self.side == 1
    }
}
/// A (size | path) key's count in a debugger table of eight keys and their counts.
fn count_key(table: &mut [u32; 16], key: u32) {
    for i in 0..8 {
        if table[i] == key {
            table[8 + i] = table[8 + i].wrapping_add(1);
            break;
        }
        if table[i] == 0 {
            table[i] = key;
            table[8 + i] = 1;
            break;
        }
    }
}

// ---- the notes -------------------------------------------------------------------------------------------------

unsafe fn ao_upload_note(t: &Thread, size: u32, cb_path: bool, src: *const c_void) {
    count_key(
        &mut *(&raw mut CyberpunkVR_DebugAsymAoUploads),
        size | if cb_path { 0x8000_0000 } else { 0 },
    );
    if !src.is_null() && t.eye() && size == CyberpunkVR_AsymAoBlockSize.load(Relaxed) {
        let n = (size as usize).min(256);
        let dst = (&raw mut CyberpunkVR_DebugAsymAoBlock[t.side as usize]).cast::<u8>();
        let _ = code::read_bytes(src as usize, core::slice::from_raw_parts_mut(dst, n));
    }
}
unsafe fn expo_upload_note(t: &Thread, size: u32, cb_path: bool, src: *const c_void) {
    let slot = match size {
        24 => 0,
        28 => 1,
        56 => 2,
        _ => usize::MAX,
    };
    if !src.is_null() && slot != usize::MAX && t.eye() {
        let dst = (&raw mut CyberpunkVR_DebugExpoBlocks[t.side as usize][slot]).cast::<u8>();
        let _ = code::read_bytes(
            src as usize,
            core::slice::from_raw_parts_mut(dst, size as usize),
        );
    }
    count_key(
        &mut *(&raw mut CyberpunkVR_DebugExpoUploads),
        size | if cb_path { 0x8000_0000 } else { 0 },
    );
}
/// The SSR and planar-SSR uploads by size and path.
unsafe fn ssr_upload_note(t: &Thread, size: u32, cb_path: bool) {
    if t.base == 0 || t.work <= t.base {
        return;
    }
    let node = t.work - t.base;
    if node != SSR_NODE_RVA && node != SSPR_NODE_RVA {
        return;
    }
    let key = size
        | if cb_path { 0x8000_0000 } else { 0 }
        | if node == SSPR_NODE_RVA {
            0x4000_0000
        } else {
            0
        };
    count_key(&mut *(&raw mut CyberpunkVR_DebugSsrUploads), key);
}
/// Every 864-byte (camera) block with its node and side, into a ring of 64, with diagnostics on.
unsafe fn cam_block_note(t: &Thread, size: u32, src: *const c_void) {
    if size != 864 || src.is_null() || t.base == 0 || !diagnostics() {
        return;
    }
    let seq = CyberpunkVR_DebugCamBlockNext.fetch_add(1, Relaxed);
    let note = &mut (*(&raw mut CyberpunkVR_DebugCamBlocks))[(seq % 64) as usize];
    note.node = if t.work > t.base {
        (t.work - t.base) as u32
    } else {
        0
    };
    note.side = t.side;
    let _ = code::read_bytes(src as usize, &mut note.block);
    note.seq = seq.wrapping_add(1);
}

// ---- the optics view's data snapshots --------------------------------------------------------------------------

/// 0 MAIN, 1 the second eye, 2 the optics view, -1 neither.
fn optics_side(t: &Thread) -> i32 {
    if !t.known {
        return -1;
    }
    // SAFETY: an extern "C" atomic of the C++ half.
    let vrcam = unsafe { g_vrcam_ctx_key.load(SeqCst) };
    if t.key == 0 {
        0
    } else if t.key == vrcam {
        1
    } else if t.key == rt().optics_feed_hash {
        2
    } else {
        -1
    }
}
/// A sequence-bracketed copy (odd while being written).
unsafe fn seq_copy(seq: &AtomicU32, dst: *mut u8, src: *const c_void, n: usize) {
    seq.fetch_add(1, SeqCst);
    let _ = code::read_bytes(src as usize, core::slice::from_raw_parts_mut(dst, n));
    seq.fetch_add(1, SeqCst);
}
unsafe fn optics_shadow_snapshot(t: &Thread, size: u32, src: *const c_void) {
    if CyberpunkVR_OpticsDataProbe.load(Relaxed) == 0
        || src.is_null()
        || size != 928
        || t.work != t.base + PREPARE_SCENE_NODE_WORK_RVA
    {
        return;
    }
    let side = optics_side(t);
    if side < 0 {
        return;
    }
    let s = side as usize;
    seq_copy(
        &CyberpunkVR_OpticsShadowSeq[s],
        (&raw mut CyberpunkVR_OpticsShadowData[s]).cast(),
        src,
        size as usize,
    );
}
/// The scope materials' blocks, recognised by their marker float at byte 96 (-713.25 / -714.25 / -715.25).
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_optics_material_snapshot(size: u32, src: *const c_void) {
    if CyberpunkVR_OpticsMaterialProbe.load(Relaxed) == 0
        || src.is_null()
        || !(100..=1024).contains(&size)
    {
        return;
    }
    let Some(marker) = code::read::<f32>(src as usize + 96) else {
        return;
    };
    let kind = if marker == -713.25 {
        0
    } else if marker == -714.25 {
        1
    } else if marker == -715.25 {
        2
    } else {
        return;
    };
    let dst = (&raw mut CyberpunkVR_OpticsMaterialData[kind]).cast::<u8>();
    CyberpunkVR_OpticsMaterialSeq[kind].fetch_add(1, SeqCst);
    let _ = code::read_bytes(
        src as usize,
        core::slice::from_raw_parts_mut(dst, (size as usize).min(336)),
    );
    (*(&raw mut CyberpunkVR_OpticsMaterialSize))[kind] = size;
    CyberpunkVR_OpticsMaterialSeq[kind].fetch_add(1, SeqCst);
}
/// The grading block (688 bytes) per side, with the data probe on.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_optics_grade_snapshot(src: *const c_void) {
    if CyberpunkVR_OpticsDataProbe.load(Relaxed) == 0 {
        return;
    }
    let t = Thread::now();
    let side = optics_side(&t);
    if side < 0 {
        return;
    }
    let s = side as usize;
    seq_copy(
        &CyberpunkVR_OpticsGradeSeq[s],
        (&raw mut CyberpunkVR_OpticsGradeData[s]).cast(),
        src,
        688,
    );
}
/// The shadow setup (928 bytes at PrepareSceneRendering) per side, with the data probe on.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_optics_shadow_snapshot(size: u32, src: *const c_void) {
    optics_shadow_snapshot(&Thread::now(), size, src);
}

// ---- the buffer uploader ---------------------------------------------------------------------------------------

/// Detour_BufUpload.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_buf_upload(idx: u32, size: u32, src: *mut c_void) -> i64 {
    let forward = rt().forward_buf_upload;
    let t = Thread::now();
    let csrc = src as *const c_void;
    cam_block_note(&t, size, csrc);
    ssr_upload_note(&t, size, false);
    if t.at(CONE_AO_NODE_RVA) {
        ao_upload_note(&t, size, false, csrc);
        let mut centred = Aligned([0u8; AO_BLOCK_MAX]);
        if cvr_ao_centre_write(size, src.cast(), centred.0.as_mut_ptr(), t.side) {
            cvr_ssr_merge_prepare_cone_ao(t.side, centred.0.as_mut_ptr(), size); // the eyes' AO as one wide display
            return forward(idx, size, centred.0.as_mut_ptr().cast());
        }
    }
    if t.at(HISTOGRAM_UPDATE_NODE_RVA) {
        expo_upload_note(&t, size, false, csrc);
        let mut centred = Aligned([0u8; EXPO_BLOCK_MAX]);
        let mut ship = 0u32;
        if cvr_expo_centre_write(
            size,
            src.cast(),
            false,
            centred.0.as_mut_ptr(),
            &mut ship,
            t.side,
        ) {
            return forward(idx, ship, centred.0.as_mut_ptr().cast());
        }
        // Counts a block it cannot grow on this path.
        cvr_expo_env_write(
            size,
            src.cast(),
            false,
            centred.0.as_mut_ptr(),
            &mut ship,
            t.side,
            t.view,
        );
    }
    if size == MARKER_CAMERA_BYTES || size == MARKER_TEXT_BYTES {
        let mut scratch = Aligned([0u8; MARKER_SCRATCH_BYTES]);
        let mut ship = size;
        if cvr_world_markers_rewrite_constants(
            size,
            src.cast(),
            scratch.0.as_mut_ptr(),
            &mut ship,
            t.side,
            t.view,
        ) {
            return forward(idx, ship, scratch.0.as_mut_ptr().cast());
        }
    }
    if size == 512 || size == 432 {
        let mut flat = Aligned([0u8; 512]);
        if cvr_flatten_hud_composite(size, src.cast(), flat.0.as_mut_ptr()) {
            return forward(idx, size, flat.0.as_mut_ptr().cast());
        }
    }
    if CyberpunkVR_JitterFix.load(Relaxed) != 0 && !src.is_null() && size == CAM_BLOCK {
        if t.side == 1 {
            cvr_grading_jitter_capture(csrc);
        } else if t.side == 0 {
            let mut jittered = [0u8; CAM_BLOCK as usize];
            if cvr_grading_jitter_apply(csrc, jittered.as_mut_ptr().cast()) {
                return forward(idx, size, jittered.as_mut_ptr().cast());
            }
        }
    }
    if CyberpunkVR_WideCensus.load(Relaxed) != 0 && diagnostics() {
        cvr_grading_wide_note(csrc, size, t.side, t.rva(), t.base);
        cvr_grading_wide_report();
        cvr_grading_temporal_report();
    }
    if cvr_grading_fog_block_here(size, csrc, t.work, t.base, t.side) {
        if t.vrcam_now() {
            cvr_grading_fog_jitter_capture(csrc);
        } else if t.main_now() {
            let mut fogged = [0u8; 384];
            if cvr_grading_fog_jitter_apply(csrc, fogged.as_mut_ptr().cast()) {
                return forward(idx, size, fogged.as_mut_ptr().cast());
            }
        }
    }
    if cvr_grading_cascs_block_here(size, csrc, t.work, t.base) && t.eye() {
        let slot = cvr_grading_cascs_ordinal(t.side);
        if t.side == 1 {
            cvr_grading_cascs_capture(csrc, slot, t.side);
        } else {
            let mut lent = [0u8; CASC_SAMPLE];
            if cvr_grading_cascs_apply(csrc, lent.as_mut_ptr().cast(), slot, t.side) {
                cvr_grading_cascs_report(false);
                return forward(idx, size, lent.as_mut_ptr().cast());
            }
        }
        cvr_grading_cascs_report(false);
    }
    if cvr_grading_cascr_block_here(size, csrc, t.work, t.base, t.cascade) && t.eye() {
        if t.side == 1 {
            cvr_grading_cascr_capture(csrc, t.cascade);
        } else {
            let mut camlent = [0u8; CAM_BLOCK as usize];
            if cvr_grading_cascr_apply(csrc, camlent.as_mut_ptr().cast(), t.cascade) {
                cvr_grading_cascr_report(false);
                return forward(idx, size, camlent.as_mut_ptr().cast());
            }
        }
        cvr_grading_cascr_report(false);
    }
    let checker_probe = CyberpunkVR_CheckerProbe.load(Relaxed) != 0;
    if checker_probe && !src.is_null() {
        cvr_grading_checker_small_note(size, t.rva(), t.vrcam);
        cvr_grading_checker_small_report();
    }
    if (CyberpunkVR_CheckerPhaseFix.load(Relaxed) != 0 || checker_probe)
        && !src.is_null()
        && size == CyberpunkVR_CheckerSize.load(Relaxed)
    {
        let mut phased = [0u8; 64];
        if cvr_grading_checker_note(csrc, phased.as_mut_ptr().cast(), t.vrcam, size) {
            return forward(idx, size, phased.as_mut_ptr().cast());
        }
    }
    let sway = CyberpunkVR_SwayTimeFix.load(Relaxed);
    if sway == 5
        && size == 480
        && !src.is_null()
        && t.base != 0
        && cvr_grading_looks_like_frame_consts(csrc)
    {
        let mut forced = [0u8; 480];
        if cvr_grading_sway_time_copy(csrc, forced.as_mut_ptr().cast()) {
            if t.rva() as usize == PREPARE_SCENE_NODE_WORK_RVA {
                if t.main_now() {
                    cvr_grading_sway_block_capture(csrc);
                } else if t.vrcam_now() {
                    cvr_grading_sway_block_apply(forced.as_mut_ptr().cast());
                }
            }
            cvr_grading_sway_dither_force(forced.as_mut_ptr().cast());
            diag(&CyberpunkVR_DebugSwayTimeFills);
            return forward(idx, size, forced.as_mut_ptr().cast());
        }
    }
    if size == 480 {
        let mut locked = [0u8; 480];
        if cvr_grading_dither_lock_build(
            csrc,
            locked.as_mut_ptr().cast(),
            t.work,
            t.base,
            t.main_now(),
            t.vrcam_now(),
        ) {
            diag(&CyberpunkVR_DebugDitherLockBuf);
            return forward(idx, size, locked.as_mut_ptr().cast());
        }
    }
    if CyberpunkVR_BlockDiff.load(Relaxed) != 0 && !src.is_null() && t.base != 0 {
        let rva = t.rva();
        let (node, aimed) = (
            CyberpunkVR_BlockDiffNode.load(Relaxed),
            CyberpunkVR_BlockDiffSize.load(Relaxed),
        );
        if node == 0 {
            if size == aimed {
                cvr_grading_blk_key_note(rva, t.vrcam);
                cvr_grading_blk_keys_report();
            }
        } else if rva == node {
            cvr_grading_blk_key_note(size, t.vrcam);
            cvr_grading_blk_keys_report();
            if size == aimed && (size != 384 || cvr_grading_fog_block_is_player_view(csrc)) {
                cvr_grading_blk_note(csrc, size, t.side);
            }
        }
    }
    let (grid, kill) = (
        CyberpunkVR_GridProbe.load(Relaxed) != 0,
        CyberpunkVR_MaskKill.load(Relaxed) != 0,
    );
    if (grid || kill)
        && !src.is_null()
        && t.base != 0
        && size == 848
        && t.rva() as usize == CASCADE_NODE_RVA
    {
        if grid {
            cvr_grading_grid_note(csrc, t.vrcam);
            cvr_grading_casc_note(csrc, t.vrcam, t.cascade);
            if t.cascade >= 0 {
                cvr_grading_casc_stack_report(t.base);
            }
        }
        if kill {
            let mut killed = [0u8; 848];
            if !code::read_bytes(src as usize, &mut killed) {
                return forward(idx, size, src);
            }
            let far_away = CyberpunkVR_MaskKillOrigin.load(Relaxed).to_le_bytes();
            killed[144 * 4..145 * 4].copy_from_slice(&far_away);
            killed[145 * 4..146 * 4].copy_from_slice(&far_away);
            diag(&CyberpunkVR_DebugMaskKills);
            return forward(idx, size, killed.as_mut_ptr().cast());
        }
    }
    let sway_diff = CyberpunkVR_SwayDiff.load(Relaxed) != 0;
    if (sway != 0 || sway_diff)
        && size == 480
        && !src.is_null()
        && t.base != 0
        && t.rva() as usize == PREPARE_SCENE_NODE_WORK_RVA
    {
        if sway_diff {
            cvr_grading_sway_diff_note(csrc, t.vrcam);
        }
        let mut patched = [0u8; 480];
        let shipped = |patched: &[u8; 480]| {
            if sway_diff {
                cvr_grading_sway_shipped_note(patched.as_ptr().cast(), t.vrcam);
                cvr_grading_sway_shipped_report();
            }
        };
        if sway == 2 {
            let frame_id = code::read::<usize>(t.base + RENDERER_GLOBAL_RVA)
                .filter(|r| *r != 0)
                .and_then(|r| code::read::<u32>(r + 0x4CC8))
                .unwrap_or(0);
            if sway_diff {
                let authored = code::read::<u32>(src as usize + 113 * 4).unwrap_or(0);
                cvr_grading_sway_order_note(t.vrcam, frame_id, authored);
            }
            if frame_id != 0
                && cvr_grading_sway_frame_apply(
                    csrc,
                    patched.as_mut_ptr().cast(),
                    t.vrcam,
                    frame_id,
                )
            {
                shipped(&patched);
                return forward(idx, size, patched.as_mut_ptr().cast());
            }
            if sway_diff {
                cvr_grading_sway_shipped_note(csrc, t.vrcam);
                cvr_grading_sway_shipped_report();
            }
        } else if sway == 4 {
            if t.main_now() {
                cvr_grading_sway_block_capture(csrc);
            } else if t.vrcam_now() && cvr_grading_sway_block_apply(patched.as_mut_ptr().cast()) {
                shipped(&patched);
                return forward(idx, size, patched.as_mut_ptr().cast());
            }
        } else if sway == 1 || sway == 3 {
            let force = sway == 3;
            let mut ship = false;
            if t.main_now() {
                cvr_grading_sway_time_capture(csrc);
                if force && cvr_grading_sway_time_copy(csrc, patched.as_mut_ptr().cast()) {
                    cvr_grading_sway_dither_force(patched.as_mut_ptr().cast());
                    ship = true;
                }
            } else if t.vrcam_now()
                && cvr_grading_sway_time_apply(csrc, patched.as_mut_ptr().cast())
            {
                if force {
                    cvr_grading_sway_dither_force(patched.as_mut_ptr().cast());
                }
                ship = true;
            }
            if ship {
                shipped(&patched);
                return forward(idx, size, patched.as_mut_ptr().cast());
            }
        }
    }
    forward(idx, size, src)
}
/// A 16-byte aligned scratch block (the C++ declared these alignas(16)).
#[repr(C, align(16))]
struct Aligned<const N: usize>([u8; N]);

// ---- the notes, for the constant uploader still in Grading.cpp ------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn cvr_grading_cam_block_note(size: u32, src: *const c_void) {
    cam_block_note(&Thread::now(), size, src);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_ssr_upload_note(size: u32, cb_path: bool) {
    ssr_upload_note(&Thread::now(), size, cb_path);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_ao_node_now() -> bool {
    Thread::now().at(CONE_AO_NODE_RVA)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_ao_upload_note(size: u32, cb_path: bool, src: *const c_void) {
    ao_upload_note(&Thread::now(), size, cb_path, src);
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_expo_node_now() -> bool {
    Thread::now().at(HISTOGRAM_UPDATE_NODE_RVA)
}
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_expo_upload_note(
    size: u32,
    cb_path: bool,
    src: *const c_void,
) {
    expo_upload_note(&Thread::now(), size, cb_path, src);
}

// ---- the constant uploader and the grading block's mirror ------------------------------------------------------

extern "C" {
    static CyberpunkVR_GradeUpProbe: AtomicI32;
    static CyberpunkVR_GradeMirrorMask: AtomicU32;
    static CyberpunkVR_DebugGradeMirrors: AtomicU64;
    static CyberpunkVR_DebugGradeMainUploads: AtomicU64;
    static CyberpunkVR_DebugGradeVrcamUploads: AtomicU64;
    static CyberpunkVR_DebugGradeOtherView: AtomicU64;
    static CyberpunkVR_OpticsGradeMirror: AtomicI32;
    static CyberpunkVR_DebugDitherLockCb: AtomicU64;
    fn cvr_cpp_stereo_log(text: *const core::ffi::c_char);
}
fn log(text: &str) {
    if let Ok(text) = std::ffi::CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
/// The grading block GenerateTonemappingLUT uploads (688 bytes), and its node.
const GRADE_CB_BYTES: usize = 0x2B0;
const TONEMAP_LUT_NODE_RVA: u32 = 0xF6_0474;
/// The SSR raymarch block (the engine's 656 bytes, grown to 704 for the merge).
const SSR_ENGINE_BLOCK: u32 = 656;
const SSR_BLOCK: usize = 704;
/// The fields mirrored MAIN -> the second view, one bit of CyberpunkVR_GradeMirrorMask each (bits 17-18: the LUT's
/// scale and bias; 19-25: the descriptor indices).
const GRADE_MIRROR_OFF: [usize; 26] = [
    0x230, 0x238, 0x220, 0x248, 0x258, 0x1B8, 0x1E0, 0x270, 0x010, 0x014, 0x018, 0x024, 0x028,
    0x04C, 0x074, 0x168, 0x18C, 0x210, 0x214, 0x19C, 0x1EC, 0x1FC, 0x224, 0x23C, 0x244, 0x264,
];
/// MAIN's and the second view's last grading blocks.
static GRADE_UP: Mutex<[Option<[u8; GRADE_CB_BYTES]>; 2]> = Mutex::new([None, None]);
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
fn grade_up_capture(src: *const c_void, v: usize) -> bool {
    let mut b = [0u8; GRADE_CB_BYTES];
    if !code::read_bytes(src as usize, &mut b) {
        return false;
    }
    lock(&GRADE_UP)[v] = Some(b);
    true
}
/// Every differing dword of the two views' grading blocks, every 5 s once both were seen.
fn grade_up_report() {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = tick();
    let last = LAST.load(Relaxed);
    if last != 0 && now.wrapping_sub(last) < 5000 {
        return;
    }
    let (Some(a), Some(b)) = ({
        let g = lock(&GRADE_UP);
        (g[0], g[1])
    }) else {
        return;
    };
    LAST.store(now, Relaxed);
    let mut line = String::new();
    let mut diffs = 0;
    for o in (0..GRADE_CB_BYTES).step_by(4) {
        let m = u32::from_le_bytes([a[o], a[o + 1], a[o + 2], a[o + 3]]);
        let v = u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        if m == v {
            continue;
        }
        diffs += 1;
        if line.len() < 1500 - 70 {
            let g = |x: u32| {
                cvr_crt::format(c"%.4g", &[cvr_crt::Arg::F(f32::from_bits(x) as f64)])
                    .to_string_lossy()
                    .into_owned()
            };
            line.push_str(&format!(
                "+{o:03X} M={m:08X}({}) V={v:08X}({})  ",
                g(m),
                g(v)
            ));
        }
    }
    log(&format!(
        "[gradeup] 688B grading block, {diffs} differing dwords: {}",
        if line.is_empty() {
            "(none)"
        } else {
            line.as_str()
        }
    ));
}
fn tick() -> u64 {
    // SAFETY: a plain system query.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
/// The second view's (or the optics view's) grading block shipped with MAIN's masked fields, written into the
/// engine's block for the call and restored after it.
unsafe fn grade_up_mirror_call(
    forward: extern "C" fn(u32, *mut c_void) -> i64,
    size: u32,
    src: *mut c_void,
) -> i64 {
    let Some(main) = lock(&GRADE_UP)[0] else {
        return forward(size, src);
    };
    let mask = CyberpunkVR_GradeMirrorMask.load(Relaxed);
    let p = src as usize;
    let mut saved = [0u32; 26];
    for (k, o) in GRADE_MIRROR_OFF.iter().enumerate() {
        if mask & (1 << k) == 0 {
            continue;
        }
        let Some(old) = code::read::<u32>(p + o) else {
            return forward(size, src);
        };
        saved[k] = old;
        let mine = u32::from_le_bytes([main[*o], main[*o + 1], main[*o + 2], main[*o + 3]]);
        if !code::write::<u32>(p + o, mine) {
            return forward(size, src);
        }
    }
    let r = forward(size, src);
    for (k, o) in GRADE_MIRROR_OFF.iter().enumerate() {
        if mask & (1 << k) != 0 {
            let _ = code::write::<u32>(p + o, saved[k]);
        }
    }
    diag(&CyberpunkVR_DebugGradeMirrors);
    r
}

/// Detour_CbUpload (sub_14041A780): the constant uploader, the same rewrites as the buffer uploader where its blocks
/// pass this way, plus the SSR merge's grown block and the grading block's mirror.
#[no_mangle]
pub unsafe extern "C" fn cvr_grading_cb_upload(size: u32, src: *mut c_void) -> i64 {
    let forward = rt().forward_cb_upload;
    let t = Thread::now();
    let csrc = src as *const c_void;
    if size == 512 || size == 432 {
        let mut flat = Aligned([0u8; 512]);
        if cvr_flatten_hud_composite(size, src.cast(), flat.0.as_mut_ptr()) {
            return forward(size, flat.0.as_mut_ptr().cast());
        }
    }
    ssr_upload_note(&t, size, true);
    cam_block_note(&t, size, csrc);
    if size == SSR_ENGINE_BLOCK && t.base != 0 {
        let node = t.work.wrapping_sub(t.base);
        if node == SSR_NODE_RVA || node == SSPR_NODE_RVA {
            let mut grown = Aligned([0u8; SSR_BLOCK]);
            let mut ship = 0u32;
            let channel = if node == SSR_NODE_RVA { 0 } else { 1 };
            if crate::ssr_merge::cvr_ssr_merge_prepare(
                channel,
                t.side,
                t.view,
                src.cast(),
                size,
                grown.0.as_mut_ptr(),
                &mut ship,
            ) {
                return forward(ship, grown.0.as_mut_ptr().cast());
            }
        }
    }
    if t.at(CONE_AO_NODE_RVA) {
        ao_upload_note(&t, size, true, csrc);
        let mut centred = Aligned([0u8; AO_BLOCK_MAX]);
        if cvr_ao_centre_write(size, src.cast(), centred.0.as_mut_ptr(), t.side) {
            cvr_ssr_merge_prepare_cone_ao(t.side, centred.0.as_mut_ptr(), size);
            return forward(size, centred.0.as_mut_ptr().cast());
        }
    }
    if t.at(HISTOGRAM_UPDATE_NODE_RVA) {
        expo_upload_note(&t, size, true, csrc);
        let mut centred = Aligned([0u8; EXPO_BLOCK_MAX]);
        let mut ship = 0u32;
        if cvr_expo_centre_write(
            size,
            src.cast(),
            true,
            centred.0.as_mut_ptr(),
            &mut ship,
            t.side,
        ) {
            crate::block_rewrites::cvr_expo_acc_extend(
                centred.0.as_mut_ptr(),
                &mut ship,
                t.side,
                t.view,
            );
            return forward(ship, centred.0.as_mut_ptr().cast());
        }
        if cvr_expo_env_write(
            size,
            src.cast(),
            true,
            centred.0.as_mut_ptr(),
            &mut ship,
            t.side,
            t.view,
        ) {
            return forward(ship, centred.0.as_mut_ptr().cast());
        }
    }
    if size == MARKER_CAMERA_BYTES || size == MARKER_TEXT_BYTES {
        let mut scratch = Aligned([0u8; MARKER_SCRATCH_BYTES]);
        let mut ship = size;
        if cvr_world_markers_rewrite_constants(
            size,
            src.cast(),
            scratch.0.as_mut_ptr(),
            &mut ship,
            t.side,
            t.view,
        ) {
            return forward(ship, scratch.0.as_mut_ptr().cast());
        }
    }
    if CyberpunkVR_JitterFix.load(Relaxed) != 0 && !src.is_null() && size == CAM_BLOCK {
        if t.side == 1 {
            cvr_grading_jitter_capture(csrc);
        } else if t.side == 0 {
            let mut jittered = [0u8; CAM_BLOCK as usize];
            if cvr_grading_jitter_apply(csrc, jittered.as_mut_ptr().cast()) {
                return forward(size, jittered.as_mut_ptr().cast());
            }
        }
    }
    if CyberpunkVR_WideCensus.load(Relaxed) != 0 && diagnostics() {
        cvr_grading_wide_note(csrc, size, t.side, t.rva(), t.base);
        cvr_grading_wide_report();
        cvr_grading_temporal_report();
    }
    if cvr_grading_fog_block_here(size, csrc, t.work, t.base, t.side) {
        if t.vrcam_now() {
            cvr_grading_fog_jitter_capture(csrc);
        } else if t.main_now() {
            let mut fogged = [0u8; 384];
            if cvr_grading_fog_jitter_apply(csrc, fogged.as_mut_ptr().cast()) {
                return forward(size, fogged.as_mut_ptr().cast());
            }
        }
    }
    if cvr_grading_cascs_block_here(size, csrc, t.work, t.base) && t.eye() {
        let slot = cvr_grading_cascs_ordinal(t.side);
        if t.side == 1 {
            cvr_grading_cascs_capture(csrc, slot, t.side);
        } else {
            let mut lent = [0u8; CASC_SAMPLE];
            if cvr_grading_cascs_apply(csrc, lent.as_mut_ptr().cast(), slot, t.side) {
                cvr_grading_cascs_report(false);
                return forward(size, lent.as_mut_ptr().cast());
            }
        }
        cvr_grading_cascs_report(false);
    }
    if cvr_grading_cascr_block_here(size, csrc, t.work, t.base, t.cascade) && t.eye() {
        if t.side == 1 {
            cvr_grading_cascr_capture(csrc, t.cascade);
        } else {
            let mut camlent = [0u8; CAM_BLOCK as usize];
            if cvr_grading_cascr_apply(csrc, camlent.as_mut_ptr().cast(), t.cascade) {
                cvr_grading_cascr_report(false);
                return forward(size, camlent.as_mut_ptr().cast());
            }
        }
        cvr_grading_cascr_report(false);
    }
    if CyberpunkVR_BlockDiff.load(Relaxed) != 0 && !src.is_null() && t.base != 0 {
        if t.rva() == CyberpunkVR_BlockDiffNode.load(Relaxed)
            && size == CyberpunkVR_BlockDiffSize.load(Relaxed)
            && (size != 384 || cvr_grading_fog_block_is_player_view(csrc))
        {
            cvr_grading_blk_note(csrc, size, t.side);
        }
    }
    let checker_probe = CyberpunkVR_CheckerProbe.load(Relaxed) != 0;
    if checker_probe && !src.is_null() {
        cvr_grading_checker_small_note(size, t.rva(), t.vrcam);
        cvr_grading_checker_small_report();
    }
    if (CyberpunkVR_CheckerPhaseFix.load(Relaxed) != 0 || checker_probe)
        && !src.is_null()
        && size == CyberpunkVR_CheckerSize.load(Relaxed)
    {
        let mut phased = [0u8; 64];
        if cvr_grading_checker_note(csrc, phased.as_mut_ptr().cast(), t.vrcam, size) {
            return forward(size, phased.as_mut_ptr().cast());
        }
    }
    if size == 480 {
        let mut locked = [0u8; 480];
        if cvr_grading_dither_lock_build(
            csrc,
            locked.as_mut_ptr().cast(),
            t.work,
            t.base,
            t.main_now(),
            t.vrcam_now(),
        ) {
            diag(&CyberpunkVR_DebugDitherLockCb);
            return forward(size, locked.as_mut_ptr().cast());
        }
    }
    // ---- the grading block (688 bytes inside GenerateTonemappingLUT): MAIN's is the reference, the second view's is
    // shipped with MAIN's masked fields, the optics view's likewise; any other view ships as it is.
    if size as usize != GRADE_CB_BYTES
        || src.is_null()
        || t.base == 0
        || t.rva() != TONEMAP_LUT_NODE_RVA
    {
        return forward(size, src);
    }
    cvr_grading_optics_grade_snapshot(csrc);
    if !t.known {
        return forward(size, src);
    }
    // SAFETY: an extern "C" atomic of the C++ half.
    let vrcam_key = unsafe { g_vrcam_ctx_key.load(Acquire) };
    let grade_vrcam = vrcam_key != 0 && t.key == vrcam_key;
    let grade_main = t.key == 0;
    let main_seen = lock(&GRADE_UP)[0].is_some();
    if t.key == rt().optics_feed_hash {
        if CyberpunkVR_OpticsGradeMirror.load(Relaxed) != 0
            && CyberpunkVR_GradeMirrorMask.load(Relaxed) != 0
            && main_seen
        {
            return grade_up_mirror_call(forward, size, src);
        }
        return forward(size, src);
    }
    if !grade_vrcam && !grade_main {
        diag(&CyberpunkVR_DebugGradeOtherView);
        return forward(size, src);
    }
    diag(if grade_vrcam {
        &CyberpunkVR_DebugGradeVrcamUploads
    } else {
        &CyberpunkVR_DebugGradeMainUploads
    });
    let probe = CyberpunkVR_GradeUpProbe.load(Relaxed) != 0;
    if grade_main {
        let ok = grade_up_capture(csrc, 0);
        let r = forward(size, src);
        if ok && probe {
            grade_up_report();
        }
        return r;
    }
    if probe {
        grade_up_capture(csrc, 1);
    }
    if CyberpunkVR_GradeMirrorMask.load(Relaxed) == 0 || !main_seen {
        return forward(size, src);
    }
    grade_up_mirror_call(forward, size, src)
}
