//! What the plugin cannot find out for itself about the cameras (formerly src/Natives/RemoteCamera.cpp, where each gate
//! is written up): which camera the player took over (VRRemoteCamera, VRTakeoverEntity), where the live player's camera
//! is during a braindance (VRPlayerCamera), the braindance scene's camera and fov (VRSceneCamera, VRBraindance,
//! VRBdSceneFov), whether a braindance is running (VRInBraindance), the taken-over camera's viewpoint offset
//! (VRDevCamOffset) and the braindance A/B switches. The scripts publish them from CET ticks; the camera writer reads
//! them. The camera state stays C++ (src/Core/VrCore.cpp): the gates of C++ linkage are reached through
//! `cvr_cpp_remote_camera_state`, the releases through C services.
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use cvr_crt::{format as cformat, Arg};
use red4ext_abi::cstring::CString;
use red4ext_abi::stack::StackFrame;
use std::sync::OnceLock;

/// The C++-linkage camera gates (src/Core/VrCore.cpp), by address.
#[repr(C)]
struct State {
    remote_cam_on: *const AtomicI32,
    remote_cam_pos_fp: *const [AtomicI32; 3],
    player_cam_on: *const AtomicI32,
    player_cam_pos_fp: *const [AtomicI32; 3],
    /// std::atomic<float>.
    bd_scene_fov: *const AtomicU32,
    takeover_entity_id: *const AtomicU64,
    bd_want_fov_milli: *const AtomicI32,
}
// SAFETY: addresses of process-lifetime atomics.
unsafe impl Send for State {}
unsafe impl Sync for State {}

#[allow(non_upper_case_globals)]
extern "C" {
    static mut CyberpunkVR_BdFovWrite: i32;
    static mut CyberpunkVR_BdHeadWrite: i32;
    static mut CyberpunkVR_HudInBraindance: i32;
    static mut CyberpunkVR_DevCamOffsetX: f32;
    static mut CyberpunkVR_DevCamOffsetY: f32;
    static mut CyberpunkVR_DevCamOffsetZ: f32;
    static g_lensComp: AtomicUsize;
    static g_devCamAimValid: AtomicI32;
    static g_devCamBaseValid: AtomicI32;
    static g_devCamViewValid: AtomicI32;
    static g_devCamPosValid: AtomicI32;
    static g_bdActive: AtomicI32;
    static mut g_bdSceneQuat: [f32; 4];
    static g_bdScenePoseValid: AtomicI32;
    static g_bdScenePosFP: [AtomicI32; 3];
    fn cvr_cpp_remote_camera_state(out: *mut State);
    fn cvr_cpp_verbose_log() -> i32;
    fn cvr_cpp_takeover_lens_release();
    fn cvr_cpp_braindance_camera_release();
    fn cvr_cpp_patch_fast_disarm();
    fn cvr_cpp_device_cam_restore_fov();
}
use super::env::GetTickCount64;

fn state() -> &'static State {
    static STATE: OnceLock<State> = OnceLock::new();
    STATE.get_or_init(|| {
        let mut s = State {
            remote_cam_on: core::ptr::null(),
            remote_cam_pos_fp: core::ptr::null(),
            player_cam_on: core::ptr::null(),
            player_cam_pos_fp: core::ptr::null(),
            bd_scene_fov: core::ptr::null(),
            takeover_entity_id: core::ptr::null(),
            bd_want_fov_milli: core::ptr::null(),
        };
        // SAFETY: the C++ half fills the addresses of its atomics.
        unsafe { cvr_cpp_remote_camera_state(&mut s) };
        s
    })
}
fn verbose() -> bool {
    // SAFETY: a plain read of the launcher's DEBUG box.
    unsafe { cvr_cpp_verbose_log() != 0 }
}
fn log(text: &str) {
    super::env::log(text);
}
fn f3(v: f32) -> String {
    cformat(c"%.3f", &[Arg::F(v as f64)]).to_str().unwrap_or("").to_string()
}
fn f2(v: f32) -> String {
    cformat(c"%.2f", &[Arg::F(v as f64)]).to_str().unwrap_or("").to_string()
}
/// The fixed point the camera components keep their world position in (1/131072 m).
fn fixed(v: f32) -> i32 {
    (v * 131072.0) as i32
}
macro_rules! params {
    ($frame:expr $(, $value:expr)*) => {
        if let Some(frame) = $frame.as_mut() {
            $(frame.get_parameter(&mut $value);)*
            frame.finish_params();
        }
    };
}
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        *out = value;
    }
}
/// The lens and its latches dropped, so nothing of one takeover can be reused by the next.
unsafe fn drop_lens_flags() {
    g_devCamPosValid.store(0, Ordering::Relaxed);
    g_devCamAimValid.store(0, Ordering::Relaxed);
    g_devCamBaseValid.store(0, Ordering::Relaxed);
    g_devCamViewValid.store(0, Ordering::Relaxed);
}

