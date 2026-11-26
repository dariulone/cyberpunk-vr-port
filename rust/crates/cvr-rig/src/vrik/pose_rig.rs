//! The pose and rig natives (formerly src/Natives/PoseRig.cpp), registered by rust/crates/cvr-ffi/src/natives.rs: which
//! bone of a weapon rig a script may write, in what space (the write queue the pose hook applies), the rig
//! identification (signatures, censuses, pairs), the recorded-pose replay the reload module drives the hands with, the
//! rest and two-hand captures, and the scanner HUD's layout.
use super::snapshot::VRIK_IsReadable;
use core::ffi::{c_char, c_void, CStr};
use cvr_body::cmath::sqrt;
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::handle::Handle;
use red4ext_abi::stack::StackFrame;

type Frame = StackFrame;

/// Anim/VrikState.hpp's table sizes.
const RIG_N: usize = 3;
const RIG_WRITES: usize = 32;
const RIG_TRACKS: usize = 16;
const SIG_MAX: usize = 128;
const SIG_NAMES: usize = 4;
const SMALL_MAX: usize = 16;
const PAIR_MAX: usize = 24;
const CENSUS_MAX: usize = 96;
const PART_N: usize = 9;
/// Utils/SharedSlots.hpp: the world map's open flag (a dedicated slot), and kDeviceScreenOpen.
const SLOT_MENU_OPEN: usize = 81;
const SLOT_DEVICE_SCREEN: usize = 164;

#[allow(non_upper_case_globals)]
extern "C" {
    static mut g_CalibrationBoneIndex: i32;
    static mut g_pSharedHands: *mut f32;
    static mut CyberpunkVR_ScannerSlots: [f32; 21];
    // The pair and small-rig tables (Anim/VrikState.hpp).
    static mut g_PairN: i32;
    static mut g_PairFull: i32;
    static mut g_PairBones: [u32; PAIR_MAX];
    static mut g_PairTracks: [u32; PAIR_MAX];
    static mut g_PairOut: [u32; PAIR_MAX];
    static mut g_PairIn: [u32; PAIR_MAX];
    static mut g_PairBuf: [usize; PAIR_MAX];
    static mut g_SmallN: i32;
    static mut g_SmallFull: i32;
    static mut g_SmallBuf: [usize; SMALL_MAX];
    static mut g_SmallA4: [u32; SMALL_MAX];
    static mut g_SmallBones: [u32; SMALL_MAX];
    static mut g_SmallOut: [u32; SMALL_MAX];
    static mut g_SmallIn: [u32; SMALL_MAX];
    // The weapon rigs and their write queue.
    static mut g_RigSeen: [u32; RIG_N];
    static mut g_RigBones: [u32; RIG_N];
    static mut g_RigTracks: [u32; RIG_N];
    static mut g_RigBuf: [usize; RIG_N];
    static mut g_RigPose: [[[f32; 7]; 20]; RIG_N];
    static mut g_RigPoseHave: [i32; RIG_N];
    static mut g_RigTrackHave: [i32; RIG_N];
    static mut g_RigTrackOn: [[i32; RIG_TRACKS]; RIG_N];
    static mut g_RigTrackSet: [[f32; RIG_TRACKS]; RIG_N];
    static mut g_RigTrackVal: [[f32; RIG_TRACKS]; RIG_N];
    static mut g_RigPassSeen: [u32; 4];
    static mut g_RigPassRn: [i32; 4];
    static mut g_RigPassDst: [i32; 4];
    static mut g_RigPassSlots: [i32; 4];
    static mut g_RigPassBufLo: [i32; 4];
    static mut g_RigPassMap0: [i32; 8];
    static mut g_RigSigN: i32;
    static mut g_RigSigWhich: [i32; SIG_MAX];
    static mut g_RigSigBones: [u32; SIG_MAX];
    static mut g_RigSigIdx: [[i32; SIG_NAMES]; SIG_MAX];
    static mut g_RigSigHash: [[u64; SIG_NAMES]; SIG_MAX];
    static mut g_RigWriteN: i32;
    static mut g_RigWriteWhich: [i32; RIG_WRITES];
    static mut g_RigWriteBone: [i32; RIG_WRITES];
    static mut g_RigWriteOff: [[f32; 3]; RIG_WRITES];
    static mut g_RigWriteBase: [[f32; 3]; RIG_WRITES];
    static mut g_RigWriteHaveBase: [i32; RIG_WRITES];
    static mut g_RigWriteApplied: [u32; RIG_WRITES];
    static mut g_RigWriteSlot: [i32; RIG_WRITES];
    static mut g_RigWritePin: [i32; RIG_WRITES];
    static mut g_RigWriteScale: [f32; RIG_WRITES];
    static mut g_RigWriteAbs: [i32; RIG_WRITES];
    static mut g_RigWriteQuat: [[f32; 4]; RIG_WRITES];
    static mut g_RigWriteQuatOn: [i32; RIG_WRITES];
    static mut g_RigWriteRotAxis: [[f32; 3]; RIG_WRITES];
    static mut g_RigWriteRotAngle: [f32; RIG_WRITES];
    static mut g_RigWriteHaveBaseRot: [i32; RIG_WRITES];
    static mut g_RigWriteBaseRot: [[f32; 4]; RIG_WRITES];
    static mut g_RigWriteEnabled: [i32; RIG_WRITES];
    // The pose census.
    static mut g_PoseCensusOn: i32;
    static mut g_PoseCensusN: i32;
    static mut g_PoseCensusFull: i32;
    static mut g_PoseCensusWeaponOut: i32;
    static mut g_PoseCensusHits: [u32; CENSUS_MAX];
    static mut g_PoseCensusA4: [u32; CENSUS_MAX];
    static mut g_PoseCensusBuf: [usize; CENSUS_MAX];
    static mut g_PoseCensusArmed: [u8; CENSUS_MAX];
    static mut g_PoseCensusHitsOut: [u32; CENSUS_MAX];
    static mut g_PoseCensusHitsIn: [u32; CENSUS_MAX];
    // The weapon's own rig.
    static mut g_WeaponRigActive: i32;
    static mut g_WeaponTrackBufA: usize;
    static mut g_WeaponTrackBufB: usize;
    static mut g_WeaponPartHave: i32;
    static mut g_WeaponPartCount: i32;
    static mut g_WeaponPartIdx: [i32; 24];
    static mut g_WeaponPartOff: [[f32; 3]; 24];
    static mut g_WeaponPartPos: [[f32; 3]; 24];
    static mut g_WeaponPartWriteOn: i32;
    static mut g_WeaponMatchCalls: u64;
    static mut g_WeaponRigDiag: [i32; 8];
    static mut g_WeaponRigNames: [c_char; 512];
    static kWeaponPartNames: [*const c_char; PART_N];
    // The fingers.
    static mut g_VRReloadFingerSet: [[i32; 32]; 2];
    static mut g_VRReloadFingerRot: [[[f32; 4]; 32]; 2];
    static mut g_VRReloadFingerActive: [i32; 2];
    static mut g_VRReloadFingerBlend: [f32; 2];
    static mut g_VRRestFingerApply: i32;
    static mut g_VRRecordFK: i32;
    static mut g_VRSmokeFingerCount: i32;
    static mut g_VRSmokeFingerCountL: i32;
    static mut g_VRSmokeFingerName: [[c_char; 48]; 32];
    static mut g_VRSmokeFingerNameL: [[c_char; 48]; 32];
    // The captures' handshakes (the animation thread answers them).
    static mut CyberpunkVR_RestFingerCaptureReq: i32;
    static mut CyberpunkVR_DebugRestFingerRefused: i32;
    static mut CyberpunkVR_DebugRestFingerHave: i32;
    static mut CyberpunkVR_RestFingerCaptureReqR: i32;
    static mut CyberpunkVR_DebugRestFingerRefusedR: i32;
    static mut CyberpunkVR_DebugRestFingerHaveR: i32;
    static mut CyberpunkVR_RestFingerApplyR: i32;
    static mut CyberpunkVR_TwoHandCaptureReq: i32;
    static mut CyberpunkVR_DebugTwoHandRefused: i32;
    static mut CyberpunkVR_DebugTwoHandHave: i32;
    static mut CyberpunkVR_DebugCarryReach: f32;
    static mut CyberpunkVR_CarryGripRadius: f32;
    // The pose hook's install (anim_pose_hook.rs, declared by its C name).
    fn cvr_cpp_install_anim_pose_hook() -> bool;
    // The C++ half (src/Natives/AnimLookup.cpp in the Rust build).
    fn cvr_cpp_install_weapon_aim_hooks() -> bool;
    fn cvr_ensure_shared_memory();
    fn cvr_cpp_persist_live_controls();
    fn cvr_anim_object_for_entity(entity: *mut c_void, component: *const c_char, any: bool) -> *mut c_void;
    fn cvr_anim_player_entity() -> *mut c_void;
    fn cvr_anim_list_components(entity: *mut c_void, out: *mut c_char, cap: usize, first_non_root: *mut *mut c_void) -> i32;
    fn cvr_anim_qword_is_heap(value: u64) -> bool;
}

