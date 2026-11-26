//! THE TAKEN-OVER CAMERA AND THE BRAINDANCE CAMERA (formerly src/Core/VrCore.cpp): whose composition owns the frame
//! (BdPushOwnsComposition, LocateOwnsTakeover), the device camera's liveness (DeviceCamActive) and its fov handed back,
//! the lens found in the taken-over entity's own component list, re-read every frame unless it is our own write
//! coming back, and the head written into it; the braindance camera found by pose among the objects the classifier
//! does not recognise -- orientation first, the position only for what is aimed the way the scene camera is, a
//! camera's fov last -- with its census line a second; the releases; the scene stick yaw's gate; the shared slots'
//! mapping; the camera math the hooks share (NormalizeQuat, MulQuat, IsPlausibleCameraSpan).
//!
//! The state stays the C++ half's -- dozens of files read it -- and is reached through the runtime; what only these
//! functions use (what we last wrote into the lens, the census) lives here.
#![allow(non_upper_case_globals)]
use crate::camera_owner as o;
use crate::vrcore_exports as v;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::Mutex;

use cvr_crt::{format, Arg};
use cvr_engine::code;

/// The C++ half's state.
#[repr(C)]
pub struct Runtime {
    bd_active: *const AtomicI32,
    bd_scene_pose_valid: *const AtomicI32,
    bd_scene_quat: *const [f32; 4],
    bd_scene_pos_fp: *const [AtomicI32; 3],
    bd_want_fov_milli: *const AtomicI32,
    bd_cam_found: *const AtomicI32,
    cam_obj_device: *const AtomicUsize,
    cam_name_offset: *const AtomicI32,
    remote_cam_on: *const AtomicI32,
    device_cam_last_ms: *const AtomicU64,
    lens_comp: *const AtomicUsize,
    takeover_entity: *const AtomicUsize,
    dev_cam_fov_saved: *const AtomicI32,
    dev_cam_fov_orig: *const f32,
    dev_cam_pos_fp: *const [AtomicI32; 3],
    dev_cam_pos_valid: *const AtomicI32,
    dev_cam_base: *mut [f32; 4],
    dev_cam_base_valid: *const AtomicI32,
    dev_cam_aim_yaw: *mut f32,
    dev_cam_aim_pitch: *mut f32,
    dev_cam_aim_valid: *const AtomicI32,
    dev_cam_view_valid: *const AtomicI32,
    normal_fov_override: *const f32,
    verbose_log: *const i32,
}
// SAFETY: the table holds addresses of statics.
unsafe impl Sync for Runtime {}
/// The table, built here (formerly VrCore.cpp's, attached at its static initialisation): the globals are
/// vrcore_exports.rs's and camera_owner.rs's.
static RT: Runtime = Runtime {
    bd_active: &raw const v::g_bdActive,
    bd_scene_pose_valid: &raw const v::g_bdScenePoseValid,
    bd_scene_quat: (&raw const v::g_bdSceneQuat).cast(),
    bd_scene_pos_fp: &raw const v::g_bdScenePosFP,
    bd_want_fov_milli: &raw const v::g_bdWantFovMilli,
    bd_cam_found: &raw const v::g_bdCamFound,
    cam_obj_device: &raw const o::CAM_OBJ_DEVICE,
    cam_name_offset: &raw const o::CAM_NAME_OFFSET,
    remote_cam_on: &raw const v::g_remoteCamOn,
    device_cam_last_ms: &raw const o::g_deviceCamLastMs,
    lens_comp: &raw const v::g_lensComp,
    takeover_entity: &raw const v::g_takeoverEntity,
    dev_cam_fov_saved: &raw const v::g_devCamFovSaved,
    dev_cam_fov_orig: (&raw const v::g_devCamFovOrig).cast(),
    dev_cam_pos_fp: &raw const v::g_devCamPosFP,
    dev_cam_pos_valid: &raw const v::g_devCamPosValid,
    dev_cam_base: (&raw const v::g_devCamBase).cast_mut().cast(),
    dev_cam_base_valid: &raw const v::g_devCamBaseValid,
    dev_cam_aim_yaw: (&raw const v::g_devCamAimYaw).cast_mut().cast(),
    dev_cam_aim_pitch: (&raw const v::g_devCamAimPitch).cast_mut().cast(),
    dev_cam_aim_valid: &raw const v::g_devCamAimValid,
    dev_cam_view_valid: &raw const v::g_devCamViewValid,
    normal_fov_override: (&raw const v::g_normalFovOverrideValue).cast(),
    verbose_log: (&raw const v::g_verboseLog).cast(),
};
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const RT).cast_mut());
fn rt() -> &'static Runtime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*RUNTIME.load(Acquire) }
}
/// A pointer of the runtime, as the reference it is.
fn r<T>(p: *const T) -> &'static T {
    // SAFETY: the C++ half's globals, alive for the process.
    unsafe { &*p }
}
fn verbose() -> bool {
    // SAFETY: the C++ half's volatile int.
    unsafe { core::ptr::read_volatile(rt().verbose_log) != 0 }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
fn log_c(text: std::ffi::CString) {
    log(&text.to_string_lossy());
}
fn now_ms() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

extern "C" {
    static CyberpunkVR_DeviceCamFollow: i32;
    static CyberpunkVR_LensHeadWrite: i32;
    static CyberpunkVR_DebugLensHeadWrites: AtomicU64;
    static CyberpunkVR_DevCamOffsetX: f32;
    static CyberpunkVR_DevCamOffsetY: f32;
    static CyberpunkVR_DevCamOffsetZ: f32;
    static CyberpunkVR_BdOneComposition: i32;
    static CyberpunkVR_BdPushTransform: i32;
    static CyberpunkVR_DevCamInLocate: i32;
    static CyberpunkVR_SceneStickYawDps: f32;
    static CyberpunkVR_SceneStickYawTier: i32;
    static CyberpunkVR_DebugPatchCamMain: u64;
    static CyberpunkVR_DebugPatchCamVrcam: u64;
    static CyberpunkVR_DebugPatchCamDevice: u32;
    static CyberpunkVR_DebugVrcamNodeHits: u64;
    static CyberpunkVR_DebugMirrorRtvHits: u64;
    static CyberpunkVR_DebugMainCamRejects: u64;
    static CyberpunkVR_DebugVrcamCamRejects: u64;
    static CyberpunkVR_DebugRttComp: u64;
    static CyberpunkVR_DebugBdCandidates: AtomicU64;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static g_sceneTier: AtomicI32;
    static g_isInVehicle: bool;
    fn cvr_camera_plausible_unit_quaternion(q: *const f32) -> bool;
    fn cvr_camera_plausible_position_vec4(v: *const f32) -> bool;
    fn cvr_locate_bd_push_transform_once(
        comp: usize,
        qx: f32,
        qy: f32,
        qz: f32,
        qw: f32,
        second_eye: bool,
        base: *const i32,
        pose: *const c_void,
        pose_id: u64,
    );
}
macro_rules! g {
    ($name:ident) => {{
        // SAFETY: a scalar of the C++ half (or of Rust under its C name), alive for the process.
        #[allow(unused_unsafe)]
        let value = unsafe { core::ptr::read_volatile(&raw const $name) };
        value
    }};
}
fn diag(counter: &AtomicU64) {
    if g!(CyberpunkVR_RuntimeDiagnostics) != 0 {
        counter.fetch_add(1, Relaxed);
    }
}
fn ptr_text(p: usize) -> String {
    format!("{p:016X}")
}
fn plausible_unit_quaternion(q: &[f32; 4]) -> bool {
    // SAFETY: four floats.
    unsafe { cvr_camera_plausible_unit_quaternion(q.as_ptr()) }
}
/// The component's world position at +0xE0, in the fixed point the engine stores (1/131072 m).
fn read_pos_fp(obj: usize) -> Option<[i32; 3]> {
    code::read::<[i32; 3]>(obj + 0xE0)
}
const FP: f32 = 1.0 / 131072.0;

// ---- the camera math the hooks share -----------------------------------------------------------------------------

/// NormalizeQuat: the identity for a degenerate one.
///
/// # Safety
/// The four pointers are writable floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_normalize_quat(x: *mut f32, y: *mut f32, z: *mut f32, w: *mut f32) {
    // SAFETY: the caller's floats.
    unsafe {
        let len_sq = *x * *x + *y * *y + *z * *z + *w * *w;
        if len_sq <= 0.000001 {
            (*x, *y, *z, *w) = (0.0, 0.0, 0.0, 1.0);
            return;
        }
        let inv = 1.0 / len_sq.sqrt();
        *x *= inv;
        *y *= inv;
        *z *= inv;
        *w *= inv;
    }
}
/// MulQuat: a * b.
///
/// # Safety
/// `out` is four writable floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_mul_quat(a: *const f32, b: *const f32, out: *mut f32) {
    // SAFETY: the caller's quaternions.
    unsafe {
        let (ax, ay, az, aw) = (*a, *a.add(1), *a.add(2), *a.add(3));
        let (bx, by, bz, bw) = (*b, *b.add(1), *b.add(2), *b.add(3));
        *out = aw * bx + ax * bw + ay * bz - az * by;
        *out.add(1) = aw * by - ax * bz + ay * bw + az * bx;
        *out.add(2) = aw * bz + ax * by - ay * bx + az * bw;
        *out.add(3) = aw * bw - ax * bx - ay * by - az * bz;
    }
}
/// IsPlausibleCameraSpan: two plausible positions less than five metres apart.
///
/// # Safety
/// `a` and `b` are null or four floats each.
#[no_mangle]
pub unsafe extern "C" fn cvr_is_plausible_camera_span(a: *const f32, b: *const f32) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    // SAFETY: the caller's vectors.
    unsafe {
        if !cvr_camera_plausible_position_vec4(a) || !cvr_camera_plausible_position_vec4(b) {
            return false;
        }
        let dx = *b - *a;
        let dy = *b.add(1) - *a.add(1);
        let dz = *b.add(2) - *a.add(2);
        dx * dx + dy * dy + dz * dz < 25.0
    }
}

