//! THE POSE HOOK'S WORK (formerly src/Hooks/AnimPoseBody.cpp; the hook itself, its SEH frame and the mirror double
//! stay in src/Hooks/AnimPose.cpp, which calls cvr_anim_pose_apply inside its __try/__finally). On every skeleton's
//! pose apply: the weapon rig's passes; on the player's: the native cyberware's own graphs, the scene suspends, the
//! two-hand capture, head aim and the ADS layers, the finger layers in their order of priority, the camera bones'
//! freeze, and VRIK -- one fresh solve per animation batch, bit-exact replays for the rest of it.
//!
//! A FAULT IN HERE (an engine pointer gone bad) unwinds to the hook's __except: Rust frames carry synchronous-only EH
//! tables, so the unwind passes them without running anything, as it passed the C++ frames compiled /EHsc.
use super::cyberware::VRIK_ApplyCyberwarePose;
use super::env::*;
use super::gestures::{cvr_gestures_apply, cvr_gestures_apply_gloves, now_ms};
use super::grasp::cvr_anim_grasp_fingers;
use super::recoil::{RecoilSample, RecoilTick, RecoilTravelMix};
use super::reload::{cvr_anim_carry_grip_pose_right, cvr_anim_reload_finger_pose, cvr_anim_rest_finger_pose,
                    cvr_anim_rest_finger_pose_right};
use super::reload_handoff::{clock_qpc, clock_qpc_frequency, cvr_anim_reload_hand_off_blend_target,
                            cvr_anim_reload_hand_off_capture, cvr_anim_reload_hand_off_update,
                            cvr_anim_reload_hand_off_weight};
use super::smoking::{cvr_anim_smoking_cig_pose, cvr_anim_smoking_lighter_pose};
use super::snapshot::{RefreshHandsSnapshot, SharedPose};
use super::two_hand::{apply_firearm_support_stop, cvr_anim_two_hand_capture, cvr_anim_two_hand_fingers, cvr_anim_two_hand_left,
                      left_firearm_drawn, rebase_firearm_support, solve_weapon_hand_pose, two_hand_weapon_hand_moved,
                      CarryLeftBlend, CarryRecordLeft, CarryReturnRight};
use super::weapon_arm::{cvr_anim_apply_head_aim_weapon_orientation, cvr_anim_apply_non_vrik_ads_muzzle_stabilizer,
                        cvr_anim_is_head_aim_weapon_active, cvr_anim_note_finger_flex, cvr_anim_prepare_aim_arm_targets,
                        cvr_anim_publish_animated_fk, cvr_anim_solve_prepared_aim_arms, cvr_anim_weapon_in_left_hand};
use super::weapon_rig::{cvr_anim_weapon_rig_capture_parts, cvr_anim_weapon_rig_census_note,
                        cvr_anim_weapon_rig_identify_and_write};
use super::wheel_grab::{cvr_anim_wheel_blend_target, cvr_anim_wheel_capture_anim, cvr_anim_wheel_fingers,
                        cvr_anim_wheel_hands_off, cvr_anim_wheel_maintain_grab, cvr_anim_wheel_publish_grab,
                        cvr_anim_wheel_store_target, cvr_anim_wheel_update};
use super::*;
use crate::math::{quat_conj, quat_mul, quat_norm, quat_rotate_vec};
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use cvr_body::cmath::{acos, atan2, cos, sin, sqrt};
use cvr_body::policy::{is_controller_vrik_enabled, is_passenger_window_combat, is_seated_vehicle_state, should_hand_left_arm_to_body_carry,
                       should_hand_right_arm_to_body_carry, should_suspend_vrik_for_body_carry,
                       should_suspend_vrik_for_finisher, should_suspend_vrik_for_takedown, should_suspend_vrik_in};
use cvr_camera::anchor::{body_orbit_model, AnchorRecipe, AnchorVector};
use cvr_camera::frames::build_hand_view_frame;
use cvr_body::xr::Quat;

const PLAYER_BASE_GRAPH: u64 = 0x7c8e_f3ac_7af9_5509;
const K_WEAPON_FLAG: usize = 144;
const CAMERA_INVALIDATED: i32 = 2;

extern "C" {
    static mut g_AnimPoseTotalCalls: u64;
    static mut g_AnimPoseMatchCalls: u64;
    static mut g_AnimPoseLastBoneBuf: usize;
    static mut g_PlayerTrackBufA: usize;
    static mut g_PlayerTrackBufB: usize;
    static mut g_WeaponTrackBufA: usize;
    static mut g_WeaponTrackBufB: usize;
    static mut g_WeaponRigActive: i32;
    static mut g_PoseCensusOn: i32;
    static mut g_VRRecordFK: i32;
    static mut g_VRSmokeFingerActive: i32;
    static mut g_VRSmokeFingerActiveL: i32;
    static mut g_VRSmokeFingerCapture: i32;
    static mut g_VRSmokeFingerCaptureL: i32;
    static mut g_VRSmokeFingerCount: i32;
    static mut g_VRSmokeFingerCountL: i32;
    static mut g_VRSmokeFingerIdx: [i32; 32];
    static mut g_VRSmokeFingerIdxL: [i32; 32];
    static mut g_VRSmokeCigIdx: i32;
    static mut g_VRSmokeLighterIdx: i32;
    static mut g_VRSmokeMouthBoneIdx: i32;
    static mut g_VRSmokeMouthAnchor: i32;
    static mut g_VRSmokeMouthDist: f32;
    static mut g_VRSmokeMouthDistL: f32;
    static mut g_VRSmokeMouthPos: [f32; 3];
    static mut g_VRSmokeMouthRot: [f32; 4];
    static mut g_VRSmokeAnchorLocalPos: [f32; 3];
    static mut g_VRSmokeAnchorLocalRot: [f32; 4];
    static mut g_VRSmokeAnchorValid: i32;
    static mut g_VRSmokeAnchorBoneSel: i32;
    static mut g_VRSmokeAnchorBoneIdx: i32;
    static mut g_VRSmokeAltAnchorLocalPos: [f32; 3];
    static mut g_VRSmokeAltAnchorLocalRot: [f32; 4];
    static mut g_VRSmokeAltAnchorValid: i32;
    static mut g_VRSmokeSmokePos: [f32; 3];
    static mut g_VRSmokeSmokeRot: [f32; 4];
    static mut g_VRSmokeMouthWorldPos: [f32; 3];
    static mut g_VRSmokeMouthWorldRot: [f32; 4];
    static mut g_VRSmokeMouthWorldValid: i32;
    static mut g_VRViewWorldPos: [f32; 3];
    static mut g_VRViewWorldRot: [f32; 4];
    static mut g_VRViewWorldValid: i32;
    static mut g_VRReloadFingerActive: [i32; 2];
    static mut g_VRTakedownState: i32;
    static mut g_VRBodyCarryingState: i32;
    static mut g_VRBodyDisposalState: i32;
    static mut g_VRBodyCarryBothHands: i32;
    static mut g_VRFinisherActive: i32;
    static mut g_VRFinisherSinceMs: u64;
    static mut g_VRBodyUnderHMD: i32;
    static mut g_VRCamBoneFreeze: i32;
    static mut g_VRFppCamIdx: [i32; 5];
    static mut g_VRHeadDrop: f32;
    static mut g_VRCamModelPos: [f32; 3];
    static mut g_VRCamModelRot: [f32; 4];
    static mut g_VRPalmModelR: [f32; 3];
    static mut g_VRPalmModelL: [f32; 3];
    static mut g_VRPalmModelRotR: [f32; 4];
    static mut g_VRPalmModelRotL: [f32; 4];
    static mut g_VRPalmModelValid: i32;
    static mut g_VRWristR_I: f32;
    static mut g_VRWristR_J: f32;
    static mut g_VRWristR_K: f32;
    static mut g_VRWristR_R: f32;
    static mut g_VRWristL_I: f32;
    static mut g_VRWristL_J: f32;
    static mut g_VRWristL_K: f32;
    static mut g_VRWristL_R: f32;
    static mut g_VRScaleR: f32;
    static mut g_VRScaleL: f32;
    static mut g_VROffRX: f32;
    static mut g_VROffRY: f32;
    static mut g_VROffRZ: f32;
    static mut g_VROffLX: f32;
    static mut g_VROffLY: f32;
    static mut g_VROffLZ: f32;
    static mut g_VRShoulderRX: f32;
    static mut g_VRShoulderRY: f32;
    static mut g_VRShoulderRZ: f32;
    static mut g_VRShoulderLX: f32;
    static mut g_VRShoulderLY: f32;
    static mut g_VRShoulderLZ: f32;
    static mut g_VRElbowPoleR: f32;
    static mut g_VRElbowPoleL: f32;
    static mut g_VRElbowSwingR: f32;
    static mut g_VRElbowSwingL: f32;
    static mut g_VRIKDbgHandFK: [f32; 3];
    static mut g_VRIKDbgTargetTrace: [f32; 3];
    static mut g_VRIKFreshTotal: i32;
    static mut g_VRIKReplayTotal: i32;
    static mut g_solveCacheSnapCtr: f32;
    static mut g_solveCacheYaw: f32;
    static g_VrikFrameEpoch: AtomicU32;
    static mut g_VrikBatchGapNew: i32;
    static mut g_VrikBatchGapSame: i32;
    static mut g_VrikBatchGapNewMin: f32;
    static mut g_VrikBatchGapSameMax: f32;
    static mut g_VrikEngineOverwriteMm: f32;
    static mut g_VrikEngineOverwriteBone: i32;
    static mut g_VrikEngineOverwriteHits: i32;
    static mut g_VrikEngineOverwritePasses: i32;
    static mut g_VrikOverwritePassHist: [i32; 8];
    static mut CyberpunkVR_NonVrikAdsStabilizer: i32;
    static mut CyberpunkVR_TwoHandCaptureReq: i32;
    static mut CyberpunkVR_VrikTakedownSuspend: i32;
    static mut CyberpunkVR_VrikBodyCarryHandOff: i32;
    static mut CyberpunkVR_VrikFinisherSuspend: i32;
    static mut CyberpunkVR_BodyOrbitRestNeck: i32;
    static mut CyberpunkVR_DebugRestNeck: [f32; 2];
    static mut CyberpunkVR_VrikBatchClock: i32;
    static mut CyberpunkVR_VrikBatchGapMs: f32;
    static mut CyberpunkVR_XrDeepDiag: i32;
    static mut CyberpunkVR_VrikSplitHeadHandRot: i32;
    static mut CyberpunkVR_VrikArmAnchorFromBody: i32;
    static mut CyberpunkVR_VrikHandFrameOneLatch: i32;
    static mut CyberpunkVR_VrikHandFrameAlign: i32;
    static mut CyberpunkVR_BodyYawFinalValid: i32;
    static mut CyberpunkVR_BodyYawFinalRad: f32;
    static mut CyberpunkVR_DebugHandMissMm: f32;

    // The C++ the work calls (src/Anim/CharacterRigRust.cpp).
    fn cvr_cpp_pose_lock();
    fn cvr_cpp_pose_device_cam_active() -> bool;
    fn cvr_cpp_pose_scene_tier() -> i32;
    fn cvr_cpp_pose_vehicle_state() -> i32;
    /// g_liveControls: xrVehicleCutsceneSuspendTier, xrLadderGripClimb, xrLeftHanded.
    fn cvr_cpp_pose_effective_cutscene_tier() -> i32;
    fn cvr_cpp_pose_id_note(sameInput: bool, poseId: u64);
    fn cvr_cpp_ladder_active() -> bool;
    fn cvr_cpp_ladder_read_hand_kinds(kinds: *mut i32);
    fn cvr_cpp_ladder_constrain_hand(side: i32, target: *mut f32, rot: *mut f32, entityPos: *const f32,
                                     entityRot: *const f32);
    fn cvr_cpp_lobby_world_shown() -> bool;
    fn cvr_cpp_fbt_calibrating() -> bool;
    fn cvr_cpp_fbt_solve_arm(boneBuf: *mut u8, side: i32, target: *const f32, rot: *const f32) -> bool;
    fn cvr_cpp_fbt_solve_lower_body(boneBuf: *mut u8, view: *const f32, rot: *const f32);
    fn cvr_cpp_swimming_active() -> bool;
    fn cvr_cpp_roomscale_camera_consumed(out: *mut f32);
    fn cvr_cpp_roomscale_camera_rest_neck(out: *mut f32) -> bool;
    fn cvr_cpp_body_yaw_follow_active() -> bool;
}

// The solve's own exports this file calls (rust/crates/cvr-rig/src/vrik).
use super::arm::{VRIK_ApplyHandStop, VRIK_BuildHandTarget, VRIK_RestoreArmRestTrans, VRIK_ScaleArmBonesFromRest,
                 VRIK_SolveArm};
