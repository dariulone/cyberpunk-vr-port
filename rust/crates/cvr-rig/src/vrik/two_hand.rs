//! The support hand on the weapon, and the aim that follows from it (formerly src/Anim/TwoHandGrip.cpp). The hold is
//! one frame of the game's own two-handed animation per weapon (CyberpunkVR_TwoHandGrip_<weapon>[@where].ini): the
//! left wrist in the right wrist's frame and its fingers. Reach for it and the fingers preview the hold; squeeze and
//! the hand welds to the weapon on a spring, the barrel follows the left hand by how it moves AGAINST the right one,
//! and the recoil reads the hold's leverage. Main's 2026-11 shape (612bd89fc, 10eaa388c, cc98dbb89, ad4044f7b,
//! 87693549b): the support is measured against the controllers, then placed on the weapon hand's final pose -- after the
//! visual recoil (kept out of the tracked grip input) and again on the wrist the arm IK actually wrote, on either hand;
//! a short gap in the grip's samples does not drop a closed hold, and the hold is let go when the weapon, the support
//! hand or the dual wield changes. The carry into the left hand and the return to the right are springs on
//! the wrists that need no game-space geometry. The controls and read-outs are src/Anim/CharacterRigState.cpp's.
use super::env::*;
use super::reload::grip_path;
use super::reload_handoff::{clock_qpc, clock_qpc_frequency};
use super::rig::{VRIK_ComputeFK, VRIK_FKCount};
use super::recoil::{RecoilSample, RecoilTravelMix};
use super::weapon_arm::{cvr_anim_mirror_finger_to_right, cvr_anim_mirror_held_hand, cvr_anim_two_guns_in_hand,
                        cvr_anim_weapon_in_left_hand};
use super::*;
use crate::math::{quat_conj, quat_mul, quat_norm, quat_rotate_vec};
use crate::printf::format_g9;
use crate::vrik::gestures::now_ms;
use core::ffi::c_char;
use cvr_body::cmath::{acos, atan2, cos, exp, sin, sqrt};
use cvr_body::policy::should_hand_left_arm_to_body_carry;

extern "C" {
    static mut g_hasWeaponEquipped: bool;
    static mut g_VRSmokeFingerCountL: i32;
    static mut g_VRSmokeFingerIdxL: [i32; 32];
    static mut g_VRSmokeFingerNameL: [[c_char; 48]; 32];
    static mut g_VRReloadFingerActive: [i32; 2];
    static mut g_VRBodyCarryingState: i32;
    static mut CyberpunkVR_WeaponName: [c_char; 64];
    static mut CyberpunkVR_WeaponClass: i32;
    static mut CyberpunkVR_VrikBodyCarryHandOff: i32;
    static mut CyberpunkVR_TwoHandGrip: i32;
    static mut CyberpunkVR_TwoHandSuppress: i32;
    static mut CyberpunkVR_DebugTwoHandHoldIdx: i32;
    static mut CyberpunkVR_CarryLeft: i32;
    static mut CyberpunkVR_CarryBlendMs: f32;
    static mut CyberpunkVR_CarryBlendZeta: f32;
    static mut CyberpunkVR_DebugCarryBlend: f32;
    static mut CyberpunkVR_TwoHandRadius: f32;
    static mut CyberpunkVR_TwoHandFadeS: f32;
    static mut CyberpunkVR_TwoHandEngageMs: f32;
    static mut CyberpunkVR_TwoHandEngageZeta: f32;
    static mut CyberpunkVR_TwoHandWeight: f32;
    static mut CyberpunkVR_TwoHandLever: f32;
    static mut CyberpunkVR_TwoHandReleaseMs: f32;
    static mut CyberpunkVR_CarryReturnHand: i32;
    static mut CyberpunkVR_CarryReturnMs: f32;
    static mut CyberpunkVR_CarryReturnZeta: f32;
    static mut CyberpunkVR_DebugCarryReturn: f32;
    static mut CyberpunkVR_CarryGripRadius: f32;
    static mut CyberpunkVR_CarryGripMs: f32;
    static mut CyberpunkVR_CarryGripZeta: f32;
    static mut CyberpunkVR_CarryGripBlend: f32;
    static mut CyberpunkVR_DebugCarryReach: f32;
    static mut CyberpunkVR_TwoHandAimGain: f32;
    static mut CyberpunkVR_TwoHandLeverFull: f32;
    static mut CyberpunkVR_TwoHandAimTau: f32;
    static mut CyberpunkVR_TwoHandAimMaxDeg: f32;
    static mut CyberpunkVR_TwoHandAimDeadDeg: f32;
    static mut CyberpunkVR_TwoHandAim: i32;
    static mut CyberpunkVR_TwoHandAimReleaseMs: f32;
    static mut CyberpunkVR_DebugTwoHandAimDeg: f32;
    static mut CyberpunkVR_TwoHandBreakRadius: f32;
    static mut CyberpunkVR_TwoHandBreakMs: f32;
    static mut CyberpunkVR_TwoHandActive: i32;
    static mut CyberpunkVR_TwoHandBlend: f32;
    static mut CyberpunkVR_DebugTwoHandDist: f32;
    static mut CyberpunkVR_DebugTwoHandHave: i32;
    static mut CyberpunkVR_TwoHandCaptureReq: i32;
    static mut CyberpunkVR_DebugTwoHandRefused: i32;
    static mut CyberpunkVR_DebugTwoHandSaved: i32;
    static mut CyberpunkVR_DebugTwoHandLoaded: i32;
    static mut CyberpunkVR_DebugTwoHandBaseMm: f32;
    // The pump's fore-end under the support hand (SetVRTwoHandSlide), and the left controller as the hold read it.
    static mut g_VRTwoHandSlide: [f32; 3];
    static mut g_VRTwoHandSlideUntil: u64;
    static mut g_VRTwoHandCtrlL: [f32; 3];
    static mut g_VRTwoHandCtrlLValid: i32;
}

/// Room for the primary, magazine and further fore-end / handle points, and the captured holds; a full table refuses
/// out loud (main: 256).
const MAX_HOLDS: usize = 256;
/// A long gun's fore-end, 35 mm either side of it, its magazine and the receiver ahead of the trigger; a katana's handle
/// every 25 mm down to the pommel: at four the last ones read were never offered (main: 12).
const MAX_CAND: usize = 12;
/// A dash or a world time scale transition can leave one pass with no fresh grip sample: a closed hold is kept through
/// a gap that short; a real release still wins once it lasts longer.
const GRIP_SAMPLE_GRACE_MS: f32 = 120.0;
/// The shared slot the reload module names the wrist it holds by (-1 none).
const SLOT_RELOAD_OWNED_HAND: usize = 162;

#[derive(Clone, Copy)]
struct Hold {
    weapon: [u8; 64],
    /// "ALIAS <weapon>": a re-shelled weapon borrows the original's hold by reference.
    alias: [u8; 64],
    off: V3,
    rot: Q,
    finger: [Q; 32],
    finger_name: [[u8; 48]; 32],
    finger_count: i32,
}
const HOLD0: Hold = Hold {
    weapon: [0; 64],
    alias: [0; 64],
    off: [0.0; 3],
    rot: [0.0; 4],
    finger: [[0.0; 4]; 32],
    finger_name: [[0; 48]; 32],
    finger_count: 0,
};

static mut HOLDS: [Hold; MAX_HOLDS] = [HOLD0; MAX_HOLDS];
static mut HOLD_COUNT: usize = 0;
static mut SCANNED: bool = false;
static mut SAVE_REQ: bool = false;
/// The hold in use (index into HOLDS), -1 none; the weapon's holds on offer; the plain one; the name selected for.
static mut CUR: i32 = -1;
static mut CAND: [i32; MAX_CAND] = [-1; MAX_CAND];
static mut CAND_N: usize = 0;
static mut PRIMARY: i32 = -1;
static mut CUR_FOR: [u8; 64] = {
    let mut a = [0u8; 64];
    a[0] = 1; // deliberately not a valid name, so the first pass always selects
    a
};

