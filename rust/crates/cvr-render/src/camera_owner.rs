//! WHICH CAMERA IS THIS (formerly ClassifyPatchCameraOwner and its fast path in src/Core/VrCore.cpp): the PatchCamera
//! site runs for every camera component in the world (UpdateWorldTransforms, ~10 200 calls a second), and this decides
//! whether the one in hand is MAIN (1), the second view's (2), a device or braindance camera the player has taken over
//! (3), or none of ours (0).
//!
//! - The latched objects answer first (the overwhelming majority of calls end there); a latch gives itself up when the
//!   object stops being the right one, so the slow path -- where the identities are actually tested -- is reached again.
//! - The slow path finds the component's own CName (its offset calibrated once), then: "camera" owned by the player
//!   (in a braindance also at the place script publishes for the live player's camera, four misses in a row before a
//!   rejection, because the published place is a tick old); the second view's `vrcam_<W>x<H>` or its braindance twin,
//!   only with a second view and only the one the RTT view-create built from; a takeover's camera (one per takeover,
//!   decided once: by the taken-over entity when script names it, else by place); a braindance camera by its fov.
//! - THE FAST PATH (`g_patchFast`, read by the site's trampoline before any call): armed only once both cameras were
//!   recognised since the last disarm, the name offset is calibrated, no braindance hunt and no takeover lens hunt is
//!   on -- written only on an actual change, so an armed steady state stores nothing on a line three job threads read.
//!
//! The latches and the other state stay the C++ half's (VrCore.cpp and the takeover natives read them); the counters
//! are its exports.
use crate::vrcore_exports as v;
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU64, AtomicUsize, Ordering::*};

use cvr_engine::code;

#[allow(non_upper_case_globals)]
extern "C" {
    static g_camObjMain: AtomicUsize;
    static g_camObjVrcam: AtomicUsize;
    static g_bdActive: AtomicI32;
    static g_lensComp: AtomicUsize;
    static g_takeoverEntity: AtomicUsize;
    static mut g_devCamLastWritten: [f32; 4];
    static g_devCamBaseValid: AtomicI32;
    static g_devCamPosValid: AtomicI32;
    static CyberpunkVR_DevCamTolM: f32;
    static CyberpunkVR_DeviceCamFollow: i32;
    static CyberpunkVR_DevCamAnyName: i32;
    static CyberpunkVR_DebugRttComp: u64;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static CyberpunkVR_DebugPatchCamMain: AtomicU64;
    static CyberpunkVR_DebugPatchCamVrcam: AtomicU64;
    static CyberpunkVR_DebugPatchCamDevice: AtomicU64;
    static CyberpunkVR_DebugPatchCamOther: AtomicU64;
    static CyberpunkVR_DebugMainCamRejects: AtomicU64;
    static CyberpunkVR_DebugVrcamCamRejects: AtomicU64;
    static CyberpunkVR_DebugCamRebinds: AtomicU64;
    static CyberpunkVR_DebugBdCamHits: AtomicU64;
    fn CyberpunkVR_VrcamCamNameHash() -> u64;
    fn CyberpunkVR_VrcamBdCamNameHash() -> u64;
    fn cvr_roomscale_player_identity() -> usize;
}

/// Camera/CameraState.hpp's PatchFastPath (the trampoline reads owner[] and armed).
#[repr(C, align(64))]
pub struct PatchFastPath {
    owner: [usize; 3],
    armed: u32,
    pad: [u32; 3],
}

