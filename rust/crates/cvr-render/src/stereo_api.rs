//! The stereo module's exported functions, defined here (formerly src/Stereo/SyncStereo.cpp's, Hud.cpp's and
//! Profiler.cpp's, src/Render/StageReplay.cpp's and src/Stereo/NativeDefinitionTracking.cpp's): the VRCAM names and
//! switch, the view the calling thread renders, the camera objects' views, the HUD's buffers, the profiler's
//! snapshots and cuts, the G-buffer partitions and the owned execution's close. Rust's exported-functions.txt names them.
#![allow(non_snake_case)]
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::render_tls;

extern "C" {
    /// vrcam.json's component and camera names (src/Stereo/SyncStereo.cpp).
    static g_vrcam_component: [u8; 96];
    static g_vrcam_camera: [u8; 96];
    static mut CyberpunkVR_VrcamEnabled: u32;
    fn cvr_vrcam_cname_hash(name: *const u8) -> u64;
    fn cvr_hud_note_composite_inputs(hud: *const c_void, blur: *const c_void, expo: *const c_void,
                                     frame_cb: *const c_void, hud_cb: *const c_void);
    fn cvr_stereo_node_cut_set(rva: u32, rtid: u32, mode: i32);
    fn cvr_stereo_prof_snapshot_nodes(rva: *mut u32, msv: *mut f64, msm: *mut f64, cv: *mut u32, cm: *mut u32,
                                      maxn: i32) -> i32;
    fn cvr_stereo_prof_snapshot_passes(rtid: *mut u32, msv: *mut f64, msm: *mut f64, maxn: i32) -> i32;
    fn cvr_srg_vrcam_partitions_configure(enable: i32);
    fn cvr_srg_scope_partitions_configure(enable: i32);
    fn cvr_ndt_close();
}

// ---- the VRCAM names and switch
#[no_mangle]
pub extern "C" fn CyberpunkVR_VrcamComponentName() -> *const c_char {
    (&raw const g_vrcam_component).cast()
}
/// CName of the VRCAM camera component (the "vrcam_<W>x<H>" object), not of its feed: how MAIN and VRCAM are told
/// apart at the camera writer.
#[no_mangle]
pub extern "C" fn CyberpunkVR_VrcamCamNameHash() -> u64 {
    // SAFETY: the plugin's NUL-terminated name.
    unsafe { cvr_vrcam_cname_hash((&raw const g_vrcam_component).cast()) }
}
#[no_mangle]
pub extern "C" fn CyberpunkVR_VrcamCameraName() -> *const c_char {
    (&raw const g_vrcam_camera).cast()
}
#[no_mangle]
pub extern "C" fn CyberpunkVR_VrcamCtxKey() -> u64 {
    crate::moved_globals::g_vrcam_ctx_key.load(Ordering::SeqCst)
}
/// The VRCAM switch: the export, and the request to the selection through bridge/vrcam_enable.txt.
#[no_mangle]
pub extern "C" fn CyberpunkVR_SetVrcamEnabled(on: u32) {
    // SAFETY: the plugin's export.
    unsafe { core::ptr::write_volatile(&raw mut CyberpunkVR_VrcamEnabled, if on != 0 { 1 } else { 0 }) };
    let request = if on != 0 { "ENABLE" } else { "DISABLE" };
    let line = if crate::vrcam_selection::bridge_write("vrcam_enable.txt", if on != 0 { "1" } else { "0" }) {
        format!("[vrcam] request {request} -> bridge/vrcam_enable.txt\0")
    } else {
        format!("[vrcam] FAILED to write bridge/vrcam_enable.txt (request {request} ignored)\0")
    };
    crate::stereo_shared::stereo_log(line.as_ptr().cast());
}

// ---- the view the calling thread renders
/// Inside a node dispatch of the second view on this thread.
#[no_mangle]
pub extern "C" fn CyberpunkVR_IsVrcamViewActive() -> i32 {
    render_tls::vrcam_node_active() as i32
}
/// Inside a dispatch of MAIN (view key 0) -- not merely "not VRCAM".
#[no_mangle]
pub extern "C" fn CyberpunkVR_IsMainViewActive() -> i32 {
    let (known, key) = active_view();
    (known && key == 0) as i32
}
/// 0 outside a view-carrying node dispatch (the view is then unknown, not MAIN); else 1 with the key in `out`.
///
/// # Safety
/// `out` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_GetActiveViewKey(out: *mut u64) -> i32 {
    let (known, key) = active_view();
    if !known {
        return 0;
    }
    if !out.is_null() {
        // SAFETY: as the caller promises.
        unsafe { *out = key };
    }
    1
}
fn active_view() -> (bool, u64) {
    // SAFETY: this thread's block, two plain fields.
    unsafe {
        let tls = render_tls::cvr_render_tls();
        ((*tls).active_view_known, (*tls).active_view_key)
    }
}

