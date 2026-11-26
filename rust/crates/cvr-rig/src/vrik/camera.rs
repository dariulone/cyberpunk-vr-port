//! The FPP camera (HMD) in model space and the body axes from its yaw.
use super::env::*;
use super::types::*;
use super::*;
use crate::math::{cross3, norm3, quat_conj, quat_mul, quat_norm, quat_rotate_vec};
use cvr_body::cmath::{atan2, cos, sin};
use std::sync::atomic::{AtomicU64, Ordering};

/// `CVR_DIAGNOSTIC(atomic_ref(x).fetch_add(1))`: counted only while the runtime diagnostics are on.
unsafe fn diagnostic_count(counter: *mut u64) {
    if CyberpunkVR_RuntimeDiagnostics != 0 {
        (*(counter as *const AtomicU64)).fetch_add(1, Ordering::Relaxed);
    }
}

/// PHASE-COHERENT MODEL TRANSFORM: the camera/entity pair of ONE publication, converted with that publication's own
/// entity quaternion; the head orientation from the same XR sample PatchCamera composes with, where it composes.
#[no_mangle]
pub unsafe extern "C" fn VRIK_ComputeCamModel(
    outPos: *mut f32,
    outRot: *mut f32,
    outEntityQuat: *mut f32,
    outPairedRot: *mut f32,
    outBodyBase: *mut f32,
    status: *mut i32,
) -> bool {
    if !status.is_null() {
        *status = CAMERA_UNAVAILABLE;
    }
    let component_pair = CyberpunkVR_VrikNativeFramePair != 0
        && CyberpunkVR_CamWriteInPatch != 0
        && CyberpunkVR_CamComposeAtWrite != 0
        && CyberpunkVR_HeadTranslationInPatch != 0;
    if CyberpunkVR_VrikNativeFramePair != 0 || CyberpunkVR_VrikTransformsFromPlugin != 0 {
        let mut snap = VrikTransformSnapshot::default();
        let mut have_snapshot = false;
        // MOUNTED, the native pair is the wrong frame (a yaw-only entity): the Lua pair carries the full entity quat.
        let prefer_full_entity_quat =
            g_isInVehicle && CyberpunkVR_VrikVehicleFullEntityQuat != 0 && CyberpunkVR_VrikTransformsFromPlugin != 0;
        if prefer_full_entity_quat {
            have_snapshot = VRIK_ReadTransformSnapshot(&mut snap);
            if have_snapshot {
                diagnostic_count(&raw mut CyberpunkVR_DebugVrikLuaPairFallback);
            }
            // A missed mounted pair must not switch to the on-foot yaw frame.
            if !have_snapshot {
                return false;
            }
        }
        if !have_snapshot && component_pair {
            have_snapshot = VRIK_ReadNativeTransformSnapshot(&mut snap);
            if have_snapshot {
                diagnostic_count(&raw mut CyberpunkVR_DebugVrikNativePairUsed);
            }
        }
        if !have_snapshot && CyberpunkVR_VrikTransformsFromPlugin != 0 && (!component_pair || g_isInVehicle) {
            have_snapshot = VRIK_ReadTransformSnapshot(&mut snap);
            if have_snapshot {
                diagnostic_count(&raw mut CyberpunkVR_DebugVrikLuaPairFallback);
            }
        }
        if have_snapshot {
            // An explicit invalidation is not a transient seqlock miss.
            if snap.valid == 0 {
                if !status.is_null() {
                    *status = if snap.unavailable != 0 { CAMERA_UNAVAILABLE } else { CAMERA_INVALIDATED };
                }
                return false;
            }
            let mut pushed_ent_q = snap.entityQuat;
            quat_norm(&mut pushed_ent_q);
            if !outEntityQuat.is_null() {
                put4(outEntityQuat, &pushed_ent_q);
            }
            let inv_pushed_ent = quat_conj(&pushed_ent_q);
            put3(outPos, &quat_rotate_vec(&inv_pushed_ent, &snap.cameraMinusEntity));
            if !outBodyBase.is_null() {
                put3(outBodyBase, &quat_rotate_vec(&inv_pushed_ent, &snap.bodyCameraMinusEntity));
            }
            let mut pushed_cam_q = snap.camQuat;
            quat_norm(&mut pushed_cam_q);
            let mut rot = quat_mul(&inv_pushed_ent, &pushed_cam_q);
            quat_norm(&mut rot);
            put4(outRot, &rot);
            if !outPairedRot.is_null() {
                put4(outPairedRot, &rot);
            }
            // Mirror PatchCamera's direct-composition gates.
            let camera_uses_direct_engine_yaw = CyberpunkVR_CamWriteInPatch != 0
                && CyberpunkVR_CamComposeAtWrite != 0
                && g_headingValid != 0
                && CyberpunkVR_HeadingFromPreWrite != 0
                && CyberpunkVR_ViewYawFromEngine != 0
                && CyberpunkVR_EngineBodyYawValid != 0
                && !g_isInVehicle;
            if camera_uses_direct_engine_yaw {
                let mut head = [0.0f32; 4];
                if cvr_cpp_vrik_frame_head_orientation(head.as_mut_ptr()) {
                    let mut wz = CyberpunkVR_EngineBodyYawZ;
                    let mut ww = CyberpunkVR_EngineBodyYawW;
                    if ww < 0.0 {
                        wz = -wz;
                        ww = -ww;
                    }
                    if wz != 0.0 || ww != 0.0 {
                        let mut current_ent_q = [0.0, 0.0, wz, ww];
                        quat_norm(&mut current_ent_q);
                        let inv_current_ent = quat_conj(&current_ent_q);
                        let view_yaw = cvr_cpp_vrik_body_tracking_yaw(2.0 * atan2(wz, ww));
                        let yaw_q = [0.0, 0.0, sin(view_yaw * 0.5), cos(view_yaw * 0.5)];
                        let pitch_q = [g_headingPitchS, 0.0, 0.0, g_headingPitchC];
                        let head_q = [head[0], -head[2], head[1], head[3]];
                        let heading_pitch = quat_mul(&yaw_q, &pitch_q);
                        let mut world_cam_q = quat_mul(&heading_pitch, &head_q);
                        quat_norm(&mut world_cam_q);
                        let mut r = quat_mul(&inv_current_ent, &world_cam_q);
                        quat_norm(&mut r);
                        put4(outRot, &r);
                    }
                }
            }
            if !status.is_null() {
                *status = CAMERA_VALID;
            }
            return true;
        }
        // A missing coherent centred pair is not permission to read the unframed legacy mirrors below.
        if component_pair && !g_isInVehicle {
            return false;
        }
    }
    if g_VRCamPosValid == 0 {
        return false;
    }
    let mut ent_q = [g_VREntityQI, g_VREntityQJ, g_VREntityQK, g_VREntityQR];
    if (ent_q[0] * ent_q[0] + ent_q[1] * ent_q[1] + ent_q[2] * ent_q[2] + ent_q[3] * ent_q[3]) < 1e-6 {
        ent_q = ID;
    }
    quat_norm(&mut ent_q);
    if !outEntityQuat.is_null() {
        put4(outEntityQuat, &ent_q);
    }
    let inv_ent = quat_conj(&ent_q);
    let cam_q = [g_VRCamI, g_VRCamJ, g_VRCamK, g_VRCamR];
    let mut rot = quat_mul(&inv_ent, &cam_q);
    quat_norm(&mut rot);
    put4(outRot, &rot);
    if !outPairedRot.is_null() {
        put4(outPairedRot, &rot);
    }
    // WELDED BODY ANCHOR: the one stabilized (cam - entity) offset the rendered view applies too.
    let d = if g_VRCamPairValid != 0 {
        [g_VRCamPairLocalX, g_VRCamPairLocalY, g_VRCamPairLocalZ]
    } else {
        [g_VRCamPosX - g_VREntityPosX, g_VRCamPosY - g_VREntityPosY, g_VRCamPosZ - g_VREntityPosZ]
    };
    let pos = quat_rotate_vec(&inv_ent, &d);
    put3(outPos, &pos);
    if !outBodyBase.is_null() {
        put3(outBodyBase, &pos);
    }
    if !status.is_null() {
        *status = CAMERA_VALID;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_BodyAxesFromCamYaw(camModelRot: *const f32, bodyRight: *mut f32, bodyUp: *mut f32, bodyFwd: *mut f32) {
    let up = [0.0f32, 0.0, 1.0];
    put3(bodyUp, &up);
    // OpenXR -Z forward maps to game/model +Y.
    let mut fwd = quat_rotate_vec(&a4(camModelRot), &[0.0, 1.0, 0.0]);
    fwd[2] = 0.0;
    if norm3(&mut fwd) < 1e-4 {
        fwd = [0.0, 1.0, 0.0];
    }
    let mut right = cross3(&fwd, &up);
    if norm3(&mut right) < 1e-4 {
        right = [1.0, 0.0, 0.0];
    }
    fwd = cross3(&up, &right);
    if norm3(&mut fwd) < 1e-4 {
        fwd = [0.0, 1.0, 0.0];
    }
    put3(bodyRight, &right);
    put3(bodyFwd, &fwd);
}
