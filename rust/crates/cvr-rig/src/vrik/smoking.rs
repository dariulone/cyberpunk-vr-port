//! The smoking mod's cigarette and lighter poses (formerly src/Anim/SmokingPose.cpp): captured finger locals and the
//! WeaponRight / WeaponLeft slots replayed every pass, the lighter thumb worked by the trigger, the cigarette pinned
//! to the lips on its own leaf (WeaponRight1), and left-handed the lighter mirrored into the right hand.
use super::env::*;
use super::weapon_arm::{cvr_anim_mirror_finger_to_right, cvr_anim_mirror_weapon_slot_to_right};
use super::*;
use crate::math::{quat_mul, quat_norm};

extern "C" {
    static mut g_VRSmokeFingerCount: i32;
    static mut g_VRSmokeFingerCountL: i32;
    static mut g_VRSmokeFingerIdx: [i32; 32];
    static mut g_VRSmokeFingerIdxL: [i32; 32];
    static mut g_VRSmokeFingerRot: [[f32; 4]; 32];
    static mut g_VRSmokeFingerRotL: [[f32; 4]; 32];
    static mut g_VRSmokeFingerRotLC: [[f32; 4]; 32];
    static mut g_VRSmokeFingerActive: i32;
    static mut g_VRSmokeFingerActiveL: i32;
    static mut g_VRSmokeFingerCapture: i32;
    static mut g_VRSmokeFingerCaptureL: i32;
    static mut g_VRSmokeFingerHave: i32;
    static mut g_VRSmokeFingerHaveL: i32;
    static mut g_VRSmokeCigIdx: i32;
    static mut g_VRSmokeLighterIdx: i32;
    static mut g_VRSmokeMouthBoneIdx: i32;
    static mut g_VRSmokeCigHave: i32;
    static mut g_VRSmokeCigEnable: i32;
    static mut g_VRSmokeCigPos: [f32; 3];
    static mut g_VRSmokeCigRot: [f32; 4];
    static mut g_VRSmokeCigOffP: [f32; 3];
    static mut g_VRSmokeCigOffQ: [f32; 4];
    static mut g_VRSmokeCigScaleY: f32;
    static mut g_VRSmokeMouthAnchor: i32;
    static mut g_VRSmokeAnchorValid: i32;
    static mut g_VRSmokeAnchorLocalPos: [f32; 3];
    static mut g_VRSmokeAnchorLocalRot: [f32; 4];
    static mut g_VRSmokeLighterHave: i32;
    static mut g_VRSmokeLighterEnable: i32;
    static mut g_VRSmokeLighterPos: [f32; 3];
    static mut g_VRSmokeLighterRot: [f32; 4];
    static mut g_VRSmokeLighterOffP: [f32; 3];
    static mut g_VRSmokeLighterOffQ: [f32; 4];
    static mut g_VRSmokeThumbFlickL: [f32; 4];
    static mut g_VRSmokeThumbPressManualL: f32;
    static mut g_VRSmokeThumbIsL: [i32; 32];
    static mut g_VRSmokeLeftUseCig: i32;
    static mut g_VRSmokeRightLighter: i32;
    static mut g_VRSmokeCigLHave: i32;
    static mut g_VRSmokeCigLPos: [f32; 3];
    static mut g_VRSmokeCigLRot: [f32; 4];
}

const K_LEFT_TRIGGER_ANALOG: usize = 154; // Utils/SharedSlots.hpp
const K_RIGHT_TRIGGER_ANALOG: usize = 160;

/// The lighter thumb's press, 0..1: the trigger of the hand holding it, analog, or the manual override.
unsafe fn thumb_press(trigger: usize) -> f32 {
    let mut press = if g_pSharedHands.is_null() { 0.0 } else { *g_pSharedHands.add(trigger) };
    if g_VRSmokeThumbPressManualL > press {
        press = g_VRSmokeThumbPressManualL;
    }
    if press < 0.0 {
        0.0
    } else if press > 1.0 {
        1.0
    } else {
        press
    }
}

#[inline(always)]
unsafe fn flick(press: f32) -> Q {
    let f = g_VRSmokeThumbFlickL;
    let mut fq = [f[0] * press, f[1] * press, f[2] * press, f[3] * press + (1.0 - press)];
    quat_norm(&mut fq);
    fq
}

#[inline(always)]
fn in_rig(bone: i32) -> bool {
    (0..MAXB).contains(&bone)
}

