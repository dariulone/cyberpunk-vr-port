//! The solve's record types (formerly in src/Anim/CharacterRig.cpp and include/Anim/CharacterRig.hpp).
use core::ptr::null_mut;

/// V's reference pose for full-body tracking calibration (CharacterRig.hpp's VrikFbtReference, read by
/// Fbt/Game/Calibration.cpp through VRIK_ReadFbtReference): model space, game axes, metres. [0] left, [1] right.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VrikFbtReference {
    pub valid: bool,
    pub female: bool,
    pub eye: [f32; 3],
    pub eyeInHead: [f32; 3],
    pub hipsPos: [f32; 3],
    pub hipsRot: [f32; 4],
    pub headPos: [f32; 3],
    pub headRot: [f32; 4],
    pub footPos: [[f32; 3]; 2],
    pub footRot: [[f32; 4]; 2],
    pub upperArmPos: [[f32; 3]; 2],
    pub foreArmPos: [[f32; 3]; 2],
    pub handPos: [[f32; 3]; 2],
}
const _: () = assert!(core::mem::size_of::<VrikFbtReference>() == 212);
impl VrikFbtReference {
    pub const ZERO: Self = Self {
        valid: false,
        female: false,
        eye: [0.0; 3],
        eyeInHead: [0.0; 3],
        hipsPos: [0.0; 3],
        hipsRot: [0.0, 0.0, 0.0, 1.0],
        headPos: [0.0; 3],
        headRot: [0.0, 0.0, 0.0, 1.0],
        footPos: [[0.0; 3]; 2],
        footRot: [[0.0, 0.0, 0.0, 1.0]; 2],
        upperArmPos: [[0.0; 3]; 2],
        foreArmPos: [[0.0; 3]; 2],
        handPos: [[0.0; 3]; 2],
    };
}

/// THE CLIP'S OWN TORSO (VRIK_PlaceBodyUnderHMD, the slide): the hips and spine locals the native pose brought, read
/// from a buffer that was not this solve's own output, and the clip's camera bone in model space beside them.
#[derive(Clone, Copy)]
pub struct NativeTorso {
    pub buffer: *const u8,
    pub revision: u64,
    pub count: i32,
    pub bones: [i32; 12],
    pub native: [[u8; 48]; 12],
    pub solved: [[u8; 48]; 12],
    pub camera: [f32; 3],
    pub hips: [f32; 3],
}
impl NativeTorso {
    pub const ZERO: Self = Self {
        buffer: core::ptr::null(),
        revision: 0,
        count: 0,
        bones: [0; 12],
        native: [[0; 48]; 12],
        solved: [[0; 48]; 12],
        camera: [0.0; 3],
        hips: [0.0; 3],
    };
}

#[derive(Clone, Copy)]
pub struct ShoulderBase {
    pub buffer: *mut u8,
    pub revision: u64,
    pub upper: i32,
    pub clavicle: i32,
    pub clavicleLocalRot: [f32; 4],
    pub upperLocalPos: [f32; 3],
}
impl ShoulderBase {
    pub const DEFAULT: Self =
        Self { buffer: null_mut(), revision: 0, upper: -1, clavicle: -1, clavicleLocalRot: [0.0; 4], upperLocalPos: [0.0; 3] };
}

/// Six leg joints and their shadows as the animation had them and as we left them; Combat also saves the pelvis and
/// its shadow.
#[derive(Clone, Copy)]
pub struct BendLegRestore {
    pub buffer: *mut u8,
    pub revision: u64,
    pub vehicle: bool,
    pub count: i32,
    pub bones: [i32; 14],
    pub native: [[f32; 12]; 14],
    pub solved: [[f32; 12]; 14],
}
impl BendLegRestore {
    pub const ZERO: Self =
        Self { buffer: null_mut(), revision: 0, vehicle: false, count: 0, bones: [0; 14], native: [[0.0; 12]; 14], solved: [[0.0; 12]; 14] };
}

/// The seated upper body (spine, neck, head, the arms and their shadows) as the animation had it and as we left it.
#[derive(Clone, Copy)]
pub struct SeatedPoseRestore {
    pub buffer: *mut u8,
    pub revision: u64,
    pub count: i32,
    pub bones: [i32; 64],
    pub native: [[f32; 12]; 64],
    pub solved: [[f32; 12]; 64],
}
impl SeatedPoseRestore {
    pub const ZERO: Self = Self { buffer: null_mut(), revision: 0, count: 0, bones: [0; 64], native: [[0.0; 12]; 64], solved: [[0.0; 12]; 64] };
}

/// The torso path while fitting a bend.
#[derive(Clone, Copy)]
pub struct VrikTorsoCandidate {
    pub bones: [i32; 12],
    pub count: i32,
    pub positions: [[f32; 3]; 12],
    pub rotations: [[f32; 4]; 12],
    pub eye: [f32; 3],
    pub valid: bool,
}
impl VrikTorsoCandidate {
    pub const ZERO: Self =
        Self { bones: [0; 12], count: 0, positions: [[0.0; 3]; 12], rotations: [[0.0; 4]; 12], eye: [0.0; 3], valid: false };
}

/// The legs as the animation has them, read before anything moves the hips.
#[derive(Clone, Copy)]
pub struct NativeLegs {
    pub valid: bool,
    pub foot: [[f32; 3]; 2],
    pub footRot: [[f32; 4]; 2],
    pub pole: [[f32; 3]; 2],
    pub hips: [f32; 3],
    pub toe: [[f32; 3]; 2],
    pub toeValid: [bool; 2],
}
impl NativeLegs {
    pub const ZERO: Self =
        Self { valid: false, foot: [[0.0; 3]; 2], footRot: [[0.0; 4]; 2], pole: [[0.0; 3]; 2], hips: [0.0; 3], toe: [[0.0; 3]; 2], toeValid: [false; 2] };
}

/// CharacterRig.hpp's `enum class VrikCameraStatus { Unavailable, Valid, Invalidated }`.
pub const CAMERA_UNAVAILABLE: i32 = 0;
pub const CAMERA_VALID: i32 = 1;
pub const CAMERA_INVALIDATED: i32 = 2;