/// VRPlayerCamera(active, x, y, z) -> Int32: where the live player's camera is (consulted during a braindance).
#[no_mangle]
pub unsafe extern "C" fn VRPlayerCamera(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let (mut active, mut x, mut y, mut z) = (0i32, 0f32, 0f32, 0f32);
    params!(frame, active, x, y, z);
    let s = state();
    if active == 0 {
        (*s.player_cam_on).store(0, Ordering::Relaxed);
        put(out, 0);
        return;
    }
    let pos = &*s.player_cam_pos_fp;
    pos[0].store(fixed(x), Ordering::Relaxed);
    pos[1].store(fixed(y), Ordering::Relaxed);
    pos[2].store(fixed(z), Ordering::Relaxed);
    (*s.player_cam_on).store(1, Ordering::Release);
    put(out, 1);
}

/// VRBdSceneFov(fov) -> Int32: the scene's own fov, handed over by the script that reads it.
#[no_mangle]
pub unsafe extern "C" fn VRBdSceneFov(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let mut fov = 0f32;
    params!(frame, fov);
    let ok = fov > 1.0 && fov < 179.0;
    if ok {
        (*state().bd_scene_fov).store(fov.to_bits(), Ordering::Relaxed);
    }
    put(out, ok as i32);
}

/// VRDevCamOffset(x, y, z) -> Int32: the taken-over camera's viewpoint offset (the panel's sliders), three metres
/// each way at most.
#[no_mangle]
pub unsafe extern "C" fn VRDevCamOffset(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let (mut x, mut y, mut z) = (0f32, 0f32, 0f32);
    params!(frame, x, y, z);
    let clamp3 = |v: f32| {
        if v < -3.0 {
            -3.0
        } else if v > 3.0 {
            3.0
        } else {
            v
        }
    };
    CyberpunkVR_DevCamOffsetX = clamp3(x);
    CyberpunkVR_DevCamOffsetY = clamp3(y);
    CyberpunkVR_DevCamOffsetZ = clamp3(z);
    if verbose() {
        log(&format!(
            "VRDevCamOffset: lens offset right={} forward={} up={} m\n",
            f3(CyberpunkVR_DevCamOffsetX),
            f3(CyberpunkVR_DevCamOffsetY),
            f3(CyberpunkVR_DevCamOffsetZ)
        ));
    }
    put(out, 1);
}

/// VRTakeoverEntity(id: String) -> Int32: the taken-over entity's id as text (every digit taken: a 64-bit hash does
/// not survive Lua's doubles). A different object ends one takeover and starts the next: the lens goes.
#[no_mangle]
pub unsafe extern "C" fn VRTakeoverEntity(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let mut text = CString::empty();
    params!(frame, text);
    let mut id = 0u64;
    for &c in text.as_str().as_bytes() {
        if !c.is_ascii_digit() {
            if id != 0 {
                break; // digits ended: "9003388ULL"
            }
            continue; // leading junk
        }
        id = id.wrapping_mul(10).wrapping_add((c - b'0') as u64);
    }
    drop(text);
    let prev = (*state().takeover_entity_id).swap(id, Ordering::Relaxed);
    if prev != id {
        cvr_cpp_takeover_lens_release();
        g_devCamBaseValid.store(0, Ordering::Relaxed);
        g_devCamPosValid.store(0, Ordering::Relaxed);
        g_devCamAimValid.store(0, Ordering::Relaxed);
        if verbose() {
            log(&format!("VRTakeoverEntity: entity id {id} (was {prev})\n"));
        }
    }
    put(out, (id != 0) as i32);
}

/// VRBdFovWrite(on) -> Int32: the braindance fov write, live.
#[no_mangle]
pub unsafe extern "C" fn VRBdFovWrite(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    CyberpunkVR_BdFovWrite = (on != 0) as i32;
    if verbose() {
        log(&format!("VRBdFovWrite: braindance fov write {}\n", if on != 0 { "ON" } else { "OFF" }));
    }
    put(out, CyberpunkVR_BdFovWrite);
}
/// VRHudSecondEye(on) -> Int32: the HUD composite in the second eye during a braindance.
#[no_mangle]
pub unsafe extern "C" fn VRHudSecondEye(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    CyberpunkVR_HudInBraindance = (on != 0) as i32;
    if verbose() {
        log(&format!(
            "VRHudSecondEye: HUD into the second eye during a braindance {}\n",
            if on != 0 { "ON" } else { "OFF" }
        ));
    }
    put(out, CyberpunkVR_HudInBraindance);
}
/// VRBdHeadWrite(on) -> Int32: the head rotation written into the braindance view.
#[no_mangle]
pub unsafe extern "C" fn VRBdHeadWrite(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    CyberpunkVR_BdHeadWrite = (on != 0) as i32;
    if verbose() {
        log(&format!("VRBdHeadWrite: braindance head write {}\n", if on != 0 { "ON" } else { "OFF" }));
    }
    put(out, CyberpunkVR_BdHeadWrite);
}

