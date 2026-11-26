//! Recoil that reaches the HANDS (formerly src/Anim/Recoil.cpp): a damped spring per hand, x'' = -w^2 x - 2 zeta w x',
//! given a velocity impulse per round and integrated in fixed 2 ms substeps with the real elapsed time; the weapon's
//! own peak angle from its kick through a knee, split by class between rise and throw, damped by a second hand's
//! leverage. The controls and read-outs are src/Anim/CharacterRigState.cpp's (include/Anim/Recoil.hpp).
use super::reload_handoff::{clock_qpc, clock_qpc_frequency};
use super::weapon_arm::cvr_anim_weapon_in_left_hand;
use crate::vrik::gestures::now_ms;
use core::sync::atomic::{AtomicU64, Ordering};
use cvr_body::cmath::{exp, pow, sqrt, tanh};

extern "C" {
    static mut CyberpunkVR_HandRecoil: i32;
    static mut CyberpunkVR_HandRecoilBackCm: f32;
    static mut CyberpunkVR_HandRecoilBackMaxCm: f32;
    static mut CyberpunkVR_HandRecoilRiseDeg: f32;
    static mut CyberpunkVR_HandRecoilReturnMs: f32;
    static mut CyberpunkVR_WeaponKickDeg: f32;
    static mut CyberpunkVR_WeaponKickScale: f32;
    static mut CyberpunkVR_WeaponClass: i32;
    static mut CyberpunkVR_WeaponKickDegLeft: f32;
    static mut CyberpunkVR_WeaponClassLeft: i32;
    static mut CyberpunkVR_DualWieldUntil: u64;
    static mut CyberpunkVR_RecoilDownPistol: f32;
    static mut CyberpunkVR_RecoilDownRifle: f32;
    static mut CyberpunkVR_RecoilDownShotgun: f32;
    static mut CyberpunkVR_PistolBackMul: f32;
    static mut CyberpunkVR_RifleRiseMul: f32;
    static mut CyberpunkVR_RifleBackMul: f32;
    static mut CyberpunkVR_ShotgunRiseMul: f32;
    static mut CyberpunkVR_ShotgunBackMul: f32;
    static mut CyberpunkVR_SniperRiseMul: f32;
    static mut CyberpunkVR_SniperBackMul: f32;
    static mut CyberpunkVR_RecoilScale: f32;
    static mut CyberpunkVR_TwoHandActive: i32;
    static mut CyberpunkVR_TwoHandRecoil: f32;
    static mut CyberpunkVR_TwoHandLever: f32;
    static mut CyberpunkVR_TwoHandWeight: f32;
    static mut CyberpunkVR_TwoHandBackMul: f32;
    static mut CyberpunkVR_HandRecoilRefKick: f32;
    static mut CyberpunkVR_HandRecoilMaxDeg: f32;
    static mut CyberpunkVR_HandRecoilReturnPow: f32;
    static mut CyberpunkVR_HandRecoilReturnMinMs: f32;
    static mut CyberpunkVR_RecoilClimbFrac: f32;
    static mut CyberpunkVR_RecoilClimbMaxDeg: f32;
    static mut CyberpunkVR_RecoilClimbMs: f32;
    static mut CyberpunkVR_DebugRecoilClimbDeg: f32;
    static mut CyberpunkVR_HandRecoilReturnMaxMs: f32;
    static mut CyberpunkVR_DebugRecoilShots: u64;
    static mut CyberpunkVR_DebugRecoilCm: f32;
    static mut CyberpunkVR_DebugRecoilTicks: u64;
    static mut CyberpunkVR_DebugRecoilAppliedDeg: f32;
    static mut CyberpunkVR_DebugRecoilPeakDeg: f32;
    static mut CyberpunkVR_DebugRecoilPeakDegLeft: f32;
    static mut CyberpunkVR_RuntimeDiagnostics: i32;
}

