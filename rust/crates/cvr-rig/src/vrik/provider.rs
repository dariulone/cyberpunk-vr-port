//! The shot down the barrel and the weapon state script publishes (formerly src/Natives/OrientationProvider.cpp),
//! natives registered by rust/crates/cvr-ffi/src/natives.rs.
//!
//! THE ORIENTATION PROVIDERS' VMT INSTRUMENT (`InstallVRProvInstrument`): an instance of each provider class
//! (entEntity / entStatic / entFuncOrientationProvider) is made to read its real vtable, and slots 3..50 are replaced
//! with stubs that count, sample and -- for the player's own attack -- override the out-quaternion: slot 33 (the
//! projectile launch) turns the shot cone onto the published muzzle, keeping each pellet's offset, and fires the hand
//! recoil once per round. entFuncPositionProvider's slot 33 (GetWorldPosition) starts the player's projectile at the
//! muzzle, on the sight line when the eye is on a sight. Whose attack it is: the attack's four participant weak
//! handles (+0xA8..+0xD8), reached through each class's own owner chain; every read is guarded.
//!
//! THE SETTERS: the muzzle and the aim hit, the carry and two-hand switches, the weapon's name, kick and class, the
//! locomotion and paired-animation states, the physical crouch's feedback, the sights, the triggers, the camera pair's
//! tuning and the shared-slot reads CET uses.
//!
//! The exported values (the CyberpunkVR_* the harness reads, g_provMuzzleSeq and g_provRecoilSeqSeen) stay defined in
//! src/Natives/OrientationProvider.cpp; the private state is here.
#![allow(static_mut_refs)]
use super::camera_math::cvr_camera_desired_half_ipd;
use super::env::*;
use super::haptics::cvr_haptics_play;
use super::recoil::RecoilOnShot;
use super::reload_handoff::cvr_anim_reload_hand_off_set;
use super::rig::VRIK_QuatRotateVec;
use super::two_hand::VRTwoHandCarryOffset;
use super::xinput::{cvr_input_publish_scanner_vision, cvr_input_world_time_scale};
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use cvr_body::cmath::sqrt;
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::rtti::{Class, IType, RttiSystem};
use red4ext_abi::stack::StackFrame;

type Frame = StackFrame;

const SLOT_LO: usize = 3;
const N_SLOTS: usize = 48;
const N_CLASSES: usize = 3;
const CLASS_NAMES: [&str; N_CLASSES] = ["entEntityOrientationProvider", "entStaticOrientationProvider", "entFuncOrientationProvider"];
/// gameIAttack's four participant weak handles.
const ATTACK_HANDLE_OFFSETS: [usize; 4] = [0xA8, 0xB8, 0xC8, 0xD8];
/// Utils/SharedSlots.hpp.
const SLOT_WEAPON_FLAG: usize = 144;
const SLOT_RELOAD_OWNED_HAND: usize = 162;
const SLOT_LEFT_MUZZLE_POS: usize = 166;
const SLOT_LEFT_MUZZLE_FWD: usize = 169;
const SLOT_LEFT_MUZZLE_VALID: usize = 172;

#[allow(non_upper_case_globals)]
extern "C" {
    // Defined in src/Natives/OrientationProvider.cpp (exported, or read by name elsewhere).
    static mut CyberpunkVR_DebugProvEntFunc: [u64; 16];
    static g_provMuzzleSeq: AtomicU32;
    static g_provRecoilSeqSeen: AtomicI32;
    static CyberpunkVR_DebugAtkCalls: AtomicU64;
    static CyberpunkVR_DebugAtkNoMatch: AtomicU64;
    static CyberpunkVR_DebugAtkMask: AtomicU64;
    static mut CyberpunkVR_DebugPosProvCalls: u64;
    static mut CyberpunkVR_DebugPosProvOverrides: u64;
    static mut CyberpunkVR_WeaponName: [u8; 64];
    static mut CyberpunkVR_DualWieldRecords: [u32; 2];
    static mut CyberpunkVR_SightZoom: i32;
    static mut CyberpunkVR_SightDot: i32;
    static mut CyberpunkVR_SightApproach: [f32; 3];
    static mut CyberpunkVR_SightLineOffset: [f32; 3];
    static mut CyberpunkVR_SightLineTick: u64;
    // Elsewhere in the C++ half and the rig.
    static CyberpunkVR_XrDeepDiag: i32;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_CarryLeft: i32;
    static mut CyberpunkVR_TwoHandSuppress: i32;
    static mut CyberpunkVR_WeaponKickDeg: f32;
    static mut CyberpunkVR_WeaponClass: i32;
    static mut CyberpunkVR_WeaponKickDegLeft: f32;
    static mut CyberpunkVR_WeaponClassLeft: i32;
    static mut CyberpunkVR_PhysicalCrouchApplied: i32;
    static mut CyberpunkVR_PhysicalCrouchActual: i32;
    static CyberpunkVR_TwoHandActive: i32;
    static mut CyberpunkVR_LeftTriggerBlockUntil: u64;
    static mut CyberpunkVR_DualWieldUntil: u64;
    static CyberpunkVR_MainIsRightEye: i32;
    static CyberpunkVR_IpdInWorldPos: i32;
    static mut g_VRSprintActive: i32;
    static mut g_VRTakedownState: i32;
    static mut g_VRBodyCarryingState: i32;
    static mut g_VRBodyDisposalState: i32;
    static mut g_VRBodyCarryBothHands: i32;
    static mut g_VRFinisherActive: i32;
    static mut g_VRFinisherSinceMs: u64;
    static mut g_VRWeaponPsmState: f32;
    static mut g_VRAimInRemaining: f32;
    static mut g_VRWeaponRaiseTransition: i32;
    static mut g_VRCamBoneFreeze: i32;
    static mut g_VRPairSlewRate: f32;
    static mut g_VRPairLeadTicks: f32;
    static mut g_VRTwoHandSlide: [f32; 3];
    static mut g_VRTwoHandSlideUntil: u64;
    static g_VRTwoHandCtrlL: [f32; 3];
    static g_VRTwoHandCtrlLValid: i32;
    fn CvrLuaHostGcWanted(want: bool) -> bool;
    fn cvr_ensure_shared_memory();
    fn cvr_anim_player_entity() -> *mut c_void;
    fn cvr_cpp_xr_set_physical_crouch_bridge_ready(ready: bool);
    fn cvr_cpp_physical_optics_stereo_ready() -> bool;
    fn cvr_cpp_set_physical_optics_path(path: i32);
}

// ---- the instrument's state -----------------------------------------------------------------------------------

static PROV_ORIG: [[AtomicUsize; N_SLOTS]; N_CLASSES] = [const { [const { AtomicUsize::new(0) }; N_SLOTS] }; N_CLASSES];
static PROV_CALLS: [[AtomicU64; N_SLOTS]; N_CLASSES] = [const { [const { AtomicU64::new(0) }; N_SLOTS] }; N_CLASSES];
static POS_PROV_ORIG: AtomicUsize = AtomicUsize::new(0);
static PROV_INSTALLED: AtomicI32 = AtomicI32::new(0);
static OVERRIDE_CLS: AtomicI32 = AtomicI32::new(-1);
static OVERRIDE_SLOT: AtomicI32 = AtomicI32::new(-1);
static OVERRIDES: AtomicU64 = AtomicU64::new(0);
static QUAT_MODE: AtomicI32 = AtomicI32::new(1);
static FWD_AXIS: AtomicI32 = AtomicI32::new(1);
static PLAYER_ENTITY: AtomicUsize = AtomicUsize::new(0);
static MUZZLE_POS_SEQ: AtomicU32 = AtomicU32::new(0);
static DELTA_SEQ: AtomicU32 = AtomicU32::new(0xFFFF_FFFF);