// ---- who owns the frame ------------------------------------------------------------------------------------------------

/// BdPushOwnsComposition: the braindance push owns the second eye -- only while the scene owns the camera (playback,
/// not the editor). A takeover deliberately does not own it.
#[no_mangle]
pub extern "C" fn cvr_bd_push_owns_composition() -> bool {
    if g!(CyberpunkVR_BdOneComposition) == 0 || (g!(CyberpunkVR_BdPushTransform) & 1) == 0 {
        return false;
    }
    let rt = rt();
    r(rt.bd_active).load(Relaxed) != 0 && r(rt.bd_scene_pose_valid).load(Acquire) != 0
}
fn device_cam_claimed() -> bool {
    r(rt().cam_obj_device).load(Relaxed) != 0
}
/// LocateOwnsTakeover: the located buffer owns this frame's camera -- a takeover once the lens is known, and only
/// where there is no dispatch of its own to write (an unclaimed camera), or for every takeover in mode 2.
#[no_mangle]
pub extern "C" fn cvr_locate_owns_takeover() -> bool {
    let mode = g!(CyberpunkVR_DevCamInLocate);
    if mode == 0 {
        return false;
    }
    if !cvr_device_cam_active() || r(rt().dev_cam_base_valid).load(Acquire) == 0 {
        return false;
    }
    if mode >= 2 {
        return true;
    }
    !device_cam_claimed()
}
/// DeviceCamActive: a camera that is not the player's is latched (a takeover's, or a braindance camera actually
/// found) and was written within the liveness window.
#[no_mangle]
pub extern "C" fn cvr_device_cam_active() -> bool {
    if g!(CyberpunkVR_DeviceCamFollow) == 0 {
        return false;
    }
    let rt = rt();
    if r(rt.remote_cam_on).load(Relaxed) == 0 && r(rt.bd_cam_found).load(Relaxed) == 0 {
        return false;
    }
    let t = r(rt.device_cam_last_ms).load(Relaxed);
    if t == 0 {
        return false;
    }
    now_ms().wrapping_sub(t) < 300
}
/// SceneStickYawArmed: the composition integrates the scene yaw and the input merge stands its snap turn down for
/// exactly these frames.
#[no_mangle]
pub extern "C" fn cvr_scene_stick_yaw_armed() -> bool {
    // SAFETY: the C++ half's atomic and bool.
    g!(CyberpunkVR_SceneStickYawDps) > 0.0
        && unsafe { g_sceneTier.load(Relaxed) } >= g!(CyberpunkVR_SceneStickYawTier)
        && !g!(g_isInVehicle)
}