/// VRSceneCamera(active, x, y, z, qi, qj, qk, qr) -> Int32: the braindance scene camera's pose -- the reference the
/// camera hunt finds the object by, nothing else.
#[no_mangle]
pub unsafe extern "C" fn VRSceneCamera(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let (mut active, mut x, mut y, mut z) = (0i32, 0f32, 0f32, 0f32);
    let (mut qi, mut qj, mut qk, mut qr) = (0f32, 0f32, 0f32, 1f32);
    params!(frame, active, x, y, z, qi, qj, qk, qr);
    if active == 0 {
        if g_bdActive.swap(0, Ordering::Relaxed) != 0 {
            g_bdScenePoseValid.store(0, Ordering::Relaxed);
            cvr_cpp_braindance_camera_release();
            if verbose() {
                log("VRSceneCamera: gate OFF\n");
            }
        }
        put(out, 0);
        return;
    }
    let ql = qi * qi + qj * qj + qk * qk + qr * qr;
    if ql > 0.9 && ql < 1.1 {
        g_bdSceneQuat = [qi, qj, qk, qr];
    }
    g_bdScenePosFP[0].store(fixed(x), Ordering::Relaxed);
    g_bdScenePosFP[1].store(fixed(y), Ordering::Relaxed);
    g_bdScenePosFP[2].store(fixed(z), Ordering::Relaxed);
    g_bdScenePoseValid.store(1, Ordering::Release);
    // The braindance camera hunt tests every component the site passes: the pointer filter stands down.
    cvr_cpp_patch_fast_disarm();
    if g_bdActive.swap(1, Ordering::Release) == 0 && verbose() {
        let q3 = |v: f32| cformat(c"%.3f", &[Arg::F(v as f64)]).to_str().unwrap_or("").to_string();
        log(&format!(
            "VRSceneCamera: gate ON, scene camera at ({} {} {}) q=({} {} {} {})\n",
            f2(x),
            f2(y),
            f2(z),
            q3(qi),
            q3(qj),
            q3(qk),
            q3(qr)
        ));
    }
    put(out, 1);
}

/// VRBraindance(active, fov) -> Int32: a braindance is running, with the fov the game reports for its camera (the
/// component whose own fov equals it is latched).
#[no_mangle]
pub unsafe extern "C" fn VRBraindance(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let (mut active, mut fov) = (0i32, 0f32);
    params!(frame, active, fov);
    let s = state();
    if active == 0 {
        if g_bdActive.swap(0, Ordering::Relaxed) != 0 {
            g_bdScenePoseValid.store(0, Ordering::Relaxed);
            cvr_cpp_braindance_camera_release();
            // The camera back exactly as the takeover path gives it back, and the lens dropped.
            cvr_cpp_device_cam_restore_fov();
            drop_lens_flags();
        }
        (*s.bd_want_fov_milli).store(0, Ordering::Relaxed);
        put(out, 0);
        return;
    }
    if fov > 5.0 && fov < 179.0 {
        (*s.bd_want_fov_milli).store((fov * 1000.0) as i32, Ordering::Relaxed);
    }
    cvr_cpp_patch_fast_disarm();
    g_bdActive.store(1, Ordering::Release);
    put(out, 1);
}

/// Until when the script last said a braindance runs (half a second past its report).
static IN_BRAINDANCE_UNTIL: AtomicU64 = AtomicU64::new(0);
/// InBraindance() (src/Natives/RemoteCamera.cpp forwards it).
#[no_mangle]
pub extern "C" fn cvr_in_braindance() -> bool {
    // SAFETY: the system clock.
    unsafe { GetTickCount64() < IN_BRAINDANCE_UNTIL.load(Ordering::Relaxed) }
}
/// VRInBraindance(active) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn VRInBraindance(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let mut active = 0i32;
    params!(frame, active);
    let was = cvr_in_braindance();
    IN_BRAINDANCE_UNTIL.store(if active != 0 { GetTickCount64() + 500 } else { 0 }, Ordering::Relaxed);
    if was != (active != 0) {
        log(&format!("VRInBraindance: braindance {}\n", if active != 0 { "started" } else { "ended" }));
    }
    put(out, (active != 0) as i32);
}

/// VRRemoteCamera(active, x, y, z) -> Int32: the taken-over camera's world position (a `cameraComponent` is followed
/// only within a metre and a half of it); 0 ends the takeover and drops the lens and its objects.
#[no_mangle]
pub unsafe extern "C" fn VRRemoteCamera(_this: *mut c_void, frame: *mut StackFrame, out: *mut i32, _unused: i64) {
    let (mut active, mut x, mut y, mut z) = (0i32, 0f32, 0f32, 0f32);
    params!(frame, active, x, y, z);
    let s = state();
    if active == 0 {
        (*s.remote_cam_on).store(0, Ordering::Relaxed);
        cvr_cpp_device_cam_restore_fov();
        drop_lens_flags();
        cvr_cpp_takeover_lens_release();
        put(out, 0);
        return;
    }
    let pos = &*s.remote_cam_pos_fp;
    pos[0].store(fixed(x), Ordering::Relaxed);
    pos[1].store(fixed(y), Ordering::Relaxed);
    pos[2].store(fixed(z), Ordering::Relaxed);
    (*s.remote_cam_on).store(1, Ordering::Release);
    // The pointer filter stands down until the lens is found.
    if g_lensComp.load(Ordering::Relaxed) == 0 {
        cvr_cpp_patch_fast_disarm();
    }
    put(out, 1);
}
