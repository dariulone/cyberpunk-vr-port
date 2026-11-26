//! The solve's own state: what src/Anim/CharacterRig.cpp kept in its anonymous namespace and function statics. Only
//! the player pose path touches it, under g_PlayerPoseStateLock (the neck mount and the full-body reference are also
//! read from other threads, under NECK_MOUNT_LOCK, as the C++ did under s_neckMountMutex).
#![allow(non_upper_case_globals, dead_code)]
use super::MAX_BONES;
use crate::vrik::types::{BendLegRestore, NativeTorso, SeatedPoseRestore, ShoulderBase, VrikFbtReference};
use cvr_camera::anchor::AnchorVector;
use std::sync::Mutex;

pub static mut s_referenceValid: [bool; MAX_BONES] = [false; MAX_BONES];
pub static mut s_referenceLocalRot: [[f32; 4]; MAX_BONES] = [[0.0; 4]; MAX_BONES];
pub static mut s_referenceLocalPos: [[f32; 3]; MAX_BONES] = [[0.0; 3]; MAX_BONES];
pub static mut s_referenceModelRot: [[f32; 4]; MAX_BONES] = [[0.0; 4]; MAX_BONES];
pub static mut s_referenceModelPos: [[f32; 3]; MAX_BONES] = [[0.0; 3]; MAX_BONES];
pub static mut s_referenceCameraHeight: f32 = 1.6;
pub static mut s_referenceCameraValid: bool = false;
/// Torso_fppCamera_Control_GRP: the bone the game's camera rides.
pub static mut s_referenceCameraBone: i32 = -1;
pub static mut s_nativeTorso: [NativeTorso; 4] = [NativeTorso::ZERO; 4];
pub static mut s_nativeTorsoNext: i32 = 0;
pub static mut s_clipBodyWeight: f32 = 0.0;
pub static mut s_clipBodyAtUs: i64 = 0;
/// The standing body's hips less the clip's, xy; a standing value until a move.
pub static mut s_clipOffset: [f32; 2] = [0.0, 0.195];
pub static mut s_clipLatched: bool = false;
pub static NECK_MOUNT_LOCK: Mutex<()> = Mutex::new(());
pub static mut s_neckMount: AnchorVector = AnchorVector { x: 0.0, y: 0.0, z: 0.0 };
pub static mut s_neckCameraEyeOffset: AnchorVector = AnchorVector { x: 0.0, y: 0.0, z: 0.0 };
pub static mut s_neckMountValid: bool = false;
pub static mut s_neutralEyeModel: [f32; 3] = [0.0; 3];
pub static mut s_eyeInHead: [f32; 3] = [0.0; 3];
pub static mut s_fbtReference: VrikFbtReference = VrikFbtReference::ZERO;
pub static mut s_fbtRigPath: [[u8; 260]; 2] = [[0; 260]; 2];
pub static mut s_fbtDumped: Vec<i32> = Vec::new();
pub static mut s_holdBone: [bool; MAX_BONES] = [false; MAX_BONES];
pub static mut s_holdKeepModel: [bool; MAX_BONES] = [false; MAX_BONES];
pub static mut s_holdCount: i32 = 0;
pub static mut s_holdKeepPos: [[f32; 3]; MAX_BONES] = [[0.0; 3]; MAX_BONES];
pub static mut s_holdKeepRot: [[f32; 4]; MAX_BONES] = [[0.0; 4]; MAX_BONES];
pub static mut s_eyeReferenceValid: bool = false;
pub static mut s_ladderFingers: [[i32; 19]; 2] = [[0; 19]; 2];
pub static mut s_scapula: [[i32; 4]; 2] = [[0; 4]; 2];
pub static mut s_scapulaCount: [i32; 2] = [0; 2];
pub static mut s_upperSkin: [[i32; 2]; 2] = [[-1, -1], [-1, -1]];
pub static mut s_shoulderBase: [ShoulderBase; 2] = [ShoulderBase::DEFAULT; 2];
pub static mut s_modelCachePos: [[f32; 3]; MAX_BONES] = [[0.0; 3]; MAX_BONES];
pub static mut s_modelCacheRot: [[f32; 4]; MAX_BONES] = [[0.0; 4]; MAX_BONES];
pub static mut s_modelCacheOwned: [bool; MAX_BONES] = [false; MAX_BONES];
pub static mut s_solveCacheCapturedMs: u64 = 0;
pub static mut s_rigFingerprint: u64 = 0;
pub static mut s_rigRevision: u64 = 0;
pub static mut s_bendLegOwned: [bool; MAX_BONES] = [false; MAX_BONES];
pub static mut s_bendLegShadowSource: [i32; MAX_BONES] = [0; MAX_BONES];
pub static mut s_bendLegRestore: [BendLegRestore; 8] = [BendLegRestore::ZERO; 8];
pub static mut s_bendRestoreCursor: u32 = 0;
/// The seated upper body's restore ring (main's cccb014f7), its cursor, and whether the local cache holds it.
pub static mut s_seatedRestore: [SeatedPoseRestore; 8] = [SeatedPoseRestore::ZERO; 8];
pub static mut s_seatedRestoreCursor: u32 = 0;
pub static mut s_localCacheSeated: bool = false;
/// Each foot's toe bone, found by name under its own foot; [0] right, [1] left.
pub static mut s_toeBone: [i32; 2] = [-1, -1];
/// Rig policy's neutral posture scratch (static rather than stack: VRIK_MAX_BONES entries).
pub static mut s_neutralPos: [[f32; 3]; MAX_BONES] = [[0.0; 3]; MAX_BONES];
pub static mut s_neutralRot: [[f32; 4]; MAX_BONES] = [[0.0; 4]; MAX_BONES];