// ---- the lens -------------------------------------------------------------------------------------------------------

/// READABLE, ASKED OF THE OS RATHER THAN ASSUMED: the engine's vectored handler takes a bad read before any guard
/// does. VirtualQuery costs real time, so it is only asked while binding.
fn address_is_readable(a: usize, bytes: usize) -> bool {
    use windows_sys::Win32::System::Memory::{
        VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS,
    };
    if !(0x10000..0x7FF0_0000_0000).contains(&a) || a & 7 != 0 {
        return false;
    }
    // SAFETY: a query about an address, into a zeroed record.
    let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    let got = unsafe {
        VirtualQuery(
            a as *const c_void,
            &mut mbi,
            core::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if got != core::mem::size_of::<MEMORY_BASIC_INFORMATION>() || mbi.State != MEM_COMMIT {
        return false;
    }
    if mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0 {
        return false;
    }
    a + bytes <= mbi.BaseAddress as usize + mbi.RegionSize
}

/// cname_hash("cameraComponent").
fn cam_comp_hash() -> u64 {
    crate::vrcam_config::cname_hash(b"cameraComponent")
}

/// BindLensFromEntity: the component named `cameraComponent` whose owner is the entity the script named, from the
/// entity's own component list (entity+0xA0 entries of (instance, refcount), +0xA8 capacity, +0xAC size).
#[no_mangle]
pub extern "C" fn cvr_bind_lens_from_entity(entity: usize) -> usize {
    let rt = rt();
    if !address_is_readable(entity, 0xB0) {
        return 0;
    }
    let (Some(entries), Some(cap), Some(mut count)) = (
        code::read::<u64>(entity + 0xA0),
        code::read::<u32>(entity + 0xA8),
        code::read::<u32>(entity + 0xAC),
    ) else {
        return 0;
    };
    count = count.min(cap).min(512); // a size past the capacity is a bad read, not a long list
    for i in 0..count {
        let slot = entries as usize + i as usize * 0x10;
        if !address_is_readable(slot, 0x10) {
            continue;
        }
        let Some(inst) = code::read::<u64>(slot) else {
            continue;
        };
        let c = inst as usize;
        // Readable as far as the rotation, because that is what the per-frame refresh takes from it.
        if !address_is_readable(c, 0x100) {
            continue;
        }
        if code::read::<u64>(c + 0x40) != Some(cam_comp_hash()) {
            continue;
        }
        if code::read::<u64>(c + 0x50).map(|o| o as usize) != Some(entity) {
            continue;
        }
        r(rt.lens_comp).store(c, Release);
        let mut p = [0i32; 3];
        for (k, v) in p.iter_mut().enumerate() {
            if let Some(x) = code::read::<u32>(c + 0xE0 + k * 4) {
                *v = x as i32;
            }
        }
        if verbose() {
            log_c(format(
                c"PatchCamera: lens is component %016llX of entity %016llX, at (%.3f, %.3f, %.3f), %u component(s)\n",
                &[
                    Arg::U(c as u64),
                    Arg::U(entity as u64),
                    Arg::F((p[0] as f32 * FP) as f64),
                    Arg::F((p[1] as f32 * FP) as f64),
                    Arg::F((p[2] as f32 * FP) as f64),
                    Arg::U(count as u64),
                ],
            ));
        }
        return c;
    }
    if verbose() {
        log(&format!(
            "PatchCamera: entity {} carries no cameraComponent among {count} component(s)\n",
            ptr_text(entity)
        ));
    }
    0
}

/// WHAT WE LAST WROTE INTO THE LENS, so its own aim can be told from our composition coming back.
struct LensWritten {
    quat: [f32; 4],
    pos_fp: [i32; 3],
}
static LENS_WRITTEN: Mutex<LensWritten> = Mutex::new(LensWritten {
    quat: [0.0; 4],
    pos_fp: [0; 3],
});
static LENS_WRITTEN_VALID: AtomicBool = AtomicBool::new(false);
fn lens_written() -> std::sync::MutexGuard<'static, LensWritten> {
    LENS_WRITTEN.lock().unwrap_or_else(|p| p.into_inner())
}
fn lens_comp() -> Option<usize> {
    let comp = r(rt().lens_comp).load(Relaxed);
    (comp > 0x10000 && comp < 0x7FF0_0000_0000 && comp & 7 == 0).then_some(comp)
}

/// PushLensHeadTransform: the head written into the lens, at the lens plus the viewpoint slider (yaw-rotated as
/// PatchCamera applies it to the second eye), MAIN's half of the separation, with the engine's change notification.
///
/// # Safety
/// `quat` is null or four floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_push_lens_head_transform(quat: *const f32) {
    if g!(CyberpunkVR_LensHeadWrite) == 0 {
        return;
    }
    let Some(comp) = lens_comp() else { return };
    if quat.is_null() {
        return;
    }
    // SAFETY: the caller's quaternion.
    let q = unsafe { *(quat as *const [f32; 4]) };
    if !plausible_unit_quaternion(&q) {
        return;
    }
    let rt = rt();
    if r(rt.dev_cam_pos_valid).load(Acquire) == 0 {
        return;
    }
    let pos = r(rt.dev_cam_pos_fp);
    let mut base = [
        pos[0].load(Relaxed),
        pos[1].load(Relaxed),
        pos[2].load(Relaxed),
    ];
    let (ox, oy, oz) = (
        g!(CyberpunkVR_DevCamOffsetX),
        g!(CyberpunkVR_DevCamOffsetY),
        g!(CyberpunkVR_DevCamOffsetZ),
    );
    if ox != 0.0 || oy != 0.0 || oz != 0.0 {
        // SAFETY: the C++ half's float.
        let yaw = if r(rt.dev_cam_aim_valid).load(Acquire) != 0 {
            unsafe { *rt.dev_cam_aim_yaw }
        } else {
            0.0
        };
        let (sy, cy) = (yaw.sin(), yaw.cos());
        base[0] = base[0].wrapping_add(((ox * cy - oy * sy) * 131072.0) as i32);
        base[1] = base[1].wrapping_add(((ox * sy + oy * cy) * 131072.0) as i32);
        base[2] = base[2].wrapping_add((oz * 131072.0) as i32);
    }
    // SAFETY: the locate hook's push, with the base in hand.
    unsafe {
        cvr_locate_bd_push_transform_once(
            comp,
            q[0],
            q[1],
            q[2],
            q[3],
            false,
            base.as_ptr(),
            core::ptr::null(),
            0,
        )
    };
    // SAFETY: an export of the C++ half.
    diag(unsafe { &*(&raw const CyberpunkVR_DebugLensHeadWrites) });

    // What is actually in the component now, not what we meant to put there: the comparison has to be exact.
    if let (Some(rp), Some(rq)) = (read_pos_fp(comp), code::read::<[f32; 4]>(comp + 0xF0)) {
        let mut w = lens_written();
        w.pos_fp = rp;
        w.quat = rq;
        LENS_WRITTEN_VALID.store(true, Release);
    }
}