// ---- which view owns a camera object
/// The camera objects bound to MAIN and to VRCAM (never bound in this build: every object is unbound).
static CAM_OBJ_MAIN: AtomicUsize = AtomicUsize::new(0);
static CAM_OBJ_VRCAM: AtomicUsize = AtomicUsize::new(0);
/// 0 = this object is not (yet) bound to any view, 1 = MAIN, 2 = VRCAM.
#[no_mangle]
pub extern "C" fn CyberpunkVR_ClassifyCamera(obj: *const c_void) -> i32 {
    let v = obj as usize;
    if v == 0 {
        0
    } else if v == CAM_OBJ_MAIN.load(Ordering::Acquire) {
        1
    } else if v == CAM_OBJ_VRCAM.load(Ordering::Acquire) {
        2
    } else {
        0
    }
}

// ---- the HUD's buffers
/// MAIN's exposure accumulator: the HUD is scaled by the same exposure in both eyes.
#[no_mangle]
pub extern "C" fn CyberpunkVR_GetHudExposureBuffer() -> *mut c_void {
    crate::stereo_shared::g_expo_main.load(Ordering::Acquire)
}
#[no_mangle]
pub extern "C" fn CyberpunkVR_GetFrameConstantBuffer() -> *mut c_void {
    crate::stereo_shared::g_frame_cb.load(Ordering::Acquire)
}
/// # Safety
/// The resources are null or live.
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_NoteHudCompositeInputs(hud: *const c_void, blur: *const c_void, expo: *const c_void,
                                                            frame_cb: *const c_void, hud_cb: *const c_void) {
    // SAFETY: as the caller promises.
    unsafe { cvr_hud_note_composite_inputs(hud, blur, expo, frame_cb, hud_cb) }
}

// ---- the profiler
#[no_mangle]
pub extern "C" fn CyberpunkVR_NodeCutSet(rva: u32, rtid: u32, mode: i32) {
    // SAFETY: the profiler's cut table.
    unsafe { cvr_stereo_node_cut_set(rva, rtid, mode) }
}
/// # Safety
/// Each output holds `maxn` values.
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_ProfSnapshotNodes(rva: *mut u32, msv: *mut f64, msm: *mut f64, cv: *mut u32,
                                                       cm: *mut u32, maxn: i32) -> i32 {
    // SAFETY: as the caller promises.
    unsafe { cvr_stereo_prof_snapshot_nodes(rva, msv, msm, cv, cm, maxn) }
}
/// # Safety
/// Each output holds `maxn` values.
#[no_mangle]
pub unsafe extern "C" fn CyberpunkVR_ProfSnapshotPasses(rtid: *mut u32, msv: *mut f64, msm: *mut f64, maxn: i32) -> i32 {
    // SAFETY: as the caller promises.
    unsafe { cvr_stereo_prof_snapshot_passes(rtid, msv, msm, maxn) }
}

// ---- the G-buffer partitions, the owned execution
#[no_mangle]
pub extern "C" fn CyberpunkVR_VrcamGbufferPartitionsConfigure(enable: i32) {
    // SAFETY: the stage replay's switch.
    unsafe { cvr_srg_vrcam_partitions_configure(enable) }
}
#[no_mangle]
pub extern "C" fn CyberpunkVR_ScopeGbufferPartitionsConfigure(enable: i32) {
    // SAFETY: the stage replay's switch.
    unsafe { cvr_srg_scope_partitions_configure(enable) }
}
#[no_mangle]
pub extern "C" fn CyberpunkVR_CloseOwnedExecution() {
    // SAFETY: the definition tracking's close.
    unsafe { cvr_ndt_close() }
}

// ---- the live controls' weapon aim
/// The weapon aim switch (OpenXRManager::GetWeaponAimEnable, cvr-xr's switches).
#[no_mangle]
pub extern "C" fn GetWeaponAimEnabled() -> bool {
    cvr_xr::switches::SWITCHES.weapon_aim_enable.load(Ordering::Relaxed) != 0
}