/// The support point, recomputed every pass the right hand is solved and read by the left one.
static mut SUP_POS: V3 = [0.0; 3];
static mut SUP_ROT: Q = ID;
static mut SUP_VALID: bool = false;
/// ...and the last one that was valid, which a release fades toward.
static mut SUP_PREV_POS: V3 = [0.0; 3];
static mut SUP_PREV_ROT: Q = ID;
static mut SUP_PREV_HAVE: bool = false;
static mut R_GRIP_WAS: bool = false;
static mut R_GRIP_FRESH: bool = false;
static mut GRIP_WAS: bool = false;
static mut GRIP_FRESH: bool = false;
static mut GRIP_MISSING_MS: f32 = 0.0;
/// The weapon, the support hand and the dual wield the hold was measured for (None before the first pass).
static mut PREV_OWNER: Option<(u64, usize, bool)> = None;
/// The weapon family the steer was measured on (the primary hold's index; -2 never).
static mut LAST_FAMILY: i32 = -2;
/// The hold's attachment on the weapon wrist, in that wrist's frame: the offset, the rotation, the fore-end's stroke.
#[derive(Clone, Copy)]
struct Attachment {
    offset: V3,
    rotation: Q,
    slide: V3,
}
static mut DRAWN_ATTACHMENT: Attachment = Attachment { offset: [0.0; 3], rotation: ID, slide: [0.0; 3] };

// The steer.
static mut AIM_X: f32 = 0.0;
static mut AIM_V: f32 = 0.0;
static mut STEER_AXIS: V3 = [0.0, 0.0, 1.0];
static mut STEER_ANG: f32 = 0.0;
static mut REF_E: V3 = [0.0; 3];
static mut S_DL: V3 = [0.0; 3];
static mut REF_HAVE: bool = false;
// TwoHandRight's own clock and springs.
static mut RIGHT_PREV_QPC: i64 = 0;
static mut WEIGHT_X: f32 = 0.0;
static mut WEIGHT_V: f32 = 0.0;
static mut FAR_MS: f32 = 0.0;
// The carry into the left hand.
static mut CARRY_ROT0: Q = ID;
static mut CARRY_HAVE: bool = false;
static mut CARRY_START_MS: f64 = 0.0;
static mut CARRY_WAS: bool = false;
// The return into the right hand.
static mut LEFT_DRAWN_POS: V3 = [0.0; 3];
static mut LEFT_DRAWN_ROT: Q = ID;
static mut LEFT_DRAWN_HAVE: bool = false;
static mut CARRY_X_POS: V3 = [0.0; 3];
static mut CARRY_X_ROT: Q = ID;
static mut CARRY_X_HAVE: bool = false;
static mut RET_POS: V3 = [0.0; 3];
static mut RET_ROT: Q = ID;
static mut RET_HAVE: bool = false;
static mut RET_START_MS: f64 = 0.0;
static mut RETURN_WAS: bool = false;
static mut RETURN_PREV_QPC: i64 = 0;
static mut GRIP_G: f32 = 0.0;
static mut GRIP_GV: f32 = 0.0;

// ---- C strings

fn cstr_len(s: &[u8]) -> usize {
    s.iter().position(|&c| c == 0).unwrap_or(s.len())
}
/// strncmp(a, b, n) == 0 over NUL-terminated buffers.
fn strn_eq(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let ca = a.get(i).copied().unwrap_or(0);
        let cb = b.get(i).copied().unwrap_or(0);
        if ca != cb {
            return false;
        }
        if ca == 0 {
            return true;
        }
    }
    true
}
/// strcmp(a, b), unsigned bytes.
fn strcmp(a: &[u8], b: &[u8]) -> i32 {
    let mut i = 0;
    loop {
        let ca = a.get(i).copied().unwrap_or(0);
        let cb = b.get(i).copied().unwrap_or(0);
        if ca != cb {
            return ca as i32 - cb as i32;
        }
        if ca == 0 {
            return 0;
        }
        i += 1;
    }
}
/// strncpy(dst, src, n): up to n bytes, the rest of the n zero-filled; dst[n..] untouched.
fn strncpy(dst: &mut [u8], src: &[u8], n: usize) {
    let l = cstr_len(src).min(n);
    dst[..l].copy_from_slice(&src[..l]);
    for d in dst[l..n].iter_mut() {
        *d = 0;
    }
}
fn as_str(s: &[u8]) -> &str {
    core::str::from_utf8(&s[..cstr_len(s)]).unwrap_or("?")
}
unsafe fn bytes_of(c: &[c_char]) -> &[u8] {
    core::slice::from_raw_parts(c.as_ptr() as *const u8, c.len())
}

unsafe fn weapon_key() -> &'static [u8] {
    let name = bytes_of(&CyberpunkVR_WeaponName);
    if name[0] != 0 {
        name
    } else {
        b"default\0"
    }
}

unsafe fn find_hold(weapon: &[u8]) -> i32 {
    for i in 0..HOLD_COUNT {
        if strn_eq(&HOLDS[i].weapon, weapon, 63) {
            return i as i32;
        }
    }
    -1
}

unsafe fn add_hold(weapon: &[u8]) -> i32 {
    let h = find_hold(weapon);
    if h >= 0 {
        return h;
    }
    if HOLD_COUNT >= MAX_HOLDS {
        return -1;
    }
    let i = HOLD_COUNT;
    HOLD_COUNT += 1;
    HOLDS[i] = HOLD0;
    strncpy(&mut HOLDS[i].weapon, weapon, 63);
    HOLDS[i].rot[3] = 1.0;
    i as i32
}

/// SELECTION IS A NAME COMPARISON, right on the first pass after a draw.
unsafe fn select_hold() {
    let w = weapon_key();
    if strn_eq(&CUR_FOR, w, 63) {
        return;
    }
    strncpy(&mut CUR_FOR, w, 63);
    CUR_FOR[63] = 0;
    CUR = find_hold(w);
    // Follow a borrowed hold, once.
    if CUR >= 0 {
        let c = &HOLDS[CUR as usize];
        if c.alias[0] != 0 && c.finger_count == 0 {
            let alias = c.alias;
            let src = find_hold(&alias);
            if src >= 0 {
                CUR = src;
            }
        }
    }
    PRIMARY = CUR;
    CAND_N = 0;
    if CUR >= 0 {
        CAND[CAND_N] = CUR;
        CAND_N += 1;
    }
    {
        // "<weapon>@..." rows, in name order.
        let wl = cstr_len(w);
        let mut alt = [0usize; MAX_CAND];
        let mut alt_n = 0usize;
        let mut i = 0usize;
        while i < HOLD_COUNT && alt_n < MAX_CAND {
            let nm = &HOLDS[i].weapon;
            if strn_eq(nm, w, wl) && nm[wl] == b'@' && HOLDS[i].finger_count > 0 {
                alt[alt_n] = i;
                alt_n += 1;
            }
            i += 1;
        }
        for a in 0..alt_n {
            for b in a + 1..alt_n {
                if strcmp(&HOLDS[alt[b]].weapon, &HOLDS[alt[a]].weapon) < 0 {
                    alt.swap(a, b);
                }
            }
        }
        let mut a = 0usize;
        while a < alt_n && CAND_N < MAX_CAND {
            CAND[CAND_N] = alt[a] as i32;
            CAND_N += 1;
            a += 1;
        }
    }
    if CUR < 0 && CAND_N > 0 {
        CUR = CAND[0];
    }
    CyberpunkVR_DebugTwoHandHoldIdx = -1;
    CyberpunkVR_DebugTwoHandHave = if CUR >= 0 { 1 } else { 0 };
    CyberpunkVR_TwoHandActive = 0;
}

// ---- the file: fgets over a text-mode read, sscanf's few formats

/// The lines fgets(line, 256) gives over the file as text mode reads it (CRLF as LF, Ctrl-Z the end).
pub(super) fn text_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    let end = bytes.iter().position(|&b| b == 0x1A).unwrap_or(bytes.len());
    let mut text = Vec::with_capacity(end);
    let mut i = 0;
    while i < end {
        if bytes[i] == b'\r' && i + 1 < end && bytes[i + 1] == b'\n' {
            i += 1;
            continue;
        }
        text.push(bytes[i]);
        i += 1;
    }
    let mut lines = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut stop = start;
        while stop < text.len() && stop - start < 255 {
            stop += 1;
            if text[stop - 1] == b'\n' {
                break;
            }
        }
        lines.push(text[start..stop].to_vec());
        start = stop;
    }
    lines
}
fn is_space(c: u8) -> bool {
    c == b' ' || (b'\t'..=b'\r').contains(&c)
}
/// The whitespace-separated tokens after a literal prefix (sscanf's " %s"/" %g" each skip leading whitespace).
fn tokens(rest: &[u8]) -> Vec<&[u8]> {
    rest.split(|&c| is_space(c)).filter(|t| !t.is_empty()).collect()
}
fn parse_float(t: &[u8]) -> Option<f32> {
    core::str::from_utf8(t).ok()?.parse::<f32>().ok()
}