/// RED4ext::Vector4.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Vector4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
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
fn lo32(v: usize) -> i32 {
    (v as u64 & 0xFFFF_FFFF) as u32 as i32
}
fn hi32(v: usize) -> i32 {
    ((v as u64) >> 32) as u32 as i32
}

/// SetVRBoneDebugIndex(index).
#[no_mangle]
pub unsafe extern "C" fn SetVRBoneDebugIndex(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut index = -1i32;
    params!(frame, index);
    g_CalibrationBoneIndex = index;
}
/// InstallVRAnimPoseHook() -> Int32: the pose-apply hook (1 when in).
#[no_mangle]
pub unsafe extern "C" fn InstallVRAnimPoseHook(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let ok = cvr_cpp_install_anim_pose_hook();
    put(out, ok as i32);
}
/// InstallWeaponAimHook() -> Int32: the family installs at boot; this is the CET Weapon module's second call.
#[no_mangle]
pub unsafe extern "C" fn InstallWeaponAimHook(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let ok = cvr_cpp_install_weapon_aim_hooks();
    put(out, ok as i32);
}
/// A flag into a shared slot, as the slot's 32 bits (not a float).
unsafe fn shared_flag(slot: usize, on: bool) {
    cvr_ensure_shared_memory();
    if !g_pSharedHands.is_null() {
        core::ptr::write_volatile((g_pSharedHands as *mut u32).add(slot), on as u32);
    }
}
/// SetVRMenuOpen(open) -> Int32: a full-screen menu (the world map) is open; the HMD stops driving the game camera.
#[no_mangle]
pub unsafe extern "C" fn SetVRMenuOpen(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut open = 0i32;
    params!(frame, open);
    shared_flag(SLOT_MENU_OPEN, open != 0);
    put(out, open);
}
/// The same flag from Rust (rust/crates/cvr-ffi/src/mechanics.rs world_map).
pub fn set_vr_menu_open(open: bool) {
    // SAFETY: the shared slot write SetVRMenuOpen makes.
    unsafe { shared_flag(SLOT_MENU_OPEN, open) };
}
/// VRScannerSlotGet(idx, comp) -> Float: 0 x, 1 y, 2 scale (out of range: 1 for the scale, 0 otherwise).
#[no_mangle]
pub unsafe extern "C" fn VRScannerSlotGet(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let (mut idx, mut comp) = (0i32, 0i32);
    params!(frame, idx, comp);
    let mut v = if comp == 2 { 1.0 } else { 0.0 };
    if (0..7).contains(&idx) && (0..3).contains(&comp) {
        v = CyberpunkVR_ScannerSlots[(idx * 3 + comp) as usize];
    }
    put(out, v);
}
/// VRScannerSlotSet(idx, x, y, scale) -> Int32: a live drag, clamped here too.
#[no_mangle]
pub unsafe extern "C" fn VRScannerSlotSet(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut idx, mut x, mut y, mut s) = (0i32, 0.0f32, 0.0f32, 1.0f32);
    params!(frame, idx, x, y, s);
    if (0..7).contains(&idx) {
        let i = idx as usize * 3;
        CyberpunkVR_ScannerSlots[i] = if x < -1920.0 {
            -1920.0
        } else if x > 1920.0 {
            1920.0
        } else {
            x
        };
        CyberpunkVR_ScannerSlots[i + 1] = if y < -1080.0 {
            -1080.0
        } else if y > 1080.0 {
            1080.0
        } else {
            y
        };
        CyberpunkVR_ScannerSlots[i + 2] = if s < 0.10 {
            0.10
        } else if s > 5.0 {
            5.0
        } else {
            s
        };
    }
    put(out, idx);
}
/// VRScannerSlotSave() -> Int32: one write when the editor closes.
#[no_mangle]
pub unsafe extern "C" fn VRScannerSlotSave(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    cvr_cpp_persist_live_controls();
    put(out, 1);
}
/// SetVRDeviceScreen(open) -> Int32: a device screen is up; the XInput merge leaves the right stick's Y to the UI.
#[no_mangle]
pub unsafe extern "C" fn SetVRDeviceScreen(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut open = 0i32;
    params!(frame, open);
    shared_flag(SLOT_DEVICE_SCREEN, open != 0);
    put(out, open);
}
/// GetVRWeaponAim() -> Int32: always 0 (kept for the crosshair module, which only asks whether the native is there).
/// Its argument list is not advanced past, as the C++ never did.
#[no_mangle]
pub unsafe extern "C" fn GetVRWeaponAim(_this: *mut c_void, _frame: *mut Frame, out: *mut i32, _unused: i64) {
    put(out, 0);
}

