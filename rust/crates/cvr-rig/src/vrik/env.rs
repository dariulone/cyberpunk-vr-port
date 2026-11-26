//! What the solve shares with the C++ half: the globals it defines (include/Anim/VrikState.hpp, CharacterRig.hpp,
//! Camera/CameraState.hpp, Core/VrCoreShared.hpp; given C linkage for this, defined in src/Natives, src/Anim/
//! CharacterRigState.cpp, src/Anim/HandSnapshot.cpp and the live controls) and the few C++ functions it calls
//! (src/Anim/CharacterRigRust.cpp forwards the ones that are C++ members or namespaced).
//!
//! The declarators follow the definitions: `volatile int` is i32, `int16_t[N]` is [i16; N], C++ bool is Rust bool
//! (one byte, 0 or 1).
#![allow(non_upper_case_globals, non_snake_case, dead_code)]
use super::MAX_BONES;
use core::ffi::c_char;

/// One coherent camera/entity publication (VrikState.hpp's VrikTransformSnapshot).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VrikTransformSnapshot {
    pub camQuat: [f32; 4],
    pub entityQuat: [f32; 4],
    pub cameraMinusEntity: [f32; 3],
    pub valid: u32,
    pub bodyCameraMinusEntity: [f32; 3],
    pub unavailable: u32,
}
const _: () = assert!(core::mem::size_of::<VrikTransformSnapshot>() == 64);