/// LEFT-HANDED: the left hand's lighter hold mirrored onto the right hand through the rig's twins.
unsafe fn right_lighter_pose(boneBuf: *mut u8) {
    if g_VRSmokeFingerHaveL != 0 {
        let press = thumb_press(K_RIGHT_TRIGGER_ANALOG);
        let fq = flick(press);
        let mut k = 0;
        while k < g_VRSmokeFingerCountL && k < 32 {
            let ku = k as usize;
            k += 1;
            let bi = g_VRSmokeFingerIdxL[ku];
            if !in_rig(bi) {
                continue;
            }
            let mut local = g_VRSmokeFingerRotL[ku];
            if press > 0.0001 && g_VRSmokeThumbIsL[ku] != 0 {
                local = quat_mul(&g_VRSmokeFingerRotL[ku], &fq);
                quat_norm(&mut local);
            }
            let mut right = -1;
            let mut out = [0.0f32; 4];
            if !cvr_anim_mirror_finger_to_right(bi, local.as_ptr(), &mut right, out.as_mut_ptr()) || !in_rig(right) {
                continue;
            }
            put4(br(boneBuf, right), &out);
        }
    }
    if g_VRSmokeLighterEnable != 0 && g_VRSmokeLighterHave != 0 && in_rig(g_VRSmokeCigIdx) {
        let pos = [
            g_VRSmokeLighterPos[0] + g_VRSmokeLighterOffP[0],
            g_VRSmokeLighterPos[1] + g_VRSmokeLighterOffP[1],
            g_VRSmokeLighterPos[2] + g_VRSmokeLighterOffP[2],
        ];
        let mut rot = quat_mul(&g_VRSmokeLighterRot, &g_VRSmokeLighterOffQ);
        quat_norm(&mut rot);
        let (mut p, mut r) = ([0.0f32; 3], [0.0f32; 4]);
        if cvr_anim_mirror_weapon_slot_to_right(pos.as_ptr(), rot.as_ptr(), p.as_mut_ptr(), r.as_mut_ptr()) {
            quat_norm(&mut r);
            put3(bt(boneBuf, g_VRSmokeCigIdx), &p);
            put4(br(boneBuf, g_VRSmokeCigIdx), &r);
        }
    }
}

#[inline(always)]
unsafe fn set_scale(boneBuf: *mut u8, bone: i32) {
    let s = bone12(boneBuf, bone);
    (*s)[8] = 1.0;
    (*s)[9] = g_VRSmokeCigScaleY;
    (*s)[10] = 1.0;
}

/// The cigarette hand: capture on request, else the grip fingers, the cigarette slot, and the mouth pin.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_smoking_cig_pose(boneBuf: *mut u8) {
    if g_VRSmokeFingerCapture != 0 {
        // Fingers: rotation only (parent-local).
        let mut k = 0;
        while k < g_VRSmokeFingerCount && k < 32 {
            let ku = k as usize;
            k += 1;
            let bi = g_VRSmokeFingerIdx[ku];
            if !in_rig(bi) {
                continue;
            }
            g_VRSmokeFingerRot[ku] = a4(br(boneBuf, bi));
        }
        if g_VRSmokeFingerCount > 0 {
            g_VRSmokeFingerHave = 1;
        }
        // Cig slot (WeaponRight): full local transform T + R.
        if in_rig(g_VRSmokeCigIdx) {
            g_VRSmokeCigPos = a3(bt(boneBuf, g_VRSmokeCigIdx));
            g_VRSmokeCigRot = a4(br(boneBuf, g_VRSmokeCigIdx));
            g_VRSmokeCigHave = 1;
        }
        g_VRSmokeFingerCapture = 0;
    } else if g_VRSmokeFingerActive != 0 {
        // Left-handed the right hand holds the lighter, not the cigarette.
        if g_VRSmokeRightLighter != 0 {
            right_lighter_pose(boneBuf);
        }
        if g_VRSmokeFingerHave != 0 && g_VRSmokeRightLighter == 0 {
            let mut k = 0;
            while k < g_VRSmokeFingerCount && k < 32 {
                let ku = k as usize;
                k += 1;
                let bi = g_VRSmokeFingerIdx[ku];
                if !in_rig(bi) {
                    continue;
                }
                put4(br(boneBuf, bi), &g_VRSmokeFingerRot[ku]);
            }
        }
        // GRIP (cig in the right hand): the captured grip local + nudge, only when not mouth-anchored.
        if g_VRSmokeRightLighter == 0
            && g_VRSmokeCigEnable != 0
            && g_VRSmokeCigHave != 0
            && !(g_VRSmokeMouthAnchor != 0 && g_VRSmokeAnchorValid != 0)
            && in_rig(g_VRSmokeCigIdx)
        {
            put3(
                bt(boneBuf, g_VRSmokeCigIdx),
                &[g_VRSmokeCigPos[0] + g_VRSmokeCigOffP[0], g_VRSmokeCigPos[1] + g_VRSmokeCigOffP[1], g_VRSmokeCigPos[2] + g_VRSmokeCigOffP[2]],
            );
            let mut out = quat_mul(&g_VRSmokeCigRot, &g_VRSmokeCigOffQ);
            quat_norm(&mut out);
            put4(br(boneBuf, g_VRSmokeCigIdx), &out);
            set_scale(boneBuf, g_VRSmokeCigIdx);
        }
        // MOUTH PIN: WeaponRight1 (a separate LEAF) to the lips, so WeaponRight stays free for a weapon.
        if g_VRSmokeMouthAnchor != 0 && g_VRSmokeAnchorValid != 0 && in_rig(g_VRSmokeMouthBoneIdx) {
            put3(bt(boneBuf, g_VRSmokeMouthBoneIdx), &g_VRSmokeAnchorLocalPos);
            put4(br(boneBuf, g_VRSmokeMouthBoneIdx), &g_VRSmokeAnchorLocalRot);
            set_scale(boneBuf, g_VRSmokeMouthBoneIdx);
        }
    }
}