use super::body::{VRIK_ApplyLadderFingers, VRIK_BeginBodySolve, VRIK_NoteShake, VRIK_PlaceBodyUnderHMD,
                  VRIK_PlaceLadderUpper, VRIK_PlaceSeatedUpper, VRIK_PlaceSwimmingBody, VRIK_PlaceVehicleCombatUpper, VRIK_TwistAbout};
use super::cache::{VRIK_AlignViewToHands, VRIK_BodyAxesFromRig, VRIK_BuildViewHandTarget, VRIK_CaptureLocalArmCache,
                   VRIK_CaptureModelCache, VRIK_DampenTorsoWeaponPose, VRIK_PinGirdleTranslations,
                   VRIK_ReplaySolveCache, VRIK_RestoreMissingCamera, VRIK_SyncShadowUpper,
                   VRIK_WorldToModelRotation};
use super::camera::{VRIK_BodyAxesFromCamYaw, VRIK_ComputeCamModel};
use super::rig::{VRIK_ComputeFK, VRIK_Cross3, VRIK_FKCount, VRIK_HoldReferencePose, VRIK_Norm3, VRIK_NoteHandAim,
                 VRIK_PublishNativeCyberwarePose, VRIK_UsesNativeCyberware, VRIK_WriteLocalPos};
use super::shoulder::{VRIK_AnchorShoulder, VRIK_GripTorso};
use super::snapshot::{VRIK_HeadResidual, VRIK_LatchViewPacket, VRIK_ResolveViewModelPos, VRIK_ResolveViewPos};

extern "C" {
    fn VRIK_ReadCurrentBodyFrame(worldPosition: *mut f32, worldRotation: *mut f32, trackingYaw: *mut f32) -> bool;
}

// ---- the work's own statics
static mut PREV_HEAD_AIM: bool = false;
static mut PREVIOUS_POSE_POLICY: i32 = -1;
static mut CAM_REST: [[f32; 7]; 5] = [[0.0; 7]; 5];
static mut CAM_CAP: [bool; 5] = [false; 5];
static mut LAST_PASS_MS: f64 = 0.0;
static mut BATCH_SEQ: u32 = 0;
static mut PASS_IN_BATCH: i32 = 0;
static mut WHEEL_LAST_QPC: i64 = 0;
static mut HEAD_OFF_B: V3 = [0.0; 3];
static mut HEAD_OFF_N: i32 = 0;
static mut SHAKE_HIST: [[[f32; 2]; 3]; 5] = [[[0.0; 2]; 3]; 5];
static mut SHAKE_N: i32 = 0;

#[inline(always)]
unsafe fn sp(i: i32) -> f32 {
    SharedPose(i)
}

unsafe fn finisher_owns_body() -> bool {
    should_suspend_vrik_for_finisher(g_VRFinisherActive, now_ms().wrapping_sub(g_VRFinisherSinceMs),
        CyberpunkVR_VrikFinisherSuspend)
}

/// The pose path's millisecond clock: the hands' counter (the tests' clock, or QPC).
unsafe fn vrik_now_ms() -> f64 {
    let f = clock_qpc_frequency();
    if f == 0.0 {
        return 0.0;
    }
    clock_qpc() as f64 * 1000.0 / f
}

/// THE GAP HISTOGRAM: passes inside one batch arrive microseconds apart, batches a frame apart.
unsafe fn note_batch_gap(gap_ms: f64, new_batch: bool) {
    if gap_ms < 0.0 || gap_ms > 1000.0 {
        return;
    }
    if new_batch {
        g_VrikBatchGapNew += 1;
        if gap_ms < g_VrikBatchGapNewMin as f64 || g_VrikBatchGapNewMin <= 0.0 {
            g_VrikBatchGapNewMin = gap_ms as f32;
        }
    } else {
        g_VrikBatchGapSame += 1;
        if gap_ms > g_VrikBatchGapSameMax as f64 {
            g_VrikBatchGapSameMax = gap_ms as f32;
        }
    }
}

fn player_pose_fits(slots: u32, bones: i32, fk_count: i32) -> bool {
    bones > 0 && bones <= MAXB && slots >= bones as u32 && fk_count >= 0 && fk_count <= bones
}

#[inline(always)]
unsafe fn rd<T: Copy>(p: *const u8, off: usize) -> T {
    (p.add(off) as *const T).read()
}

fn qrot(q: &Q, v: &V3) -> V3 {
    quat_rotate_vec(q, v)
}
fn qmul_n(a: &Q, b: &Q) -> Q {
    let mut o = quat_mul(a, b);
    quat_norm(&mut o);
    o
}


/// The work of one pose apply; `locked` reports whether g_PlayerPoseStateLock was taken (the hook releases it).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_pose_apply(a1: *mut c_void, a2: *mut c_void, a4: u32, locked: *mut bool) {
    g_AnimPoseTotalCalls = g_AnimPoseTotalCalls.wrapping_add(1);
    if !(g_PlayerTrackBufA != 0 || g_PlayerTrackBufB != 0) {
        return;
    }
    let head_aim_work = cvr_anim_is_head_aim_weapon_active();
    let non_vrik_ads_work = !g_pSharedHands.is_null()
        && CyberpunkVR_NonVrikAdsStabilizer != 0
        && *g_pSharedHands.add(K_WEAPON_FLAG) > 0.5;
    if g_VRBind <= 0
        && !head_aim_work
        && !non_vrik_ads_work
        && g_VRDiagCapture == 0
        && g_WeaponRigActive == 0
        && g_PoseCensusOn == 0
        && g_VRRecordFK == 0
        && CyberpunkVR_TwoHandCaptureReq == 0
        && g_VRSmokeFingerActive == 0
        && g_VRSmokeFingerCapture == 0
        && g_VRSmokeFingerActiveL == 0
        && g_VRSmokeFingerCaptureL == 0
    {
        return;
    }

    let pose_desc = *(a2 as *mut *mut u8).add(7);
    if pose_desc.is_null() {
        return;
    }
    let bone_buf = *(pose_desc as *mut *mut u8);
    let track_buf = *(pose_desc as *mut usize).add(3);
    let player_pass = !bone_buf.is_null() && track_buf != 0 && (track_buf == g_PlayerTrackBufA || track_buf == g_PlayerTrackBufB);
    if player_pass {
        cvr_cpp_pose_lock();
        *locked = true;
        // A rebind may finish while this job waits for the shared solve state: recheck.
        let slots = rd::<u32>(pose_desc, 8);
        if (track_buf != g_PlayerTrackBufA && track_buf != g_PlayerTrackBufB)
            || !player_pose_fits(slots, g_VRBoneCount, g_VRFKCount)
        {
            return;
        }
        if head_aim_work != PREV_HEAD_AIM {
            g_solveCacheTick = 0xFFFF_FFFF;
            g_solveCacheN = 0;
            PREV_HEAD_AIM = head_aim_work;
        }
    }
    // IDENTIFY BY BONE NAME (a1[8] is the rig object).
    if !bone_buf.is_null() && track_buf != 0 && !player_pass {
        cvr_anim_weapon_rig_identify_and_write(a1 as *mut *mut u8, a2 as *mut *mut u8, a4, pose_desc, bone_buf, track_buf);
        // The Osprey's screen hinge (optic_hinge.rs), after the rig lock is released: only in the pass of the requested
        // object's own update.
        let (object, descriptor) = super::anim_pose_hook::object_scope();
        super::optic_hinge::apply(object, descriptor, a1 as *mut *mut u8, a2 as *mut *mut u8, a4, pose_desc, bone_buf, track_buf);
    }
    // CENSUS -- the abandoned statistical route, left gated off.
    if g_PoseCensusOn != 0 && !bone_buf.is_null() && track_buf != 0 && !player_pass {
        cvr_anim_weapon_rig_census_note(a4, track_buf);
    }
    // THE WEAPON'S PASS.
    if g_WeaponRigActive != 0 && !bone_buf.is_null() && track_buf != 0
        && (track_buf == g_WeaponTrackBufA || track_buf == g_WeaponTrackBufB)
    {
        cvr_anim_weapon_rig_capture_parts(bone_buf);
    }
    if !player_pass {
        return;
    }
    player_pass_work(a1 as *mut u8, pose_desc, bone_buf, track_buf, head_aim_work, non_vrik_ads_work);
}

unsafe fn controls() -> [i32; 3] {
    let mut c = [0i32; 3];
    super::live_reads::pose_controls(c.as_mut_ptr());
    c
}