/// VRPairs(mode, field) -> Int32: -1 clears, -3 counts, -5 saturated; mode >= 0: 0 bones, 1 tracks, 2 hits armed,
/// 3 hits holstered, 4/5 the last track buffer low/high.
#[no_mangle]
pub unsafe extern "C" fn VRPairs(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut mode, mut field) = (-3i32, 0i32);
    params!(frame, mode, field);
    let Some(out) = out.as_mut() else { return };
    *out = -1;
    match mode {
        -1 => {
            g_PairN = 0;
            g_PairFull = 0;
            *out = 1;
            return;
        }
        -3 => {
            *out = g_PairN;
            return;
        }
        -5 => {
            *out = g_PairFull;
            return;
        }
        _ => {}
    }
    if mode < 0 || mode >= g_PairN || mode as usize >= PAIR_MAX {
        return;
    }
    let m = mode as usize;
    match field {
        0 => *out = g_PairBones[m] as i32,
        1 => *out = g_PairTracks[m] as i32,
        2 => *out = g_PairOut[m] as i32,
        3 => *out = g_PairIn[m] as i32,
        4 => *out = lo32(g_PairBuf[m]),
        5 => *out = hi32(g_PairBuf[m]),
        _ => {}
    }
}
/// VRPairUse(index, which) -> Int32: arm pair entry `index` as weapon rig `which` (0 magazine, 1 frame).
#[no_mangle]
pub unsafe extern "C" fn VRPairUse(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut index, mut which) = (-1i32, 0i32);
    params!(frame, index, which);
    let _guard = RigPoseStateGuard::new();
    put(out, 0);
    if index < 0 || index >= g_PairN || index as usize >= PAIR_MAX || which < 0 || which as usize >= RIG_N {
        return;
    }
    let (i, w) = (index as usize, which as usize);
    g_RigBuf[w] = g_PairBuf[i];
    g_RigBones[w] = g_PairBones[i];
    g_RigTracks[w] = g_PairTracks[i];
    put(out, 1);
}
/// VRRigStatus(field) -> Int32: 0/1 passes matched for magazine/frame, 2/3 bones, 4/5 tracks, 6/7 buffer low, 8/9
/// buffer high.
#[no_mangle]
pub unsafe extern "C" fn VRRigStatus(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut field = 0i32;
    params!(frame, field);
    let _guard = RigPoseStateGuard::new();
    let Some(out) = out.as_mut() else { return };
    *out = -1;
    let w = (field & 1) as usize;
    match field >> 1 {
        0 => *out = g_RigSeen[w] as i32,
        1 => *out = g_RigBones[w] as i32,
        2 => *out = g_RigTracks[w] as i32,
        3 => *out = lo32(g_RigBuf[w]),
        4 => *out = hi32(g_RigBuf[w]),
        _ => {}
    }
}

