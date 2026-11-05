// SmokingPose -- the smoking mod's cigarette and lighter poses, out of the VRIK solve.
//
// Not VRIK either, and the reason it is its own file rather than sharing one with the reload fingers
// is that it is a different feature with a different owner: the cigarette rides WeaponRight while the
// lighter rides the left hand, and each has its own capture, its own anchor and its own release.
//
// TWO SEPARATE ENTRY POINTS, deliberately. The cig and the lighter are independent -- one hand can be
// holding a lit cigarette while the other has no lighter at all -- so they are guarded separately in
// the detour and cannot be collapsed into one call.
//
// The MOUTH PIN is the part worth reading before changing anything here: WeaponRight1 is a separate
// LEAF pinned to the lips when the cig is anchored, so WeaponRight stays free for a weapon and only
// the cig moves. Pinning the parent instead moves the hand, the arm and the body with it.
//
// Guards stay in src/Hooks/AnimPose.cpp. No __try needed: the detour's __except is dynamic and covers
// callees in other translation units.

#include "Anim/VrikHook.hpp"
#include "Anim/CharacterRig.hpp"
#include "Anim/WeaponRig.hpp"
#include "Hooks/Hook.hpp"
#include <MinHook.h>
#include "Anim/SmokingPose.hpp"
#include "Anim/LeftHanded.hpp"