/// RefreshLensFromComponent: the lens re-read every frame -- its place, its aim (yaw and pitch) and the heartbeat --
/// unless what is there is our own write coming back.
#[no_mangle]
pub extern "C" fn cvr_refresh_lens_from_component() {
    let Some(comp) = lens_comp() else { return };
    let rt = rt();
    let p = read_pos_fp(comp);
    if let Some(p) = p {
        let ours = LENS_WRITTEN_VALID.load(Acquire) && lens_written().pos_fp == p;
        if !ours {
            let pos = r(rt.dev_cam_pos_fp);
            for (slot, v) in pos.iter().zip(p) {
                slot.store(v, Relaxed);
            }
            r(rt.dev_cam_pos_valid).store(1, Release);
        }
        // THE HEARTBEAT: a lens re-read this frame IS the liveness DeviceCamActive asks about.
        crate::camera_owner::cvr_stamp_device_cam();
    }
    let Some(q) = code::read::<[f32; 4]>(comp + 0xF0) else {
        return;
    };
    if !plausible_unit_quaternion(&q) {
        return;
    }
    let ours = LENS_WRITTEN_VALID.load(Acquire) && lens_written().quat == q;
    if ours {
        return;
    }
    // SAFETY: the C++ half's floats.
    unsafe {
        *rt.dev_cam_base = q;
        let fx = 2.0 * (q[0] * q[1] - q[2] * q[3]);
        let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
        let fz = 2.0 * (q[1] * q[2] + q[0] * q[3]);
        let fh = (fx * fx + fy * fy).sqrt();
        *rt.dev_cam_aim_yaw = (-fx).atan2(fy);
        *rt.dev_cam_aim_pitch = fz.atan2(fh);
    }
    r(rt.dev_cam_aim_valid).store(1, Release);
    r(rt.dev_cam_base_valid).store(1, Release);
}