// ---- VrCore.cpp's objects the camera halves read (formerly C++ definitions)
/// PatchFastPath g_patchFast{}: the trampoline's fast-path block (its address is in the site's code).
#[export_name = "?g_patchFast@@3UPatchFastPath@@A"]
#[allow(non_upper_case_globals)]
pub static mut g_patchFast: PatchFastPath = PatchFastPath { owner: [0; 3], armed: 0, pad: [0; 3] };
/// std::atomic<unsigned long long> g_deviceCamLastMs{0}: the device camera's last tick.
#[export_name = "?g_deviceCamLastMs@@3U?$atomic@_K@std@@A"]
#[allow(non_upper_case_globals)]
pub static g_deviceCamLastMs: AtomicU64 = AtomicU64::new(0);
/// The camera objects' device and name offset (VrCore.cpp's file statics g_camObjDevice{0}, g_camNameOffset{-1}).
pub static CAM_OBJ_DEVICE: AtomicUsize = AtomicUsize::new(0);
pub static CAM_NAME_OFFSET: AtomicI32 = AtomicI32::new(-1);

/// What the C++ half provides.
#[repr(C)]
pub struct Runtime {
    cam_obj_device: *const AtomicUsize,
    cam_name_offset: *const AtomicI32,
    player_cam_on: *const AtomicI32,
    remote_cam_on: *const AtomicI32,
    takeover_entity_id: *const AtomicU64,
    player_cam_pos_fp: *const [AtomicI32; 3],
    remote_cam_pos_fp: *const [AtomicI32; 3],
    bd_want_fov_milli: *const AtomicI32,
    device_cam_last_ms: *const AtomicU64,
    patch_fast: *mut PatchFastPath,
    verbose_log: *const i32,
}
// SAFETY: the table holds addresses of statics.
unsafe impl Sync for Runtime {}
/// The table, built here (formerly VrCore.cpp's, attached at its static initialisation).
static RT: Runtime = Runtime {
    cam_obj_device: &raw const CAM_OBJ_DEVICE,
    cam_name_offset: &raw const CAM_NAME_OFFSET,
    player_cam_on: &raw const v::g_playerCamOn,
    remote_cam_on: &raw const v::g_remoteCamOn,
    takeover_entity_id: &raw const v::g_takeoverEntityId,
    player_cam_pos_fp: &raw const v::g_playerCamPosFP,
    remote_cam_pos_fp: &raw const v::g_remoteCamPosFP,
    bd_want_fov_milli: &raw const v::g_bdWantFovMilli,
    device_cam_last_ms: &raw const g_deviceCamLastMs,
    patch_fast: &raw mut g_patchFast,
    verbose_log: (&raw const v::g_verboseLog).cast(),
};
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const RT).cast_mut());
fn rt() -> &'static Runtime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*RUNTIME.load(Acquire) }
}
// SAFETY (these helpers): the C++ half's globals, alive for the process.
fn device() -> &'static AtomicUsize {
    unsafe { &*rt().cam_obj_device }
}
fn name_offset() -> &'static AtomicI32 {
    unsafe { &*rt().cam_name_offset }
}
fn bd_active() -> bool {
    unsafe { g_bdActive.load(Relaxed) != 0 }
}
fn player_cam_on() -> bool {
    unsafe { (*rt().player_cam_on).load(Relaxed) != 0 }
}
fn remote_cam_on() -> bool {
    unsafe { (*rt().remote_cam_on).load(Relaxed) != 0 }
}
fn verbose() -> bool {
    unsafe { core::ptr::read_volatile(rt().verbose_log) != 0 }
}
fn device_cam_follow() -> bool {
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_DeviceCamFollow) != 0 }
}
fn rtt_comp() -> usize {
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_DebugRttComp) as usize }
}
fn diag(counter: &AtomicU64) {
    // SAFETY: an export of the C++ half.
    if unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) } != 0 {
        counter.fetch_add(1, Relaxed);
    }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
fn ptr(p: usize) -> String {
    format!("{p:016X}")
}
fn second_view_wanted() -> bool {
    !crate::engine_stereo::enabled() && !crate::engine_stereo::mono_launch()
}

/// cname_hash("camera").
const CAM_NAME_MAIN: u64 = 0x6FCF_DF92_6F11_594E;
/// A braindance's MAIN within this of the place script publishes for the live player's camera.
const PLAYER_CAM_TOL_M: f32 = 2.0;