namespace cvr {
namespace anim {

namespace {

// The lighter thumb's press, 0..1: the trigger of the hand holding it, analog, or the manual override (tuning).
float ThumbPress(int trigger) {
    float press = g_pSharedHands ? g_pSharedHands[trigger] : 0.0f;
    if (g_VRSmokeThumbPressManualL > press) press = g_VRSmokeThumbPressManualL;
    return press < 0.0f ? 0.0f : (press > 1.0f ? 1.0f : press);
}

// LEFT-HANDED: the lighter in the RIGHT hand. The left hand's lighter hold -- its fingers, the thumb on the wheel and
// the lighter's seat on WeaponLeft -- mirrored onto the right hand through the rig's twins (Anim/LeftHanded.cpp), the
// thumb worked by the right trigger as the left trigger works it in the left hand.
void RightLighterPose(uint8_t* boneBuf) {
    if (g_VRSmokeFingerHaveL) {
        const float press = ThumbPress(vrshared::kRightTriggerAnalog);
        float fq[4] = { g_VRSmokeThumbFlickL[0]*press, g_VRSmokeThumbFlickL[1]*press,
                        g_VRSmokeThumbFlickL[2]*press, g_VRSmokeThumbFlickL[3]*press + (1.0f - press) };
        VRIK_QuatNorm(fq);
        for (int k = 0; k < g_VRSmokeFingerCountL && k < 32; ++k) {
            const int bi = g_VRSmokeFingerIdxL[k];
            if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
            float local[4] = { g_VRSmokeFingerRotL[k][0], g_VRSmokeFingerRotL[k][1],
                               g_VRSmokeFingerRotL[k][2], g_VRSmokeFingerRotL[k][3] };
            if (press > 0.0001f && g_VRSmokeThumbIsL[k]) { VRIK_QuatMul(g_VRSmokeFingerRotL[k], fq, local); VRIK_QuatNorm(local); }
            int right = -1;
            float out[4];
            if (!MirrorFingerToRight(bi, local, &right, out) || right < 0 || right >= VRIK_MAX_BONES) continue;
            float* q = reinterpret_cast<float*>(boneBuf + right * 48 + VRIK_ROT_OFF);
            q[0] = out[0]; q[1] = out[1]; q[2] = out[2]; q[3] = out[3];
        }
    }
    if (g_VRSmokeLighterEnable && g_VRSmokeLighterHave && g_VRSmokeCigIdx >= 0 && g_VRSmokeCigIdx < VRIK_MAX_BONES) {
        const float pos[3] = { g_VRSmokeLighterPos[0] + g_VRSmokeLighterOffP[0], g_VRSmokeLighterPos[1] + g_VRSmokeLighterOffP[1],
                               g_VRSmokeLighterPos[2] + g_VRSmokeLighterOffP[2] };
        const float base[4] = { g_VRSmokeLighterRot[0], g_VRSmokeLighterRot[1], g_VRSmokeLighterRot[2], g_VRSmokeLighterRot[3] };
        const float off[4]  = { g_VRSmokeLighterOffQ[0], g_VRSmokeLighterOffQ[1], g_VRSmokeLighterOffQ[2], g_VRSmokeLighterOffQ[3] };
        float rot[4]; VRIK_QuatMul(base, off, rot); VRIK_QuatNorm(rot);
        float p[3], r[4];
        if (MirrorWeaponSlotToRight(pos, rot, p, r)) {
            VRIK_QuatNorm(r);
            float* t = reinterpret_cast<float*>(boneBuf + g_VRSmokeCigIdx * 48 + VRIK_TRANS_OFF);
            float* q = reinterpret_cast<float*>(boneBuf + g_VRSmokeCigIdx * 48 + VRIK_ROT_OFF);
            t[0] = p[0]; t[1] = p[1]; t[2] = p[2];
            q[0] = r[0]; q[1] = r[1]; q[2] = r[2]; q[3] = r[3];
        }
    }
}

}  // namespace

void VrikSmokingCigPose(uint8_t* boneBuf) {
                    if (g_VRSmokeFingerCapture) {
                        // Fingers: rotation only (parent-local; no translation => no skin stretch).
                        for (int k = 0; k < g_VRSmokeFingerCount && k < 32; ++k) {
                            const int bi = g_VRSmokeFingerIdx[k];
                            if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                            const float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                            g_VRSmokeFingerRot[k][0] = q[0]; g_VRSmokeFingerRot[k][1] = q[1];
                            g_VRSmokeFingerRot[k][2] = q[2]; g_VRSmokeFingerRot[k][3] = q[3];
                        }
                        if (g_VRSmokeFingerCount > 0) g_VRSmokeFingerHave = 1;
                        // Cig slot (WeaponRight): full local transform T + R.
                        if (g_VRSmokeCigIdx >= 0 && g_VRSmokeCigIdx < VRIK_MAX_BONES) {
                            const float* t = reinterpret_cast<float*>(boneBuf + g_VRSmokeCigIdx * 48 + VRIK_TRANS_OFF);
                            const float* q = reinterpret_cast<float*>(boneBuf + g_VRSmokeCigIdx * 48 + VRIK_ROT_OFF);
                            g_VRSmokeCigPos[0]=t[0]; g_VRSmokeCigPos[1]=t[1]; g_VRSmokeCigPos[2]=t[2];
                            g_VRSmokeCigRot[0]=q[0]; g_VRSmokeCigRot[1]=q[1]; g_VRSmokeCigRot[2]=q[2]; g_VRSmokeCigRot[3]=q[3];
                            g_VRSmokeCigHave = 1;
                        }
                        g_VRSmokeFingerCapture = 0;
                    } else if (g_VRSmokeFingerActive) {
                        // Left-handed the right hand holds the lighter, not the cigarette.
                        if (g_VRSmokeRightLighter) RightLighterPose(boneBuf);
                        if (g_VRSmokeFingerHave && !g_VRSmokeRightLighter) {
                            for (int k = 0; k < g_VRSmokeFingerCount && k < 32; ++k) {
                                const int bi = g_VRSmokeFingerIdx[k];
                                if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                                float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                                q[0] = g_VRSmokeFingerRot[k][0]; q[1] = g_VRSmokeFingerRot[k][1];
                                q[2] = g_VRSmokeFingerRot[k][2]; q[3] = g_VRSmokeFingerRot[k][3];
                            }
                        }
                        // Place the cig slot. Runs EVERY pass (before the solve/replay split), so
                        // whatever we write here survives all 4-5 replays. When the mouth anchor is
                        // active AND the fresh arm solve has produced a head-anchored local, replay
                        // that local (cig stays at the lips even on replay passes / hand lowered);
                        // otherwise the captured grip local + live nudge (cig in the hand).
                        // GRIP (cig in the RIGHT hand): pose WeaponRight with the captured grip local +
                        // nudge -- ONLY when NOT mouth-anchored.
                        if (!g_VRSmokeRightLighter && g_VRSmokeCigEnable && g_VRSmokeCigHave && !(g_VRSmokeMouthAnchor && g_VRSmokeAnchorValid)
                            && g_VRSmokeCigIdx >= 0 && g_VRSmokeCigIdx < VRIK_MAX_BONES) {
                            float* t = reinterpret_cast<float*>(boneBuf + g_VRSmokeCigIdx * 48 + VRIK_TRANS_OFF);
                            float* q = reinterpret_cast<float*>(boneBuf + g_VRSmokeCigIdx * 48 + VRIK_ROT_OFF);
                            t[0] = g_VRSmokeCigPos[0] + g_VRSmokeCigOffP[0];
                            t[1] = g_VRSmokeCigPos[1] + g_VRSmokeCigOffP[1];
                            t[2] = g_VRSmokeCigPos[2] + g_VRSmokeCigOffP[2];
                            const float base[4] = { g_VRSmokeCigRot[0], g_VRSmokeCigRot[1], g_VRSmokeCigRot[2], g_VRSmokeCigRot[3] };
                            const float off[4]  = { g_VRSmokeCigOffQ[0], g_VRSmokeCigOffQ[1], g_VRSmokeCigOffQ[2], g_VRSmokeCigOffQ[3] };
                            float out[4]; VRIK_QuatMul(base, off, out); VRIK_QuatNorm(out);
                            q[0]=out[0]; q[1]=out[1]; q[2]=out[2]; q[3]=out[3];
                            float* s = reinterpret_cast<float*>(boneBuf + g_VRSmokeCigIdx * 48 + 32);
                            s[0] = 1.0f; s[1] = g_VRSmokeCigScaleY; s[2] = 1.0f;
                        }
                        // MOUTH PIN: pin WeaponRight1 (a separate LEAF) to the lips when anchored, so
                        // WeaponRight stays free for a weapon and only the cig moves (body/head untouched).
                        if (g_VRSmokeMouthAnchor && g_VRSmokeAnchorValid
                            && g_VRSmokeMouthBoneIdx >= 0 && g_VRSmokeMouthBoneIdx < VRIK_MAX_BONES) {
                            float* t = reinterpret_cast<float*>(boneBuf + g_VRSmokeMouthBoneIdx * 48 + VRIK_TRANS_OFF);
                            float* q = reinterpret_cast<float*>(boneBuf + g_VRSmokeMouthBoneIdx * 48 + VRIK_ROT_OFF);
                            t[0]=g_VRSmokeAnchorLocalPos[0]; t[1]=g_VRSmokeAnchorLocalPos[1]; t[2]=g_VRSmokeAnchorLocalPos[2];
                            q[0]=g_VRSmokeAnchorLocalRot[0]; q[1]=g_VRSmokeAnchorLocalRot[1]; q[2]=g_VRSmokeAnchorLocalRot[2]; q[3]=g_VRSmokeAnchorLocalRot[3];
                            float* s = reinterpret_cast<float*>(boneBuf + g_VRSmokeMouthBoneIdx * 48 + 32);
                            s[0] = 1.0f; s[1] = g_VRSmokeCigScaleY; s[2] = 1.0f;
                        }
                    }
}

void VrikSmokingLighterPose(uint8_t* boneBuf) {
                    if (g_VRSmokeFingerCaptureL) {
                        const bool capCig = (g_VRSmokeLeftUseCig != 0);   // capture into the cig-left buffer?
                        for (int k = 0; k < g_VRSmokeFingerCountL && k < 32; ++k) {
                            const int bi = g_VRSmokeFingerIdxL[k];
                            if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                            const float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                            float* dst = capCig ? g_VRSmokeFingerRotLC[k] : g_VRSmokeFingerRotL[k];
                            dst[0] = q[0]; dst[1] = q[1]; dst[2] = q[2]; dst[3] = q[3];
                        }
                        if (g_VRSmokeFingerCountL > 0) { if (capCig) g_VRSmokeCigLHave = 1; else g_VRSmokeFingerHaveL = 1; }
                        if (g_VRSmokeLighterIdx >= 0 && g_VRSmokeLighterIdx < VRIK_MAX_BONES) {
                            const float* t = reinterpret_cast<float*>(boneBuf + g_VRSmokeLighterIdx * 48 + VRIK_TRANS_OFF);
                            const float* q = reinterpret_cast<float*>(boneBuf + g_VRSmokeLighterIdx * 48 + VRIK_ROT_OFF);
                            if (capCig) {
                                g_VRSmokeCigLPos[0]=t[0]; g_VRSmokeCigLPos[1]=t[1]; g_VRSmokeCigLPos[2]=t[2];
                                g_VRSmokeCigLRot[0]=q[0]; g_VRSmokeCigLRot[1]=q[1]; g_VRSmokeCigLRot[2]=q[2]; g_VRSmokeCigLRot[3]=q[3];
                                g_VRSmokeCigLHave = 1;
                            } else {
                                g_VRSmokeLighterPos[0]=t[0]; g_VRSmokeLighterPos[1]=t[1]; g_VRSmokeLighterPos[2]=t[2];
                                g_VRSmokeLighterRot[0]=q[0]; g_VRSmokeLighterRot[1]=q[1]; g_VRSmokeLighterRot[2]=q[2]; g_VRSmokeLighterRot[3]=q[3];
                                g_VRSmokeLighterHave = 1;
                            }
                        }
                        g_VRSmokeFingerCaptureL = 0;
                    } else if (g_VRSmokeFingerActiveL) {
                        const bool useCig = (g_VRSmokeLeftUseCig != 0);      // left hand holds the cig, not the lighter
                        const bool haveFing = useCig ? (g_VRSmokeCigLHave != 0) : (g_VRSmokeFingerHaveL != 0);
                        if (haveFing) {
                            // Thumb press amount: the left VR trigger, analog, or the manual override
                            // (tuning). nlerp identity->flick by that amount, composed onto the thumb
                            // bones' rest rotation. The lighter-wheel flick is meaningless for the
                            // cig, so it is suppressed when useCig.
                            //
                            // The slot moved. This used to read [67], which now carries the
                            // hand-sample millisecond stamp -- a number the clamp below turns into a
                            // permanent 1.0, so the thumb sat fully flicked and never answered the
                            // trigger at all. Same reassignment that had the lighter igniting by
                            // itself; this was the last reader left on the old number.
                            const float press = ThumbPress(vrshared::kLeftTriggerAnalog);
                            float fq[4] = { g_VRSmokeThumbFlickL[0]*press,
                                            g_VRSmokeThumbFlickL[1]*press,
                                            g_VRSmokeThumbFlickL[2]*press,
                                            g_VRSmokeThumbFlickL[3]*press + (1.0f - press) };
                            VRIK_QuatNorm(fq);
                            const bool pressing = !useCig && press > 0.0001f;
                            for (int k = 0; k < g_VRSmokeFingerCountL && k < 32; ++k) {
                                const int bi = g_VRSmokeFingerIdxL[k];
                                if (bi < 0 || bi >= VRIK_MAX_BONES) continue;
                                const float* rot = useCig ? g_VRSmokeFingerRotLC[k] : g_VRSmokeFingerRotL[k];
                                float* q = reinterpret_cast<float*>(boneBuf + bi * 48 + VRIK_ROT_OFF);
                                if (pressing && g_VRSmokeThumbIsL[k]) {
                                    float out[4]; VRIK_QuatMul(rot, fq, out); VRIK_QuatNorm(out);
                                    q[0]=out[0]; q[1]=out[1]; q[2]=out[2]; q[3]=out[3];
                                } else {
                                    q[0] = rot[0]; q[1] = rot[1]; q[2] = rot[2]; q[3] = rot[3];
                                }
                            }
                        }
                        const bool slotApply = useCig ? (g_VRSmokeCigLHave != 0)
                                                      : (g_VRSmokeLighterEnable && g_VRSmokeLighterHave);
                        if (slotApply && g_VRSmokeLighterIdx >= 0 && g_VRSmokeLighterIdx < VRIK_MAX_BONES) {
                            float* t = reinterpret_cast<float*>(boneBuf + g_VRSmokeLighterIdx * 48 + VRIK_TRANS_OFF);
                            float* q = reinterpret_cast<float*>(boneBuf + g_VRSmokeLighterIdx * 48 + VRIK_ROT_OFF);
                            if (useCig) {
                                t[0] = g_VRSmokeCigLPos[0]; t[1] = g_VRSmokeCigLPos[1]; t[2] = g_VRSmokeCigLPos[2];
                                q[0]=g_VRSmokeCigLRot[0]; q[1]=g_VRSmokeCigLRot[1]; q[2]=g_VRSmokeCigLRot[2]; q[3]=g_VRSmokeCigLRot[3];
                                VRIK_QuatNorm(q);
                            } else {
                                t[0] = g_VRSmokeLighterPos[0] + g_VRSmokeLighterOffP[0];
                                t[1] = g_VRSmokeLighterPos[1] + g_VRSmokeLighterOffP[1];
                                t[2] = g_VRSmokeLighterPos[2] + g_VRSmokeLighterOffP[2];
                                const float base[4] = { g_VRSmokeLighterRot[0], g_VRSmokeLighterRot[1], g_VRSmokeLighterRot[2], g_VRSmokeLighterRot[3] };
                                const float off[4]  = { g_VRSmokeLighterOffQ[0], g_VRSmokeLighterOffQ[1], g_VRSmokeLighterOffQ[2], g_VRSmokeLighterOffQ[3] };
                                float out[4]; VRIK_QuatMul(base, off, out); VRIK_QuatNorm(out);
                                q[0]=out[0]; q[1]=out[1]; q[2]=out[2]; q[3]=out[3];
                            }
                        }
                    }
}

}  // namespace anim
}  // namespace cvr
