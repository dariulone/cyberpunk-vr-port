//! The solve's caches and the bones it publishes: the shadow chain synchronised to the solved upper body, the
//! model and local-arm caches a replayed pass writes, the probes the natives read, and the view/hand frame helpers.
use super::env::*;
use super::rig::*;
use super::state::*;
use super::types::*;
use super::*;
use crate::math::{quat_conj, quat_mul, quat_norm, quat_rotate_vec};
use cvr_body::cmath::{cos, sin};

#[no_mangle]
pub unsafe extern "C" fn VRIK_AlignViewToHands(
    viewWorldRot: *const f32,
    viewHeadXr: *const f32,
    handHeadXr: *const f32,
    outWorldRot: *mut f32,
) -> bool {
    let mut view_head = a4(viewHeadXr);
    let mut hand_head = a4(handHeadXr);
    let mut view = a4(viewWorldRot);
    for q in [&view_head, &hand_head, &view] {
        let n = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
        if !n.is_finite() || n < 1e-6 {
            return false;
        }
    }
    quat_norm(&mut view_head);
    quat_norm(&mut hand_head);
    quat_norm(&mut view);
    let inverse = quat_conj(&view_head);
    let delta_xr = quat_mul(&inverse, &hand_head);
    let delta_game = [delta_xr[0], -delta_xr[2], delta_xr[1], delta_xr[3]];
    let mut out = quat_mul(&view, &delta_game);
    quat_norm(&mut out);
    put4(outWorldRot, &out);
    true
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_WorldToModelRotation(fallbackYaw: f32, entityRotation: *const f32, result: *mut f32) {
    if !entityRotation.is_null() {
        let mut r = quat_conj(&a4(entityRotation));
        quat_norm(&mut r);
        put4(result, &r);
    } else {
        *result = 0.0;
        *result.add(1) = 0.0;
        *result.add(2) = -sin(fallbackYaw * 0.5);
        *result.add(3) = cos(fallbackYaw * 0.5);
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_BuildViewHandTarget(
    viewModelPos: *const f32,
    viewModelRot: *const f32,
    handLocalPos: *const f32,
    handLocalRot: *const f32,
    scale: f32,
    offset: *const f32,
    target: *mut f32,
    controllerModelRot: *mut f32,
) {
    let mapped = [*handLocalPos * scale, -*handLocalPos.add(2) * scale, *handLocalPos.add(1) * scale];
    let view_rot = a4(viewModelRot);
    let delta = quat_rotate_vec(&view_rot, &mapped);
    for k in 0..3 {
        *target.add(k) = *viewModelPos.add(k) + delta[k] + *offset.add(k);
    }
    let mapped_rot = [*handLocalRot, -*handLocalRot.add(2), *handLocalRot.add(1), *handLocalRot.add(3)];
    let mut r = quat_mul(&view_rot, &mapped_rot);
    quat_norm(&mut r);
    put4(controllerModelRot, &r);
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_SyncShadowUpper(boneBuf: *mut u8, preserveLowerBody: bool) {
    let count = VRIK_FKCount().min(MAXB);
    VRIK_ComputeFK(boneBuf, count);
    for bone in 0..count {
        let bu = bone as usize;
        let leg = s_bendLegShadowSource[bu];
        let source = if g_VRShadowSource[bu] >= 0 {
            g_VRShadowSource[bu]
        } else if leg >= 0 && s_bendLegOwned[leg as usize] {
            leg
        } else {
            -1
        };
        if source < 0 || source >= count {
            continue;
        }
        if preserveLowerBody && (source == g_VRHipsIdx || !g_VRUpperOwned[source as usize]) {
            continue;
        }
        let parent = g_VRBoneParent[bu] as i32;
        let su = source as usize;
        write_local_pos(boneBuf, bone, &parent_pos(parent), &parent_rot(parent), &g_fkPos[su]);
        write_local_rot(boneBuf, bone, &parent_rot(parent), &g_fkRot[su]);
        g_fkPos[bu] = g_fkPos[su];
        g_fkRot[bu] = g_fkRot[su];
    }
    // Also refresh unmapped children of the shadow chain.
    VRIK_ComputeFK(boneBuf, count);
    for saved in s_bendLegRestore.iter_mut() {
        if saved.buffer == boneBuf && saved.revision == s_rigRevision && saved.count >= 6 && s_bendLegOwned[saved.bones[0] as usize] {
            for i in 6..saved.count as usize {
                copy_bone(&mut saved.solved[i], boneBuf, saved.bones[i]);
            }
            break;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_CaptureModelCache(preserveLowerBody: bool) {
    s_localCacheSeated = false;
    let count = VRIK_FKCount().min(MAXB);
    // The completed player FK: this hand packet's final right shoulder, elbow and wrist for the rifle elbow fixture (the
    // renderer looks them up by each eye's pose id).
    if cvr_host::rifle_elbow::mode() != 0 {
        let bones = [g_VRRightUpperArmIdx, g_VRRightForeArmIdx, g_VRRightBoneIdx];
        let joints = bones.iter().all(|&b| b >= 0 && b < count).then(|| {
            let mut j = [0.0f32; 9];
            for (i, &b) in bones.iter().enumerate() {
                j[3 * i..3 * i + 3].copy_from_slice(&g_fkPos[b as usize]);
            }
            j
        });
        cvr_host::rifle_elbow::complete(g_handsStableSeq, joints);
    }
    g_solveCacheN = 0;
    g_solveCacheModel = true;
    s_solveCacheCapturedMs = super::gestures::now_ms();
    s_modelCacheOwned = [false; MAX_BONES];
    for bone in 0..count {
        let bu = bone as usize;
        let leg = s_bendLegShadowSource[bu];
        let shadow = g_VRShadowSource[bu];
        if preserveLowerBody
            && (bone == g_VRHipsIdx
                || shadow == g_VRHipsIdx
                || (shadow >= 0 && !g_VRUpperOwned[shadow as usize])
                || (!g_VRUpperOwned[bu] && shadow < 0))
        {
            continue;
        }
        if !g_VRUpperOwned[bu] && shadow < 0 && !s_bendLegOwned[bu] && !(leg >= 0 && s_bendLegOwned[leg as usize]) {
            continue;
        }
        s_modelCacheOwned[bu] = true;
        s_modelCachePos[bu] = g_fkPos[bu];
        s_modelCacheRot[bu] = g_fkRot[bu];
        let n = g_solveCacheN as usize;
        g_solveCacheIdx[n] = bone;
        g_solveCacheVal[n][..3].copy_from_slice(&g_fkPos[bu]);
        g_solveCacheVal[n][3..7].copy_from_slice(&g_fkRot[bu]);
        g_solveCacheN += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_CaptureLocalArmCache(boneBuf: *mut u8, right: bool, left: bool, seatedUpper: bool) {
    g_solveCacheN = 0;
    g_solveCacheModel = false;
    s_localCacheSeated = seatedUpper;
    s_solveCacheCapturedMs = super::gestures::now_ms();
    let count = g_VRBoneCount.min(MAXB);
    let push = |bone: i32| {
        if boneBuf.is_null() || bone < 0 || bone >= count || g_solveCacheN >= 96 {
            return;
        }
        for i in 0..g_solveCacheN as usize {
            if g_solveCacheIdx[i] == bone {
                return;
            }
        }
        let n = g_solveCacheN as usize;
        g_solveCacheIdx[n] = bone;
        g_solveCacheVal[n][..3].copy_from_slice(&a3(bt(boneBuf, bone)));
        g_solveCacheVal[n][3..7].copy_from_slice(&a4(br(boneBuf, bone)));
        g_solveCacheN += 1;
    };
    let arm = |upper: i32, fore: i32, hand: i32, twist: [i32; 3]| {
        if upper >= 0 && upper < count {
            push(g_VRBoneParent[upper as usize] as i32);
        }
        push(upper);
        push(fore);
        push(hand);
        for t in twist {
            push(t);
        }
    };
    if right {
        arm(g_VRRightUpperArmIdx, g_VRRightForeArmIdx, g_VRRightBoneIdx, g_VRForeTwistR);
    }
    if left {
        arm(g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx, g_VRLeftBoneIdx, g_VRForeTwistL);
    }
    // ...and the spine the arms hang from, while a grip has turned it.
    if g_VRIKTorsoOn {
        for bone in g_VRIKTorsoBones {
            push(bone);
        }
    }
    if seatedUpper {
        let mut bones = [0i32; 64];
        let n = seated_bones(&mut bones);
        for &bone in &bones[..n] {
            push(bone);
        }
        // the native pose was saved before the spine moved; the arm IK and the shadows are done, so the complete
        // owned output is recorded now
        for i in 0..s_seatedRestore.len() {
            if s_seatedRestore[i].buffer == boneBuf && s_seatedRestore[i].revision == s_rigRevision {
                note_seated_pose(&mut s_seatedRestore[i], boneBuf);
                break;
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_ReplaySolveCache(boneBuf: *mut u8) {
    if g_solveCacheModel {
        VRIK_ReplayModelCache(boneBuf);
        return;
    }
    let seated = if s_localCacheSeated && g_solveCacheN > 0 { save_seated_pose(boneBuf) } else { core::ptr::null_mut() };
    for i in 0..g_solveCacheN.max(0) as usize {
        let bone = g_solveCacheIdx[i];
        if bone < 0 || bone >= g_VRBoneCount.min(MAXB) {
            continue;
        }
        let v = g_solveCacheVal[i];
        put3(bt(boneBuf, bone), &[v[0], v[1], v[2]]);
        put4(br(boneBuf, bone), &[v[3], v[4], v[5], v[6]]);
    }
    note_seated_pose(seated, boneBuf);
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_RestoreMissingCamera(boneBuf: *mut u8, status: i32, tick: u32, nowMs: u64) -> bool {
    if status == CAMERA_UNAVAILABLE && g_solveCacheN > 0 && nowMs >= s_solveCacheCapturedMs && nowMs - s_solveCacheCapturedMs <= 250 {
        VRIK_ReplaySolveCache(boneBuf);
        g_solveCacheTick = tick;
        return true;
    }
    g_solveCacheN = 0;
    g_solveCacheModel = false;
    g_solveCacheTick = 0xFFFF_FFFF;
    false
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_ReplayModelCache(boneBuf: *mut u8) {
    let count = VRIK_FKCount().min(MAXB);
    let vehicle_lower = if g_VRIKVehicleUpperActive { save_vehicle_lower(boneBuf) } else { core::ptr::null_mut() };
    for bone in 0..count {
        let bu = bone as usize;
        let parent = g_VRBoneParent[bu] as i32;
        let have_parent = parent >= 0 && parent < bone;
        let ppos = if have_parent { g_fkPos[parent as usize] } else { ZERO3 };
        let prot = if have_parent { g_fkRot[parent as usize] } else { ID };
        if s_modelCacheOwned[bu] {
            write_local_pos(boneBuf, bone, &ppos, &prot, &s_modelCachePos[bu]);
            write_local_rot(boneBuf, bone, &prot, &s_modelCacheRot[bu]);
            g_fkPos[bu] = s_modelCachePos[bu];
            g_fkRot[bu] = s_modelCacheRot[bu];
        } else {
            let p = a3(bt(boneBuf, bone));
            let q = a4(br(boneBuf, bone));
            let mut r = quat_mul(&prot, &q);
            quat_norm(&mut r);
            g_fkRot[bu] = r;
            let mut t = quat_rotate_vec(&prot, &p);
            for k in 0..3 {
                t[k] += ppos[k];
            }
            g_fkPos[bu] = t;
        }
    }
    if !vehicle_lower.is_null() {
        let v = &mut *vehicle_lower;
        for i in 0..v.count as usize {
            copy_bone(&mut v.solved[i], boneBuf, v.bones[i]);
        }
    }
    VRIK_PublishBodyBones();
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_PublishBodyBones() {
    let count = g_VRSpineCount.clamp(0, 8);
    let chest = if count != 0 { g_VRSpineIdx[(count - 1) as usize] } else { -1 };
    let mid = if count > 1 { g_VRSpineIdx[(count / 2) as usize] } else { chest };
    let bones = [
        g_VRHipsIdx,
        mid,
        chest,
        g_VRNeckIdx,
        g_VRHeadBoneIdx,
        g_VRLeftUpLegIdx,
        g_VRLeftLegIdx,
        g_VRLeftFootIdx,
        g_VRRightUpLegIdx,
        g_VRRightLegIdx,
        g_VRRightFootIdx,
    ];
    for slot in 0..11 {
        let bone = bones[slot];
        let ok = bone >= 0 && bone < VRIK_FKCount();
        g_VRBodyBoneOk[slot] = ok as i32;
        if ok {
            g_VRBodyBone[slot] = g_fkPos[bone as usize];
        }
    }
    for side in 0..2 {
        let toe = s_toeBone[side];
        if toe >= 0 && toe < VRIK_FKCount() {
            CyberpunkVR_DebugToes[side] = g_fkPos[toe as usize];
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_DampenTorsoWeaponPose(boneBuf: *mut u8) {
    // Only the anatomical spine here: identity is not a reference basis.
    let mut i = 0;
    while i < g_VRSpineCount && i < 8 {
        let bone = g_VRSpineIdx[i as usize];
        i += 1;
        if bone < 0 || bone >= g_VRBoneCount || !s_referenceValid[bone as usize] {
            continue;
        }
        put4(br(boneBuf, bone), &s_referenceLocalRot[bone as usize]);
    }
}

/// Animation translations are not rest geometry: the rig reference before calibration and IK.
#[no_mangle]
pub unsafe extern "C" fn VRIK_PinGirdleTranslations(boneBuf: *mut u8) {
    let clavicle = |upper: i32| if (0..MAXB).contains(&upper) { g_VRBoneParent[upper as usize] as i32 } else { -1 };
    let idx = [
        clavicle(g_VRRightUpperArmIdx),
        g_VRRightUpperArmIdx,
        g_VRRightForeArmIdx,
        g_VRRightBoneIdx,
        clavicle(g_VRLeftUpperArmIdx),
        g_VRLeftUpperArmIdx,
        g_VRLeftForeArmIdx,
        g_VRLeftBoneIdx,
    ];
    for i in idx {
        if i < 0 || i >= g_VRBoneCount || !s_referenceValid[i as usize] {
            continue;
        }
        put3(bt(boneBuf, i), &s_referenceLocalPos[i as usize]);
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRIK_ResetShoulderReference(boneBuf: *mut u8, upperIdx: i32) {
    if upperIdx < 0 || upperIdx >= g_VRBoneCount {
        return;
    }
    let clavicle = g_VRBoneParent[upperIdx as usize] as i32;
    if clavicle < 0 || clavicle >= upperIdx || !s_referenceValid[clavicle as usize] || !s_referenceValid[upperIdx as usize] {
        return;
    }
    put3(bt(boneBuf, clavicle), &s_referenceLocalPos[clavicle as usize]);
    put4(br(boneBuf, clavicle), &s_referenceLocalRot[clavicle as usize]);
    put3(bt(boneBuf, upperIdx), &s_referenceLocalPos[upperIdx as usize]);
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
}

/// The body's axes from the pelvis (or, for a passenger's twisted trunk, the chest) relative to its rig basis.
#[no_mangle]
pub unsafe extern "C" fn VRIK_BodyAxesFromRig(bodyRight: *mut f32, bodyUp: *mut f32, bodyFwd: *mut f32, upperTorso: bool) -> bool {
    let basis = if upperTorso && g_VRSpineCount > 0 { g_VRSpineIdx[(g_VRSpineCount - 1) as usize] } else { g_VRHipsIdx };
    if basis < 0 || basis >= VRIK_FKCount() || !s_referenceValid[basis as usize] {
        return false;
    }
    let inverse = quat_conj(&s_referenceModelRot[basis as usize]);
    let mut body = quat_mul(&g_fkRot[basis as usize], &inverse);
    let mut norm = 0.0f32;
    for q in body {
        if !q.is_finite() {
            return false;
        }
        norm += q * q;
    }
    if norm < 1e-8 {
        return false;
    }
    quat_norm(&mut body);
    put3(bodyRight, &quat_rotate_vec(&body, &[1.0, 0.0, 0.0]));
    put3(bodyUp, &quat_rotate_vec(&body, &[0.0, 0.0, 1.0]));
    put3(bodyFwd, &quat_rotate_vec(&body, &[0.0, 1.0, 0.0]));
    true
}

/// A model-space point carried by this solve's thorax turn: each rotation about its own pivot, lower bone first.
pub unsafe fn torso_carry(p: &mut V3) {
    for i in 0..2 {
        let r = [p[0] - g_VRIKTorsoP[i][0], p[1] - g_VRIKTorsoP[i][1], p[2] - g_VRIKTorsoP[i][2]];
        let o = quat_rotate_vec(&g_VRIKTorsoQ[i], &r);
        for k in 0..3 {
            p[k] = g_VRIKTorsoP[i][k] + o[k];
        }
    }
}