unsafe fn parse_into(h: usize, bytes: &[u8]) {
    for line in text_lines(bytes) {
        if line.is_empty() || line[0] == b'#' || line[0] == b'\n' || line[0] == b'\r' {
            continue;
        }
        if let Some(rest) = line.strip_prefix(b"ALIAS") {
            if let Some(tok) = tokens(rest).first() {
                let mut al = [0u8; 64];
                let n = tok.len().min(63);
                al[..n].copy_from_slice(&tok[..n]);
                if al[0] != 0 {
                    strncpy(&mut HOLDS[h].alias, &al, 63);
                    continue;
                }
            }
        }
        if let Some(rest) = line.strip_prefix(b"W") {
            let t = tokens(rest);
            let v: Vec<f32> = t.iter().take(7).map_while(|x| parse_float(x)).collect();
            if v.len() == 7 {
                HOLDS[h].off = [v[0], v[1], v[2]];
                HOLDS[h].rot = [v[3], v[4], v[5], v[6]];
                quat_norm(&mut HOLDS[h].rot);
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix(b"F") {
            let t = tokens(rest);
            if t.len() >= 5 && HOLDS[h].finger_count < 32 {
                let v: Vec<f32> = t[1..5].iter().map_while(|x| parse_float(x)).collect();
                if v.len() == 4 {
                    let k = HOLDS[h].finger_count as usize;
                    let mut nm = [0u8; 64];
                    let n = t[0].len().min(63);
                    nm[..n].copy_from_slice(&t[0][..n]);
                    strncpy(&mut HOLDS[h].finger_name[k], &nm, 47);
                    HOLDS[h].finger[k] = [v[0], v[1], v[2], v[3]];
                    HOLDS[h].finger_count += 1;
                }
            }
        }
    }
}

/// THE HOLD AS THE WEAPON HAND HAS IT: captured with the weapon in the right hand, mirrored with it in the left.
unsafe fn hold_pose(h: usize) -> (V3, Q) {
    let mut off = HOLDS[h].off;
    let mut rot = HOLDS[h].rot;
    if cvr_anim_weapon_in_left_hand() {
        cvr_anim_mirror_held_hand(off.as_mut_ptr(), rot.as_mut_ptr());
    }
    (off, rot)
}

/// The support hand in the reload module's numbering (0 left, 1 right).
unsafe fn support_hand() -> usize {
    if cvr_anim_weapon_in_left_hand() {
        1
    } else {
        0
    }
}
unsafe fn left_grip_pressed() -> f32 {
    if g_pSharedHands.is_null() {
        0.0
    } else {
        *g_pSharedHands.add(if support_hand() != 0 { 49 } else { 155 })
    }
}
/// The reload module has explicitly claimed the support wrist (shared[162]).
unsafe fn reload_owns_support_hand() -> bool {
    if g_pSharedHands.is_null() {
        return false;
    }
    let owner = *g_pSharedHands.add(SLOT_RELOAD_OWNED_HAND);
    owner > -0.5 && (owner + 0.5) as usize == support_hand()
}
/// ReloadPose's TwoHandOwnsReloadFinger (main's 87693549b): with the hold closed the reload's finger pose stays off a
/// hand the reload has not claimed.
pub(crate) unsafe fn two_hand_owns_reload_finger(hand: usize) -> bool {
    if CyberpunkVR_TwoHandActive == 0 || g_pSharedHands.is_null() {
        return false;
    }
    let owner = *g_pSharedHands.add(SLOT_RELOAD_OWNED_HAND);
    !(owner > -0.5 && (owner + 0.5) as usize == hand)
}
unsafe fn right_grip_pressed() -> f32 {
    if g_pSharedHands.is_null() {
        0.0
    } else {
        *g_pSharedHands.add(49)
    }
}

/// Pick the hold for the weapon in hand, from outside the pose pass.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_two_hand_select_hold_for_carry() {
    select_hold();
}

/// CAPTURE, inside the pose apply with VRIK OFF so the buffer holds the game's own two-handed animation.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_two_hand_capture(boneBuf: *mut u8) {
    if CyberpunkVR_TwoHandCaptureReq == 0 {
        return;
    }
    CyberpunkVR_TwoHandCaptureReq = 0;
    // 1 no weapon, 2 VRIK is writing the arms, 4 the rig's bones are not resolved yet.
    let mut why = 0;
    if !g_hasWeaponEquipped {
        why |= 1;
    }
    if g_VRBind != 0 {
        why |= 2;
    }
    if g_VRLeftBoneIdx < 0 || g_VRRightBoneIdx < 0 {
        why |= 4;
    }
    if why != 0 {
        CyberpunkVR_DebugTwoHandRefused = why;
        log(&format!(
            "TwoHandCapture REFUSED ({}):{}{}{}  weapon='{}' holds={}/{}\n",
            why,
            if why & 1 != 0 { " no-weapon" } else { "" },
            if why & 2 != 0 { " vrik-on(g_VRBind!=0)" } else { "" },
            if why & 4 != 0 { " bones-unresolved" } else { "" },
            as_str(weapon_key()),
            HOLD_COUNT,
            MAX_HOLDS
        ));
        return;
    }
    VRIK_ComputeFK(boneBuf, VRIK_FKCount());
    let (r, l) = (g_VRRightBoneIdx as usize, g_VRLeftBoneIdx as usize);
    let p_r = g_fkPos[r];
    let q_r = g_fkRot[r];
    let p_l = g_fkPos[l];
    let q_l = g_fkRot[l];
    // 16 = THE HANDS ARE NOT ON THE SAME WEAPON: a one-handed stance's left arm hangs at the hip.
    {
        let (dx, dy, dz) = (p_l[0] - p_r[0], p_l[1] - p_r[1], p_l[2] - p_r[2]);
        let base = sqrt(dx * dx + dy * dy + dz * dz);
        CyberpunkVR_DebugTwoHandBaseMm = base * 1000.0;
        if base > 0.60 {
            CyberpunkVR_DebugTwoHandRefused = 16;
            log(&format!(
                "TwoHandCapture REFUSED (16): wrists {:.0} mm apart -- that is a one-handed stance, not a hold. \
                 weapon='{}'\n",
                (base * 1000.0) as f64,
                as_str(weapon_key())
            ));
            return;
        }
    }
    let h = add_hold(weapon_key());
    if h < 0 {
        CyberpunkVR_DebugTwoHandRefused = 8;
        log(&format!(
            "TwoHandCapture REFUSED (8): the hold table is FULL -- {} of {} used, weapon='{}'\n",
            HOLD_COUNT,
            MAX_HOLDS,
            as_str(weapon_key())
        ));
        return;
    }
    let hu = h as usize;
    let q_rc = quat_conj(&q_r);
    let d = [p_l[0] - p_r[0], p_l[1] - p_r[1], p_l[2] - p_r[2]];
    HOLDS[hu].off = quat_rotate_vec(&q_rc, &d);
    HOLDS[hu].rot = quat_mul(&q_rc, &q_l);
    quat_norm(&mut HOLDS[hu].rot);
    HOLDS[hu].finger_count = 0;
    let mut k = 0;
    while k < g_VRSmokeFingerCountL && k < 32 {
        let bi = g_VRSmokeFingerIdxL[k as usize];
        if (0..MAXB).contains(&bi) {
            let fc = HOLDS[hu].finger_count as usize;
            HOLDS[hu].finger[fc] = a4(br(boneBuf, bi));
            let name = bytes_of(&g_VRSmokeFingerNameL[k as usize]);
            strncpy(&mut HOLDS[hu].finger_name[fc], name, 47);
            HOLDS[hu].finger_count += 1;
        }
        k += 1;
    }
    CUR = h;
    let weapon = HOLDS[hu].weapon;
    strncpy(&mut CUR_FOR, &weapon, 63);
    CyberpunkVR_DebugTwoHandHave = 1;
    CyberpunkVR_DebugTwoHandRefused = 0;
    SAVE_REQ = true;
    log(&format!(
        "TwoHandCapture OK: weapon='{}' base={:.0} mm fingers={} holds={}/{}\n",
        as_str(&weapon),
        CyberpunkVR_DebugTwoHandBaseMm as f64,
        HOLDS[hu].finger_count,
        HOLD_COUNT,
        MAX_HOLDS
    ));
}

/// WHAT THE LEFT-HAND CARRY NEEDS: the weapon carried back by the hold's offset, in the left wrist's frame,
/// conj(rot) * (-off). 0 without a captured hold.
#[no_mangle]
pub unsafe extern "C" fn VRTwoHandCarryOffset(idx: i32) -> f32 {
    select_hold();
    let h = if PRIMARY >= 0 { PRIMARY } else { CUR };
    if h < 0 || !(0..=2).contains(&idx) {
        return 0.0;
    }
    let hd = &HOLDS[h as usize];
    let neg = [-hd.off[0], -hd.off[1], -hd.off[2]];
    let rc = quat_conj(&hd.rot);
    quat_rotate_vec(&rc, &neg)[idx as usize]
}