static SEEN_MAIN: AtomicBool = AtomicBool::new(false);
static SEEN_VRCAM: AtomicBool = AtomicBool::new(false);
thread_local! {
    static VRCAM_MISMATCH: Cell<i32> = const { Cell::new(0) };
    static MAIN_MISMATCH: Cell<i32> = const { Cell::new(0) };
}

/// PatchFastDisarm: the latch is gone or the world changed under it -- the slow path has to find the cameras again.
#[no_mangle]
pub extern "C" fn cvr_patch_fast_disarm() {
    SEEN_MAIN.store(false, Relaxed);
    SEEN_VRCAM.store(false, Relaxed);
    let pf = rt().patch_fast;
    // SAFETY: the C++ half's struct; no store in the steady state.
    unsafe {
        if core::ptr::read_volatile(&raw const (*pf).armed) != 0 {
            core::ptr::write_volatile(&raw mut (*pf).armed, 0);
        }
    }
}

fn patch_fast_note(kind: i32) {
    match kind {
        1 => SEEN_MAIN.store(true, Relaxed),
        2 => SEEN_VRCAM.store(true, Relaxed),
        _ => {}
    }
    // Without a second view (engine stereo, a mono launch) MAIN is the only camera there will be.
    let second_view = second_view_wanted();
    if !SEEN_MAIN.load(Relaxed) || (second_view && !SEEN_VRCAM.load(Relaxed)) {
        return;
    }
    if name_offset().load(Acquire) < 0 || bd_active() {
        return;
    }
    // ...and not while a takeover's lens is still being looked for (the lens is reached through the entity, which any
    // component of it reaches) -- unless the camera is claimed, which needs no hunt.
    // SAFETY: the C++ half's global.
    if remote_cam_on() && unsafe { g_lensComp.load(Relaxed) } == 0 && device().load(Relaxed) == 0 {
        return;
    }
    // SAFETY: as above.
    let (m, v) = unsafe { (g_camObjMain.load(Relaxed), g_camObjVrcam.load(Relaxed)) };
    let d = device().load(Relaxed);
    if m == 0 || (second_view && v == 0) {
        return;
    }
    let pf = rt().patch_fast;
    // SAFETY: the C++ half's struct, written only on a change.
    unsafe {
        for (i, want) in [m, v, d].into_iter().enumerate() {
            if core::ptr::read_volatile(&raw const (*pf).owner[i]) != want {
                core::ptr::write_volatile(&raw mut (*pf).owner[i], want);
            }
        }
        if core::ptr::read_volatile(&raw const (*pf).armed) != 1 {
            core::ptr::write_volatile(&raw mut (*pf).armed, 1);
        }
    }
}