/// A quaternion or vector the stubs and the setters share, as atomic lanes (the C++ kept plain volatile floats).
struct Lanes<const N: usize>([AtomicU32; N]);
impl<const N: usize> Lanes<N> {
    const fn new(v: [f32; N]) -> Self {
        let mut lanes = [const { AtomicU32::new(0) }; N];
        let mut i = 0;
        while i < N {
            lanes[i] = AtomicU32::new(v[i].to_bits());
            i += 1;
        }
        Lanes(lanes)
    }
    fn get(&self) -> [f32; N] {
        core::array::from_fn(|i| f32::from_bits(self.0[i].load(Ordering::Relaxed)))
    }
    fn set(&self, v: [f32; N]) {
        for (lane, x) in self.0.iter().zip(v) {
            lane.store(x.to_bits(), Ordering::Relaxed);
        }
    }
}
static LAST_Q: Lanes<4> = Lanes::new([0.0, 0.0, 0.0, 1.0]);
static ORIG_Q: Lanes<4> = Lanes::new([0.0, 0.0, 0.0, 1.0]);
static CTRL_Q: Lanes<4> = Lanes::new([0.0, 0.0, 0.0, 1.0]);
static HMD_Q: Lanes<4> = Lanes::new([0.0, 0.0, 0.0, 1.0]);
static MUZZLE_Q: Lanes<4> = Lanes::new([0.0, 0.0, 0.0, 1.0]);
/// The muzzle's world orientation (+Y the barrel), as SetVRMuzzleQuat last gave it.
pub fn muzzle_q() -> [f32; 4] {
    MUZZLE_Q.get()
}
/// When the weapon mechanic last reported the drawn gun is a smart one (GetTickCount64 ms; 0 = not smart): a smart
/// gun's lock cone follows the barrel while this stays fresh (weapon_aim.rs, the targeting update).
pub static SMART_WEAPON_STAMP_MS: AtomicU64 = AtomicU64::new(0);
/// SetVRSmartWeapon calls, and how many said smart.
#[no_mangle]
pub static CyberpunkVR_SmartWeaponReports: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_SmartWeaponSmartReports: AtomicU64 = AtomicU64::new(0);
static MUZZLE_POS: Lanes<3> = Lanes::new([0.0; 3]);
static DELTA_Q: Lanes<4> = Lanes::new([0.0, 0.0, 0.0, 1.0]);
static AIM_HIT: Lanes<3> = Lanes::new([0.0; 3]);
static AIM_NORMAL: Lanes<3> = Lanes::new([0.0; 3]);
static AIM_HIT_SEQ: AtomicU32 = AtomicU32::new(0);
static AIM_HIT_STAMP_MS: AtomicU64 = AtomicU64::new(0);

fn shared(slot: usize) -> f32 {
    // SAFETY: the mapped shared block, read only when it is there.
    unsafe {
        if g_pSharedHands.is_null() {
            0.0
        } else {
            core::ptr::read_volatile(g_pSharedHands.add(slot))
        }
    }
}
fn have_shared() -> bool {
    // SAFETY: a pointer read.
    unsafe { !g_pSharedHands.is_null() }
}
unsafe fn set_shared(slot: usize, v: f32) {
    if !g_pSharedHands.is_null() {
        core::ptr::write_volatile(g_pSharedHands.add(slot), v);
    }
}
fn tick_ms() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { GetTickCount64() }
}