extern "C" {
    // ---- the rig as the natives resolved it (src/Natives)
    pub static mut g_VRBoneParent: [i16; MAX_BONES];
    pub static mut g_VRBoneCount: i32;
    pub static mut g_VRFKCount: i32;
    pub static mut g_VRHipsIdx: i32;
    pub static mut g_VRNeckIdx: i32;
    pub static mut g_VRNeck1Idx: i32;
    pub static mut g_VRHeadBoneIdx: i32;
    pub static mut g_VRRightUpLegIdx: i32;
    pub static mut g_VRRightLegIdx: i32;
    pub static mut g_VRRightFootIdx: i32;
    pub static mut g_VRLeftUpLegIdx: i32;
    pub static mut g_VRLeftLegIdx: i32;
    pub static mut g_VRLeftFootIdx: i32;
    pub static mut g_VRRightUpperArmIdx: i32;
    pub static mut g_VRRightForeArmIdx: i32;
    pub static mut g_VRRightBoneIdx: i32;
    pub static mut g_VRLeftUpperArmIdx: i32;
    pub static mut g_VRLeftForeArmIdx: i32;
    pub static mut g_VRLeftBoneIdx: i32;
    pub static mut g_VRForeTwistR: [i32; 3];
    pub static mut g_VRForeTwistL: [i32; 3];
    pub static mut g_VRSpineIdx: [i32; 8];
    pub static mut g_VRSpineCount: i32;

    // ---- the bind and its live settings
    pub static mut g_pSharedHands: *mut f32;
    pub static mut g_VRBind: i32;
    pub static mut g_VRBindScale: f32;
    pub static mut g_VRBindOffX: f32;
    pub static mut g_VRBindOffY: f32;
    pub static mut g_VRBindOffZ: f32;
    pub static mut g_VRBindAxis: i32;
    pub static mut g_VRUseHeadRelative: i32;
    pub static mut g_VRDiagCapture: i32;
    pub static mut g_VRUserArmLenR: f32;
    pub static mut g_VRUserArmLenL: f32;
    pub static mut g_VRSquatThreshold: f32;
    pub static mut g_VRLocomotionState: i32;
    pub static mut g_VRLocomotionDetailed: i32;

    // ---- the camera and entity pushed from Lua (legacy scalar mirrors)
    pub static mut g_VRCamI: f32;
    pub static mut g_VRCamJ: f32;
    pub static mut g_VRCamK: f32;
    pub static mut g_VRCamR: f32;
    pub static mut g_VRCamPosX: f32;
    pub static mut g_VRCamPosY: f32;
    pub static mut g_VRCamPosZ: f32;
    pub static mut g_VREntityPosX: f32;
    pub static mut g_VREntityPosY: f32;
    pub static mut g_VREntityPosZ: f32;
    pub static mut g_VRCamPosValid: i32;
    pub static mut g_VREntityQI: f32;
    pub static mut g_VREntityQJ: f32;
    pub static mut g_VREntityQK: f32;
    pub static mut g_VREntityQR: f32;
    pub static mut g_VRCamPairLocalX: f32;
    pub static mut g_VRCamPairLocalY: f32;
    pub static mut g_VRCamPairLocalZ: f32;
    pub static mut g_VRCamPairValid: i32;

    // ---- the camera writer's state (Camera/CameraState.hpp, Core/VrCoreShared.hpp)
    pub static mut g_headingPitchS: f32;
    pub static mut g_headingPitchC: f32;
    pub static mut g_headingValid: u32;
    pub static mut g_isInVehicle: bool;

    // ---- probes the natives read
    pub static mut g_VRBodyBone: [[f32; 3]; 11];
    pub static mut g_VRBodyBoneOk: [i32; 11];
    pub static mut g_VRIKDbgChest: [f32; 3];
    pub static mut g_VRIKDbgChestTgt: [f32; 3];
    pub static mut g_VRIKDbgHipsYaw: f32;
    pub static mut g_VRIKDbgShModel: [f32; 3];
    pub static mut g_VRIKDbgClav: [[f32; 8]; 2];
    pub static mut g_VRIKDbgTarget: [f32; 3];
    pub static mut g_VRIKDbgShoulder: [f32; 3];
    pub static mut g_VRIKDbgElbow: [f32; 3];
    pub static mut g_VRIKDbgLens: [f32; 2];
    pub static mut g_VRIKDbgLocal: [f32; 4];
    pub static mut g_VRIKDbgTargetL: [f32; 3];
    pub static mut g_VRIKDbgShoulderL: [f32; 3];
    pub static mut g_VRIKDbgElbowL: [f32; 3];
    pub static mut g_VRIKDbgLensL: [f32; 2];
    pub static mut g_VRIKDbgLocalL: [f32; 4];

    // ---- the solve's own shared state (src/Anim/CharacterRigState.cpp)
    pub static mut g_fkPos: [[f32; 3]; MAX_BONES];
    pub static mut g_fkRot: [[f32; 4]; MAX_BONES];
    pub static mut g_VRFKSnapPos: [[f32; 3]; MAX_BONES];
    pub static mut g_VRFKSnapRot: [[f32; 4]; MAX_BONES];
    pub static mut g_VRFKSnapCount: i32;
    pub static mut g_VRUpperOwned: [bool; MAX_BONES];
    pub static mut g_VRShadowSource: [i32; MAX_BONES];
    pub static mut g_VRHeadReferenceValid: bool;
    pub static mut g_VRHeadReferenceModelRot: [f32; 4];
    pub static mut g_VRIKVehicleUpperActive: bool;
    pub static mut g_solveCacheModel: bool;
    pub static mut g_solveCacheTick: u32;
    pub static mut g_solveCacheN: i32;
    pub static mut g_solveCacheIdx: [i32; MAX_BONES];
    pub static mut g_solveCacheVal: [[f32; 7]; MAX_BONES];
    pub static mut g_VRIKBodyBendAngle: f32;
    pub static mut g_VRIKBodyBendPelvis: [f32; 3];
    pub static mut g_VRIKBodyEyeTarget: [f32; 3];
    pub static mut g_VRIKBodyEyeSolved: [f32; 3];
    pub static mut g_VRIKBodyEyeBound: bool;
    pub static mut g_VRIKGirdleReach: [[f32; 4]; 2];
    pub static mut g_VRIKHandPin: [[f32; 3]; 2];
    pub static mut g_VRIKHandAim: [[f32; 8]; 2];
    pub static mut g_VRIKTorsoOn: bool;
    pub static mut g_VRCyberwareFamily: i32;
    pub static mut g_VRHandStopModel: [[f32; 3]; 2];
    pub static mut g_VRHandStopValid: [i32; 2];
    pub static mut g_VRHandStopDeadband: f32;
    pub static mut g_VRHandRotLock: [[f32; 4]; 2];
    pub static mut g_VRHandRotLockOn: [i32; 2];
    pub static mut g_VRHandRawModel: [[f32; 3]; 2];
    pub static mut g_VRHandRawValid: [i32; 2];
    pub static mut g_VRHandRawRot: [[f32; 4]; 2];
    pub static mut g_VRBendLimitClockUs: Option<unsafe extern "C" fn() -> i64>;
    /// The hands' and the solve's counter for tests (ticks at 1 MHz); null = QueryPerformanceCounter.
    pub static g_VRIKClockQpc: Option<unsafe extern "C" fn() -> i64>;
    pub static mut g_VrikShakePeakMm: [[f32; 4]; 2];
    pub static mut g_VrikShakeSlowMm: [[f32; 4]; 2];
    pub static mut g_VrikShakeSlowN: [[i32; 4]; 2];
    pub static mut s_vrSharedSquatDrop: f32;
    pub static mut g_viewPkt: [f32; 17];
    pub static mut g_viewPktValid: bool;
    pub static mut g_handsStableSeq: u32;
    pub static mut g_handsStableValid: bool;
    pub static mut g_vrikViewScaleUsed: f32;
    pub static mut g_VRPlayerYaw: f32;

    // ---- live controls and probes (extern "C" in C++ already)
    pub static mut CyberpunkVR_GripReach: i32;
    pub static mut CyberpunkVR_SquatKneeMode: i32;
    pub static mut CyberpunkVR_SquatFeetUnderHips: i32;
    pub static mut CyberpunkVR_SquatSymmetric: i32;
    pub static mut CyberpunkVR_DebugSquatStance: [f32; 2];
    pub static mut CyberpunkVR_DebugToes: [[f32; 3]; 2];
    pub static mut CyberpunkVR_GripProtract: f32;
    pub static mut CyberpunkVR_GripTorsoMaxDeg: f32;
    pub static mut CyberpunkVR_DebugGripShortMm: [f32; 2];
    pub static mut CyberpunkVR_DebugGripTorsoDeg: f32;
    pub static mut CyberpunkVR_DebugGripReach: [f32; 6];
    pub static mut CyberpunkVR_ClipBodyMoves: i32;
    pub static mut CyberpunkVR_DebugClipBody: [f32; 12];
    pub static mut CyberpunkVR_NativeCyberwarePasses: i32;
    pub static mut CyberpunkVR_TwoHandActive: i32;
    pub static mut CyberpunkVR_VrikElbowPolicy: i32;
    pub static mut CyberpunkVR_CamComposeAtWrite: i32;
    pub static mut CyberpunkVR_CamWriteInPatch: i32;
    pub static mut CyberpunkVR_HeadTranslationInPatch: i32;
    pub static mut CyberpunkVR_HeadingFromPreWrite: i32;
    pub static mut CyberpunkVR_EngineBodyYawValid: i32;
    pub static mut CyberpunkVR_ViewYawFromEngine: i32;
    pub static mut CyberpunkVR_EngineBodyYawZ: f32;
    pub static mut CyberpunkVR_EngineBodyYawW: f32;
    pub static mut CyberpunkVR_VrikTransformsFromPlugin: i32;
    pub static mut CyberpunkVR_VrikNativeFramePair: i32;
    pub static mut CyberpunkVR_VrikVehicleFullEntityQuat: i32;
    pub static mut CyberpunkVR_DebugVrikNativePairUsed: u64;
    pub static mut CyberpunkVR_DebugVrikLuaPairFallback: u64;
    pub static mut CyberpunkVR_RuntimeDiagnostics: i32;

    // ---- C++ functions (C linkage)
    pub fn VRIK_ReadTransformSnapshot(out: *mut VrikTransformSnapshot) -> bool;
    pub fn VRIK_ReadNativeTransformSnapshot(out: *mut VrikTransformSnapshot) -> bool;
    // src/Anim/CharacterRigRust.cpp: the C++ members and namespaced functions
    pub fn cvr_cpp_vrik_frame_head_orientation(out: *mut f32) -> bool;
    pub fn cvr_cpp_vrik_body_tracking_yaw(native_yaw: f32) -> f32;
    pub fn cvr_cpp_vrik_fbt_body_active() -> bool;
    pub fn cvr_cpp_vrik_log(text: *const c_char);
    pub fn cvr_cpp_vrik_config_file(name: *const c_char, out: *mut c_char, size: usize) -> bool;
    pub fn cvr_cpp_vrik_snap_trace(line: *const c_char);
}