unsafe fn steer_reset() {
    AIM_X = 0.0;
    AIM_V = 0.0;
    STEER_ANG = 0.0;
    REF_HAVE = false;
    CyberpunkVR_DebugTwoHandAimDeg = 0.0;
}

/// The seconds since `prev` on the hands' counter, 0.016 on the first call and outside (0, 0.25].
unsafe fn pass_dt(prev: &mut i64) -> f32 {
    let mut dt = 0.016f32;
    let f = clock_qpc_frequency();
    let now = clock_qpc();
    if *prev != 0 && f != 0.0 {
        dt = ((now - *prev) as f64 / f) as f32;
    }
    *prev = now;
    if dt < 0.0 || dt > 0.25 {
        dt = 0.016;
    }
    dt
}

/// x'' = -w^2 (x - goal) - 2 zeta w x' in fixed 2 ms substeps.
fn spring(x: &mut f32, v: &mut f32, goal: f32, wn: f32, zeta: f32, dt: f32) {
    let mut steps = (dt / 0.002) as i32 + 1;
    if steps > 64 {
        steps = 64;
    }
    let sdt = dt / steps as f32;
    for _ in 0..steps {
        let a = -wn * wn * (*x - goal) - 2.0 * zeta * wn * *v;
        *v += a * sdt;
        *x += *v * sdt;
    }
}

unsafe fn end_hold() {
    CyberpunkVR_TwoHandActive = 0;
    CyberpunkVR_TwoHandBlend = 0.0;
    steer_reset();
}

/// The attachment placed on the weapon wrist `bone` at `target`: the support point and its rotation, kept as the last
/// valid one too.
unsafe fn place_attachment(target: &V3, bone: &Q, a: &Attachment) {
    let off = quat_rotate_vec(bone, &a.offset);
    for i in 0..3 {
        SUP_POS[i] = target[i] + off[i] + a.slide[i];
    }
    SUP_ROT = quat_mul(bone, &a.rotation);
    quat_norm(&mut SUP_ROT);
    SUP_PREV_POS = SUP_POS;
    SUP_PREV_ROT = SUP_ROT;
    SUP_VALID = true;
    SUP_PREV_HAVE = true;
}

