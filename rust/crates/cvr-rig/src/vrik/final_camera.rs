//! FinalCamera (formerly src/Hooks/FinalCamera.cpp): reads back the camera transform the engine is about to render
//! with, inside CRenderNode_PrepareSceneRendering (2.5 sub_14229B520+EF), once per view.
//!
//! On foot, position, quaternion and eye identify its XR label (vrik::pose_identity, then the placed-pose and
//! write-record histories of vrik::camera_link); the matched pose goes into the render/Present FIFO of its view, so an
//! eye is submitted with the pose its pixels were drawn with. MAIN also publishes the barrel packet (the camera
//! quaternion and the latched muzzle direction, one packet). It is a LEAF: nothing Locate or Patch read is written
//! here.
//!
//! THE MONO WRITE PATH (CyberpunkVR_CamWriteInFinal, default off): `rsi` is the render camera + 0x70 -- int32
//! fixed-point position at [0..2], the rotation rows the matrix bake consumes at +0xC0. Writing here owns both eyes
//! and every other view of the image (distant, reflection, shadow): one orientation for all, the lateral IPD term only
//! for a view whose eye is known.
use super::camera_link::{
    cvr_camera_barrel_frame_publish, cvr_camera_cam_write_quat_read, cvr_camera_cam_write_record_find, cvr_camera_placed_pose_enabled,
    cvr_camera_placed_pose_find, BarrelFrame, HeadPose,
};
use super::camera_math::{
    cvr_camera_desired_half_ipd, cvr_camera_plausible_unit_quaternion, cvr_camera_plausible_unit_vector3, cvr_camera_right_vector_from_quaternion,
    cvr_camera_write_render_camera_basis,
};
use super::cyberware::cvr_cyberware_launcher_line;
use super::pose_identity::{cvr_pose_read_camera, PoseIdentity};
use cvr_engine::code::read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut g_finalCameraHits: u64;
    static mut g_lastLocateSeq: u32;
    static mut g_renderedSeq: u32;
    static mut g_headQuatValid: u32;
    static CyberpunkVR_StereoModuleLoaded: i32;
    static CyberpunkVR_PoseReadBack: i32;
    static CyberpunkVR_CamWriteInFinal: i32;
    static CyberpunkVR_CamFinalViewScope: i32;
    static mut CyberpunkVR_DebugFinalAge: u32;
    static mut CyberpunkVR_DebugFinalTies: u32;
    static mut CyberpunkVR_DebugFinalTieHits: u64;
    static mut CyberpunkVR_DebugFinalMatch: u64;
    static mut CyberpunkVR_DebugFinalNoMatch: u64;
    static mut CyberpunkVR_DebugVrcamFinalAge: u32;
    static mut CyberpunkVR_DebugVrcamFinalMatch: u64;
    static mut CyberpunkVR_DebugVrcamFinalNoMatch: u64;
    static mut CyberpunkVR_DebugViewCamMain: u64;
    static mut CyberpunkVR_DebugViewCamVrcam: u64;
    static mut CyberpunkVR_DebugViewCamOther: u64;
    static CyberpunkVR_PoseIdFinal: [AtomicU64; 2];
    static CyberpunkVR_PoseIdFinalMiss: [AtomicU64; 2];
    fn CyberpunkVR_IsVrcamViewActive() -> i32;
    fn CyberpunkVR_IsMainViewActive() -> i32;
    fn cvr_cpp_telemetry_final(hits: u32, rsi: usize);
    fn cvr_cpp_shot_shared() -> *mut f32;
}

fn diagnostics() -> bool {
    // SAFETY: a plain exported int.
    unsafe { CyberpunkVR_RuntimeDiagnostics != 0 }
}
fn floats<const N: usize>(at: usize) -> Option<[f32; N]> {
    read::<[f32; N]>(at)
}

/// The last real muzzle direction (see the barrel packet below).
static MUZZLE_FWD: Mutex<[f32; 3]> = Mutex::new([0.0; 3]);

/// The pose label of the frame this camera is drawn with: the pose identity first, then the placed-pose or
/// write-record history.
unsafe fn match_rendered_pose(camera: usize, q: &[f32; 4], view: u32, pose: &mut HeadPose, age: &mut u32, ties: &mut u32) -> bool {
    let mut identity = PoseIdentity::default();
    // SAFETY: as the callers vouch: a camera address and locals.
    unsafe {
        if cvr_pose_read_camera(camera, view, &mut identity) {
            *pose = identity.head;
            *age = 0;
            *ties = 1;
            if diagnostics() {
                CyberpunkVR_PoseIdFinal[view as usize - 1].fetch_add(1, Ordering::Relaxed);
            }
            return true;
        }
        if diagnostics() {
            CyberpunkVR_PoseIdFinalMiss[view as usize - 1].fetch_add(1, Ordering::Relaxed);
        }
        if !cvr_camera_placed_pose_enabled() {
            return cvr_camera_cam_write_record_find(q, pose, age, ties);
        }
        let Some(position) = read::<[i32; 3]>(camera) else { return false };
        cvr_camera_placed_pose_find(view, q, &position, pose, age, ties)
    }
}