// ---- THE RIG POSE WRITES (main's Anim/RigPoseWrites.cpp, 16c8d26fa): script requests and the animation's pose passes
// share the rig tables, serialized by one lock (the pass holds it in cvr_anim_weapon_rig_identify_and_write); the
// translation channel apart, so a rotation or scale write leaves the bone's animated translation standing.
static mut RIG_STATE_LOCK: windows_sys::Win32::System::Threading::SRWLOCK = windows_sys::Win32::System::Threading::SRWLOCK_INIT;
/// Whether each write slot owns its bone's translation (guarded by the rig state lock).
pub static mut RIG_WRITE_TRANSLATION: [bool; RIG_WRITES] = [false; RIG_WRITES];
pub fn lock_rig_pose_state() {
    // SAFETY: the process's one lock.
    unsafe { windows_sys::Win32::System::Threading::AcquireSRWLockExclusive(&raw mut RIG_STATE_LOCK) }
}
pub fn unlock_rig_pose_state() {
    // SAFETY: held by this thread.
    unsafe { windows_sys::Win32::System::Threading::ReleaseSRWLockExclusive(&raw mut RIG_STATE_LOCK) }
}
/// The lock for a scope.
pub struct RigPoseStateGuard;
impl RigPoseStateGuard {
    pub fn new() -> Self {
        lock_rig_pose_state();
        RigPoseStateGuard
    }
}
impl Drop for RigPoseStateGuard {
    fn drop(&mut self) {
        unlock_rig_pose_state();
    }
}
fn valid_key(which: i32, bone: i32) -> bool {
    which >= 0 && (which as usize) < RIG_N && bone >= 0 && (bone as usize) < super::MAX_BONES
}
fn finite3(x: f32, y: f32, z: f32) -> bool {
    x.is_finite() && y.is_finite() && z.is_finite()
}
/// ResetSlot: a slot as new.
unsafe fn reset_slot(slot: usize) {
    g_RigWriteEnabled[slot] = 0;
    RIG_WRITE_TRANSLATION[slot] = false;
    g_RigWriteHaveBase[slot] = 0;
    g_RigWriteHaveBaseRot[slot] = 0;
    g_RigWriteApplied[slot] = 0;
    g_RigWriteSlot[slot] = -1;
    g_RigWritePin[slot] = -1;
    g_RigWriteAbs[slot] = 0;
    g_RigWriteQuatOn[slot] = 0;
    g_RigWriteScale[slot] = 0.0;
    g_RigWriteRotAngle[slot] = 0.0;
    g_RigWriteOff[slot] = [0.0; 3];
    g_RigWriteBase[slot] = [0.0; 3];
    g_RigWriteRotAxis[slot] = [0.0, 1.0, 0.0];
    g_RigWriteQuat[slot] = [0.0, 0.0, 0.0, 1.0];
    g_RigWriteBaseRot[slot] = [0.0, 0.0, 0.0, 1.0];
}
/// The write-queue slot keyed by (which, bone), made (reset) when there is none; None when the queue is full. Under
/// the lock.
unsafe fn write_slot(which: i32, bone: i32) -> Option<usize> {
    if !valid_key(which, bone) {
        return None;
    }
    let count = g_RigWriteN.clamp(0, RIG_WRITES as i32) as usize;
    for i in 0..count {
        if g_RigWriteWhich[i] == which && g_RigWriteBone[i] == bone {
            return Some(i);
        }
    }
    if count >= RIG_WRITES {
        return None;
    }
    reset_slot(count);
    g_RigWriteWhich[count] = which;
    g_RigWriteBone[count] = bone;
    g_RigWriteN = count as i32 + 1;
    Some(count)
}
/// WriteRigOffset: a translation offset onto the bone's rest; the slot, or -1.
pub fn write_rig_offset(which: i32, bone: i32, x: f32, y: f32, z: f32) -> i32 {
    if !finite3(x, y, z) {
        return -1;
    }
    let _guard = RigPoseStateGuard::new();
    // SAFETY: the rig tables, under their lock.
    unsafe {
        let Some(slot) = write_slot(which, bone) else { return -1 };
        if g_RigWriteAbs[slot] != 0 {
            g_RigWriteHaveBase[slot] = 0;
        }
        g_RigWriteAbs[slot] = 0;
        RIG_WRITE_TRANSLATION[slot] = true;
        g_RigWriteOff[slot] = [x, y, z];
        g_RigWriteEnabled[slot] = 1;
        slot as i32
    }
}
/// WriteRigRotation: degrees about a local axis (no axis: no rotation); the slot, or -1.
pub fn write_rig_rotation(which: i32, bone: i32, angle: f32, x: f32, y: f32, z: f32) -> i32 {
    if !angle.is_finite() || !finite3(x, y, z) {
        return -1;
    }
    let length = cvr_body::cmath::hypot(cvr_body::cmath::hypot(x, y), z);
    if !length.is_finite() {
        return -1;
    }
    let (angle, x, y, z) = if length < 1e-6 { (0.0, 0.0, 1.0, 0.0) } else { (angle, x / length, y / length, z / length) };
    let _guard = RigPoseStateGuard::new();
    // SAFETY: the rig tables, under their lock.
    unsafe {
        let Some(slot) = write_slot(which, bone) else { return -1 };
        g_RigWriteRotAxis[slot] = [x, y, z];
        g_RigWriteRotAngle[slot] = angle;
        g_RigWriteEnabled[slot] = 1;
        slot as i32
    }
}
/// WriteRigScale: a uniform scale, clamped to 0..4 (0 stops writing it); the slot, or -1.
pub fn write_rig_scale(which: i32, bone: i32, scale: f32) -> i32 {
    if !scale.is_finite() {
        return -1;
    }
    let _guard = RigPoseStateGuard::new();
    // SAFETY: the rig tables, under their lock.
    unsafe {
        let Some(slot) = write_slot(which, bone) else { return -1 };
        g_RigWriteScale[slot] = scale.clamp(0.0, 4.0);
        g_RigWriteEnabled[slot] = 1;
        slot as i32
    }
}
/// WriteRigPose: a recorded local pose replayed (absolute) or a translation offset with a rotation; the slot, or -1.
#[allow(clippy::too_many_arguments)]
pub fn write_rig_pose(which: i32, bone: i32, x: f32, y: f32, z: f32, qi: f32, qj: f32, qk: f32, qr: f32, absolute: bool) -> i32 {
    if !finite3(x, y, z) || !finite3(qi, qj, qk) || !qr.is_finite() {
        return -1;
    }
    let length = sqrt(qi * qi + qj * qj + qk * qk + qr * qr);
    if !length.is_finite() {
        return -1;
    }
    let _guard = RigPoseStateGuard::new();
    // SAFETY: the rig tables, under their lock.
    unsafe {
        let Some(slot) = write_slot(which, bone) else { return -1 };
        if g_RigWriteAbs[slot] != 0 && !absolute {
            g_RigWriteHaveBase[slot] = 0;
        }
        g_RigWriteOff[slot] = [x, y, z];
        g_RigWriteAbs[slot] = absolute as i32;
        RIG_WRITE_TRANSLATION[slot] = true;
        g_RigWriteQuatOn[slot] = (length > 1e-4) as i32;
        if length > 1e-4 {
            g_RigWriteQuat[slot] = [qi / length, qj / length, qk / length, qr / length];
        }
        g_RigWriteEnabled[slot] = 1;
        slot as i32
    }
}
/// ReleaseRigWrite: the bone handed back to the animation (its slot dormant; a known rest base kept for the next
/// gesture on the same bone -- a weapon switch's clear discards it).
pub fn release_rig_write(which: i32, bone: i32) -> bool {
    let _guard = RigPoseStateGuard::new();
    // SAFETY: the rig tables, under their lock.
    unsafe {
        for slot in 0..g_RigWriteN.clamp(0, RIG_WRITES as i32) as usize {
            if g_RigWriteWhich[slot] != which || g_RigWriteBone[slot] != bone {
                continue;
            }
            g_RigWriteEnabled[slot] = 0;
            RIG_WRITE_TRANSLATION[slot] = false;
            g_RigWriteAbs[slot] = 0;
            g_RigWriteQuatOn[slot] = 0;
            g_RigWriteRotAngle[slot] = 0.0;
            g_RigWriteScale[slot] = 0.0;
            g_RigWriteOff[slot] = [0.0; 3];
            return true;
        }
        false
    }
}
/// ClearRigWrites: every slot reset and every float track handed back to the animation.
pub fn clear_rig_writes() {
    let _guard = RigPoseStateGuard::new();
    // SAFETY: the rig tables, under their lock.
    unsafe {
        for slot in 0..RIG_WRITES {
            reset_slot(slot);
        }
        for w in 0..RIG_N {
            g_RigTrackOn[w] = [0; RIG_TRACKS];
        }
        g_RigWriteN = 0;
    }
}
/// VRRigWrite(which, bone, x, y, z) -> Int32: queue a translation offset onto one bone of one rig; the slot.
#[no_mangle]
pub unsafe extern "C" fn VRRigWrite(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut which, mut bone, mut x, mut y, mut z) = (-1i32, -1i32, 0.0f32, 0.0f32, 0.0f32);
    params!(frame, which, bone, x, y, z);
    put(out, write_rig_offset(which, bone, x, y, z));
}
/// VRRigWriteRot(which, bone, angle, ax, ay, az) -> Int32: a rotation (degrees about a local axis) on one rig bone.
#[no_mangle]
pub unsafe extern "C" fn VRRigWriteRot(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut which, mut bone) = (-1i32, -1i32);
    let (mut angle, mut ax, mut ay, mut az) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    params!(frame, which, bone, angle, ax, ay, az);
    put(out, write_rig_rotation(which, bone, angle, ax, ay, az));
}
/// VRRigWriteOff(which, bone) -> Int32: hand one bone back to the animation (1 if a slot existed).
#[no_mangle]
pub unsafe extern "C" fn VRRigWriteOff(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut which, mut bone) = (-1i32, -1i32);
    params!(frame, which, bone);
    put(out, release_rig_write(which, bone) as i32);
}
/// VRRigWriteClear() -> Bool: every slot cleared and every float track handed back to the animation (the panic button
/// the scripts reach for: a magazine left hidden by a stale write would be invisible with nothing to explain it).
#[no_mangle]
pub unsafe extern "C" fn VRRigWriteClear(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    params!(frame);
    clear_rig_writes();
    put(out, true);
}