type Spring = cvr_body::recoil_spring::HandRecoilSpring;
const SPRING0: Spring = Spring { x: 0.0, v: 0.0 };
/// The installed parts' factor on each gun's pose amplitudes ([1] the weapon in hand, [0] the left gun), set by the
/// weapon mechanic's attachment recoil (SetVRAttachmentRecoil), as f32 bits.
static ATTACHMENT_SCALE: [core::sync::atomic::AtomicU32; 2] =
    [core::sync::atomic::AtomicU32::new(0x3F80_0000), core::sync::atomic::AtomicU32::new(0x3F80_0000)];
/// The last consumed shot's largest sampled rise per gun: the sequence in the high word, the degrees' bits in the low.
/// One pose writer, a read-only snapshot for the script side (GetVRLastShotPeak).
static SHOT_PEAK: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// RecoilSetAttachmentScale: a factor in [0, 1] (anything else is 1).
pub fn set_attachment_scale(side: i32, scale: f32) {
    if side == 0 || side == 1 {
        let v = if (0.0..=1.0).contains(&scale) { scale } else { 1.0 };
        ATTACHMENT_SCALE[side as usize].store(v.to_bits(), Ordering::Relaxed);
    }
}
/// RecoilLastShotPeak: the gun's last shot sequence (low 32 bits) and its largest sampled rise in degrees.
pub fn last_shot_peak(side: i32) -> (u32, f32) {
    let peak = if side == 0 || side == 1 { SHOT_PEAK[side as usize].load(Ordering::Relaxed) } else { 0 };
    ((peak >> 32) as u32, f32::from_bits(peak as u32))
}

static mut LAST_SHOT_MS: f64 = 0.0;
/// One counter per gun crossing from the shot detour's thread: [1] the weapon in hand, [0] the left gun.
static SHOT_SEQ: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static mut SPRING: [Spring; 2] = [SPRING0; 2];
static mut SEEN_SEQ: [u64; 2] = [0; 2];
static mut PENDING_SHOTS: [u64; 2] = [0; 2];
static mut LAST_MS: f64 = 0.0;
static mut CLIMB_DEG: f32 = 0.0;
/// The refractory window's last round per gun.
static mut ROUND_MS: [f64; 2] = [0.0; 2];

unsafe fn dual_live() -> bool {
    now_ms() < core::ptr::read_volatile(&raw const CyberpunkVR_DualWieldUntil)
}

unsafe fn kick_of(side: usize) -> f32 {
    if side == 0 && dual_live() && CyberpunkVR_WeaponKickDegLeft > 0.0 {
        return CyberpunkVR_WeaponKickDegLeft;
    }
    CyberpunkVR_WeaponKickDeg
}
unsafe fn class_of(side: usize) -> i32 {
    if side == 0 && dual_live() && CyberpunkVR_WeaponClassLeft > 0 {
        return CyberpunkVR_WeaponClassLeft;
    }
    CyberpunkVR_WeaponClass
}

/// The performance counter in milliseconds.
unsafe fn now_ms_qpc() -> f64 {
    let f = clock_qpc_frequency();
    if f == 0.0 {
        return 0.0;
    }
    clock_qpc() as f64 * 1000.0 / f
}

/// THE WEAPON'S OWN PEAK ANGLE: the reference scaled by this weapon's kick, compressed by the knee.
unsafe fn weapon_rise_deg(side: usize) -> f32 {
    let mut rise = CyberpunkVR_HandRecoilRiseDeg;
    let kick = kick_of(side);
    if kick > 0.0 && CyberpunkVR_HandRecoilRefKick > 0.01 {
        rise *= (kick * CyberpunkVR_WeaponKickScale) / CyberpunkVR_HandRecoilRefKick;
        let knee = CyberpunkVR_HandRecoilRiseDeg;
        let cap = CyberpunkVR_HandRecoilMaxDeg;
        if rise > knee && cap > knee {
            rise = knee + (cap - knee) * tanh((rise - knee) / (cap - knee));
        }
        if rise > cap {
            rise = cap;
        }
    }
    rise
}