// ---- handing back ---------------------------------------------------------------------------------------------------

/// DeviceCamRestoreFov: the camera's authored fov written back when control is released -- through a pointer proved
/// alive by a non-zero CName at the calibrated offset (and `cameraComponent` for a takeover).
#[no_mangle]
pub extern "C" fn cvr_device_cam_restore_fov() {
    let rt = rt();
    if r(rt.dev_cam_fov_saved).swap(0, AcqRel) == 0 {
        return;
    }
    let obj = r(rt.cam_obj_device).load(Relaxed);
    if obj < 0x10000 {
        return;
    }
    let off = r(rt.cam_name_offset).load(Acquire);
    if off < 0 {
        return;
    }
    let Some(name) = code::read::<u64>(obj + off as usize) else {
        return;
    };
    if name == 0 {
        return;
    }
    if r(rt.bd_cam_found).load(Relaxed) == 0 && name != cam_comp_hash() {
        return;
    }
    let Some(cur) = code::read::<f32>(obj + 0x128) else {
        return;
    };
    // SAFETY: the C++ half's float.
    let orig = unsafe { core::ptr::read_volatile(rt.dev_cam_fov_orig) };
    if !(orig > 1.0 && orig < 179.0) {
        return;
    }
    let _ = code::write::<f32>(obj + 0x128, orig);
    if verbose() {
        log_c(format(
            c"PatchCamera: device camera fov handed back %.3f -> %.3f\n",
            &[Arg::F(cur as f64), Arg::F(orig as f64)],
        ));
    }
}