/// VRReloadFingerClear(hand) -> Int32: the hand's pending grip pose cleared (its fingers back under tracking).
#[no_mangle]
pub unsafe extern "C" fn VRReloadFingerClear(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut hand = -1i32;
    params!(frame, hand);
    put(out, 0);
    if !(0..=1).contains(&hand) {
        return;
    }
    g_VRReloadFingerSet[hand as usize] = [0; 32];
    put(out, 1);
}
/// A NUL-terminated name in a fixed slot, as bytes.
fn slot_bytes(name: &[c_char]) -> &[u8] {
    let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    // SAFETY: c_char and u8 share a layout.
    unsafe { core::slice::from_raw_parts(name.as_ptr() as *const u8, n) }
}
/// VRReloadFingerSet(hand, name, qx, qy, qz, qw) -> Int32: one finger bone's target rotation (runtime space), the
/// bone matched by name, case-insensitive; the finger slot, or -1.
#[no_mangle]
pub unsafe extern "C" fn VRReloadFingerSet(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut hand = -1i32;
    let mut name = CString::empty();
    let (mut qx, mut qy, mut qz, mut qw) = (0.0f32, 0.0f32, 0.0f32, 1.0f32);
    params!(frame, hand, name, qx, qy, qz, qw);
    put(out, -1);
    if !(0..=1).contains(&hand) {
        return;
    }
    let bone = name.as_str().as_bytes();
    if bone.is_empty() {
        return;
    }
    let h = hand as usize;
    let count = if h == 0 { g_VRSmokeFingerCountL } else { g_VRSmokeFingerCount };
    for k in 0..count.clamp(0, 32) as usize {
        let finger = if h == 0 {
            slot_bytes(&g_VRSmokeFingerNameL[k])
        } else {
            slot_bytes(&g_VRSmokeFingerName[k])
        };
        if !finger.is_empty() && finger.eq_ignore_ascii_case(bone) {
            g_VRReloadFingerRot[h][k] = [qx, qy, qz, qw];
            g_VRReloadFingerSet[h][k] = 1;
            put(out, k as i32);
            return;
        }
    }
}
/// VRTwoHandCapture() -> Int32: take one frame of the game's own two-handed hold (VRIK off).
#[no_mangle]
pub unsafe extern "C" fn VRTwoHandCapture(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    CyberpunkVR_DebugTwoHandRefused = 0;
    CyberpunkVR_TwoHandCaptureReq = 1;
    put(out, 1);
}
/// VRTwoHandStatus() -> Int32: 1 captured, 0 nothing yet, or -(refusal bits).
#[no_mangle]
pub unsafe extern "C" fn VRTwoHandStatus(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let v = if CyberpunkVR_DebugTwoHandRefused != 0 {
        -CyberpunkVR_DebugTwoHandRefused
    } else {
        (CyberpunkVR_DebugTwoHandHave != 0) as i32
    };
    put(out, v);
}
/// VRRestFingerCapture() -> Int32: the resting left hand, captured on the animation thread.
#[no_mangle]
pub unsafe extern "C" fn VRRestFingerCapture(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    CyberpunkVR_DebugRestFingerRefused = 0;
    CyberpunkVR_RestFingerCaptureReq = 1;
    put(out, 1);
}
/// GetVRCarryReach() -> Float: the grip-to-grip reach while the weapon is carried (-1 when it is not).
#[no_mangle]
pub unsafe extern "C" fn GetVRCarryReach(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    params!(frame);
    put(out, CyberpunkVR_DebugCarryReach);
}
/// GetVRCarryNear() -> Int32: 1 within the grip radius, 0 not, -1 no carry running.
#[no_mangle]
pub unsafe extern "C" fn GetVRCarryNear(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let d = CyberpunkVR_DebugCarryReach;
    let v = if d < 0.0 {
        -1
    } else if d <= CyberpunkVR_CarryGripRadius {
        1
    } else {
        0
    };
    put(out, v);
}
/// VRRestFingerCaptureRight() -> Int32.
#[no_mangle]
pub unsafe extern "C" fn VRRestFingerCaptureRight(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    CyberpunkVR_DebugRestFingerRefusedR = 0;
    CyberpunkVR_RestFingerCaptureReqR = 1;
    put(out, 1);
}
/// VRRestFingerStatusRight() -> Int32.
#[no_mangle]
pub unsafe extern "C" fn VRRestFingerStatusRight(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let v = if CyberpunkVR_DebugRestFingerRefusedR != 0 {
        -CyberpunkVR_DebugRestFingerRefusedR
    } else {
        (CyberpunkVR_DebugRestFingerHaveR != 0) as i32
    };
    put(out, v);
}
/// VRRestFingerApplyRight(on) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn VRRestFingerApplyRight(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut on = 1i32;
    params!(frame, on);
    CyberpunkVR_RestFingerApplyR = (on != 0) as i32;
    put(out, CyberpunkVR_RestFingerApplyR);
}
/// VRRestFingerStatus() -> Int32.
#[no_mangle]
pub unsafe extern "C" fn VRRestFingerStatus(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    let v = if CyberpunkVR_DebugRestFingerRefused != 0 {
        -CyberpunkVR_DebugRestFingerRefused
    } else {
        (CyberpunkVR_DebugRestFingerHave != 0) as i32
    };
    put(out, v);
}
/// VRRestFingerApply(on) -> Int32: off puts the game's own two-handed-grip fingers back.
#[no_mangle]
pub unsafe extern "C" fn VRRestFingerApply(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut on = 1i32;
    params!(frame, on);
    g_VRRestFingerApply = (on != 0) as i32;
    put(out, g_VRRestFingerApply);
}
/// VRReloadFingerApply(hand, on) -> Int32 (the blend is the caller's, set before).
#[no_mangle]
pub unsafe extern "C" fn VRReloadFingerApply(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut hand, mut on) = (-1i32, 0i32);
    params!(frame, hand, on);
    put(out, 0);
    if !(0..=1).contains(&hand) {
        return;
    }
    g_VRReloadFingerActive[hand as usize] = (on != 0) as i32;
    put(out, 1);
}
/// VRReloadFingerBlend(hand, alpha) -> Int32: how much of the grip pose is mixed onto the tracked fingers (0..1).
#[no_mangle]
pub unsafe extern "C" fn VRReloadFingerBlend(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut hand, mut alpha) = (-1i32, 1.0f32);
    params!(frame, hand, alpha);
    put(out, 0);
    if !(0..=1).contains(&hand) {
        return;
    }
    if alpha < 0.0 {
        alpha = 0.0;
    }
    if alpha > 1.0 {
        alpha = 1.0;
    }
    g_VRReloadFingerBlend[hand as usize] = alpha;
    put(out, 1);
}
/// VRRigReset() -> Bool: forget the identified weapon rigs (a weapon draw or switch).
#[no_mangle]
pub unsafe extern "C" fn VRRigReset(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    params!(frame);
    let _guard = RigPoseStateGuard::new();
    g_RigSigN = 0;
    for w in 0..RIG_N {
        g_RigPoseHave[w] = 0;
        g_RigPose[w] = [[0.0; 7]; 20];
        g_RigBuf[w] = 0;
        g_RigBones[w] = 0;
        g_RigTrackHave[w] = 0;
        g_RigTrackOn[w] = [0; RIG_TRACKS];
    }
    g_RigPassSeen = [0; 4];
    g_RigPassRn = [0; 4];
    g_RigPassDst = [0; 4];
    put(out, true);
}
/// VRRigWriteScale(which, bone, scale) -> Int32: a uniform scale on one rig bone (<= 0 stops writing it; at most 4).
#[no_mangle]
pub unsafe extern "C" fn VRRigWriteScale(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut which, mut bone, mut scale) = (-1i32, -1i32, 0.0f32);
    params!(frame, which, bone, scale);
    put(out, write_rig_scale(which, bone, scale));
}
/// VRRigWriteAbs(which, bone, x, y, z, qi, qj, qk, qr, abs) -> Int32: replay a recorded local pose verbatim.
#[no_mangle]
pub unsafe extern "C" fn VRRigWriteAbs(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut which, mut bone, mut abs) = (-1i32, -1i32, 1i32);
    let (mut x, mut y, mut z) = (0.0f32, 0.0f32, 0.0f32);
    let (mut qi, mut qj, mut qk, mut qr) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    params!(frame, which, bone, x, y, z, qi, qj, qk, qr, abs);
    put(out, write_rig_pose(which, bone, x, y, z, qi, qj, qk, qr, abs != 0));
}
/// VRRigBone(which, bone, field) -> Float: one weapon-rig bone's local transform as the game left it (0..2
/// translation, 3..6 rotation); 0 for an unseen rig or bone.
#[no_mangle]
pub unsafe extern "C" fn VRRigBone(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let (mut which, mut bone, mut field) = (-1i32, -1i32, 0i32);
    params!(frame, which, bone, field);
    let _guard = RigPoseStateGuard::new();
    put(out, 0.0);
    if which < 0 || which as usize >= RIG_N || !(0..20).contains(&bone) || !(0..=6).contains(&field) {
        return;
    }
    if g_RigPoseHave[which as usize] == 0 {
        return;
    }
    put(out, g_RigPose[which as usize][bone as usize][field as usize]);
}