/// What the class and the second hand's leverage leave of the angle.
unsafe fn grip_rise_factor(side: usize) -> f32 {
    let mut f = 1.0f32;
    let cls = class_of(side);
    if cls == 2 {
        f = CyberpunkVR_RifleRiseMul;
    } else if cls == 3 {
        f = CyberpunkVR_ShotgunRiseMul;
    } else if cls == 4 {
        f = CyberpunkVR_SniperRiseMul;
    }
    if CyberpunkVR_TwoHandActive != 0 {
        let mut lev = CyberpunkVR_TwoHandLever;
        if lev < 0.0 {
            lev = 0.0;
        }
        if lev > 1.0 {
            lev = 1.0;
        }
        let mut keep = CyberpunkVR_TwoHandRecoil;
        if keep < 0.0 {
            keep = 0.0;
        }
        if keep > 1.0 {
            keep = 1.0;
        }
        f *= 1.0 - (1.0 - keep) * lev;
    }
    f
}

/// One round out of one gun: ONE ROUND, ONE IMPULSE (a 40 ms refractory window per gun), and its haptic envelope.
unsafe fn on_shot_side(side: usize) {
    let now = now_ms_qpc();
    if ROUND_MS[side] > 0.0 && (now - ROUND_MS[side]) < 40.0 {
        return;
    }
    ROUND_MS[side] = now;
    LAST_SHOT_MS = now;
    SHOT_SEQ[side].fetch_add(1, Ordering::Release);
    // Left-handed the weapon in hand is the LEFT controller's.
    let hand = if cvr_anim_weapon_in_left_hand() { 1 - side as i32 } else { side as i32 };
    let dual = dual_live();
    super::haptics::cvr_haptics_shot(class_of(side), !dual && side == 1 && CyberpunkVR_TwoHandActive != 0, hand, kick_of(side), dual);
}

#[no_mangle]
pub unsafe extern "C" fn RecoilOnShot() {
    on_shot_side(1);
}

#[no_mangle]
pub unsafe extern "C" fn RecoilOnShotLeft() {
    on_shot_side(0);
}