/// TakeoverLensRelease: the takeover's objects dropped (VRRemoteCamera(0)).
#[no_mangle]
pub extern "C" fn cvr_takeover_lens_release() {
    let rt = rt();
    LENS_WRITTEN_VALID.store(false, Relaxed);
    let lens = r(rt.lens_comp).swap(0, Relaxed);
    r(rt.takeover_entity).store(0, Relaxed);
    r(rt.cam_obj_device).store(0, Relaxed);
    if lens != 0 && verbose() {
        log(&format!(
            "PatchCamera: lens released (was {})\n",
            ptr_text(lens)
        ));
    }
}

/// BraindanceCameraRelease: the fov handed back, then everything of this braindance forgotten.
#[no_mangle]
pub extern "C" fn cvr_braindance_camera_release() {
    let rt = rt();
    if r(rt.bd_cam_found).load(Relaxed) == 0 {
        return;
    }
    cvr_device_cam_restore_fov(); // reads the latched object, so BEFORE it is cleared
    r(rt.bd_cam_found).store(0, Relaxed);
    r(rt.cam_obj_device).store(0, Relaxed);
    r(rt.dev_cam_pos_valid).store(0, Relaxed);
    r(rt.dev_cam_base_valid).store(0, Relaxed);
    r(rt.dev_cam_view_valid).store(0, Relaxed);
    r(rt.dev_cam_aim_valid).store(0, Relaxed);
    if verbose() {
        log("PatchCamera: braindance camera released\n");
    }
}

// ---- the braindance camera ------------------------------------------------------------------------------------------

/// How close a patched object has to be to the pose script reports for the scene camera: 0.995 is the orientation that
/// discriminates, the distance covers the frame of lag between the script read and this write.
const BD_POS_TOL_M: f32 = 0.60;
const BD_DOT_TOL: f32 = 0.995;

/// The census, one line a second, informative when NOTHING matches.
struct Census {
    last_ms: u64,
    best_dot: f32,
    best_dist: f32,
    best_name: u64,
    best_obj: usize,
    seen: u64,
    p_main: u64,
    p_vrcam: u64,
    p_dev: u64,
    p_nodes: u64,
    p_rtv: u64,
    short_name: [u64; 8],
    short_dist: [f32; 8],
    short_fov: [f32; 8],
    short_n: usize,
}
static CENSUS: Mutex<Census> = Mutex::new(Census {
    last_ms: 0,
    best_dot: 0.0,
    best_dist: 1.0e9,
    best_name: 0,
    best_obj: 0,
    seen: 0,
    p_main: 0,
    p_vrcam: 0,
    p_dev: 0,
    p_nodes: 0,
    p_rtv: 0,
    short_name: [0; 8],
    short_dist: [0.0; 8],
    short_fov: [0.0; 8],
    short_n: 0,
});

fn name_at(obj: usize) -> u64 {
    let off = r(rt().cam_name_offset).load(Acquire);
    if off >= 0 {
        code::read::<u64>(obj + off as usize).unwrap_or(0)
    } else {
        0
    }
}