/// The lighter (or the left-hand cigarette) hand.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_smoking_lighter_pose(boneBuf: *mut u8) {
    if g_VRSmokeFingerCaptureL != 0 {
        let cap_cig = g_VRSmokeLeftUseCig != 0;
        let mut k = 0;
        while k < g_VRSmokeFingerCountL && k < 32 {
            let ku = k as usize;
            k += 1;
            let bi = g_VRSmokeFingerIdxL[ku];
            if !in_rig(bi) {
                continue;
            }
            let q = a4(br(boneBuf, bi));
            if cap_cig {
                g_VRSmokeFingerRotLC[ku] = q;
            } else {
                g_VRSmokeFingerRotL[ku] = q;
            }
        }
        if g_VRSmokeFingerCountL > 0 {
            if cap_cig {
                g_VRSmokeCigLHave = 1;
            } else {
                g_VRSmokeFingerHaveL = 1;
            }
        }
        if in_rig(g_VRSmokeLighterIdx) {
            let t = a3(bt(boneBuf, g_VRSmokeLighterIdx));
            let q = a4(br(boneBuf, g_VRSmokeLighterIdx));
            if cap_cig {
                g_VRSmokeCigLPos = t;
                g_VRSmokeCigLRot = q;
                g_VRSmokeCigLHave = 1;
            } else {
                g_VRSmokeLighterPos = t;
                g_VRSmokeLighterRot = q;
                g_VRSmokeLighterHave = 1;
            }
        }
        g_VRSmokeFingerCaptureL = 0;
    } else if g_VRSmokeFingerActiveL != 0 {
        let use_cig = g_VRSmokeLeftUseCig != 0;
        let have_fing = if use_cig { g_VRSmokeCigLHave != 0 } else { g_VRSmokeFingerHaveL != 0 };
        if have_fing {
            // Thumb press: the left trigger, analog, or the manual override; no lighter-wheel flick for the cig.
            let press = thumb_press(K_LEFT_TRIGGER_ANALOG);
            let fq = flick(press);
            let pressing = !use_cig && press > 0.0001;
            let mut k = 0;
            while k < g_VRSmokeFingerCountL && k < 32 {
                let ku = k as usize;
                k += 1;
                let bi = g_VRSmokeFingerIdxL[ku];
                if !in_rig(bi) {
                    continue;
                }
                let rot = if use_cig { g_VRSmokeFingerRotLC[ku] } else { g_VRSmokeFingerRotL[ku] };
                if pressing && g_VRSmokeThumbIsL[ku] != 0 {
                    let mut out = quat_mul(&rot, &fq);
                    quat_norm(&mut out);
                    put4(br(boneBuf, bi), &out);
                } else {
                    put4(br(boneBuf, bi), &rot);
                }
            }
        }
        let slot_apply = if use_cig { g_VRSmokeCigLHave != 0 } else { g_VRSmokeLighterEnable != 0 && g_VRSmokeLighterHave != 0 };
        if slot_apply && in_rig(g_VRSmokeLighterIdx) {
            if use_cig {
                put3(bt(boneBuf, g_VRSmokeLighterIdx), &g_VRSmokeCigLPos);
                let mut q = g_VRSmokeCigLRot;
                quat_norm(&mut q);
                put4(br(boneBuf, g_VRSmokeLighterIdx), &q);
            } else {
                put3(
                    bt(boneBuf, g_VRSmokeLighterIdx),
                    &[
                        g_VRSmokeLighterPos[0] + g_VRSmokeLighterOffP[0],
                        g_VRSmokeLighterPos[1] + g_VRSmokeLighterOffP[1],
                        g_VRSmokeLighterPos[2] + g_VRSmokeLighterOffP[2],
                    ],
                );
                let mut out = quat_mul(&g_VRSmokeLighterRot, &g_VRSmokeLighterOffQ);
                quat_norm(&mut out);
                put4(br(boneBuf, g_VRSmokeLighterIdx), &out);
            }
        }
    }
}