/// Advance both springs to now: once per fresh solve, before either arm is built.
#[no_mangle]
pub unsafe extern "C" fn RecoilTick() {
    if CyberpunkVR_RuntimeDiagnostics != 0 {
        CyberpunkVR_DebugRecoilTicks = CyberpunkVR_DebugRecoilTicks.wrapping_add(1);
    }
    let now = now_ms_qpc();
    // Off, or a pause past a quarter second: the springs rest and the rounds fired meanwhile are not owed.
    if CyberpunkVR_HandRecoil == 0 || (LAST_MS > 0.0 && now - LAST_MS > 250.0) {
        for side in 0..2usize {
            SPRING[side] = SPRING0;
            SEEN_SEQ[side] = SHOT_SEQ[side].load(Ordering::Acquire);
            PENDING_SHOTS[side] = 0;
        }
        LAST_MS = now;
        CLIMB_DEG = 0.0;
        CyberpunkVR_DebugRecoilClimbDeg = 0.0;
        CyberpunkVR_DebugRecoilCm = 0.0;
        return;
    }
    let mut dt = if LAST_MS > 0.0 { ((now - LAST_MS) * 0.001) as f32 } else { 0.0 };
    LAST_MS = now;
    if dt < 0.0 {
        dt = 0.0;
    }

    for g in 0..2usize {
        let seq = SHOT_SEQ[g].load(Ordering::Acquire);
        if seq == SEEN_SEQ[g] {
            continue;
        }
        let shots = seq.wrapping_sub(SEEN_SEQ[g]);
        SEEN_SEQ[g] = seq;
        if CyberpunkVR_RuntimeDiagnostics != 0 {
            CyberpunkVR_DebugRecoilShots = CyberpunkVR_DebugRecoilShots.wrapping_add(shots);
        }
        // The burst walks the muzzle up: only the weapon in hand climbs.
        if g == 1 {
            CLIMB_DEG += weapon_rise_deg(1) * grip_rise_factor(1) * CyberpunkVR_RecoilClimbFrac * shots as f32;
            if CLIMB_DEG > CyberpunkVR_RecoilClimbMaxDeg {
                CLIMB_DEG = CyberpunkVR_RecoilClimbMaxDeg;
            }
        }
        PENDING_SHOTS[g] = PENDING_SHOTS[g].wrapping_add(shots);
    }

    // Per-weapon settle: the reference return time scaled by the weapon's own kick.
    let mut w = [0.0f32; 2];
    for (s, ws) in w.iter_mut().enumerate() {
        let mut ret = CyberpunkVR_HandRecoilReturnMs;
        let kick = kick_of(s);
        if kick > 0.0 && CyberpunkVR_HandRecoilRefKick > 0.01 {
            let r = (kick * CyberpunkVR_WeaponKickScale) / CyberpunkVR_HandRecoilRefKick;
            ret *= pow(r, CyberpunkVR_HandRecoilReturnPow);
            if ret < CyberpunkVR_HandRecoilReturnMinMs {
                ret = CyberpunkVR_HandRecoilReturnMinMs;
            }
            if ret > CyberpunkVR_HandRecoilReturnMaxMs {
                ret = CyberpunkVR_HandRecoilReturnMaxMs;
            }
        }
        if ret < 20.0 {
            ret = 20.0;
        }
        *ws = 6.283_185_31 / (ret * 0.001);
    }
    // The impulse: one round reaching exactly the configured amplitude is v0 = 1.914 w (peak 0.523 v0 / w).
    let dual = dual_live();
    if PENDING_SHOTS[1] != 0 {
        SPRING[1].v += 1.914 * w[1] * PENDING_SHOTS[1] as f32;
        if !dual {
            SPRING[0].v += 1.914 * w[0] * PENDING_SHOTS[1] as f32;
        }
        PENDING_SHOTS[1] = 0;
    }
    if PENDING_SHOTS[0] != 0 {
        SPRING[0].v += 1.914 * w[0] * PENDING_SHOTS[0] as f32;
        PENDING_SHOTS[0] = 0;
    }
    // The spring in closed form over the actual interval (cvr_body::recoil_spring).
    for (s, ws) in w.iter().enumerate() {
        SPRING[s].advance(*ws, dt);
    }
    // THE WALK BACK DOWN, first order.
    {
        let tau = if CyberpunkVR_RecoilClimbMs > 1.0 { CyberpunkVR_RecoilClimbMs * 0.001 } else { 0.001 };
        CLIMB_DEG *= exp(-dt / tau);
        if CLIMB_DEG < 0.0005 {
            CLIMB_DEG = 0.0;
        }
    }
    CyberpunkVR_DebugRecoilClimbDeg = CLIMB_DEG;
    CyberpunkVR_DebugRecoilCm = SPRING[0].x * CyberpunkVR_HandRecoilBackCm;
}

/// How the throw is split between backwards and down, unit length.
#[no_mangle]
pub unsafe extern "C" fn RecoilTravelMix(outBack: *mut f32, outDown: *mut f32) {
    let mut d = CyberpunkVR_RecoilDownPistol;
    // a sniper is shouldered like a rifle
    if CyberpunkVR_WeaponClass == 2 || CyberpunkVR_WeaponClass == 4 {
        d = CyberpunkVR_RecoilDownRifle;
    } else if CyberpunkVR_WeaponClass == 3 {
        d = CyberpunkVR_RecoilDownShotgun;
    }
    if d < 0.0 {
        d = 0.0;
    }
    if d > 1.0 {
        d = 1.0;
    }
    let back = 1.0 - d;
    let len = sqrt(back * back + d * d);
    let k = if len > 1e-6 { 1.0 / len } else { 0.0 };
    if !outBack.is_null() {
        *outBack = back * k;
    }
    if !outDown.is_null() {
        *outDown = d * k;
    }
}