/// BraindanceCameraMatch: for every object the classifier does not recognise, from inside the camera writer -- the
/// orientation first (four multiplies, no memory read), the position only for what is aimed the way the scene camera
/// is, and a camera's fov last (a senses component rides the head exactly where the camera sits).
///
/// # Safety
/// `quat` is null or four floats.
#[no_mangle]
pub unsafe extern "C" fn cvr_braindance_camera_match(obj: usize, quat: *const f32) -> bool {
    if g!(CyberpunkVR_DeviceCamFollow) == 0 {
        return false;
    }
    let rt = rt();
    if r(rt.bd_active).load(Relaxed) == 0 || r(rt.bd_scene_pose_valid).load(Acquire) == 0 {
        return false;
    }
    if obj < 0x10000 || quat.is_null() {
        return false;
    }
    // SAFETY: the caller's quaternion; the C++ half's scene quaternion.
    let (q, s) = unsafe {
        (
            *(quat as *const [f32; 4]),
            core::ptr::read_volatile(rt.bd_scene_quat),
        )
    };
    let mut dot = q[0] * s[0] + q[1] * s[1] + q[2] * s[2] + q[3] * s[3];
    if dot < 0.0 {
        dot = -dot; // q and -q are the same rotation
    }

    let mut c = CENSUS.lock().unwrap_or_else(|p| p.into_inner());
    c.seen += 1;

    let mut dist = -1.0f32;
    if dot >= 0.95 {
        if let Some(p) = read_pos_fp(obj) {
            let scene = r(rt.bd_scene_pos_fp);
            let mut d2 = 0.0f32;
            for i in 0..3 {
                let d = (p[i].wrapping_sub(scene[i].load(Relaxed))) as f32 * FP;
                d2 += d * d;
            }
            dist = d2.sqrt();
        }
    }

    // A SHORTLIST OF WHAT IS OUT THERE: every distinct component name within 1.5 m of the scene camera's pose.
    if dot >= 0.99 && dist >= 0.0 && dist <= 1.5 {
        let nm = name_at(obj);
        let fv = code::read::<f32>(obj + 0x128).unwrap_or(-1.0);
        let n = c.short_n;
        if let Some(i) = c.short_name[..n].iter().position(|&x| x == nm) {
            if dist < c.short_dist[i] {
                c.short_dist[i] = dist;
                c.short_fov[i] = fv;
            }
        } else if n < 8 {
            c.short_name[n] = nm;
            c.short_dist[n] = dist;
            c.short_fov[n] = fv;
            c.short_n += 1;
        }
    }

    if dot > c.best_dot || (dist >= 0.0 && dist < c.best_dist) {
        if dot > c.best_dot {
            c.best_dot = dot;
        }
        if dist >= 0.0 && dist < c.best_dist {
            c.best_dist = dist;
            c.best_obj = obj;
            c.best_name = name_at(obj);
        }
    }

    let now = now_ms();
    if c.last_ms == 0 {
        c.last_ms = now;
    } else if now.wrapping_sub(c.last_ms) >= 1000 {
        c.last_ms = now;
        let (c_main, c_vrcam, c_dev, c_nodes, c_rtv) = (
            g!(CyberpunkVR_DebugPatchCamMain),
            g!(CyberpunkVR_DebugPatchCamVrcam),
            g!(CyberpunkVR_DebugPatchCamDevice) as u64,
            g!(CyberpunkVR_DebugVrcamNodeHits),
            g!(CyberpunkVR_DebugMirrorRtvHits),
        );
        if verbose() {
            log_c(format(
                c"[bd] best dot=%.4f dist=%.2fm name=0x%016llX obj=%016llX unknowns=%llu | latched=%016llX found=%d | patch main=+%llu vrcam=+%llu dev=+%llu | vrcamNodes=+%llu rtv=+%llu | rejects main=%llu vrcam=%llu rtt=%016llX\n",
                &[
                    Arg::F(c.best_dot as f64),
                    Arg::F(if c.best_dist >= 1.0e8 { -1.0 } else { c.best_dist as f64 }),
                    Arg::U(c.best_name),
                    Arg::U(c.best_obj as u64),
                    Arg::U(c.seen),
                    Arg::U(r(rt.cam_obj_device).load(Relaxed) as u64),
                    Arg::I(r(rt.bd_cam_found).load(Relaxed)),
                    Arg::U(c_main.wrapping_sub(c.p_main)),
                    Arg::U(c_vrcam.wrapping_sub(c.p_vrcam)),
                    Arg::U(c_dev.wrapping_sub(c.p_dev)),
                    Arg::U(c_nodes.wrapping_sub(c.p_nodes)),
                    Arg::U(c_rtv.wrapping_sub(c.p_rtv)),
                    Arg::U(g!(CyberpunkVR_DebugMainCamRejects)),
                    Arg::U(g!(CyberpunkVR_DebugVrcamCamRejects)),
                    Arg::U(g!(CyberpunkVR_DebugRttComp)),
                ],
            ));
            for i in 0..c.short_n {
                log_c(format(
                    c"[bd]   near the scene camera: name=0x%016llX dist=%.3fm fov=%.3f\n",
                    &[
                        Arg::U(c.short_name[i]),
                        Arg::F(c.short_dist[i] as f64),
                        Arg::F(c.short_fov[i] as f64),
                    ],
                ));
            }
        }
        c.short_n = 0;
        (c.p_main, c.p_vrcam, c.p_dev, c.p_nodes, c.p_rtv) =
            (c_main, c_vrcam, c_dev, c_nodes, c_rtv);
        c.best_dot = 0.0;
        c.best_dist = 1.0e9;
        c.best_name = 0;
        c.best_obj = 0;
        c.seen = 0;
    }
    drop(c);

    if dot < BD_DOT_TOL || dist < 0.0 || dist > BD_POS_TOL_M {
        return false;
    }

    // AND IT HAS TO BE A CAMERA: a plausible fov at +0x128 equal to the one script reports for the active camera, to
    // ours once forced, or to the authored one we saved.
    let Some(cam_fov) = code::read::<f32>(obj + 0x128) else {
        return false;
    };
    if !(cam_fov > 1.0 && cam_fov < 179.0) {
        return false;
    }
    let script_fov = r(rt.bd_want_fov_milli).load(Relaxed) as f32 * 0.001;
    // SAFETY: the C++ half's floats.
    let (ours, orig) = unsafe {
        (
            core::ptr::read_volatile(rt.normal_fov_override),
            core::ptr::read_volatile(rt.dev_cam_fov_orig),
        )
    };
    let fov_agrees = (script_fov > 1.0 && (cam_fov - script_fov).abs() < 0.75)
        || (ours > 1.0 && (cam_fov - ours).abs() < 0.75)
        || (r(rt.dev_cam_fov_saved).load(Relaxed) != 0 && (cam_fov - orig).abs() < 0.75);
    if !fov_agrees {
        return false;
    }

    let prev = r(rt.cam_obj_device).swap(obj, Relaxed);
    if prev != obj {
        let nm = name_at(obj);
        if verbose() {
            log_c(format(
                c"PatchCamera: BRAINDANCE camera %016llX (was %016llX) name=0x%016llX dot=%.4f dist=%.3fm\n",
                &[Arg::U(obj as u64), Arg::U(prev as u64), Arg::U(nm), Arg::F(dot as f64), Arg::F(dist as f64)],
            ));
        }
        r(rt.dev_cam_base_valid).store(0, Relaxed); // a different camera: re-latch its aim
        r(rt.dev_cam_pos_valid).store(0, Relaxed);
    }
    // SAFETY: an export of the C++ half.
    diag(unsafe { &*(&raw const CyberpunkVR_DebugBdCandidates) });
    r(rt.bd_cam_found).store(1, Release);
    crate::camera_owner::cvr_stamp_device_cam();
    true
}