/// UpdateSupport: reach, steering and release measured against the controllers, with the weapon hand built from its
/// own controller (`hm` model space, +Y the barrel, turned by the support hand's steer); the selected attachment in the
/// wrist's frame, for placement on the weapon hand's final pose.
unsafe fn update_support(targetR: *const f32, hm: *mut f32, wristR: *const f32, leftCtrlModel: *const f32) -> Option<Attachment> {
    let dt = pass_dt(&mut RIGHT_PREV_QPC);
    SUP_VALID = false;
    CyberpunkVR_DebugTwoHandDist = -1.0;
    if !leftCtrlModel.is_null() {
        g_VRTwoHandCtrlL = a3(leftCtrlModel);
    }
    g_VRTwoHandCtrlLValid = if leftCtrlModel.is_null() { 0 } else { 1 };

    // A NEW OWNER: a fading attachment is still a pose on the previous object or hand, and a squeeze held across the
    // transfer belongs to that previous owner.
    let support = support_hand();
    let dual = cvr_anim_two_guns_in_hand();
    let revision = super::xinput::PRIMARY_WEAPON_REVISION.load(core::sync::atomic::Ordering::Relaxed);
    let grip_down = left_grip_pressed() > 0.5;
    if let Some((prev_revision, prev_support, prev_dual)) = PREV_OWNER {
        if revision != prev_revision || support != prev_support || dual != prev_dual {
            SUP_PREV_HAVE = false;
            CyberpunkVR_TwoHandActive = 0;
            CyberpunkVR_TwoHandBlend = 0.0;
            (WEIGHT_X, WEIGHT_V, FAR_MS) = (0.0, 0.0, 0.0);
            GRIP_MISSING_MS = 0.0;
            GRIP_FRESH = false;
            if support != prev_support {
                GRIP_WAS = grip_down;
            }
            steer_reset();
        }
    }
    PREV_OWNER = Some((revision, support, dual));

    // THE WEIGHT, a spring toward 1 while engaged and 0 while not.
    {
        let goal = if CyberpunkVR_TwoHandActive != 0 { 1.0 } else { 0.0 };
        let ask_ms = if goal > 0.5 { CyberpunkVR_TwoHandEngageMs } else { CyberpunkVR_TwoHandReleaseMs };
        let span = if ask_ms > 1.0 { ask_ms } else { 1.0 };
        let mut zeta = CyberpunkVR_TwoHandEngageZeta;
        if zeta < 0.05 {
            zeta = 0.05;
        }
        if zeta > 1.5 {
            zeta = 1.5;
        }
        let wn = 6.283_185_31 / (span * 0.001);
        spring(&mut WEIGHT_X, &mut WEIGHT_V, goal, wn, zeta, dt);
        if WEIGHT_X < 0.0 {
            WEIGHT_X = 0.0;
            if WEIGHT_V < 0.0 {
                WEIGHT_V = 0.0;
            }
        }
        if WEIGHT_X > 1.2 {
            WEIGHT_X = 1.2;
        }
        CyberpunkVR_TwoHandWeight = WEIGHT_X;
    }

    // A SQUEEZE IS SPENT BY WHOEVER USED IT, decided before anything below can return.
    {
        let down = grip_down;
        if down {
            GRIP_MISSING_MS = 0.0;
        } else if GRIP_MISSING_MS < GRIP_SAMPLE_GRACE_MS {
            GRIP_MISSING_MS += dt * 1000.0;
        }
        if !down {
            GRIP_FRESH = false;
        } else if !GRIP_WAS {
            GRIP_FRESH = true;
        }
        if down && g_VRReloadFingerActive[support_hand()] != 0 {
            GRIP_FRESH = false;
        }
        GRIP_WAS = down;
    }
    // THE RELOAD OWNS THIS HAND WHILE IT IS WORKING -- once it has explicitly claimed the wrist (shared[162]): a preview
    // overlapping the support point while dashing leaves the closed hold the owner through its fade; the carry too, a
    // carried body, the weapon in the left hand, a gun in each hand.
    if g_VRReloadFingerActive[support] != 0 && (CyberpunkVR_TwoHandActive == 0 || reload_owns_support_hand()) {
        end_hold();
        return None;
    }
    if (CyberpunkVR_TwoHandSuppress & 1) != 0
        || should_hand_left_arm_to_body_carry(g_VRBodyCarryingState, CyberpunkVR_VrikBodyCarryHandOff)
    {
        end_hold();
        return None;
    }
    if CyberpunkVR_CarryLeft != 0 {
        end_hold();
        return None;
    }
    if dual {
        end_hold();
        return None;
    }
    select_hold();
    if CyberpunkVR_TwoHandGrip == 0 || CUR < 0 || !g_hasWeaponEquipped || targetR.is_null() || hm.is_null() {
        end_hold();
        return None;
    }
    let target_r = a3(targetR);

    // THE FORE-END'S STROKE, taken out of the hand before anything is measured against it.
    let mut slide = [0.0f32; 3];
    if CyberpunkVR_TwoHandActive != 0 && now_ms() < g_VRTwoHandSlideUntil {
        slide = g_VRTwoHandSlide;
    }
    let mut left_hand = [0.0f32; 3];
    let left_at = if !leftCtrlModel.is_null() {
        let c = a3(leftCtrlModel);
        for k in 0..3 {
            left_hand[k] = c[k] - slide[k];
        }
        Some(left_hand)
    } else {
        None
    };

    let mut hr = a4(hm);
    let pressed = left_grip_pressed() > 0.5;
    // THE OFFSET LIVES IN THE BONE'S FRAME: the right wrist bone is hm * wristR.
    let mut bone = if !wristR.is_null() {
        let mut b = quat_mul(&hr, &a4(wristR));
        quat_norm(&mut b);
        b
    } else {
        hr
    };

    // WHICH OF THE WEAPON'S HOLDS.
    {
        let mut cur_k: i32 = -1;
        for k in 0..CAND_N {
            if CAND[k] == CUR {
                cur_k = k as i32;
            }
        }
        let avail = |k: i32| ((CyberpunkVR_TwoHandSuppress >> (k + 1)) & 1) == 0;
        if CyberpunkVR_TwoHandActive != 0 && cur_k >= 0 && !avail(cur_k) {
            CyberpunkVR_TwoHandActive = 0;
        }
        if CyberpunkVR_TwoHandActive == 0 && CAND_N > 0 {
            let mut best: i32 = -1;
            let mut best_d = 1e9f32;
            for k in 0..CAND_N {
                if !avail(k as i32) {
                    continue;
                }
                let (ho, _) = hold_pose(CAND[k] as usize);
                let o = quat_rotate_vec(&bone, &ho);
                let mut d = 0.0f32;
                if let Some(la) = left_at {
                    let dx = la[0] - (target_r[0] + o[0]);
                    let dy = la[1] - (target_r[1] + o[1]);
                    let dz = la[2] - (target_r[2] + o[2]);
                    d = sqrt(dx * dx + dy * dy + dz * dz);
                }
                if best < 0 || d < best_d {
                    best = k as i32;
                    best_d = d;
                }
            }
            if best < 0 {
                CyberpunkVR_TwoHandBlend = 0.0;
                CyberpunkVR_DebugTwoHandHoldIdx = -1;
                CyberpunkVR_DebugTwoHandDist = -1.0;
                steer_reset();
                return None;
            }
            if cur_k < 0 || !avail(cur_k) || CyberpunkVR_TwoHandBlend <= 0.01 || best_d <= CyberpunkVR_TwoHandRadius {
                CUR = CAND[best as usize];
                cur_k = best;
            }
        }
        CyberpunkVR_DebugTwoHandHoldIdx = cur_k;
    }
    let cur = CUR as usize;
    let (hold_off, hold_rot) = hold_pose(cur);
    let mut off = quat_rotate_vec(&bone, &hold_off);
    let mut sup = [target_r[0] + off[0], target_r[1] + off[1], target_r[2] + off[2]];
    let held_off = HOLDS[cur].off;
    // The hold's leverage, published for the recoil.
    {
        let rl = sqrt(held_off[0] * held_off[0] + held_off[1] * held_off[1] + held_off[2] * held_off[2]);
        let mut lev = if CyberpunkVR_TwoHandLeverFull > 1e-3 { rl / CyberpunkVR_TwoHandLeverFull } else { 1.0 };
        if lev > 1.0 {
            lev = 1.0;
        }
        if lev < 0.0 {
            lev = 0.0;
        }
        CyberpunkVR_TwoHandLever = lev;
    }
    let mut dist = -1.0f32;
    if let Some(la) = left_at {
        let (dx, dy, dz) = (la[0] - sup[0], la[1] - sup[1], la[2] - sup[2]);
        dist = sqrt(dx * dx + dy * dy + dz * dz);
    }
    CyberpunkVR_DebugTwoHandDist = dist;

    // ENGAGE on a fresh squeeze inside the radius; HOLD until it is released.
    let in_range = dist >= 0.0 && dist <= CyberpunkVR_TwoHandRadius;
    let mut engaged_now = false;
    if CyberpunkVR_TwoHandActive != 0 {
        if !pressed && GRIP_MISSING_MS >= GRIP_SAMPLE_GRACE_MS {
            CyberpunkVR_TwoHandActive = 0;
        }
    } else if pressed && in_range && GRIP_FRESH {
        CyberpunkVR_TwoHandActive = 1;
        engaged_now = true;
    }
    // The finger ramp; a spent squeeze is offered nothing.
    let can_take = !pressed || GRIP_FRESH;
    let step = if CyberpunkVR_TwoHandFadeS > 0.01 { dt / CyberpunkVR_TwoHandFadeS } else { 1.0 };
    let want = if CyberpunkVR_TwoHandActive != 0 { 1.0 } else if in_range && can_take { 1.0 } else { 0.0 };
    if CyberpunkVR_TwoHandBlend < want {
        CyberpunkVR_TwoHandBlend += step;
        if CyberpunkVR_TwoHandBlend > want {
            CyberpunkVR_TwoHandBlend = want;
        }
    } else if CyberpunkVR_TwoHandBlend > want {
        CyberpunkVR_TwoHandBlend -= step;
        if CyberpunkVR_TwoHandBlend < want {
            CyberpunkVR_TwoHandBlend = want;
        }
    }
    // THE STANDARD PISTOL SUPPORT POINT IS A POSE, not a second orientation controller (main's 10eaa388c): the captured
    // fingers, the weld and the supported recoil stay; explicit absolute aiming remains, fore-end grips keep steering.
    let steer = CyberpunkVR_TwoHandAim != 0 && !(CyberpunkVR_WeaponClass == 1 && CyberpunkVR_TwoHandAim == 1);
    if !steer || LAST_FAMILY != PRIMARY {
        steer_reset();
    }
    LAST_FAMILY = PRIMARY;
    // HOW MUCH OF THE STEER IS ON THE WEAPON: in with the hold, out on its own and latched.
    {
        let on = steer && CyberpunkVR_TwoHandActive != 0;
        let goal = if on { 1.0 } else { 0.0 };
        let ask_ms = if on { CyberpunkVR_TwoHandEngageMs } else { CyberpunkVR_TwoHandAimReleaseMs };
        let span = if ask_ms > 1.0 { ask_ms } else { 1.0 };
        let mut zeta = if on { CyberpunkVR_TwoHandEngageZeta } else { 1.0 };
        if zeta < 0.05 {
            zeta = 0.05;
        }
        if zeta > 1.5 {
            zeta = 1.5;
        }
        let wn = 6.283_185_31 / (span * 0.001);
        spring(&mut AIM_X, &mut AIM_V, goal, wn, zeta, dt);
        if AIM_X < 0.0 {
            AIM_X = 0.0;
            if AIM_V < 0.0 {
                AIM_V = 0.0;
            }
        }
        if AIM_X > 1.2 {
            AIM_X = 1.2;
        }
    }
    // THE STEER, measured only while the hand is on the weapon, low-passed in the bone's own frame.
    if steer && CyberpunkVR_TwoHandActive != 0 {
        if let Some(la) = left_at {
            let rl = sqrt(held_off[0] * held_off[0] + held_off[1] * held_off[1] + held_off[2] * held_off[2]);
            let d = [la[0] - target_r[0], la[1] - target_r[1], la[2] - target_r[2]];
            let dl = sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
            if rl > 1e-4 && dl > 1e-4 {
                let bc = quat_conj(&bone);
                let dm = [d[0] / dl, d[1] / dl, d[2] / dl];
                let d_l = quat_rotate_vec(&bc, &dm);
                if engaged_now || !REF_HAVE {
                    REF_E = d_l;
                    S_DL = d_l;
                    REF_HAVE = true;
                }
                // SCALED BY THE LEVERAGE, like the deadzone and the ceiling: this is all the drawn support hand follows,
                // so its whole constant reads as that hand trailing the weapon hand; at a rifle's baseline a quarter.
                let mut tau_lever = if CyberpunkVR_TwoHandLeverFull > 1e-3 { rl / CyberpunkVR_TwoHandLeverFull } else { 1.0 };
                if tau_lever > 1.0 {
                    tau_lever = 1.0;
                }
                let scaled = CyberpunkVR_TwoHandAimTau * (1.0 - 0.75 * tau_lever);
                let tau = if scaled > 0.01 { scaled } else { 0.01 };
                let a = 1.0 - exp(-dt / tau);
                for i in 0..3 {
                    S_DL[i] += (d_l[i] - S_DL[i]) * a;
                }
                // 1 relative: from where the left hand was at the squeeze; 2 absolute: from the captured hold.
                let ref_l = if CyberpunkVR_TwoHandAim == 2 {
                    [hold_off[0] / rl, hold_off[1] / rl, hold_off[2] / rl]
                } else {
                    let el = sqrt(REF_E[0] * REF_E[0] + REF_E[1] * REF_E[1] + REF_E[2] * REF_E[2]);
                    let ei = if el > 1e-6 { 1.0 / el } else { 0.0 };
                    [REF_E[0] * ei, REF_E[1] * ei, REF_E[2] * ei]
                };
                STEER_ANG = 0.0;
                let sl = sqrt(S_DL[0] * S_DL[0] + S_DL[1] * S_DL[1] + S_DL[2] * S_DL[2]);
                if sl > 1e-5 {
                    let ds = [S_DL[0] / sl, S_DL[1] / sl, S_DL[2] / sl];
                    let dot = ref_l[0] * ds[0] + ref_l[1] * ds[1] + ref_l[2] * ds[2];
                    let axis_l = [ref_l[1] * ds[2] - ref_l[2] * ds[1], ref_l[2] * ds[0] - ref_l[0] * ds[2],
                                  ref_l[0] * ds[1] - ref_l[1] * ds[0]];
                    let al = sqrt(axis_l[0] * axis_l[0] + axis_l[1] * axis_l[1] + axis_l[2] * axis_l[2]);
                    if al > 1e-6 && dot > -0.999 {
                        let mut ang = atan2(al, dot);
                        // THE OFF HAND STEERS IN PROPORTION TO ITS LEVERAGE; deadzone and ceiling by the same.
                        let mut lev = if CyberpunkVR_TwoHandLeverFull > 1e-3 { rl / CyberpunkVR_TwoHandLeverFull } else { 1.0 };
                        if lev > 1.0 {
                            lev = 1.0;
                        }
                        if lev < 0.0 {
                            lev = 0.0;
                        }
                        let dead = CyberpunkVR_TwoHandAimDeadDeg * (1.0 - 0.75 * lev) * 0.017_453_292_52;
                        let cap = CyberpunkVR_TwoHandAimMaxDeg * lev * 0.017_453_292_52;
                        ang = if ang > dead { ang - dead } else { 0.0 };
                        ang *= CyberpunkVR_TwoHandAimGain * lev;
                        if ang > cap {
                            ang = cap;
                        }
                        STEER_AXIS = [axis_l[0] / al, axis_l[1] / al, axis_l[2] / al];
                        STEER_ANG = ang;
                    }
                }
            }
        }
    }
    if CyberpunkVR_TwoHandActive == 0 {
        REF_HAVE = false;
    }
    // ...AND THE STEER GOES ONTO THE WEAPON by however much of it has arrived (or is still leaving).
    {
        let mut aw = AIM_X;
        if aw < 0.0 {
            aw = 0.0;
        }
        if aw > 1.0 {
            aw = 1.0;
        }
        let ang = STEER_ANG * aw;
        CyberpunkVR_DebugTwoHandAimDeg = ang * 57.295_779_5;
        if ang > 1e-5 {
            let s = sin(ang * 0.5);
            let q_l = [STEER_AXIS[0] * s, STEER_AXIS[1] * s, STEER_AXIS[2] * s, cos(ang * 0.5)];
            let mut nb = quat_mul(&bone, &q_l);
            quat_norm(&mut nb);
            bone = nb;
            if !wristR.is_null() {
                let w = a4(wristR);
                let wc = [-w[0], -w[1], -w[2], w[3]];
                let mut nh = quat_mul(&bone, &wc);
                quat_norm(&mut nh);
                hr = nh;
            } else {
                hr = bone;
            }
            put4(hm, &hr);
            // the support point from the AIMED hand
            off = quat_rotate_vec(&bone, &hold_off);
            sup = [target_r[0] + off[0], target_r[1] + off[1], target_r[2] + off[2]];
        }
    }
    // A HELD GRIP ENDS WHERE THE HAND DOES: past the break radius for BreakMs, against the steered support point before
    // the visual recoil.
    {
        let mut away = false;
        if CyberpunkVR_TwoHandActive != 0 && CyberpunkVR_TwoHandAim != 2 && CyberpunkVR_TwoHandBreakRadius > 0.0 {
            if let Some(la) = left_at {
                let (dx, dy, dz) = (la[0] - sup[0], la[1] - sup[1], la[2] - sup[2]);
                away = sqrt(dx * dx + dy * dy + dz * dz) > CyberpunkVR_TwoHandBreakRadius;
            }
        }
        FAR_MS = if away { FAR_MS + dt * 1000.0 } else { 0.0 };
        if away && FAR_MS >= CyberpunkVR_TwoHandBreakMs {
            CyberpunkVR_TwoHandActive = 0;
            FAR_MS = 0.0;
        }
    }
    Some(Attachment { offset: hold_off, rotation: hold_rot, slide })
}