/// `rsi` = the render camera + 0x70, from the stub at the site.
#[no_mangle]
pub unsafe extern "C" fn OnFinalCameraCallback(rsi: *mut f32) {
    // SAFETY: the plugin's globals, the engine's camera under SEH reads, and C++ services.
    unsafe {
        g_finalCameraHits = g_finalCameraHits.wrapping_add(1);
        cvr_cpp_telemetry_final(g_finalCameraHits as u32, rsi as usize);
        let locate_seq = core::ptr::read_volatile(&raw const g_lastLocateSeq);
        if locate_seq != 0 {
            core::ptr::write_volatile(&raw mut g_renderedSeq, locate_seq);
        }
        let at = rsi as usize;
        if rsi.is_null() || at < 0x10000 {
            return;
        }
        // ---- READ BACK THE POSE THIS FRAME IS ACTUALLY BEING BUILT WITH ----
        // rsi + 4 floats is the camera quaternion at object+0x80, the one we composed and wrote into the placed
        // component, carried through the view producer.
        if CyberpunkVR_StereoModuleLoaded != 0 {
            let is_vrcam = CyberpunkVR_IsVrcamViewActive() != 0;
            let is_main = !is_vrcam && CyberpunkVR_IsMainViewActive() != 0;
            if is_main {
                let mut matched = HeadPose::default();
                if let Some(camq) = floats::<4>(at + 16).filter(|q| cvr_camera_plausible_unit_quaternion(q.as_ptr())) {
                    // LATCHED: with no weapon the muzzle quaternion is identity and the publisher sends its +Y as
                    // exactly (0,1,0) -- not a barrel direction. The last real one is kept, so the direction and the
                    // quaternion leave as ONE packet.
                    let mut latched = MUZZLE_FWD.lock().unwrap_or_else(|p| p.into_inner());
                    let sh = cvr_cpp_shot_shared();
                    if !sh.is_null() && *sh.add(27) >= 0.5 {
                        let (x, y, z) = (*sh.add(24), *sh.add(25), *sh.add(26));
                        let identity_default = x == 0.0 && y == 1.0 && z == 0.0;
                        if !identity_default && x * x + y * y + z * z > 0.25 {
                            *latched = [x, y, z];
                        }
                    }
                    // The selected arm launcher aims the mark, with no gun ever drawn to latch a direction from.
                    let (mut launcher_origin, mut launcher_fwd) = ([0.0f32; 3], [0.0f32; 3]);
                    let muzzle =
                        if cvr_cyberware_launcher_line(launcher_origin.as_mut_ptr(), launcher_fwd.as_mut_ptr()) { launcher_fwd } else { *latched };
                    drop(latched);
                    if muzzle[0] * muzzle[0] + muzzle[1] * muzzle[1] + muzzle[2] * muzzle[2] > 0.25 {
                        cvr_camera_barrel_frame_publish(&BarrelFrame { cam_quat: camq, muzzle_fwd: muzzle });
                    }
                    if CyberpunkVR_PoseReadBack != 0 {
                        let (mut age, mut ties) = (0u32, 0u32);
                        if match_rendered_pose(at, &camq, 1, &mut matched, &mut age, &mut ties) {
                            CyberpunkVR_DebugFinalAge = age;
                            CyberpunkVR_DebugFinalTies = ties;
                            if diagnostics() {
                                if ties > 1 {
                                    CyberpunkVR_DebugFinalTieHits += 1;
                                }
                                CyberpunkVR_DebugFinalMatch += 1;
                            }
                        } else if diagnostics() {
                            CyberpunkVR_DebugFinalNoMatch += 1;
                        }
                    }
                }
                // Keep an invalid slot on a miss, so a later success cannot label this frame by shifting the
                // render/Present FIFO by one entry.
                if CyberpunkVR_PoseReadBack != 0 {
                    super::xr_calls::push_rendered_frame_pose(&matched, false);
                }
            }
            // THE SECOND VIEW, IDENTIFIED THE SAME WAY, INTO ITS OWN QUEUE (one queue cannot carry two views without
            // running at double rate). The barrel packet is not duplicated.
            if is_vrcam && CyberpunkVR_PoseReadBack != 0 {
                let mut matched = HeadPose::default();
                if let Some(camq) = floats::<4>(at + 16).filter(|q| cvr_camera_plausible_unit_quaternion(q.as_ptr())) {
                    let (mut age, mut ties) = (0u32, 0u32);
                    if match_rendered_pose(at, &camq, 2, &mut matched, &mut age, &mut ties) {
                        CyberpunkVR_DebugVrcamFinalAge = age;
                        if diagnostics() {
                            CyberpunkVR_DebugVrcamFinalMatch += 1;
                        }
                    } else if diagnostics() {
                        CyberpunkVR_DebugVrcamFinalNoMatch += 1;
                    }
                }
                super::xr_calls::push_rendered_frame_pose(&matched, true);
            }
        }

        // ---- MONO: THE PER-VIEW WRITE SITE ----
        // (The verbose probe after this block in the C++ original was unreachable: both paths return here.)
        if CyberpunkVR_CamWriteInFinal == 0 {
            return;
        }
        // ASK THE DISPATCHER, DO NOT HASH A NAME: the view key is the CAMERA name's hash, the owner classification
        // the COMPONENT's.
        let is_vrcam = CyberpunkVR_IsVrcamViewActive() != 0;
        let is_main = !is_vrcam && CyberpunkVR_IsMainViewActive() != 0;
        let is_eye = is_main || is_vrcam;
        // EVERY VIEW IN THE IMAGE: the distant, reflection and shadow views belong to the same eye and instant.
        if is_eye {
            if diagnostics() {
                if is_main {
                    CyberpunkVR_DebugViewCamMain += 1;
                } else {
                    CyberpunkVR_DebugViewCamVrcam += 1;
                }
            }
        } else {
            if diagnostics() {
                CyberpunkVR_DebugViewCamOther += 1;
            }
            if CyberpunkVR_CamFinalViewScope == 0 {
                return;
            }
        }
        let mut hq = [0.0f32, 0.0, 0.0, 1.0];
        if core::ptr::read_volatile(&raw const g_headQuatValid) == 0
            || !cvr_camera_cam_write_quat_read(&mut hq, core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut())
            || !cvr_camera_plausible_unit_quaternion(hq.as_ptr())
        {
            return;
        }
        // One orientation for every view -- it is one head. The eyes differ by the lateral IPD term and nothing else.
        cvr_camera_write_render_camera_basis(rsi, hq.as_ptr());
        let half = if is_eye { cvr_camera_desired_half_ipd() } else { 0.0 };
        if half != 0.0 {
            let mut right = [0.0f32; 3];
            cvr_camera_right_vector_from_quaternion(hq.as_ptr(), right.as_mut_ptr());
            if cvr_camera_plausible_unit_vector3(right.as_ptr()) {
                let sign = if is_vrcam { 1.0f32 } else { -1.0 }; // MAIN = left, VRCAM = right
                let pos = rsi as *mut i32;
                for (i, r) in right.iter().enumerate() {
                    *pos.add(i) = (*pos.add(i)).wrapping_add((r * half * sign * 131072.0) as i32);
                }
            }
        }
    }
}