#[allow(clippy::cognitive_complexity)]
unsafe fn player_pass_work(a1: *mut u8, pose_desc: *mut u8, bone_buf: *mut u8, track_buf: usize, head_aim_work: bool,
                           non_vrik_ads_work: bool) {
    g_AnimPoseMatchCalls = g_AnimPoseMatchCalls.wrapping_add(1);
    g_AnimPoseLastBoneBuf = bone_buf as usize;
    let track_count = rd::<u32>(pose_desc, 0x20);
    if VRIK_UsesNativeCyberware() {
        // a1+0x20 is the graph resource; CResource+0x30 its path hash.
        let graph = rd::<usize>(a1, 0x20);
        let path: u64 = if graph != 0 { rd::<u64>(graph as *const u8, 0x30) } else { 0 };
        if VRIK_UsesNativeCyberware() && path != PLAYER_BASE_GRAPH {
            let mounted = sp(31) > 0.5;
            let c = controls();
            let suspended = should_suspend_vrik_in(cvr_cpp_pose_scene_tier(), mounted, cvr_cpp_pose_effective_cutscene_tier(), c[0])
                || should_suspend_vrik_for_takedown(g_VRTakedownState, CyberpunkVR_VrikTakedownSuspend)
                || should_suspend_vrik_for_body_carry(g_VRBodyCarryingState, g_VRBodyDisposalState, g_VRLocomotionState,
                    CyberpunkVR_VrikBodyCarryHandOff)
                || finisher_owns_body();
            if g_solveCacheModel && g_solveCacheN > 0 && !head_aim_work && !suspended && !cvr_cpp_pose_device_cam_active()
                && !cvr_cpp_ladder_active()
            {
                VRIK_ApplyCyberwarePose(bone_buf, track_buf as *mut f32, track_count);
            }
            VRIK_PublishNativeCyberwarePose(bone_buf, path);
            return;
        }
    }
    // The recorder publishes the engine's ANIMATED bones (VRIK off) so reload poses can be authored.
    if g_VRRecordFK != 0 {
        cvr_anim_publish_animated_fk(bone_buf);
    }
    // SEQLOCK: one consistent pose frame for this whole solve.
    RefreshHandsSnapshot();
    let pose_mounted = sp(31) > 0.5;
    let pose_vehicle_state = cvr_cpp_pose_vehicle_state();
    let pose_window_combat = pose_mounted && is_passenger_window_combat(pose_vehicle_state);
    let pose_seated = pose_mounted && is_seated_vehicle_state(pose_vehicle_state);
    let pose_ladder = !pose_mounted && cvr_cpp_ladder_active();
    let c = controls();
    let ladder_vrik = pose_ladder && is_controller_vrik_enabled(g_VRBind, head_aim_work) && c[1] != 0;
    let mut ladder_kinds = [0i32; 2];
    if ladder_vrik {
        cvr_cpp_ladder_read_hand_kinds(ladder_kinds.as_mut_ptr());
    }
    let pose_paired_animation = should_suspend_vrik_for_takedown(g_VRTakedownState, CyberpunkVR_VrikTakedownSuspend)
        || should_suspend_vrik_for_body_carry(g_VRBodyCarryingState, g_VRBodyDisposalState, g_VRLocomotionState,
            CyberpunkVR_VrikBodyCarryHandOff)
        || finisher_owns_body();
    let carry_left_arm = !pose_mounted && should_hand_left_arm_to_body_carry(g_VRBodyCarryingState, CyberpunkVR_VrikBodyCarryHandOff);
    let carry_right_arm = !pose_mounted
        && should_hand_right_arm_to_body_carry(g_VRBodyCarryingState, g_VRBodyCarryBothHands, CyberpunkVR_VrikBodyCarryHandOff);
    // The paired-animation bits make leaving a grapple or a carry drop the solve cache.
    let pose_policy = (if pose_ladder {
        3 + 4 * ladder_kinds[0] + 16 * ladder_kinds[1]
    } else if pose_window_combat {
        2
    } else if pose_seated {
        64
    } else if pose_mounted {
        1
    } else {
        0
    }) + if pose_paired_animation { 1024 } else { 0 }
        + if carry_left_arm { 2048 } else { 0 }
        + if carry_right_arm { 4096 } else { 0 };
    if PREVIOUS_POSE_POLICY != pose_policy {
        g_solveCacheN = 0;
        g_solveCacheModel = false;
        g_solveCacheTick = 0xFFFF_FFFF;
        PREVIOUS_POSE_POLICY = pose_policy;
    }
    // CUTSCENE FULL-SUSPEND, a surveillance camera, a paired animation: the engine's own pose stands.
    {
        let tier = cvr_cpp_pose_scene_tier();
        let suspend = should_suspend_vrik_in(tier, pose_mounted, cvr_cpp_pose_effective_cutscene_tier(), c[0]);
        if suspend || cvr_cpp_pose_device_cam_active() || pose_paired_animation {
            return;
        }
    }
    // THE TWO-HAND CAPTURE READS FIRST, BEFORE ANY LAYER OF OURS WRITES.
    cvr_anim_two_hand_capture(bone_buf);
    // HEAD AIM: the view packet latched even when VRIK is not solving.
    let ads_eye_alignment_active = head_aim_work || non_vrik_ads_work;
    if ads_eye_alignment_active {
        VRIK_LatchViewPacket();
    }
    cvr_anim_apply_head_aim_weapon_orientation(bone_buf);
    cvr_anim_apply_non_vrik_ads_muzzle_stabilizer(bone_buf);
    cvr_anim_prepare_aim_arm_targets(bone_buf);
    cvr_anim_solve_prepared_aim_arms(bone_buf);
    cvr_anim_note_finger_flex(bone_buf, false);

    // THE FINGER LAYERS. A hand holding a carried body (or the native reload's) keeps the animation's fingers: taken
    // before any layer, put back after the last.
    let mut carried_fingers = [[[0u8; 48]; 32]; 2];
    let reload_fingers = cvr_anim_reload_hand_off_weight();
    let carried_weight = [if carry_right_arm { 1.0 } else { reload_fingers }, if carry_left_arm { 1.0 } else { reload_fingers }];
    let carried_count = [
        if carried_weight[0] > 0.0 { g_VRSmokeFingerCount.min(32) } else { 0 },
        if carried_weight[1] > 0.0 { g_VRSmokeFingerCountL.min(32) } else { 0 },
    ];
    let carried_idx = [&raw const g_VRSmokeFingerIdx, &raw const g_VRSmokeFingerIdxL];
    for h in 0..2 {
        for k in 0..carried_count[h].max(0) as usize {
            let bi = (*carried_idx[h])[k];
            if (0..MAXB).contains(&bi) {
                carried_fingers[h][k] = *bone_bytes(bone_buf, bi);
            }
        }
    }
    let lobby_hands = cvr_cpp_lobby_world_shown();
    if !VRIK_UsesNativeCyberware() {
        cvr_anim_rest_finger_pose(bone_buf, lobby_hands || (ladder_vrik && ladder_kinds[0] == 0));
    }
    if !VRIK_UsesNativeCyberware() {
        cvr_anim_rest_finger_pose_right(bone_buf, lobby_hands || (ladder_vrik && ladder_kinds[1] == 0));
    }
    if !VRIK_UsesNativeCyberware() && !ladder_vrik {
        cvr_gestures_apply(bone_buf);
    }
    if !VRIK_UsesNativeCyberware() {
        cvr_anim_carry_grip_pose_right(bone_buf);
    }
    if !VRIK_UsesNativeCyberware() {
        cvr_anim_two_hand_fingers(bone_buf);
    }
    cvr_anim_grasp_fingers(bone_buf);
    if g_VRReloadFingerActive[0] != 0 || g_VRReloadFingerActive[1] != 0 {
        cvr_anim_reload_finger_pose(bone_buf);
    }
    cvr_anim_wheel_fingers(bone_buf);
    if g_VRSmokeFingerCount > 0 || g_VRSmokeCigIdx >= 0 {
        cvr_anim_smoking_cig_pose(bone_buf);
    }
    if g_VRSmokeFingerCountL > 0 || g_VRSmokeLighterIdx >= 0 {
        cvr_anim_smoking_lighter_pose(bone_buf);
    }
    if ladder_vrik {
        for side in 0..2 {
            VRIK_ApplyLadderFingers(bone_buf, side, ladder_kinds[side as usize], core::ptr::null(), core::ptr::null());
        }
    }
    for h in 0..2 {
        for k in 0..carried_count[h].max(0) as usize {
            let bi = (*carried_idx[h])[k];
            if !(0..MAXB).contains(&bi) {
                continue;
            }
            let w = carried_weight[h];
            if w >= 0.999 {
                *bone_bytes(bone_buf, bi) = carried_fingers[h][k];
                continue;
            }
            let qp = br(bone_buf, bi);
            let q = a4(qp);
            let a = a4(carried_fingers[h][k].as_ptr().add(16) as *const f32);
            let s = if (q[0] * a[0] + q[1] * a[1] + q[2] * a[2] + q[3] * a[3]) < 0.0 { -w } else { w };
            let n = [q[0] * (1.0 - w) + a[0] * s, q[1] * (1.0 - w) + a[1] * s, q[2] * (1.0 - w) + a[2] * s,
                     q[3] * (1.0 - w) + a[3] * s];
            let len = sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2] + n[3] * n[3]);
            if len > 1e-6 {
                put4(qp, &[n[0] / len, n[1] / len, n[2] / len, n[3] / len]);
            }
        }
    }
    // GLOVES LAST.
    cvr_gestures_apply_gloves(bone_buf);
    // FINGERS ON THE WORLD, over every layer: a hand whose body stopped at a table bends its fingers on it
    // (finger_contact). Not a hand whose fingers a carried body or a reload's hand-off owns.
    {
        let busy = [carried_weight[1] > 0.0 || g_VRReloadFingerActive[0] != 0, carried_weight[0] > 0.0 || g_VRReloadFingerActive[1] != 0];
        super::finger_contact::finger_contact(bone_buf, &busy);
    }
    cvr_anim_note_finger_flex(bone_buf, true);

    camera_bone_freeze(bone_buf);

    // VRIK FULL-ARM IK (mode 4), not while head aim owns the weapon.
    if is_controller_vrik_enabled(g_VRBind, head_aim_work) && !g_pSharedHands.is_null() && g_VRBoneCount > 0
        && g_VRHeadBoneIdx >= 0
    {
        vrik_pass(bone_buf, track_buf, track_count, pose_mounted, pose_window_combat, pose_seated, pose_ladder, carry_left_arm,
                  carry_right_arm, &c);
        if RETURNED {
            RETURNED = false;
            return;
        }
    }
    // Full-body tracking calibration: V stands still in the reference pose.
    if cvr_cpp_fbt_calibrating() {
        VRIK_HoldReferencePose(bone_buf);
    }
}

/// vrik_pass returned out of the whole work (C++'s `return result` from inside the solve).
static mut RETURNED: bool = false;

/// FPP-CAMERA BONE FREEZE: the five fppCamera joints' rest captured on the first pass, their deviation measured every
/// pass, and per mode their swing (and translation) pinned to rest, the twist about the model vertical left live.
unsafe fn camera_bone_freeze(bone_buf: *mut u8) {
    let mut max_dev_deg = 0.0f32;
    let mut max_dev_bone = -1.0f32;
    for ci in 0..5usize {
        let bi = g_VRFppCamIdx[ci];
        if !(0..MAXB).contains(&bi) {
            continue;
        }
        let t = bt(bone_buf, bi);
        let q = br(bone_buf, bi);
        if !CAM_CAP[ci] {
            CAM_REST[ci] = [*t, *t.add(1), *t.add(2), *q, *q.add(1), *q.add(2), *q.add(3)];
            CAM_CAP[ci] = true;
            continue;
        }
        // Raw anim deviation from rest, BEFORE any rewrite below.
        {
            let r = &CAM_REST[ci];
            let mut d = *q * r[3] + *q.add(1) * r[4] + *q.add(2) * r[5] + *q.add(3) * r[6];
            if d < 0.0 {
                d = -d;
            }
            if d > 1.0 {
                d = 1.0;
            }
            let dev_deg = 2.0 * acos(d) * 57.295_779_5;
            if dev_deg > max_dev_deg {
                max_dev_deg = dev_deg;
                max_dev_bone = ci as f32;
            }
        }
        let aim_jnt = ci == 1;
        let freeze = g_VRCamBoneFreeze;
        let freeze_this = freeze == 4 || (aim_jnt && (freeze == 1 || freeze == 2 || freeze == 3));
        if !freeze_this {
            continue;
        }
        if aim_jnt && freeze == 2 {
            let r = CAM_REST[ci];
            *t = r[0];
            *t.add(1) = r[1];
            *t.add(2) = r[2];
            put4(q, &[r[3], r[4], r[5], r[6]]);
            continue;
        }
        // Swing-only freeze (modes 1/3 on Aim_JNT; mode 4 on all five).
        let mut rp: Q = ID;
        {
            let mut chain = [0i32; 24];
            let mut cn = 0usize;
            let mut p = g_VRBoneParent[bi as usize] as i32;
            while (0..MAXB).contains(&p) && cn < 24 {
                chain[cn] = p;
                cn += 1;
                p = g_VRBoneParent[p as usize] as i32;
            }
            for k in (0..cn).rev() {
                let pq = a4(br(bone_buf, chain[k]));
                rp = quat_mul(&rp, &pq);
            }
            quat_norm(&mut rp);
        }
        let rp_inv = quat_conj(&rp);
        let mut axis = qrot(&rp_inv, &[0.0, 0.0, 1.0]);
        let an = sqrt(axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]);
        if an > 1e-6 {
            axis[0] /= an;
            axis[1] /= an;
            axis[2] /= an;
            let mut tw_live = [0.0f32; 4];
            VRIK_TwistAbout(q, axis.as_ptr(), tw_live.as_mut_ptr());
            let r = CAM_REST[ci];
            let q_rest = [r[3], r[4], r[5], r[6]];
            let mut tw_rest = [0.0f32; 4];
            VRIK_TwistAbout(q_rest.as_ptr(), axis.as_ptr(), tw_rest.as_mut_ptr());
            let tw_rest_inv = quat_conj(&tw_rest);
            let swing_rest = quat_mul(&q_rest, &tw_rest_inv);
            let mut q_out = quat_mul(&swing_rest, &tw_live);
            quat_norm(&mut q_out);
            if aim_jnt && freeze == 1 {
                *t = r[0];
                *t.add(1) = r[1];
                *t.add(2) = r[2];
            }
            put4(q, &q_out);
        }
    }
    if !g_pSharedHands.is_null() {
        *g_pSharedHands.add(82) = max_dev_deg;
        *g_pSharedHands.add(83) = max_dev_bone;
    }
}

/// VRIK: one fresh solve per animation batch, bit-exact replays for the rest of it.
#[allow(clippy::too_many_arguments)]
unsafe fn vrik_pass(bone_buf: *mut u8, track_buf: usize, track_count: u32, pose_mounted: bool, pose_window_combat: bool,
                    pose_seated: bool, pose_ladder: bool, carry_left_arm: bool, carry_right_arm: bool, c: &[i32; 3]) {
    // THE CLOCK IS THE ANIMATION BATCH ITSELF: a gap of more than a few ms is a new batch.
    let pass_now_ms = vrik_now_ms();
    let pass_gap_ms = if LAST_PASS_MS > 0.0 { pass_now_ms - LAST_PASS_MS } else { 1000.0 };
    LAST_PASS_MS = pass_now_ms;
    if pass_gap_ms > CyberpunkVR_VrikBatchGapMs as f64 {
        BATCH_SEQ = BATCH_SEQ.wrapping_add(1);
        PASS_IN_BATCH = 0;
    } else {
        PASS_IN_BATCH += 1;
    }
    if CyberpunkVR_XrDeepDiag != 0 {
        note_batch_gap(pass_gap_ms, pass_gap_ms > CyberpunkVR_VrikBatchGapMs as f64);
    }
    let tick_now = if CyberpunkVR_VrikBatchClock != 0 { BATCH_SEQ } else { g_VrikFrameEpoch.load(Ordering::Relaxed) };
    // SNAP EVENT AWARE REPLAY DECISION: only the snap counter [147] breaks a tick's replay.
    let snap_ctr_now = *g_pSharedHands.add(147);
    let snap_event = g_solveCacheN > 0 && snap_ctr_now != g_solveCacheSnapCtr;
    if tick_now == g_solveCacheTick && g_solveCacheN > 0 && !snap_event {
        // HOW FAR THE ENGINE HAD MOVED THE BONE BEFORE WE PUT IT BACK (diagnostic).
        if CyberpunkVR_XrDeepDiag != 0 {
            let mut worst = 0.0f32;
            let mut worst_idx = -1;
            if g_solveCacheModel {
                VRIK_ComputeFK(bone_buf, VRIK_FKCount());
            }
            for ci in 0..g_solveCacheN.max(0) as usize {
                let bi = g_solveCacheIdx[ci];
                let t0 = if g_solveCacheModel { g_fkPos[bi as usize] } else { a3(bt(bone_buf, bi)) };
                let dx = t0[0] - g_solveCacheVal[ci][0];
                let dy = t0[1] - g_solveCacheVal[ci][1];
                let dz = t0[2] - g_solveCacheVal[ci][2];
                let mm = sqrt(dx * dx + dy * dy + dz * dz) * 1000.0;
                if mm < 2000.0 && mm > worst {
                    worst = mm;
                    worst_idx = bi;
                }
            }
            if worst > g_VrikEngineOverwriteMm {
                g_VrikEngineOverwriteMm = worst;
                g_VrikEngineOverwriteBone = worst_idx;
            }
            if worst > 10.0 {
                g_VrikEngineOverwriteHits += 1;
                let slot = if PASS_IN_BATCH < 7 { PASS_IN_BATCH } else { 7 };
                g_VrikOverwritePassHist[slot as usize] += 1;
            }
            g_VrikEngineOverwritePasses += 1;
        }
        VRIK_ReplaySolveCache(bone_buf);
        if !pose_ladder && (!pose_mounted || pose_window_combat) {
            VRIK_ApplyCyberwarePose(bone_buf, track_buf as *mut f32, track_count);
        }
        g_VRIKReplayTotal += 1;
        return;
    }
    fresh_solve(bone_buf, track_buf, track_count, pose_mounted, pose_window_combat, pose_seated, pose_ladder, carry_left_arm,
                carry_right_arm, c, tick_now, snap_ctr_now, snap_event);
}