/// SolveWeaponHandPose: the support measured against the controllers, then the visual recoil on the weapon hand (the
/// rise about the steered controller's own right axis, the back travel horizontal in the view's frame), then the
/// attachment placed on that final pose -- the recoil stays out of the tracked grip input.
pub(crate) unsafe fn solve_weapon_hand_pose(target: *mut f32, controller_rot: *mut f32, wrist: *const f32, support_ctrl: *const f32,
                                            view_rot: *const f32) {
    let attachment = update_support(target, controller_rot, wrist, support_ctrl);
    let (mut back_m, mut rise_rad) = (0.0f32, 0.0f32);
    RecoilSample(1, 1, &mut back_m, &mut rise_rad);
    if rise_rad != 0.0 {
        let half = 0.5 * rise_rad;
        let local_rise = [sin(half), 0.0, 0.0, cos(half)];
        let mut raised = quat_mul(&a4(controller_rot), &local_rise);
        quat_norm(&mut raised);
        put4(controller_rot, &raised);
    }
    if back_m != 0.0 {
        let (mut back, mut down) = (1.0f32, 0.0f32);
        RecoilTravelMix(&mut back, &mut down);
        let vf = quat_rotate_vec(&a4(view_rot), &[0.0, 1.0, 0.0]);
        let horizontal = sqrt(vf[0] * vf[0] + vf[1] * vf[1]);
        if horizontal > 1e-5 {
            *target.add(0) -= vf[0] / horizontal * back * back_m;
            *target.add(1) -= vf[1] / horizontal * back * back_m;
        }
        *target.add(2) -= down * back_m;
    }
    let Some(attachment) = attachment else { return };
    DRAWN_ATTACHMENT = attachment;
    let mut bone = quat_mul(&a4(controller_rot), &a4(wrist));
    quat_norm(&mut bone);
    place_attachment(&a3(target), &bone, &attachment);
}
/// A firearm (the weapon classes 1..4) drawn in the left hand.
pub(crate) unsafe fn left_firearm_drawn() -> bool {
    (1..=4).contains(&CyberpunkVR_WeaponClass) && cvr_anim_weapon_in_left_hand()
}
/// RebaseFirearmSupport: a confirmed firearm hold placed again on the weapon wrist the arm solver actually wrote (its
/// collision adjustment and reach limit included); true unless the reload has claimed the support wrist.
pub(crate) unsafe fn rebase_firearm_support(bone_buf: *mut u8, weapon_wrist: i32) -> bool {
    if !SUP_VALID || CyberpunkVR_TwoHandActive == 0 || !(1..=4).contains(&CyberpunkVR_WeaponClass) || weapon_wrist < 0
        || weapon_wrist >= VRIK_FKCount()
    {
        return false;
    }
    VRIK_ComputeFK(bone_buf, weapon_wrist + 1);
    let w = weapon_wrist as usize;
    place_attachment(&g_fkPos[w], &g_fkRot[w], &DRAWN_ATTACHMENT);
    !reload_owns_support_hand()
}
/// TwoHandWeaponHandMoved: the weapon hand was stopped (hand collision) after the support point was built from it; the
/// point on the weapon moved with it, and the support hand goes along.
pub(crate) unsafe fn two_hand_weapon_hand_moved(moved: &V3) {
    for k in 0..3 {
        SUP_POS[k] += moved[k];
        SUP_PREV_POS[k] += moved[k];
    }
}
/// ApplyFirearmSupportStop: the support wrist welded, its stop applied once (physics receives the requested grip
/// exactly once), and the stop's own correction faded out of the drawn pose as the attachment takes ownership.
pub(crate) unsafe fn apply_firearm_support_stop(side: i32, target: &mut V3, hand_rot: &mut Q) {
    let attached = cvr_anim_two_hand_left(target.as_mut_ptr(), hand_rot.as_mut_ptr());
    let (requested, rotation) = (*target, *hand_rot);
    super::arm::VRIK_ApplyHandStop(side, target.as_mut_ptr(), hand_rot.as_mut_ptr());
    if attached {
        let w = CyberpunkVR_TwoHandWeight.clamp(0.0, 1.0);
        for k in 0..3 {
            target[k] += (requested[k] - target[k]) * w;
        }
        let a = *hand_rot;
        slerp_into(&a, &rotation, w, hand_rot);
    }
}

/// The performance counter in milliseconds.
unsafe fn carry_now_ms() -> f64 {
    let f = clock_qpc_frequency();
    let n = clock_qpc();
    if f != 0.0 {
        n as f64 * 1000.0 / f
    } else {
        0.0
    }
}

/// A spring released from 1 with zero velocity and pulled to 0: the analytic step response.
fn released_spring(zeta: f32, wn: f32, t: f32) -> f32 {
    if zeta < 0.999 {
        let wd = wn * sqrt(1.0 - zeta * zeta);
        exp(-zeta * wn * t) * (cos(wd * t) + (zeta * wn / wd) * sin(wd * t))
    } else {
        exp(-wn * t) * (1.0 + wn * t)
    }
}