/// divss xmm9,[rsi+40h] / add r8,1090h (2.5 sub_14229B520+EF; 2.31 sub_1407854C0+B8 had 0F20h)
const PATTERN: &[u8] = b"\xF3\x44\x0F\x5E\x4E\x40\x49\x81\xC0\x90\x10\x00\x00";
const MASK: &[u8] = b"xxxxxxxxxxxxx";
const REPLACE: usize = 13;

/// Installed from src/Hooks/FinalCamera.cpp's registration (Boot, 14).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_final_camera_install() -> bool {
    use cvr_engine::code::find_pattern;
    use cvr_engine::trampoline::{allocate, patch_jump, Code};
    let Some(found) = find_pattern("Cyberpunk2077.exe", PATTERN, MASK) else { return false };
    let Some(tramp) = allocate(found, 512) else { return false };
    let mut c = Code::new(tramp);
    c.volatile_call(4, &[0x48, 0x89, 0xF1], OnFinalCameraCallback as *const () as u64) // rcx = rsi
        .bytes(&PATTERN[..REPLACE]) // the original divss and add
        .jmp(found + REPLACE);
    // SAFETY: fresh arena memory, then a site whose 13 bytes the stub reproduces.
    unsafe {
        c.commit();
        patch_jump(found, REPLACE, tramp);
    }
    true
}