/// The view-frame hand basis both arms are built in: the view's world rotation (vq), the view origin in the model
/// (vpM), the hand frame's model rotation (rvM), and the model -> world rotation that frame stands for (the entity's
/// own on the native frame, else the inverse of the world -> model turn ec).
#[allow(clippy::too_many_arguments)]
unsafe fn view_hand_basis(vp_view: &V3, native_hand_frame: bool, native_entity_rot: &Q, native_view_rot: &Q,
                          native_view_pos: &V3, hmd_rel: &Q, vrik_in_vehicle: bool, cam_model_entity_quat: &Q,
                          raw_cam_model_pos: &V3) -> (Q, V3, Q, Q) {
    let mut vq: Q = if native_hand_frame {
        quat_mul(native_entity_rot, native_view_rot)
    } else if CyberpunkVR_VrikHandFrameOneLatch != 0 && g_viewPktValid {
        [g_viewPkt[0], g_viewPkt[1], g_viewPkt[2], g_viewPkt[3]]
    } else {
        [sp(104), sp(105), sp(106), sp(107)]
    };
    quat_norm(&mut vq);
    // WORLD->MODEL yaw: the game heading from the same view packet as vq; the body's true yaw under body follow.
    let mut vyaw = if g_viewPktValid {
        g_viewPkt[8]
    } else {
        let f_x = 2.0 * (vq[0] * vq[1] - vq[3] * vq[2]);
        let f_y = 1.0 - 2.0 * (vq[0] * vq[0] + vq[2] * vq[2]);
        atan2(-f_x, f_y)
    };
    if cvr_cpp_body_yaw_follow_active() && CyberpunkVR_BodyYawFinalValid != 0 {
        vyaw = CyberpunkVR_BodyYawFinalRad;
    }
    let mut ec = [0.0f32; 4];
    let entity = if vrik_in_vehicle && CyberpunkVR_VrikVehicleFullEntityQuat != 0 && CyberpunkVR_VrikTransformsFromPlugin != 0 {
        cam_model_entity_quat.as_ptr()
    } else {
        core::ptr::null()
    };
    VRIK_WorldToModelRotation(vyaw, entity, ec.as_mut_ptr());
    // TIME-ALIGN THE HAND FRAME: vq's head part replaced by the hand sample's.
    let mut vq_use = vq;
    if !native_hand_frame && g_viewPktValid && CyberpunkVR_VrikHandFrameAlign != 0 {
        VRIK_AlignViewToHands(vq.as_ptr(), (&raw const g_viewPkt[13]) as *const f32, hmd_rel.as_ptr(), vq_use.as_mut_ptr());
    }
    let mut rv_m = qmul_n(&ec, &vq_use);
    if native_hand_frame {
        rv_m = *native_view_rot;
    }
    // THE VIEW-FRAME BRANCH TAKES THE VIEW ORIGIN, NOT THE ANCHOR.
    let vp_w = [vp_view[0] - g_VREntityPosX, vp_view[1] - g_VREntityPosY, vp_view[2] - g_VREntityPosZ];
    let mut vp_m = qrot(&ec, &vp_w);
    if native_hand_frame {
        vp_m = *native_view_pos;
    } else if !vrik_in_vehicle && CyberpunkVR_VrikNativeFramePair != 0 {
        VRIK_ResolveViewModelPos(raw_cam_model_pos.as_ptr(), cam_model_entity_quat.as_ptr(), vp_m.as_mut_ptr());
    }
    let m2w = if native_hand_frame { *native_entity_rot } else { [-ec[0], -ec[1], -ec[2], ec[3]] };
    (vq, vp_m, rv_m, m2w)
}

/// The base-frame fallback (no view pose): the controller in the recenter base, mapped and rotated by the hand frame.
#[allow(clippy::too_many_arguments)]
fn base_frame_hand(hmd_rel: &Q, vr_pos: &V3, vr_quat: &Q, hmd_pos_base: &V3, scale: f32, cam_model_hand_rot: &Q,
                   hand_anchor: &V3, off: &V3, wrist: &Q, target: &mut V3, hand_rot: &mut Q) {
    let mut hq = *hmd_rel;
    quat_norm(&mut hq);
    let mut cbx = qrot(&hq, vr_pos);
    cbx[0] += hmd_pos_base[0];
    cbx[1] += hmd_pos_base[1];
    cbx[2] += hmd_pos_base[2];
    let mp = [cbx[0] * scale, -cbx[2] * scale, cbx[1] * scale];
    let rp = qrot(cam_model_hand_rot, &mp);
    for k in 0..3 {
        target[k] = hand_anchor[k] + rp[k] + off[k];
    }
    let cq = qmul_n(&hq, vr_quat);
    let lq = [cq[0], -cq[2], cq[1], cq[3]];
    let hm = quat_mul(cam_model_hand_rot, &lq);
    *hand_rot = qmul_n(&hm, wrist);
}

unsafe fn write_bone_local(bone_buf: *mut u8, bone: i32, pos: &V3, rot: &Q) {
    put3(bt(bone_buf, bone), pos);
    put4(br(bone_buf, bone), rot);
}