/// Slerp from a toward b by w (b flipped to a's side), into `out`.
fn slerp_into(a: &Q, b0: &Q, w: f32, out: &mut Q) {
    let mut b = *b0;
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        for c in b.iter_mut() {
            *c = -*c;
        }
        dot = -dot;
    }
    if dot > 0.9995 {
        for i in 0..4 {
            out[i] = a[i] + (b[i] - a[i]) * w;
        }
    } else {
        let th0 = acos(dot);
        let th = th0 * w;
        let s0 = sin(th0);
        let sa = sin(th0 - th) / s0;
        let sb = sin(th) / s0;
        for i in 0..4 {
            out[i] = a[i] * sa + b[i] * sb;
        }
    }
    quat_norm(out);
}

/// COMING OUT OF THE HOLD WITHOUT A JUMP: the welded wrist rotation latched as the carry starts, faded out on a
/// spring.
#[no_mangle]
pub unsafe extern "C" fn CarryLeftBlend(handRot: *mut f32) {
    let on = CyberpunkVR_CarryLeft != 0;
    if on && !CARRY_WAS {
        // The hold as it last stood -- the sticky copy, not the live one.
        if SUP_PREV_HAVE {
            CARRY_ROT0 = SUP_PREV_ROT;
            CARRY_HAVE = true;
            CARRY_START_MS = carry_now_ms();
        } else {
            CARRY_HAVE = false;
        }
    }
    if !on {
        CARRY_HAVE = false;
    }
    CARRY_WAS = on;
    CyberpunkVR_DebugCarryBlend = 0.0;
    if !on || !CARRY_HAVE || handRot.is_null() {
        return;
    }
    let span = if CyberpunkVR_CarryBlendMs > 1.0 { CyberpunkVR_CarryBlendMs } else { 1.0 };
    let t = ((carry_now_ms() - CARRY_START_MS) / 1000.0) as f32;
    let total = span / 1000.0;
    if t >= total * 1.6 {
        CARRY_HAVE = false;
        return;
    }
    let mut zeta = CyberpunkVR_CarryBlendZeta;
    if zeta < 0.05 {
        zeta = 0.05;
    }
    if zeta > 1.5 {
        zeta = 1.5;
    }
    let wn = 6.283_185_31 / total;
    let mut w = released_spring(zeta, wn, t);
    if w < 0.0 && w > -0.0005 {
        w = 0.0;
    }
    CyberpunkVR_DebugCarryBlend = w;
    let a = a4(handRot);
    let mut out = [0.0f32; 4];
    slerp_into(&a, &CARRY_ROT0, w, &mut out);
    put4(handRot, &out);
}

fn pose_mul(pa: &V3, qa: &Q, pb: &V3, qb: &Q) -> (V3, Q) {
    let r = quat_rotate_vec(qa, pb);
    let pc = [pa[0] + r[0], pa[1] + r[1], pa[2] + r[2]];
    let mut qc = quat_mul(qa, qb);
    quat_norm(&mut qc);
    (pc, qc)
}
fn pose_inv(p: &V3, q: &Q) -> (V3, Q) {
    let qo = quat_conj(q);
    let r = quat_rotate_vec(&qo, p);
    ([-r[0], -r[1], -r[2]], qo)
}
/// Slerp from identity toward q by w, w outside [0,1] included.
fn quat_from_identity(q: &Q, w: f32) -> Q {
    let mut b = *q;
    let mut dot = b[3];
    if dot < 0.0 {
        for c in b.iter_mut() {
            *c = -*c;
        }
        dot = -dot;
    }
    let mut out = if dot > 0.9995 {
        [b[0] * w, b[1] * w, b[2] * w, 1.0 + (b[3] - 1.0) * w]
    } else {
        let th0 = acos(dot);
        let th = th0 * w;
        let s0 = sin(th0);
        let sa = sin(th0 - th) / s0;
        let sb = sin(th) / s0;
        [b[0] * sb, b[1] * sb, b[2] * sb, sa + b[3] * sb]
    };
    quat_norm(&mut out);
    out
}

/// The drawn left wrist, recorded at the end of the left hand's own solve.
#[no_mangle]
pub unsafe extern "C" fn CarryRecordLeft(pos: *const f32, rot: *const f32) {
    if pos.is_null() || rot.is_null() {
        return;
    }
    LEFT_DRAWN_POS = a3(pos);
    LEFT_DRAWN_ROT = a4(rot);
    LEFT_DRAWN_HAVE = true;
}

/// THE RETURN INTO THE RIGHT HAND, ON A SPRING: X = inv(L0) * R0 at the hand-over, D = inv(R) * (L1 * X) at the
/// return, the right wrist drawn at R * D faded to R. Also the reach back for the carried weapon and its offer.
#[no_mangle]
pub unsafe extern "C" fn CarryReturnRight(target: *mut f32, hm: *mut f32, wristR: *const f32) {
    let on = CyberpunkVR_CarryLeft != 0;
    if target.is_null() || hm.is_null() {
        RETURN_WAS = on;
        return;
    }
    let tgt = a3(target);
    let hmv = a4(hm);
    let bone = if !wristR.is_null() {
        let mut b = quat_mul(&hmv, &a4(wristR));
        quat_norm(&mut b);
        b
    } else {
        hmv
    };
    if on && !RETURN_WAS {
        // the carry has just begun: how the weapon sits on the hand that now holds it
        if LEFT_DRAWN_HAVE {
            let (li, lq) = pose_inv(&LEFT_DRAWN_POS, &LEFT_DRAWN_ROT);
            (CARRY_X_POS, CARRY_X_ROT) = pose_mul(&li, &lq, &tgt, &bone);
            CARRY_X_HAVE = true;
        } else {
            CARRY_X_HAVE = false;
        }
        RET_HAVE = false; // a new carry cancels a return still settling
        R_GRIP_FRESH = false; // and this carry has SPENT the squeeze that started it
    } else if !on && RETURN_WAS {
        // ...and it has just ended: where the weapon IS, against the wrist it lands on
        if CARRY_X_HAVE && LEFT_DRAWN_HAVE {
            let (wp, wq) = pose_mul(&LEFT_DRAWN_POS, &LEFT_DRAWN_ROT, &CARRY_X_POS, &CARRY_X_ROT);
            let (ri, rq) = pose_inv(&tgt, &bone);
            (RET_POS, RET_ROT) = pose_mul(&ri, &rq, &wp, &wq);
            RET_HAVE = true;
            RET_START_MS = carry_now_ms();
        }
        CARRY_X_HAVE = false;
    }
    RETURN_WAS = on;

    // HOW FAR THIS HAND IS FROM THE CARRIED WEAPON, and the offer that answers it.
    {
        let dt = pass_dt(&mut RETURN_PREV_QPC);
        let r_down = right_grip_pressed() > 0.5;
        if !r_down {
            R_GRIP_FRESH = false;
        } else if !R_GRIP_WAS {
            R_GRIP_FRESH = true;
        }
        R_GRIP_WAS = r_down;
        let can_take = !r_down || R_GRIP_FRESH;
        let mut dist = -1.0f32;
        if on && CARRY_X_HAVE && LEFT_DRAWN_HAVE {
            let (wp, _) = pose_mul(&LEFT_DRAWN_POS, &LEFT_DRAWN_ROT, &CARRY_X_POS, &CARRY_X_ROT);
            let (dx, dy, dz) = (wp[0] - tgt[0], wp[1] - tgt[1], wp[2] - tgt[2]);
            dist = sqrt(dx * dx + dy * dy + dz * dz);
        }
        CyberpunkVR_DebugCarryReach = dist;
        let goal = if dist >= 0.0 && dist <= CyberpunkVR_CarryGripRadius && can_take { 1.0 } else { 0.0 };
        let span = if CyberpunkVR_CarryGripMs > 1.0 { CyberpunkVR_CarryGripMs } else { 1.0 };
        let mut gz = CyberpunkVR_CarryGripZeta;
        if gz < 0.05 {
            gz = 0.05;
        }
        if gz > 1.5 {
            gz = 1.5;
        }
        let gwn = 6.283_185_31 / (span * 0.001);
        spring(&mut GRIP_G, &mut GRIP_GV, goal, gwn, gz, dt);
        if GRIP_G < 0.0 {
            GRIP_G = 0.0;
            if GRIP_GV < 0.0 {
                GRIP_GV = 0.0;
            }
        }
        if GRIP_G > 1.0 {
            GRIP_G = 1.0; // fingers do not curl PAST the pose they were captured in
        }
        CyberpunkVR_CarryGripBlend = GRIP_G;
    }

    CyberpunkVR_DebugCarryReturn = 0.0;
    if !RET_HAVE || CyberpunkVR_CarryReturnHand == 0 {
        return;
    }
    let span = if CyberpunkVR_CarryReturnMs > 1.0 { CyberpunkVR_CarryReturnMs } else { 1.0 };
    let total = span / 1000.0;
    let t = ((carry_now_ms() - RET_START_MS) / 1000.0) as f32;
    if t < 0.0 || t >= total * 1.6 {
        RET_HAVE = false;
        return;
    }
    let mut zeta = CyberpunkVR_CarryReturnZeta;
    if zeta < 0.05 {
        zeta = 0.05;
    }
    if zeta > 1.5 {
        zeta = 1.5;
    }
    let wn = 6.283_185_31 / total;
    let mut w = released_spring(zeta, wn, t);
    if w > 1.2 {
        w = 1.2;
    }
    if w < -0.3 {
        w = -0.3;
    }
    CyberpunkVR_DebugCarryReturn = w;
    if w > -0.0005 && w < 0.0005 {
        return;
    }
    // Position first, in the wrist's own frame; then the twist, and back out to the controller frame.
    let off = [RET_POS[0] * w, RET_POS[1] * w, RET_POS[2] * w];
    let off_m = quat_rotate_vec(&bone, &off);
    put3(target, &[tgt[0] + off_m[0], tgt[1] + off_m[1], tgt[2] + off_m[2]]);
    let dq = quat_from_identity(&RET_ROT, w);
    let mut nb = quat_mul(&bone, &dq);
    quat_norm(&mut nb);
    if !wristR.is_null() {
        let wr = a4(wristR);
        let wc = [-wr[0], -wr[1], -wr[2], wr[3]];
        let mut nh = quat_mul(&nb, &wc);
        quat_norm(&mut nh);
        put4(hm, &nh);
    } else {
        put4(hm, &nb);
    }
}