/// The CName hash: FNV-1a 64 over the name's bytes.
fn name_hash(s: &[u8]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in s {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
}
/// VRRigSignature(which, bones, i0, n0, i1, n1, i2, n2, i3, n3) -> Int32: register a rig signature (a slot is reused
/// only by an identical signature); the slot, or -1 when the table is full.
#[no_mangle]
pub unsafe extern "C" fn VRRigSignature(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut which, mut bones) = (-1i32, 0i32);
    let (mut i0, mut i1, mut i2, mut i3) = (-1i32, -1i32, -1i32, -1i32);
    let (mut n0, mut n1, mut n2, mut n3) = (CString::empty(), CString::empty(), CString::empty(), CString::empty());
    params!(frame, which, bones, i0, n0, i1, n1, i2, n2, i3, n3);
    let _guard = RigPoseStateGuard::new();
    put(out, -1);
    if which < 0 || which as usize >= RIG_N || bones <= 0 {
        return;
    }
    let idx = [i0, i1, i2, i3];
    let names = [&n0, &n1, &n2, &n3];
    let mut widx = [0i32; SIG_NAMES];
    let mut whash = [0u64; SIG_NAMES];
    for k in 0..SIG_NAMES {
        let s = names[k].as_str().as_bytes();
        if idx[k] < 0 || s.is_empty() {
            widx[k] = -1;
            whash[k] = 0;
        } else {
            widx[k] = idx[k];
            whash[k] = name_hash(s);
        }
    }
    let n = g_RigSigN;
    let mut slot = None;
    for k in 0..n.clamp(0, SIG_MAX as i32) as usize {
        if g_RigSigWhich[k] != which || g_RigSigBones[k] != bones as u32 {
            continue;
        }
        if g_RigSigIdx[k] == widx && g_RigSigHash[k] == whash {
            slot = Some(k);
            break;
        }
    }
    let slot = match slot {
        Some(s) => s,
        None => {
            if n as usize >= SIG_MAX {
                return;
            }
            n as usize
        }
    };
    g_RigSigIdx[slot] = widx;
    g_RigSigHash[slot] = whash;
    g_RigSigWhich[slot] = which;
    g_RigSigBones[slot] = bones as u32;
    if slot as i32 >= g_RigSigN {
        g_RigSigN = slot as i32 + 1;
    }
    put(out, slot as i32);
}
/// VRRigTrack(which, idx) -> Float: a weapon rig's float track as the game leaves it (idx < 0: the track count).
#[no_mangle]
pub unsafe extern "C" fn VRRigTrack(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let (mut which, mut idx) = (-1i32, -1i32);
    params!(frame, which, idx);
    put(out, -1.0);
    if which < 0 || which as usize >= RIG_N {
        return;
    }
    let w = which as usize;
    if idx < 0 {
        put(out, g_RigTracks[w] as f32);
        return;
    }
    if idx as usize >= RIG_TRACKS || g_RigTrackHave[w] == 0 {
        return;
    }
    put(out, g_RigTrackVal[w][idx as usize]);
}
/// VRRigTrackWrite(which, idx, value, on) -> Int32: drive one float track (on = 0 hands it back).
#[no_mangle]
pub unsafe extern "C" fn VRRigTrackWrite(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut which, mut idx, mut value, mut on) = (-1i32, -1i32, 0.0f32, 1i32);
    params!(frame, which, idx, value, on);
    put(out, 0);
    if which < 0 || which as usize >= RIG_N || idx < 0 || idx as usize >= RIG_TRACKS {
        return;
    }
    g_RigTrackSet[which as usize][idx as usize] = value;
    g_RigTrackOn[which as usize][idx as usize] = (on != 0) as i32;
    put(out, 1);
}
/// VRRecordFK(on) -> Int32: the reload recorder's whole-skeleton FK of the animated pose.
#[no_mangle]
pub unsafe extern "C" fn VRRecordFK(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    g_VRRecordFK = (on != 0) as i32;
    put(out, g_VRRecordFK);
}
/// VRRigWriteDiag(slot, field) -> Int32: the write queue's per-slot state (slot < 0: the count).
#[no_mangle]
pub unsafe extern "C" fn VRRigWriteDiag(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut slot, mut field) = (-1i32, 0i32);
    params!(frame, slot, field);
    let Some(out) = out.as_mut() else { return };
    if slot < 0 {
        *out = g_RigWriteN;
        return;
    }
    *out = -1;
    if slot as usize >= RIG_WRITES {
        return;
    }
    let s = slot as usize;
    let mm = |v: f32| (v * 1000.0) as i32;
    match field {
        0 => *out = g_RigWriteWhich[s],
        1 => *out = g_RigWriteBone[s],
        2 => *out = g_RigWriteHaveBase[s],
        3 => *out = g_RigWriteApplied[s] as i32,
        4..=6 => *out = mm(g_RigWriteOff[s][(field - 4) as usize]),
        7..=9 => *out = mm(g_RigWriteBase[s][(field - 7) as usize]),
        10 => *out = g_RigWriteSlot[s],
        11 => *out = g_RigWritePin[s],
        12..=15 => *out = mm(g_RigWriteBaseRot[s][(field - 12) as usize]),
        16 => *out = g_RigWriteHaveBaseRot[s],
        20..=23 => *out = g_RigPassSeen[(field - 20) as usize] as i32,
        24..=27 => *out = g_RigPassRn[(field - 24) as usize],
        28..=31 => *out = g_RigPassDst[(field - 28) as usize],
        32..=35 => *out = g_RigPassSlots[(field - 32) as usize],
        36..=39 => *out = g_RigPassBufLo[(field - 36) as usize],
        40..=47 => *out = g_RigPassMap0[(field - 40) as usize],
        _ => {}
    }
}
/// VRSmallRig(mode, field) -> Int32: the small-rig table (-1 clear, -3 count, -5 saturated; mode >= 0: 0 bones,
/// 1 a4, 2/3 buffer low/high, 4 hits armed, 5 hits holstered).
#[no_mangle]
pub unsafe extern "C" fn VRSmallRig(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut mode, mut field) = (-3i32, 0i32);
    params!(frame, mode, field);
    let Some(out) = out.as_mut() else { return };
    *out = -1;
    match mode {
        -1 => {
            g_SmallN = 0;
            g_SmallFull = 0;
            *out = 1;
            return;
        }
        -3 => {
            *out = g_SmallN;
            return;
        }
        -5 => {
            *out = g_SmallFull;
            return;
        }
        _ => {}
    }
    if mode < 0 || mode >= g_SmallN || mode as usize >= SMALL_MAX {
        return;
    }
    let m = mode as usize;
    match field {
        0 => *out = g_SmallBones[m] as i32,
        1 => *out = g_SmallA4[m] as i32,
        2 => *out = lo32(g_SmallBuf[m]),
        3 => *out = hi32(g_SmallBuf[m]),
        4 => *out = g_SmallOut[m] as i32,
        5 => *out = g_SmallIn[m] as i32,
        _ => {}
    }
}
/// The weapon's rig armed on a recorded track buffer.
unsafe fn arm_weapon_rig(buffer: usize) {
    g_WeaponTrackBufA = buffer;
    g_WeaponTrackBufB = 0;
    g_WeaponPartHave = 0;
    g_WeaponPartCount = 24;
    g_WeaponRigActive = 1;
}
/// VRSmallRigUse(index) -> Int32: arm small-rig entry `index` as the weapon's rig.
#[no_mangle]
pub unsafe extern "C" fn VRSmallRigUse(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut index = -1i32;
    params!(frame, index);
    put(out, 0);
    if index < 0 || index >= g_SmallN || index as usize >= SMALL_MAX {
        g_WeaponRigActive = 0;
        return;
    }
    arm_weapon_rig(g_SmallBuf[index as usize]);
    put(out, 1);
}
/// VRPoseCensus(mode, field) -> Int32: -1 clear and enable, -2 disable, -3 count, -4 weapon in hand (field), -5
/// saturated; mode >= 0: 0 hits, 1 a4, 2/3 buffer low/high, 4 born armed, 5 hits armed, 6 hits holstered.
#[no_mangle]
pub unsafe extern "C" fn VRPoseCensus(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut mode, mut field) = (-3i32, 0i32);
    params!(frame, mode, field);
    let Some(out) = out.as_mut() else { return };
    *out = -1;
    match mode {
        -1 => {
            g_PoseCensusN = 0;
            g_PoseCensusFull = 0;
            g_PoseCensusOn = 1;
            *out = 1;
            return;
        }
        -4 => {
            g_PoseCensusWeaponOut = (field != 0) as i32;
            *out = 1;
            return;
        }
        -5 => {
            *out = g_PoseCensusFull;
            return;
        }
        -2 => {
            g_PoseCensusOn = 0;
            *out = 0;
            return;
        }
        -3 => {
            *out = g_PoseCensusN;
            return;
        }
        _ => {}
    }
    if mode < 0 || mode >= g_PoseCensusN || mode as usize >= CENSUS_MAX {
        return;
    }
    let m = mode as usize;
    match field {
        0 => *out = g_PoseCensusHits[m] as i32,
        1 => *out = g_PoseCensusA4[m] as i32,
        2 => *out = lo32(g_PoseCensusBuf[m]),
        3 => *out = hi32(g_PoseCensusBuf[m]),
        4 => *out = g_PoseCensusArmed[m] as i32,
        5 => *out = g_PoseCensusHitsOut[m] as i32,
        6 => *out = g_PoseCensusHitsIn[m] as i32,
        _ => {}
    }
}
/// VRWeaponRigUse(index) -> Int32: arm census entry `index` as the weapon's rig.
#[no_mangle]
pub unsafe extern "C" fn VRWeaponRigUse(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut index = -1i32;
    params!(frame, index);
    put(out, 0);
    if index < 0 || index >= g_PoseCensusN || index as usize >= CENSUS_MAX {
        g_WeaponRigActive = 0;
        return;
    }
    arm_weapon_rig(g_PoseCensusBuf[index as usize]);
    put(out, 1);
}
/// VRWeaponPartSetIdx(slot, bone) -> Bool: a part's bone index, from the rig asset.
#[no_mangle]
pub unsafe extern "C" fn VRWeaponPartSetIdx(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut slot, mut bone) = (-1i32, -1i32);
    params!(frame, slot, bone);
    put(out, false);
    if !(0..24).contains(&slot) {
        return;
    }
    g_WeaponPartIdx[slot as usize] = if bone >= 0 && (bone as usize) < super::MAX_BONES { bone } else { -1 };
    put(out, true);
}
/// VRWeaponPartOffset(slot, x, y, z) -> Bool: the write test, one part's local translation offset (metres).
#[no_mangle]
pub unsafe extern "C" fn VRWeaponPartOffset(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let (mut slot, mut x, mut y, mut z) = (-1i32, 0.0f32, 0.0f32, 0.0f32);
    params!(frame, slot, x, y, z);
    put(out, false);
    if !(0..24).contains(&slot) {
        return;
    }
    g_WeaponPartOff[slot as usize] = [x, y, z];
    g_WeaponPartWriteOn = 1;
    put(out, true);
}
/// VRWeaponRigArm(weapon) -> Int32: arm the weapon's rig -- its track buffers and its part bones by name; the parts
/// resolved, 0 for a rig with none of them, -1 when the rig itself is not found.
#[no_mangle]
pub unsafe extern "C" fn VRWeaponRigArm(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut weapon = Handle::null();
    params!(frame, weapon);
    put(out, -1);
    g_WeaponRigActive = 0;
    g_WeaponPartHave = 0;
    g_WeaponPartCount = 0;
    g_WeaponTrackBufA = 0;
    g_WeaponTrackBufB = 0;
    g_WeaponPartIdx = [-1; 24];
    if weapon.instance.is_null() {
        return;
    }
    g_WeaponRigDiag = [0; 8];
    g_WeaponRigNames[0] = 0;
    // First the weapon's own entity (its animated component is not "root": any will do), then the player's, where an
    // attached weapon's rig may be registered instead; every component name is reported either way.
    let mut anim = cvr_anim_object_for_entity(weapon.instance as *mut c_void, core::ptr::null(), true);
    g_WeaponRigDiag[0] = (!anim.is_null()) as i32;
    let mut from_player: *mut c_void = core::ptr::null_mut();
    let player = cvr_anim_player_entity();
    let on_player = cvr_anim_list_components(
        player,
        (&raw mut g_WeaponRigNames) as *mut c_char,
        core::mem::size_of::<[c_char; 512]>(),
        &mut from_player,
    );
    g_WeaponRigDiag[5] = on_player;
    g_WeaponRigDiag[6] = (!from_player.is_null()) as i32;
    if anim.is_null() {
        anim = from_player;
    }
    if anim.is_null() || !VRIK_IsReadable(anim, 0x40) {
        return;
    }
    let base = anim as *const u8;
    let owner_a = *(base.add(0x8) as *const *const u8);
    if VRIK_IsReadable(owner_a as *const c_void, 0x48) {
        g_WeaponTrackBufA = *(owner_a.add(0x40) as *const usize);
    }
    let owner_b = *(base.add(0x18) as *const *const u8);
    if VRIK_IsReadable(owner_b as *const c_void, 0x20) {
        g_WeaponTrackBufB = *(owner_b.add(0x18) as *const usize);
    }
    g_WeaponRigDiag[1] = (g_WeaponTrackBufA != 0) as i32;
    g_WeaponRigDiag[2] = (g_WeaponTrackBufB != 0) as i32;
    if g_WeaponTrackBufA == 0 && g_WeaponTrackBufB == 0 {
        return;
    }
    let mut found = 0;
    // anim::AnimatedObject::metaRig (+0x08); anim::MetaRig::boneNames (+0x20).
    let meta_rig = owner_a;
    if !meta_rig.is_null() && cvr_anim_qword_is_heap(meta_rig as u64) {
        let names = (*(meta_rig.add(0x20) as *const DynArray<CName>)).as_slice();
        g_WeaponRigDiag[3] = names.len() as i32;
        for k in 0..PART_N {
            let want = CStr::from_ptr(kWeaponPartNames[k]).to_bytes();
            if let Some(b) = names.iter().position(|n| n.text().as_bytes() == want && !want.is_empty()) {
                g_WeaponPartIdx[k] = b as i32;
                found += 1;
            }
        }
    }
    g_WeaponRigDiag[4] = found;
    g_WeaponPartCount = PART_N as i32;
    g_WeaponRigActive = 1;
    put(out, found);
}
/// VRWeaponPartLocal(slot) -> Vector4: one part's captured parent-local translation (W = 1 once seen).
#[no_mangle]
pub unsafe extern "C" fn VRWeaponPartLocal(_this: *mut c_void, frame: *mut Frame, out: *mut Vector4, _unused: i64) {
    let mut slot = 0i32;
    params!(frame, slot);
    let Some(out) = out.as_mut() else { return };
    *out = Vector4::default();
    if slot < 0 || slot as usize >= PART_N || g_WeaponPartHave == 0 || g_WeaponPartIdx[slot as usize] < 0 {
        return;
    }
    let p = g_WeaponPartPos[slot as usize];
    *out = Vector4 {
        x: p[0],
        y: p[1],
        z: p[2],
        w: 1.0,
    };
}
/// VRWeaponRigNames() -> String: the animated component names found on the player entity.
#[no_mangle]
pub unsafe extern "C" fn VRWeaponRigNames(_this: *mut c_void, frame: *mut Frame, out: *mut CString, _unused: i64) {
    params!(frame);
    if out.is_null() {
        return;
    }
    let names = String::from_utf8_lossy(slot_bytes(&*(&raw const g_WeaponRigNames))).into_owned();
    CString::assign(out, &names);
}
/// VRWeaponRigStatus(slot) -> Int32: -1 the rig's matched passes, -2..-9 the arming diagnostics, else a part's bone.
#[no_mangle]
pub unsafe extern "C" fn VRWeaponRigStatus(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let mut slot = -1i32;
    params!(frame, slot);
    let Some(out) = out.as_mut() else { return };
    *out = if slot == -1 {
        (g_WeaponMatchCalls & 0x7FFF_FFFF) as i32
    } else if (-9..=-2).contains(&slot) {
        g_WeaponRigDiag[(-slot - 2) as usize]
    } else if slot < 0 {
        -1
    } else if (slot as usize) < PART_N {
        g_WeaponPartIdx[slot as usize]
    } else {
        -1
    };
}