fn dot3(a: &V3, b: &V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// THE FRESH SOLVE: the camera, the body under the HMD, the shoulders, both arms built in the view frame and solved,
/// the holster distances, and the cache the rest of the batch replays.
#[allow(clippy::too_many_arguments, clippy::cognitive_complexity)]
unsafe fn fresh_solve(bone_buf: *mut u8, track_buf: usize, track_count: u32, pose_mounted: bool, pose_window_combat: bool,
                      pose_seated: bool, pose_ladder: bool, carry_left_arm: bool, carry_right_arm: bool, c: &[i32; 3], tick_now: u32,
                      snap_ctr_now: f32, snap_event: bool) {
    g_VRIKFreshTotal += 1;
    let solve_hand_sequence = g_handsStableSeq;
    let solve_pose_id: u64 = if sp(115) == 2.0 { (1u64 << 63) | solve_hand_sequence as u64 } else { 0 };
    // Latch the render view packet ONCE for this solve.
    VRIK_LatchViewPacket();
    if g_viewPktValid {
        g_solveCacheYaw = g_viewPkt[8];
    }
    // Hand recoil advances ONCE per fresh solve.
    RecoilTick();
    g_solveCacheSnapCtr = snap_ctr_now;
    let vrik_in_vehicle = pose_mounted;
    let vrik_window_combat = pose_window_combat;
    let mut vehicle_upper_solved = false;
    let mut cam_model_pos: V3 = [0.0; 3];
    let mut cam_model_rot: Q = ID;
    let mut cam_model_entity_quat: Q = ID;
    let mut cam_model_paired_rot: Q = ID;
    let mut paired_body_base: V3 = [0.0; 3];
    let mut camera_status: i32 = 0;
    let cam_model_valid = VRIK_ComputeCamModel(cam_model_pos.as_mut_ptr(), cam_model_rot.as_mut_ptr(),
        cam_model_entity_quat.as_mut_ptr(), cam_model_paired_rot.as_mut_ptr(), paired_body_base.as_mut_ptr(),
        &mut camera_status);
    if !cam_model_valid {
        cvr_anim_wheel_maintain_grab();
        let action = if snap_event { CAMERA_INVALIDATED } else { camera_status };
        if VRIK_RestoreMissingCamera(bone_buf, action, tick_now, now_ms()) {
            g_VRIKReplayTotal += 1;
        }
        RETURNED = true;
        return;
    }
    VRIK_ComputeFK(bone_buf, VRIK_FKCount());
    // WHEEL GRAB and the native reload: captured from the pure animated pose.
    cvr_anim_wheel_capture_anim(0, g_VRRightBoneIdx);
    cvr_anim_wheel_capture_anim(1, g_VRLeftBoneIdx);
    cvr_anim_reload_hand_off_capture();
    cvr_anim_reload_hand_off_update();
    {
        let qc = clock_qpc();
        let qf = clock_qpc_frequency();
        let mut dt = 0.016f32;
        if WHEEL_LAST_QPC != 0 && qf > 0.0 {
            dt = ((qc - WHEEL_LAST_QPC) as f64 / qf) as f32;
        }
        WHEEL_LAST_QPC = qc;
        cvr_anim_wheel_update(dt);
    }
    // AN ARM HANDED TO THE ANIMATION: a hand on the wheel, or an arm holding a carried body.
    let arm_to_anim_r = cvr_anim_wheel_hands_off(0) || carry_right_arm;
    let arm_to_anim_l = cvr_anim_wheel_hands_off(1) || carry_left_arm;
    VRIK_BeginBodySolve(bone_buf);
    if !vrik_in_vehicle && !pose_ladder {
        VRIK_DampenTorsoWeaponPose(bone_buf);
        VRIK_PinGirdleTranslations(bone_buf);
    } else if vrik_window_combat || pose_ladder {
        VRIK_PinGirdleTranslations(bone_buf);
        VRIK_ComputeFK(bone_buf, VRIK_FKCount());
    }
    // IK-style arm-length calibration; an arm handed to the animation keeps the rig's own lengths.
    {
        if arm_to_anim_r {
            VRIK_RestoreArmRestTrans(bone_buf, track_buf, g_VRBoneCount, g_VRRightUpperArmIdx, g_VRRightForeArmIdx,
                                     g_VRRightBoneIdx, false);
        } else {
            VRIK_ScaleArmBonesFromRest(bone_buf, track_buf, g_VRBoneCount, g_VRRightForeArmIdx, g_VRRightBoneIdx,
                                       g_VRUserArmLenR);
        }
        if arm_to_anim_l {
            VRIK_RestoreArmRestTrans(bone_buf, track_buf, g_VRBoneCount, g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx,
                                     g_VRLeftBoneIdx, true);
        } else {
            VRIK_ScaleArmBonesFromRest(bone_buf, track_buf, g_VRBoneCount, g_VRLeftForeArmIdx, g_VRLeftBoneIdx,
                                       g_VRUserArmLenL);
        }
        VRIK_ComputeFK(bone_buf, VRIK_FKCount());
    }
    let mut rh_wrist_model: V3 = [0.0; 3];
    let mut rh_wrist_valid = false;
    let mut lh_wrist_model: V3 = [0.0; 3];
    let mut lh_wrist_valid = false;
    let h_idx = g_VRHeadBoneIdx;
    // THE ARMS HANG OFF THE ENGINE'S OWN FK HEAD, this frame's: a pointer into the FK, read where it is used.
    let head_model_pos: *const f32 =
        if (0..MAXB).contains(&h_idx) { (&raw const g_fkPos[h_idx as usize]) as *const f32 } else { core::ptr::null() };
    let hmd_rel: Q = [sp(16), sp(17), sp(18), sp(19)];
    let consumed = if sp(115) == 2.0 {
        [sp(112), sp(113)]
    } else {
        let mut v = [0.0f32; 2];
        cvr_cpp_roomscale_camera_consumed(v.as_mut_ptr());
        v
    };
    let mut hmd_pos_base: V3 = [0.0; 3];
    VRIK_HeadResidual(consumed[0], consumed[1], hmd_pos_base.as_mut_ptr());
    let mut native_hand_frame = false;
    super::physhands::pose::begin_pose_frame(&super::physhands::types::PoseFrame::default());
    let mut native_view_pos: V3 = [0.0; 3];
    let mut native_view_rot: Q = ID;
    let mut native_head_delta_model: V3 = [0.0; 3];
    let mut body_orbit_model_v: V3 = [0.0; 3];
    let mut native_entity_pos: V3 = [0.0; 3];
    let mut native_entity_rot: Q = ID;
    let mut native_tracking_yaw = 0.0f32;
    let mut hand_recipe = AnchorRecipe::default();
    if !vrik_in_vehicle && cam_model_valid && CyberpunkVR_VrikNativeFramePair != 0 && CyberpunkVR_CamWriteInPatch != 0
        && CyberpunkVR_CamComposeAtWrite != 0 && CyberpunkVR_HeadTranslationInPatch != 0
        && CyberpunkVR_HeadingFromPreWrite != 0 && CyberpunkVR_ViewYawFromEngine != 0 && g_headingValid != 0
        && VRIK_ReadCurrentBodyFrame(native_entity_pos.as_mut_ptr(), native_entity_rot.as_mut_ptr(), &mut native_tracking_yaw)
        && super::camera_link::cvr_camera_anchor_recipe_read(&mut hand_recipe)
    {
        let body_yaw = 2.0 * atan2(native_entity_rot[2], native_entity_rot[3]);
        let mut rest = [0.0f32; 2];
        let have_rest_neck = CyberpunkVR_BodyOrbitRestNeck != 0 && cvr_cpp_roomscale_camera_rest_neck(rest.as_mut_ptr());
        let rest_neck_vector = AnchorVector { x: rest[0], y: rest[1], z: 0.0 };
        CyberpunkVR_DebugRestNeck[0] = if have_rest_neck { rest[0] } else { f32::NAN };
        CyberpunkVR_DebugRestNeck[1] = if have_rest_neck { rest[1] } else { f32::NAN };
        let orbit = body_orbit_model(&hand_recipe, body_yaw, native_tracking_yaw,
            if have_rest_neck { Some(&rest_neck_vector) } else { None });
        body_orbit_model_v[0] = orbit.x;
        body_orbit_model_v[1] = orbit.y;
        // The visible body's dead zones (off by default): off the neck the orbit stands it under, tracking axes to model
        // axes as the rest neck goes into the orbit.
        if orbit.x == orbit.x {
            if let Some(zones) = super::roomscale_move::body_dead_zone_offset() {
                let s = cvr_camera::anchor::body_orbit_scale(&hand_recipe);
                let d = cvr_camera::anchor::rotate_anchor_yaw(AnchorVector { x: zones.x * s, y: zones.y * s, z: 0.0 },
                    native_tracking_yaw - body_yaw);
                body_orbit_model_v[0] += d.x;
                body_orbit_model_v[1] += d.y;
            }
        }
        // THE BODY UNDER THE RENDERED HEAD, EVERY FRAME (strict roomscale; main's 0.2.3: the body jerked while walking).
        // The capsule is placed from the head sample its tick took; ticks and head samples do not pair one to one, so
        // walking the capsule moved 0, 0, then 30 mm a tick while the camera -- this frame's head -- moved evenly. The body
        // now stands under this frame's head: the capsule's place plus the head's motion since the sample it was placed
        // from (this frame's residual less the eye's then), as BONELAB's body is pulled under the head every tick.
        if orbit.x == orbit.x {
            if let Some(tick_eye) = super::roomscale_move::strict_tick_eye() {
                let since = [hmd_pos_base[0] - tick_eye.x, -hmd_pos_base[2] - tick_eye.y];
                let len = (since[0] * since[0] + since[1] * since[1]).sqrt();
                if len.is_finite() && len < 0.25 {
                    let s = cvr_camera::anchor::body_orbit_scale(&hand_recipe);
                    let d = cvr_camera::anchor::rotate_anchor_yaw(AnchorVector { x: since[0] * s, y: since[1] * s, z: 0.0 },
                        native_tracking_yaw - body_yaw);
                    body_orbit_model_v[0] += d.x;
                    body_orbit_model_v[1] += d.y;
                }
            }
        }
        let base = AnchorVector { x: paired_body_base[0], y: paired_body_base[1], z: paired_body_base[2] };
        let head_q = Quat { x: hmd_rel[0], y: hmd_rel[1], z: hmd_rel[2], w: hmd_rel[3] };
        let pitch = Quat { x: g_headingPitchS, y: 0.0, z: 0.0, w: g_headingPitchC };
        let frame = build_hand_view_frame(base, &hand_recipe,
            AnchorVector { x: hmd_pos_base[0], y: hmd_pos_base[1], z: hmd_pos_base[2] }, head_q, body_yaw,
            native_tracking_yaw, pitch);
        native_view_pos = [frame.position.x, frame.position.y, frame.position.z];
        let neutral = build_hand_view_frame(base, &hand_recipe, AnchorVector::default(), head_q, body_yaw,
            native_tracking_yaw, pitch);
        native_head_delta_model = [frame.position.x - neutral.position.x, frame.position.y - neutral.position.y,
                                   frame.position.z - neutral.position.z];
        native_view_rot = [frame.rotation.x, frame.rotation.y, frame.rotation.z, frame.rotation.w];
        cam_model_rot = native_view_rot;
        cam_model_paired_rot = native_view_rot;
        native_hand_frame = true;
    }
    // Physics gets the primary avatar's model basis and the tracking reference from this same solve. Removing the HMD
    // contribution leaves locomotion/stick motion; torso follow is only a change of model coordinates.
    {
        let (mut p, mut q, mut yaw) = ([0.0f32; 3], ID, 0.0f32);
        if native_hand_frame {
            p = native_entity_pos;
            q = native_entity_rot;
        }
        let mut have = native_hand_frame || VRIK_ReadCurrentBodyFrame(p.as_mut_ptr(), q.as_mut_ptr(), &mut yaw);
        if vrik_in_vehicle && !native_hand_frame {
            // SEATED the body frame is the movement's, and a mounted player's movement keeps its place at zero (its
            // body-yaw site still fires): the physics hands were sent to the world's origin in every vehicle and met
            // nothing of the car (2026-11-26). The entity's own world transform is the seat's.
            let seat = super::npc_grip::entity_frame(super::roomscale_move::cvr_roomscale_player_identity());
            match seat {
                Some((ep, eq)) => {
                    p = ep;
                    q = eq;
                    have = true;
                    super::physhands::census::note_seat(&ep);
                }
                None => {
                    have = false;
                    static SEAT_TOLD: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
                    let now = now_ms() as u64;
                    if now.wrapping_sub(SEAT_TOLD.load(Ordering::Relaxed)) > 30_000 {
                        SEAT_TOLD.store(now, Ordering::Relaxed);
                        cvr_host::log!("[physhands] seated: the player entity's world transform not read: no physics hands in the vehicle
");
                    }
                }
            }
        }
        if have {
            use cvr_physhands::handspace::{self, Rotation, Vector};
            let owner = super::roomscale_move::cvr_roomscale_player_identity();
            let origin = super::physhands::step::cvr_cpp_xr_tracking_origin_serial();
            let mut physics_frame = super::physhands::types::PoseFrame::default();
            physics_frame.entity = handspace::make(Vector::new(p[0], p[1], p[2]), Rotation { x: q[0], y: q[1], z: q[2], w: q[3] }, owner, origin);
            if native_hand_frame {
                physics_frame.tracking = handspace::from_view(
                    &physics_frame.entity,
                    Vector::new(native_view_pos[0], native_view_pos[1], native_view_pos[2]),
                    Rotation { x: native_view_rot[0], y: native_view_rot[1], z: native_view_rot[2], w: native_view_rot[3] },
                    Vector::new(native_head_delta_model[0], native_head_delta_model[1], native_head_delta_model[2]),
                    Rotation { x: hmd_rel[0], y: -hmd_rel[2], z: hmd_rel[1], w: hmd_rel[3] },
                );
            }
            super::physhands::pose::begin_pose_frame(&physics_frame);
        }
    }
    let resolve_view_world = |out: &mut V3| -> bool {
        if !native_hand_frame {
            return VRIK_ResolveViewPos(out.as_mut_ptr());
        }
        let r = qrot(&native_entity_rot, &native_view_pos);
        for k in 0..3 {
            out[k] = r[k] + native_entity_pos[k];
        }
        true
    };
    if !head_model_pos.is_null() {
        // Fallback body frame, replaced by HMD/body yaw once the camera pose is available.
        let mut body_up: V3;
        let mut body_right: V3 = [1.0, 0.0, 0.0];
        {
            let root_p = g_fkPos[0];
            let head = a3(head_model_pos);
            body_up = [head[0] - root_p[0], head[1] - root_p[1], head[2] - root_p[2]];
            if VRIK_Norm3(body_up.as_mut_ptr()) < 1e-4 {
                body_up = [0.0, 0.0, 1.0];
            }
            if g_VRRightUpperArmIdx >= 0 && g_VRLeftUpperArmIdx >= 0 {
                let rs = g_fkPos[g_VRRightUpperArmIdx as usize];
                let ls = g_fkPos[g_VRLeftUpperArmIdx as usize];
                body_right = [rs[0] - ls[0], rs[1] - ls[1], rs[2] - ls[2]];
                if VRIK_Norm3(body_right.as_mut_ptr()) < 1e-4 {
                    body_right = [1.0, 0.0, 0.0];
                }
            }
        }
        let mut body_fwd: V3 = [0.0; 3];
        VRIK_Cross3(body_up.as_ptr(), body_right.as_ptr(), body_fwd.as_mut_ptr());
        if VRIK_Norm3(body_fwd.as_mut_ptr()) < 1e-4 {
            body_fwd = [0.0, 1.0, 0.0];
        }
        let cam_model_hand_rot: Q = if CyberpunkVR_VrikSplitHeadHandRot != 0 { cam_model_paired_rot } else { cam_model_rot };
        let raw_cam_model_pos = cam_model_pos;
        if cam_model_valid && !g_pSharedHands.is_null() && !vrik_in_vehicle && CyberpunkVR_HeadTranslationInPatch == 0 {
            cam_model_pos[0] += sp(91);
            cam_model_pos[1] += sp(92);
            cam_model_pos[2] += sp(93);
        }
        // Anchor = the render-view point: body camera plus the view-only offsets.
        let mut hand_anchor = cam_model_pos;
        if !g_pSharedHands.is_null() && sp(123) == 1.0 {
            let bkx = if vrik_in_vehicle { 0.0 } else { sp(91) };
            let bky = if vrik_in_vehicle { 0.0 } else { sp(92) };
            let bkz = if vrik_in_vehicle { 0.0 } else { sp(93) };
            hand_anchor[0] += sp(120) - bkx;
            hand_anchor[1] += sp(121) - bky;
            hand_anchor[2] += sp(122) - bkz;
        }
        if CyberpunkVR_XrDeepDiag != 0 {
            VRIK_NoteShake(0, 1, hand_anchor.as_ptr());
        }
        if CyberpunkVR_XrDeepDiag != 0 {
            VRIK_NoteShake(1, 1, hand_anchor.as_ptr());
        }
        if !(CyberpunkVR_VrikArmAnchorFromBody != 0
            && VRIK_BodyAxesFromRig(body_right.as_mut_ptr(), body_up.as_mut_ptr(), body_fwd.as_mut_ptr(), vrik_window_combat))
            && cam_model_valid
        {
            VRIK_BodyAxesFromCamYaw(cam_model_hand_rot.as_ptr(), body_right.as_mut_ptr(), body_up.as_mut_ptr(),
                                    body_fwd.as_mut_ptr());
        }
        // BODY ANCHOR: no smoothing, glued to the camera.
        if cam_model_valid && g_VRBodyUnderHMD != 0 && !vrik_in_vehicle {
            let fwd_signed = body_fwd;
            let mut body_cam_model_pos = cam_model_pos;
            if CyberpunkVR_HeadTranslationInPatch != 0 {
                body_cam_model_pos = paired_body_base;
            }
            let view_target = if native_hand_frame { native_view_pos.as_ptr() } else { hand_anchor.as_ptr() };
            if pose_ladder {
                VRIK_PlaceLadderUpper(bone_buf, view_target, cam_model_rot.as_ptr(), h_idx);
            } else if cvr_cpp_swimming_active() {
                VRIK_PlaceSwimmingBody(bone_buf, view_target, cam_model_rot.as_ptr(), h_idx);
            } else {
                VRIK_PlaceBodyUnderHMD(bone_buf, body_cam_model_pos.as_ptr(), cam_model_rot.as_ptr(), h_idx,
                    fwd_signed.as_ptr(),
                    if native_hand_frame { native_head_delta_model.as_ptr() } else { core::ptr::null() },
                    if native_hand_frame { body_orbit_model_v.as_ptr() } else { core::ptr::null() });
            }
            if !pose_ladder && !cvr_cpp_swimming_active() {
                cvr_cpp_fbt_solve_lower_body(bone_buf,
                    if native_hand_frame { native_view_pos.as_ptr() } else { cam_model_pos.as_ptr() },
                    cam_model_rot.as_ptr());
            }
            // Re-taken AFTER the placement.
            if !(CyberpunkVR_VrikArmAnchorFromBody != 0
                && VRIK_BodyAxesFromRig(body_right.as_mut_ptr(), body_up.as_mut_ptr(), body_fwd.as_mut_ptr(), false))
            {
                VRIK_BodyAxesFromCamYaw(cam_model_hand_rot.as_ptr(), body_right.as_mut_ptr(), body_up.as_mut_ptr(),
                                        body_fwd.as_mut_ptr());
            }
            // The mount, derived once from the neutral rig.
            if !g_pSharedHands.is_null() {
                let mut mount = AnchorVector::default();
                let valid = super::rig::cvr_vrik_read_neck_camera_mount(&mut mount);
                *g_pSharedHands.add(85) = mount.x;
                *g_pSharedHands.add(86) = mount.y;
                *g_pSharedHands.add(87) = mount.z;
                *g_pSharedHands.add(88) = if valid { 1.0 } else { 0.0 };
            }
        }
        if cam_model_valid && g_VRBodyUnderHMD != 0 && (vrik_window_combat || pose_seated) {
            let mut upper_head = cam_model_rot;
            if g_viewPktValid && CyberpunkVR_VrikVehicleFullEntityQuat != 0 && CyberpunkVR_VrikTransformsFromPlugin != 0 {
                let mut view = [0.0f32; 4];
                for k in 0..4 {
                    view[k] = if CyberpunkVR_VrikHandFrameOneLatch != 0 { g_viewPkt[k] } else { sp(104 + k as i32) };
                }
                quat_norm(&mut view);
                let mut aligned = view;
                if CyberpunkVR_VrikHandFrameAlign != 0 {
                    VRIK_AlignViewToHands(view.as_ptr(), (&raw const g_viewPkt[13]) as *const f32, hmd_rel.as_ptr(),
                                          aligned.as_mut_ptr());
                }
                let mut inverse = [0.0f32; 4];
                VRIK_WorldToModelRotation(0.0, cam_model_entity_quat.as_ptr(), inverse.as_mut_ptr());
                upper_head = qmul_n(&inverse, &aligned);
            }
            vehicle_upper_solved = if vrik_window_combat {
                VRIK_PlaceVehicleCombatUpper(bone_buf, cam_model_pos.as_ptr(), upper_head.as_ptr(), h_idx)
            } else {
                VRIK_PlaceSeatedUpper(bone_buf, cam_model_pos.as_ptr(), paired_body_base.as_ptr(), upper_head.as_ptr(), h_idx, arm_to_anim_r,
                                      arm_to_anim_l)
            };
            if vehicle_upper_solved {
                VRIK_BodyAxesFromRig(body_right.as_mut_ptr(), body_up.as_mut_ptr(), body_fwd.as_mut_ptr(), true);
            }
        }
        // WEAPON-POSE-IMMUNE HEAD REFERENCE: the (head bone - camera) relation frozen after ~90 frames.
        let mut head_ref = a3(head_model_pos);
        if cam_model_valid {
            if HEAD_OFF_N < 90 {
                let hp = a3(head_model_pos);
                let dw = [hp[0] - cam_model_pos[0], hp[1] - cam_model_pos[1], hp[2] - cam_model_pos[2]];
                let b0 = dot3(&dw, &body_right);
                let b1 = dot3(&dw, &body_fwd);
                let b2 = dot3(&dw, &body_up);
                let k = 1.0 / (HEAD_OFF_N + 1) as f32;
                HEAD_OFF_B[0] += (b0 - HEAD_OFF_B[0]) * k;
                HEAD_OFF_B[1] += (b1 - HEAD_OFF_B[1]) * k;
                HEAD_OFF_B[2] += (b2 - HEAD_OFF_B[2]) * k;
                HEAD_OFF_N += 1;
            }
            for k in 0..3 {
                head_ref[k] = cam_model_pos[k] + body_right[k] * HEAD_OFF_B[0] + body_fwd[k] * HEAD_OFF_B[1]
                    + body_up[k] * HEAD_OFF_B[2];
            }
        }
        let calibrated_shoulder_model = |shoulder_body: &V3| -> V3 {
            let mut o = [0.0f32; 3];
            for k in 0..3 {
                o[k] = head_ref[k] + body_right[k] * shoulder_body[0] + body_up[k] * shoulder_body[1]
                    - body_fwd[k] * shoulder_body[2];
            }
            o
        };
        // THE GIRDLE HANGS OFF THE BODY: the neck, the chest, the camera as a last resort.
        let mut stable_anchor: *const f32 = head_model_pos;
        let mut stable_anchor_buf: V3 = [0.0; 3];
        let mut anchor_bone = -1;
        if CyberpunkVR_VrikArmAnchorFromBody != 0 {
            if (0..MAXB).contains(&g_VRNeckIdx) {
                anchor_bone = g_VRNeckIdx;
            } else if g_VRSpineCount > 0 {
                let top = g_VRSpineIdx[(g_VRSpineCount - 1) as usize];
                if (0..MAXB).contains(&top) {
                    anchor_bone = top;
                }
            }
        }
        if anchor_bone >= 0 {
            stable_anchor_buf = g_fkPos[anchor_bone as usize];
            stable_anchor = stable_anchor_buf.as_ptr();
        } else if cam_model_valid {
            let k_neck_behind = 0.0f32;
            stable_anchor_buf[0] = cam_model_pos[0] - body_fwd[0] * k_neck_behind;
            stable_anchor_buf[1] = cam_model_pos[1] - body_fwd[1] * k_neck_behind;
            stable_anchor_buf[2] = cam_model_pos[2] + g_VRHeadDrop - s_vrSharedSquatDrop;
            stable_anchor = stable_anchor_buf.as_ptr();
        }
        // The thorax turning into a grip the left arm cannot reach.
        if !pose_ladder && !arm_to_anim_l {
            VRIK_GripTorso(bone_buf, body_right.as_ptr(), body_up.as_ptr());
        } else {
            g_VRIKTorsoOn = false;
        }
        let neck_anchor = anchor_bone == g_VRNeckIdx;
        // THE HELD FIREARM IS BUILT BEFORE ITS SUPPORT ARM (main's ad4044f7b): each wrist sends one physics target and
        // takes one arm solve in this pass; left-handed the left arm goes first.
        let left_firearm = left_firearm_drawn();
        let (mut built_weapon_pose, mut solved_support) = (false, false);
        let arm_order: [u8; 2] = if left_firearm { [1, 0] } else { [0, 1] };
        for arm in arm_order {
        if arm == 0 {

        // ---------------------------------------------------------------- right arm
        if sp(8) > 0.0 && g_VRRightUpperArmIdx >= 0 {
            let vr_pos: V3 = [sp(9), sp(10), sp(11)];
            if CyberpunkVR_XrDeepDiag != 0 {
                VRIK_NoteShake(1, 0, vr_pos.as_ptr());
            }
            let vr_quat: Q = [sp(12), sp(13), sp(14), sp(15)];
            let mut target: V3 = [0.0; 3];
            let mut hand_rot: Q = [0.0; 4];
            let mut dbg_target_pre_stop: V3 = [0.0; 3];
            let mut dbg_hand_diag = false;
            let wrist_r: Q = [g_VRWristR_I, g_VRWristR_J, g_VRWristR_K, g_VRWristR_R];
            // the drawn weapon's own wrist turn (weapon_wrist), when this hand holds it
            let wrist_r: Q = if cvr_anim_weapon_in_left_hand() { wrist_r } else { super::weapon_wrist::apply(wrist_r) };
            let off_r: V3 = [g_VROffRX, g_VROffRY, g_VROffRZ];
            let shoulder_body_r: V3 = [g_VRShoulderRX, g_VRShoulderRY, g_VRShoulderRZ];
            let mut shoulder_model_r: V3 = [0.0; 3];
            if arm_to_anim_r {
                shoulder_model_r = g_fkPos[g_VRRightUpperArmIdx as usize];
            } else if cam_model_valid {
                VRIK_AnchorShoulder(bone_buf, g_VRRightUpperArmIdx, stable_anchor, false, body_right.as_ptr(), body_up.as_ptr(),
                    g_VRShoulderRX, shoulder_model_r.as_mut_ptr(), neck_anchor);
            } else {
                shoulder_model_r = calibrated_shoulder_model(&shoulder_body_r);
                apply_shoulder_anchor(bone_buf, g_VRRightUpperArmIdx, &shoulder_model_r);
            }
            VRIK_BuildHandTarget(shoulder_model_r.as_ptr(), shoulder_body_r.as_ptr(), hmd_rel.as_ptr(), vr_pos.as_ptr(),
                vr_quat.as_ptr(), wrist_r.as_ptr(), g_VRScaleR, off_r.as_ptr(), target.as_mut_ptr(), hand_rot.as_mut_ptr());
            // HAND == GIZMO.
            let mut vr_view_pos_m: V3 = [0.0; 3];
            let mut vr_view_rot_m: Q = ID;
            let mut vr_view_frame_ok = false;
            if cam_model_valid {
                let mut vp_view: V3 = [0.0; 3];
                if resolve_view_world(&mut vp_view) {
                    let (vq, vp_m, rv_m, m2w) = view_hand_basis(&vp_view, native_hand_frame, &native_entity_rot, &native_view_rot,
                        &native_view_pos, &hmd_rel, vrik_in_vehicle, &cam_model_entity_quat, &raw_cam_model_pos);
                    // the frame the arms are built in, for what is drawn from the hands in the world (the menus' beams)
                    super::vr_menu::note_hands_frame(m2w, vp_view, vp_m);
                    // The view pose itself, and the exhale-smoke mouth point, in world space.
                    g_VRViewWorldPos = vp_view;
                    g_VRViewWorldRot = vq;
                    g_VRViewWorldValid = 1;
                    {
                        let mo = g_VRSmokeSmokePos;
                        let mo_w = qrot(&vq, &mo);
                        g_VRSmokeMouthWorldPos = [vp_view[0] + mo_w[0], vp_view[1] + mo_w[1], vp_view[2] + mo_w[2]];
                        let sr = g_VRSmokeSmokeRot;
                        g_VRSmokeMouthWorldRot = qmul_n(&vq, &sr);
                        g_VRSmokeMouthWorldValid = 1;
                    }
                    vr_view_pos_m = vp_m;
                    vr_view_rot_m = rv_m;
                    vr_view_frame_ok = true;
                    let mut hm: Q = [0.0; 4];
                    VRIK_BuildViewHandTarget(vp_m.as_ptr(), rv_m.as_ptr(), vr_pos.as_ptr(), vr_quat.as_ptr(), g_VRScaleR,
                        off_r.as_ptr(), target.as_mut_ptr(), hm.as_mut_ptr());
                    {
                        let lp = [sp(1), sp(2), sp(3)];
                        let lmp = [lp[0] * g_VRScaleL, -lp[2] * g_VRScaleL, lp[1] * g_VRScaleL];
                        let lrp = qrot(&rv_m, &lmp);
                        let l_ctrl = [vp_m[0] + lrp[0] + g_VROffLX, vp_m[1] + lrp[1] + g_VROffLY, vp_m[2] + lrp[2] + g_VROffLZ];
                        // the weapon coming back into this hand first, then the hold measured, the kick, the support placed
                        if !cvr_anim_weapon_in_left_hand() {
                            CarryReturnRight(target.as_mut_ptr(), hm.as_mut_ptr(), wrist_r.as_ptr());
                            solve_weapon_hand_pose(target.as_mut_ptr(), hm.as_mut_ptr(), wrist_r.as_ptr(), l_ctrl.as_ptr(), rv_m.as_ptr());
                            built_weapon_pose = true;
                        }
                    }
                    hand_rot = qmul_n(&hm, &wrist_r);
                    dbg_target_pre_stop = target;
                    dbg_hand_diag = true;
                } else {
                    base_frame_hand(&hmd_rel, &vr_pos, &vr_quat, &hmd_pos_base, g_VRScaleR, &cam_model_hand_rot,
                        &hand_anchor, &off_r, &wrist_r, &mut target, &mut hand_rot);
                }
            }
            if dbg_hand_diag {
                let sx = target[0] - dbg_target_pre_stop[0];
                let sy = target[1] - dbg_target_pre_stop[1];
                let sz = target[2] - dbg_target_pre_stop[2];
                CyberpunkVR_DebugHandMissMm = sqrt(sx * sx + sy * sy + sz * sz) * 1000.0;
            }
            rh_wrist_model = target;
            // SHAKE, MEASURED AT FIVE LEVELS (second differences of consecutive fresh solves).
            {
                let mut av: V3 = [0.0; 3];
                VRIK_ResolveViewPos(av.as_mut_ptr());
                let lv: [V3; 5] = [vr_pos, av, target, shoulder_model_r, cam_model_pos];
                if SHAKE_N >= 2 {
                    for l in 0..5usize {
                        let mut a2 = 0.0f32;
                        for k in 0..3 {
                            let d = lv[l][k] - 2.0 * SHAKE_HIST[l][k][0] + SHAKE_HIST[l][k][1];
                            a2 += d * d;
                        }
                        let mm = sqrt(a2) * 1000.0;
                        let slot = if l < 3 { 224 + l } else { 231 + (l - 3) };
                        if mm < 500.0 && mm > *g_pSharedHands.add(slot) {
                            *g_pSharedHands.add(slot) = mm;
                        }
                    }
                }
                for l in 0..5usize {
                    for k in 0..3 {
                        SHAKE_HIST[l][k][1] = SHAKE_HIST[l][k][0];
                        SHAKE_HIST[l][k][0] = lv[l][k];
                    }
                }
                if SHAKE_N < 2 {
                    SHAKE_N += 1;
                }
            }
            rh_wrist_valid = true;
            if CyberpunkVR_XrDeepDiag != 0 {
                VRIK_NoteShake(1, 2, target.as_ptr());
            }
            cvr_anim_wheel_store_target(0, target.as_ptr());
            VRIK_NoteHandAim(0, target.as_ptr(), hand_rot.as_ptr());
            if left_firearm && solved_support && !arm_to_anim_r {
                apply_firearm_support_stop(1, &mut target, &mut hand_rot);
            } else {
                if left_firearm && built_weapon_pose && !arm_to_anim_r {
                    cvr_anim_two_hand_left(target.as_mut_ptr(), hand_rot.as_mut_ptr());
                }
                let unstopped = target;
                VRIK_ApplyHandStop(1, target.as_mut_ptr(), hand_rot.as_mut_ptr());
                if !cvr_anim_weapon_in_left_hand() {
                    two_hand_weapon_hand_moved(&[target[0] - unstopped[0], target[1] - unstopped[1], target[2] - unstopped[2]]);
                }
            }
            cvr_anim_wheel_blend_target(0, target.as_mut_ptr(), hand_rot.as_mut_ptr());
            cvr_anim_reload_hand_off_blend_target(0, target.as_mut_ptr(), hand_rot.as_mut_ptr());
            if pose_ladder && native_hand_frame {
                cvr_cpp_ladder_constrain_hand(1, target.as_mut_ptr(), hand_rot.as_mut_ptr(), native_entity_pos.as_ptr(),
                                              native_entity_rot.as_ptr());
            }
            if !arm_to_anim_r && !cvr_cpp_fbt_solve_arm(bone_buf, 1, target.as_ptr(), hand_rot.as_ptr()) {
                VRIK_SolveArm(bone_buf, g_VRRightUpperArmIdx, g_VRRightForeArmIdx, g_VRRightBoneIdx, target.as_ptr(),
                    hand_rot.as_ptr(), body_right.as_ptr(), body_up.as_ptr(), body_fwd.as_ptr(),
                    g_VRElbowPoleR * 0.017_453_292_52, g_VRElbowSwingR, false, true, pose_ladder);
            }
            g_VRPalmModelR = if arm_to_anim_r { target } else { g_VRIKHandPin[0] };
            g_VRPalmModelRotR = hand_rot;
            if !left_firearm && built_weapon_pose && !arm_to_anim_r {
                solved_support = rebase_firearm_support(bone_buf, g_VRRightBoneIdx);
            }
            g_VRSmokeMouthDist = sqrt(vr_pos[0] * vr_pos[0] + vr_pos[1] * vr_pos[1] + vr_pos[2] * vr_pos[2]);
            // MOUTH ANCHOR: the cig slot pinned to the head-anchored mouth point, through the solved hand.
            if g_VRSmokeMouthAnchor != 0 && (0..MAXB).contains(&g_VRSmokeMouthBoneIdx) && vr_view_frame_ok {
                let mut chain = [0i32; 16];
                let mut cn = 0usize;
                let mut reached_hand = false;
                let mut a = g_VRBoneParent[g_VRSmokeMouthBoneIdx as usize] as i32;
                while (0..MAXB).contains(&a) && cn < 16 {
                    if a == g_VRRightBoneIdx {
                        reached_hand = true;
                        break;
                    }
                    chain[cn] = a;
                    cn += 1;
                    a = g_VRBoneParent[a as usize] as i32;
                }
                if reached_hand {
                    let mut pm_pos = target;
                    let mut pm_rot = hand_rot;
                    for k in (0..cn).rev() {
                        let lt = a3(bt(bone_buf, chain[k]));
                        let lr = a4(br(bone_buf, chain[k]));
                        let rp = qrot(&pm_rot, &lt);
                        pm_pos[0] += rp[0];
                        pm_pos[1] += rp[1];
                        pm_pos[2] += rp[2];
                        pm_rot = qmul_n(&pm_rot, &lr);
                    }
                    let mo = g_VRSmokeMouthPos;
                    let mrl = g_VRSmokeMouthRot;
                    let mo_w = qrot(&vr_view_rot_m, &mo);
                    let mouth_pos = [vr_view_pos_m[0] + mo_w[0], vr_view_pos_m[1] + mo_w[1], vr_view_pos_m[2] + mo_w[2]];
                    let mouth_rot = qmul_n(&vr_view_rot_m, &mrl);
                    let pm_conj = quat_conj(&pm_rot);
                    let rel = [mouth_pos[0] - pm_pos[0], mouth_pos[1] - pm_pos[1], mouth_pos[2] - pm_pos[2]];
                    let local_pos = qrot(&pm_conj, &rel);
                    let local_rot = qmul_n(&pm_conj, &mouth_rot);
                    g_VRSmokeAnchorLocalPos = local_pos;
                    g_VRSmokeAnchorLocalRot = local_rot;
                    g_VRSmokeAnchorValid = 1;
                    write_bone_local(bone_buf, g_VRSmokeMouthBoneIdx, &local_pos, &local_rot);
                }
            }
            // GENERAL mouth-pin: the same lips pose on a non-hand bone (Neck1/Head/Neck).
            {
                let ab_sel = g_VRSmokeAnchorBoneSel;
                let ab = if ab_sel == 1 {
                    g_VRNeck1Idx
                } else if ab_sel == 2 {
                    g_VRHeadBoneIdx
                } else if ab_sel == 3 {
                    g_VRNeckIdx
                } else {
                    -1
                };
                g_VRSmokeAnchorBoneIdx = ab;
                if g_VRSmokeMouthAnchor != 0 && vr_view_frame_ok && (0..MAXB).contains(&ab) {
                    let pa = g_VRBoneParent[ab as usize] as i32;
                    if (0..MAXB).contains(&pa) {
                        let amo = g_VRSmokeMouthPos;
                        let amrl = g_VRSmokeMouthRot;
                        let amo_w = qrot(&vr_view_rot_m, &amo);
                        let amouth_pos = [vr_view_pos_m[0] + amo_w[0], vr_view_pos_m[1] + amo_w[1], vr_view_pos_m[2] + amo_w[2]];
                        let amouth_rot = qmul_n(&vr_view_rot_m, &amrl);
                        let ppos = g_fkPos[pa as usize];
                        let prot = g_fkRot[pa as usize];
                        let p_inv = quat_conj(&prot);
                        let alrot = qmul_n(&p_inv, &amouth_rot);
                        let ad = [amouth_pos[0] - ppos[0], amouth_pos[1] - ppos[1], amouth_pos[2] - ppos[2]];
                        let alpos = qrot(&p_inv, &ad);
                        g_VRSmokeAltAnchorLocalPos = alpos;
                        g_VRSmokeAltAnchorLocalRot = alrot;
                        g_VRSmokeAltAnchorValid = 1;
                        write_bone_local(bone_buf, ab, &alpos, &alrot);
                    }
                } else if ab_sel == 0 {
                    g_VRSmokeAltAnchorValid = 0;
                }
            }
        }

        } else {
        // ---------------------------------------------------------------- left arm
        if sp(0) > 0.0 && g_VRLeftUpperArmIdx >= 0 {
            let vr_pos: V3 = [sp(1), sp(2), sp(3)];
            if CyberpunkVR_XrDeepDiag != 0 {
                VRIK_NoteShake(0, 0, vr_pos.as_ptr());
            }
            let vr_quat: Q = [sp(4), sp(5), sp(6), sp(7)];
            g_VRSmokeMouthDistL = sqrt(vr_pos[0] * vr_pos[0] + vr_pos[1] * vr_pos[1] + vr_pos[2] * vr_pos[2]);
            let mut target: V3 = [0.0; 3];
            let mut hand_rot: Q = [0.0; 4];
            let wrist_l: Q = [g_VRWristL_I, g_VRWristL_J, g_VRWristL_K, g_VRWristL_R];
            let wrist_l: Q = if cvr_anim_weapon_in_left_hand() { super::weapon_wrist::apply(wrist_l) } else { wrist_l };
            let off_l: V3 = [g_VROffLX, g_VROffLY, g_VROffLZ];
            let shoulder_body_l: V3 = [g_VRShoulderLX, g_VRShoulderLY, g_VRShoulderLZ];
            let mut shoulder_model_l: V3 = [0.0; 3];
            if arm_to_anim_l {
                shoulder_model_l = g_fkPos[g_VRLeftUpperArmIdx as usize];
            } else if cam_model_valid {
                VRIK_AnchorShoulder(bone_buf, g_VRLeftUpperArmIdx, stable_anchor, true, body_right.as_ptr(), body_up.as_ptr(),
                    g_VRShoulderRX, shoulder_model_l.as_mut_ptr(), neck_anchor);
            } else {
                shoulder_model_l = calibrated_shoulder_model(&shoulder_body_l);
                apply_shoulder_anchor(bone_buf, g_VRLeftUpperArmIdx, &shoulder_model_l);
            }
            VRIK_BuildHandTarget(shoulder_model_l.as_ptr(), shoulder_body_l.as_ptr(), hmd_rel.as_ptr(), vr_pos.as_ptr(),
                vr_quat.as_ptr(), wrist_l.as_ptr(), g_VRScaleL, off_l.as_ptr(), target.as_mut_ptr(), hand_rot.as_mut_ptr());
            if cam_model_valid {
                let mut vp_view: V3 = [0.0; 3];
                if resolve_view_world(&mut vp_view) {
                    let (_vq, vp_m, rv_m, _m2w) = view_hand_basis(&vp_view, native_hand_frame, &native_entity_rot, &native_view_rot,
                        &native_view_pos, &hmd_rel, vrik_in_vehicle, &cam_model_entity_quat, &raw_cam_model_pos);
                    super::reach_dev::note_left_view(&vp_m, &rv_m, g_VRScaleL, &off_l, &wrist_l);
                    let mut hm: Q = [0.0; 4];
                    VRIK_BuildViewHandTarget(vp_m.as_ptr(), rv_m.as_ptr(), vr_pos.as_ptr(), vr_quat.as_ptr(), g_VRScaleL,
                        off_l.as_ptr(), target.as_mut_ptr(), hm.as_mut_ptr());
                    // LEFT-HANDED THIS IS THE WEAPON HAND.
                    let weapon_hand_l = cvr_anim_weapon_in_left_hand();
                    if weapon_hand_l {
                        let rp = [sp(9), sp(10), sp(11)];
                        let rmp = [rp[0] * g_VRScaleR, -rp[2] * g_VRScaleR, rp[1] * g_VRScaleR];
                        let rrp = qrot(&rv_m, &rmp);
                        let r_ctrl = [vp_m[0] + rrp[0] + g_VROffRX, vp_m[1] + rrp[1] + g_VROffRY, vp_m[2] + rrp[2] + g_VROffRZ];
                        solve_weapon_hand_pose(target.as_mut_ptr(), hm.as_mut_ptr(), wrist_l.as_ptr(), r_ctrl.as_ptr(), rv_m.as_ptr());
                        built_weapon_pose = true;
                    }
                    hand_rot = qmul_n(&hm, &wrist_l);
                    if !weapon_hand_l {
                        // a held grip moves this wrist onto the support point -- unless it was already placed on the
                        // weapon wrist the IK wrote
                        if !solved_support {
                            cvr_anim_two_hand_left(target.as_mut_ptr(), hand_rot.as_mut_ptr());
                        }
                        CarryLeftBlend(hand_rot.as_mut_ptr());
                        CarryRecordLeft(target.as_ptr(), hand_rot.as_ptr());
                    }
                    // RECOIL on a free hand -- not while it is welded to the weapon.
                    {
                        let mut back_m = 0.0f32;
                        let mut rise_rad = 0.0f32;
                        if !weapon_hand_l && CyberpunkVR_TwoHandActive == 0 {
                            RecoilSample(0, 0, &mut back_m, &mut rise_rad);
                        }
                        if back_m != 0.0 || rise_rad != 0.0 {
                            let (mut b_f, mut d_f) = (1.0f32, 0.0f32);
                            RecoilTravelMix(&mut b_f, &mut d_f);
                            let vf = qrot(&rv_m, &[0.0, 1.0, 0.0]);
                            let vl = sqrt(vf[0] * vf[0] + vf[1] * vf[1]);
                            if vl > 1e-5 {
                                let bx = -vf[0] / vl * b_f * back_m;
                                let by = -vf[1] / vl * b_f * back_m;
                                target[0] += bx;
                                target[1] += by;
                            }
                            target[2] -= d_f * back_m;
                            let ax = qrot(&hm, &[1.0, 0.0, 0.0]);
                            let h2 = rise_rad * 0.5;
                            let s2 = sin(h2);
                            let kq = [ax[0] * s2, ax[1] * s2, ax[2] * s2, cos(h2)];
                            hand_rot = qmul_n(&kq, &hand_rot);
                        }
                    }
                } else {
                    base_frame_hand(&hmd_rel, &vr_pos, &vr_quat, &hmd_pos_base, g_VRScaleL, &cam_model_hand_rot,
                        &hand_anchor, &off_l, &wrist_l, &mut target, &mut hand_rot);
                }
            }
            if CyberpunkVR_XrDeepDiag != 0 {
                VRIK_NoteShake(0, 2, target.as_ptr());
            }
            lh_wrist_model = target;
            lh_wrist_valid = true;
            cvr_anim_wheel_store_target(1, target.as_ptr());
            VRIK_NoteHandAim(1, target.as_ptr(), hand_rot.as_ptr());
            if !left_firearm && solved_support && !arm_to_anim_l {
                apply_firearm_support_stop(0, &mut target, &mut hand_rot);
                CarryRecordLeft(target.as_ptr(), hand_rot.as_ptr());
            } else {
                let unstopped = target;
                VRIK_ApplyHandStop(0, target.as_mut_ptr(), hand_rot.as_mut_ptr());
                if cvr_anim_weapon_in_left_hand() {
                    two_hand_weapon_hand_moved(&[target[0] - unstopped[0], target[1] - unstopped[1], target[2] - unstopped[2]]);
                }
            }
            cvr_anim_wheel_blend_target(1, target.as_mut_ptr(), hand_rot.as_mut_ptr());
            cvr_anim_reload_hand_off_blend_target(1, target.as_mut_ptr(), hand_rot.as_mut_ptr());
            if pose_ladder && native_hand_frame {
                cvr_cpp_ladder_constrain_hand(0, target.as_mut_ptr(), hand_rot.as_mut_ptr(), native_entity_pos.as_ptr(),
                                              native_entity_rot.as_ptr());
            }
            if !arm_to_anim_l && !cvr_cpp_fbt_solve_arm(bone_buf, 0, target.as_ptr(), hand_rot.as_ptr()) {
                VRIK_SolveArm(bone_buf, g_VRLeftUpperArmIdx, g_VRLeftForeArmIdx, g_VRLeftBoneIdx, target.as_ptr(),
                    hand_rot.as_ptr(), body_right.as_ptr(), body_up.as_ptr(), body_fwd.as_ptr(),
                    g_VRElbowPoleL * 0.017_453_292_52, g_VRElbowSwingL, true, true, pose_ladder);
            }
            g_VRPalmModelL = if arm_to_anim_l { target } else { g_VRIKHandPin[1] };
            g_VRPalmModelRotL = hand_rot;
            g_VRCamModelPos = if native_hand_frame && cvr_cpp_vrik_fbt_body_active() { native_view_pos } else { cam_model_pos };
            g_VRCamModelRot = cam_model_hand_rot;
            g_VRPalmModelValid = 1;
            if left_firearm && built_weapon_pose && !arm_to_anim_l {
                solved_support = rebase_firearm_support(bone_buf, g_VRLeftBoneIdx);
            }
        }
        }
        }
        // Other left-hand items keep the carry / interaction pass: the right arm was solved before the weapon hand built
        // the support point, so a held two-hand grip solves it again onto that point.
        if !left_firearm && cvr_anim_weapon_in_left_hand() && sp(8) > 0.0 && g_VRRightUpperArmIdx >= 0 && !arm_to_anim_r {
            let mut t = g_VRPalmModelR;
            let mut q = g_VRPalmModelRotR;
            if cvr_anim_two_hand_left(t.as_mut_ptr(), q.as_mut_ptr()) {
                VRIK_ApplyHandStop(1, t.as_mut_ptr(), q.as_mut_ptr());
                cvr_anim_reload_hand_off_blend_target(0, t.as_mut_ptr(), q.as_mut_ptr());
                if cvr_cpp_fbt_solve_arm(bone_buf, 1, t.as_ptr(), q.as_ptr()) {
                    g_VRPalmModelR = t;
                } else {
                    VRIK_SolveArm(bone_buf, g_VRRightUpperArmIdx, g_VRRightForeArmIdx, g_VRRightBoneIdx, t.as_ptr(), q.as_ptr(),
                        body_right.as_ptr(), body_up.as_ptr(), body_fwd.as_ptr(), g_VRElbowPoleR * 0.017_453_292_52,
                        g_VRElbowSwingR, false, true, pose_ladder);
                    g_VRPalmModelR = g_VRIKHandPin[0];
                }
                g_VRPalmModelRotR = q;
            }
        }
        cvr_anim_wheel_publish_grab();
        holster_distances(c[2] != 0, lh_wrist_valid, &lh_wrist_model, rh_wrist_valid, &rh_wrist_model);
        if !pose_ladder && (!vrik_in_vehicle || (vrik_window_combat && vehicle_upper_solved)) {
            VRIK_ApplyCyberwarePose(bone_buf, track_buf as *mut f32, track_count);
        }
        // One completed pose feeds physics. Its targets and self-collision geometry never come from unrelated FK
        // diagnostics or a later entity transform.
        if g_VRDiagCapture != 0 || super::physhands::pose::pose_frame_pending() {
            VRIK_ComputeFK(bone_buf, VRIK_FKCount());
            if g_VRDiagCapture != 0 {
                if (0..MAXB).contains(&g_VRRightBoneIdx) {
                    g_VRIKDbgHandFK = g_fkPos[g_VRRightBoneIdx as usize];
                    g_VRIKDbgTargetTrace = g_VRIKDbgTarget;
                }
                // Whole-rig snapshot for VRBoneModelPos/Rot: the only point in the pass where the arms are SOLVED and
                // the FK is fresh.
                let n = VRIK_FKCount();
                let lim = if n < MAXB { n } else { MAXB };
                for i in 0..lim.max(0) as usize {
                    g_VRFKSnapPos[i] = g_fkPos[i];
                    g_VRFKSnapRot[i] = g_fkRot[i];
                }
                g_VRFKSnapCount = lim;
                super::shared_globals::g_VRFKSnapSeq.fetch_add(1, core::sync::atomic::Ordering::Release);
                super::frame_order::note(b'K');
            }
            super::physhands::pose::publish_pose_frame();
        }
    }
    // Passenger combat also replays its upper body in model space; ordinary seats keep the arm-only cache.
    {
        // window combat fits its pelvis to the HMD; seated driving replays upper locals so the native seat and root keep
        // ownership
        if !vrik_in_vehicle || (vrik_window_combat && vehicle_upper_solved) {
            VRIK_SyncShadowUpper(bone_buf, pose_ladder);
            VRIK_CaptureModelCache(pose_ladder);
            let same_input = solve_hand_sequence == g_handsStableSeq;
            cvr_cpp_pose_id_note(same_input, solve_pose_id);
        } else if pose_seated && vehicle_upper_solved {
            VRIK_SyncShadowUpper(bone_buf, true);
            VRIK_CaptureLocalArmCache(bone_buf, true, true, true);
        } else {
            VRIK_CaptureLocalArmCache(bone_buf, !arm_to_anim_r, !arm_to_anim_l, false);
        }
        g_solveCacheTick = tick_now;
    }
}

/// HAND-TO-HOLSTER distances [20..22], from the SOLVED drawing wrist (left-handed, the mirror).
unsafe fn holster_distances(draw_left: bool, lh_valid: bool, lh: &V3, rh_valid: bool, rh_w: &V3) {
    let sh = g_pSharedHands;
    if !(if draw_left { lh_valid } else { rh_valid }) {
        *sh.add(20) = -1.0;
        *sh.add(21) = -1.0;
        *sh.add(22) = -1.0;
        return;
    }
    let rh = if draw_left { lh } else { rh_w };
    let side = if draw_left { -1.0f32 } else { 1.0 };
    let draw_hip = if draw_left { g_VRLeftUpLegIdx } else { g_VRRightUpLegIdx };
    let cross_hip = if draw_left { g_VRRightUpLegIdx } else { g_VRLeftUpLegIdx };
    let draw_shoulder = if draw_left { g_VRLeftUpperArmIdx } else { g_VRRightUpperArmIdx };
    let mut br_: V3 = [1.0, 0.0, 0.0];
    let mut bu: V3 = [0.0, 0.0, 1.0];
    let have_shoulders = g_VRRightUpperArmIdx >= 0 && g_VRLeftUpperArmIdx >= 0 && g_VRRightUpperArmIdx < MAXB
        && g_VRLeftUpperArmIdx < MAXB;
    if have_shoulders {
        let rs = g_fkPos[g_VRRightUpperArmIdx as usize];
        let ls = g_fkPos[g_VRLeftUpperArmIdx as usize];
        br_ = [rs[0] - ls[0], rs[1] - ls[1], rs[2] - ls[2]];
        let n = sqrt(br_[0] * br_[0] + br_[1] * br_[1] + br_[2] * br_[2]);
        if n > 1e-4 {
            br_[0] /= n;
            br_[1] /= n;
            br_[2] /= n;
        }
    }
    if (0..MAXB).contains(&g_VRHeadBoneIdx) {
        let root = g_fkPos[0];
        let head = g_fkPos[g_VRHeadBoneIdx as usize];
        bu = [head[0] - root[0], head[1] - root[1], head[2] - root[2]];
        let n = sqrt(bu[0] * bu[0] + bu[1] * bu[1] + bu[2] * bu[2]);
        if n > 1e-4 {
            bu[0] /= n;
            bu[1] /= n;
            bu[2] /= n;
        }
    }
    let mut bf: V3 = [bu[1] * br_[2] - bu[2] * br_[1], bu[2] * br_[0] - bu[0] * br_[2], bu[0] * br_[1] - bu[1] * br_[0]];
    {
        let n = sqrt(bf[0] * bf[0] + bf[1] * bf[1] + bf[2] * bf[2]);
        if n > 1e-4 {
            bf[0] /= n;
            bf[1] /= n;
            bf[2] /= n;
        }
    }
    const RIGHT_OFF: f32 = 0.18;
    const LEFT_OFF: f32 = 0.05;
    let d3 = |ax: f32, ay: f32, az: f32, bx: f32, by: f32, bz: f32| -> f32 {
        let (dx, dy, dz) = (ax - bx, ay - by, az - bz);
        sqrt(dx * dx + dy * dy + dz * dz)
    };
    if (0..MAXB).contains(&draw_hip) {
        let p = g_fkPos[draw_hip as usize];
        let o = side * RIGHT_OFF;
        *sh.add(20) = d3(rh[0], rh[1], rh[2], p[0] + br_[0] * o, p[1] + br_[1] * o, p[2] + br_[2] * o);
    } else {
        *sh.add(20) = -1.0;
    }
    if (0..MAXB).contains(&cross_hip) {
        let p = g_fkPos[cross_hip as usize];
        let o = side * LEFT_OFF;
        *sh.add(21) = d3(rh[0], rh[1], rh[2], p[0] - br_[0] * o, p[1] - br_[1] * o, p[2] - br_[2] * o);
    } else {
        *sh.add(21) = -1.0;
    }
    if have_shoulders {
        let rs = g_fkPos[draw_shoulder as usize];
        const UP: f32 = 0.05;
        const BACK: f32 = 0.10;
        let d_a = d3(rh[0], rh[1], rh[2], rs[0] + bu[0] * UP - bf[0] * BACK, rs[1] + bu[1] * UP - bf[1] * BACK,
            rs[2] + bu[2] * UP - bf[2] * BACK);
        let d_b = d3(rh[0], rh[1], rh[2], rs[0] + bu[0] * UP + bf[0] * BACK, rs[1] + bu[1] * UP + bf[1] * BACK,
            rs[2] + bu[2] * UP + bf[2] * BACK);
        *sh.add(22) = if d_a < d_b { d_a } else { d_b };
    } else {
        *sh.add(22) = -1.0;
    }
}

/// The calibrated shoulder written into the rig (the fallback without a camera), and the FK refreshed.
unsafe fn apply_shoulder_anchor(bone_buf: *mut u8, upper_idx: i32, sh: &V3) {
    let parent = if (0..MAXB).contains(&upper_idx) { g_VRBoneParent[upper_idx as usize] as i32 } else { -1 };
    if (0..MAXB).contains(&parent) {
        VRIK_WriteLocalPos(bone_buf, upper_idx, g_fkPos[parent as usize].as_ptr(), g_fkRot[parent as usize].as_ptr(),
                           sh.as_ptr());
    }
    g_fkPos[upper_idx as usize] = *sh;
    VRIK_ComputeFK(bone_buf, VRIK_FKCount());
}