/// THE LEFT HAND'S HALF: the wrist mixed toward the hold by its weight (live pose, or the last one while it fades).
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_two_hand_left(target: *mut f32, handRot: *mut f32) -> bool {
    if target.is_null() {
        return false;
    }
    if CyberpunkVR_CarryLeft != 0 || cvr_anim_two_guns_in_hand() {
        return false;
    }
    let mut w = CyberpunkVR_TwoHandWeight;
    if w <= 0.0005 {
        return false;
    }
    if w > 1.0 {
        w = 1.0;
    }
    let (sp, sq) = if SUP_VALID {
        (SUP_POS, SUP_ROT)
    } else if SUP_PREV_HAVE {
        (SUP_PREV_POS, SUP_PREV_ROT)
    } else {
        return false;
    };
    for k in 0..3 {
        *target.add(k) += (sp[k] - *target.add(k)) * w;
    }
    if !handRot.is_null() {
        let a = a4(handRot);
        let mut out = [0.0f32; 4];
        slerp_into(&a, &sq, w, &mut out);
        put4(handRot, &out);
    }
    true
}

/// The captured finger curl, mixed on by the ramp; mirrored to the right hand with the weapon in the left.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_two_hand_fingers(boneBuf: *mut u8) {
    select_hold();
    if CUR < 0 || HOLDS[CUR as usize].finger_count <= 0 {
        return;
    }
    let b = CyberpunkVR_TwoHandBlend;
    if b <= 0.001 {
        return;
    }
    let mirrored = cvr_anim_weapon_in_left_hand();
    let h = &HOLDS[CUR as usize];
    let mut k = 0usize;
    while (k as i32) < h.finger_count && k < 32 {
        let kk = k;
        k += 1;
        let mut bi: i32 = -1;
        let mut s = 0;
        while s < g_VRSmokeFingerCountL && s < 32 {
            if strcmp(&h.finger_name[kk], bytes_of(&g_VRSmokeFingerNameL[s as usize])) == 0 {
                bi = g_VRSmokeFingerIdxL[s as usize];
                break;
            }
            s += 1;
        }
        if !(0..MAXB).contains(&bi) {
            continue;
        }
        let mut tq = h.finger[kk];
        if mirrored {
            let mut mq = [0.0f32; 4];
            if !cvr_anim_mirror_finger_to_right(bi, tq.as_ptr(), &mut bi, mq.as_mut_ptr()) {
                continue;
            }
            tq = mq;
        }
        let qp = br(boneBuf, bi);
        if b >= 0.999 {
            put4(qp, &tq);
            continue;
        }
        let q = a4(qp);
        let dot = q[0] * tq[0] + q[1] * tq[1] + q[2] * tq[2] + q[3] * tq[3];
        let s = if dot < 0.0 { -b } else { b };
        let nq = [q[0] * (1.0 - b) + tq[0] * s, q[1] * (1.0 - b) + tq[1] * s, q[2] * (1.0 - b) + tq[2] * s,
                  q[3] * (1.0 - b) + tq[3] * s];
        let nl = sqrt(nq[0] * nq[0] + nq[1] * nq[1] + nq[2] * nq[2] + nq[3] * nq[3]);
        if nl > 1e-6 {
            put4(qp, &[nq[0] / nl, nq[1] / nl, nq[2] / nl, nq[3] / nl]);
        }
    }
}

/// DISK, OFF THE ANIMATION THREAD: every CyberpunkVR_TwoHandGrip_*.ini read once at startup; a capture saved.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_two_hand_tick() {
    if !SCANNED {
        SCANNED = true;
        if let Some(pattern) = grip_path("CyberpunkVR_TwoHandGrip_*.ini") {
            let pattern = std::path::PathBuf::from(pattern);
            if let Some(dir) = pattern.parent() {
                let pre = b"CyberpunkVR_TwoHandGrip_";
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for e in entries.flatten() {
                        let name = e.file_name();
                        let nm = name.to_string_lossy();
                        let nb = nm.as_bytes();
                        // FindFirstFile's "*.ini", then the C++'s case-sensitive prefix
                        if nb.len() < 4 || !nb[nb.len() - 4..].eq_ignore_ascii_case(b".ini") || !nb.starts_with(pre) {
                            continue;
                        }
                        let mut weapon = [0u8; 64];
                        let tail = &nb[pre.len()..];
                        let n = tail.len().min(63);
                        weapon[..n].copy_from_slice(&tail[..n]);
                        if let Some(dot) = weapon[..cstr_len(&weapon)].iter().rposition(|&c| c == b'.') {
                            weapon[dot] = 0;
                        }
                        if weapon[0] == 0 {
                            continue;
                        }
                        let h = add_hold(&weapon);
                        if h < 0 {
                            break;
                        }
                        if let Some(path) = grip_path(&nm) {
                            if let Ok(bytes) = std::fs::read(path) {
                                parse_into(h as usize, &bytes);
                            }
                        }
                    }
                }
            }
        }
        CyberpunkVR_DebugTwoHandLoaded = HOLD_COUNT as i32;
        CUR_FOR[0] = 1;
        CUR_FOR[1] = 0; // force a re-select against the freshly loaded table
    }
    if !SAVE_REQ {
        return;
    }
    SAVE_REQ = false;
    if CUR < 0 {
        CyberpunkVR_DebugTwoHandSaved = -1;
        return;
    }
    let h = &HOLDS[CUR as usize];
    let weapon = as_str(&h.weapon);
    let mut text = format!(
        "# CyberpunkVR two-hand grip pose v1 (auto-generated by VRTwoHandCapture)\n\
         # Weapon: {}. One frame of the game's own two-handed hold, captured with VRIK off.\n\
         # W px py pz qx qy qz qw        left wrist IN THE RIGHT WRIST'S FRAME\n\
         # F <bone> qx qy qz qw          finger: parent-local rotation only\n",
        weapon
    );
    text.push_str(&format!(
        "W {} {} {} {} {} {} {}\n",
        format_g9(h.off[0]),
        format_g9(h.off[1]),
        format_g9(h.off[2]),
        format_g9(h.rot[0]),
        format_g9(h.rot[1]),
        format_g9(h.rot[2]),
        format_g9(h.rot[3])
    ));
    let mut k = 0;
    while k < h.finger_count && k < 32 {
        let f = &h.finger[k as usize];
        text.push_str(&format!(
            "F {} {} {} {} {}\n",
            as_str(&h.finger_name[k as usize]),
            format_g9(f[0]),
            format_g9(f[1]),
            format_g9(f[2]),
            format_g9(f[3])
        ));
        k += 1;
    }
    let saved = grip_path(&format!("CyberpunkVR_TwoHandGrip_{}.ini", weapon))
        .map(|path| std::fs::write(path, text.replace('\n', "\r\n")).is_ok())
        .unwrap_or(false);
    CyberpunkVR_DebugTwoHandSaved = if saved { 1 } else { -1 };
}