// ---- the shared slots -----------------------------------------------------------------------------------------------

/// GetShotShared: the 1 KB `CyberpunkVR_Hands_Shared` mapping (opened, or created when the other side has not yet),
/// mapped once.
#[no_mangle]
pub extern "C" fn cvr_get_shot_shared() -> *mut f32 {
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingA, MapViewOfFile, OpenFileMappingA, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
    };
    static SHARED: AtomicPtr<f32> = AtomicPtr::new(core::ptr::null_mut());
    static HANDLE: AtomicUsize = AtomicUsize::new(0);
    let p = SHARED.load(Acquire);
    if !p.is_null() {
        return p;
    }
    // SAFETY: named mapping calls with literal names and sizes.
    unsafe {
        let name = c"CyberpunkVR_Hands_Shared".as_ptr() as *const u8;
        let mut h = OpenFileMappingA(FILE_MAP_ALL_ACCESS, 0, name);
        if h.is_null() {
            h = CreateFileMappingA(
                windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE,
                core::ptr::null(),
                PAGE_READWRITE,
                0,
                1024,
                name,
            );
        }
        if h.is_null() {
            return core::ptr::null_mut();
        }
        HANDLE.store(h as usize, Relaxed);
        let view = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 1024);
        SHARED.store(view.Value as *mut f32, Release);
        view.Value as *mut f32
    }
}