/// One hand's displacement in its own frame: metres of throw and radians of rise. Only a hand on the weapon.
#[no_mangle]
pub unsafe extern "C" fn RecoilSample(side: i32, weaponHand: i32, outBackM: *mut f32, outRiseRad: *mut f32) {
    *outBackM = 0.0;
    *outRiseRad = 0.0;
    if CyberpunkVR_HandRecoil == 0 || !(0..=1).contains(&side) {
        return;
    }
    let mut weapon_hand = weaponHand;
    // A GUN IN EACH HAND makes the left one a weapon hand too.
    let partner_gun = side == 0 && dual_live();
    if partner_gun {
        weapon_hand = 1;
    }
    if weapon_hand == 0 && CyberpunkVR_TwoHandActive == 0 {
        return;
    }
    let su = side as usize;
    let mut rise = weapon_rise_deg(su);
    let cls = class_of(su);
    let rise_w = rise;

    let mut back_mul = 1.0f32;
    if cls == 1 {
        back_mul = CyberpunkVR_PistolBackMul;
    } else if cls == 2 {
        back_mul = CyberpunkVR_RifleBackMul;
    } else if cls == 3 {
        back_mul = CyberpunkVR_ShotgunBackMul;
    } else if cls == 4 {
        back_mul = CyberpunkVR_SniperBackMul;
    }
    let x = SPRING[su].x;
    // THE TRAVEL RIDES THE WEAPON'S SHAPING, NOT THE GRIP'S.
    let mut back_cm = CyberpunkVR_HandRecoilBackCm * back_mul;
    if CyberpunkVR_HandRecoilRiseDeg > 0.01 {
        back_cm *= rise_w / CyberpunkVR_HandRecoilRiseDeg;
    }
    // ...and a second hand on the weapon shortens the throw, ceiling included.
    {
        let mut w = CyberpunkVR_TwoHandWeight;
        if w < 0.0 {
            w = 0.0;
        }
        if w > 1.0 {
            w = 1.0;
        }
        let mut m = CyberpunkVR_TwoHandBackMul;
        if m < 0.0 {
            m = 0.0;
        }
        if m > 1.0 {
            m = 1.0;
        }
        back_mul *= 1.0 + (m - 1.0) * w;
        back_cm *= 1.0 + (m - 1.0) * w;
    }
    let back_max = CyberpunkVR_HandRecoilBackMaxCm * back_mul;
    if back_cm > back_max {
        back_cm = back_max;
    }
    // One installed-part factor on the final pose amplitudes, after the caps; both hands on a single weapon share its
    // primary scale.
    let scale = CyberpunkVR_RecoilScale * f32::from_bits(ATTACHMENT_SCALE[if partner_gun { 0 } else { 1 }].load(Ordering::Relaxed));
    *outBackM = x * back_cm * 0.01 * scale;

    // ...AND THE ANGLE, where the class and the second hand belong.
    rise = rise_w * grip_rise_factor(su);
    let mut deg = x * rise + if weapon_hand != 0 && side == 1 { CLIMB_DEG } else { 0.0 };
    // THE CEILING HOLDS FOR STACKED SHOTS TOO.
    let cap_deg = CyberpunkVR_HandRecoilMaxDeg;
    if deg > cap_deg {
        deg = cap_deg;
    }
    if deg < -cap_deg {
        deg = -cap_deg;
    }
    deg *= scale;
    *outRiseRad = deg * 0.017_453_292_52;
    if weapon_hand != 0 && SEEN_SEQ[su] != 0 {
        let seq = SEEN_SEQ[su] as u32;
        let (previous_seq, previous) = last_shot_peak(side);
        let peak = deg.abs();
        if seq != previous_seq || peak > previous {
            SHOT_PEAK[su].store((u64::from(seq) << 32) | u64::from(peak.to_bits()), Ordering::Relaxed);
        }
    }
    if side == 1 {
        let deg = *outRiseRad * 57.295_779_5;
        CyberpunkVR_DebugRecoilAppliedDeg = deg;
        let a = if deg < 0.0 { -deg } else { deg };
        if a > CyberpunkVR_DebugRecoilPeakDeg {
            CyberpunkVR_DebugRecoilPeakDeg = a;
        }
    } else {
        let a = (*outRiseRad).abs() * 57.295_779_5;
        if a > CyberpunkVR_DebugRecoilPeakDegLeft {
            CyberpunkVR_DebugRecoilPeakDegLeft = a;
        }
    }
}

/// When the last round left, on the recoil's own clock.
#[no_mangle]
pub unsafe extern "C" fn RecoilLastShotMs() -> f64 {
    LAST_SHOT_MS
}