/// `count` floats of engine memory copied out through cvr-engine's guarded copy, float by float, stopping at the first
/// that faults (formerly src/Utils/MemorySafeC.cpp's, over MemorySafe.hpp's ReadFloatSafe); false then, or for null
/// pointers.
///
/// # Safety
/// `out` holds `count` floats.
pub unsafe fn cvr_cpp_read_floats_safe(src: *const f32, out: *mut f32, count: usize) -> bool {
    if src.is_null() || out.is_null() {
        return false;
    }
    for i in 0..count {
        match cvr_engine::code::read::<f32>(src.wrapping_add(i) as usize) {
            // SAFETY: as documented.
            Some(value) => unsafe { *out.add(i) = value },
            None => return false,
        }
    }
    true
}
/// `count` floats written to engine memory the same way (WriteFloatSafe), stopping at the first that faults.
///
/// # Safety
/// `values` holds `count` floats.
pub unsafe fn cvr_cpp_write_floats_safe(dst: *mut f32, values: *const f32, count: usize) -> bool {
    if dst.is_null() || values.is_null() {
        return false;
    }
    for i in 0..count {
        // SAFETY: as documented.
        if !cvr_engine::code::write::<f32>(dst.wrapping_add(i) as usize, unsafe { *values.add(i) }) {
            return false;
        }
    }
    true
}