/// A world orientation whose chosen forward axis (0 +X, 1 +Y, 2 +Z) is the forward vector, up world +Z.
fn build_orient_from_fwd(mut fx: f32, mut fy: f32, mut fz: f32, fwd_axis: i32) -> [f32; 4] {
    let fl = sqrt(fx * fx + fy * fy + fz * fz);
    if fl < 1e-4 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    fx /= fl;
    fy /= fl;
    fz /= fl;
    let (mut ux, mut uy, mut uz) = (0.0f32, 0.0f32, 1.0f32);
    if fz.abs() > 0.95 {
        ux = 0.0;
        uy = 1.0;
        uz = 0.0;
    }
    let (mut rx, mut ry, mut rz) = (fy * uz - fz * uy, fz * ux - fx * uz, fx * uy - fy * ux);
    let mut rl = sqrt(rx * rx + ry * ry + rz * rz);
    if rl < 1e-4 {
        rx = 1.0;
        ry = 0.0;
        rz = 0.0;
        rl = 1.0;
    }
    rx /= rl;
    ry /= rl;
    rz /= rl;
    let (vx, vy, vz) = (ry * fz - rz * fy, rz * fx - rx * fz, rx * fy - ry * fx);
    let (m00, m01, m02, m10, m11, m12, m20, m21, m22);
    if fwd_axis == 1 {
        m00 = rx;
        m10 = ry;
        m20 = rz;
        m01 = fx;
        m11 = fy;
        m21 = fz;
        m02 = vx;
        m12 = vy;
        m22 = vz;
    } else if fwd_axis == 0 {
        m00 = fx;
        m10 = fy;
        m20 = fz;
        m01 = rx;
        m11 = ry;
        m21 = rz;
        m02 = vx;
        m12 = vy;
        m22 = vz;
    } else {
        m00 = rx;
        m10 = ry;
        m20 = rz;
        m01 = vx;
        m11 = vy;
        m21 = vz;
        m02 = fx;
        m12 = fy;
        m22 = fz;
    }
    let mut q = [0.0f32; 4];
    let tr = m00 + m11 + m22;
    if tr > 0.0 {
        let s = sqrt(tr + 1.0) * 2.0;
        q[3] = 0.25 * s;
        q[0] = (m21 - m12) / s;
        q[1] = (m02 - m20) / s;
        q[2] = (m10 - m01) / s;
    } else if m00 > m11 && m00 > m22 {
        let s = sqrt(1.0 + m00 - m11 - m22) * 2.0;
        q[3] = (m21 - m12) / s;
        q[0] = 0.25 * s;
        q[1] = (m01 + m10) / s;
        q[2] = (m02 + m20) / s;
    } else if m11 > m22 {
        let s = sqrt(1.0 + m11 - m00 - m22) * 2.0;
        q[3] = (m02 - m20) / s;
        q[0] = (m01 + m10) / s;
        q[1] = 0.25 * s;
        q[2] = (m12 + m21) / s;
    } else {
        let s = sqrt(1.0 + m22 - m00 - m11) * 2.0;
        q[3] = (m10 - m01) / s;
        q[0] = (m02 + m20) / s;
        q[1] = (m12 + m21) / s;
        q[2] = 0.25 * s;
    }
    q
}
/// o = q * v * q^-1 (q = x, y, z, w).
fn prov_rot_vec(q: &[f32; 4], v: &[f32; 3]) -> [f32; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let tx = 2.0 * (y * v[2] - z * v[1]);
    let ty = 2.0 * (z * v[0] - x * v[2]);
    let tz = 2.0 * (x * v[1] - y * v[0]);
    [
        v[0] + w * tx + (y * tz - z * ty),
        v[1] + w * ty + (z * tx - x * tz),
        v[2] + w * tz + (x * ty - y * tx),
    ]
}
fn plausible(p: usize) -> bool {
    (0x10000..0x0000_7FFF_FFFF_FFFF).contains(&p) && p & 7 == 0
}
fn read_usize(at: usize) -> Option<usize> {
    cvr_engine::code::read::<usize>(at)
}
/// The attack's participant handles that hold the player (bit off/8 each), or None when unreadable.
fn attack_mask(attack: usize, player: usize) -> Option<u64> {
    let mut found = 0u64;
    for off in ATTACK_HANDLE_OFFSETS {
        if read_usize(attack + off)? == player {
            found |= 1 << (off / 8);
        }
    }
    Some(found)
}
/// Whether this orientation provider's attack is the local player's (false when it cannot tell). Each class keeps
/// its owner differently: entEntity names it (+0x58), entStatic has none, entFunc reaches the attack through its
/// source (+0x78, the attack at source+0x10).
fn prov_is_players_attack(class: usize, provider: usize) -> bool {
    let player = PLAYER_ENTITY.load(Ordering::Relaxed);
    if player == 0 || !plausible(provider) || class == 1 {
        return false;
    }
    if class == 0 {
        return read_usize(provider + 0x58) == Some(player);
    }
    let Some(src) = read_usize(provider + 0x78) else { return false };
    if !plausible(src) {
        return false;
    }
    let Some(attack) = read_usize(src + 0x10) else { return false };
    if !plausible(attack) {
        return false;
    }
    let diagnostics = unsafe { CyberpunkVR_RuntimeDiagnostics != 0 };
    // SAFETY: diagnostic counters (CVR_DIAGNOSTIC).
    if diagnostics {
        unsafe { CyberpunkVR_DebugAtkCalls.fetch_add(1, Ordering::Relaxed) };
    }
    match attack_mask(attack, player) {
        Some(0) => {
            if diagnostics {
                unsafe { CyberpunkVR_DebugAtkNoMatch.fetch_add(1, Ordering::Relaxed) };
            }
            false
        }
        Some(found) => {
            if diagnostics {
                unsafe { CyberpunkVR_DebugAtkMask.fetch_or(found, Ordering::Relaxed) };
            }
            true
        }
        None => false,
    }
}
/// The position provider's owner test: its source at +0x88, the attack at source+0x10.
fn pos_prov_is_players_attack(provider: usize) -> bool {
    let player = PLAYER_ENTITY.load(Ordering::Relaxed);
    if player == 0 || !plausible(provider) {
        return false;
    }
    let Some(src) = read_usize(provider + 0x88) else { return false };
    if !plausible(src) {
        return false;
    }
    let Some(attack) = read_usize(src + 0x10) else { return false };
    if !plausible(attack) {
        return false;
    }
    matches!(attack_mask(attack, player), Some(m) if m != 0)
}
/// Natives/Natives.cpp IsReadable: committed, accessible memory covering n bytes (a VirtualQuery).
fn is_readable(a: usize, n: usize) -> bool {
    use windows_sys::Win32::System::Memory::*;
    if a < 0x10000 {
        return false;
    }
    // SAFETY: an out-parameter query of an address.
    let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    if unsafe { VirtualQuery(a as *const c_void, &mut mbi, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 {
        return false;
    }
    if mbi.State != MEM_COMMIT || mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0 {
        return false;
    }
    let readable = PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_WRITECOPY;
    if mbi.Protect & readable == 0 {
        return false;
    }
    a + n <= mbi.BaseAddress as usize + mbi.RegionSize
}

type ProvFn = unsafe extern "C" fn(usize, usize, usize, usize) -> usize;
fn call_original(original: usize, rcx: usize, rdx: usize, r8: usize, r9: usize) -> usize {
    if original == 0 {
        return 0;
    }
    // SAFETY: the slot's original function, with the arguments the engine passed.
    unsafe { core::mem::transmute::<usize, ProvFn>(original)(rcx, rdx, r8, r9) }
}

/// entFuncPositionProvider slot 33 (GetWorldPosition(this, Vector3* out)): the player's projectile starts at the
/// published muzzle -- on the sight line above it with the eye on a sight.
unsafe extern "C" fn pos_prov_stub33(rcx: usize, rdx: usize, r8: usize, r9: usize) -> usize {
    let ret = call_original(POS_PROV_ORIG.load(Ordering::Relaxed), rcx, rdx, r8, r9);
    if CyberpunkVR_RuntimeDiagnostics != 0 {
        CyberpunkVR_DebugPosProvCalls += 1;
    }
    if !have_shared() || MUZZLE_POS_SEQ.load(Ordering::Relaxed) == 0 || shared(SLOT_WEAPON_FLAG) <= 0.5 || shared(27) <= 0.5 || !pos_prov_is_players_attack(rcx)
    {
        return ret;
    }
    let mut p = MUZZLE_POS.get();
    let l2 = p[0] * p[0] + p[1] * p[1] + p[2] * p[2];
    if !l2.is_finite() || l2 <= 1.0 {
        return ret;
    }
    let forward = [shared(24), shared(25), shared(26)];
    if let Some(lift) = super::weapon_aim::sight_line_offset(&forward) {
        p[0] += lift[0];
        p[1] += lift[1];
        p[2] += lift[2];
    }
    let out = if rdx != 0 { rdx } else { ret };
    if !plausible(out & !7) {
        return ret;
    }
    if cvr_engine::code::write::<[f32; 3]>(out, p) {
        CyberpunkVR_DebugPosProvOverrides += 1;
    }
    ret
}

/// One instrumented slot (class C, stub S = real slot - 3): count, sample and -- where it carries the shot -- override
/// the out-quaternion, after calling the original.
unsafe extern "C" fn prov_stub<const C: usize, const S: usize>(rcx: usize, rdx: usize, r8: usize, r9: usize) -> usize {
    PROV_CALLS[C][S].fetch_add(1, Ordering::Relaxed);
    if C == 2 && (30..46).contains(&S) && CyberpunkVR_RuntimeDiagnostics != 0 {
        CyberpunkVR_DebugProvEntFunc[S - 30] += 1;
    }
    let ret = call_original(PROV_ORIG[C][S].load(Ordering::Relaxed), rcx, rdx, r8, r9);
    let outp = if rdx != 0 { rdx } else { ret };
    // The out-buffer is read only where it is needed (a VirtualQuery per call is visible frame time on the stubs every
    // provider in the world runs through): the slot carrying the shot, the manual override, or the debug sampling.
    let carries_shot = S == 30 || (C == 2 && (S == 33 || S == 34));
    let need_out =
        carries_shot || (OVERRIDE_CLS.load(Ordering::Relaxed) == C as i32 && OVERRIDE_SLOT.load(Ordering::Relaxed) == S as i32) || CyberpunkVR_XrDeepDiag != 0;
    if !need_out || !is_readable(outp, 16) {
        return ret;
    }
    let Some(mut q) = cvr_engine::code::read::<[f32; 4]>(outp) else { return ret };
    let m = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if !(m > 0.9 && m < 1.1) {
        return ret;
    }
    LAST_Q.set(q);
    if C == 2 && (S == 30 || S == 33 || S == 34) {
        // The hand recoil, once per round, for the player's projectile launch (real slot 33) only.
        if S == 30 && prov_is_players_attack(C, rcx) {
            let seq = g_provMuzzleSeq.load(Ordering::Relaxed) as i32;
            let seen = g_provRecoilSeqSeen.load(Ordering::Relaxed);
            if seen != seq && g_provRecoilSeqSeen.compare_exchange(seen, seq, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                RecoilOnShot();
            }
        }
        ORIG_Q.set(q);
        if have_shared() {
            CTRL_Q.set([shared(12), shared(13), shared(14), shared(15)]);
            HMD_Q.set([shared(16), shared(17), shared(18), shared(19)]);
        }
    }
    // The launch override: slot 33 on any provider class, for the player's own attack with a weapon out and its
    // muzzle published; the native SetVRProvOverrideSlot path as the manual fallback.
    let shared_on = have_shared() && shared(SLOT_WEAPON_FLAG) > 0.5 && shared(27) > 0.5 && S == 30 && prov_is_players_attack(C, rcx);
    let native_on = OVERRIDE_CLS.load(Ordering::Relaxed) == C as i32 && OVERRIDE_SLOT.load(Ordering::Relaxed) == S as i32;
    let mut changed = false;
    if (shared_on || native_on) && have_shared() {
        let mode = if shared_on { 6 } else { QUAT_MODE.load(Ordering::Relaxed) };
        let mul = |a: [f32; 4], d: [f32; 4]| -> [f32; 4] {
            let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
            let (dx, dy, dz, dw) = (d[0], d[1], d[2], d[3]);
            [
                aw * dx + ax * dw + ay * dz - az * dy,
                aw * dy - ax * dz + ay * dw + az * dx,
                aw * dz + ax * dy - ay * dx + az * dw,
                aw * dw - ax * dx - ay * dy - az * dz,
            ]
        };
        match mode {
            6 => {
                // The muzzle with the spread kept: the whole cone turned onto the barrel (delta from the frame's
                // first pellet).
                let mq = MUZZLE_Q.get();
                if mq[0] * mq[0] + mq[1] * mq[1] + mq[2] * mq[2] + mq[3] * mq[3] > 0.5 {
                    let seq = g_provMuzzleSeq.load(Ordering::Relaxed);
                    if DELTA_SEQ.load(Ordering::Relaxed) != seq {
                        let (cqx, cqy, cqz, cqw) = (-q[0], -q[1], -q[2], q[3]);
                        DELTA_Q.set([
                            mq[3] * cqx + mq[0] * cqw + mq[1] * cqz - mq[2] * cqy,
                            mq[3] * cqy - mq[0] * cqz + mq[1] * cqw + mq[2] * cqx,
                            mq[3] * cqz + mq[0] * cqy - mq[1] * cqx + mq[2] * cqw,
                            mq[3] * cqw - mq[0] * cqx - mq[1] * cqy - mq[2] * cqz,
                        ]);
                        DELTA_SEQ.store(seq, Ordering::Relaxed);
                    }
                    let d = DELTA_Q.get();
                    let (dx, dy, dz, dw) = (d[0], d[1], d[2], d[3]);
                    let (ax, ay, az, aw) = (q[0], q[1], q[2], q[3]);
                    q = [
                        dw * ax + dx * aw + dy * az - dz * ay,
                        dw * ay - dx * az + dy * aw + dz * ax,
                        dw * az + dx * ay - dy * ax + dz * aw,
                        dw * aw - dx * ax - dy * ay - dz * az,
                    ];
                    changed = true;
                }
            }
            5 => {
                let mq = MUZZLE_Q.get();
                if mq[0] * mq[0] + mq[1] * mq[1] + mq[2] * mq[2] + mq[3] * mq[3] > 0.5 {
                    let axis = FWD_AXIS.load(Ordering::Relaxed);
                    if axis == 1 {
                        q = mq;
                    } else {
                        let mut ax = [0.0f32; 3];
                        ax[if axis == 0 { 0 } else { 2 }] = 1.0;
                        let fw = prov_rot_vec(&mq, &ax);
                        q = build_orient_from_fwd(fw[0], fw[1], fw[2], 1);
                    }
                    changed = true;
                }
            }
            4 => {
                let cq = [shared(12), shared(13), shared(14), shared(15)];
                let hq = [shared(16), shared(17), shared(18), shared(19)];
                if cq.iter().map(|v| v * v).sum::<f32>() > 0.5 && hq.iter().map(|v| v * v).sum::<f32>() > 0.5 {
                    let cb = prov_rot_vec(&cq, &[0.0, 0.0, -1.0]);
                    let ch = prov_rot_vec(&[-hq[0], -hq[1], -hq[2], hq[3]], &cb);
                    let cw = prov_rot_vec(&q, &ch);
                    q = build_orient_from_fwd(cw[0], cw[1], cw[2], 1);
                    changed = true;
                }
            }
            3 => {
                let (cqx, cqy, cqz, cqw) = (shared(12), shared(13), shared(14), shared(15));
                let (hqx, hqy, hqz, hqw) = (shared(16), shared(17), shared(18), shared(19));
                if cqx * cqx + cqy * cqy + cqz * cqz + cqw * cqw > 0.5 && hqx * hqx + hqy * hqy + hqz * hqz + hqw * hqw > 0.5 {
                    let sc = [cqx, -cqz, cqy, cqw];
                    let sh = [hqx, -hqz, hqy, hqw];
                    let ih = [-sh[0], -sh[1], -sh[2], sh[3]];
                    let d = [
                        ih[3] * sc[0] + ih[0] * sc[3] + ih[1] * sc[2] - ih[2] * sc[1],
                        ih[3] * sc[1] - ih[0] * sc[2] + ih[1] * sc[3] + ih[2] * sc[0],
                        ih[3] * sc[2] + ih[0] * sc[1] - ih[1] * sc[0] + ih[2] * sc[3],
                        ih[3] * sc[3] - ih[0] * sc[0] - ih[1] * sc[1] - ih[2] * sc[2],
                    ];
                    q = mul(q, d);
                    changed = true;
                }
            }
            2 => {
                let (hx, hy, hz, hw) = (shared(12), shared(13), shared(14), shared(15));
                if hx * hx + hy * hy + hz * hz + hw * hw > 0.5 {
                    q = mul(q, [hx, -hz, hy, hw]);
                    changed = true;
                }
            }
            1 => {
                let (fx, fy, fz) = (shared(60), shared(61), shared(62));
                if fx * fx + fy * fy + fz * fz > 0.25 {
                    q = build_orient_from_fwd(fx, fy, fz, FWD_AXIS.load(Ordering::Relaxed));
                    changed = true;
                }
            }
            _ => {
                let c = [shared(53), shared(54), shared(55), shared(56)];
                if c[0] * c[0] + c[1] * c[1] + c[2] * c[2] + c[3] * c[3] > 0.5 {
                    q = c;
                    changed = true;
                }
            }
        }
    }
    if changed && cvr_engine::code::write::<[f32; 4]>(outp, q) {
        OVERRIDES.fetch_add(1, Ordering::Relaxed);
    }
    ret
}

#[rustfmt::skip]
static STUBS: [[ProvFn; N_SLOTS]; N_CLASSES] = [
    [prov_stub::<0, 0>, prov_stub::<0, 1>, prov_stub::<0, 2>, prov_stub::<0, 3>, prov_stub::<0, 4>, prov_stub::<0, 5>, prov_stub::<0, 6>, prov_stub::<0, 7>, prov_stub::<0, 8>, prov_stub::<0, 9>, prov_stub::<0, 10>, prov_stub::<0, 11>, prov_stub::<0, 12>, prov_stub::<0, 13>, prov_stub::<0, 14>, prov_stub::<0, 15>, prov_stub::<0, 16>, prov_stub::<0, 17>, prov_stub::<0, 18>, prov_stub::<0, 19>, prov_stub::<0, 20>, prov_stub::<0, 21>, prov_stub::<0, 22>, prov_stub::<0, 23>, prov_stub::<0, 24>, prov_stub::<0, 25>, prov_stub::<0, 26>, prov_stub::<0, 27>, prov_stub::<0, 28>, prov_stub::<0, 29>, prov_stub::<0, 30>, prov_stub::<0, 31>, prov_stub::<0, 32>, prov_stub::<0, 33>, prov_stub::<0, 34>, prov_stub::<0, 35>, prov_stub::<0, 36>, prov_stub::<0, 37>, prov_stub::<0, 38>, prov_stub::<0, 39>, prov_stub::<0, 40>, prov_stub::<0, 41>, prov_stub::<0, 42>, prov_stub::<0, 43>, prov_stub::<0, 44>, prov_stub::<0, 45>, prov_stub::<0, 46>, prov_stub::<0, 47>],
    [prov_stub::<1, 0>, prov_stub::<1, 1>, prov_stub::<1, 2>, prov_stub::<1, 3>, prov_stub::<1, 4>, prov_stub::<1, 5>, prov_stub::<1, 6>, prov_stub::<1, 7>, prov_stub::<1, 8>, prov_stub::<1, 9>, prov_stub::<1, 10>, prov_stub::<1, 11>, prov_stub::<1, 12>, prov_stub::<1, 13>, prov_stub::<1, 14>, prov_stub::<1, 15>, prov_stub::<1, 16>, prov_stub::<1, 17>, prov_stub::<1, 18>, prov_stub::<1, 19>, prov_stub::<1, 20>, prov_stub::<1, 21>, prov_stub::<1, 22>, prov_stub::<1, 23>, prov_stub::<1, 24>, prov_stub::<1, 25>, prov_stub::<1, 26>, prov_stub::<1, 27>, prov_stub::<1, 28>, prov_stub::<1, 29>, prov_stub::<1, 30>, prov_stub::<1, 31>, prov_stub::<1, 32>, prov_stub::<1, 33>, prov_stub::<1, 34>, prov_stub::<1, 35>, prov_stub::<1, 36>, prov_stub::<1, 37>, prov_stub::<1, 38>, prov_stub::<1, 39>, prov_stub::<1, 40>, prov_stub::<1, 41>, prov_stub::<1, 42>, prov_stub::<1, 43>, prov_stub::<1, 44>, prov_stub::<1, 45>, prov_stub::<1, 46>, prov_stub::<1, 47>],
    [prov_stub::<2, 0>, prov_stub::<2, 1>, prov_stub::<2, 2>, prov_stub::<2, 3>, prov_stub::<2, 4>, prov_stub::<2, 5>, prov_stub::<2, 6>, prov_stub::<2, 7>, prov_stub::<2, 8>, prov_stub::<2, 9>, prov_stub::<2, 10>, prov_stub::<2, 11>, prov_stub::<2, 12>, prov_stub::<2, 13>, prov_stub::<2, 14>, prov_stub::<2, 15>, prov_stub::<2, 16>, prov_stub::<2, 17>, prov_stub::<2, 18>, prov_stub::<2, 19>, prov_stub::<2, 20>, prov_stub::<2, 21>, prov_stub::<2, 22>, prov_stub::<2, 23>, prov_stub::<2, 24>, prov_stub::<2, 25>, prov_stub::<2, 26>, prov_stub::<2, 27>, prov_stub::<2, 28>, prov_stub::<2, 29>, prov_stub::<2, 30>, prov_stub::<2, 31>, prov_stub::<2, 32>, prov_stub::<2, 33>, prov_stub::<2, 34>, prov_stub::<2, 35>, prov_stub::<2, 36>, prov_stub::<2, 37>, prov_stub::<2, 38>, prov_stub::<2, 39>, prov_stub::<2, 40>, prov_stub::<2, 41>, prov_stub::<2, 42>, prov_stub::<2, 43>, prov_stub::<2, 44>, prov_stub::<2, 45>, prov_stub::<2, 46>, prov_stub::<2, 47>],
];

/// CClass::CreateInstance(zero): a default instance of the class (the engine's own allocator and constructor).
unsafe fn create_instance(class: &Class) -> *mut c_void {
    static CREATE: red4ext_abi::reloc::Reloc = red4ext_abi::reloc::Reloc::new(red4ext_abi::hashes::CClass_CreateInstance);
    type F = unsafe extern "C" fn(*const Class, u32, bool) -> *mut c_void;
    let Some(f) = CREATE.func::<F>() else { return core::ptr::null_mut() };
    let size = (*(class as *const Class as *const IType)).size();
    f(class, size, true)
}
/// A vtable's slots [first, first + count) replaced by `stubs`, the originals into `originals`.
unsafe fn patch_vtable(vt: usize, first: usize, stubs: &[usize], originals: &[AtomicUsize]) -> bool {
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
    let base = (vt + first * 8) as *mut usize;
    let mut old = 0u32;
    if VirtualProtect(base as *const c_void, stubs.len() * 8, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        return false;
    }
    for (i, (&stub, original)) in stubs.iter().zip(originals).enumerate() {
        original.store(*base.add(i), Ordering::Relaxed);
        *base.add(i) = stub;
    }
    VirtualProtect(base as *const c_void, stubs.len() * 8, old, &mut old);
    true
}
/// The class's instance vtable, read from an instance made for it.
unsafe fn instance_vtable(name: &str) -> Result<usize, i32> {
    let rtti = RttiSystem::get().ok_or(-1)?;
    let class = rtti.get_class(CName::hashed(name)).ok_or(-1)?;
    let instance = create_instance(class);
    if instance.is_null() {
        return Err(-2);
    }
    match read_usize(instance as usize) {
        Some(vt) if vt != 0 => Ok(vt),
        _ => Err(-3),
    }
}
unsafe fn install_prov_class(c: usize) -> i32 {
    let vt = match instance_vtable(CLASS_NAMES[c]) {
        Ok(vt) => vt,
        Err(e) => return e,
    };
    let stubs: Vec<usize> = STUBS[c].iter().map(|&f| f as *const () as usize).collect();
    if patch_vtable(vt, SLOT_LO, &stubs, &PROV_ORIG[c]) {
        1
    } else {
        -4
    }
}
unsafe fn install_position_prov_class() -> i32 {
    let vt = match instance_vtable("entFuncPositionProvider") {
        Ok(vt) => vt,
        Err(e) => return e,
    };
    if patch_vtable(vt, 33, &[pos_prov_stub33 as *const () as usize], core::slice::from_ref(&POS_PROV_ORIG)) {
        1
    } else {
        -4
    }
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

/// InstallVRProvInstrument() -> Int32: the provider classes instrumented (4 expected; 2 when already in).
#[no_mangle]
pub unsafe extern "C" fn InstallVRProvInstrument(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    if PROV_INSTALLED.load(Ordering::Relaxed) != 0 {
        put(out, 2);
        return;
    }
    let mut ok = 0;
    for c in 0..N_CLASSES {
        if install_prov_class(c) == 1 {
            ok += 1;
        }
    }
    if install_position_prov_class() == 1 {
        ok += 1;
    }
    PROV_INSTALLED.store((ok > 0) as i32, Ordering::Relaxed);
    put(out, ok);
}
/// SetVRProvOverrideSlot(class * 1000 + real slot; -1 off).
#[no_mangle]
pub unsafe extern "C" fn SetVRProvOverrideSlot(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut v = -1i32;
    params!(frame, v);
    if v < 0 {
        OVERRIDE_CLS.store(-1, Ordering::Relaxed);
        OVERRIDE_SLOT.store(-1, Ordering::Relaxed);
    } else {
        OVERRIDE_CLS.store(v / 1000, Ordering::Relaxed);
        OVERRIDE_SLOT.store(v % 1000 - SLOT_LO as i32, Ordering::Relaxed);
    }
}
/// SetVRProvQuatMode(mode, forward axis).
#[no_mangle]
pub unsafe extern "C" fn SetVRProvQuatMode(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut mode, mut axis) = (1i32, 1i32);
    params!(frame, mode, axis);
    QUAT_MODE.store(mode, Ordering::Relaxed);
    FWD_AXIS.store(axis, Ordering::Relaxed);
}
/// SetVRMuzzleQuat(i, j, k, r): the muzzle's world orientation, once a frame while a weapon is out; it also caches
/// the player's entity (the stubs run on workers and must never look it up) and publishes the barrel's forward.
#[no_mangle]
pub unsafe extern "C" fn SetVRMuzzleQuat(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut i, mut j, mut k, mut r) = (0.0f32, 0.0f32, 0.0f32, 1.0f32);
    params!(frame, i, j, k, r);
    MUZZLE_Q.set([i, j, k, r]);
    g_provMuzzleSeq.fetch_add(1, Ordering::Relaxed);
    static PLAYER_REFRESH: AtomicU32 = AtomicU32::new(0);
    if PLAYER_REFRESH.fetch_add(1, Ordering::Relaxed) % 180 == 0 {
        let player = cvr_anim_player_entity();
        if !player.is_null() {
            PLAYER_ENTITY.store(player as usize, Ordering::Relaxed);
        }
    }
    if have_shared() {
        set_shared(24, 2.0 * (i * j - k * r));
        set_shared(25, 1.0 - 2.0 * (i * i + k * k));
        set_shared(26, 2.0 * (j * k + i * r));
        set_shared(27, 1.0);
    }
}
/// SetVRMuzzlePos(x, y, z): the muzzle's world position (a near-origin local answer keeps the last world sample).
#[no_mangle]
pub unsafe extern "C" fn SetVRMuzzlePos(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut x, mut y, mut z) = (0.0f32, 0.0f32, 0.0f32);
    params!(frame, x, y, z);
    if x * x + y * y + z * z < 1.0 {
        return;
    }
    MUZZLE_POS.set([x, y, z]);
    MUZZLE_POS_SEQ.fetch_add(1, Ordering::Relaxed);
    if have_shared() {
        set_shared(200, x);
        set_shared(201, y);
        set_shared(202, z);
        set_shared(203, 1.0);
    }
}
/// SetVRCarryLeft(on): the weapon moves into the left slot.
#[no_mangle]
pub unsafe extern "C" fn SetVRCarryLeft(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    CyberpunkVR_CarryLeft = (on != 0) as i32;
}
/// SetVRTwoHandSuppress(mask): 1 the whole hold, bit k+1 only the weapon's k-th hold.
#[no_mangle]
pub unsafe extern "C" fn SetVRTwoHandSuppress(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    CyberpunkVR_TwoHandSuppress = if on > 0 { on } else { 0 };
}
/// GetVRCarryOffset(idx) -> Float.
#[no_mangle]
pub unsafe extern "C" fn GetVRCarryOffset(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let mut idx = 0i32;
    params!(frame, idx);
    let v = VRTwoHandCarryOffset(idx);
    put(out, v);
}
/// SetVRAimHit(x, y, z, nx, ny, nz): the aim line's hit point and surface normal (near-origin answers dropped).
#[no_mangle]
pub unsafe extern "C" fn SetVRAimHit(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut x, mut y, mut z, mut nx, mut ny, mut nz) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    params!(frame, x, y, z, nx, ny, nz);
    if x * x + y * y + z * z < 1.0 {
        return;
    }
    AIM_HIT.set([x, y, z]);
    AIM_NORMAL.set([nx, ny, nz]);
    AIM_HIT_SEQ.fetch_add(1, Ordering::Relaxed);
    AIM_HIT_STAMP_MS.store(tick_ms(), Ordering::Relaxed);
}
/// SetVRWeaponName(name): the weapon family in hand, as a file-name-safe lower-case key.
#[no_mangle]
pub unsafe extern "C" fn SetVRWeaponName(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut s = CString::empty();
    params!(frame, s);
    let name = &mut *(&raw mut CyberpunkVR_WeaponName);
    let mut j = 0usize;
    for c in s.as_str().bytes() {
        if j >= 63 {
            break;
        }
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' {
            name[j] = c;
            j += 1;
        } else if c.is_ascii_uppercase() {
            name[j] = c.to_ascii_lowercase();
            j += 1;
        }
    }
    name[j] = 0;
}
/// SetVRSmartWeapon(smart): the drawn gun's evolution, once a frame beside SetVRMuzzleQuat.
#[no_mangle]
pub unsafe extern "C" fn SetVRSmartWeapon(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut smart = false;
    params!(frame, smart);
    let now = windows_sys::Win32::System::SystemInformation::GetTickCount64();
    SMART_WEAPON_STAMP_MS.store(if smart { now } else { 0 }, Ordering::Relaxed);
    CyberpunkVR_SmartWeaponReports.fetch_add(1, Ordering::Relaxed);
    CyberpunkVR_SmartWeaponSmartReports.fetch_add(u64::from(smart), Ordering::Relaxed);
}
/// SetVRAttachmentRecoil(side, scale): the installed parts' factor on a gun's hand recoil (1 the weapon in hand, 0 the
/// left gun), from the weapon mechanic's attachment recoil.
#[no_mangle]
pub unsafe extern "C" fn SetVRAttachmentRecoil(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut side, mut scale) = (0i32, 1.0f32);
    params!(frame, side, scale);
    super::recoil::set_attachment_scale(side, scale);
}
/// GetVRLastShotPeak(side) -> Vector4: the gun's last shot's largest sampled rise in degrees (x) and its sequence's low
/// and high 16 bits (y, z).
#[no_mangle]
pub unsafe extern "C" fn GetVRLastShotPeak(_this: *mut c_void, frame: *mut Frame, out: *mut [f32; 4], _unused: i64) {
    let mut side = 0i32;
    params!(frame, side);
    let (seq, degrees) = super::recoil::last_shot_peak(side);
    put(out, [degrees, (seq & 0xffff) as f32, (seq >> 16) as f32, 0.0]);
}
/// SetVRWeaponKick(degrees): the weapon's own RecoilKickMax for the hand spring.
#[no_mangle]
pub unsafe extern "C" fn SetVRWeaponKick(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut k = 0.0f32;
    params!(frame, k);
    CyberpunkVR_WeaponKickDeg = if k > 0.0 && k < 100.0 { k } else { 0.0 };
}
/// SetVRWeaponClass(class): 0 unknown .. 5 melee.
#[no_mangle]
pub unsafe extern "C" fn SetVRWeaponClass(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut c = 0i32;
    params!(frame, c);
    CyberpunkVR_WeaponClass = if (0..=5).contains(&c) { c } else { 0 };
}
/// SetVRLeftWeaponKick(kick, class): the left gun's, while two guns are in hand.
#[no_mangle]
pub unsafe extern "C" fn SetVRLeftWeaponKick(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut k, mut c) = (0.0f32, 0i32);
    params!(frame, k, c);
    CyberpunkVR_WeaponKickDegLeft = if k > 0.0 && k < 100.0 { k } else { 0.0 };
    CyberpunkVR_WeaponClassLeft = if (0..=5).contains(&c) { c } else { 0 };
}
/// VRHapticEvent(kind, hands).
#[no_mangle]
pub unsafe extern "C" fn VRHapticEvent(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut kind, mut hands) = (0i32, 0i32);
    params!(frame, kind, hands);
    cvr_haptics_play(kind, hands);
}
macro_rules! state_setter {
    ($name:ident, $doc:literal, $v:ident: $t:ty = $init:expr, $body:block) => {
        #[doc = $doc]
        #[no_mangle]
        pub unsafe extern "C" fn $name(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
            let mut $v: $t = $init;
            params!(frame, $v);
            $body
            put(out, 1);
        }
    };
}
state_setter!(SetVRSprintActive, "SetVRSprintActive(active) -> Int32: the game's own sprint toggle.", active: i32 = 0, {
    g_VRSprintActive = (active != 0) as i32;
});
state_setter!(SetVRLocomotionState, "SetVRLocomotionState(state) -> Int32 (gamePSMLocomotionStates; negative unknown).", state: i32 = -1, {
    g_VRLocomotionState = state;
});
state_setter!(SetVRLocomotionDetailed, "SetVRLocomotionDetailed(state) -> Int32 (gamePSMDetailedLocomotionStates).", state: i32 = -1, {
    g_VRLocomotionDetailed = state;
});
state_setter!(SetVRScannerState, "SetVRScannerState(state) -> Int32.", state: i32 = 0, {
    cvr_input_publish_scanner_vision(state);
});
state_setter!(SetVRWorldTimeScale, "SetVRWorldTimeScale(scale) -> Int32: the world's time dilation (main's 0.2.4).", scale: f32 = 1.0, {
    cvr_input_world_time_scale(scale);
});
/// SetVRPairedAnimationState(takedown, bodyCarrying, bodyDisposal) -> Int32 (negative unknown).
#[no_mangle]
pub unsafe extern "C" fn SetVRPairedAnimationState(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut takedown, mut carrying, mut disposal) = (-1i32, -1i32, -1i32);
    params!(frame, takedown, carrying, disposal);
    g_VRTakedownState = takedown;
    g_VRBodyCarryingState = carrying;
    g_VRBodyDisposalState = disposal;
    put(out, 1);
}
state_setter!(SetVRBodyCarryBothHands, "SetVRBodyCarryBothHands(bothHands) -> Int32 (AnimFeature_Carry.useBothHands).", both: bool = false, {
    g_VRBodyCarryBothHands = both as i32;
});
/// SetVRReloadAnimation(active, finishIn) -> Int32: the native reload; both arms follow its animation.
#[no_mangle]
pub unsafe extern "C" fn SetVRReloadAnimation(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut active, mut finish_in) = (false, -1.0f32);
    params!(frame, active, finish_in);
    cvr_anim_reload_hand_off_set(active, finish_in);
    put(out, 1);
}
state_setter!(SetVRFinisherActive, "SetVRFinisherActive(active) -> Int32: a finisher's workspot (VRIK steps aside).", active: bool = false, {
    if active && g_VRFinisherActive != 1 {
        g_VRFinisherSinceMs = tick_ms();
    }
    g_VRFinisherActive = active as i32;
});
/// GetVRPhysicalCrouch() -> Int32.
#[no_mangle]
pub unsafe extern "C" fn GetVRPhysicalCrouch(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    put(out, super::xr_calls::physical_crouch_requested() as i32);
}
/// GetVRPhysicalCrouchCameraAnchor() -> Vector4: the body camera in model space from the native pair (W = 1 valid).
#[no_mangle]
pub unsafe extern "C" fn GetVRPhysicalCrouchCameraAnchor(_this: *mut c_void, frame: *mut Frame, out: *mut [f32; 4], _unused: i64) {
    params!(frame);
    let Some(out) = out.as_mut() else { return };
    *out = [0.0; 4];
    let mut sample = VrikTransformSnapshot::default();
    if !VRIK_ReadNativeTransformSnapshot(&mut sample) || sample.valid == 0 || sample.unavailable != 0 {
        return;
    }
    let inverse = [-sample.entityQuat[0], -sample.entityQuat[1], -sample.entityQuat[2], sample.entityQuat[3]];
    let mut model = [0.0f32; 3];
    VRIK_QuatRotateVec(inverse.as_ptr(), sample.bodyCameraMinusEntity.as_ptr(), model.as_mut_ptr());
    if model.iter().all(|v| v.is_finite()) && model[2] > 0.3 && model[2] < 2.5 {
        *out = [model[0], model[1], model[2], 1.0];
    }
}
/// SetVRPhysicalCrouchFeedback(applied, actual) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn SetVRPhysicalCrouchFeedback(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut applied, mut actual) = (-1i32, 0i32);
    params!(frame, applied, actual);
    set_physical_crouch_feedback(applied, actual);
    put(out, 1);
}
/// SetVRPhysicalCrouchFeedback's effect, for Rust (rust/crates/cvr-ffi/src/mechanics.rs physical_crouch).
pub fn set_physical_crouch_feedback(applied: i32, actual: i32) {
    // SAFETY: the bridge flag and the two exports, written on the main thread as the native writes them.
    unsafe {
        cvr_cpp_xr_set_physical_crouch_bridge_ready(applied >= 0);
        CyberpunkVR_PhysicalCrouchApplied = (applied > 0) as i32;
        CyberpunkVR_PhysicalCrouchActual = (actual > 0) as i32;
    }
}
/// SetVRWeaponPoseState(weaponState, aimInRemaining) -> Int32 (the non-VRIK ADS stabilizer's inputs).
#[no_mangle]
pub unsafe extern "C" fn SetVRWeaponPoseState(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    let (mut state, mut remaining) = (0i32, 0.0f32);
    params!(frame, state, remaining);
    g_VRWeaponPsmState = state as f32;
    g_VRAimInRemaining = remaining;
    put(out, 1);
}
state_setter!(SetVRWeaponRaiseTransition, "SetVRWeaponRaiseTransition(active) -> Int32.", active: i32 = 0, {
    g_VRWeaponRaiseTransition = (active != 0) as i32;
});
/// SetVRReloadOwnedHand(hand): the wrist the physical reload owns (anything not 0/1 publishes -1).
#[no_mangle]
pub unsafe extern "C" fn SetVRReloadOwnedHand(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut hand = -1i32;
    params!(frame, hand);
    set_shared(SLOT_RELOAD_OWNED_HAND, if hand == 0 || hand == 1 { hand as f32 } else { -1.0 });
}
/// SetVRTwoHandSlide(x, y, z): the pump racked under the support hand (model space), held 300 ms per call.
#[no_mangle]
pub unsafe extern "C" fn SetVRTwoHandSlide(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut x, mut y, mut z) = (0.0f32, 0.0f32, 0.0f32);
    params!(frame, x, y, z);
    let l2 = x * x + y * y + z * z;
    if !l2.is_finite() || l2 > 0.25 * 0.25 {
        x = 0.0;
        y = 0.0;
        z = 0.0;
    }
    g_VRTwoHandSlide = [x, y, z];
    g_VRTwoHandSlideUntil = if l2 > 0.0 && l2.is_finite() { tick_ms() + 300 } else { 0 };
}
/// GetVRTwoHandActive() -> Bool.
#[no_mangle]
pub unsafe extern "C" fn GetVRTwoHandActive(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    params!(frame);
    put(out, CyberpunkVR_TwoHandActive != 0);
}
/// GetVRTwoHandLeftCtrl() -> Vector4: the left controller in model space as the hold read it (W = 1 valid).
#[no_mangle]
pub unsafe extern "C" fn GetVRTwoHandLeftCtrl(_this: *mut c_void, frame: *mut Frame, out: *mut [f32; 4], _unused: i64) {
    params!(frame);
    let c = g_VRTwoHandCtrlL;
    put(out, [c[0], c[1], c[2], if g_VRTwoHandCtrlLValid != 0 { 1.0 } else { 0.0 }]);
}
/// SetVRLeftTriggerBlock(on): the left trigger works a pump's fore-end (300 ms per call).
#[no_mangle]
pub unsafe extern "C" fn SetVRLeftTriggerBlock(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    CyberpunkVR_LeftTriggerBlockUntil = if on != 0 { tick_ms() + 300 } else { 0 };
}
/// SetVRDualWield(on): two guns in hand (500 ms per call).
#[no_mangle]
pub unsafe extern "C" fn SetVRDualWield(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut on = 0i32;
    params!(frame, on);
    CyberpunkVR_DualWieldUntil = if on != 0 { tick_ms() + 500 } else { 0 };
    if on == 0 {
        publish_dual_weapons(0, 0);
    }
}
/// The two guns' attachment instances while two are in hand (main's 66872b5a7, Hooks/DualWieldWeaponIdentity.hpp):
/// compared only, never dereferenced nor held alive. A shot's AttackData names the firing entity even after its
/// triggers were released (Heywood Hurricane) or when both guns are one model.
static DUAL_WEAPON_LEFT: AtomicUsize = AtomicUsize::new(0);
static DUAL_WEAPON_RIGHT: AtomicUsize = AtomicUsize::new(0);
pub fn publish_dual_weapons(left: usize, right: usize) {
    DUAL_WEAPON_LEFT.store(left, Ordering::Release);
    DUAL_WEAPON_RIGHT.store(right, Ordering::Release);
}
/// ShotWeaponSide: Some(true) the left gun, Some(false) the right, None when the shot's weapon is neither or the two
/// are not told apart.
pub fn shot_weapon_side(shot: usize) -> Option<bool> {
    cvr_input_shot_side(shot, DUAL_WEAPON_LEFT.load(Ordering::Acquire), DUAL_WEAPON_RIGHT.load(Ordering::Acquire))
}
fn cvr_input_shot_side(shot: usize, left: usize, right: usize) -> Option<bool> {
    if shot == 0 || left == 0 || right == 0 || left == right {
        return None;
    }
    if shot == left {
        Some(true)
    } else if shot == right {
        Some(false)
    } else {
        None
    }
}
/// SetVRDualWieldWeapons(left, right): the two guns' entities (null for none).
#[no_mangle]
pub unsafe extern "C" fn SetVRDualWieldWeapons(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut left, mut right) = (red4ext_abi::handle::Handle::null(), red4ext_abi::handle::Handle::null());
    params!(frame, left, right);
    publish_dual_weapons(left.instance as usize, right.instance as usize);
}
/// SetVRLeftMuzzle(x, y, z, i, j, k, r): the left gun's muzzle while two guns are in hand.
#[no_mangle]
pub unsafe extern "C" fn SetVRLeftMuzzle(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut x, mut y, mut z) = (0.0f32, 0.0f32, 0.0f32);
    let (mut i, mut j, mut k, mut r) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    params!(frame, x, y, z, i, j, k, r);
    if !have_shared() {
        return;
    }
    let valid = x * x + y * y + z * z >= 1.0 && (i * i + j * j + k * k + r * r) > 0.5;
    if valid {
        set_shared(SLOT_LEFT_MUZZLE_POS, x);
        set_shared(SLOT_LEFT_MUZZLE_POS + 1, y);
        set_shared(SLOT_LEFT_MUZZLE_POS + 2, z);
        set_shared(SLOT_LEFT_MUZZLE_FWD, 2.0 * (i * j - k * r));
        set_shared(SLOT_LEFT_MUZZLE_FWD + 1, 1.0 - 2.0 * (i * i + k * k));
        set_shared(SLOT_LEFT_MUZZLE_FWD + 2, 2.0 * (j * k + i * r));
    }
    set_shared(SLOT_LEFT_MUZZLE_VALID, if valid { 1.0 } else { 0.0 });
}
/// SetVRDualWieldRecords(left, right): the two guns' item records (0 = no gun).
#[no_mangle]
pub unsafe extern "C" fn SetVRDualWieldRecords(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut left, mut right) = (0i32, 0i32);
    params!(frame, left, right);
    CyberpunkVR_DualWieldRecords = [left as u32, right as u32];
}
/// The Optics mechanics' calls (the plugin's own port of mods/cet/CyberpunkVRPort_Optics): the natives' work.
///
/// # Safety
/// The session flag's read.
pub unsafe fn optics_enabled() -> bool {
    super::xr_calls::session_running()
}
/// VRPhysicalOpticsCameraReady's answer.
///
/// # Safety
/// As above.
pub unsafe fn optics_camera_ready() -> bool {
    super::xr_calls::session_running() && cvr_cpp_physical_optics_stereo_ready()
}
/// SetVRPhysicalOpticsPath's work.
pub fn set_optics_path(path: i32) {
    // SAFETY: the C++ half's optics path setter.
    unsafe { cvr_cpp_set_physical_optics_path(if (0..=4).contains(&path) { path } else { 0 }) };
}
/// SetVRSightZoom's work.
pub fn set_sight_zoom(active: bool) {
    // SAFETY: the plugin's own exported flag, written on the main thread.
    unsafe { CyberpunkVR_SightZoom = active as i32 };
}
/// SetVRSightDot's work.
pub fn set_sight_dot(active: bool) {
    // SAFETY: as above.
    unsafe { CyberpunkVR_SightDot = active as i32 };
}
/// SetVRSightApproach's work.
pub fn set_sight_approach(x: f32, y: f32, z: f32) {
    let sane = x.is_finite() && y.is_finite() && z.is_finite() && x * x + y * y + z * z < 0.15 * 0.15;
    // SAFETY: as above.
    unsafe { CyberpunkVR_SightApproach = if sane { [x, y, z] } else { [0.0; 3] } };
}
/// SetVRSightLine's work.
pub fn set_sight_line(x: f32, y: f32, z: f32) {
    let l2 = x * x + y * y + z * z;
    // SAFETY: as above.
    unsafe {
        if !l2.is_finite() || l2 < 1e-8 || l2 > 0.3 * 0.3 {
            CyberpunkVR_SightLineTick = 0;
            return;
        }
        CyberpunkVR_SightLineOffset = [x, y, z];
        CyberpunkVR_SightLineTick = tick_ms();
    }
}
/// GetVRMainEyeOffset's answer.
///
/// # Safety
/// The camera's flags' read.
pub unsafe fn main_eye_offset() -> f32 {
    if CyberpunkVR_IpdInWorldPos != 0 {
        cvr_camera_desired_half_ipd() * if CyberpunkVR_MainIsRightEye != 0 { 1.0 } else { -1.0 }
    } else {
        0.0
    }
}
/// GetVRSharedSlot's answer.
///
/// # Safety
/// The shared block's read.
pub unsafe fn shared_slot(idx: i32) -> f32 {
    if have_shared() && (0..256).contains(&idx) {
        shared(idx as usize)
    } else {
        0.0
    }
}
/// VRPhysicalOpticsEnabled() -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRPhysicalOpticsEnabled(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    params!(frame);
    put(out, super::xr_calls::session_running());
}
/// VRPhysicalOpticsCameraReady() -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRPhysicalOpticsCameraReady(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    params!(frame);
    put(out, super::xr_calls::session_running() && cvr_cpp_physical_optics_stereo_ready());
}
/// VRLuaHostGc(want) -> Bool: the Lua host paces the collector this frame.
#[no_mangle]
pub unsafe extern "C" fn VRLuaHostGc(_this: *mut c_void, frame: *mut Frame, out: *mut bool, _unused: i64) {
    let mut want = false;
    params!(frame, want);
    let active = CvrLuaHostGcWanted(want);
    put(out, active);
}
/// SetVRPhysicalOpticsPath(path): 0 none, 1 collimator, 2 shader, 3 camera, 4 native.
#[no_mangle]
pub unsafe extern "C" fn SetVRPhysicalOpticsPath(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut path = 0i32;
    params!(frame, path);
    cvr_cpp_set_physical_optics_path(if (0..=4).contains(&path) { path } else { 0 });
}
/// SetVRSightZoom(active).
#[no_mangle]
pub unsafe extern "C" fn SetVRSightZoom(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut active = false;
    params!(frame, active);
    CyberpunkVR_SightZoom = active as i32;
}
/// SetVRSightDot(active).
#[no_mangle]
pub unsafe extern "C" fn SetVRSightDot(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut active = false;
    params!(frame, active);
    CyberpunkVR_SightDot = active as i32;
}
/// SetVRSightApproach(x, y, z): the eye brought to the sight (world metres, under 0.15 m).
#[no_mangle]
pub unsafe extern "C" fn SetVRSightApproach(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut x, mut y, mut z) = (0.0f32, 0.0f32, 0.0f32);
    params!(frame, x, y, z);
    let sane = x.is_finite() && y.is_finite() && z.is_finite() && x * x + y * y + z * z < 0.15 * 0.15;
    CyberpunkVR_SightApproach = if sane { [x, y, z] } else { [0.0; 3] };
}
/// SetVRSightLine(x, y, z): the bore-to-sight-line offset (world metres); (0,0,0) clears it.
#[no_mangle]
pub unsafe extern "C" fn SetVRSightLine(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let (mut x, mut y, mut z) = (0.0f32, 0.0f32, 0.0f32);
    params!(frame, x, y, z);
    let l2 = x * x + y * y + z * z;
    if !l2.is_finite() || l2 < 1e-8 || l2 > 0.3 * 0.3 {
        CyberpunkVR_SightLineTick = 0;
        return;
    }
    CyberpunkVR_SightLineOffset = [x, y, z];
    CyberpunkVR_SightLineTick = tick_ms();
}
/// GetVRMainEyeOffset() -> Float: MAIN relative to the head centre along its right axis (+/- half the IPD, or 0).
#[no_mangle]
pub unsafe extern "C" fn GetVRMainEyeOffset(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    params!(frame);
    let v = if CyberpunkVR_IpdInWorldPos != 0 {
        cvr_camera_desired_half_ipd() * if CyberpunkVR_MainIsRightEye != 0 { 1.0 } else { -1.0 }
    } else {
        0.0
    };
    put(out, v);
}
/// SetVRZoomLevel(zoom): diagnostics only, shared[28].
#[no_mangle]
pub unsafe extern "C" fn SetVRZoomLevel(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut z = 1.0f32;
    params!(frame, z);
    set_shared(28, if z > 0.01 && z < 64.0 { z } else { 1.0 });
}
/// SetVRMeleeFire(frames): the melee RT impulse, shared[29].
#[no_mangle]
pub unsafe extern "C" fn SetVRMeleeFire(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut v = 0i32;
    params!(frame, v);
    set_shared(29, v as f32);
}
/// SetVRTriggerMode(mode): the port's say over the right trigger, shared[161] (0 pass, 1 swallow, 2 press).
#[no_mangle]
pub unsafe extern "C" fn SetVRTriggerMode(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut v = 0i32;
    params!(frame, v);
    cvr_ensure_shared_memory();
    set_shared(161, v.clamp(0, 2) as f32);
}
/// SetVRCamBoneFreeze(mode): 0 stock .. 4.
#[no_mangle]
pub unsafe extern "C" fn SetVRCamBoneFreeze(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut v = 0i32;
    params!(frame, v);
    g_VRCamBoneFreeze = v.clamp(0, 4);
}
/// SetVRPairSlew(rate): the clean pair's XY slew rate (m/s, 0.1..10).
#[no_mangle]
pub unsafe extern "C" fn SetVRPairSlew(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut v = 1.0f32;
    params!(frame, v);
    g_VRPairSlewRate = if v < 0.1 {
        0.1
    } else if v > 10.0 {
        10.0
    } else {
        v
    };
}
/// SetVRPairLead(ticks): the clean pair's prediction lead (0..2).
#[no_mangle]
pub unsafe extern "C" fn SetVRPairLead(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut v = 0.0f32;
    params!(frame, v);
    g_VRPairLeadTicks = if v < 0.0 {
        0.0
    } else if v > 2.0 {
        2.0
    } else {
        v
    };
}
/// SetVRCamAck(seq): the Lua ack of the consumed publish, shared[152].
#[no_mangle]
pub unsafe extern "C" fn SetVRCamAck(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    let mut v = 0.0f32;
    params!(frame, v);
    cvr_ensure_shared_memory();
    set_shared(152, v);
}
/// GetVRMeleeTrigger() -> Int32: the held-trigger power flag (shared[30]).
#[no_mangle]
pub unsafe extern "C" fn GetVRMeleeTrigger(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _unused: i64) {
    params!(frame);
    put(out, (have_shared() && shared(30) > 0.5) as i32);
}
/// GetVRSharedSlot(idx) -> Float: a raw shared slot (0..255).
#[no_mangle]
pub unsafe extern "C" fn GetVRSharedSlot(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let mut idx = 0i32;
    params!(frame, idx);
    put(
        out,
        if have_shared() && (0..256).contains(&idx) {
            shared(idx as usize)
        } else {
            0.0
        },
    );
}
/// GetVRProvDump(idx) -> Float: the instrument's counters and samples.
#[no_mangle]
pub unsafe extern "C" fn GetVRProvDump(_this: *mut c_void, frame: *mut Frame, out: *mut f32, _unused: i64) {
    let mut idx = 0i32;
    params!(frame, idx);
    let v: f64 = match idx {
        103 => QUAT_MODE.load(Ordering::Relaxed) as f64,
        104 => FWD_AXIS.load(Ordering::Relaxed) as f64,
        100 => PROV_INSTALLED.load(Ordering::Relaxed) as f64,
        101 => OVERRIDES.load(Ordering::Relaxed) as f64,
        102 => {
            let c = OVERRIDE_CLS.load(Ordering::Relaxed);
            if c < 0 {
                -1.0
            } else {
                (c * 1000 + OVERRIDE_SLOT.load(Ordering::Relaxed)) as f64
            }
        }
        200..=203 => LAST_Q.get()[(idx - 200) as usize] as f64,
        210..=213 => ORIG_Q.get()[(idx - 210) as usize] as f64,
        214..=217 => CTRL_Q.get()[(idx - 214) as usize] as f64,
        218..=221 => HMD_Q.get()[(idx - 218) as usize] as f64,
        i if i >= 0 && (i as usize) < N_CLASSES * N_SLOTS => PROV_CALLS[i as usize / N_SLOTS][i as usize % N_SLOTS].load(Ordering::Relaxed) as f64,
        _ => 0.0,
    };
    put(out, v as f32);
}
/// ResetVRProvCounts(): the per-slot counters and samples cleared.
#[no_mangle]
pub unsafe extern "C" fn ResetVRProvCounts(_this: *mut c_void, frame: *mut Frame, _out: *mut c_void, _unused: i64) {
    params!(frame);
    for row in &PROV_CALLS {
        for c in row {
            c.store(0, Ordering::Relaxed);
        }
    }
    OVERRIDES.store(0, Ordering::Relaxed);
    for q in [&LAST_Q, &ORIG_Q, &CTRL_Q, &HMD_Q] {
        q.set([0.0; 4]);
    }
}
/// The muzzle's last world position (SetVRMuzzlePos) and how many have been given (NightCoop's aim).
pub fn muzzle_world() -> ([f32; 3], u32) {
    (MUZZLE_POS.get(), MUZZLE_POS_SEQ.load(Ordering::Relaxed))
}