// ---- what CharacterRig.cpp gave external linkage but nothing else reads
/// R, L: that hand was in a grip on the last solve...
pub static mut g_VRIKGrip: [bool; 2] = [false; 2];
/// ...and its wrist's shortfall, metres (negative = slack).
pub static mut g_VRIKShort: [f32; 2] = [0.0; 2];
/// Where this solve's thorax turn carried the LEFT shoulder, model space.
pub static mut g_VRIKTorsoShift: [f32; 3] = [0.0; 3];
/// The turn's two rotations, model space, lower bone first...
pub static mut g_VRIKTorsoQ: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 1.0]; 2];
/// ...and the point each one turns about.
pub static mut g_VRIKTorsoP: [[f32; 3]; 2] = [[0.0; 3]; 2];
/// The two spine bones the turn is written on, for the replay cache.
pub static mut g_VRIKTorsoBones: [i32; 2] = [-1, -1];
pub static mut g_VRCyberGraphHashes: [u64; 16] = [0; 16];
pub static mut g_VRCyberGraphCalls: [u64; 16] = [0; 16];
/// Upper-arm REST local translation per side (0 right, 1 left), for the shoulder protraction.
pub static mut g_vrikUpArmRest: [[f32; 3]; 2] = [[0.0; 3]; 2];
pub static mut g_vrikUpArmRestCap: [bool; 2] = [false; 2];

// ---- function statics
pub static mut s_turn: f32 = 0.0; // VRIK_GripTorso
pub static mut s_gripTorsoFreq: i64 = 0;
pub static mut s_gripTorsoPrev: i64 = 0;
pub static mut s_wristEase: [f32; 2] = [0.0; 2]; // WristHeldEase
pub static mut s_wristAt: [i64; 2] = [0; 2];
pub static mut s_armRestTrackBuf: usize = 0; // VRIK_ArmRestTrans
pub static mut s_armRestBoneCount: i32 = 0;
pub static mut s_armRestValid: [bool; MAX_BONES] = [false; MAX_BONES];
pub static mut s_armRestTrans: [[f32; 3]; MAX_BONES] = [[0.0; 3]; MAX_BONES];
pub static mut s_squatEMA: f32 = 0.0;
pub static mut s_squatInit: bool = false;
pub static mut s_bendLimitW: f32 = 0.0;
pub static mut s_bendEased: f32 = 0.0;
pub static mut s_bendLimitUs: i64 = 0;
pub static mut s_stanceMotion: f32 = 0.0; // SymmetricStance
pub static mut s_stancePrev: [[f32; 2]; 2] = [[0.0; 2]; 2];
pub static mut s_stancePrevValid: bool = false;
pub static mut s_stancePrevTime: i64 = 0;

/// The arm solve's statics (VRIK_SolveArm), per side.
pub struct ArmStatics {
    pub rest_up_len: [f32; 2],
    pub rest_fore_len: [f32; 2],
    pub length_revision: [u64; 2],
    pub ax_loc_up: [[f32; 3]; 2],
    pub hg_loc_up: [[f32; 3]; 2],
    pub ax_loc_fore: [[f32; 3]; 2],
    pub hg_loc_fore: [[f32; 3]; 2],
    pub palm_loc: [[f32; 3]; 2],
    pub arm_rest: [[f32; 3]; 2],
    pub bend_rest: [[f32; 3]; 2],
    pub rig_cap: [bool; 2],
    pub axis_revision: [u64; 2],
    pub y_sm: [[f32; 3]; 2],
    pub y_init: [bool; 2],
    pub y_t: [i64; 2],
    pub y_revision: [u64; 2],
    pub y_blend: [f32; 2],
}
pub static mut ARM: ArmStatics = ArmStatics {
    rest_up_len: [0.0; 2],
    rest_fore_len: [0.0; 2],
    length_revision: [0; 2],
    ax_loc_up: [[0.0; 3]; 2],
    hg_loc_up: [[0.0; 3]; 2],
    ax_loc_fore: [[0.0; 3]; 2],
    hg_loc_fore: [[0.0; 3]; 2],
    palm_loc: [[0.0; 3]; 2],
    arm_rest: [[0.0; 3]; 2],
    bend_rest: [[0.0; 3]; 2],
    rig_cap: [false; 2],
    axis_revision: [0; 2],
    y_sm: [[0.0; 3]; 2],
    y_init: [false; 2],
    y_t: [0; 2],
    y_revision: [0; 2],
    y_blend: [-1.0; 2],
};

/// VRIK_NoteShake's history per hand and stage.
#[derive(Clone, Copy)]
pub struct ShakeHist {
    pub p1: [f32; 3],
    pub p2: [f32; 3],
    pub n: i32,
}
pub static mut g_vrikShakeHist: [[ShakeHist; 4]; 2] = [[ShakeHist { p1: [0.0; 3], p2: [0.0; 3], n: 0 }; 4]; 2];