#[link(name = "kernel32")]
extern "system" {
    pub fn QueryPerformanceCounter(count: *mut i64) -> i32;
    pub fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
    pub fn GetTickCount64() -> u64;
    fn MultiByteToWideChar(code_page: u32, flags: u32, text: *const c_char, bytes: i32, wide: *mut u16, chars: i32) -> i32;
}

/// A path the C++ half made as an ANSI string (cvr::paths::FileA), as fopen reads it: through the ANSI code page.
pub unsafe fn ansi_path(text: *const c_char) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    const CP_ACP: u32 = 0;
    let n = MultiByteToWideChar(CP_ACP, 0, text, -1, core::ptr::null_mut(), 0);
    if n <= 1 {
        return None;
    }
    let mut wide = vec![0u16; n as usize];
    if MultiByteToWideChar(CP_ACP, 0, text, -1, wide.as_mut_ptr(), n) != n {
        return None;
    }
    wide.pop(); // the terminator
    Some(std::ffi::OsString::from_wide(&wide).into())
}

/// The QPC reading and frequency, as LARGE_INTEGER QuadParts.
/// ...or the tests' counter (g_VRIKClockQpc, ticks at 1 MHz) when they set one.
pub fn qpc() -> (i64, i64) {
    // SAFETY: a function pointer the tests set before the trace and clear after it.
    if let Some(clock) = unsafe { g_VRIKClockQpc } {
        return (unsafe { clock() }, 1_000_000);
    }
    let (mut now, mut freq) = (0i64, 0i64);
    // SAFETY: plain Win32 calls writing two locals.
    unsafe {
        QueryPerformanceCounter(&mut now);
        QueryPerformanceFrequency(&mut freq);
    }
    (now, freq)
}

/// `Log(...)`, formatted on this side.
pub fn log(text: &str) {
    let mut bytes = Vec::with_capacity(text.len() + 1);
    bytes.extend(text.bytes().filter(|&b| b != 0));
    bytes.push(0);
    // SAFETY: a NUL-terminated string the call only reads.
    unsafe { cvr_cpp_vrik_log(bytes.as_ptr() as *const c_char) }
}

/// `std::chrono::steady_clock::now()` as MSVC implements it (QPC): whole seconds and the remainder scaled
/// separately (no overflow), in nanoseconds.
pub fn steady_ns() -> i64 {
    let (now, freq) = qpc();
    let whole = (now / freq) * 1_000_000_000;
    let part = (now % freq) * 1_000_000_000 / freq;
    whole + part
}
/// ...as `duration_cast<microseconds>` of it (truncating).
pub fn steady_us() -> i64 {
    steady_ns() / 1000
}