fn main_camera_owner_matches(camera: usize) -> bool {
    // SAFETY: Rust's roomscale identity.
    let player = unsafe { cvr_roomscale_player_identity() };
    if player == 0 {
        return true; // the name-based bootstrap before GetPlayer is available
    }
    // SDK IComponent::owner: the same identity the native VRIK pair uses.
    code::read::<u64>(camera + 0x50).is_some_and(|owner| owner as usize == player)
}
fn cam_name_still_main(obj: usize) -> bool {
    let off = name_offset().load(Acquire);
    if off < 0 {
        return true; // not calibrated yet: no latch is dropped over it
    }
    code::read::<u64>(obj + off as usize).is_none_or(|nm| nm == CAM_NAME_MAIN)
}
/// The component's world position at +0xE0 (1/131072 m fixed point) within `tol` of `want`; None when unreadable.
fn position_within(obj: usize, want: &[AtomicI32; 3], tol: f32) -> Option<bool> {
    let mut p = [0i32; 3];
    for (i, v) in p.iter_mut().enumerate() {
        *v = code::read::<u32>(obj + 0xE0 + i * 4)? as i32;
    }
    let k = 1.0f32 / 131072.0;
    let mut d2 = 0.0f32;
    for i in 0..3 {
        let d = p[i].wrapping_sub(want[i].load(Relaxed)) as f32 * k;
        d2 += d * d;
    }
    Some(d2 <= tol * tol)
}
/// The live player's camera (an unreadable position is not a rejection: dropping MAIN would leave the head without a
/// camera).
fn player_cam_position_matches(obj: usize) -> bool {
    // SAFETY: the C++ half's array.
    position_within(obj, unsafe { &*rt().player_cam_pos_fp }, PLAYER_CAM_TOL_M).unwrap_or(true)
}
/// The camera the script side named for a takeover.
#[no_mangle]
pub extern "C" fn cvr_device_cam_position_matches(obj: usize) -> bool {
    // SAFETY: an export of the C++ half.
    let t = unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_DevCamTolM) };
    let tol = if t > 0.05 && t < 100.0 { t } else { 1.5 };
    // SAFETY: the C++ half's array.
    position_within(obj, unsafe { &*rt().remote_cam_pos_fp }, tol).unwrap_or(false)
}
/// StampDeviceCam: the lens aim refresh -- after 300 ms without a patch, what was last written is zeroed so the aim is
/// re-read on the next one (the base and position flags are cleared only when control is released).
#[no_mangle]
pub extern "C" fn cvr_stamp_device_cam() {
    // SAFETY: a plain system query; the C++ half's globals.
    unsafe {
        let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
        let prev = (*rt().device_cam_last_ms).swap(now, Relaxed);
        if prev == 0 || now.wrapping_sub(prev) >= 300 {
            core::ptr::write_volatile(&raw mut g_devCamLastWritten, [0.0; 4]);
        }
    }
}

/// ClassifyPatchCameraOwner: 0 none of ours, 1 MAIN, 2 the second view, 3 a device or braindance camera.
#[no_mangle]
pub extern "C" fn cvr_classify_patch_camera_owner(owner: *mut c_void) -> i32 {
    let obj = owner as usize;
    if obj < 0x10000 {
        return 0;
    }
    // SAFETY (the block): the C++ half's latches and counters.
    unsafe {
        // The fast paths, which give the latch up when the object stops being the right one.
        if obj == g_camObjMain.load(Relaxed) {
            if main_camera_owner_matches(obj)
                && (!bd_active() || !player_cam_on() || cam_name_still_main(obj))
            {
                diag(&CyberpunkVR_DebugPatchCamMain);
                patch_fast_note(1);
                return 1;
            }
            g_camObjMain.store(0, Relaxed);
            cvr_patch_fast_disarm();
            diag(&CyberpunkVR_DebugMainCamRejects);
        }
        if obj == g_camObjVrcam.load(Relaxed) {
            let rtt = rtt_comp();
            if rtt == 0 || obj == rtt {
                VRCAM_MISMATCH.with(|m| m.set(0));
                diag(&CyberpunkVR_DebugPatchCamVrcam);
                patch_fast_note(2);
                return 2;
            }
            // One mismatch is not a reason to drop the write: the RTT component is written on the render thread and
            // can disagree for a pass; a real replacement persists.
            let m = VRCAM_MISMATCH.with(|m| {
                m.set(m.get() + 1);
                m.get()
            });
            if m < 4 {
                diag(&CyberpunkVR_DebugPatchCamVrcam);
                patch_fast_note(2);
                return 2;
            }
            VRCAM_MISMATCH.with(|m| m.set(0));
            g_camObjVrcam.store(0, Relaxed);
            cvr_patch_fast_disarm();
            diag(&CyberpunkVR_DebugVrcamCamRejects);
        }
        if device_cam_follow() && obj == device().load(Relaxed) {
            diag(&CyberpunkVR_DebugPatchCamDevice);
            cvr_stamp_device_cam();
            return 3;
        }

        let vrcam = CyberpunkVR_VrcamCamNameHash();
        let mut off = name_offset().load(Acquire);
        if off < 0 {
            let mut k = 0x08usize;
            while k <= 0x80 {
                let Some(v) = code::read::<u64>(obj + k) else {
                    break;
                };
                let vrcam_bd = CyberpunkVR_VrcamBdCamNameHash();
                if v == CAM_NAME_MAIN
                    || (vrcam != 0 && v == vrcam)
                    || (vrcam_bd != 0 && v == vrcam_bd)
                {
                    name_offset().store(k as i32, Release);
                    log(&format!(
                        "PatchCamera: component name CName found at owner+0x{k:02X} (main=0x{CAM_NAME_MAIN:016X} vrcam=0x{vrcam:016X})\n"
                    ));
                    off = k as i32;
                    break;
                }
                k += 8;
            }
            if off < 0 {
                return 0; // this object is not one of ours; try the next
            }
        }
        let name = match code::read::<u64>(obj + off as usize) {
            Some(n) if n != 0 => n,
            _ => return 0,
        };
        if name == CAM_NAME_MAIN {
            // Other puppets can keep updating a component named "camera": their activity cannot prove the latched
            // camera belongs to GetPlayer.
            if !main_camera_owner_matches(obj) {
                diag(&CyberpunkVR_DebugMainCamRejects);
                diag(&CyberpunkVR_DebugPatchCamOther);
                return 0;
            }
            // Two objects answer to this name in a braindance; the place script publishes for the LIVE player's camera
            // decides, and only a repeated miss rejects (a rejection skips this pass's camera write).
            if bd_active() && player_cam_on() && !player_cam_position_matches(obj) {
                let m = MAIN_MISMATCH.with(|m| {
                    m.set(m.get() + 1);
                    m.get()
                });
                if m >= 4 {
                    MAIN_MISMATCH.with(|m| m.set(0));
                    diag(&CyberpunkVR_DebugMainCamRejects);
                    diag(&CyberpunkVR_DebugPatchCamOther);
                    return 0;
                }
            } else {
                MAIN_MISMATCH.with(|m| m.set(0));
            }
            let prev = g_camObjMain.load(Relaxed);
            if prev != obj && prev != 0 && bd_active() && verbose() {
                log(&format!(
                    "PatchCamera: MAIN camera is now {} (was {})\n",
                    ptr(obj),
                    ptr(prev)
                ));
            }
            g_camObjMain.store(obj, Relaxed); // the fast path's latch
            diag(&CyberpunkVR_DebugCamRebinds);
            diag(&CyberpunkVR_DebugPatchCamMain);
            patch_fast_note(1);
            return 1;
        }
        let vrcam_bd = CyberpunkVR_VrcamBdCamNameHash();
        if (vrcam != 0 && name == vrcam) || (vrcam_bd != 0 && name == vrcam_bd) {
            // No second view, no second camera: these components render nothing then, and a latch would only be a
            // pointer that could outlive its object.
            if !second_view_wanted() {
                return 0;
            }
            // Two sets of these components are authored and one is enabled at a time: the RTT view-create's component
            // is the identity once known.
            let rtt = rtt_comp();
            if rtt != 0 && obj != rtt {
                diag(&CyberpunkVR_DebugVrcamCamRejects);
                diag(&CyberpunkVR_DebugPatchCamOther);
                return 0;
            }
            let prev = g_camObjVrcam.load(Relaxed);
            if prev != obj && prev != 0 && verbose() {
                log(&format!(
                    "PatchCamera: second view is now {} (was {}) name=0x{name:016X} rtt={}\n",
                    ptr(obj),
                    ptr(prev),
                    ptr(rtt)
                ));
            }
            g_camObjVrcam.store(obj, Relaxed);
            diag(&CyberpunkVR_DebugCamRebinds);
            diag(&CyberpunkVR_DebugPatchCamVrcam);
            return 2;
        }
        if device_cam_follow() && remote_cam_on() {
            let name_device = crate::vrcam_config::cname_hash(b"cameraComponent");
            let name_ok = name == name_device
                || core::ptr::read_volatile(&raw const CyberpunkVR_DevCamAnyName) != 0;
            // A sticky latch: one camera per takeover, decided once, released when the takeover ends.
            let cur = device().load(Relaxed);
            if cur != 0 {
                if obj != cur {
                    diag(&CyberpunkVR_DebugPatchCamOther);
                    return 0;
                }
                diag(&CyberpunkVR_DebugPatchCamDevice);
                cvr_stamp_device_cam();
                return 3;
            }
            // The entity first (exact, motion cannot break it), the place only as a fallback.
            let want_id = (*rt().takeover_entity_id).load(Relaxed);
            let mut identified = false;
            if want_id != 0 {
                let owner = code::read::<u64>(obj + 0x50).filter(|&o| o > 0x10000);
                let oid = owner.and_then(|o| code::read::<u64>(o as usize + 0x48));
                if let (Some(owner), Some(oid)) = (owner, oid) {
                    if oid == want_id {
                        let ent = owner as usize;
                        if g_takeoverEntity.swap(ent, Relaxed) != ent {
                            crate::camera_takeover::cvr_bind_lens_from_entity(ent);
                        }
                        identified = name == name_device;
                    }
                }
            } else {
                identified = name_ok && cvr_device_cam_position_matches(obj);
            }
            if identified {
                let prev = device().swap(obj, Relaxed);
                if prev != obj && verbose() {
                    log(&format!(
                        "PatchCamera: device camera component {} (was {}) hits={}\n",
                        ptr(obj),
                        ptr(prev),
                        CyberpunkVR_DebugPatchCamDevice.load(Relaxed) as u32
                    ));
                }
                diag(&CyberpunkVR_DebugCamRebinds);
                diag(&CyberpunkVR_DebugPatchCamDevice);
                g_devCamBaseValid.store(0, Relaxed); // a different camera: re-latch
                g_devCamPosValid.store(0, Relaxed);
                cvr_stamp_device_cam();
                return 3;
            }
        }
        // BRAINDANCE: the camera whose own fov is the one the script reports for the active camera, matched once and
        // then latched by address.
        if device_cam_follow() && bd_active() {
            let want_milli = (*rt().bd_want_fov_milli).load(Relaxed);
            if want_milli > 5000 && want_milli < 179000 {
                if let Some(cam_fov) =
                    code::read::<f32>(obj + 0x128).filter(|f| *f > 1.0 && *f < 179.0)
                {
                    let got_milli = (cam_fov * 1000.0) as i32;
                    if got_milli > want_milli - 30 && got_milli < want_milli + 30 {
                        let prev = device().swap(obj, Relaxed);
                        if prev != obj && verbose() {
                            log(&cvr_crt::format(
                                c"PatchCamera: braindance camera %p (was %p) name=0x%016llX fov=%.3f want=%.3f\n",
                                &[
                                    cvr_crt::Arg::U(obj as u64),
                                    cvr_crt::Arg::U(prev as u64),
                                    cvr_crt::Arg::U(name),
                                    cvr_crt::Arg::F(f64::from(cam_fov)),
                                    cvr_crt::Arg::F(f64::from(want_milli) * 0.001),
                                ],
                            )
                            .to_string_lossy());
                        }
                        diag(&CyberpunkVR_DebugBdCamHits);
                        diag(&CyberpunkVR_DebugCamRebinds);
                        diag(&CyberpunkVR_DebugPatchCamDevice);
                        g_devCamBaseValid.store(0, Relaxed);
                        g_devCamPosValid.store(0, Relaxed);
                        cvr_stamp_device_cam();
                        return 3;
                    }
                }
            }
        }
        diag(&CyberpunkVR_DebugPatchCamOther);
        0
    }
}
